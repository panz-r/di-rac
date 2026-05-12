use crate::agent::trajectory::{Trajectory, Role, ToolMessageMeta, ToolCallEntry, Message};
use crate::agent::parser::StreamingToolAccumulator;
use crate::agent::file_context::FileContextTracker;
use crate::agent::artifact::ArtifactStore;
use crate::agent::environment::EnvironmentManager;
use crate::observer::Observer;
use crate::context::{ContextManager, ConservativeEstimator, TokenEstimator, TurnMetrics, ToolCallRecord};
use crate::context::lifecycle::ContextLifecycleManager;
use crate::context::distiller::{ContextDistiller, DistillerSource};
use crate::context::task_state::TaskStateReducer;
use crate::agent::metrics::ContextMetrics;
use crate::daemons::{
    UnixDaemonClient, GatewayStreamClient, GatewayRequest, GatewayMessage, CommandDaemon,
    ResilientDaemon,
};
use crate::protocol::{CoreEvent, FrontendMessage};
use crate::tools::{ToolExecutor, ToolCoordinator};
use crate::prompt::{ContextCompiler, DynamicContext, session::SessionContext};
use crate::tools::background::BackgroundCommandTracker;
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
    pub background_tracker: Arc<BackgroundCommandTracker>,
    pub abort: Arc<AtomicBool>,
    pub consecutive_mistake_count: usize,
    pub max_consecutive_mistakes: usize,
    pub request_id_counter: i64,
    pub frontend_rx: Option<mpsc::Receiver<FrontendMessage>>,
    pub frontend_tx: mpsc::Sender<FrontendMessage>,
    pub mode: AgentMode,
    pub file_context: FileContextTracker,
    pub artifact_store: Arc<tokio::sync::Mutex<ArtifactStore>>,
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
    /// Latest critical files from distiller enrichment. Used during GC to protect artifacts.
    critical_artifact_files: std::collections::HashSet<String>,
    /// Whether to use the reranking pipeline for context selection. Opt-in; default false.
    use_reranking: bool,
    pub output_manager: Arc<std::sync::Mutex<crate::tools::output_manager::OutputManager>>,
    pub read_file_cache: std::sync::Mutex<crate::tools::read_file::ReadFileCache>,
    /// Cumulative token usage across all turns in this task.
    cumulative_tokens: usize,
}

impl AgentEngine {
    pub fn new(
        id: Uuid,
        analyzer_daemon: Arc<tokio::sync::Mutex<ResilientDaemon>>,
        command_daemon: Arc<tokio::sync::Mutex<CommandDaemon>>,
        _central_client: Arc<UnixDaemonClient>,
        gateway_client: Arc<GatewayStreamClient>,
    ) -> Self {
        let background_tracker = Arc::new(BackgroundCommandTracker::new());
        let artifact_store = Arc::new(tokio::sync::Mutex::new(ArtifactStore::new()));
        let output_manager = Arc::new(std::sync::Mutex::new(crate::tools::output_manager::OutputManager::new()));
        let (frontend_tx, frontend_rx) = mpsc::channel(32);
        Self {
            id,
            trajectory: Trajectory::new(),
            observer: Observer::new(),
            context_manager: ContextManager::new(32000, 24000),
            gateway_client,
            tool_executor: ToolExecutor::new(
                analyzer_daemon, command_daemon,
                background_tracker.clone(),
                artifact_store.clone(),
                output_manager.clone(),
            ),
            coordinator: ToolCoordinator::new(),
            approval_manager: ApprovalManager::new(),
            background_tracker,
            abort: Arc::new(AtomicBool::new(false)),
            consecutive_mistake_count: 0,
            max_consecutive_mistakes: 6,
            request_id_counter: 0,
            frontend_rx: Some(frontend_rx),
            frontend_tx,
            mode: AgentMode::Act,
            file_context: FileContextTracker::new(),
            artifact_store: artifact_store.clone(),
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
            critical_artifact_files: std::collections::HashSet::new(),
            use_reranking: false,
            output_manager,
            read_file_cache: std::sync::Mutex::new(crate::tools::read_file::ReadFileCache::new()),
            cumulative_tokens: 0,
        }
    }

    /// Receive from the frontend channel with the current timeout.
    /// Returns None on timeout or channel closure.
    /// Apply hash-anchored formatting to a raw read file result.
    fn format_read_result(&mut self, raw: &serde_json::Value) -> serde_json::Value {
        use crate::tools::read_file::{format_full, format_preview, format_outline, format_skeleton};

        let path = raw.get("path").and_then(|v| v.as_str()).unwrap_or("?");
        let detail = raw.get("detail").and_then(|v| v.as_str()).unwrap_or("full");
        let mut cache = self.read_file_cache.lock().unwrap();

        match detail {
            "outline" => {
                if let Some(analyzer_data) = raw.get("analyzer_data") {
                    format_outline(path, analyzer_data, &mut cache)
                } else {
                    json!({ "path": path, "error": "No analyzer data for outline" })
                }
            }
            "skeleton" => {
                if let Some(analyzer_data) = raw.get("analyzer_data") {
                    format_skeleton(path, analyzer_data, &mut cache)
                } else {
                    json!({ "path": path, "error": "No analyzer data for skeleton" })
                }
            }
            "preview" => {
                if let Some(content) = raw.get("content").and_then(|v| v.as_str()) {
                    let read_count = self.file_context.files_read.get(path)
                        .map(|s| s.read_count).unwrap_or(0);
                    let (output, hash, _) = format_preview(path, content, read_count, &mut cache);
                    self.file_context.mark_read(path, &hash);
                    serde_json::Value::String(output)
                } else {
                    json!({ "path": path, "error": "No content for preview" })
                }
            }
            _ => {
                // full (default)
                if let Some(content) = raw.get("content").and_then(|v| v.as_str()) {
                    let range = raw.get("range").and_then(|v| v.as_array())
                        .and_then(|a| {
                            let start = a.get(0)?.as_u64()? as usize;
                            let end = a.get(1)?.as_u64()? as usize;
                            Some((start, end))
                        });
                    let (output, hash, _) = format_full(path, content, range, &mut cache);
                    self.file_context.mark_read(path, &hash);
                    serde_json::Value::String(output)
                } else {
                    json!({ "path": path, "error": "No content for full read" })
                }
            }
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
        if let Some(ref mut rx) = self.frontend_rx {
            while let Ok(msg) = rx.try_recv() {
                if let FrontendMessage::UserResponse { text, .. } = msg {
                    self.task_reducer.process(&text, false);
                    self.trajectory.add_message(
                        Role::User,
                        json!(text),
                        self.estimator.count_text(&text),
                    );
                }
                // Non-UserResponse messages are unexpected here; discard them.
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

        // 1. API extraction
        eprintln!("[di-core] run_turn: extracting APIs...");
        let current_apis = self.extract_current_apis().await?;
        eprintln!("[di-core] run_turn: APIs extracted ({} apis)", current_apis.len());

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
        }

        // 3. Build context frame (system = stable + session + dynamic, messages = history)
        let bg_summary = self.background_tracker.get_summary().await;
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

        let dynamic = DynamicContext {
            file_context: &self.file_context,
            observations: &self.context_manager.vault,
            current_apis: &current_apis,
            background_summary: &bg_summary,
            distilled_context: &None,
            task_state_summary: &task_summary,
            tail_reminder: &tail_reminder,
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
            serde_json::Value::Null
        } else {
            json!(redacted_text)
        };
        let assistant_thinking = if thinking_text.is_empty() { None } else { Some(thinking_text) };
        let tool_call_entries: Vec<ToolCallEntry> = tools.iter().enumerate().map(|(i, tc)| {
            ToolCallEntry {
                id: format!("call_{}", i),
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

                let description = format!("Execute {} on behalf of agent", tool.name);
                self.emit_event(CoreEvent::ApprovalNeeded {
                    agent_id: self.id,
                    tool: tool.name.clone(),
                    args: tool.args.clone(),
                    description: description.clone(),
                })?;

                // Block waiting for approval response from frontend.
                // Buffer any UserResponse messages that arrive while waiting.
                let approved = loop {
                    let msg = self.recv_frontend().await;
                    match msg {
                        Some(FrontendMessage::ApprovalResponse { approved, .. }) => break approved,
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
            // - read: full content read with hash (enables stale detection)
            // - search/repo/symbols: metadata observation only (no stale detection)
            if let Some(ref path) = path_arg {
                match tool_name.as_str() {
                    "read" => {
                        if let Ok(ref result) = exec_result {
                            let content_str = result.to_string();
                            let hash = crate::util::stable_hash(content_str.as_bytes());
                            self.file_context.mark_read(path, &hash);
                        }
                    }
                    "search" | "repo" | "symbols" => {
                        self.file_context.mark_metadata_observed(path);
                    }
                    "write" | "edit" => {
                        self.file_context.mark_edited(path);
                        self.coordinator.invalidate_for_path(path);
                        self.coordinator.invalidate_search_and_repo();
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
                        // Output budget enforcement: write large bash results to disk.
                        // Only bash — other tools have their own size management
                        // (read has detail levels, search has context_lines, etc.)
                        let mut result = if tool_name == "bash" {
                            let om = self.output_manager.lock().unwrap();
                            om.enforce_budget(result, &tool_name)
                        } else {
                            result
                        };

                        // Apply read file formatting: hash-anchored lines, unchanged detection
                        if tool_name == "read" && result.get("_read_raw").is_some() {
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

    /// Collect artifact refs from all trajectory messages, build checkpoint,
    /// truncate trajectory, GC artifacts, emit event.
    async fn finalize_compaction(&mut self, progress_summary: String, continuation: String) -> Result<()> {
        let msg_count = self.trajectory.messages.len();
        let mut artifact_refs: Vec<String> = Vec::new();
        let mut latest_failures: Vec<String> = Vec::new();
        for msg in &self.trajectory.messages {
            if let Some(ref id) = msg.tool_meta.artifact_ref {
                artifact_refs.push(id.clone());
            }
            artifact_refs.extend(crate::agent::artifact::extract_artifact_refs(&msg.content.to_string()));
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
        artifact_refs.sort();
        artifact_refs.dedup();

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
            artifact_refs,
            latest_failures,
            decisions: Vec::new(),
            abandoned_approaches: Vec::new(),
            thematic_tags,
            source_event_range: Some(format!("0..{}", msg_count)),
        });

        self.trajectory.truncate_with_continuation(continuation, checkpoint);

        let live_refs = crate::agent::artifact::collect_live_refs(
            self.trajectory.last_checkpoint.as_ref(),
            &self.trajectory.messages,
            10,
            &self.critical_artifact_files,
        );
        self.artifact_store.lock().await.gc_unreferenced(&live_refs);

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
        let bg_summary = self.background_tracker.get_summary().await;

        let (enriched, critical_files) = self.enrich_with_distiller(&deterministic_summary, recent_assistant, &file_summary).await;
        self.critical_artifact_files = critical_files.into_iter().collect();

        let mut continuation = ContextManager::continuation_prompt(&enriched);
        if let Some(bg) = bg_summary {
            continuation.push_str(&format!("\n\n{}", bg));
        }

        self.finalize_compaction(deterministic_summary, continuation).await
    }

    async fn perform_compaction(&mut self, summary: &str) -> Result<()> {
        let continuation_base = ContextManager::continuation_prompt(summary);
        let bg_summary = self.background_tracker.get_summary().await;

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
        let (enriched, critical_files) = self.enrich_with_distiller(&continuation_base, recent_summaries, &file_summary).await;
        self.critical_artifact_files = critical_files.into_iter().collect();

        let mut continuation = enriched;
        if let Some(bg) = bg_summary {
            continuation.push_str(&format!("\n\n{}", bg));
        }

        self.finalize_compaction(summary.to_string(), continuation).await
    }

    async fn extract_current_apis(&self) -> Result<HashSet<String>> {
        Ok(HashSet::new())
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
    pub analyzer_daemon: Arc<tokio::sync::Mutex<ResilientDaemon>>,
    pub command_daemon: Arc<tokio::sync::Mutex<CommandDaemon>>,
    pub central_client: Arc<UnixDaemonClient>,
    pub gateway_client: Arc<GatewayStreamClient>,
    /// Default provider config for new agents (set by frontend via SetProviderConfig).
    pub default_provider_config: Option<crate::daemons::ProviderConfig>,
    /// Distiller-specific provider config (separate model/temperature).
    pub distiller_config: Option<crate::daemons::ProviderConfig>,
    /// Shared distiller instance (boxed trait object).
    pub distiller: Option<std::sync::Arc<tokio::sync::RwLock<Box<dyn ContextDistiller>>>>,
    /// Global distiller metrics — not borrowed from any agent.
    distiller_metrics: Arc<ContextMetrics>,
    /// Distiller config version, incremented on each set_distiller_config.
    distiller_config_version: std::sync::atomic::AtomicU64,
}

impl MultiAgentOrchestrator {
    pub fn new(analyzer_daemon: ResilientDaemon, command_daemon: CommandDaemon, central_socket: &str, gateway_socket: &str) -> Self {
        Self {
            agents: HashMap::new(),
            frontend_channels: HashMap::new(),
            analyzer_daemon: Arc::new(tokio::sync::Mutex::new(analyzer_daemon)),
            command_daemon: Arc::new(tokio::sync::Mutex::new(command_daemon)),
            central_client: Arc::new(UnixDaemonClient::new(central_socket)),
            gateway_client: Arc::new(GatewayStreamClient::with_socket(gateway_socket)),
            default_provider_config: None,
            distiller_config: None,
            distiller: None,
            distiller_metrics: ContextMetrics::new(),
            distiller_config_version: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn spawn_agent(&mut self, _task: String) -> Uuid {
        let id = Uuid::new_v4();
        let mut agent = AgentEngine::new(
            id,
            self.analyzer_daemon.clone(),
            self.command_daemon.clone(),
            self.central_client.clone(),
            self.gateway_client.clone(),
        );
        agent.provider_config = self.default_provider_config.clone();
        agent.distiller = self.distiller.clone();
        // Store the sender for routing frontend messages to this agent
        self.frontend_channels.insert(id, agent.frontend_tx.clone());
        self.agents.insert(id, agent);
        id
    }

    /// Route a frontend message (approval, followup answer, user response) to the agent's channel.
    /// Returns true if the message was sent successfully.
    pub async fn send_to_agent(&self, agent_id: Uuid, msg: FrontendMessage) -> bool {
        if let Some(tx) = self.frontend_channels.get(&agent_id) {
            tx.send(msg).await.is_ok()
        } else {
            eprintln!("[di-core] send_to_agent: no channel for agent {}", agent_id);
            false
        }
    }

    /// Clean up the frontend channel for a finished agent.
    pub fn cleanup_agent(&mut self, agent_id: &Uuid) {
        self.frontend_channels.remove(agent_id);
    }

    /// Update the frontend response timeout for all agents.
    pub fn set_all_frontend_timeouts(&mut self, duration_ms: u64) {
        for agent in self.agents.values_mut() {
            agent.frontend_timeout_ms = Some(duration_ms);
        }
    }

    /// Set the provider config from the frontend. Stored as default for new agents
    /// and applied to all running agents immediately.
    pub fn set_provider_config(&mut self, config: crate::daemons::ProviderConfig) {
        self.default_provider_config = Some(config.clone());
        for agent in self.agents.values_mut() {
            agent.provider_config = Some(config.clone());
        }
    }

    /// Set the distiller config and create the distiller instance.
    /// Shares the distiller with all running and future agents.
    pub fn set_distiller_config(&mut self, config: crate::daemons::ProviderConfig) {
        self.distiller_config = Some(config.clone());
        self.distiller_config_version.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    }

    pub fn abort_agent(&mut self, agent_id: Uuid) -> bool {
        if let Some(agent) = self.agents.get(&agent_id) {
            agent.request_abort();
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
        self.send_to_agent(agent_id, FrontendMessage::UserResponse { agent_id, text }).await;
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
