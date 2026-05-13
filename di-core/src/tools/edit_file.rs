use crate::daemons::ResilientDaemon;
use crate::tools::ToolCall;
use crate::tools::response::{ToolResponse, ToolErrorCode, ToolError};
use serde_json::json;
use std::sync::Arc;

pub async fn edit_file(
    _command_daemon: &Arc<tokio::sync::Mutex<ResilientDaemon>>,
    call: &ToolCall,
) -> ToolResponse {
    let edits = match parse_edits(call) {
        Ok(e) => e,
        Err(e) => return ToolResponse::fail(ToolErrorCode::MissingArgument, e, "edit"),
    };

    let dry_run = call.args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut results = Vec::new();
    for (path, old, new) in edits {
        let content = match tokio::fs::read_to_string(&path).await {
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

        if dry_run {
            let diff = build_diff(&old, &new);
            results.push(json!({ "path": path, "status": "dry_run", "diff": diff }));
        } else {
            let new_content = content.replacen(&old, &new, 1);
            match tokio::fs::write(&path, &new_content).await {
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
    }

    ToolResponse::ok(json!({ "edits": results }))
}

fn build_diff(old: &str, new: &str) -> String {
    let mut diff = String::new();
    for line in old.lines() {
        diff.push_str(&format!("-{}\n", line));
    }
    for line in new.lines() {
        diff.push_str(&format!("+{}\n", line));
    }
    diff
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
