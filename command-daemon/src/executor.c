#define _GNU_SOURCE
#include "executor.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <time.h>
#include <signal.h>
#include <sys/wait.h>
#include <errno.h>

static const char *long_running_patterns[] = {
    "npm install", "npm ci", "npm run build", "npm run test", "npm test",
    "pnpm install", "pnpm build", "pnpm test",
    "yarn install", "yarn build", "yarn test",
    "bun install", "bun build", "bun test",
    "cargo build", "cargo test", "cargo check",
    "go build", "go test",
    "pip install", "pip3 install",
    "make", "cmake", "ctest",
    "docker build", "podman build",
    "pytest", "jest", "vitest", "mocha",
    NULL,
};

int executor_is_long_running(const char *command) {
    for (int i = 0; long_running_patterns[i]; i++) {
        if (strstr(command, long_running_patterns[i]))
            return 1;
    }
    return 0;
}

static void capture_output(int fd, char **buf, size_t *len, int max_size) {
    char tmp[4096];
    ssize_t n;
    while ((n = read(fd, tmp, sizeof(tmp))) > 0) {
        size_t new_len = *len + (size_t)n;
        if (*len < (size_t)max_size) {
            size_t append = new_len > (size_t)max_size ? (size_t)max_size - *len : (size_t)n;
            *buf = realloc(*buf, *len + append + 1);
            if (*buf) {
                memcpy(*buf + *len, tmp, append);
                *len += append;
                (*buf)[*len] = '\0';
            }
        }
    }
}

/* Extract CWD from stderr. Looks for "DIRAC_CWD:<path>\n" lines. */
static void extract_cwd(char *stderr_buf, size_t stderr_len, char *cwd_out, size_t cwd_size) {
    char *marker = stderr_buf;
    char *last_cwd = NULL;
    size_t last_cwd_len = 0;
    while ((marker = strstr(marker, EXEC_CWD_MARKER)) != NULL) {
        char *start = marker + strlen(EXEC_CWD_MARKER);
        char *end = strchr(start, '\n');
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

int executor_run(const char *command, const char *cwd, int timeout_ms, ExecResult *result) {
    int stdout_pipe[2], stderr_pipe[2];
    if (pipe(stdout_pipe) < 0 || pipe(stderr_pipe) < 0) return -1;

    /* Build the full command with CWD tracking suffix */
    const char *cwd_suffix = "; printf '" EXEC_CWD_MARKER "%s\\n' \"$PWD\" >&2";
    size_t cmd_len = strlen(command) + strlen(cwd_suffix) + 1;
    char *full_cmd = malloc(cmd_len);
    if (!full_cmd) { close(stdout_pipe[0]); close(stdout_pipe[1]); close(stderr_pipe[0]); close(stderr_pipe[1]); return -1; }
    snprintf(full_cmd, cmd_len, "%s%s", command, cwd_suffix);

    pid_t pid = fork();
    if (pid < 0) {
        free(full_cmd);
        close(stdout_pipe[0]); close(stdout_pipe[1]);
        close(stderr_pipe[0]); close(stderr_pipe[1]);
        return -1;
    }

    if (pid == 0) {
        /* Child */
        close(stdout_pipe[0]);
        close(stderr_pipe[0]);
        dup2(stdout_pipe[1], STDOUT_FILENO);
        dup2(stderr_pipe[1], STDERR_FILENO);
        close(stdout_pipe[1]);
        close(stderr_pipe[1]);
        if (cwd && chdir(cwd) != 0) {
            /* If chdir fails, continue in inherited cwd */
        }
        execl("/bin/bash", "bash", "-c", full_cmd, NULL);
        _exit(127);
    }

    /* Parent */
    free(full_cmd);
    close(stdout_pipe[1]);
    close(stderr_pipe[1]);

    memset(result, 0, sizeof(*result));
    result->stdout_buf = NULL;
    result->stderr_buf = NULL;
    result->exit_code = -1;

    /* Capture output */
    capture_output(stdout_pipe[0], &result->stdout_buf, &result->stdout_len, EXEC_MAX_OUTPUT);
    capture_output(stderr_pipe[0], &result->stderr_buf, &result->stderr_len, EXEC_MAX_OUTPUT);

    close(stdout_pipe[0]);
    close(stderr_pipe[0]);

    /* Wait with timeout */
    int status = 0;
    pid_t ret;
    int waited = 0;
    struct timespec ts = {0, 10000000}; /* 10ms */
    int elapsed = 0;

    while (elapsed < timeout_ms) {
        ret = waitpid(pid, &status, WNOHANG);
        if (ret == pid) { waited = 1; break; }
        if (ret < 0) break;
        nanosleep(&ts, NULL);
        elapsed += 10;
    }

    if (!waited) {
        /* Timeout - kill the process */
        kill(pid, SIGKILL);
        waitpid(pid, &status, 0);
        result->timed_out = 1;
    }

    if (WIFEXITED(status)) {
        result->exit_code = WEXITSTATUS(status);
    } else if (WIFSIGNALED(status)) {
        result->exit_code = 128 + WTERMSIG(status);
    }

    result->truncated = (result->stdout_len >= (size_t)EXEC_MAX_OUTPUT ||
                         result->stderr_len >= (size_t)EXEC_MAX_OUTPUT) ? 1 : 0;

    /* Extract CWD from stderr */
    strncpy(result->cwd, cwd ? cwd : "", sizeof(result->cwd) - 1);
    if (result->stderr_buf && result->stderr_len > 0) {
        extract_cwd(result->stderr_buf, result->stderr_len, result->cwd, sizeof(result->cwd));
    }

    /* Strip CWD marker lines from stderr before returning to caller */
    if (result->stderr_buf) {
        char *src = result->stderr_buf;
        char *dst = result->stderr_buf;
        while (*src) {
            char *line_start = src;
            char *newline = strchr(src, '\n');
            if (!newline) {
                /* Last line without newline */
                if (strstr(line_start, EXEC_CWD_MARKER) != line_start) {
                    size_t len = strlen(line_start);
                    memmove(dst, src, len);
                    dst += len;
                }
                break;
            }
            newline++; /* include the \n */
            size_t line_len = (size_t)(newline - line_start);
            if (strstr(line_start, EXEC_CWD_MARKER) != line_start) {
                memmove(dst, src, line_len);
                dst += line_len;
            }
            src = newline;
        }
        *dst = '\0';
        result->stderr_len = (size_t)(dst - result->stderr_buf);
    }

    return 0;
}

void executor_result_free(ExecResult *result) {
    free(result->stdout_buf);
    free(result->stderr_buf);
    result->stdout_buf = NULL;
    result->stderr_buf = NULL;
    result->stdout_len = 0;
    result->stderr_len = 0;
}
