use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum CoreEvent {
    /// Initial task context or resume state
    TaskInitialized {
        agent_id: Uuid,
        history_count: usize,
    },
    /// Agent is generating a thought
    ThoughtDelta {
        agent_id: Uuid,
        text: String,
    },
    /// Agent has finished thinking
    ThoughtFinished {
        agent_id: Uuid,
    },
    /// A tool call is being executed
    ToolCallStarted {
        agent_id: Uuid,
        tool: String,
        args: serde_json::Value,
    },
    /// Tool result received
    ToolCallFinished {
        agent_id: Uuid,
        result: serde_json::Value,
    },
    /// Observer (System 1/2) has an insight
    ObserverSignal {
        agent_id: Uuid,
        source: String, 
        confidence: f32,
        message: String,
        action: Option<String>,
    },
    /// Performance metrics for the turn
    MetricsUpdate {
        agent_id: Uuid,
        sqs: f32,
        token_usage: usize,
        latency_ms: u64,
    },
    /// Terminal state reached
    TaskFinished {
        agent_id: Uuid,
        success: bool,
        message: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum FrontendMessage {
    /// Start a new agent for a task
    SpawnAgent {
        task: String,
    },
    /// User provided input to a specific agent
    UserResponse {
        agent_id: Uuid,
        text: String,
    },
    /// User requested an interrupt for a specific agent
    Interrupt {
        agent_id: Uuid,
    },
}
