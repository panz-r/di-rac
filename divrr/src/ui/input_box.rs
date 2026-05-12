use crate::app::{App, Mode};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if app.mode == Mode::Settings {
        // Don't render input box when settings overlay is active
        let hint = Paragraph::new(Line::from(Span::styled(
            "  Settings open — press Esc to close",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(hint, area);
        return;
    }

    let prefix = if app.queue_focused && !app.input_queue.is_empty() {
        "[Queue] >>> ".to_string()
    } else if app.agents.len() >= 1 {
        let name = app
            .active_agent()
            .map(|a| a.name.as_str())
            .unwrap_or("?");
        if app.agents.len() > 1 {
            format!("[{}*] >>> ", name)
        } else {
            format!("[{}] >>> ", name)
        }
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
            let text_before_cursor = &app.input.content[..app.input.cursor.min(app.input.content.len())];
            let row = text_before_cursor.lines().count().max(1) - 1;
            let last_newline = text_before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
            let col = text_before_cursor[last_newline..].len() as u16;
            frame.set_cursor_position((
                if row == 0 { area.x + prefix_len + col } else { area.x + col },
                area.y + row as u16,
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
        Mode::Settings => {
            // Handled by early return above
        }
        Mode::Action => {
            spans.push(Span::styled(
                "[Space: select action]",
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}
