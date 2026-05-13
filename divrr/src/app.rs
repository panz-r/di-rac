use crate::agent::{AgentState, AgentStatus, PendingInput};
use crate::input::InputBuffer;
use crate::message::{CoreEvent, FrontendMessage};
use crate::settings::{SettingsLoadResult, SettingsState};
use crate::ui;

/// Append a timestamped line to ~/.dirac/divrr.log (best-effort, never fails).
pub fn log_event(msg: &str) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let path = std::path::Path::new(&home).join(".dirac").join("divrr.log");
    let _ = std::fs::OpenOptions::new().append(true).create(true).open(&path)
        .map(|mut f| {
            use std::io::Write;
            let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let _ = writeln!(f, "[{}] {}", ts, msg);
        });
}

/// Prefix prepended to thinking content in System blocks.
pub const THINKING_PREFIX: char = '\u{00B7}';
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub name: &'static str,
    pub description: &'static str,
    pub prefix: &'static str,
}

const COMMANDS: &[CommandEntry] = &[
    CommandEntry { name: "settings", description: "Open provider settings panel", prefix: "" },
    CommandEntry { name: "quit", description: "Exit divrr", prefix: "q" },
    CommandEntry { name: "interrupt", description: "Interrupt active agent", prefix: "" },
    CommandEntry { name: "new", description: "Spawn a new agent with a task", prefix: "" },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Command,
    Settings,
    Action,
    SaveDialog,
}

pub struct SaveDialogState {
    pub path: String,
    pub cursor: usize,
    pub exists_warned: bool,
    pub block_text: String,
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
    pub content_lines: usize,
    pub visible_lines: usize,
    pub auto_scroll: bool,
    /// Block-level cursor — index into the active agent's block list.
    pub selected_block: usize,
    pub action_cursor: usize,
    pub status_message: Option<String>,
    pub settings: Option<SettingsState>,
    pub command_palette: Vec<CommandEntry>,
    pub palette_cursor: usize,
    /// Auto-approve all tool calls without prompting.
    pub auto_approve: bool,
    /// Pending messages to send to di-core (populated by settings save).
    pub pending_messages: Vec<FrontendMessage>,
    /// Channel to send async results back to the event loop.
    pub event_tx: Option<mpsc::UnboundedSender<crate::AppEvent>>,
    /// Active save dialog state.
    pub save_dialog: Option<Box<SaveDialogState>>,
    /// Expand state saved before entering Action mode, restored on close.
    pub saved_expanded: Option<std::collections::HashSet<usize>>,
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
            content_lines: 0,
            visible_lines: 24,
            auto_scroll: true,
            selected_block: 0,
            action_cursor: 0,
            status_message: None,
            settings: None,
            command_palette: Vec::new(),
            palette_cursor: 0,
            auto_approve: false,
            pending_messages: Vec::new(),
            event_tx: None,
            save_dialog: None,
            saved_expanded: None,
        }
    }

    pub fn active_agent(&self) -> Option<&AgentState> {
        self.agents.get(self.active_tab)
    }

    pub fn active_agent_mut(&mut self) -> Option<&mut AgentState> {
        self.agents.get_mut(self.active_tab)
    }

    /// Count rendered lines using the canonical log representation.
    pub fn count_rendered_lines(&self, width: usize) -> usize {
        self.active_agent().map(|a| {
            let mut total = a.log.total_lines(width, &a.expanded);
            if a.pending_input.is_some() { total += 1; }
            total
        }).unwrap_or(0)
    }

    pub fn clamp_scroll(&mut self) {
        if self.content_lines > self.visible_lines {
            let max_scroll = self.content_lines.saturating_sub(self.visible_lines);
            if self.auto_scroll {
                self.scroll_offset = max_scroll;
                // Keep selected_block on the last block when auto-scrolling
                if let Some(agent) = self.active_agent() {
                    let count = agent.log.blocks().len();
                    if count > 0 {
                        self.selected_block = count - 1;
                    }
                }
            } else {
                self.scroll_offset = self.scroll_offset.min(max_scroll);
            }
        } else {
            self.scroll_offset = 0;
        }
    }

    /// Move the block selection cursor by `delta` (positive = down, negative = up).
    /// Adjusts scroll_offset to keep the selected block visible.
    fn move_block_cursor(&mut self, delta: i32) {
        let block_count = self.active_agent()
            .map(|a| a.log.blocks().len())
            .unwrap_or(0);
        if block_count == 0 {
            return;
        }

        // Compute new selected_block
        let new = if delta > 0 {
            self.selected_block.saturating_add(delta as usize).min(block_count - 1)
        } else {
            self.selected_block.saturating_sub((-delta) as usize)
        };
        self.selected_block = new;

        // Compute the line offset where the selected block starts
        let block_start = self.active_agent()
            .map(|a| {
                let width = 80;
                let mut acc = 0usize;
                for (i, block) in a.log.blocks().iter().enumerate() {
                    if i == new { break; }
                    let is_expanded = a.expanded.contains(&i);
                    acc += crate::agent::block_line_count(block, width, is_expanded);
                }
                acc
            })
            .unwrap_or(0);

        // Compute the height of the selected block
        let block_height = self.active_agent()
            .map(|a| {
                let width = 80;
                let block = &a.log.blocks()[new];
                crate::agent::block_line_count(block, width, a.expanded.contains(&new))
            })
            .unwrap_or(1);

        // Scroll to keep the selected block visible
        let visible = self.visible_lines;
        if block_start < self.scroll_offset {
            self.scroll_offset = block_start;
        } else if block_start + block_height > self.scroll_offset + visible {
            self.scroll_offset = block_start + block_height.saturating_sub(visible);
        }
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
            CoreEvent::TaskPresented { agent_id, .. } => *agent_id,
            CoreEvent::ContextCompacted { agent_id, .. } => *agent_id,
            CoreEvent::BackgroundCommandStarted { agent_id, .. } => *agent_id,
            CoreEvent::BackgroundCommandFinished { agent_id, .. } => *agent_id,
            CoreEvent::ObserverSignal { agent_id, .. } => *agent_id,
            CoreEvent::FrontendTimeout { agent_id, .. } => *agent_id,
        };

        match event {
            CoreEvent::TaskInitialized { agent_id, .. } => {
                let idx = self.agents.len() + 1;
                let agent = AgentState::new(agent_id, format!("Agent-{}", idx));
                self.agents.push(agent);
                self.active_tab = self.active_tab.min(self.agents.len() - 1);
                self.status_message = Some(format!("Agent-{} initialized", idx));
            }
            CoreEvent::ThoughtDelta { text, thinking, .. } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    let is_thinking = thinking;
                    let was_thinking = agent.log.streaming_is_thinking();
                    let has_streaming = agent.log.streaming().is_some();

                    if !has_streaming {
                        agent.log.set_streaming(
                            if is_thinking { format!("{} {}", THINKING_PREFIX, text) } else { text.clone() },
                            is_thinking,
                        );
                        agent.status = AgentStatus::Running;
                    } else if was_thinking != is_thinking {
                        agent.log.finalize_streaming();
                        agent.log.set_streaming(
                            if is_thinking { format!("{} {}", THINKING_PREFIX, text) } else { text.clone() },
                            is_thinking,
                        );
                    } else {
                        agent.log.append_streaming(&text);
                    }
                    agent.last_activity = Utc::now();
                }
            }
            CoreEvent::ThoughtFinished { .. } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.log.finalize_streaming();
                    agent.last_activity = Utc::now();
                }
            }
            CoreEvent::ToolCallStarted { tool, args, .. } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    let summary = summarize_tool_args(&tool, &args);
                    agent.log.push_tool_call(tool, summary);
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
                    agent.log.set_tool_result(msg);
                    agent.last_activity = Utc::now();
                }
            }
            CoreEvent::ApprovalNeeded {
                tool, args, description, ..
            } => {
                if self.auto_approve {
                    self.pending_messages.push(FrontendMessage::ApprovalResponse {
                        agent_id,
                        approved: true,
                    });
                } else {
                    // Remove any existing pending input for this agent to prevent duplicates
                    self.input_queue.retain(|(id, _)| *id != agent_id);
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
            }
            CoreEvent::FollowupQuestion {
                question, options, ..
            } => {
                // Remove any existing pending input for this agent to prevent duplicates
                self.input_queue.retain(|(id, _)| *id != agent_id);
                let pending = PendingInput::Followup {
                    question: question.clone(),
                    options: options.clone(),
                };
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.pending_input = Some(pending.clone());
                    agent.status = AgentStatus::Waiting;
                    agent.log.push_assistant(question);
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
                    agent.log.clear_streaming();
                    agent.pending_input = None;
                    let msg = if success {
                        format!("Task complete: {}", message)
                    } else {
                        format!("Task ended: {}", message)
                    };
                    agent.log.push_finish(msg, success);
                    agent.last_activity = Utc::now();
                }
            }
            CoreEvent::TaskPresented { message, .. } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.status = AgentStatus::Waiting;
                    agent.pending_input = None;
                    agent.log.push_system(format!("Result: {}", message));
                    agent.last_activity = Utc::now();
                }
            }
            CoreEvent::ContextCompacted {
                remaining_tokens, ..
            } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.log.push_system(format!("Context compacted ({} tokens remaining)", remaining_tokens));
                }
            }
            CoreEvent::BackgroundCommandStarted {
                command_id,
                command,
                ..
            } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.log.push_system(format!("Background: {} ({})", command, command_id));
                }
            }
            CoreEvent::BackgroundCommandFinished {
                command_id,
                exit_code,
                ..
            } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.log.push_system(format!(
                        "Background {} done (exit: {})",
                        command_id,
                        exit_code.map(|c| c.to_string()).unwrap_or("?".to_string())
                    ));
                }
            }
            CoreEvent::ObserverSignal { message, .. } => {
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.log.push_system(format!("[observer] {}", message));
                }
            }
            CoreEvent::FrontendTimeout { tool, question, .. } => {
                // Remove stale queue items for this agent
                self.input_queue.retain(|(id, _)| *id != agent_id);
                if let Some(agent) = self.find_agent_mut(&agent_id) {
                    agent.pending_input = None;
                    if agent.status == AgentStatus::Waiting {
                        agent.status = AgentStatus::Running;
                    }
                    let detail = tool.as_deref().unwrap_or_else(|| question.as_deref().unwrap_or("unknown"));
                    agent.log.push_system(format!("Timed out waiting for response: {}", detail));
                }
            }
        }
    }

    /// Handle a key event and optionally produce a message to send to di-core.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<FrontendMessage> {
        if key.kind != KeyEventKind::Press {
            return None;
        }

        match self.mode {
            Mode::Normal => self.handle_normal_mode(key),
            Mode::Insert => self.handle_insert_mode(key),
            Mode::Command => self.handle_command_mode(key),
            Mode::Settings => self.handle_settings_mode(key),
            Mode::Action => self.handle_action_mode(key),
            Mode::SaveDialog => self.handle_save_dialog(key),
        }
    }

    /// Enter Action mode: save expand state, force-expand selected block.
    fn enter_action_mode(&mut self) {
        if let Some(agent) = self.active_agent() {
            self.saved_expanded = Some(agent.expanded.clone());
        }
        let idx = self.selected_block;
        if let Some(agent) = self.active_agent_mut() {
            agent.expanded.insert(idx);
        }
        self.mode = Mode::Action;
        self.action_cursor = 0;
    }

    /// Exit Action mode: restore saved expand state (unless user explicitly toggled).
    fn exit_action_mode(&mut self) {
        if let Some(saved) = self.saved_expanded.take() {
            if let Some(agent) = self.active_agent_mut() {
                agent.expanded = saved;
            }
        }
        self.mode = Mode::Normal;
    }

    pub fn show_action_palette(&self) -> bool {
        true
    }

    pub fn handle_paste(&mut self, text: &str) {
        match self.mode {
            Mode::Settings => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        s.secret_edit_buffer.insert_str(s.secret_edit_cursor, text);
                        s.secret_edit_cursor += text.len();
                    }
                }
            }
            Mode::SaveDialog => {
                if let Some(d) = &mut self.save_dialog {
                    let byte_pos = char_to_byte(&d.path, d.cursor);
                    d.path.insert_str(byte_pos, text);
                    d.cursor += text.chars().count();
                    d.exists_warned = false;
                }
            }
            Mode::Insert => {
                for c in text.chars() {
                    self.input.insert(c);
                }
            }
            _ => {}
        }
    }

    fn handle_settings_mode(&mut self, key: KeyEvent) -> Option<FrontendMessage> {
        match key.code {
            KeyCode::Esc => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        s.cancel_secret_edit();
                    } else if s.selector_open {
                        s.cancel_selector();
                    } else {
                        self.settings = None;
                        self.mode = Mode::Normal;
                    }
                }
            }
            KeyCode::Up => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        // no-op
                    } else if s.selector_open {
                        s.move_up();
                    } else {
                        s.move_up();
                    }
                }
            }
            KeyCode::Down => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        // no-op
                    } else if s.selector_open {
                        s.move_down();
                    } else {
                        s.move_down();
                    }
                }
            }
            KeyCode::Left => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        s.secret_edit_left();
                    } else if !s.selector_open {
                        s.select_left();
                    }
                }
            }
            KeyCode::Right => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        s.secret_edit_right();
                    } else if !s.selector_open {
                        s.select_right();
                    }
                }
            }
            KeyCode::Home => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        s.secret_edit_home();
                    }
                }
            }
            KeyCode::End => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        s.secret_edit_end();
                    }
                }
            }
            KeyCode::Delete => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        s.secret_edit_delete();
                    }
                }
            }
            KeyCode::Tab => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        // no-op in secret edit
                    } else {
                        let fo = s.field_offset();
                        if s.cursor > 0 && fo < s.fields.len() && s.fields[fo].kind() == crate::settings::FieldKind::Secret {
                            s.open_secret_edit();
                        } else {
                            s.open_selector();
                        }
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        s.confirm_secret_edit();
                    } else {
                        self.pending_messages = s.save();
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        s.secret_edit_backspace();
                    } else {
                        s.backspace();
                    }
                }
            }
            KeyCode::Char(c) => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        s.secret_edit_type_char(c);
                    } else {
                        s.type_char(c);
                    }
                }
            }
            _ => {}
        }
        None
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
                self.refresh_palette();
                None
            }
            KeyCode::Char('q') => {
                self.should_quit = true;
                None
            }
            KeyCode::Char('Q') => {
                self.queue_focused = !self.queue_focused;
                self.status_message = if self.queue_focused {
                    Some("Queue focused".to_string())
                } else {
                    Some("Queue unfocused".to_string())
                };
                None
            }
            KeyCode::Char('j') => {
                self.auto_scroll = false;
                self.scroll_offset += 1;
                self.clamp_scroll();
                None
            }
            KeyCode::Char('k') => {
                self.auto_scroll = false;
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                None
            }
            KeyCode::Down => {
                self.auto_scroll = false;
                self.move_block_cursor(1);
                None
            }
            KeyCode::Up => {
                self.auto_scroll = false;
                self.move_block_cursor(-1);
                None
            }
            KeyCode::PageDown | KeyCode::Char('J') => {
                self.auto_scroll = false;
                let page = self.visible_lines.saturating_sub(2).max(1);
                self.scroll_offset = self.scroll_offset.saturating_add(page);
                self.clamp_scroll();
                None
            }
            KeyCode::PageUp | KeyCode::Char('K') => {
                self.auto_scroll = false;
                let page = self.visible_lines.saturating_sub(2).max(1);
                self.scroll_offset = self.scroll_offset.saturating_sub(page);
                None
            }
            KeyCode::Home => {
                self.auto_scroll = false;
                self.scroll_offset = 0;
                None
            }
            KeyCode::End => {
                self.auto_scroll = true;
                self.clamp_scroll();
                None
            }
            KeyCode::Char('e') => {
                let idx = self.selected_block;
                if let Some(agent) = self.active_agent_mut() {
                    if idx < agent.log.blocks().len() {
                        if agent.expanded.contains(&idx) {
                            agent.expanded.remove(&idx);
                        } else {
                            agent.expanded.insert(idx);
                        }
                    }
                }
                None
            }
            KeyCode::Enter => {
                // If there's a pending input, respond to it
                self.respond_to_pending()
            }
            KeyCode::Tab => {
                let max = self.agents.len().max(1);
                self.active_tab = (self.active_tab + 1) % max;
                self.scroll_offset = 0;
                self.selected_block = 0;
                None
            }
            KeyCode::BackTab => {
                self.auto_approve = !self.auto_approve;
                if self.auto_approve {
                    // Flush all queued approvals
                    let queue = std::mem::take(&mut self.input_queue);
                    for (agent_id, pending) in queue {
                        if matches!(pending, PendingInput::Approval { .. }) {
                            self.pending_messages.push(FrontendMessage::ApprovalResponse {
                                agent_id,
                                approved: true,
                            });
                            if let Some(agent) = self.find_agent_mut(&agent_id) {
                                agent.pending_input = None;
                                agent.status = AgentStatus::Running;
                            }
                        }
                    }
                    self.status_message = Some("Auto-approve: ON (queued approved)".to_string());
                } else {
                    self.status_message = Some("Auto-approve: OFF".to_string());
                }
                None
            }
            KeyCode::Char(' ') => {
                let has_blocks = self.active_agent()
                    .map(|a| !a.log.blocks().is_empty())
                    .unwrap_or(false);
                if has_blocks {
                    self.enter_action_mode();
                }
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

    fn handle_action_mode(&mut self, key: KeyEvent) -> Option<FrontendMessage> {
        const ACTIONS: &[&str] = &["Expand/Collapse", "Save to file", "Copy to clipboard"];
        const ACTION_COUNT: usize = ACTIONS.len();

        match key.code {
            // Escape → restore original expand state, close
            KeyCode::Esc => {
                self.exit_action_mode();
                None
            }
            // Space → close (toggle off)
            KeyCode::Char(' ') => {
                self.exit_action_mode();
                None
            }
            // Navigation keys → process normally
            KeyCode::Up | KeyCode::Char('k') => {
                self.action_cursor = self.action_cursor.saturating_sub(1);
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.action_cursor = (self.action_cursor + 1).min(ACTION_COUNT - 1);
                None
            }
            KeyCode::Char('1') => { self.action_cursor = 0; self.execute_block_action() }
            KeyCode::Char('2') => { self.action_cursor = 1; self.execute_block_action() }
            KeyCode::Char('3') => { self.action_cursor = 2; self.execute_block_action() }
            KeyCode::Enter => self.execute_block_action(),
            // Any other key → close
            _ => {
                self.exit_action_mode();
                None
            }
        }
    }

    fn execute_block_action(&mut self) -> Option<FrontendMessage> {
        let block_text = self.active_agent()
            .and_then(|a| a.log.blocks().get(self.selected_block))
            .map(|b| block_full_text(b))
            .unwrap_or_default();

        if block_text.is_empty() {
            self.exit_action_mode();
            self.status_message = Some("No content in selected block".to_string());
            return None;
        }

        match self.action_cursor {
            0 => {
                // Expand/Collapse — toggle in the saved state so it persists after close
                let idx = self.selected_block;
                if let Some(saved) = &mut self.saved_expanded {
                    if saved.contains(&idx) {
                        saved.remove(&idx);
                    } else {
                        saved.insert(idx);
                    }
                }
                self.exit_action_mode();
                self.status_message = None;
            }
            1 => {
                // Open save dialog with a timestamped default path
                let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
                let default_path = format!("~/block-{}.md", ts);
                self.mode = Mode::SaveDialog;
                self.save_dialog = Some(Box::new(SaveDialogState {
                    cursor: default_path.chars().count(),
                    path: default_path,
                    exists_warned: false,
                    block_text,
                }));
            }
            2 => {
                // Copy to clipboard
                self.exit_action_mode();
                match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&block_text)) {
                    Ok(_) => self.status_message = Some("Copied to clipboard".to_string()),
                    Err(e) => self.status_message = Some(format!("Clipboard failed: {}", e)),
                }
            }
            _ => { self.exit_action_mode(); }
        }
        None
    }

    fn handle_save_dialog(&mut self, key: KeyEvent) -> Option<FrontendMessage> {
        let dialog = match &mut self.save_dialog {
            Some(d) => d,
            None => { self.mode = Mode::Normal; return None; }
        };

        match key.code {
            KeyCode::Esc => {
                self.save_dialog = None;
                self.mode = Mode::Normal;
                return None;
            }
            KeyCode::Enter => {
                let path = dialog.path.trim().to_string();
                if path.is_empty() {
                    return None;
                }
                let text = dialog.block_text.clone();
                let was_warned = dialog.exists_warned;

                let expanded = shellexpand::tilde(&path).to_string();
                let p = std::path::Path::new(&expanded);

                if p.exists() && !was_warned {
                    dialog.exists_warned = true;
                    return None;
                }

                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                match std::fs::write(p, &text) {
                    Ok(_) => self.status_message = Some(format!("Saved to {}", path)),
                    Err(e) => self.status_message = Some(format!("Save failed: {}", e)),
                }
                self.save_dialog = None;
                self.mode = Mode::Normal;
                return None;
            }
            KeyCode::Backspace => {
                if dialog.cursor > 0 {
                    dialog.cursor -= 1;
                    let byte_pos = char_to_byte(&dialog.path, dialog.cursor);
                    dialog.path.remove(byte_pos);
                    dialog.exists_warned = false;
                }
            }
            KeyCode::Delete => {
                let char_count = dialog.path.chars().count();
                if dialog.cursor < char_count {
                    let byte_pos = char_to_byte(&dialog.path, dialog.cursor);
                    dialog.path.remove(byte_pos);
                    dialog.exists_warned = false;
                }
            }
            KeyCode::Left => {
                dialog.cursor = dialog.cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                dialog.cursor = (dialog.cursor + 1).min(dialog.path.chars().count());
            }
            KeyCode::Home => {
                dialog.cursor = 0;
            }
            KeyCode::End => {
                dialog.cursor = dialog.path.chars().count();
            }
            KeyCode::Char(c) => {
                let byte_pos = char_to_byte(&dialog.path, dialog.cursor);
                dialog.path.insert(byte_pos, c);
                dialog.cursor += 1;
                dialog.exists_warned = false;
            }
            _ => {}
        }
        None
    }

    fn handle_command_mode(&mut self, key: KeyEvent) -> Option<FrontendMessage> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.command_buffer.clear();
                self.command_palette.clear();
                None
            }
            KeyCode::Enter => {
                // Use palette selection if visible, otherwise execute typed command
                let cmd = if !self.command_palette.is_empty() && self.palette_cursor < self.command_palette.len() {
                    self.command_palette[self.palette_cursor].name.to_string()
                } else {
                    self.command_buffer.trim().to_string()
                };
                self.mode = Mode::Normal;
                self.command_buffer.clear();
                self.command_palette.clear();
                self.execute_command(&cmd)
            }
            KeyCode::Tab => {
                // Complete to the first matching command
                if let Some(entry) = self.command_palette.first() {
                    self.command_buffer = entry.name.to_string();
                    self.refresh_palette();
                }
                None
            }
            KeyCode::Up => {
                if self.palette_cursor > 0 {
                    self.palette_cursor -= 1;
                }
                None
            }
            KeyCode::Down => {
                if self.palette_cursor < self.command_palette.len().saturating_sub(1) {
                    self.palette_cursor += 1;
                }
                None
            }
            KeyCode::Char(c) => {
                self.command_buffer.push(c);
                self.refresh_palette();
                None
            }
            KeyCode::Backspace => {
                self.command_buffer.pop();
                if self.command_buffer.is_empty() {
                    self.mode = Mode::Normal;
                    self.command_palette.clear();
                } else {
                    self.refresh_palette();
                }
                None
            }
            _ => None,
        }
    }

    fn refresh_palette(&mut self) {
        let prefix = self.command_buffer.trim();
        self.command_palette = COMMANDS.iter()
            .filter(|cmd| {
                if prefix.is_empty() { return true; }
                cmd.name.starts_with(prefix) || cmd.prefix == prefix
            })
            .cloned()
            .collect();
        self.palette_cursor = 0;
    }

    fn execute_command(&mut self, cmd: &str) -> Option<FrontendMessage> {
        match cmd {
            "q" | "quit" => {
                self.should_quit = true;
                None
            }
            "settings" => {
                self.settings = Some(SettingsState::new_empty());
                self.mode = Mode::Settings;
                // Spawn background load of gateway data
                if let Some(tx) = &self.event_tx {
                    let tx = tx.clone();
                    tokio::task::spawn_blocking(move || {
                        let mut gw = crate::settings::GatewayConnection::new();
                        let gateway_ok = gw.ensure_connected().is_ok();
                        let providers = if gateway_ok {
                            crate::settings::query_list_providers(&mut gw).unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        let all = crate::settings::load_all_settings();
                        let rs = all.roles.get("act").cloned().unwrap_or_default();
                        let (fields, models, provider_info) = crate::settings::build_role_fields(&rs, &providers, &mut gw, gateway_ok);
                        let _ = tx.send(crate::AppEvent::SettingsLoaded(
                            SettingsLoadResult::Initial {
                                providers,
                                fields,
                                model_entries: models,
                                provider_info,
                                gateway_available: gateway_ok,
                            }
                        ));
                    });
                }
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
                if task.is_empty() {
                    None
                } else if task.chars().any(|c| c.is_control()) {
                    self.status_message = Some("Task contains control characters".to_string());
                    None
                } else {
                    // Queue SetProviderConfig before SpawnAgent
                    self.queue_provider_config();
                    Some(FrontendMessage::SpawnAgent { task })
                }
            }
            _ => {
                self.status_message = Some(format!("Unknown command: :{}", cmd));
                None
            }
        }
    }

    /// Queue SetProviderConfig messages for all configured roles into pending_messages.
    fn queue_provider_config(&mut self) {
        let all = crate::settings::load_all_settings();
        for role in crate::settings::ROLES {
            if let Some(rs) = all.roles.get(*role) {
                if !rs.provider.is_empty() && !rs.model.is_empty() {
                    let params: std::collections::HashMap<String, serde_json::Value> = rs.provider_params.iter().map(|(k, v)| {
                        (k.clone(), crate::settings::string_to_json_value(v))
                    }).collect();
                    self.pending_messages.push(FrontendMessage::SetProviderConfig {
                        role: role.to_string(),
                        provider: rs.provider.clone(),
                        model: rs.model.clone(),
                        api_key: if rs.api_key.is_empty() { None } else { Some(rs.api_key.clone()) },
                        base_url: if rs.base_url.is_empty() { None } else { Some(rs.base_url.clone()) },
                        params,
                    });
                }
            }
        }
    }

    fn submit_input(&mut self) -> Option<FrontendMessage> {
        let text = self.input.submit();
        if text.is_empty() {
            return None;
        }
        self.auto_scroll = true;

        // If queue is focused and there are pending inputs, respond to the first one
        if self.queue_focused && !self.input_queue.is_empty() {
            return self.respond_to_queue_item(&text);
        }

        // If the active agent has a pending input, respond to it
        if let Some(agent) = self.active_agent() {
            if let Some(pending) = agent.pending_input.clone() {
                let agent_id = agent.id;
                return match &pending {
                    PendingInput::Approval { .. } => {
                        let approved = matches!(text.to_lowercase().as_str(), "y" | "yes" | "");
                        // Remove the exact matching item from the queue
                        if let Some(i) = self.input_queue.iter().position(|(id, p)| {
                            id == &agent_id && *p == pending
                        }) {
                            self.input_queue.remove(i);
                        }
                        self.clear_pending_for_agent(&agent_id);
                        Some(FrontendMessage::ApprovalResponse { agent_id, approved })
                    }
                    PendingInput::Followup { .. } => {
                        if let Some(i) = self.input_queue.iter().position(|(id, p)| {
                            id == &agent_id && *p == pending
                        }) {
                            self.input_queue.remove(i);
                        }
                        self.clear_pending_for_agent(&agent_id);
                        Some(FrontendMessage::FollowupAnswer { agent_id, text })
                    }
                };
            }
        }

        // Normal text input to agent
        if let Some(agent) = self.active_agent() {
            let agent_id = agent.id;
            // Add user message to conversation log
            if let Some(agent) = self.active_agent_mut() {
                agent.log.push_user(text.clone());
            }
            Some(FrontendMessage::UserResponse { agent_id, text })
        } else {
            None
        }
    }

    fn respond_to_pending(&mut self) -> Option<FrontendMessage> {
        if let Some(agent) = self.active_agent() {
            if let Some(pending) = agent.pending_input.clone() {
                let agent_id = agent.id;
                return match &pending {
                    PendingInput::Approval { .. } => {
                        // Remove the exact matching item from the queue
                        if let Some(i) = self.input_queue.iter().position(|(id, p)| {
                            id == &agent_id && *p == pending
                        }) {
                            self.input_queue.remove(i);
                        }
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

    /// After answering one pending item, update the agent's state only if
    /// no more pending items remain for this agent in the queue.
    fn clear_pending_for_agent(&mut self, agent_id: &Uuid) {
        let next_pending = self.input_queue.iter()
            .find(|(id, _)| id == agent_id)
            .map(|(_, p)| p.clone());
        if let Some(next) = next_pending {
            if let Some(agent) = self.find_agent_mut(agent_id) {
                agent.pending_input = Some(next);
            }
        } else if let Some(agent) = self.find_agent_mut(agent_id) {
            agent.pending_input = None;
            if agent.status == AgentStatus::Waiting {
                agent.status = AgentStatus::Running;
            }
        }
    }

    fn find_agent_mut(&mut self, id: &Uuid) -> Option<&mut AgentState> {
        self.agents.iter_mut().find(|a| a.id == *id)
    }

    /// Apply a settings load result from a background gateway operation.
    pub fn apply_settings_load(&mut self, result: SettingsLoadResult) {
        if let Some(s) = &mut self.settings {
            match result {
                SettingsLoadResult::Initial { .. } => s.apply_initial_load(result),
                SettingsLoadResult::ProviderChanged { .. } => s.apply_provider_changed(result),
                SettingsLoadResult::RoleSwitched { .. } => s.apply_role_switched(result),
                SettingsLoadResult::Saved { .. } => {
                    s.apply_saved(result);
                }
            }
        }
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
        "get_outputs" => {
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
            match action {
                "list" => "list".to_string(),
                "read" => {
                    let file = args.get("file").and_then(|v| v.as_str()).unwrap_or("?");
                    format!("read {}", file)
                }
                "clear" => "clear".to_string(),
                _ => action.to_string(),
            }
        }
        "symbols" => {
            let sub = args.get("subcommand").and_then(|v| v.as_str()).unwrap_or("search");
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() { sub.to_string() } else { format!("{} {}", sub, name) }
        }
        _ => args.to_string().chars().take(40).collect(),
    }
}

fn format_result_summary(result: &serde_json::Value) -> String {
    // If the result is a plain string value (e.g., OutputManager reference), use directly
    if let Some(s) = result.as_str() {
        let lines: Vec<&str> = s.lines().take(4).collect();
        return lines.join("\n");
    }
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

/// Convert a char index to a byte index in a string.
fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

fn block_full_text(block: &crate::agent::Block) -> String {
    match block {
        crate::agent::Block::User { content }
        | crate::agent::Block::Assistant { content }
        | crate::agent::Block::System { content } => content.clone(),
        crate::agent::Block::Tool { call, result } => {
            let mut s = format!("Tool: {} ({})\n", call.tool, call.args_summary);
            if let Some(r) = result {
                s.push_str(&r.content);
            }
            s
        }
        crate::agent::Block::Finish { message, .. } => message.clone(),
    }
}
