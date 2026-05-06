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
#include <fcntl.h>

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

/* Get monotonic time in milliseconds */
static long now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

/* Fork a command without blocking. Sets up pipes and returns immediately. */
int executor_fork(const char *command, const char *cwd, ExecChild *out) {
    int stdout_pipe[2], stderr_pipe[2];
    if (pipe(stdout_pipe) < 0 || pipe(stderr_pipe) < 0) return -1;

    const char *cwd_suffix = "; _ec=$?; printf '" EXEC_CWD_MARKER "%s\\n' \"$PWD\" >&2; exit $_ec";
    size_t cmd_len = strlen(command) + strlen(cwd_suffix) + 1;
    char *full_cmd = malloc(cmd_len);
    if (!full_cmd) {
        close(stdout_pipe[0]); close(stdout_pipe[1]);
        close(stderr_pipe[0]); close(stderr_pipe[1]);
        return -1;
    }
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
        if (cwd && chdir(cwd) != 0) { /* continue in inherited cwd */ }
        execl("/bin/bash", "bash", "-c", full_cmd, NULL);
        _exit(127);
    }

    /* Parent */
    free(full_cmd);
    close(stdout_pipe[1]);
    close(stderr_pipe[1]);

    /* Set pipes to non-blocking */
    fcntl(stdout_pipe[0], F_SETFL, O_NONBLOCK);
    fcntl(stderr_pipe[0], F_SETFL, O_NONBLOCK);

    memset(out, 0, sizeof(*out));
    out->pid = pid;
    out->stdout_fd = stdout_pipe[0];
    out->stderr_fd = stderr_pipe[0];
    out->start_ms = now_ms();
    out->active = 1;
    strncpy(out->cwd, cwd ? cwd : "", sizeof(out->cwd) - 1);

    return 0;
}

void exec_child_cleanup(ExecChild *child) {
    if (child->stdout_buf) { free(child->stdout_buf); child->stdout_buf = NULL; }
    if (child->stderr_buf) { free(child->stderr_buf); child->stderr_buf = NULL; }
    if (child->id) { free(child->id); child->id = NULL; }
    if (child->stdout_fd >= 0) { close(child->stdout_fd); child->stdout_fd = -1; }
    if (child->stderr_fd >= 0) { close(child->stderr_fd); child->stderr_fd = -1; }
    child->stdout_len = 0;
    child->stderr_len = 0;
    child->active = 0;
}
