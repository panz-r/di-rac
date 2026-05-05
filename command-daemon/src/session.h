#ifndef SESSION_H
#define SESSION_H

#include <stddef.h>

#define SESSION_MAX 64
#define SESSION_ID_MAX 128
#define SESSION_ENV_MAX 32
#define SESSION_ENV_KEY_MAX 64
#define SESSION_ENV_VAL_MAX 256
#define SESSION_CWD_MAX 4096
#define SESSION_TIMEOUT_S 1800 /* 30 minutes */

typedef struct {
    char key[SESSION_ENV_KEY_MAX];
    char val[SESSION_ENV_VAL_MAX];
} SessionEnv;

typedef struct {
    char id[SESSION_ID_MAX];
    char cwd[SESSION_CWD_MAX];
    SessionEnv env[SESSION_ENV_MAX];
    int env_count;
    long last_activity; /* time_t as unix seconds */
} Session;

typedef struct {
    Session sessions[SESSION_MAX];
    int count;
} SessionStore;

void session_store_init(SessionStore *store);
Session *session_get_or_create(SessionStore *store, const char *id, const char *default_cwd);
Session *session_get(SessionStore *store, const char *id);
void session_cleanup_expired(SessionStore *store);

#endif
