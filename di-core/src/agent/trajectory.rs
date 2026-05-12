use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use crate::context::distiller::schemas::Checkpoint;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

/// Structured metadata for Tool messages, recording exactly which file paths
/// were read or written and whether the result was compacted into an artifact.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolMessageMeta {
    pub tool_name: String,
    pub paths_read: Vec<String>,
    pub paths_written: Vec<String>,
    pub is_compacted: bool,
    pub artifact_ref: Option<String>,
}

/// A tool call as emitted by the LLM (for gateway message construction).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEntry {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub role: Role,
    pub content: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub tokens: usize,
    pub is_compressed: bool,
    #[serde(default)]
    pub tool_meta: ToolMessageMeta,
    /// Tool calls attached to this assistant message (empty for non-tool-call messages).
    #[serde(default)]
    pub tool_calls: Vec<ToolCallEntry>,
    /// Tool call ID that this tool message is responding to (for tool role messages).
    #[serde(default)]
    pub tool_call_id: Option<String>,
    /// Thinking text from the assistant (extended thinking / chain-of-thought).
    #[serde(default)]
    pub thinking: Option<String>,
}

pub struct Trajectory {
    pub messages: Vec<Message>,
    pub last_checkpoint: Option<Checkpoint>,
}

impl Trajectory {
    pub fn new() -> Self {
        Self { messages: Vec::new(), last_checkpoint: None }
    }

    pub fn add_message(&mut self, role: Role, content: serde_json::Value, tokens: usize) -> Uuid {
        let msg = Message {
            id: Uuid::new_v4(),
            role,
            content,
            timestamp: Utc::now(),
            tokens,
            is_compressed: false,
            tool_meta: ToolMessageMeta::default(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        };
        let id = msg.id;
        self.messages.push(msg);
        id
    }

    /// Add a tool result message, auto-linking to the Nth tool call from the last assistant message.
    pub fn add_tool_result(&mut self, content: serde_json::Value, tokens: usize, tool_index: usize, meta: ToolMessageMeta) -> Uuid {
        let tool_call_id = self.messages.iter().rev()
            .find(|m| matches!(m.role, Role::Assistant) && !m.tool_calls.is_empty())
            .and_then(|m| m.tool_calls.get(tool_index).map(|tc| tc.id.clone()));
        let msg = Message {
            id: Uuid::new_v4(),
            role: Role::Tool,
            content,
            timestamp: Utc::now(),
            tokens,
            is_compressed: false,
            tool_meta: meta,
            tool_calls: Vec::new(),
            tool_call_id,
            thinking: None,
        };
        let id = msg.id;
        self.messages.push(msg);
        id
    }

    pub fn get_total_tokens(&self) -> usize {
        self.messages.iter().map(|m| m.tokens).sum()
    }

    /// Truncate the entire trajectory, injecting the continuation summary as
    /// a User message so the model retains context about prior work.
    pub fn truncate_with_continuation(&mut self, continuation: String, checkpoint: Option<Checkpoint>) {
        self.last_checkpoint = checkpoint;
        self.messages.clear();

        // Inject the continuation as a User message so the model sees what happened before
        if !continuation.is_empty() {
            let tokens = continuation.len() / 3; // rough estimate
            self.add_message(Role::User, serde_json::json!(continuation), tokens);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_clears_and_sets_checkpoint() {
        let mut traj = Trajectory::new();
        traj.add_message(Role::User, serde_json::json!("hello"), 5);
        traj.add_message(Role::Assistant, serde_json::json!("hi"), 5);
        assert_eq!(traj.messages.len(), 2);

        let checkpoint = Checkpoint {
            progress_summary: "done".to_string(),
            completed: vec!["step1".to_string()],
            remaining: vec![],
            risks: vec![],
            modified_files: vec![],
            artifact_refs: vec!["tool/bash/1".to_string()],
            latest_failures: vec![],
            decisions: vec![],
            abandoned_approaches: vec![],
            thematic_tags: vec![],
            source_event_range: None,
        };

        traj.truncate_with_continuation("continuation".to_string(), Some(checkpoint));

        // After truncation, one User message (the continuation) remains
        assert_eq!(traj.messages.len(), 1);
        assert!(matches!(traj.messages[0].role, Role::User));
        assert!(traj.last_checkpoint.is_some());
        assert_eq!(traj.last_checkpoint.unwrap().artifact_refs.len(), 1);
    }

    #[test]
    fn no_system_message_after_truncation() {
        let mut traj = Trajectory::new();
        traj.add_message(Role::User, serde_json::json!("hello"), 5);
        traj.truncate_with_continuation("continuation".to_string(), None);
        // One User message (the continuation) remains, no System messages
        assert_eq!(traj.messages.len(), 1);
        assert!(matches!(traj.messages[0].role, Role::User));
        assert!(!traj.messages.iter().any(|m| matches!(m.role, Role::System)));
    }

    #[test]
    fn empty_continuation_produces_no_messages() {
        let mut traj = Trajectory::new();
        traj.add_message(Role::User, serde_json::json!("hello"), 5);
        traj.truncate_with_continuation(String::new(), None);
        assert!(traj.messages.is_empty());
    }
}
