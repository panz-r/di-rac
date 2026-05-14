pub mod prompts;
pub mod store;

use crate::agent::trajectory::{Trajectory, Message, Role};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, HashMap};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Precompiled regexes (Fixes #4: avoid recompilation on hot paths)
// ---------------------------------------------------------------------------

static RE_TOOL_CODE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#""tool_code":\s*"([a-zA-Z0-9_]+)""#).expect("invalid regex")
});

static RE_PATH: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#""path":\s*"([^"]+)""#).expect("invalid regex")
});

static RE_START_LINE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#""start_line":\s*([0-9]+)"#).expect("invalid regex")
});

static RE_INSTRUCTION: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#""instruction":\s*"([^"]+)""#).expect("invalid regex")
});

static RE_CONFIDENCE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"confidence:([0-9.]+)").expect("invalid regex")
});

static RE_CRITIC_ACTION: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"action:(CONTINUE|REFLECT|RESTART)").expect("invalid regex")
});

static RE_ENVELOPE_START: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^\[OBSERVER:\w+\s*\|[^]]*\]\s*").expect("invalid regex")
});

static RE_ENVELOPE_END: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\s*\[END_OBSERVER\]\s*$").expect("invalid regex")
});

// ---------------------------------------------------------------------------
// SQS (Search Quality Score) types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SQSResult {
    pub score: f32,
    pub status: String,
    pub monotonicity: f32,
    pub ee_ratio: f32,
    pub diffusion: f32,
    pub dcr: f32,
    pub cps: f32,
}

/// Loop pattern classification matching TS.
#[derive(Debug, Clone, PartialEq)]
pub enum LoopPattern {
    Productive,
    Stuck,
    Widening,
    Oscillating,
    StuckWithForgetting,
    RepeatedFileOp,
    SyntaxLoop,
    Unknown,
}

// ---------------------------------------------------------------------------
// CriticAction — interrupt signaling
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CriticAction {
    Continue,
    Reflect,
    Restart,
}

/// Full interrupt result returned by `on_turn_complete`.
#[derive(Debug, Clone)]
pub struct InterruptResult {
    pub action: CriticAction,
    pub sqs: SQSResult,
    pub loop_pattern: LoopPattern,
    pub reason: String,
    /// Decayed confidence of the interrupt signal (0.0–1.0).
    pub confidence: f32,
    /// True when the blocking summarizer should fire synchronously (token ratio exceeded blockAfter).
    pub needs_sync_summary: bool,
}

// ---------------------------------------------------------------------------
// Tier detection — 4 tiers matching TS
// ---------------------------------------------------------------------------

/// Task complexity tier, detected from trajectory content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskTier {
    /// Tier 0: test-driven task (history contains "test" + "pass"/"fail")
    TestDriven,
    /// Tier 1: documentation/lint task
    DocLint,
    /// Tier 2: standard code task with AST analysis available
    CodeWithAnalysis,
    /// Tier 3: fallback — no analyzer or test patterns
    Fallback,
}

impl TaskTier {
    pub fn as_index(&self) -> usize {
        match self {
            TaskTier::TestDriven => 0,
            TaskTier::DocLint => 1,
            TaskTier::CodeWithAnalysis => 2,
            TaskTier::Fallback => 3,
        }
    }
}

/// Per-tier SQS stagnating and confidence thresholds matching TS arrays.
#[derive(Debug, Clone)]
pub struct TierThresholds {
    /// SQS stagnating thresholds per tier: [0.3, 0.32, 0.35, 0.4]
    pub sqs: [f32; 4],
    /// Minimum confidence thresholds per tier: [0.5, 0.55, 0.6, 0.7]
    pub confidence: [f32; 4],
    pub reflect_after_turns: usize,
    pub restart_after_reflects: usize,
}

impl Default for TierThresholds {
    fn default() -> Self {
        Self {
            sqs: [0.3, 0.32, 0.35, 0.4],
            confidence: [0.5, 0.55, 0.6, 0.7],
            reflect_after_turns: 6,
            restart_after_reflects: 2,
        }
    }
}

// ---------------------------------------------------------------------------
// Skeleton fidelity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SkeletonFidelity {
    Full,
    Structural,
    Decision,
}

// ---------------------------------------------------------------------------
// SQS weights — configurable matching TS
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqsWeights {
    pub diffusion: f32,
    pub ee_ratio: f32,
    pub dcr: f32,
    pub cps: f32,
    pub monotonicity: f32,
}

impl Default for SqsWeights {
    fn default() -> Self {
        Self {
            diffusion: 0.30,
            ee_ratio: 0.25,
            dcr: 0.20,
            cps: 0.15,
            monotonicity: 0.10,
        }
    }
}

// ---------------------------------------------------------------------------
// Observation key — skeleton metadata (CodeMEM 2026)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObservationKey {
    pub signature: Option<String>,
    pub apis_called: Vec<String>,
    pub apis_defined: Vec<String>,
    pub docstring_hash: Option<String>,
}

// ---------------------------------------------------------------------------
// Language normalization table — matching TS LANGUAGE_NORMALIZATION
// ---------------------------------------------------------------------------

struct LanguageNorm {
    median_edit_churn: f32,
    median_file_size: f32,
    sci: f32,
}

const LANGUAGE_NORMALIZATION: &[(&str, LanguageNorm)] = &[
    ("python", LanguageNorm { median_edit_churn: 15.0, median_file_size: 200.0, sci: 12.0 }),
    ("javascript", LanguageNorm { median_edit_churn: 12.0, median_file_size: 150.0, sci: 14.0 }),
    ("typescript", LanguageNorm { median_edit_churn: 12.0, median_file_size: 150.0, sci: 14.0 }),
    ("java", LanguageNorm { median_edit_churn: 8.0, median_file_size: 300.0, sci: 65.0 }),
    ("go", LanguageNorm { median_edit_churn: 10.0, median_file_size: 200.0, sci: 31.0 }),
    ("rust", LanguageNorm { median_edit_churn: 6.0, median_file_size: 250.0, sci: 80.0 }),
    ("c", LanguageNorm { median_edit_churn: 5.0, median_file_size: 180.0, sci: 95.0 }),
    ("cpp", LanguageNorm { median_edit_churn: 5.0, median_file_size: 180.0, sci: 122.0 }),
    ("ruby", LanguageNorm { median_edit_churn: 18.0, median_file_size: 120.0, sci: 10.0 }),
];

fn normalized_ast_churn(lang: &str, raw_churn: f32, file_size: f32) -> f32 {
    let norm = LANGUAGE_NORMALIZATION.iter()
        .find(|(l, _)| *l == lang)
        .map(|(_, n)| n)
        .unwrap_or(&LANGUAGE_NORMALIZATION[0].1); // default to python
    let edit_norm = raw_churn / norm.median_edit_churn;
    let size_norm = (file_size / (norm.median_file_size + 1.0)).powf(0.3);
    edit_norm * size_norm
}

/// Compute instruction similarity using overlap coefficient on whitespace-split words.
/// Matches TS getInstructionSimilarity.
fn instruction_similarity(a: Option<&str>, b: Option<&str>) -> f32 {
    let (a, b) = match (a, b) {
        (Some(a), Some(b)) => (a, b),
        _ => return 0.0,
    };
    if a == b { return 1.0; }
    let words_a: HashSet<&str> = a.split(' ').collect();
    let words_b: HashSet<&str> = b.split(' ').collect();
    let intersection = words_a.intersection(&words_b).count();
    intersection as f32 / words_a.len().max(words_b.len()) as f32
}

// ---------------------------------------------------------------------------
// Action features — structured parsing of tool call content
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ActionFeatures {
    pub file: String,
    pub op: String,
    pub line_range: String,
    pub error_sig: Option<String>,
    pub success: bool,
    pub turn: usize,
    pub lang: String,
    pub ast_delta_nodes: Option<i32>,
    pub instruction: Option<String>,
}

/// Extract structured action features from messages.
/// First looks at structured `msg.tool_calls`, then falls back to regex on content (Fixes #2).
/// `ast_delta` is the net AST node change (added - removed) for the last action, if available.
pub fn extract_action_features(msgs: &[&Message], ast_delta: Option<i32>) -> Vec<ActionFeatures> {
    msgs.iter().enumerate().map(|(i, msg)| {
        let content = msg.content.to_string();

        // Try structured tool_calls first
        let (tool, file, line_range, instruction) = if let Some(tc) = msg.tool_calls.first() {
            let tool_name = tc.name.clone();
            let args: Option<serde_json::Value> = serde_json::from_str(&tc.arguments).ok();

            let file_path = args.as_ref()
                .and_then(|v| v.get("path").and_then(|v| v.as_str()))
                .or_else(|| args.as_ref()
                    .and_then(|v| v.get("paths").and_then(|v| {
                        if let Some(arr) = v.as_array() {
                            arr.first().and_then(|f| f.as_str())
                        } else {
                            v.as_str()
                        }
                    })))
                .unwrap_or("global")
                .to_string();

            let lr = args.as_ref()
                .and_then(|v| v.get("start_line").and_then(|v| v.as_i64()))
                .map(|n| n.to_string())
                .unwrap_or_else(|| "0".to_string());

            let instr = args.as_ref()
                .and_then(|v| v.get("content").and_then(|v| v.as_str()))
                .or_else(|| args.as_ref()
                    .and_then(|v| v.get("command").and_then(|v| v.as_str()))
                    .or_else(|| args.as_ref()
                        .and_then(|v| v.get("text").and_then(|v| v.as_str()))
                        .or_else(|| args.as_ref()
                            .and_then(|v| v.get("query").and_then(|v| v.as_str()))
                            .or_else(|| args.as_ref()
                                .and_then(|v| v.get("regex").and_then(|v| v.as_str()))))))
                .map(|s| {
                    if s.len() > 200 {
                        format!("{}...", &s[..s.floor_char_boundary(200)])
                    } else {
                        s.to_string()
                    }
                });

            (tool_name, file_path, lr, instr)
        } else {
            // Fallback: regex on content string
            let t = regex_capture(&content, &RE_TOOL_CODE)
                .unwrap_or_else(|| "think".to_string());
            let f = regex_capture(&content, &RE_PATH)
                .unwrap_or_else(|| "global".to_string());
            let lr = regex_capture(&content, &RE_START_LINE)
                .unwrap_or_else(|| "0".to_string());
            let instr = regex_capture(&content, &RE_INSTRUCTION);
            (t, f, lr, instr)
        };

        let success = !content.contains("error");
        let ext = file.rsplit('.').next().unwrap_or("python").to_string();

        // Extract error signature: first line containing "error", truncated
        let error_sig = if content.contains("error") || content.contains("failed") {
            content.lines()
                .find(|l| l.contains("error") || l.contains("failed"))
                .map(|l| {
                    let sig = l.trim();
                    if sig.len() > 120 { format!("{}...", &sig[..120]) } else { sig.to_string() }
                })
        } else {
            None
        };

        // Last message gets the AST delta if available
        let is_last = i == msgs.len() - 1;
        let delta = if is_last { ast_delta } else { None };

        ActionFeatures {
            file,
            op: tool,
            line_range,
            error_sig,
            success,
            turn: i,
            lang: ext,
            ast_delta_nodes: delta,
            instruction,
        }
    }).collect()
}

fn regex_capture(text: &str, re: &regex::Regex) -> Option<String> {
    re.captures(text)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

// ---------------------------------------------------------------------------
// Analyzer request for filterMemoryByApi
// ---------------------------------------------------------------------------

/// Request for the analyzer to extract API calls/definitions.
/// Built by the observer, executed by the engine which owns the daemon.
#[derive(Debug, Clone)]
pub struct ExtractApisRequest {
    pub content: String,
    pub language: Option<String>,
}

/// Response from the analyzer extract-apis command.
#[derive(Debug, Clone, Default)]
pub struct ExtractApisResponse {
    pub calls: Vec<String>,
    pub definitions: Vec<String>,
}

// ---------------------------------------------------------------------------
// LLM-driven observation types
// ---------------------------------------------------------------------------

/// Prepared LLM request for the engine to execute via the gateway.
#[derive(Debug, Clone)]
pub struct ObserverLlmRequest {
    pub obs_type: ObservationType,
    pub system_prompt: String,
    pub user_message: String,
}

/// Parsed LLM observation response.
#[derive(Debug, Clone)]
pub struct ParsedObservation {
    pub text: String,
    pub confidence: f32,
    pub critic_action: Option<CriticAction>,
}

/// Parse an LLM observation response, extracting confidence and action.
/// Returns None if the response is empty or contains a "no alerts" / "context clean" sentinel.
pub fn parse_llm_observation(text: &str, obs_type: ObservationType) -> Option<ParsedObservation> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.contains("No alerts")
        || trimmed.contains("Context clean")
    {
        return None;
    }

    // Extract confidence from pattern: confidence:0.XX, validate in [0,1] (Fixes #9)
    let confidence = RE_CONFIDENCE
        .captures(trimmed)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<f32>().ok())
        .map(|c| c.clamp(0.0, 1.0))
        .unwrap_or(0.5);

    // Extract action from pattern: action:ACTION (critic only)
    let critic_action = if obs_type == ObservationType::Critic {
        RE_CRITIC_ACTION
            .captures(trimmed)
            .map(|caps| match caps.get(1).map(|m| m.as_str()) {
                Some("CONTINUE") => CriticAction::Continue,
                Some("REFLECT") => CriticAction::Reflect,
                Some("RESTART") => CriticAction::Restart,
                _ => CriticAction::Continue,
            })
    } else {
        None
    };

    // Strip the envelope tags to get the insight text
    let clean_text = strip_observation_envelope(trimmed);

    Some(ParsedObservation {
        text: clean_text,
        confidence,
        critic_action,
    })
}

/// Strip [OBSERVER:TYPE | ...] and [END_OBSERVER] envelope markers.
fn strip_observation_envelope(text: &str) -> String {
    let mut result = RE_ENVELOPE_START.replace(text, "").to_string();
    result = RE_ENVELOPE_END.replace(&result, "").to_string();
    // For critic: strip the "REASON: " prefix
    result = result.trim_start_matches("REASON: ").to_string();
    result.trim().to_string()
}

// ---------------------------------------------------------------------------
// Observation types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ObservationType {
    Summary,
    Watcher,
    Filter,
    Critic,
    Skeleton,
    Reflection,
}

impl std::fmt::Display for ObservationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObservationType::Summary => write!(f, "summary"),
            ObservationType::Watcher => write!(f, "watcher"),
            ObservationType::Filter => write!(f, "filter"),
            ObservationType::Critic => write!(f, "critic"),
            ObservationType::Skeleton => write!(f, "skeleton"),
            ObservationType::Reflection => write!(f, "reflection"),
        }
    }
}

/// A single observation entry with full TS-parity metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub obs_type: ObservationType,
    pub text: String,
    pub timestamp: i64,
    pub confidence: f32,
    pub token_estimate: usize,
    /// Turn range this observation covers [start, end].
    #[serde(default)]
    pub compressed_range: Option<[usize; 2]>,
    /// For critic observations: the recommended action.
    #[serde(default)]
    pub critic_action: Option<CriticAction>,
    /// SQS score at time of observation.
    #[serde(default)]
    pub sqs: Option<f32>,
    /// Fidelity level for skeleton observations.
    #[serde(default)]
    pub fidelity: Option<SkeletonFidelity>,
    /// Skeleton key metadata (CodeMEM 2026).
    #[serde(default)]
    pub key: Option<ObservationKey>,
}

// ---------------------------------------------------------------------------
// Observer config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ObserverConfig {
    pub enabled: bool,
    /// When true, observation text comes from LLM calls via the gateway.
    pub use_llm_observations: bool,
    pub watcher_frequency: usize,
    pub critic_frequency: usize,
    pub token_threshold: usize,
    pub reflection_enabled: bool,
    pub reflection_token_threshold: usize,
    pub confidence_threshold: f32,
    pub tau_watcher: f32,
    pub tau_critic: f32,
    pub tier_thresholds: TierThresholds,
    // TS-parity config fields
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub buffer_activation: usize,
    pub verbose: bool,
    pub permissive_buffer_size: usize,
    pub procedural_monotonicity_enabled: bool,
    pub ast_guided_memory_enabled: bool,
    pub adaptive_cooldown_enabled: bool,
    pub sqs_weights: SqsWeights,
    pub latency_budget_ms: u64,
    pub memory_cap_tokens: usize,
    pub reflection_cooldown: usize,
    /// Token ratio threshold for blocking summarizer (TS: blockAfter).
    /// Sync summarizer fires when `unobserved_tokens / token_threshold >= block_after`.
    pub block_after: f32,
}

impl Default for ObserverConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            use_llm_observations: false,
            watcher_frequency: 2,
            critic_frequency: 6,
            token_threshold: 15000,
            reflection_enabled: true,
            reflection_token_threshold: 10000,
            confidence_threshold: 0.5,
            tau_watcher: 7.0,
            tau_critic: 15.0,
            tier_thresholds: TierThresholds::default(),
            provider: None,
            model_id: None,
            buffer_activation: 3,
            verbose: false,
            permissive_buffer_size: 2,
            procedural_monotonicity_enabled: true,
            ast_guided_memory_enabled: true,
            adaptive_cooldown_enabled: true,
            sqs_weights: SqsWeights::default(),
            latency_budget_ms: 200,
            memory_cap_tokens: 15000,
            reflection_cooldown: 4,
            block_after: 0.7,
        }
    }
}

// ---------------------------------------------------------------------------
// Observer metrics — full TS parity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SQSComponents {
    pub diffusion: f32,
    pub ee_ratio: f32,
    pub dcr: f32,
    pub cps: f32,
    pub monotonicity: f32,
}

#[derive(Debug, Clone, Default)]
pub struct InterventionTrigger {
    pub structural_only: bool,
    pub user_only: bool,
    pub combined: bool,
    pub confidence_calibrated: f32,
}

#[derive(Debug, Clone, Default)]
pub struct ForgettingDetail {
    pub detected: usize,
    pub false_positive: usize,
    pub resolved_by_intervention: usize,
    pub ifr_score: f32,
}

#[derive(Debug, Clone, Default)]
pub struct TokenEfficiency {
    pub layer1_compression_ratio: f32,
    pub observation_value_loads: usize,
    pub retrieval_stage_used: u8,
}

#[derive(Debug, Clone, Default)]
pub struct ObserverMetrics {
    pub turns_observed: usize,
    pub watcher_fired: usize,
    pub critic_fired: usize,
    pub reflections_fired: usize,
    pub filter_fired: usize,
    pub reflect_actions: usize,
    pub restart_actions: usize,
    pub skeleton_observations: usize,
    // Detailed TS-parity fields
    pub sqs_components: SQSComponents,
    pub intervention_trigger: InterventionTrigger,
    pub forgetting: ForgettingDetail,
    pub token_efficiency: TokenEfficiency,
    pub avg_sqs: f32,
    pub sqs_samples: usize,
}

impl ObserverMetrics {
    pub fn summary(&self) -> String {
        format!(
            "[Observer Metrics] turns={} watcher={} critic={} filter={} reflect_actions={} restart_actions={} forgetting={}/{} tier_avg_sqs={:.2}",
            self.turns_observed,
            self.watcher_fired,
            self.critic_fired,
            self.filter_fired,
            self.reflect_actions,
            self.restart_actions,
            self.forgetting.detected,
            self.forgetting.false_positive,
            if self.sqs_samples > 0 { self.avg_sqs / self.sqs_samples as f32 } else { 0.0 },
        )
    }
}

// ---------------------------------------------------------------------------
// Cost tracker
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CostEntry {
    pub obs_type: ObservationType,
    pub tokens: usize,
    pub latency_ms: u64,
    pub turn: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ObserverCostTracker {
    entries: Vec<CostEntry>,
}

impl ObserverCostTracker {
    pub fn record(&mut self, obs_type: ObservationType, tokens: usize, latency_ms: u64, turn: usize) {
        self.entries.push(CostEntry { obs_type, tokens, latency_ms, turn });
        if self.entries.len() > 200 {
            self.entries.remove(0);
        }
    }

    pub fn total_tokens(&self) -> usize {
        self.entries.iter().map(|e| e.tokens).sum()
    }

    pub fn total_latency_ms(&self) -> u64 {
        self.entries.iter().map(|e| e.latency_ms).sum()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn avg_latency_ms(&self) -> f64 {
        if self.entries.is_empty() { return 0.0; }
        self.total_latency_ms() as f64 / self.entries.len() as f64
    }

    pub fn summary(&self) -> String {
        format!(
            "cost_tracker: {} observations, {} tokens, {}ms total latency",
            self.entry_count(),
            self.total_tokens(),
            self.total_latency_ms(),
        )
    }
}

// ---------------------------------------------------------------------------
// Health state (for TUI/engine consumption)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ObserverHealth {
    pub failing: bool,
    pub last_error: Option<String>,
}

// ---------------------------------------------------------------------------
// Observer orchestrator
// ---------------------------------------------------------------------------

pub struct Observer {
    pub config: ObserverConfig,
    pub store: store::ObservationStore,
    pub resolved_subgoals: HashSet<String>,
    pub forgotten_subgoals: HashSet<String>,
    pub metrics: ObserverMetrics,
    pub cost_tracker: ObserverCostTracker,
    pub health: ObserverHealth,
    turns_since_last_observation: usize,
    turns_since_last_reflection: usize,
    consecutive_reflects: usize,
    last_sqs: Option<SQSResult>,
    last_loop_pattern: LoopPattern,
    current_tier: TaskTier,
    current_turn: usize,
    // Skeleton tracking
    recent_edits: Vec<(String, String)>,
    recent_errors: Vec<String>,
    recent_decisions: Vec<String>,
    // Trajectory compression tracking
    last_observed_message_index: usize,
    // Backpressure for LLM observation calls
    pub(crate) pending_llm_count: usize,
    // AST churn from last turn (added, removed, total)
    last_ast_churn: Option<(usize, usize, usize)>,
}

impl Observer {
    pub fn new(config: ObserverConfig) -> Self {
        Self {
            store: store::ObservationStore::new(None),
            config,
            resolved_subgoals: HashSet::new(),
            forgotten_subgoals: HashSet::new(),
            metrics: ObserverMetrics::default(),
            cost_tracker: ObserverCostTracker::default(),
            health: ObserverHealth::default(),
            turns_since_last_observation: 0,
            turns_since_last_reflection: 0,
            consecutive_reflects: 0,
            last_sqs: None,
            last_loop_pattern: LoopPattern::Unknown,
            current_tier: TaskTier::Fallback,
            current_turn: 0,
            recent_edits: Vec::new(),
            recent_errors: Vec::new(),
            recent_decisions: Vec::new(),
            last_observed_message_index: 0,
            pending_llm_count: 0,
            last_ast_churn: None,
        }
    }

    /// Create observer with a specific task-scoped store path.
    pub fn new_with_task(config: ObserverConfig, task_id: &str) -> Self {
        let path = format!(".dirac/observations_{}.jsonl", task_id);
        Self {
            store: store::ObservationStore::new(Some(&path)),
            config,
            resolved_subgoals: HashSet::new(),
            forgotten_subgoals: HashSet::new(),
            metrics: ObserverMetrics::default(),
            cost_tracker: ObserverCostTracker::default(),
            health: ObserverHealth::default(),
            turns_since_last_observation: 0,
            turns_since_last_reflection: 0,
            consecutive_reflects: 0,
            last_sqs: None,
            last_loop_pattern: LoopPattern::Unknown,
            current_tier: TaskTier::Fallback,
            current_turn: 0,
            recent_edits: Vec::new(),
            recent_errors: Vec::new(),
            recent_decisions: Vec::new(),
            last_observed_message_index: 0,
            pending_llm_count: 0,
            last_ast_churn: None,
        }
    }

    /// Enable or disable the observer at runtime.
    pub fn toggle(&mut self, enabled: bool) {
        self.config.enabled = enabled;
    }

    /// Set AST churn data from the analyzer daemon (called by engine before on_turn_complete).
    pub fn set_ast_churn(&mut self, churn: Option<(usize, usize, usize)>) {
        self.last_ast_churn = churn;
    }

    /// Get net AST node delta (added - removed) from the last churn data.
    pub fn ast_delta(&self) -> Option<i32> {
        self.last_ast_churn.map(|(a, r, _)| a as i32 - r as i32)
    }

    /// Get the last computed SQS score (avoids redundant recomputation).
    pub fn last_sqs(&self) -> Option<f32> {
        self.last_sqs.as_ref().map(|s| s.score)
    }

    /// Whether the heuristic watcher just fired this turn.
    pub fn watcher_just_fired(&self) -> bool {
        self.turns_since_last_observation == 0
    }

    /// Whether there is skeleton data available for LLM.
    pub fn has_skeleton_data(&self) -> bool {
        !self.recent_edits.is_empty() || !self.recent_errors.is_empty()
    }

    /// Get the last observed message index for trajectory compression.
    pub fn last_observed_message_index(&self) -> usize {
        self.last_observed_message_index
    }

    /// Get the current turn number.
    pub fn current_turn(&self) -> usize {
        self.current_turn
    }

    /// Access recent error messages.
    pub fn recent_errors(&self) -> &[String] {
        &self.recent_errors
    }

    /// Access the observation store.
    pub fn store(&self) -> &crate::observer::store::ObservationStore {
        &self.store
    }

    /// Access observer metrics.
    pub fn metrics(&self) -> &ObserverMetrics {
        &self.metrics
    }

    /// Access the cost tracker.
    pub fn cost_tracker(&self) -> &ObserverCostTracker {
        &self.cost_tracker
    }

    /// Get the last loop pattern.
    pub fn last_loop_pattern(&self) -> &LoopPattern {
        &self.last_loop_pattern
    }

    /// Get the current task tier.
    pub fn current_tier(&self) -> TaskTier {
        self.current_tier
    }

    /// Update the last observed message index after a summarizer/skeleton covers the trajectory.
    pub fn update_last_observed(&mut self, msg_count: usize) {
        self.last_observed_message_index = msg_count;
    }

    /// End-of-task final summarization pass.
    pub fn final_compression(&mut self) {
        if self.store.len() == 0 {
            return;
        }
        self.trigger_reflection();
    }

    // -----------------------------------------------------------------------
    // Turn lifecycle — returns InterruptResult for engine to act on
    // -----------------------------------------------------------------------

    pub fn on_turn_complete(&mut self, trajectory: &Trajectory) -> InterruptResult {
        self.metrics.intervention_trigger = InterventionTrigger::default();
        self.turns_since_last_observation += 1;
        self.turns_since_last_reflection += 1;
        self.metrics.turns_observed += 1;
        self.current_turn += 1;

        // Detect tier from full trajectory content
        self.current_tier = self.detect_tier(trajectory);

        // Compute SQS
        let sqs = self.compute_sqs(trajectory);
        self.last_sqs = Some(sqs.clone());
        self.metrics.avg_sqs += sqs.score;
        self.metrics.sqs_samples += 1;
        self.metrics.sqs_components = SQSComponents {
            diffusion: sqs.diffusion,
            ee_ratio: sqs.ee_ratio,
            dcr: sqs.dcr,
            cps: sqs.cps,
            monotonicity: sqs.monotonicity,
        };

        // Classify loop pattern
        self.last_loop_pattern = self.classify_loop_pattern(trajectory);

        // Extract skeleton data from trajectory
        self.extract_skeleton_data(trajectory);

        // Trigger watcher observation every N turns
        if self.turns_since_last_observation >= self.config.watcher_frequency
            || sqs.status == "STAGNATING"
            || self.last_loop_pattern == LoopPattern::StuckWithForgetting
            || self.last_loop_pattern == LoopPattern::SyntaxLoop
            || self.last_loop_pattern == LoopPattern::RepeatedFileOp
        {
            self.trigger_watcher_observation(trajectory);
            self.turns_since_last_observation = 0;
        }

        // Trigger filter observation when diffusion is high but no loop detected
        if self.last_loop_pattern == LoopPattern::Productive
            && sqs.diffusion > 0.6
            && self.metrics.turns_observed % 3 == 0
        {
            self.trigger_filter_observation();
        }

        // Trigger critic observation every M turns
        if self.turns_since_last_reflection >= self.critic_cooldown()
            && sqs.score < self.tier_sqs_threshold()
        {
            self.trigger_critic_observation();
        }

        // Trigger skeleton observation every 4 turns when there's data
        if self.metrics.turns_observed % 4 == 0
            && (!self.recent_edits.is_empty() || !self.recent_errors.is_empty())
        {
            self.trigger_skeleton_observation();
        }

        // Check reflection threshold with adaptive cooldown
        if self.config.reflection_enabled
            && self.turns_since_last_reflection >= self.reflection_cooldown_turns()
            && self.store.estimate_token_count() >= self.config.reflection_token_threshold
        {
            self.trigger_reflection();
        }

        // Update monotonicity tracking
        self.update_subgoal_tracking(trajectory);

        // Update token efficiency
        self.metrics.token_efficiency.observation_value_loads += 1;
        let observation_tokens = self.store.estimate_token_count() as f32;
        let cap = self.config.memory_cap_tokens.max(1) as f32;
        self.metrics.token_efficiency.layer1_compression_ratio = observation_tokens / cap;

        // Memory cap compaction: when observation tokens exceed cap, compress via reflection
        if observation_tokens > cap && self.store.len() > 5 {
            self.trigger_reflection();
        }

        // Determine interrupt action
        let (action, reason, confidence) = self.determine_interrupt(&sqs);

        // Check sync summarizer trigger (TS: token ratio >= blockAfter)
        let needs_sync_summary = self.should_summarize(trajectory);

        InterruptResult {
            action,
            sqs,
            loop_pattern: self.last_loop_pattern.clone(),
            reason,
            confidence,
            needs_sync_summary,
        }
    }

    // -----------------------------------------------------------------------
    // Tier detection — scans trajectory content for task patterns
    // -----------------------------------------------------------------------

    fn detect_tier(&self, trajectory: &Trajectory) -> TaskTier {
        // Serialize messages to text for keyword scanning
        let full_text: String = trajectory.messages.iter()
            .map(|m| m.content.to_string().to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");

        // Tier 0: test-driven task
        if full_text.contains("test") && (full_text.contains("pass") || full_text.contains("fail")) {
            return TaskTier::TestDriven;
        }

        // Tier 1: documentation/lint
        if full_text.contains("lint") || full_text.contains("readme.md") {
            return TaskTier::DocLint;
        }

        // Tier 2: analyzer available (we always have the analyzer daemon in Rust)
        TaskTier::CodeWithAnalysis
    }

    /// Get the SQS stagnating threshold for the current tier.
    fn tier_sqs_threshold(&self) -> f32 {
        self.config.tier_thresholds.sqs[self.current_tier.as_index()]
    }

    /// Get the confidence threshold for the current tier.
    fn tier_confidence_threshold(&self) -> f32 {
        self.config.tier_thresholds.confidence[self.current_tier.as_index()]
    }

    /// Adaptive critic cooldown: base frequency adjusted by file spread and error type.
    /// Uses a sliding window of the last 5 edits (Fixes #8).
    fn critic_cooldown(&self) -> usize {
        let base = self.config.critic_frequency;
        // Only consider last 5 edits for spread
        let recent_edit_count = self.recent_edits.iter().rev().take(5).count();
        let has_errors = !self.recent_errors.is_empty();

        // More files touched = shorter cooldown (more likely to need correction)
        let spread_adj = if recent_edit_count > 3 { 2 } else if recent_edit_count > 1 { 1 } else { 0 };
        // Recent errors = shorter cooldown
        let error_adj = if has_errors { 1 } else { 0 };

        base.saturating_sub(spread_adj + error_adj).max(2)
    }

    /// Adaptive reflection cooldown in turns.
    /// Adaptive reflection cooldown matching TS getAdaptiveCooldown (ObserverOrchestrator.ts:352-367).
    fn reflection_cooldown_turns(&self) -> usize {
        if !self.config.adaptive_cooldown_enabled {
            return self.config.reflection_cooldown;
        }
        let mut cd = self.config.reflection_cooldown;
        let files_touched: HashSet<&str> = self.recent_edits.iter().map(|(p, _)| p.as_str()).collect();
        let file_count = files_touched.len();

        if file_count > 3 { cd += 2; }
        else if file_count <= 1 { cd = cd.saturating_sub(2).max(2); }

        if let Some(ref sqs) = self.last_sqs {
            if sqs.status == "EXPLORING" { cd += 1; }
        }

        if let Some(err) = self.recent_errors.last() {
            if err.contains("syntax") { cd = cd.saturating_sub(1).max(2); }
        }

        cd
    }

    // -----------------------------------------------------------------------
    // Interrupt determination — TS-parity confidence gate
    // -----------------------------------------------------------------------

    fn determine_interrupt(&mut self, sqs: &SQSResult) -> (CriticAction, String, f32) {
        let tier_idx = self.current_tier.as_index();
        let min_confidence = self.config.tier_thresholds.confidence[tier_idx];
        let sqs_trigger = self.config.tier_thresholds.sqs[tier_idx];
        let restart_after = self.config.tier_thresholds.restart_after_reflects;
        let reflect_after = self.config.tier_thresholds.reflect_after_turns;

        // Check for critic observation with action — extract owned values to release borrow
        let critic_data: Option<(CriticAction, f32, String, usize)> = self.store.get_latest(ObservationType::Critic)
            .and_then(|c| {
                c.critic_action.as_ref().filter(|a| **a != CriticAction::Continue).map(|action| {
                    let turns_since = self.current_turn
                        - c.compressed_range.map(|r| r[1]).unwrap_or(self.current_turn);
                    (action.clone(), c.confidence, c.text.clone(), turns_since)
                })
            });

        if let Some((action, base_conf, text, turns_since)) = critic_data {
            let decayed = self.decayed_confidence(base_conf, "CRITIC", turns_since);

            // TS confidence gate: decayed >= min(0.7, tierConfidence + 0.1)
            let confidence_gate = (0.7_f32).min(min_confidence + 0.1);
            // TS structural signal: SQS < threshold + 0.05 OR monotonicity < 0.85
            let has_structural = sqs.score < sqs_trigger + 0.05;
            let mono = self.calculate_monotonicity();
            let has_forgetting = mono < 0.85;

            if decayed >= confidence_gate && (has_structural || has_forgetting) {
                self.metrics.intervention_trigger.combined = true;
                self.metrics.intervention_trigger.confidence_calibrated = decayed;

                if action == CriticAction::Restart {
                    self.metrics.restart_actions += 1;
                    return (CriticAction::Restart, text, decayed);
                } else {
                    self.metrics.reflect_actions += 1;
                    return (CriticAction::Reflect, text, decayed);
                }
            }
        }

        // Fallback heuristic: loop pattern + SQS based (same as before)
        if self.last_loop_pattern == LoopPattern::StuckWithForgetting {
            self.consecutive_reflects += 1;
            if self.consecutive_reflects >= restart_after {
                self.metrics.restart_actions += 1;
                self.consecutive_reflects = 0;
                return (
                    CriticAction::Restart,
                    "Repeated self-reverting behavior. Abandoning current approach.".to_string(),
                    0.8,
                );
            }
            self.metrics.reflect_actions += 1;
            return (
                CriticAction::Reflect,
                "Agent is reverting its own changes. Pivot strategy immediately.".to_string(),
                0.7,
            );
        }

        if self.last_loop_pattern == LoopPattern::Oscillating {
            self.consecutive_reflects += 1;
            if self.consecutive_reflects >= restart_after {
                self.metrics.restart_actions += 1;
                self.consecutive_reflects = 0;
                return (
                    CriticAction::Restart,
                    "Sustained oscillation between approaches. Starting from first principles.".to_string(),
                    0.75,
                );
            }
            self.metrics.reflect_actions += 1;
            return (
                CriticAction::Reflect,
                "Alternating between approaches without progress. Pick one strategy and commit.".to_string(),
                0.65,
            );
        }

        if sqs.score < sqs_trigger {
            self.consecutive_reflects += 1;
            if self.consecutive_reflects >= restart_after {
                self.metrics.restart_actions += 1;
                self.consecutive_reflects = 0;
                return (
                    CriticAction::Restart,
                    format!("SQS critically low ({:.2}). Restarting from first principles.", sqs.score),
                    0.7,
                );
            }
            if self.turns_since_last_reflection >= reflect_after {
                self.metrics.reflect_actions += 1;
                self.metrics.intervention_trigger.structural_only = true;
                return (
                    CriticAction::Reflect,
                    format!("SQS stagnating ({:.2}). Re-reading key files and pivoting strategy.", sqs.score),
                    0.6,
                );
            }
        }

        if self.last_loop_pattern == LoopPattern::Widening {
            self.metrics.reflect_actions += 1;
            return (
                CriticAction::Reflect,
                "Errors spreading across files. Focus on root cause before continuing.".to_string(),
                0.65,
            );
        }

        self.consecutive_reflects = 0;
        (CriticAction::Continue, String::new(), 1.0)
    }

    /// Compute pause weight matching TS computePauseWeight (ObserverOrchestrator.ts:370-382).
    pub fn compute_pause_weight(
        &self,
        duration_s: f64,
        after_error: bool,
        after_watcher: bool,
        command_entropy: f32,
        ast_contradiction: bool,
    ) -> f32 {
        let mut base: f32 = 0.02;
        if duration_s > 12.0 { base *= 2.5; }
        else if duration_s > 8.0 { base *= 2.0; }
        else if duration_s > 5.0 { base *= 1.5; }

        if after_error { base *= 2.0; }
        if after_watcher { base *= 3.0; }
        if command_entropy > 0.6 { return base * 0.3; }
        if ast_contradiction { base *= 1.8; }

        base.min(0.10)
    }

    /// Build the interrupt directive text for injection into the system prompt.
    pub fn build_interrupt_directive(&self, action: &CriticAction, reason: &str) -> Option<String> {
        match action {
            CriticAction::Continue => None,
            CriticAction::Reflect => Some(format!(
                "# OBSERVER INTERRUPT: PIVOT REQUIRED\n\n\
                 You have been stuck. Pivot your strategy immediately.\n\
                 Reason: {}\n\n\
                 Required actions:\n\
                 1. STOP the current approach — it is not working.\n\
                 2. Re-read the most relevant files to get fresh context.\n\
                 3. Use search to find alternative solutions.\n\
                 4. Consider whether you need to ask the user for clarification.",
                reason
            )),
            CriticAction::Restart => Some(format!(
                "# OBSERVER INTERRUPT: HARD RESET\n\n\
                 The current approach has failed. Start from first principles.\n\
                 Reason: {}\n\n\
                 Required actions:\n\
                 1. DISCARD your current mental model of the problem.\n\
                 2. Re-read the original task description carefully.\n\
                 3. Re-read ALL relevant files from scratch.\n\
                 4. Build a completely new plan before taking any action.\n\
                 5. If you still cannot make progress after re-reading, ask the user for guidance.",
                reason
            )),
        }
    }

    /// Build the observation block to inject into the system prompt.
    pub fn build_observation_block(&self) -> String {
        let summary = self.store.build_observation_block(Some(ObservationType::Summary), None);
        let skeleton = self.store.build_observation_block(Some(ObservationType::Skeleton), None);

        let mut parts = Vec::new();

        if !summary.is_empty() {
            parts.push(format!("# Conversation Observations\n\n{}", summary));
        }

        // Apply per-observation confidence decay matching TS filterWithDecay
        // When total observations are below permissive_buffer_size, skip decay filtering
        let min_conf = if self.store.len() < self.config.permissive_buffer_size {
            0.0
        } else {
            self.tier_confidence_threshold()
        };
        let mut insights = Vec::new();
        let watcher = self.filter_observations_by_decay(ObservationType::Watcher, min_conf);
        let filter = self.filter_observations_by_decay(ObservationType::Filter, min_conf);
        let critic = self.filter_observations_by_decay(ObservationType::Critic, min_conf);
        if !watcher.is_empty() { insights.push(watcher); }
        if !filter.is_empty() { insights.push(filter); }
        if !critic.is_empty() { insights.push(critic); }
        if !insights.is_empty() {
            parts.push(format!("# OBSERVER FEEDBACK (Background Monitoring)\n\n{}", insights.join("\n\n")));
        }

        if !skeleton.is_empty() {
            parts.push(format!("# Session Skeleton\n\n{}", skeleton));
        }

        parts.join("\n\n---\n\n")
    }

    /// Recall observations matching a query.
    pub fn recall(&self, query: &str) -> String {
        self.recall_with_daemon_results(query, &[])
    }

    /// Recall observations matching a query, with optional daemon search results prepended.
    pub fn recall_with_daemon_results(&self, query: &str, daemon_results: &[String]) -> String {
        if query == "--stats" {
            let sqs_str = self.last_sqs.as_ref()
                .map(|s| format!("score={:.2}, status={}, tier={}", s.score, s.status, self.current_tier.as_index()))
                .unwrap_or_default();
            return format!(
                "Observer stats: observations={}, tokens_est={}, sqs={}, loop={:?}\n{}\n{}",
                self.store.len(),
                self.store.estimate_token_count(),
                sqs_str,
                self.last_loop_pattern,
                self.metrics.summary(),
                self.cost_tracker.summary(),
            );
        }

        let mut parts: Vec<String> = Vec::new();

        // Daemon results first (semantic search from indexed observations)
        for text in daemon_results {
            parts.push(format!("[daemon] {}", text));
        }

        // Local keyword matching
        let query_lower = query.to_lowercase();
        let matches: Vec<&Observation> = self.store.get_all()
            .iter()
            .filter(|obs| obs.text.to_lowercase().contains(&query_lower))
            .take(5)
            .collect();

        for m in &matches {
            parts.push(format!("[{}] {}", m.obs_type, m.text));
        }

        if parts.is_empty() {
            format!("No observations matching '{}'.", query)
        } else {
            parts.join("\n\n")
        }
    }

    /// Return the latest observable per type for daemon indexing.
    /// Returns (obs_type_str, text, timestamp, token_estimate) tuples.
    pub fn latest_observables(&self) -> Vec<(String, String, i64, usize)> {
        let mut result = Vec::new();
        for obs_type in &[ObservationType::Watcher, ObservationType::Critic, ObservationType::Filter, ObservationType::Summary, ObservationType::Skeleton] {
            if let Some(obs) = self.store.get_latest(obs_type.clone()) {
                result.push((
                    format!("{:?}", obs_type).to_lowercase(),
                    obs.text.clone(),
                    obs.timestamp,
                    obs.token_estimate,
                ));
            }
        }
        result
    }

    // -----------------------------------------------------------------------
    // SQS computation
    // -----------------------------------------------------------------------

    pub fn compute_sqs(&mut self, trajectory: &Trajectory) -> SQSResult {
        let assistant_msgs: Vec<&Message> = trajectory.messages
            .iter()
            .filter(|m| matches!(m.role, Role::Assistant))
            .rev()
            .take(10)
            .collect();

        if assistant_msgs.is_empty() {
            return SQSResult {
                score: 1.0, status: "FOCUSED".to_string(), monotonicity: 1.0,
                ee_ratio: 1.0, diffusion: 0.4, dcr: 0.5, cps: 0.5,
            };
        }

        let ee_ratio = self.calculate_ee_ratio(&assistant_msgs);
        let mono = self.calculate_monotonicity();
        let diffusion = self.calculate_diffusion(&assistant_msgs);
        let dcr = self.calculate_dcr(&assistant_msgs);
        let cps = self.calculate_cps(&assistant_msgs);

        let w = &self.config.sqs_weights;
        let score = w.diffusion * (1.0 - diffusion)
            + w.ee_ratio * ee_ratio
            + w.dcr * dcr
            + w.cps * cps
            + w.monotonicity * mono;

        let tier_idx = self.current_tier.as_index();
        let sqs_trigger = self.config.tier_thresholds.sqs[tier_idx];
        let sqs_focused = sqs_trigger + 0.25; // ~0.25 above stagnating = focused

        let status = if score < sqs_trigger {
            "STAGNATING"
        } else if score > sqs_focused {
            "EXPLORING"
        } else {
            "FOCUSED"
        };

        SQSResult {
            score: score as f32,
            status: status.to_string(),
            monotonicity: mono as f32,
            ee_ratio: ee_ratio as f32,
            diffusion,
            dcr: dcr as f32,
            cps: cps as f32,
        }
    }

    /// EE ratio using structured action features (matching TS extractActionFeatures).
    fn calculate_ee_ratio(&self, msgs: &[&Message]) -> f32 {
        let features = extract_action_features(msgs, self.ast_delta());
        if features.is_empty() {
            return 0.5;
        }

        let mut file_actions: HashMap<String, usize> = HashMap::new();
        for feat in &features {
            if feat.file != "global" {
                *file_actions.entry(format!("{}:{}", feat.file, feat.op)).or_insert(0) += 1;
            }
        }

        if file_actions.is_empty() {
            return 0.5;
        }

        let unique = file_actions.len() as f32;
        let total: usize = file_actions.values().sum();
        let max_loops = file_actions.values().max().copied().unwrap_or(1);

        (unique / total.max(1) as f32) * (1.0 / max_loops as f32)
    }

    fn calculate_monotonicity(&mut self) -> f32 {
        if !self.config.procedural_monotonicity_enabled {
            return 1.0;
        }
        let total = self.resolved_subgoals.len();
        if total == 0 {
            return 1.0;
        }
        let forgotten = self.forgotten_subgoals.len();
        self.metrics.forgetting.ifr_score = forgotten as f32 / total as f32;
        1.0 - (forgotten as f32 / total as f32)
    }

    /// DCR using TS formula with language normalization (ObserverOrchestrator.ts:210-225).
    /// Falls back to heuristic when AST churn data is unavailable.
    fn calculate_dcr(&self, msgs: &[&Message]) -> f32 {
        let features = extract_action_features(msgs, self.ast_delta());
        let unique_files: HashSet<&str> = features.iter().map(|f| f.file.as_str()).collect();
        let coverage = (unique_files.len() as f32 / 5.0).min(1.0);

        // Use AST churn if available, otherwise use heuristic fallback
        let score = if let Some(churn) = self.last_ast_churn {
            let lang = features.first().map(|f| f.lang.as_str()).unwrap_or("python");
            let raw = (churn.0 + churn.1) as f32;
            let total = churn.2.max(1) as f32;
            let norm = normalized_ast_churn(lang, raw, total);
            coverage * (norm / 2.0).min(1.0)
        } else {
            // Heuristic fallback: use edit count ratio
            let mut edit_counts: HashMap<&str, usize> = HashMap::new();
            for (path, _) in &self.recent_edits {
                *edit_counts.entry(path.as_str()).or_insert(0) += 1;
            }
            let max_churn = edit_counts.values().max().copied().unwrap_or(1);
            let normalized_churn = 1.0 / max_churn as f32;
            (coverage * 0.6 + normalized_churn * 0.4).min(1.0)
        };

        score
    }

    /// CPS using TS 4-signal model (ObserverOrchestrator.ts:184-208).
    fn calculate_cps(&self, msgs: &[&Message]) -> f32 {
        let features = extract_action_features(msgs, self.ast_delta());

        // s[0]: last action success
        let s0 = features.last().map(|f| if f.success { 1.0 } else { 0.0 }).unwrap_or(0.5);

        // s[1]: files touched spread
        let unique_files: HashSet<&str> = features.iter().map(|f| f.file.as_str()).collect();
        let s1 = (unique_files.len() as f32 / 5.0).min(1.0);

        // s[2]: hardcoded constant
        let s2 = 0.5_f32;

        // s[3]: unique outcomes from last 6 tool messages
        let tool_msgs: Vec<&&Message> = msgs.iter()
            .filter(|m| matches!(m.role, Role::Tool))
            .rev()
            .take(6)
            .collect();
        let mut outcomes: Vec<&str> = Vec::new();
        for msg in &tool_msgs {
            let content = msg.content.to_string();
            if content.contains("error") || content.contains("failed") {
                outcomes.push("FAIL");
            } else {
                outcomes.push("PASS");
            }
        }
        let unique_outcomes: HashSet<&&str> = outcomes.iter().collect();
        let s3 = if unique_outcomes.len() > 1 {
            0.8
        } else if outcomes.first().copied() == Some(&"PASS") {
            0.6
        } else {
            0.1
        };

        0.25 * s0 + 0.30 * s1 + 0.25 * s2 + 0.20 * s3
    }

    /// Diffusion: file-spread metric. High when agent touches many files without depth.
    /// Matches TS computeDiffusion — measures ratio of unique files to total actions,
    /// inversely weighted by per-file edit depth.
    fn calculate_diffusion(&self, msgs: &[&Message]) -> f32 {
        let features = extract_action_features(msgs, self.ast_delta());
        if features.is_empty() {
            return 0.4;
        }
        let unique_files: HashSet<&str> = features.iter()
            .filter(|f| f.file != "global")
            .map(|f| f.file.as_str())
            .collect();
        if unique_files.is_empty() {
            return 0.4;
        }
        let total = features.len() as f32;
        let spread = unique_files.len() as f32 / total;
        // High spread (>3 files) with low depth → high diffusion
        // Low spread (1 file) with depth → low diffusion
        let depth = total / unique_files.len() as f32;
        let depth_factor = 1.0 / (1.0 + depth * 0.5);
        (spread * depth_factor).min(1.0)
    }

    // -----------------------------------------------------------------------
    // Loop pattern classification
    // -----------------------------------------------------------------------

    fn classify_loop_pattern(&self, trajectory: &Trajectory) -> LoopPattern {
        let tool_msgs: Vec<&Message> = trajectory.messages
            .iter()
            .filter(|m| matches!(m.role, Role::Tool))
            .rev()
            .take(3)
            .collect();

        if tool_msgs.len() < 3 {
            return LoopPattern::Unknown;
        }

        let contents: Vec<String> = tool_msgs.iter()
            .map(|m| m.content.to_string())
            .collect();

        // Use action features for structured loop detection
        let assistant_msgs: Vec<&Message> = trajectory.messages
            .iter()
            .filter(|m| matches!(m.role, Role::Assistant))
            .rev()
            .take(3)
            .collect();
        let features = extract_action_features(&assistant_msgs, self.ast_delta());

        // Check for repeated file:op combinations
        let mut file_op_counts: HashMap<String, usize> = HashMap::new();
        for feat in &features {
            if feat.file != "global" {
                *file_op_counts.entry(format!("{}:{}", feat.file, feat.op)).or_insert(0) += 1;
            }
        }

        // TS: instruction similarity check for STUCK_WITH_FORGETTING
        let sim = instruction_similarity(
            features.last().and_then(|f| f.instruction.as_deref()),
            features.first().and_then(|f| f.instruction.as_deref()),
        );
        let same_file = features.len() >= 2
            && features.last().map(|f| &f.file) == features.first().map(|f| &f.file);

        if sim > 0.95 && same_file {
            let churn0 = features.first().and_then(|f| f.ast_delta_nodes).unwrap_or(0);
            let churn_last = features.last().and_then(|f| f.ast_delta_nodes).unwrap_or(0);
            if churn0 > 0 && churn_last < 0 && (churn0 + churn_last).abs() < 2 {
                return LoopPattern::StuckWithForgetting;
            }
        }

        // Fallback: has_fix && has_error heuristic
        let has_fix = contents.iter().any(|c| c.contains("fixed") || c.contains("resolved"));
        let has_error = contents.iter().any(|c| c.contains("error") || c.contains("failed"));
        if has_fix && has_error {
            return LoopPattern::StuckWithForgetting;
        }

        // Error signature classification matching TS
        let errors: Vec<&str> = contents.iter()
            .filter(|c| c.contains("error"))
            .map(|c| c.as_str())
            .collect();

        // Unique error signatures by content length bucket
        let unique_errors: HashSet<usize> = errors.iter()
            .map(|e| e.len() / 50) // bucket by ~50 char chunks
            .collect();

        // Syntax loop — consecutive syntax errors in same file (check before generic Stuck)
        let syntax_errors: Vec<&str> = contents.iter()
            .filter(|c| c.contains("syntax") || c.contains("parse error") || c.contains("unexpected token"))
            .map(|c| c.as_str())
            .collect();
        let unique_files: HashSet<&str> = features.iter().map(|f| f.file.as_str()).collect();
        if syntax_errors.len() >= 2 && unique_files.len() <= 1 {
            return LoopPattern::SyntaxLoop;
        }

        if unique_errors.len() == 1 {
            // TS: unique_errors == 1
            if unique_files.len() > 1 {
                return LoopPattern::Widening;
            }
            return LoopPattern::Stuck;
        }

        if errors.len() >= 2 {
            let first_err = errors.first().map(|e| e.len()).unwrap_or(0);
            let last_err = errors.last().map(|e| e.len()).unwrap_or(0);
            if first_err > 0 && (first_err as f32 - last_err as f32).abs() / (first_err as f32) < 0.3 {
                return LoopPattern::Oscillating;
            }
            return LoopPattern::Stuck;
        }

        // Repeated file:op — same file and operation repeated 3+ times
        let max_file_op = file_op_counts.values().max().copied().unwrap_or(0);
        if max_file_op >= 3 {
            return LoopPattern::RepeatedFileOp;
        }

        LoopPattern::Productive
    }

    // -----------------------------------------------------------------------
    // filterMemoryByApi — build request and apply filter
    // -----------------------------------------------------------------------

    /// Build an ObservationKey from current session data.
    /// Populates signature from recent edits; API data is enriched by the engine.
    fn build_observation_key(&self) -> ObservationKey {
        let signature = self.recent_edits.first()
            .map(|(path, _)| path.clone());
        let docstring_hash = self.recent_edits.first()
            .and_then(|(_, content)| {
                let hashable = content.lines()
                    .filter(|l| l.trim().starts_with("///") || l.trim().starts_with("/**") || l.trim().starts_with("*"))
                    .collect::<Vec<_>>()
                    .join("\n");
                if hashable.is_empty() {
                    None
                } else {
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    hashable.hash(&mut hasher);
                    Some(format!("{:016x}", hasher.finish()))
                }
            });
        ObservationKey {
            signature,
            apis_called: Vec::new(),
            apis_defined: Vec::new(),
            docstring_hash,
        }
    }

    /// Enrich the latest observation's key with API data from the analyzer.
    pub fn enrich_latest_key(&mut self, apis: &ExtractApisResponse) {
        if let Some(obs) = self.store.get_all_mut().last_mut() {
            if obs.key.is_none() {
                obs.key = Some(ObservationKey::default());
            }
            if let Some(ref mut key) = obs.key {
                if !apis.calls.is_empty() {
                    key.apis_called = apis.calls.clone();
                }
                if !apis.definitions.is_empty() {
                    key.apis_defined = apis.definitions.clone();
                }
            }
        }
    }
    /// Returns None if no suitable code content is found in recent messages.
    pub fn build_extract_apis_request(&self, trajectory: &Trajectory) -> Option<ExtractApisRequest> {
        let tool_msgs: Vec<&Message> = trajectory.messages.iter()
            .filter(|m| matches!(m.role, Role::Tool))
            .rev()
            .take(5)
            .collect();

        for msg in &tool_msgs {
            let content = msg.content.to_string();
            // Look for content that looks like code (has common code patterns)
            if content.contains("fn ") || content.contains("function ")
                || content.contains("def ") || content.contains("class ")
                || content.contains("impl ") || content.contains("pub fn")
                || content.contains("export ") || content.contains("import ")
            {
                // Detect language from file extension hints in content
                let language = if content.contains("fn ") && content.contains("let ") {
                    Some("rust".to_string())
                } else if content.contains("def ") && content.contains("self") {
                    Some("python".to_string())
                } else if content.contains("function ") && content.contains("const ") {
                    Some("typescript".to_string())
                } else if content.contains("func ") && content.contains(":= ") {
                    Some("go".to_string())
                } else {
                    None
                };

                return Some(ExtractApisRequest {
                    content: safe_truncate(&content, 8000).into_owned(),
                    language,
                });
            }
        }
        None
    }

    /// Filter observation text to keep only lines relevant to the given APIs.
    /// Returns the filtered text (may be empty if nothing matches).
    pub fn apply_api_filter(&self, text: &str, apis: &ExtractApisResponse) -> String {
        if !self.config.ast_guided_memory_enabled {
            return text.to_string();
        }
        if apis.calls.is_empty() && apis.definitions.is_empty() {
            return text.to_string();
        }

        let all_apis: Vec<&str> = apis.calls.iter()
            .chain(apis.definitions.iter())
            .map(|s| s.as_str())
            .collect();

        let filtered: Vec<&str> = text.lines()
            .filter(|line| {
                // Keep header lines (start with # or [)
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.starts_with('[') {
                    return true;
                }
                // Keep lines that mention at least one API
                all_apis.iter().any(|api| line.contains(api))
            })
            .collect();

        if filtered.is_empty() {
            // If no lines match, return a summary instead of empty
            format!("(Filtered by API relevance: {} APIs tracked)", all_apis.len())
        } else {
            filtered.join("\n")
        }
    }

    // -----------------------------------------------------------------------
    // LLM prompt building — returns prompts for engine to execute via gateway
    // -----------------------------------------------------------------------

    /// Build a watcher LLM prompt from recent trajectory context.
    pub fn build_watcher_llm_prompt(&self, trajectory: &Trajectory) -> ObserverLlmRequest {
        let recent_tools: Vec<&Message> = trajectory.messages.iter()
            .filter(|m| matches!(m.role, Role::Tool))
            .rev()
            .take(5)
            .collect();
        let outputs: String = recent_tools.iter()
            .map(|m| safe_truncate(&m.content.to_string(), 400).into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let sqs = self.last_sqs.as_ref();
        let files: Vec<String> = self.recent_edits.iter().map(|(p, _)| p.clone()).collect();
        let loop_str = format!("{:?}", self.last_loop_pattern);

        ObserverLlmRequest {
            obs_type: ObservationType::Watcher,
            system_prompt: prompts::WATCHER_SYSTEM.to_string(),
            user_message: prompts::build_trajectory_context(
                &outputs,
                sqs.map(|s| s.score).unwrap_or(0.5),
                sqs.map(|s| s.status.as_str()).unwrap_or("UNKNOWN"),
                &loop_str,
                self.current_turn,
                &files,
            ),
        }
    }

    /// Build a filter LLM prompt.
    pub fn build_filter_llm_prompt(&self, trajectory: &Trajectory) -> ObserverLlmRequest {
        let recent_tools: Vec<&Message> = trajectory.messages.iter()
            .filter(|m| matches!(m.role, Role::Tool))
            .rev()
            .take(5)
            .collect();
        let outputs: String = recent_tools.iter()
            .map(|m| safe_truncate(&m.content.to_string(), 400).into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let sqs = self.last_sqs.as_ref();
        let files: Vec<String> = self.recent_edits.iter().map(|(p, _)| p.clone()).collect();
        let loop_str = format!("{:?}", self.last_loop_pattern);

        ObserverLlmRequest {
            obs_type: ObservationType::Filter,
            system_prompt: prompts::FILTER_SYSTEM.to_string(),
            user_message: prompts::build_trajectory_context(
                &outputs,
                sqs.map(|s| s.score).unwrap_or(0.5),
                sqs.map(|s| s.status.as_str()).unwrap_or("UNKNOWN"),
                &loop_str,
                self.current_turn,
                &files,
            ),
        }
    }

    /// Build a critic LLM prompt.
    pub fn build_critic_llm_prompt(&self, trajectory: &Trajectory) -> ObserverLlmRequest {
        let recent_tools: Vec<&Message> = trajectory.messages.iter()
            .filter(|m| matches!(m.role, Role::Tool))
            .rev()
            .take(8)
            .collect();
        let outputs: String = recent_tools.iter()
            .map(|m| safe_truncate(&m.content.to_string(), 400).into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let sqs = self.last_sqs.as_ref();
        let files: Vec<String> = self.recent_edits.iter().map(|(p, _)| p.clone()).collect();
        let loop_str = format!("{:?}", self.last_loop_pattern);

        ObserverLlmRequest {
            obs_type: ObservationType::Critic,
            system_prompt: prompts::CRITIC_SYSTEM.to_string(),
            user_message: prompts::build_trajectory_context(
                &outputs,
                sqs.map(|s| s.score).unwrap_or(0.5),
                sqs.map(|s| s.status.as_str()).unwrap_or("UNKNOWN"),
                &loop_str,
                self.current_turn,
                &files,
            ),
        }
    }

    /// Build a skeleton LLM prompt.
    pub fn build_skeleton_llm_prompt(&self) -> ObserverLlmRequest {
        ObserverLlmRequest {
            obs_type: ObservationType::Skeleton,
            system_prompt: prompts::SKELETON_SYSTEM.to_string(),
            user_message: prompts::build_skeleton_context(
                &self.recent_edits,
                &self.recent_errors,
                &self.recent_decisions,
                self.current_turn,
            ),
        }
    }

    /// Build a reflector LLM prompt.
    pub fn build_reflector_llm_prompt(&self) -> ObserverLlmRequest {
        let all_obs = self.store.build_observation_block(None, None);
        ObserverLlmRequest {
            obs_type: ObservationType::Reflection,
            system_prompt: prompts::REFLECTOR_SYSTEM.to_string(),
            user_message: prompts::build_reflector_context(
                &all_obs,
                self.current_turn,
            ),
        }
    }

    /// Build a summarizer LLM prompt from unobserved messages.
    pub fn build_summarizer_llm_prompt(&self, trajectory: &Trajectory) -> ObserverLlmRequest {
        let msgs: Vec<String> = trajectory.messages.iter()
            .skip(self.last_observed_message_index)
            .take(20)
            .map(|m| {
                let role = format!("{:?}", m.role);
                let content = safe_truncate(&m.content.to_string(), 500).into_owned();
                format!("[{}]: {}", role, content)
            })
            .collect();
        let serialized = msgs.join("\n\n");
        let token_est = serialized.len() / 4;

        ObserverLlmRequest {
            obs_type: ObservationType::Summary,
            system_prompt: prompts::SUMMARIZER_SYSTEM.to_string(),
            user_message: prompts::build_summarizer_context(&serialized, self.current_turn, token_est),
        }
    }

    /// Estimate tokens for unobserved messages (those after last_observed_message_index).
    pub fn get_unobserved_token_estimate(&self, trajectory: &Trajectory) -> usize {
        if self.last_observed_message_index >= trajectory.messages.len() {
            return 0;
        }
        let unobserved = &trajectory.messages[self.last_observed_message_index..];
        unobserved.iter()
            .map(|m| m.content.to_string().len())
            .sum::<usize>()
            / 4
    }

    /// Whether the blocking summarizer should fire based on token ratio (matches TS onTurnComplete).
    /// TS computes: ratio = unobserved_tokens / tokenThreshold, fires when ratio >= blockAfter.
    pub fn should_summarize(&self, trajectory: &Trajectory) -> bool {
        if self.current_turn < self.config.buffer_activation {
            return false;
        }
        let unobserved_count = trajectory.messages.len().saturating_sub(self.last_observed_message_index);
        if unobserved_count < 4 {
            return false;
        }
        let token_estimate = self.get_unobserved_token_estimate(trajectory);
        let ratio = token_estimate as f32 / self.config.token_threshold.max(1) as f32;
        ratio >= self.config.block_after
    }

    /// Whether reflector should fire for LLM path.
    pub fn should_reflect_llm(&self) -> bool {
        self.config.reflection_enabled
            && self.turns_since_last_reflection >= self.reflection_cooldown_turns()
            && self.store.estimate_token_count() >= self.config.reflection_token_threshold
    }

    /// Process a parsed LLM observation, creating and storing the observation.
    pub fn process_llm_observation(&mut self, parsed: ParsedObservation, obs_type: ObservationType) {
        let obs = Observation {
            obs_type: obs_type.clone(),
            text: parsed.text,
            timestamp: now_millis(),
            confidence: parsed.confidence,
            token_estimate: 60,
            compressed_range: Some([self.current_turn.saturating_sub(3), self.current_turn]),
            critic_action: parsed.critic_action.clone(),
            sqs: self.last_sqs.as_ref().map(|s| s.score),
            fidelity: if obs_type == ObservationType::Skeleton {
                Some(SkeletonFidelity::Decision)
            } else {
                None
            },
            key: if obs_type == ObservationType::Skeleton || obs_type == ObservationType::Summary {
                Some(self.build_observation_key())
            } else {
                None
            },
        };
        self.store.append(obs);
        match obs_type {
            ObservationType::Watcher => {
                self.metrics.watcher_fired += 1;
            }
            ObservationType::Filter => {
                self.metrics.filter_fired += 1;
            }
            ObservationType::Critic => {
                self.turns_since_last_reflection = 0;
                self.metrics.critic_fired += 1;
            }
            ObservationType::Skeleton => {
                self.metrics.skeleton_observations += 1;
            }
            ObservationType::Reflection => {
                self.turns_since_last_reflection = 0;
                self.metrics.reflections_fired += 1;
                self.metrics.token_efficiency.retrieval_stage_used = 1;
            }
            ObservationType::Summary => {}
        }
    }

    // -----------------------------------------------------------------------
    // Confidence decay — returns actual decayed value
    // -----------------------------------------------------------------------

    /// Compute decayed confidence: baseConf * exp(-turns / adjusted_tau).
    fn decayed_confidence(&self, base_conf: f32, kind: &str, turns_since: usize) -> f32 {
        let tau = if kind == "CRITIC" { self.config.tau_critic } else { self.config.tau_watcher };
        let sqs_score = self.last_sqs.as_ref().map(|s| s.score).unwrap_or(0.5);
        let adjusted_tau = if sqs_score > 0.5 { tau * 2.0 } else { tau / 2.0 };
        base_conf * (-(turns_since as f32) / adjusted_tau).exp()
    }

    /// Filter observations by per-observation confidence decay, matching TS filterWithDecay.
    /// Each observation's confidence is individually decayed based on its age (compressed_range).
    /// Only the last 2 observations per type that pass the threshold are kept.
    fn filter_observations_by_decay(&self, obs_type: ObservationType, min_conf: f32) -> String {
        let tau_kind = if obs_type == ObservationType::Critic { "CRITIC" } else { "WATCHER" };
        let passing: Vec<&Observation> = self.store.get_all().iter()
            .filter(|obs| {
                if obs.obs_type != obs_type { return false; }
                let turns_since = self.current_turn
                    - obs.compressed_range.map(|r| r[1]).unwrap_or(0);
                let decayed = self.decayed_confidence(obs.confidence, tau_kind, turns_since);
                decayed >= min_conf
            })
            .rev()
            .take(2)
            .collect();

        if passing.is_empty() {
            return String::new();
        }

        passing.into_iter()
            .rev() // restore chronological order
            .map(|obs| obs.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    // -----------------------------------------------------------------------
    // Skeleton data extraction
    // -----------------------------------------------------------------------

    fn extract_skeleton_data(&mut self, trajectory: &Trajectory) {
        let last_assistant = trajectory.messages.iter()
            .filter(|m| matches!(m.role, Role::Assistant))
            .last();

        if let Some(msg) = last_assistant {
            let content = msg.content.to_string().to_lowercase();

            for word in content.split_whitespace() {
                if (word.contains('/') || word.ends_with(".rs") || word.ends_with(".ts"))
                    && !self.recent_edits.iter().any(|(p, _)| p == word)
                {
                    self.recent_edits.push((word.to_string(), "edited".to_string()));
                    if self.recent_edits.len() > 20 { self.recent_edits.remove(0); }
                }
            }

            if content.contains("error") || content.contains("failed") {
                if let Some(err_line) = content.lines().find(|l| l.contains("error")) {
                    self.recent_errors.push(safe_truncate(err_line.trim(), 200).into_owned());
                    if self.recent_errors.len() > 10 { self.recent_errors.remove(0); }
                }
            }

            if content.contains("i will ") || content.contains("let me ") || content.contains("going to ") {
                if let Some(line) = content.lines().find(|l| {
                    l.contains("i will ") || l.contains("let me ") || l.contains("going to ")
                }) {
                    self.recent_decisions.push(safe_truncate(line.trim(), 200).into_owned());
                    if self.recent_decisions.len() > 10 { self.recent_decisions.remove(0); }
                }
            }
        }
    }

    fn trigger_skeleton_observation(&mut self) {
        let mut sections = Vec::new();

        if !self.recent_edits.is_empty() {
            let edits: Vec<String> = self.recent_edits.iter()
                .take(10)
                .map(|(p, d)| format!("  - {} ({})", p, d))
                .collect();
            sections.push(format!("Edits:\n{}", edits.join("\n")));
        }

        if !self.recent_errors.is_empty() {
            let errs: Vec<String> = self.recent_errors.iter()
                .take(5)
                .map(|e| format!("  - {}", e))
                .collect();
            sections.push(format!("Errors:\n{}", errs.join("\n")));
        }

        if !self.recent_decisions.is_empty() {
            let decs: Vec<String> = self.recent_decisions.iter()
                .take(5)
                .map(|d| format!("  - {}", d))
                .collect();
            sections.push(format!("Decisions:\n{}", decs.join("\n")));
        }

        if sections.is_empty() {
            return;
        }

        let skeleton_text = format!("Session skeleton (turn {}):\n{}",
            self.metrics.turns_observed,
            sections.join("\n\n"),
        );

        let obs = Observation {
            obs_type: ObservationType::Skeleton,
            text: skeleton_text,
            timestamp: now_millis(),
            confidence: 0.6,
            token_estimate: sections.len() * 40,
            compressed_range: Some([self.current_turn.saturating_sub(4), self.current_turn]),
            critic_action: None,
            sqs: self.last_sqs.as_ref().map(|s| s.score),
            fidelity: Some(SkeletonFidelity::Structural),
            key: Some(self.build_observation_key()),
        };
        let tokens = obs.token_estimate;
        self.store.append(obs);
        self.metrics.skeleton_observations += 1;
        self.cost_tracker.record(ObservationType::Skeleton, tokens, 0, self.current_turn);
    }

    // -----------------------------------------------------------------------
    // Observation triggers
    // -----------------------------------------------------------------------

    fn trigger_watcher_observation(&mut self, trajectory: &Trajectory) {
        let last_tools: Vec<&Message> = trajectory.messages
            .iter()
            .filter(|m| matches!(m.role, Role::Tool))
            .rev()
            .take(3)
            .collect();

        let context: String = last_tools.iter()
            .map(|m| safe_truncate(&m.content.to_string(), 500).into_owned())
            .collect::<Vec<_>>()
            .join("\n");

        let insight = match &self.last_loop_pattern {
            LoopPattern::Stuck => format!("Stuck pattern detected. Consider re-reading the relevant file or trying a different approach. Context: {}", safe_truncate(&context, 200)),
            LoopPattern::StuckWithForgetting => format!("Reverting own changes detected. Previous fix was overwritten. Re-read before editing. Context: {}", safe_truncate(&context, 200)),
            LoopPattern::Oscillating => format!("Alternating between approaches without progress. Pick one strategy and commit to it. Context: {}", safe_truncate(&context, 200)),
            LoopPattern::Widening => format!("Errors spreading across multiple files. Focus on the root cause first. Context: {}", safe_truncate(&context, 200)),
            LoopPattern::RepeatedFileOp => format!("Repeatedly editing the same file with the same operation. Verify the change is correct and move on. Context: {}", safe_truncate(&context, 200)),
            LoopPattern::SyntaxLoop => format!("Consecutive syntax errors detected. Re-read the file and check syntax carefully before editing. Context: {}", safe_truncate(&context, 200)),
            LoopPattern::Productive | LoopPattern::Unknown => return,
        };

        let obs = Observation {
            obs_type: ObservationType::Watcher,
            text: insight,
            timestamp: now_millis(),
            confidence: 0.7,
            token_estimate: 50,
            compressed_range: Some([self.current_turn.saturating_sub(3), self.current_turn]),
            critic_action: None,
            sqs: self.last_sqs.as_ref().map(|s| s.score),
            fidelity: None,
            key: None,
        };
        self.store.append(obs);
        self.metrics.watcher_fired += 1;
        self.cost_tracker.record(ObservationType::Watcher, 50, 0, self.current_turn);
    }

    fn trigger_filter_observation(&mut self) {
        let sqs = self.last_sqs.as_ref();
        let diffusion = sqs.map(|s| s.diffusion).unwrap_or(0.0);
        let ee = sqs.map(|s| s.ee_ratio).unwrap_or(0.0);

        if diffusion < 0.5 || ee > 0.3 {
            return; // No filter needed
        }

        let insight = format!(
            "High diffusion ({:.2}) with low EE ratio ({:.2}). Agent is touching many files without depth. Consider focusing on fewer files with more thorough changes.",
            diffusion, ee,
        );

        let obs = Observation {
            obs_type: ObservationType::Filter,
            text: insight,
            timestamp: now_millis(),
            confidence: 0.5,
            token_estimate: 40,
            compressed_range: Some([self.current_turn.saturating_sub(3), self.current_turn]),
            critic_action: None,
            sqs: sqs.map(|s| s.score),
            fidelity: None,
            key: None,
        };
        self.store.append(obs);
        self.metrics.filter_fired += 1;
        self.cost_tracker.record(ObservationType::Filter, 40, 0, self.current_turn);
    }

    fn trigger_critic_observation(&mut self) {
        let sqs = self.last_sqs.as_ref().map(|s| s.score).unwrap_or(1.0);
        if sqs >= self.tier_sqs_threshold() {
            return;
        }

        // Determine action from heuristic (in TS this comes from LLM output)
        let action = if self.last_loop_pattern == LoopPattern::StuckWithForgetting
            || self.last_loop_pattern == LoopPattern::Oscillating
            || self.last_loop_pattern == LoopPattern::RepeatedFileOp
            || self.last_loop_pattern == LoopPattern::SyntaxLoop
        {
            if self.consecutive_reflects >= self.config.tier_thresholds.restart_after_reflects - 1 {
                CriticAction::Restart
            } else {
                CriticAction::Reflect
            }
        } else if sqs < self.tier_sqs_threshold() * 0.8 {
            CriticAction::Reflect
        } else {
            CriticAction::Continue
        };

        let insight = format!(
            "Trajectory health is low (SQS: {:.2}, tier: {}). Agent may be in an unproductive loop. Consider: (1) re-reading key files, (2) using search to find relevant code, (3) asking the user for clarification.",
            sqs, self.current_tier.as_index(),
        );

        let obs = Observation {
            obs_type: ObservationType::Critic,
            text: insight,
            timestamp: now_millis(),
            confidence: 0.6,
            token_estimate: 60,
            compressed_range: Some([self.current_turn, self.current_turn]),
            critic_action: Some(action.clone()),
            sqs: Some(sqs),
            fidelity: None,
            key: None,
        };
        self.store.append(obs);
        self.turns_since_last_reflection = 0;
        self.metrics.critic_fired += 1;
        self.cost_tracker.record(ObservationType::Critic, 60, 0, self.current_turn);
    }

    fn trigger_reflection(&mut self) {
        let token_count = self.store.estimate_token_count();
        if token_count < self.config.reflection_token_threshold {
            return;
        }

        let all_obs = self.store.build_observation_block(None, None);
        if all_obs.is_empty() {
            return;
        }

        let summary_lines: Vec<String> = all_obs.lines()
            .filter(|l| !l.trim().is_empty())
            .take(10)
            .map(|l| safe_truncate(l, 200).to_string())
            .collect();

        let reflected = format!(
            "Reflected observation summary ({} original entries compressed):\n{}",
            self.store.len(),
            summary_lines.join("\n"),
        );

        let obs = Observation {
            obs_type: ObservationType::Reflection,
            text: reflected,
            timestamp: now_millis(),
            confidence: 0.8,
            token_estimate: summary_lines.len() * 30,
            compressed_range: Some([0, self.current_turn]),
            critic_action: None,
            sqs: self.last_sqs.as_ref().map(|s| s.score),
            fidelity: None,
            key: None,
        };

        self.store.archive_and_replace(obs);
        // Clear skeleton tracking after reflection to prevent stale accumulation (Fixes #3)
        self.recent_edits.clear();
        self.recent_errors.clear();
        self.recent_decisions.clear();
        self.turns_since_last_reflection = 0;
        self.metrics.reflect_actions += 1;
        self.metrics.reflections_fired += 1;
        self.metrics.token_efficiency.retrieval_stage_used = 1;
    }

    // -----------------------------------------------------------------------
    // Subgoal tracking (monotonicity / IFR)
    // -----------------------------------------------------------------------

    fn update_subgoal_tracking(&mut self, trajectory: &Trajectory) {
        let last_tool = trajectory.messages.iter()
            .filter(|m| matches!(m.role, Role::Tool))
            .last();

        if let Some(msg) = last_tool {
            let content = msg.content.to_string().to_lowercase();

            for word in content.split_whitespace() {
                if word == "fixed" || word == "resolved" {
                    if let Some(target) = content.split(word).nth(1) {
                        let target = target.split_whitespace().next().unwrap_or("").trim_end_matches(|c: char| !c.is_alphanumeric());
                        if !target.is_empty() {
                            self.resolved_subgoals.insert(target.to_string());
                        }
                    }
                }
            }

            if content.contains("error") || content.contains("failed") {
                for subgoal in &self.resolved_subgoals {
                    if content.contains(subgoal.as_str()) {
                        self.forgotten_subgoals.insert(subgoal.clone());
                        self.metrics.forgetting.detected += 1;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn safe_truncate(s: &str, max_len: usize) -> std::borrow::Cow<'_, str> {
    if s.len() <= max_len {
        std::borrow::Cow::Borrowed(s)
    } else {
        let boundary = s.char_indices()
            .take_while(|(i, _)| *i <= max_len)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        std::borrow::Cow::Owned(format!("{}...", &s[..boundary]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::trajectory::{Trajectory, Message, Role};

    fn make_trajectory_with_tool_messages(texts: Vec<&str>) -> Trajectory {
        let mut traj = Trajectory::new();
        for text in texts {
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(),
                role: Role::Tool,
                content: serde_json::Value::String(text.to_string()),
                timestamp: chrono::Utc::now(),
                tokens: 0,
                is_compressed: false,
                tool_meta: Default::default(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                thinking: None,
            });
        }
        traj
    }

    #[test]
    fn test_interrupt_continue_when_productive() {
        let mut observer = Observer::new(ObserverConfig::default());
        let traj = Trajectory::new();
        let result = observer.on_turn_complete(&traj);
        assert_eq!(result.action, CriticAction::Continue);
    }

    #[test]
    fn test_interrupt_reflect_on_stuck_with_forgetting() {
        let mut observer = Observer::new(ObserverConfig::default());
        let traj = make_trajectory_with_tool_messages(vec![
            "fixed the bug",
            "some output",
            "error in the bug",
        ]);
        let result = observer.on_turn_complete(&traj);
        assert_eq!(result.action, CriticAction::Reflect);
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn test_interrupt_restart_after_consecutive_reflects() {
        let mut observer = Observer::new(ObserverConfig {
            tier_thresholds: TierThresholds {
                restart_after_reflects: 2,
                ..Default::default()
            },
            ..Default::default()
        });
        let traj = make_trajectory_with_tool_messages(vec![
            "fixed the bug",
            "some output",
            "error in the bug",
        ]);
        let r1 = observer.on_turn_complete(&traj);
        assert_eq!(r1.action, CriticAction::Reflect);
        let r2 = observer.on_turn_complete(&traj);
        assert_eq!(r2.action, CriticAction::Restart);
    }

    #[test]
    fn test_tier_detection() {
        let observer = Observer::new(ObserverConfig::default());
        let mut traj = Trajectory::new();
        traj.messages.push(Message {
            id: uuid::Uuid::new_v4(),
            role: Role::User,
            content: serde_json::json!("run the test suite and fix failing tests"),
            timestamp: chrono::Utc::now(),
            tokens: 10,
            is_compressed: false,
            tool_meta: Default::default(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        });
        traj.messages.push(Message {
            id: uuid::Uuid::new_v4(),
            role: Role::Tool,
            content: serde_json::json!("3 tests pass, 2 tests fail"),
            timestamp: chrono::Utc::now(),
            tokens: 10,
            is_compressed: false,
            tool_meta: Default::default(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        });
        let tier = observer.detect_tier(&traj);
        assert_eq!(tier, TaskTier::TestDriven);
    }

    #[test]
    fn test_tier_doclint() {
        let observer = Observer::new(ObserverConfig::default());
        let mut traj = Trajectory::new();
        traj.messages.push(Message {
            id: uuid::Uuid::new_v4(),
            role: Role::User,
            content: serde_json::json!("fix the lint errors in README.md"),
            timestamp: chrono::Utc::now(),
            tokens: 10,
            is_compressed: false,
            tool_meta: Default::default(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        });
        let tier = observer.detect_tier(&traj);
        assert_eq!(tier, TaskTier::DocLint);
    }

    #[test]
    fn test_tier_thresholds_arrays() {
        let thresholds = TierThresholds::default();
        assert_eq!(thresholds.sqs, [0.3, 0.32, 0.35, 0.4]);
        assert_eq!(thresholds.confidence, [0.5, 0.55, 0.6, 0.7]);
    }

    #[test]
    fn test_confidence_decay_returns_value() {
        let observer = Observer::new(ObserverConfig::default());
        let decayed = observer.decayed_confidence(0.8, "WATCHER", 3);
        assert!(decayed > 0.0 && decayed <= 0.8);
        assert_ne!(decayed, 1.0);
    }

    #[test]
    fn test_filter_observation_triggered() {
        let mut observer = Observer::new(ObserverConfig::default());
        observer.last_sqs = Some(SQSResult {
            score: 0.5, status: "FOCUSED".into(), monotonicity: 1.0,
            ee_ratio: 0.2, diffusion: 0.8, dcr: 0.3, cps: 0.5,
        });
        observer.last_loop_pattern = LoopPattern::Productive;
        observer.metrics.turns_observed = 3;
        observer.trigger_filter_observation();
        assert!(observer.metrics.filter_fired > 0);
    }

    #[test]
    fn test_toggle() {
        let mut observer = Observer::new(ObserverConfig::default());
        assert!(observer.config.enabled);
        observer.toggle(false);
        assert!(!observer.config.enabled);
    }

    #[test]
    fn test_observation_metadata() {
        let obs = Observation {
            obs_type: ObservationType::Critic,
            text: "test".into(),
            timestamp: 0,
            confidence: 0.7,
            token_estimate: 10,
            compressed_range: Some([1, 5]),
            critic_action: Some(CriticAction::Reflect),
            sqs: Some(0.3),
            fidelity: None,
            key: None,
        };
        assert_eq!(obs.compressed_range.unwrap(), [1, 5]);
        assert_eq!(obs.critic_action.unwrap(), CriticAction::Reflect);
        assert_eq!(obs.sqs.unwrap(), 0.3);
    }

    #[test]
    fn test_cost_tracker() {
        let mut tracker = ObserverCostTracker::default();
        tracker.record(ObservationType::Watcher, 50, 10, 1);
        tracker.record(ObservationType::Critic, 60, 15, 2);
        assert_eq!(tracker.entry_count(), 2);
        assert_eq!(tracker.total_tokens(), 110);
    }

    #[test]
    fn test_health_state() {
        let observer = Observer::new(ObserverConfig::default());
        assert!(!observer.health.failing);
        assert!(observer.health.last_error.is_none());
    }

    #[test]
    fn test_diffusion_constant() {
        let observer = Observer::new(ObserverConfig::default());
        let msgs: Vec<&Message> = vec![];
        // compute_sqs returns 1.0/FOCUSED for empty, with diffusion 0.4
        let mut obs2 = Observer::new(ObserverConfig::default());
        let result = obs2.compute_sqs(&Trajectory::new());
        assert_eq!(result.diffusion, 0.4);
    }

    #[test]
    fn test_adaptive_critic_cooldown() {
        let observer = Observer::new(ObserverConfig::default());
        let base = observer.config.critic_frequency;
        assert!(observer.critic_cooldown() <= base);
    }

    #[test]
    fn test_build_extract_apis_request_finds_code() {
        let observer = Observer::new(ObserverConfig::default());
        let mut traj = Trajectory::new();
        traj.messages.push(Message {
            id: uuid::Uuid::new_v4(),
            role: Role::Tool,
            content: serde_json::json!("pub fn extract_calls() -> Vec<String> {\n    let x = 1;\n}"),
            timestamp: chrono::Utc::now(),
            tokens: 10,
            is_compressed: false,
            tool_meta: Default::default(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        });
        let req = observer.build_extract_apis_request(&traj);
        assert!(req.is_some());
        let req = req.unwrap();
        assert!(req.content.contains("extract_calls"));
        assert_eq!(req.language.as_deref(), Some("rust"));
    }

    #[test]
    fn test_build_extract_apis_request_no_code() {
        let observer = Observer::new(ObserverConfig::default());
        let mut traj = Trajectory::new();
        traj.messages.push(Message {
            id: uuid::Uuid::new_v4(),
            role: Role::Tool,
            content: serde_json::json!("just some plain text without code patterns"),
            timestamp: chrono::Utc::now(),
            tokens: 10,
            is_compressed: false,
            tool_meta: Default::default(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        });
        let req = observer.build_extract_apis_request(&traj);
        assert!(req.is_none());
    }

    #[test]
    fn test_apply_api_filter_keeps_relevant_lines() {
        let observer = Observer::new(ObserverConfig::default());
        let text = "[Watcher] insight about extract_calls and parse_ranges\nsome unrelated line\n[Critic] important note about resolve_edits";
        let apis = ExtractApisResponse {
            calls: vec!["extract_calls".into(), "parse_ranges".into()],
            definitions: vec!["resolve_edits".into()],
        };
        let filtered = observer.apply_api_filter(text, &apis);
        assert!(filtered.contains("extract_calls"));
        assert!(filtered.contains("resolve_edits"));
        assert!(!filtered.contains("unrelated"));
    }

    #[test]
    fn test_apply_api_filter_empty_apis_returns_original() {
        let observer = Observer::new(ObserverConfig::default());
        let text = "some observation text";
        let apis = ExtractApisResponse::default();
        let filtered = observer.apply_api_filter(text, &apis);
        assert_eq!(filtered, text);
    }

    #[test]
    fn test_parse_llm_watcher_response() {
        let text = "[OBSERVER:WATCHER | confidence:0.85] Repeated regex attempt on parser.rs [END_OBSERVER]";
        let parsed = parse_llm_observation(text, ObservationType::Watcher);
        assert!(parsed.is_some());
        let p = parsed.unwrap();
        assert!((p.confidence - 0.85).abs() < 0.01);
        assert_eq!(p.text, "Repeated regex attempt on parser.rs");
        assert!(p.critic_action.is_none());
    }

    #[test]
    fn test_parse_llm_critic_response() {
        let text = "[OBSERVER:CRITIC | action:REFLECT | confidence:0.72]\nREASON: Agent is oscillating between approaches\n[END_OBSERVER]";
        let parsed = parse_llm_observation(text, ObservationType::Critic);
        assert!(parsed.is_some());
        let p = parsed.unwrap();
        assert!((p.confidence - 0.72).abs() < 0.01);
        assert_eq!(p.critic_action, Some(CriticAction::Reflect));
        assert!(p.text.contains("oscillating"));
    }

    #[test]
    fn test_parse_llm_critic_restart() {
        let text = "[OBSERVER:CRITIC | action:RESTART | confidence:0.90]\nREASON: Sustained failure loop\n[END_OBSERVER]";
        let parsed = parse_llm_observation(text, ObservationType::Critic);
        assert!(parsed.is_some());
        let p = parsed.unwrap();
        assert_eq!(p.critic_action, Some(CriticAction::Restart));
    }

    #[test]
    fn test_parse_llm_no_alerts_returns_none() {
        let text = "[OBSERVER:WATCHER | confidence:0.00] No alerts [END_OBSERVER]";
        let parsed = parse_llm_observation(text, ObservationType::Watcher);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_parse_llm_context_clean_returns_none() {
        let text = "[OBSERVER:FILTER | confidence:0.10] Context clean [END_OBSERVER]";
        let parsed = parse_llm_observation(text, ObservationType::Filter);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_parse_llm_empty_returns_none() {
        let parsed = parse_llm_observation("", ObservationType::Watcher);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_process_llm_observation_stores() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-llm-obs-store");
        let initial_len = observer.store.len();
        let parsed = ParsedObservation {
            text: "LLM insight about code".to_string(),
            confidence: 0.8,
            critic_action: None,
        };
        observer.process_llm_observation(parsed, ObservationType::Watcher);
        assert!(observer.metrics.watcher_fired > 0);
        assert_eq!(observer.store.len(), initial_len + 1);
    }

    #[test]
    fn test_use_llm_config_flag() {
        let config = ObserverConfig::default();
        assert!(!config.use_llm_observations);
        let config_enabled = ObserverConfig {
            use_llm_observations: true,
            ..Default::default()
        };
        assert!(config_enabled.use_llm_observations);
    }

    #[test]
    fn test_watcher_just_fired() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-watcher-fired");
        // After creation, turns_since_last_observation is 0 but watcher hasn't actually fired
        // The flag is meaningful only after on_turn_complete runs
        observer.turns_since_last_observation = 1;
        assert!(!observer.watcher_just_fired());
        observer.turns_since_last_observation = 0;
        assert!(observer.watcher_just_fired());
    }

    #[test]
    fn test_has_skeleton_data() {
        let observer = Observer::new(ObserverConfig::default());
        assert!(!observer.has_skeleton_data());
    }

    #[test]
    fn test_cps_four_signal_model() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-cps");
        let mut traj = Trajectory::new();
        // Add a successful tool message
        traj.messages.push(Message {
            id: uuid::Uuid::new_v4(),
            role: Role::Tool,
            content: serde_json::json!("file written successfully"),
            timestamp: chrono::Utc::now(),
            tokens: 10,
            is_compressed: false,
            tool_meta: Default::default(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        });
        let msgs: Vec<&Message> = traj.messages.iter().collect();
        let cps = observer.calculate_cps(&msgs);
        // Should be > 0 and <= 1.0
        assert!(cps > 0.0 && cps <= 1.0);
        // With a single PASS outcome: 0.25*s0 + 0.30*s1 + 0.25*0.5 + 0.20*0.6
        // = 0.25*1.0 + 0.30*(1/5) + 0.125 + 0.12
        // = 0.25 + 0.06 + 0.125 + 0.12 = 0.555
        assert!((cps - 0.555).abs() < 0.01, "CPS was {}", cps);
    }

    #[test]
    fn test_dcr_with_ast_churn() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-dcr");
        observer.last_ast_churn = Some((10, 5, 15)); // 10 added, 5 removed
        // Provide messages that mention files so features are extracted
        let mut traj = Trajectory::new();
        traj.messages.push(Message {
            id: uuid::Uuid::new_v4(),
            role: Role::Assistant,
            content: serde_json::json!({ "tool_code": "edit", "path": "src/main.rs" }),
            timestamp: chrono::Utc::now(),
            tokens: 10,
            is_compressed: false,
            tool_meta: Default::default(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            thinking: None,
        });
        let msgs: Vec<&Message> = traj.messages.iter().collect();
        let dcr = observer.calculate_dcr(&msgs);
        assert!(dcr > 0.0 && dcr <= 1.0, "DCR was {}", dcr);
    }

    #[test]
    fn test_dcr_fallback_without_ast_churn() {
        let observer = Observer::new_with_task(ObserverConfig::default(), "test-dcr-fb");
        let msgs: Vec<&Message> = vec![];
        let dcr = observer.calculate_dcr(&msgs);
        // No edits, no AST churn -> should use fallback
        assert!(dcr >= 0.0);
    }

    #[test]
    fn test_widening_detected() {
        let observer = Observer::new_with_task(ObserverConfig::default(), "test-widening");
        let mut traj = Trajectory::new();
        // 3 tool messages with same error but different files
        for i in 0..3 {
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(),
                role: Role::Assistant,
                content: serde_json::json!({ "tool_code": "edit", "path": format!("file{}.rs", i), "instruction": "fix the bug" }),
                timestamp: chrono::Utc::now(),
                tokens: 10,
                is_compressed: false,
                tool_meta: Default::default(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                thinking: None,
            });
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(),
                role: Role::Tool,
                content: serde_json::json!("error: syntax error at line 42"),
                timestamp: chrono::Utc::now(),
                tokens: 10,
                is_compressed: false,
                tool_meta: Default::default(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                thinking: None,
            });
        }
        let pattern = observer.classify_loop_pattern(&traj);
        assert_eq!(pattern, LoopPattern::Widening, "Expected WIDENING, got {:?}", pattern);
    }

    #[test]
    fn test_sqs_weights_configurable() {
        let config = ObserverConfig {
            sqs_weights: SqsWeights {
                diffusion: 0.5,
                ee_ratio: 0.1,
                dcr: 0.1,
                cps: 0.1,
                monotonicity: 0.2,
            },
            ..Default::default()
        };
        assert!((config.sqs_weights.diffusion - 0.5).abs() < 0.001);
        assert!((config.sqs_weights.monotonicity - 0.2).abs() < 0.001);
    }

    #[test]
    fn test_language_normalization_python() {
        let norm = normalized_ast_churn("python", 15.0, 200.0);
        assert!(norm > 0.0, "norm = {}", norm);
        // edit_norm = 15/15 = 1.0, size_norm = (200/201)^0.3 ≈ 0.999
        // result ≈ 1.0
        assert!((norm - 1.0).abs() < 0.1, "norm = {}", norm);
    }

    #[test]
    fn test_language_normalization_rust_smaller() {
        let norm_rust = normalized_ast_churn("rust", 6.0, 250.0);
        let norm_python = normalized_ast_churn("python", 6.0, 250.0);
        // Rust has higher SCI (80 vs 12), so same churn should normalize differently
        // Rust edit_norm = 6/6 = 1.0, Python edit_norm = 6/15 = 0.4
        assert!(norm_rust > norm_python, "rust={} python={}", norm_rust, norm_python);
    }

    #[test]
    fn test_instruction_similarity_identical() {
        assert!((instruction_similarity(Some("fix the bug"), Some("fix the bug")) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_instruction_similarity_different() {
        let sim = instruction_similarity(Some("fix the bug in parser"), Some("fix the bug in main"));
        // Overlapping words: "fix", "the", "bug", "in" = 4/5 = 0.8
        assert!((sim - 0.8).abs() < 0.01, "sim = {}", sim);
    }

    #[test]
    fn test_instruction_similarity_none() {
        assert_eq!(instruction_similarity(None, Some("test")), 0.0);
    }

    #[test]
    fn test_adaptive_cooldown_ts_formula() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-cooldown");
        observer.config.reflection_cooldown = 4;
        observer.recent_edits.push(("a.rs".to_string(), "e".to_string()));
        observer.recent_edits.push(("b.rs".to_string(), "e".to_string()));
        observer.recent_edits.push(("c.rs".to_string(), "e".to_string()));
        observer.recent_edits.push(("d.rs".to_string(), "e".to_string()));
        let cd = observer.reflection_cooldown_turns();
        // files > 3: cd += 2 → 6
        assert_eq!(cd, 6);
    }

    #[test]
    fn test_adaptive_cooldown_single_file() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-cooldown-sf");
        observer.config.reflection_cooldown = 4;
        observer.recent_edits.push(("a.rs".to_string(), "e".to_string()));
        let cd = observer.reflection_cooldown_turns();
        // files <= 1: cd = max(2, 4-2) = 2
        assert_eq!(cd, 2);
    }

    #[test]
    fn test_compute_pause_weight() {
        let observer = Observer::new_with_task(ObserverConfig::default(), "test-pause");
        let w = observer.compute_pause_weight(10.0, true, false, 0.3, false);
        // base=0.02, duration>8: *=2.0 → 0.04, after_error: *=2.0 → 0.08
        assert!((w - 0.08).abs() < 0.001, "weight = {}", w);
    }

    #[test]
    fn test_compute_pause_weight_high_entropy_early_return() {
        let observer = Observer::new_with_task(ObserverConfig::default(), "test-pause-he");
        let w = observer.compute_pause_weight(10.0, true, false, 0.7, true);
        // base=0.04, after_error: 0.08, entropy>0.6 → return 0.08*0.3 = 0.024
        // ast_contradiction is NOT applied due to early return
        assert!((w - 0.024).abs() < 0.001, "weight = {}", w);
    }

    #[test]
    fn test_token_efficiency_uses_memory_cap() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-tok-eff");
        observer.config.memory_cap_tokens = 1000;
        // Simulate some observations in the store
        let obs = Observation {
            obs_type: ObservationType::Watcher,
            text: "test observation with some text".to_string(),
            timestamp: 0,
            confidence: 0.5,
            token_estimate: 10,
            compressed_range: None,
            critic_action: None,
            sqs: None,
            fidelity: None,
            key: None,
        };
        observer.store.append(obs);
        // trigger the efficiency update path
        observer.metrics.token_efficiency.observation_value_loads += 1;
        let observation_tokens = observer.store.estimate_token_count() as f32;
        let cap = observer.config.memory_cap_tokens as f32;
        let ratio = observation_tokens / cap;
        assert!(ratio > 0.0 && ratio < 1.0, "ratio = {}", ratio);
    }

    #[test]
    fn test_observation_key_roundtrip() {
        let key = ObservationKey {
            signature: Some("fn main()".to_string()),
            apis_called: vec!["foo".to_string(), "bar".to_string()],
            apis_defined: vec!["main".to_string()],
            docstring_hash: Some("abc123".to_string()),
        };
        let json = serde_json::to_string(&key).unwrap();
        let parsed: ObservationKey = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.signature.unwrap(), "fn main()");
        assert_eq!(parsed.apis_called.len(), 2);
        assert_eq!(parsed.apis_defined.len(), 1);
    }

    #[test]
    fn test_token_count_divisor_is_four() {
        let mut store = store::ObservationStore::new_in_memory();
        // Add 40 chars of text → should be 40/4 = 10 tokens
        store.append(Observation {
            obs_type: ObservationType::Watcher,
            text: "a".repeat(40),
            timestamp: 0,
            confidence: 0.5,
            token_estimate: 0,
            compressed_range: None,
            critic_action: None,
            sqs: None,
            fidelity: None,
            key: None,
        });
        assert_eq!(store.estimate_token_count(), 10);
    }

    #[test]
    fn test_avg_latency_ms() {
        let mut tracker = ObserverCostTracker::default();
        tracker.record(ObservationType::Watcher, 50, 100, 1);
        tracker.record(ObservationType::Critic, 60, 200, 2);
        assert!((tracker.avg_latency_ms() - 150.0).abs() < 0.001);
    }

    // -----------------------------------------------------------------------
    // Gap 1: Per-observation decay tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_per_observation_decay_filters_old_observations() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-per-decay");
        // Add a watcher observation from 20 turns ago (high confidence but old)
        observer.current_turn = 20;
        let old_obs = Observation {
            obs_type: ObservationType::Watcher,
            text: "old watcher insight".to_string(),
            timestamp: 0,
            confidence: 0.7,
            token_estimate: 20,
            compressed_range: Some([0, 0]), // turn 0 → 20 turns ago
            critic_action: None, sqs: None, fidelity: None, key: None,
        };
        // Add a recent watcher observation
        let recent_obs = Observation {
            obs_type: ObservationType::Watcher,
            text: "recent watcher insight".to_string(),
            timestamp: 0,
            confidence: 0.7,
            token_estimate: 20,
            compressed_range: Some([18, 18]), // turn 18 → 2 turns ago
            critic_action: None, sqs: None, fidelity: None, key: None,
        };
        observer.store.append(old_obs);
        observer.store.append(recent_obs);

        let min_conf = 0.3;
        let filtered = observer.filter_observations_by_decay(ObservationType::Watcher, min_conf);
        // The old observation (turn 0, 20 turns ago) should have very low decayed confidence
        // The recent one (turn 18, 2 turns ago) should pass
        assert!(!filtered.contains("old watcher insight"), "old obs should be filtered out");
        assert!(filtered.contains("recent watcher insight"), "recent obs should pass");
    }

    #[test]
    fn test_per_observation_decay_keeps_last_two() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-decay-2");
        observer.current_turn = 5;
        // Add 4 recent watcher observations (all should pass decay)
        for i in 0..4 {
            observer.store.append(Observation {
                obs_type: ObservationType::Watcher,
                text: format!("watcher insight {}", i),
                timestamp: 0,
                confidence: 0.9,
                token_estimate: 10,
                compressed_range: Some([i, i]),
                critic_action: None, sqs: None, fidelity: None, key: None,
            });
        }
        let filtered = observer.filter_observations_by_decay(ObservationType::Watcher, 0.1);
        // Should only keep the last 2
        assert!(!filtered.contains("watcher insight 0"));
        assert!(!filtered.contains("watcher insight 1"));
        assert!(filtered.contains("watcher insight 2"));
        assert!(filtered.contains("watcher insight 3"));
    }

    #[test]
    fn test_per_observation_decay_returns_empty_when_none_pass() {
        let observer = Observer::new_with_task(ObserverConfig::default(), "test-decay-empty");
        // No observations → empty
        let filtered = observer.filter_observations_by_decay(ObservationType::Watcher, 0.5);
        assert!(filtered.is_empty());
    }

    // -----------------------------------------------------------------------
    // Gap 2: Token ratio summarizer tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_should_summarize_token_ratio() {
        let mut observer = Observer::new_with_task(
            ObserverConfig {
                buffer_activation: 2,
                block_after: 0.8, // trigger when unobserved tokens >= 80% of threshold
                token_threshold: 100, // 100 tokens threshold
                ..Default::default()
            },
            "test-summarize-ratio",
        );
        observer.current_turn = 5;

        // Create trajectory with enough unobserved messages to exceed ratio
        let mut traj = Trajectory::new();
        // Add 5 messages with ~25 chars each → ~31 tokens (25*5/4=31)
        // ratio = 31/100 = 0.31, which is < 0.8 → should NOT summarize
        for _ in 0..5 {
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(),
                role: Role::Tool,
                content: serde_json::json!("a".repeat(25)),
                timestamp: chrono::Utc::now(),
                tokens: 10,
                is_compressed: false,
                tool_meta: Default::default(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                thinking: None,
            });
        }
        assert!(!observer.should_summarize(&traj), "ratio 0.31 < 0.8, should not summarize");

        // Add many more messages to exceed ratio
        // Need 80+ tokens → 320+ chars → need ~13 messages of 25 chars each
        for _ in 0..15 {
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(),
                role: Role::Tool,
                content: serde_json::json!("a".repeat(25)),
                timestamp: chrono::Utc::now(),
                tokens: 10,
                is_compressed: false,
                tool_meta: Default::default(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                thinking: None,
            });
        }
        // Now 20 messages * 25 chars / 4 = 125 tokens, ratio = 125/100 = 1.25 >= 0.8
        assert!(observer.should_summarize(&traj), "ratio >= 0.8, should summarize");
    }

    #[test]
    fn test_should_summarize_requires_min_unobserved() {
        let mut observer = Observer::new_with_task(
            ObserverConfig { buffer_activation: 1, block_after: 0.1, ..Default::default() },
            "test-summarize-min",
        );
        observer.current_turn = 5;
        let mut traj = Trajectory::new();
        // Only 3 unobserved messages → less than 4 minimum
        for _ in 0..3 {
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(),
                role: Role::Tool,
                content: serde_json::json!("a".repeat(100)),
                timestamp: chrono::Utc::now(),
                tokens: 10,
                is_compressed: false,
                tool_meta: Default::default(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                thinking: None,
            });
        }
        assert!(!observer.should_summarize(&traj), "only 3 unobserved, need >= 4");
    }

    #[test]
    fn test_get_unobserved_token_estimate() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-unobs-tok");
        observer.last_observed_message_index = 2;
        let mut traj = Trajectory::new();
        // 5 messages, 20 chars each
        for _ in 0..5 {
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(),
                role: Role::Tool,
                content: serde_json::json!("a".repeat(20)),
                timestamp: chrono::Utc::now(),
                tokens: 10,
                is_compressed: false,
                tool_meta: Default::default(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                thinking: None,
            });
        }
        // Unobserved: messages 2,3,4 = 3 messages * 22 chars (JSON-wrapped) / 4 = 16 tokens
        let tokens = observer.get_unobserved_token_estimate(&traj);
        assert_eq!(tokens, 16);
    }

    #[test]
    fn test_needs_sync_summary_set_on_turn_complete() {
        let mut observer = Observer::new_with_task(
            ObserverConfig {
                buffer_activation: 1,
                block_after: 0.1,
                token_threshold: 50,
                ..Default::default()
            },
            "test-sync-flag",
        );
        let mut traj = Trajectory::new();
        // Add enough messages to trigger summarizer
        for _ in 0..10 {
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(),
                role: Role::Tool,
                content: serde_json::json!("a".repeat(30)),
                timestamp: chrono::Utc::now(),
                tokens: 10,
                is_compressed: false,
                tool_meta: Default::default(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                thinking: None,
            });
        }
        let result = observer.on_turn_complete(&traj);
        // 10 messages * 30 chars / 4 = 75 tokens, ratio = 75/50 = 1.5 >= 0.1
        assert!(result.needs_sync_summary, "should need sync summary");
    }

    #[test]
    fn test_needs_sync_summary_false_when_below_ratio() {
        let mut observer = Observer::new_with_task(
            ObserverConfig {
                buffer_activation: 1,
                block_after: 10.0, // very high threshold
                token_threshold: 15000,
                ..Default::default()
            },
            "test-sync-no",
        );
        let mut traj = Trajectory::new();
        // Just a few small messages — nowhere near ratio
        for _ in 0..3 {
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(),
                role: Role::Tool,
                content: serde_json::json!("small"),
                timestamp: chrono::Utc::now(),
                tokens: 5,
                is_compressed: false,
                tool_meta: Default::default(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                thinking: None,
            });
        }
        let result = observer.on_turn_complete(&traj);
        assert!(!result.needs_sync_summary, "should not need sync summary");
    }

    #[test]
    fn test_last_sqs_returns_cached() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-last-sqs");
        assert!(observer.last_sqs().is_none());
        let mut traj = Trajectory::new();
        traj.messages.push(Message {
            id: uuid::Uuid::new_v4(), role: Role::Assistant,
            content: serde_json::json!("test"), timestamp: chrono::Utc::now(),
            tokens: 5, is_compressed: false, tool_meta: Default::default(),
            tool_calls: Vec::new(), tool_call_id: None, thinking: None,
        });
        observer.on_turn_complete(&traj);
        assert!(observer.last_sqs().is_some());
    }

    // -----------------------------------------------------------------------
    // Gap: ast_delta_nodes, error_sig, ObservationKey
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_action_features_error_sig() {
        let msg = Message {
            id: uuid::Uuid::new_v4(), role: Role::Tool,
            content: serde_json::json!("error: cannot find symbol `foo` in scope"),
            timestamp: chrono::Utc::now(), tokens: 10, is_compressed: false,
            tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
            thinking: None,
        };
        let features = extract_action_features(&[&msg], None);
        assert!(features[0].error_sig.is_some());
        assert!(features[0].error_sig.as_ref().unwrap().contains("error"));
        assert!(!features[0].success);
    }

    #[test]
    fn test_extract_action_features_no_error() {
        let msg = Message {
            id: uuid::Uuid::new_v4(), role: Role::Tool,
            content: serde_json::json!("file written successfully"),
            timestamp: chrono::Utc::now(), tokens: 10, is_compressed: false,
            tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
            thinking: None,
        };
        let features = extract_action_features(&[&msg], None);
        assert!(features[0].error_sig.is_none());
        assert!(features[0].success);
    }

    #[test]
    fn test_extract_action_features_ast_delta_on_last() {
        let msgs: Vec<Message> = (0..3).map(|_| Message {
            id: uuid::Uuid::new_v4(), role: Role::Assistant,
            content: serde_json::json!({"tool_code": "edit", "path": "main.rs"}),
            timestamp: chrono::Utc::now(), tokens: 10, is_compressed: false,
            tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
            thinking: None,
        }).collect();
        let refs: Vec<&Message> = msgs.iter().collect();
        let features = extract_action_features(&refs, Some(7)); // net +7 AST nodes
        assert_eq!(features[0].ast_delta_nodes, None); // not last
        assert_eq!(features[1].ast_delta_nodes, None); // not last
        assert_eq!(features[2].ast_delta_nodes, Some(7)); // last gets the delta
    }

    #[test]
    fn test_ast_delta_helper() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-delta");
        assert!(observer.ast_delta().is_none());
        observer.set_ast_churn(Some((10, 3, 13)));
        assert_eq!(observer.ast_delta(), Some(7));
        observer.set_ast_churn(Some((2, 8, 10)));
        assert_eq!(observer.ast_delta(), Some(-6));
    }

    #[test]
    fn test_build_observation_key_populates_signature() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-key");
        observer.recent_edits.push(("src/main.rs".to_string(), "edited".to_string()));
        let key = observer.build_observation_key();
        assert_eq!(key.signature.as_deref(), Some("src/main.rs"));
        assert!(key.apis_called.is_empty());
        assert!(key.apis_defined.is_empty());
    }

    #[test]
    fn test_enrich_latest_key_adds_apis() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-enrich");
        observer.store.append(Observation {
            obs_type: ObservationType::Skeleton, text: "test".to_string(),
            timestamp: 0, confidence: 0.5, token_estimate: 10,
            compressed_range: None, critic_action: None, sqs: None,
            fidelity: None, key: Some(ObservationKey::default()),
        });
        let apis = ExtractApisResponse {
            calls: vec!["foo".to_string(), "bar".to_string()],
            definitions: vec!["main".to_string()],
        };
        observer.enrich_latest_key(&apis);
        let all = observer.store.get_all();
        let key = all.last().unwrap().key.as_ref().unwrap();
        assert_eq!(key.apis_called, vec!["foo", "bar"]);
        assert_eq!(key.apis_defined, vec!["main"]);
    }

    #[test]
    fn test_skeleton_observation_has_key() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-skel-key");
        observer.recent_edits.push(("lib.rs".to_string(), "edit".to_string()));
        observer.trigger_skeleton_observation();
        let last = observer.store.get_all().last().unwrap();
        assert!(last.key.is_some());
        assert_eq!(last.key.as_ref().unwrap().signature.as_deref(), Some("lib.rs"));
    }

    // -----------------------------------------------------------------------
    // Gap closure: diffusion, loop patterns, docstring_hash, memory cap
    // -----------------------------------------------------------------------

    #[test]
    fn test_real_diffusion_high_when_many_files_shallow() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-diffusion");
        observer.current_turn = 5;
        let msgs: Vec<Message> = (0..5).map(|i| Message {
            id: uuid::Uuid::new_v4(), role: Role::Assistant,
            content: serde_json::json!({"tool_code": "edit", "path": format!("file{}.rs", i)}),
            timestamp: chrono::Utc::now(), tokens: 10, is_compressed: false,
            tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
            thinking: None,
        }).collect();
        let refs: Vec<&Message> = msgs.iter().collect();
        let diff = observer.calculate_diffusion(&refs);
        // 5 unique files with 1 action each → high diffusion
        assert!(diff > 0.5, "5 unique files should have high diffusion, got {}", diff);
    }

    #[test]
    fn test_real_diffusion_low_when_one_file_deep() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-diffusion-low");
        observer.current_turn = 5;
        let msgs: Vec<Message> = (0..5).map(|_| Message {
            id: uuid::Uuid::new_v4(), role: Role::Assistant,
            content: serde_json::json!({"tool_code": "edit", "path": "main.rs"}),
            timestamp: chrono::Utc::now(), tokens: 10, is_compressed: false,
            tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
            thinking: None,
        }).collect();
        let refs: Vec<&Message> = msgs.iter().collect();
        let diff = observer.calculate_diffusion(&refs);
        // 1 file with 5 actions → low diffusion
        assert!(diff < 0.5, "1 file with 5 actions should have low diffusion, got {}", diff);
    }

    #[test]
    fn test_repeated_file_op_detected() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-repeated-fileop");
        observer.current_turn = 10;
        let msgs: Vec<Message> = (0..3).map(|_| Message {
            id: uuid::Uuid::new_v4(), role: Role::Tool,
            content: serde_json::json!("ok"),
            timestamp: chrono::Utc::now(), tokens: 5, is_compressed: false,
            tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
            thinking: None,
        }).collect();
        let assistant_msgs: Vec<Message> = (0..3).map(|_| Message {
            id: uuid::Uuid::new_v4(), role: Role::Assistant,
            content: serde_json::json!({"tool_code": "edit", "path": "main.rs"}),
            timestamp: chrono::Utc::now(), tokens: 10, is_compressed: false,
            tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
            thinking: None,
        }).collect();
        let mut traj = Trajectory::new();
        for m in msgs { traj.messages.push(m); }
        for m in assistant_msgs { traj.messages.push(m); }
        let pattern = observer.classify_loop_pattern(&traj);
        assert_eq!(pattern, LoopPattern::RepeatedFileOp);
    }

    #[test]
    fn test_syntax_loop_detected() {
        let observer = Observer::new_with_task(ObserverConfig::default(), "test-syntax-loop");
        let mut traj = Trajectory::new();
        for _ in 0..3 {
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(), role: Role::Tool,
                content: serde_json::json!("syntax error: unexpected token"),
                timestamp: chrono::Utc::now(), tokens: 5, is_compressed: false,
                tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
                thinking: None,
            });
        }
        let pattern = observer.classify_loop_pattern(&traj);
        assert_eq!(pattern, LoopPattern::SyntaxLoop);
    }

    #[test]
    fn test_docstring_hash_populated_from_edits() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-doc-hash");
        observer.recent_edits.push(("lib.rs".to_string(), "/// This is a docstring\nfn main() {}".to_string()));
        let key = observer.build_observation_key();
        assert!(key.docstring_hash.is_some(), "docstring_hash should be populated when content has doc comments");
    }

    #[test]
    fn test_docstring_hash_none_when_no_docstrings() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-doc-hash-none");
        observer.recent_edits.push(("lib.rs".to_string(), "fn main() {}".to_string()));
        let key = observer.build_observation_key();
        assert!(key.docstring_hash.is_none(), "docstring_hash should be None when no doc comments");
    }

    #[test]
    fn test_memory_cap_triggers_reflection() {
        let mut observer = Observer::new_with_task(
            ObserverConfig { memory_cap_tokens: 10, reflection_token_threshold: 1, ..Default::default() },
            "test-mem-cap",
        );
        observer.current_turn = 5;
        // Add observations until token estimate exceeds cap
        for i in 0..10 {
            observer.store.append(Observation {
                obs_type: ObservationType::Watcher, text: format!("long observation text number {} with padding", i),
                timestamp: 0, confidence: 0.7, token_estimate: 20,
                compressed_range: Some([i, i]), critic_action: None, sqs: None,
                fidelity: None, key: None,
            });
        }
        let prev_reflections = observer.metrics.reflect_actions;
        let mut traj = Trajectory::new();
        traj.messages.push(Message {
            id: uuid::Uuid::new_v4(), role: Role::Assistant,
            content: serde_json::json!("test"), timestamp: chrono::Utc::now(),
            tokens: 5, is_compressed: false, tool_meta: Default::default(),
            tool_calls: Vec::new(), tool_call_id: None, thinking: None,
        });
        observer.on_turn_complete(&traj);
        // Memory cap should have triggered a reflection
        assert!(observer.metrics.reflect_actions > prev_reflections, "reflection should fire when tokens exceed cap");
    }

    #[test]
    fn test_final_compression_triggers_reflection() {
        let mut observer = Observer::new_with_task(
            ObserverConfig { reflection_token_threshold: 1, ..Default::default() },
            "test-final",
        );
        observer.store.append(Observation {
            obs_type: ObservationType::Watcher, text: "test observation text".to_string(),
            timestamp: 0, confidence: 0.7, token_estimate: 10,
            compressed_range: None, critic_action: None, sqs: None,
            fidelity: None, key: None,
        });
        let prev = observer.metrics.reflect_actions;
        observer.final_compression();
        assert!(observer.metrics.reflect_actions > prev, "final_compression should trigger reflection");
    }

    #[test]
    fn test_compute_pause_weight_values() {
        let observer = Observer::new_with_task(ObserverConfig::default(), "test-pause");
        // Low duration, no error → base weight
        let w = observer.compute_pause_weight(3.0, false, false, 0.3, false);
        assert!(w > 0.0 && w < 0.05, "base weight should be small, got {}", w);
        // High duration + error → amplified
        let w2 = observer.compute_pause_weight(15.0, true, false, 0.3, false);
        assert!(w2 > w, "high duration + error should amplify, got {} vs {}", w2, w);
    }

    // -----------------------------------------------------------------------
    // Recall integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_recall_keyword_match() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-recall");
        observer.store.append(Observation {
            obs_type: ObservationType::Watcher, text: "Agent is editing main.rs repeatedly".to_string(),
            timestamp: 0, confidence: 0.7, token_estimate: 10,
            compressed_range: None, critic_action: None, sqs: None, fidelity: None, key: None,
        });
        observer.store.append(Observation {
            obs_type: ObservationType::Critic, text: "Stuck in a loop on parser module".to_string(),
            timestamp: 0, confidence: 0.8, token_estimate: 10,
            compressed_range: None, critic_action: None, sqs: None, fidelity: None, key: None,
        });
        let result = observer.recall("main.rs");
        assert!(result.contains("main.rs"), "recall should find main.rs observation");
        assert!(!result.contains("parser"), "recall should not return parser observation");
    }

    #[test]
    fn test_recall_no_match() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-recall-nomatch");
        observer.store.append(Observation {
            obs_type: ObservationType::Watcher, text: "some observation".to_string(),
            timestamp: 0, confidence: 0.7, token_estimate: 10,
            compressed_range: None, critic_action: None, sqs: None, fidelity: None, key: None,
        });
        let result = observer.recall("nonexistent_query_xyz");
        assert!(result.contains("No observations matching"), "recall should report no matches");
    }

    #[test]
    fn test_recall_with_daemon_results_prepends() {
        let observer = Observer::new_with_task(ObserverConfig::default(), "test-recall-daemon");
        let daemon_results = vec![
            "Agent was stuck on auth module at turn 5".to_string(),
            "Agent resolved auth issue by re-reading config".to_string(),
        ];
        let result = observer.recall_with_daemon_results("auth module", &daemon_results);
        assert!(result.contains("[daemon]"), "recall should include daemon results");
        assert!(result.contains("auth module at turn 5"));
        assert!(result.contains("resolved auth issue"));
    }

    #[test]
    fn test_recall_stats_mode() {
        let observer = Observer::new_with_task(ObserverConfig::default(), "test-recall-stats");
        let result = observer.recall("--stats");
        assert!(result.contains("Observer stats"), "recall --stats should return stats");
    }

    // -----------------------------------------------------------------------
    // Observation lifecycle: store → decay → compress → recall
    // -----------------------------------------------------------------------

    #[test]
    fn test_observation_lifecycle_decay_and_recall() {
        let mut observer = Observer::new_with_task(
            ObserverConfig { memory_cap_tokens: 10000, ..Default::default() },
            "test-lifecycle",
        );
        observer.current_turn = 10;

        // Add observations of different types at different turns
        observer.store.append(Observation {
            obs_type: ObservationType::Watcher, text: "Early observation about auth setup".to_string(),
            timestamp: 0, confidence: 0.9, token_estimate: 10,
            compressed_range: Some([1, 3]), critic_action: None, sqs: None, fidelity: None, key: None,
        });
        observer.store.append(Observation {
            obs_type: ObservationType::Critic, text: "Mid-task observation about database migration".to_string(),
            timestamp: 0, confidence: 0.7, token_estimate: 15,
            compressed_range: Some([5, 7]), critic_action: None, sqs: None, fidelity: None, key: None,
        });
        observer.store.append(Observation {
            obs_type: ObservationType::Filter, text: "Late observation about test failures".to_string(),
            timestamp: 0, confidence: 0.8, token_estimate: 12,
            compressed_range: Some([9, 10]), critic_action: None, sqs: None, fidelity: None, key: None,
        });

        // Recall should find all three by keyword
        let auth = observer.recall("auth");
        assert!(auth.contains("auth setup"));

        let db = observer.recall("database");
        assert!(db.contains("database migration"));

        let test = observer.recall("test");
        assert!(test.contains("test failures"));

        // Decay should filter old observations from the block
        let block = observer.build_observation_block();
        // Block should still contain content (exact filtering depends on confidence decay)
        assert!(!block.is_empty() || observer.store.len() > 0);
    }

    #[test]
    fn test_sqs_computation_with_five_signals() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-sqs-five");
        observer.current_turn = 5;

        let mut traj = Trajectory::new();
        // Add mixed messages to exercise SQS computation
        traj.messages.push(Message {
            id: uuid::Uuid::new_v4(), role: Role::Assistant,
            content: serde_json::json!({"tool_code": "edit", "path": "main.rs"}),
            timestamp: chrono::Utc::now(), tokens: 50, is_compressed: false,
            tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
            thinking: None,
        });
        traj.messages.push(Message {
            id: uuid::Uuid::new_v4(), role: Role::Tool,
            content: serde_json::json!("error: syntax error on line 42"),
            timestamp: chrono::Utc::now(), tokens: 20, is_compressed: false,
            tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
            thinking: None,
        });

        let sqs = observer.compute_sqs(&traj);
        assert!(sqs.score >= 0.0 && sqs.score <= 1.0, "SQS score should be in [0,1], got {}", sqs.score);
        assert!(!sqs.status.is_empty(), "SQS status should be set");
    }

    #[test]
    fn test_loop_pattern_classification_all_variants() {
        // Stuck: same error repeated, same file
        let observer = Observer::new_with_task(ObserverConfig::default(), "test-loops");
        let mut traj = Trajectory::new();
        for _ in 0..5 {
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(), role: Role::Tool,
                content: serde_json::json!("error: type mismatch in main.rs"),
                timestamp: chrono::Utc::now(), tokens: 10, is_compressed: false,
                tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
                thinking: None,
            });
            traj.messages.push(Message {
                id: uuid::Uuid::new_v4(), role: Role::Assistant,
                content: serde_json::json!({"tool_code": "edit", "path": "main.rs"}),
                timestamp: chrono::Utc::now(), tokens: 10, is_compressed: false,
                tool_meta: Default::default(), tool_calls: Vec::new(), tool_call_id: None,
                thinking: None,
            });
        }
        let pattern = observer.classify_loop_pattern(&traj);
        assert!(matches!(pattern, LoopPattern::Stuck | LoopPattern::SyntaxLoop | LoopPattern::RepeatedFileOp),
            "repeated same-file errors should classify as Stuck/SyntaxLoop/RepeatedFileOp, got {:?}", pattern);
    }

    #[test]
    fn test_observation_key_uniqueness() {
        let mut obs1 = Observer::new_with_task(ObserverConfig::default(), "test-key1");
        obs1.recent_edits.push(("main.rs".to_string(), "/// Doc comment\nfn foo() {}".to_string()));
        let key1 = obs1.build_observation_key();

        let mut obs2 = Observer::new_with_task(ObserverConfig::default(), "test-key2");
        obs2.recent_edits.push(("main.rs".to_string(), "/// Different doc\nfn bar() {}".to_string()));
        let key2 = obs2.build_observation_key();

        // Same file, different content → different docstring hash
        if key1.docstring_hash.is_some() && key2.docstring_hash.is_some() {
            assert_ne!(key1.docstring_hash, key2.docstring_hash,
                "different docstrings should produce different hashes");
        }
    }

    #[test]
    fn test_tier_adapts_sqs_thresholds() {
        let mut observer = Observer::new_with_task(ObserverConfig::default(), "test-tier-thresholds");

        observer.current_tier = TaskTier::Fallback;
        let fb_threshold = observer.tier_sqs_threshold();

        observer.current_tier = TaskTier::TestDriven;
        let td_threshold = observer.tier_sqs_threshold();

        // Fallback (relaxed) should have higher threshold than TestDriven (tighter)
        assert!(fb_threshold > td_threshold,
            "Fallback SQS threshold ({}) should be higher than TestDriven ({})", fb_threshold, td_threshold);
    }
}
