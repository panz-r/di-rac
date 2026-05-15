use crate::tools::ToolCall;
use crate::daemons::{StreamChunk, ContentBlock};
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
                if std::env::var("DI_DEBUG").is_ok() {
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
            if std::env::var("DI_DEBUG").is_ok() {
                eprintln!("[parser] finalize: name={} args_len={} args={:?}",
                    pending.name, pending.arguments_str.len(),
                    if pending.arguments_str.len() > 200 { &pending.arguments_str[..200] } else { &pending.arguments_str });
            }
        }

        for (_, pending) in self.pending {
            if pending.name.is_empty() {
                continue;
            }
            let args: Value = serde_json::from_str(&pending.arguments_str)
                .unwrap_or_else(|_| {
                    if std::env::var("DI_DEBUG").is_ok() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk(chunk_type: &str, text_delta: Option<&str>, thinking: Option<&str>,
                  tool_call_id: Option<&str>, tool_call_name: Option<&str>,
                  json_delta: Option<&str>, index: Option<i64>,
                  content: Option<&str>, content_blocks: Option<Vec<ContentBlock>>) -> StreamChunk
    {
        StreamChunk {
            chunk_type: chunk_type.to_string(),
            text_delta: text_delta.map(String::from),
            thinking: thinking.map(String::from),
            tool_call_id: tool_call_id.map(String::from),
            tool_call_name: tool_call_name.map(String::from),
            json_delta: json_delta.map(String::from),
            index,
            content: content.map(|s| serde_json::Value::String(s.to_string())),
            content_blocks,
            usage: None,
            finish_reason: None,
        }
    }

    // --- feed_chunk: text delta ---
    #[test]
    fn feed_chunk_text_delta_returns_false() {
        let mut acc = StreamingToolAccumulator::new();
        let chunk = make_chunk("delta", Some("hello"), None, None, None, None, None, None, None);
        assert!(!acc.feed_chunk(&chunk));
        assert!(acc.pending.is_empty());
    }

    // --- feed_chunk: tool call with id + name ---
    #[test]
    fn feed_chunk_tool_call_with_name_creates_pending() {
        let mut acc = StreamingToolAccumulator::new();
        let chunk = make_chunk("delta", None, None, Some("call_1"), Some("bash"),
                               None, Some(0), None, None);
        assert!(acc.feed_chunk(&chunk));
        let entry = acc.pending.get("call_1").unwrap();
        assert_eq!(entry.name, "bash");
        assert_eq!(entry.arguments_str, "");
    }

    // --- feed_chunk: tool call id without name (MiniMax-style) ---
    #[test]
    fn feed_chunk_tool_call_id_without_name_creates_placeholder() {
        let mut acc = StreamingToolAccumulator::new();
        let chunk = make_chunk("delta", None, None, Some("call_1"), None,
                               None, None, None, None);
        assert!(acc.feed_chunk(&chunk));
        let entry = acc.pending.get("call_1").unwrap();
        assert!(entry.name.is_empty());
        assert_eq!(entry.arguments_str, "");
    }

    // --- feed_chunk: json_delta accumulated into pending ---
    #[test]
    fn feed_chunk_json_delta_appends_to_pending() {
        let mut acc = StreamingToolAccumulator::new();
        // First chunk: create entry with id + name
        let chunk1 = make_chunk("delta", None, None, Some("call_1"), Some("bash"),
                                None, None, None, None);
        acc.feed_chunk(&chunk1);
        // Second chunk: accumulate json_delta
        let chunk2 = make_chunk("delta", None, None, Some("call_1"), Some("bash"),
                                Some(r#"{"cmd":"ls"}"#), None, None, None);
        assert!(acc.feed_chunk(&chunk2));
        let entry = acc.pending.get("call_1").unwrap();
        assert_eq!(entry.arguments_str, r#"{"cmd":"ls"}"#);
    }

    // --- feed_chunk: json_delta without prior id uses index mapping ---
    #[test]
    fn feed_chunk_json_delta_without_id_uses_index() {
        let mut acc = StreamingToolAccumulator::new();
        // First chunk: "content" type sets up index mapping
        let content_block = ContentBlock {
            block_type: "tool_use".to_string(),
            id: Some("call_1".to_string()),
            name: Some("bash".to_string()),
            text: None,
            input: None,
            extra: serde_json::Map::new(),
        };
        let chunk1 = StreamChunk {
            chunk_type: "content".to_string(),
            index: Some(0),
            content: Some(serde_json::Value::String("tool_use".to_string())),
            content_blocks: Some(vec![content_block]),
            text_delta: None,
            thinking: None,
            tool_call_id: None,
            tool_call_name: None,
            json_delta: None,
            usage: None,
            finish_reason: None,
        };
        assert!(acc.feed_chunk(&chunk1));
        assert!(acc.pending.contains_key("call_1"));

        // Second chunk: json_delta with index but no tool_call_id
        let chunk2 = make_chunk("delta", None, None, None, None,
                                Some(r#"{"cmd":"ls"}"#), Some(0), None, None);
        assert!(acc.feed_chunk(&chunk2));
        let entry = acc.pending.get("call_1").unwrap();
        assert_eq!(entry.arguments_str, r#"{"cmd":"ls"}"#);
    }

    // --- feed_chunk: content block registers tool_use ---
    #[test]
    fn feed_chunk_content_block_registers_tool() {
        let mut acc = StreamingToolAccumulator::new();
        let content_block = ContentBlock {
            block_type: "tool_use".to_string(),
            id: Some("tool_abc".to_string()),
            name: Some("read".to_string()),
            text: None,
            input: None,
            extra: serde_json::Map::new(),
        };
        let chunk = StreamChunk {
            chunk_type: "content".to_string(),
            index: Some(0),
            content: Some(serde_json::Value::String("tool_use".to_string())),
            content_blocks: Some(vec![content_block]),
            text_delta: None,
            thinking: None,
            tool_call_id: None,
            tool_call_name: None,
            json_delta: None,
            usage: None,
            finish_reason: None,
        };
        assert!(acc.feed_chunk(&chunk));
        let entry = acc.pending.get("tool_abc").unwrap();
        assert_eq!(entry.name, "read");
    }

    // --- feed_chunk: unknown chunk type returns false ---
    #[test]
    fn feed_chunk_unknown_type_returns_false() {
        let mut acc = StreamingToolAccumulator::new();
        let chunk = make_chunk("stop", None, None, None, None, None, None, None, None);
        assert!(!acc.feed_chunk(&chunk));
    }

    // --- finalize: empty accumulator ---
    #[test]
    fn finalize_empty_returns_empty() {
        let acc = StreamingToolAccumulator::new();
        let tools = acc.finalize("");
        assert!(tools.is_empty());
    }

    // --- finalize: single tool call ---
    #[test]
    fn finalize_single_tool() {
        let mut acc = StreamingToolAccumulator::new();
        let chunk = make_chunk("delta", None, None, Some("call_1"), Some("bash"),
                                Some(r#"{"cmd":"ls"}"#), None, None, None);
        acc.feed_chunk(&chunk);
        let tools = acc.finalize("");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "bash");
        assert_eq!(tools[0].args, serde_json::json!({"cmd":"ls"}));
    }

    // --- finalize: skips empty-named tool calls ---
    #[test]
    fn finalize_skips_empty_name() {
        let mut acc = StreamingToolAccumulator::new();
        // Create a pending entry with empty name
        let chunk = make_chunk("delta", None, None, Some("call_1"), None,
                                None, None, None, None);
        acc.feed_chunk(&chunk);
        let tools = acc.finalize("");
        assert!(tools.is_empty());
    }

    // --- finalize: malformed JSON args produce empty object ---
    #[test]
    fn finalize_malformed_args_returns_empty_object() {
        let mut acc = StreamingToolAccumulator::new();
        let chunk = make_chunk("delta", None, None, Some("call_1"), Some("bash"),
                                Some("not valid json"), None, None, None);
        acc.feed_chunk(&chunk);
        let tools = acc.finalize("");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].args, serde_json::json!({}));
    }

    // --- finalize: prefers native over XML fallback ---
    #[test]
    fn finalize_prefers_native_over_xml() {
        let mut acc = StreamingToolAccumulator::new();
        let chunk = make_chunk("delta", None, None, Some("call_1"), Some("bash"),
                                Some(r#"{"cmd":"ls"}"#), None, None, None);
        acc.feed_chunk(&chunk);
        // fallback_text has XML too, but native should win
        let tools = acc.finalize(r#"<tool_code name="bash">{"cmd":"whoami"}</tool_code>"#);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "bash");
        assert_eq!(tools[0].args, serde_json::json!({"cmd":"ls"}));
    }

    // --- finalize: XML fallback when native is empty ---
    #[test]
    fn finalize_xml_fallback() {
        let acc = StreamingToolAccumulator::new();
        let tools = acc.finalize(r#"Some text <tool_code name="read">{"path":"/etc/hosts"}</tool_code> more text"#);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read");
        assert_eq!(tools[0].args, serde_json::json!({"path":"/etc/hosts"}));
    }

    // --- finalize: multiple tool calls ---
    #[test]
    fn finalize_multiple_tools() {
        let mut acc = StreamingToolAccumulator::new();
        let chunk1 = make_chunk("delta", None, None, Some("call_1"), Some("bash"),
                                Some(r#"{"cmd":"ls"}"#), None, None, None);
        acc.feed_chunk(&chunk1);
        let chunk2 = make_chunk("delta", None, None, Some("call_2"), Some("read"),
                                Some(r#"{"path":"/etc"}"#), None, None, None);
        acc.feed_chunk(&chunk2);
        let tools = acc.finalize("");
        assert_eq!(tools.len(), 2);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"read"));
    }
}
