use crate::tools::ToolCall;
use anyhow::{Result, anyhow};

/// Builds the follow-up question event data. The actual waiting for the
/// frontend response happens in AgentEngine (which owns the channel).
pub fn parse_followup_question(call: &ToolCall) -> Result<(String, Option<Vec<String>>)> {
    let question = call.args.get("question").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing question argument for ask_followup_question"))?;

    let options: Option<Vec<String>> = call.args.get("options")
        .and_then(|v| v.as_str())
        .and_then(|s| serde_json::from_str(s).ok())
        .or_else(|| {
            call.args.get("options")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        });

    Ok((question.to_string(), options))
}
