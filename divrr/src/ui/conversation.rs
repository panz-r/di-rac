use crate::agent::{Block};
use crate::app::App;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block as WidgetBlock, Paragraph, Wrap};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let agent = match app.agents.get(app.active_tab) {
        Some(a) => a,
        None => {
            let placeholder = Paragraph::new("No agent running. Use :new <task> to start.")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(placeholder, area);
            return;
        }
    };

    let max_width = area.width as usize;
    let mut lines: Vec<Line> = Vec::new();

    // Render each block
    let highlight_active = app.mode == crate::app::Mode::Action;
    for (i, block) in agent.log.blocks().iter().enumerate() {
        let is_expanded = agent.expanded.contains(&i);
        let is_selected = i == app.selected_block;
        let is_highlighted = highlight_active && is_selected;
        render_block(&mut lines, block, max_width, is_expanded, is_selected, is_highlighted);
    }

    // Streaming text (active thinking/response)
    if let Some(streaming) = agent.log.streaming() {
        let is_thinking = streaming.is_thinking;
        let style = if is_thinking {
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
        } else {
            Style::default().fg(Color::Blue)
        };
        let content = truncate_single(&streaming.content, max_width.saturating_sub(10));
        lines.push(Line::from(vec![
            Span::styled("< ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
            Span::styled("Agent: ", Style::default().fg(Color::Blue)),
            Span::styled(content, style),
            Span::styled("\u{2588}", Style::default().fg(Color::Blue)),
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
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )]));
    }

    let widget = WidgetBlock::default();
    let paragraph = Paragraph::new(lines)
        .block(widget)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));

    frame.render_widget(paragraph, area);
}

fn render_block(lines: &mut Vec<Line>, block: &Block, max_width: usize, is_expanded: bool, is_selected: bool, is_highlighted: bool) {
    let marker = if is_selected {
        Span::styled("▸ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Span::raw("  ")
    };

    let hl_bg = if is_highlighted { Some(Color::Rgb(30, 30, 30)) } else { None };

    let start = lines.len();
    match block {
        Block::User { content } => {
            if is_expanded {
                for (i, line) in content.lines().enumerate() {
                    if i == 0 {
                        lines.push(Line::from(vec![
                            if is_selected { marker.clone() } else { Span::styled("> ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)) },
                            Span::styled("User: ", Style::default().fg(Color::Green)),
                            Span::raw(truncate_single(line, max_width.saturating_sub(10))),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled("       ", Style::default().fg(Color::Green)),
                            Span::raw(truncate_single(line, max_width.saturating_sub(8))),
                        ]));
                    }
                }
            } else {
                let first_line = content.lines().next().unwrap_or("");
                lines.push(Line::from(vec![
                    if is_selected { marker.clone() } else { Span::styled("> ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)) },
                    Span::styled("User: ", Style::default().fg(Color::Green)),
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
                            if is_selected { marker.clone() } else { Span::styled("< ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)) },
                            Span::styled("Agent: ", Style::default().fg(Color::Blue)),
                            Span::raw(truncate_single(line, max_width.saturating_sub(10))),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::raw("        "),
                            Span::raw(truncate_single(line, max_width.saturating_sub(10))),
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
                    if is_selected { marker.clone() } else { Span::styled("< ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)) },
                    Span::styled("Agent: ", Style::default().fg(Color::Blue)),
                    Span::raw(truncate_single(first_line, max_width.saturating_sub(14))),
                    if hint.is_empty() {
                        Span::raw("")
                    } else {
                        Span::styled(hint, Style::default().fg(Color::DarkGray))
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
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(truncate_single(&call.args_summary, max_width.saturating_sub(12))),
                ]));
                if let Some(res) = result {
                    for line in res.content.lines() {
                        lines.push(Line::from(vec![
                            Span::styled("    ", Style::default().fg(Color::Cyan)),
                            Span::styled("-> ", Style::default().fg(Color::Cyan)),
                            Span::raw(truncate_single(line, max_width.saturating_sub(6))),
                        ]));
                    }
                } else {
                    lines.push(Line::from(vec![
                        Span::styled("    ", Style::default().fg(Color::Cyan)),
                        Span::styled("-> ", Style::default().fg(Color::Cyan)),
                        Span::styled("running...", Style::default().fg(Color::DarkGray)),
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
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(truncate_single(&call.args_summary, max_width.saturating_sub(14))),
                    if status_hint.is_empty() {
                        Span::raw("")
                    } else {
                        Span::styled(status_hint, Style::default().fg(Color::DarkGray))
                    },
                ]));
            }
        }
        Block::System { content } => {
            let is_thinking = content.starts_with(crate::app::THINKING_PREFIX);
            let style = if is_thinking {
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            if is_thinking {
                // Strip the '· ' prefix from content
                let stripped = &content[crate::app::THINKING_PREFIX.len_utf8()..];
                if is_expanded {
                    for (i, line) in stripped.lines().enumerate() {
                        if i == 0 {
                            // First line: marker overdraws the interpunct
                            lines.push(Line::from(vec![
                                if is_selected { marker.clone() } else { Span::styled(format!("{} ", crate::app::THINKING_PREFIX), style) },
                                Span::styled(truncate_single(line, max_width.saturating_sub(2)), style),
                            ]));
                        } else {
                            lines.push(Line::from(vec![
                                Span::styled(truncate_single(line, max_width.saturating_sub(2)), style),
                            ]));
                        }
                    }
                } else {
                    let first_line = stripped.lines().next().unwrap_or("");
                    lines.push(Line::from(vec![
                        if is_selected { marker.clone() } else { Span::styled(format!("{} ", crate::app::THINKING_PREFIX), style) },
                        Span::styled(truncate_single(first_line, max_width.saturating_sub(2)), style),
                    ]));
                }
            } else {
                // Non-thinking system blocks
                if is_expanded {
                    for line in content.lines() {
                        lines.push(Line::from(vec![Span::styled(
                            truncate_single(line, max_width.saturating_sub(2)),
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
                Style::default()
                    .fg(if *success { Color::Green } else { Color::Red })
                    .add_modifier(Modifier::BOLD),
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

fn truncate_single(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}
