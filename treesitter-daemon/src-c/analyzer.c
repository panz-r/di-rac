#include "analyzer.h"
#include "queries.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

ParsedSource* analyzer_parse(const char *source, Language lang) {
    const TSLanguage *ts_lang = get_ts_language(lang);
    if (!ts_lang) return NULL;

    TSParser *parser = ts_parser_new();
    ts_parser_set_language(parser, ts_lang);

    TSTree *tree = ts_parser_parse_string(parser, NULL, source, strlen(source));
    ts_parser_delete(parser);

    if (!tree) return NULL;

    ParsedSource *ps = malloc(sizeof(ParsedSource));
    ps->source = strdup(source);
    ps->lang = lang;
    ps->tree = tree;
    return ps;
}

void analyzer_free_source(ParsedSource *ps) {
    if (!ps) return;
    ts_tree_delete(ps->tree);
    free(ps->source);
    free(ps);
}

static char* get_node_text(TSNode node, const char *source) {
    uint32_t start = ts_node_start_byte(node);
    uint32_t end = ts_node_end_byte(node);
    uint32_t len = end - start;
    char *text = malloc(len + 1);
    memcpy(text, source + start, len);
    text[len] = '\0';
    return text;
}

static char* get_node_signature(TSNode node, const char *source, Language lang) {
    uint32_t start = ts_node_start_byte(node);
    uint32_t end = ts_node_end_byte(node);
    const char *snippet = source + start;
    uint32_t len = end - start;

    const char *bracket = strchr(snippet, '{');
    if (lang == LANG_PYTHON) bracket = strchr(snippet, ':');

    if (bracket && (uint32_t)(bracket - snippet) < len) {
        len = bracket - snippet;
        if (lang == LANG_PYTHON) len++;
    } else {
        const char *newline = strchr(snippet, '\n');
        if (newline && (uint32_t)(newline - snippet) < len) {
            len = newline - snippet;
        }
    }

    char *sig = malloc(len + 1);
    memcpy(sig, snippet, len);
    sig[len] = '\0';
    
    while (len > 0 && (sig[len-1] == ' ' || sig[len-1] == '\r' || sig[len-1] == '\n')) {
        sig[--len] = '\0';
    }
    return sig;
}

static char* get_c_function_name(TSNode node, const char *source) {
    TSNode decl = ts_node_child_by_field_name(node, "declarator", strlen("declarator"));
    while (!ts_node_is_null(decl)) {
        const char *kind = ts_node_type(decl);
        if (strcmp(kind, "identifier") == 0) return get_node_text(decl, source);
        TSNode inner = ts_node_child_by_field_name(decl, "declarator", strlen("declarator"));
        if (ts_node_is_null(inner)) {
            uint32_t n = ts_node_child_count(decl);
            for (uint32_t i = 0; i < n; i++) {
                TSNode c = ts_node_child(decl, i);
                if (strcmp(ts_node_type(c), "identifier") == 0) return get_node_text(c, source);
            }
            break;
        }
        decl = inner;
    }
    return NULL;
}

static void walk_collect_functions(TSNode node, const char *source, AnalyzerCtx *ctx, Symbol **symbols, size_t *count, size_t *cap) {
    const char *kind = ts_node_type(node);
    bool is_func = (strcmp(kind, "function_definition") == 0 || strcmp(kind, "method_declaration") == 0);

    if (is_func) {
        char *name = get_c_function_name(node, source);
        if (!name) name = strdup("unknown");
        if (*count == *cap) {
            *cap = *cap ? *cap * 2 : 16;
            *symbols = realloc(*symbols, sizeof(Symbol) * (*cap));
        }
        Symbol *sym = &((*symbols)[(*count)++]);
        memset(sym, 0, sizeof(Symbol));
        sym->kind = KIND_FUNCTION;
        sym->name = name;
        sym->start_line = ts_node_start_point(node).row + 1;
        sym->end_line = ts_node_end_point(node).row + 1;
        sym->start_byte = ts_node_start_byte(node);
        sym->end_byte = ts_node_end_byte(node);
        sym->signature = get_node_signature(node, source, LANG_C);
        sym->handle = malloc(strlen(name) + 10);
        sprintf(sym->handle, "fn:%s", name);
    }

    uint32_t n = ts_node_child_count(node);
    for (uint32_t i = 0; i < n; i++) {
        walk_collect_functions(ts_node_child(node, i), source, ctx, symbols, count, cap);
    }
}

static void walk_collect_classes(TSNode node, const char *source, AnalyzerCtx *ctx, Symbol **symbols, size_t *count, size_t *cap) {
    const char *kind = ts_node_type(node);
    bool is_class = (strcmp(kind, "class_declaration") == 0 || strcmp(kind, "struct_specifier") == 0);

    if (is_class) {
        TSNode name_node = ts_node_child_by_field_name(node, "name", strlen("name"));
        char *name = !ts_node_is_null(name_node) ? get_node_text(name_node, source) : strdup("unknown");
        if (*count == *cap) {
            *cap = *cap ? *cap * 2 : 16;
            *symbols = realloc(*symbols, sizeof(Symbol) * (*cap));
        }
        Symbol *sym = &((*symbols)[(*count)++]);
        memset(sym, 0, sizeof(Symbol));
        sym->kind = KIND_CLASS;
        sym->name = name;
        sym->start_line = ts_node_start_point(node).row + 1;
        sym->end_line = ts_node_end_point(node).row + 1;
        sym->start_byte = ts_node_start_byte(node);
        sym->end_byte = ts_node_end_byte(node);
        sym->signature = get_node_signature(node, source, LANG_C);
        sym->handle = malloc(strlen(name) + 10);
        sprintf(sym->handle, "class:%s", name);
    }

    uint32_t n = ts_node_child_count(node);
    for (uint32_t i = 0; i < n; i++) {
        walk_collect_classes(ts_node_child(node, i), source, ctx, symbols, count, cap);
    }
}

SymbolResult* analyzer_extract_symbols(ParsedSource *ps, AnalyzerCtx *ctx) {
    if (ps->lang == LANG_C || ps->lang == LANG_CPP || ps->lang == LANG_JAVA || ps->lang == LANG_CSHARP || ps->lang == LANG_RUBY || ps->lang == LANG_PHP) {
        Symbol *symbols = NULL;
        size_t count = 0;
        size_t cap = 0;
        walk_collect_functions(ts_tree_root_node(ps->tree), ps->source, ctx, &symbols, &count, &cap);
        walk_collect_classes(ts_tree_root_node(ps->tree), ps->source, ctx, &symbols, &count, &cap);
        SymbolResult *res = malloc(sizeof(SymbolResult));
        res->symbols = symbols;
        res->count = count;
        return res;
    }

    LanguageQueries queries = get_queries(ps->lang);
    if (!queries.symbol_query) return NULL;

    uint32_t error_offset;
    TSQueryError error_type;
    TSQuery *query = ts_query_new(get_ts_language(ps->lang), queries.symbol_query, strlen(queries.symbol_query), &error_offset, &error_type);
    if (!query) return NULL;

    TSQueryCursor *cursor = ts_query_cursor_new();
    ts_query_cursor_exec(cursor, query, ts_tree_root_node(ps->tree));

    Symbol *symbols = NULL;
    size_t count = 0;
    size_t cap = 0;

    TSQueryMatch match;
    while (ts_query_cursor_next_match(cursor, &match)) {
        Symbol sym = {0};
        bool valid = false;
        for (uint32_t i = 0; i < match.capture_count; i++) {
            TSQueryCapture cap_node = match.captures[i];
            uint32_t name_len;
            const char *cap_name = ts_query_capture_name_for_id(query, cap_node.index, &name_len);
            if (strcmp(cap_name, "func.name") == 0 || strcmp(cap_name, "class.name") == 0 || strcmp(cap_name, "method.name") == 0) {
                sym.name = get_node_text(cap_node.node, ps->source);
            } else if (strcmp(cap_name, "func.def") == 0 || strcmp(cap_name, "class.def") == 0 || strcmp(cap_name, "method.def") == 0) {
                sym.kind = (strcmp(cap_name, "class.def") == 0) ? KIND_CLASS : (strcmp(cap_name, "method.def") == 0 ? KIND_METHOD : KIND_FUNCTION);
                valid = true;
                sym.start_line = ts_node_start_point(cap_node.node).row + 1;
                sym.end_line = ts_node_end_point(cap_node.node).row + 1;
                sym.start_byte = ts_node_start_byte(cap_node.node);
                sym.end_byte = ts_node_end_byte(cap_node.node);
                sym.signature = get_node_signature(cap_node.node, ps->source, ps->lang);
            }
        }
        if (valid) {
            if (count == cap) {
                cap = cap ? cap * 2 : 16;
                symbols = realloc(symbols, sizeof(Symbol) * cap);
            }
            if (sym.name) {
                const char *prefix = (sym.kind == KIND_CLASS) ? "class" : "fn";
                sym.handle = malloc(strlen(sym.name) + 10);
                sprintf(sym.handle, "%s:%s", prefix, sym.name);
            }
            symbols[count++] = sym;
        }
    }
    ts_query_cursor_delete(cursor);
    ts_query_delete(query);
    SymbolResult *res = malloc(sizeof(SymbolResult));
    res->symbols = symbols;
    res->count = count;
    return res;
}

ImportResult* analyzer_extract_imports(ParsedSource *ps, AnalyzerCtx *ctx) {
    LanguageQueries queries = get_queries(ps->lang);
    if (!queries.import_query || strlen(queries.import_query) == 0) return NULL;

    uint32_t error_offset;
    TSQueryError error_type;
    TSQuery *query = ts_query_new(get_ts_language(ps->lang), queries.import_query, strlen(queries.import_query), &error_offset, &error_type);
    if (!query) return NULL;

    TSQueryCursor *cursor = ts_query_cursor_new();
    ts_query_cursor_exec(cursor, query, ts_tree_root_node(ps->tree));

    Import *imports = NULL;
    size_t count = 0;
    size_t cap = 0;

    TSQueryMatch match;
    while (ts_query_cursor_next_match(cursor, &match)) {
        Import imp = {0};
        bool valid = false;
        for (uint32_t i = 0; i < match.capture_count; i++) {
            TSQueryCapture cap_node = match.captures[i];
            uint32_t name_len;
            const char *cap_name = ts_query_capture_name_for_id(query, cap_node.index, &name_len);
            if (strcmp(cap_name, "module") == 0) {
                char *raw = get_node_text(cap_node.node, ps->source);
                if (raw[0] == '"' || raw[0] == '\'') {
                    size_t rl = strlen(raw);
                    imp.module = malloc(rl);
                    strncpy(imp.module, raw + 1, rl - 2);
                    imp.module[rl - 2] = '\0';
                    free(raw);
                } else {
                    imp.module = raw;
                }
                valid = true;
                imp.line = ts_node_start_point(cap_node.node).row + 1;
            } else if (strcmp(cap_name, "name") == 0 || strcmp(cap_name, "default_import") == 0) {
                imp.names = realloc(imp.names, sizeof(char*) * (imp.names_count + 1));
                imp.names[imp.names_count++] = get_node_text(cap_node.node, ps->source);
            }
        }
        if (valid) {
            if (count == cap) {
                cap = cap ? cap * 2 : 16;
                imports = realloc(imports, sizeof(Import) * cap);
            }
            imports[count++] = imp;
        }
    }
    ts_query_cursor_delete(cursor);
    ts_query_delete(query);
    ImportResult *res = malloc(sizeof(ImportResult));
    res->imports = imports;
    res->count = count;
    return res;
}

void analyzer_free_symbols(SymbolResult *sr) {
    if (!sr) return;
    for (size_t i = 0; i < sr->count; i++) {
        free(sr->symbols[i].name);
        free(sr->symbols[i].handle);
        free(sr->symbols[i].signature);
    }
    free(sr->symbols);
    free(sr);
}

void analyzer_free_imports(ImportResult *ir) {
    if (!ir) return;
    for (size_t i = 0; i < ir->count; i++) {
        free(ir->imports[i].module);
        for (size_t j = 0; j < ir->imports[i].names_count; j++) free(ir->imports[i].names[j]);
        free(ir->imports[i].names);
    }
    free(ir->imports);
    free(ir);
}

ApiDependencies* analyzer_extract_apis(ParsedSource *ps) {
    LanguageQueries queries = get_queries(ps->lang);
    if (!queries.symbol_query) return NULL;

    uint32_t error_offset;
    TSQueryError error_type;
    TSQuery *query = ts_query_new(get_ts_language(ps->lang), queries.symbol_query, strlen(queries.symbol_query), &error_offset, &error_type);
    if (!query) return NULL;

    TSQueryCursor *cursor = ts_query_cursor_new();
    ts_query_cursor_exec(cursor, query, ts_tree_root_node(ps->tree));

    ApiDependencies *ad = calloc(1, sizeof(ApiDependencies));
    TSQueryMatch match;
    while (ts_query_cursor_next_match(cursor, &match)) {
        for (uint32_t i = 0; i < match.capture_count; i++) {
            TSQueryCapture cap = match.captures[i];
            uint32_t name_len;
            const char *cap_name = ts_query_capture_name_for_id(query, cap.index, &name_len);
            if (strcmp(cap_name, "func.name") == 0 || strcmp(cap_name, "method.name") == 0) {
                ad->definitions = realloc(ad->definitions, sizeof(char*) * (ad->definitions_count + 1));
                ad->definitions[ad->definitions_count++] = get_node_text(cap.node, ps->source);
            } else if (strcmp(cap_name, "call.name") == 0) {
                ad->calls = realloc(ad->calls, sizeof(char*) * (ad->calls_count + 1));
                ad->calls[ad->calls_count++] = get_node_text(cap.node, ps->source);
            }
        }
    }
    ts_query_cursor_delete(cursor);
    ts_query_delete(query);
    return ad;
}

void analyzer_free_apis(ApiDependencies *ad) {
    if (!ad) return;
    for (size_t i = 0; i < ad->calls_count; i++) free(ad->calls[i]);
    free(ad->calls);
    for (size_t i = 0; i < ad->definitions_count; i++) free(ad->definitions[i]);
    free(ad->definitions);
    free(ad);
}

const char* symbol_kind_to_str(SymbolKind kind) {
    switch (kind) {
        case KIND_FUNCTION: return "function";
        case KIND_CLASS:    return "class";
        case KIND_METHOD:   return "method";
        case KIND_VARIABLE: return "variable";
        case KIND_INTERFACE: return "interface";
        case KIND_MODULE: return "module";
        default:            return "unknown";
    }
}

static uint32_t count_nodes(TSNode node) {
    uint32_t count = 1;
    uint32_t n = ts_node_child_count(node);
    for (uint32_t i = 0; i < n; i++) {
        count += count_nodes(ts_node_child(node, i));
    }
    return count;
}

void analyzer_ast_churn(const char *file_path, const char *new_content, struct jsonw *w) {
    Language lang = LANG_UNKNOWN;
    const char *ext = strrchr(file_path, '.');
    if (ext) lang = parse_language_name(ext + 1);

    ParsedSource *ps_new = analyzer_parse(new_content, lang);
    if (!ps_new) {
        jsonw_kv_int(w, "added", 0); jsonw_kv_int(w, "removed", 0); jsonw_kv_int(w, "total", 0);
        return;
    }

    uint32_t new_count = count_nodes(ts_tree_root_node(ps_new->tree));

    FILE *f = fopen(file_path, "r");
    if (!f) {
        jsonw_kv_int(w, "added", (int)new_count);
        jsonw_kv_int(w, "removed", 0);
        jsonw_kv_int(w, "total", (int)new_count);
        analyzer_free_source(ps_new);
        return;
    }

    fseek(f, 0, SEEK_END);
    long fsize = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *old_content = malloc(fsize + 1);
    if (fread(old_content, 1, fsize, f) != (size_t)fsize) {
        free(old_content); fclose(f);
        jsonw_kv_int(w, "added", (int)new_count); jsonw_kv_int(w, "removed", 0); jsonw_kv_int(w, "total", (int)new_count);
        analyzer_free_source(ps_new);
        return;
    }
    old_content[fsize] = '\0';
    fclose(f);

    ParsedSource *ps_old = analyzer_parse(old_content, lang);
    free(old_content);

    if (!ps_old) {
        jsonw_kv_int(w, "added", (int)new_count); jsonw_kv_int(w, "removed", 0); jsonw_kv_int(w, "total", (int)new_count);
        analyzer_free_source(ps_new);
        return;
    }

    uint32_t old_count = count_nodes(ts_tree_root_node(ps_old->tree));

    int delta = (int)new_count - (int)old_count;
    jsonw_kv_int(w, "added", delta > 0 ? delta : 0);
    jsonw_kv_int(w, "removed", delta < 0 ? -delta : 0);
    jsonw_kv_int(w, "total", (int)new_count);

    analyzer_free_source(ps_new);
    analyzer_free_source(ps_old);
}
