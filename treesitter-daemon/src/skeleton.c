#include "analyzer.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int append_to_skeleton(char **skel, size_t *len, size_t *cap, const char *str) {
    size_t str_len = strlen(str);
    if (*len + str_len >= *cap) {
        size_t new_cap = (*cap + str_len) * 2;
        void *tmp = realloc(*skel, new_cap);
        if (!tmp) return -1; /* OOM — partial result preserved, caller notified */
        *skel = tmp;
        *cap = new_cap;
    }
    memcpy(*skel + *len, str, str_len);
    *len += str_len;
    (*skel)[*len] = '\0';
    return 0;
}

char* analyzer_generate_skeleton(ParsedSource *ps) {
    SymbolResult *sr = analyzer_extract_symbols(ps, NULL);
    if (!sr) return strdup("");

    size_t cap = 4096;
    size_t len = 0;
    char *skel = malloc(cap);
    if (!skel) {
        analyzer_free_symbols(sr);
        return NULL;
    }
    skel[0] = '\0';

    for (size_t i = 0; i < sr->count; i++) {
        const char *sig = sr->symbols[i].signature ? sr->symbols[i].signature : "???";
        if (append_to_skeleton(&skel, &len, &cap, sig) < 0) break;
        if (append_to_skeleton(&skel, &len, &cap, "\n") < 0) break;
    }

    analyzer_free_symbols(sr);
    return skel;
}
