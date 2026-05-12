use crate::daemons::CommandDaemon;
use crate::tools::ToolCall;
use crate::tools::response::{ToolResponse, ToolErrorCode, ToolError};
use serde_json::json;
use std::sync::Arc;

pub async fn edit_file(
    _command_daemon: &Arc<tokio::sync::Mutex<CommandDaemon>>,
    call: &ToolCall,
) -> ToolResponse {
    let edits = match parse_edits(call) {
        Ok(e) => e,
        Err(e) => return ToolResponse::fail(ToolErrorCode::MissingArgument, e, "edit"),
    };

    let mut results = Vec::new();
    for (path, old, new) in edits {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                return ToolResponse::fail(
                    ToolErrorCode::IoFileNotFound,
                    format!("Failed to read {}: {}", path, e),
                    "edit",
                );
            }
        };

        if !content.contains(&old) {
            return ToolResponse::Failure {
                error: ToolError::new(
                    ToolErrorCode::AnchorNotFound,
                    format!("old_text not found in {}", path),
                    "edit",
                ).with_details(json!({ "path": path })),
                metadata: None,
            };
        }

        let occurrences = content.matches(&old).count();
        if occurrences > 1 {
            return ToolResponse::Failure {
                error: ToolError::new(
                    ToolErrorCode::AnchorAmbiguous,
                    format!("old_text found {} times in {} (expected exactly 1)", occurrences, path),
                    "edit",
                ).with_details(json!({ "path": path })),
                metadata: None,
            };
        }

        let new_content = content.replacen(&old, &new, 1);
        match std::fs::write(&path, &new_content) {
            Ok(_) => results.push(json!({ "path": path, "status": "success" })),
            Err(e) => {
                return ToolResponse::fail(
                    ToolErrorCode::PatchApplyFailed,
                    format!("Failed to write {}: {}", path, e),
                    "edit",
                );
            }
        }
    }

    ToolResponse::ok(json!({ "edits": results }))
}

fn parse_edits(call: &ToolCall) -> Result<Vec<(String, String, String)>, String> {
    if let Some(files) = call.args.get("files").and_then(|v| v.as_array()) {
        let mut all_edits = Vec::new();
        for file_entry in files {
            let path = file_entry.get("path").and_then(|v| v.as_str())
                .ok_or_else(|| "Missing path in files array".to_string())?;
            let edits = file_entry.get("edits").and_then(|v| v.as_array())
                .ok_or_else(|| "Missing edits in files array".to_string())?;
            for edit in edits {
                let old = edit.get("old_text").or_else(|| edit.get("old_string"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing old_text in edit".to_string())?;
                let new = edit.get("new_text").or_else(|| edit.get("new_string"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing new_text in edit".to_string())?;
                all_edits.push((path.to_string(), old.to_string(), new.to_string()));
            }
        }
        Ok(all_edits)
    } else {
        let path = call.args.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| "Missing path argument for edit_file".to_string())?;
        let edits = call.args.get("edits").and_then(|v| v.as_array())
            .ok_or_else(|| "Missing edits array".to_string())?;
        let mut all_edits = Vec::new();
        for edit in edits {
            let old = edit.get("old_text").or_else(|| edit.get("old_string"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing old_text in edit".to_string())?;
            let new = edit.get("new_text").or_else(|| edit.get("new_string"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing new_text in edit".to_string())?;
            all_edits.push((path.to_string(), old.to_string(), new.to_string()));
        }
        Ok(all_edits)
    }
}
