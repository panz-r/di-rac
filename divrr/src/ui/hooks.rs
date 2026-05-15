use crate::app::App;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

pub fn render(frame: &mut Frame, app: &App) {
    let hooks = match &app.hooks_editor {
        Some(h) => h,
        None => return,
    };

    let theme = &app.theme;
    let size = frame.area();

    // Full-screen overlay — clear everything first
    let area = Rect::new(0, 0, size.width, size.height);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Hook Editor ")
        .title_style(theme.accent_bold())
        .borders(Borders::ALL)
        .border_style(theme.text_dim());
    let inner = block.inner(area);
    let ih = inner.height;

    frame.render_widget(block, area);

    // Layout: help(1) + editor + status(1)
    let header_h = 1u16;
    let footer_h = 1u16;
    let editor_h = ih.saturating_sub(header_h + footer_h).max(1);

    let rows = Layout::vertical([
        Constraint::Length(header_h),
        Constraint::Fill(1),
        Constraint::Length(footer_h),
    ]).split(inner);

    // Help bar
    let help_text = Line::from(vec![
        Span::styled(" Ctrl+S Session  ", Style::default().fg(theme.accent)),
        Span::styled(" Esc Close  ", Style::default().fg(theme.warning)),
        Span::styled(" F5 Preview  ", Style::default().fg(theme.accent)),
        Span::styled(" Ctrl+R Repo  ", Style::default().fg(theme.warning)),
    ]);
    frame.render_widget(Paragraph::new(help_text), rows[0]);

    // Editor content with scrolling — no word wrap, preserve newlines
    let display_text = if hooks.source.is_empty() {
        "  (empty — type .dhook DSL here)".to_string()
    } else {
        let (before, after) = hooks.source.split_at(hooks.cursor.min(hooks.source.len()));
        let mut with_cursor = String::with_capacity(hooks.source.len() + 2);
        with_cursor.push_str(before);
        with_cursor.push('|');
        with_cursor.push_str(after);
        with_cursor
    };
    let line_count = display_text.lines().count();
    let visible_lines = editor_h as usize;

    // Compute scroll offset so cursor stays visible
    let cursor_line = hooks.source[..hooks.cursor.min(hooks.source.len())].matches('\n').count();
    let scroll = if cursor_line >= visible_lines {
        cursor_line - visible_lines + 1
    } else {
        0
    };

    let editor_lines: Vec<Line> = display_text.lines()
        .map(|l| Line::from(Span::raw(l.to_string())))
        .collect();
    let editor_paragraph = Paragraph::new(editor_lines)
        .scroll((scroll as u16, 0));
    frame.render_widget(editor_paragraph, rows[1]);

    // Status bar (diagnostics + save state)
    let line_count = hooks.source.lines().count().to_string();
    let status_parts: Vec<Span> = if hooks.diagnostics.is_empty() {
        vec![
            Span::styled(" No errors", theme.success_style()),
            Span::raw(" | "),
            Span::styled(format!(" {} lines", line_count), theme.text_dim()),
        ]
    } else {
        let mut parts = Vec::new();
        for d in hooks.diagnostics.iter().take(2) {
            parts.push(Span::styled(format!(" {}", d), theme.warning_style()));
            parts.push(Span::raw(" |"));
        }
        parts
    };

    let save_msg = if let Some(ref err) = hooks.error {
        err.clone()
    } else if hooks.saving {
        " Saving...".to_string()
    } else if hooks.saved {
        " Saved".to_string()
    } else {
        " Unsaved".to_string()
    };
    let save_style = if hooks.error.is_some() { theme.error_style() }
        else if hooks.saving { Style::default().fg(theme.accent) }
        else if hooks.saved { theme.success_style() }
        else { theme.warning_style() };

    let mut all_spans: Vec<Span> = status_parts;
    if !all_spans.is_empty() {
        all_spans.push(Span::raw(" | "));
    }
    all_spans.push(Span::styled(save_msg, save_style));
    let status_line = Line::from(all_spans);
    frame.render_widget(Paragraph::new(status_line), rows[2]);
}
