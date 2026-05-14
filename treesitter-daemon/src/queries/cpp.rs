/// Tree-sitter query for C++ symbol extraction.
pub const SYMBOL_QUERY: &str = r#"
(function_definition
  (function_declarator
    (identifier) @func.name)) @func.def

(function_definition
  (pointer_declarator
    (function_declarator
      (identifier) @func.name))) @func.def

(function_definition
  (reference_declarator
    (function_declarator
      (identifier) @func.name))) @func.def

(class_specifier
  (type_identifier) @class.name) @class.def

(struct_specifier
  (type_identifier) @class.name) @class.def
"#;

/// Tree-sitter query for C++ import extraction.
pub const IMPORT_QUERY: &str = r#"
(preproc_include
  (string) @module) @import
"#;

/// C++ call extraction via query — matches function calls.
pub const CALL_QUERY: &str = r#"
(call_expression
  function: (identifier) @call.name) @call

(call_expression
  function: (field_identifier) @call.name) @call
"#;
