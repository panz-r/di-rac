use crate::extractor;
use crate::language::Language;
use serde::Serialize;
use tree_sitter::Node;

#[derive(Debug, Clone, Serialize)]
pub struct IndexedSymbol {
    pub n: String,
    pub t: String, // "d" (definition), "r" (reference), "a" (declaration), "i" (import/include)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k: Option<String>,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

/// Index a file: extract definitions, references, and imports.
/// Returns indexed symbols in a format compatible with SymbolIndexService.
pub fn index_file(
    source: &str,
    tree: &tree_sitter::Tree,
    language: Language,
) -> Vec<IndexedSymbol> {
    let mut symbols = Vec::new();

    // 1. Definitions from the outline extraction
    let defs = extractor::extract_symbols(source, tree, language);
    for def in &defs {
        let kind = match def.kind {
            extractor::SymbolKind::Function => "function",
            extractor::SymbolKind::Class => "class",
            extractor::SymbolKind::Method => "method",
        };
        symbols.push(IndexedSymbol {
            n: def.name.clone(),
            t: "d".to_string(),
            k: Some(kind.to_string()),
            start_line: def.start_line,
            start_col: 1,
            end_line: def.end_line,
            end_col: 1,
        });
    }

    // 2. Imports/includes
    let imports = extractor::extract_imports(source, tree, language);
    for imp in &imports {
        symbols.push(IndexedSymbol {
            n: imp.module.clone(),
            t: "i".to_string(),
            k: None,
            start_line: imp.line,
            start_col: 1,
            end_line: imp.line,
            end_col: 1,
        });
    }

    // 3. References: walk AST collecting identifier nodes
    let root = tree.root_node();
    let mut ref_count = 0;
    collect_references(root, source, &mut symbols, &mut ref_count, 1000);

    symbols
}

fn collect_references<'a>(
    node: Node<'a>,
    source: &str,
    symbols: &mut Vec<IndexedSymbol>,
    count: &mut usize,
    max: usize,
) {
    if *count >= max {
        return;
    }

    let kind = node.kind();
    if kind.contains("identifier")
        || kind == "property_identifier"
        || kind == "type_identifier"
    {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            if !text.is_empty() {
                let start = node.start_position();
                let end = node.end_position();
                symbols.push(IndexedSymbol {
                    n: text.to_string(),
                    t: "r".to_string(),
                    k: None,
                    start_line: start.row + 1,
                    start_col: start.column + 1,
                    end_line: end.row + 1,
                    end_col: end.column + 1,
                });
                *count += 1;
            }
        }
    }

    for i in 0..node.child_count() {
        if *count >= max {
            return;
        }
        if let Some(child) = node.child(i) {
            collect_references(child, source, symbols, count, max);
        }
    }
}
