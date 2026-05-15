use crate::tools::ToolCall;
use crate::daemons::StreamChunk;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

static XML_TOOL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<tool_code\s+name="([^"]+)">\s*(.*?)\s*</tool_code>"#).expect("invalid xml tool regex")
});

/// Accumulates partial tool calls from streaming chunks.
/// Mirrors the TS `PendingToolUse` at StreamResponseHandler.ts:13.
struct PendingTool {
    name: String,
    arguments_str: String,
}

pub struct StreamingToolAccumulator {
    pending: HashMap<String, PendingTool>,
    /// Maps content block index → tool_call_id.
    /// The api-gateway sends tool_call_id only on the "content" chunk that starts
    /// a tool_use block, NOT on subsequent "delta" chunks. Deltas are correlated
    /// by Index. This mirrors the TS code's index-based accumulator lookup.
    index_to_id: HashMap<i64, String>,
}

impl StreamingToolAccumulator {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            index_to_id: HashMap::new(),
        }
    }

    /// Feed a stream chunk to the accumulator. Returns true if the chunk
    /// was a tool-call delta that was absorbed (not text).
    pub fn feed_chunk(&mut self, chunk: &StreamChunk) -> bool {
        // Handle content block that starts a tool_use
        if chunk.chunk_type == "content" {
            let is_tool_use = chunk.content.as_ref()
                .and_then(|v| v.as_str())
                .map(|s| s == "tool_use")
                .unwrap_or(false);
            if let Some(ref blocks) = chunk.content_blocks {
                for block in blocks {
                    if block.block_type == "tool_use" {
                        if let (Some(id), Some(name)) = (&block.id, &block.name) {
                            self.pending.entry(id.clone()).or_insert_with(|| PendingTool {
                                name: name.clone(),
                                arguments_str: String::new(),
                            });
                            if let Some(idx) = chunk.index {
                                self.index_to_id.insert(idx, id.clone());
                            }
                        }
                    }
                }
            }
            // Also handle the flat fields (api-gateway sends tool info this way)
            if let (Some(id), Some(name)) = (&chunk.tool_call_id, &chunk.tool_call_name) {
                self.pending.entry(id.clone()).or_insert_with(|| PendingTool {
                    name: name.clone(),
                    arguments_str: String::new(),
                });
                if let Some(idx) = chunk.index {
                    self.index_to_id.insert(idx, id.clone());
                }
                if std::env::var("DIRAC_DEBUG").is_ok() {
                    eprintln!("[parser] content chunk: idx={:?} tool_call_id={} name={} is_tool_use={}",
                        chunk.index, id, name, is_tool_use);
                }
            }
            return true;
        }

        // Handle delta — accumulate arguments.
        // Order matters: OpenAI-compatible providers send tool_call_id + tool_call_name +
        // json_delta on the SAME chunk, so we must create the pending entry BEFORE
        // trying to accumulate json_delta.
        if chunk.chunk_type == "delta" {
            // 1. Ensure pending entry exists if tool info is present.
            // Some providers (e.g. MiniMax) send tool_call_id without tool_call_name
            // in the first chunk, with the name arriving in a later separate chunk.
            // Create a placeholder entry even without a name so the tool call isn't lost.
            if let Some(id) = &chunk.tool_call_id {
                self.pending.entry(id.clone()).or_insert_with(|| PendingTool {
                    name: chunk.tool_call_name.clone().unwrap_or_default(),
                    arguments_str: String::new(),
                });
                if let Some(idx) = chunk.index {
                    self.index_to_id.insert(idx, id.clone());
                }
            }

            // 2. Accumulate json_delta if present
            if let Some(json) = &chunk.json_delta {
                let id = chunk.tool_call_id.as_ref().cloned()
                    .or_else(|| chunk.index.and_then(|idx| self.index_to_id.get(&idx).cloned()));
                if let Some(id) = id {
                    if let Some(pending) = self.pending.get_mut(&id) {
                        pending.arguments_str.push_str(json);
                        return true;
                    }
                }
            }

            // 3. If tool info was present in any form, mark as absorbed
            if chunk.tool_call_id.is_some() || chunk.tool_call_name.is_some() || chunk.json_delta.is_some() {
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
        for (_, pending) in &self.pending {
            if std::env::var("DIRAC_DEBUG").is_ok() {
                eprintln!("[parser] finalize: name={} args_len={} args={:?}",
                    pending.name, pending.arguments_str.len(),
                    if pending.arguments_str.len() > 200 { &pending.arguments_str[..200] } else { &pending.arguments_str });
            }
        }

        for (_, pending) in self.pending {
            let args: Value = serde_json::from_str(&pending.arguments_str)
                .unwrap_or_else(|_| {
                    if std::env::var("DIRAC_DEBUG").is_ok() {
                        eprintln!("[parser] finalize: FAILED to parse args for {}: {:?}", pending.name, pending.arguments_str);
                    }
                    serde_json::json!({})
                });
            tools.push(ToolCall {
                name: pending.name,
                args,
            });
        }

        // Fallback: XML tool_code parsing — only if no native tool calls were found,
        // to prevent duplicate execution when the model emits both formats.
        if tools.is_empty() {
            for cap in XML_TOOL_REGEX.captures_iter(fallback_text) {
                let name = cap[1].to_string();
                let args_json = &cap[2];
                if let Ok(args) = serde_json::from_str(args_json) {
                    tools.push(ToolCall { name, args });
                }
            }
        }

        tools
    }
}
