use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
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
        args: serde_json::Value,
    },
    ToolCallFinished {
        agent_id: Uuid,
        call_id: String,
        result: serde_json::Value,
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
    /// Agent needs user approval before executing a tool.
    ApprovalNeeded {
        agent_id: Uuid,
        approval_id: Uuid,
        tool: String,
        args: serde_json::Value,
        description: String,
    },
    /// Agent is asking the user a follow-up question.
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
    /// Agent presented a result (done tool) but awaits possible follow-up.
    TaskPresented {
        agent_id: Uuid,
        message: String,
    },
    /// Agent timed out waiting for frontend response (approval or followup).
    FrontendTimeout {
        agent_id: Uuid,
        tool: Option<String>,
        question: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
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
    /// Frontend responds to an approval request.
    ApprovalResponse {
        agent_id: Uuid,
        approval_id: Option<Uuid>,
        approved: bool,
    },
    /// Frontend responds to a follow-up question.
    FollowupAnswer {
        agent_id: Uuid,
        text: String,
    },
    /// Frontend tells agent how long to wait for responses (ms).
    Timeout {
        duration_ms: u64,
    },
    /// Frontend passes provider config so di-core can use it in gateway requests.
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
