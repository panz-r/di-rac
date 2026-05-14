/// Tree-sitter query for Python symbol extraction.
/// Captures:
///   @func.def    – the function_definition node
///   @func.name   – the function name (identifier)
///   @func.params – the parameters node
///   @class.def   – the class_definition node
///   @class.name  – the class name (identifier)
///   @class.super – superclass argument_list if present
pub const SYMBOL_QUERY: &str = r#"
(function_definition
  name: (identifier) @func.name
  parameters: (parameters) @func.params) @func.def

(class_definition
  name: (identifier) @class.name
  superclasses: (argument_list)? @class.super) @class.def
"#;

/// Tree-sitter query for Python import extraction.
pub const IMPORT_QUERY: &str = r#"
(import_statement
  name: (dotted_name) @module) @import

(import_from_statement
  module_name: (dotted_name) @module
  name: (dotted_name) @name) @import_from

(import_from_statement
  module_name: (dotted_name) @module
  name: (aliased_import) @alias) @import_from
"#;

/// Tree-sitter query for Python call extraction.
pub const CALL_QUERY: &str = r#"
(call
  function: (identifier) @call.name) @call

(call
  function: (attribute
    attribute: (identifier) @call.name)) @call
"#;
