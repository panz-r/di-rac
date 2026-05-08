use crate::tools::ToolCall;
use regex::Regex;
use anyhow::Result;

pub struct ResponseParser {
    tool_regex: Regex,
}

impl ResponseParser {
    pub fn new() -> Self {
        Self {
            // Match <tool_code name="...">JSON_ARGS</tool_code>
            tool_regex: Regex::new(r#"(?s)<tool_code\s+name="([^"]+)">\s*(.*?)\s*</tool_code>"#).unwrap(),
        }
    }

    pub fn parse(&self, text: &str) -> (String, Vec<ToolCall>) {
        let mut thought = text.to_string();
        let mut tools = Vec::new();

        for cap in self.tool_regex.captures_iter(text) {
            let name = cap[1].to_string();
            let args_json = &cap[2];
            
            if let Ok(args) = serde_json::from_str(args_json) {
                tools.push(ToolCall { name, args });
                // Strip the tool call from the thought to keep it clean
                thought = thought.replace(&cap[0], "");
            }
        }

        (thought.trim().to_string(), tools)
    }
}
