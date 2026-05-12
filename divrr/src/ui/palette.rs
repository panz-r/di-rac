use crate::app::App;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

/// Render the command palette popup above the input line.
pub fn render(frame: &mut Frame, input_area: Rect, app: &App) {
    if app.command_palette.is_empty() {
        return;
    }

    let count = app.command_palette.len().min(8) as u16;
    let w = 44u16;
    let h = count + 2; // border
    let y = input_area.y.saturating_sub(h);
    let x = input_area.x + 4; // past ">>> "

    let area = Rect::new(x, y, w, h);
    frame.render_widget(Clear, area);

    let mut lines = Vec::new();
    for (i, cmd) in app.command_palette.iter().enumerate() {
        let is_selected = i == app.palette_cursor;
        let style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        let marker = if is_selected { "\u{25B6} " } else { "  " };
        let desc_style = Style::default().fg(Color::DarkGray);

        lines.push(Line::from(vec![
            Span::styled(format!("{}:{:<14}", marker, cmd.name), style),
            Span::styled(format!(" {}", cmd.description), desc_style),
        ]));
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}
