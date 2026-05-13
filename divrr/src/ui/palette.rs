use crate::app::App;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Paragraph};
use ratatui::text::{Line, Span};

/// Render the command palette popup above the input line.
pub fn render(frame: &mut Frame, input_area: Rect, app: &App) {
    if app.command_palette.is_empty() {
        return;
    }

    let theme = &app.theme;
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
            theme.selected_bold()
        } else {
            theme.text_dim()
        };

        let marker = if is_selected { "\u{25B6} " } else { "  " };

        lines.push(Line::from(vec![
            Span::styled(format!("{}:{:<14}", marker, cmd.name), style),
            Span::styled(format!(" {}", cmd.description), theme.text_dim()),
        ]));
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}
