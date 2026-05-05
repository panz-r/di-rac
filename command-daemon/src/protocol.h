#ifndef PROTOCOL_H
#define PROTOCOL_H

#include "executor.h"
#include "session.h"

/* Max length of a single JSON line from stdin */
#define PROTO_MAX_LINE 65536

/* Dispatch a request line (JSON string) and write response to stdout.
   store is the session store, default_cwd is the workspace root. */
void proto_handle_request(const char *line, SessionStore *store, const char *default_cwd);

#endif
