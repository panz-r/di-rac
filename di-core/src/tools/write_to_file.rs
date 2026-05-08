use crate::daemons::{CommandRequest, CommandResponse, UnixDaemonClient};
use crate::tools::ToolCall;
use anyhow::{Result, anyhow};
use serde_json::json;
use std::sync::Arc;

pub async fn write_to_file(
    command_client: &Arc<UnixDaemonClient>,
    call: &ToolCall,
) -> Result<serde_json::Value> {
    let path = call.args.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing path argument for write_to_file"))?;
    let content = call.args.get("content").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing content argument for write_to_file"))?;

    let create_dirs = call.args.get("create_dirs")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if create_dirs {
        let parent = std::path::Path::new(path).parent()
            .and_then(|p| p.to_str())
            .map(String::from);
        if let Some(dir) = parent {
            if !dir.is_empty() {
                let mkdir_resp: CommandResponse = command_client.send_request(CommandRequest {
                    command: "shell".to_string(),
                    args: vec![format!("mkdir -p {}", dir)],
                    cwd: None,
                }).await?;
                if !mkdir_resp.ok {
                    return Err(anyhow!("Failed to create directory {}: {}", dir, mkdir_resp.stderr));
                }
            }
        }
    }

    let resp: CommandResponse = command_client.send_request(CommandRequest {
        command: "write-file".to_string(),
        args: vec![path.to_string(), content.to_string()],
        cwd: None,
    }).await?;

    if resp.ok {
        Ok(json!({
            "path": path,
            "status": "success",
            "lines": content.lines().count()
        }))
    } else {
        Err(anyhow!("Failed to write file {}: {}", path, resp.stderr))
    }
}
