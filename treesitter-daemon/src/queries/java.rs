// Queries for Java.
// Procedural extraction is used for symbols; these queries provide import support.

pub const SYMBOL_QUERY: &str = r#"
(function_declaration) @func.def
(method_declaration name: (identifier) @func.name) @func.def
(constructor_declaration name: (identifier) @func.name) @func.def
(class_declaration name: (identifier) @class.name) @class.def
(interface_declaration name: (identifier) @class.name) @class.def
"#;

pub const IMPORT_QUERY: &str = r#"
(import_declaration
  (scoped_identifier) @module) @import
"#;

/// Java method invocation extraction.
pub const CALL_QUERY: &str = r#"
(method_invocation
  name: (identifier) @call.name) @call
"#;
