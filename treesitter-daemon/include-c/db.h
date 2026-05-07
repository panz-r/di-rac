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

void db_search_symbols(IndexDB *db, const char *query, const char *kind_filter, int limit, struct jsonw *w);

#endif
