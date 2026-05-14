use crate::app::App;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let mut spans: Vec<Span> = Vec::new();

    if app.agents.len() <= 1 {
        // Single agent: compact header
        if let Some(agent) = app.agents.first() {
            spans.push(Span::styled(
                agent.name.clone(),
                theme.accent_bold(),
            ));
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("State: {}", agent.display_status()),
                status_color(theme, agent.display_status()),
            ));
            if let Some(metrics) = &agent.metrics {
                spans.push(Span::raw(format!(
                    " | Tokens: {}",
                    metrics.token_usage
                )));
            }
            if app.auto_approve {
                spans.push(Span::raw(" | "));
                spans.push(Span::styled("AUTO", theme.alert_bold()));
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
                theme.accent_bold()
            } else {
                theme.text_dim()
            };
            let marker = if agent.is_waiting() { "*" } else { "" };
            spans.push(Span::styled(format!("[{}{}]", agent.name, marker), style));
        }

        if !app.input_queue.is_empty() {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("Queue: {} inputs", app.input_queue.len()),
                Style::default().fg(theme.warning),
            ));
        }
        if app.auto_approve {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled("AUTO", theme.alert_bold()));
        }
    }

    // Mode indicator
    match app.mode {
        crate::app_types::Mode::Normal => {}
        crate::app_types::Mode::Insert => {
            spans.push(Span::styled(
                " | INSERT",
                theme.success_style(),
            ));
        }
        crate::app_types::Mode::Command => {
            spans.push(Span::styled(
                " | COMMAND",
                Style::default().fg(theme.command),
            ));
        }
        crate::app_types::Mode::Settings => {
            spans.push(Span::styled(
                " | SETTINGS",
                Style::default().fg(theme.warning),
            ));
        }
        crate::app_types::Mode::Action => {
            spans.push(Span::styled(
                " | ACTION",
                Style::default().fg(theme.accent),
            ));
        }
        crate::app_types::Mode::SaveDialog => {
            spans.push(Span::styled(
                " | SAVE",
                Style::default().fg(theme.warning),
            ));
        }
    }

    // Status message (e.g., errors, confirmations)
    if let Some(msg) = &app.status_message {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(msg.as_str(), theme.text_dim()));
    }

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

fn status_color(theme: &Theme, status: &str) -> Style {
    match status {
        "Running" | "Thinking" => theme.success_style(),
        "Waiting" => Style::default().fg(theme.alert),
        "Error" => theme.error_style(),
        "Finished" => theme.text_dim(),
        _ => Style::default(),
    }
}
