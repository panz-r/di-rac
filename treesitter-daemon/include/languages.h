#ifndef LANGUAGES_H
#define LANGUAGES_H

#include <tree_sitter/api.h>

typedef enum {
    LANG_PYTHON,
    LANG_TYPESCRIPT,
    LANG_JAVASCRIPT,
    LANG_C,
    LANG_CPP,
    LANG_RUST,
    LANG_GO,
    LANG_BASH,
    LANG_JAVA,
    LANG_CSHARP,
    LANG_RUBY,
    LANG_PHP,
    LANG_UNKNOWN
} Language;

/* Grammar declarations (from bundled libs) */
extern const TSLanguage *tree_sitter_python(void);
extern const TSLanguage *tree_sitter_typescript(void);
extern const TSLanguage *tree_sitter_c(void);
extern const TSLanguage *tree_sitter_cpp(void);
extern const TSLanguage *tree_sitter_rust(void);
extern const TSLanguage *tree_sitter_go(void);
extern const TSLanguage *tree_sitter_bash(void);
extern const TSLanguage *tree_sitter_java(void);
extern const TSLanguage *tree_sitter_c_sharp(void);
extern const TSLanguage *tree_sitter_ruby(void);
extern const TSLanguage *tree_sitter_php(void);

const TSLanguage* get_ts_language(Language lang);
Language parse_language_name(const char *name);
const char* get_language_name(Language lang);

#endif
