#ifndef PROTOCOL_H
#define PROTOCOL_H

#include "executor.h"
#include "session.h"

/* Max length of a single JSON line from stdin */
#define PROTO_MAX_LINE 65536

/* JSON helpers shared between main.c and protocol.c */
char *json_get_string(const char *json, const char *key);
void write_json_string(const char *s);
void write_json_string_limited(const char *s, size_t max_len);

/* Handle session_info request (sync, non-blocking) */
void proto_handle_session_info(const char *line, SessionStore *store, const char *default_cwd);

#endif
