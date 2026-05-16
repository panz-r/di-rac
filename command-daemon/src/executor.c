#define _GNU_SOURCE
#include "executor.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <time.h>
#include <signal.h>
#include <sys/wait.h>
#include <sys/syscall.h>
#include <errno.h>
#include <fcntl.h>
#include <libgen.h>

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

static long now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static void get_exe_dir(char *buf, size_t size) {
    ssize_t len = readlink("/proc/self/exe", buf, size - 1);
    if (len == -1) {
        strncpy(buf, ".", size - 1);
        buf[size - 1] = '\0';
        return;
    }
    buf[len] = '\0';
    /* Find last slash to get directory */
    char *last_slash = strrchr(buf, '/');
    if (last_slash) *last_slash = '\0';
    else strncpy(buf, ".", size - 1);
}

static char* shell_escape(const char *s) {
    if (!s) return strdup("");
    size_t len = strlen(s);
    /* Worst case: every char is a single quote, each becomes 4 chars: '\'' */
    char *escaped = malloc(len * 4 + 1);
    if (!escaped) return NULL;
    char *d = escaped;
    for (size_t i = 0; i < len; i++) {
        if (s[i] == '\'') {
            memcpy(d, "'\\''", 4);
            d += 4;
        } else {
            *d++ = s[i];
        }
    }
    *d = '\0';
    return escaped;
}

/* Fork a command without blocking. Sets up pipes and returns immediately. */
int executor_fork(const char *command, const char *cwd, ExecChild *out) {
    memset(out, 0, sizeof(*out));
    out->stdout_fd = -1;
    out->stderr_fd = -1;
    out->pidfd = -1;

    char exe_dir[4096];
    get_exe_dir(exe_dir, sizeof(exe_dir));

    char *runner_path = NULL;
    char *python_wasm = NULL;
    char *python_lib = NULL;
    char *shim_path = NULL;
    char *new_path = NULL;
    char *esc_runner = NULL;
    char *esc_wasm = NULL;
    char *esc_lib = NULL;
    char *esc_path = NULL;
    char *esc_lib_dir = NULL;
    char *full_cmd = NULL;
    int stdout_pipe[2] = {-1, -1}, stderr_pipe[2] = {-1, -1};

    if (asprintf(&runner_path, "%s/wasm-runner", exe_dir) < 0) goto cleanup;
    if (access(runner_path, F_OK) != 0) {
        free(runner_path);
        if (asprintf(&runner_path, "%s/../dist/wasm-runner", exe_dir) < 0) { runner_path = NULL; goto cleanup; }
    }
    if (asprintf(&python_wasm, "%s/../standalone/runtime-files/python.wasm", exe_dir) < 0) goto cleanup;
    if (asprintf(&python_lib, "%s/../standalone/runtime-files/usr/local/lib", exe_dir) < 0) goto cleanup;
    if (asprintf(&shim_path, "%s/shims", exe_dir) < 0) goto cleanup;
    if (access(shim_path, F_OK) != 0) {
        free(shim_path);
        if (asprintf(&shim_path, "%s/../dist/shims", exe_dir) < 0) { shim_path = NULL; goto cleanup; }
    }

    char *old_path = getenv("PATH");
    if (old_path && *old_path) {
        if (asprintf(&new_path, "%s:%s", shim_path, old_path) < 0) goto cleanup;
    } else {
        new_path = strdup(shim_path);
        if (!new_path) goto cleanup;
    }

    /* Escape paths for safe shell interpolation */
    if (!(esc_runner = shell_escape(runner_path))) goto cleanup;
    if (!(esc_wasm = shell_escape(python_wasm))) goto cleanup;
    if (!(esc_lib = shell_escape(python_lib))) goto cleanup;
    if (!(esc_path = shell_escape(new_path))) goto cleanup;
    if (!(esc_lib_dir = shell_escape(exe_dir))) goto cleanup;

    const char *python_wrapper = 
        "python3() { "
        "\"$WASM_RUNNER\" --wasm \"$PYTHON_WASM\" --preopen \"/lib:$PYTHON_LIB\" --preopen \".:.\" -- \"$@\"; "
        "}; "
        "python() { python3 \"$@\"; }; "
        "export -f python3; export -f python; ";

    const char *cwd_suffix = "\n_ec=$?; printf '" EXEC_CWD_MARKER "%s\\n' \"$PWD\" >&2; exit $_ec";
    
    /* Intersperse environment variables into the bash command string itself to avoid setenv() in child */
    if (asprintf(&full_cmd, "WASM_RUNNER='%s' PYTHON_WASM='%s' PYTHON_LIB='%s' PATH='%s' LD_LIBRARY_PATH='%s'${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}\n%s\n%s%s", 
                 esc_runner, esc_wasm, esc_lib, esc_path, esc_lib_dir, python_wrapper, command, cwd_suffix) < 0) goto cleanup;

    if (pipe(stdout_pipe) < 0 || pipe(stderr_pipe) < 0) goto cleanup;

    pid_t pid = fork();
    if (pid < 0) goto cleanup;

    if (pid == 0) {
        /* Child - ultra-safe block (strictly async-signal-safe) */
        close(stdout_pipe[0]);
        close(stderr_pipe[0]);
        dup2(stdout_pipe[1], STDOUT_FILENO);
        dup2(stderr_pipe[1], STDERR_FILENO);
        close(stdout_pipe[1]);
        close(stderr_pipe[1]);
        
        if (cwd && chdir(cwd) != 0) _exit(1);

        execl("/bin/bash", "bash", "-c", full_cmd, NULL);
        _exit(127);
    }

    /* Parent */
    close(stdout_pipe[1]);
    close(stderr_pipe[1]);

    fcntl(stdout_pipe[0], F_SETFL, O_NONBLOCK);
    fcntl(stderr_pipe[0], F_SETFL, O_NONBLOCK);

    out->pid = pid;
    out->pidfd = syscall(SYS_pidfd_open, pid, 0);  /* may be -1 on old kernels */
    out->stdout_fd = stdout_pipe[0];
    out->stderr_fd = stderr_pipe[0];
    out->start_ms = now_ms();
    out->active = 1;
    out->id = NULL;

    if (cwd) {
        strncpy(out->cwd, cwd, sizeof(out->cwd) - 1);
        out->cwd[sizeof(out->cwd) - 1] = '\0';
    } else {
        out->cwd[0] = '\0';
    }

    free(runner_path); free(python_wasm); free(python_lib); free(shim_path); free(new_path);
    free(esc_runner); free(esc_wasm); free(esc_lib); free(esc_path); free(esc_lib_dir); free(full_cmd);
    return 0;


cleanup:
    if (stdout_pipe[0] != -1) { close(stdout_pipe[0]); close(stdout_pipe[1]); }
    if (stderr_pipe[0] != -1) { close(stderr_pipe[0]); close(stderr_pipe[1]); }
    free(runner_path); free(python_wasm); free(python_lib); free(shim_path); free(new_path);
    free(esc_runner); free(esc_wasm); free(esc_lib); free(esc_path); free(esc_lib_dir); free(full_cmd);
    return -1;
}

void exec_child_cleanup(ExecChild *child) {
    if (child->stdout_buf) { free(child->stdout_buf); child->stdout_buf = NULL; }
    if (child->stderr_buf) { free(child->stderr_buf); child->stderr_buf = NULL; }
    if (child->id) { free(child->id); child->id = NULL; }
    if (child->stdout_fd >= 0) { close(child->stdout_fd); child->stdout_fd = -1; }
    if (child->stderr_fd >= 0) { close(child->stderr_fd); child->stderr_fd = -1; }
    if (child->pidfd >= 0) { close(child->pidfd); child->pidfd = -1; }
    child->stdout_len = 0;
    child->stderr_len = 0;
    child->active = 0;
}
