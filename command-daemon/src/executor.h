#ifndef EXECUTOR_H
#define EXECUTOR_H

#include <stddef.h>
#include <sys/types.h>

#define EXEC_MAX_OUTPUT 10240   /* 10KB */
#define EXEC_CWD_MARKER "DI_CWD:"
#define EXEC_MAX_CHILDREN 8

/* Long-running command patterns (regex-free, simple substring match) */
#define LONG_RUNNING_MAX 16

/* A running child process tracked by the event loop */
typedef struct {
    pid_t pid;
    char *id;              /* request ID (malloc'd) */
    int stdout_fd;
    int stderr_fd;
    char *stdout_buf;
    size_t stdout_len;
    size_t total_stdout_bytes;  /* bytes received before truncation */
    char *stderr_buf;
    size_t stderr_len;
    size_t total_stderr_bytes;  /* bytes received before truncation */
    int stdout_done;       /* stdout pipe closed */
    int stderr_done;       /* stderr pipe closed */
    int exited;
    int exit_code;
    int timed_out;
    long start_ms;         /* monotonic start time in ms */
    int timeout_ms;        /* command timeout */
    long last_progress_ms; /* monotonic time of last progress event */
    char cwd[4096];        /* resolved cwd after command */
    char session_id[128];  /* session to update cwd on (empty = no session) */
    int active;            /* slot in use */
    int pidfd;             /* pidfd for child exit notification (-1 if unavailable) */
} ExecChild;

/* Detect if a command is likely long-running (build/test/etc) */
int executor_is_long_running(const char *command);

/* Fork a command (non-blocking). Returns 0 on success, -1 on error.
   Child is tracked in *out. */
int executor_fork(const char *command, const char *cwd, ExecChild *out);

/* Free buffers and close fds in ExecChild */
void exec_child_cleanup(ExecChild *child);

#endif
