use chrono::{DateTime, Utc};
use std::collections::HashSet;
use uuid::Uuid;

/// Maximum byte size for any single block's content. Larger content is truncated.
const MAX_BLOCK_BYTES: usize = 1_048_576; // 1 MiB

fn truncate_content(mut s: String) -> String {
    const { assert!(MAX_BLOCK_BYTES > 0, "MAX_BLOCK_BYTES must be positive") };
    if s.len() <= MAX_BLOCK_BYTES {
        return s;
    }
    let end = s.floor_char_boundary(MAX_BLOCK_BYTES);
    s.truncate(end);
    s.push_str("\n… [truncated]");
    s
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Running,
    Waiting,
    Error,
    Finished,
}

// ---------------------------------------------------------------------------
// Conversation Log — append-only block model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub call_id: String,
    pub tool: String,
    pub args_summary: String,
}

#[derive(Debug, Clone)]
pub struct ToolResultInfo {
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum Block {
    User {
        content: String,
    },
    Assistant {
        content: String,
    },
    Tool {
        call: ToolCallInfo,
        result: Option<ToolResultInfo>,
    },
    System {
        content: String,
    },
    Finish {
        message: String,
        success: bool,
    },
}

#[derive(Debug, Clone)]
pub struct StreamingBlock {
    pub content: String,
    pub is_thinking: bool,
}

#[derive(Debug, Clone)]
pub struct ConversationLog {
    blocks: Vec<Block>,
    streaming: Option<StreamingBlock>,
    /// Monotonically increasing generation counter — bumped on every mutation.
    /// Used to invalidate visual line caches.
    generation: u64,
}

impl ConversationLog {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            streaming: None,
            generation: 0,
        }
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn streaming(&self) -> &Option<StreamingBlock> {
        &self.streaming
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn push_user(&mut self, content: String) {
        if content.is_empty() {
            return;
        }
        self.blocks.push(Block::User { content: truncate_content(content) });
        self.generation += 1;
    }

    pub fn push_assistant(&mut self, content: String) {
        if !content.is_empty() {
            self.blocks.push(Block::Assistant { content: truncate_content(content) });
            self.generation += 1;
        }
    }

    pub fn push_system(&mut self, content: String) {
        self.blocks.push(Block::System { content: truncate_content(content) });
        self.generation += 1;
    }

    pub fn push_tool_call(&mut self, call_id: String, tool: String, args_summary: String) {
        self.blocks.push(Block::Tool {
            call: ToolCallInfo { call_id, tool, args_summary },
            result: None,
        });
        self.generation += 1;
    }

    /// Set the result on the Tool block matching the given call_id.
    pub fn set_tool_result(&mut self, call_id: &str, content: String) {
        let content = truncate_content(content);
        for block in self.blocks.iter_mut().rev() {
            if let Block::Tool { call, result } = block {
                if result.is_none() && call.call_id == call_id {
                    *result = Some(ToolResultInfo { content });
                    self.generation += 1;
                    return;
                }
            }
        }
    }

    pub fn push_finish(&mut self, message: String, success: bool) {
        self.blocks.push(Block::Finish { message, success });
        self.generation += 1;
    }

    pub fn set_streaming(&mut self, content: String, is_thinking: bool) {
        self.streaming = Some(StreamingBlock { content, is_thinking });
        self.generation += 1;
    }

    pub fn append_streaming(&mut self, text: &str) {
        if let Some(ref mut s) = self.streaming {
            let remaining = MAX_BLOCK_BYTES.saturating_sub(s.content.len());
            if remaining > 0 {
                let to_push = if text.len() <= remaining {
                    text
                } else {
                    let mut end = remaining;
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    &text[..end]
                };
                s.content.push_str(to_push);
                self.generation += 1;
            }
        }
    }

    #[allow(dead_code)]
    pub fn streaming_text(&self) -> Option<&str> {
        self.streaming.as_ref().map(|s| s.content.as_str())
    }

    pub fn streaming_is_thinking(&self) -> bool {
        self.streaming.as_ref().map(|s| s.is_thinking).unwrap_or(false)
    }

    /// Finalize the streaming block into a permanent block.
    pub fn finalize_streaming(&mut self) {
        if let Some(s) = self.streaming.take() {
            if !s.content.is_empty() {
                let content = truncate_content(s.content);
                if s.is_thinking {
                    self.blocks.push(Block::System { content });
                } else {
                    self.blocks.push(Block::Assistant { content });
                }
            }
            self.generation += 1;
        }
    }

    /// Clear the streaming block without storing it.
    pub fn clear_streaming(&mut self) {
        if self.streaming.take().is_some() {
            self.generation += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// PendingInput, Metrics, AgentState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum PendingInput {
    Approval {
        tool: String,
        #[allow(dead_code)]
        args: serde_json::Value,
        description: String,
    },
    Followup {
        question: String,
        options: Option<Vec<String>>,
    },
}

#[derive(Debug, Clone)]
pub struct Metrics {
    #[allow(dead_code)]
    pub sqs: f32,
    pub token_usage: usize,
    #[allow(dead_code)]
    pub latency_ms: u64,
}

pub struct AgentState {
    pub id: Uuid,
    pub name: String,
    pub status: AgentStatus,
    pub log: ConversationLog,
    pub expanded: HashSet<usize>,
    /// Per-block wrap toggle — when set, expanded blocks show full text instead of truncating.
    pub wrapped: HashSet<usize>,
    pub pending_input: Option<PendingInput>,
    pub metrics: Option<Metrics>,
    pub last_activity: DateTime<Utc>,
}

impl AgentState {
    pub fn new(id: Uuid, name: String) -> Self {
        Self {
            id,
            name,
            status: AgentStatus::Running,
            log: ConversationLog::new(),
            expanded: HashSet::new(),
            wrapped: HashSet::new(),
            pending_input: None,
            metrics: None,
            last_activity: Utc::now(),
        }
    }

    pub fn is_waiting(&self) -> bool {
        self.pending_input.is_some()
    }

    pub fn display_status(&self) -> &str {
        match self.status {
            AgentStatus::Running => {
                if self.log.streaming().is_some() {
                    "Thinking"
                } else {
                    "Running"
                }
            }
            AgentStatus::Waiting => "Waiting",
            AgentStatus::Error => "Error",
            AgentStatus::Finished => "Finished",
        }
    }

    pub fn format_timestamp(&self) -> String {
        self.last_activity.format("%H:%M").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // truncate_content
    // -----------------------------------------------------------------------
    #[test]
    fn truncate_ascii_short() {
        let s = "hello".to_string();
        assert_eq!(truncate_content(s), "hello");
    }

    #[test]
    fn truncate_ascii_exact() {
        let s = "a".repeat(MAX_BLOCK_BYTES);
        assert_eq!(truncate_content(s.clone()).len(), MAX_BLOCK_BYTES);
    }

    #[test]
    fn truncate_ascii_overshoot() {
        let s = "a".repeat(MAX_BLOCK_BYTES + 50);
        let result = truncate_content(s);
        // Content truncated to MAX, then "\n… [truncated]" appended
        assert!(result.len() > MAX_BLOCK_BYTES);
        assert!(result.len() <= MAX_BLOCK_BYTES + 30);
        assert!(result.ends_with("[truncated]"));
    }

    #[test]
    fn truncate_unicode_overshoot() {
        // Each 'é' is 2 bytes in UTF-8
        let s = "é".repeat(MAX_BLOCK_BYTES / 2 + 100);
        let result = truncate_content(s);
        assert!(result.ends_with("[truncated]"));
        // Should not panic or cut in the middle of a char
    }

    #[test]
    fn truncate_multibyte_boundary() {
        // Build a string that overshoots by just a few bytes, with a multi-byte
        // char right at the boundary.
        let base = "a".repeat(MAX_BLOCK_BYTES - 2); // 2 bytes short
        let s = format!("{}{}", base, "é"); // now MAX_BLOCK_BYTES + 0 (é is 2 bytes)
        // This is still <= MAX_BLOCK_BYTES? base is MAX-2, "é" is 2 bytes, total = MAX
        assert_eq!(s.len(), MAX_BLOCK_BYTES);
        let result = truncate_content(s);
        assert_eq!(result.len(), MAX_BLOCK_BYTES);
    }

    // -----------------------------------------------------------------------
    // ConversationLog
    // -----------------------------------------------------------------------
    #[test]
    fn log_new_is_empty() {
        let log = ConversationLog::new();
        assert!(log.blocks().is_empty());
        assert!(log.streaming().is_none());
        assert_eq!(log.generation(), 0);
    }

    #[test]
    fn log_push_user() {
        let mut log = ConversationLog::new();
        log.push_user("hello".to_string());
        assert_eq!(log.blocks().len(), 1);
        assert_eq!(log.generation(), 1);
    }

    #[test]
    fn log_push_user_empty() {
        let mut log = ConversationLog::new();
        log.push_user("".to_string());
        assert_eq!(log.blocks().len(), 0);
    }

    #[test]
    fn log_push_assistant_empty() {
        let mut log = ConversationLog::new();
        log.push_assistant("".to_string());
        assert_eq!(log.blocks().len(), 0);
    }

    #[test]
    fn log_push_system() {
        let mut log = ConversationLog::new();
        log.push_system("sys".to_string());
        assert_eq!(log.blocks().len(), 1);
    }

    #[test]
    fn log_push_tool_call_and_result() {
        let mut log = ConversationLog::new();
        log.push_tool_call("call-1".to_string(), "bash".to_string(), "ls".to_string());
        assert_eq!(log.blocks().len(), 1);
        assert_eq!(log.generation(), 1);

        log.set_tool_result("call-1", "output".to_string());
        assert_eq!(log.generation(), 2);

        // Verify the block
        if let Block::Tool { call, result } = &log.blocks()[0] {
            assert_eq!(call.call_id, "call-1");
            assert!(result.is_some());
            assert_eq!(result.as_ref().unwrap().content, "output");
        } else {
            panic!("expected Tool block");
        }
    }

    #[test]
    fn log_set_tool_result_unknown_id() {
        let mut log = ConversationLog::new();
        log.push_tool_call("call-1".to_string(), "read".to_string(), "f.txt".to_string());
        let gen = log.generation();
        log.set_tool_result("call-999", "data".to_string());
        // generation must not change when no block matches
        assert_eq!(log.generation(), gen);
    }

    #[test]
    fn log_push_finish() {
        let mut log = ConversationLog::new();
        log.push_finish("done".to_string(), true);
        assert_eq!(log.blocks().len(), 1);
        if let Block::Finish { message, success } = &log.blocks()[0] {
            assert_eq!(message, "done");
            assert!(*success);
        } else {
            panic!("expected Finish block");
        }
    }

    #[test]
    fn log_append_streaming_basic() {
        let mut log = ConversationLog::new();
        log.set_streaming("".to_string(), false);
        log.append_streaming("hello");
        assert_eq!(log.streaming().as_ref().map(|s| s.content.as_str()), Some("hello"));
    }

    #[test]
    fn log_append_streaming_does_not_exceed_limit() {
        let mut log = ConversationLog::new();
        log.set_streaming("".to_string(), false);

        // Fill to just under the limit
        let base = "a".repeat(MAX_BLOCK_BYTES - 5);
        log.append_streaming(&base);
        assert_eq!(log.streaming().as_ref().map(|s| s.content.len()), Some(MAX_BLOCK_BYTES - 5));

        // Append more than remaining space
        log.append_streaming("bbbbbbbbbb"); // 10 bytes, only 5 remaining
        assert_eq!(log.streaming().as_ref().map(|s| s.content.len()), Some(MAX_BLOCK_BYTES));
    }

    #[test]
    fn log_append_streaming_after_full() {
        let mut log = ConversationLog::new();
        log.set_streaming("".to_string(), false);
        // Fill exactly to limit
        let base = "a".repeat(MAX_BLOCK_BYTES);
        log.append_streaming(&base);
        assert_eq!(log.streaming().as_ref().map(|s| s.content.len()), Some(MAX_BLOCK_BYTES));

        // Further appends should be no-ops
        log.append_streaming("extra");
        assert_eq!(log.streaming().as_ref().map(|s| s.content.len()), Some(MAX_BLOCK_BYTES));
    }

    #[test]
    fn log_append_streaming_no_streaming() {
        let mut log = ConversationLog::new();
        log.append_streaming("hello"); // no streaming block set → no-op
        assert!(log.streaming().is_none());
    }

    #[test]
    fn log_finalize_streaming() {
        let mut log = ConversationLog::new();
        log.set_streaming("hello world".to_string(), false);
        log.finalize_streaming();
        assert!(log.streaming().is_none());
        assert_eq!(log.blocks().len(), 1);
        if let Block::Assistant { content } = &log.blocks()[0] {
            assert_eq!(content, "hello world");
        } else {
            panic!("expected Assistant block");
        }
    }

    #[test]
    fn log_finalize_streaming_thinking() {
        let mut log = ConversationLog::new();
        log.set_streaming("thinking".to_string(), true);
        log.finalize_streaming();
        if let Block::System { content } = &log.blocks()[0] {
            assert_eq!(content, "thinking");
        } else {
            panic!("expected System block");
        }
    }

    #[test]
    fn log_finalize_streaming_empty() {
        let mut log = ConversationLog::new();
        log.set_streaming("".to_string(), false);
        log.finalize_streaming();
        assert!(log.blocks().is_empty()); // empty streaming → no block
    }

    #[test]
    fn log_clear_streaming() {
        let mut log = ConversationLog::new();
        log.set_streaming("data".to_string(), false);
        log.clear_streaming();
        assert!(log.streaming().is_none());
    }

    #[test]
    fn log_generation_increments_on_mutations() {
        let mut log = ConversationLog::new();
        assert_eq!(log.generation(), 0);
        log.push_user("a".to_string());
        assert_eq!(log.generation(), 1);
        log.push_assistant("b".to_string());
        assert_eq!(log.generation(), 2);
        log.set_streaming("c".to_string(), false);
        assert_eq!(log.generation(), 3);
        log.append_streaming("d");
        assert_eq!(log.generation(), 4);
        log.finalize_streaming();
        assert_eq!(log.generation(), 5);
    }

    #[test]
    fn log_streaming_text() {
        let mut log = ConversationLog::new();
        assert_eq!(log.streaming_text(), None);
        log.set_streaming("hi".to_string(), false);
        assert_eq!(log.streaming_text(), Some("hi"));
    }

    #[test]
    fn log_streaming_is_thinking() {
        let mut log = ConversationLog::new();
        assert!(!log.streaming_is_thinking());
        log.set_streaming("".to_string(), true);
        assert!(log.streaming_is_thinking());
    }
}
