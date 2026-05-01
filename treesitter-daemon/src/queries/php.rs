// Queries for PHP.
// Procedural extraction is used for symbols; these queries provide import support.

pub const SYMBOL_QUERY: &str = r#"
(function_definition name: (name) @func.name) @func.def
(method_declaration name: (name) @func.name) @func.def
(class_declaration name: (name) @class.name) @class.def
(interface_declaration name: (name) @class.name) @class.def
(trait_declaration name: (name) @class.name) @class.def
"#;

pub const IMPORT_QUERY: &str = r#"
(use_declaration (name) @module) @import
"#;
