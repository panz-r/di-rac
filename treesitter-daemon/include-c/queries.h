#ifndef QUERIES_H
#define ANALYZER_H

#include "languages.h"

typedef struct {
    const char *symbol_query;
    const char *import_query;
} LanguageQueries;

LanguageQueries get_queries(Language lang);

#endif
