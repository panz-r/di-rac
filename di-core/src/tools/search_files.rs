use crate::daemons::ResilientDaemon;
use crate::tools::ToolCall;
use crate::tools::response::{ToolResponse, ToolErrorCode};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::OnceCell;

const MAX_RESULTS: usize = 30;
const MAX_LINE_LENGTH: usize = 300;
const MAX_BYTE_SIZE: usize = 104_857; // 0.1 * 1024 * 1024
const MAX_RG_JSON_LINES: usize = 150; // MAX_RESULTS * 5

// ---------------------------------------------------------------------------
// Backend detection
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum SearchBackend {
    Ripgrep,
    Grep,
}

static BACKEND: OnceCell<SearchBackend> = OnceCell::const_new();

async fn detect_backend(command_daemon: &Arc<tokio::sync::Mutex<ResilientDaemon>>) -> SearchBackend {
    if let Some(&b) = BACKEND.get() {
        return b;
    }
    let request = json!({"type": "execute", "command": "rg --version"});
    let backend = match command_daemon.lock().await.send_request::<serde_json::Value, crate::daemons::ExecuteResult>(request).await {
        Ok(r) if r.exit_code == 0 => SearchBackend::Ripgrep,
        _ => SearchBackend::Grep,
    };
    let _ = BACKEND.set(backend);
    backend
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn search_files(
    command_daemon: &Arc<tokio::sync::Mutex<ResilientDaemon>>,
    call: &ToolCall,
) -> ToolResponse {
    let pattern = match call.args.get("pattern")
        .or_else(|| call.args.get("regex"))
        .and_then(|v| v.as_str())
    {
        Some(p) => p.to_string(),
        None => return ToolResponse::fail(ToolErrorCode::MissingArgument, "Missing pattern argument for search", "search"),
    };

    let paths: Vec<String> = call.args.get("paths")
        .or_else(|| call.args.get("path"))
        .and_then(|v| {
            v.as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .or_else(|| v.as_str().map(|s| vec![s.to_string()]))
        })
        .unwrap_or_else(|| vec![".".to_string()]);

    let context_lines = call.args.get("context_lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .min(10) as usize;

    let file_pattern = call.args.get("file_pattern")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "*".to_string());

    let backend = detect_backend(command_daemon).await;

    let mut all_parsed: Vec<ParsedResults> = Vec::new();
    let mut total_matches = 0usize;

    for search_path in &paths {
        let command = match backend {
            SearchBackend::Ripgrep => build_rg_command(&pattern, &file_pattern, context_lines, search_path),
            SearchBackend::Grep => build_grep_command(&pattern, &file_pattern, context_lines, search_path),
        };

        let request = json!({
            "type": "execute",
            "command": command,
        });
        let result: crate::daemons::ExecuteResult = match command_daemon.lock().await.send_request::<serde_json::Value, crate::daemons::ExecuteResult>(request).await {
            Ok(r) => r,
            Err(e) => {
                return ToolResponse::fail(
                    ToolErrorCode::DaemonUnavailable,
                    format!("Failed to execute search: {}", e),
                    "search",
                );
            }
        };

        // rg exits with code 1 when no matches found — that's not an error
        // grep exits with code 1 when no matches found — same
        if result.exit_code > 1 {
            let stderr = result.stderr.trim();
            if !stderr.is_empty() {
                return ToolResponse::fail(
                    ToolErrorCode::DaemonUnavailable,
                    format!("Search error: {}", stderr),
                    "search",
                );
            }
        }

        let parsed = match backend {
            SearchBackend::Ripgrep => parse_rg_json_output(&result.stdout),
            SearchBackend::Grep => parse_grep_output(&result.stdout),
        };
        total_matches += parsed.match_count;
        all_parsed.push(parsed);
    }

    // Merge all parsed results into a single list for formatting
    let merged_files: Vec<FileResult> = all_parsed.into_iter().flat_map(|p| p.files).collect();
    let merged = ParsedResults { files: merged_files, match_count: total_matches };

    let output = format_results(&merged, total_matches);

    ToolResponse::ok(json!({
        "pattern": pattern,
        "match_count": total_matches,
        "results": output,
    }))
}

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

struct ParsedResults {
    files: Vec<FileResult>,
    match_count: usize,
}

struct FileResult {
    path: String,
    lines: Vec<LineResult>,
}

struct LineResult {
    line_number: usize,
    content: String,
    is_match: bool,
}

// ---------------------------------------------------------------------------
// Ripgrep command & parsing
// ---------------------------------------------------------------------------

fn build_rg_command(pattern: &str, file_pattern: &str, context_lines: usize, search_path: &str) -> String {
    let mut args = vec![
        "rg".to_string(),
        "--json".to_string(),
        "-e".to_string(),
        pattern.to_string(),
        "--max-count".to_string(),
        MAX_RESULTS.to_string(),
        "--glob".to_string(),
        file_pattern.to_string(),
        // Dotfile exclusion
        "--glob".to_string(),
        "!.*".to_string(),
        "--glob".to_string(),
        "!**/.*".to_string(),
    ];

    if context_lines > 0 {
        args.push("--context".to_string());
        args.push(context_lines.to_string());
    }

    args.push(search_path.to_string());
    args.iter().map(|a| shell_escape(a)).collect::<Vec<_>>().join(" ")
}

fn parse_rg_json_output(raw: &str) -> ParsedResults {
    let mut files: Vec<FileResult> = Vec::new();
    let mut current_file: Option<FileResult> = None;
    let mut match_count = 0usize;
    let mut line_count = 0usize;

    for line in raw.lines() {
        line_count += 1;
        if line_count > MAX_RG_JSON_LINES {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match msg_type {
            "begin" => {
                if let Some(path) = val.get("data").and_then(|d| d.get("path")).and_then(|p| p.get("text")).and_then(|t| t.as_str()) {
                    if let Some(cf) = current_file.take() {
                        files.push(cf);
                    }
                    current_file = Some(FileResult {
                        path: path.to_string(),
                        lines: Vec::new(),
                    });
                }
            }
            "match" => {
                if let (Some(data), Some(ref mut cf)) = (val.get("data"), current_file.as_mut()) {
                    let line_number = data.get("line_number").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let content = data.get("lines")
                        .and_then(|l| l.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    if match_count < MAX_RESULTS {
                        cf.lines.push(LineResult {
                            line_number,
                            content: content.trim_end().to_string(),
                            is_match: true,
                        });
                    }
                    match_count += 1;
                }
            }
            "context" => {
                if let (Some(data), Some(ref mut cf)) = (val.get("data"), current_file.as_mut()) {
                    let line_number = data.get("line_number").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    // Don't overwrite match with context if they overlap
                    let already_match = cf.lines.iter().any(|l| l.line_number == line_number && l.is_match);
                    if !already_match {
                        let content = data.get("lines")
                            .and_then(|l| l.get("text"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        cf.lines.push(LineResult {
                            line_number,
                            content: content.trim_end().to_string(),
                            is_match: false,
                        });
                    }
                }
            }
            "end" => {
                if let Some(cf) = current_file.take() {
                    files.push(cf);
                }
            }
            _ => {}
        }
    }

    if let Some(cf) = current_file.take() {
        files.push(cf);
    }

    ParsedResults { files, match_count }
}

// ---------------------------------------------------------------------------
// Grep command & parsing
// ---------------------------------------------------------------------------

fn build_grep_command(pattern: &str, file_pattern: &str, context_lines: usize, search_path: &str) -> String {
    let mut args = vec![
        "grep".to_string(),
        "-rnE".to_string(),
        "-e".to_string(),
        pattern.to_string(),
        "--include".to_string(),
        file_pattern.to_string(),
        // Dotfile exclusion
        "--exclude".to_string(),
        ".*".to_string(),
        "--exclude-dir".to_string(),
        ".*".to_string(),
    ];

    if context_lines > 0 {
        args.push("-C".to_string());
        args.push(context_lines.to_string());
    }

    args.push(search_path.to_string());
    args.iter().map(|a| shell_escape(a)).collect::<Vec<_>>().join(" ")
}

/// Parse grep -rnE output.
/// Match lines:   `filepath:linenum:content`
/// Context lines: `filepath-linenum-content`
/// Group separators: `--`
fn parse_grep_output(raw: &str) -> ParsedResults {
    let mut files: Vec<FileResult> = Vec::new();
    let mut files_by_path: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut match_count = 0usize;

    for line in raw.lines() {
        let line = line.trim_end();
        if line.is_empty() || line == "--" {
            continue;
        }
        if match_count >= MAX_RESULTS + (MAX_RESULTS * context_lines_estimate()) {
            break;
        }

        // Try to split into filepath:linenum:content or filepath-linenum-content
        let (filepath, linenum, content, is_match) = match parse_grep_line(line) {
            Some(v) => v,
            None => continue,
        };

        if is_match {
            match_count += 1;
            if match_count > MAX_RESULTS {
                continue; // Keep counting but don't add more match lines
            }
        }

        let file_idx = *files_by_path.entry(filepath.clone()).or_insert_with(|| {
            files.push(FileResult { path: filepath, lines: Vec::new() });
            files.len() - 1
        });

        files[file_idx].lines.push(LineResult {
            line_number: linenum,
            content: content.trim_end().to_string(),
            is_match,
        });
    }

    ParsedResults { files, match_count }
}

/// Rough estimate of context lines per match for output limiting.
fn context_lines_estimate() -> usize {
    6 // conservative upper bound per match
}

/// Parse a single grep output line into (filepath, linenum, content, is_match).
fn parse_grep_line(line: &str) -> Option<(String, usize, &str, bool)> {
    // Match lines use ':' after line number; context lines use '-'
    // We need to find the line number separator. The filepath may contain
    // colons (rare) but the linenum is always a number.
    // Strategy: find the first ':' or '-' that is followed by digits then
    // ':' or '-' then content.

    let bytes = line.as_bytes();
    let len = bytes.len();

    // Try all positions for the first separator
    for i in 1..len.saturating_sub(3) {
        let c = bytes[i];
        if c != b':' && c != b'-' {
            continue;
        }

        // Scan forward for digits (line number)
        let mut j = i + 1;
        while j < len && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j == i + 1 || j >= len {
            continue; // No digits found
        }

        // The character after the digits must match the first separator
        if bytes[j] != c {
            continue;
        }

        let filepath = &line[..i];
        let linenum_str = &line[i + 1..j];
        let content = &line[j + 1..];

        if let Ok(linenum) = linenum_str.parse::<usize>() {
            let is_match = c == b':';
            return Some((filepath.to_string(), linenum, content, is_match));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// TS-parity output formatting
// ---------------------------------------------------------------------------

fn format_results(parsed: &ParsedResults, total_match_count: usize) -> String {
    if parsed.files.is_empty() || total_match_count == 0 {
        return "Found 0 results.".to_string();
    }

    let mut output = String::new();

    // Header
    let header = if total_match_count >= MAX_RESULTS {
        format!("Showing first {} of {}+ results. Use a more specific search if necessary.\n\n", MAX_RESULTS, total_match_count)
    } else if total_match_count == 1 {
        "Found 1 result.\n\n".to_string()
    } else {
        format!("Found {} results.\n\n", total_match_count)
    };
    output.push_str(&header);
    let mut byte_size = output.len();

    let mut was_limit_reached = false;

    for file in &parsed.files {
        // Read full file for anchor computation
        let anchor_index = match std::fs::read_to_string(&file.path) {
            Ok(content) => {
                let lines: Vec<&str> = content.split('\n').collect();
                crate::util::FileAnchorIndex::new(&lines)
            }
            Err(_) => crate::util::FileAnchorIndex::new(&[] as &[&str]),
        };

        // Relative posix path
        let rel_path = relative_posix_path(&file.path);
        let file_header = format!("{}\n│----\n", rel_path);
        let header_bytes = file_header.len();

        if byte_size + header_bytes >= MAX_BYTE_SIZE {
            was_limit_reached = true;
            break;
        }
        output.push_str(&file_header);
        byte_size += header_bytes;

        let mut file_skipped = 0usize;
        let mut last_line_num: Option<usize> = None;

        for line in &file.lines {
            // Skip long lines (don't truncate)
            if line.content.len() > MAX_LINE_LENGTH {
                if line.is_match {
                    file_skipped += 1;
                }
                continue;
            }

            // Insert separator for non-contiguous lines
            if let Some(last) = last_line_num {
                if line.line_number != last + 1 {
                    let sep = "│----\n";
                    if byte_size + sep.len() >= MAX_BYTE_SIZE {
                        was_limit_reached = true;
                        break;
                    }
                    output.push_str(sep);
                    byte_size += sep.len();
                }
            }

            // Get anchor hash (1-indexed line -> 0-indexed)
            let anchor = anchor_index.get_hash(line.line_number - 1);
            let anchor_str = if anchor.is_empty() {
                format!("L{}", line.line_number)
            } else {
                anchor.to_string()
            };

            let trimmed = line.content.trim_end();
            let line_str = format!("│{}|{}\n", anchor_str, trimmed);
            let line_bytes = line_str.len();

            if byte_size + line_bytes >= MAX_BYTE_SIZE {
                was_limit_reached = true;
                break;
            }

            output.push_str(&line_str);
            byte_size += line_bytes;
            last_line_num = Some(line.line_number);
        }

        if was_limit_reached {
            break;
        }

        // Skipped results note
        if file_skipped > 0 {
            let note = format!("│ ({} result{} skipped due to line length limits)\n",
                file_skipped, if file_skipped > 1 { "s" } else { "" });
            if byte_size + note.len() < MAX_BYTE_SIZE {
                output.push_str(&note);
                byte_size += note.len();
            }
        }

        // Closing separator
        let closing = "│----\n\n";
        if byte_size + closing.len() < MAX_BYTE_SIZE {
            output.push_str(closing);
            byte_size += closing.len();
        } else {
            was_limit_reached = true;
            break;
        }
    }

    if was_limit_reached {
        let truncation = "\n[Results truncated due to exceeding the 0.1MB size limit. Please use a more specific search pattern.]";
        if byte_size + truncation.len() < MAX_BYTE_SIZE {
            output.push_str(truncation);
        }
    }

    output.trim().to_string()
}

/// Convert an absolute path to relative posix format.
fn relative_posix_path(abs_path: &str) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let path = Path::new(abs_path);

    let rel = if path.is_absolute() {
        pathdiff::diff_paths(path, &cwd).unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    };

    rel.to_string_lossy().replace('\\', "/")
}

// ---------------------------------------------------------------------------
// Shell escaping
// ---------------------------------------------------------------------------

fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let needs_quoting = s.chars().any(|c| {
        !c.is_alphanumeric() && c != '-' && c != '_' && c != '.' && c != '/' && c != '@' && c != '+'
    });
    if needs_quoting {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grep_line_parsing_match() {
        let (path, num, content, is_match) = parse_grep_line("src/main.rs:42:fn hello() {").unwrap();
        assert_eq!(path, "src/main.rs");
        assert_eq!(num, 42);
        assert_eq!(content, "fn hello() {");
        assert!(is_match);
    }

    #[test]
    fn grep_line_parsing_context() {
        let (path, num, content, is_match) = parse_grep_line("src/main.rs-41-// comment").unwrap();
        assert_eq!(path, "src/main.rs");
        assert_eq!(num, 41);
        assert_eq!(content, "// comment");
        assert!(!is_match);
    }

    #[test]
    fn grep_line_parsing_group_separator() {
        assert!(parse_grep_line("--").is_none());
    }

    #[test]
    fn format_empty_results() {
        let parsed = ParsedResults { files: vec![], match_count: 0 };
        let output = format_results(&parsed, 0);
        assert_eq!(output, "Found 0 results.");
    }

    #[test]
    fn format_header_singular() {
        let parsed = ParsedResults {
            files: vec![FileResult {
                path: "test.rs".to_string(),
                lines: vec![LineResult { line_number: 1, content: "hello".to_string(), is_match: true }],
            }],
            match_count: 1,
        };
        let output = format_results(&parsed, 1);
        assert!(output.starts_with("Found 1 result."));
        assert!(output.contains("│----"));
    }

    #[test]
    fn format_header_capped() {
        let parsed = ParsedResults {
            files: vec![FileResult {
                path: "test.rs".to_string(),
                lines: vec![LineResult { line_number: 1, content: "hello".to_string(), is_match: true }],
            }],
            match_count: 30,
        };
        let output = format_results(&parsed, 30);
        assert!(output.starts_with("Showing first 30 of 30+ results."));
    }

    #[test]
    fn shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "hello");
    }

    #[test]
    fn shell_escape_special() {
        assert_eq!(shell_escape("hello world"), "'hello world'");
    }

    #[test]
    fn shell_escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }
}
