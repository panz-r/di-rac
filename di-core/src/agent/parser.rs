use crate::tools::ToolCall;
use crate::daemons::StreamChunk;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

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

    /// Parse a complete (non-streaming) response for tool calls.
    pub fn parse(&self, text: &str) -> (String, Vec<ToolCall>) {
        let mut thought = text.to_string();
        let mut tools = Vec::new();

        for cap in self.tool_regex.captures_iter(text) {
            let name = cap[1].to_string();
            let args_json = &cap[2];

            if let Ok(args) = serde_json::from_str(args_json) {
                tools.push(ToolCall { name, args });
                thought = thought.replace(&cap[0], "");
            }
        }

        (thought.trim().to_string(), tools)
    }
}

/// Accumulates partial tool calls from streaming chunks.
/// Mirrors the TS `PendingToolUse` at StreamResponseHandler.ts:13.
struct PendingTool {
    name: String,
    arguments_str: String,
}

pub struct StreamingToolAccumulator {
    pending: HashMap<String, PendingTool>,
}

impl StreamingToolAccumulator {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }

    /// Feed a stream chunk to the accumulator. Returns true if the chunk
    /// was a tool-call delta that was absorbed (not text).
    pub fn feed_chunk(&mut self, chunk: &StreamChunk) -> bool {
        // Handle content block that starts a tool_use
        if chunk.chunk_type == "content" {
            if let Some(ref blocks) = chunk.content_blocks {
                for block in blocks {
                    if block.block_type == "tool_use" {
                        if let (Some(id), Some(name)) = (&block.id, &block.name) {
                            self.pending.entry(id.clone()).or_insert_with(|| PendingTool {
                                name: name.clone(),
                                arguments_str: String::new(),
                            });
                        }
                    }
                }
            }
            // Also handle the flat fields (some providers send tool info this way)
            if let (Some(id), Some(name)) = (&chunk.tool_call_id, &chunk.tool_call_name) {
                self.pending.entry(id.clone()).or_insert_with(|| PendingTool {
                    name: name.clone(),
                    arguments_str: String::new(),
                });
            }
            return true;
        }

        // Handle delta — accumulate arguments
        if chunk.chunk_type == "delta" {
            if let (Some(id), Some(json)) = (&chunk.tool_call_id, &chunk.json_delta) {
                if let Some(pending) = self.pending.get_mut(id) {
                    pending.arguments_str.push_str(json);
                    return true;
                }
            }
            // Also handle tool_call_name on a delta (some providers)
            if let (Some(id), Some(name)) = (&chunk.tool_call_id, &chunk.tool_call_name) {
                self.pending.entry(id.clone()).or_insert_with(|| PendingTool {
                    name: name.clone(),
                    arguments_str: String::new(),
                });
                return true;
            }
            return false; // text delta, not tool
        }

        false
    }

    /// Finalize all accumulated tool calls into complete ToolCall objects.
    /// Also falls back to XML parsing for non-native tool calls.
    pub fn finalize(self, fallback_text: &str) -> Vec<ToolCall> {
        let mut tools = Vec::new();

        // Native tool calls from streaming
        for (_, pending) in self.pending {
            let args: Value = serde_json::from_str(&pending.arguments_str)
                .unwrap_or_else(|_| serde_json::json!({}));
            tools.push(ToolCall {
                name: pending.name,
                args,
            });
        }

        // Fallback: XML tool_code parsing from accumulated text
        let xml_regex = Regex::new(r#"(?s)<tool_code\s+name="([^"]+)">\s*(.*?)\s*</tool_code>"#).unwrap();
        for cap in xml_regex.captures_iter(fallback_text) {
            let name = cap[1].to_string();
            let args_json = &cap[2];
            if let Ok(args) = serde_json::from_str(args_json) {
                tools.push(ToolCall { name, args });
            }
        }

        tools
    }
}
