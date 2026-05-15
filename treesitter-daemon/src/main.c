#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <stdbool.h>
#include <pthread.h>
#include <poll.h>
#include <signal.h>
#include <tree_sitter/api.h>

#include "analyzer.h"
#include "db.h"
#include "json.h"
#include "json-write.h"

#define MAX_LINE 1048576 
#define MAX_THREADS 8
#define MAX_FILE_SIZE (50 * 1024 * 1024)  /* 50 MB max file read */
#define LINE_BUF_SZ (1024 * 1024)

typedef struct {
    pthread_mutex_t stdout_lock;
    pthread_mutex_t thread_count_lock;
    pthread_mutex_t db_lock;
    int active_threads;
    volatile int running;
    AnalyzerCtx base;
} GlobalCtx;

typedef struct {
    char *line;
    GlobalCtx *gctx;
} RequestTask;

__thread FILE *jsonw_output_fallback = NULL;

#include <sys/syscall.h>
static void jsonw_id(struct jsonw *w, const char *raw_id, int id_len) {
    jsonw_key(w, "id");
    if (!raw_id || id_len <= 0) {
        jsonw_null(w);
    } else if (raw_id[0] == '"' && id_len >= 2 && raw_id[id_len - 1] == '"') {
        // Quoted string id: strip quotes and output as JSON string
        jsonw_strn(w, raw_id + 1, id_len - 2, 0);
    } else if (raw_id[0] == '"') {
        // Unterminated quoted id — fall back to null
        jsonw_null(w);
    } else {
        // Numeric or bare id: write raw value (already valid JSON)
        fwrite(raw_id, 1, id_len, w->f);
        w->need_comma = true;
    }
}

static void send_error(pthread_mutex_t *lock, const char *raw_id, int id_len, const char *code, const char *message) {
    // Map daemon-internal error codes to standard error types
    const char *error_type = "ToolInternalError";
    if (strcmp(code, "PARSE_FAILED") == 0)               error_type = "ParseError";
    else if (strcmp(code, "INVALID_REQUEST") == 0)        error_type = "InvalidRequest";
    else if (strcmp(code, "UNKNOWN_COMMAND") == 0)        error_type = "UnknownCommand";
    else if (strcmp(code, "FILE_ERROR") == 0)             error_type = "FileError";
    else if (strcmp(code, "THREAD_REJECTED") == 0 ||
             strcmp(code, "THREAD_FAILED") == 0)           error_type = "ServerOverloaded";

    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "error");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", false);
    jsonw_kv_str(&w, "code", code);
    jsonw_kv_str(&w, "error_type", error_type);
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
                jsonw_kv_str(w, "t", symbol_kind_to_short(sr->symbols[i].kind));
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

/* Forward declaration for path safety check used below */
static bool is_path_safe(const char *path, const char *workspace_root);

static void handle_repo_map(pthread_mutex_t *lock, const char *raw_id, int id_len, const char *dir, AnalyzerCtx *ctx) {
    const char *root = (dir && dir[0]) ? dir : ctx->workspace_root;
    
    /* Reject dir outside workspace for security */
    if (dir && dir[0] && !is_path_safe(dir, ctx->workspace_root)) {
        send_error(lock, raw_id, id_len, "PATH_NOT_SAFE", "Directory is outside workspace root");
        return;
    }
    
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
    int ok = db_index_observation((IndexDB*)ctx->db, type, content, timestamp, tokens);
    pthread_mutex_unlock(db_lock);

    if (ok != 0) fprintf(stderr, "[warn] db_index_observation failed\n");

    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ok");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", ok == 0);
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
    int ok = db_index_critic_decision((IndexDB*)ctx->db, text, turn, confidence);
    pthread_mutex_unlock(db_lock);
    if (ok != 0) fprintf(stderr, "[warn] db_index_critic_decision failed\n");
    pthread_mutex_lock(lock);
    struct jsonw w; jsonw_init(&w, stdout); jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ok"); jsonw_id(&w, raw_id, id_len); jsonw_kv_bool(&w, "ok", ok == 0);
    jsonw_object_close(&w); jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_search_critic_decisions(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *query, int limit, AnalyzerCtx *ctx) {
    if (!ctx->db) { send_error(lock, raw_id, id_len, "DB_NOT_OPEN", "Index database is not open"); return; }
    pthread_mutex_lock(lock);
    struct jsonw w; jsonw_init(&w, stdout); jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "search_critic_decisions_result"); jsonw_id(&w, raw_id, id_len);
    pthread_mutex_lock(db_lock);
    db_search_critic_decisions((IndexDB*)ctx->db, query, limit, &w);
    pthread_mutex_unlock(db_lock);
    jsonw_object_close(&w); jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_index_watcher_pattern(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *text, const char *file_hash, int turn, AnalyzerCtx *ctx) {
    if (!ctx->db) { send_error(lock, raw_id, id_len, "DB_NOT_OPEN", "Index database is not open"); return; }
    pthread_mutex_lock(db_lock);
    int ok = db_index_watcher_pattern((IndexDB*)ctx->db, text, file_hash, turn);
    pthread_mutex_unlock(db_lock);
    if (ok != 0) fprintf(stderr, "[warn] db_index_watcher_pattern failed\n");
    pthread_mutex_lock(lock);
    struct jsonw w; jsonw_init(&w, stdout); jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ok"); jsonw_id(&w, raw_id, id_len); jsonw_kv_bool(&w, "ok", ok == 0);
    jsonw_object_close(&w); jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_search_watcher_patterns(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, const char *query, int limit, AnalyzerCtx *ctx) {
    if (!ctx->db) { send_error(lock, raw_id, id_len, "DB_NOT_OPEN", "Index database is not open"); return; }
    pthread_mutex_lock(lock);
    struct jsonw w; jsonw_init(&w, stdout); jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "search_watcher_patterns_result"); jsonw_id(&w, raw_id, id_len);
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
    if (size < 0) { fclose(f);
        send_error(lock, raw_id, id_len, "FILE_READ_ERROR", "Failed to determine file size");
        return;
    }
    if (size == 0 || size > MAX_FILE_SIZE) { fclose(f);
        send_error(lock, raw_id, id_len, "FILE_TOO_LARGE", "File exceeds maximum indexable size");
        return;
    }
    char *content = malloc(size + 1);
    if (!content) { fclose(f); send_error(lock, raw_id, id_len, "OOM", "Failed to allocate buffer for file content"); return; }
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
    int db_ok = db_index_file((IndexDB*)ctx->db, file, 0.0, "TODO_HASH", sr, ir);
    pthread_mutex_unlock(db_lock);

    if (db_ok != 0) {
        fprintf(stderr, "[warn] db_index_file failed for %s\n", file);
    }

    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "index_file_result");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", true);
    if ((sr && sr->error_code != 0) || db_ok != 0) jsonw_kv_bool(&w, "truncated", true);
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
    int ok = 1;
    if (ctx->db) {
        pthread_mutex_lock(db_lock);
        ok = db_invalidate_file((IndexDB*)ctx->db, file);
        pthread_mutex_unlock(db_lock);
        if (ok != 0) fprintf(stderr, "[warn] db_invalidate_file failed for %s\n", file);
    }
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ok");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", ok == 0);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

static void handle_clear_index(pthread_mutex_t *lock, pthread_mutex_t *db_lock, const char *raw_id, int id_len, AnalyzerCtx *ctx) {
    int ok = 1;
    if (ctx->db) {
        pthread_mutex_lock(db_lock);
        ok = db_clear((IndexDB*)ctx->db);
        pthread_mutex_unlock(db_lock);
        if (ok != 0) fprintf(stderr, "[warn] db_clear failed\n");
    }
    pthread_mutex_lock(lock);
    struct jsonw w;
    jsonw_init(&w, stdout);
    jsonw_object_open(&w);
    jsonw_kv_str(&w, "type", "ok");
    jsonw_id(&w, raw_id, id_len);
    jsonw_kv_bool(&w, "ok", ok == 0);
    jsonw_object_close(&w);
    jsonw_flush(&w);
    pthread_mutex_unlock(lock);
}

/* Verify that path resolves within workspace_root (security: prevent
   arbitrary file reads outside the intended tree). */
static bool is_path_safe(const char *path, const char *workspace_root) {
    if (!path || !workspace_root || !workspace_root[0]) return false;
    char resolved[4096];
    if (!realpath(path, resolved)) return false;
    size_t rl = strlen(workspace_root);
    // workspace_root may have a trailing slash; normalise by checking prefix
    return strncmp(resolved, workspace_root, rl) == 0 &&
           (resolved[rl] == '/' || resolved[rl] == '\0');
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
    bool handled = false;
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
        handled = true;
    } else if (strcmp(command, "outline") == 0) {
        if (content[0] == '\0' && file[0] != '\0') {
            if (!is_path_safe(file, task->gctx->base.workspace_root)) { goto send_err; }
            FILE *f = fopen(file, "r");
            if (f) {
                fseek(f, 0, SEEK_END);
                long fsize = ftell(f);
                fseek(f, 0, SEEK_SET);
                if (fsize <= 0 || fsize > MAX_FILE_SIZE) { fclose(f); goto send_err; }
                char *fcontent = malloc(fsize + 1);
                if (!fcontent) { fclose(f); goto send_err; }
                size_t bytes_read = fread(fcontent, 1, fsize, f);
                if (bytes_read != (size_t)fsize) { free(fcontent); fclose(f); goto send_err; }
                fcontent[bytes_read] = '\0';
                handle_outline(&task->gctx->stdout_lock, raw_id, id_len, fcontent, lang, &task->gctx->base);
                handled = true;
                free(fcontent);
                fclose(f);
            } else {
                goto send_err;
            }
        } else {
            handle_outline(&task->gctx->stdout_lock, raw_id, id_len, content, lang, &task->gctx->base);
            handled = true;
        }
    } else if (strcmp(command, "extract-apis") == 0) {
        handle_extract_apis(&task->gctx->stdout_lock, raw_id, id_len, content, lang);
        handled = true;
    } else if (strcmp(command, "skeleton") == 0) {
        if (content[0] == '\0' && file[0] != '\0') {
            FILE *f = fopen(file, "r");
            if (f) {
                fseek(f, 0, SEEK_END);
                long fsize = ftell(f);
                fseek(f, 0, SEEK_SET);
                if (fsize <= 0 || fsize > MAX_FILE_SIZE) { fclose(f); goto send_err; }
                char *fcontent = malloc(fsize + 1);
                if (!fcontent) { fclose(f); goto send_err; }
                size_t bytes_read = fread(fcontent, 1, fsize, f);
                if (bytes_read != (size_t)fsize) { free(fcontent); fclose(f); goto send_err; }
                fcontent[bytes_read] = '\0';
                handle_skeleton(&task->gctx->stdout_lock, raw_id, id_len, fcontent, lang);
                handled = true;
                free(fcontent);
                fclose(f);
            } else {
                goto send_err;
            }
        } else {
            handle_skeleton(&task->gctx->stdout_lock, raw_id, id_len, content, lang);
            handled = true;
        }
    } else if (strcmp(command, "repo-map") == 0) {
        handle_repo_map(&task->gctx->stdout_lock, raw_id, id_len, dir, &task->gctx->base);
        handled = true;
    } else if (strcmp(command, "search-symbols") == 0 || strcmp(command, "search-index") == 0) {
        handle_search_symbols(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, query, kind[0] ? kind : NULL, limit, &task->gctx->base);
        handled = true;
    } else if (strcmp(command, "index-file") == 0) {
        if (!is_path_safe(file, task->gctx->base.workspace_root)) { goto send_err; }
        handle_index_file(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, file, &task->gctx->base);
        handled = true;
    } else if (strcmp(command, "index-observation") == 0) {
        handle_index_observation(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, obs_type, content, timestamp, tokens, &task->gctx->base);
        handled = true;
    } else if (strcmp(command, "search-observations") == 0) {
        handle_search_observations(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, query, limit, &task->gctx->base);
        handled = true;
    } else if (strcmp(command, "index-critic-decision") == 0) {
        handle_index_critic_decision(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, content, turn, confidence, &task->gctx->base);
        handled = true;
    } else if (strcmp(command, "search-critic-decisions") == 0) {
        handle_search_critic_decisions(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, query, limit, &task->gctx->base);
        handled = true;
    } else if (strcmp(command, "index-watcher-pattern") == 0) {
        handle_index_watcher_pattern(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, content, file_hash, turn, &task->gctx->base);
        handled = true;
    } else if (strcmp(command, "search-watcher-patterns") == 0) {
        handle_search_watcher_patterns(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, query, limit, &task->gctx->base);
        handled = true;
    } else if (strcmp(command, "ast-churn") == 0) {
        if (!is_path_safe(file, task->gctx->base.workspace_root)) { goto send_err; }
        handle_ast_churn(&task->gctx->stdout_lock, raw_id, id_len, file, content);
        handled = true;
    } else if (strcmp(command, "invalidate-file") == 0) {
        if (!is_path_safe(file, task->gctx->base.workspace_root)) { goto send_err; }
        handle_invalidate_file(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, file, &task->gctx->base);
        handled = true;
    } else if (strcmp(command, "clear-index") == 0) {
        handle_clear_index(&task->gctx->stdout_lock, &task->gctx->db_lock, raw_id, id_len, &task->gctx->base);
        handled = true;
    } else if (strcmp(command, "shutdown") == 0) {
        /* Send response before jumping to cleanup */
        pthread_mutex_lock(&task->gctx->stdout_lock);
        struct jsonw w; jsonw_init(&w, stdout); jsonw_object_open(&w);
        jsonw_kv_bool(&w, "ok", true); jsonw_id(&w, raw_id, id_len); jsonw_kv_str(&w, "status", "shutting_down");
        jsonw_object_close(&w); jsonw_flush(&w);
        pthread_mutex_unlock(&task->gctx->stdout_lock);
        task->gctx->running = 0;
        goto cleanup;
    } else if (command[0] && !handled) {
        send_error(&task->gctx->stdout_lock, raw_id, id_len, "UNKNOWN_COMMAND", "Unknown analyzer command");
        goto cleanup;
    }

send_err:
    /* File-read failure (reached via goto send_err from outline/skeleton/index-file/ast-churn).
       Send an error response only if no handler already wrote one. */
    if (!handled) {
        send_error(&task->gctx->stdout_lock, raw_id, id_len, "FILE_IO_ERROR", "Failed to read file (missing, empty, or too large)");
    }

cleanup:
    jsonw_output_fallback = NULL;
    pthread_mutex_lock(&task->gctx->thread_count_lock);
    task->gctx->active_threads--;
    pthread_mutex_unlock(&task->gctx->thread_count_lock);
    free(task->line);
    free(task);
    return NULL;
}

static volatile int sig_shutdown = 0;

static void handle_signal(int sig) {
    (void)sig;
    sig_shutdown = 1;
}

int main(int argc, char *argv[]) {
    // Ignore SIGPIPE: writing to a broken pipe kills the daemon otherwise.
    // With SIG_IGN the write fails with EPIPE and we continue.
    signal(SIGPIPE, SIG_IGN);
    signal(SIGTERM, handle_signal);
    signal(SIGINT, handle_signal);

    GlobalCtx gctx = {0};
    pthread_mutex_init(&gctx.stdout_lock, NULL);
    pthread_mutex_init(&gctx.thread_count_lock, NULL);
    
    char db_path[4096] = "";
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--oneshot") == 0) gctx.base.oneshot = true;
        else if (strcmp(argv[i], "-w") == 0 && i + 1 < argc) {
            snprintf(gctx.base.workspace_root, sizeof(gctx.base.workspace_root), "%s", argv[i+1]);
            i++;
        } else if (strcmp(argv[i], "--db") == 0 && i + 1 < argc) {
            snprintf(db_path, sizeof(db_path), "%s", argv[i+1]);
            i++;
        }
    }
    if (!gctx.base.workspace_root[0]) (void)getcwd(gctx.base.workspace_root, sizeof(gctx.base.workspace_root));
    if (db_path[0]) gctx.base.db = db_open(db_path);

    gctx.running = 1;

    if (!gctx.base.oneshot) {
        fprintf(stderr, "divrr-analyzer: ready (C version)\n");

        // poll-based stdin reader + stdout writer. Single poll loop handles
        // both fds: when stdin is readable, read and spawn workers; when
        // stdout is writable, drain the response queue to the real fd.
        char *line_buf = malloc(LINE_BUF_SZ);
        size_t buf_used = 0;

        while (1) {
            /* Check shutdown flags (set by shutdown command or signal handler)
               on every iteration — poll timeout ensures we wake regularly. */
            if (!gctx.running || sig_shutdown) {
                gctx.running = 0;
                break;
            }

            struct pollfd pfd;
            pfd.fd = STDIN_FILENO;
            pfd.events = POLLIN;

            int ret = poll(&pfd, 1, 100);
            if (ret <= 0) continue;

            if (pfd.revents & POLLIN) {
                // If buffer is nearly full without a newline, discard
                // to prevent DOS from oversized lines.
                if (buf_used >= LINE_BUF_SZ / 2) {
                    // Check if there's any newline in the buffer
                    char *nl = memchr(line_buf, '\n', buf_used);
                    if (!nl) {
                        // No newline found — discard all data and reset
                        buf_used = 0;
                    } else {
                        // Keep from last newline onward
                        size_t keep = buf_used - ((nl + 1) - line_buf);
                        memmove(line_buf, nl + 1, keep);
                        buf_used = keep;
                    }
                }
                ssize_t n = read(STDIN_FILENO, line_buf + buf_used, LINE_BUF_SZ - buf_used - 1);
                if (n <= 0) break;
                buf_used += (size_t)n;
                line_buf[buf_used] = '\0';

                char *start = line_buf;
                while (1) {
                    char *nl = strchr(start, '\n');
                    if (!nl) break;
                    *nl = '\0';

                    if (start[0] && start[0] != '\r' && start[0] != '\0') {
                        RequestTask *task = malloc(sizeof(RequestTask));
                        if (!task) {
                            send_error(&gctx.stdout_lock, NULL, 0, "OOM", "Failed to allocate request task");
                            start = nl + 1;
                            continue;
                        }
                        task->line = strdup(start);
                        if (!task->line) {
                            free(task);
                            send_error(&gctx.stdout_lock, NULL, 0, "OOM", "Failed to copy request line");
                            start = nl + 1;
                            continue;
                        }
                        task->gctx = &gctx;
                        pthread_mutex_lock(&gctx.thread_count_lock);
                        if (gctx.active_threads >= MAX_THREADS) {
                            pthread_mutex_unlock(&gctx.thread_count_lock);
                            send_error(&gctx.stdout_lock, NULL, 0, "THREAD_REJECTED",
                                       "Server at maximum thread capacity");
                            free(task->line);
                            free(task);
                        } else {
                            gctx.active_threads++;
                            pthread_mutex_unlock(&gctx.thread_count_lock);
                            pthread_t thread;
                            if (pthread_create(&thread, NULL, request_worker, task) != 0) {
                                pthread_mutex_lock(&gctx.thread_count_lock);
                                gctx.active_threads--;
                                pthread_mutex_unlock(&gctx.thread_count_lock);
                                send_error(&gctx.stdout_lock, NULL, 0, "THREAD_FAILED",
                                           "Failed to spawn worker thread");
                                free(task->line);
                                free(task);
                            } else {
                                pthread_detach(thread);
                            }
                        }
                    }
                    start = nl + 1;
                }

                // Keep incomplete trailing data
                size_t consumed = (size_t)(start - line_buf);
                if (consumed > 0) {
                    size_t remaining = buf_used - consumed;
                    if (remaining > 0) memmove(line_buf, start, remaining);
                    buf_used = remaining;
                }
            }

            /* Check for connection close or error on stdin */
            if (pfd.revents & (POLLHUP | POLLERR)) break;
        }
        free(line_buf);

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
