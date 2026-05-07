#ifndef JSON_H
#define JSON_H

/*
 * Minimal zero-copy JSON parser for the command daemon.
 * All functions are static inline — no linking needed.
 *
 * Usage:
 *   struct json j = JSON_INIT(buf, len);
 *   const char *val; int vlen;
 *   while ((vlen = json_next(&j, &val)) > 0) { ... }
 *
 * For objects:
 *   struct json j;
 *   json_enter_object(&j, buf, len);
 *   char key[128];
 *   while ((vlen = json_next_key(&j, key, sizeof(key), &val)) > 0) { ... }
 */

#include <stdbool.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

struct json {
    const char *cur;
    const char *end;
};

#define JSON_INIT(data, len) ((struct json){ .cur = (data), .end = (data) + (len) })

/* ---- tokenizer ---- */

/* Skip whitespace and commas (commas are structural noise in objects/arrays). */
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

/* Skip past a container (object or array). *p must be '{' or '['.
 * Tracks nesting depth. Returns pointer past closing delimiter. */
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

/* Get the next token. Sets *value to point into the input buffer.
 * Returns token length (>0), 0 at end, -1 on error.
 * For containers, *value points to the opening brace/bracket and
 * the returned length covers the entire container. */
static inline int json_next(struct json *j, const char **value) {
    j->cur = json_skip_ws(j->cur, j->end);
    if (j->cur >= j->end) return 0;

    const char *start = j->cur;
    char c = *start;

    if (c == '"') {
        /* Quoted string: value includes the quotes */
        const char *after = json_skip_string(start, j->end);
        *value = start;
        j->cur = after;
        return (int)(after - start);
    }

    if (c == '{' || c == '[') {
        /* Container: value spans from opening to closing delimiter */
        const char *after = json_skip_container(start, j->end);
        *value = start;
        j->cur = after;
        return (int)(after - start);
    }

    if (c == '}' || c == ']') {
        /* End of container */
        return 0;
    }

    /* Bare word: true, false, null, or number.
     * Read until delimiter. */
    const char *p = start;
    while (p < j->end && *p != ',' && *p != '}' && *p != ']' &&
           *p != ' ' && *p != '\t' && *p != '\n' && *p != '\r') {
        p++;
    }
    *value = start;
    j->cur = p;
    return (int)(p - start);
}

/* ---- container entry ---- */

/* Enter a container token. The token must start with '{' or '['.
 * Resets the iterator to walk inside the container. Returns 0 on success, -1 on error. */
static inline int json_enter(struct json *j, const char *container, int container_len) {
    if (container_len < 2) return -1;
    char open = container[0];
    if (open != '{' && open != '[') return -1;
    char close = (open == '{') ? '}' : ']';
    /* Find closing delimiter (should be the last char) */
    const char *clos = container + container_len - 1;
    while (clos > container && (*clos == ' ' || *clos == '\t' || *clos == '\n' || *clos == '\r'))
        clos--;
    if (*clos != close) return -1;
    j->cur = container + 1; /* past opening delimiter */
    j->end = clos;          /* at closing delimiter */
    return 0;
}

/* Convenience: enter an object from a buffer. */
static inline int json_enter_object(struct json *j, const char *data, int len) {
    const char *val;
    int vlen = json_next(&(struct json){ .cur = data, .end = data + len }, &val);
    if (vlen <= 0 || val[0] != '{') return -1;
    return json_enter(j, val, vlen);
}

/* ---- object iteration ---- */

/* Read next key-value pair from an object.
 * Copies the key (unescaped, without quotes) into key_buf.
 * Sets *value to point at the value token in the input buffer.
 * Returns value length (>0), 0 at end, -1 on error. */
static inline int json_next_key(struct json *j, char *key_buf, int key_max, const char **value) {
    /* Read key token */
    const char *key_tok;
    int klen = json_next(j, &key_tok);
    if (klen <= 0) return klen; /* 0 = end, -1 = error */

    /* Unescape key (it's a quoted string like "type") */
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

    /* Skip the ':' separator between key and value */
    j->cur = json_skip_ws(j->cur, j->end);
    if (j->cur < j->end && *j->cur == ':') j->cur++;

    /* Read value token */
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
        /* Not a string — return empty */
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

/* Parse an integer value. Returns 1 on success, 0 on failure. */
static inline int json_get_int(const char *val, int len, int *out) {
    if (!json_is_int(val, len)) return 0;
    char tmp[32];
    int copy = len < 31 ? len : 31;
    memcpy(tmp, val, (size_t)copy);
    tmp[copy] = '\0';
    *out = (int)strtol(tmp, NULL, 10);
    return 1;
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
            return -1; /* key found but not a string */
        }
    }
    return -1; /* key not found */
}

/* Copy a JSON token as a raw string into buf. 
 * If it's a quoted string, it unescapes it. 
 * If it's a bare word (number, bool), it just copies the bytes. */
static inline int json_get_raw_str(const char *val, int len, char *buf, int buf_max) {
    if (len <= 0) { if (buf_max > 0) buf[0] = '\0'; return 0; }
    if (val[0] == '"') return json_get_str(val, len, buf, buf_max);
    int to_copy = len < buf_max ? len : buf_max - 1;
    memcpy(buf, val, (size_t)to_copy);
    buf[to_copy] = '\0';
    return to_copy;
}

#endif /* JSON_H */
