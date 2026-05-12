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

    let recursive = call.args.get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut results = Vec::new();
    for path in &paths {
        match analyzer_daemon.lock().await.send_request::<_, AnalyzerResponse>(AnalyzerRequest {
            command: "repo-map".to_string(),
            file: Some(path.clone()),
            content: None,
            language: None,
            query: None,
        }).await {
            Ok(resp) if resp.ok => {
                results.push(json!({
                    "path": path,
                    "recursive": recursive,
                    "entries": resp.data
                }));
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

    ToolResponse::ok(json!({ "results": results }))
}
