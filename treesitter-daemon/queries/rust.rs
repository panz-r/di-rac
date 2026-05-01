/// Tree-sitter query for Rust symbol extraction.
pub const SYMBOL_QUERY: &str = r#"
(function_item
  name: (identifier) @func.name) @func.def

(struct_item
  name: (type_identifier) @class.name) @class.def

(enum_item
  name: (type_identifier) @class.name) @class.def

(trait_item
  name: (type_identifier) @class.name) @class.def

(impl_item
  type: (type_identifier) @class.name) @class.def
"#;

/// Tree-sitter query for Rust import extraction (use).
pub const IMPORT_QUERY: &str = r#"
(use_declaration
  argument: (scoped_identifier) @module) @import
"#;
