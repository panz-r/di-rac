#include "analyzer.h"
#include "db.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>
#include <ctype.h>

static char* strcasestr_portable(const char *haystack, const char *needle) {
    if (!*needle) return (char *)haystack;
    for (; *haystack; haystack++) {
        if (tolower(*haystack) == tolower(*needle)) {
            const char *h, *n;
            for (h = haystack, n = needle; *h && *n; h++, n++) {
                if (tolower(*h) != tolower(*n)) break;
            }
            if (!*n) return (char *)haystack;
        }
    }
    return NULL;
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

typedef struct {
    char path[4096];
} WalkStack;

void analyzer_search_symbols(AnalyzerCtx *ctx, const char *query, const char *kind_filter, int limit, struct jsonw *w) {
    if (ctx->db) {
        db_search_symbols((IndexDB*)ctx->db, query, kind_filter, limit, w);
        return;
    }

    /* Fallback to live walk if no DB */
    WalkStack *stack = malloc(sizeof(WalkStack) * 1024);
    int stack_ptr = 0;
    strncpy(stack[stack_ptr++].path, ctx->workspace_root, 4095);

    int results_found = 0;
    jsonw_key(w, "results");
    jsonw_array_open(w);

    while (stack_ptr > 0 && results_found < limit) {
        char current_dir[4096];
        strncpy(current_dir, stack[--stack_ptr].path, 4095);

        DIR *d = opendir(current_dir);
        if (!d) continue;

        struct dirent *de;
        while ((de = readdir(d)) != NULL && results_found < limit) {
            if (de->d_name[0] == '.') continue;
            if (strcmp(de->d_name, "node_modules") == 0) continue;

            char full_path[4096];
            snprintf(full_path, sizeof(full_path), "%s/%s", current_dir, de->d_name);

            struct stat st;
            if (lstat(full_path, &st) < 0) continue;

            if (S_ISDIR(st.st_mode)) {
                if (stack_ptr < 1024) strncpy(stack[stack_ptr++].path, full_path, 4095);
            } else if (S_ISREG(st.st_mode)) {
                Language lang = get_lang_from_ext(full_path);
                if (lang != LANG_UNKNOWN) {
                    FILE *f = fopen(full_path, "r");
                    if (f) {
                        fseek(f, 0, SEEK_END);
                        long size = ftell(f);
                        fseek(f, 0, SEEK_SET);
                        if (size < 512000) {
                            char *content = malloc(size + 1);
                            fread(content, 1, size, f);
                            content[size] = '\0';
                            
                            ParsedSource *ps = analyzer_parse(content, lang);
                            if (ps) {
                                SymbolResult *sr = analyzer_extract_symbols(ps, ctx);
                                if (sr) {
                                    for (size_t i = 0; i < sr->count && results_found < limit; i++) {
                                        if (strcasestr_portable(sr->symbols[i].name, query)) {
                                            if (kind_filter && strcmp(symbol_kind_to_str(sr->symbols[i].kind), kind_filter) != 0) continue;

                                            jsonw_object_open(w);
                                            jsonw_kv_str(w, "name", sr->symbols[i].name);
                                            jsonw_kv_str(w, "kind", symbol_kind_to_str(sr->symbols[i].kind));
                                            jsonw_kv_str(w, "handle", sr->symbols[i].handle);
                                            jsonw_kv_str(w, "file", full_path);
                                            jsonw_kv_int(w, "start_line", sr->symbols[i].start_line);
                                            jsonw_object_close(w);
                                            results_found++;
                                        }
                                    }
                                    analyzer_free_symbols(sr);
                                }
                                analyzer_free_source(ps);
                            }
                            free(content);
                        }
                        fclose(f);
                    }
                }
            }
        }
        closedir(d);
    }
    jsonw_array_close(w);
    free(stack);
}
