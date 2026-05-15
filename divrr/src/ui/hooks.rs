use crate::app::App;
use crate::theme::Theme;
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

    let panel_w = size.width.min(96).max(60);
    let panel_h = size.height.min(36).max(16);

    let x = (size.width.saturating_sub(panel_w)) / 2;
    let y = (size.height.saturating_sub(panel_h)) / 2;
    let area = Rect::new(x, y, panel_w, panel_h);

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Hook Editor (session overlay) ")
        .title_style(theme.accent_bold())
        .borders(Borders::ALL)
        .border_style(theme.text_dim());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(1),
        Constraint::Length(4),
        Constraint::Min(2),
        Constraint::Length(1),
    ]).split(inner);

    // Help bar
    let help_text = Line::from(vec![
        Span::styled(" Ctrl+S Save session  ", Style::default().fg(theme.accent)),
        Span::styled(" Esc Close  ", Style::default().fg(theme.warning)),
        Span::styled(" F5 Preview  ", Style::default().fg(theme.accent)),
        Span::styled(" Ctrl+R Save repo  ", Style::default().fg(theme.warning)),
    ]);
    frame.render_widget(Paragraph::new(help_text), rows[0]);

    // Editor area
    let editor_block = Block::default()
        .borders(Borders::TOP)
        .border_style(theme.text_dim())
        .title(" Source ");
    let editor_inner = editor_block.inner(rows[1]);
    frame.render_widget(editor_block, rows[1]);

    let display_text = if hooks.source.is_empty() {
        "  (empty — type your .dhook DSL here)".to_string()
    } else {
        let mut display = hooks.source.clone();
        let pos = hooks.cursor.min(display.len());
        display.insert(pos, '|');
        display.insert(pos + 1, '|');
        format!(" {}", display.lines().collect::<Vec<_>>().join("\n "))
    };

    let editor_paragraph = Paragraph::new(Line::from(Span::raw(display_text)))
        .wrap(Wrap { trim: false });
    frame.render_widget(editor_paragraph, editor_inner);

    // Separator
    let sep = Paragraph::new(Line::from(Span::styled(
        "── Diagnostics ──────────────────────",
        theme.text_dim(),
    )));
    frame.render_widget(sep, rows[2]);

    // Diagnostics
    let diag_text = if hooks.diagnostics.is_empty() {
        vec![Line::from(Span::styled("  No errors", theme.success_style()))]
    } else {
        hooks.diagnostics.iter()
            .map(|e| Line::from(Span::styled(format!("  {}", e), theme.warning_style())))
            .collect()
    };
    let diag_paragraph = Paragraph::new(diag_text).wrap(Wrap { trim: false });
    frame.render_widget(diag_paragraph, rows[3]);

    // Preview
    let preview_block = Block::default()
        .borders(Borders::TOP)
        .border_style(theme.text_dim())
        .title(" Preview ");
    let preview_text = if hooks.preview.is_empty() {
        vec![Line::from(Span::styled("  (press F5 to preview)", theme.text_dim()))]
    } else {
        hooks.preview.lines()
            .map(|l| Line::from(Span::raw(format!("  {}", l))))
            .collect()
    };
    let preview_paragraph = Paragraph::new(preview_text).wrap(Wrap { trim: false });
    frame.render_widget(preview_block, rows[2]); // overlay on separator
    frame.render_widget(preview_paragraph, rows[4]);

    // Status
    let status = if let Some(ref err) = hooks.error {
        Line::from(Span::styled(format!(" Error: {}", err), theme.error_style()))
    } else if hooks.saving {
        Line::from(Span::styled(" Saving...", Style::default().fg(theme.accent)))
    } else if hooks.saved {
        Line::from(Span::styled(" Saved (session)", theme.success_style()))
    } else {
        Line::from(Span::styled(" Unsaved changes", theme.warning_style()))
    };
    frame.render_widget(Paragraph::new(status), rows[5]);
}
