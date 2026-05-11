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

#include "trie.h"

#define MAX_EVENTS 64
#define BUF_SIZE 65536
#define SOCKET_PATH "/tmp/di-vrr-coord.sock"

typedef struct {
    int fd;
    char buffer[BUF_SIZE + 1];
    size_t len;
} client_ctx_t;

static client_ctx_t *all_clients[MAX_EVENTS];
static char persist_path[4096] = {0};
static trie_t *lock_trie = NULL;
static volatile sig_atomic_t shutdown_requested = 0;

static int send_json(int fd, const char *json) {
    size_t len = strlen(json);
    ssize_t written = write(fd, json, len);
    if (written < 0) {
        if (errno != EPIPE) {
            fprintf(stderr, "[di-vrr] send_json failed on fd %d: %s\n", fd, strerror(errno));
        }
        return -1;
    }
    if ((size_t)written < len) {
        fprintf(stderr, "[di-vrr] partial send on fd %d (%zd of %zu bytes)\n", fd, written, len);
        return -1;
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

    /* Compute exact size needed for JSON message */
    size_t msg_size = 64 + strlen(escaped_path) + strlen(escaped_key)
                      + (value ? strlen(escaped_value) : 4);
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

    for (int i = 0; i < MAX_EVENTS; i++) {
        if (all_clients[i] && all_clients[i]->fd != sender_fd) {
            send_json(all_clients[i]->fd, msg);
        }
    }
    free(msg);
    return 0;
}

static void handle_shutdown(int sig) {
    (void)sig;
    shutdown_requested = 1;
}

/* Unescape a JSON string in place (or into dst). Handles:
 *   \\ → \   \" → "   \n → NL   \r → CR   \t → TAB
 *   \b → BS   \f → FF   \/ → /   \uXXXX → pass through
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
                case 'u':  /* \uXXXX — pass through as literal \uXXXX for now */ {
                               if (dst_pos + 6 >= dst_len) return -1;
                               dst[dst_pos++] = 'u';
                               for (int i = 0; i < 4; i++) {
                                   if (!*(p + 1 + i)) return -1;
                                   dst[dst_pos++] = *(++p);
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

    /* Extract raw string then unescape into caller's buffer */
    char raw[8192];
    size_t raw_len = (size_t)(end - start);
    if (raw_len >= sizeof(raw)) raw_len = sizeof(raw) - 1;
    memcpy(raw, start, raw_len);
    raw[raw_len] = '\0';

    if (json_unescape(raw, out, out_len) < 0) return NULL;
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

static void process_single_object(int fd, const char *json, trie_t *trie) {
    char method[64] = {0}, path[8192] = {0};
    
    if (!find_string_val(json, "method", method, sizeof(method)) ||
        !find_string_val(json, "path", path, sizeof(path))) {
        send_json(fd, "{\"status\": \"error\", \"message\": \"invalid protocol format\"}\n");
        return;
    }

    if (strcmp(method, "acquire") == 0) {
        bool wait = find_bool_val(json, "wait", false);
        int res = trie_acquire_lock(trie, path, fd, wait);
        if (res == 0) send_json(fd, "{\"status\": \"ok\"}\n");
        else if (res == 1) send_json(fd, "{\"status\": \"waiting\"}\n");
        else send_json(fd, "{\"status\": \"denied\"}\n");
    } else if (strcmp(method, "release") == 0) {
        int next_fd = trie_release_lock(trie, path, fd);
        send_json(fd, "{\"status\": \"ok\"}\n");
        if (next_fd != -1) {
            char escaped_path[4096];
            if (json_escape_string(path, escaped_path, sizeof(escaped_path)) < 0) {
                // Fall back to original path if escaping fails (should be rare)
                snprintf(escaped_path, sizeof(escaped_path), "%s", path);
            }
            char resp[8192 + 128];
            snprintf(resp, sizeof(resp), "{\"status\": \"granted\", \"path\": \"%s\"}\n", escaped_path);
            send_json(next_fd, resp);
        }
    } else if (strcmp(method, "set_config") == 0) {
        char key[256] = {0}, value[4096] = {0}, *val_ptr = NULL;
        if (!find_string_val(json, "key", key, sizeof(key))) {
            send_json(fd, "{\"status\": \"error\", \"message\": \"missing key\"}\n");
            return;
        }
        if (find_string_val(json, "value", value, sizeof(value))) val_ptr = value;
        bool transient = find_bool_val(json, "transient", false);
        
        trie_set_config(trie, path, fd, key, val_ptr, transient);
        send_json(fd, "{\"status\": \"ok\"}\n");
        
        if (!transient) {
            broadcast_config_update(fd, path, key, val_ptr);
            if (persist_path[0]) trie_save_persist(trie, persist_path);
        }
    } else if (strcmp(method, "get_config") == 0) {
        char key[256] = {0};
        if (!find_string_val(json, "key", key, sizeof(key))) {
            send_json(fd, "{\"status\": \"error\", \"message\": \"missing key\"}\n");
            return;
        }
        char *val = trie_get_config(trie, path, fd, key);
        if (val) {
            char escaped_val[8192];
            if (json_escape_string(val, escaped_val, sizeof(escaped_val)) < 0) {
                send_json(fd, "{\"status\": \"error\", \"message\": \"value too large\"}\n");
            } else {
                char resp[8192 + 128];
                snprintf(resp, sizeof(resp), "{\"status\": \"ok\", \"value\": \"%s\"}\n", escaped_val);
                send_json(fd, resp);
            }
            free(val);
        } else {
            send_json(fd, "{\"status\": \"ok\", \"value\": null}\n");
        }
    } else {
        send_json(fd, "{\"status\": \"error\", \"message\": \"unknown method\"}\n");
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

static size_t process_json_stream(int fd, char *data, size_t len, trie_t *trie) {
    char *p = data;
    char *end = data + len;
    while (p < end) {
        char *obj_start = strchr(p, '{');
        if (!obj_start) {
            p = end;
            break;
        }
        const char *obj_end = find_end_of_object(obj_start, end);
        if (obj_end) {
            char *mutable_obj_end = (char*)obj_end;
            char saved = *mutable_obj_end;
            *mutable_obj_end = '\0';
            process_single_object(fd, obj_start, trie);
            *mutable_obj_end = saved;
            p = mutable_obj_end;
        } else {
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
        if (errno == EAGAIN || errno == EWOULDBLOCK) return 0;
        return -1;
    }
    if (n == 0) return -1;

    if (ctx->len + n >= BUF_SIZE) {
        send_json(ctx->fd, "{\"status\": \"error\", \"message\": \"buffer overflow\"}\n");
        ctx->len = 0;
        return 0;
    }
    memcpy(ctx->buffer + ctx->len, read_buf, n);
    ctx->len += n;
    ctx->buffer[ctx->len] = '\0';
    size_t consumed = process_json_stream(ctx->fd, ctx->buffer, ctx->len, trie);
    if (consumed > 0) {
        size_t remaining = ctx->len - consumed;
        if (remaining > 0) memmove(ctx->buffer, ctx->buffer + consumed, remaining);
        ctx->len = remaining;
    }
    return 0;
}

int main(int argc, char *argv[]) {
    int listen_fd, epoll_fd;
    struct sockaddr_un addr;
    struct epoll_event ev, events[MAX_EVENTS];
    
    lock_trie = trie_create();
    signal(SIGINT, handle_shutdown);
    signal(SIGTERM, handle_shutdown);

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--persist") == 0 && i + 1 < argc) {
            strncpy(persist_path, argv[i + 1], sizeof(persist_path) - 1);
            trie_load_persist(lock_trie, persist_path);
            i++;
        }
    }

    memset(all_clients, 0, sizeof(all_clients));
    listen_fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (listen_fd == -1) { perror("socket"); exit(1); }

    int reuse = 1;
    setsockopt(listen_fd, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));

    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, SOCKET_PATH, sizeof(addr.sun_path) - 1);
    unlink(SOCKET_PATH);

    if (bind(listen_fd, (struct sockaddr *)&addr, sizeof(addr)) == -1) { perror("bind"); exit(1); }
    if (listen(listen_fd, 128) == -1) { perror("listen"); exit(1); }
    
    int flags = fcntl(listen_fd, F_GETFL, 0);
    fcntl(listen_fd, F_SETFL, flags | O_NONBLOCK);

    epoll_fd = epoll_create1(0);
    ev.events = EPOLLIN;
    ev.data.fd = listen_fd;
    epoll_ctl(epoll_fd, EPOLL_CTL_ADD, listen_fd, &ev);

    printf("[di-vrr] Coordination Daemon ready on %s\n", SOCKET_PATH);
    if (persist_path[0]) printf("[di-vrr] Persistence enabled: %s\n", persist_path);

    while (1) {
        if (shutdown_requested) {
            if (persist_path[0] && lock_trie) trie_save_persist(lock_trie, persist_path);
            unlink(SOCKET_PATH);
            break;
        }
        int nfds = epoll_wait(epoll_fd, events, MAX_EVENTS, -1);
        if (nfds < 0) {
            if (errno == EINTR) continue;
            perror("epoll_wait"); break;
        }
        for (int i = 0; i < nfds; i++) {
            if (events[i].data.fd == listen_fd) {
                int client_fd = accept(listen_fd, NULL, NULL);
                if (client_fd == -1) continue;
                
                int cflags = fcntl(client_fd, F_GETFL, 0);
                fcntl(client_fd, F_SETFL, cflags | O_NONBLOCK);

                client_ctx_t *ctx = calloc(1, sizeof(client_ctx_t));
                if (!ctx) { close(client_fd); continue; }
                ctx->fd = client_fd;

                int slot = -1;
                for (int j = 0; j < MAX_EVENTS; j++) {
                    if (all_clients[j] == NULL) { slot = j; break; }
                }
                if (slot == -1) {
                    close(client_fd);
                    free(ctx);
                    continue;
                }
                all_clients[slot] = ctx;

                ev.events = EPOLLIN;
                ev.data.ptr = ctx;
                epoll_ctl(epoll_fd, EPOLL_CTL_ADD, client_fd, &ev);
            } else {
                client_ctx_t *ctx = events[i].data.ptr;
                if (handle_client_data(ctx, lock_trie) < 0) {
                    int wakeup[1024];
                    char *w_paths[1024];
                    size_t w_count = trie_cleanup_fd(lock_trie, ctx->fd, wakeup, w_paths, 1024);
                    if (w_count == 1024) {
                        fprintf(stderr, "[di-vrr] warning: wakeup notification capped at 1024 for fd %d\n", ctx->fd);
                    }
                    for (size_t j = 0; j < w_count; j++) {
                        char escaped_path[4096];
                        if (json_escape_string(w_paths[j], escaped_path, sizeof(escaped_path)) < 0) {
                            snprintf(escaped_path, sizeof(escaped_path), "%s", w_paths[j]);
                        }
                        char resp[8192 + 128];
                        snprintf(resp, sizeof(resp), "{\"status\": \"granted\", \"path\": \"%s\"}\n", escaped_path);
                        send_json(wakeup[j], resp);
                        free(w_paths[j]);
                    }
                    epoll_ctl(epoll_fd, EPOLL_CTL_DEL, ctx->fd, NULL);
                    for (int j = 0; j < MAX_EVENTS; j++) {
                        if (all_clients[j] == ctx) { all_clients[j] = NULL; break; }
                    }
                    close(ctx->fd);
                    free(ctx);
                }
            }
        }
    }
    trie_destroy(lock_trie);
    return 0;
}
