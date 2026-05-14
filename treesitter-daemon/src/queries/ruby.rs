// Queries for Ruby.
// Procedural extraction is used for symbols; these queries provide import support.

pub const SYMBOL_QUERY: &str = r#"
(method name: (identifier) @func.name) @func.def
(singleton_method name: (identifier) @func.name) @func.def
(class name: (constant) @class.name) @class.def
(module name: (constant) @class.name) @class.def
"#;

pub const IMPORT_QUERY: &str = r#"
(call method: (identifier) @import)
"#;

/// Ruby call extraction.
pub const CALL_QUERY: &str = r#"
(call
  method: (identifier) @call.name) @call
"#;
