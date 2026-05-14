/// Tree-sitter query for TypeScript/JavaScript symbol extraction.
pub const SYMBOL_QUERY: &str = r#"
(function_declaration
  name: (identifier) @func.name) @func.def

(generator_function_declaration
  name: (identifier) @func.name) @func.def

(class_declaration
  name: (type_identifier) @class.name) @class.def

(method_definition
  name: (property_identifier) @method.name) @method.def
"#;

/// Tree-sitter query for TypeScript/JavaScript import extraction.
pub const IMPORT_QUERY: &str = r#"
(import_statement
  (import_clause
    (named_imports
      (import_specifier
        (identifier) @name)))
  (string) @module) @import

(import_statement
  (import_clause
    (identifier) @default_import)
  (string) @module) @import

(import_statement
  (import_clause
    (namespace_import
      (identifier) @name))
  (string) @module) @import
"#;

/// Tree-sitter query for TypeScript/JavaScript call extraction.
pub const CALL_QUERY: &str = r#"
(call_expression
  function: (identifier) @call.name) @call

(call_expression
  function: (member_expression
    property: (property_identifier) @call.name)) @call
"#;
