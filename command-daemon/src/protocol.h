#ifndef PROTOCOL_H
#define PROTOCOL_H

#include "executor.h"
#include "session.h"
#include "safety.h"
#include <pthread.h>

/* Max length of a single JSON line from stdin */
#define PROTO_MAX_LINE 65536

#define RECENT_FILES_MAX 100

typedef struct {
    char paths[RECENT_FILES_MAX][4096];
    int count;
    int head;
    pthread_mutex_t lock;
} RecentFilesStore;

/* Context passed to the protocol handler */
struct proto_ctx {
    ExecChild *children;
    int max_children;
    SessionStore *sessions;
    RecentFilesStore *recent_files;
    const char *workspace_root;
    pthread_mutex_t stdout_lock;
};

/* Parse and dispatch a single JSON request line.
 * Returns 0 on success, -1 on parse error. */
int proto_handle_line(const char *line, int line_len, struct proto_ctx *ctx);

#endif /* PROTOCOL_H */
