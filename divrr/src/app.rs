use crate::agent::{AgentState, AgentStatus, PendingInput};
use crate::app_types::{CommandEntry, Mode, SaveDialogState};
use crate::clipboard::copy_to_clipboard;
use crate::input::InputBuffer;
use crate::message::FrontendMessage;
use crate::settings::{SettingsLoadResult, SettingsState};
use crate::ui;
use ratatui::text::Line;
use crate::line_cache::LineCache;
use std::collections::HashSet;

// Async operation lifecycle:
//   Settings operations that need gateway I/O are kicked off via `pending_async`
//   on the SettingsState. The main loop detects this, spawns a blocking task,
//   and the result comes back as a SettingsLoaded event. While `saving` is true,
//   key events are ignored (except Esc to cancel). This avoids holding &mut App
//   across an async boundary.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use tokio::sync::mpsc;
use uuid::Uuid;

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
    /// Saved expanded/wrapped sets while in action mode (restored on cancel).
    saved_expanded: Option<HashSet<usize>>,
    saved_wrapped: Option<HashSet<usize>>,
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
    pub save_dialog: Option<SaveDialogState>,

    /// Cached visual line counts per block (invalidated on width/content/expand change).
    line_cache: Option<LineCache>,
    /// Whether we've already shown the stream stall warning this streaming session.
    pub stream_stall_warned: bool,
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
            saved_expanded: None,
            saved_wrapped: None,
            status_message: None,
            settings: None,
            command_palette: Vec::new(),
            palette_cursor: 0,
            auto_approve: false,
            pending_messages: Vec::new(),
            event_tx: None,
            save_dialog: None,

            line_cache: None,
            stream_stall_warned: false,
        }
    }

    pub fn active_agent(&self) -> Option<&AgentState> {
        self.agents.get(self.active_tab)
    }

    pub fn active_agent_mut(&mut self) -> Option<&mut AgentState> {
        self.agents.get_mut(self.active_tab)
    }

    /// Ensure the visual line cache is populated and valid.
    /// Builds per-block line counts for ALL blocks (needed for scroll math).
    /// Only caches rendered `Line` objects for visible blocks ± buffer.
    fn ensure_line_cache(&mut self, width: u16) {
        let (generation, expanded, wrapped) = self.active_agent()
            .map(|a| (a.log.generation(), a.expanded.clone(), a.wrapped.clone()))
            .unwrap_or((0, HashSet::new(), HashSet::new()));

        if self.line_cache.as_ref().is_some_and(|c| c.is_valid(width, generation, &expanded, &wrapped)) {
            self.ensure_visible_blocks_cached(width);
            return;
        }

        // Take old cache first to release the mutable borrow before agent access
        let prev = self.line_cache.take();

        let Some(agent) = self.active_agent() else {
            return; // line_cache already None
        };

        let block_count = agent.log.blocks().len();
        let can_reuse = prev.as_ref().is_some_and(|c| {
            c.width == width
                && c.expanded == expanded
                && c.wrapped == wrapped
                && c.per_block.len() == block_count
        });

        if can_reuse {
            // Incremental: only recompute the last block (streaming optimization)
            let mut cache = prev.unwrap();
            let last = block_count - 1;
            let is_expanded = expanded.contains(&last);
            let is_wrapped = wrapped.contains(&last);
            let mut lines = Vec::new();
            crate::ui::conversation::build_block_lines(
                &mut lines, &agent.log.blocks()[last], width as usize,
                is_expanded, is_wrapped, false, false, &self.theme,
            );
            let count = ratatui::widgets::Paragraph::new(lines.clone())
                .wrap(ratatui::widgets::Wrap { trim: false })
                .line_count(width);
            let old_count = cache.per_block[last];
            cache.per_block[last] = count;
            cache.blocks_total = cache.blocks_total - old_count + count;
            cache.generation = generation;
            cache.cached_block_lines[last] = Some(lines);
            let _ = agent; // NLL: release immutable borrow
            self.line_cache = Some(cache);
            return;
        }

        // Full rebuild: collect rendered data from all blocks
        let rendered: Vec<(usize, Vec<Line<'static>>)> = agent.log.blocks().iter().enumerate()
            .map(|(i, block)| {
                let is_expanded = expanded.contains(&i);
                let is_wrapped = wrapped.contains(&i);
                let mut lines = Vec::new();
                crate::ui::conversation::build_block_lines(
                    &mut lines, block, width as usize, is_expanded, is_wrapped,
                    false, false, &self.theme,
                );
                let count = ratatui::widgets::Paragraph::new(lines.clone())
                    .wrap(ratatui::widgets::Wrap { trim: false })
                    .line_count(width);
                (count, lines)
            })
            .collect();
        let _ = agent; // NLL: release immutable borrow

        // Build cache (no active agent borrow)
        let mut per_block = Vec::with_capacity(rendered.len());
        let mut blocks_total = 0usize;
        let mut cached_block_lines: Vec<Option<Vec<Line<'static>>>> = vec![None; rendered.len()];

        for (i, (count, lines)) in rendered.into_iter().enumerate() {
            per_block.push(count);
            blocks_total += count;
            if self.is_block_visible(i) {
                cached_block_lines[i] = Some(lines);
            }
        }

        self.line_cache = Some(LineCache {
            width,
            generation,
            expanded,
            wrapped,
            per_block,
            blocks_total,
            cached_block_lines,
        });
    }

    /// Check if a block index is within the visible viewport ± buffer.
    fn is_block_visible(&self, block_idx: usize) -> bool {
        let Some(cache) = &self.line_cache else { return true };
        let mut cum = 0usize;
        let scroll_start = self.scroll_offset;
        let scroll_end = self.scroll_offset + self.visible_lines;
        let buffer = self.visible_lines; // ± one full screen
        for (i, &count) in cache.per_block.iter().enumerate() {
            let block_start = cum;
            let block_end = cum + count;
            if block_end.saturating_sub(1) >= scroll_start.saturating_sub(buffer)
                && block_start <= scroll_end + buffer
            {
                if i == block_idx {
                    return true;
                }
            }
            cum += count;
            if cum > scroll_end + buffer + 1 && i > block_idx {
                return false;
            }
        }
        false
    }

    /// Render `Line` objects for blocks in the visible viewport that aren't cached yet.
    fn ensure_visible_blocks_cached(&mut self, width: u16) {
        // Collect indices that need rendering and expanded/wrapped state
        let (to_render, expanded, wrapped) = {
            let cache = match &self.line_cache {
                Some(c) => c,
                None => return,
            };
            let expanded = cache.expanded.clone();
            let wrapped = cache.wrapped.clone();
            let mut indices = Vec::new();
            let mut cum = 0usize;
            let scroll_start = self.scroll_offset;
            let scroll_end = self.scroll_offset + self.visible_lines;
            let buffer = self.visible_lines;
            for (i, &count) in cache.per_block.iter().enumerate() {
                let block_start = cum;
                let block_end = cum + count;
                if block_end.saturating_sub(1) >= scroll_start.saturating_sub(buffer)
                    && block_start <= scroll_end + buffer
                {
                    if cache.cached_block_lines[i].is_none() {
                        indices.push(i);
                    }
                }
                cum += count;
                if cum > scroll_end + buffer + 1 {
                    break;
                }
            }
            (indices, expanded, wrapped)
        };

        // Evict blocks far from viewport (cap at 3× visible area)
        if let Some(cache) = &mut self.line_cache {
            let total_cached = cache.cached_block_lines.iter().filter(|l| l.is_some()).count();
            if total_cached > self.visible_lines * 3 {
                let mut cum = 0usize;
                let scroll_start = self.scroll_offset;
                let scroll_end = self.scroll_offset + self.visible_lines;
                let evict_buffer = self.visible_lines * 2;
                for (i, &count) in cache.per_block.iter().enumerate() {
                    let block_start = cum;
                    let block_end = cum + count;
                    let in_range = block_end.saturating_sub(1) >= scroll_start.saturating_sub(evict_buffer)
                        && block_start <= scroll_end + evict_buffer;
                    if !in_range && cache.cached_block_lines[i].is_some() {
                        cache.cached_block_lines[i] = None;
                    }
                    cum += count;
                }
            }
        }

        if to_render.is_empty() { return; }

        // Build rendered lines for missing blocks (read agent, then write cache)
        let rendered: Vec<(usize, Vec<Line<'static>>)> = {
            let agent = match self.active_agent() {
                Some(a) => a,
                None => return,
            };
            to_render.into_iter().filter_map(|i| {
                let block = &agent.log.blocks()[i];
                let is_expanded = expanded.contains(&i);
                let is_wrapped = wrapped.contains(&i);
                let mut lines = Vec::new();
                crate::ui::conversation::build_block_lines(
                    &mut lines, block, width as usize, is_expanded, is_wrapped,
                    false, false, &self.theme,
                );
                Some((i, lines))
            }).collect()
        };

        // Store into cache
        let cache = match &mut self.line_cache {
            Some(c) => c,
            None => return,
        };
        for (i, lines) in rendered {
            cache.cached_block_lines[i] = Some(lines);
        }
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

    /// Enter Action mode — save expanded/wrapped state for restore on cancel.
    /// Auto-expands the selected block for preview while the palette is open.
    fn enter_action_mode(&mut self) {
        let (expanded, wrapped) = self.active_agent()
            .map(|a| (a.expanded.clone(), a.wrapped.clone()))
            .unwrap_or_default();
        self.saved_expanded = Some(expanded);
        self.saved_wrapped = Some(wrapped);
        // Auto-expand selected block for preview
        let block = self.selected_block;
        if let Some(agent) = self.active_agent_mut() {
            agent.expanded.insert(block);
        }
        self.mode = Mode::Action;
        self.action_cursor = 0;
    }

    /// Exit Action mode — restore saved expanded/wrapped state.
    fn exit_action_mode(&mut self) {
        if let (Some(expanded), Some(wrapped)) = (self.saved_expanded.take(), self.saved_wrapped.take()) {
            if let Some(agent) = self.active_agent_mut() {
                agent.expanded = expanded;
                agent.wrapped = wrapped;
            }
        }
        self.mode = Mode::Normal;
    }

    /// Exit Action mode keeping the current expanded/wrapped state (user applied a change).
    fn exit_action_mode_keep_state(&mut self) {
        self.saved_expanded = None;
        self.saved_wrapped = None;
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
        let action = match &mut self.settings {
            Some(s) => s.handle_key(key),
            None => return None,
        };
        match action {
            crate::settings::SettingsAction::Close => {
                self.settings = None;
                self.mode = Mode::Normal;
            }
            crate::settings::SettingsAction::Save => {
                if let Some(s) = &mut self.settings {
                    self.pending_messages.extend(s.save());
                    self.theme = crate::theme::Theme::by_name(&s.all_settings.theme);
                    self.line_cache = None;
                }
            }
            crate::settings::SettingsAction::None => {}
        }
        None
    }

    fn handle_normal_mode(&mut self, key: KeyEvent) -> Option<FrontendMessage> {
        // Check for Y/n approval keys before the general key dispatch
        if let (Some(agent), KeyCode::Char(c)) = (self.active_agent(), key.code) {
            if let Some(pending) = agent.pending_input.clone() {
                if matches!(pending, PendingInput::Approval { .. }) {
                    let agent_id = agent.id;
                    match c {
                        'y' | 'Y' => {
                            if let Some(i) = self.input_queue.iter().position(|(id, p)| {
                                id == &agent_id && *p == pending
                            }) {
                                self.input_queue.remove(i);
                            }
                            self.clear_pending_for_agent(&agent_id);
                            return Some(FrontendMessage::ApprovalResponse { agent_id, approved: true });
                        }
                        'n' | 'N' => {
                            if let Some(i) = self.input_queue.iter().position(|(id, p)| {
                                id == &agent_id && *p == pending
                            }) {
                                self.input_queue.remove(i);
                            }
                            self.clear_pending_for_agent(&agent_id);
                            return Some(FrontendMessage::ApprovalResponse { agent_id, approved: false });
                        }
                        _ => {}
                    }
                }
            }
        }

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
                self.clamp_scroll();
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
                // Enter insert mode (queue input or general reply)
                self.mode = Mode::Insert;
                None
            }
            KeyCode::Tab => {
                let max = self.agents.len().max(1);
                self.active_tab = (self.active_tab + 1) % max;
                self.scroll_offset = 0;
                self.selected_block = 0;
                self.line_cache = None;
                None
            }
            KeyCode::BackTab => {
                self.auto_approve = !self.auto_approve;
                let msg = if self.auto_approve {
                    "Auto-approve: ON (future approvals)".to_string()
                } else {
                    "Auto-approve: OFF".to_string()
                };
                if let Some(agent) = self.active_agent_mut() {
                    agent.log.push_system(msg);
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
                self.action_cursor = (self.action_cursor + 1).min(crate::app_types::BLOCK_ACTION_COUNT - 1);
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
                self.exit_action_mode_keep_state();
                self.status_message = None;
            }
            1 => {
                // Open save dialog with a timestamped default path
                self.saved_expanded = None;
                self.saved_wrapped = None;
                let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
                let default_path = format!("~/block-{}.md", ts);
                self.mode = Mode::SaveDialog;
                self.save_dialog = Some(SaveDialogState {
                    cursor: default_path.chars().count(),
                    path: default_path,
                    exists_warned: false,
                    block_text,
                });
            }
            2 => {
                // Copy to clipboard — try multiple methods
                self.exit_action_mode_keep_state();
                match copy_to_clipboard(&block_text) {
                    Ok(()) => self.status_message = Some("Copied to clipboard".to_string()),
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
                self.exit_action_mode_keep_state();
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
                if !crate::summarize::is_safe_save_path(&path) {
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
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        self.status_message = Some(format!("Save failed: could not create directory: {}", e));
                        self.save_dialog = None;
                        self.mode = Mode::Normal;
                        return None;
                    }
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
    /// SetProviderConfig messages are forwarded even if the settings panel
    /// was already closed (e.g., Esc during async save).
    pub fn apply_settings_load(&mut self, result: SettingsLoadResult) {
        match result {
            SettingsLoadResult::Saved { error, messages } => {
                self.pending_messages.extend(messages);
                if let Some(s) = &mut self.settings {
                    s.saving = false;
                    s.error = error;
                    if s.error.is_none() {
                        s.saved = true;
                    }
                }
            }
            other => {
                if let Some(s) = &mut self.settings {
                    match other {
                        SettingsLoadResult::Initial { .. } => s.apply_initial_load(other),
                        SettingsLoadResult::ProviderChanged { .. } => s.apply_provider_changed(other),
                        SettingsLoadResult::RoleSwitched { .. } => s.apply_role_switched(other),
                        _ => {}
                    }
                }
            }
        }
    }

    /// Render the full UI.
    /// Borrow the cached per-block lines (for fast-path rendering).
    pub fn line_cache_blocks(&self) -> Option<&[Option<Vec<Line<'static>>>]> {
        self.line_cache.as_ref().map(|c| c.cached_block_lines.as_slice())
    }

    pub fn view(&self, frame: &mut Frame) {
        ui::render(frame, self);
    }

    /// Check if the active agent's streaming has stalled (no delta for 30s).
    /// Sets a transient status_message that clears once streaming resumes.
    pub fn check_stream_stall(&mut self) {
        if self.stream_stall_warned {
            return;
        }
        let agent = match self.active_agent() {
            Some(a) => a,
            None => return,
        };
        if agent.log.streaming().is_none() {
            return;
        }
        let elapsed = chrono::Utc::now()
            .signed_duration_since(agent.last_activity)
            .num_seconds();
        if elapsed >= 30 {
            self.status_message = Some(format!("No response for {}s — provider may be hung", elapsed));
            self.stream_stall_warned = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Multi-agent queue servicing
    // -----------------------------------------------------------------------
    fn make_agent(id: Uuid) -> AgentState {
        AgentState::new(id, format!("Agent-{:x}", id.as_u128() & 0xFFFF))
    }

    #[test]
    fn respond_to_queue_item_removes_and_clears_pending() {
        let mut app = App::new();
        let id_a = Uuid::from_u128(1);
        let id_b = Uuid::from_u128(2);
        app.agents.push(make_agent(id_a));
        app.agents.push(make_agent(id_b));

        // Set up pending inputs for both agents
        app.agents[0].pending_input = Some(PendingInput::Approval {
            tool: "bash".to_string(),
            args: serde_json::json!({"command": "ls"}),
            description: "Run ls".to_string(),
        });
        app.agents[1].pending_input = Some(PendingInput::Approval {
            tool: "read".to_string(),
            args: serde_json::json!({"path": "f.txt"}),
            description: "Read file".to_string(),
        });

        // Push both to queue (agent A first)
        let pending_a = app.agents[0].pending_input.clone().unwrap();
        let pending_b = app.agents[1].pending_input.clone().unwrap();
        app.input_queue.push((id_a, pending_a));
        app.input_queue.push((id_b, pending_b));
        assert_eq!(app.input_queue.len(), 2);

        // Respond to first queue item (agent A)
        let msg = app.respond_to_queue_item("y");
        assert!(msg.is_some());
        assert_eq!(app.input_queue.len(), 1);
        // Agent A's pending should be cleared (no more items for A)
        assert!(app.agents[0].pending_input.is_none());
        // Agent B's pending should still be set
        assert!(app.agents[1].pending_input.is_some());

        // Respond to second queue item (agent B)
        let msg = app.respond_to_queue_item("y");
        assert!(msg.is_some());
        assert!(app.input_queue.is_empty());
        assert!(app.agents[1].pending_input.is_none());
    }

    #[test]
    fn respond_to_queue_item_empty_queue_returns_none() {
        let mut app = App::new();
        app.agents.push(make_agent(Uuid::from_u128(3)));
        assert!(app.input_queue.is_empty());
        let msg = app.respond_to_queue_item("yes");
        assert!(msg.is_none());
    }

    #[test]
    fn clear_pending_for_agent_advances_to_next_queue_item() {
        let mut app = App::new();
        let id = Uuid::from_u128(4);
        app.agents.push(make_agent(id));

        // Two pending inputs for the SAME agent
        let p1 = PendingInput::Approval {
            tool: "bash".to_string(),
            args: serde_json::json!({"command": "ls"}),
            description: "first".to_string(),
        };
        let p2 = PendingInput::Approval {
            tool: "read".to_string(),
            args: serde_json::json!({"path": "f.txt"}),
            description: "second".to_string(),
        };

        app.agents[0].pending_input = Some(p1.clone());
        app.input_queue.push((id, p1));
        app.input_queue.push((id, p2));

        // Respond to first item
        let msg = app.respond_to_queue_item("y");
        assert!(msg.is_some());
        // Queue should have 1 item left
        assert_eq!(app.input_queue.len(), 1);
        // Agent should still have pending_input = next item
        assert!(app.agents[0].pending_input.is_some());
        // The remaining item should be the second one
        if let Some(PendingInput::Approval { description, .. }) = &app.agents[0].pending_input {
            assert_eq!(description, "second");
        } else {
            panic!("expected Approval");
        }
    }

    #[test]
    fn y_approves_in_normal_mode() {
        let mut app = App::new();
        let id = Uuid::from_u128(5);
        app.agents.push(make_agent(id));

        app.agents[0].pending_input = Some(PendingInput::Approval {
            tool: "bash".to_string(),
            args: serde_json::json!({"command": "ls"}),
            description: "test".to_string(),
        });

        let key = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        let msg = app.handle_normal_mode(key);
        assert!(msg.is_some());
        if let Some(FrontendMessage::ApprovalResponse { agent_id, approved }) = msg {
            assert_eq!(agent_id, id);
            assert!(approved);
        } else {
            panic!("expected ApprovalResponse");
        }
    }

    #[test]
    fn n_denies_in_normal_mode() {
        let mut app = App::new();
        let id = Uuid::from_u128(5);
        app.agents.push(make_agent(id));

        app.agents[0].pending_input = Some(PendingInput::Approval {
            tool: "bash".to_string(),
            args: serde_json::json!({"command": "ls"}),
            description: "test".to_string(),
        });

        let key = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        let msg = app.handle_normal_mode(key);
        assert!(msg.is_some());
        if let Some(FrontendMessage::ApprovalResponse { agent_id, approved }) = msg {
            assert_eq!(agent_id, id);
            assert!(!approved);
        } else {
            panic!("expected ApprovalResponse");
        }
    }
}


