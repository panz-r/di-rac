use crate::agent::trajectory::{Trajectory, Role};
use crate::agent::parser::{ResponseParser, StreamingToolAccumulator};
use crate::agent::file_context::FileContextTracker;
use crate::agent::environment::EnvironmentManager;
use crate::observer::Observer;
use crate::context::ContextManager;
use crate::daemons::{
    UnixDaemonClient, GatewayStreamClient, GatewayRequest, GatewayMessage,
    CentralRequest, CentralResponse,
};
use crate::protocol::{CoreEvent, FrontendMessage};
use crate::tools::{ToolExecutor, ToolCoordinator};
use crate::tools::background::BackgroundCommandTracker;
use crate::tools::approval::ApprovalManager;
use anyhow::{Result, anyhow};
use serde_json::{json, Value};
use std::collections::{HashSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub enum AgentMode {
    Plan,
    Act,
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
    pub analyzer_client: Arc<UnixDaemonClient>,
    pub command_client: Arc<UnixDaemonClient>,
    pub central_client: Arc<UnixDaemonClient>,
    pub gateway_client: Arc<GatewayStreamClient>,
    pub tool_executor: ToolExecutor,
    pub coordinator: ToolCoordinator,
    pub approval_manager: ApprovalManager,
    pub background_tracker: Arc<BackgroundCommandTracker>,
    pub parser: ResponseParser,
    pub abort: Arc<AtomicBool>,
    pub consecutive_mistake_count: usize,
    pub max_consecutive_mistakes: usize,
    pub request_id_counter: i64,
    pub frontend_rx: Option<mpsc::Receiver<FrontendMessage>>,
    pub frontend_tx: mpsc::Sender<FrontendMessage>,
    pub mode: AgentMode,
    pub file_context: FileContextTracker,
    pub environment: EnvironmentManager,
    /// How long (ms) to wait for frontend responses before timing out.
    /// Set to Some(0) to disable timeout (indefinite wait). None uses default.
    pub frontend_timeout_ms: Option<u64>,
}

impl AgentEngine {
    pub fn new(
        id: Uuid,
        analyzer_client: Arc<UnixDaemonClient>,
        command_client: Arc<UnixDaemonClient>,
        central_client: Arc<UnixDaemonClient>,
        gateway_client: Arc<GatewayStreamClient>,
    ) -> Self {
        let background_tracker = Arc::new(BackgroundCommandTracker::new());
        let (frontend_tx, frontend_rx) = mpsc::channel(32);
        Self {
            id,
            trajectory: Trajectory::new(),
            observer: Observer::new(),
            context_manager: ContextManager::new(32000, 24000),
            analyzer_client: analyzer_client.clone(),
            command_client: command_client.clone(),
            central_client,
            gateway_client,
            tool_executor: ToolExecutor::new(analyzer_client, command_client, background_tracker.clone()),
            coordinator: ToolCoordinator::new(),
            approval_manager: ApprovalManager::new(),
            background_tracker,
            parser: ResponseParser::new(),
            abort: Arc::new(AtomicBool::new(false)),
            consecutive_mistake_count: 0,
            max_consecutive_mistakes: 3,
            request_id_counter: 0,
            frontend_rx: Some(frontend_rx),
            frontend_tx,
            mode: AgentMode::Act,
            file_context: FileContextTracker::new(),
            environment: EnvironmentManager::new(),
            frontend_timeout_ms: None,
        }
    }

    /// Receive from the frontend channel with the current timeout.
    /// Returns None on timeout or channel closure.
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

    /// Run a complete task: loop over turns until completion, abort, or mistake limit.
    pub async fn run_task(&mut self, initial_task: String) -> Result<()> {
        self.trajectory.add_message(Role::User, json!(initial_task), initial_task.len() / 4);
        self.emit_event(CoreEvent::TaskInitialized {
            agent_id: self.id,
            history_count: 0,
        })?;

        loop {
            if self.is_aborted() {
                self.emit_event(CoreEvent::TaskFinished {
                    agent_id: self.id,
                    success: false,
                    message: "Interrupted by user".to_string(),
                })?;
                return Ok(());
            }

            let tools_used = match self.run_turn().await {
                Ok(count) => count,
                Err(e) => {
                    self.emit_event(CoreEvent::TaskFinished {
                        agent_id: self.id,
                        success: false,
                        message: format!("Error: {}", e),
                    })?;
                    return Err(e);
                }
            };

            if self.is_aborted() {
                self.emit_event(CoreEvent::TaskFinished {
                    agent_id: self.id,
                    success: false,
                    message: "Interrupted by user".to_string(),
                })?;
                return Ok(());
            }

            if tools_used == 0 {
                self.consecutive_mistake_count += 1;
                if self.consecutive_mistake_count >= self.max_consecutive_mistakes {
                    self.emit_event(CoreEvent::TaskFinished {
                        agent_id: self.id,
                        success: false,
                        message: "Too many consecutive turns without tool use".to_string(),
                    })?;
                    return Ok(());
                }
                // Inject nudge
                self.trajectory.add_message(
                    Role::User,
                    json!("You must respond with a tool call. Use the available tools to make progress on the task."),
                    20,
                );
            } else {
                self.consecutive_mistake_count = 0;
            }
        }
    }

    /// Execute one turn of the agent loop. Returns the number of tools used.
    pub async fn run_turn(&mut self) -> Result<usize> {
        // 0. Fetch config and gather environment
        let system_prompt = self.get_config("system_prompt").await?
            .as_str().unwrap_or("You are a helpful assistant.").to_string();
        let _model = self.get_config("model").await?
            .as_str().unwrap_or("gpt-4").to_string();

        // Refresh environment details if not yet gathered
        if self.environment.get_details().is_none() {
            self.environment.gather();
        }

        // 1. API extraction
        let current_apis = self.extract_current_apis()?;

        // 2. Auto-compaction check
        if self.context_manager.should_auto_compact(&self.trajectory) {
            self.trajectory.add_message(
                Role::User,
                json!(ContextManager::auto_compact_instruction()),
                100,
            );
        }

        // 3. Build prompt with environment and file context
        let bg_summary = self.background_tracker.get_summary().await;
        let env_details = self.environment.get_details().map(String::from);
        let file_ctx = self.file_context.get_summary();
        let file_ctx_ref = if file_ctx.is_empty() { None } else { Some(file_ctx.as_str()) };

        let mut enriched_prompt = system_prompt.clone();
        if let Some(env) = &env_details {
            enriched_prompt.push_str(&format!("\n\n{}", env));
        }
        if let Some(fc) = file_ctx_ref {
            enriched_prompt.push_str(&format!("\n\n{}", fc));
        }
        if self.mode == AgentMode::Plan {
            enriched_prompt.push_str("\n\n[Plan Mode] You may only use read-only tools, ask_followup_question, attempt_completion, and compact. Do not modify any files.");
        }

        let messages = self.context_manager.build_prompt(
            &enriched_prompt,
            &self.trajectory,
            &current_apis,
            bg_summary.as_deref(),
        );

        // 4. Observer
        let sqs = self.observer.compute_sqs(&self.trajectory);
        self.emit_event(CoreEvent::MetricsUpdate {
            agent_id: self.id,
            sqs: sqs.score,
            token_usage: messages.iter().map(|m| m.tokens).sum(),
            latency_ms: 0,
        })?;

        // 5. Streaming LLM call
        self.request_id_counter += 1;
        let gateway_msgs: Vec<GatewayMessage> = messages.iter().map(|m| GatewayMessage {
            role: match m.role {
                Role::User => "user".to_string(),
                Role::Assistant => "assistant".to_string(),
                Role::System => "system".to_string(),
                Role::Tool => "tool".to_string(),
            },
            content: m.content.clone(),
        }).collect();

        let request = GatewayRequest {
            id: self.request_id_counter,
            stream: true,
            provider: None, // uses the gateway's default provider
            messages: gateway_msgs,
            system: None,
            tools: None,
            max_tokens: None,
            temperature: None,
            thinking: None,
            timeout: Some(240000),
        };

        let mut chunk_rx = self.gateway_client.stream_chat(request).await?;

        // 6. Accumulate streaming response
        let mut full_text = String::new();
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
                    self.trajectory.add_message(Role::Assistant, json!(full_text.clone()), full_text.len() / 4);
                    return Err(e);
                }
            };

            match chunk.chunk_type.as_str() {
                "delta" => {
                    if !tool_accumulator.feed_chunk(&chunk) {
                        // Text delta
                        if let Some(text) = &chunk.text_delta {
                            full_text.push_str(text);
                            let _ = self.emit_event(CoreEvent::ThoughtDelta {
                                agent_id: self.id,
                                text: text.clone(),
                            });
                        }
                        if let Some(thinking) = &chunk.thinking {
                            // Reasoning delta — emit for UI but don't add to main text
                            let _ = self.emit_event(CoreEvent::ThoughtDelta {
                                agent_id: self.id,
                                text: format!("[thinking] {}", thinking),
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

        // Record assistant thought
        self.trajectory.add_message(Role::Assistant, json!(full_text.clone()), full_text.len() / 4);
        self.emit_event(CoreEvent::ThoughtFinished { agent_id: self.id })?;

        // Finalize tool calls (native + XML fallback)
        let tools = tool_accumulator.finalize(&full_text);

        // 7. Execute tools
        for tool in &tools {
            if self.is_aborted() {
                break;
            }

            // Mode gate: Plan mode restricts to read-only tools
            if self.mode == AgentMode::Plan && !PLAN_MODE_TOOLS.contains(&tool.name.as_str()) {
                let skip_msg = json!({ "status": "blocked", "message": format!("Tool '{}' not allowed in Plan mode", tool.name) });
                self.trajectory.add_message(Role::Tool, skip_msg.clone(), 50);
                self.emit_event(CoreEvent::ToolCallFinished {
                    agent_id: self.id,
                    result: skip_msg,
                })?;
                continue;
            }

            // Track file context
            match tool.name.as_str() {
                "read" | "search" | "repo" | "symbols" => {
                    if let Some(path) = tool.args.get("path").and_then(|v| v.as_str()) {
                        self.file_context.mark_read(path);
                    }
                }
                "write" | "edit" => {
                    if let Some(path) = tool.args.get("path").and_then(|v| v.as_str()) {
                        self.file_context.mark_edited(path);
                    }
                }
                _ => {}
            }

            // Approval gate: check if tool needs user approval
            if !self.approval_manager.should_auto_approve(&tool.name) {
                let description = format!("Execute {} on behalf of agent", tool.name);
                self.emit_event(CoreEvent::ApprovalNeeded {
                    agent_id: self.id,
                    tool: tool.name.clone(),
                    args: tool.args.clone(),
                    description: description.clone(),
                })?;

                // Block waiting for approval response from frontend
                let msg = self.recv_frontend().await;
                let approved = match msg {
                    Some(FrontendMessage::ApprovalResponse { approved, .. }) => approved,
                    Some(FrontendMessage::Timeout { duration_ms }) => {
                        self.frontend_timeout_ms = Some(duration_ms);
                        self.emit_event(CoreEvent::FrontendTimeout {
                            agent_id: self.id,
                            tool: Some(tool.name.clone()),
                            question: None,
                        })?;
                        false
                    }
                    _ => {
                        // Channel closed or unexpected message — treat as denial
                        self.emit_event(CoreEvent::FrontendTimeout {
                            agent_id: self.id,
                            tool: Some(tool.name.clone()),
                            question: None,
                        })?;
                        false
                    }
                };

                if !approved {
                    let skip_msg = json!({ "status": "denied", "message": "Frontend timeout or denial" });
                    self.trajectory.add_message(Role::Tool, skip_msg.clone(), 50);
                    self.emit_event(CoreEvent::ToolCallFinished {
                        agent_id: self.id,
                        result: skip_msg,
                    })?;
                    continue;
                }
            }

            self.emit_event(CoreEvent::ToolCallStarted {
                agent_id: self.id,
                tool: tool.name.clone(),
                args: tool.args.clone(),
            })?;

            match self.tool_executor.execute(tool, &mut self.coordinator).await {
                Ok(result) => {
                    // Handle frontend-interactive tools
                    let action = result.get("_frontend_action").and_then(|v| v.as_str());

                    if action == Some("attempt_completion") || action == Some("plan_response") {
                        // Both done and plan tools can signal completion
                        if action == Some("plan_response") {
                            // Plan mode: emit the plan, don't abort
                            let plan = result.get("plan").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let plan_json = json!({ "plan": plan, "status": "planned" });
                            self.trajectory.add_message(Role::Tool, plan_json.clone(), 50);
                            self.emit_event(CoreEvent::ToolCallFinished {
                                agent_id: self.id,
                                result: plan_json,
                            })?;
                        } else {
                        let message = result.get("result").and_then(|v| v.as_str()).unwrap_or("Task complete").to_string();
                        self.emit_event(CoreEvent::ToolCallFinished {
                            agent_id: self.id,
                            result: json!({ "status": "completed", "message": &message }),
                        })?;
                        self.emit_event(CoreEvent::TaskFinished {
                            agent_id: self.id,
                            success: true,
                            message,
                        })?;
                        self.request_abort();
                        return Ok(tools.len());
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

                        // Block waiting for followup answer from frontend
                        let msg = self.recv_frontend().await;
                        let answer_text = match msg {
                            Some(FrontendMessage::FollowupAnswer { text, .. }) => text,
                            Some(FrontendMessage::Timeout { duration_ms }) => {
                                self.frontend_timeout_ms = Some(duration_ms);
                                self.emit_event(CoreEvent::FrontendTimeout {
                                    agent_id: self.id,
                                    tool: None,
                                    question: Some(question.clone()),
                                })?;
                                String::new()
                            }
                            _ => {
                                self.emit_event(CoreEvent::FrontendTimeout {
                                    agent_id: self.id,
                                    tool: None,
                                    question: Some(question.clone()),
                                })?;
                                String::new()
                            }
                        };

                        let answer_json = json!({ "question": question, "answer": answer_text, "status": "answered" });
                        self.trajectory.add_message(Role::Tool, answer_json.clone(), 50);
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
                        return Ok(tools.len());
                    } else if result.get("compact").and_then(|v| v.as_bool()).unwrap_or(false) {
                        let summary = result.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                        self.perform_compaction(summary).await?;
                        self.emit_event(CoreEvent::ToolCallFinished {
                            agent_id: self.id,
                            result: json!({ "status": "compacted" }),
                        })?;
                    } else {
                        self.trajectory.add_message(Role::Tool, result.clone(), 100);
                        self.emit_event(CoreEvent::ToolCallFinished {
                            agent_id: self.id,
                            result,
                        })?;
                    }
                }
                Err(e) => {
                    let error_msg = json!({ "error": e.to_string() });
                    self.trajectory.add_message(Role::Tool, error_msg.clone(), 50);
                    self.emit_event(CoreEvent::ToolCallFinished {
                        agent_id: self.id,
                        result: error_msg,
                    })?;
                }
            }
        }

        Ok(tools.len())
    }

    async fn perform_compaction(&mut self, summary: &str) -> Result<()> {
        let mut continuation = ContextManager::continuation_prompt(summary);
        if let Some(bg) = self.background_tracker.get_summary().await {
            continuation.push_str(&format!("\n\n{}", bg));
        }
        self.trajectory.truncate_with_continuation(continuation);
        self.emit_event(CoreEvent::ContextCompacted {
            agent_id: self.id,
            remaining_tokens: self.trajectory.get_total_tokens(),
        })?;
        Ok(())
    }

    async fn get_config(&self, key: &str) -> Result<Value> {
        let resp: CentralResponse = self.central_client.send_request(CentralRequest {
            command: "get".to_string(),
            key: Some(key.to_string()),
            value: None,
        })?;

        if resp.ok {
            Ok(resp.value.unwrap_or(Value::Null))
        } else {
            Err(anyhow!("Failed to fetch config for {}: {:?}", key, resp.error))
        }
    }

    fn extract_current_apis(&self) -> Result<HashSet<String>> {
        let mut apis = HashSet::new();
        if let Some(msg) = self.trajectory.messages.iter().filter(|m| matches!(m.role, Role::Assistant)).last() {
            let content = msg.content.to_string();
            if let Ok(resp) = self.analyzer_client.extract_apis(&content, "python") {
                for call in resp.calls { apis.insert(call); }
                for def in resp.definitions { apis.insert(def); }
            }
        }
        Ok(apis)
    }

    fn emit_event(&self, event: CoreEvent) -> Result<()> {
        println!("{}", serde_json::to_string(&event)?);
        Ok(())
    }
}

pub struct MultiAgentOrchestrator {
    pub agents: HashMap<Uuid, AgentEngine>,
    pub frontend_channels: HashMap<Uuid, mpsc::Sender<FrontendMessage>>,
    pub analyzer_client: Arc<UnixDaemonClient>,
    pub command_client: Arc<UnixDaemonClient>,
    pub central_client: Arc<UnixDaemonClient>,
    pub gateway_client: Arc<GatewayStreamClient>,
}

impl MultiAgentOrchestrator {
    pub fn new(analyzer_socket: &str, command_socket: &str, central_socket: &str, gateway_socket: &str) -> Self {
        Self {
            agents: HashMap::new(),
            frontend_channels: HashMap::new(),
            analyzer_client: Arc::new(UnixDaemonClient::new(analyzer_socket)),
            command_client: Arc::new(UnixDaemonClient::new(command_socket)),
            central_client: Arc::new(UnixDaemonClient::new(central_socket)),
            gateway_client: Arc::new(GatewayStreamClient::with_socket(gateway_socket)),
        }
    }

    pub fn spawn_agent(&mut self, task: String) -> Uuid {
        let id = Uuid::new_v4();
        let agent = AgentEngine::new(
            id,
            self.analyzer_client.clone(),
            self.command_client.clone(),
            self.central_client.clone(),
            self.gateway_client.clone(),
        );
        // Store the sender for routing frontend messages to this agent
        self.frontend_channels.insert(id, agent.frontend_tx.clone());
        self.agents.insert(id, agent);
        id
    }

    /// Route a frontend message (approval, followup answer) to the agent's channel.
    pub async fn send_to_agent(&self, agent_id: Uuid, msg: FrontendMessage) -> bool {
        if let Some(tx) = self.frontend_channels.get(&agent_id) {
            tx.send(msg).await.is_ok()
        } else {
            false
        }
    }

    /// Update the frontend response timeout for all agents.
    pub fn set_all_frontend_timeouts(&mut self, duration_ms: u64) {
        for agent in self.agents.values_mut() {
            agent.frontend_timeout_ms = Some(duration_ms);
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
        self.agents.remove(&agent_id)
    }

    pub async fn handle_user_response(&mut self, agent_id: Uuid, text: String) -> Result<()> {
        if let Some(agent) = self.agents.get_mut(&agent_id) {
            agent.trajectory.add_message(Role::User, json!(text), text.len() / 4);
            agent.run_turn().await?;
        }
        Ok(())
    }

    pub fn emit_event(&self, event: CoreEvent) -> Result<()> {
        println!("{}", serde_json::to_string(&event)?);
        Ok(())
    }
}
