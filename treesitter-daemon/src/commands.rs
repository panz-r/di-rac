use crate::error::AnalyzerError;
use crate::extractor::{self, Import, Symbol};
use crate::parser;
use crate::parser::ParsedSource;
use crate::skeleton;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandOutput {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols: Option<Vec<Symbol>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imports: Option<Vec<Import>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skeleton: Option<String>,
    // --- expand-symbol fields ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    // --- batch fields ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<serde_json::Value>>,
    // --- repo-map fields ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<serde_json::Value>>,
    // --- status / warm-cache fields ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<serde_json::Value>,
}

impl CommandOutput {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"ok":false,"error":{"code":"INTERNAL_ERROR","message":"Serialization failed"}}"#.to_string()
        })
    }
}

fn make_output(
    id: Option<serde_json::Value>,
    symbols: Option<Vec<Symbol>>,
    imports: Option<Vec<Import>>,
    skeleton: Option<String>,
) -> CommandOutput {
    CommandOutput {
        ok: true, id, symbols, imports, skeleton,
        body: None, start_line: None, end_line: None,
        results: None, files: None, status: None,
    }
}

pub fn outline(parsed: &ParsedSource, id: Option<serde_json::Value>) -> CommandOutput {
    let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    let imports = extractor::extract_imports(&parsed.source, &parsed.tree, parsed.language);
    make_output(id, Some(symbols), Some(imports), None)
}

pub fn symbols(parsed: &ParsedSource, id: Option<serde_json::Value>) -> CommandOutput {
    let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    let imports = extractor::extract_imports(&parsed.source, &parsed.tree, parsed.language);
    make_output(id, Some(symbols), Some(imports), None)
}

pub fn handles(parsed: &ParsedSource, id: Option<serde_json::Value>) -> CommandOutput {
    let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    make_output(id, Some(symbols), None, None)
}

pub fn skeleton_cmd(parsed: &ParsedSource, id: Option<serde_json::Value>) -> CommandOutput {
    let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    let skel = skeleton::generate_skeleton(&parsed.source, &parsed.tree, parsed.language);
    make_output(id, Some(symbols), None, Some(skel))
}

// ── New command handlers ─────────────────────────────────────────

pub fn expand_symbol_cmd(
    parsed: &ParsedSource,
    handle: &str,
    id: Option<serde_json::Value>,
) -> CommandOutput {
    let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);

    let sym = symbols.iter().find(|s| s.handle == handle);
    match sym {
        Some(s) => {
            let body = extract_body_by_lines(&parsed.source, s.start_line, s.end_line);
            let start_line = s.start_line;
            let end_line = s.end_line;
            CommandOutput {
                ok: true,
                id,
                symbols: Some(symbols),
                imports: None,
                skeleton: None,
                body: Some(body),
                start_line: Some(start_line),
                end_line: Some(end_line),
                results: None,
                files: None,
                status: None,
            }
        }
        None => error_output(id, "SYMBOL_NOT_FOUND", format!("Symbol not found: {}", handle)),
    }
}

/// Extract source text for a given line range (1-based, inclusive).
fn extract_body_by_lines(source: &str, start_line: usize, end_line: usize) -> String {
    source
        .lines()
        .skip(start_line.saturating_sub(1))
        .take(end_line.saturating_sub(start_line.saturating_sub(1)))
        .collect::<Vec<&str>>()
        .join("\n")
}

/// Batch: run a subcommand across multiple files.
pub fn batch_cmd(
    file_list: Vec<String>,
    subcommand: String,
    id: Option<serde_json::Value>,
    workspace_root: Option<&std::path::Path>,
) -> CommandOutput {
    let mut results: Vec<serde_json::Value> = Vec::new();

    for file_path_str in &file_list {
        let path = std::path::PathBuf::from(file_path_str);
        let parsed = match parser::parse_source(Some(&path), None, None, workspace_root) {
            Ok(p) => p,
            Err(e) => {
                results.push(serde_json::json!({
                    "file": file_path_str,
                    "ok": false,
                    "error": {"code": e.code, "message": e.message}
                }));
                continue;
            }
        };
        match dispatch(&parsed, &subcommand, None) {
            Ok(output) => {
                let mut val = serde_json::to_value(&output).unwrap_or(serde_json::Value::Null);
                if let Some(obj) = val.as_object_mut() {
                    obj.insert("file".to_string(), serde_json::json!(file_path_str));
                }
                results.push(val);
            }
            Err(e) => {
                results.push(serde_json::json!({
                    "file": file_path_str,
                    "ok": false,
                    "error": {"code": e.code, "message": e.message}
                }));
            }
        }
    }

    CommandOutput {
        ok: true,
        id,
        symbols: None,
        imports: None,
        skeleton: None,
        body: None,
        start_line: None,
        end_line: None,
        results: Some(results),
        files: None,
        status: None,
    }
}

/// Search all cached files for symbols matching a query string.
pub fn search_symbols_cmd(
    cache: &crate::cache::ParseCache,
    query: &str,
    kind_filter: Option<&str>,
    max_results: Option<usize>,
    id: Option<serde_json::Value>,
) -> CommandOutput {
    let mut results: Vec<serde_json::Value> = Vec::new();
    let query_lower = query.to_lowercase();
    let max = max_results.unwrap_or(100);

    for (path, parsed) in cache.iter() {
        if results.len() >= max {
            break;
        }
        let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
        for sym in &symbols {
            if results.len() >= max {
                break;
            }
            if let Some(kf) = kind_filter {
                if sym.kind.to_string().to_lowercase() != kf.to_lowercase() {
                    continue;
                }
            }
            if sym.name.to_lowercase().contains(&query_lower) {
                let mut val = serde_json::to_value(sym).unwrap_or(serde_json::Value::Null);
                if let Some(obj) = val.as_object_mut() {
                    obj.insert("file".to_string(), serde_json::json!(path.display().to_string()));
                }
                results.push(val);
            }
        }
    }

    CommandOutput {
        ok: true,
        id,
        symbols: None,
        imports: None,
        skeleton: None,
        body: None,
        start_line: None,
        end_line: None,
        results: Some(results),
        files: None,
        status: None,
    }
}

/// Repo-map: walk a directory tree, parse every source file, return top-level symbols.
pub fn repo_map_cmd(
    root: Option<&std::path::Path>,
    workspace_root: Option<&std::path::Path>,
    id: Option<serde_json::Value>,
) -> CommandOutput {
    let search_root = root.or(workspace_root);
    let search_path = match search_root {
        Some(p) => p.to_path_buf(),
        None => {
            return error_output(id, "INVALID_COMMAND", "repo-map requires 'root' or workspace root".to_string());
        }
    };

    let mut file_results: Vec<serde_json::Value> = Vec::new();

    if let Err(e) = walk_and_parse(&search_path, workspace_root, &mut file_results) {
        return error_output(id, "INTERNAL_ERROR", format!("Failed to walk directory: {}", e));
    }

    CommandOutput {
        ok: true,
        id,
        symbols: None,
        imports: None,
        skeleton: None,
        body: None,
        start_line: None,
        end_line: None,
        results: None,
        files: Some(file_results),
        status: None,
    }
}

/// Recursively walk a directory and parse recognised source files.
fn walk_and_parse(
    root: &std::path::Path,
    workspace_root: Option<&std::path::Path>,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;

    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        // Skip hidden files/dirs and common ignores.
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name.starts_with('.') || file_name == "node_modules" || file_name == "target" {
            continue;
        }

        if path.is_dir() {
            walk_and_parse(&path, workspace_root, results)?;
        } else if let Some(lang) = crate::language::Language::from_path(&path) {
            match parser::parse_source(Some(&path), None, None, workspace_root) {
                Ok(parsed) => {
                    let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, lang);
                    let imports = extractor::extract_imports(&parsed.source, &parsed.tree, lang);
                    let val = serde_json::json!({
                        "file": path.display().to_string(),
                        "language": lang.as_str(),
                        "symbols": symbols,
                        "imports": imports,
                    });
                    results.push(val);
                }
                Err(_) => {
                    // Skip files that fail to parse.
                }
            }
        }
    }
    Ok(())
}

/// Warm the cache by parsing a list of files.
pub fn warm_cache_cmd(
    cache: &mut crate::cache::ParseCache,
    file_list: Vec<String>,
    id: Option<serde_json::Value>,
    workspace_root: Option<&std::path::Path>,
) -> CommandOutput {
    let mut parsed_count = 0;
    let mut errors: Vec<serde_json::Value> = Vec::new();

    for file_path_str in &file_list {
        let path = std::path::PathBuf::from(file_path_str);
        match parser::parse_source(Some(&path), None, None, workspace_root) {
            Ok(ps) => {
                if let Ok(canonical) = path.canonicalize() {
                    cache.insert(canonical, ps);
                    parsed_count += 1;
                }
            }
            Err(e) => {
                errors.push(serde_json::json!({
                    "file": file_path_str,
                    "error": {"code": e.code, "message": e.message}
                }));
            }
        }
    }

    CommandOutput {
        ok: true,
        id,
        symbols: None,
        imports: None,
        skeleton: None,
        body: None,
        start_line: None,
        end_line: None,
        results: None,
        files: None,
        status: Some(serde_json::json!({
            "cached": parsed_count,
            "errors": errors,
            "total_entries": cache.len()
        })),
    }
}

/// Re-parse a single file and store it in the cache.
pub fn reparse_cmd(
    cache: &mut crate::cache::ParseCache,
    file: Option<&str>,
    content: Option<&str>,
    language_override: Option<&str>,
    workspace_root: Option<&std::path::Path>,
    id: Option<serde_json::Value>,
) -> CommandOutput {
    let file_path = file.map(std::path::PathBuf::from);
    let file_path_ref = file_path.as_deref();

    match parser::parse_source(file_path_ref, content, language_override, workspace_root) {
        Ok(ps) => {
            if let Some(ref p) = file_path_ref {
                if let Ok(canonical) = p.canonicalize() {
                    cache.insert(canonical, ps);
                }
            }
            CommandOutput {
                ok: true,
                id,
                symbols: None,
                imports: None,
                skeleton: None,
                body: None,
                start_line: None,
                end_line: None,
                results: None,
                files: None,
                status: Some(serde_json::json!({"cached": true, "total_entries": cache.len()})),
            }
        }
        Err(e) => error_output(id, e.code.to_string().as_str(), e.message),
    }
}

/// Clear the cache.
pub fn clear_cache_cmd(cache: &mut crate::cache::ParseCache, id: Option<serde_json::Value>) -> CommandOutput {
    cache.clear();
    CommandOutput {
        ok: true,
        id,
        symbols: None,
        imports: None,
        skeleton: None,
        body: None,
        start_line: None,
        end_line: None,
        results: None,
        files: None,
        status: Some(serde_json::json!({"entries": 0})),
    }
}

/// Return current cache metrics.
pub fn status_cmd(cache: &crate::cache::ParseCache, id: Option<serde_json::Value>) -> CommandOutput {
    let s = cache.status();
    CommandOutput {
        ok: true,
        id,
        symbols: None,
        imports: None,
        skeleton: None,
        body: None,
        start_line: None,
        end_line: None,
        results: None,
        files: None,
        status: Some(serde_json::to_value(&s).unwrap_or(serde_json::Value::Null)),
    }
}

fn error_output(
    id: Option<serde_json::Value>,
    code: &str,
    message: String,
) -> CommandOutput {
    CommandOutput {
        ok: false,
        id,
        symbols: None,
        imports: None,
        skeleton: None,
        body: None,
        start_line: None,
        end_line: None,
        results: None,
        files: None,
        status: Some(serde_json::json!({"error": {"code": code, "message": message}})),
    }
}

pub fn dispatch(
    parsed: &ParsedSource,
    command: &str,
    id: Option<serde_json::Value>,
) -> Result<CommandOutput, AnalyzerError> {
    match command {
        "outline" => Ok(outline(parsed, id)),
        "symbols" => Ok(symbols(parsed, id)),
        "handles" => Ok(handles(parsed, id)),
        "skeleton" => Ok(skeleton_cmd(parsed, id)),
        other => Err(AnalyzerError::invalid_command(other)),
    }
}
