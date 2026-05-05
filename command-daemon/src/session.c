#include "session.h"
#include <string.h>
#include <time.h>

void session_store_init(SessionStore *store) {
    memset(store, 0, sizeof(*store));
}

Session *session_get(SessionStore *store, const char *id) {
    if (!id || !id[0]) return NULL;
    for (int i = 0; i < store->count; i++) {
        if (strcmp(store->sessions[i].id, id) == 0) {
            store->sessions[i].last_activity = (long)time(NULL);
            return &store->sessions[i];
        }
    }
    return NULL;
}

Session *session_get_or_create(SessionStore *store, const char *id, const char *default_cwd) {
    if (!id || !id[0]) return NULL;
    Session *s = session_get(store, id);
    if (s) return s;

    if (store->count >= SESSION_MAX) {
        /* Evict oldest */
        int oldest = 0;
        for (int i = 1; i < store->count; i++) {
            if (store->sessions[i].last_activity < store->sessions[oldest].last_activity)
                oldest = i;
        }
        session_get(store, store->sessions[oldest].id); /* just for effect */
        memmove(&store->sessions[oldest], &store->sessions[oldest + 1],
                (store->count - oldest - 1) * sizeof(Session));
        store->count--;
    }

    s = &store->sessions[store->count++];
    memset(s, 0, sizeof(*s));
    strncpy(s->id, id, SESSION_ID_MAX - 1);
    strncpy(s->cwd, default_cwd, SESSION_CWD_MAX - 1);
    s->last_activity = (long)time(NULL);
    return s;
}

void session_cleanup_expired(SessionStore *store) {
    long now = (long)time(NULL);
    int write = 0;
    for (int read = 0; read < store->count; read++) {
        if (now - store->sessions[read].last_activity < SESSION_TIMEOUT_S) {
            if (write != read)
                store->sessions[write] = store->sessions[read];
            write++;
        }
    }
    store->count = write;
}
