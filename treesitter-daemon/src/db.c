#include "db.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

IndexDB* db_open(const char *path) {
    IndexDB *db = malloc(sizeof(IndexDB));
    if (sqlite3_open(path, &db->db) != SQLITE_OK) {
        fprintf(stderr, "[db] Failed to open database: %s\n", sqlite3_errmsg(db->db));
        free(db);
        return NULL;
    }

    const char *schema = 
        "PRAGMA journal_mode=WAL;"
        "PRAGMA synchronous=NORMAL;"
        "CREATE TABLE IF NOT EXISTS files ("
        "  id INTEGER PRIMARY KEY AUTOINCREMENT,"
        "  path TEXT UNIQUE NOT NULL,"
        "  mtime REAL NOT NULL,"
        "  content_hash TEXT NOT NULL"
        ");"
        "CREATE TABLE IF NOT EXISTS symbols ("
        "  id INTEGER PRIMARY KEY AUTOINCREMENT,"
        "  file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,"
        "  name TEXT NOT NULL,"
        "  kind TEXT,"
        "  start_line INTEGER NOT NULL,"
        "  end_line INTEGER NOT NULL,"
        "  handle TEXT"
        ");"
        "CREATE TABLE IF NOT EXISTS observer_logs ("
        "  id INTEGER PRIMARY KEY AUTOINCREMENT,"
        "  type TEXT NOT NULL,"
        "  content TEXT NOT NULL,"
        "  timestamp REAL NOT NULL,"
        "  tokens INTEGER NOT NULL"
        ");"
        "CREATE VIRTUAL TABLE IF NOT EXISTS observer_logs_fts USING fts5("
        "  content, "
        "  tokenize='trigram',"
        "  content_rowid='id'"
        ");"
        "CREATE VIRTUAL TABLE IF NOT EXISTS critic_decisions_fts USING fts5("
        "  decision_text, "
        "  turn_number, "
        "  confidence"
        ");"
        "CREATE VIRTUAL TABLE IF NOT EXISTS watcher_patterns_fts USING fts5("
        "  pattern_text, "
        "  file_hash, "
        "  turn_number"
        ");"
        "CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);"
        "CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);";

    char *err = NULL;
    if (sqlite3_exec(db->db, schema, NULL, NULL, &err) != SQLITE_OK) {
        fprintf(stderr, "[db] Schema init failed: %s\n", err);
        sqlite3_free(err);
        sqlite3_close(db->db);
        free(db);
        return NULL;
    }

    return db;
}

void db_close(IndexDB *db) {
    if (!db) return;
    sqlite3_close(db->db);
    free(db);
}

int db_index_file(IndexDB *db, const char *path, double mtime, const char *hash, SymbolResult *sr, ImportResult *ir) {
    (void)ir;
    sqlite3_stmt *stmt;

    if (sqlite3_exec(db->db, "BEGIN TRANSACTION", NULL, NULL, NULL) != SQLITE_OK) return -1;

    if (sqlite3_prepare_v2(db->db, "INSERT OR REPLACE INTO files (path, mtime, content_hash) VALUES (?, ?, ?)", -1, &stmt, NULL) != SQLITE_OK) {
        sqlite3_exec(db->db, "ROLLBACK", NULL, NULL, NULL);
        return -1;
    }
    sqlite3_bind_text(stmt, 1, path, -1, SQLITE_TRANSIENT);
    sqlite3_bind_double(stmt, 2, mtime);
    sqlite3_bind_text(stmt, 3, hash, -1, SQLITE_TRANSIENT);
    if (sqlite3_step(stmt) != SQLITE_DONE) { sqlite3_finalize(stmt); sqlite3_exec(db->db, "ROLLBACK", NULL, NULL, NULL); return -1; }
    int64_t file_id = sqlite3_last_insert_rowid(db->db);
    sqlite3_finalize(stmt);

    if (sqlite3_prepare_v2(db->db, "DELETE FROM symbols WHERE file_id = ?", -1, &stmt, NULL) != SQLITE_OK) {
        sqlite3_exec(db->db, "ROLLBACK", NULL, NULL, NULL);
        return -1;
    }
    sqlite3_bind_int64(stmt, 1, file_id);
    if (sqlite3_step(stmt) != SQLITE_DONE) { sqlite3_finalize(stmt); sqlite3_exec(db->db, "ROLLBACK", NULL, NULL, NULL); return -1; }
    sqlite3_finalize(stmt);

    if (sr) {
        if (sqlite3_prepare_v2(db->db, "INSERT INTO symbols (file_id, name, kind, start_line, end_line, handle) VALUES (?, ?, ?, ?, ?, ?)", -1, &stmt, NULL) != SQLITE_OK) {
            sqlite3_exec(db->db, "ROLLBACK", NULL, NULL, NULL);
            return -1;
        }
        for (size_t i = 0; i < sr->count; i++) {
            sqlite3_reset(stmt);
            sqlite3_bind_int64(stmt, 1, file_id);
            sqlite3_bind_text(stmt, 2, sr->symbols[i].name, -1, SQLITE_TRANSIENT);
            sqlite3_bind_text(stmt, 3, symbol_kind_to_str(sr->symbols[i].kind), -1, SQLITE_TRANSIENT);
            sqlite3_bind_int(stmt, 4, sr->symbols[i].start_line);
            sqlite3_bind_int(stmt, 5, sr->symbols[i].end_line);
            sqlite3_bind_text(stmt, 6, sr->symbols[i].handle, -1, SQLITE_TRANSIENT);
            if (sqlite3_step(stmt) != SQLITE_DONE) { sqlite3_finalize(stmt); sqlite3_exec(db->db, "ROLLBACK", NULL, NULL, NULL); return -1; }
        }
        sqlite3_finalize(stmt);
    }

    if (sqlite3_exec(db->db, "COMMIT", NULL, NULL, NULL) != SQLITE_OK) return -1;
    return 0;
}

int db_invalidate_file(IndexDB *db, const char *path) {
    sqlite3_stmt *stmt;
    if (sqlite3_prepare_v2(db->db, "DELETE FROM files WHERE path = ?", -1, &stmt, NULL) != SQLITE_OK) return -1;
    sqlite3_bind_text(stmt, 1, path, -1, SQLITE_TRANSIENT);
    if (sqlite3_step(stmt) != SQLITE_DONE) { sqlite3_finalize(stmt); return -1; }
    sqlite3_finalize(stmt);
    return 0;
}

int db_clear(IndexDB *db) {
    if (sqlite3_exec(db->db, "DELETE FROM symbols; DELETE FROM files; DELETE FROM observer_logs; DELETE FROM observer_logs_fts; DELETE FROM critic_decisions_fts; DELETE FROM watcher_patterns_fts;", NULL, NULL, NULL) != SQLITE_OK) return -1;
    return 0;
}

int db_index_observation(IndexDB *db, const char *type, const char *content, double timestamp, int token_estimate) {
    sqlite3_stmt *stmt;
    if (sqlite3_prepare_v2(db->db, "INSERT INTO observer_logs (type, content, timestamp, tokens) VALUES (?, ?, ?, ?)", -1, &stmt, NULL) != SQLITE_OK) return -1;
    sqlite3_bind_text(stmt, 1, type, -1, SQLITE_TRANSIENT);
    sqlite3_bind_text(stmt, 2, content, -1, SQLITE_TRANSIENT);
    sqlite3_bind_double(stmt, 3, timestamp);
    sqlite3_bind_int(stmt, 4, token_estimate);
    if (sqlite3_step(stmt) != SQLITE_DONE) { sqlite3_finalize(stmt); return -1; }
    int64_t row_id = sqlite3_last_insert_rowid(db->db);
    sqlite3_finalize(stmt);

    if (sqlite3_prepare_v2(db->db, "INSERT INTO observer_logs_fts (rowid, content) VALUES (?, ?)", -1, &stmt, NULL) != SQLITE_OK) return -1;
    sqlite3_bind_int64(stmt, 1, row_id);
    sqlite3_bind_text(stmt, 2, content, -1, SQLITE_TRANSIENT);
    if (sqlite3_step(stmt) != SQLITE_DONE) { sqlite3_finalize(stmt); return -1; }
    sqlite3_finalize(stmt);

    return 0;
}

int db_search_observations(IndexDB *db, const char *query, int limit, struct jsonw *w) {
    sqlite3_stmt *stmt;
    const char *sql = "SELECT l.type, l.content, l.timestamp, l.tokens "
                      "FROM observer_logs l JOIN observer_logs_fts f ON l.id = f.rowid "
                      "WHERE observer_logs_fts MATCH ? ORDER BY rank LIMIT ?";

    if (sqlite3_prepare_v2(db->db, sql, -1, &stmt, NULL) != SQLITE_OK) {
        jsonw_key(w, "results");
        jsonw_array_open(w);
        jsonw_array_close(w);
        jsonw_key(w, "ok");
        jsonw_bool(w, 0);
        jsonw_key(w, "code");
        jsonw_str(w, "SEARCH_FAILED");
        jsonw_key(w, "message");
        jsonw_str(w, sqlite3_errmsg(db->db));
        return -1;
    }

    sqlite3_bind_text(stmt, 1, query, -1, SQLITE_TRANSIENT);
    sqlite3_bind_int(stmt, 2, limit);

    jsonw_key(w, "results");
    jsonw_array_open(w);
    while (sqlite3_step(stmt) == SQLITE_ROW) {
        jsonw_object_open(w);
        jsonw_kv_str(w, "type", (const char*)sqlite3_column_text(stmt, 0));
        jsonw_kv_str(w, "content", (const char*)sqlite3_column_text(stmt, 1));
        jsonw_kv_double(w, "timestamp", sqlite3_column_double(stmt, 2));
        jsonw_kv_int(w, "tokens", sqlite3_column_int(stmt, 3));
        jsonw_object_close(w);
    }
    jsonw_array_close(w);
    sqlite3_finalize(stmt);
    jsonw_key(w, "ok");
    jsonw_bool(w, 1);
    return 0;
}

int db_index_critic_decision(IndexDB *db, const char *text, int turn, double confidence) {
    sqlite3_stmt *stmt;
    if (sqlite3_prepare_v2(db->db, "INSERT INTO critic_decisions_fts (decision_text, turn_number, confidence) VALUES (?, ?, ?)", -1, &stmt, NULL) != SQLITE_OK) return -1;
    sqlite3_bind_text(stmt, 1, text, -1, SQLITE_TRANSIENT);
    sqlite3_bind_int(stmt, 2, turn);
    sqlite3_bind_double(stmt, 3, confidence);
    if (sqlite3_step(stmt) != SQLITE_DONE) { sqlite3_finalize(stmt); return -1; }
    sqlite3_finalize(stmt);
    return 0;
}

int db_search_critic_decisions(IndexDB *db, const char *query, int limit, struct jsonw *w) {
    sqlite3_stmt *stmt;
    const char *sql = "SELECT decision_text, turn_number, confidence FROM critic_decisions_fts WHERE critic_decisions_fts MATCH ? ORDER BY rank LIMIT ?";
    if (sqlite3_prepare_v2(db->db, sql, -1, &stmt, NULL) != SQLITE_OK) {
        jsonw_key(w, "results"); jsonw_array_open(w); jsonw_array_close(w);
        jsonw_key(w, "ok"); jsonw_bool(w, 0);
        jsonw_key(w, "code"); jsonw_str(w, "SEARCH_FAILED");
        jsonw_key(w, "message"); jsonw_str(w, sqlite3_errmsg(db->db));
        return -1;
    }
    sqlite3_bind_text(stmt, 1, query, -1, SQLITE_TRANSIENT);
    sqlite3_bind_int(stmt, 2, limit);
    jsonw_key(w, "results");
    jsonw_array_open(w);
    while (sqlite3_step(stmt) == SQLITE_ROW) {
        jsonw_object_open(w);
        jsonw_kv_str(w, "text", (const char*)sqlite3_column_text(stmt, 0));
        jsonw_kv_int(w, "turn", sqlite3_column_int(stmt, 1));
        jsonw_kv_double(w, "confidence", sqlite3_column_double(stmt, 2));
        jsonw_object_close(w);
    }
    jsonw_array_close(w);
    sqlite3_finalize(stmt);
    jsonw_key(w, "ok"); jsonw_bool(w, 1);
    return 0;
}

int db_index_watcher_pattern(IndexDB *db, const char *text, const char *file_hash, int turn) {
    sqlite3_stmt *stmt;
    if (sqlite3_prepare_v2(db->db, "INSERT INTO watcher_patterns_fts (pattern_text, file_hash, turn_number) VALUES (?, ?, ?)", -1, &stmt, NULL) != SQLITE_OK) return -1;
    sqlite3_bind_text(stmt, 1, text, -1, SQLITE_TRANSIENT);
    sqlite3_bind_text(stmt, 2, file_hash ? file_hash : "", -1, SQLITE_TRANSIENT);
    sqlite3_bind_int(stmt, 3, turn);
    if (sqlite3_step(stmt) != SQLITE_DONE) { sqlite3_finalize(stmt); return -1; }
    sqlite3_finalize(stmt);
    return 0;
}

int db_search_watcher_patterns(IndexDB *db, const char *query, int limit, struct jsonw *w) {
    sqlite3_stmt *stmt;
    const char *sql = "SELECT pattern_text, file_hash, turn_number FROM watcher_patterns_fts WHERE watcher_patterns_fts MATCH ? ORDER BY rank LIMIT ?";
    if (sqlite3_prepare_v2(db->db, sql, -1, &stmt, NULL) != SQLITE_OK) {
        jsonw_key(w, "results"); jsonw_array_open(w); jsonw_array_close(w);
        jsonw_key(w, "ok"); jsonw_bool(w, 0);
        jsonw_key(w, "code"); jsonw_str(w, "SEARCH_FAILED");
        jsonw_key(w, "message"); jsonw_str(w, sqlite3_errmsg(db->db));
        return -1;
    }
    sqlite3_bind_text(stmt, 1, query, -1, SQLITE_TRANSIENT);
    sqlite3_bind_int(stmt, 2, limit);
    jsonw_key(w, "results");
    jsonw_array_open(w);
    while (sqlite3_step(stmt) == SQLITE_ROW) {
        jsonw_object_open(w);
        jsonw_kv_str(w, "text", (const char*)sqlite3_column_text(stmt, 0));
        jsonw_kv_str(w, "file_hash", (const char*)sqlite3_column_text(stmt, 1));
        jsonw_kv_int(w, "turn", sqlite3_column_int(stmt, 2));
        jsonw_object_close(w);
    }
    jsonw_array_close(w);
    sqlite3_finalize(stmt);
    jsonw_key(w, "ok"); jsonw_bool(w, 1);
    return 0;
}

void db_search_symbols(IndexDB *db, const char *query, const char *kind_filter, int limit, struct jsonw *w) {
    sqlite3_stmt *stmt;
    char pattern[256];
    snprintf(pattern, sizeof(pattern), "%%%s%%", query);

    const char *sql;
    if (kind_filter) {
        sql = "SELECT s.name, s.kind, s.handle, f.path, s.start_line "
              "FROM symbols s JOIN files f ON s.file_id = f.id "
              "WHERE s.name LIKE ? AND s.kind = ? LIMIT ?";
    } else {
        sql = "SELECT s.name, s.kind, s.handle, f.path, s.start_line "
              "FROM symbols s JOIN files f ON s.file_id = f.id "
              "WHERE s.name LIKE ? LIMIT ?";
    }

    if (sqlite3_prepare_v2(db->db, sql, -1, &stmt, NULL) != SQLITE_OK) {
        jsonw_key(w, "results");
        jsonw_array_open(w);
        jsonw_array_close(w);
        jsonw_key(w, "ok");
        jsonw_bool(w, 0);
        return;
    }
    sqlite3_bind_text(stmt, 1, pattern, -1, SQLITE_TRANSIENT);
    if (kind_filter) {
        sqlite3_bind_text(stmt, 2, kind_filter, -1, SQLITE_TRANSIENT);
        sqlite3_bind_int(stmt, 3, limit);
    } else {
        sqlite3_bind_int(stmt, 2, limit);
    }

    jsonw_key(w, "results");
    jsonw_array_open(w);
    while (sqlite3_step(stmt) == SQLITE_ROW) {
        jsonw_object_open(w);
        jsonw_kv_str(w, "name", (const char*)sqlite3_column_text(stmt, 0));
        jsonw_kv_str(w, "kind", (const char*)sqlite3_column_text(stmt, 1));
        jsonw_kv_str(w, "handle", (const char*)sqlite3_column_text(stmt, 2));
        jsonw_kv_str(w, "file", (const char*)sqlite3_column_text(stmt, 3));
        jsonw_kv_int(w, "start_line", sqlite3_column_int(stmt, 4));
        jsonw_object_close(w);
    }
    jsonw_array_close(w);
    sqlite3_finalize(stmt);
    jsonw_key(w, "ok");
    jsonw_bool(w, 1);
}
