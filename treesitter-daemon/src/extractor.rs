use crate::language::Language;
use crate::queries::get_queries;
use serde::Serialize;
use tree_sitter::{Node, Query, QueryCursor, Tree};

/// A discovered symbol (function, class, method, variable).
#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub handle: String,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

// ── Procedural extraction for C/C++ ──────────────────────────────────────────────
// Tree-sitter queries are unreliable for the C and C++ grammars (possibly due to
// an ABI mismatch between tree-sitter-language v0.1 and tree-sitter v0.23).
// Instead, we walk the AST procedurally to extract symbols and imports.

/// Extract symbols by walking the AST directly (for C/C++).
fn extract_symbols_procedural(source: &str, tree: &Tree, language: Language) -> Vec<Symbol> {
    let root = tree.root_node();
    let mut symbols: Vec<Symbol> = Vec::new();

    // First pass: collect class/struct definitions (byte ranges) for parent detection.
    let mut class_ranges: Vec<(usize, usize, String)> = Vec::new();

    walk_collect_classes(root, source, &mut class_ranges, language);

    // Second pass: collect function definitions, assigning parents.
    walk_collect_functions(root, source, &mut symbols, &class_ranges, language);

    // Collect class/struct definitions themselves.
    walk_collect_classes_as_symbols(root, source, &mut symbols, language);

    symbols
}

/// Check if a class/struct/union specifier has a definition body (not just a type reference).
fn has_class_body(node: Node) -> bool {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if matches!(child.kind(), "field_declaration_list" | "enumerator_list" | "base_class_clause") {
                return true;
            }
        }
    }
    false
}

fn walk_collect_classes(
    node: Node,
    source: &str,
    ranges: &mut Vec<(usize, usize, String)>,
    language: Language,
) {
    let kind = node.kind();
    let is_class = match language {
        Language::C => matches!(kind, "struct_specifier" | "union_specifier"),
        Language::Cpp => matches!(kind, "class_specifier" | "struct_specifier" | "union_specifier"),
        Language::Java => matches!(kind, "class_declaration" | "interface_declaration" | "enum_declaration"),
        Language::CSharp => matches!(kind, "class_declaration" | "interface_declaration" | "struct_declaration" | "enum_declaration"),
        Language::Ruby => matches!(kind, "class" | "module"),
        Language::Php => matches!(kind, "class_declaration" | "interface_declaration" | "trait_declaration" | "enum_declaration"),
        _ => false,
    };

    if is_class && has_class_body(node) {
        if let Some(name) = child_text_by_field(node, "name", source) {
            ranges.push((node.start_byte(), node.end_byte(), name.to_string()));
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            walk_collect_classes(child, source, ranges, language);
        }
    }
}

/// Walk the AST and collect function definitions.
fn walk_collect_functions(
    node: Node,
    source: &str,
    symbols: &mut Vec<Symbol>,
    class_ranges: &[(usize, usize, String)],
    language: Language,
) {
    let is_func = match language {
        Language::C | Language::Cpp => node.kind() == "function_definition",
        Language::Java => matches!(node.kind(), "method_declaration" | "constructor_declaration"),
        Language::CSharp => matches!(node.kind(), "method_declaration" | "constructor_declaration" | "local_function_statement"),
        Language::Ruby => matches!(node.kind(), "method" | "singleton_method"),
        Language::Php => matches!(node.kind(), "function_definition" | "method_declaration"),
        _ => node.kind() == "function_definition",
    };
    if is_func {
        if let Some(name) = get_c_function_name(node, source) {
            let class_name = find_parent_class(node, class_ranges);
            let handle = if let Some(ref cn) = class_name {
                format!("fn:{}.{}", cn, name)
            } else {
                format!("fn:{}", name)
            };
            let kind = if class_name.is_some() {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            let parent = class_name.map(|cn| format!("class:{}", cn));
            symbols.push(Symbol {
                name: name.to_string(),
                kind,
                handle,
                start_line: node.start_position().row + 1,
                end_line: node.end_position().row + 1,
                signature: Some(node_signature(node, source, language)),
                parent,
            });
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            walk_collect_functions(child, source, symbols, class_ranges, language);
        }
    }
}

fn walk_collect_classes_as_symbols(
    node: Node,
    source: &str,
    symbols: &mut Vec<Symbol>,
    language: Language,
) {
    let kind = node.kind();
    let is_class = match language {
        Language::C => matches!(kind, "struct_specifier" | "union_specifier"),
        Language::Cpp => matches!(kind, "class_specifier" | "struct_specifier" | "union_specifier"),
        Language::Java => matches!(kind, "class_declaration" | "interface_declaration" | "enum_declaration"),
        Language::CSharp => matches!(kind, "class_declaration" | "interface_declaration" | "struct_declaration" | "enum_declaration"),
        Language::Ruby => matches!(kind, "class" | "module"),
        Language::Php => matches!(kind, "class_declaration" | "interface_declaration" | "trait_declaration" | "enum_declaration"),
        _ => false,
    };

    if is_class && has_class_body(node) {
        if let Some(name) = child_text_by_field(node, "name", source) {
            symbols.push(Symbol {
                name: name.to_string(),
                kind: SymbolKind::Class,
                handle: format!("class:{}", name),
                start_line: node.start_position().row + 1,
                end_line: node.end_position().row + 1,
                signature: Some(node_signature(node, source, language)),
                parent: None,
            });
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            walk_collect_classes_as_symbols(child, source, symbols, language);
        }
    }
}

/// Extract imports procedurally for C/C++ (preprocessor includes).
fn extract_imports_procedural(source: &str, tree: &Tree) -> Vec<Import> {
    let root = tree.root_node();
    let mut imports: Vec<Import> = Vec::new();
    walk_collect_includes(root, source, &mut imports);
    imports
}

fn walk_collect_includes(node: Node, source: &str, imports: &mut Vec<Import>) {
    if node.kind() == "preproc_include" {
        // The include path is in a child string node. Look for it.
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                let kind = child.kind();
                if kind.contains("string") || kind.contains("path") {
                    if let Ok(text) = child.utf8_text(source.as_bytes()) {
                        let module = text.trim_matches('"').trim_matches('<').trim_matches('>').to_string();
                        if !module.is_empty() {
                            let line = node.start_position().row + 1;
                            if !imports.iter().any(|i: &Import| i.module == module && i.line == line) {
                                imports.push(Import {
                                    module,
                                    names: Vec::new(),
                                    line,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            walk_collect_includes(child, source, imports);
        }
    }
}

/// Get the text of the first child node matching one of the given kinds.
fn get_c_function_name<'a>(node: Node<'a>, source: &'a str) -> Option<&'a str> {
    // For Java/C#/Ruby/PHP, use the "name" field which is standard.
    if let Some(name) = child_text_by_field(node, "name", source) {
        return Some(name);
    }
    // For Ruby, sometimes the method name is in a "method" field
    if let Some(name) = child_text_by_field(node, "method", source) {
        return Some(name);
    }
    // Fallback: look for identifier/constant child
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            let ck = child.kind();
            if ck == "identifier" || ck == "constant" || ck == "name" {
                return child.utf8_text(source.as_bytes()).ok();
            }
        }
    }
    // C-specific logic below
    // Direct function_declarator child
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            match child.kind() {
                "function_declarator" => {
                    return child_text_by_field(child, "declarator", source)
                        .or_else(|| child_text_by_field(child, "name", source));
                }
                "pointer_declarator" => {
                    // int *foo(...)
                    if let Some(fd) = child.child_by_field_name("declarator").or_else(|| {
                        // Look for function_declarator child
                        (0..child.child_count())
                            .find_map(|j| child.child(j))
                            .filter(|c| c.kind() == "function_declarator")
                    }) {
                        if let Some(id) = child_text_by_field(fd, "declarator", source) {
                            return Some(id);
                        }
                    }
                }
                "reference_declarator" => {
                    // int &foo(...)
                    for j in 0..child.child_count() {
                        if let Some(inner) = child.child(j) {
                            if inner.kind() == "function_declarator" {
                                if let Some(id) = child_text_by_field(inner, "declarator", source) {
                                    return Some(id);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Function,
    Class,
    Method,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolKind::Function => write!(f, "function"),
            SymbolKind::Class => write!(f, "class"),
            SymbolKind::Method => write!(f, "method"),
        }
    }
}

/// A discovered import statement.
#[derive(Debug, Clone, Serialize)]
pub struct Import {
    pub module: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    pub line: usize,
}

/// Extract all symbols from a parsed source.
pub fn extract_symbols(
    source: &str,
    tree: &Tree,
    language: Language,
) -> Vec<Symbol> {
    // C and C++ use procedural extraction because tree-sitter queries
    // are not reliably supported for these grammars.
    if matches!(language, Language::C | Language::Cpp | Language::Java | Language::CSharp | Language::Ruby | Language::Php) {
        return extract_symbols_procedural(source, tree, language);
    }

    let queries = get_queries(language);
    let lang = language.tree_sitter_language();
    let query = match Query::new(&lang, queries.symbol_query) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };

    let mut cursor = QueryCursor::new();
    let root = tree.root_node();
    let source_bytes = source.as_bytes();

    // First pass: collect class definitions so we can assign parents.
    // We pair class.def with class.name captures from the same match.
    let mut class_ranges: Vec<(usize, usize, String)> = Vec::new();

    for match_ in cursor.matches(&query, root, source_bytes) {
        let mut class_def_node: Option<Node> = None;
        let mut class_name_str: Option<&str> = None;

        for capture in match_.captures {
            let capture_name = &query.capture_names()[capture.index as usize];
            match *capture_name {
                "class.def" => class_def_node = Some(capture.node),
                "class.name" => class_name_str = capture.node.utf8_text(source_bytes).ok(),
                _ => {}
            }
        }

        if let (Some(node), Some(name)) = (class_def_node, class_name_str) {
            class_ranges.push((node.start_byte(), node.end_byte(), name.to_string()));
        }
    }

    // Second pass: collect all symbols
    let mut cursor = QueryCursor::new();
    let mut symbols: Vec<Symbol> = Vec::new();

    for match_ in cursor.matches(&query, root, source_bytes) {
        let mut class_def_node: Option<Node> = None;
        let mut class_name_str: Option<&str> = None;

        for capture in match_.captures {
            let capture_name = &query.capture_names()[capture.index as usize];
            let node = capture.node;

            match *capture_name {
                "func.def" => {
                    if let Some(name) = child_text_by_field(node, "name", source) {
                        let parent_class = find_parent_class(node, &class_ranges);
                        let handle = if let Some(ref cn) = parent_class {
                            format!("fn:{}.{}", cn, name)
                        } else {
                            format!("fn:{}", name)
                        };
                        let kind = if parent_class.is_some() {
                            SymbolKind::Method
                        } else {
                            SymbolKind::Function
                        };
                        let parent = parent_class.map(|cn| format!("class:{}", cn));
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind,
                            handle,
                            start_line: node.start_position().row + 1,
                            end_line: node.end_position().row + 1,
                            signature: Some(node_signature(node, source, language)),
                            parent,
                        });
                    }
                }
                "class.def" => {
                    class_def_node = Some(node);
                }
                "class.name" => {
                    class_name_str = node.utf8_text(source_bytes).ok();
                }
                "method.def" => {
                    if let Some(name) = child_text_by_field(node, "name", source) {
                        let parent_class = find_parent_class(node, &class_ranges)
                            .or_else(|| extract_receiver_type(node, source, language));
                        let handle = if let Some(ref cn) = parent_class {
                            format!("fn:{}.{}", cn, name)
                        } else {
                            format!("fn:{}", name)
                        };
                        let parent = parent_class.map(|cn| format!("class:{}", cn));
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: SymbolKind::Method,
                            handle,
                            start_line: node.start_position().row + 1,
                            end_line: node.end_position().row + 1,
                            signature: Some(node_signature(node, source, language)),
                            parent,
                        });
                    }
                }
                _ => {}
            }
        }

        // Emit class.def after processing all captures in the match.
        // Skip impl_item (Rust) — it provides methods but is not itself a class.
        if let (Some(node), Some(name)) = (class_def_node, class_name_str) {
            if node.kind() == "impl_item" {
                continue;
            }
            symbols.push(Symbol {
                name: name.to_string(),
                kind: SymbolKind::Class,
                handle: format!("class:{}", name),
                start_line: node.start_position().row + 1,
                end_line: node.end_position().row + 1,
                signature: Some(node_signature(node, source, language)),
                parent: None,
            });
        }
    }

    symbols
}

/// Try to extract a receiver/self type from a method declaration (Go receiver).
/// For Go `func (b *Bar) Baz()`, returns `Some("Bar")`.
fn extract_receiver_type<'a>(node: Node<'a>, source: &'a str, language: Language) -> Option<String> {
    if !matches!(language, Language::Go) {
        return None;
    }
    // Go method: (method_declaration (parameter_list (parameter_declaration ...)))
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "parameter_list" {
                // This is the receiver parameter list
                if let Some(name) = find_type_name_recursive(child, source) {
                    return Some(name);
                }
            }
        }
    }
    None
}

/// Recursively find a type_identifier inside a node tree.
fn find_type_name_recursive<'a>(node: Node<'a>, source: &'a str) -> Option<String> {
    if node.kind() == "type_identifier" {
        return node.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if let Some(name) = find_type_name_recursive(child, source) {
                return Some(name);
            }
        }
    }
    None
}


/// Extract all imports from a parsed source.
pub fn extract_imports(
    source: &str,
    tree: &Tree,
    language: Language,
) -> Vec<Import> {
    // C and C++ use procedural extraction
    if matches!(language, Language::C | Language::Cpp | Language::Java | Language::CSharp | Language::Ruby | Language::Php) {
        return extract_imports_procedural(source, tree);
    }

    let queries = get_queries(language);
    let lang = language.tree_sitter_language();
    let query = match Query::new(&lang, queries.import_query) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };

    let mut cursor = QueryCursor::new();
    let root = tree.root_node();
    let source_bytes = source.as_bytes();

    let mut imports: Vec<Import> = Vec::new();

    for match_ in cursor.matches(&query, root, source_bytes) {
        let mut module = String::new();
        let mut names: Vec<String> = Vec::new();
        let mut line: usize = 0;
        let mut is_import = false;

        for capture in match_.captures {
            let capture_name = &query.capture_names()[capture.index as usize];
            let text = capture.node.utf8_text(source_bytes).unwrap_or("");
            let node_line = capture.node.start_position().row + 1;

            match *capture_name {
                "import" | "import_from" => {
                    is_import = true;
                    line = node_line;
                }
                "module" => {
                    module = text.trim_matches('"').trim_matches('\'').to_string();
                    if line == 0 {
                        line = node_line;
                    }
                }
                "name" | "specifier" | "default_import" => {
                    names.push(text.to_string());
                    if line == 0 {
                        line = node_line;
                    }
                }
                "alias" => {
                    if let Some(alias_name) = child_text_by_field(capture.node, "alias", source) {
                        names.push(alias_name.to_string());
                    } else {
                        names.push(text.to_string());
                    }
                    if line == 0 {
                        line = node_line;
                    }
                }
                _ => {}
            }
        }

        if is_import && !module.is_empty() {
            let is_duplicate = imports.iter().any(|i| {
                i.module == module && i.line == line && i.names == names
            });
            if !is_duplicate {
                imports.push(Import {
                    module,
                    names,
                    line,
                });
            }
        }
    }

    imports
}

/// Get the text of a child node by field name.
fn child_text_by_field<'a>(node: Node<'a>, field: &str, source: &'a str) -> Option<&'a str> {
    node.child_by_field_name(field)
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
}

/// Find the enclosing class name for a node based on byte ranges.
fn find_parent_class(node: Node, class_ranges: &[(usize, usize, String)]) -> Option<String> {
    let node_start = node.start_byte();
    let node_end = node.end_byte();

    class_ranges
        .iter()
        .rev()
        .find(|(start, end, _)| *start <= node_start && node_end <= *end)
        .map(|(_, _, name)| name.clone())
}

/// Generate a human-readable signature for a definition node.
fn node_signature(node: Node, source: &str, language: Language) -> String {
    let start = node.start_byte();
    let end = node.end_byte();

    match language {
        Language::Python => {
            let full_text = &source[start..end];
            if let Some(colon_pos) = full_text.find(':') {
                full_text[..=colon_pos].to_string()
            } else {
                full_text.lines().next().unwrap_or("").to_string()
            }
        }
        Language::TypeScript | Language::JavaScript | Language::C | Language::Cpp | Language::Rust | Language::Go | Language::Bash | Language::Java | Language::CSharp | Language::Ruby | Language::Php => {
            let full_text = &source[start..end];
            if let Some(brace_pos) = full_text.find('{') {
                full_text[..brace_pos].trim().to_string()
            } else {
                full_text.lines().next().unwrap_or("").to_string()
            }
        }
    }
}
