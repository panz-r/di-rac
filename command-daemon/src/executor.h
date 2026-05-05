#ifndef EXECUTOR_H
#define EXECUTOR_H

#include <stddef.h>

#define EXEC_MAX_OUTPUT 10240   /* 10KB */
#define EXEC_HEAD_SIZE 4096
#define EXEC_TAIL_SIZE 4096
#define EXEC_CWD_MARKER "DIRAC_CWD:"

/* Long-running command patterns (regex-free, simple substring match) */
#define LONG_RUNNING_MAX 16

typedef struct {
    int exit_code;
    char *stdout_buf;
    char *stderr_buf;
    size_t stdout_len;
    size_t stderr_len;
    int timed_out;
    int truncated;
    char cwd[4096];     /* working directory after command */
} ExecResult;

/* Detect if a command is likely long-running (build/test/etc) */
int executor_is_long_running(const char *command);

/* Execute a command via /bin/bash -c. cwd is the working directory.
   timeout_ms is the timeout in milliseconds.
   Returns 0 on success, -1 on spawn error. */
int executor_run(const char *command, const char *cwd, int timeout_ms, ExecResult *result);

/* Free buffers in ExecResult */
void executor_result_free(ExecResult *result);

#endif
