use crate::agent::{AgentState, MessageRole};
use crate::app::App;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

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

    let mut lines: Vec<Line> = Vec::new();

    // Render messages
    for msg in &agent.messages {
        let styled_line = match msg.role {
            MessageRole::User => {
                vec![
                    Span::styled("> ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled("User: ", Style::default().fg(Color::Green)),
                    Span::raw(truncate(&msg.content, area.width as usize - 10)),
                ]
            }
            MessageRole::Assistant => {
                vec![
                    Span::styled("< ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                    Span::styled("Agent: ", Style::default().fg(Color::Blue)),
                    Span::raw(truncate(&msg.content, area.width as usize - 10)),
                ]
            }
            MessageRole::Tool => {
                if let Some(tool) = &msg.tool_name {
                    vec![
                        Span::styled(
                            format!("  {} ", tool),
                            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(truncate(&msg.content, area.width as usize - 12)),
                    ]
                } else {
                    vec![
                        Span::styled("  -> ", Style::default().fg(Color::Cyan)),
                        Span::raw(truncate(&msg.content, area.width as usize - 6)),
                    ]
                }
            }
            MessageRole::System => {
                let is_thinking = msg.content.starts_with("[thinking]");
                let style = if is_thinking {
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                vec![Span::styled(
                    truncate(&msg.content, area.width as usize - 2),
                    style,
                )]
            }
        };
        lines.push(Line::from(styled_line));
    }

    // Streaming text (active thinking)
    if let Some(streaming) = &agent.streaming_text {
        let is_thinking = streaming.starts_with("[thinking]");
        let style = if is_thinking {
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
        } else {
            Style::default().fg(Color::Blue)
        };
        lines.push(Line::from(vec![
            Span::styled("< ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
            Span::styled("Agent: ", Style::default().fg(Color::Blue)),
            Span::styled(truncate(streaming, area.width as usize - 10), style),
            Span::styled("\u{2588}", Style::default().fg(Color::Blue)), // block cursor
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

    // Finished message
    if let Some(finish) = &agent.finish_message {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            format!("--- {} ---", finish),
            Style::default()
                .fg(if agent.status == crate::agent::AgentStatus::Finished {
                    Color::Green
                } else {
                    Color::Red
                })
                .add_modifier(Modifier::BOLD),
        )]));
    }

    let block = Block::default();
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));

    frame.render_widget(paragraph, area);
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.replace('\n', " ")
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated.replace('\n', " "))
    }
}
