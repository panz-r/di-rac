#include "protocol.h"
#include "json.h"
#include "json-write.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>
#include <time.h>
#include <stdbool.h>
#include <pthread.h>

/* ---- response helpers ---- */

static void send_error(pthread_mutex_t *lock, const char *id, const char *code, const char *message) {
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "error");
    jsonw_kv_str_or_null(&w, "id", id);
    jsonw_kv_str(&w, "code", code);
    jsonw_kv_str(&w, "message", message);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void send_ack(pthread_mutex_t *lock, const char *id, int timeout_ms) {
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ack");
    jsonw_kv_str_or_null(&w, "id", id);
    jsonw_kv_int(&w, "timeout_ms", timeout_ms);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void send_blocked_result(pthread_mutex_t *lock, const char *id, const struct safety_result *sr) {
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "result");
    jsonw_kv_str_or_null(&w, "id", id);
    jsonw_key(&w, "stdout"); jsonw_str(&w, "");
    jsonw_key(&w, "stderr"); jsonw_str(&w, "");
    jsonw_kv_int(&w, "exit_code", 1);
    jsonw_key(&w, "meta");
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "mode_used", "full");
    jsonw_kv_str(&w, "cwd", "");
    jsonw_kv_bool(&w, "truncated", false);
    jsonw_kv_int(&w, "truncation_offset", 0);
    jsonw_kv_str(&w, "hint", "blocked for safety");
    jsonw_kv_str(&w, "blocked", sr->match_count > 0 ? sr->reasons[0] : "unknown");
    jsonw_kv_bool(&w, "timed_out", false);
    jsonw_key(&w, "detected_patterns");
    jsonw_array_open(&w);
    for (int i = 0; i < sr->match_count; i++)
        jsonw_str(&w, sr->reasons[i]);
    jsonw_array_close(&w);
    jsonw_object_close(&w); /* meta */
    jsonw_object_close(&w); /* top */
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

/* ---- walk handler ---- */

typedef struct WalkNode {
    char path[4096];
    struct WalkNode *next;
} WalkNode;

typedef struct {
    char path[4096];
    long mtime;
    long size;
} WalkFile;

typedef struct {
    char id[64];
    char root[4096];
    int limit;
    bool recursive;
    pthread_mutex_t *stdout_lock;
} WalkRequest;

static int should_ignore(const char *name) {
    if (name[0] == '.' && name[1] == '\0') return 1;
    if (name[0] == '.' && name[1] == '.' && name[2] == '\0') return 1;
    if (name[0] == '.') return 1;
    if (strcmp(name, "node_modules") == 0) return 1;
    if (strcmp(name, "dist") == 0) return 1;
    if (strcmp(name, "build") == 0) return 1;
    return 0;
}

static void* walk_thread_worker(void *arg) {
    WalkRequest *req = (WalkRequest*)arg;
    
    WalkFile *files = malloc(sizeof(WalkFile) * req->limit);
    int files_found = 0;

    /* BFS using a queue */
    WalkNode *head = malloc(sizeof(WalkNode));
    if (head) {
        strncpy(head->path, req->root, 4095);
        head->next = NULL;
    }
    WalkNode *tail = head;

    while (head && files_found < req->limit) {
        WalkNode *curr = head;
        head = head->next;
        if (!head) tail = NULL;

        DIR *d = opendir(curr->path);
        if (d) {
            struct dirent *de;
            while ((de = readdir(d)) != NULL && files_found < req->limit) {
                if (should_ignore(de->d_name)) continue;

                char full_path[4096];
                snprintf(full_path, sizeof(full_path), "%s/%s", curr->path, de->d_name);

                struct stat st;
                if (lstat(full_path, &st) < 0) continue;

                if (S_ISDIR(st.st_mode)) {
                    if (req->recursive) {
                        WalkNode *n = malloc(sizeof(WalkNode));
                        if (n) {
                            strncpy(n->path, full_path, 4095);
                            n->next = NULL;
                            if (tail) tail->next = n;
                            else head = n;
                            tail = n;
                        }
                    }
                } else if (S_ISREG(st.st_mode)) {
                    strncpy(files[files_found].path, full_path, 4095);
                    files[files_found].mtime = (long)st.st_mtime;
                    files[files_found].size = (long)st.st_size;
                    files_found++;
                }
            }
            closedir(d);
        }
        free(curr);
    }

    while (head) {
        WalkNode *tmp = head;
        head = head->next;
        free(tmp);
    }

    /* Send results atomically */
    pthread_mutex_lock(req->stdout_lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "walk_result");
    jsonw_key(&w, "id");
    if (req->id[0]) {
        /* Write raw numeric ID if possible */
        bool numeric = true;
        for (int i=0; req->id[i]; i++) if (req->id[i] < '0' || req->id[i] > '9') numeric = false;
        if (numeric) fprintf(stdout, "%s", req->id);
        else fprintf(stdout, "\"%s\"", req->id);
        w.need_comma = true;
    } else { jsonw_null(&w); }

    jsonw_key(&w, "files");
    jsonw_array_open(&w);
    for (int i = 0; i < files_found; i++) {
        jsonw_object_open(&w);
        jsonw_kv_str(&w, "path", files[i].path);
        jsonw_kv_int(&w, "mtime", files[i].mtime);
        jsonw_kv_int(&w, "size", files[i].size);
        jsonw_object_close(&w);
    }
    jsonw_array_close(&w);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(req->stdout_lock);

    free(files);
    free(req);
    return NULL;
}

static void handle_walk(const char *line, int line_len, struct proto_ctx *ctx) {
    WalkRequest *req = calloc(1, sizeof(WalkRequest));
    req->limit = 1000;
    req->recursive = true;
    req->stdout_lock = ctx->stdout_lock;

    struct json j;
    if (json_enter_object(&j, line, line_len) < 0) {
        send_error(ctx->stdout_lock, "", "INVALID_REQUEST", "Malformed JSON");
        free(req);
        return;
    }
    char key[64];
    const char *val;
    int vlen;
    while ((vlen = json_next_key(&j, key, (int)sizeof(key), &val)) > 0) {
        if (strcmp(key, "id") == 0) json_get_raw_str(val, vlen, req->id, sizeof(req->id));
        else if (strcmp(key, "dir") == 0) json_get_str(val, vlen, req->root, sizeof(req->root));
        else if (strcmp(key, "limit") == 0) json_get_int(val, vlen, &req->limit);
        else if (strcmp(key, "recursive") == 0) json_get_bool(val, vlen, &req->recursive);
    }

    if (!req->root[0]) strncpy(req->root, ctx->workspace_root, sizeof(req->root) - 1);

    pthread_t thread;
    if (pthread_create(&thread, NULL, walk_thread_worker, req) != 0) {
        send_error(ctx->stdout_lock, req->id, "THREAD_FAILED", "Failed to spawn walk thread");
        free(req);
    } else {
        pthread_detach(thread);
    }
}

/* ---- recent_files handler ---- */

static void handle_recent_files(const char *line, int line_len, struct proto_ctx *ctx) {
    char id[64] = "";
    struct json j;
    if (json_enter_object(&j, line, line_len) >= 0) {
        char key[64]; const char *val; int vlen;
        while ((vlen = json_next_key(&j, key, (int)sizeof(key), &val)) > 0) {
            if (strcmp(key, "id") == 0) json_get_raw_str(val, vlen, id, (int)sizeof(id));
        }
    }

    pthread_mutex_lock(ctx->stdout_lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "recent_files_result");
    jsonw_kv_str_or_null(&w, "id", id);
    jsonw_key(&w, "files");
    jsonw_array_open(&w);

    RecentFilesStore *s = ctx->recent_files;
    pthread_mutex_lock(&s->lock);
    for (int i = 0; i < s->count; i++) {
        int idx = (s->head - 1 - i + RECENT_FILES_MAX) % RECENT_FILES_MAX;
        jsonw_str(&w, s->paths[idx]);
    }
    pthread_mutex_unlock(&s->lock);

    jsonw_array_close(&w);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(ctx->stdout_lock);
}

/* ---- session_info handler ---- */

static void handle_session_info(const char *line, int line_len,
                                struct proto_ctx *ctx) {
    char id[64] = "", session_id[128] = "";
    struct json j;
    if (json_enter_object(&j, line, line_len) < 0) {
        send_error(ctx->stdout_lock, "", "INVALID_REQUEST", "Malformed JSON");
        return;
    }
    char key[64];
    const char *val;
    int vlen;
    while ((vlen = json_next_key(&j, key, (int)sizeof(key), &val)) > 0) {
        if (strcmp(key, "id") == 0) json_get_raw_str(val, vlen, id, (int)sizeof(id));
        else if (strcmp(key, "session_id") == 0) json_get_str(val, vlen, session_id, (int)sizeof(session_id));
    }

    if (!session_id[0]) {
        send_error(ctx->stdout_lock, id, "INVALID_REQUEST", "Missing required field 'session_id'");
        return;
    }

    Session *s = session_get_or_create(ctx->sessions, session_id, ctx->workspace_root);

    pthread_mutex_lock(ctx->stdout_lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "session_info_result");
    jsonw_kv_str_or_null(&w, "id", id);
    jsonw_kv_str(&w, "session_id", session_id);
    jsonw_kv_str(&w, "cwd", s ? s->cwd : ctx->workspace_root);
    jsonw_key(&w, "env"); jsonw_object_open(&w); jsonw_object_close(&w);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(ctx->stdout_lock);
}

/* ---- execute handler ---- */

static ExecChild *alloc_child(ExecChild *children, int max_children) {
    for (int i = 0; i < max_children; i++) {
        if (!children[i].active) return &children[i];
    }
    return NULL;
}

static void handle_execute(const char *line, int line_len,
                           struct proto_ctx *ctx) {
    char id[64] = "", command[PROTO_MAX_LINE] = "", session_id[128] = "";
    int client_timeout_s = -1;
    struct json j;
    if (json_enter_object(&j, line, line_len) < 0) {
        send_error(ctx->stdout_lock, "", "INVALID_REQUEST", "Malformed JSON");
        return;
    }
    char key[64];
    const char *val;
    int vlen;
    char cwd_field[4096] = "";
    while ((vlen = json_next_key(&j, key, (int)sizeof(key), &val)) > 0) {
        if (strcmp(key, "id") == 0) json_get_raw_str(val, vlen, id, (int)sizeof(id));
        else if (strcmp(key, "command") == 0) json_get_str(val, vlen, command, (int)sizeof(command));
        else if (strcmp(key, "session_id") == 0) json_get_str(val, vlen, session_id, (int)sizeof(session_id));
        else if (strcmp(key, "cwd") == 0) json_get_str(val, vlen, cwd_field, (int)sizeof(cwd_field));
        else if (strcmp(key, "timeout") == 0) json_get_int(val, vlen, &client_timeout_s);
    }

    if (!command[0]) {
        send_error(ctx->stdout_lock, id, "INVALID_REQUEST", "Missing required field 'command'");
        return;
    }

    struct safety_result sr = safety_check(command);
    if (sr.blocked) {
        send_blocked_result(ctx->stdout_lock, id, &sr);
        return;
    }

    ExecChild *slot = alloc_child(ctx->children, ctx->max_children);
    if (!slot) {
        send_error(ctx->stdout_lock, id, "BUSY", "Too many concurrent commands");
        return;
    }

    const char *cwd = ctx->workspace_root;
    /* Prefer explicit `cwd` field from client, then session cwd, then workspace root */
    if (cwd_field[0]) {
        cwd = cwd_field;
    } else if (session_id[0]) {
        Session *session = session_get_or_create(ctx->sessions, session_id, ctx->workspace_root);
        if (session) cwd = session->cwd;
    }
    session_cleanup_expired(ctx->sessions);

    int timeout_ms;
    if (client_timeout_s > 0) {
        int requested_ms = client_timeout_s * 1000;
        if (requested_ms > 600000) requested_ms = 600000;
        if (requested_ms < 1000) requested_ms = 1000;
        timeout_ms = requested_ms;
    } else {
        timeout_ms = executor_is_long_running(command) ? 600000 : 300000;
    }

    if (executor_fork(command, cwd, slot) < 0) {
        send_error(ctx->stdout_lock, id, "FORK_FAILED", "Failed to start command");
        return;
    }

    slot->id = strdup(id);
    slot->timeout_ms = timeout_ms;
    send_ack(ctx->stdout_lock, id, timeout_ms);
}

/* ---- dispatch ---- */

int proto_handle_line(const char *line, int line_len, struct proto_ctx *ctx) {
    char type[32] = "";
    if (json_obj_find_str(line, line_len, "type", type, (int)sizeof(type)) < 0) {
        send_error(ctx->stdout_lock, "", "INVALID_REQUEST", "Missing 'type' field");
        return -1;
    }

    if (strcmp(type, "session_info") == 0) {
        handle_session_info(line, line_len, ctx);
    } else if (strcmp(type, "execute") == 0) {
        handle_execute(line, line_len, ctx);
    } else if (strcmp(type, "walk") == 0) {
        handle_walk(line, line_len, ctx);
    } else if (strcmp(type, "recent_files") == 0) {
        handle_recent_files(line, line_len, ctx);
    } else {
        char id[64] = "";
        json_obj_find_str(line, line_len, "id", id, (int)sizeof(id));
        send_error(ctx->stdout_lock, id, "UNKNOWN_TYPE", "Unknown request type");
        return -1;
    }
    return 0;
}
