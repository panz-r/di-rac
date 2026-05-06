#ifndef ANALYZER_H
#define ANALYZER_H

#include <stdint.h>
#include <stdbool.h>
#include <tree_sitter/api.h>
#include "languages.h"
#include "json-write.h"

/* --- Core Types --- */

typedef enum {
    KIND_FUNCTION,
    KIND_CLASS,
    KIND_METHOD,
    KIND_VARIABLE,
    KIND_INTERFACE,
    KIND_MODULE,
    KIND_UNKNOWN
} SymbolKind;

typedef struct {
    char *name;
    SymbolKind kind;
    char *handle;
    uint32_t start_line;
    uint32_t end_line;
    uint32_t start_byte;
    uint32_t end_byte;
    char *signature;
} Symbol;

typedef struct {
    Symbol *symbols;
    size_t count;
} SymbolResult;

typedef struct {
    char *module;
    char **names;
    size_t names_count;
    uint32_t line;
} Import;

typedef struct {
    Import *imports;
    size_t count;
} ImportResult;

typedef struct {
    char *source;
    Language lang;
    TSTree *tree;
} ParsedSource;

typedef struct {
    char workspace_root[4096];
    bool oneshot;
    void *db; /* Pointer to IndexDB (opaque here to avoid sqlite3.h in header) */
} AnalyzerCtx;

/* --- Core Analysis Functions --- */

ParsedSource* analyzer_parse(const char *source, Language lang);
void analyzer_free_source(ParsedSource *ps);

SymbolResult* analyzer_extract_symbols(ParsedSource *ps, AnalyzerCtx *ctx);
void analyzer_free_symbols(SymbolResult *sr);

ImportResult* analyzer_extract_imports(ParsedSource *ps, AnalyzerCtx *ctx);
void analyzer_free_imports(ImportResult *ir);

char* analyzer_generate_skeleton(ParsedSource *ps);
void analyzer_repo_map(const char *root, struct jsonw *w);
void analyzer_search_symbols(AnalyzerCtx *ctx, const char *query, const char *kind_filter, int limit, struct jsonw *w);

const char* symbol_kind_to_str(SymbolKind kind);

#endif
