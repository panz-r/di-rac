use crate::db::{IndexDatabase, IndexStatus};
use crate::error::AnalyzerError;
use crate::extractor::{self, Import, Symbol};
use crate::parser;
use crate::parser::ParsedSource;
use crate::skeleton;
use crate::symbol_range;
use crate::context;
use crate::indexer;
use sha2::{Sha256, Digest};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<usize>,
    // --- generic data for new commands ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
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
        truncated: None, total_count: None, data: None,
    }
}

pub fn outline(parsed: &ParsedSource, id: Option<serde_json::Value>, max_results: Option<usize>) -> CommandOutput {
    let mut symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    let imports = extractor::extract_imports(&parsed.source, &parsed.tree, parsed.language);
    let total_count = symbols.len();
    let truncated = if let Some(max) = max_results {
        if total_count > max {
            symbols.truncate(max);
            true
        } else { false }
    } else { false };
    let mut out = make_output(id, Some(symbols), Some(imports), None);
    if truncated {
        out.truncated = Some(true);
        out.total_count = Some(total_count);
    }
    out
}

pub fn symbols(parsed: &ParsedSource, id: Option<serde_json::Value>, max_results: Option<usize>) -> CommandOutput {
    let mut symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    let imports = extractor::extract_imports(&parsed.source, &parsed.tree, parsed.language);
    let total_count = symbols.len();
    let truncated = if let Some(max) = max_results {
        if total_count > max {
            symbols.truncate(max);
            true
        } else { false }
    } else { false };
    let mut out = make_output(id, Some(symbols), Some(imports), None);
    if truncated {
        out.truncated = Some(true);
        out.total_count = Some(total_count);
    }
    out
}

pub fn handles(parsed: &ParsedSource, id: Option<serde_json::Value>, max_results: Option<usize>) -> CommandOutput {
    let mut symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    let total_count = symbols.len();
    let truncated = if let Some(max) = max_results {
        if total_count > max {
            symbols.truncate(max);
            true
        } else { false }
    } else { false };
    let mut out = make_output(id, Some(symbols), None, None);
    if truncated {
        out.truncated = Some(true);
        out.total_count = Some(total_count);
    }
    out
}

pub fn skeleton_cmd(parsed: &ParsedSource, id: Option<serde_json::Value>, max_results: Option<usize>) -> CommandOutput {
    let mut symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    let skel = skeleton::generate_skeleton(&parsed.source, &parsed.tree, parsed.language);
    let total_count = symbols.len();
    let truncated = if let Some(max) = max_results {
        if total_count > max {
            symbols.truncate(max);
            true
        } else { false }
    } else { false };
    let mut out = make_output(id, Some(symbols), None, Some(skel));
    if truncated {
        out.truncated = Some(true);
        out.total_count = Some(total_count);
    }
    out
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
                truncated: None,
                total_count: None,
                data: None,
            }
        }
        None => error_output(id, "SYMBOL_NOT_FOUND", format!("Symbol not found: {}", handle)),
    }
}

/// Find a symbol by handle (or plain name) and return its extended byte range.
pub fn symbol_range_cmd(
    parsed: &ParsedSource,
    handle: &str,
    id: Option<serde_json::Value>,
) -> CommandOutput {
    let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);

    // Try exact handle match first, then fall back to plain name match
    let sym = symbols
        .iter()
        .find(|s| s.handle == handle)
        .or_else(|| symbols.iter().find(|s| s.name == handle));

    let sym = match sym {
        Some(s) => s,
        None => return error_output(id, "SYMBOL_NOT_FOUND", format!("Symbol not found: {}", handle)),
    };

    // Locate the AST node by byte range
    let root = parsed.tree.root_node();
    let target_node = root.descendant_for_byte_range(sym.start_byte, sym.end_byte);

    let target_node = match target_node {
        Some(n) => {
            // Walk up to the nearest definition-type ancestor to avoid getting
            // a child node (e.g. just the identifier instead of the full function)
            let mut candidate = n;
            while let Some(parent) = candidate.parent() {
                if parent.start_byte() == sym.start_byte && parent.end_byte() == sym.end_byte {
                    candidate = parent;
                } else {
                    break;
                }
            }
            candidate
        }
        None => {
            // Fallback: return the symbol's stored range
            return CommandOutput {
                ok: true,
                id,
                symbols: None,
                imports: None,
                skeleton: None,
                body: None,
                start_line: Some(sym.start_line),
                end_line: Some(sym.end_line),
                results: None,
                files: None,
                status: None,
                truncated: None,
                total_count: None,
                data: Some(serde_json::json!({
                    "start_byte": sym.start_byte,
                    "end_byte": sym.end_byte,
                    "start_line": sym.start_line,
                    "end_line": sym.end_line,
                    "name_text": sym.name,
                    "handle": sym.handle,
                })),
            };
        }
    };

    let extended = symbol_range::get_extended_range(target_node);
    let name_text = sym.name.clone();
    let handle_out = sym.handle.clone();

    CommandOutput {
        ok: true,
        id,
        symbols: None,
        imports: None,
        skeleton: None,
        body: None,
        start_line: Some(extended.start_line),
        end_line: Some(extended.end_line),
        results: None,
        files: None,
        status: None,
        truncated: None,
        total_count: None,
        data: Some(serde_json::json!({
            "start_byte": extended.start_byte,
            "end_byte": extended.end_byte,
            "start_line": extended.start_line,
            "end_line": extended.end_line,
            "name_text": name_text,
            "handle": handle_out,
        })),
    }
}

/// Resolve context (imports, class head, properties) for a symbol.
pub fn symbol_context_cmd(
    parsed: &ParsedSource,
    handle: &str,
    id: Option<serde_json::Value>,
) -> CommandOutput {
    match context::get_symbol_context(&parsed.source, &parsed.tree, parsed.language, handle) {
        Ok(result) => CommandOutput {
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
            status: None,
            truncated: None,
            total_count: None,
            data: Some(serde_json::json!({
                "imports": result.imports,
                "class_head": result.class_head,
                "properties": result.properties,
            })),
        },
        Err(msg) => error_output(id, "CONTEXT_ERROR", msg),
    }
}

/// Index a file: extract definitions, references, and imports.
pub fn index_file_cmd(
    parsed: &ParsedSource,
    id: Option<serde_json::Value>,
) -> CommandOutput {
    let symbols = indexer::index_file(&parsed.source, &parsed.tree, parsed.language);
    let imports = extractor::extract_imports(&parsed.source, &parsed.tree, parsed.language);

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
        status: None,
        truncated: None,
        total_count: None,
        data: Some(serde_json::json!({
            "symbols": symbols,
            "imports": imports,
        })),
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
    max_results: Option<usize>,
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
        match dispatch(&parsed, &subcommand, None, max_results) {
            Ok(output) => {
                let val = serde_json::json!({
                    "file": file_path_str,
                    "ok": true,
                    "data": output
                });
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
        truncated: None,
        total_count: None,
                data: None,
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
        truncated: None,
        total_count: None,
                data: None,
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
        truncated: None,
        total_count: None,
                data: None,
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
        truncated: None,
        total_count: None,
                data: None,
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
                truncated: None,
                total_count: None,
                data: None,
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
        truncated: None,
        total_count: None,
                data: None,
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
        truncated: None,
        total_count: None,
                data: None,
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
        truncated: None,
        total_count: None,
                data: None,
    }
}

/// Invalidate cached entry for a single file path.
pub fn file_changed_cmd(
    cache: &mut crate::cache::ParseCache,
    file_path_str: &str,
    id: Option<serde_json::Value>,
    workspace_root: Option<&std::path::Path>,
) -> CommandOutput {
    let path = std::path::PathBuf::from(file_path_str);
    let key = if let Ok(canonical) = path.canonicalize() {
        canonical
    } else if path.is_absolute() {
        path
    } else if let Some(ws) = workspace_root {
        ws.join(&path)
    } else {
        path
    };
    let existed = cache.remove(&key).is_some();
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
        status: Some(serde_json::json!({"removed": existed, "path": key.display().to_string()})),
        truncated: None,
        total_count: None,
        data: None,
    }
}

// ── Persistent index commands ─────────────────────────────────────────────────

/// Index a file and persist symbols/imports to SQLite.
pub fn index_file_persist_cmd(
    parsed: &ParsedSource,
    file_path: &str,
    db: &IndexDatabase,
    id: Option<serde_json::Value>,
) -> CommandOutput {
    let symbols = indexer::index_file(&parsed.source, &parsed.tree, parsed.language);
    let imports = extractor::extract_imports(&parsed.source, &parsed.tree, parsed.language);

    // Compute content hash for change detection
    let mut hasher = Sha256::new();
    hasher.update(parsed.source.as_bytes());
    let content_hash = format!("{:x}", hasher.finalize());

    // Get mtime from filesystem (0 if content-based)
    let mtime = std::fs::metadata(file_path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64())
        .unwrap_or(0.0);

    match db.index_file(file_path, mtime, &content_hash, &symbols, &imports) {
        Ok(count) => CommandOutput {
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
            status: None,
            truncated: None,
            total_count: None,
            data: Some(serde_json::json!({
                "indexed": true,
                "symbol_count": count,
                "import_count": imports.len(),
            })),
        },
        Err(e) => error_output(id, "INDEX_ERROR", format!("Failed to index file: {}", e)),
    }
}

/// Invalidate (remove) all index entries for a file.
pub fn invalidate_file_cmd(
    db: &IndexDatabase,
    file_path: &str,
    id: Option<serde_json::Value>,
) -> CommandOutput {
    match db.invalidate_file(file_path) {
        Ok(()) => CommandOutput {
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
            status: None,
            truncated: None,
            total_count: None,
            data: Some(serde_json::json!({"ok": true, "file": file_path})),
        },
        Err(e) => error_output(id, "INDEX_ERROR", format!("Failed to invalidate file: {}", e)),
    }
}

/// Search the persistent index for symbols by name.
pub fn search_index_cmd(
    db: &IndexDatabase,
    query: &str,
    kind_filter: Option<&str>,
    max_results: Option<usize>,
    id: Option<serde_json::Value>,
) -> CommandOutput {
    let max = max_results.unwrap_or(100);
    match db.search_symbols(query, kind_filter, max) {
        Ok(results) => {
            let json_results: Vec<serde_json::Value> = results
                .into_iter()
                .map(|r| {
                    serde_json::json!({
                        "name": r.name,
                        "type": r.type_,
                        "kind": r.kind,
                        "file": r.file_path,
                        "start_line": r.start_line,
                        "start_col": r.start_col,
                        "end_line": r.end_line,
                        "end_col": r.end_col,
                    })
                })
                .collect();
            CommandOutput {
                ok: true,
                id,
                symbols: None,
                imports: None,
                skeleton: None,
                body: None,
                start_line: None,
                end_line: None,
                results: Some(json_results),
                files: None,
                status: None,
                truncated: None,
                total_count: None,
                data: None,
            }
        }
        Err(e) => error_output(id, "INDEX_ERROR", format!("Search failed: {}", e)),
    }
}

/// Return index statistics.
pub fn index_status_cmd(db: &IndexDatabase, id: Option<serde_json::Value>) -> CommandOutput {
    match db.index_status() {
        Ok(status) => CommandOutput {
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
                "file_count": status.file_count,
                "symbol_count": status.symbol_count,
                "import_count": status.import_count,
            })),
            truncated: None,
            total_count: None,
            data: None,
        },
        Err(e) => error_output(id, "INDEX_ERROR", format!("Status failed: {}", e)),
    }
}

/// Clear all entries from the persistent index.
pub fn clear_index_cmd(db: &IndexDatabase, id: Option<serde_json::Value>) -> CommandOutput {
    match db.clear_index() {
        Ok(()) => CommandOutput {
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
            status: Some(serde_json::json!({"ok": true, "message": "Index cleared"})),
            truncated: None,
            total_count: None,
            data: None,
        },
        Err(e) => error_output(id, "INDEX_ERROR", format!("Clear failed: {}", e)),
    }
}

pub fn dispatch(
    parsed: &ParsedSource,
    command: &str,
    id: Option<serde_json::Value>,
    max_results: Option<usize>,
) -> Result<CommandOutput, AnalyzerError> {
    match command {
        "outline" => Ok(outline(parsed, id, max_results)),
        "symbols" => Ok(symbols(parsed, id, max_results)),
        "handles" => Ok(handles(parsed, id, max_results)),
        "skeleton" => Ok(skeleton_cmd(parsed, id, max_results)),
        "check-syntax" => Ok(check_syntax_cmd(parsed, id)),
        "index-file" => Ok(index_file_cmd(parsed, id)),
        other => Err(AnalyzerError::invalid_command(other)),
    }
}

// ── check-syntax ──────────────────────────────────────────────────────

pub fn check_syntax_cmd(
    parsed: &ParsedSource,
    id: Option<serde_json::Value>,
) -> CommandOutput {
    let root = parsed.tree.root_node();
    let has_errors = root.has_error();

    let mut errors: Vec<serde_json::Value> = Vec::new();
    if has_errors {
        collect_errors(root, &mut errors, 20);
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
        status: None,
        truncated: None,
        total_count: None,
        data: Some(serde_json::json!({
            "has_errors": has_errors,
            "errors": errors,
        })),
    }
}

fn collect_errors(
    node: tree_sitter::Node,
    errors: &mut Vec<serde_json::Value>,
    max: usize,
) {
    if errors.len() >= max {
        return;
    }

    if node.is_error() {
        let start = node.start_position();
        let end = node.end_position();
        errors.push(serde_json::json!({
            "start_line": start.row + 1,
            "start_col": start.column + 1,
            "end_line": end.row + 1,
            "end_col": end.column + 1,
            "message": format!("Syntax error at line {}, column {}", start.row + 1, start.column + 1),
            "is_missing": false,
        }));
    } else if node.is_missing() {
        let start = node.start_position();
        let end = node.end_position();
        errors.push(serde_json::json!({
            "start_line": start.row + 1,
            "start_col": start.column + 1,
            "end_line": end.row + 1,
            "end_col": end.column + 1,
            "message": format!("Missing '{}' at line {}, column {}", node.kind(), start.row + 1, start.column + 1),
            "is_missing": true,
        }));
    }

    for i in 0..node.child_count() {
        if errors.len() >= max {
            return;
        }
        if let Some(child) = node.child(i) {
            collect_errors(child, errors, max);
        }
    }
}
