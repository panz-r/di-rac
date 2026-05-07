#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <poll.h>
#include <signal.h>
#include <sys/wait.h>
#include <errno.h>
#include <time.h>
#include <fcntl.h>
#include <sys/inotify.h>
#include <libgen.h>
#include <pthread.h>
#include "protocol.h"
#include "json-write.h"
#include "session.h"
#include "executor.h"

#define MAX_CHILDREN EXEC_MAX_CHILDREN
#define STDIN_BUF_SIZE 65536

static ExecChild children[MAX_CHILDREN];
static RecentFilesStore recent_files;
static pthread_mutex_t stdout_lock = PTHREAD_MUTEX_INITIALIZER;

/* Get monotonic time in ms */
static long now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

/* Extract CWD from stderr. Looks for EXEC_CWD_MARKER lines. */
static void extract_cwd(const char *stderr_buf, size_t stderr_len, char *cwd_out, size_t cwd_size) {
    const char *marker = stderr_buf;
    const char *last_cwd = NULL;
    size_t last_cwd_len = 0;
    while ((marker = strstr(marker, EXEC_CWD_MARKER)) != NULL) {
        const char *start = marker + strlen(EXEC_CWD_MARKER);
        const char *end = strchr(start, '\n');
        if (!end) end = stderr_buf + stderr_len;
        last_cwd = start;
        last_cwd_len = (size_t)(end - start);
        marker = end + 1;
    }
    if (last_cwd && last_cwd_len > 0 && last_cwd_len < cwd_size) {
        memcpy(cwd_out, last_cwd, last_cwd_len);
        cwd_out[last_cwd_len] = '\0';
    }
}

/* Strip CWD marker lines from stderr */
static void strip_cwd_markers(char *stderr_buf, size_t *stderr_len) {
    if (!stderr_buf) return;
    char *src = stderr_buf;
    char *dst = stderr_buf;
    while (*src) {
        char *line_start = src;
        char *newline = strchr(src, '\n');
        if (!newline) {
            if (strstr(line_start, EXEC_CWD_MARKER) != line_start) {
                size_t len = strlen(line_start);
                memmove(dst, src, len);
                dst += len;
            }
            break;
        }
        newline++;
        size_t line_len = (size_t)(newline - line_start);
        if (strstr(line_start, EXEC_CWD_MARKER) != line_start) {
            memmove(dst, src, line_len);
            dst += line_len;
        }
        src = newline;
    }
    *dst = '\0';
    *stderr_len = (size_t)(dst - stderr_buf);
}

static void main_buf_append(char **buf, size_t *len, size_t *total,
                            const char *data, size_t data_len, int max_size) {
    *total += data_len;
    if (*len >= (size_t)max_size) return;
    size_t avail = (size_t)max_size - *len;
    size_t append = data_len < avail ? data_len : avail;
    char *new_buf = realloc(*buf, *len + append + 1);
    if (new_buf) {
        *buf = new_buf;
        memcpy(*buf + *len, data, append);
        *len += append;
        (*buf)[*len] = '\0';
    }
}

static void drain_pipe(int fd, ExecChild *child, int is_stdout) {
    char tmp[4096];
    while (1) {
        ssize_t n = read(fd, tmp, sizeof(tmp));
        if (n <= 0) break;
        if (is_stdout) {
            main_buf_append(&child->stdout_buf, &child->stdout_len,
                            &child->total_stdout_bytes, tmp, (size_t)n, EXEC_MAX_OUTPUT);
        } else {
            main_buf_append(&child->stderr_buf, &child->stderr_len,
                            &child->total_stderr_bytes, tmp, (size_t)n, EXEC_MAX_OUTPUT);
        }
    }
}

static void send_child_result(ExecChild *child) {
    if (child->stderr_buf && child->stderr_len > 0) {
        extract_cwd(child->stderr_buf, child->stderr_len, child->cwd, sizeof(child->cwd));
        strip_cwd_markers(child->stderr_buf, &child->stderr_len);
    }

    int truncated = (child->stdout_len >= (size_t)EXEC_MAX_OUTPUT ||
                     child->stderr_len >= (size_t)EXEC_MAX_OUTPUT) ? 1 : 0;
    int truncation_offset = truncated ? (int)(child->total_stdout_bytes > child->total_stderr_bytes
                                              ? child->total_stdout_bytes
                                              : child->total_stderr_bytes) : 0;

    pthread_mutex_lock(&stdout_lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "result");
    jsonw_kv_str_or_null(&w, "id", child->id);
    jsonw_key(&w, "stdout");
    jsonw_strn(&w, child->stdout_buf ? child->stdout_buf : "", (int)child->stdout_len, 0);
    jsonw_key(&w, "stderr");
    jsonw_strn(&w, child->stderr_buf ? child->stderr_buf : "", (int)child->stderr_len, 0);
    jsonw_kv_int(&w, "exit_code", child->exit_code);
    jsonw_key(&w, "meta");
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "cwd", child->cwd);
    jsonw_kv_bool(&w, "truncated", truncated);
    jsonw_kv_int(&w, "truncation_offset", truncation_offset);
    jsonw_kv_bool(&w, "timed_out", child->timed_out);
    jsonw_object_close(&w);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(&stdout_lock);
}

int main(int argc, char *argv[]) {
    char workspace_root[4096] = "";
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--workspace-root") == 0 && i + 1 < argc) {
            strncpy(workspace_root, argv[i + 1], sizeof(workspace_root) - 1);
            i++;
        }
    }
    if (!workspace_root[0]) {
        if (getcwd(workspace_root, sizeof(workspace_root)) == NULL) {
            strcpy(workspace_root, "/");
        }
    }

    SessionStore store;
    session_store_init(&store);
    memset(&recent_files, 0, sizeof(recent_files));
    pthread_mutex_init(&recent_files.lock, NULL);

    struct proto_ctx ctx = {
        .children = children,
        .max_children = MAX_CHILDREN,
        .sessions = &store,
        .recent_files = &recent_files,
        .workspace_root = workspace_root,
        .stdout_lock = &stdout_lock,
    };

    int inotify_fd = inotify_init1(IN_NONBLOCK);
    if (inotify_fd >= 0) inotify_add_watch(inotify_fd, workspace_root, IN_MODIFY | IN_CREATE | IN_MOVED_TO);

    fprintf(stderr, "ready\n");
    fflush(stderr);

    fcntl(STDIN_FILENO, F_SETFL, O_NONBLOCK);
    char stdin_buf[STDIN_BUF_SIZE];
    size_t stdin_len = 0;

    while (1) {
        struct pollfd pfds[2 + MAX_CHILDREN * 2];
        int nfds = 0;
        ExecChild *fd_child_map[2 + MAX_CHILDREN * 2];

        pfds[0].fd = STDIN_FILENO; pfds[0].events = POLLIN; nfds = 1; fd_child_map[0] = NULL;
        if (inotify_fd >= 0) { pfds[nfds].fd = inotify_fd; pfds[nfds].events = POLLIN; nfds++; fd_child_map[nfds-1] = NULL; }

        for (int i = 0; i < MAX_CHILDREN; i++) {
            ExecChild *c = &children[i];
            if (!c->active) continue;
            if (!c->stdout_done && c->stdout_fd >= 0) { pfds[nfds].fd = c->stdout_fd; pfds[nfds].events = POLLIN; fd_child_map[nfds++] = c; }
            if (!c->stderr_done && c->stderr_fd >= 0) { pfds[nfds].fd = c->stderr_fd; pfds[nfds].events = POLLIN; fd_child_map[nfds++] = c; }
        }

        int pret = poll(pfds, (nfds_t)nfds, 100);
        if (pret < 0 && errno != EINTR) break;

        if (pret > 0) {
            if (pfds[0].revents & (POLLIN | POLLHUP | POLLERR)) {
                ssize_t n = read(STDIN_FILENO, stdin_buf + stdin_len, STDIN_BUF_SIZE - stdin_len - 1);
                if (n > 0) {
                    stdin_len += (size_t)n;
                    stdin_buf[stdin_len] = '\0';
                    char *line_start = stdin_buf;
                    char *newline;
                    while ((newline = strchr(line_start, '\n')) != NULL) {
                        *newline = '\0';
                        proto_handle_line(line_start, (int)(newline - line_start), &ctx);
                        line_start = newline + 1;
                    }
                    size_t processed = (size_t)(line_start - stdin_buf);
                    if (processed > 0) {
                        memmove(stdin_buf, line_start, stdin_len - processed + 1);
                        stdin_len -= processed;
                    }
                } else if (n == 0 && (pfds[0].revents & (POLLHUP | POLLERR))) {
                    break;
                }
            }
            if (inotify_fd >= 0) {
                for (int j = 0; j < nfds; j++) {
                    if (pfds[j].fd == inotify_fd && (pfds[j].revents & POLLIN)) {
                        char buf[4096] __attribute__ ((aligned(__alignof__(struct inotify_event))));
                        ssize_t len = read(inotify_fd, buf, sizeof(buf));
                        if (len > 0) {
                            const struct inotify_event *event;
                            for (char *ptr = buf; ptr < buf + len; ptr += sizeof(struct inotify_event) + event->len) {
                                event = (const struct inotify_event *)ptr;
                                if (event->len > 0 && !(event->mask & IN_ISDIR)) {
                                    pthread_mutex_lock(&recent_files.lock);
                                    strncpy(recent_files.paths[recent_files.head], event->name, 4095);
                                    recent_files.head = (recent_files.head + 1) % RECENT_FILES_MAX;
                                    if (recent_files.count < RECENT_FILES_MAX) recent_files.count++;
                                    pthread_mutex_unlock(&recent_files.lock);
                                }
                            }
                        }
                    }
                }
            }
            for (int j = 1; j < nfds; j++) {
                ExecChild *c = fd_child_map[j];
                if (!c || !c->active) continue;
                if (pfds[j].revents & (POLLIN | POLLHUP | POLLERR)) {
                    char tmp[4096];
                    ssize_t sn = read(pfds[j].fd, tmp, sizeof(tmp));
                    if (sn > 0) {
                        if (pfds[j].fd == c->stdout_fd) main_buf_append(&c->stdout_buf, &c->stdout_len, &c->total_stdout_bytes, tmp, (size_t)sn, EXEC_MAX_OUTPUT);
                        else main_buf_append(&c->stderr_buf, &c->stderr_len, &c->total_stderr_bytes, tmp, (size_t)sn, EXEC_MAX_OUTPUT);
                    } else {
                        if (pfds[j].fd == c->stdout_fd) c->stdout_done = 1; else c->stderr_done = 1;
                    }
                }
            }
        }

        for (int i = 0; i < MAX_CHILDREN; i++) {
            ExecChild *c = &children[i];
            if (!c->active) continue;
            if (!c->exited) {
                int status;
                if (waitpid(c->pid, &status, WNOHANG) == c->pid) {
                    c->exited = 1;
                    c->exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : (WIFSIGNALED(status) ? 128 + WTERMSIG(status) : 1);
                    drain_pipe(c->stdout_fd, c, 1); c->stdout_done = 1;
                    drain_pipe(c->stderr_fd, c, 0); c->stderr_done = 1;
                } else if (now_ms() - c->start_ms >= c->timeout_ms) {
                    kill(c->pid, SIGKILL);
                    c->exited = 1;
                    c->timed_out = 1;
                    c->exit_code = 124;
                    drain_pipe(c->stdout_fd, c, 1); c->stdout_done = 1;
                    drain_pipe(c->stderr_fd, c, 0); c->stderr_done = 1;
                }
            }
            if (c->exited && c->stdout_done && c->stderr_done) {
                send_child_result(c);
                exec_child_cleanup(c);
            }
        }
    }
    return 0;
}
