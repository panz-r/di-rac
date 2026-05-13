use crate::daemons::{AnalyzerRequest, AnalyzerResponse, ResilientDaemon};
use crate::tools::ToolCall;
use crate::tools::response::{ToolResponse, ToolErrorCode, ToolError};
use serde_json::json;
use std::sync::Arc;

const MAX_FILES_LIMIT: usize = 200;

pub async fn list_files(
    analyzer_daemon: &Arc<tokio::sync::Mutex<ResilientDaemon>>,
    call: &ToolCall,
) -> ToolResponse {
    let paths: Vec<String> = call.args.get("paths")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_else(|| {
            call.args.get("path")
                .and_then(|v| v.as_str())
                .map(|s| vec![s.to_string()])
                .unwrap_or_else(|| vec![".".to_string()])
        });

    let detail = call.args.get("detail")
        .and_then(|v| v.as_str())
        .unwrap_or("summary");

    // --detail files: walk filesystem directly for a plain file listing
    if detail == "files" {
        return list_files_filesystem(&paths);
    }

    // Default: use analyzer for symbol-aware listing
    let mut output_parts: Vec<String> = Vec::new();

    for path in &paths {
        match analyzer_daemon.lock().await.send_request::<_, AnalyzerResponse>(AnalyzerRequest {
            command: "repo-map".to_string(),
            file: Some(path.clone()),
            content: None,
            language: None,
            query: None,
        }).await {
            Ok(resp) if resp.ok => {
                let section = format_repo_results(path, &resp.data);
                output_parts.push(section);
            }
            Ok(resp) => {
                return ToolResponse::Failure {
                    error: ToolError::new(ToolErrorCode::ToolInternalError, format!("Failed to list: {:?}", resp.data), "repo")
                        .with_details(json!({ "path": path })),
                    metadata: None,
                };
            }
            Err(e) => {
                return ToolResponse::Failure {
                    error: ToolError::new(ToolErrorCode::DaemonUnavailable, e.to_string(), "repo")
                        .with_details(json!({ "path": path })),
                    metadata: None,
                };
            }
        }
    }

    ToolResponse::ok(json!(output_parts.join("\n\n")))
}

// ---------------------------------------------------------------------------
// --detail files: filesystem walk with mtime sorting
// ---------------------------------------------------------------------------

fn list_files_filesystem(paths: &[String]) -> ToolResponse {
    let mut all_output: Vec<String> = Vec::new();

    for root in paths {
        let mut entries: Vec<FileEntry> = Vec::new();
        collect_files(std::path::Path::new(root), &mut entries, &mut 0);

        // Sort by modification time, newest first
        entries.sort_by(|a, b| b.mtime.cmp(&a.mtime));

        // Truncate if over limit
        let limit_hit = entries.len() > MAX_FILES_LIMIT;
        entries.truncate(MAX_FILES_LIMIT);

        if entries.is_empty() {
            all_output.push(format!("Contents of {}:\n  (empty directory)", root));
            continue;
        }

        let mut lines = vec![format!("Contents of {}:", root)];
        for entry in &entries {
            lines.push(format!("  {} ({} lines)", entry.relative_path, entry.line_count));
        }

        if limit_hit {
            lines.push(format!("  ... (showing {} of more files, use more specific path)", MAX_FILES_LIMIT));
        }

        all_output.push(lines.join("\n"));
    }

    let separator = format!("\\n\n{}\n\n", "=".repeat(20));
    ToolResponse::ok(json!(all_output.join(&separator)))
}

struct FileEntry {
    relative_path: String,
    line_count: usize,
    mtime: std::time::SystemTime,
}

/// Patterns to skip during filesystem walk (matching diracignore defaults).
const SKIP_DIRS: &[&str] = &[
    "node_modules", ".git", ".hg", ".svn", "__pycache__", ".cache",
    "target", "dist", "build", ".next", ".nuxt", ".turbo", "vendor",
    ".tox", ".mypy_cache", ".pytest_cache", ".direnv",
];

const SKIP_FILES: &[&str] = &[
    ".DS_Store", "Thumbs.db",
];

fn collect_files(dir: &std::path::Path, entries: &mut Vec<FileEntry>, count: &mut usize) {
    if *count >= MAX_FILES_LIMIT * 2 {
        return; // Safety limit for recursion
    }

    let Ok(read_dir) = std::fs::read_dir(dir) else { return };

    for entry in read_dir.flatten() {
        let path = entry.path();
        let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();

        if path.is_dir() {
            // Skip hidden dirs and known skip dirs
            if file_name.starts_with('.') || SKIP_DIRS.contains(&file_name.as_str()) {
                continue;
            }
            collect_files(&path, entries, count);
        } else {
            // Skip hidden files and known skip files
            if file_name.starts_with('.') || SKIP_FILES.contains(&file_name.as_str()) {
                continue;
            }

            let mtime = entry.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);

            // Read file to count lines (skip binary/large files)
            let line_count = std::fs::read_to_string(&path)
                .map(|c| c.lines().count())
                .unwrap_or(0);

            let relative_path = path.to_string_lossy().replace('\\', "/");

            entries.push(FileEntry {
                relative_path,
                line_count,
                mtime,
            });
            *count += 1;

            if *count >= MAX_FILES_LIMIT * 2 {
                return;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Default mode: analyzer symbol-based listing
// ---------------------------------------------------------------------------

/// Format analyzer repo-map data as human-readable text.
fn format_repo_results(root_path: &str, data: &serde_json::Value) -> String {
    let files = data.get("files").and_then(|v| v.as_array());
    match files {
        Some(files) if !files.is_empty() => {
            let mut lines = Vec::new();
            lines.push(format!("{} ({} files with symbols)", root_path, files.len()));
            for entry in files {
                let file_path = entry.get("file").and_then(|v| v.as_str()).unwrap_or("?");
                let symbols = entry.get("symbols").and_then(|v| v.as_array());
                if let Some(syms) = symbols {
                    let sym_strs: Vec<String> = syms.iter()
                        .filter_map(|s| {
                            let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                            let kind = s.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
                            Some(format!("{} {}", kind, name))
                        })
                        .collect();
                    lines.push(format!("  {} [{}]", file_path, sym_strs.join(", ")));
                } else {
                    lines.push(format!("  {}", file_path));
                }
            }
            lines.join("\n")
        }
        _ => format!("{} (no files with symbols found)", root_path),
    }
}
