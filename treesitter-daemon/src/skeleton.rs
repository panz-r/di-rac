use crate::language::Language;
use tree_sitter::Tree;

/// Generate a skeleton representation of source code.
pub fn generate_skeleton(source: &str, tree: &Tree, language: Language) -> String {
    let root = tree.root_node();
    let source_bytes = source.as_bytes();
    let mut output = String::new();

    for i in 0..root.child_count() {
        if let Some(child) = root.child(i) {
            if i > 0 {
                if let Some(prev) = root.child(i - 1) {
                    let prev_end_line = prev.end_position().row;
                    let curr_start_line = child.start_position().row;
                    if curr_start_line > prev_end_line + 1 {
                        for _ in 0..(curr_start_line - prev_end_line).min(3) {
                            output.push('\n');
                        }
                    }
                }
            }
            skeleton_node(child, source, source_bytes, language, 0, &mut output);
        }
    }

    output.trim_end().to_string()
}

fn skeleton_node(
    node: tree_sitter::Node,
    _source: &str,
    source_bytes: &[u8],
    language: Language,
    depth: usize,
    output: &mut String,
) {
    let kind = node.kind().to_string();

    match language {
        Language::Python => {
            skeleton_node_python(node, _source, source_bytes, &kind, depth, output);
        }
        _ => {
            skeleton_node_brace(node, _source, source_bytes, &kind, language, depth, output);
        }
    }
}

fn is_definition_node(kind: &str, language: Language) -> bool {
    match language {
        Language::Python => {
            matches!(kind, "function_definition" | "class_definition")
        }
        Language::TypeScript | Language::JavaScript => {
            matches!(
                kind,
                "function_declaration"
                    | "generator_function_declaration"
                    | "class_declaration"
                    | "method_definition"
            )
        }
        Language::C => {
            matches!(kind, "function_definition")
        }
        Language::Cpp => {
            matches!(
                kind,
                "function_definition" | "class_specifier" | "struct_specifier"
            )
        }
        Language::Java => {
            matches!(
                kind,
                "method_declaration" | "constructor_declaration"
                    | "class_declaration" | "interface_declaration"
                    | "enum_declaration"
            )
        }
        Language::CSharp => {
            matches!(
                kind,
                "method_declaration" | "constructor_declaration"
                    | "class_declaration" | "interface_declaration"
                    | "struct_declaration" | "enum_declaration"
            )
        }
        Language::Ruby => {
            matches!(
                kind,
                "method" | "singleton_method" | "class" | "module"
            )
        }
        Language::Php => {
            matches!(
                kind,
                "function_definition" | "method_declaration"
                    | "class_declaration" | "interface_declaration"
                    | "trait_declaration" | "enum_declaration"
            )
        }
        Language::Rust => {
            matches!(
                kind,
                "function_item" | "struct_item" | "enum_item"
                    | "trait_item" | "impl_item" | "mod_item"
            )
        }
        Language::Go => {
            matches!(
                kind,
                "function_declaration" | "method_declaration" | "type_declaration"
            )
        }
        Language::Bash => {
            matches!(kind, "function_definition")
        }
    }
}

fn skeleton_node_python(
    node: tree_sitter::Node,
    _source: &str,
    source_bytes: &[u8],
    kind: &str,
    depth: usize,
    output: &mut String,
) {
    let indent = "    ".repeat(depth);

    if is_definition_node(kind, Language::Python) {
        let full_text = node.utf8_text(source_bytes).unwrap_or("");
        if let Some(colon_pos) = full_text.find(':') {
            let signature = &full_text[..=colon_pos];
            let first_line = signature.lines().next().unwrap_or(signature);
            output.push_str(&format!("{}{}\n", indent, first_line));
            output.push_str(&format!("{}    pass\n", indent));
        }
    } else if is_python_block(kind) {
        let full_text = node.utf8_text(source_bytes).unwrap_or("");
        if let Some(colon_pos) = full_text.find(':') {
            let header = &full_text[..=colon_pos];
            let first_line = header.lines().next().unwrap_or(header);
            output.push_str(&format!("{}{}\n", indent, first_line));
            output.push_str(&format!("{}    pass\n", indent));
        }
    } else {
        let text = node.utf8_text(source_bytes).unwrap_or("");
        for line in text.lines() {
            if !line.trim().is_empty() {
                output.push_str(&format!("{}{}\n", indent, line));
            } else {
                output.push('\n');
            }
        }
    }
}

fn skeleton_node_brace(
    node: tree_sitter::Node,
    _source: &str,
    source_bytes: &[u8],
    kind: &str,
    language: Language,
    depth: usize,
    output: &mut String,
) {
    let indent = "    ".repeat(depth);

    if is_definition_node(kind, language) {
        let full_text = node.utf8_text(source_bytes).unwrap_or("");
        if let Some(brace_pos) = full_text.find('{') {
            let before_brace = &full_text[..brace_pos];
            let sig = before_brace.trim();
            output.push_str(&format!("{}{} {{ … }}\n", indent, sig));
        } else {
            let first_line = full_text.lines().next().unwrap_or(full_text);
            output.push_str(&format!("{}{}\n", indent, first_line));
        }
    } else if is_brace_block(kind) {
        let full_text = node.utf8_text(source_bytes).unwrap_or("");
        if let Some(brace_pos) = full_text.find('{') {
            let before_brace = &full_text[..brace_pos];
            let header = before_brace.trim();
            output.push_str(&format!("{}{} {{ … }}\n", indent, header));
        } else {
            let first_line = full_text.lines().next().unwrap_or(full_text);
            output.push_str(&format!("{}{}\n", indent, first_line));
        }
    } else {
        let text = node.utf8_text(source_bytes).unwrap_or("");
        for line in text.lines() {
            output.push_str(&format!("{}{}\n", indent, line));
        }
    }
}

fn is_python_block(kind: &str) -> bool {
    matches!(
        kind,
        "if_statement"
            | "for_statement"
            | "while_statement"
            | "try_statement"
            | "with_statement"
            | "elif_clause"
            | "else_clause"
            | "except_clause"
            | "finally_clause"
    )
}

fn is_brace_block(kind: &str) -> bool {
    matches!(
        kind,
        "if_statement"
            | "for_statement"
            | "for_in_statement"
            | "while_statement"
            | "try_statement"
            | "switch_statement"
            | "catch_clause"
    )
}
