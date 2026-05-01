/// Tree-sitter query for Go symbol extraction.
pub const SYMBOL_QUERY: &str = r#"
(function_declaration
  name: (identifier) @func.name) @func.def

(method_declaration
  name: (field_identifier) @method.name) @method.def

(type_declaration
  (type_spec
    name: (type_identifier) @class.name
    type: (struct_type)) @class.def)

(type_declaration
  (type_spec
    name: (type_identifier) @class.name
    type: (interface_type)) @class.def)
"#;

/// Tree-sitter query for Go import extraction.
pub const IMPORT_QUERY: &str = r#"
(import_declaration
  (import_spec_list
    (import_spec
      path: (interpreted_string_literal) @module))) @import

(import_declaration
  (import_spec
    path: (interpreted_string_literal) @module)) @import
"#;
