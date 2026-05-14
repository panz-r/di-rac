/// Tree-sitter query for Bash symbol extraction.
pub const SYMBOL_QUERY: &str = r#"
(function_definition
  name: (word) @func.name) @func.def
"#;

/// No imports in Bash.
pub const IMPORT_QUERY: &str = "";

/// Bash calls are just command names — no structured call expressions.
pub const CALL_QUERY: &str = "";
