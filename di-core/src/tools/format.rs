use super::response::ToolError;

// ---------------------------------------------------------------------------
// LLM error formatting - policy-driven, redacted by default
// Produces <tool_error severity="..."> XML tags matching TS formatToolErrorForLLM
// ---------------------------------------------------------------------------

/// Registry of LLM-safe error templates keyed by error code.
/// Details are selectively included: file paths yes, stack traces no,
/// input hashes no, internal routing no.
const LLM_TEMPLATES: &[(&str, &str)] = &[
    ("io.file.notFound", "The requested file was not found. Check the path or search the repository before retrying."),
    ("io.file.permissionDenied", "Permission denied. Check file permissions or try a different location."),
    ("io.file.changed", "The file has changed since it was last read. Re-read the relevant section before modifying."),
    ("anchor.notFound", "The edit target could not be found. The file may have changed. Re-read the relevant section before trying again."),
    ("anchor.ambiguous", "Multiple potential edit targets found. Re-read the file and use a more specific anchor."),
    ("patch.applyFailed", "The edit could not be applied. Re-read the current file content and retry with updated anchors."),
    ("patch.conflict", "The edit conflicts with existing content. Re-read the file section and resolve the conflict."),
    ("shell.spawnFailed", "The command could not be started. Check the command syntax and try again."),
    ("shell.timeout", "The command timed out. Use --timeout for longer-running commands or simplify the operation."),
    ("shell.blocked", "The command was blocked for safety reasons. Check the hint for an allowed alternative."),
    ("daemon.unavailable", "A required service is unavailable. The operation may succeed on retry."),
    ("daemon.timeout", "A service request timed out. Retrying with a narrower scope may help."),
    ("validation.missingArgument", "A required argument is missing. Check the tool's expected parameters."),
    ("validation.invalidInput", "The input is invalid. Check the argument format and retry."),
    ("context.stale", "The context may be stale. Re-read the relevant files before proceeding."),
    ("tool.internalError", "The tool encountered an internal error. Do not retry the same call repeatedly."),
    ("tool.rateLimited", "The request was rate-limited. Wait a moment before retrying."),
    ("unknown", "An unexpected error occurred. Try an alternative approach or report the issue."),
];

/// Recovery steps per error code, matching TS formatToolErrorGuidance.
const RECOVERY_STEPS: &[(&str, &[&str])] = &[
    ("io.file.notFound", &[
        "repo <parent-dir> -- Check what files exist in the parent directory",
        "search <path> --regex <pattern> -- Search for a file matching the pattern",
    ]),
    ("anchor.notFound", &[
        "read <path> -- Re-read the file to get current anchors",
    ]),
    ("anchor.ambiguous", &[
        "read <path> --range \"<range>\" -- Read a more specific section",
    ]),
    ("patch.applyFailed", &[
        "read <path> -- Re-read current content and retry with updated anchors",
    ]),
    ("patch.conflict", &[
        "read <path> --range \"<range>\" -- Read the conflicting section",
    ]),
    ("shell.timeout", &[
        "bash <command> --timeout <ms> -- Use a longer timeout for slow commands",
    ]),
    ("io.file.changed", &[
        "read <path> --range \"<range>\" -- Re-read the changed section",
    ]),
    ("context.stale", &[
        "read <path> -- Re-read files that may have changed",
    ]),
];

/// Map Rust ErrorSeverity to TS severity strings for the XML tag.
fn severity_to_ts(severity: &super::response::ErrorSeverity) -> &'static str {
    use super::response::ErrorSeverity;
    match severity {
        ErrorSeverity::Warning | ErrorSeverity::Error => "recoverable",
        ErrorSeverity::Critical => "unrecoverable",
    }
}

/// Format a tool error for the LLM using <tool_error> XML tags with recovery steps.
/// Matches TS formatToolErrorForLLM output format.
pub fn format_error_for_llm(error: &ToolError) -> String {
    let code_str = error.code.as_str();
    let severity = severity_to_ts(&error.severity);

    let template = LLM_TEMPLATES
        .iter()
        .find(|(code, _)| *code == code_str)
        .map(|(_, msg)| *msg)
        .unwrap_or("An error occurred. Try an alternative approach.");

    let mut body = template.to_string();

    // Selectively include safe details as "Additional context:" block
    let mut detail_parts: Vec<String> = Vec::new();
    if let Some(details) = &error.details {
        if let Some(path) = details.get("path").and_then(|v| v.as_str()) {
            detail_parts.push(format!("path: {}", serde_json::json!(path)));
        }
        if let Some(cmd) = details.get("command").and_then(|v| v.as_str()) {
            detail_parts.push(format!("command: {}", serde_json::json!(cmd)));
        }
        if let Some(exit_code) = details.get("exit_code").and_then(|v| v.as_i64()) {
            detail_parts.push(format!("exit_code: {}", exit_code));
        }
    }
    if !detail_parts.is_empty() {
        body.push_str(&format!("\nAdditional context: {}", detail_parts.join(", ")));
    }

    // Append recovery steps if available
    let steps = RECOVERY_STEPS.iter()
        .find(|(code, _)| *code == code_str)
        .map(|(_, steps)| *steps);
    if let Some(steps) = steps {
        if !steps.is_empty() {
            body.push_str("\n\nSuggested next steps:");
            for (i, step) in steps.iter().enumerate() {
                body.push_str(&format!("\n{}. {}", i + 1, step));
            }
        }
    }

    format!("<tool_error severity=\"{}\">\n{}\n</tool_error>", severity, body)
}

/// Format a tool error for logging (full detail, not LLM-safe).
pub fn format_error_for_log(error: &ToolError) -> String {
    format!(
        "[{}] {} | severity={} recoverability={} tool={} retries={}{}",
        error.code.as_str(),
        error.message,
        error.severity,
        error.recoverability_str(),
        error.metadata.tool_name,
        error.metadata.retry_count,
        error.details
            .as_ref()
            .map(|d| format!(" details={}", d))
            .unwrap_or_default(),
    )
}
