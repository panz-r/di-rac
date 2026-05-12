use crate::daemons::GatewayMessage;
use serde_json::json;

const DISTILLER_SYSTEM: &str = "\
You are a context distillation assistant. Your job is to compress and structure \
information from coding agent tool outputs. You must respond with valid JSON \
matching the exact schema provided. Be concise and information-dense. Never \
invent information not present in the input. If uncertain, omit rather than guess.\n\n\
IMPORTANT: Your response must be ONLY valid JSON. No markdown, no explanation.";

pub fn system_prompt() -> String {
    DISTILLER_SYSTEM.to_string()
}

#[allow(dead_code)]
pub fn tool_result_messages(input: &super::ToolDistillInput) -> Vec<GatewayMessage> {
    let tool_output_str = serde_json::to_string_pretty(&input.tool_result)
        .unwrap_or_else(|_| input.tool_result.to_string());

    let truncated = if tool_output_str.len() > 6000 {
        format!("{}...[truncated at 6000 chars]", &tool_output_str[..6000])
    } else {
        tool_output_str
    };

    vec![
        GatewayMessage::simple("user", json!(format!(
                "Distill the following tool result into a structured summary.\n\n\
                Tool: {}\n\
                Arguments: {}\n\
                Estimated tokens: {}\n\
                Tool output:\n```\n{}\n```\n\n\
                Respond with JSON matching this schema:\n\
                {{\n  \
                  \"summary\": \"<concise information-dense summary>\",\n  \
                  \"key_facts\": [\"<fact1>\", \"<fact2>\"],\n  \
                  \"errors\": [\"<error1>\"] or [],\n  \
                  \"files_referenced\": [\"<path1>\"] or [],\n  \
                  \"estimated_tokens\": <int>,\n  \
                  \"exchange_core\": \"<one-sentence core finding>\",\n  \
                  \"specific_context\": [\"<domain detail>\"] or [],\n  \
                  \"thematic_tags\": [\"<topic>\"] or [],\n  \
                  \"symbols_referenced\": [\"<symbol>\"] or [],\n  \
                  \"exact_evidence\": [\"<verbatim quote>\"] or [],\n  \
                  \"hypotheses\": [\"<speculation>\"] or [],\n  \
                  \"source_event_ids\": []\n\
                }}\n\n\
                Rules:\n\
                - summary must capture all actionable information\n\
                - key_facts: only facts relevant to the ongoing task\n\
                - errors: any error messages or status codes\n\
                - files_referenced: file paths mentioned in the output\n\
                - estimated_tokens: estimate token count of your summary\n\
                - exchange_core: the single most important finding in one sentence\n\
                - specific_context: domain details (error codes, configs, types)\n\
                - thematic_tags: topic labels (e.g. 'authentication', 'file-io', 'build')\n\
                - symbols_referenced: function, class, or type names found\n\
                - exact_evidence: up to 3 verbatim quotes from the original output\n\
                - hypotheses: labeled speculations, never present as facts\n\
                - source_event_ids: leave as empty array",
                input.tool_name,
                serde_json::to_string(&input.tool_args).unwrap_or_default(),
                input.estimated_tokens,
                truncated,
            )),
        ),
    ]
}

pub fn task_state_messages(input: &super::TaskStateInput) -> Vec<GatewayMessage> {
    let assistant_block = input.recent_assistant_summaries
        .iter()
        .enumerate()
        .map(|(i, s)| format!("Turn {}: {}", i + 1, s))
        .collect::<Vec<_>>()
        .join("\n\n");

    let obs_block = input.key_observations.join("\n- ");

    vec![
        GatewayMessage::simple("user", json!(format!(
                "Consolidate the following task state into a structured patch.\n\n\
                Recent assistant turns:\n{}\n\n\
                File context:\n{}\n\n\
                Key observations:\n- {}\n\n\
                Respond with JSON matching this schema:\n\
                {{\n  \
                  \"enriched_summary\": \"<comprehensive task progress summary>\",\n  \
                  \"open_subgoals\": [\"<goal1>\", \"<goal2>\"],\n  \
                  \"decisions\": [\"<decision1>\"],\n  \
                  \"critical_files\": [\"<path1>\"]\n\
                }}\n\n\
                Rules:\n\
                - enriched_summary: must capture ALL progress, decisions, and findings\n\
                - open_subgoals: only unresolved items\n\
                - decisions: architectural or implementation choices made\n\
                - critical_files: files important for completing the remaining task",
                assistant_block,
                input.file_context_summary,
                obs_block,
            )),
        ),
    ]
}
