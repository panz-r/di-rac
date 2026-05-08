#include "analyzer.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>

typedef struct {
    char path[4096];
} WalkStack;

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
    WalkStack *stack = malloc(sizeof(WalkStack) * 1024);
    int stack_ptr = 0;
    strncpy(stack[stack_ptr++].path, root, 4095);

    jsonw_key(w, "files");
    jsonw_array_open(w);

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
                if (stack_ptr < 1024) strncpy(stack[stack_ptr++].path, full_path, 4095);
            } else if (S_ISREG(st.st_mode)) {
                Language lang = get_lang_from_ext(full_path);
                if (lang != LANG_UNKNOWN) {
                    /* Read and parse */
                    FILE *f = fopen(full_path, "r");
                    if (f) {
                        fseek(f, 0, SEEK_END);
                        long size = ftell(f);
                        fseek(f, 0, SEEK_SET);
                        if (size < 102400) { /* 100KB limit for repo map parsing */
                            char *content = malloc(size + 1);
                            size_t bytes_read = fread(content, 1, size, f);
                            if (bytes_read != (size_t)size) {
                                free(content);
                                fclose(f);
                                continue;  /* skip file on incomplete read */
                            }
                            content[size] = '\0';
                            
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
