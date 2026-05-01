/// Debug test for C query matching issue.
use tree_sitter::{Parser, Query, QueryCursor};

#[test]
fn test_c_parsing_and_queries() {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_c::LANGUAGE.into()).unwrap();

    let source = "int add(int a, int b) { return a + b; }\nint main(void) { return 0; }\n";

    let tree = parser.parse(source, None).unwrap();
    let root = tree.root_node();

    // Verify parsing works
    println!("Root kind: {}", root.kind());
    assert_eq!(root.kind(), "translation_unit");
    
    // Count function_definition nodes by walking
    let mut func_count = 0;
    let mut all_kinds: Vec<String> = Vec::new();
    walk_nodes(root, &mut |n| {
        if n.is_named() {
            all_kinds.push(n.kind().to_string());
            if n.kind() == "function_definition" {
                func_count += 1;
            }
        }
    });
    
    println!("Named node kinds: {:?}", all_kinds);
    println!("Function definitions found by walk: {}", func_count);
    assert!(func_count >= 2, "Expected at least 2 function_definition nodes, got {}", func_count);

    let lang: tree_sitter::Language = tree_sitter_c::LANGUAGE.into();
    println!("Language version: {}", lang.version());

    let test_queries = [
        "(function_definition) @f",
        "(_) @any",
        "(function_definition (function_declarator (identifier) @name)) @f",
        "(translation_unit) @tu",
        "(identifier) @id",
    ];

    for qstr in &test_queries {
        println!("\n--- Query: {} ---", qstr);
        match Query::new(&lang, qstr) {
            Ok(query) => {
                println!("  Pattern count: {}", query.pattern_count());
                println!("  Capture names: {:?}", &query.capture_names()[..]);
                let mut cursor = QueryCursor::new();
                let source_bytes = source.as_bytes();
                
                // Use matches API like extractor.rs does
                let mut match_count = 0;
                for match_ in cursor.matches(&query, root, source_bytes) {
                    match_count += 1;
                    for capture in match_.captures {
                        let name = &query.capture_names()[capture.index as usize];
                        let text = capture.node.utf8_text(source_bytes).unwrap_or("???");
                        println!("    capture '{}': {:?}  [line {}]", name, text, capture.node.start_position().row + 1);
                    }
                }
                println!("  Match count: {}", match_count);
            }
            Err(e) => println!("  QUERY ERROR: {:?}", e),
        }
    }
}

fn walk_nodes(node: tree_sitter::Node, f: &mut dyn FnMut(tree_sitter::Node)) {
    f(node);
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            walk_nodes(c, f);
        }
    }
}
