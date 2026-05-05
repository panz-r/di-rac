#include "protocol.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Minimal JSON helpers for our limited schema.
   We don't need a full parser — just extract known fields from flat JSON objects. */

static char *json_get_string(const char *json, const char *key) {
    /* Find "key": "value" — returns malloc'd string or NULL */
    char search[256];
    snprintf(search, sizeof(search), "\"%s\"", key);
    const char *p = strstr(json, search);
    if (!p) return NULL;
    p += strlen(search);
    while (*p == ' ' || *p == ':' || *p == '\t') p++;
    if (*p != '"') return NULL;
    p++;
    const char *end = p;
    while (*end && *end != '"') {
        if (*end == '\\') end++; /* skip escaped chars */
        end++;
    }
    size_t len = (size_t)(end - p);
    char *val = malloc(len + 1);
    if (!val) return NULL;
    /* Simple unescape */
    size_t j = 0;
    for (size_t i = 0; i < len; i++) {
        if (p[i] == '\\' && i + 1 < len) {
            char next = p[i + 1];
            if (next == 'n') { val[j++] = '\n'; i++; }
            else if (next == 't') { val[j++] = '\t'; i++; }
            else if (next == '\\') { val[j++] = '\\'; i++; }
            else if (next == '"') { val[j++] = '"'; i++; }
            else { val[j++] = p[i]; }
        } else {
            val[j++] = p[i];
        }
    }
    val[j] = '\0';
    return val;
}

/* Write a JSON-escaped string to stdout. */
static void write_json_string(const char *s) {
    putchar('"');
    for (; *s; s++) {
        unsigned char c = (unsigned char)*s;
        if (c == '"') fputs("\\\"", stdout);
        else if (c == '\\') fputs("\\\\", stdout);
        else if (c == '\n') fputs("\\n", stdout);
        else if (c == '\r') fputs("\\r", stdout);
        else if (c == '\t') fputs("\\t", stdout);
        else if (c < 0x20) printf("\\u%04x", c);
        else putchar(c);
    }
    putchar('"');
}

/* Write a JSON-escaped string with length limit */
static void write_json_string_limited(const char *s, size_t max_len) {
    size_t len = strlen(s);
    if (len <= max_len) {
        write_json_string(s);
        return;
    }
    /* Head + ... + Tail */
    size_t head = max_len / 2;
    size_t tail = max_len / 2;
    char *buf = malloc(head + tail + 16);
    if (!buf) { write_json_string(""); return; }
    memcpy(buf, s, head);
    memcpy(buf + head, "...[truncated]...", 17);
    memcpy(buf + head + 17, s + len - tail, tail);
    size_t total = head + 17 + tail;
    /* Write raw to avoid double-escaping */
    putchar('"');
    for (size_t i = 0; i < total; i++) {
        unsigned char c = (unsigned char)buf[i];
        if (c == '"') fputs("\\\"", stdout);
        else if (c == '\\') fputs("\\\\", stdout);
        else if (c == '\n') fputs("\\n", stdout);
        else if (c == '\r') fputs("\\r", stdout);
        else if (c == '\t') fputs("\\t", stdout);
        else if (c < 0x20) printf("\\u%04x", c);
        else putchar(c);
    }
    putchar('"');
    free(buf);
}

static void handle_execute(const char *line, SessionStore *store, const char *default_cwd) {
    char *id = json_get_string(line, "id");
    char *command = json_get_string(line, "command");
    char *session_id = json_get_string(line, "session_id");

    if (!command) {
        printf("{\"type\":\"error\",\"id\":\"%s\",\"code\":\"INVALID_REQUEST\",\"message\":\"Missing required field 'command'\"}\n",
               id ? id : "");
        fflush(stdout);
        free(id); free(command); free(session_id);
        return;
    }

    /* Determine cwd from session or default */
    const char *cwd = default_cwd;
    Session *session = NULL;
    if (session_id && session_id[0]) {
        session = session_get_or_create(store, session_id, default_cwd);
        if (session) cwd = session->cwd;
    }

    /* Cleanup expired sessions periodically */
    session_cleanup_expired(store);

    /* Determine timeout */
    int timeout_ms = 30000;
    if (executor_is_long_running(command)) {
        timeout_ms = 300000;
    }

    /* Execute */
    ExecResult result;
    int rc = executor_run(command, cwd, timeout_ms, &result);

    /* Update session cwd */
    if (session && result.cwd[0]) {
        strncpy(session->cwd, result.cwd, sizeof(session->cwd) - 1);
    }

    /* Build response */
    printf("{\"type\":\"result\",\"id\":\"%s\",", id ? id : "");
    printf("\"stdout\":");
    if (rc == 0 && result.stdout_buf) {
        write_json_string_limited(result.stdout_buf, 8000);
    } else {
        write_json_string("");
    }
    printf(",\"stderr\":");
    if (rc == 0 && result.stderr_buf) {
        write_json_string_limited(result.stderr_buf, 2000);
    } else {
        write_json_string("");
    }
    printf(",\"exit_code\":%d,", rc == 0 ? result.exit_code : -1);
    printf("\"meta\":{");
    printf("\"mode_used\":\"%s\",", "full");
    printf("\"cwd\":");
    write_json_string(result.cwd[0] ? result.cwd : (cwd ? cwd : ""));
    printf(",\"truncated\":%s", result.truncated ? "true" : "false");
    printf(",\"truncation_offset\":null");
    printf(",\"hint\":null");
    printf(",\"blocked\":null");
    printf(",\"timed_out\":%s", result.timed_out ? "true" : "false");
    printf(",\"detected_patterns\":[]");
    printf("}}\n");
    fflush(stdout);

    if (rc == 0) executor_result_free(&result);
    free(id); free(command); free(session_id);
}

static void handle_session_info(const char *line, SessionStore *store, const char *default_cwd) {
    char *id = json_get_string(line, "id");
    char *session_id = json_get_string(line, "session_id");

    if (!session_id || !session_id[0]) {
        printf("{\"type\":\"error\",\"id\":\"%s\",\"code\":\"INVALID_REQUEST\",\"message\":\"Missing required field 'session_id'\"}\n",
               id ? id : "");
        fflush(stdout);
        free(id); free(session_id);
        return;
    }

    Session *s = session_get_or_create(store, session_id, default_cwd);

    printf("{\"type\":\"session_info_result\",\"id\":\"%s\",", id ? id : "");
    printf("\"session_id\":\"%s\",", session_id);
    printf("\"cwd\":");
    write_json_string(s ? s->cwd : default_cwd);
    printf(",\"env\":{}}\n");
    fflush(stdout);

    free(id); free(session_id);
}

void proto_handle_request(const char *line, SessionStore *store, const char *default_cwd) {
    if (strstr(line, "\"execute\"")) {
        handle_execute(line, store, default_cwd);
    } else if (strstr(line, "\"session_info\"")) {
        handle_session_info(line, store, default_cwd);
    } else {
        char *id = json_get_string(line, "id");
        printf("{\"type\":\"error\",\"id\":\"%s\",\"code\":\"UNKNOWN_TYPE\",\"message\":\"Unknown request type\"}\n",
               id ? id : "");
        fflush(stdout);
        free(id);
    }
}
