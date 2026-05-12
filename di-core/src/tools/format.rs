use super::response::ToolError;

// ---------------------------------------------------------------------------
// LLM error formatting — policy-driven, redacted by default
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

/// Format a tool error for the LLM. Uses the template registry for safety.
/// Details from the error are selectively included (never stack traces or hashes).
pub fn format_error_for_llm(error: &ToolError) -> String {
    let code_str = error.code.as_str();

    // Look up template
    let template = LLM_TEMPLATES
        .iter()
        .find(|(code, _)| *code == code_str)
        .map(|(_, msg)| *msg)
        .unwrap_or("An error occurred. Try an alternative approach.");

    let mut parts = vec![template.to_string()];

    // Selectively include safe details
    if let Some(details) = &error.details {
        if let Some(path) = details.get("path").and_then(|v| v.as_str()) {
            parts.push(format!("File: {}", path));
        }
        if let Some(cmd) = details.get("command").and_then(|v| v.as_str()) {
            parts.push(format!("Command: {}", cmd));
        }
        if let Some(exit_code) = details.get("exit_code").and_then(|v| v.as_i64()) {
            parts.push(format!("Exit code: {}", exit_code));
        }
    }

    // Include remediation hint if available
    if let Some(rem) = &error.remediation {
        if !rem.suggested_tools.is_empty() {
            parts.push(format!("Suggested: {}", rem.suggested_tools.join(", ")));
        }
    }

    parts.join(" ")
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
