use crate::agent::{AgentState, Block};
use crate::app::App;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block as WidgetBlock, Paragraph, Wrap};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let agent = match app.agents.get(app.active_tab) {
        Some(a) => a,
        None => {
            let placeholder = Paragraph::new("No agent running. Use :new <task> to start.")
                .style(app.theme.text_dim());
            frame.render_widget(placeholder, area);
            return;
        }
    };

    let cached = app.line_cache_blocks();
    let lines = build_all_lines(agent, area.width as usize, app.selected_block, app.mode, &app.theme, cached);

    let widget = WidgetBlock::default();
    let paragraph = Paragraph::new(lines)
        .block(widget)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));

    frame.render_widget(paragraph, area);
}

/// Build all Line for the conversation view (blocks + streaming + pending).
/// When `cached_block_lines` is available and valid, reuses cached line objects
/// for non-selected blocks that have been rendered; builds on-the-fly for uncached blocks.
pub fn build_all_lines(
    agent: &AgentState, max_width: usize, selected_block: usize,
    mode: crate::app_types::Mode, theme: &Theme,
    cached_block_lines: Option<&[Option<Vec<Line<'static>>>]>,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::with_capacity(agent.log.blocks().len() + 2);
    let highlight_active = mode == crate::app_types::Mode::Action;

    if let Some(cached) = cached_block_lines {
        if cached.len() == agent.log.blocks().len() {
            for (i, cached_lines) in cached.iter().enumerate() {
                let is_selected = i == selected_block;
                let is_highlighted = highlight_active && is_selected;
                if is_selected || is_highlighted {
                    let block = &agent.log.blocks()[i];
                    let is_expanded = agent.expanded.contains(&i);
                    let is_wrapped = agent.wrapped.contains(&i);
                    build_block_lines(&mut lines, block, max_width, is_expanded, is_wrapped,
                        true, is_highlighted, theme);
                } else if let Some(block_lines) = cached_lines {
                    lines.extend(block_lines.iter().cloned());
                } else {
                    let block = &agent.log.blocks()[i];
                    let is_expanded = agent.expanded.contains(&i);
                    let is_wrapped = agent.wrapped.contains(&i);
                    build_block_lines(&mut lines, block, max_width, is_expanded, is_wrapped,
                        false, false, theme);
                }
            }
        } else {
            for (i, block) in agent.log.blocks().iter().enumerate() {
                let is_expanded = agent.expanded.contains(&i);
                let is_wrapped = agent.wrapped.contains(&i);
                let is_selected = i == selected_block;
                let is_highlighted = highlight_active && is_selected;
                build_block_lines(&mut lines, block, max_width, is_expanded, is_wrapped,
                    is_selected, is_highlighted, theme);
            }
        }
    } else {
        for (i, block) in agent.log.blocks().iter().enumerate() {
            let is_expanded = agent.expanded.contains(&i);
            let is_wrapped = agent.wrapped.contains(&i);
            let is_selected = i == selected_block;
            let is_highlighted = highlight_active && is_selected;
            build_block_lines(&mut lines, block, max_width, is_expanded, is_wrapped,
                is_selected, is_highlighted, theme);
        }
    }

    // Streaming text (active thinking/response)
    if let Some(streaming) = agent.log.streaming() {
        let is_thinking = streaming.is_thinking;
        let style = if is_thinking {
            theme.dim_italic()
        } else {
            Style::default().fg(theme.accent)
        };
        let content = truncate_single(&streaming.content, max_width.saturating_sub(10));
        lines.push(Line::from(vec![
            Span::styled("< ", theme.accent_bold()),
            Span::styled("Agent: ", Style::default().fg(theme.accent)),
            Span::styled(content, style),
            Span::styled("\u{2588}", Style::default().fg(theme.accent)),
        ]));
    }

    // Pending input indicator
    if let Some(pending) = &agent.pending_input {
        let hint = match pending {
            crate::agent::PendingInput::Approval { tool, description, .. } => {
                format!("[Approve {}? {} — press i to type Y/n]", tool, description)
            }
            crate::agent::PendingInput::Followup { question, options } => {
                let opts = options
                    .as_ref()
                    .map(|o| format!(" [{}]", o.join("/")))
                    .unwrap_or_default();
                format!("[Answer: {}{} — press i to respond]", question, opts)
            }
        };
        lines.push(Line::from(vec![Span::styled(
            hint,
            theme.warning_bold(),
        )]));
    }

    lines
}

/// Build Lines for a single block. Shared between rendering and line counting.
pub fn build_block_lines(lines: &mut Vec<Line>, block: &Block, max_width: usize, is_expanded: bool, is_wrapped: bool, is_selected: bool, is_highlighted: bool, theme: &Theme) {
    let marker = if is_selected {
        Span::styled("\u{25B8} ", theme.selected_bold())
    } else {
        Span::raw("  ")
    };

    let hl_bg = if is_highlighted { Some(theme.selected_bg) } else { None };

    let start = lines.len();
    match block {
        Block::User { content } => {
            if is_expanded {
                for (i, line) in content.lines().enumerate() {
                    if i == 0 {
                        lines.push(Line::from(vec![
                            if is_selected { marker.clone() } else { Span::styled("> ", theme.success_bold()) },
                            Span::styled("User: ", Style::default().fg(theme.success)),
                            Span::raw(maybe_truncate(line, max_width.saturating_sub(10), is_wrapped)),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled("       ", Style::default().fg(theme.success)),
                            Span::raw(maybe_truncate(line, max_width.saturating_sub(8), is_wrapped)),
                        ]));
                    }
                }
            } else {
                let first_line = content.lines().next().unwrap_or("");
                lines.push(Line::from(vec![
                    if is_selected { marker.clone() } else { Span::styled("> ", theme.success_bold()) },
                    Span::styled("User: ", Style::default().fg(theme.success)),
                    Span::raw(truncate_single(first_line, max_width.saturating_sub(10))),
                ]));
            }
        }
        Block::Assistant { content } => {
            if content.is_empty() { return; }
            if is_expanded {
                for (i, line) in content.lines().enumerate() {
                    if i == 0 {
                        lines.push(Line::from(vec![
                            if is_selected { marker.clone() } else { Span::styled("< ", theme.accent_bold()) },
                            Span::styled("Agent: ", Style::default().fg(theme.accent)),
                            Span::raw(maybe_truncate(line, max_width.saturating_sub(10), is_wrapped)),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::raw("        "),
                            Span::raw(maybe_truncate(line, max_width.saturating_sub(10), is_wrapped)),
                        ]));
                    }
                }
            } else {
                let first_line = content.lines().next().unwrap_or("");
                let total_lines = content.lines().count();
                let hint = if total_lines > 1 {
                    format!(" ({} lines)", total_lines)
                } else {
                    String::new()
                };
                lines.push(Line::from(vec![
                    if is_selected { marker.clone() } else { Span::styled("< ", theme.accent_bold()) },
                    Span::styled("Agent: ", Style::default().fg(theme.accent)),
                    Span::raw(truncate_single(first_line, max_width.saturating_sub(14))),
                    if hint.is_empty() {
                        Span::raw("")
                    } else {
                        Span::styled(hint, theme.text_dim())
                    },
                ]));
            }
        }
        Block::Tool { call, result } => {
            if is_expanded {
                lines.push(Line::from(vec![
                    if is_selected { marker.clone() } else { Span::raw("  ") },
                    Span::styled(
                        format!("{} ", call.tool),
                        theme.accent_bold(),
                    ),
                    Span::raw(maybe_truncate(&call.args_summary, max_width.saturating_sub(12), is_wrapped)),
                ]));
                if let Some(res) = result {
                    for line in res.content.lines() {
                        lines.push(Line::from(vec![
                            Span::styled("    ", Style::default().fg(theme.accent)),
                            Span::styled("-> ", Style::default().fg(theme.accent)),
                            Span::raw(maybe_truncate(line, max_width.saturating_sub(6), is_wrapped)),
                        ]));
                    }
                } else {
                    lines.push(Line::from(vec![
                        Span::styled("    ", Style::default().fg(theme.accent)),
                        Span::styled("-> ", Style::default().fg(theme.accent)),
                        Span::styled("running...", theme.text_dim()),
                    ]));
                }
            } else {
                let status_hint = result.as_ref().map(|r| {
                    let lcount = r.content.lines().count();
                    if lcount > 1 { format!(" ({} lines)", lcount) } else { String::new() }
                }).unwrap_or_default();
                lines.push(Line::from(vec![
                    if is_selected { marker.clone() } else { Span::raw("  ") },
                    Span::styled(
                        format!("{} ", call.tool),
                        theme.accent_bold(),
                    ),
                    Span::raw(truncate_single(&call.args_summary, max_width.saturating_sub(14))),
                    if status_hint.is_empty() {
                        Span::raw("")
                    } else {
                        Span::styled(status_hint, theme.text_dim())
                    },
                ]));
            }
        }
        Block::System { content } => {
            let is_thinking = content.starts_with(crate::summarize::THINKING_PREFIX);
            let style = if is_thinking {
                theme.dim_italic()
            } else {
                theme.text_dim()
            };

            if is_thinking {
                let stripped = &content[crate::summarize::THINKING_PREFIX.len_utf8()..];
                if is_expanded {
                    for (i, line) in stripped.lines().enumerate() {
                        if i == 0 {
                            lines.push(Line::from(vec![
                                if is_selected { marker.clone() } else { Span::styled(format!("{} ", crate::summarize::THINKING_PREFIX), style) },
                                Span::styled(maybe_truncate(line, max_width.saturating_sub(2), is_wrapped), style),
                            ]));
                        } else {
                            lines.push(Line::from(vec![
                                Span::styled(maybe_truncate(line, max_width.saturating_sub(2), is_wrapped), style),
                            ]));
                        }
                    }
                } else {
                    let first_line = stripped.lines().next().unwrap_or("");
                    lines.push(Line::from(vec![
                        if is_selected { marker.clone() } else { Span::styled(format!("{} ", crate::summarize::THINKING_PREFIX), style) },
                        Span::styled(truncate_single(first_line, max_width.saturating_sub(2)), style),
                    ]));
                }
            } else {
                if is_expanded {
                    for line in content.lines() {
                        lines.push(Line::from(vec![Span::styled(
                            maybe_truncate(line, max_width.saturating_sub(2), is_wrapped),
                            style,
                        )]));
                    }
                } else {
                    let first_line = content.lines().next().unwrap_or("");
                    lines.push(Line::from(vec![Span::styled(
                        truncate_single(first_line, max_width.saturating_sub(2)),
                        style,
                    )]));
                }
            }
        }
        Block::Finish { message, success } => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                format!("--- {} ---", message),
                if *success { theme.success_bold() } else { theme.error_bold() },
            )]));
        }
    }

    // Apply highlight background to all lines rendered for this block
    if let Some(bg) = hl_bg {
        for line in lines.iter_mut().skip(start) {
            line.style = line.style.patch(Style::default().bg(bg));
        }
    }
}

/// Truncate a single line to max_len characters (appending "…").
/// When wrapping is enabled, return the full string unchanged.
fn maybe_truncate(s: &str, max_len: usize, is_wrapped: bool) -> String {
    if is_wrapped {
        s.to_string()
    } else {
        truncate_single(s, max_len)
    }
}

fn truncate_single(s: &str, max_len: usize) -> String {
    // Fast path: if byte length fits, char count definitely fits (each char >= 1 byte)
    if s.len() <= max_len {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
    format!("{}...", truncated)
}
