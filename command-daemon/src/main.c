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

/* Append data to a dynamic buffer, up to max_size bytes. */
static void main_buf_append(char **buf, size_t *len, size_t *total,
                            const char *data, size_t data_len, int max_size) {
    *total += data_len;
    if (*len >= (size_t)max_size) return;
    size_t avail = (size_t)max_size - *len;
    size_t append = data_len < avail ? data_len : avail;
    *buf = realloc(*buf, *len + append + 1);
    if (*buf) {
        memcpy(*buf + *len, data, append);
        *len += append;
        (*buf)[*len] = '\0';
    }
}

/* Drain a pipe into the child's buffer */
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

/* Generate a hint string based on command outcome. */
static const char *compute_hint(ExecChild *child, int truncated) {
    if (child->timed_out)
        return "command timed out";
    if (truncated)
        return "output exceeded limit";
    return NULL;
}

/* Send the final result for a completed child */
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
    const char *hint = compute_hint(child, truncated);

    pthread_mutex_lock(&stdout_lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "result");
    jsonw_kv_str_or_null(&w, "id", child->id);

    jsonw_key(&w, "stdout");
    jsonw_strn(&w, child->stdout_buf ? child->stdout_buf : "",
               (int)(child->stdout_buf ? child->stdout_len : 0), 8000);
    jsonw_key(&w, "stderr");
    jsonw_strn(&w, child->stderr_buf ? child->stderr_buf : "",
               (int)(child->stderr_buf ? child->stderr_len : 0), 2000);

    jsonw_kv_int(&w, "exit_code", child->exit_code);

    jsonw_key(&w, "meta");
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "mode_used", "full");
    jsonw_kv_str(&w, "cwd", child->cwd);
    jsonw_kv_bool(&w, "truncated", truncated);
    jsonw_kv_int(&w, "truncation_offset", truncation_offset);
    jsonw_kv_str_or_null(&w, "hint", hint);
    jsonw_kv_bool(&w, "timed_out", child->timed_out);
    jsonw_key(&w, "detected_patterns");
    jsonw_array_open(&w);
    jsonw_array_close(&w);
    jsonw_object_close(&w);

    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(&stdout_lock);
}

static void add_recent_file(const char *path) {
    pthread_mutex_lock(&recent_files.lock);
    /* Avoid duplicates */
    int prev = (recent_files.head - 1 + RECENT_FILES_MAX) % RECENT_FILES_MAX;
    if (recent_files.count > 0 && strcmp(recent_files.paths[prev], path) == 0) {
        pthread_mutex_unlock(&recent_files.lock);
        return;
    }

    strncpy(recent_files.paths[recent_files.head], path, 4095);
    recent_files.head = (recent_files.head + 1) % RECENT_FILES_MAX;
    if (recent_files.count < RECENT_FILES_MAX) recent_files.count++;
    pthread_mutex_unlock(&recent_files.lock);
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
        .stdout_lock = stdout_lock,
    };

    /* inotify setup */
    int inotify_fd = inotify_init1(IN_NONBLOCK);
    if (inotify_fd >= 0) {
        inotify_add_watch(inotify_fd, workspace_root, IN_MODIFY | IN_CREATE | IN_MOVED_TO);
    }

    fprintf(stderr, "ready\n");
    fflush(stderr);

    fcntl(STDIN_FILENO, F_SETFL, O_NONBLOCK);

    char *line = NULL;
    size_t line_cap = 0;
    FILE *in = stdin;

    while (1) {
        struct pollfd pfds[2 + MAX_CHILDREN * 2];
        int nfds = 0;
        ExecChild *fd_child_map[2 + MAX_CHILDREN * 2];

        pfds[0].fd = STDIN_FILENO;
        pfds[0].events = POLLIN;
        pfds[0].revents = 0;
        fd_child_map[0] = NULL;
        nfds = 1;

        if (inotify_fd >= 0) {
            pfds[nfds].fd = inotify_fd;
            pfds[nfds].events = POLLIN;
            pfds[nfds].revents = 0;
            fd_child_map[nfds] = NULL;
            nfds++;
        }

        for (int i = 0; i < MAX_CHILDREN; i++) {
            ExecChild *c = &children[i];
            if (!c->active) continue;
            if (!c->stdout_done && c->stdout_fd >= 0) {
                pfds[nfds].fd = c->stdout_fd;
                pfds[nfds].events = POLLIN;
                pfds[nfds].revents = 0;
                fd_child_map[nfds] = c;
                nfds++;
            }
            if (!c->stderr_done && c->stderr_fd >= 0) {
                pfds[nfds].fd = c->stderr_fd;
                pfds[nfds].events = POLLIN;
                pfds[nfds].revents = 0;
                fd_child_map[nfds] = c;
                nfds++;
            }
        }

        int poll_ms = 100;
        int pret = poll(pfds, (nfds_t)nfds, poll_ms);
        if (pret < 0) {
            if (errno == EINTR) continue;
            break;
        }

        if (pret > 0) {
            /* Handle inotify events */
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
                                    add_recent_file(event->name);
                                }
                            }
                        }
                    }
                }
            }

            /* Handle child pipe events */
            for (int j = 0; j < nfds; j++) {
                ExecChild *c = fd_child_map[j];
                if (!c || !c->active) continue;

                if (pfds[j].revents & POLLIN) {
                    char tmp[4096];
                    ssize_t n = read(pfds[j].fd, tmp, sizeof(tmp));
                    if (n > 0) {
                        int is_stdout = (pfds[j].fd == c->stdout_fd);
                        if (is_stdout) {
                            main_buf_append(&c->stdout_buf, &c->stdout_len,
                                            &c->total_stdout_bytes, tmp, (size_t)n, EXEC_MAX_OUTPUT);
                        } else {
                            main_buf_append(&c->stderr_buf, &c->stderr_len,
                                            &c->total_stderr_bytes, tmp, (size_t)n, EXEC_MAX_OUTPUT);
                        }
                    } else if (n <= 0) {
                        if (pfds[j].fd == c->stdout_fd) c->stdout_done = 1;
                        else c->stderr_done = 1;
                    }
                }
                if (pfds[j].revents & (POLLHUP | POLLERR)) {
                    if (pfds[j].fd == c->stdout_fd) {
                        drain_pipe(c->stdout_fd, c, 1);
                        c->stdout_done = 1;
                    } else {
                        drain_pipe(c->stderr_fd, c, 0);
                        c->stderr_done = 1;
                    }
                }
            }
        }

        /* Check for child exit or timeout */
        for (int i = 0; i < MAX_CHILDREN; i++) {
            ExecChild *c = &children[i];
            if (!c->active) continue;
            if (!c->exited) {
                int status;
                pid_t ret = waitpid(c->pid, &status, WNOHANG);
                if (ret == c->pid) {
                    c->exited = 1;
                    if (WIFEXITED(status)) c->exit_code = WEXITSTATUS(status);
                    else if (WIFSIGNALED(status)) c->exit_code = 128 + WTERMSIG(status);
                    if (!c->stdout_done) { drain_pipe(c->stdout_fd, c, 1); c->stdout_done = 1; }
                    if (!c->stderr_done) { drain_pipe(c->stderr_fd, c, 0); c->stderr_done = 1; }
                }
            }
            if (!c->exited) {
                long elapsed = now_ms() - c->start_ms;
                if (elapsed >= c->timeout_ms) {
                    kill(c->pid, SIGKILL);
                    c->exited = 1;
                    c->timed_out = 1;
                    c->exit_code = 124;
                    if (!c->stdout_done) { drain_pipe(c->stdout_fd, c, 1); c->stdout_done = 1; }
                    if (!c->stderr_done) { drain_pipe(c->stderr_fd, c, 0); c->stderr_done = 1; }
                }
            }
            if (c->exited && c->stdout_done && c->stderr_done) {
                send_child_result(c);
                exec_child_cleanup(c);
            }
        }

        /* Handle stdin */
        if (pfds[0].revents & POLLIN) {
            while (1) {
                ssize_t len = getline(&line, &line_cap, in);
                if (len < 0) {
                    if (errno == EAGAIN || errno == EWOULDBLOCK) break;
                    break;
                }
                if (len == 0) continue;
                if (line[len - 1] == '\n') line[len - 1] = '\0';
                if (len > 1 && line[len - 2] == '\r') line[len - 2] = '\0';
                if (line[0] == '\0') continue;
                proto_handle_line(line, (int)strlen(line), &ctx);
            }
        }
        if (pfds[0].revents & (POLLHUP | POLLERR)) break;
    }

    for (int i = 0; i < MAX_CHILDREN; i++) {
        if (children[i].active) {
            kill(children[i].pid, SIGKILL);
            waitpid(children[i].pid, NULL, 0);
            exec_child_cleanup(&children[i]);
        }
    }
    free(line);
    return 0;
}
