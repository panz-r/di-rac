use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Running,
    Waiting,
    Error,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    System,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub tool_name: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum PendingInput {
    Approval {
        tool: String,
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
    pub sqs: f32,
    pub token_usage: usize,
    pub latency_ms: u64,
}

pub struct AgentState {
    pub id: Uuid,
    pub name: String,
    pub status: AgentStatus,
    pub messages: Vec<ChatMessage>,
    pub pending_input: Option<PendingInput>,
    pub streaming_text: Option<String>,
    pub metrics: Option<Metrics>,
    pub last_activity: DateTime<Utc>,
    pub finish_message: Option<String>,
}

impl AgentState {
    pub fn new(id: Uuid, name: String) -> Self {
        Self {
            id,
            name,
            status: AgentStatus::Running,
            messages: Vec::new(),
            pending_input: None,
            streaming_text: None,
            metrics: None,
            last_activity: Utc::now(),
            finish_message: None,
        }
    }

    pub fn is_waiting(&self) -> bool {
        self.pending_input.is_some()
    }

    pub fn display_status(&self) -> &str {
        match self.status {
            AgentStatus::Running => {
                if self.streaming_text.is_some() {
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
