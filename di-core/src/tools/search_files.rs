use crate::daemons::{AnalyzerRequest, AnalyzerResponse, UnixDaemonClient};
use crate::tools::ToolCall;
use anyhow::{Result, anyhow};
use serde_json::json;
use std::sync::Arc;

pub async fn search_files(
    analyzer_client: &Arc<UnixDaemonClient>,
    call: &ToolCall,
) -> Result<serde_json::Value> {
    let pattern = call.args.get("pattern")
        .or_else(|| call.args.get("regex"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing pattern argument for search"))?;

    let paths: Vec<String> = call.args.get("paths")
        .or_else(|| call.args.get("path"))
        .and_then(|v| {
            v.as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .or_else(|| v.as_str().map(|s| vec![s.to_string()]))
        })
        .unwrap_or_else(|| vec![".".to_string()]);

    let _context_lines = call.args.get("context_lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(3);

    let file_pattern = call.args.get("file_pattern")
        .and_then(|v| v.as_str())
        .map(String::from);

    let mut results = Vec::new();
    for path in &paths {
        let mut query = pattern.to_string();
        if let Some(ref fp) = file_pattern {
            query = format!("{} --glob {}", pattern, fp);
        }

        let resp: AnalyzerResponse = analyzer_client.send_request(AnalyzerRequest {
            command: "grep".to_string(),
            query: Some(query),
            file: Some(path.clone()),
            content: None,
            language: None,
        })?;

        results.push(json!({
            "path": path,
            "matches": resp.data
        }));
    }

    Ok(json!({ "pattern": pattern, "results": results }))
}
