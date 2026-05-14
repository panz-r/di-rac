#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/epoll.h>
#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <signal.h>
#include <stdatomic.h>

#include "trie.h"

#define MAX_EVENTS 128
#define MAX_BUFFER_SIZE (1 << 20)  /* 1MB per client — prevents slowloris DoS */

__attribute__((constructor))
static void ignore_sigpipe(void) {
    signal(SIGPIPE, SIG_IGN);
}
#define BUF_SIZE 65536
static const char *socket_path = "/tmp/di-vrr-coord.sock";
static const char *bound_socket_path = NULL;

typedef struct {
    int fd;
    char *buffer;
    size_t buf_cap;
    size_t len;
    char *outbuf;
    size_t out_cap;
    size_t out_len;
    bool out_epoll_registered;
} client_ctx_t;

static client_ctx_t *all_clients[MAX_EVENTS];
static int fd_to_slot[MAX_EVENTS];  /* O(1) inverse map: fd → slot index, -1 if not registered */
static int free_slots[MAX_EVENTS];  /* free-list stack */
static int free_slots_top = 0;     /* top of stack (0 = empty) */
static char persist_path[4096] = {0};
static trie_t *lock_trie = NULL;
static volatile sig_atomic_t shutdown_requested = 0;
static int g_epoll_fd = -1;
static _Atomic int g_rejected_connections = 0;

static client_ctx_t* ctx_by_fd(int fd);

static int send_json(client_ctx_t *ctx, const char *json) {
    if (!ctx) return -1;
    size_t len = strlen(json);
    if (ctx->out_len + len <= ctx->out_cap) {
        memcpy(ctx->outbuf + ctx->out_len, json, len);
        ctx->out_len += len;
        if (!ctx->out_epoll_registered) {
            struct epoll_event ev = { .events = EPOLLIN | EPOLLOUT, .data = { .ptr = ctx } };
            if (epoll_ctl(g_epoll_fd, EPOLL_CTL_MOD, ctx->fd, &ev) < 0) {
                fprintf(stderr, "[di-vrr] epoll_ctl MOD failed to register EPOLLOUT on fd %d: %s\n", ctx->fd, strerror(errno));
            } else {
                ctx->out_epoll_registered = true;
            }
        }
        return 0;
    }
    ssize_t written = write(ctx->fd, json, len);
    if (written < 0) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            /* Socket buffer full — try to append to outbuf as fallback */
            if (ctx->out_len + len <= ctx->out_cap) {
                memcpy(ctx->outbuf + ctx->out_len, json, len);
                ctx->out_len += len;
                return 0;
            }
            fprintf(stderr, "[di-vrr] send_json would block on fd %d, message dropped\n", ctx->fd);
            return -1;
        }
        if (errno != EPIPE) {
            fprintf(stderr, "[di-vrr] send_json failed on fd %d: %s\n", ctx->fd, strerror(errno));
        }
        return -1;
    }
    if ((size_t)written < len) {
        /* Partial write — append remainder to outbuf for drain_output to resume */
        size_t remaining = len - (size_t)written;
        if (ctx->out_len + remaining <= ctx->out_cap) {
            memcpy(ctx->outbuf + ctx->out_len, json + written, remaining);
            ctx->out_len += remaining;
            if (!ctx->out_epoll_registered) {
                struct epoll_event ev = { .events = EPOLLIN | EPOLLOUT, .data = { .ptr = ctx } };
                if (epoll_ctl(g_epoll_fd, EPOLL_CTL_MOD, ctx->fd, &ev) < 0) {
                    fprintf(stderr, "[di-vrr] epoll_ctl MOD failed in partial send: %s\n", strerror(errno));
                } else {
                    ctx->out_epoll_registered = true;
                }
            }
        } else {
            fprintf(stderr, "[di-vrr] partial send overflow on fd %d (%zd of %zu bytes)\n", ctx->fd, written, len);
        }
        return 0;
    }
    return 0;
}

static int drain_output(client_ctx_t *ctx) {
    if (ctx->out_len == 0) return 0;
    ssize_t sent;
    do {
        sent = write(ctx->fd, ctx->outbuf, ctx->out_len);
    } while (sent < 0 && errno == EINTR);
    if (sent < 0) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) return 0;
        fprintf(stderr, "[di-vrr] drain_output write failed on fd %d: %s\n", ctx->fd, strerror(errno));
        return -1;
    }
    size_t consumed = (size_t)sent;
    if (consumed < ctx->out_len) {
        memmove(ctx->outbuf, ctx->outbuf + consumed, ctx->out_len - consumed);
    }
    ctx->out_len -= consumed;
    ctx->outbuf[ctx->out_len] = '\0';
    if (ctx->out_len == 0 && ctx->out_epoll_registered) {
        struct epoll_event ev = { .events = EPOLLIN, .data = { .ptr = ctx } };
        if (epoll_ctl(g_epoll_fd, EPOLL_CTL_MOD, ctx->fd, &ev) < 0) {
            fprintf(stderr, "[di-vrr] epoll_ctl MOD to remove EPOLLOUT failed on fd %d: %s\n", ctx->fd, strerror(errno));
        } else {
            ctx->out_epoll_registered = false;
        }
    }
    return 0;
}

/* Escape a string for embedding in a JSON string field.
 * Returns number of bytes written to dst, or -1 if dst_len is insufficient.
 * Caller must ensure dst has at least (strlen(src) * 2 + 1) bytes.
 */
static int json_escape_string(const char *src, char *dst, size_t dst_len) {
    size_t dst_pos = 0;
    for (const char *p = src; *p; p++) {
        if (dst_pos + 2 >= dst_len) return -1;
        if (*p == '\\' || *p == '"') {
            dst[dst_pos++] = '\\';
            dst[dst_pos++] = *p;
        } else if ((unsigned char)*p < 0x20) {
            // Control character — escape as \u00XX
            if (dst_pos + 6 >= dst_len) return -1;
            snprintf(dst + dst_pos, dst_len - dst_pos, "\\u00%02x", (unsigned char)*p);
            dst_pos += 6;
        } else {
            dst[dst_pos++] = *p;
        }
    }
    dst[dst_pos] = '\0';
    return (int)dst_pos;
}

static int broadcast_config_update(int sender_fd, const char *path, const char *key, const char *value) {
    /* Heap-allocate to handle worst-case expansion: control chars expand to
     * \u00XX (6 bytes per input byte), so for an 8192-byte input we need 49152
     * bytes. Add headroom for JSON overhead. */
    size_t path_len = strlen(path);
    size_t escaped_path_size = path_len * 6 + 1;
    char *escaped_path = malloc(escaped_path_size);
    size_t key_len = strlen(key);
    size_t escaped_key_size = key_len * 6 + 1;
    char *escaped_key = malloc(escaped_key_size);
    char *escaped_value = NULL;
    size_t escaped_value_size = 0;

    if (!escaped_path || !escaped_key) {
        free(escaped_path); free(escaped_key);
        return -1;
    }
    if (value) {
        escaped_value_size = strlen(value) * 6 + 1;
        escaped_value = malloc(escaped_value_size);
        if (!escaped_value) {
            free(escaped_path); free(escaped_key);
            return -1;
        }
    }

    if (json_escape_string(path, escaped_path, escaped_path_size) < 0) {
        free(escaped_path); free(escaped_key); free(escaped_value);
        return -1;
    }
    if (json_escape_string(key, escaped_key, escaped_key_size) < 0) {
        free(escaped_path); free(escaped_key); free(escaped_value);
        return -1;
    }
    if (value && json_escape_string(value, escaped_value, escaped_value_size) < 0) {
        free(escaped_path); free(escaped_key); free(escaped_value);
        return -1;
    }

    /* Compute exact size needed for JSON message using snprintf NULL trick */
    int needed = snprintf(NULL, 0,
             "{\"status\": \"config_update\", \"path\": \"%s\", \"key\": \"%s\", \"value\": %s%s%s}\n",
             escaped_path, escaped_key,
             value ? "\"" : "", value ? escaped_value : "null", value ? "\"" : "");
    if (needed < 0 || (size_t)needed > SIZE_MAX / 2) {
        free(escaped_path); free(escaped_key); free(escaped_value);
        return -1;
    }
    size_t msg_size = (size_t)needed + 1;
    char *msg = malloc(msg_size);
    if (!msg) {
        free(escaped_path); free(escaped_key); free(escaped_value);
        return -1;
    }

    int len = snprintf(msg, msg_size,
             "{\"status\": \"config_update\", \"path\": \"%s\", \"key\": \"%s\", \"value\": %s%s%s}\n",
             escaped_path, escaped_key,
             value ? "\"" : "", value ? escaped_value : "null", value ? "\"" : "");

    free(escaped_path); free(escaped_key); free(escaped_value);

    if (len < 0 || (size_t)len >= msg_size) { free(msg); return -1; }

    /* Snapshot all_clients to avoid concurrent modification if trie_cleanup_fd
     * fires on another fd while we're iterating (both run on the main event loop
     * but interleave at arbitrary points). */
    client_ctx_t *snap[MAX_EVENTS];
    int snap_count = 0;
    for (int i = 0; i < MAX_EVENTS; i++) {
        if (all_clients[i] && all_clients[i]->fd != sender_fd) {
            snap[snap_count++] = all_clients[i];
        }
    }
    for (int i = 0; i < snap_count; i++) {
        if (send_json(snap[i], msg) < 0) {
            fprintf(stderr, "[di-vrr] broadcast: send_json failed to fd %d\n", snap[i]->fd);
        }
    }
    free(msg);
    return 0;
}

static void handle_stats(int sig) {
    (void)sig;
    size_t total_clients = 0;
    for (int i = 0; i < MAX_EVENTS; i++) if (all_clients[i]) total_clients++;

    size_t trie_nodes = 0, trie_waiters = 0, trie_locks = 0;
    if (lock_trie) trie_get_stats(lock_trie, &trie_nodes, &trie_waiters, &trie_locks);

    char buf[512];
    int n = snprintf(buf, sizeof(buf),
            "[di-vrr] --- Health Snapshot ---\n"
            "[di-vrr] Clients: %zu/%d\n"
            "[di-vrr] Trie nodes: %zu\n"
            "[di-vrr] Locked paths: %zu\n"
            "[di-vrr] Total waiters: %zu\n"
            "[di-vrr] --------------------------\n",
            total_clients, MAX_EVENTS,
            trie_nodes, trie_locks, trie_waiters);
    ssize_t _r = write(STDERR_FILENO, buf, (size_t)n < sizeof(buf) ? (size_t)n : sizeof(buf));
    (void)_r;
}

static void handle_shutdown(int sig) {
    (void)sig;
    shutdown_requested = 1;
}

/* Unescape a JSON string in place (or into dst). Handles:
 *   \\ → \   \" → "   \n → NL   \r → CR   \t → TAB
 *   \b → BS   \f → FF   \/ → /   \uXXXX → UTF-8 bytes
 * Returns bytes written to dst, or -1 if dst_len insufficient.
 */
static int json_unescape(const char *src, char *dst, size_t dst_len) {
    size_t dst_pos = 0;
    for (const char *p = src; *p; p++) {
        if (*p != '\\') {
            if (dst_pos + 1 >= dst_len) return -1;
            dst[dst_pos++] = *p;
        } else {
            if (*(p + 1) == '\0') return -1;
            p++;
            switch (*p) {
                case '\\': if (dst_pos + 1 >= dst_len) return -1; dst[dst_pos++] = '\\'; break;
                case '"':  if (dst_pos + 1 >= dst_len) return -1; dst[dst_pos++] = '"';  break;
                case 'n':  if (dst_pos + 1 >= dst_len) return -1; dst[dst_pos++] = '\n'; break;
                case 'r':  if (dst_pos + 1 >= dst_len) return -1; dst[dst_pos++] = '\r'; break;
                case 't':  if (dst_pos + 1 >= dst_len) return -1; dst[dst_pos++] = '\t'; break;
                case 'b':  if (dst_pos + 1 >= dst_len) return -1; dst[dst_pos++] = '\b'; break;
                case 'f':  if (dst_pos + 1 >= dst_len) return -1; dst[dst_pos++] = '\f'; break;
                case '/':  if (dst_pos + 1 >= dst_len) return -1; dst[dst_pos++] = '/';  break;
                case 'u': {
                               /* \uXXXX — decode hex and emit UTF-8 for BMP */
                               if (*p == '\0' || *(p+1) == '\0' || *(p+2) == '\0' || *(p+3) == '\0') return -1;
                               if (dst_pos + 4 >= dst_len) return -1;
                               unsigned int cp = 0;
                               for (int i = 0; i < 4; i++) {
                                   char c = *++p;
                                   int digit = 0;
                                   if (c >= '0' && c <= '9') digit = c - '0';
                                   else if (c >= 'a' && c <= 'f') digit = c - 'a' + 10;
                                   else if (c >= 'A' && c <= 'F') digit = c - 'A' + 10;
                                   else return -1;
                                   cp = (cp << 4) | digit;
                               }
                               /* Encode code point as UTF-8 */
                               if (cp <= 0x7F) {
                                   if (dst_pos + 1 >= dst_len) return -1;
                                   dst[dst_pos++] = (char)cp;
                               } else if (cp <= 0x7FF) {
                                   if (dst_pos + 2 >= dst_len) return -1;
                                   dst[dst_pos++] = (char)(0xC0 | (cp >> 6));
                                   dst[dst_pos++] = (char)(0x80 | (cp & 0x3F));
                               } else {
                                   if (dst_pos + 3 >= dst_len) return -1;
                                   dst[dst_pos++] = (char)(0xE0 | (cp >> 12));
                                   dst[dst_pos++] = (char)(0x80 | ((cp >> 6) & 0x3F));
                                   dst[dst_pos++] = (char)(0x80 | (cp & 0x3F));
                               }
                           } break;
                default:   if (dst_pos + 2 >= dst_len) return -1;
                           dst[dst_pos++] = '\\';
                           dst[dst_pos++] = *p;
                           break;
            }
        }
    }
    dst[dst_pos] = '\0';
    return (int)dst_pos;
}

static const char* find_string_val(const char *json, const char *key, char *out, size_t out_len) {
    char pattern[128];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char *p = strstr(json, pattern);
    if (!p) return NULL;
    p = strchr(p + strlen(pattern), ':');
    if (!p) return NULL;
    while (*p == ' ' || *p == ':' || *p == '\t') p++;
    if (*p != '"') return NULL;
    const char *start = p + 1;
    const char *end = start;
    while (*end != '"' && *end != '\0') {
        if (*end == '\\' && *(end + 1) != '\0') end++;
        end++;
    }
    if (*end != '"') return NULL;

    /* Dynamically allocate raw buffer — no hard limit on token size */
    size_t raw_len = (size_t)(end - start);
    char *raw = malloc(raw_len + 1);
    if (!raw) return NULL;
    memcpy(raw, start, raw_len);
    raw[raw_len] = '\0';

    int res = json_unescape(raw, out, out_len);
    free(raw);
    if (res < 0) return NULL;
    return end;
}

static bool find_bool_val(const char *json, const char *key, bool default_val) {
    char pattern[128];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char *p = strstr(json, pattern);
    if (!p) return default_val;
    p = strchr(p + strlen(pattern), ':');
    if (!p) return default_val;
    while (*p == ' ' || *p == ':' || *p == '\t') p++;
    if (strncmp(p, "true", 4) == 0) return true;
    if (strncmp(p, "false", 5) == 0) return false;
    return default_val;
}

static void process_single_object(client_ctx_t *ctx, const char *json, trie_t *trie) {
    int fd = ctx->fd;
    char method[64] = {0}, path[8192] = {0};

    if (!find_string_val(json, "method", method, sizeof(method)) ||
        !find_string_val(json, "path", path, sizeof(path))) {
        send_json(ctx, "{\"status\": \"error\", \"message\": \"invalid protocol format\"}\n");
        return;
    }

    if (strcmp(method, "acquire") == 0) {
        bool wait = find_bool_val(json, "wait", false);
        int res = trie_acquire_lock(trie, path, fd, wait);
        if (res == 0) send_json(ctx, "{\"status\": \"ok\"}\n");
        else if (res == 1) send_json(ctx, "{\"status\": \"waiting\"}\n");
        else send_json(ctx, "{\"status\": \"denied\"}\n");
    } else if (strcmp(method, "release") == 0) {
        int next_fd = trie_release_lock(trie, path, fd);
        send_json(ctx, "{\"status\": \"ok\"}\n");
        if (next_fd != -1) {
            client_ctx_t *next_ctx = ctx_by_fd(next_fd);
            char stack_esc[4096];
            size_t esc_size = strlen(path) * 6 + 1;
            char *escaped_path = (esc_size <= sizeof(stack_esc)) ? stack_esc : malloc(esc_size);
            if (!escaped_path) {
                send_json(next_ctx, "{\"status\": \"granted\", \"path\": \"<nomem>\"}\n");
            } else {
                if (json_escape_string(path, escaped_path, esc_size) < 0) {
                    send_json(next_ctx, "{\"status\": \"granted\", \"path\": \"<overflow>\"}\n");
                } else {
                    char resp[8192];
                    int needed = snprintf(NULL, 0,
                            "{\"status\": \"granted\", \"path\": \"%s\"}\n", escaped_path);
                    if (needed < 0 || needed >= (int)sizeof(resp)) {
                        send_json(next_ctx, "{\"status\": \"granted\", \"path\": \"<overflow>\"}\n");
                    } else {
                        snprintf(resp, sizeof(resp),
                                "{\"status\": \"granted\", \"path\": \"%s\"}\n", escaped_path);
                        send_json(next_ctx, resp);
                    }
                }
                if (escaped_path != stack_esc) free(escaped_path);
            }
        }
    } else if (strcmp(method, "set_config") == 0) {
        char key[256] = {0}, value[4096] = {0}, *val_ptr = NULL;
        if (!find_string_val(json, "key", key, sizeof(key))) {
            send_json(ctx, "{\"status\": \"error\", \"message\": \"missing key\"}\n");
            return;
        }
        if (find_string_val(json, "value", value, sizeof(value))) val_ptr = value;
        bool transient = find_bool_val(json, "transient", false);

        int set_res = trie_set_config(trie, path, fd, key, val_ptr, transient);
        if (set_res < 0) {
            send_json(ctx, "{\"status\": \"error\", \"message\": \"config set failed\"}\n");
            return;
        }
        send_json(ctx, "{\"status\": \"ok\"}\n");

        if (!transient) {
            int br = broadcast_config_update(fd, path, key, val_ptr);
            if (br < 0) {
                fprintf(stderr, "[di-vrr] broadcast_config_update failed for fd %d path=%s key=%s\n",
                        fd, path, key);
            }
            if (persist_path[0] && trie_save_persist(trie, persist_path) < 0) {
                fprintf(stderr, "[di-vrr] trie_save_persist failed on set_config for path=%s key=%s\n", path, key);
            }
        }
    } else if (strcmp(method, "get_config") == 0) {
        char key[256] = {0};
        if (!find_string_val(json, "key", key, sizeof(key))) {
            send_json(ctx, "{\"status\": \"error\", \"message\": \"missing key\"}\n");
            return;
        }
        char *val = trie_get_config(trie, path, fd, key);
        if (val) {
            size_t esc_size = strlen(val) * 6 + 1;
            char *escaped_val = malloc(esc_size);
            if (!escaped_val) {
                send_json(ctx, "{\"status\": \"error\", \"message\": \"out of memory\"}\n");
                free(val);
                return;
            }
            if (json_escape_string(val, escaped_val, esc_size) < 0) {
                send_json(ctx, "{\"status\": \"error\", \"message\": \"value too large\"}\n");
                free(escaped_val);
                free(val);
                return;
            }
            int needed = snprintf(NULL, 0,
                    "{\"status\": \"ok\", \"value\": \"%s\"}\n", escaped_val);
            if (needed < 0) {
                send_json(ctx, "{\"status\": \"error\", \"message\": \"format error\"}\n");
                free(escaped_val);
                free(val);
                return;
            }
            char *resp = malloc((size_t)needed + 1);
            if (!resp) {
                send_json(ctx, "{\"status\": \"error\", \"message\": \"out of memory\"}\n");
                free(escaped_val);
                free(val);
                return;
            }
            snprintf(resp, (size_t)needed + 1,
                    "{\"status\": \"ok\", \"value\": \"%s\"}\n", escaped_val);
            send_json(ctx, resp);
            free(resp);
            free(escaped_val);
            free(val);
        } else {
            send_json(ctx, "{\"status\": \"ok\", \"value\": null}\n");
        }
    } else if (strcmp(method, "status") == 0) {
        /* Runtime health snapshot */
        size_t total_clients = 0;
        for (int i = 0; i < MAX_EVENTS; i++) if (all_clients[i]) total_clients++;

        size_t trie_nodes = 0, trie_waiters = 0, trie_locks = 0;
        if (lock_trie) trie_get_stats(lock_trie, &trie_nodes, &trie_waiters, &trie_locks);

        int needed = snprintf(NULL, 0,
                "{\"status\": \"ok\", \"clients\": %zu, \"max_clients\": %zu, "
                "\"trie_nodes\": %zu, \"locked_paths\": %zu, \"total_waiters\": %zu, \"rejected\": %d}\n",
                total_clients, (size_t)MAX_EVENTS,
                trie_nodes, trie_locks, trie_waiters, g_rejected_connections);
        if (needed < 0 || (size_t)needed > SIZE_MAX / 2) {
            send_json(ctx, "{\"status\": \"error\", \"message\": \"format error\"}\n");
            return;
        }
        char resp[512];
        if ((size_t)needed >= sizeof(resp)) {
            send_json(ctx, "{\"status\": \"error\", \"message\": \"format error\"}\n");
            return;
        }
        snprintf(resp, sizeof(resp),
                "{\"status\": \"ok\", \"clients\": %zu, \"max_clients\": %zu, "
                "\"trie_nodes\": %zu, \"locked_paths\": %zu, \"total_waiters\": %zu, \"rejected\": %d}\n",
                total_clients, (size_t)MAX_EVENTS,
                trie_nodes, trie_locks, trie_waiters, g_rejected_connections);
        send_json(ctx, resp);
    } else {
        send_json(ctx, "{\"status\": \"error\", \"message\": \"unknown method\"}\n");
    }
}

static const char* find_end_of_object(const char *start, const char *end) {
    int depth = 0;
    bool in_string = false;
    for (const char *p = start; p < end; p++) {
        if (*p == '"') {
            if (!in_string) {
                in_string = true;
            } else {
                // Check if this quote is escaped (odd number of backslashes before it)
                int backslash_count = 0;
                for (const char *q = p - 1; q >= start && *q == '\\'; q--) backslash_count++;
                if (backslash_count % 2 == 0) in_string = false;
            }
            continue;
        }
        if (in_string) continue;
        if (*p == '{') depth++;
        else if (*p == '}') {
            depth--;
            if (depth == 0) return p + 1;
        }
    }
    return NULL;
}

static size_t process_json_stream(client_ctx_t *ctx, char *data, size_t len, trie_t *trie) {
    char *p = data;
    char *end = data + len;
    while (p < end) {
        char *obj_start = strchr(p, '{');
        if (!obj_start) break;
        const char *obj_end = find_end_of_object(obj_start, end);
        if (obj_end) {
            char *mutable_obj_end = (char*)obj_end;
            char saved = *mutable_obj_end;
            *mutable_obj_end = '\0';
            process_single_object(ctx, obj_start, trie);
            *mutable_obj_end = saved;
            p = mutable_obj_end;
        } else {
            /* No closing brace found — either the object is truncated in this
             * chunk (no '}' exists at all after the '{') or an escaped quote
             * inside a string is fooling find_end_of_object. For case 1, break
             * and wait for more data to arrive. For case 2 (escaped quote),
             * memchr will find a '}' but find_end_of_object couldn't reach it —
             * also break to wait. In both cases, p = obj_start so the same '{'
             * is retried when more data arrives. */
            p = obj_start;
            break;
        }
    }
    return p - data;
}

static int handle_client_data(client_ctx_t *ctx, trie_t *trie) {
    char read_buf[8192];
    ssize_t n = read(ctx->fd, read_buf, sizeof(read_buf));
    if (n < 0) {
        if (errno == EINTR) {
            n = read(ctx->fd, read_buf, sizeof(read_buf));
            if (n < 0) {
                if (errno == EAGAIN || errno == EWOULDBLOCK) return 0;
                return -1;
            }
        } else if (errno == EAGAIN || errno == EWOULDBLOCK) {
            return 0;
        } else {
            return -1;
        }
    }
    if (n == 0) return -1;

    if (ctx->len + (size_t)n >= ctx->buf_cap) {
        size_t new_cap = ctx->buf_cap ? ctx->buf_cap * 2 : 4096;
        if (new_cap < ctx->len + (size_t)n + 1) new_cap = ctx->len + (size_t)n + 1;
        if (new_cap > MAX_BUFFER_SIZE) new_cap = MAX_BUFFER_SIZE;
        if (ctx->len + (size_t)n + 1 > new_cap) {
            send_json(ctx, "{\"status\": \"error\", \"message\": \"buffer overflow\"}\n");
            ctx->len = 0;
            return 0;
        }
        char *tmp = realloc(ctx->buffer, new_cap);
        if (!tmp) {
            send_json(ctx, "{\"status\": \"error\", \"message\": \"buffer overflow\"}\n");
            ctx->len = 0;
            return 0;
        }
        ctx->buffer = tmp;
        ctx->buf_cap = new_cap;
    }
    memcpy(ctx->buffer + ctx->len, read_buf, n);
    ctx->len += n;
    ctx->buffer[ctx->len] = '\0';
    size_t consumed = process_json_stream(ctx, ctx->buffer, ctx->len, trie);
    if (consumed > 0) {
        size_t remaining = ctx->len - consumed;
        if (remaining > 0) memmove(ctx->buffer, ctx->buffer + consumed, remaining);
        ctx->len = remaining;
    }
    return 0;
}

static client_ctx_t* ctx_by_fd(int fd) {
    if (fd < 0 || fd >= MAX_EVENTS) return NULL;
    int slot = fd_to_slot[fd];
    if (slot < 0) return NULL;
    return all_clients[slot];
}

/* Callback for trie_cleanup_fd to immediately send granted notifications,
 * including for grants that exceed the wakeup array capacity. */
static void send_granted_cb(int fd, const char *path, void *ctx) {
    (void)ctx;
    if (!path || fd < 0) return;
    client_ctx_t *c = ctx_by_fd(fd);
    if (!c) return;
    char stack_esc[4096], resp[8192];
    size_t esc_size = strlen(path) * 6 + 1;
    char *escaped = (esc_size <= sizeof(stack_esc)) ? stack_esc : malloc(esc_size);
    if (!escaped) {
        send_json(c, "{\"status\": \"granted\", \"path\": \"<nomem>\"}\n");
        return;
    }
    if (json_escape_string(path, escaped, esc_size) < 0) {
        send_json(c, "{\"status\": \"granted\", \"path\": \"<overflow>\"}\n");
    } else {
        int needed = snprintf(NULL, 0,
                "{\"status\": \"granted\", \"path\": \"%s\"}\n", escaped);
        if (needed < 0 || needed >= (int)sizeof(resp)) {
            send_json(c, "{\"status\": \"granted\", \"path\": \"<overflow>\"}\n");
        } else {
            snprintf(resp, sizeof(resp),
                    "{\"status\": \"granted\", \"path\": \"%s\"}\n", escaped);
            send_json(c, resp);
        }
    }
    if (escaped != stack_esc) free(escaped);
}

int main(int argc, char *argv[]) {
    int listen_fd;
    struct sockaddr_un addr;
    struct epoll_event ev, events[MAX_EVENTS];
    
    lock_trie = trie_create();
    if (!lock_trie) {
        fprintf(stderr, "[di-vrr] Fatal: cannot create trie\n");
        exit(1);
    }
    signal(SIGINT, handle_shutdown);
    signal(SIGTERM, handle_shutdown);
    signal(SIGUSR1, handle_stats);

    const char *path_to_bind = socket_path;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--persist") == 0 && i + 1 < argc) {
            strncpy(persist_path, argv[i + 1], sizeof(persist_path) - 1);
            persist_path[sizeof(persist_path) - 1] = '\0';
            if (trie_load_persist(lock_trie, persist_path) < 0) {
                fprintf(stderr, "[di-vrr] Warning: failed to load persistence from %s: %s\n",
                        persist_path, strerror(errno));
            }
            i++;
        } else if (strcmp(argv[i], "--socket") == 0 && i + 1 < argc) {
            path_to_bind = argv[i + 1];
            i++;
        }
    }
    bound_socket_path = path_to_bind;

    memset(all_clients, 0, sizeof(all_clients));
    for (int i = 0; i < MAX_EVENTS; i++) fd_to_slot[i] = -1;
    for (int i = 0; i < MAX_EVENTS; i++) free_slots[i] = i;
    free_slots_top = MAX_EVENTS;
    listen_fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (listen_fd == -1) { perror("socket"); exit(1); }

    int reuse = 1;
    if (setsockopt(listen_fd, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse)) < 0) {
        fprintf(stderr, "[di-vrr] setsockopt SO_REUSEADDR failed: %s\n", strerror(errno));
    }
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, path_to_bind, sizeof(addr.sun_path) - 1);
    if (unlink(path_to_bind) < 0 && errno != ENOENT) {
        fprintf(stderr, "[di-vrr] unlink(%s) failed: %s\n", path_to_bind, strerror(errno));
    }

    if (bind(listen_fd, (struct sockaddr *)&addr, sizeof(addr)) == -1) { perror("bind"); exit(1); }
    if (listen(listen_fd, 128) == -1) { perror("listen"); exit(1); }

    int flags = fcntl(listen_fd, F_GETFL, 0);
    if (flags < 0) { perror("fcntl F_GETFL"); exit(1); }
    if (fcntl(listen_fd, F_SETFL, flags | O_NONBLOCK) < 0) { perror("fcntl F_SETFL"); exit(1); }

    g_epoll_fd = epoll_create1(0);
    ev.events = EPOLLIN;
    ev.data.fd = listen_fd;
    epoll_ctl(g_epoll_fd, EPOLL_CTL_ADD, listen_fd, &ev);

    printf("[di-vrr] Coordination Daemon ready on %s\n", bound_socket_path);
    if (persist_path[0]) printf("[di-vrr] Persistence enabled: %s\n", persist_path);

    while (1) {
        if (shutdown_requested) {
            if (persist_path[0] && lock_trie) {
                if (trie_save_persist(lock_trie, persist_path) < 0) {
                    fprintf(stderr, "[di-vrr] CRITICAL: failed to save persistence on shutdown\n");
                }
            }
            unlink(bound_socket_path);
            break;
        }
        int nfds = epoll_wait(g_epoll_fd, events, MAX_EVENTS, -1);
        if (nfds < 0) {
            if (errno == EINTR) continue;
            perror("epoll_wait"); break;
        }
        for (int i = 0; i < nfds; i++) {
            if (events[i].data.fd == listen_fd) {
                int client_fd = accept(listen_fd, NULL, NULL);
                if (client_fd == -1) {
                    if (errno == EAGAIN || errno == EWOULDBLOCK) continue;
                    perror("accept"); break;
                }
                
                int cflags = fcntl(client_fd, F_GETFL, 0);
                if (cflags < 0) { perror("fcntl F_GETFL"); close(client_fd); continue; }
                if (fcntl(client_fd, F_SETFL, cflags | O_NONBLOCK) < 0) { perror("fcntl F_SETFL"); close(client_fd); continue; }

                client_ctx_t *ctx = calloc(1, sizeof(client_ctx_t));
                if (!ctx) { close(client_fd); continue; }
                ctx->fd = client_fd;
                ctx->buffer = malloc(4096);
                if (!ctx->buffer) { free(ctx); close(client_fd); continue; }
                ctx->buf_cap = 4096;
                ctx->len = 0;
                ctx->outbuf = malloc(4096);
                if (!ctx->outbuf) { free(ctx->buffer); free(ctx); close(client_fd); continue; }
                ctx->out_cap = 4096;
                ctx->out_len = 0;
                ctx->out_epoll_registered = false;

                int slot = --free_slots_top;
                if (slot < 0) {
                    free_slots_top++; /* restore — we didn't use a slot */
                    static const char rej[] = "{\"status\": \"error\", \"message\": \"server busy\"}\n";
                    ssize_t _r = write(client_fd, rej, sizeof(rej) - 1);
                    if (_r < 0) {
                        fprintf(stderr, "[di-vrr] rejected write failed on fd %d: %s\n", client_fd, strerror(errno));
                    }
                    if (close(client_fd) < 0) {
                        fprintf(stderr, "[di-vrr] close failed on fd %d: %s\n", client_fd, strerror(errno));
                    }
                    free(ctx);
                    fprintf(stderr, "[di-vrr] rejected connection: client limit reached (%d)\n", MAX_EVENTS);
                    g_rejected_connections++;
                    continue;
                }
                all_clients[slot] = ctx;
                if (client_fd >= MAX_EVENTS) {
                    fprintf(stderr, "[di-vrr] reject: client_fd %d >= MAX_EVENTS (%d)\n", client_fd, MAX_EVENTS);
                    free(ctx->buffer); free(ctx->outbuf); free(ctx);
                    close(client_fd);
                    free_slots[free_slots_top++] = slot;
                    continue;
                }
                fd_to_slot[client_fd] = slot;

                ev.events = EPOLLIN;
                ev.data.ptr = ctx;
                if (epoll_ctl(g_epoll_fd, EPOLL_CTL_ADD, client_fd, &ev) < 0) {
                    fprintf(stderr, "[di-vrr] failed to epoll_ctl ADD for client_fd %d: %s\n", client_fd, strerror(errno));
                    all_clients[slot] = NULL;
                    fd_to_slot[client_fd] = -1;
                    free_slots[free_slots_top++] = slot;
                    close(client_fd);
                    free(ctx->buffer);
                    free(ctx->outbuf);
                    free(ctx);
                    continue;
                }
            } else {
                client_ctx_t *ctx = events[i].data.ptr;
                if (events[i].events & EPOLLOUT) {
                    if (drain_output(ctx) < 0) {
                        trie_cleanup_fd(lock_trie, ctx->fd, NULL, NULL, 0,
                                        send_granted_cb, NULL);
                        int slot = fd_to_slot[ctx->fd];
                        fd_to_slot[ctx->fd] = -1;
                        if (slot >= 0) {
                            all_clients[slot] = NULL;
                            free_slots[free_slots_top++] = slot;
                        }
                        if (epoll_ctl(g_epoll_fd, EPOLL_CTL_DEL, ctx->fd, NULL) < 0 && errno != EBADF) {
                            fprintf(stderr, "[di-vrr] epoll_ctl DEL failed on fd %d: %s\n", ctx->fd, strerror(errno));
                        }
                        if (close(ctx->fd) < 0) {
                            fprintf(stderr, "[di-vrr] close failed on fd %d: %s\n", ctx->fd, strerror(errno));
                        }
                        free(ctx->buffer);
                        free(ctx->outbuf);
                        free(ctx);
                        continue;
                    }
                }
                if (events[i].events & EPOLLIN) {
                    if (handle_client_data(ctx, lock_trie) < 0) {
                        trie_cleanup_fd(lock_trie, ctx->fd, NULL, NULL, 0,
                                        send_granted_cb, NULL);
                        int slot = fd_to_slot[ctx->fd];
                        fd_to_slot[ctx->fd] = -1;
                        if (slot >= 0) {
                            all_clients[slot] = NULL;
                            free_slots[free_slots_top++] = slot;
                        }
                        if (epoll_ctl(g_epoll_fd, EPOLL_CTL_DEL, ctx->fd, NULL) < 0 && errno != EBADF) {
                            fprintf(stderr, "[di-vrr] epoll_ctl DEL failed on fd %d: %s\n", ctx->fd, strerror(errno));
                        }
                        if (close(ctx->fd) < 0) {
                            fprintf(stderr, "[di-vrr] close failed on fd %d: %s\n", ctx->fd, strerror(errno));
                        }
                        free(ctx->buffer);
                        free(ctx->outbuf);
                        free(ctx);
                        continue;
                    }
                }
            }
        }
    }
    trie_destroy(lock_trie);
    return 0;
}
