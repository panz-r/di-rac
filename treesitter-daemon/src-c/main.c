#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <stdbool.h>
#include <pthread.h>
#include <tree_sitter/api.h>

#include "analyzer.h"
#include "db.h"
#include "json.h"
#include "json-write.h"

#define MAX_LINE 1048576 
#define MAX_THREADS 8

typedef struct {
    pthread_mutex_t stdout_lock;
    pthread_mutex_t thread_count_lock;
    pthread_mutex_t db_lock;
    int active_threads;
    AnalyzerCtx base;
} GlobalCtx;

typedef struct {
    char *line;
    GlobalCtx *gctx;
} RequestTask;

static void jsonw_id(struct jsonw *w, const char *raw_id, int id_len) {
    jsonw_key(w, "id");
    if (!raw_id || id_len <= 0) {
        jsonw_null(w);
    } else {
        fwrite(raw_id, 1, id_len, w->f);
        w->need_comma = true;
    }
}

static void send_error(pthread_mutex_t *lock, const char *raw_id, int id_len, const char *code, const char *message) {
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "error");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", false);
    jsonw_kv_str(&w, "code", code);
    jsonw_kv_str(&w, "message", message);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_status(pthread_mutex_t *lock, const char *raw_id, int id_len, AnalyzerCtx *ctx) {
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "status_result");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    jsonw_kv_str(&w, "workspace_root", ctx->workspace_root);
    jsonw_kv_bool(&w, "db_open", ctx->db != NULL);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void write_outline_payload(struct jsonw *w, SymbolResult *sr, ImportResult *ir, bool compact) {
    jsonw_key(w, "symbols");
    jsonw_array_open(w);
    if (sr) {
        for (size_t i = 0; i < sr->count; i++) {
            jsonw_object_open(w);
            if (compact) {
                jsonw_kv_str(w, "n", sr->symbols[i].name);
                jsonw_kv_str(w, "t", "d");
                jsonw_kv_str(w, "k", symbol_kind_to_str(sr->symbols[i].kind));
                jsonw_kv_int(w, "start_line", sr->symbols[i].start_line);
                jsonw_kv_int(w, "start_col", 1);
                jsonw_kv_int(w, "end_line", sr->symbols[i].end_line);
                jsonw_kv_int(w, "end_col", 1);
            } else {
                jsonw_kv_str(w, "name", sr->symbols[i].name);
                jsonw_kv_str(w, "kind", symbol_kind_to_str(sr->symbols[i].kind));
                jsonw_kv_str(w, "handle", sr->symbols[i].handle);
                jsonw_kv_int(w, "start_line", sr->symbols[i].start_line);
                jsonw_kv_int(w, "end_line", sr->symbols[i].end_line);
                jsonw_kv_str(w, "signature", sr->symbols[i].signature);
            }
            jsonw_object_close(w);
        }
    }
    jsonw_array_close(w);

    jsonw_key(w, "imports");
    jsonw_array_open(w);
    if (ir) {
        for (size_t i = 0; i < ir->count; i++) {
            jsonw_object_open(w);
            jsonw_kv_str(w, "module", ir->imports[i].module);
            jsonw_kv_int(w, "line", ir->imports[i].line);
            jsonw_key(w, "names");
            jsonw_array_open(w);
            for (size_t j = 0; j < ir->imports[i].names_count; j++) {
                jsonw_str(w, ir->imports[i].names[j]);
            }
            jsonw_array_close(w);
            jsonw_object_close(w);
        }
    }
    jsonw_array_close(w);
}

static void handle_outline(pthread_mutex_t *lock, const char *raw_id, int id_len, const char *content, const char *lang_name, AnalyzerCtx *ctx) {
    Language lang = parse_language_name(lang_name);
    ParsedSource *ps = analyzer_parse(content, lang);
    if (!ps) {
        send_error(lock, raw_id, id_len, "PARSE_FAILED", "Failed to parse source code");
        return;
    }

    SymbolResult *sr = analyzer_extract_symbols(ps, ctx);
    ImportResult *ir = analyzer_extract_imports(ps, ctx);

    if (!sr && !ir) {
        send_error(lock, raw_id, id_len, "EXTRACTION_FAILED", "Symbol extraction returned NULL");
        analyzer_free_source(ps);
        return;
    }

    if (sr && sr->error_code != 0) {
        fprintf(stderr, "[warn] symbol extraction OOM at content — %zu symbols collected\n", sr->count);
    }
    if (ir && ir->error_code != 0) {
        fprintf(stderr, "[warn] import extraction OOM at content — %zu imports collected\n", ir->count);
    }

    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "outline_result");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    if (sr && sr->error_code != 0) jsonw_kv_bool(&w, "truncated", true);
    write_outline_payload(&w, sr, ir, false);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);

    analyzer_free_symbols(sr);
    analyzer_free_imports(ir);
    analyzer_free_source(ps);
}

static void handle_extract_apis(pthread_mutex_t *lock, const char *raw_id, int id_len, const char *content, const char *lang_name) {
    Language lang = parse_language_name(lang_name);
    ParsedSource *ps = analyzer_parse(content, lang);
    if (!ps) {
        send_error(lock, raw_id, id_len, "PARSE_FAILED", "Failed to parse source code");
        return;
    }

    ApiDependencies *ad = analyzer_extract_apis(ps);
    
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "extract_apis_result");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    jsonw_key(&w, "calls");
    jsonw_array_open(&w);
    if (ad) for (size_t i = 0; i < ad->calls_count; i++) jsonw_str(&w, ad->calls[i]);
    jsonw_array_close(&w);
    jsonw_key(&w, "definitions");
    jsonw_array_open(&w);
    if (ad) for (size_t i = 0; i < ad->definitions_count; i++) jsonw_str(&w, ad->definitions[i]);
    jsonw_array_close(&w);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);

    analyzer_free_apis(ad);
    analyzer_free_source(ps);
}

static void handle_skeleton(pthread_mutex_t *lock, const char *raw_id, int id_len, const char *content, const char *lang_name) {
    Language lang = parse_language_name(lang_name);
    ParsedSource *ps = analyzer_parse(content, lang);
    if (!ps) {
        send_error(lock, raw_id, id_len, "PARSE_FAILED", "Failed to parse source code");
        return;
    }

    char *skel = analyzer_generate_skeleton(ps);
    
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "skeleton_result");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    jsonw_kv_str(&w, "skeleton", skel);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);

    free(skel);
    analyzer_free_source(ps);
}

static void handle_repo_map(pthread_mutex_t *lock, const char *raw_id, int id_len, const char *dir, AnalyzerCtx *ctx) {
    const char *root = (dir && dir[0]) ? dir : ctx->workspace_root;
    
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "repo_map_result");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    analyzer_repo_map(root, &w);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_search_symbols(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *query, const char *kind, int limit, AnalyzerCtx *ctx) {
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "search_symbols_result");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    
    if (ctx->db) pthread_mutex_lock(db_lock);
    analyzer_search_symbols(ctx, query, kind, limit, &w);
    if (ctx->db) pthread_mutex_unlock(db_lock);

    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_index_observation(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *type, const char *content, double timestamp, int tokens, AnalyzerCtx *ctx) {
    if (!ctx->db) { send_error(lock, raw_id, id_len, "DB_NOT_OPEN", "Index database is not open"); return; }
    
    pthread_mutex_lock(db_lock);
    db_index_observation((IndexDB*)ctx->db, type, content, timestamp, tokens);
    pthread_mutex_unlock(db_lock);

    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ok");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_search_observations(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *query, int limit, AnalyzerCtx *ctx) {
    if (!ctx->db) { send_error(lock, raw_id, id_len, "DB_NOT_OPEN", "Index database is not open"); return; }

    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "search_observations_result");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    
    pthread_mutex_lock(db_lock);
    db_search_observations((IndexDB*)ctx->db, query, limit, &w);
    pthread_mutex_unlock(db_lock);

    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_index_critic_decision(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *text, int turn, double confidence, AnalyzerCtx *ctx) {
    if (!ctx->db) { send_error(lock, raw_id, id_len, "DB_NOT_OPEN", "Index database is not open"); return; }
    pthread_mutex_lock(db_lock);
    db_index_critic_decision((IndexDB*)ctx->db, text, turn, confidence);
    pthread_mutex_unlock(db_lock);
    pthread_mutex_lock(lock);
    struct jsonw w; jsonw_init(&w, stdout); jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ok"); jsonw_id(&w, raw_id, id_len); jsonw_kv_bool(&w, "ok", true);
    jsonw_object_close(&w); jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_search_critic_decisions(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *query, int limit, AnalyzerCtx *ctx) {
    if (!ctx->db) { send_error(lock, raw_id, id_len, "DB_NOT_OPEN", "Index database is not open"); return; }
    pthread_mutex_lock(lock);
    struct jsonw w; jsonw_init(&w, stdout); jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "search_critic_decisions_result"); jsonw_id(&w, raw_id, id_len); jsonw_kv_bool(&w, "ok", true);
    pthread_mutex_lock(db_lock);
    db_search_critic_decisions((IndexDB*)ctx->db, query, limit, &w);
    pthread_mutex_unlock(db_lock);
    jsonw_object_close(&w); jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_index_watcher_pattern(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *text, const char *file_hash, int turn, AnalyzerCtx *ctx) {
    if (!ctx->db) { send_error(lock, raw_id, id_len, "DB_NOT_OPEN", "Index database is not open"); return; }
    pthread_mutex_lock(db_lock);
    db_index_watcher_pattern((IndexDB*)ctx->db, text, file_hash, turn);
    pthread_mutex_unlock(db_lock);
    pthread_mutex_lock(lock);
    struct jsonw w; jsonw_init(&w, stdout); jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ok"); jsonw_id(&w, raw_id, id_len); jsonw_kv_bool(&w, "ok", true);
    jsonw_object_close(&w); jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_search_watcher_patterns(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *query, int limit, AnalyzerCtx *ctx) {
    if (!ctx->db) { send_error(lock, raw_id, id_len, "DB_NOT_OPEN", "Index database is not open"); return; }
    pthread_mutex_lock(lock);
    struct jsonw w; jsonw_init(&w, stdout); jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "search_watcher_patterns_result"); jsonw_id(&w, raw_id, id_len); jsonw_kv_bool(&w, "ok", true);
    pthread_mutex_lock(db_lock);
    db_search_watcher_patterns((IndexDB*)ctx->db, query, limit, &w);
    pthread_mutex_unlock(db_lock);
    jsonw_object_close(&w); jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_ast_churn(pthread_mutex_t *lock, const char *raw_id, int id_len, const char *file, const char *content) {
    pthread_mutex_lock(lock);
    struct jsonw w; jsonw_init(&w, stdout); jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ast_churn_result"); jsonw_id(&w, raw_id, id_len); jsonw_kv_bool(&w, "ok", true);
    analyzer_ast_churn(file, content, &w);
    jsonw_object_close(&w); jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_index_file(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *file, AnalyzerCtx *ctx) {
    if (!ctx->db) {
        send_error(lock, raw_id, id_len, "DB_NOT_OPEN", "Index database is not open");
        return;
    }

    FILE *f = fopen(file, "r");
    if (!f) {
        send_error(lock, raw_id, id_len, "FILE_NOT_FOUND", "Failed to open file for indexing");
        return;
    }

    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *content = malloc(size + 1);
    if (!content) { fclose(f); return; }
    if (fread(content, 1, size, f) != (size_t)size) { free(content); fclose(f); send_error(lock, raw_id, id_len, "READ_ERROR", "Incomplete read"); return; }
    content[size] = '\0';
    fclose(f);

    Language lang = LANG_UNKNOWN;
    const char *ext = strrchr(file, '.');
    if (ext) lang = parse_language_name(ext + 1);

    ParsedSource *ps = analyzer_parse(content, lang);
    if (!ps) {
        free(content);
        send_error(lock, raw_id, id_len, "PARSE_FAILED", "Failed to parse file for indexing");
        return;
    }

    SymbolResult *sr = analyzer_extract_symbols(ps, ctx);
    ImportResult *ir = analyzer_extract_imports(ps, ctx);

    if (!sr && !ir) {
        send_error(lock, raw_id, id_len, "EXTRACTION_FAILED", "Symbol extraction returned NULL");
        analyzer_free_source(ps);
        free(content);
        return;
    }

    if (sr && sr->error_code != 0) {
        fprintf(stderr, "[warn] symbol extraction OOM at file %s — %zu symbols collected\n", file, sr->count);
    }
    if (ir && ir->error_code != 0) {
        fprintf(stderr, "[warn] import extraction OOM at file %s — %zu imports collected\n", file, ir->count);
    }

    pthread_mutex_lock(db_lock);
    db_index_file((IndexDB*)ctx->db, file, 0.0, "TODO_HASH", sr, ir);
    pthread_mutex_unlock(db_lock);

    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "index_file_result");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    if (sr && sr->error_code != 0) jsonw_kv_bool(&w, "truncated", true);
    jsonw_key(&w, "data");
    jsonw_object_open(&w);
    write_outline_payload(&w, sr, ir, true);
    jsonw_object_close(&w);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);

    analyzer_free_symbols(sr);
    analyzer_free_imports(ir);
    analyzer_free_source(ps);
    free(content);
}

static void handle_invalidate_file(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *file, AnalyzerCtx *ctx) {
    if (ctx->db) {
        pthread_mutex_lock(db_lock);
        db_invalidate_file((IndexDB*)ctx->db, file);
        pthread_mutex_unlock(db_lock);
    }
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ok");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_clear_index(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, AnalyzerCtx *ctx) {
    if (ctx->db) {
        pthread_mutex_lock(db_lock);
        db_clear((IndexDB*)ctx->db);
        pthread_mutex_unlock(db_lock);
    }
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ok");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void* request_worker(void *arg) {
    RequestTask *task = (RequestTask*)arg;
    int len = strlen(task->line);
    
    struct json j;
    if (json_enter_object(&j, task->line, len) < 0) {
        send_error(&task->gctx->stdout_lock, NULL, 0, "INVALID_REQUEST", "Malformed JSON");
        goto cleanup;
    }

    const char *raw_id = NULL;
    int id_len = 0;
    char command[64] = "", file[4096] = "", content[MAX_LINE] = "", lang[32] = "", dir[4096] = "", query[256] = "", kind[64] = "", obs_type[32] = "", file_hash[128] = "";
    int limit = 100, tokens = 0, turn = 0;
    double timestamp = 0.0, confidence = 0.0;
    char key[64];
    const char *val;
    int vlen;

    while ((vlen = json_next_key(&j, key, sizeof(key), &val)) > 0) {
        if (strcmp(key, "id") == 0) { raw_id = val; id_len = vlen; }
        else if (strcmp(key, "command") == 0) json_get_str(val, vlen, command, sizeof(command));
        else if (strcmp(key, "file") == 0) json_get_str(val, vlen, file, sizeof(file));
        else if (strcmp(key, "content") == 0) json_get_str(val, vlen, content, sizeof(content));
        else if (strcmp(key, "language") == 0) json_get_str(val, vlen, lang, sizeof(lang));
        else if (strcmp(key, "dir") == 0 || strcmp(key, "root") == 0) json_get_str(val, vlen, dir, sizeof(dir));
        else if (strcmp(key, "query") == 0) json_get_str(val, vlen, query, sizeof(query));
        else if (strcmp(key, "kind") == 0) json_get_str(val, vlen, kind, sizeof(kind));
        else if (strcmp(key, "type") == 0) json_get_str(val, vlen, obs_type, sizeof(obs_type));
        else if (strcmp(key, "timestamp") == 0) json_get_double(val, vlen, &timestamp);
        else if (strcmp(key, "tokens") == 0) json_get_int(val, vlen, &tokens);
        else if (strcmp(key, "turn") == 0) json_get_int(val, vlen, &turn);
        else if (strcmp(key, "confidence") == 0) json_get_double(val, vlen, &confidence);
        else if (strcmp(key, "file_hash") == 0) json_get_str(val, vlen, file_hash, sizeof(file_hash));
        else if (strcmp(key, "limit") == 0 || strcmp(key, "max_results") == 0) json_get_int(val, vlen, &limit);
    }

    if (strcmp(command, "status") == 0) {
        handle_status(&task->gctx->stdout_lock, raw_id, id_len, &task->gctx->base);
    } else if (strcmp(command, "outline") == 0) {
        if (content[0] == '\0' && file[0] != '\0') {
            FILE *f = fopen(file, "r");
            if (f) {
                fseek(f, 0, SEEK_END);
                long fsize = ftell(f);
                fseek(f, 0, SEEK_SET);
                char *fcontent = malloc(fsize + 1);
                if (fcontent) {
                    size_t bytes_read = fread(fcontent, 1, fsize, f);
                    fcontent[bytes_read] = '\0';
                    handle_outline(&task->gctx->stdout_lock, raw_id, id_len, fcontent, lang, &task->gctx->base);
                    free(fcontent);
                }
                fclose(f);
            }
        } else {
            handle_outline(&task->gctx->stdout_lock, raw_id, id_len, content, lang, &task->gctx->base);
        }
    } else if (strcmp(command, "extract-apis") == 0) {
        handle_extract_apis(&task->gctx->stdout_lock, raw_id, id_len, content, lang);
    } else if (strcmp(command, "skeleton") == 0) {
        if (content[0] == '\0' && file[0] != '\0') {
            FILE *f = fopen(file, "r");
            if (f) {
                fseek(f, 0, SEEK_END);
                long fsize = ftell(f);
                fseek(f, 0, SEEK_SET);
                char *fcontent = malloc(fsize + 1);
                if (fcontent) {
                    size_t bytes_read = fread(fcontent, 1, fsize, f);
                    fcontent[bytes_read] = '\0';
                    handle_skeleton(&task->gctx->stdout_lock, raw_id, id_len, fcontent, lang);
                    free(fcontent);
                }
                fclose(f);
            }
        } else {
            handle_skeleton(&task->gctx->stdout_lock, raw_id, id_len, content, lang);
        }
    } else if (strcmp(command, "repo-map") == 0) {
        handle_repo_map(&task->gctx->stdout_lock, raw_id, id_len, dir, &task->gctx->base);
    } else if (strcmp(command, "search-symbols") == 0 || strcmp(command, "search-index") == 0) {
        handle_search_symbols(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, query, kind[0] ? kind : NULL, limit, &task->gctx->base);
    } else if (strcmp(command, "index-file") == 0) {
        handle_index_file(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, file, &task->gctx->base);
    } else if (strcmp(command, "index-observation") == 0) {
        handle_index_observation(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, obs_type, content, timestamp, tokens, &task->gctx->base);
    } else if (strcmp(command, "search-observations") == 0) {
        handle_search_observations(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, query, limit, &task->gctx->base);
    } else if (strcmp(command, "index-critic-decision") == 0) {
        handle_index_critic_decision(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, content, turn, confidence, &task->gctx->base);
    } else if (strcmp(command, "search-critic-decisions") == 0) {
        handle_search_critic_decisions(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, query, limit, &task->gctx->base);
    } else if (strcmp(command, "index-watcher-pattern") == 0) {
        handle_index_watcher_pattern(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, content, file_hash, turn, &task->gctx->base);
    } else if (strcmp(command, "search-watcher-patterns") == 0) {
        handle_search_watcher_patterns(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, query, limit, &task->gctx->base);
    } else if (strcmp(command, "ast-churn") == 0) {
        handle_ast_churn(&task->gctx->stdout_lock, raw_id, id_len, file, content);
    } else if (strcmp(command, "invalidate-file") == 0) {
        handle_invalidate_file(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, file, &task->gctx->base);
    } else if (strcmp(command, "clear-index") == 0) {
        handle_clear_index(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, &task->gctx->base);
    } else if (strcmp(command, "shutdown") == 0) {
        pthread_mutex_lock(&task->gctx->stdout_lock);
        struct jsonw w; jsonw_init(&w, stdout); jsonw_object_open(&w);
        jsonw_kv_bool(&w, "ok", true); jsonw_id(&w, raw_id, id_len); jsonw_kv_str(&w, "status", "shutting_down");
        jsonw_object_close(&w); jsonw_flush(&w);
        pthread_mutex_unlock(&task->gctx->stdout_lock);
        exit(0);
    } else {
        send_error(&task->gctx->stdout_lock, raw_id, id_len, "UNKNOWN_COMMAND", "Unknown analyzer command");
    }

cleanup:
    pthread_mutex_lock(&task->gctx->thread_count_lock);
    task->gctx->active_threads--;
    pthread_mutex_unlock(&task->gctx->thread_count_lock);
    free(task->line);
    free(task);
    return NULL;
}

int main(int argc, char *argv[]) {
    GlobalCtx gctx = {0};
    pthread_mutex_init(&gctx.stdout_lock, NULL);
    pthread_mutex_init(&gctx.thread_count_lock, NULL);
    pthread_mutex_init(&gctx.db_lock, NULL);
    
    char db_path[4096] = "";
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--oneshot") == 0) gctx.base.oneshot = true;
        else if (strcmp(argv[i], "-w") == 0 && i + 1 < argc) {
            strncpy(gctx.base.workspace_root, argv[i+1], sizeof(gctx.base.workspace_root)-1);
            i++;
        } else if (strcmp(argv[i], "--db") == 0 && i + 1 < argc) {
            strncpy(db_path, argv[i+1], sizeof(db_path)-1);
            i++;
        }
    }
    if (!gctx.base.workspace_root[0]) getcwd(gctx.base.workspace_root, sizeof(gctx.base.workspace_root));
    if (db_path[0]) gctx.base.db = db_open(db_path);

    if (!gctx.base.oneshot) {
        fprintf(stderr, "di-rvv-analyzer: ready (C version)\n");
        char *line = NULL;
        size_t cap = 0;
        while (getline(&line, &cap, stdin) > 0) {
            if (line[0] == '\n' || line[0] == '\r' || line[0] == '\0') continue;
            RequestTask *task = malloc(sizeof(RequestTask));
            task->line = strdup(line);
            task->gctx = &gctx;
            pthread_mutex_lock(&gctx.thread_count_lock);
            gctx.active_threads++;
            pthread_mutex_unlock(&gctx.thread_count_lock);
            pthread_t thread;
            if (pthread_create(&thread, NULL, request_worker, task) != 0) {
                pthread_mutex_lock(&gctx.thread_count_lock);
                gctx.active_threads--;
                pthread_mutex_unlock(&gctx.thread_count_lock);
                send_error(&gctx.stdout_lock, NULL, 0, "THREAD_FAILED", "Failed to spawn worker thread");
                free(task->line);
                free(task);
            } else {
                pthread_detach(thread);
            }
        }
        free(line);
        while (1) {
            pthread_mutex_lock(&gctx.thread_count_lock);
            int count = gctx.active_threads;
            pthread_mutex_unlock(&gctx.thread_count_lock);
            if (count == 0) break;
            usleep(10000);
        }
    }
    if (gctx.base.db) db_close((IndexDB*)gctx.base.db);
    return 0;
}
