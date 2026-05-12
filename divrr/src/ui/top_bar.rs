use crate::app::App;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans: Vec<Span> = Vec::new();

    if app.agents.len() <= 1 {
        // Single agent: compact header
        if let Some(agent) = app.agents.first() {
            spans.push(Span::styled(
                agent.name.clone(),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("State: {}", agent.display_status()),
                status_color(agent.display_status()),
            ));
            if agent.is_waiting() {
                if let Some(pending) = &agent.pending_input {
                    let hint = match pending {
                        crate::agent::PendingInput::Approval { tool, .. } => {
                            format!("Input: Approve {}? [Y/n]", tool)
                        }
                        crate::agent::PendingInput::Followup { options, .. } => {
                            if let Some(opts) = options {
                                format!("Input: [{}]", opts.join("/"))
                            } else {
                                "Input: answer needed".to_string()
                            }
                        }
                    };
                    spans.push(Span::raw(" | "));
                    spans.push(Span::styled(hint, Style::default().fg(Color::Yellow)));
                }
            }
            if let Some(metrics) = &agent.metrics {
                spans.push(Span::raw(format!(
                    " | Tokens: {}",
                    metrics.token_usage
                )));
            }
            if app.auto_approve {
                spans.push(Span::raw(" | "));
                spans.push(Span::styled("AUTO", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
            }
            spans.push(Span::raw(format!(" | {}", agent.format_timestamp())));
        }
    } else {
        // Multiple agents: tab bar
        for (i, agent) in app.agents.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" "));
            }
            let is_active = i == app.active_tab;
            let style = if is_active {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let marker = if agent.is_waiting() { "*" } else { "" };
            spans.push(Span::styled(format!("[{}{}]", agent.name, marker), style));
        }

        if !app.input_queue.is_empty() {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("Queue: {} inputs", app.input_queue.len()),
                Style::default().fg(Color::Yellow),
            ));
        }
    }

    // Mode indicator
    match app.mode {
        crate::app::Mode::Normal => {}
        crate::app::Mode::Insert => {
            spans.push(Span::styled(
                " | INSERT",
                Style::default().fg(Color::Green),
            ));
        }
        crate::app::Mode::Command => {
            spans.push(Span::styled(
                " | COMMAND",
                Style::default().fg(Color::Magenta),
            ));
        }
        crate::app::Mode::Settings => {
            spans.push(Span::styled(
                " | SETTINGS",
                Style::default().fg(Color::Yellow),
            ));
        }
    }

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

fn status_color(status: &str) -> Style {
    match status {
        "Running" | "Thinking" => Style::default().fg(Color::Green),
        "Waiting" => Style::default().fg(Color::Yellow),
        "Error" => Style::default().fg(Color::Red),
        "Finished" => Style::default().fg(Color::DarkGray),
        _ => Style::default(),
    }
}
