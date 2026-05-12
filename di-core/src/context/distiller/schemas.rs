use serde::{Deserialize, Serialize};

/// Distilled tool result — replaces the raw tool output in the trajectory
/// when the model-backed distiller produces a valid result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct DistilledToolResult {
    pub summary: String,
    pub key_facts: Vec<String>,
    pub errors: Vec<String>,
    pub files_referenced: Vec<String>,
    pub estimated_tokens: usize,
    #[serde(default)]
    pub artifact_ref: Option<String>,

    // Phase 3 retrieval enrichment
    #[serde(default)]
    pub exchange_core: String,
    #[serde(default)]
    pub specific_context: Vec<String>,
    #[serde(default)]
    pub thematic_tags: Vec<String>,
    #[serde(default)]
    pub symbols_referenced: Vec<String>,
    #[serde(default)]
    pub exact_evidence: Vec<String>,
    #[serde(default)]
    pub hypotheses: Vec<String>,
    #[serde(default)]
    pub source_event_ids: Vec<String>,
}

/// Task state patch — applied during compaction to enrich the continuation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatePatch {
    pub enriched_summary: String,
    pub open_subgoals: Vec<String>,
    pub decisions: Vec<String>,
    pub critical_files: Vec<String>,
}

/// Checkpoint — a structured snapshot generated at compaction time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub progress_summary: String,
    pub completed: Vec<String>,
    pub remaining: Vec<String>,
    pub risks: Vec<String>,
    pub modified_files: Vec<FileChange>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub latest_failures: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub abandoned_approaches: Vec<String>,
    #[serde(default)]
    pub thematic_tags: Vec<String>,
    #[serde(default)]
    pub source_event_range: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub change_description: String,
}
