use crate::extractor;
use crate::language::Language;
use serde::Serialize;
use tree_sitter::Node;

#[derive(Debug, Clone, Serialize)]
pub struct SymbolContextResult {
    pub imports: Vec<String>,
    pub class_head: Option<String>,
    pub properties: Vec<String>,
}

/// Resolve context for a symbol: relevant imports, class head, and class properties
/// that reference identifiers used within the symbol's body.
pub fn get_symbol_context(
    source: &str,
    tree: &tree_sitter::Tree,
    language: Language,
    handle: &str,
) -> Result<SymbolContextResult, String> {
    let symbols = extractor::extract_symbols(source, tree, language);
    let sym = symbols
        .iter()
        .find(|s| s.handle == handle)
        .or_else(|| symbols.iter().find(|s| s.name == handle))
        .ok_or_else(|| format!("Symbol not found: {}", handle))?;

    let root = tree.root_node();
    let target_node = root
        .descendant_for_byte_range(sym.start_byte, sym.end_byte)
        .ok_or("Could not locate AST node")?;

    // 1. Collect identifiers used within the target node
    let used_identifiers = collect_identifiers(target_node, source);

    // 2. Collect relevant imports
    let imports = extractor::extract_imports(source, tree, language);
    let source_lines: Vec<&str> = source.lines().collect();
    let mut relevant_import_lines: Vec<String> = Vec::new();

    for imp in &imports {
        let import_text = source_lines
            .get(imp.line - 1)
            .unwrap_or(&"")
            .to_string();
        for id in &used_identifiers {
            if word_match(&import_text, id) {
                relevant_import_lines.push(import_text.clone());
                break;
            }
        }
    }

    // 3. Find containing class and its properties
    let class_node = find_containing_class(target_node, language);
    let mut class_head: Option<String> = None;
    let mut properties: Vec<String> = Vec::new();

    if let Some(cn) = class_node {
        // Class head: first line of class declaration
        let class_start_line = cn.start_position().row;
        if let Some(line) = source_lines.get(class_start_line) {
            class_head = Some(line.to_string());
        }

        // Find property nodes within this class
        let prop_nodes = find_class_properties(cn, language);
        for prop_node in prop_nodes {
            let prop_name = get_property_name(prop_node, source);
            if let Some(name) = prop_name {
                if used_identifiers.contains(&name) {
                    let start = prop_node.start_position().row;
                    let end = prop_node.end_position().row;
                    for i in start..=end {
                        if let Some(line) = source_lines.get(i) {
                            properties.push(line.to_string());
                        }
                    }
                }
            }
        }
    }

    Ok(SymbolContextResult {
        imports: relevant_import_lines,
        class_head,
        properties,
    })
}

/// Collect all identifier texts from a subtree.
fn collect_identifiers(node: Node, source: &str) -> Vec<String> {
    let mut ids = Vec::new();
    collect_identifiers_recursive(node, source, &mut ids);
    ids
}

fn collect_identifiers_recursive(node: Node, source: &str, ids: &mut Vec<String>) {
    let kind = node.kind();
    if kind.contains("identifier") || kind == "property_identifier" || kind == "type_identifier" {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            ids.push(text.to_string());
        }
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_identifiers_recursive(child, source, ids);
        }
    }
}

/// Check if `word` appears as a whole word in `text`.
fn word_match(text: &str, word: &str) -> bool {
    let text_lower = text.to_lowercase();
    let word_lower = word.to_lowercase();
    for i in 0..text_lower.len() {
        if text_lower[i..].starts_with(&word_lower) {
            let before_is_boundary = i == 0
                || !text_lower.as_bytes()[i - 1].is_ascii_alphanumeric()
                    && text_lower.as_bytes()[i - 1] != b'_';
            let after_pos = i + word_lower.len();
            let after_is_boundary = after_pos >= text_lower.len()
                || (!text_lower.as_bytes()[after_pos].is_ascii_alphanumeric()
                    && text_lower.as_bytes()[after_pos] != b'_');
            if before_is_boundary && after_is_boundary {
                return true;
            }
        }
    }
    false
}

/// Walk parent chain to find the containing class/struct/interface.
fn find_containing_class<'a>(node: Node<'a>, language: Language) -> Option<Node<'a>> {
    let class_types: &[&str] = match language {
        Language::TypeScript | Language::JavaScript => &["class_declaration"],
        Language::Python => &["class_definition"],
        Language::Rust => &["struct_item", "enum_item", "impl_item", "trait_item"],
        Language::Go => &["type_declaration"],
        Language::Java => &["class_declaration", "interface_declaration", "enum_declaration"],
        Language::CSharp => &["class_declaration", "struct_declaration", "interface_declaration"],
        Language::Ruby => &["class"],
        Language::Php => &["class_declaration", "interface_declaration", "trait_declaration"],
        Language::C | Language::Cpp => &["struct_specifier", "class_specifier"],
        _ => return None,
    };

    let mut current = node;
    while let Some(parent) = current.parent() {
        if class_types.contains(&parent.kind()) {
            return Some(parent);
        }
        current = parent;
    }
    None
}

/// Find property/field nodes within a class body.
fn find_class_properties<'a>(class_node: Node<'a>, language: Language) -> Vec<Node<'a>> {
    let property_types: &[&str] = match language {
        Language::TypeScript | Language::JavaScript => {
            &["public_field_definition", "private_property_definition"]
        }
        Language::Python => &["assignment"],
        Language::Java | Language::CSharp => &["field_declaration"],
        Language::Rust => &["field_item"],
        Language::Go => &["field_declaration"],
        Language::Ruby => &[],
        Language::Php => &["property_declaration", "class_constant_declaration"],
        Language::C | Language::Cpp => &["field_declaration"],
        _ => &[],
    };

    let mut props = Vec::new();
    find_properties_recursive(class_node, property_types, &mut props, 0);
    props
}

fn find_properties_recursive<'a>(
    node: Node<'a>,
    property_types: &[&str],
    props: &mut Vec<Node<'a>>,
    depth: usize,
) {
    // Don't recurse deeper than 4 levels into class body to avoid picking up
    // properties from nested classes
    if depth > 4 {
        return;
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if property_types.contains(&child.kind()) {
                props.push(child);
            }
            // Recurse into body-like children but not into method-like children
            let kind = child.kind();
            if !kind.contains("function") && !kind.contains("method") {
                find_properties_recursive(child, property_types, props, depth + 1);
            }
        }
    }
}

/// Extract the name of a property node.
fn get_property_name<'a>(node: Node<'a>, source: &'a str) -> Option<String> {
    // Try "name" field first (standard for many grammars)
    if let Some(name_node) = node.child_by_field_name("name") {
        if let Ok(text) = name_node.utf8_text(source.as_bytes()) {
            return Some(text.to_string());
        }
    }

    // Fallback: find first identifier-like child
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            let kind = child.kind();
            if kind == "property_identifier"
                || kind == "identifier"
                || kind == "field_identifier"
                || kind == "name"
            {
                let text = child.utf8_text(source.as_bytes()).ok()?;
                if text != "self" && text != "this" {
                    return Some(text.to_string());
                }
            }
        }
    }

    None
}
