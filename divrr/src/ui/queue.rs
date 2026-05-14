use crate::agent::PendingInput;
use crate::app::App;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if app.input_queue.is_empty() {
        return;
    }

    let theme = &app.theme;
    let mut lines: Vec<Line> = Vec::new();
    for (i, (agent_id, pending)) in app.input_queue.iter().enumerate() {
        let agent_name = app
            .agents
            .iter()
            .find(|a| a.id == *agent_id)
            .map(|a| a.name.as_str())
            .unwrap_or("???");

        let (label, detail) = match pending {
            PendingInput::Approval { tool, description, .. } => {
                let prompt = if app.queue_focused && i == 0 { " [Y/n]" } else { "" };
                (format!("Approve {}?{}", tool, prompt), description.clone())
            }
            PendingInput::Followup { question, options, .. } => {
                let opts = options
                    .as_ref()
                    .map(|o| format!(" [{}]", o.join("/")))
                    .unwrap_or_default();
                (question.clone(), opts)
            }
        };

        let style = if i == 0 {
            theme.warning_bold()
        } else {
            theme.text_dim()
        };

        let line_spans = if detail.is_empty() {
            vec![
                Span::styled(format!("[{}] ", i + 1), Style::default().fg(theme.warning)),
                Span::styled(format!("{}: ", agent_name), Style::default().fg(theme.accent)),
                Span::styled(label, style),
            ]
        } else {
            vec![
                Span::styled(format!("[{}] ", i + 1), Style::default().fg(theme.warning)),
                Span::styled(format!("{}: ", agent_name), Style::default().fg(theme.accent)),
                Span::styled(label, style),
                Span::raw(" "),
                Span::styled(detail, theme.text_dim()),
            ]
        };

        lines.push(Line::from(line_spans));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use crate::agent::PendingInput;

    fn make_pending_approval() -> PendingInput {
        PendingInput::Approval {
            tool: "bash".to_string(),
            description: "run tests".to_string(),
            args: serde_json::Value::Null,
        }
    }

    fn make_pending_followup() -> PendingInput {
        PendingInput::Followup {
            question: "Which file?".to_string(),
            options: Some(vec!["a.rs".to_string(), "b.rs".to_string()]),
        }
    }

    #[test]
    fn test_queue_entry_rendering() {
        let approval = make_pending_approval();
        let followup = make_pending_followup();

        match &approval {
            PendingInput::Approval { tool, description, .. } => {
                assert_eq!(tool, "bash");
                assert_eq!(description, "run tests");
            }
            _ => panic!("expected Approval"),
        }

        match &followup {
            PendingInput::Followup { question, options, .. } => {
                assert_eq!(question, "Which file?");
                assert_eq!(options.as_ref().unwrap().len(), 2);
            }
            _ => panic!("expected Followup"),
        }
    }
}
