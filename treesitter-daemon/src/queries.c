#include "queries.h"
#include <stddef.h>

static const char *BASH_SYMBOL = "(function_definition name: (word) @func.name) @func.def";
static const char *BASH_IMPORT = "";

static const char *CPP_SYMBOL = 
"(function_definition (function_declarator (identifier) @func.name)) @func.def "
"(function_definition (pointer_declarator (function_declarator (identifier) @func.name))) @func.def "
"(function_definition (reference_declarator (function_declarator (identifier) @func.name))) @func.def "
"(class_specifier (type_identifier) @class.name) @class.def "
"(struct_specifier (type_identifier) @class.name) @class.def";
static const char *CPP_IMPORT = "(preproc_include (string) @module) @import";

static const char *C_SYMBOL = NULL;  /* C uses manual AST walk, not queries */
static const char *C_IMPORT = "";

static const char *CSHARP_SYMBOL = 
"(method_declaration name: (identifier) @func.name) @func.def "
"(class_declaration name: (identifier) @class.name) @class.def "
"(interface_declaration name: (identifier) @class.name) @class.def "
"(struct_declaration name: (identifier) @class.name) @class.def";
static const char *CSHARP_IMPORT = "(using_directive (identifier) @module) @import";

static const char *GO_SYMBOL = 
"(function_declaration name: (identifier) @func.name) @func.def "
"(method_declaration name: (field_identifier) @method.name) @method.def "
"(type_declaration (type_spec name: (type_identifier) @class.name type: (struct_type)) @class.def) "
"(type_declaration (type_spec name: (type_identifier) @class.name type: (interface_type)) @class.def)";
static const char *GO_IMPORT = 
"(import_declaration (import_spec_list (import_spec path: (interpreted_string_literal) @module))) @import "
"(import_declaration (import_spec path: (interpreted_string_literal) @module)) @import";

static const char *JAVA_SYMBOL = 
"(function_declaration) @func.def "
"(method_declaration name: (identifier) @func.name) @func.def "
"(constructor_declaration name: (identifier) @func.name) @func.def "
"(class_declaration name: (identifier) @class.name) @class.def "
"(interface_declaration name: (identifier) @class.name) @class.def";
static const char *JAVA_IMPORT = "(import_declaration (scoped_identifier) @module) @import";

static const char *PHP_SYMBOL = 
"(function_definition name: (name) @func.name) @func.def "
"(method_declaration name: (name) @func.name) @func.def "
"(class_declaration name: (name) @class.name) @class.def "
"(interface_declaration name: (name) @class.name) @class.def "
"(trait_declaration name: (name) @class.name) @class.def";
static const char *PHP_IMPORT = "(use_declaration (name) @module) @import";

static const char *PYTHON_SYMBOL = 
"(function_definition name: (identifier) @func.name parameters: (parameters) @func.params) @func.def "
"(class_definition name: (identifier) @class.name superclasses: (argument_list)? @class.super) @class.def";
static const char *PYTHON_IMPORT = 
"(import_statement name: (dotted_name) @module) @import "
"(import_from_statement module_name: (dotted_name) @module name: (dotted_name) @name) @import_from "
"(import_from_statement module_name: (dotted_name) @module name: (aliased_import) @alias) @import_from";

static const char *RUBY_SYMBOL = 
"(method name: (identifier) @func.name) @func.def "
"(singleton_method name: (identifier) @func.name) @func.def "
"(class name: (constant) @class.name) @class.def "
"(module name: (constant) @class.name) @class.def";
static const char *RUBY_IMPORT = "(call method: (identifier) @import)";

static const char *RUST_SYMBOL = 
"(function_item name: (identifier) @func.name) @func.def "
"(struct_item name: (type_identifier) @class.name) @class.def "
"(enum_item name: (type_identifier) @class.name) @class.def "
"(trait_item name: (type_identifier) @class.name) @class.def "
"(impl_item type: (type_identifier) @class.name) @class.def";
static const char *RUST_IMPORT = "(use_declaration argument: (scoped_identifier) @module) @import";

static const char *TYPESCRIPT_SYMBOL = 
"(function_declaration name: (identifier) @func.name) @func.def "
"(generator_function_declaration name: (identifier) @func.name) @func.def "
"(class_declaration name: (type_identifier) @class.name) @class.def "
"(method_definition name: (property_identifier) @method.name) @method.def";
static const char *TYPESCRIPT_IMPORT = 
"(import_statement (import_clause (named_imports (import_specifier (identifier) @name))) (string) @module) @import "
"(import_statement (import_clause (identifier) @default_import) (string) @module) @import "
"(import_statement (import_clause (namespace_import (identifier) @name)) (string) @module) @import";

LanguageQueries get_queries(Language lang) {
    switch (lang) {
        case LANG_PYTHON:     return (LanguageQueries){PYTHON_SYMBOL, PYTHON_IMPORT};
        case LANG_TYPESCRIPT: 
        case LANG_JAVASCRIPT: return (LanguageQueries){TYPESCRIPT_SYMBOL, TYPESCRIPT_IMPORT};
        case LANG_C:          return (LanguageQueries){C_SYMBOL, C_IMPORT};
        case LANG_CPP:        return (LanguageQueries){CPP_SYMBOL, CPP_IMPORT};
        case LANG_RUST:       return (LanguageQueries){RUST_SYMBOL, RUST_IMPORT};
        case LANG_GO:         return (LanguageQueries){GO_SYMBOL, GO_IMPORT};
        case LANG_BASH:       return (LanguageQueries){BASH_SYMBOL, BASH_IMPORT};
        case LANG_JAVA:       return (LanguageQueries){JAVA_SYMBOL, JAVA_IMPORT};
        case LANG_CSHARP:     return (LanguageQueries){CSHARP_SYMBOL, CSHARP_IMPORT};
        case LANG_RUBY:       return (LanguageQueries){RUBY_SYMBOL, RUBY_IMPORT};
        case LANG_PHP:        return (LanguageQueries){PHP_SYMBOL, PHP_IMPORT};
        default:              return (LanguageQueries){NULL, NULL};
    }
}
