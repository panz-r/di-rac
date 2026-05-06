#include "languages.h"
#include <string.h>

const TSLanguage* get_ts_language(Language lang) {
    switch (lang) {
        case LANG_PYTHON:     return tree_sitter_python();
        case LANG_TYPESCRIPT: return tree_sitter_typescript();
        case LANG_JAVASCRIPT: return tree_sitter_typescript();
        case LANG_C:          return tree_sitter_c();
        case LANG_CPP:        return tree_sitter_cpp();
        case LANG_RUST:       return tree_sitter_rust();
        case LANG_GO:         return tree_sitter_go();
        case LANG_BASH:       return tree_sitter_bash();
        case LANG_JAVA:       return tree_sitter_java();
        case LANG_CSHARP:     return tree_sitter_c_sharp();
        case LANG_RUBY:       return tree_sitter_ruby();
        case LANG_PHP:        return tree_sitter_php();
        default:              return NULL;
    }
}

Language parse_language_name(const char *name) {
    if (!name) return LANG_UNKNOWN;
    if (strcmp(name, "python") == 0) return LANG_PYTHON;
    if (strcmp(name, "typescript") == 0) return LANG_TYPESCRIPT;
    if (strcmp(name, "javascript") == 0) return LANG_JAVASCRIPT;
    if (strcmp(name, "c") == 0) return LANG_C;
    if (strcmp(name, "cpp") == 0) return LANG_CPP;
    if (strcmp(name, "rust") == 0) return LANG_RUST;
    if (strcmp(name, "go") == 0) return LANG_GO;
    if (strcmp(name, "bash") == 0) return LANG_BASH;
    if (strcmp(name, "java") == 0) return LANG_JAVA;
    if (strcmp(name, "csharp") == 0) return LANG_CSHARP;
    if (strcmp(name, "ruby") == 0) return LANG_RUBY;
    if (strcmp(name, "php") == 0) return LANG_PHP;
    return LANG_UNKNOWN;
}

const char* get_language_name(Language lang) {
    switch (lang) {
        case LANG_PYTHON:     return "python";
        case LANG_TYPESCRIPT: return "typescript";
        case LANG_JAVASCRIPT: return "javascript";
        case LANG_C:          return "c";
        case LANG_CPP:        return "cpp";
        case LANG_RUST:       return "rust";
        case LANG_GO:         return "go";
        case LANG_BASH:       return "bash";
        case LANG_JAVA:       return "java";
        case LANG_CSHARP:     return "csharp";
        case LANG_RUBY:       return "ruby";
        case LANG_PHP:        return "php";
        default:              return "unknown";
    }
}
