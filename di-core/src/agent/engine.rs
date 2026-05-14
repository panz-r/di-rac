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
use std::sync::LazyLock;
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
        let boundary = s.floor_char_boundary(max_len);
        std::borrow::Cow::Owned(format!("{}...", &s[..boundary]))
    }
}

/// Wrap a tool result in the compact pipe-delimited envelope format
/// matching the TS ToolResultUtils.wrapInEnvelope behavior.
///
/// Format: `OK | tokens:N | lines:N | cached:yes/no` header followed by content.
/// Also handles ERROR, TRUNCATED, and EMPTY variants.
fn wrap_in_envelope(content: &str, tool_name: &str, is_cached: bool, cumulative_tokens: usize, read_count: usize) -> String {
    let trimmed = content.trim_start();

    // Skip if already wrapped
    if trimmed.starts_with("OK |") || trimmed.starts_with("ERROR |") || trimmed.starts_with("TRUNCATED |") || trimmed.starts_with("EMPTY |") {
        return content.to_string();
    }

    // Skip structured JSON with status/ok fields
    if trimmed.starts_with('{') {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if parsed.get("status").is_some() || parsed.get("ok").is_some() {
                return content.to_string();
            }
        }
    }

    let lines = content.lines().count();
    let tokens = (content.len() + 3) / 4; // ~4 chars per token
    let is_truncated = content.contains("[truncated]") || content.contains("[Content reduced");
    let is_error = trimmed.starts_with("<tool_error");
    let is_empty = trimmed.is_empty()
        || trimmed == "No definitions found."
        || trimmed == "No matches found."
        || trimmed == "No results found."
        || trimmed.starts_with("Found 0 results");

    let cached_field = if is_cached { " | cached:yes" } else { "" };

    if is_error {
        // Extract message from <tool_error> tag
        static SEVERITY_RE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r#"severity="[^"]*""#).unwrap());
        let body = trimmed.replace("<tool_error>", "").replace("</tool_error>", "");
        let clean = SEVERITY_RE.replace(&body, "").trim().to_string();
        let code = if clean.contains("not found") || clean.contains("could not be found") { "ENOENT" }
            else if clean.contains("permission") || clean.contains("blocked") { "EPERM" }
            else if clean.contains("locked") { "ELOCK" }
            else if clean.contains("anchor") { "ANCHOR_MISS" }
            else if clean.contains("argument") || clean.contains("parameter") { "EINVAL" }
            else { "TOOL_ERROR" };
        let msg = if clean.len() > 300 { format!("{}...", &clean[..clean.floor_char_boundary(300)]) } else { clean.clone() };
        let hint = get_error_hint(code, &msg);
        let hint_truncated = if hint.len() > 200 { &hint[..hint.floor_char_boundary(200)] } else { hint };
        return format!("ERROR | {} | {} | hint:{} | tokens:{}", code, msg, hint_truncated, tokens);
    }

    let cumulative_field = format!(" | cumulative:{}", cumulative_tokens);

    if is_truncated {
        let hint = "Output truncated. Use --range or --detail for targeted reads.";
        return format!("TRUNCATED | lines:{} | hint:{} | tokens:{}{}{}\n{}", lines, hint, tokens, cached_field, cumulative_field, content);
    }

    if is_empty {
        let hint = match tool_name {
            "read" => "No definitions found. File may be empty or unsupported type.",
            "search" => "No matches. Try broader pattern, different path, or --context for surrounding lines.",
            "symbols" => "No symbol matches. Try different pattern, --kind function, or use search for text patterns.",
            "repo" => "No results. Try different path or --detail files.",
            _ => "No results found. Try different parameters.",
        };
        return format!("EMPTY | hint:{} | tokens:{}{}{}", hint, tokens, cached_field, cumulative_field);
    }

    // Normal success
    let lines_field = if lines > 1 { format!(" | lines:{}", lines) } else { String::new() };
    let reads_field = if read_count > 0 { format!(" | reads:{}", read_count) } else { String::new() };
    format!("OK | tokens:{}{}{}{}{}\n{}", tokens, lines_field, cached_field, cumulative_field, reads_field, content)
}

/// Error-specific hints matching TS ToolHints.getErrorHint.
/// Returns a suggestion string for the given error code or message content.
fn get_error_hint(code: &str, message: &str) -> &'static str {
    match code {
        "ENOENT" | "io.file.notFound" => "File not found. Try: repo <parent-dir> to list files, or search --pattern <name> to find it.",
        "EPERM" | "io.file.permissionDenied" => "Permission denied. Check file permissions or use a different path.",
        "ELOCK" | "io.file.locked" => "File locked by another process. Wait and retry.",
        "ANCHOR_MISS" | "anchor.notFound" => "Anchor not found. Re-read the file (read --detail outline) to get fresh anchors.",
        "anchor.contentMismatch" => "Content at anchor has changed. Re-read the file before editing.",
        "anchor.invalidFormat" => "Invalid anchor format. Use hash-anchored lines from read --detail outline.",
        "edit.multiFileConflict" => "Multi-file conflict. Edit each file separately.",
        "EINVAL" | "validation.missingArgument" | "validation.invalidInput" => "Invalid argument. Check parameter types and retry.",
        "lsp.timeout" => "Language server timed out. Retry or use a non-AST approach.",
        "lsp.connectionLost" => "Language server connection lost. Retry — it may recover.",
        "tool.internalError" => "Internal error. Retry once, or try an alternative approach.",
        _ => {
            if message.contains("not found") { "File not found. Try: repo <parent-dir> to list files, or search --pattern <name>." }
            else if message.contains("anchor") { "Anchor not found. Re-read the file (read --detail outline) to get fresh anchors." }
            else { "Tool execution failed. Try a different approach or check your inputs." }
        }
    }
}

/// Short description for tool results, matching TS handler.getDescription().
fn tool_description(tool_name: &str) -> &'static str {
    match tool_name {
        "read" => "Read file",
        "write" => "Write file",
        "edit" => "Edit file",
        "bash" => "Bash command",
        "search" => "Search files",
        "repo" => "Repo listing",
        "symbols" => "Symbols",
        "compact" => "Compact",
        "ask" => "Ask",
        "done" => "Done",
        "plan" => "Plan",
        "task" => "Task",
        "tools" => "Tool search",
        "get_outputs" => "Get outputs",
        _ => "Tool",
    }
}

/// Score tool call ambiguity (0.0–1.0) matching TS AmbiguityScorer.
/// At >0.4: append guidance hint. At >0.6: retry recommended.
fn score_ambiguity(
    tool_name: &str,
    args: &serde_json::Value,
    file_context: &crate::agent::file_context::FileContextTracker,
) -> f64 {
    let mut score = 0.0f64;
    match tool_name {
        "read" => {
            let has_range = args.get("range").or(args.get("start_line")).is_some();
            let has_detail = args.get("detail").is_some();
            let path = args.get("path").and_then(|v| v.as_str());
            if !has_range && !has_detail {
                if let Some(p) = path {
                    let read_count = file_context.files_read.get(p).map(|s| s.read_count).unwrap_or(0);
                    if read_count > 3 {
                        score += 0.3;
                    }
                }
            }
        }
        "search" => {
            let pattern = args.get("pattern")
                .or_else(|| args.get("regex"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if pattern.is_empty() {
                score += 0.5;
            } else if pattern.len() <= 2 {
                score += 0.3;
            }
        }
        "edit" => {
            let has_anchor = args.get("anchor").is_some();
            if !has_anchor {
                score += 0.4;
            }
            let path = args.get("path").and_then(|v| v.as_str());
            if let Some(p) = path {
                if !file_context.files_read.contains_key(p) {
                    score += 0.2;
                }
            }
        }
        "write" => {
            let has_content = args.get("content").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false);
            if !has_content {
                score += 0.4;
            }
        }
        _ => {}
    }
    score.min(1.0)
}

/// Build exploration hints matching TS ToolHints.getSuccessHint.
fn build_exploration_hint(tool_name: &str, path: Option<&str>) -> Option<String> {
    match tool_name {
        "read" => {
            let p = path.unwrap_or("<path>");
            Some(format!("\n---\nFollow-up: symbols {} --action expand --symbol <handle> | read {} --detail outline", p, p))
        }
        "search" => {
            Some("\n---\nFollow-up: read <matched-path> | repo <matched-path>".to_string())
        }
        "repo" => {
            Some("\n---\nFollow-up: read <path> | symbols search --name <query> | search <path> --pattern <regex>".to_string())
        }
        "symbols" => {
            Some("\n---\nFollow-up: symbols <path> --action expand --symbol <handle> | symbols <path> refs --name <name>".to_string())
        }
        _ => None,
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
    pub write_missing_content_count: usize,
    pub circuit_breakers: crate::agent::recovery::CircuitBreakerRegistry,
    pub stagnation_detector: std::sync::Mutex<crate::agent::recovery::StagnationDetector>,
    pub recovery_telemetry: std::sync::Arc<crate::agent::recovery::RecoveryTelemetry>,
    /// Per-file consecutive edit count for streak breaker.
    pub edit_streaks: HashMap<String, usize>,
    /// Total net lines changed this session (for budget guard).
    pub net_change_lines: isize,
    /// Files edited in current turn (for overlapping edit detection).
    pub turn_edits: HashSet<String>,
    /// Bash execution history: (id, command, exit_code).
    pub bash_history: Vec<(String, String, i32)>,
    /// Whether the last LLM output was truncated (hit MAX_TOKENS).
    pub last_output_truncated: bool,
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
    /// Files to re-read into context after compaction completes.
    pending_compact_required_files: Vec<String>,
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
            observer: Observer::new(crate::observer::ObserverConfig::default()),
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
            max_consecutive_mistakes: 5,
            write_missing_content_count: 0,
            circuit_breakers: crate::agent::recovery::CircuitBreakerRegistry::new(),
            stagnation_detector: std::sync::Mutex::new(crate::agent::recovery::StagnationDetector::new()),
            recovery_telemetry: std::sync::Arc::new(crate::agent::recovery::RecoveryTelemetry::new()),
            edit_streaks: HashMap::new(),
            net_change_lines: 0,
            turn_edits: HashSet::new(),
            bash_history: Vec::new(),
            last_output_truncated: false,
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
            pending_compact_required_files: Vec::new(),
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
                }).await?;
                return Ok(());
            }

            let outcome = match self.run_turn().await {
                Ok(o) => o,
                Err(e) => {
                    let err_msg = format!("{}", e);
                    // Gateway sends structured code "CONTEXT_EXCEEDED" — match on that.
                    if err_msg.starts_with("CONTEXT_EXCEEDED:") {
                        eprintln!("[di-core] context window exceeded, triggering hard compaction");
                        match self.perform_runtime_compaction().await {
                            Ok(()) => {
                                self.lifecycle.notify_compaction_complete();
                                self.trajectory.add_message(
                                    Role::User,
                                    serde_json::json!("Context was too long and has been compacted. Continue the task from where you left off."),
                                    20,
                                );
                                continue;
                            }
                            Err(ce) => {
                                self.emit_event(CoreEvent::TaskFinished {
                                    agent_id: self.id,
                                    success: false,
                                    message: format!("Context exceeded and compaction failed: {}", ce),
                                }).await?;
                                return Err(e);
                            }
                        }
                    }
                    self.emit_event(CoreEvent::TaskFinished {
                        agent_id: self.id,
                        success: false,
                        message: format!("Error: {}", e),
                    }).await?;
                    return Err(e);
                }
            };

            match outcome {
                TurnOutcome::Finished => {
                    self.observer.final_compression();
                    self.flush_observer_telemetry();
                    return Ok(());
                }
                TurnOutcome::Continue { tools_used: 0 } => {
                    self.consecutive_mistake_count += 1;
                    if self.consecutive_mistake_count >= self.max_consecutive_mistakes {
                        self.emit_event(CoreEvent::TaskFinished {
                            agent_id: self.id,
                            success: false,
                            message: "Too many consecutive turns without tool use".to_string(),
                        }).await?;
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

    // -----------------------------------------------------------------------
    // Pre-flight firewall
    // -----------------------------------------------------------------------

    /// Run pre-flight checks before tool execution. Returns Some(error_json) if
    /// the tool should be blocked, None if it should proceed.
    /// Returns (block_error, modified_args) where modified_args contains
    /// any auto-fixes applied to the tool arguments.
    fn run_preflight_firewall(
        &mut self,
        tool: &crate::tools::ToolCall,
    ) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
        let tool_name = tool.name.as_str();

        match tool_name {
            "read" => self.preflight_read(tool),
            "write" | "edit" => self.preflight_mutation(tool),
            "bash" => self.preflight_bash(tool),
            "done" => self.preflight_done(tool),
            _ => (None, None),
        }
    }

    fn preflight_read(&mut self, tool: &crate::tools::ToolCall) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
        // Paradoxical range auto-fix: swap start/end if inverted
        let start = tool.args.get("start_line").and_then(|v| v.as_u64());
        let end = tool.args.get("end_line").and_then(|v| v.as_u64());
        if let (Some(s), Some(e)) = (start, end) {
            if s > e && s >= 1 {
                let mut modified = tool.args.clone();
                if let Some(map) = modified.as_object_mut() {
                    map.insert("start_line".to_string(), json!(e));
                    map.insert("end_line".to_string(), json!(s));
                }
                return (None, Some(modified));
            }
        }

        (None, None)
    }

    fn preflight_mutation(&mut self, tool: &crate::tools::ToolCall) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
        let tool_name = tool.name.as_str();
        let path = tool.args.get("path").and_then(|v| v.as_str()).map(String::from);

        // Block mutations after truncated LLM output (arguments may be incomplete)
        if self.last_output_truncated {
            self.last_output_truncated = false;
            return (Some(json!({
                "status": "error",
                "error": "<tool_error severity=\"recoverable\">Previous output was truncated. Your arguments may be incomplete. Please retry the full operation.</tool_error>"
            })), None);
        }

        if let Some(ref path) = path {
            // Protected file locking
            let protected_suffixes = [
                "package-lock.json", "yarn.lock", "Cargo.lock", ".gitignore",
            ];
            let protected_patterns = [".generated.", ".min.", ".bundle."];
            let is_protected = protected_suffixes.iter().any(|s| path.ends_with(s))
                || protected_patterns.iter().any(|p| path.contains(p))
                || path.starts_with("vendor/");
            if is_protected {
                return (Some(json!({
                    "status": "error",
                    "error": format!("<tool_error severity=\"recoverable\">File '{}' is protected. Generated, lock, and vendor files should not be edited directly.</tool_error>", path)
                })), None);
            }

            // Truncation placeholder guard for write/edit content
            if let Some(content) = tool.args.get("content").and_then(|v| v.as_str()) {
                if crate::agent::recovery::detect_truncation(content) {
                    return (Some(json!({
                        "status": "error",
                        "error": "<tool_error severity=\"recoverable\">Content contains truncation placeholders (e.g. '... rest'). Provide complete content without placeholders.</tool_error>"
                    })), None);
                }
            }

            // Per-file edit streak breaker
            let streak = self.edit_streaks.entry(path.clone()).or_insert(0);
            *streak += 1;
            if *streak >= 5 {
                return (Some(json!({
                    "status": "error",
                    "error": format!("<tool_error severity=\"recoverable\">File '{}' has been edited {} times consecutively. Read the file first to verify its current state.</tool_error>", path, streak)
                })), None);
            }

            // Overlapping edit block: same file edited twice in current turn
            if tool_name == "edit" && self.turn_edits.contains(path.as_str()) {
                return (Some(json!({
                    "status": "error",
                    "error": format!("<tool_error severity=\"recoverable\">File '{}' was already edited this turn. Line numbers may have shifted. Re-read the file first.</tool_error>", path)
                })), None);
            }

            // Net-change budget check (500 lines)
            if self.net_change_lines.abs() > 500 {
                return (Some(json!({
                    "status": "error",
                    "error": format!("<tool_error severity=\"recoverable\">Session net change budget exceeded ({} lines). Break the task into smaller increments.</tool_error>", self.net_change_lines)
                })), None);
            }
        }

        (None, None)
    }

    fn preflight_bash(&mut self, tool: &crate::tools::ToolCall) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
        // Block mutations after truncated LLM output
        if self.last_output_truncated {
            self.last_output_truncated = false;
            return (Some(json!({
                "status": "error",
                "error": "<tool_error severity=\"recoverable\">Previous output was truncated. Your command may be incomplete. Please retry.</tool_error>"
            })), None);
        }

        // /tmp redirect: rewrite /tmp paths to .dirac-tmp/
        if let Some(cmd) = tool.args.get("command").and_then(|v| v.as_str()) {
            if cmd.contains("/tmp/") {
                let redirected = cmd.replace("/tmp/", ".dirac-tmp/");
                let mut modified = tool.args.clone();
                if let Some(map) = modified.as_object_mut() {
                    map.insert("command".to_string(), json!(redirected));
                }
                let _ = std::fs::create_dir_all(".dirac-tmp");
                return (None, Some(modified));
            }
        }

        (None, None)
    }

    fn preflight_done(&mut self, _tool: &crate::tools::ToolCall) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
        // Proof-of-execution completion gate: if files were edited this session,
        // check that at least one bash command succeeded after the last edit.
        if !self.file_context.files_edited.is_empty() && !self.bash_history.is_empty() {
            let last_bash = self.bash_history.last().unwrap();
            if last_bash.2 != 0 {
                return (Some(json!({
                    "status": "error",
                    "error": "<tool_error severity=\"recoverable\">Files were edited but no verification command succeeded. Run a test or build command to verify changes first.</tool_error>"
                })), None);
            }
        }

        (None, None)
    }

    /// Execute one turn of the agent loop.
    pub async fn run_turn(&mut self) -> Result<TurnOutcome> {
        debug_log!("[di-core] run_turn: agent {} starting, provider={:?}",
            self.id, self.provider_config.as_ref().map(|c| &c.id));
        eprintln!("[di-core] run_turn start: agent {} turn {}", self.id, self.turn_counter);

        // Update activity timestamp
        self.last_activity = std::time::Instant::now();
        let turn_start = std::time::Instant::now();

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
                skills: None,
                custom_instructions: None,
            };

            self.context_compiler = Some(ContextCompiler::new(&session));
            debug_log!("[di-core] run_turn: context compiler initialized, session prefix {} chars",
                self.context_compiler.as_ref().expect("context compiler initialized above").session_prefix_len());
        }

        // 1. API extraction via observer + analyzer daemon
        let mut current_apis: HashSet<String> = HashSet::new();
        let mut api_filter_response: Option<crate::observer::ExtractApisResponse> = None;
        if self.observer.config.enabled {
            if let Some(req) = self.observer.build_extract_apis_request(&self.trajectory) {
                let analyzer_req = crate::daemons::AnalyzerRequest {
                    command: "extract-apis".to_string(),
                    file: None,
                    content: Some(req.content.clone()),
                    language: req.language.clone(),
                    query: None,
                    subcommand: None,
                };
                match self.tool_executor.analyzer_daemon().lock().await.send_request::<_, crate::daemons::AnalyzerResponse>(analyzer_req).await {
                    Ok(resp) if resp.ok => {
                        if let Some(arr) = resp.data.get("calls").and_then(|v| v.as_array()) {
                            for call in arr {
                                if let Some(name) = call.as_str() {
                                    current_apis.insert(name.to_string());
                                }
                            }
                        }
                        if let Some(arr) = resp.data.get("definitions").and_then(|v| v.as_array()) {
                            for def in arr {
                                if let Some(name) = def.as_str() {
                                    current_apis.insert(name.to_string());
                                }
                            }
                        }
                        let calls = resp.data.get("calls")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        let definitions = resp.data.get("definitions")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        api_filter_response = Some(crate::observer::ExtractApisResponse { calls, definitions });
                    }
                    Ok(_) | Err(_) => {} // Analyzer unavailable — keep empty apis
                }
            }
        }

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
            // Re-read required files into context after compaction (max 8 files)
            let required_files: Vec<String> = self.pending_compact_required_files.drain(..).take(8).collect();
            let mut total_required_chars = 0usize;
            const MAX_REQUIRED_CHARS: usize = 100_000;
            for path in &required_files {
                if total_required_chars >= MAX_REQUIRED_CHARS { break; }
                if let Ok(content) = std::fs::read_to_string(path) {
                    total_required_chars += content.len();
                    let tokens = self.estimator.count_text(&content);
                    self.trajectory.add_message(
                        Role::User,
                        json!(format!("[Context reload after compaction: {} ({} lines)]", path, content.lines().count())),
                        self.estimator.count_text(&format!("[Context reload: {}]", path)),
                    );
                    // Add the file content as a tool result so the model can see it
                    self.trajectory.add_tool_result(
                        json!(content),
                        tokens,
                        0,
                        crate::agent::trajectory::ToolMessageMeta {
                            tool_name: "read".to_string(),
                            paths_read: vec![path.clone()],
                            paths_written: Vec::new(),
                            is_compacted: false,
                            artifact_ref: None,
                        },
                    );
                }
            }
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

        // Pre-compute AST churn for observer DCR signal
        if self.observer.config.enabled {
            let ast_churn = self.compute_ast_churn().await;
            self.observer.set_ast_churn(ast_churn);
        }

        // Observer: trigger turn lifecycle (SQS, loop classification, watcher/critic/reflection)
        let prev_filter_fired = self.observer.metrics.filter_fired;
        let interrupt = if self.observer.config.enabled {
            Some(self.observer.on_turn_complete(&self.trajectory))
        } else {
            None
        };
        let filter_just_fired = self.observer.metrics.filter_fired > prev_filter_fired;

        // LLM-driven observations: build prompts and call gateway
        if self.observer.config.enabled && self.observer.config.use_llm_observations {
            self.run_llm_observations(&interrupt, filter_just_fired).await;
        }

        // Blocking summarizer: when unobserved token ratio exceeds blockAfter, compress synchronously
        if self.observer.config.enabled && self.observer.config.use_llm_observations {
            if let Some(ref intr) = interrupt {
                if intr.needs_sync_summary {
                    self.run_sync_summarizer().await;
                }
            }
        }

        // Enrich latest observation key with API data from analyzer
        if self.observer.config.enabled {
            if let Some(ref apis) = api_filter_response {
                self.observer.enrich_latest_key(apis);
            }
        }

        // Index latest observations to the analyzer daemon for semantic search
        if self.observer.config.enabled {
            self.index_observations_to_daemon().await;
        }

        // Compute pause weight and flush telemetry
        if self.observer.config.enabled {
            let duration_s = turn_start.elapsed().as_secs_f64();
            let after_error = self.observer.recent_errors().last().is_some();
            let after_watcher = self.observer.watcher_just_fired();
            let entropy = self.observer.last_sqs().map(|s| 1.0 - s).unwrap_or(0.5);
            let ast_contra = self.observer.ast_delta().map(|d| d < -5).unwrap_or(false);
            let _pause_weight = self.observer.compute_pause_weight(
                duration_s, after_error, after_watcher, entropy, ast_contra,
            );
            self.flush_observer_telemetry();
        }

        let observer_block = if self.observer.config.enabled {
            let mut parts = Vec::new();
            // Interrupt directive takes priority
            if let Some(ref intr) = interrupt {
                if let Some(directive) = self.observer.build_interrupt_directive(&intr.action, &intr.reason) {
                    parts.push(directive);
                }
            }
            let mut block = self.observer.build_observation_block();
            // Apply API filter to reduce observation noise when APIs are tracked
            if !block.is_empty() {
                if let Some(ref apis) = api_filter_response {
                    block = self.observer.apply_api_filter(&block, apis);
                }
                if !block.is_empty() {
                    parts.push(block);
                }
            }
            if parts.is_empty() { None } else { Some(parts.join("\n\n---\n\n")) }
        } else {
            None
        };

        let dynamic = DynamicContext {
            file_context: &self.file_context,
            observations: &self.context_manager.vault,
            current_apis: &current_apis,
            background_summary: &None,
            distilled_context: &None,
            task_state_summary: &task_summary,
            tail_reminder: &tail_reminder,
            observer_block: &observer_block,
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

        // Trajectory message compression: when observer has summarized messages,
        // replace messages 2..last_observed with the observation block (matching TS prepareContext).
        // This is a read-only view — the stored trajectory is never mutated.
        let needs_compression = observer_block.is_some()
            && self.observer.last_observed_message_index() > 2
            && self.observer.last_observed_message_index() < self.trajectory.messages.len();

        let mut compressed_trajectory;
        let trajectory_ref: &Trajectory = if needs_compression {
            let idx = self.observer.last_observed_message_index();
            compressed_trajectory = Trajectory::new();
            compressed_trajectory.messages.extend(self.trajectory.messages[..2].iter().cloned());
            compressed_trajectory.messages.extend(self.trajectory.messages[idx..].iter().cloned());
            compressed_trajectory.last_checkpoint = self.trajectory.last_checkpoint.clone();
            &compressed_trajectory
        } else {
            &self.trajectory
        };

        let messages = if self.use_reranking {
            let active_files: std::collections::HashSet<String> = self.file_context.files_read.keys().cloned()
                .chain(self.file_context.files_edited.iter().cloned())
                .collect();
            let task_keywords = crate::context::reranker::extract_task_keywords(
                &self.task_reducer.state.current_goal,
            );
            self.context_manager.build_prompt_with_reranking(
                trajectory_ref,
                &self.file_context.files_edited,
                Some(&self.task_reducer),
                history_budget,
                &active_files,
                &task_keywords,
            )
        } else {
            self.context_manager.build_prompt_with_stale_check(
                trajectory_ref,
                &self.file_context.files_edited,
                Some(&self.task_reducer),
                history_budget,
            )
        };

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
                            }).await;
                        }
                        if let Some(thinking) = &chunk.thinking {
                            thinking_text.push_str(thinking);
                            let _ = self.emit_event(CoreEvent::ThoughtDelta {
                                agent_id: self.id,
                                text: thinking.clone(),
                                thinking: true,
                            }).await;
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
                    // Detect MAX_TOKENS: if the model hit its output limit,
                    // inject a continuation prompt so it can resume.
                    if chunk.finish_reason.as_deref() == Some("MAX_TOKENS") {
                        full_text.push_str("\n\n[Output was truncated due to token limit. Please continue from where you left off.]");
                        self.last_output_truncated = true;
                    }
                }
                "complete" => break,
                _ => {}
            }
        }

        // Emit actual token usage from provider
        if let Some(usage) = &_usage_total {
            self.cumulative_tokens += usage.total_tokens as usize;
            let sqs_score = self.observer.last_sqs().unwrap_or(0.5);
            self.emit_event(CoreEvent::MetricsUpdate {
                agent_id: self.id,
                sqs: sqs_score,
                token_usage: self.cumulative_tokens,
                latency_ms: 0,
            }).await?;
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
        // Extract call IDs before tool_call_entries is moved into the Message.
        let tool_call_ids: Vec<String> = tool_call_entries.iter().map(|e| e.id.clone()).collect();

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
        self.emit_event(CoreEvent::ThoughtFinished { agent_id: self.id }).await?;

        // 7. Execute tools
        eprintln!("[di-core] run_turn: executing {} tools", tools.len());
        self.turn_edits.clear();
        for (ti, tool) in tools.iter().enumerate() {
            if self.is_aborted() {
                break;
            }

            let call_id = tool_call_ids.get(ti).cloned().unwrap_or_default();

            // Mode gate: Plan mode restricts to read-only tools
            if self.mode == AgentMode::Plan && !PLAN_MODE_TOOLS.contains(&tool.name.as_str()) {
                let skip_msg = json!({ "status": "blocked", "message": format!("Tool '{}' not allowed in Plan mode", tool.name) });
                self.trajectory.add_tool_result(skip_msg.clone(), 50, ti, ToolMessageMeta::default());
                self.emit_event(CoreEvent::ToolCallFinished {
                    agent_id: self.id,
                    call_id: call_id.clone(),
                    result: skip_msg,
                }).await?;
                continue;
            }

            // Track file context (moved after execution so we can hash result content)
            let path_arg = tool.args.get("path").and_then(|v| v.as_str()).map(String::from);
            let tool_name = tool.name.clone();
            // Extract bash command for post-execution security checks
            let bash_command = if tool_name == "bash" {
                tool.args.get("command").and_then(|v| v.as_str()).map(String::from)
            } else {
                None
            };

            // Pre-execution approval gate: destructive tools (write/edit/bash)
            // require user approval BEFORE execution. Read-only tools auto-approve.
            let is_safe_bash = tool.name == "bash" && bash_command.as_deref().map(|c| crate::tools::approval::ApprovalManager::is_safe_bash_command(c)).unwrap_or(false);
            if !self.approval_manager.should_auto_approve(&tool.name) && !is_safe_bash {
                // Emit tool call details FIRST so the user sees what they're approving
                self.emit_event(CoreEvent::ToolCallStarted {
                    agent_id: self.id,
                    call_id: call_id.clone(),
                    tool: tool.name.clone(),
                    args: tool.args.clone(),
                }).await?;

                let approval_id = Uuid::new_v4();
                let description = format!("Execute {} on behalf of agent", tool.name);
                self.emit_event(CoreEvent::ApprovalNeeded {
                    agent_id: self.id,
                    approval_id,
                    tool: tool.name.clone(),
                    args: tool.args.clone(),
                    description: description.clone(),
                }).await?;

                // Block waiting for approval response from frontend.
                // Match on approval_id to prevent replay attacks from stale responses.
                // Note: emit_event flushes stdout before we block here, so the frontend
                // receives ApprovalNeeded. The main loop uses try_send (non-blocking)
                // to route the response, preventing deadlock even under fast auto-approve.
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
                            }).await?;
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
                            }).await?;
                            break false;
                        }
                    }
                };

                if !approved {
                    let skip_msg = json!({ "status": "denied", "message": "Frontend timeout or denial" });
                    self.trajectory.add_tool_result(skip_msg.clone(), 50, ti, ToolMessageMeta::default());
                    self.emit_event(CoreEvent::ToolCallFinished {
                        agent_id: self.id,
                        call_id: call_id.clone(),
                        result: skip_msg,
                    }).await?;
                    continue;
                }
            } else {
                // Auto-approved: emit tool call started normally
                self.emit_event(CoreEvent::ToolCallStarted {
                    agent_id: self.id,
                    call_id: call_id.clone(),
                    tool: tool.name.clone(),
                    args: tool.args.clone(),
                }).await?;
            }

            eprintln!("[di-core] run_turn: executing tool {} ({})", ti, tool.name);

            // Circuit breaker: skip execution if tool circuit is open
            if !self.circuit_breakers.allow_execution(&tool_name) {
                self.recovery_telemetry.blocked_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let msg = format!(
                    "[Circuit breaker open for '{}'. Too many consecutive failures. Waiting before retry.]",
                    tool_name
                );
                self.trajectory.add_tool_result(
                    json!({"status": "error", "error": msg}),
                    50, ti, ToolMessageMeta::default(),
                );
                continue;
            }

            // Pre-flight firewall: block dangerous or malformed tool calls
            let (preflight_block, modified_args) = self.run_preflight_firewall(tool);
            if let Some(block_error) = preflight_block {
                self.recovery_telemetry.blocked_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.trajectory.add_tool_result(block_error, 50, ti, ToolMessageMeta::default());
                continue;
            }

            // If pre-flight auto-fixed args, create a modified tool for execution
            let exec_tool;
            let tool = if let Some(ref new_args) = modified_args {
                exec_tool = crate::tools::ToolCall {
                    name: tool.name.clone(),
                    args: new_args.clone(),
                };
                &exec_tool
            } else {
                tool
            };

            // Track turn edits for overlapping edit detection
            if matches!(tool_name.as_str(), "write" | "edit") {
                if let Some(p) = tool.args.get("path").and_then(|v| v.as_str()) {
                    self.turn_edits.insert(p.to_string());
                }
            }

            let exec_result = self.tool_executor.execute(tool, &mut self.coordinator).await;
            eprintln!("[di-core] run_turn: tool {} done ({})", ti, if exec_result.is_ok() { "ok" } else { "err" });

            // Record success/failure for circuit breaker
            if exec_result.is_ok() {
                let result_ref = exec_result.as_ref().unwrap();
                let is_error = result_ref.get("status").and_then(|v| v.as_str()) == Some("error");
                if is_error {
                    self.circuit_breakers.record_failure(&tool_name);
                    self.recovery_telemetry.intercepted_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                } else {
                    self.circuit_breakers.record_success(&tool_name);
                }
            } else {
                self.circuit_breakers.record_failure(&tool_name);
                self.recovery_telemetry.escalated_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }

            // Stagnation detection
            if let Ok(ref result_val) = exec_result {
                let args_hash = crate::util::fast_hash(result_val.to_string().as_bytes());
                if let Ok(mut det) = self.stagnation_detector.lock() {
                    if let Some(warning) = det.record(&tool_name, &args_hash) {
                        self.trajectory.add_message(
                            Role::User,
                            json!(format!("[SYSTEM: STAGNATION] {}", warning)),
                            30,
                        );
                    }
                }
            }

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
                                call_id: call_id.clone(),
                                result: plan_json,
                            }).await?;
                        } else {
                        let message = result.get("result").and_then(|v| v.as_str()).unwrap_or("Task complete").to_string();
                        self.trajectory.add_tool_result(json!({ "status": "completed", "message": &message }), self.estimator.count_text(&message), ti, ToolMessageMeta::default());
                        self.emit_event(CoreEvent::ToolCallFinished {
                            agent_id: self.id,
                            call_id: call_id.clone(),
                            result: json!({ "status": "completed", "message": &message }),
                        }).await?;
                        // Emit TaskPresented instead of TaskFinished — agent signals done
                        // but the user should be able to send follow-up messages
                        self.emit_event(CoreEvent::TaskPresented {
                            agent_id: self.id,
                            message: message.clone(),
                        }).await?;
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
                        }).await?;

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
                                    }).await?;
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
                                    }).await?;
                                    break String::new();
                                }
                            }
                        };

                        let answer_json = json!({ "question": question, "answer": answer_text, "status": "answered" });
                        self.trajectory.add_tool_result(answer_json.clone(), 50, ti, ToolMessageMeta::default());
                        self.emit_event(CoreEvent::ToolCallFinished {
                            agent_id: self.id,
                            call_id: call_id.clone(),
                            result: answer_json,
                        }).await?;
                    } else if action == Some("new_task") {
                        // New task: emit event for orchestrator to spawn a new agent
                        let task_text = result.get("task").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        self.emit_event(CoreEvent::TaskFinished {
                            agent_id: self.id,
                            success: true,
                            message: format!("Spawning new task: {}", task_text),
                        }).await?;
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
                            // Store required files for re-reading after compaction
                            if let Some(files) = result.get("required_files").and_then(|v| v.as_array()) {
                                self.pending_compact_required_files = files.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect();
                            }
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
                            call_id: call_id.clone(),
                            result: json!({ "status": if advisory.allowed { "compact_advisory" } else { "compact_rejected" } }),
                        }).await?;
                    } else {
                        let mut result = result;

                        // Apply read file formatting FIRST: hash-anchored lines, unchanged detection.
                        // Must run before OutputManager budget enforcement so that:
                        // (a) the _read_raw marker is still present for detection, and
                        // (b) OutputManager measures formatted text size, not raw JSON.
                        if tool_name == "read" && result.get("_read_raw").is_some() {
                            if let Some(p) = result.get("path").and_then(|v| v.as_str()) {
                                self.file_context.pre_increment_read(p);
                            }
                            if let Some(results) = result.get("results").and_then(|v| v.as_array()) {
                                for r in results {
                                    if let Some(p) = r.get("path").and_then(|v| v.as_str()) {
                                        self.file_context.pre_increment_read(p);
                                    }
                                }
                            }
                            result = self.format_read_result(&result);
                        }

                        // Output budget enforcement: write large tool results to disk.
                        // Now runs on formatted text (for reads) or raw output (for bash).
                        if tool_name == "bash" || tool_name == "read" {
                            let om = self.output_manager.lock().unwrap();
                            result = om.enforce_budget(result, &tool_name);
                        }

                        // Write-execute risk detection: warn when bash runs a script
                        // that was written/edited by the agent in this session.
                        if tool_name == "bash" {
                            if let Some(ref cmd) = bash_command {
                                let script_extensions = [".sh", ".py", ".rb", ".js", ".ts", ".pl", ".lua", ".php", ".bash"];
                                let risky_paths: Vec<&str> = self.file_context.files_edited.iter()
                                    .filter(|p| {
                                        let path = p.as_str();
                                        let has_script_ext = script_extensions.iter().any(|ext| path.ends_with(ext));
                                        has_script_ext && cmd.contains(path)
                                    })
                                    .map(|p| p.as_str())
                                    .collect();
                                if !risky_paths.is_empty() {
                                    let warning = format!(
                                        "\n[security: executing AI-written file: {}]",
                                        risky_paths.join(", ")
                                    );
                                    if let Some(s) = result.as_str() {
                                        result = serde_json::json!(format!("{}{}", s, warning));
                                    }
                                }
                            }

                            // Bash mistake tracking: non-zero exit increments mistakes, zero resets.
                            // Empty command also increments.
                            if let Some(s) = result.as_str() {
                                let exit_code = s.strip_prefix("exit:")
                                    .and_then(|rest| rest.lines().next())
                                    .and_then(|line| line.trim().parse::<i32>().ok())
                                    .unwrap_or(-1);

                                // Bash execution history tracking
                                let exec_id = format!("exec-{}", self.bash_history.len() + 1);
                                let cmd_str = bash_command.clone().unwrap_or_default();
                                self.bash_history.push((exec_id.clone(), cmd_str, exit_code));

                                // Append execution_id to result for proof-of-execution
                                let exec_tag = format!("\n[execution_id: {}]", exec_id);
                                result = serde_json::json!(format!("{}{}", s, exec_tag));

                                // Mistake tracking
                                match exit_code {
                                    0 => self.consecutive_mistake_count = 0,
                                    _ => self.consecutive_mistake_count += 1,
                                }
                            }
                        }

                        // --verify: re-read the target file after write/edit to confirm changes
                        if matches!(tool_name.as_str(), "write" | "edit") && path_arg.is_some() {
                            let verify_requested = tools.get(ti)
                                .and_then(|t| t.args.get("verify"))
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if verify_requested {
                                if let Some(ref path) = path_arg {
                                    let verify_msg = if let Ok(content) = std::fs::read_to_string(path) {
                                        let line_count = crate::tools::read_file::count_lines(&content);
                                        format!("\n[verify: {} ({} lines) — changes confirmed on disk]", path, line_count)
                                    } else {
                                        format!("\n[verify: WARNING — could not re-read {} after write]", path)
                                    };
                                    // Write tool returns a plain string; edit tool returns {"result": "...", ...}
                                    if let Some(s) = result.as_str() {
                                        result = serde_json::json!(format!("{}{}", s, verify_msg));
                                    } else if let Some(s) = result.get("result").and_then(|v| v.as_str()).map(String::from) {
                                        if let Some(obj) = result.as_object_mut() {
                                            obj.insert("result".to_string(), serde_json::json!(format!("{}{}", s, verify_msg)));
                                        }
                                    }
                                }
                            }
                        }

                        // Progressive write error: escalate guidance on repeated missing-content failures
                        if tool_name == "write" {
                            let error_str = result.get("error").and_then(|v| v.as_str()).unwrap_or("");
                            let is_missing_content = result.get("status").and_then(|v| v.as_str()) == Some("error")
                                && error_str.contains("Missing content");
                            if is_missing_content {
                                self.write_missing_content_count += 1;
                                let guidance = match self.write_missing_content_count {
                                    1 => "Tip: Use --content flag to provide file content, e.g. write path --content 'your content'.".to_string(),
                                    2 => "You've called write without content twice. The write tool requires a --content argument with the file's full content.".to_string(),
                                    _ => format!("Repeated write without content ({} times). Each write call MUST include --content with the complete file text.", self.write_missing_content_count),
                                };
                                if let Some(obj) = result.as_object_mut() {
                                    if let Some(error_val) = obj.get_mut("error") {
                                        if let Some(e) = error_val.as_str() {
                                            let updated = format!("{}\n\n{}", e, guidance);
                                            *error_val = serde_json::Value::String(updated);
                                        }
                                    }
                                }
                            } else if result.get("status").is_none() || result.get("status").and_then(|v| v.as_str()) != Some("error") {
                                self.write_missing_content_count = 0;
                            }
                        }

                        // Done tool: append recovery telemetry summary
                        if tool_name == "done" {
                            let intercepted = self.recovery_telemetry.intercepted_count.load(std::sync::atomic::Ordering::Relaxed);
                            if intercepted > 0 {
                                let summary = self.recovery_telemetry.summary();
                                if let Some(s) = result.as_str() {
                                    result = serde_json::json!(format!("{}\n\n{}", s, summary));
                                }
                            }
                        }

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
                        let result_str = result.to_string();
                        let is_cached = result_str.starts_with("\"[Cache Hit]");
                        // Apply envelope wrapping to content string results
                        // Error results carry <tool_error> XML in the "error" field
                        let inner = if result.get("status").and_then(|v| v.as_str()) == Some("error") {
                            // Error result: extract the error field which contains <tool_error> XML
                            result.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error").to_string()
                        } else if let Some(s) = result.as_str() {
                            s.to_string()
                        } else {
                            result_str.clone()
                        };
                        // Prefix with tool description matching TS format
                        let desc = tool_description(&tool_name);
                        let mut content_for_envelope = format!("{} Result:\n{}", desc, inner);
                        // Append exploration hints for read/search/repo/symbols
                        if let Some(hint) = build_exploration_hint(&tool_name, path_arg.as_deref()) {
                            content_for_envelope.push_str(&hint);
                        }
                        // Append ambiguity hint if score > 0.4
                        if let Some(tc) = tools.get(ti) {
                            let amb_score = score_ambiguity(&tool_name, &tc.args, &self.file_context);
                            if amb_score > 0.4 {
                                content_for_envelope.push_str(&format!(
                                    "\n[HINT] This call had high ambiguity ({:.2}). Consider using --clarify next time.",
                                    amb_score
                                ));
                            }
                        }
                        let read_count = self.file_context.files_read.len();
                        let enveloped = wrap_in_envelope(&content_for_envelope, &tool_name, is_cached, self.cumulative_tokens, read_count);
                        let safe_result_str = crate::util::secrets::redact_secrets(&enveloped);
                        let safe_result: serde_json::Value = serde_json::from_str(&safe_result_str).unwrap_or_else(|_| json!(safe_result_str));
                        if safe_result_str != result_str { self.metrics.inc_redaction(); }
                        self.trajectory.add_tool_result(safe_result, estimated_tokens, ti, meta);
                        self.emit_event(CoreEvent::ToolCallFinished {
                            agent_id: self.id,
                            call_id: call_id.clone(),
                            result,
                        }).await?;
                    }
                }
                Err(e) => {
                    let safe_error = crate::util::secrets::redact_secrets(&e.to_string());
                    if safe_error != e.to_string() { self.metrics.inc_redaction(); }
                    let error_msg = json!({ "error": safe_error });
                    self.trajectory.add_tool_result(error_msg.clone(), 50, ti, ToolMessageMeta::default());
                    self.emit_event(CoreEvent::ToolCallFinished {
                        agent_id: self.id,
                        call_id: call_id.clone(),
                        result: error_msg,
                    }).await?;
                }
            }
        }

        // Tool call count warning (matching TS: warn at 50, then every 25 after)
        if self.tool_call_counter >= 50 && (self.tool_call_counter - 50) % 25 == 0 {
            let warning = format!(
                "[SYSTEM NOTE: You have executed {} tool calls in this task. Consider whether you have enough information to complete the task or should attempt completion.]",
                self.tool_call_counter
            );
            self.trajectory.add_message(
                Role::User, json!(warning), self.estimator.count_text(&warning),
            );
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
        }).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // LLM-driven observer calls via gateway
    // -----------------------------------------------------------------------

    /// Run LLM-driven observations: build prompts, call gateway, parse responses.
    /// This runs after the heuristic observer cycle to potentially upgrade observations.
    async fn run_llm_observations(&mut self, interrupt: &Option<crate::observer::InterruptResult>, filter_just_fired: bool) {
        use crate::observer::{ObservationType, parse_llm_observation};
        use std::time::Instant;

        // Determine which observation types should fire this turn
        let mut pending: Vec<crate::observer::ObserverLlmRequest> = Vec::new();

        // Watcher: if the heuristic watcher just fired
        if self.observer.watcher_just_fired() && self.observer.pending_llm_count < 3 {
            pending.push(self.observer.build_watcher_llm_prompt(&self.trajectory));
        }

        // Critic: if SQS is below threshold
        if let Some(ref intr) = interrupt {
            if intr.action != crate::observer::CriticAction::Continue && self.observer.pending_llm_count < 3 {
                pending.push(self.observer.build_critic_llm_prompt(&self.trajectory));
            }
        }

        // Skeleton: every 4 turns when there's data
        if self.observer.metrics.turns_observed % 4 == 0
            && self.observer.has_skeleton_data()
            && self.observer.pending_llm_count < 3
        {
            pending.push(self.observer.build_skeleton_llm_prompt());
        }

        // Filter: when heuristic filter just fired this turn
        if filter_just_fired && self.observer.pending_llm_count < 3 {
            pending.push(self.observer.build_filter_llm_prompt(&self.trajectory));
        }

        // Reflector: when reflection threshold met
        if self.observer.should_reflect_llm() && self.observer.pending_llm_count < 2 {
            pending.push(self.observer.build_reflector_llm_prompt());
        }

        // Summarizer: when buffer_activation turns pass without summary
        if self.observer.should_summarize(&self.trajectory) && self.observer.pending_llm_count < 3 {
            pending.push(self.observer.build_summarizer_llm_prompt(&self.trajectory));
        }

        // Execute each prompt through the gateway with latency tracking
        let budget_ms = self.observer.config.latency_budget_ms;
        let mut cumulative_latency: u64 = 0;
        for req in pending {
            // Skip if latency budget exceeded (0 = no budget)
            if budget_ms > 0 && cumulative_latency >= budget_ms {
                break;
            }
            let obs_type = req.obs_type.clone();
            self.observer.pending_llm_count += 1;
            let start = Instant::now();
            match self.call_observer_gateway(&req.system_prompt, &req.user_message).await {
                Some(response_text) => {
                    let latency_ms = start.elapsed().as_millis() as u64;
                    cumulative_latency += latency_ms;
                    self.observer.health.failing = false;
                    self.observer.health.last_error = None;
                    if let Some(parsed) = parse_llm_observation(&response_text, obs_type.clone()) {
                        self.observer.cost_tracker.record(obs_type.clone(), 60, latency_ms, self.observer.metrics.turns_observed);
                        self.observer.process_llm_observation(parsed, obs_type);
                    }
                }
                None => {
                    self.observer.health.failing = true;
                    self.observer.health.last_error = Some(format!("LLM observation failed for {:?}", obs_type));
                    debug_log!("[di-core] observer LLM call failed for {:?}, using heuristic", obs_type);
                }
            }
            self.observer.pending_llm_count -= 1;
        }
    }

    /// Blocking summarizer: synchronously compresses unobserved messages via LLM.
    /// Called when unobserved token ratio exceeds blockAfter (TS runSummarizerSync).
    async fn run_sync_summarizer(&mut self) {
        use crate::observer::{ObservationType, SkeletonFidelity, parse_llm_observation};

        let req = self.observer.build_summarizer_llm_prompt(&self.trajectory);
        let token_est = self.observer.get_unobserved_token_estimate(&self.trajectory);

        if let Some(response_text) = self.call_observer_gateway(&req.system_prompt, &req.user_message).await {
            if let Some(parsed) = parse_llm_observation(&response_text, ObservationType::Summary) {
                let turn = self.observer.current_turn();
                let msg_len = self.trajectory.messages.len();
                let obs = crate::observer::Observation {
                    obs_type: ObservationType::Summary,
                    text: parsed.text,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0),
                    confidence: parsed.confidence,
                    token_estimate: token_est,
                    compressed_range: Some([turn.saturating_sub(3), turn]),
                    critic_action: None,
                    sqs: self.observer.last_sqs(),
                    fidelity: None,
                    key: None,
                };
                self.observer.store.append(obs);
                self.observer.update_last_observed(msg_len);
                debug_log!("[di-core] sync summarizer compressed {} unobserved tokens", token_est);
            }
        }
    }

    /// Flush observer telemetry to .dirac-logs/observer-telemetry.jsonl.
    fn flush_observer_telemetry(&self) {
        use std::io::Write;
        let dir = std::path::Path::new(".dirac-logs");
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join("observer-telemetry.jsonl");
        let entry = serde_json::json!({
            "turn": self.observer.current_turn(),
            "agent": self.id.to_string(),
            "sqs": self.observer.last_sqs(),
            "loop_pattern": format!("{:?}", self.observer.last_loop_pattern()),
            "tier": format!("{:?}", self.observer.current_tier()),
            "observations": self.observer.store().len(),
            "observation_tokens": self.observer.store().estimate_token_count(),
            "metrics": {
                "turns_observed": self.observer.metrics().turns_observed,
                "watcher_fired": self.observer.metrics().watcher_fired,
                "critic_fired": self.observer.metrics().critic_fired,
                "filter_fired": self.observer.metrics().filter_fired,
                "reflect_actions": self.observer.metrics().reflect_actions,
                "restart_actions": self.observer.metrics().restart_actions,
                "sqs_samples": self.observer.metrics().sqs_samples,
                "avg_sqs": if self.observer.metrics().sqs_samples > 0 {
                    self.observer.metrics().avg_sqs / self.observer.metrics().sqs_samples as f32
                } else { 0.0 },
            },
            "cost": {
                "total_tokens": self.observer.cost_tracker().total_tokens(),
                "total_latency_ms": self.observer.cost_tracker().total_latency_ms(),
                "entries": self.observer.cost_tracker().entry_count(),
            },
        });
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(f, "{}", entry);
        }
    }

    /// Index the latest observation of each type to the analyzer daemon for semantic search.
    async fn index_observations_to_daemon(&self) {
        let latest = self.observer.latest_observables();
        if latest.is_empty() {
            return;
        }
        let daemon = self.tool_executor.analyzer_daemon();
        let mut daemon = match daemon.try_lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        for (obs_type, text, ts, tokens) in latest {
            let req = crate::daemons::AnalyzerRequest {
                command: "index-observation".to_string(),
                file: Some(ts.to_string()),
                content: Some(obs_type),
                language: None,
                query: Some(text),
                subcommand: Some(tokens.to_string()),
            };
            let _ = daemon.send_request::<_, crate::daemons::AnalyzerResponse>(req).await;
        }
    }

    /// Search observations via the analyzer daemon, returning matching content strings.
    pub async fn search_observations_via_daemon(&self, query: &str, limit: usize) -> Vec<String> {
        let daemon = self.tool_executor.analyzer_daemon();
        let mut daemon = match daemon.try_lock() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let req = crate::daemons::AnalyzerRequest {
            command: "search-observations".to_string(),
            file: None,
            content: None,
            language: None,
            query: Some(query.to_string()),
            subcommand: None,
        };
        match daemon.send_request::<_, crate::daemons::AnalyzerResponse>(req).await {
            Ok(resp) => {
                resp.data.get("data")
                    .and_then(|d| d.get("results"))
                    .and_then(|r| r.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|e| e.get("content").and_then(|c| c.as_str()).map(String::from))
                            .take(limit)
                            .collect()
                    })
                    .unwrap_or_default()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Pre-compute AST churn from the last edit for observer DCR.
    async fn compute_ast_churn(&mut self) -> Option<(usize, usize, usize)> {
        let last_assistant = self.trajectory.messages.iter()
            .filter(|m| matches!(m.role, Role::Assistant))
            .last();
        let msg = last_assistant?;
        let content = msg.content.to_string();

        let file_path = regex::Regex::new(r#"path":\s*"([^"]+)""#).ok()
            .and_then(|re| re.captures(&content))
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())?;
        let new_content = regex::Regex::new(r#"new_content":\s*"([^"]{10,})""#).ok()
            .and_then(|re| re.captures(&content))
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())?;

        let lang = file_path.rsplit('.').next().unwrap_or("rs").to_string();
        let analyzer_req = crate::daemons::AnalyzerRequest {
            command: "ast-churn".to_string(),
            file: Some(file_path),
            content: Some(new_content),
            language: Some(lang),
            query: None,
            subcommand: None,
        };
        match self.tool_executor.analyzer_daemon().lock().await.send_request::<_, crate::daemons::AnalyzerResponse>(analyzer_req).await {
            Ok(resp) if resp.ok => {
                let added = resp.data.get("added").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let removed = resp.data.get("removed").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let total = added + removed;
                Some((added, removed, total))
            }
            _ => None,
        }
    }

    /// Call the gateway for an observer LLM request. Returns the full text response or None.
    async fn call_observer_gateway(&self, system: &str, user_message: &str) -> Option<String> {
        use crate::daemons::{GatewayRequest, GatewayMessage};

        let request_id = self.request_id_counter + 10_000_000; // Offset to avoid collision

        let messages = vec![
            GatewayMessage::simple("user", serde_json::Value::String(user_message.to_string())),
        ];

        // Observer can override provider/model from config
        let observer_provider = if self.observer.config.provider.is_some() || self.observer.config.model_id.is_some() {
            let base = self.provider_config.as_ref();
            Some(crate::daemons::ProviderConfig {
                id: self.observer.config.provider.clone()
                    .or_else(|| base.map(|b| b.id.clone()))
                    .unwrap_or_default(),
                model: self.observer.config.model_id.clone()
                    .or_else(|| base.map(|b| b.model.clone()))
                    .unwrap_or_default(),
                api_key: base.and_then(|b| b.api_key.clone()),
                base_url: base.and_then(|b| b.base_url.clone()),
                region: base.and_then(|b| b.region.clone()),
                project_id: base.and_then(|b| b.project_id.clone()),
                params: base.map(|b| b.params.clone()).unwrap_or_default(),
            })
        } else {
            None
        };

        let request = GatewayRequest {
            id: request_id,
            stream: false,
            timeout: Some(30), // 30s timeout for observer calls
            provider: observer_provider,
            messages,
            system: Some(system.to_string()),
            tools: None,
            max_tokens: Some(300), // Observer responses are short
            temperature: Some(0.3), // Low creativity for structured output
            thinking: None,
        };

        let mut rx = match self.gateway_client.stream_chat(request).await {
            Ok(rx) => rx,
            Err(_) => return None,
        };

        let mut full_text = String::new();
        while let Some(chunk_result) = rx.recv().await {
            match chunk_result {
                Ok(chunk) => {
                    if let Some(delta) = &chunk.text_delta {
                        full_text.push_str(delta);
                    }
                    if chunk.chunk_type == "complete" {
                        break;
                    }
                }
                Err(_) => return None,
            }
        }

        if full_text.is_empty() {
            None
        } else {
            Some(full_text)
        }
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

    async fn emit_event(&self, event: CoreEvent) -> Result<()> {
        match serde_json::to_string(&event) {
            Ok(json) => {
                // Use spawn_blocking to avoid blocking the tokio worker thread
                // during stdout write+flush. This prevents pipe-buffer deadlocks
                // where di-core blocks on flush while divrr blocks on writing a
                // response to di-core's stdin.
                tokio::task::spawn_blocking(move || {
                    use std::io::Write;
                    let stdout = std::io::stdout();
                    let mut handle = stdout.lock();
                    let _ = writeln!(handle, "{}", json);
                    let _ = handle.flush();
                }).await?;
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

    /// Update observer config on all agents from frontend settings.
    pub fn set_observer_config(&mut self, msg: crate::protocol::FrontendMessage) {
        if let crate::protocol::FrontendMessage::SetObserverConfig {
            enabled,
            use_llm_observations,
            watcher_frequency,
            critic_frequency,
            verbose,
            token_threshold,
            buffer_activation,
            block_after,
            reflection_enabled,
            reflection_token_threshold,
            procedural_monotonicity_enabled,
            ast_guided_memory_enabled,
            adaptive_cooldown_enabled,
            latency_budget_ms,
            permissive_buffer_size,
            observer_provider,
            observer_model_id,
        } = msg
        {
            for agent in self.agents.values_mut() {
                agent.observer.config.enabled = enabled;
                agent.observer.config.use_llm_observations = use_llm_observations;
                agent.observer.config.watcher_frequency = watcher_frequency;
                agent.observer.config.critic_frequency = critic_frequency;
                agent.observer.config.verbose = verbose;
                agent.observer.config.token_threshold = token_threshold;
                agent.observer.config.buffer_activation = buffer_activation;
                agent.observer.config.block_after = block_after;
                agent.observer.config.reflection_enabled = reflection_enabled;
                agent.observer.config.reflection_token_threshold = reflection_token_threshold;
                agent.observer.config.procedural_monotonicity_enabled = procedural_monotonicity_enabled;
                agent.observer.config.ast_guided_memory_enabled = ast_guided_memory_enabled;
                agent.observer.config.adaptive_cooldown_enabled = adaptive_cooldown_enabled;
                agent.observer.config.latency_budget_ms = latency_budget_ms;
                agent.observer.config.permissive_buffer_size = permissive_buffer_size;
                agent.observer.config.provider = observer_provider.clone();
                agent.observer.config.model_id = observer_model_id.clone();
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

    pub async fn emit_event(&self, event: CoreEvent) -> Result<()> {
        match serde_json::to_string(&event) {
            Ok(json) => {
                tokio::task::spawn_blocking(move || {
                    use std::io::Write;
                    let stdout = std::io::stdout();
                    let mut handle = stdout.lock();
                    let _ = writeln!(handle, "{}", json);
                    let _ = handle.flush();
                }).await?;
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
