pub mod top_bar;
pub mod conversation;
pub mod queue;
pub mod input_box;

use crate::app::App;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};

pub fn render(frame: &mut Frame, app: &App) {
    let size = frame.area();

    let queue_height = if app.input_queue.is_empty() {
        0
    } else {
        app.input_queue.len().min(5) as u16
    };

    let constraints = vec![
        Constraint::Length(1),      // top bar
        Constraint::Fill(1),        // conversation
        Constraint::Length(queue_height), // queue (0 when empty)
        Constraint::Length(if app.mode == crate::app::Mode::Command { 1 } else { 1 }), // input box
    ];

    let areas = Layout::vertical(&constraints).split(size);
    let (top_area, conv_area, queue_area, input_area) = (areas[0], areas[1], areas[2], areas[3]);

    top_bar::render(frame, top_area, app);
    conversation::render(frame, conv_area, app);

    if queue_height > 0 {
        queue::render(frame, queue_area, app);
    }

    input_box::render(frame, input_area, app);
}
