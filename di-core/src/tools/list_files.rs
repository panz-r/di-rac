use crate::daemons::{AnalyzerRequest, AnalyzerResponse, ResilientDaemon};
use crate::tools::ToolCall;
use crate::tools::response::{ToolResponse, ToolErrorCode, ToolError};
use serde_json::json;
use std::sync::Arc;

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

/// Format analyzer repo-map data as human-readable text.
/// Input: {"files": [{"file": "path", "symbols": [{"name": "...", "kind": "..."}]}]}
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
