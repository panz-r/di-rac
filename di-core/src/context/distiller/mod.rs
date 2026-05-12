pub mod noop;
pub mod model;
pub mod schemas;
pub mod prompts;
pub mod validate;
pub mod admission;

use crate::daemons::ProviderConfig;
use crate::agent::metrics::ContextMetrics;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::sync::Arc;

/// Input type for tool result distillation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ToolDistillInput {
    pub tool_name: String,
    pub tool_args: serde_json::Value,
    pub tool_result: serde_json::Value,
    pub estimated_tokens: usize,
    pub source_event_ids: Vec<Uuid>,
}

/// Input type for task state consolidation.
#[derive(Debug, Clone)]
pub struct TaskStateInput {
    pub recent_assistant_summaries: Vec<String>,
    pub file_context_summary: String,
    pub key_observations: Vec<String>,
    pub source_event_ids: Vec<Uuid>,
}

/// Provenance metadata attached to every distiller output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub source_event_ids: Vec<Uuid>,
    pub confidence: f32,
    pub source: DistillerSource,
    #[serde(default)]
    pub config_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistillerSource {
    Model,
    DeterministicFallback,
}

/// Result wrapper — errors cause fallback, they never propagate to the caller.
#[derive(Debug)]
pub struct DistillationResult<T> {
    pub output: T,
    pub provenance: Provenance,
}

/// Timeout configuration for distiller model calls.
pub struct DistillerTimeouts {
    #[allow(dead_code)]
    pub inline_ms: u64,
    pub compaction_ms: u64,
}

impl Default for DistillerTimeouts {
    fn default() -> Self {
        Self { inline_ms: 8000, compaction_ms: 30000 }
    }
}

/// The Context Distiller trait. All methods return Result but the caller
/// MUST treat errors as "fallback to deterministic" rather than fatal errors.
#[async_trait::async_trait]
pub trait ContextDistiller: Send + Sync {
    #[allow(dead_code)]
    async fn distill_tool_result(&self, input: ToolDistillInput) -> DistillationResult<DistilledToolResult>;
    async fn consolidate_task_state(&self, input: TaskStateInput) -> DistillationResult<TaskStatePatch>;
}

/// Factory: returns NoopContextDistiller if no config, ModelContextDistiller otherwise.
pub fn new_distiller(
    config: Option<ProviderConfig>,
    gateway_client: Arc<crate::daemons::GatewayStreamClient>,
    metrics: Option<Arc<ContextMetrics>>,
    timeouts: Option<DistillerTimeouts>,
) -> Box<dyn ContextDistiller> {
    match config {
        Some(cfg) => Box::new(model::ModelContextDistiller::new(cfg, gateway_client, metrics, timeouts)),
        None => Box::new(noop::NoopContextDistiller),
    }
}

pub use schemas::{DistilledToolResult, TaskStatePatch};
