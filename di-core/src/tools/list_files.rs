use crate::daemons::{AnalyzerRequest, AnalyzerResponse, UnixDaemonClient};
use crate::tools::ToolCall;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

pub async fn list_files(
    analyzer_client: &Arc<UnixDaemonClient>,
    call: &ToolCall,
) -> Result<serde_json::Value> {
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
        let resp: AnalyzerResponse = analyzer_client.send_request(AnalyzerRequest {
            command: "list-files".to_string(),
            file: Some(path.clone()),
            content: None,
            language: None,
            query: None,
        }).await?;

        if resp.ok {
            results.push(json!({
                "path": path,
                "recursive": recursive,
                "entries": resp.data
            }));
        } else {
            results.push(json!({
                "path": path,
                "error": format!("Failed to list: {:?}", resp.data)
            }));
        }
    }

    Ok(json!({ "results": results }))
}
