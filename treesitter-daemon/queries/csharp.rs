// Queries for C#.
// Procedural extraction is used for symbols; these queries provide import support.

pub const SYMBOL_QUERY: &str = r#"
(method_declaration name: (identifier) @func.name) @func.def
(class_declaration name: (identifier) @class.name) @class.def
(interface_declaration name: (identifier) @class.name) @class.def
(struct_declaration name: (identifier) @class.name) @class.def
"#;

pub const IMPORT_QUERY: &str = r#"
(using_directive (identifier) @module) @import
"#;
