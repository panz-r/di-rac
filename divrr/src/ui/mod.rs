pub mod top_bar;
pub mod conversation;
pub mod queue;
pub mod input_box;
pub mod settings;
pub mod palette;

use crate::app::App;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

pub fn render(frame: &mut Frame, app: &App) {
    let size = frame.area();

    let queue_height = if app.input_queue.is_empty() {
        0
    } else {
        app.input_queue.len().min(5) as u16
    };

    let input_height = if app.input.multi_line {
        app.input.content.lines().count().max(1).min(8) as u16
    } else {
        1
    };

    let constraints = vec![
        Constraint::Length(1),      // top bar
        Constraint::Fill(1),        // conversation
        Constraint::Length(queue_height), // queue (0 when empty)
        Constraint::Length(input_height), // input box
    ];

    let areas = Layout::vertical(&constraints).split(size);
    let (top_area, conv_area, queue_area, input_area) = (areas[0], areas[1], areas[2], areas[3]);

    top_bar::render(frame, top_area, app);
    conversation::render(frame, conv_area, app);

    if queue_height > 0 {
        queue::render(frame, queue_area, app);
    }

    input_box::render(frame, input_area, app);

    // Command palette popup (above input box)
    if app.mode == crate::app::Mode::Command && !app.command_palette.is_empty() {
        palette::render(frame, input_area, app);
    }

    // Action palette popup (spacebar on selected block)
    if app.mode == crate::app::Mode::Action {
        render_action_palette(frame, input_area, app);
    }

    // Settings overlay on top of everything
    if app.settings.is_some() {
        settings::render(frame, app);
    }
}

const BLOCK_ACTIONS: &[(&str, &str)] = &[
    ("1 Expand", "Toggle expand/collapse"),
    ("2 Save",   "Write block to .di/out/"),
    ("3 Copy",   "Copy to clipboard"),
];

fn render_action_palette(frame: &mut Frame, input_area: Rect, app: &App) {
    let count = BLOCK_ACTIONS.len() as u16;
    let w = 36u16;
    let h = count + 2;
    let y = input_area.y.saturating_sub(h);
    let x = input_area.x + 4;

    let area = Rect::new(x, y, w, h);
    frame.render_widget(Clear, area);

    let mut lines = Vec::new();
    for (i, (label, desc)) in BLOCK_ACTIONS.iter().enumerate() {
        let is_selected = i == app.action_cursor;
        let marker = if is_selected { "\u{25B6} " } else { "  " };
        let style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(vec![
            Span::styled(marker.to_string(), style),
            Span::styled(format!("{:<10}", label), style),
            Span::styled(format!(" {}", desc), Style::default().fg(Color::DarkGray)),
        ]));
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}
