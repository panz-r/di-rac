use crate::tools::ToolCall;
use anyhow::{Result, anyhow};

/// Parse the attempt_completion arguments. The actual completion signaling
/// happens in AgentEngine.
pub fn parse_completion(call: &ToolCall) -> Result<(String, Option<String>)> {
    let result = call.args.get("result")
        .or_else(|| call.args.get("response"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing result argument for attempt_completion"))?;

    let command = call.args.get("command")
        .and_then(|v| v.as_str())
        .map(String::from);

    Ok((result.to_string(), command))
}
