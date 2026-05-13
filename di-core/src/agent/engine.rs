use crate::agent::trajectory::{Trajectory, Role, ToolMessageMeta, ToolCallEntry, Message};
use crate::agent::parser::StreamingToolAccumulator;
use crate::agent::file_context::FileContextTracker;
use crate::agent::environment::EnvironmentManager;
use crate::observer::Observer;
use crate::context::{ContextManager, ConservativeEstimator, TokenEstimator, TurnMetrics, ToolCallRecord};
use crate::context::lifecycle::ContextLifecycleManager;
use crate::context::distiller::{ContextDistiller, DistillerSource};
use crate::context::task_state::TaskStateReducer;
use crate::agent::metrics::ContextMetrics;
use crate::daemons::{
    GatewayStreamClient, GatewayRequest, GatewayMessage,
    ResilientDaemon,
};
use crate::protocol::{CoreEvent, FrontendMessage};
use crate::tools::{ToolExecutor, ToolCoordinator};
use crate::prompt::{ContextCompiler, DynamicContext, session::SessionContext};
use crate::tools::approval::ApprovalManager;
use anyhow::Result;
use serde_json::json;
use std::collections::{HashSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Debug log — only prints when DIRAC_DEBUG is set.
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if std::env::var("DIRAC_DEBUG").is_ok() {
            eprintln!($($arg)*);
        }
    };
}

/// Truncate a string to `max_len` chars at a char boundary (safe for UTF-8).
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentMode {
    Plan,
    Act,
}

/// Outcome of a single turn, used by run_task to decide whether to continue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TurnOutcome {
    /// Turn completed with N tools used. Continue the loop.
    Continue { tools_used: usize },
    /// Task completed successfully (done tool, new_task, or plan_response).
    Finished,
}

/// Tools allowed in Plan mode (read-only + ask + done + plan + compact).
const PLAN_MODE_TOOLS: &[&str] = &[
    "read", "search", "repo", "symbols",
    "ask", "done", "plan", "compact", "tools",
];

pub struct AgentEngine {
    pub id: Uuid,
    pub trajectory: Trajectory,
    pub observer: Observer,
    pub context_manager: ContextManager,
    pub gateway_client: Arc<GatewayStreamClient>,
    pub tool_executor: ToolExecutor,
    pub coordinator: ToolCoordinator,
    pub approval_manager: ApprovalManager,
    pub abort: Arc<AtomicBool>,
    pub consecutive_mistake_count: usize,
    pub max_consecutive_mistakes: usize,
    pub request_id_counter: i64,
    pub tool_call_counter: i64,
    pub frontend_rx: Option<mpsc::Receiver<FrontendMessage>>,
    pub frontend_tx: mpsc::Sender<FrontendMessage>,
    pub mode: AgentMode,
    pub file_context: FileContextTracker,
    pub environment: EnvironmentManager,
    /// Shared metrics for the context compilation system.
    pub metrics: Arc<ContextMetrics>,
    /// Task state reducer — classifies user messages and tracks goal/constraint state.
    pub task_reducer: TaskStateReducer,
    /// How long (ms) to wait for frontend responses before timing out.
    /// Set to Some(0) to disable timeout (indefinite wait). None uses default.
    pub frontend_timeout_ms: Option<u64>,
    /// Provider config passed from the frontend.
    pub provider_config: Option<crate::daemons::ProviderConfig>,
    /// Calibrated token estimator — replaces inline len()/4 with model-aware estimation.
    pub estimator: ConservativeEstimator,
    /// Turn counter for lifecycle metrics.
    turn_counter: usize,
    /// Context lifecycle manager: state machine for adaptive compaction.
    pub lifecycle: ContextLifecycleManager,
    /// Timestamp of last activity (turn execution). Used for idle detection.
    pub last_activity: std::time::Instant,
    /// Context distiller (shared via Arc<RwLock>). None if no distiller config.
    pub distiller: Option<std::sync::Arc<tokio::sync::RwLock<Box<dyn ContextDistiller>>>>,
    /// Context compiler: builds three-layer system string (stable + session + dynamic).
    context_compiler: Option<ContextCompiler>,
    /// Pending model-initiated compaction summary. Advisory only — runtime decides when to compact.
    pending_compact_summary: Option<String>,
    /// Whether to use the reranking pipeline for context selection. Opt-in; default false.
    use_reranking: bool,
    pub output_manager: Arc<std::sync::Mutex<crate::tools::output_manager::OutputManager>>,
    pub read_file_cache: std::sync::Mutex<crate::tools::read_file::ReadFileCache>,
    /// Cumulative token usage across all turns in this task.
    cumulative_tokens: usize,
    /// Shared provider config — orchestrator updates this, agent reads at turn start.
    shared_provider_config: Arc<tokio::sync::RwLock<Option<crate::daemons::ProviderConfig>>>,
    /// Shared distiller — orchestrator updates this, agent reads at turn start.
    shared_distiller: Arc<tokio::sync::RwLock<Option<std::sync::Arc<tokio::sync::RwLock<Box<dyn ContextDistiller>>>>>>,
    /// Shared timeout — orchestrator updates this, agent reads at turn start.
    shared_timeout_ms: Arc<std::sync::Mutex<Option<u64>>>,
}

impl AgentEngine {
    pub fn new(
        id: Uuid,
        analyzer_daemon: Arc<tokio::sync::Mutex<ResilientDaemon>>,
        command_daemon: Arc<tokio::sync::Mutex<ResilientDaemon>>,
        gateway_client: Arc<GatewayStreamClient>,
    ) -> Self {
        let output_manager = Arc::new(std::sync::Mutex::new(crate::tools::output_manager::OutputManager::new()));
        let (frontend_tx, frontend_rx) = mpsc::channel(256);
        Self {
            id,
            trajectory: Trajectory::new(),
            observer: Observer::new(),
            context_manager: ContextManager::new(32000, 24000),
            gateway_client,
            tool_executor: ToolExecutor::new(
                analyzer_daemon, command_daemon,
                output_manager.clone(),
            ),
            coordinator: ToolCoordinator::new(),
            approval_manager: ApprovalManager::new(),
            abort: Arc::new(AtomicBool::new(false)),
            consecutive_mistake_count: 0,
            max_consecutive_mistakes: 6,
            request_id_counter: 0,
            tool_call_counter: 0,
            frontend_rx: Some(frontend_rx),
            frontend_tx,
            mode: AgentMode::Act,
            file_context: FileContextTracker::new(),
            environment: EnvironmentManager::new(),
            metrics: ContextMetrics::new(),
            task_reducer: TaskStateReducer::new(),
            frontend_timeout_ms: None,
            provider_config: None,
            distiller: None,
            estimator: ConservativeEstimator::default_conservative(),
            turn_counter: 0,
            lifecycle: ContextLifecycleManager::new(),
            last_activity: std::time::Instant::now(),
            context_compiler: None,
            pending_compact_summary: None,
            use_reranking: false,
            output_manager,
            read_file_cache: std::sync::Mutex::new(crate::tools::read_file::ReadFileCache::new()),
            cumulative_tokens: 0,
            shared_provider_config: Arc::new(tokio::sync::RwLock::new(None)),
            shared_distiller: Arc::new(tokio::sync::RwLock::new(None)),
            shared_timeout_ms: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Receive from the frontend channel with the current timeout.
    /// Returns None on timeout or channel closure.
    /// Apply hash-anchored formatting to a raw read file result.
    fn format_read_result(&mut self, raw: &serde_json::Value) -> serde_json::Value {
        // Multi-file: format each result and join with dividers
        if raw.get("_multi_file").is_some() {
            if let Some(results) = raw.get("results").and_then(|v| v.as_array()) {
                let formatted: Vec<String> = results.iter().map(|r| {
                    let f = self.format_single_read(r);
                    f.as_str().unwrap_or("").to_string()
                }).collect();
                // Join with file dividers
                let mut output = String::new();
                for (i, text) in formatted.iter().enumerate() {
                    if i > 0 {
                        output.push_str("\n\n");
                    }
                    let path = results[i].get("path").and_then(|v| v.as_str()).unwrap_or("?");
                    output.push_str(&format!("--- {} ---\n{}", path, text));
                }
                return serde_json::Value::String(output);
            }
        }

        self.format_single_read(raw)
    }

    fn format_single_read(&mut self, raw: &serde_json::Value) -> serde_json::Value {
        use crate::tools::read_file::{DEFAULT_PREVIEW_LINES, AUTO_EXPAND_PREVIEW_LINES, AUTO_EXPAND_READ_THRESHOLD};

        let path = raw.get("path").and_then(|v| v.as_str()).unwrap_or("?");
        let detail = raw.get("detail").and_then(|v| v.as_str()).unwrap_or("full");
        let mut cache = self.read_file_cache.lock().unwrap();

        // Handle errors from multi-file
        if let Some(error) = raw.get("error").and_then(|v| v.as_str()) {
            return serde_json::Value::String(error.to_string());
        }

        // Markdown outline/hint (pre-computed in handler)
        if raw.get("_markdown").is_some() {
            if let Some(md_output) = raw.get("md_output").and_then(|v| v.as_str()) {
                let hash = crate::util::stable_hash(md_output.as_bytes());
                cache.set_hash(path, detail, None, format!("{:.8}", hash));
                return serde_json::Value::String(format!("[File Hash: {:.8}]\n{}", hash, md_output));
            }
        }

        // Section not found warning
        let section_warning = raw.get("_section_not_found").and_then(|v| v.as_str())
            .map(|s| format!("\n[warning: section '{}' not found in outline]", s));

        // Pagination: compute range from page + cursor
        let page = raw.get("page").and_then(|v| v.as_str());
        let computed_range = if let Some(page_val) = page {
            let cursor = cache.get_cursor(path);
            let read_count = self.file_context.files_read.get(path)
                .map(|s| s.read_count).unwrap_or(0);
            let page_size = if read_count >= AUTO_EXPAND_READ_THRESHOLD {
                AUTO_EXPAND_PREVIEW_LINES
            } else {
                DEFAULT_PREVIEW_LINES
            };
            let total_lines = raw.get("lines").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            if page_val == "section" {
                // Section page: use the range from section jump in handler
                if let Some(range) = raw.get("range").and_then(|v| v.as_array()) {
                    let range_start = range.get(0).and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                    let range_end = range.get(1).and_then(|v| v.as_u64()).unwrap_or(range_start as u64) as usize;
                    cache.set_cursor(path, range_start);
                    Some((range_start, range_end))
                } else {
                    Some((cursor, (cursor + page_size - 1).min(total_lines)))
                }
            } else {
                let start = match page_val {
                    "next" => (cursor + page_size).min(total_lines),
                    "prev" => cursor.saturating_sub(page_size).max(1),
                    _ => cursor,
                };
                cache.set_cursor(path, start);
                Some((start, (start + page_size - 1).min(total_lines)))
            }
        } else {
            None
        };

        let result = Self::format_at_detail(
            raw, detail, &mut cache, &self.file_context, computed_range,
        );

        // Release the cache lock before calling apply_budget_degradation (which takes its own lock)
        drop(cache);

        let mut final_result = result;
        if let Some(warn) = section_warning {
            final_result = serde_json::Value::String(format!("{}\n{}", final_result.as_str().unwrap_or(""), warn));
        }
        final_result = self.apply_budget_degradation(final_result, raw, detail);
        final_result
    }

    /// Format a raw read result at a specific detail level.
    /// Used by format_single_read for the initial render and by apply_budget_degradation
    /// for re-rendering at lower detail levels.
    fn format_at_detail(
        raw: &serde_json::Value,
        detail: &str,
        cache: &mut crate::tools::read_file::ReadFileCache,
        file_context: &FileContextTracker,
        range: Option<(usize, usize)>,
    ) -> serde_json::Value {
        use crate::tools::read_file::{
            format_full, format_preview, format_outline, format_skeleton,
            format_hint, format_ranges, format_chunk_map, merge_ranges,
        };

        let path = raw.get("path").and_then(|v| v.as_str()).unwrap_or("?");

        match detail {
            "outline" => {
                if let Some(analyzer_data) = raw.get("analyzer_data") {
                    format_outline(path, analyzer_data, cache)
                } else {
                    json!({ "path": path, "error": "No analyzer data for outline" })
                }
            }
            "hint" => {
                if let Some(analyzer_data) = raw.get("analyzer_data") {
                    format_hint(path, analyzer_data, cache)
                } else {
                    json!({ "path": path, "error": "No analyzer data for hint" })
                }
            }
            "skeleton" => {
                if let Some(analyzer_data) = raw.get("analyzer_data") {
                    format_skeleton(path, analyzer_data, cache)
                } else {
                    json!({ "path": path, "error": "No analyzer data for skeleton" })
                }
            }
            "preview" => {
                if let Some(content) = raw.get("content").and_then(|v| v.as_str()) {
                    let read_count = file_context.files_read.get(path)
                        .map(|s| s.read_count).unwrap_or(0);
                    let (mut output, _, _) = format_preview(path, content, read_count, cache);
                    if let Some(analyzer_data) = raw.get("analyzer_data") {
                        let chunk_map = format_chunk_map(analyzer_data);
                        if !chunk_map.is_empty() {
                            output.push_str(&chunk_map);
                        }
                    }
                    serde_json::Value::String(output)
                } else {
                    json!({ "path": path, "error": "No content for preview" })
                }
            }
            _ => {
                // full (default)
                if let Some(content) = raw.get("content").and_then(|v| v.as_str()) {
                    let single_range = range.or_else(|| {
                        raw.get("range").and_then(|v| v.as_array())
                            .and_then(|a| {
                                let start = a.get(0)?.as_u64()? as usize;
                                let end = a.get(1)?.as_u64()? as usize;
                                Some((start, end))
                            })
                    });

                    let ranges_raw = raw.get("ranges").and_then(|v| v.as_array());
                    let multi_ranges: Option<Vec<(usize, usize)>> = ranges_raw.and_then(|arr| {
                        let parsed: Vec<(usize, usize)> = arr.iter().filter_map(|a| {
                            let arr = a.as_array()?;
                            Some((arr.get(0)?.as_u64()? as usize, arr.get(1)?.as_u64()? as usize))
                        }).collect();
                        if parsed.is_empty() { None } else { Some(merge_ranges(parsed)) }
                    });

                    if let Some(ranges) = multi_ranges {
                        let (output, _, _) = format_ranges(path, content, &ranges, cache);
                        serde_json::Value::String(output)
                    } else {
                        let (output, _, _) = format_full(path, content, single_range, cache);
                        serde_json::Value::String(output)
                    }
                } else {
                    json!({ "path": path, "error": "No content for full read" })
                }
            }
        }
    }

    /// If the formatted output exceeds a budget threshold, downgrade the detail level
    /// through the cascade: full → preview → skeleton → outline → hint.
    fn apply_budget_degradation(&mut self, result: serde_json::Value, raw: &serde_json::Value, current_detail: &str) -> serde_json::Value {
        const MAX_OUTPUT_CHARS: usize = 24000; // ~8000 tokens at 3 chars/token
        const DEGRADATION_PATH: &[&str] = &["full", "preview", "skeleton", "outline", "hint"];
        const DEGRADATION_NOTE: &str = "\n[Content reduced to fit budget -- use specific line ranges for full detail]";

        let output = match &result {
            serde_json::Value::String(s) => s.clone(),
            _ => return result,
        };

        if output.len() <= MAX_OUTPUT_CHARS || output.contains("unchanged") {
            return result;
        }

        let current_idx = DEGRADATION_PATH.iter().position(|&d| d == current_detail).unwrap_or(0);
        if current_idx >= DEGRADATION_PATH.len() - 1 {
            // Already at lowest level — truncate as last resort
            let truncated: String = output.chars().take(MAX_OUTPUT_CHARS).collect();
            return serde_json::Value::String(format!("{}{}", truncated, DEGRADATION_NOTE));
        }

        // Walk the degradation cascade: try each lower detail level
        let mut cache = self.read_file_cache.lock().unwrap();
        // Preserve the user's original range through degradation
        let original_range = raw.get("range").and_then(|v| v.as_array())
            .and_then(|a| {
                let start = a.get(0)?.as_u64()? as usize;
                let end = a.get(1)?.as_u64()? as usize;
                Some((start, end))
            });
        for try_idx in (current_idx + 1)..DEGRADATION_PATH.len() {
            let try_detail = DEGRADATION_PATH[try_idx];
            let degraded = Self::format_at_detail(raw, try_detail, &mut cache, &self.file_context, original_range);

            if let serde_json::Value::String(ref s) = degraded {
                if s.len() <= MAX_OUTPUT_CHARS {
                    return serde_json::Value::String(format!("{}{}", s, DEGRADATION_NOTE));
                }
            }
            // This level still too large, continue to next
        }

        // Even the lowest level exceeded budget — return it truncated
        let last = Self::format_at_detail(
            raw, DEGRADATION_PATH.last().unwrap(), &mut cache, &self.file_context, original_range,
        );
        match last {
            serde_json::Value::String(s) => {
                let truncated: String = s.chars().take(MAX_OUTPUT_CHARS).collect();
                serde_json::Value::String(format!("{}{}", truncated, DEGRADATION_NOTE))
            }
            _ => result,
        }
    }

    /// Scan conversation history for the most recent file hash for a given path.
    /// Walks backwards through tool results looking for read tool responses containing
    /// `[File Hash: <hex>]`. Handles multi-file responses with `--- <path> ---` dividers.
    pub fn extract_last_known_hash(&self, target_path: &str) -> Option<String> {
        let target_normalized = target_path.trim_start_matches("./");

        for msg in self.trajectory.messages.iter().rev() {
            if msg.role != Role::Tool { continue; }
            if msg.tool_meta.tool_name != "read" { continue; }

            let text = match msg.content.as_str() {
                Some(s) => s,
                None => continue,
            };

            // Check if this path was in the read set
            let path_matches = msg.tool_meta.paths_read.iter().any(|p| {
                p.trim_start_matches("./") == target_normalized
            });
            if !path_matches { continue; }

            // For multi-file responses, isolate the section for this path
            let section = if text.contains(&format!("--- {} ---", target_normalized))
                         || text.contains(&format!("--- {} ---", target_path)) {
                let divider = format!("--- {} ---", target_normalized);
                let alt_divider = format!("--- {} ---", target_path);
                let start = text.find(&divider).or_else(|| text.find(&alt_divider));
                if let Some(idx) = start {
                    let section_start = idx + divider.len();
                    let rest = &text[section_start..];
                    let end = rest.find("\n--- ").unwrap_or(rest.len());
                    &rest[..end]
                } else {
                    text
                }
            } else {
                text
            };

            // Extract [File Hash: <hex>]
            if let Some(hash) = Self::extract_hash_from_text(section) {
                return Some(hash);
            }
        }
        None
    }

    /// Extract the first hash value from text matching any of the read output formats:
    /// `[File Hash: <hex>]`, `[Lines X-Y, Hash: <hex>]`, `[... (Hash: <hex>)]`
    fn extract_hash_from_text(text: &str) -> Option<String> {
        // Try the common patterns in order of specificity
        // Pattern 1: `[File Hash: <hex>]`
        if let Some(hash) = Self::extract_hash_after_marker(text, "[File Hash: ") {
            return Some(hash);
        }
        // Pattern 2: `[Lines X-Y, Hash: <hex>]`
        if let Some(hash) = Self::extract_hash_after_marker(text, ", Hash: ") {
            return Some(hash);
        }
        // Pattern 3: `(Hash: <hex>)` — unchanged detection
        if let Some(hash) = Self::extract_hash_after_marker(text, "(Hash: ") {
            return Some(hash);
        }
        None
    }

    fn extract_hash_after_marker(text: &str, marker: &str) -> Option<String> {
        let start = text.find(marker)?;
        let rest = &text[start + marker.len()..];
        let end = rest.chars().position(|c| c == ']' || c == ')')?;
        let hash = &rest[..end];
        if !hash.is_empty() && hash.chars().all(|c| c.is_ascii_hexdigit()) {
            Some(hash.to_string())
        } else {
            None
        }
    }

    async fn recv_frontend(&mut self) -> Option<FrontendMessage> {
        match &mut self.frontend_rx {
            Some(rx) => {
                match self.frontend_timeout_ms {
                    Some(0) | None => rx.recv().await,
                    Some(ms) => {
                        match tokio::time::timeout(std::time::Duration::from_millis(ms), rx.recv()).await {
                            Ok(msg) => msg,
                            Err(_) => None, // timed out
                        }
                    }
                }
            }
            None => None,
        }
    }

    pub fn is_aborted(&self) -> bool {
        self.abort.load(Ordering::Relaxed)
    }

    pub fn request_abort(&self) {
        self.abort.store(true, Ordering::Relaxed);
    }

    /// Drain any pending UserResponse messages from the frontend channel.
    /// Called between turns so user text sent while the agent was busy gets processed.
    fn drain_user_responses(&mut self) {
        let mut should_abort = false;
        // Collect non-matching messages to re-inject after draining
        let mut to_reinject = Vec::new();
        if let Some(ref mut rx) = self.frontend_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    FrontendMessage::UserResponse { text, .. } => {
                        self.task_reducer.process(&text, false);
                        self.trajectory.add_message(
                            Role::User,
                            json!(text),
                            self.estimator.count_text(&text),
                        );
                    }
                    FrontendMessage::Interrupt { .. } => {
                        should_abort = true;
                    }
                    other => {
                        // Don't discard — leave for recv_frontend to handle
                        to_reinject.push(other);
                    }
                }
            }
        }
        // Re-inject non-consumed messages back into the channel
        for msg in to_reinject {
            let _ = self.frontend_tx.try_send(msg);
        }
        if should_abort {
            self.request_abort();
        }
        // Sync runtime config updates from shared state
        self.sync_shared_config();
    }

    /// Pull latest config from shared state (written by orchestrator).
    fn sync_shared_config(&mut self) {
        if let Ok(timeout) = self.shared_timeout_ms.lock() {
            if let Some(dur) = *timeout {
                self.frontend_timeout_ms = Some(dur);
            }
        }
        if let Ok(guard) = self.shared_provider_config.try_read() {
            if let Some(ref config) = *guard {
                self.provider_config = Some(config.clone());
            }
        }
        if let Ok(guard) = self.shared_distiller.try_read() {
            if let Some(ref dist) = *guard {
                self.distiller = Some(dist.clone());
            }
        }
    }

    /// Run a complete task: loop over turns until completion, abort, or mistake limit.
    pub async fn run_task(&mut self, initial_task: String) -> Result<()> {
        self.task_reducer.process(&initial_task, true);
        self.trajectory.add_message(Role::User, json!(initial_task), self.estimator.count_text(&initial_task));

        // TaskInitialized is emitted by the orchestrator in main.rs

        loop {
            // Process any user text that arrived while the previous turn was running
            self.drain_user_responses();
            if self.is_aborted() {
                self.emit_event(CoreEvent::TaskFinished {
                    agent_id: self.id,
                    success: false,
                    message: "Interrupted by user".to_string(),
                })?;
                return Ok(());
            }

            let outcome = match self.run_turn().await {
                Ok(o) => o,
                Err(e) => {
                    self.emit_event(CoreEvent::TaskFinished {
                        agent_id: self.id,
                        success: false,
                        message: format!("Error: {}", e),
                    })?;
                    return Err(e);
                }
            };

            match outcome {
                TurnOutcome::Finished => return Ok(()),
                TurnOutcome::Continue { tools_used: 0 } => {
                    self.consecutive_mistake_count += 1;
                    if self.consecutive_mistake_count >= self.max_consecutive_mistakes {
                        self.emit_event(CoreEvent::TaskFinished {
                            agent_id: self.id,
                            success: false,
                            message: "Too many consecutive turns without tool use".to_string(),
                        })?;
                        return Ok(());
                    }
                    self.trajectory.add_message(
                        Role::User,
                        json!("You must respond with a tool call. Use the available tools to make progress on the task."),
                        20,
                    );
                }
                TurnOutcome::Continue { tools_used: _ } => {
                    self.consecutive_mistake_count = 0;
                }
            }

            // Periodic cleanup: remove old output files every 10 turns
            if self.turn_counter % 10 == 0 {
                self.output_manager.lock().unwrap().cleanup();
            }
        }
    }

    /// Execute one turn of the agent loop.
    pub async fn run_turn(&mut self) -> Result<TurnOutcome> {
        debug_log!("[di-core] run_turn: agent {} starting, provider={:?}",
            self.id, self.provider_config.as_ref().map(|c| &c.id));
        eprintln!("[di-core] run_turn start: agent {} turn {}", self.id, self.turn_counter);

        // Update activity timestamp
        self.last_activity = std::time::Instant::now();

        // 0. Init context compiler once (stable prefix + session info)
        if self.context_compiler.is_none() {
            if self.environment.get_details().is_none() {
                self.environment.gather();
            }

            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".into());
            let _home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".into());
            let cores = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);

            let session = SessionContext {
                os: "linux".to_string(),
                shell,
                cwd,
                available_cores: cores,
                mode: self.mode,
                yolo_mode: false,
                skills: None,
                custom_instructions: None,
            };

            self.context_compiler = Some(ContextCompiler::new(&session));
            debug_log!("[di-core] run_turn: context compiler initialized, session prefix {} chars",
                self.context_compiler.as_ref().expect("context compiler initialized above").session_prefix_len());
        }

        // 1. API extraction (stub — always empty until analyzer integration)
        let current_apis: HashSet<String> = HashSet::new();

        // 2. Lifecycle-aware compaction: evaluate state, compact if due
        let current_tokens = self.trajectory.get_total_tokens();
        let token_limit = self.context_compiler.as_ref()
            .map(|c| c.token_limit())
            .unwrap_or(128_000);
        self.lifecycle.evaluate(current_tokens, token_limit);
        if self.lifecycle.should_compact() {
            if let Some(summary) = self.pending_compact_summary.take() {
                self.perform_compaction(&summary).await?;
            } else {
                self.perform_runtime_compaction().await?;
            }
            self.lifecycle.notify_compaction_complete();
            // Reset retry counters after compaction — pre-compaction errors may no longer be relevant
            self.coordinator.reset_router();
        }

        // 3. Build context frame (system = stable + session + dynamic, messages = history)
        let task_summary = Some(self.task_reducer.to_critical_summary());

        // Build tail reminder: goal + constraints + stale files + latest failure
        let stale_files: Vec<String> = self.file_context.files_read.iter()
            .filter(|(_, state)| state.edited_since_read)
            .map(|(path, _)| path.clone())
            .collect();
        let latest_failure = self.trajectory.messages.iter().rev()
            .filter(|m| matches!(m.role, Role::Tool))
            .find_map(|m| {
                let content = m.content.to_string();
                let lower = content.to_lowercase();
                if lower.contains("error") || lower.contains("failed") {
                    Some(content)
                } else {
                    None
                }
            });
        let tail_reminder = Some(self.task_reducer.to_tail_reminder(&stale_files, latest_failure.as_deref()));

        let compaction_summary = self.trajectory.last_checkpoint.as_ref()
            .map(|cp| cp.progress_summary.clone());

        let dynamic = DynamicContext {
            file_context: &self.file_context,
            observations: &self.context_manager.vault,
            current_apis: &current_apis,
            background_summary: &None,
            distilled_context: &None,
            task_state_summary: &task_summary,
            tail_reminder: &tail_reminder,
            compaction_summary: &compaction_summary,
        };

        // Current-frame budget: measure system string first, then compute history budget
        let history_budget = if let Some(compiler) = self.context_compiler.as_ref() {
            let (_system_str, system_tokens) = compiler.build_system_string(&dynamic);
            let tools_tokens = compiler.tools_token_count();
            compiler.compute_history_budget(system_tokens, tools_tokens)
        } else {
            8000
        };

        let messages = if self.use_reranking {
            let active_files: std::collections::HashSet<String> = self.file_context.files_read.keys().cloned()
                .chain(self.file_context.files_edited.iter().cloned())
                .collect();
            let task_keywords = crate::context::reranker::extract_task_keywords(
                &self.task_reducer.state.current_goal,
            );
            self.context_manager.build_prompt_with_reranking(
                &self.trajectory,
                &self.file_context.files_edited,
                Some(&self.task_reducer),
                history_budget,
                &active_files,
                &task_keywords,
            )
        } else {
            self.context_manager.build_prompt_with_stale_check(
                &self.trajectory,
                &self.file_context.files_edited,
                Some(&self.task_reducer),
                history_budget,
            )
        };

        // 4. Observer — compute SQS but don't emit metrics yet (wait for actual usage)
        let _sqs = self.observer.compute_sqs(&self.trajectory).score;

        // 5. Build gateway messages (only user/assistant/tool — no system)
        let mut gateway_msgs: Vec<GatewayMessage> = messages.iter()
            .filter(|m| !matches!(m.role, Role::System))
            .map(|m| {
                let role = match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                    Role::System => "system",
                };
                // Gateway expects content as a string, not a JSON object.
                let content = match &m.content {
                    serde_json::Value::Null => None,
                    serde_json::Value::String(s) => Some(serde_json::Value::String(s.clone())),
                    other => Some(serde_json::Value::String(other.to_string())),
                };

                GatewayMessage {
                    role: role.to_string(),
                    content,
                    content_blocks: None,
                    tool_calls: if m.tool_calls.is_empty() { None } else {
                        Some(m.tool_calls.iter().map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments,
                                }
                            })
                        }).collect())
                    },
                    tool_use_id: m.tool_call_id.clone(),
                    thinking: m.thinking.clone(),
                    name: None,
                }
            }).collect();

        // Gateway providers require at least one message.
        // After compaction, the trajectory may be empty.
        if gateway_msgs.is_empty() {
            gateway_msgs.push(GatewayMessage {
                role: "user".to_string(),
                content: Some(serde_json::json!("[Session continued from previous context. See task state in system prompt.]")),
                content_blocks: None,
                tool_calls: None,
                tool_use_id: None,
                thinking: None,
                name: None,
            });
        }

        // 6. Compile context frame
        let frame = self.context_compiler.as_mut().expect("context compiler initialized in run_turn prologue")
            .build_frame(&dynamic, gateway_msgs);

        debug_log!("[di-core] run_turn: sending gateway request ({} msgs, system {} chars, {} tools)",
            frame.messages.len(), frame.system.len(), frame.tools.len());
        self.request_id_counter += 1;
        let request = GatewayRequest {
            id: self.request_id_counter,
            stream: true,
            provider: self.provider_config.clone(),
            messages: frame.messages,
            system: Some(frame.system),
            tools: Some(frame.tools),
            max_tokens: None,
            temperature: None,
            thinking: None,
            timeout: Some(240000),
        };

        // Debug: dump request to log
        if std::env::var("DIRAC_DEBUG").is_ok() {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/di-core-request.log") {
                use std::io::Write;
                let _ = writeln!(f, "=== Request {} ===", self.request_id_counter);
                let _ = writeln!(f, "provider: {:?}", self.provider_config.as_ref().map(|c| &c.id));
                let _ = writeln!(f, "num_messages: {}", request.messages.len());
                let _ = writeln!(f, "num_tools: {}", request.tools.as_ref().map(|t| t.len()).unwrap_or(0));
                if let Some(tools) = &request.tools {
                    for (i, t) in tools.iter().enumerate() {
                        let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        let _ = writeln!(f, "  tool[{}]: {}", i, name);
                    }
                }
                for (i, msg) in request.messages.iter().enumerate() {
                    let content_preview = msg.content.as_ref()
                        .map(|c| safe_truncate(&c.to_string(), 200).into_owned())
                        .unwrap_or_else(|| "null".to_string());
                    let _ = writeln!(f, "  msg[{}]: role={} tool_calls={} content={}", i, msg.role,
                        msg.tool_calls.as_ref().map(|tc| tc.len()).unwrap_or(0), content_preview);
                }
            }
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/di-core-stream.log") {
                use std::io::Write;
                let _ = writeln!(f, "\n=== Turn {} ===", self.request_id_counter);
            }
        }

        let mut chunk_rx = self.gateway_client.stream_chat(request).await.map_err(|e| {
            eprintln!("gateway stream_chat failed for agent {}: {}", self.id, e);
            e
        })?;
        debug_log!("[di-core] run_turn: streaming response");

        // 7. Accumulate streaming response
        let mut full_text = String::new();
        let mut thinking_text = String::new();
        let mut tool_accumulator = StreamingToolAccumulator::new();
        let mut _usage_total: Option<crate::daemons::Usage> = None;

        while let Some(result) = chunk_rx.recv().await {
            if self.is_aborted() {
                full_text.push_str("\n[interrupted by user]");
                break;
            }

            let chunk = match result {
                Ok(c) => c,
                Err(e) => {
                    self.trajectory.add_message(Role::Assistant, json!(full_text.clone()), self.estimator.count_text(&full_text));
                    return Err(e);
                }
            };

            // Debug: dump every chunk to /tmp/di-core-stream.log
            if std::env::var("DIRAC_DEBUG").is_ok() {
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/di-core-stream.log") {
                    use std::io::Write;
                    let _ = writeln!(f, "[chunk] type={} index={:?} tool_call_id={:?} tool_call_name={:?} json_delta={:?} text_delta={:?}",
                        chunk.chunk_type, chunk.index, chunk.tool_call_id, chunk.tool_call_name,
                        chunk.json_delta.as_ref().map(|s| format!("{}chars:{}", s.len(), safe_truncate(s, 60))),
                        chunk.text_delta.as_ref().map(|s| format!("{}chars", s.len())));
                }
            }

            match chunk.chunk_type.as_str() {
                "delta" => {
                    if !tool_accumulator.feed_chunk(&chunk) {
                        // Text delta
                        if let Some(text) = &chunk.text_delta {
                            full_text.push_str(text);
                            let _ = self.emit_event(CoreEvent::ThoughtDelta {
                                agent_id: self.id,
                                text: text.clone(),
                                thinking: false,
                            });
                        }
                        if let Some(thinking) = &chunk.thinking {
                            thinking_text.push_str(thinking);
                            let _ = self.emit_event(CoreEvent::ThoughtDelta {
                                agent_id: self.id,
                                text: thinking.clone(),
                                thinking: true,
                            });
                        }
                    }
                }
                "content" => {
                    tool_accumulator.feed_chunk(&chunk);
                }
                "stop" => {
                    if let Some(usage) = &chunk.usage {
                        _usage_total = Some(usage.clone());
                    }
                }
                "complete" => break,
                _ => {}
            }
        }

        // Emit actual token usage from provider
        if let Some(usage) = &_usage_total {
            self.cumulative_tokens += usage.total_tokens as usize;
            let sqs = self.observer.compute_sqs(&self.trajectory);
            self.emit_event(CoreEvent::MetricsUpdate {
                agent_id: self.id,
                sqs: sqs.score,
                token_usage: self.cumulative_tokens,
                latency_ms: 0,
            })?;
        }

        // Record assistant thought (redact secrets before storage)
        let redacted_text = crate::util::secrets::redact_secrets(&full_text);
        if redacted_text != full_text { self.metrics.inc_redaction(); }
        let thinking_text = crate::util::secrets::redact_secrets(&thinking_text);
        // Note: thinking_text comparison not tracked since it's a move, not a reference

        // Finalize tool calls (native + XML fallback)
        let tools = tool_accumulator.finalize(&full_text);
        // Debug: log finalize results
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/di-core-stream.log") {
            use std::io::Write;
            let _ = writeln!(f, "[finalize] {} tools, full_text_len={}", tools.len(), full_text.len());
            for (i, t) in tools.iter().enumerate() {
                let _ = writeln!(f, "[finalize] tool[{}]: name={} args={}", i, t.name, t.args);
            }
        }

        let assistant_content = if redacted_text.is_empty() {
            json!("")
        } else {
            json!(redacted_text)
        };
        let assistant_thinking = if thinking_text.is_empty() { None } else { Some(thinking_text) };
        let tool_call_entries: Vec<ToolCallEntry> = tools.iter().map(|tc| {
            self.tool_call_counter += 1;
            ToolCallEntry {
                id: format!("call_{}", self.tool_call_counter),
                name: tc.name.clone(),
                arguments: tc.args.to_string(),
            }
        }).collect();

        self.trajectory.messages.push(Message {
            id: Uuid::new_v4(),
            role: Role::Assistant,
            content: assistant_content,
            timestamp: chrono::Utc::now(),
            tokens: self.estimator.count_text(&redacted_text),
            is_compressed: false,
            tool_meta: ToolMessageMeta::default(),
            tool_calls: tool_call_entries,
            tool_call_id: None,
            thinking: assistant_thinking,
        });
        self.emit_event(CoreEvent::ThoughtFinished { agent_id: self.id })?;

        // 7. Execute tools
        eprintln!("[di-core] run_turn: executing {} tools", tools.len());
        for (ti, tool) in tools.iter().enumerate() {
            if self.is_aborted() {
                break;
            }

            // Mode gate: Plan mode restricts to read-only tools
            if self.mode == AgentMode::Plan && !PLAN_MODE_TOOLS.contains(&tool.name.as_str()) {
                let skip_msg = json!({ "status": "blocked", "message": format!("Tool '{}' not allowed in Plan mode", tool.name) });
                self.trajectory.add_tool_result(skip_msg.clone(), 50, ti, ToolMessageMeta::default());
                self.emit_event(CoreEvent::ToolCallFinished {
                    agent_id: self.id,
                    result: skip_msg,
                })?;
                continue;
            }

            // Track file context (moved after execution so we can hash result content)
            let path_arg = tool.args.get("path").and_then(|v| v.as_str()).map(String::from);
            let tool_name = tool.name.clone();

            // Pre-execution approval gate: destructive tools (write/edit/bash)
            // require user approval BEFORE execution. Read-only tools auto-approve.
            if !self.approval_manager.should_auto_approve(&tool.name) {
                // Emit tool call details FIRST so the user sees what they're approving
                self.emit_event(CoreEvent::ToolCallStarted {
                    agent_id: self.id,
                    tool: tool.name.clone(),
                    args: tool.args.clone(),
                })?;

                let approval_id = Uuid::new_v4();
                let description = format!("Execute {} on behalf of agent", tool.name);
                self.emit_event(CoreEvent::ApprovalNeeded {
                    agent_id: self.id,
                    approval_id,
                    tool: tool.name.clone(),
                    args: tool.args.clone(),
                    description: description.clone(),
                })?;

                // Block waiting for approval response from frontend.
                // Match on approval_id to prevent replay attacks from stale responses.
                let approved = loop {
                    let msg = self.recv_frontend().await;
                    match msg {
                        Some(FrontendMessage::ApprovalResponse { approval_id: ref resp_id, approved, .. }) => {
                            // Accept if no ID (backward compat) or if IDs match
                            let matches = resp_id.map_or(true, |rid| rid == approval_id);
                            if matches {
                                break approved;
                            }
                            // Stale response — discard and keep waiting
                            continue;
                        }
                        Some(FrontendMessage::UserResponse { text, .. }) => {
                            self.task_reducer.process(&text, false);
                            self.trajectory.add_message(
                                Role::User, json!(text), self.estimator.count_text(&text),
                            );
                            continue;
                        }
                        Some(FrontendMessage::Timeout { duration_ms }) => {
                            self.frontend_timeout_ms = Some(duration_ms);
                            self.emit_event(CoreEvent::FrontendTimeout {
                                agent_id: self.id,
                                tool: Some(tool.name.clone()),
                                question: None,
                            })?;
                            break false;
                        }
                        Some(FrontendMessage::Interrupt { .. }) => {
                            self.request_abort();
                            break false;
                        }
                        _ => {
                            self.emit_event(CoreEvent::FrontendTimeout {
                                agent_id: self.id,
                                tool: Some(tool.name.clone()),
                                question: None,
                            })?;
                            break false;
                        }
                    }
                };

                if !approved {
                    let skip_msg = json!({ "status": "denied", "message": "Frontend timeout or denial" });
                    self.trajectory.add_tool_result(skip_msg.clone(), 50, ti, ToolMessageMeta::default());
                    self.emit_event(CoreEvent::ToolCallFinished {
                        agent_id: self.id,
                        result: skip_msg,
                    })?;
                    continue;
                }
            } else {
                // Auto-approved: emit tool call started normally
                self.emit_event(CoreEvent::ToolCallStarted {
                    agent_id: self.id,
                    tool: tool.name.clone(),
                    args: tool.args.clone(),
                })?;
            }

            eprintln!("[di-core] run_turn: executing tool {} ({})", ti, tool.name);
            let exec_result = self.tool_executor.execute(tool, &mut self.coordinator).await;
            eprintln!("[di-core] run_turn: tool {} done ({})", ti, if exec_result.is_ok() { "ok" } else { "err" });

            // Track file context after execution
            // - search/repo/symbols: metadata observation only (no stale detection)
            // - read: handled by format_read_result with correct content hash
            // - write/edit: invalidate caches
            if let Some(ref path) = path_arg {
                match tool_name.as_str() {
                    "search" | "repo" | "symbols" => {
                        self.file_context.mark_metadata_observed(path);
                    }
                    "write" | "edit" => {
                        self.file_context.mark_edited(path);
                        self.coordinator.invalidate_for_path(path);
                        self.coordinator.invalidate_search_and_repo();
                        // Invalidate read cache so subsequent reads don't report "unchanged"
                        self.read_file_cache.lock().unwrap().invalidate_for_path(path);
                    }
                    _ => {}
                }
            }

            match exec_result {
                Ok(result) => {
                    // Handle frontend-interactive tools
                    let action = result.get("_frontend_action").and_then(|v| v.as_str());

                    if action == Some("attempt_completion") || action == Some("plan_response") {
                        // Both done and plan tools can signal completion
                        if action == Some("plan_response") {
                            // Plan mode: emit the plan, don't abort
                            let plan = result.get("plan").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let plan_json = json!({ "plan": plan, "status": "planned" });
                            self.trajectory.add_tool_result(plan_json.clone(), 50, ti, ToolMessageMeta::default());
                            self.emit_event(CoreEvent::ToolCallFinished {
                                agent_id: self.id,
                                result: plan_json,
                            })?;
                        } else {
                        let message = result.get("result").and_then(|v| v.as_str()).unwrap_or("Task complete").to_string();
                        self.trajectory.add_tool_result(json!({ "status": "completed", "message": &message }), self.estimator.count_text(&message), ti, ToolMessageMeta::default());
                        self.emit_event(CoreEvent::ToolCallFinished {
                            agent_id: self.id,
                            result: json!({ "status": "completed", "message": &message }),
                        })?;
                        // Emit TaskPresented instead of TaskFinished — agent signals done
                        // but the user should be able to send follow-up messages
                        self.emit_event(CoreEvent::TaskPresented {
                            agent_id: self.id,
                            message: message.clone(),
                        })?;
                        eprintln!("[di-core] TaskPresented emitted, waiting for user follow-up");
                        // Block waiting for user follow-up (approve/continue/new message)
                        let user_msg = loop {
                            let msg = self.recv_frontend().await;
                            match msg {
                                Some(FrontendMessage::UserResponse { text, .. }) => break Some(text),
                                Some(FrontendMessage::ApprovalResponse { approved: false, .. }) => break None,
                                Some(FrontendMessage::ApprovalResponse { approved: true, .. }) => {
                                    // User acknowledged the result — continue waiting for real input
                                    continue;
                                }
                                Some(FrontendMessage::Timeout { duration_ms }) => {
                                    self.frontend_timeout_ms = Some(duration_ms);
                                    break None;
                                }
                                Some(FrontendMessage::Interrupt { .. }) => {
                                    self.request_abort();
                                    break None;
                                }
                                _ => continue,
                            }
                        };
                        if let Some(text) = user_msg {
                            self.task_reducer.process(&text, false);
                            self.trajectory.add_message(Role::User, json!(text), self.estimator.count_text(&text));
                        } else {
                            return Ok(TurnOutcome::Continue { tools_used: tools.len() });
                        }
                        }
                    } else if action == Some("followup_question") {
                        let question = result.get("question").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let options = result.get("options").as_ref().and_then(|v| {
                            v.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        });
                        self.emit_event(CoreEvent::FollowupQuestion {
                            agent_id: self.id,
                            question: question.clone(),
                            options: options.clone(),
                        })?;

                        // Block waiting for followup answer from frontend.
                        // Buffer any UserResponse messages that arrive while waiting.
                        let answer_text = loop {
                            let msg = self.recv_frontend().await;
                            match msg {
                                Some(FrontendMessage::FollowupAnswer { text, .. }) => break text,
                                Some(FrontendMessage::UserResponse { text, .. }) => {
                                    self.task_reducer.process(&text, false);
                                    self.trajectory.add_message(
                                        Role::User, json!(text), self.estimator.count_text(&text),
                                    );
                                    continue;
                                }
                                Some(FrontendMessage::Timeout { duration_ms }) => {
                                    self.frontend_timeout_ms = Some(duration_ms);
                                    self.emit_event(CoreEvent::FrontendTimeout {
                                        agent_id: self.id,
                                        tool: None,
                                        question: Some(question.clone()),
                                    })?;
                                    break String::new();
                                }
                                Some(FrontendMessage::Interrupt { .. }) => {
                                    self.request_abort();
                                    break String::new();
                                }
                                _ => {
                                    self.emit_event(CoreEvent::FrontendTimeout {
                                        agent_id: self.id,
                                        tool: None,
                                        question: Some(question.clone()),
                                    })?;
                                    break String::new();
                                }
                            }
                        };

                        let answer_json = json!({ "question": question, "answer": answer_text, "status": "answered" });
                        self.trajectory.add_tool_result(answer_json.clone(), 50, ti, ToolMessageMeta::default());
                        self.emit_event(CoreEvent::ToolCallFinished {
                            agent_id: self.id,
                            result: answer_json,
                        })?;
                    } else if action == Some("new_task") {
                        // New task: emit event for orchestrator to spawn a new agent
                        let task_text = result.get("task").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        self.emit_event(CoreEvent::TaskFinished {
                            agent_id: self.id,
                            success: true,
                            message: format!("Spawning new task: {}", task_text),
                        })?;
                        self.request_abort();
                        return Ok(TurnOutcome::Finished);
                    } else if result.get("compact").and_then(|v| v.as_bool()).unwrap_or(false) {
                        let summary = result.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let safe_summary = crate::util::secrets::redact_secrets(&summary);
                        if safe_summary != summary { self.metrics.inc_redaction(); }
                        // Lifecycle-aware advisory: check pressure before accepting
                        let current_tokens = self.trajectory.get_total_tokens();
                        let token_limit = self.context_compiler.as_ref()
                            .map(|c| c.token_limit())
                            .unwrap_or(128_000);
                        let advisory = self.lifecycle.evaluate_compact_advisory(
                            &safe_summary, current_tokens, token_limit,
                        );
                        if advisory.allowed {
                            self.pending_compact_summary = Some(safe_summary);
                            let guidance_suffix = advisory.guidance
                                .map(|g| format!(" {}", g))
                                .unwrap_or_default();
                            self.trajectory.add_tool_result(json!({
                                "status": "compact_advisory",
                                "message": format!("Compaction accepted ({} pressure). Will execute on next turn if needed.{}",
                                    match advisory.pressure_level {
                                        crate::context::lifecycle::PressureLevel::Critical => "critical",
                                        crate::context::lifecycle::PressureLevel::High => "high",
                                        crate::context::lifecycle::PressureLevel::Moderate => "moderate",
                                        crate::context::lifecycle::PressureLevel::Low => "low",
                                    },
                                    guidance_suffix)
                            }), 50, ti, ToolMessageMeta::default());
                        } else {
                            let msg = advisory.guidance.unwrap_or_else(|| "Compact rejected: pressure too low.".into());
                            self.trajectory.add_tool_result(json!({
                                "status": "compact_rejected",
                                "message": msg,
                            }), 50, ti, ToolMessageMeta::default());
                        }
                        self.emit_event(CoreEvent::ToolCallFinished {
                            agent_id: self.id,
                            result: json!({ "status": if advisory.allowed { "compact_advisory" } else { "compact_rejected" } }),
                        })?;
                    } else {
                        // Output budget enforcement: write large tool results to disk.
                        let mut result = if tool_name == "bash" || tool_name == "read" {
                            let om = self.output_manager.lock().unwrap();
                            om.enforce_budget(result, &tool_name)
                        } else {
                            result
                        };

                        // Apply read file formatting: hash-anchored lines, unchanged detection
                        if tool_name == "read" && result.get("_read_raw").is_some() {
                            // Pre-increment read count so auto-expand logic sees correct count
                            if let Some(p) = result.get("path").and_then(|v| v.as_str()) {
                                self.file_context.pre_increment_read(p);
                            }
                            // Also pre-increment for multi-file reads
                            if let Some(results) = result.get("results").and_then(|v| v.as_array()) {
                                for r in results {
                                    if let Some(p) = r.get("path").and_then(|v| v.as_str()) {
                                        self.file_context.pre_increment_read(p);
                                    }
                                }
                            }
                            result = self.format_read_result(&result);
                        }

                        let estimated_tokens = self.estimator.count_text(&result.to_string());

                        // Store fresh results as-is. Compaction/distillation only
                        // applies to older messages during context construction.
                        let meta = ToolMessageMeta {
                            tool_name: tool_name.clone(),
                            paths_read: if tool_name == "read" {
                                path_arg.iter().cloned().collect()
                            } else { Vec::new() },
                            paths_written: if matches!(tool_name.as_str(), "write" | "edit") {
                                path_arg.iter().cloned().collect()
                            } else { Vec::new() },
                            is_compacted: false,
                            artifact_ref: None,
                        };
                        let safe_result_str = crate::util::secrets::redact_secrets(&result.to_string());
                        let safe_result: serde_json::Value = serde_json::from_str(&safe_result_str).unwrap_or_else(|_| json!(safe_result_str));
                        if safe_result_str != result.to_string() { self.metrics.inc_redaction(); }
                        self.trajectory.add_tool_result(safe_result, estimated_tokens, ti, meta);
                        self.emit_event(CoreEvent::ToolCallFinished {
                            agent_id: self.id,
                            result,
                        })?;
                    }
                }
                Err(e) => {
                    let safe_error = crate::util::secrets::redact_secrets(&e.to_string());
                    if safe_error != e.to_string() { self.metrics.inc_redaction(); }
                    let error_msg = json!({ "error": safe_error });
                    self.trajectory.add_tool_result(error_msg.clone(), 50, ti, ToolMessageMeta::default());
                    self.emit_event(CoreEvent::ToolCallFinished {
                        agent_id: self.id,
                        result: error_msg,
                    })?;
                }
            }
        }

        // Record turn metrics for lifecycle evaluation
        self.turn_counter += 1;
        let total_tokens = self.trajectory.get_total_tokens();
        let tool_result_tokens: usize = self.trajectory.messages.iter()
            .filter(|m| matches!(m.role, Role::Tool))
            .map(|m| m.tokens)
            .sum();
        let stale_read_count = self.file_context.files_read.iter()
            .filter(|(_, s)| s.edited_since_read)
            .count();
        let tool_call_records: Vec<ToolCallRecord> = tools.iter().map(|t| {
            let args_str = serde_json::to_string(&t.args).unwrap_or_default();
            ToolCallRecord {
                tool_name: t.name.clone(),
                args_hash: {
                    let mut h: u64 = 0xcbf29ce484222325;
                    for b in args_str.bytes() {
                        h ^= b as u64;
                        h = h.wrapping_mul(0x100000001b3);
                    }
                    h
                },
            }
        }).collect();
        self.lifecycle.metrics_mut().record_turn(TurnMetrics {
            total_tokens,
            tool_result_tokens,
            active_message_count: self.trajectory.messages.len(),
            stale_read_count,
            tool_calls: tool_call_records,
        });

        Ok(TurnOutcome::Continue { tools_used: tools.len() })
    }

    /// Optional distiller enrichment of a summary. Returns (enriched_text, critical_files).
    async fn enrich_with_distiller(
        &self,
        fallback: &str,
        recent_assistant: Vec<String>,
        file_summary: &str,
    ) -> (String, Vec<String>) {
        if let Some(distiller_arc) = &self.distiller {
            let distiller = distiller_arc.read().await;
            let source_ids: Vec<Uuid> = self.trajectory.messages.iter().rev().take(20).map(|m| m.id).collect();
            let input = crate::context::distiller::TaskStateInput {
                recent_assistant_summaries: recent_assistant,
                file_context_summary: file_summary.to_string(),
                key_observations: Vec::new(),
                source_event_ids: source_ids,
            };
            let result = distiller.consolidate_task_state(input).await;
            match result.provenance.source {
                DistillerSource::Model => {
                    let mut e = result.output.enriched_summary;
                    if !result.output.open_subgoals.is_empty() {
                        e.push_str(&format!("\n\nOpen subgoals:\n{}",
                            result.output.open_subgoals.iter().map(|g| format!("- {}", g)).collect::<Vec<_>>().join("\n")));
                    }
                    if !result.output.decisions.is_empty() {
                        e.push_str(&format!("\n\nDecisions made:\n{}",
                            result.output.decisions.iter().map(|d| format!("- {}", d)).collect::<Vec<_>>().join("\n")));
                    }
                    if !result.output.critical_files.is_empty() {
                        e.push_str(&format!("\n\nCritical files: {}", result.output.critical_files.join(", ")));
                    }
                    (e, result.output.critical_files)
                }
                DistillerSource::DeterministicFallback => (fallback.to_string(), Vec::new()),
            }
        } else {
            (fallback.to_string(), Vec::new())
        }
    }

    /// Build checkpoint from trajectory messages, truncate, emit event.
    async fn finalize_compaction(&mut self, progress_summary: String, continuation: String) -> Result<()> {
        let msg_count = self.trajectory.messages.len();
        let mut latest_failures: Vec<String> = Vec::new();
        for msg in &self.trajectory.messages {
            // Collect last 5 tool error messages
            if matches!(msg.role, Role::Tool) {
                let content = msg.content.to_string();
                let lower = content.to_lowercase();
                if lower.contains("error") || lower.contains("failed") || lower.contains("fatal") {
                    if latest_failures.len() < 5 {
                        let truncated = if content.len() > 200 {
                            safe_truncate(&content, 200).into_owned()
                        } else {
                            content
                        };
                        latest_failures.push(truncated);
                    }
                }
            }
        }

        // Extract thematic tags from active constraints
        let thematic_tags = extract_thematic_tags(&self.task_reducer.to_critical_summary());

        // Collect modified files
        let modified_files: Vec<crate::context::distiller::schemas::FileChange> =
            self.file_context.files_edited.iter()
                .map(|p| crate::context::distiller::schemas::FileChange {
                    path: p.clone(),
                    change_description: String::new(),
                })
                .collect();

        // Extract risks from active constraints
        let risks = self.task_reducer.state.active_constraints.clone();

        let checkpoint = Some(crate::context::distiller::schemas::Checkpoint {
            progress_summary,
            completed: Vec::new(),
            remaining: Vec::new(),
            risks,
            modified_files,
            artifact_refs: Vec::new(),
            latest_failures,
            decisions: Vec::new(),
            abandoned_approaches: Vec::new(),
            thematic_tags,
            source_event_range: Some(format!("0..{}", msg_count)),
        });

        self.trajectory.truncate_with_continuation(continuation, checkpoint);

        self.emit_event(CoreEvent::ContextCompacted {
            agent_id: self.id,
            remaining_tokens: self.trajectory.get_total_tokens(),
        })?;
        Ok(())
    }

    /// Runtime-owned compaction — builds a deterministic summary and truncates
    /// without requiring the model to call the compact tool.
    async fn perform_runtime_compaction(&mut self) -> Result<()> {
        let task_summary = self.task_reducer.to_critical_summary();
        let file_summary = self.file_context.get_summary();

        let recent_assistant: Vec<String> = self.trajectory.messages.iter()
            .rev()
            .filter(|m| matches!(m.role, Role::Assistant))
            .take(5)
            .map(|m| {
                let s = m.content.to_string();
                safe_truncate(&s, 300).into_owned()
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let mut summary_parts = vec![task_summary];
        if !file_summary.is_empty() {
            summary_parts.push(format!("File context:\n{}", file_summary));
        }
        if !recent_assistant.is_empty() {
            summary_parts.push(format!("Recent progress:\n{}", recent_assistant.join("\n")));
        }

        let deterministic_summary = summary_parts.join("\n\n");

        let (enriched, _critical_files) = self.enrich_with_distiller(&deterministic_summary, recent_assistant, &file_summary).await;

        let continuation = ContextManager::continuation_prompt(&enriched);

        self.finalize_compaction(deterministic_summary, continuation).await
    }

    async fn perform_compaction(&mut self, summary: &str) -> Result<()> {
        let continuation_base = ContextManager::continuation_prompt(summary);

        let recent_summaries: Vec<String> = self.trajectory.messages.iter()
            .rev()
            .filter(|m| matches!(m.role, Role::Assistant))
            .take(5)
            .map(|m| m.content.to_string())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let file_summary = self.file_context.get_summary();
        let (enriched, _critical_files) = self.enrich_with_distiller(&continuation_base, recent_summaries, &file_summary).await;

        let continuation = enriched;

        self.finalize_compaction(summary.to_string(), continuation).await
    }

    fn emit_event(&self, event: CoreEvent) -> Result<()> {
        match serde_json::to_string(&event) {
            Ok(json) => {
                use std::io::Write;
                let stdout = std::io::stdout();
                let mut handle = stdout.lock();
                // Ignore write/flush errors (broken pipe) — the agent should continue running.
                let _ = writeln!(handle, "{}", json);
                let _ = handle.flush();
            }
            Err(e) => {
                eprintln!("[di-core] emit_event: serialization failed: {}", e);
            }
        }
        Ok(())
    }
}

pub struct MultiAgentOrchestrator {
    pub agents: HashMap<Uuid, AgentEngine>,
    pub frontend_channels: HashMap<Uuid, mpsc::Sender<FrontendMessage>>,
    /// Shared abort handles — survive agent removal so abort_agent works for running tasks.
    abort_handles: HashMap<Uuid, Arc<std::sync::atomic::AtomicBool>>,
    pub analyzer_daemon: Arc<tokio::sync::Mutex<ResilientDaemon>>,
    pub command_daemon: Arc<tokio::sync::Mutex<ResilientDaemon>>,
    pub gateway_client: Arc<GatewayStreamClient>,
    /// Default provider config for new agents (set by frontend via SetProviderConfig).
    pub default_provider_config: Option<crate::daemons::ProviderConfig>,
    /// Distiller-specific provider config (separate model/temperature).
    pub distiller_config: Option<crate::daemons::ProviderConfig>,
    /// Shared distiller instance (boxed trait object).
    pub distiller: Option<std::sync::Arc<tokio::sync::RwLock<Box<dyn ContextDistiller>>>>,
    /// Global distiller metrics — not borrowed from any agent.
    distiller_metrics: Arc<ContextMetrics>,
    /// Shared runtime config — one per agent, updated by orchestrator, read by agent each turn.
    runtime_configs: HashMap<Uuid, RuntimeConfig>,
}

/// Shared runtime config for a single agent. Orchestrator writes, agent reads.
struct RuntimeConfig {
    provider_config: Arc<tokio::sync::RwLock<Option<crate::daemons::ProviderConfig>>>,
    distiller: Arc<tokio::sync::RwLock<Option<std::sync::Arc<tokio::sync::RwLock<Box<dyn ContextDistiller>>>>>>,
    timeout_ms: Arc<std::sync::Mutex<Option<u64>>>,
}

impl MultiAgentOrchestrator {
    pub fn new(analyzer_daemon: ResilientDaemon, command_daemon: ResilientDaemon, gateway_socket: &str) -> Self {
        Self {
            agents: HashMap::new(),
            frontend_channels: HashMap::new(),
            abort_handles: HashMap::new(),
            analyzer_daemon: Arc::new(tokio::sync::Mutex::new(analyzer_daemon)),
            command_daemon: Arc::new(tokio::sync::Mutex::new(command_daemon)),
            gateway_client: Arc::new(GatewayStreamClient::with_socket(gateway_socket)),
            default_provider_config: None,
            distiller_config: None,
            distiller: None,
            distiller_metrics: ContextMetrics::new(),
            runtime_configs: HashMap::new(),
        }
    }

    pub fn spawn_agent(&mut self, _task: String) -> Uuid {
        let id = Uuid::new_v4();
        let mut agent = AgentEngine::new(
            id,
            self.analyzer_daemon.clone(),
            self.command_daemon.clone(),
            self.gateway_client.clone(),
        );
        agent.provider_config = self.default_provider_config.clone();
        agent.distiller = self.distiller.clone();

        // Wire shared runtime config so orchestrator can push updates to running agents
        let rc = RuntimeConfig {
            provider_config: agent.shared_provider_config.clone(),
            distiller: agent.shared_distiller.clone(),
            timeout_ms: agent.shared_timeout_ms.clone(),
        };
        self.runtime_configs.insert(id, rc);

        // Store the sender for routing frontend messages to this agent
        self.frontend_channels.insert(id, agent.frontend_tx.clone());
        // Store abort handle so abort_agent works after the agent is moved out
        self.abort_handles.insert(id, agent.abort.clone());
        self.agents.insert(id, agent);
        id
    }

    /// Route a frontend message (approval, followup answer, user response) to the agent's channel.
    /// Returns true if the message was sent successfully.
    /// Uses try_send to avoid blocking the main loop if the agent's channel is full.
    pub fn send_to_agent(&self, agent_id: Uuid, msg: FrontendMessage) -> bool {
        if let Some(tx) = self.frontend_channels.get(&agent_id) {
            match tx.try_send(msg) {
                Ok(()) => true,
                Err(tokio::sync::mpsc::error::TrySendError::Full(msg)) => {
                    eprintln!("[di-core] send_to_agent: channel full for agent {}, dropping {:?}", agent_id, msg);
                    false
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => false,
            }
        } else {
            eprintln!("[di-core] send_to_agent: no channel for agent {}", agent_id);
            false
        }
    }

    /// Clean up the frontend channel and abort handle for a finished agent.
    pub fn cleanup_agent(&mut self, agent_id: &Uuid) {
        self.frontend_channels.remove(agent_id);
        self.abort_handles.remove(agent_id);
        self.runtime_configs.remove(agent_id);
    }

    /// Update the frontend response timeout for all agents.
    pub fn set_all_frontend_timeouts(&mut self, duration_ms: u64) {
        for agent in self.agents.values_mut() {
            agent.frontend_timeout_ms = Some(duration_ms);
        }
        // Also update running agents via shared config
        for rc in self.runtime_configs.values() {
            if let Ok(mut guard) = rc.timeout_ms.lock() {
                *guard = Some(duration_ms);
            }
        }
    }

    /// Set the provider config from the frontend. Stored as default for new agents
    /// and applied to all running agents immediately.
    pub fn set_provider_config(&mut self, config: crate::daemons::ProviderConfig) {
        self.default_provider_config = Some(config.clone());
        for agent in self.agents.values_mut() {
            agent.provider_config = Some(config.clone());
        }
        // Also update running agents via shared config
        for rc in self.runtime_configs.values() {
            if let Ok(mut guard) = rc.provider_config.try_write() {
                *guard = Some(config.clone());
            }
        }
    }

    /// Set the distiller config and create the distiller instance.
    /// Shares the distiller with all running and future agents.
    pub fn set_distiller_config(&mut self, config: crate::daemons::ProviderConfig) {
        self.distiller_config = Some(config.clone());
        let distiller = crate::context::distiller::new_distiller(
            Some(config),
            self.gateway_client.clone(),
            Some(self.distiller_metrics.clone()),
            None,
        );
        let distiller_arc = std::sync::Arc::new(tokio::sync::RwLock::new(distiller));
        self.distiller = Some(distiller_arc.clone());
        for agent in self.agents.values_mut() {
            agent.distiller = Some(distiller_arc.clone());
        }
        // Also update running agents via shared config
        for rc in self.runtime_configs.values() {
            if let Ok(mut guard) = rc.distiller.try_write() {
                *guard = Some(distiller_arc.clone());
            }
        }
    }

    pub fn abort_agent(&mut self, agent_id: Uuid) -> bool {
        // Check agents map first (agent not yet spawned), then abort_handles (agent running)
        if let Some(agent) = self.agents.get(&agent_id) {
            agent.request_abort();
            true
        } else if let Some(abort_flag) = self.abort_handles.get(&agent_id) {
            abort_flag.store(true, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn remove_agent(&mut self, agent_id: Uuid) -> Option<AgentEngine> {
        // Do NOT remove from frontend_channels — the sender must stay registered
        // so that send_to_agent can route approval/followup/user messages to the
        // spawned task. The channel stays open until the agent is dropped.
        self.agents.remove(&agent_id)
    }

    pub async fn handle_user_response(&self, agent_id: Uuid, text: String) -> Result<()> {
        self.send_to_agent(agent_id, FrontendMessage::UserResponse { agent_id, text });
        Ok(())
    }

    pub fn emit_event(&self, event: CoreEvent) -> Result<()> {
        match serde_json::to_string(&event) {
            Ok(json) => {
                use std::io::Write;
                let stdout = std::io::stdout();
                let mut handle = stdout.lock();
                let _ = writeln!(handle, "{}", json);
                let _ = handle.flush();
            }
            Err(e) => eprintln!("[di-core] emit_event: serialization failed: {}", e),
        }
        Ok(())
    }
}

/// Extract short thematic tags from the task state summary by keyword matching.
fn extract_thematic_tags(summary: &str) -> Vec<String> {
    let lower = summary.to_lowercase();
    let mut tags = Vec::new();
    let keywords = [
        ("refactor", "refactor"), ("test", "testing"), ("debug", "debugging"),
        ("security", "security"), ("performance", "performance"), ("api", "api"),
        ("database", "database"), ("config", "configuration"), ("deploy", "deployment"),
        ("error", "error-handling"), ("auth", "authentication"), ("parse", "parsing"),
        ("build", "build-system"), ("ui", "ui"), ("cli", "cli"),
    ];
    for (keyword, tag) in &keywords {
        if lower.contains(keyword) {
            tags.push(tag.to_string());
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_hash_from_text tests (static method, no engine needed) ---

    #[test]
    fn extract_hash_from_single_file_result() {
        let text = "[File Hash: a1b2c3d4]\n   1 │ abc|fn main() {";
        assert_eq!(AgentEngine::extract_hash_from_text(text), Some("a1b2c3d4".to_string()));
    }

    #[test]
    fn extract_hash_from_range_result() {
        let text = "[Lines 10-20, Hash: deadbeef]\n  10 │ xyz|code here";
        assert_eq!(AgentEngine::extract_hash_from_text(text), Some("deadbeef".to_string()));
    }

    #[test]
    fn extract_hash_from_unchanged_result() {
        let text = "[Full file: unchanged since your last read (Hash: abcdef12)]";
        assert_eq!(AgentEngine::extract_hash_from_text(text), Some("abcdef12".to_string()));
    }

    #[test]
    fn extract_hash_no_hash_returns_none() {
        let text = "No file hash here, just plain text";
        assert_eq!(AgentEngine::extract_hash_from_text(text), None);
    }

    #[test]
    fn extract_hash_non_hex_returns_none() {
        let text = "[File Hash: xyz!]";
        assert_eq!(AgentEngine::extract_hash_from_text(text), None);
    }

    // --- format_at_detail tests (static method, no engine needed) ---

    #[test]
    fn format_at_detail_full() {
        let raw = json!({
            "path": "test.rs",
            "detail": "full",
            "content": "fn main() {}\n",
            "lines": 2,
        });
        let mut cache = crate::tools::read_file::ReadFileCache::new();
        let ctx = FileContextTracker::new();
        let result = AgentEngine::format_at_detail(&raw, "full", &mut cache, &ctx, None);
        let s = result.as_str().unwrap();
        assert!(s.contains("[File Hash:"));
        assert!(s.contains("fn main()"));
    }

    #[test]
    fn format_at_detail_preview() {
        let content: String = (0..500).map(|i| format!("line {}\n", i)).collect();
        let raw = json!({
            "path": "big.rs",
            "detail": "preview",
            "content": content,
            "lines": 500,
        });
        let mut cache = crate::tools::read_file::ReadFileCache::new();
        let ctx = FileContextTracker::new();
        let result = AgentEngine::format_at_detail(&raw, "preview", &mut cache, &ctx, None);
        let s = result.as_str().unwrap();
        assert!(s.contains("Showing first 200 lines"));
    }

    #[test]
    fn format_at_detail_outline_with_analyzer_data() {
        let raw = json!({
            "path": "test.rs",
            "detail": "outline",
            "analyzer_data": {
                "symbols": [
                    {"name": "main", "kind": "function", "handle": "fn:main", "start_line": 1, "end_line": 3, "signature": "fn main()"}
                ]
            },
        });
        let mut cache = crate::tools::read_file::ReadFileCache::new();
        let ctx = FileContextTracker::new();
        let result = AgentEngine::format_at_detail(&raw, "outline", &mut cache, &ctx, None);
        let s = result.as_str().unwrap();
        assert!(s.contains("[File Hash:"));
        assert!(s.contains("fn main()"));
        assert!(s.contains("lines 1-3"));
    }

    #[test]
    fn format_at_detail_hint_with_analyzer_data() {
        let raw = json!({
            "path": "test.rs",
            "detail": "hint",
            "analyzer_data": {
                "symbols": [
                    {"name": "main", "kind": "function", "handle": "fn:main", "start_line": 1, "end_line": 3, "signature": ""},
                    {"name": "Config", "kind": "struct", "handle": "st:Config", "start_line": 5, "end_line": 10, "signature": ""}
                ]
            },
        });
        let mut cache = crate::tools::read_file::ReadFileCache::new();
        let ctx = FileContextTracker::new();
        let result = AgentEngine::format_at_detail(&raw, "hint", &mut cache, &ctx, None);
        let s = result.as_str().unwrap();
        assert!(s.contains("function main"));
        assert!(s.contains("struct Config"));
        // Hint should NOT have line numbers
        assert!(!s.contains("lines 1-3"));
    }

    #[test]
    fn format_at_detail_skeleton_with_analyzer_data() {
        let raw = json!({
            "path": "test.rs",
            "detail": "skeleton",
            "analyzer_data": {
                "skeleton": "fn main() {\n    // ...\n}\n"
            },
        });
        let mut cache = crate::tools::read_file::ReadFileCache::new();
        let ctx = FileContextTracker::new();
        let result = AgentEngine::format_at_detail(&raw, "skeleton", &mut cache, &ctx, None);
        let s = result.as_str().unwrap();
        assert!(s.contains("[File Hash:"));
        assert!(s.contains("fn main()"));
    }

    // --- Budget degradation cascade test ---

    #[test]
    fn degradation_cascade_walks_full_to_preview() {
        // Create a large file that exceeds the 24000 char budget when rendered as full
        let content: String = (0..5000).map(|i| format!("line number {} with some content to make it longer\n", i)).collect();
        let raw = json!({
            "path": "big.rs",
            "detail": "full",
            "content": content,
            "lines": 5000,
        });

        let mut cache = crate::tools::read_file::ReadFileCache::new();
        let ctx = FileContextTracker::new();

        // Format as full — will exceed budget
        let full_result = AgentEngine::format_at_detail(&raw, "full", &mut cache, &ctx, None);
        let full_str = full_result.as_str().unwrap();
        assert!(full_str.len() > 24000, "Full result ({}) should exceed 24000 budget", full_str.len());

        // Now walk the degradation path manually (simulating apply_budget_degradation)
        const MAX_OUTPUT_CHARS: usize = 24000;
        const DEGRADATION_PATH: &[&str] = &["full", "preview", "skeleton", "outline", "hint"];

        let mut found_fitting = false;
        for try_idx in 1..DEGRADATION_PATH.len() {
            let degraded = AgentEngine::format_at_detail(&raw, DEGRADATION_PATH[try_idx], &mut cache, &ctx, None);
            if let serde_json::Value::String(s) = &degraded {
                if s.len() <= MAX_OUTPUT_CHARS {
                    // Preview should fit since it only shows 200 lines
                    assert_eq!(DEGRADATION_PATH[try_idx], "preview");
                    assert!(s.contains("line number 0"));
                    found_fitting = true;
                    break;
                }
            }
        }
        assert!(found_fitting, "Should find a fitting detail level in the cascade");
    }

    // --- extract_last_known_hash against trajectory ---

    #[test]
    fn extract_last_known_hash_finds_recent_read() {
        let mut traj = Trajectory::new();

        let mut tool_meta = ToolMessageMeta::default();
        tool_meta.tool_name = "read".to_string();
        tool_meta.paths_read = vec!["src/main.rs".to_string()];
        let msg = Message {
            id: Uuid::new_v4(),
            role: Role::Tool,
            content: serde_json::Value::String("[File Hash: abcdef12]\n   1 │ abc|fn main() {".to_string()),
            timestamp: chrono::Utc::now(),
            tokens: 50,
            is_compressed: false,
            tool_meta,
            tool_calls: vec![],
            tool_call_id: Some("call_1".to_string()),
            thinking: None,
        };
        traj.messages.push(msg);

        // Test extraction by iterating messages directly (same logic as extract_last_known_hash)
        let target = "src/main.rs";
        let mut found: Option<String> = None;
        for msg in traj.messages.iter().rev() {
            if msg.role != Role::Tool { continue; }
            if msg.tool_meta.tool_name != "read" { continue; }
            if !msg.tool_meta.paths_read.iter().any(|p| p.trim_start_matches("./") == target.trim_start_matches("./")) { continue; }
            if let Some(s) = msg.content.as_str() {
                found = AgentEngine::extract_hash_from_text(s);
                if found.is_some() { break; }
            }
        }
        assert_eq!(found, Some("abcdef12".to_string()));
    }

    #[test]
    fn extract_last_known_hash_multi_file_dividers() {
        let mut traj = Trajectory::new();

        let multi_content = "--- src/main.rs ---\n[File Hash: aaaa1111]\n   1 │ abc|fn main() {\n\n--- src/lib.rs ---\n[File Hash: bbbb2222]\n   1 │ def|pub fn lib()";
        let mut tool_meta = ToolMessageMeta::default();
        tool_meta.tool_name = "read".to_string();
        tool_meta.paths_read = vec!["src/main.rs".to_string(), "src/lib.rs".to_string()];
        let msg = Message {
            id: Uuid::new_v4(),
            role: Role::Tool,
            content: serde_json::Value::String(multi_content.to_string()),
            timestamp: chrono::Utc::now(),
            tokens: 100,
            is_compressed: false,
            tool_meta,
            tool_calls: vec![],
            tool_call_id: Some("call_1".to_string()),
            thinking: None,
        };
        traj.messages.push(msg);

        // Test the divider-isolation logic from extract_last_known_hash
        let text = traj.messages.last().unwrap().content.as_str().unwrap();
        // For src/lib.rs, should isolate the section after "--- src/lib.rs ---"
        let divider = "--- src/lib.rs ---";
        let idx = text.find(divider).unwrap();
        let section = &text[idx + divider.len()..];
        let end = section.find("\n--- ").unwrap_or(section.len());
        let section = &section[..end];
        let hash = AgentEngine::extract_hash_from_text(section);
        assert_eq!(hash, Some("bbbb2222".to_string()));

        // For src/main.rs
        let divider = "--- src/main.rs ---";
        let idx = text.find(divider).unwrap();
        let section = &text[idx + divider.len()..];
        let end = section.find("\n--- ").unwrap_or(section.len());
        let section = &section[..end];
        let hash = AgentEngine::extract_hash_from_text(section);
        assert_eq!(hash, Some("aaaa1111".to_string()));
    }
}
