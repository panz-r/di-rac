use super::schemas::*;
use regex::Regex;
use std::sync::LazyLock;

/// Validation errors — these cause fallback, never propagation.
#[derive(Debug)]
pub enum ValidationError {
    SchemaMismatch,
    EmptyOutput,
    AbsolutePath,
    SecretDetected,
    StackTraceDetected,
}

const MAX_SUMMARY_LEN: usize = 8000;

#[allow(dead_code)]
pub fn validate_tool_result(result: &DistilledToolResult) -> Result<(), ValidationError> {
    if result.summary.is_empty() {
        return Err(ValidationError::EmptyOutput);
    }
    if result.summary.len() > MAX_SUMMARY_LEN {
        return Err(ValidationError::SchemaMismatch);
    }
    if result.estimated_tokens == 0 {
        return Err(ValidationError::SchemaMismatch);
    }
    scan_for_secrets(&result.summary)?;
    check_no_absolute_paths(&result.files_referenced)?;
    if result.exchange_core.len() > 500 {
        return Err(ValidationError::SchemaMismatch);
    }
    if result.exact_evidence.len() > 3 {
        return Err(ValidationError::SchemaMismatch);
    }
    if result.thematic_tags.len() > 10 {
        return Err(ValidationError::SchemaMismatch);
    }
    if result.symbols_referenced.len() > 20 {
        return Err(ValidationError::SchemaMismatch);
    }
    check_no_absolute_paths(&result.symbols_referenced)?;
    Ok(())
}

pub fn validate_task_state_patch(patch: &TaskStatePatch) -> Result<(), ValidationError> {
    if patch.enriched_summary.is_empty() {
        return Err(ValidationError::EmptyOutput);
    }
    if patch.enriched_summary.len() > MAX_SUMMARY_LEN * 2 {
        return Err(ValidationError::SchemaMismatch);
    }
    scan_for_secrets(&patch.enriched_summary)?;
    scan_for_stack_traces(&patch.enriched_summary)?;
    check_no_absolute_paths(&patch.critical_files)?;
    Ok(())
}

/// Check that no paths are absolute.
fn check_no_absolute_paths(paths: &[String]) -> Result<(), ValidationError> {
    for p in paths {
        if p.starts_with('/') || (p.len() >= 3 && p.as_bytes()[1] == b':' && (p.as_bytes()[2] == b'\\' || p.as_bytes()[2] == b'/')) {
            return Err(ValidationError::AbsolutePath);
        }
    }
    Ok(())
}

/// Precompiled secret patterns — avoids recompilation on every call.
static SECRET_RE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    let patterns = vec![
        r"sk-[a-zA-Z0-9]{20,}",           // OpenAI-style keys
        r"AKIA[0-9A-Z]{16}",              // AWS access keys
        r"ghp_[a-zA-Z0-9]{36}",           // GitHub PATs
        r"xox[bpaors]-[a-zA-Z0-9\-]+",    // Slack tokens
        r#"api[_\-]?key\s*[:=]\s*["']?[a-zA-Z0-9]{20,}"#, // generic api_key=
    ];
    patterns.into_iter().map(|p| Regex::new(p).expect("invalid secret regex")).collect()
});

/// Precompiled stack trace patterns.
static STACKTRACE_RE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    let patterns = vec![
        r"at\s+\S+\s+\(.*:\d+:\d+\)",        // JS/TS: at func (file:line:col)
        r#"File\s+"[^"]+",\s*line\s+\d+"#,    // Python: File "path", line N
        r"^\s+at\s+\S+",                       // Java/C#: at com.example.Method
    ];
    patterns.into_iter().map(|p| Regex::new(p).expect("invalid stacktrace regex")).collect()
});

fn scan_for_secrets(text: &str) -> Result<(), ValidationError> {
    for re in SECRET_RE.iter() {
        if re.is_match(text) {
            return Err(ValidationError::SecretDetected);
        }
    }
    Ok(())
}

/// Detect stack trace patterns in text.
fn scan_for_stack_traces(text: &str) -> Result<(), ValidationError> {
    for re in STACKTRACE_RE.iter() {
        if re.is_match(text) {
            return Err(ValidationError::StackTraceDetected);
        }
    }
    Ok(())
}

/// Faithfulness: verify that files_referenced in a distilled tool result
/// are grounded in the original tool output. Checks basenames for presence.
/// Phase 1: logs warnings rather than rejecting.
#[allow(dead_code)]
pub fn validate_tool_result_faithfulness(result: &DistilledToolResult, original_output: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    let lower_output = original_output.to_lowercase();
    for file in &result.files_referenced {
        let basename = file.rsplit('/').next().unwrap_or(file).to_lowercase();
        if !lower_output.contains(&basename) && !lower_output.contains(&file.to_lowercase()) {
            warnings.push(format!("file_referenced '{}' not found in original output", file));
        }
    }
    warnings
}

/// Faithfulness: verify that exact_evidence quotes appear in the original output.
/// Returns warnings for ungrounded quotes.
#[allow(dead_code)]
pub fn validate_exact_evidence_faithfulness(result: &DistilledToolResult, original_output: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    let lower_output = original_output.to_lowercase();
    for quote in &result.exact_evidence {
        let snippet = if quote.len() > 30 {
            let boundary = quote.floor_char_boundary(30);
            &quote[..boundary]
        } else {
            quote
        };
        if !lower_output.contains(&snippet.to_lowercase()) {
            let display = if quote.len() > 50 { format!("{}...", &quote[..50]) } else { quote.clone() };
            warnings.push(format!("exact_evidence '{}' not found in original output", display));
        }
    }
    warnings
}
