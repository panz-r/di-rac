use crate::agent::{AgentState, AgentStatus, ChatMessage, MessageRole, PendingInput};
use crate::backend::DiCoreBackend;
use crate::input::InputBuffer;
use crate::message::{CoreEvent, FrontendMessage};
use crate::ui;
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Command,
}

pub struct App {
    pub agents: Vec<AgentState>,
    pub active_tab: usize,
    pub mode: Mode,
    pub input: InputBuffer,
    pub command_buffer: String,
    pub input_queue: Vec<(Uuid, PendingInput)>,
    pub queue_focused: bool,
    pub should_quit: bool,
    pub scroll_offset: usize,
    pub status_message: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            active_tab: 0,
            mode: Mode::Normal,
            input: InputBuffer::new(),
            command_buffer: String::new(),
            input_queue: Vec::new(),
            queue_focused: false,
            should_quit: false,
            scroll_offset: 0,
            status_message: None,
        }
    }

    pub fn active_agent(&self) -> Option<&AgentState> {
        self.agents.get(self.active_tab)
    }

    pub fn active_agent_mut(&mut self) -> Option<&mut AgentState> {
        self.agents.get_mut(self.active_tab)
    }

    /// Process a CoreEvent from di-core into state updates.
    pub fn handle_core_event(&mut self, event: CoreEvent) {
        let agent_id = match &event {
            CoreEvent::TaskInitialized { agent_id, .. } => *agent_id,
            CoreEvent::ThoughtDelta { agent_id, .. } => *agent_id,
            CoreEvent::ThoughtFinished { agent_id } => *agent_id,
            CoreEvent::ToolCallStarted { agent_id, .. } => *agent_id,
            CoreEvent::ToolCallFinished { agent_id, .. } => *agent_id,
            CoreEvent::ApprovalNeeded { agent_id, .. } => *agent_id,
            CoreEvent::FollowupQuestion { agent_id, .. } => *agent_id,
            CoreEvent::MetricsUpdate { agent_id, .. } => *agent_id,
            CoreEvent::TaskFinished { agent_id, .. } => *agent_id,
            CoreEvent::ContextCompacted { agent_id, .. } => *agent_id,
            CoreEvent::BackgroundCommandStarted { agent_id, .. } => *agent_id,
            CoreEvent::BackgroundCommandFinished { agent_id, .. } => *agent_id,
            CoreEvent::ObserverSignal { agent_id, .. } => *agent_id,
        };

        match event {
            CoreEvent::TaskInitialized { agent_id, .. } => {
                let idx = self.agents.len() + 1;
                let agent = AgentState::new(agent_id, format!("Agent-{}", idx));
                self.agents.push(agent);
                self.status_message = Some(format!("Agent-{} initialized", idx));
            }
            CoreEvent::ThoughtDelta { text, .. } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    if agent.streaming_text.is_none() {
                        agent.streaming_text = Some(String::new());
                        agent.status = AgentStatus::Running;
                    }
                    agent.streaming_text.as_mut().unwrap().push_str(&text);
                    agent.last_activity = Utc::now();
                }
            }
            CoreEvent::ThoughtFinished { .. } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    if let Some(text) = agent.streaming_text.take() {
                        if !text.is_empty() {
                            // Check if it's a thinking block
                            let (role, content) = if text.starts_with("[thinking] ") {
                                (MessageRole::System, text)
                            } else {
                                (MessageRole::Assistant, text)
                            };
                            agent.messages.push(ChatMessage {
                                role,
                                content,
                                tool_name: None,
                                timestamp: Utc::now(),
                            });
                        }
                    }
                    agent.last_activity = Utc::now();
                }
            }
            CoreEvent::ToolCallStarted { tool, args, .. } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    let summary = summarize_tool_args(&tool, &args);
                    agent.messages.push(ChatMessage {
                        role: MessageRole::Tool,
                        content: format!("{}({})", tool, summary),
                        tool_name: Some(tool),
                        timestamp: Utc::now(),
                    });
                    agent.status = AgentStatus::Running;
                    agent.last_activity = Utc::now();
                }
            }
            CoreEvent::ToolCallFinished { result, .. } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    let status = result
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("done");
                    let msg = if status == "denied" {
                        format!("Denied: {}", result.get("message").and_then(|v| v.as_str()).unwrap_or(""))
                    } else if status == "compacted" {
                        "Context compacted".to_string()
                    } else if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
                        format!("Error: {}", err)
                    } else {
                        format_result_summary(&result)
                    };
                    agent.messages.push(ChatMessage {
                        role: MessageRole::Tool,
                        content: msg,
                        tool_name: None,
                        timestamp: Utc::now(),
                    });
                    agent.last_activity = Utc::now();
                }
            }
            CoreEvent::ApprovalNeeded {
                tool, args, description, ..
            } => {
                let pending = PendingInput::Approval {
                    tool,
                    args,
                    description,
                };
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.pending_input = Some(pending.clone());
                    agent.status = AgentStatus::Waiting;
                    agent.last_activity = Utc::now();
                }
                self.input_queue.push((agent_id, pending));
            }
            CoreEvent::FollowupQuestion {
                question, options, ..
            } => {
                let pending = PendingInput::Followup {
                    question: question.clone(),
                    options: options.clone(),
                };
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.pending_input = Some(pending.clone());
                    agent.status = AgentStatus::Waiting;
                    agent.messages.push(ChatMessage {
                        role: MessageRole::Assistant,
                        content: question,
                        tool_name: None,
                        timestamp: Utc::now(),
                    });
                    agent.last_activity = Utc::now();
                }
                self.input_queue.push((agent_id, pending));
            }
            CoreEvent::MetricsUpdate {
                sqs,
                token_usage,
                latency_ms,
                ..
            } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.metrics = Some(crate::agent::Metrics {
                        sqs,
                        token_usage,
                        latency_ms,
                    });
                }
            }
            CoreEvent::TaskFinished {
                success, message, ..
            } => {
                self.input_queue.retain(|(id, _)| *id != agent_id);
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.status = if success {
                        AgentStatus::Finished
                    } else {
                        AgentStatus::Error
                    };
                    agent.finish_message = Some(message.clone());
                    agent.streaming_text = None;
                    agent.pending_input = None;
                    agent.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: if success {
                            format!("Task complete: {}", message)
                        } else {
                            format!("Task ended: {}", message)
                        },
                        tool_name: None,
                        timestamp: Utc::now(),
                    });
                    agent.last_activity = Utc::now();
                }
            }
            CoreEvent::ContextCompacted {
                remaining_tokens, ..
            } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: format!("Context compacted ({} tokens remaining)", remaining_tokens),
                        tool_name: None,
                        timestamp: Utc::now(),
                    });
                }
            }
            CoreEvent::BackgroundCommandStarted {
                command_id,
                command,
                ..
            } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: format!("Background: {} ({})", command, command_id),
                        tool_name: None,
                        timestamp: Utc::now(),
                    });
                }
            }
            CoreEvent::BackgroundCommandFinished {
                command_id,
                exit_code,
                ..
            } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: format!(
                            "Background {} done (exit: {})",
                            command_id,
                            exit_code.map(|c| c.to_string()).unwrap_or("?".to_string())
                        ),
                        tool_name: None,
                        timestamp: Utc::now(),
                    });
                }
            }
            CoreEvent::ObserverSignal { message, .. } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: format!("[observer] {}", message),
                        tool_name: None,
                        timestamp: Utc::now(),
                    });
                }
            }
        }
    }

    /// Handle a key event and optionally produce a message to send to di-core.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<FrontendMessage> {
        match self.mode {
            Mode::Normal => self.handle_normal_mode(key),
            Mode::Insert => self.handle_insert_mode(key),
            Mode::Command => self.handle_command_mode(key),
        }
    }

    fn handle_normal_mode(&mut self, key: KeyEvent) -> Option<FrontendMessage> {
        match key.code {
            KeyCode::Char('i') => {
                self.mode = Mode::Insert;
                None
            }
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
                None
            }
            KeyCode::Char('q') => {
                self.should_quit = true;
                None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_offset += 1;
                None
            }
            KeyCode::Char('G') => {
                self.scroll_offset = 0;
                None
            }
            KeyCode::Enter => {
                // If there's a pending input, respond to it
                self.respond_to_pending()
            }
            KeyCode::Char('g') => {
                // g prefix — check next key via state? For v1, just handle 'g' as noop
                None
            }
            _ => None,
        }
    }

    fn handle_insert_mode(&mut self, key: KeyEvent) -> Option<FrontendMessage> {
        // Handle modifier combos first
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('v') => {
                    self.input.toggle_multi_line();
                    return None;
                }
                KeyCode::Char('r') => {
                    // TODO: reverse search
                    return None;
                }
                _ => return None,
            }
        }

        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                None
            }
            KeyCode::Enter => {
                if self.input.multi_line {
                    self.input.insert('\n');
                    None
                } else {
                    self.submit_input()
                }
            }
            KeyCode::Char(c) => {
                self.input.insert(c);
                None
            }
            KeyCode::Backspace => {
                self.input.backspace();
                None
            }
            KeyCode::Delete => {
                self.input.delete();
                None
            }
            KeyCode::Left => {
                self.input.move_left();
                None
            }
            KeyCode::Right => {
                self.input.move_right();
                None
            }
            KeyCode::Home => {
                self.input.move_home();
                None
            }
            KeyCode::End => {
                self.input.move_end();
                None
            }
            KeyCode::Up => {
                self.input.history_up();
                None
            }
            KeyCode::Down => {
                self.input.history_down();
                None
            }
            _ => None,
        }
    }

    fn handle_command_mode(&mut self, key: KeyEvent) -> Option<FrontendMessage> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                None
            }
            KeyCode::Enter => {
                let cmd = self.command_buffer.trim().to_string();
                self.mode = Mode::Normal;
                self.command_buffer.clear();
                self.execute_command(&cmd)
            }
            KeyCode::Char(c) => {
                self.command_buffer.push(c);
                None
            }
            KeyCode::Backspace => {
                self.command_buffer.pop();
                if self.command_buffer.is_empty() {
                    self.mode = Mode::Normal;
                }
                None
            }
            _ => None,
        }
    }

    fn execute_command(&mut self, cmd: &str) -> Option<FrontendMessage> {
        match cmd {
            "q" | "quit" => {
                self.should_quit = true;
                None
            }
            "interrupt" => {
                if let Some(agent) = self.active_agent() {
                    let id = agent.id;
                    Some(FrontendMessage::Interrupt { agent_id: id })
                } else {
                    None
                }
            }
            _ if cmd.starts_with("new ") => {
                let task = cmd[4..].trim().to_string();
                if !task.is_empty() {
                    Some(FrontendMessage::SpawnAgent { task })
                } else {
                    None
                }
            }
            _ => {
                self.status_message = Some(format!("Unknown command: :{}", cmd));
                None
            }
        }
    }

    fn submit_input(&mut self) -> Option<FrontendMessage> {
        let text = self.input.submit();
        if text.is_empty() {
            return None;
        }

        // If queue is focused and there are pending inputs, respond to the first one
        if self.queue_focused && !self.input_queue.is_empty() {
            return self.respond_to_queue_item(&text);
        }

        // If the active agent has a pending input, respond to it
        if let Some(agent) = self.active_agent() {
            if let Some(pending) = &agent.pending_input {
                let agent_id = agent.id;
                return match pending {
                    PendingInput::Approval { .. } => {
                        let approved = matches!(text.to_lowercase().as_str(), "y" | "yes" | "");
                        self.clear_pending_for_agent(&agent_id);
                        Some(FrontendMessage::ApprovalResponse { agent_id, approved })
                    }
                    PendingInput::Followup { .. } => {
                        self.clear_pending_for_agent(&agent_id);
                        Some(FrontendMessage::FollowupAnswer { agent_id, text })
                    }
                };
            }
        }

        // Normal text input to agent
        if let Some(agent) = self.active_agent() {
            let agent_id = agent.id;
            // Add user message to conversation
            if let Some(agent) = self.active_agent_mut() {
                agent.messages.push(ChatMessage {
                    role: MessageRole::User,
                    content: text.clone(),
                    tool_name: None,
                    timestamp: Utc::now(),
                });
            }
            Some(FrontendMessage::UserResponse { agent_id, text })
        } else {
            None
        }
    }

    fn respond_to_pending(&mut self) -> Option<FrontendMessage> {
        if let Some(agent) = self.active_agent() {
            if let Some(pending) = &agent.pending_input {
                let agent_id = agent.id;
                return match pending {
                    PendingInput::Approval { .. } => {
                        self.clear_pending_for_agent(&agent_id);
                        Some(FrontendMessage::ApprovalResponse {
                            agent_id,
                            approved: true,
                        })
                    }
                    PendingInput::Followup { .. } => {
                        // Need user text — switch to insert mode
                        self.mode = Mode::Insert;
                        None
                    }
                };
            }
        }
        // No pending input — switch to insert mode
        self.mode = Mode::Insert;
        None
    }

    fn respond_to_queue_item(&mut self, text: &str) -> Option<FrontendMessage> {
        if let Some((agent_id, pending)) = self.input_queue.first().cloned() {
            let msg = match &pending {
                PendingInput::Approval { .. } => {
                    let approved = matches!(text.to_lowercase().as_str(), "y" | "yes" | "");
                    FrontendMessage::ApprovalResponse { agent_id, approved }
                }
                PendingInput::Followup { .. } => {
                    FrontendMessage::FollowupAnswer {
                        agent_id,
                        text: text.to_string(),
                    }
                }
            };
            self.input_queue.remove(0);
            self.clear_pending_for_agent(&agent_id);
            Some(msg)
        } else {
            None
        }
    }

    fn clear_pending_for_agent(&mut self, agent_id: &Uuid) {
        if let Some(agent) = self.find_agent_mut(agent_id) {
            agent.pending_input = None;
            if agent.status == AgentStatus::Waiting {
                agent.status = AgentStatus::Running;
            }
        }
        self.input_queue.retain(|(id, _)| id != agent_id);
    }

    fn find_agent_mut(&mut self, id: &Uuid) -> Option<&mut AgentState> {
        self.agents.iter_mut().find(|a| a.id == *id)
    }

    /// Render the full UI.
    pub fn view(&self, frame: &mut Frame) {
        ui::render(frame, self);
    }
}

fn summarize_tool_args(tool: &str, args: &serde_json::Value) -> String {
    match tool {
        "read" => args.get("path").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
        "write" => args.get("path").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
        "edit" => args.get("path").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
        "bash" => {
            let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("?");
            if cmd.len() > 60 {
                format!("{}...", &cmd[..57])
            } else {
                cmd.to_string()
            }
        }
        "search" => args.get("pattern").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
        _ => args.to_string().chars().take(40).collect(),
    }
}

fn format_result_summary(result: &serde_json::Value) -> String {
    if let Some(s) = result.get("status").and_then(|v| v.as_str()) {
        s.to_string()
    } else if let Some(s) = result.get("stdout").and_then(|v| v.as_str()) {
        let lines: Vec<&str> = s.lines().take(3).collect();
        lines.join("\n")
    } else {
        let s = result.to_string();
        if s.len() > 80 {
            format!("{}...", &s[..77])
        } else {
            s
        }
    }
}
