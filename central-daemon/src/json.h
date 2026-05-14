/*
 * Minimal zero-copy JSON parser for central-daemon.
 * All functions are static inline — no linking needed.
 *
 * Usage:
 *   struct json j;
 *   json_enter_object(&j, buf, len);
 *   char key[128];
 *   const char *val;
 *   while (json_next_key(&j, key, sizeof(key), &val) > 0) { ... }
 *
 * Quick extract:
 *   char buf[256];
 *   int n = json_obj_find_str(json_str, json_len, "method", buf, sizeof(buf));
 *   if (n > 0) { ... }
 */

#ifndef CENTRAL_JSON_H
#define CENTRAL_JSON_H

#include <stdbool.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

struct json {
    const char *cur;
    const char *end;
};

#define JSON_INIT(data, len) ((struct json){ .cur = (data), .end = (data) + (len) })

/* Skip whitespace and commas */
static inline const char *json_skip_ws(const char *p, const char *end) {
    while (p < end && (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r' || *p == ','))
        p++;
    return p;
}

/* Skip past a quoted string starting at *p (which must be '"').
 * Handles escape sequences. Returns pointer past closing quote, or end on error. */
static inline const char *json_skip_string(const char *p, const char *end) {
    p++; /* skip opening quote */
    while (p < end) {
        if (*p == '\\') { p += 2; continue; }
        if (*p == '"') return p + 1;
        p++;
    }
    return end;
}

/* Skip past a container (object or array). *p must be '{' or '['. */
static inline const char *json_skip_container(const char *p, const char *end) {
    char open = *p, close = (open == '{') ? '}' : ']';
    int depth = 1;
    p++;
    while (p < end && depth > 0) {
        if (*p == '"') { p = json_skip_string(p, end); continue; }
        if (*p == open) depth++;
        else if (*p == close) depth--;
        p++;
    }
    return p;
}

/* Get the next token. Returns token length (>0), 0 at end, -1 on error. */
static inline int json_next(struct json *j, const char **value) {
    j->cur = json_skip_ws(j->cur, j->end);
    if (j->cur >= j->end) return 0;

    const char *start = j->cur;
    char c = *start;

    if (c == '"') {
        const char *after = json_skip_string(start, j->end);
        *value = start;
        j->cur = after;
        return (int)(after - start);
    }

    if (c == '{' || c == '[') {
        const char *after = json_skip_container(start, j->end);
        *value = start;
        j->cur = after;
        return (int)(after - start);
    }

    if (c == '}' || c == ']') {
        return 0;
    }

    /* Bare word: true, false, null, or number */
    const char *p = start;
    while (p < j->end && *p != ',' && *p != '}' && *p != ']' &&
           *p != ' ' && *p != '\t' && *p != '\n' && *p != '\r') {
        p++;
    }
    *value = start;
    j->cur = p;
    return (int)(p - start);
}

/* Enter a container token. Returns 0 on success, -1 on error. */
static inline int json_enter(struct json *j, const char *container, int container_len) {
    if (container_len < 2) return -1;
    char open = container[0];
    if (open != '{' && open != '[') return -1;
    char close = (open == '{') ? '}' : ']';
    const char *clos = container + container_len - 1;
    while (clos > container && (*clos == ' ' || *clos == '\t' || *clos == '\n' || *clos == '\r'))
        clos--;
    if (*clos != close) return -1;
    j->cur = container + 1;
    j->end = clos;
    return 0;
}

/* Enter an object from a buffer. Returns 0 on success, -1 on error. */
static inline int json_enter_object(struct json *j, const char *data, int len) {
    const char *val;
    int vlen = json_next(&(struct json){ .cur = (data), .end = (data) + (len) }, &val);
    if (vlen <= 0 || val[0] != '{') return -1;
    return json_enter(j, val, vlen);
}

/* Read next key-value pair from an object.
 * Copies the key (unescaped, without quotes) into key_buf.
 * Sets *value to point at the value token in the input buffer.
 * Returns value length (>0), 0 at end, -1 on error. */
static inline int json_next_key(struct json *j, char *key_buf, int key_max, const char **value) {
    const char *key_tok;
    int klen = json_next(j, &key_tok);
    if (klen <= 0) return klen;

    if (key_tok[0] == '"' && klen >= 2) {
        const char *ks = key_tok + 1;
        const char *ke = key_tok + klen - 1;
        int to_copy = (int)(ke - ks);
        if (to_copy >= key_max) to_copy = key_max - 1;
        memcpy(key_buf, ks, (size_t)to_copy);
        key_buf[to_copy] = '\0';
    } else {
        int to_copy = klen < key_max ? klen : key_max - 1;
        memcpy(key_buf, key_tok, (size_t)to_copy);
        key_buf[to_copy] = '\0';
    }

    j->cur = json_skip_ws(j->cur, j->end);
    if (j->cur < j->end && *j->cur == ':') j->cur++;

    int vlen = json_next(j, value);
    return vlen;
}

/* ---- type checks ---- */

static inline bool json_is_null(const char *val, int len) {
    return len == 4 && memcmp(val, "null", 4) == 0;
}

static inline bool json_is_string(const char *val, int len) {
    return len >= 2 && val[0] == '"' && val[len - 1] == '"';
}

static inline bool json_is_container(const char *val, int len) {
    return len >= 2 && (val[0] == '{' || val[0] == '[');
}

static inline bool json_is_bool(const char *val, int len) {
    return (len == 4 && memcmp(val, "true", 4) == 0) ||
           (len == 5 && memcmp(val, "false", 5) == 0);
}

static inline bool json_is_int(const char *val, int len) {
    if (len <= 0) return false;
    int i = 0;
    if (val[0] == '-') i++;
    if (i >= len) return false;
    for (; i < len; i++) {
        if (val[i] < '0' || val[i] > '9') return false;
    }
    return true;
}

/* ---- typed extractors (copy-out) ---- */

/* Unescape a JSON string into buf. Returns written length, or -1 on overflow.
 * val should point to the opening quote, len includes both quotes. */
static inline int json_get_str(const char *val, int len, char *buf, int buf_max) {
    if (len < 2 || val[0] != '"' || val[len - 1] != '"') {
        if (buf_max > 0) buf[0] = '\0';
        return 0;
    }
    const char *src = val + 1;
    const char *end = val + len - 1;
    int j = 0;
    while (src < end && j < buf_max - 1) {
        if (*src == '\\' && src + 1 < end) {
            char next = src[1];
            if (next == 'n') { buf[j++] = '\n'; src += 2; }
            else if (next == 't') { buf[j++] = '\t'; src += 2; }
            else if (next == 'r') { buf[j++] = '\r'; src += 2; }
            else if (next == '\\') { buf[j++] = '\\'; src += 2; }
            else if (next == '"') { buf[j++] = '"'; src += 2; }
            else if (next == '/') { buf[j++] = '/'; src += 2; }
            else if (next == 'b') { buf[j++] = '\b'; src += 2; }
            else if (next == 'f') { buf[j++] = '\f'; src += 2; }
            else { buf[j++] = *src++; }
        } else {
            buf[j++] = *src++;
        }
    }
    buf[j] = '\0';
    return j;
}

/* Parse a boolean value. Returns 1 on success, 0 on failure. */
static inline int json_get_bool(const char *val, int len, bool *out) {
    if (len == 4 && memcmp(val, "true", 4) == 0) { *out = true; return 1; }
    if (len == 5 && memcmp(val, "false", 5) == 0) { *out = false; return 1; }
    return 0;
}

/* ---- quick-extract convenience ---- */

/* Find a key in a JSON object string, copy the string value into buf.
 * Returns the unescaped string length, or -1 if not found / not a string. */
static inline int json_obj_find_str(const char *json_str, int json_len,
                                    const char *key, char *buf, int buf_max) {
    struct json j;
    if (json_enter_object(&j, json_str, json_len) < 0) return -1;

    char kbuf[128];
    const char *val;
    int vlen;
    while ((vlen = json_next_key(&j, kbuf, (int)sizeof(kbuf), &val)) > 0) {
        if (strcmp(kbuf, key) == 0) {
            if (json_is_string(val, vlen))
                return json_get_str(val, vlen, buf, buf_max);
            return -1;
        }
    }
    return -1;
}

#endif /* CENTRAL_JSON_H */