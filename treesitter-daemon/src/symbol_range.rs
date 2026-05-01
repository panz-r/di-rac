use tree_sitter::Node;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtendedRange {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
}

const WRAPPER_TYPES: &[&str] = &[
    "export_statement",
    "export_declaration",
    "ambient_declaration",
    "decorated_definition",
    "internal_module",
];

const LEADING_TYPES: &[&str] = &["comment", "decorator", "attribute"];

/// Port of ASTAnchorBridge.getExtendedRange.
///
/// 1. Walk the parent chain for wrapper types (export, decorator, etc.)
/// 2. Walk previous named siblings for leading comments/decorators/attributes
pub fn get_extended_range(target_node: Node) -> ExtendedRange {
    let mut start_byte = target_node.start_byte();
    let mut end_byte = target_node.end_byte();
    let mut start_line = target_node.start_position().row + 1;
    let end_line = target_node.end_position().row + 1;

    // Walk parent chain for wrapper types
    let mut current = target_node;
    while let Some(parent) = current.parent() {
        if WRAPPER_TYPES.contains(&parent.kind()) {
            start_byte = parent.start_byte();
            end_byte = parent.end_byte();
            start_line = parent.start_position().row + 1;
            current = parent;
        } else {
            break;
        }
    }

    // Walk previous named siblings for leading comments/decorators/attributes
    while let Some(prev) = current.prev_named_sibling() {
        let kind = prev.kind();
        if LEADING_TYPES.contains(&kind) || kind.contains("comment") {
            start_byte = prev.start_byte();
            start_line = prev.start_position().row + 1;
            current = prev;
        } else {
            break;
        }
    }

    ExtendedRange {
        start_byte,
        end_byte,
        start_line,
        end_line,
    }
}
