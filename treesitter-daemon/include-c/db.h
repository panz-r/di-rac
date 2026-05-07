#ifndef DB_H
#define DB_H

#include <sqlite3.h>
#include <stdbool.h>
#include <stdint.h>
#include "analyzer.h"

typedef struct {
    sqlite3 *db;
} IndexDB;

IndexDB* db_open(const char *path);
void db_close(IndexDB *db);

int db_index_file(IndexDB *db, const char *path, double mtime, const char *hash, SymbolResult *sr, ImportResult *ir);
int db_invalidate_file(IndexDB *db, const char *path);
int db_clear(IndexDB *db);

int db_index_observation(IndexDB *db, const char *type, const char *content, double timestamp, int token_estimate);
void db_search_observations(IndexDB *db, const char *query, int limit, struct jsonw *w);

int db_index_critic_decision(IndexDB *db, const char *text, int turn, double confidence);
void db_search_critic_decisions(IndexDB *db, const char *query, int limit, struct jsonw *w);

int db_index_watcher_pattern(IndexDB *db, const char *text, const char *file_hash, int turn);
void db_search_watcher_patterns(IndexDB *db, const char *query, int limit, struct jsonw *w);

void db_search_symbols(IndexDB *db, const char *query, const char *kind_filter, int limit, struct jsonw *w);

#endif
