#include "protocol.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Minimal JSON helpers for our limited schema. */

char *json_get_string(const char *json, const char *key) {
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
        if (*end == '\\') end++;
        end++;
    }
    size_t len = (size_t)(end - p);
    char *val = malloc(len + 1);
    if (!val) return NULL;
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
void write_json_string(const char *s) {
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
void write_json_string_limited(const char *s, size_t max_len) {
    size_t len = strlen(s);
    if (len <= max_len) {
        write_json_string(s);
        return;
    }
    size_t head = max_len / 2;
    size_t tail = max_len / 2;
    char *buf = malloc(head + tail + 16);
    if (!buf) { write_json_string(""); return; }
    memcpy(buf, s, head);
    memcpy(buf + head, "...[truncated]...", 17);
    memcpy(buf + head + 17, s + len - tail, tail);
    size_t total = head + 17 + tail;
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

void proto_handle_session_info(const char *line, SessionStore *store, const char *default_cwd) {
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
