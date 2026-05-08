use crate::daemons::{UnixDaemonClient, AnalyzerRequest, AnalyzerResponse};
use crate::tools::ToolCall;
use anyhow::{Result, anyhow};
use serde_json::json;
use std::sync::Arc;

/// AST symbol operations: search definitions, replace bodies, rename, find references.
pub async fn symbols(
    analyzer_client: &Arc<UnixDaemonClient>,
    call: &ToolCall,
) -> Result<serde_json::Value> {
    let subcommand = call.args.get("subcommand")
        .or_else(|| call.args.get("action"))
        .and_then(|v| v.as_str())
        .unwrap_or("search");

    match subcommand {
        "search" => symbol_search(analyzer_client, call).await,
        "replace" => symbol_replace(analyzer_client, call).await,
        "rename" => symbol_rename(analyzer_client, call).await,
        "refs" => symbol_refs(analyzer_client, call).await,
        _ => Err(anyhow!("Unknown symbols subcommand: {}. Use search, replace, rename, or refs.", subcommand)),
    }
}

async fn symbol_search(
    analyzer_client: &Arc<UnixDaemonClient>,
    call: &ToolCall,
) -> Result<serde_json::Value> {
    let name = call.args.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let kind = call.args.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let path = call.args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    let resp: AnalyzerResponse = analyzer_client.send_request(AnalyzerRequest {
        command: "symbols-search".to_string(),
        query: Some(name.to_string()),
        file: Some(path.to_string()),
        content: if kind.is_empty() { None } else { Some(kind.to_string()) },
        language: None,
    }).await?;

    if resp.ok {
        Ok(resp.data)
    } else {
        Ok(json!({ "matches": 0, "hint": "Try without --kind or use search for text patterns" }))
    }
}

async fn symbol_replace(
    analyzer_client: &Arc<UnixDaemonClient>,
    call: &ToolCall,
) -> Result<serde_json::Value> {
    let name = call.args.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing name for symbols replace"))?;
    let text = call.args.get("text").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing text for symbols replace"))?;

    let resp: AnalyzerResponse = analyzer_client.send_request(AnalyzerRequest {
        command: "symbols-replace".to_string(),
        query: Some(name.to_string()),
        file: None,
        content: Some(text.to_string()),
        language: None,
    }).await?;

    if resp.ok {
        Ok(resp.data)
    } else {
        Err(anyhow!("Symbol replace failed: {:?}", resp.data))
    }
}

async fn symbol_rename(
    analyzer_client: &Arc<UnixDaemonClient>,
    call: &ToolCall,
) -> Result<serde_json::Value> {
    let old_name = call.args.get("old").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing old name for symbols rename"))?;
    let new_name = call.args.get("new").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing new name for symbols rename"))?;

    let resp: AnalyzerResponse = analyzer_client.send_request(AnalyzerRequest {
        command: "symbols-rename".to_string(),
        query: Some(format!("{}:{}", old_name, new_name)),
        file: None,
        content: None,
        language: None,
    }).await?;

    if resp.ok {
        Ok(resp.data)
    } else {
        Err(anyhow!("Symbol rename failed: {:?}", resp.data))
    }
}

async fn symbol_refs(
    analyzer_client: &Arc<UnixDaemonClient>,
    call: &ToolCall,
) -> Result<serde_json::Value> {
    let name = call.args.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing name for symbols refs"))?;

    let resp: AnalyzerResponse = analyzer_client.send_request(AnalyzerRequest {
        command: "symbols-refs".to_string(),
        query: Some(name.to_string()),
        file: None,
        content: None,
        language: None,
    }).await?;

    if resp.ok {
        Ok(resp.data)
    } else {
        Ok(json!({ "matches": 0, "hint": "Try symbols search to find the symbol first" }))
    }
}
