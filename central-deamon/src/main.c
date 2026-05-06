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

#include "trie.h"

#define MAX_EVENTS 64
#define BUF_SIZE 65536
#define SOCKET_PATH "/tmp/di-vrr-coord.sock"

typedef struct {
    int fd;
    char buffer[BUF_SIZE + 1];
    size_t len;
} client_ctx_t;

static int set_nonblocking(int fd) {
    int flags = fcntl(fd, F_GETFL, 0);
    if (flags == -1) return -1;
    return fcntl(fd, F_SETFL, flags | O_NONBLOCK);
}

static void send_json(int fd, const char *json) {
    size_t len = strlen(json);
    if (write(fd, json, len) < (ssize_t)len && errno != EPIPE) {
        /* Ignore */
    }
}

static const char* find_string_val(const char *json, const char *key, char *out, size_t out_len) {
    char pattern[128];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char *p = strstr(json, pattern);
    if (!p) return NULL;
    p = strchr(p + strlen(pattern), ':');
    if (!p) return NULL;
    p = strchr(p, '"');
    if (!p) return NULL;
    const char *start = p + 1;
    const char *end = strchr(start, '"');
    if (!end) return NULL;
    size_t len = end - start;
    if (len >= out_len) len = out_len - 1;
    memcpy(out, start, len);
    out[len] = '\0';
    return end;
}

static void process_single_object(int fd, const char *json, trie_t *trie) {
    char method[64] = {0}, path[4096] = {0};
    
    if (!find_string_val(json, "method", method, sizeof(method)) ||
        !find_string_val(json, "path", path, sizeof(path))) {
        send_json(fd, "{\"status\": \"error\", \"message\": \"invalid protocol format\"}\n");
        return;
    }

    if (strcmp(method, "acquire") == 0) {
        int res = trie_acquire_lock(trie, path, fd);
        if (res == 0) send_json(fd, "{\"status\": \"ok\"}\n");
        else if (res == 1) send_json(fd, "{\"status\": \"waiting\"}\n");
        else send_json(fd, "{\"status\": \"denied\"}\n");
    } else if (strcmp(method, "release") == 0) {
        int next_fd = trie_release_lock(trie, path, fd);
        send_json(fd, "{\"status\": \"ok\"}\n");
        if (next_fd != -1) {
            send_json(next_fd, "{\"status\": \"granted\", \"path\": \"*\"}\n");
        }
    } else {
        send_json(fd, "{\"status\": \"error\", \"message\": \"unknown method\"}\n");
    }
}

static const char* find_end_of_object(const char *start, const char *end) {
    int depth = 0;
    bool in_string = false;
    for (const char *p = start; p < end; p++) {
        if (*p == '"' && (p == start || *(p-1) != '\\')) in_string = !in_string;
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

    if (ctx->len + n > BUF_SIZE) {
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

int main() {
    int listen_fd, epoll_fd;
    struct sockaddr_un addr;
    struct epoll_event ev, events[MAX_EVENTS];
    trie_t *lock_trie = trie_create();

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
    set_nonblocking(listen_fd);

    epoll_fd = epoll_create1(0);
    ev.events = EPOLLIN;
    ev.data.fd = listen_fd;
    epoll_ctl(epoll_fd, EPOLL_CTL_ADD, listen_fd, &ev);

    printf("[di-vrr] Coordination Daemon ready on %s\n", SOCKET_PATH);

    while (1) {
        int nfds = epoll_wait(epoll_fd, events, MAX_EVENTS, -1);
        for (int i = 0; i < nfds; i++) {
            if (events[i].data.fd == listen_fd) {
                int client_fd = accept(listen_fd, NULL, NULL);
                if (client_fd == -1) continue;
                set_nonblocking(client_fd);
                client_ctx_t *ctx = calloc(1, sizeof(client_ctx_t));
                ctx->fd = client_fd;
                ev.events = EPOLLIN;
                ev.data.ptr = ctx;
                epoll_ctl(epoll_fd, EPOLL_CTL_ADD, client_fd, &ev);
            } else {
                client_ctx_t *ctx = events[i].data.ptr;
                if (handle_client_data(ctx, lock_trie) < 0) {
                    int wakeup[256];
                    size_t w_count = trie_cleanup_fd(lock_trie, ctx->fd, wakeup, 256);
                    for (size_t j = 0; j < (w_count > 256 ? 256 : w_count); j++) {
                        send_json(wakeup[j], "{\"status\": \"granted\", \"path\": \"*\"}\n");
                    }
                    epoll_ctl(epoll_fd, EPOLL_CTL_DEL, ctx->fd, NULL);
                    close(ctx->fd);
                    free(ctx);
                }
            }
        }
    }
    trie_destroy(lock_trie);
    return 0;
}
