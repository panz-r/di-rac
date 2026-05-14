use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Events emitted by di-core on stdout (NDJSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum CoreEvent {
    TaskInitialized {
        agent_id: Uuid,
        history_count: usize,
    },
    ThoughtDelta {
        agent_id: Uuid,
        text: String,
        #[serde(default)]
        thinking: bool,
    },
    ThoughtFinished {
        agent_id: Uuid,
    },
    ToolCallStarted {
        agent_id: Uuid,
        call_id: String,
        tool: String,
        args: Value,
    },
    ToolCallFinished {
        agent_id: Uuid,
        call_id: String,
        result: Value,
    },
    BackgroundCommandStarted {
        agent_id: Uuid,
        command_id: String,
        command: String,
    },
    BackgroundCommandFinished {
        agent_id: Uuid,
        command_id: String,
        exit_code: Option<i32>,
    },
    ContextCompacted {
        agent_id: Uuid,
        remaining_tokens: usize,
    },
    ApprovalNeeded {
        agent_id: Uuid,
        tool: String,
        args: Value,
        description: String,
    },
    FollowupQuestion {
        agent_id: Uuid,
        question: String,
        options: Option<Vec<String>>,
    },
    ObserverSignal {
        agent_id: Uuid,
        source: String,
        confidence: f32,
        message: String,
        action: Option<String>,
    },
    MetricsUpdate {
        agent_id: Uuid,
        sqs: f32,
        token_usage: usize,
        latency_ms: u64,
    },
    TaskFinished {
        agent_id: Uuid,
        success: bool,
        message: String,
    },
    TaskPresented {
        agent_id: Uuid,
        message: String,
    },
    FrontendTimeout {
        agent_id: Uuid,
        tool: Option<String>,
        question: Option<String>,
    },
}

/// Messages sent by the frontend to di-core on stdin (NDJSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum FrontendMessage {
    SpawnAgent {
        task: String,
    },
    UserResponse {
        agent_id: Uuid,
        text: String,
    },
    Interrupt {
        agent_id: Uuid,
    },
    ApprovalResponse {
        agent_id: Uuid,
        approved: bool,
    },
    FollowupAnswer {
        agent_id: Uuid,
        text: String,
    },
    /// Pass provider config from frontend to di-core so it can use it in gateway requests.
    SetProviderConfig {
        role: String,
        provider: String,
        model: String,
        api_key: Option<String>,
        base_url: Option<String>,
        /// Provider-specific parameters (temperature, top_p, max_tokens, etc.)
        params: std::collections::HashMap<String, serde_json::Value>,
    },
    /// Frontend passes observer behavior settings to di-core.
    SetObserverConfig {
        enabled: bool,
        use_llm_observations: bool,
        watcher_frequency: usize,
        critic_frequency: usize,
        verbose: bool,
        token_threshold: usize,
        buffer_activation: usize,
        block_after: f32,
        reflection_enabled: bool,
        reflection_token_threshold: usize,
        procedural_monotonicity_enabled: bool,
        ast_guided_memory_enabled: bool,
        adaptive_cooldown_enabled: bool,
        latency_budget_ms: u64,
        permissive_buffer_size: usize,
        observer_provider: Option<String>,
        observer_model_id: Option<String>,
    },
}
