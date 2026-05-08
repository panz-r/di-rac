use crate::daemons::{CommandRequest, CommandResponse, UnixDaemonClient};
use crate::tools::ToolCall;
use anyhow::{Result, anyhow};
use serde_json::json;
use std::sync::Arc;

pub async fn edit_file(
    command_client: &Arc<UnixDaemonClient>,
    call: &ToolCall,
) -> Result<serde_json::Value> {
    // Support both single-file and multi-file batch editing
    let edits = if let Some(files) = call.args.get("files").and_then(|v| v.as_array()) {
        // Multi-file batch: [{path, edits: [{old_text, new_text}]}]
        let mut all_edits = Vec::new();
        for file_entry in files {
            let path = file_entry.get("path").and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing path in files array"))?;
            let edits = file_entry.get("edits").and_then(|v| v.as_array())
                .ok_or_else(|| anyhow!("Missing edits in files array"))?;
            for edit in edits {
                let old = edit.get("old_text").or_else(|| edit.get("old_string"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing old_text in edit"))?;
                let new = edit.get("new_text").or_else(|| edit.get("new_string"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing new_text in edit"))?;
                all_edits.push((path.to_string(), old.to_string(), new.to_string()));
            }
        }
        all_edits
    } else {
        // Single file
        let path = call.args.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing path argument for edit_file"))?;
        let edits = call.args.get("edits").and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("Missing edits array"))?;
        let mut all_edits = Vec::new();
        for edit in edits {
            let old = edit.get("old_text").or_else(|| edit.get("old_string"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing old_text in edit"))?;
            let new = edit.get("new_text").or_else(|| edit.get("new_string"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing new_text in edit"))?;
            all_edits.push((path.to_string(), old.to_string(), new.to_string()));
        }
        all_edits
    };

    let mut results = Vec::new();
    for (path, old, new) in edits {
        let resp: CommandResponse = command_client.send_request(CommandRequest {
            command: "replace".to_string(),
            args: vec![path.clone(), old, new],
            cwd: None,
        }).await?;

        results.push(if resp.ok {
            json!({ "path": path, "status": "success" })
        } else {
            json!({ "path": path, "status": "error", "error": resp.stderr })
        });
    }

    Ok(json!({ "edits": results }))
}
