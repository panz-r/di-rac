#ifndef JSON_WRITE_H
#define JSON_WRITE_H

/*
 * Minimal JSON builder for the command daemon.
 * Writes to a FILE*. All functions are static inline.
 *
 * Usage:
 *   struct jsonw w;
 *   jsonw_init(&w, stdout);
 *   jsonw_object_open(&w);
 *     jsonw_key(&w, "type"); jsonw_str(&w, "result");
 *     jsonw_key(&w, "exit_code"); jsonw_int(&w, 0);
 *     jsonw_key(&w, "meta");
 *     jsonw_object_open(&w);
 *       jsonw_key(&w, "truncated"); jsonw_bool(&w, false);
 *     jsonw_object_close(&w);
 *   jsonw_object_close(&w);
 *   jsonw_flush(&w);
 */

#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <inttypes.h>

struct jsonw {
    FILE *f;
    bool need_comma;   /* need a comma before the next item */
};

/* Thread-local override: when non-NULL, jsonw_init uses this FILE* instead
   of the passed argument. Workers set this to an open_memstream to capture
   output for the main loop's poll-based flush to the real stdout. */
extern __thread FILE *jsonw_output_fallback;

static inline void jsonw_init(struct jsonw *w, FILE *f) {
    w->f = jsonw_output_fallback ? jsonw_output_fallback : f;
    w->need_comma = false;
}

/* Write comma separator if needed between items in an object/array. */
static inline void jsonw_comma(struct jsonw *w) {
    if (w->need_comma) fputc(',', w->f);
}

/* Write a JSON-escaped string (without surrounding quotes) to the FILE.
 * If max_len > 0 and len exceeds max_len, writes head + "...[truncated]..." + tail. */
static inline void jsonw_write_escaped(struct jsonw *w, const char *s, int len, int max_len) {
    if (max_len > 0 && len > max_len) {
        /* Truncation: head + marker + tail */
        int head = max_len / 2;
        int tail = max_len / 2;
        const char *marker = "...[truncated]...";

        /* Write head */
        for (int i = 0; i < head; i++) {
            unsigned char c = (unsigned char)s[i];
            if (c == '"') fputs("\\\"", w->f);
            else if (c == '\\') fputs("\\\\", w->f);
            else if (c == '\n') fputs("\\n", w->f);
            else if (c == '\r') fputs("\\r", w->f);
            else if (c == '\t') fputs("\\t", w->f);
            else if (c < 0x20) fprintf(w->f, "\\u%04x", c);
            else fputc(c, w->f);
        }
        /* Write marker as-is (no escaping needed — it's plain ASCII) */
        fputs(marker, w->f);
        /* Write tail */
        for (int i = len - tail; i < len; i++) {
            unsigned char c = (unsigned char)s[i];
            if (c == '"') fputs("\\\"", w->f);
            else if (c == '\\') fputs("\\\\", w->f);
            else if (c == '\n') fputs("\\n", w->f);
            else if (c == '\r') fputs("\\r", w->f);
            else if (c == '\t') fputs("\\t", w->f);
            else if (c < 0x20) fprintf(w->f, "\\u%04x", c);
            else fputc(c, w->f);
        }
    } else {
        /* No truncation */
        for (int i = 0; i < len; i++) {
            unsigned char c = (unsigned char)s[i];
            if (c == '"') fputs("\\\"", w->f);
            else if (c == '\\') fputs("\\\\", w->f);
            else if (c == '\n') fputs("\\n", w->f);
            else if (c == '\r') fputs("\\r", w->f);
            else if (c == '\t') fputs("\\t", w->f);
            else if (c < 0x20) fprintf(w->f, "\\u%04x", c);
            else fputc(c, w->f);
        }
    }
}

/* ---- containers ---- */

static inline void jsonw_object_open(struct jsonw *w) {
    jsonw_comma(w);
    fputc('{', w->f);
    w->need_comma = false;
}

static inline void jsonw_object_close(struct jsonw *w) {
    fputc('}', w->f);
    w->need_comma = true;
}

static inline void jsonw_array_open(struct jsonw *w) {
    jsonw_comma(w);
    fputc('[', w->f);
    w->need_comma = false;
}

static inline void jsonw_array_close(struct jsonw *w) {
    fputc(']', w->f);
    w->need_comma = true;
}

/* ---- key ---- */

static inline void jsonw_key(struct jsonw *w, const char *key) {
    jsonw_comma(w);
    fputc('"', w->f);
    fputs(key, w->f); /* keys are assumed to be safe ASCII identifiers */
    fputs("\":", w->f);
    w->need_comma = false;
}

/* ---- value writers ---- */

static inline void jsonw_str(struct jsonw *w, const char *val) {
    jsonw_comma(w);
    fputc('"', w->f);
    jsonw_write_escaped(w, val, (int)strlen(val), 0);
    fputc('"', w->f);
    w->need_comma = true;
}

/* String with length and optional truncation limit.
 * max_len > 0: truncate with head+tail+marker if len > max_len.
 * max_len == 0: no truncation. */
static inline void jsonw_strn(struct jsonw *w, const char *val, int len, int max_len) {
    jsonw_comma(w);
    fputc('"', w->f);
    jsonw_write_escaped(w, val, len, max_len);
    fputc('"', w->f);
    w->need_comma = true;
}

static inline void jsonw_int(struct jsonw *w, int64_t val) {
    jsonw_comma(w);
    fprintf(w->f, "%" PRId64, val);
    w->need_comma = true;
}

static inline void jsonw_bool(struct jsonw *w, bool val) {
    jsonw_comma(w);
    fputs(val ? "true" : "false", w->f);
    w->need_comma = true;
}

static inline void jsonw_double(struct jsonw *w, double val) {
    jsonw_comma(w);
    fprintf(w->f, "%f", val);
    w->need_comma = true;
}

static inline void jsonw_null(struct jsonw *w) {
    jsonw_comma(w);
    fputs("null", w->f);
    w->need_comma = true;
}

/* ---- convenience: key + value in one call ---- */

static inline void jsonw_kv_str(struct jsonw *w, const char *key, const char *val) {
    jsonw_key(w, key);
    if (val) {
        jsonw_str(w, val);
    } else {
        jsonw_null(w);
    }
}

static inline void jsonw_kv_strn(struct jsonw *w, const char *key,
                                  const char *val, int len, int max_len) {
    jsonw_key(w, key);
    jsonw_strn(w, val, len, max_len);
}

static inline void jsonw_kv_int(struct jsonw *w, const char *key, int64_t val) {
    jsonw_key(w, key);
    jsonw_int(w, val);
}

static inline void jsonw_kv_bool(struct jsonw *w, const char *key, bool val) {
    jsonw_key(w, key);
    jsonw_bool(w, val);
}

static inline void jsonw_kv_double(struct jsonw *w, const char *key, double val) {
    jsonw_key(w, key);
    jsonw_double(w, val);
}

/* Write key + value, but if val is NULL write key + null instead. */
static inline void jsonw_kv_str_or_null(struct jsonw *w, const char *key, const char *val) {
    jsonw_key(w, key);
    if (val) jsonw_str(w, val);
    else jsonw_null(w);
}

/* ---- flush ---- */

static inline void jsonw_flush(struct jsonw *w) {
    fputc('\n', w->f);
    fflush(w->f);
}

#endif /* JSON_WRITE_H */
