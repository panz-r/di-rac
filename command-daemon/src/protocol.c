#include "protocol.h"
#include "json.h"
#include "json-write.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>
#include <time.h>

/* ---- response helpers ---- */

static void send_error(const char *id, const char *code, const char *message) {
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "error");
    jsonw_kv_str_or_null(&w, "id", id);
    jsonw_kv_str(&w, "code", code);
    jsonw_kv_str(&w, "message", message);
    jsonw_object_close(&w);
    jsonw_flush(&w);
}

static void send_ack(const char *id, int timeout_ms) {
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ack");
    jsonw_kv_str_or_null(&w, "id", id);
    jsonw_kv_int(&w, "timeout_ms", timeout_ms);
    jsonw_object_close(&w);
    jsonw_flush(&w);
}

static void send_blocked_result(const char *id, const struct safety_result *sr) {
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
}

/* ---- walk handler ---- */

typedef struct {
    char path[4096];
} WalkStack;

static int should_ignore(const char *name) {
    if (name[0] == '.' && name[1] == '\0') return 1;
    if (name[0] == '.' && name[1] == '.' && name[2] == '\0') return 1;
    if (name[0] == '.') return 1;
    if (strcmp(name, "node_modules") == 0) return 1;
    if (strcmp(name, "dist") == 0) return 1;
    if (strcmp(name, "build") == 0) return 1;
    if (strcmp(name, "bin") == 0) return 1;
    if (strcmp(name, "obj") == 0) return 1;
    if (strcmp(name, "venv") == 0) return 1;
    if (strcmp(name, ".venv") == 0) return 1;
    return 0;
}

static void handle_walk(const char *line, int line_len, struct proto_ctx *ctx) {
    char id[64] = "", dir_param[4096] = "";
    struct json j;
    if (json_enter_object(&j, line, line_len) < 0) {
        send_error("", "INVALID_REQUEST", "Malformed JSON");
        return;
    }
    char key[64];
    const char *val;
    int vlen;
    while ((vlen = json_next_key(&j, key, (int)sizeof(key), &val)) > 0) {
        if (strcmp(key, "id") == 0) json_get_str(val, vlen, id, (int)sizeof(id));
        else if (strcmp(key, "dir") == 0) json_get_str(val, vlen, dir_param, (int)sizeof(dir_param));
    }

    const char *root = dir_param[0] ? dir_param : ctx->workspace_root;
    
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "walk_result");
    jsonw_kv_str_or_null(&w, "id", id);
    jsonw_key(&w, "files");
    jsonw_array_open(&w);

    WalkStack *stack = malloc(sizeof(WalkStack) * 4096);
    int stack_ptr = 0;
    strncpy(stack[stack_ptr++].path, root, 4095);

    while (stack_ptr > 0) {
        char current_dir[4096];
        strncpy(current_dir, stack[--stack_ptr].path, 4095);

        DIR *d = opendir(current_dir);
        if (!d) continue;

        struct dirent *de;
        while ((de = readdir(d)) != NULL) {
            if (should_ignore(de->d_name)) continue;

            char full_path[4096];
            snprintf(full_path, sizeof(full_path), "%s/%s", current_dir, de->d_name);

            struct stat st;
            if (lstat(full_path, &st) < 0) continue;

            if (S_ISDIR(st.st_mode)) {
                if (stack_ptr < 4096) {
                    strncpy(stack[stack_ptr++].path, full_path, 4095);
                }
            } else if (S_ISREG(st.st_mode)) {
                jsonw_object_open(&w);
                jsonw_kv_str(&w, "path", full_path + strlen(root) + (full_path[strlen(root)] == '/' ? 1 : 0));
                jsonw_kv_int(&w, "mtime", (int)st.st_mtime);
                jsonw_kv_int(&w, "size", (int)st.st_size);
                jsonw_object_close(&w);
            }
        }
        closedir(d);
    }

    free(stack);
    jsonw_array_close(&w);
    jsonw_object_close(&w);
    jsonw_flush(&w);
}

/* ---- session_info handler ---- */

static void handle_session_info(const char *line, int line_len,
                                struct proto_ctx *ctx) {
    char id[64] = "", session_id[128] = "";
    struct json j;
    if (json_enter_object(&j, line, line_len) < 0) {
        send_error("", "INVALID_REQUEST", "Malformed JSON");
        return;
    }
    char key[64];
    const char *val;
    int vlen;
    while ((vlen = json_next_key(&j, key, (int)sizeof(key), &val)) > 0) {
        if (strcmp(key, "id") == 0) json_get_str(val, vlen, id, (int)sizeof(id));
        else if (strcmp(key, "session_id") == 0) json_get_str(val, vlen, session_id, (int)sizeof(session_id));
    }

    if (!session_id[0]) {
        send_error(id, "INVALID_REQUEST", "Missing required field 'session_id'");
        return;
    }

    Session *s = session_get_or_create(ctx->sessions, session_id, ctx->workspace_root);

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
        send_error("", "INVALID_REQUEST", "Malformed JSON");
        return;
    }
    char key[64];
    const char *val;
    int vlen;
    while ((vlen = json_next_key(&j, key, (int)sizeof(key), &val)) > 0) {
        if (strcmp(key, "id") == 0) json_get_str(val, vlen, id, (int)sizeof(id));
        else if (strcmp(key, "command") == 0) json_get_str(val, vlen, command, (int)sizeof(command));
        else if (strcmp(key, "session_id") == 0) json_get_str(val, vlen, session_id, (int)sizeof(session_id));
        else if (strcmp(key, "timeout") == 0) json_get_int(val, vlen, &client_timeout_s);
    }

    if (!command[0]) {
        send_error(id, "INVALID_REQUEST", "Missing required field 'command'");
        return;
    }

    struct safety_result sr = safety_check(command);
    if (sr.blocked) {
        send_blocked_result(id, &sr);
        return;
    }

    ExecChild *slot = alloc_child(ctx->children, ctx->max_children);
    if (!slot) {
        send_error(id, "BUSY", "Too many concurrent commands");
        return;
    }

    const char *cwd = ctx->workspace_root;
    if (session_id[0]) {
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
        timeout_ms = executor_is_long_running(command) ? 300000 : 30000;
    }

    if (executor_fork(command, cwd, slot) < 0) {
        send_error(id, "FORK_FAILED", "Failed to start command");
        return;
    }

    slot->id = strdup(id);
    slot->timeout_ms = timeout_ms;
    send_ack(id, timeout_ms);
}

/* ---- dispatch ---- */

int proto_handle_line(const char *line, int line_len, struct proto_ctx *ctx) {
    char type[32] = "";
    if (json_obj_find_str(line, line_len, "type", type, (int)sizeof(type)) < 0) {
        send_error("", "INVALID_REQUEST", "Missing 'type' field");
        return -1;
    }

    if (strcmp(type, "session_info") == 0) {
        handle_session_info(line, line_len, ctx);
    } else if (strcmp(type, "execute") == 0) {
        handle_execute(line, line_len, ctx);
    } else if (strcmp(type, "walk") == 0) {
        handle_walk(line, line_len, ctx);
    } else {
        char id[64] = "";
        json_obj_find_str(line, line_len, "id", id, (int)sizeof(id));
        send_error(id, "UNKNOWN_TYPE", "Unknown request type");
        return -1;
    }
    return 0;
}
