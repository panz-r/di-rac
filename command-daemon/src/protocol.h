#ifndef PROTOCOL_H
#define PROTOCOL_H

#include "executor.h"
#include "session.h"
#include "safety.h"

/* Max length of a single JSON line from stdin */
#define PROTO_MAX_LINE 65536

/* Context passed to the protocol handler */
struct proto_ctx {
    ExecChild *children;
    int max_children;
    SessionStore *sessions;
    const char *workspace_root;
};

/* Parse and dispatch a single JSON request line.
 * Returns 0 on success, -1 on parse error. */
int proto_handle_line(const char *line, int line_len, struct proto_ctx *ctx);

#endif /* PROTOCOL_H */
