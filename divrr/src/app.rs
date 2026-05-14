use crate::agent::{AgentState, AgentStatus, PendingInput};
use crate::app_types::{CommandEntry, Mode, SaveDialogState};
use crate::input::InputBuffer;
use crate::message::FrontendMessage;
use crate::settings::{SettingsLoadResult, SettingsState};
use crate::ui;
use ratatui::text::Line;
use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Cached per-block rendered lines and visual line counts.
/// Invalidated when width, generation, expand state, or wrap state changes.
struct VisualLineCache {
    width: u16,
    generation: u64,
    expanded: HashSet<usize>,
    wrapped: HashSet<usize>,
    /// Per-block visual line count after wrapping.
    per_block: Vec<usize>,
    /// Total visual lines (blocks only, excluding streaming/pending).
    blocks_total: usize,
    /// Cached rendered `Line` objects for each block (without selection marker/highlight).
    cached_block_lines: Vec<Vec<Line<'static>>>,
}

impl VisualLineCache {
    fn is_valid(&self, width: u16, generation: u64, expanded: &HashSet<usize>, wrapped: &HashSet<usize>) -> bool {
        self.width == width && self.generation == generation && self.expanded == *expanded && self.wrapped == *wrapped
    }
}

pub struct App {
    pub theme: crate::theme::Theme,
    pub agents: Vec<AgentState>,
    pub active_tab: usize,
    pub mode: crate::app_types::Mode,
    pub input: InputBuffer,
    pub command_buffer: String,
    pub input_queue: Vec<(Uuid, PendingInput)>,
    pub queue_focused: bool,
    pub should_quit: bool,
    pub scroll_offset: usize,
    pub content_lines: usize,
    pub visible_lines: usize,
    /// Last known conversation area width (set during render, used for block cursor math).
    pub conv_width: usize,
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

    /// Cached visual line counts per block (invalidated on width/content/expand change).
    line_cache: Option<VisualLineCache>,
}

impl App {
    pub fn new() -> Self {
        Self {
            theme: crate::theme::Theme::copper_cobalt(),
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
            conv_width: 80,
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

            line_cache: None,
        }
    }

    pub fn active_agent(&self) -> Option<&AgentState> {
        self.agents.get(self.active_tab)
    }

    pub fn active_agent_mut(&mut self) -> Option<&mut AgentState> {
        self.agents.get_mut(self.active_tab)
    }

    /// Ensure the visual line cache is populated and valid.
    fn ensure_line_cache(&mut self, width: u16) {
        let (generation, expanded, wrapped) = self.active_agent()
            .map(|a| (a.log.generation(), a.expanded.clone(), a.wrapped.clone()))
            .unwrap_or((0, HashSet::new(), HashSet::new()));

        if self.line_cache.as_ref().is_some_and(|c| c.is_valid(width, generation, &expanded, &wrapped)) {
            return;
        }

        // Recompute per-block rendered lines and visual line counts
        let agent = match self.active_agent() {
            Some(a) => a,
            None => {
                self.line_cache = None;
                return;
            }
        };

        let mut per_block = Vec::with_capacity(agent.log.blocks().len());
        let mut blocks_total = 0usize;
        let mut cached_block_lines = Vec::with_capacity(agent.log.blocks().len());
        for (i, block) in agent.log.blocks().iter().enumerate() {
            let is_expanded = agent.expanded.contains(&i);
            let is_wrapped = agent.wrapped.contains(&i);
            let mut lines = Vec::new();
            crate::ui::conversation::build_block_lines(
                &mut lines, block, width as usize, is_expanded, is_wrapped,
                false, false, &self.theme,
            );
            let count = ratatui::widgets::Paragraph::new(lines.clone())
                .wrap(ratatui::widgets::Wrap { trim: false })
                .line_count(width);
            per_block.push(count);
            blocks_total += count;
            cached_block_lines.push(lines);
        }

        self.line_cache = Some(VisualLineCache {
            width,
            generation,
            expanded,
            wrapped,
            per_block,
            blocks_total,
            cached_block_lines,
        });
    }

    /// Count total rendered lines (blocks + streaming + pending) with caching.
    pub fn count_rendered_lines(&mut self, width: u16) -> usize {
        self.ensure_line_cache(width);
        let blocks_total = self.line_cache.as_ref().map(|c| c.blocks_total).unwrap_or(0);

        // Add streaming and pending input lines (not cached — change every frame during streaming)
        let mut total = blocks_total;
        if let Some(agent) = self.active_agent() {
            if agent.log.streaming().is_some() { total += 1; }
            if agent.pending_input.is_some() { total += 1; }
        }
        total
    }

    pub fn clamp_scroll(&mut self) {
        if self.content_lines > self.visible_lines {
            let max_scroll = self.content_lines.saturating_sub(self.visible_lines);
            if self.auto_scroll {
                self.scroll_offset = max_scroll;
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

        // Ensure cache is warm, then use cached per-block counts
        let width = (self.conv_width as u16).max(1);
        self.ensure_line_cache(width);

        let (block_start, block_height) = self.line_cache.as_ref()
            .map(|cache| {
                let start: usize = cache.per_block.iter().take(new).sum();
                let height = cache.per_block.get(new).copied().unwrap_or(1);
                (start, height)
            })
            .unwrap_or((0, 1));

        // Scroll to keep the selected block visible
        let visible = self.visible_lines;
        if block_start < self.scroll_offset {
            self.scroll_offset = block_start;
        } else if block_start + block_height > self.scroll_offset + visible {
            self.scroll_offset = block_start + block_height.saturating_sub(visible);
        }
    }

    /// Process a CoreEvent from di-core into state updates.
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

    /// Enter Action mode.
    fn enter_action_mode(&mut self) {
        self.mode = Mode::Action;
        self.action_cursor = 0;
    }

    /// Exit Action mode.
    fn exit_action_mode(&mut self) {
        self.mode = Mode::Normal;
    }

    pub fn handle_paste(&mut self, text: &str) {
        match self.mode {
            Mode::Settings => {
                if let Some(s) = &mut self.settings {
                    if s.secret_edit_open {
                        let filtered: String = text.chars().filter(|c| !c.is_control()).collect();
                        s.secret_edit_buffer.insert_str(s.secret_edit_cursor, &filtered);
                        s.secret_edit_cursor += filtered.len();
                    } else if !s.selector_open && !s.loading && s.cursor > 0 {
                        let fo = s.field_offset();
                        if let Some(crate::settings::SettingsField::Text { value, .. }) = s.fields.get_mut(fo) {
                            let filtered: String = text.chars().filter(|c| !c.is_control()).collect();
                            value.push_str(&filtered);
                            s.saved = false;
                            s.error = None;
                        }
                    }
                }
            }
            Mode::SaveDialog => {
                if let Some(d) = &mut self.save_dialog {
                    let byte_pos = crate::summarize::char_to_byte(&d.path, d.cursor);
                    d.path.insert_str(byte_pos, text);
                    d.cursor += text.chars().count();
                    d.exists_warned = false;
                }
            }
            Mode::Insert => {
                self.input.insert_str(text);
            }
            _ => {}
        }
    }

    fn handle_settings_mode(&mut self, key: KeyEvent) -> Option<FrontendMessage> {
        // While loading or saving, only allow Esc to close
        if let Some(s) = &self.settings {
            if s.loading || s.saving {
                if key.code == KeyCode::Esc {
                    self.settings = None;
                    self.mode = Mode::Normal;
                }
                return None;
            }
        }

        match key.code {
            KeyCode::Esc => {
                if let Some(s) = &self.settings {
                    if s.secret_edit_open {
                        self.settings.as_mut().unwrap().cancel_secret_edit();
                    } else if s.selector_open {
                        self.settings.as_mut().unwrap().cancel_selector();
                    } else if s.saving {
                        // Don't close while async save is in flight
                    } else {
                        self.settings = None;
                        self.mode = Mode::Normal;
                    }
                }
            }
            KeyCode::BackTab => {
                if let Some(s) = &mut self.settings {
                    if !s.selector_open && !s.secret_edit_open && !s.loading {
                        s.switch_panel();
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
                        self.pending_messages.extend(s.save());
                        // Apply theme change immediately
                        self.theme = crate::theme::Theme::by_name(&s.all_settings.theme);
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
                crate::commands::refresh_palette(self);
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
                self.status_message = Some(if self.auto_approve {
                    "Auto-approve: ON (future approvals)".to_string()
                } else {
                    "Auto-approve: OFF".to_string()
                });
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
        const ACTION_COUNT: usize = 4;

        match key.code {
            KeyCode::Esc => {
                self.exit_action_mode();
                None
            }
            KeyCode::Char(' ') => {
                self.exit_action_mode();
                None
            }
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
            KeyCode::Char('4') => { self.action_cursor = 3; self.execute_block_action() }
            KeyCode::Enter => self.execute_block_action(),
            // Navigation keys that should be ignored without closing the palette
            KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End => None,
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
            .map(crate::summarize::block_full_text)
            .unwrap_or_default();

        if block_text.is_empty() {
            self.exit_action_mode();
            self.status_message = Some("No content in selected block".to_string());
            return None;
        }

        match self.action_cursor {
            0 => {
                // Expand/Collapse — toggle directly on live state
                let idx = self.selected_block;
                if let Some(agent) = self.active_agent_mut() {
                    if agent.expanded.contains(&idx) {
                        agent.expanded.remove(&idx);
                    } else {
                        agent.expanded.insert(idx);
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
            3 => {
                // Wrap/Unwrap — toggle directly on live state
                let idx = self.selected_block;
                if let Some(agent) = self.active_agent_mut() {
                    if agent.wrapped.contains(&idx) {
                        agent.wrapped.remove(&idx);
                    } else {
                        agent.wrapped.insert(idx);
                    }
                }
                self.exit_action_mode();
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

                // Reject path traversal
                if path.split(std::path::is_separator).any(|c| c == "..") {
                    self.status_message = Some("Save failed: path must not contain '..'".to_string());
                    self.save_dialog = None;
                    self.mode = Mode::Normal;
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
                    let byte_pos = crate::summarize::char_to_byte(&dialog.path, dialog.cursor);
                    dialog.path.remove(byte_pos);
                    dialog.exists_warned = false;
                }
            }
            KeyCode::Delete => {
                let char_count = dialog.path.chars().count();
                if dialog.cursor < char_count {
                    let byte_pos = crate::summarize::char_to_byte(&dialog.path, dialog.cursor);
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
                let byte_pos = crate::summarize::char_to_byte(&dialog.path, dialog.cursor);
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
                crate::commands::execute_command(self, &cmd)
            }
            KeyCode::Tab => {
                if let Some(entry) = self.command_palette.first() {
                    self.command_buffer = entry.name.to_string();
                    crate::commands::refresh_palette(self);
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
                crate::commands::refresh_palette(self);
                None
            }
            KeyCode::Backspace => {
                self.command_buffer.pop();
                if self.command_buffer.is_empty() {
                    self.mode = Mode::Normal;
                    self.command_palette.clear();
                } else {
                    crate::commands::refresh_palette(self);
                }
                None
            }
            _ => None,
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
            let len = text.len();
            self.input.content = text;
            self.input.cursor = len;
            self.input.history.pop();
            self.status_message = Some("No agent running. Use :new <task> to start.".to_string());
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
            if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                agent.pending_input = Some(next);
            }
        } else if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
            agent.pending_input = None;
            if agent.status == AgentStatus::Waiting {
                agent.status = AgentStatus::Running;
            }
        }
    }

    /// Close the active agent tab. Sends Interrupt if still running,
    /// removes queue items, and adjusts tab/scroll state.
    pub fn close_active_tab(&mut self) -> Option<FrontendMessage> {
        let agent = match self.agents.get(self.active_tab) {
            Some(a) => a,
            None => {
                self.status_message = Some("No active tab to close".to_string());
                return None;
            }
        };
        let id = agent.id;
        let is_running = matches!(agent.status, AgentStatus::Running | AgentStatus::Waiting);

        // Remove all pending queue items for this agent
        self.input_queue.retain(|(qid, _)| *qid != id);

        // Remove the agent
        self.agents.remove(self.active_tab);

        // Adjust active_tab to stay in bounds
        if self.agents.is_empty() {
            self.active_tab = 0;
        } else if self.active_tab >= self.agents.len() {
            self.active_tab = self.agents.len() - 1;
        }

        // Reset view state for the new active tab
        self.scroll_offset = 0;
        self.selected_block = 0;
        self.line_cache = None;

        self.status_message = Some("Tab closed".to_string());

        // Send Interrupt if agent was still running
        if is_running {
            Some(FrontendMessage::Interrupt { agent_id: id })
        } else {
            None
        }
    }

    /// Apply a settings load result from a background gateway operation.
    pub fn apply_settings_load(&mut self, result: SettingsLoadResult) {
        if let Some(s) = &mut self.settings {
            match &result {
                SettingsLoadResult::Initial { .. } => s.apply_initial_load(result),
                SettingsLoadResult::ProviderChanged { .. } => s.apply_provider_changed(result),
                SettingsLoadResult::RoleSwitched { .. } => s.apply_role_switched(result),
                SettingsLoadResult::Saved { .. } => {
                    let messages = s.apply_saved(result);
                    self.pending_messages.extend(messages);
                }
            }
        }
    }

    /// Render the full UI.
    /// Borrow the cached per-block lines (for fast-path rendering).
    pub fn line_cache_blocks(&self) -> Option<&[Vec<Line<'static>>]> {
        self.line_cache.as_ref().map(|c| c.cached_block_lines.as_slice())
    }

    pub fn view(&self, frame: &mut Frame) {
        ui::render(frame, self);
    }
}


