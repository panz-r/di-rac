use crate::app::{App, Mode};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let prefix = if app.queue_focused && !app.input_queue.is_empty() {
        "[Queue] >>> ".to_string()
    } else if app.agents.len() > 1 {
        let name = app
            .active_agent()
            .map(|a| a.name.as_str())
            .unwrap_or("?");
        format!("[{}*] >>> ", name)
    } else {
        ">>> ".to_string()
    };

    let prefix_len = prefix.len() as u16;

    let mut spans: Vec<Span> = vec![Span::styled(
        &prefix,
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )];

    match app.mode {
        Mode::Normal => {
            spans.push(Span::styled(
                &app.input.content,
                Style::default().fg(Color::DarkGray),
            ));
        }
        Mode::Insert => {
            spans.push(Span::raw(&app.input.content));
            frame.set_cursor_position((
                area.x + prefix_len + app.input.cursor as u16,
                area.y,
            ));
        }
        Mode::Command => {
            spans.push(Span::styled(": ", Style::default().fg(Color::Magenta)));
            spans.push(Span::styled(
                &app.command_buffer,
                Style::default().fg(Color::Magenta),
            ));
            frame.set_cursor_position((
                area.x + prefix_len + 2 + app.command_buffer.len() as u16,
                area.y,
            ));
        }
    }

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}
