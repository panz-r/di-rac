use super::*;
use super::schemas::*;

/// Zero-cost deterministic distiller. Produces the same output the system
/// would have produced without the distiller. Used when no model config
/// is provided, or when the model-backed distiller fails.
pub struct NoopContextDistiller;

#[async_trait::async_trait]
impl ContextDistiller for NoopContextDistiller {
    async fn distill_tool_result(
        &self,
        input: ToolDistillInput,
    ) -> DistillationResult<DistilledToolResult> {
        let output_str = input.tool_result.to_string();
        let lines: Vec<&str> = output_str.lines().collect();
        let _status = lines.iter()
            .find(|l| !l.trim().is_empty())
            .map(|l| truncate_to(l, 200))
            .unwrap_or_default();

        let key_facts: Vec<String> = lines.iter()
            .rev()
            .filter(|l| !l.trim().is_empty())
            .take(5)
            .map(|l| truncate_to(l, 200))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let errors: Vec<String> = lines.iter()
            .filter(|l| {
                let lower = l.to_lowercase();
                lower.contains("error") || lower.contains("failed") || lower.contains("fatal")
            })
            .take(3)
            .map(|l| truncate_to(l, 200))
            .collect();

        DistillationResult {
            output: DistilledToolResult {
                summary: truncate_to(&output_str, 2000),
                key_facts,
                errors,
                files_referenced: extract_file_paths(&output_str),
                estimated_tokens: input.estimated_tokens,
                artifact_ref: None,
                exchange_core: String::new(),
                specific_context: Vec::new(),
                thematic_tags: Vec::new(),
                symbols_referenced: extract_symbols(&output_str),
                exact_evidence: Vec::new(),
                hypotheses: Vec::new(),
                source_event_ids: input.source_event_ids.iter().map(|u| u.to_string()).collect(),
            },
            provenance: Provenance {
                source_event_ids: input.source_event_ids,
                confidence: 0.1, // Below model threshold but nonzero: deterministic fallback ran
                source: DistillerSource::DeterministicFallback,
                config_version: 0,
            },
        }
    }

    async fn consolidate_task_state(
        &self,
        input: TaskStateInput,
    ) -> DistillationResult<TaskStatePatch> {
        let mut parts = Vec::new();
        if !input.recent_assistant_summaries.is_empty() {
            parts.push(input.recent_assistant_summaries.join("\n\n"));
        }
        if !input.file_context_summary.is_empty() {
            parts.push(format!("File context: {}", input.file_context_summary));
        }

        DistillationResult {
            output: TaskStatePatch {
                enriched_summary: parts.join("\n\n"),
                open_subgoals: Vec::new(),
                decisions: Vec::new(),
                critical_files: Vec::new(),
            },
            provenance: Provenance {
                source_event_ids: input.source_event_ids,
                confidence: 0.1, // Below model threshold but nonzero: deterministic fallback ran
                source: DistillerSource::DeterministicFallback,
                config_version: 0,
            },
        }
    }
}

#[allow(dead_code)]
fn truncate_to(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}...", &s[..max - 3]) }
}

#[allow(dead_code)]
fn extract_file_paths(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"(?:^|\s|['"(){}])([\w./-]+\.\w{1,10})(?:\s|$|['"(){}:,])"#).unwrap();
    let mut paths: Vec<String> = re.captures_iter(text)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .filter(|p| p.contains('/') || p.contains('.'))
        .take(10)
        .collect();
    paths.dedup();
    paths
}

#[allow(dead_code)]
fn extract_symbols(text: &str) -> Vec<String> {
    let sig_prefixes = ["pub async fn ", "pub fn ", "fn ", "struct ", "impl ", "class ", "def ", "const ", "type ", "enum ", "trait "];
    let mut symbols = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        for prefix in &sig_prefixes {
            if trimmed.starts_with(prefix) {
                let rest = &trimmed[prefix.len()..];
                let name = rest.split(|c: char| c.is_whitespace() || c == '(' || c == '<' || c == '{')
                    .next()
                    .unwrap_or("")
                    .trim();
                if !name.is_empty() && symbols.len() < 10 {
                    symbols.push(name.to_string());
                }
                break;
            }
        }
    }
    symbols
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::distiller::*;

    #[test]
    fn noop_summary_is_bounded() {
        let long_output = "x".repeat(10000);
        let input = ToolDistillInput {
            tool_name: "bash".to_string(),
            tool_args: serde_json::json!({}),
            tool_result: serde_json::json!(long_output),
            estimated_tokens: 2500,
            source_event_ids: vec![],
        };

        let noop = NoopContextDistiller;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(noop.distill_tool_result(input));

        assert!(result.output.summary.len() <= 2003, "summary should be bounded to ~2000 chars, got {}", result.output.summary.len());
        assert!(result.output.artifact_ref.is_none());
    }
}
