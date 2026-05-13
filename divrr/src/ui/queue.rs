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
                (format!("Approve {}?", tool), description.clone())
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

        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", i + 1), Style::default().fg(theme.warning)),
            Span::styled(format!("{}: ", agent_name), Style::default().fg(theme.accent)),
            Span::styled(format!("{}{}", label, detail), style),
        ]));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}
