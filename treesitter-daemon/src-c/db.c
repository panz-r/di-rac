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
        "CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);"
        "CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);";

    char *err = NULL;
    if (sqlite3_exec(db->db, schema, NULL, NULL, &err) != SQLITE_OK) {
        fprintf(stderr, "[db] Schema error: %s\n", err);
        sqlite3_free(err);
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
    
    sqlite3_exec(db->db, "BEGIN TRANSACTION", NULL, NULL, NULL);

    sqlite3_prepare_v2(db->db, "INSERT OR REPLACE INTO files (path, mtime, content_hash) VALUES (?, ?, ?)", -1, &stmt, NULL);
    sqlite3_bind_text(stmt, 1, path, -1, SQLITE_STATIC);
    sqlite3_bind_double(stmt, 2, mtime);
    sqlite3_bind_text(stmt, 3, hash, -1, SQLITE_STATIC);
    sqlite3_step(stmt);
    int64_t file_id = sqlite3_last_insert_rowid(db->db);
    sqlite3_finalize(stmt);

    sqlite3_prepare_v2(db->db, "DELETE FROM symbols WHERE file_id = ?", -1, &stmt, NULL);
    sqlite3_bind_int64(stmt, 1, file_id);
    sqlite3_step(stmt);
    sqlite3_finalize(stmt);

    if (sr) {
        sqlite3_prepare_v2(db->db, "INSERT INTO symbols (file_id, name, kind, start_line, end_line, handle) VALUES (?, ?, ?, ?, ?, ?)", -1, &stmt, NULL);
        for (size_t i = 0; i < sr->count; i++) {
            sqlite3_reset(stmt);
            sqlite3_bind_int64(stmt, 1, file_id);
            sqlite3_bind_text(stmt, 2, sr->symbols[i].name, -1, SQLITE_STATIC);
            sqlite3_bind_text(stmt, 3, symbol_kind_to_str(sr->symbols[i].kind), -1, SQLITE_STATIC);
            sqlite3_bind_int(stmt, 4, sr->symbols[i].start_line);
            sqlite3_bind_int(stmt, 5, sr->symbols[i].end_line);
            sqlite3_bind_text(stmt, 6, sr->symbols[i].handle, -1, SQLITE_STATIC);
            sqlite3_step(stmt);
        }
        sqlite3_finalize(stmt);
    }

    sqlite3_exec(db->db, "COMMIT", NULL, NULL, NULL);
    return 0;
}

int db_invalidate_file(IndexDB *db, const char *path) {
    sqlite3_stmt *stmt;
    sqlite3_prepare_v2(db->db, "DELETE FROM files WHERE path = ?", -1, &stmt, NULL);
    sqlite3_bind_text(stmt, 1, path, -1, SQLITE_STATIC);
    sqlite3_step(stmt);
    sqlite3_finalize(stmt);
    return 0;
}

int db_clear(IndexDB *db) {
    sqlite3_exec(db->db, "DELETE FROM symbols; DELETE FROM files; DELETE FROM observer_logs; DELETE FROM observer_logs_fts;", NULL, NULL, NULL);
    return 0;
}

int db_index_observation(IndexDB *db, const char *type, const char *content, double timestamp, int token_estimate) {
    sqlite3_stmt *stmt;
    sqlite3_prepare_v2(db->db, "INSERT INTO observer_logs (type, content, timestamp, tokens) VALUES (?, ?, ?, ?)", -1, &stmt, NULL);
    sqlite3_bind_text(stmt, 1, type, -1, SQLITE_STATIC);
    sqlite3_bind_text(stmt, 2, content, -1, SQLITE_STATIC);
    sqlite3_bind_double(stmt, 3, timestamp);
    sqlite3_bind_int(stmt, 4, token_estimate);
    sqlite3_step(stmt);
    int64_t row_id = sqlite3_last_insert_rowid(db->db);
    sqlite3_finalize(stmt);

    sqlite3_prepare_v2(db->db, "INSERT INTO observer_logs_fts (rowid, content) VALUES (?, ?)", -1, &stmt, NULL);
    sqlite3_bind_int64(stmt, 1, row_id);
    sqlite3_bind_text(stmt, 2, content, -1, SQLITE_STATIC);
    sqlite3_step(stmt);
    sqlite3_finalize(stmt);

    return 0;
}

void db_search_observations(IndexDB *db, const char *query, int limit, struct jsonw *w) {
    sqlite3_stmt *stmt;
    const char *sql = "SELECT l.type, l.content, l.timestamp, l.tokens "
                      "FROM observer_logs l JOIN observer_logs_fts f ON l.id = f.rowid "
                      "WHERE observer_logs_fts MATCH ? ORDER BY rank LIMIT ?";
    
    if (sqlite3_prepare_v2(db->db, sql, -1, &stmt, NULL) != SQLITE_OK) {
        jsonw_key(w, "results");
        jsonw_array_open(w);
        jsonw_array_close(w);
        return;
    }

    sqlite3_bind_text(stmt, 1, query, -1, SQLITE_STATIC);
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

    sqlite3_prepare_v2(db->db, sql, -1, &stmt, NULL);
    sqlite3_bind_text(stmt, 1, pattern, -1, SQLITE_STATIC);
    if (kind_filter) {
        sqlite3_bind_text(stmt, 2, kind_filter, -1, SQLITE_STATIC);
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
}
