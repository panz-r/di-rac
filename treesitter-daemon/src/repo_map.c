#include "analyzer.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>

/* Dynamic BFS queue for directory traversal — grows as needed */
typedef struct {
    char (*paths)[4096];
    int count;
    int capacity;
    int had_error; /* -1 = OOM occurred, results may be incomplete */
} WalkQueue;

static void queue_init(WalkQueue *q) {
    q->capacity = 256;
    q->paths = malloc(sizeof(char[4096]) * q->capacity);
    q->count = 0;
    q->had_error = (q->paths == NULL) ? -1 : 0;
}

static void queue_free(WalkQueue *q) {
    free(q->paths);
}

static int queue_push(WalkQueue *q, const char *path) {
    if (q->capacity == 0) { q->had_error = -1; return -1; }
    if (q->count == q->capacity) {
        int new_cap = q->capacity * 2;
        char (*tmp)[4096] = realloc(q->paths, sizeof(char[4096]) * new_cap);
        if (!tmp) { q->had_error = -1; return -1; }
        q->paths = tmp;
        q->capacity = new_cap;
    }
    if (snprintf(q->paths[q->count++], 4096, "%s", path) >= 4096) return -1;
    return 0;
}

static int queue_pop(WalkQueue *q, char *out) {
    if (q->capacity == 0) return -1; /* OOM state — queue unusable */
    if (q->count == 0) return -1;
    if (snprintf(out, 4096, "%s", q->paths[--q->count]) >= 4096) return -1;
    return 0;
}

static int should_ignore(const char *name) {
    if (name[0] == '.') return 1;
    if (strcmp(name, "node_modules") == 0) return 1;
    if (strcmp(name, "dist") == 0) return 1;
    if (strcmp(name, "build") == 0) return 1;
    if (strcmp(name, "target") == 0) return 1;
    return 0;
}

static Language get_lang_from_ext(const char *path) {
    const char *ext = strrchr(path, '.');
    if (!ext) return LANG_UNKNOWN;
    if (strcmp(ext, ".py") == 0) return LANG_PYTHON;
    if (strcmp(ext, ".ts") == 0) return LANG_TYPESCRIPT;
    if (strcmp(ext, ".js") == 0) return LANG_JAVASCRIPT;
    if (strcmp(ext, ".c") == 0) return LANG_C;
    if (strcmp(ext, ".cpp") == 0 || strcmp(ext, ".cc") == 0 || strcmp(ext, ".h") == 0) return LANG_CPP;
    if (strcmp(ext, ".rs") == 0) return LANG_RUST;
    if (strcmp(ext, ".go") == 0) return LANG_GO;
    if (strcmp(ext, ".java") == 0) return LANG_JAVA;
    if (strcmp(ext, ".cs") == 0) return LANG_CSHARP;
    if (strcmp(ext, ".rb") == 0) return LANG_RUBY;
    if (strcmp(ext, ".php") == 0) return LANG_PHP;
    return LANG_UNKNOWN;
}

void analyzer_repo_map(const char *root, struct jsonw *w) {
    WalkQueue queue;
    queue_init(&queue);
    if (queue.capacity == 0) {
        /* OOM at init — report and bail */
        jsonw_key(w, "files");
        jsonw_array_open(w);
        jsonw_array_close(w);
        jsonw_key(w, "partial");
        jsonw_bool(w, 1);
        jsonw_key(w, "error");
        jsonw_str(w, "out of memory during initialization");
        queue_free(&queue);
        return;
    }
    queue_push(&queue, root);  /* root is already NUL-terminated from strncpy */

    jsonw_key(w, "files");
    jsonw_array_open(w);

    char current_dir[4096];
    while (queue_pop(&queue, current_dir) == 0) {
        DIR *d = opendir(current_dir);
        if (!d) continue;

        struct dirent *de;
        while ((de = readdir(d)) != NULL) {
            if (should_ignore(de->d_name)) continue;

            char full_path[4096];
            int len = snprintf(full_path, sizeof(full_path), "%s/%s", current_dir, de->d_name);
            if (len < 0 || len >= (int)sizeof(full_path)) continue;  // truncation or error

            struct stat st;
            if (lstat(full_path, &st) < 0) continue;

            if (S_ISDIR(st.st_mode)) {
                if (queue_push(&queue, full_path) < 0) {
                    /* out of memory — skip this directory silently */
                }
            } else if (S_ISREG(st.st_mode)) {
                Language lang = get_lang_from_ext(full_path);
                if (lang != LANG_UNKNOWN) {
                    /* Read and parse */
                    FILE *f = fopen(full_path, "r");
                    if (f) {
                        fseek(f, 0, SEEK_END);
                        long size = ftell(f);
                        fseek(f, 0, SEEK_SET);
                        if (size < 0 || size >= 102400) {
                            fclose(f);
                            continue;
                        }
                        char *content = malloc(size + 1);
                        if (!content) {
                            fclose(f);
                            continue;
                        }
                        size_t bytes_read = fread(content, 1, size, f);
                        if (bytes_read != (size_t)size) {
                            free(content);
                            fclose(f);
                            continue;
                        }
                        content[bytes_read] = '\0';
                        ParsedSource *ps = analyzer_parse(content, lang);
                        if (ps) {
                            SymbolResult *sr = analyzer_extract_symbols(ps, NULL);
                            if (sr && sr->count > 0) {
                                jsonw_object_open(w);
                                jsonw_kv_str(w, "file", full_path + strlen(root) + (full_path[strlen(root)] == '/' ? 1 : 0));
                                jsonw_key(w, "symbols");
                                jsonw_array_open(w);
                                for (size_t i = 0; i < sr->count; i++) {
                                    jsonw_object_open(w);
                                    jsonw_kv_str(w, "name", sr->symbols[i].name);
                                    jsonw_kv_str(w, "kind", symbol_kind_to_str(sr->symbols[i].kind));
                                    jsonw_object_close(w);
                                }
                                jsonw_array_close(w);
                                jsonw_object_close(w);
                            }
                            analyzer_free_symbols(sr);
                            analyzer_free_source(ps);
                        }
                        free(content);
                        fclose(f);
                    }
                }
            }
        }
        closedir(d);
    }
    jsonw_array_close(w);
    if (queue.had_error) {
        jsonw_key(w, "partial");
        jsonw_bool(w, 1);
    }
    queue_free(&queue);
}