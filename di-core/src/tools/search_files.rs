use crate::daemons::ResilientDaemon;
use crate::tools::ToolCall;
use crate::tools::response::{ToolResponse, ToolErrorCode};
use serde_json::json;
use std::sync::Arc;

const MAX_RESULTS: usize = 30;
const MAX_LINE_LENGTH: usize = 300;

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
        .unwrap_or(3)
        .min(10);

    let file_pattern = call.args.get("file_pattern")
        .and_then(|v| v.as_str())
        .map(String::from);

    let mut all_output = Vec::new();
    let mut total_matches = 0usize;

    for path in &paths {
        let mut rg_args = vec![
            "rg".to_string(),
            "--json".to_string(),
            "-e".to_string(),
            pattern.clone(),
            "--max-count".to_string(),
            MAX_RESULTS.to_string(),
        ];

        if let Some(ref fp) = file_pattern {
            rg_args.push("--glob".to_string());
            rg_args.push(fp.clone());
        }

        if context_lines > 0 {
            rg_args.push("--context".to_string());
            rg_args.push(context_lines.to_string());
        }

        rg_args.push(path.clone());

        let command = build_rg_command(&rg_args);

        let request = json!({
            "type": "execute",
            "command": command,
        });
        let result: crate::daemons::ExecuteResult = match command_daemon.lock().await.send_request(request).await {
            Ok(r) => r,
            Err(e) => {
                return ToolResponse::fail(
                    ToolErrorCode::DaemonUnavailable,
                    format!("Failed to execute ripgrep: {}", e),
                    "search",
                );
            }
        };

        // rg exits with code 1 when no matches found — that's not an error
        if result.exit_code != 0 && result.exit_code != 1 {
            let stderr = result.stderr.trim();
            if !stderr.is_empty() {
                return ToolResponse::fail(
                    ToolErrorCode::DaemonUnavailable,
                    format!("ripgrep error: {}", stderr),
                    "search",
                );
            }
        }

        let parsed = parse_rg_json_output(&result.stdout);
        total_matches += parsed.match_count;
        all_output.push(format_search_results(&parsed, path));
    }

    let combined = all_output.join("\n\n");
    let output = if combined.is_empty() {
        format!("No results found for pattern '{}' in {:?} ({} files searched)", pattern, paths, paths.len())
    } else {
        combined
    };

    ToolResponse::ok(json!({
        "pattern": pattern,
        "match_count": total_matches,
        "results": output,
    }))
}

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

fn parse_rg_json_output(raw: &str) -> ParsedResults {
    let mut files: Vec<FileResult> = Vec::new();
    let mut current_file: Option<FileResult> = None;
    let mut match_count = 0usize;

    for line in raw.lines() {
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
                    let content = truncate_line(content);

                    cf.lines.push(LineResult {
                        line_number,
                        content,
                        is_match: true,
                    });
                    match_count += 1;
                }
            }
            "context" => {
                if let (Some(data), Some(ref mut cf)) = (val.get("data"), current_file.as_mut()) {
                    let line_number = data.get("line_number").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let content = data.get("lines")
                        .and_then(|l| l.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    let content = truncate_line(content);

                    cf.lines.push(LineResult {
                        line_number,
                        content,
                        is_match: false,
                    });
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

fn truncate_line(s: &str) -> String {
    if s.len() <= MAX_LINE_LENGTH {
        s.trim_end().to_string()
    } else {
        format!("{}...", &s[..MAX_LINE_LENGTH])
    }
}

fn format_search_results(parsed: &ParsedResults, _search_path: &str) -> String {
    if parsed.files.is_empty() {
        return String::new();
    }

    let mut output = String::new();

    for file in &parsed.files {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&format!("{}\n", file.path));
        output.push_str("---\n");

        let mut prev_line: Option<usize> = None;
        for line in &file.lines {
            if let Some(prev) = prev_line {
                if line.line_number > prev + 1 {
                    output.push_str("...\n");
                }
            }

            let prefix = if line.is_match { ">" } else { " " };
            output.push_str(&format!("{} {:>4} | {}\n", prefix, line.line_number, line.content));
            prev_line = Some(line.line_number);
        }
    }

    output
}

fn build_rg_command(args: &[String]) -> String {
    args.iter().map(|a| shell_escape(a)).collect::<Vec<_>>().join(" ")
}

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
