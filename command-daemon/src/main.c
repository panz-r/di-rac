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
#include "protocol.h"
#include "session.h"
#include "executor.h"

#define MAX_CHILDREN EXEC_MAX_CHILDREN

static ExecChild children[MAX_CHILDREN];

/* Find a free child slot */
static ExecChild *alloc_child(void) {
    for (int i = 0; i < MAX_CHILDREN; i++) {
        if (!children[i].active) return &children[i];
    }
    return NULL;
}

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
static void main_buf_append(char **buf, size_t *len, const char *data, size_t data_len, int max_size) {
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
        char **buf = is_stdout ? &child->stdout_buf : &child->stderr_buf;
        size_t *len = is_stdout ? &child->stdout_len : &child->stderr_len;
        main_buf_append(buf, len, tmp, (size_t)n, EXEC_MAX_OUTPUT);
    }
}

/* Send the final result for a completed child */
static void send_child_result(ExecChild *child) {
    /* Extract and strip CWD from stderr */
    if (child->stderr_buf && child->stderr_len > 0) {
        extract_cwd(child->stderr_buf, child->stderr_len, child->cwd, sizeof(child->cwd));
        strip_cwd_markers(child->stderr_buf, &child->stderr_len);
    }

    int truncated = (child->stdout_len >= (size_t)EXEC_MAX_OUTPUT ||
                     child->stderr_len >= (size_t)EXEC_MAX_OUTPUT) ? 1 : 0;

    printf("{\"type\":\"result\",\"id\":\"%s\",", child->id ? child->id : "");
    printf("\"stdout\":");
    write_json_string_limited(child->stdout_buf ? child->stdout_buf : "", 8000);
    printf(",\"stderr\":");
    write_json_string_limited(child->stderr_buf ? child->stderr_buf : "", 2000);
    printf(",\"exit_code\":%d,", child->exit_code);
    printf("\"meta\":{");
    printf("\"mode_used\":\"full\",");
    printf("\"cwd\":");
    write_json_string(child->cwd);
    printf(",\"truncated\":%s", truncated ? "true" : "false");
    printf(",\"truncation_offset\":null");
    printf(",\"hint\":null");
    printf(",\"blocked\":null");
    printf(",\"timed_out\":%s", child->timed_out ? "true" : "false");
    printf(",\"detected_patterns\":[]");
    printf("}}\n");
    fflush(stdout);
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

    fprintf(stderr, "ready\n");
    fflush(stderr);

    /* Set stdin to non-blocking for poll-based reading */
    fcntl(STDIN_FILENO, F_SETFL, O_NONBLOCK);

    char *line = NULL;
    size_t line_cap = 0;
    FILE *in = stdin;

    /* Event loop */
    while (1) {
        /* Build poll set: stdin + all child pipes */
        struct pollfd pfds[1 + MAX_CHILDREN * 2];
        int nfds = 0;
        ExecChild *fd_child_map[1 + MAX_CHILDREN * 2];

        /* Always poll stdin */
        pfds[0].fd = STDIN_FILENO;
        pfds[0].events = POLLIN;
        pfds[0].revents = 0;
        fd_child_map[0] = NULL;
        nfds = 1;

        /* Poll child pipes */
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

        int poll_ms = 100; /* 100ms tick for timeout checks */
        int pret = poll(pfds, (nfds_t)nfds, poll_ms);
        if (pret < 0) {
            if (errno == EINTR) continue;
            break;
        }

        /* Handle child pipe events */
        if (pret > 0) {
            for (int j = 1; j < nfds; j++) {
                ExecChild *c = fd_child_map[j];
                if (!c || !c->active) continue;

                if (pfds[j].revents & POLLIN) {
                    char tmp[4096];
                    ssize_t n = read(pfds[j].fd, tmp, sizeof(tmp));
                    if (n > 0) {
                        int is_stdout = (pfds[j].fd == c->stdout_fd);
                        char **buf = is_stdout ? &c->stdout_buf : &c->stderr_buf;
                        size_t *len = is_stdout ? &c->stdout_len : &c->stderr_len;
                        main_buf_append(buf, len, tmp, (size_t)n, EXEC_MAX_OUTPUT);
                    } else if (n <= 0) {
                        /* Pipe closed */
                        if (pfds[j].fd == c->stdout_fd) c->stdout_done = 1;
                        else c->stderr_done = 1;
                    }
                }
                if (pfds[j].revents & (POLLHUP | POLLERR)) {
                    /* Drain remaining then close */
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

        /* Check if any children have exited or timed out */
        for (int i = 0; i < MAX_CHILDREN; i++) {
            ExecChild *c = &children[i];
            if (!c->active) continue;

            /* Check for child exit (non-blocking) */
            if (!c->exited) {
                int status;
                pid_t ret = waitpid(c->pid, &status, WNOHANG);
                if (ret == c->pid) {
                    c->exited = 1;
                    if (WIFEXITED(status)) c->exit_code = WEXITSTATUS(status);
                    else if (WIFSIGNALED(status)) c->exit_code = 128 + WTERMSIG(status);

                    /* Drain remaining output */
                    if (!c->stdout_done) { drain_pipe(c->stdout_fd, c, 1); c->stdout_done = 1; }
                    if (!c->stderr_done) { drain_pipe(c->stderr_fd, c, 0); c->stderr_done = 1; }
                }
            }

            /* Check timeout */
            if (!c->exited) {
                long elapsed = now_ms() - c->start_ms;
                if (elapsed >= c->timeout_ms) {
                    kill(c->pid, SIGKILL);
                    c->exited = 1;
                    c->timed_out = 1;
                    c->exit_code = 124;
                    /* Drain remaining */
                    if (!c->stdout_done) { drain_pipe(c->stdout_fd, c, 1); c->stdout_done = 1; }
                    if (!c->stderr_done) { drain_pipe(c->stderr_fd, c, 0); c->stderr_done = 1; }
                }
            }

            /* If exited and all pipes drained, send result and cleanup */
            if (c->exited && c->stdout_done && c->stderr_done) {
                send_child_result(c);
                exec_child_cleanup(c);
            }
        }

        /* Handle stdin (non-blocking) */
        if (pfds[0].revents & POLLIN) {
            while (1) {
                ssize_t len = getline(&line, &line_cap, in);
                if (len < 0) {
                    if (errno == EAGAIN || errno == EWOULDBLOCK) break;
                    break; /* EOF or error */
                }
                if (len == 0) continue;
                if (line[len - 1] == '\n') line[len - 1] = '\0';
                if (len > 1 && line[len - 2] == '\r') line[len - 2] = '\0';
                if (line[0] == '\0') continue;

                /* Dispatch request */
                if (strstr(line, "\"session_info\"")) {
                    proto_handle_session_info(line, &store, workspace_root);
                } else if (strstr(line, "\"execute\"")) {
                    /* Parse request */
                    char *id = json_get_string(line, "id");
                    char *command = json_get_string(line, "command");
                    char *session_id = json_get_string(line, "session_id");

                    if (!command) {
                        printf("{\"type\":\"error\",\"id\":\"%s\",\"code\":\"INVALID_REQUEST\",\"message\":\"Missing required field 'command'\"}\n",
                               id ? id : "");
                        fflush(stdout);
                        free(id); free(command); free(session_id);
                        continue;
                    }

                    /* Find a free slot */
                    ExecChild *slot = alloc_child();
                    if (!slot) {
                        printf("{\"type\":\"error\",\"id\":\"%s\",\"code\":\"BUSY\",\"message\":\"Too many concurrent commands\"}\n",
                               id ? id : "");
                        fflush(stdout);
                        free(id); free(command); free(session_id);
                        continue;
                    }

                    /* Resolve cwd from session */
                    const char *cwd = workspace_root;
                    Session *session = NULL;
                    if (session_id && session_id[0]) {
                        session = session_get_or_create(&store, session_id, workspace_root);
                        if (session) cwd = session->cwd;
                    }
                    session_cleanup_expired(&store);

                    /* Fork the command */
                    if (executor_fork(command, cwd, slot) < 0) {
                        printf("{\"type\":\"error\",\"id\":\"%s\",\"code\":\"FORK_FAILED\",\"message\":\"Failed to start command\"}\n",
                               id ? id : "");
                        fflush(stdout);
                        free(id); free(command); free(session_id);
                        continue;
                    }

                    slot->id = id;
                    int timeout_ms = executor_is_long_running(command) ? 300000 : 30000;
                    slot->timeout_ms = timeout_ms;

                    /* Update session cwd on completion (handled in send_child_result) */
                    /* Store session pointer index for later update */
                    (void)session; /* session cwd updated by CWD marker extraction */

                    /* Send ack immediately */
                    printf("{\"type\":\"ack\",\"id\":\"%s\"}\n", id ? id : "");
                    fflush(stdout);

                    free(command);
                    free(session_id);
                    /* id is now owned by slot */
                } else {
                    char *id = json_get_string(line, "id");
                    printf("{\"type\":\"error\",\"id\":\"%s\",\"code\":\"UNKNOWN_TYPE\",\"message\":\"Unknown request type\"}\n",
                           id ? id : "");
                    fflush(stdout);
                    free(id);
                }
            }
        }

        /* Check for stdin EOF */
        if (pfds[0].revents & (POLLHUP | POLLERR)) {
            break;
        }
    }

    /* Cleanup: kill any running children */
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
