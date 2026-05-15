use crate::app::App;
use crate::app_types::Mode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;

    if app.mode == Mode::Settings {
        let hint = Paragraph::new(Line::from(Span::styled(
            "  Settings open — press Esc to close",
            theme.text_dim(),
        )));
        frame.render_widget(hint, area);
        return;
    }

    let prefix = if app.queue_focused && !app.input_queue.is_empty() {
        "[Queue] >>> ".to_string()
    } else if !app.agents.is_empty() {
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
    let scroll = app.input.scroll_offset;

    let prefix_style = if app.mode == Mode::Insert {
        theme.alert_bold()
    } else {
        theme.accent_bold()
    };

    let mut spans: Vec<Span> = vec![Span::styled(
        &prefix,
        prefix_style,
    )];

    match app.mode {
        Mode::Normal => {
            spans.push(Span::styled(
                &app.input.content,
                theme.text_dim(),
            ));
        }
        Mode::Insert => {
            spans.push(Span::raw(&app.input.content));
        }
        Mode::Command => {
            spans.push(Span::styled(": ", Style::default().fg(theme.command)));
            spans.push(Span::styled(
                &app.command_buffer,
                Style::default().fg(theme.command),
            ));
        }
        Mode::Settings => {
            // Handled by early return above
        }
        Mode::Action => {
            spans.push(Span::styled(
                "[Space: select action]",
                theme.text_dim(),
            ));
        }
        Mode::SaveDialog => {
            spans.push(Span::styled(
                "[Save dialog open]",
                theme.text_dim(),
            ));
        }
        Mode::Hooks => {
            spans.push(Span::styled(
                "[Hooks editor open]",
                theme.text_dim(),
            ));
        }
    }

    let mut paragraph = Paragraph::new(Line::from(spans));
    if app.input.multi_line && scroll > 0 {
        paragraph = paragraph.scroll((scroll as u16, 0));
    }
    frame.render_widget(paragraph, area);

    // Cursor positioning
    match app.mode {
        Mode::Insert => {
            let cursor_row = app.input.cursor_row();
            let visual_row = cursor_row.saturating_sub(scroll);

            let text_before_cursor = &app.input.content[..app.input.cursor.min(app.input.content.len())];
            let last_newline = text_before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
            let line_part = &text_before_cursor[last_newline..];
            let col = unicode_width::UnicodeWidthStr::width(line_part) as u16;

            let cx = if visual_row == 0 { area.x + prefix_len + col } else { area.x + col };
            let cy = area.y + (visual_row as u16).min(area.height.saturating_sub(1));
            frame.set_cursor_position((cx, cy));
        }
        Mode::Command => {
            let cmd_visual = unicode_width::UnicodeWidthStr::width(app.command_buffer.as_str()) as u16;
            frame.set_cursor_position((
                area.x + prefix_len + 2 + cmd_visual,
                area.y,
            ));
        }
        _ => {}
    }
}
