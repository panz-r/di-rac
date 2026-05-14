use chrono::{DateTime, Utc};
use std::collections::HashSet;
use uuid::Uuid;

/// Maximum byte size for any single block's content. Larger content is truncated.
const MAX_BLOCK_BYTES: usize = 1_048_576; // 1 MiB

fn truncate_content(s: String) -> String {
    if s.len() <= MAX_BLOCK_BYTES {
        s
    } else {
        // Find a char boundary near the limit
        let mut end = MAX_BLOCK_BYTES;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        let mut truncated = String::from(&s[..end]);
        truncated.push_str("\n… [truncated]");
        truncated
    }
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
            if s.content.len() < MAX_BLOCK_BYTES {
                s.content.push_str(text);
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
