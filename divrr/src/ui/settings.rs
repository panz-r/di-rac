use crate::app::App;
use crate::settings::{FieldKind, ROLES, role_label};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

// Label column width for field rows
const LABEL_W: usize = 38;

pub fn render(frame: &mut Frame, app: &App) {
    let settings = match &app.settings {
        Some(s) => s,
        None => return,
    };

    let size = frame.area();
    let panel_w = size.width.min(72);

    if settings.loading {
        render_loading_overlay(frame, size, panel_w);
        return;
    }

    if settings.secret_edit_open {
        render_secret_edit_modal(frame, settings, size, panel_w);
        return;
    }

    if settings.selector_open {
        render_selector_modal(frame, settings, size, panel_w);
        return;
    }

    // Layout: 2 border + 1 role tabs + 1 separator + fields + 1 status
    let field_count = settings.fields.len();
    let desired_h: u16 = 5 + field_count as u16;
    let panel_h = desired_h.min(size.height);
    let max_visible_fields = (panel_h as usize).saturating_sub(5);

    // Scroll so active field stays visible
    let active_field = if settings.cursor > 0 { settings.cursor - 1 } else { 0 };
    let scroll = if active_field >= max_visible_fields {
        active_field - max_visible_fields + 1
    } else {
        0
    };
    let visible_fields = field_count.min(max_visible_fields);

    let x = (size.width.saturating_sub(panel_w)) / 2;
    let y = (size.height.saturating_sub(panel_h)) / 2;
    let area = Rect::new(x, y, panel_w, panel_h);

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Provider Settings ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut constraints = vec![
        Constraint::Length(1), // role tabs
        Constraint::Length(1), // separator
    ];
    for _ in 0..visible_fields {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(0)); // status

    let rows = Layout::vertical(&constraints).split(inner);

    // -- Role tabs --
    render_role_tabs(frame, rows[0], settings);

    // -- Fields (single-line each) --
    let provider_settings = settings.provider_info.as_ref()
        .map(|info| info.settings.as_slice())
        .unwrap_or(&[]);

    for vi in 0..visible_fields {
        let fi = scroll + vi;
        if fi >= field_count { break; }
        let field = &settings.fields[fi];
        let row = rows[2 + vi];
        let is_active = settings.cursor == fi + 1;
        let is_selector = field.kind() == FieldKind::Selector;
        let is_secret = field.kind() == FieldKind::Secret;
        let is_dynamic = fi >= 4;

        let active_label = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
        let dim_label = Style::default().fg(Color::DarkGray);
        let label_style = if is_active { active_label } else { dim_label };

        // Build label text
        let label_text = if is_dynamic {
            let idx = fi - 4;
            if let Some(ps) = provider_settings.get(idx) {
                match ps.setting_type.as_str() {
                    "slider" => format!(" {} [{}]:", field.label(), format_range(ps.min, ps.max, ps.step)),
                    "toggle" => format!(" {} [toggle]:", field.label()),
                    "number" => format!(" {} [{}]:", field.label(), format_range(ps.min, ps.max, None)),
                    _ => format!(" {}:", field.label()),
                }
            } else {
                format!(" {}:", field.label())
            }
        } else {
            format!(" {}:", field.label())
        };

        // Build value text
        let display = field.display_value();
        let is_empty = display.is_empty();
        let value_style = if is_active { Style::default().fg(Color::White) } else { Style::default().fg(Color::Gray) };
        let placeholder_style = Style::default().fg(Color::DarkGray);

        let mut value_str = String::new();
        let mut value_fg = value_style;

        if is_empty {
            match field.kind() {
                FieldKind::Secret => {
                    if is_active {
                        value_str = "\u{2588} enter API key...".into();
                        value_fg = placeholder_style;
                    }
                }
                FieldKind::Text => {
                    let ph = if is_dynamic { "(default)" } else { "(optional)" };
                    value_str = if is_active { format!("\u{2588} {}", ph) } else { ph.to_string() };
                    value_fg = placeholder_style;
                }
                _ => {}
            }
        } else {
            value_str = display.clone();
            if is_active && !is_selector {
                value_str.push('\u{2588}');
            }
        }

        // Pad label to fixed width, then value
        let padded_label = format!("{:<width$}", label_text, width = LABEL_W);

        let mut spans = vec![
            Span::styled(&padded_label, label_style),
        ];

        // Value spans
        if is_active {
            if is_empty {
                // Show placeholder with cursor block
                spans.push(Span::styled(&value_str, value_fg));
            } else if is_selector || is_secret {
                spans.push(Span::styled(&value_str, value_style));
            } else {
                // Non-empty text/number field: show value + cursor block as separate spans
                spans.push(Span::styled(&display, value_style));
                spans.push(Span::styled("\u{2588}", Style::default().fg(Color::White)));
            }
        } else if !value_str.is_empty() {
            spans.push(Span::styled(&value_str, value_fg));
        }

        // Hints
        if is_active {
            if is_secret {
                spans.push(Span::styled(" [Tab=edit]", Style::default().fg(Color::DarkGray)));
            } else if is_selector {
                spans.push(Span::styled(" [\u{2190}\u{2192} Tab]", Style::default().fg(Color::DarkGray)));
            }
        }

        let line = Paragraph::new(Line::from(spans));
        frame.render_widget(line, row);

        // Cursor position for text fields
        if is_active && !is_selector && !is_secret && !is_empty {
            let cursor_x = LABEL_W + display.len();
            frame.set_cursor_position((row.x + cursor_x as u16, row.y));
        }
    }

    // -- Status --
    let status_row = *rows.last().unwrap();
    let scroll_hint = if field_count > max_visible_fields {
        let vis_end = (scroll + visible_fields).min(field_count);
        format!(" [{}-{}/{}]", scroll + 1, vis_end, field_count)
    } else {
        String::new()
    };

    let status_text = if settings.saved && settings.error.is_none() {
        Line::from(Span::styled(
            format!(" Saved! Press Esc to close{}", scroll_hint),
            Style::default().fg(Color::Green),
        ))
    } else if let Some(err) = &settings.error {
        Line::from(Span::styled(
            format!(" {}{}", err, scroll_hint),
            Style::default().fg(Color::Red),
        ))
    } else {
        Line::from(Span::styled(
            format!(" Enter=save  Esc=cancel  Tab=select  j/k=nav{}", scroll_hint),
            Style::default().fg(Color::DarkGray),
        ))
    };
    frame.render_widget(Paragraph::new(status_text).alignment(Alignment::Center), status_row);
}

fn format_range(min: Option<f64>, max: Option<f64>, step: Option<f64>) -> String {
    match (min, max) {
        (Some(lo), Some(hi)) => {
            let lo_str = if lo == lo.floor() { format!("{}", lo as i64) } else { format!("{:.2}", lo) };
            let hi_str = if hi == hi.floor() { format!("{}", hi as i64) } else { format!("{:.2}", hi) };
            if let Some(s) = step {
                let s_str = if s == s.floor() { format!("{}", s as i64) } else { format!("{:.2}", s) };
                format!("{}..{} step {}", lo_str, hi_str, s_str)
            } else {
                format!("{}..{}", lo_str, hi_str)
            }
        }
        (Some(lo), None) => format!("{}..", lo),
        (None, Some(hi)) => format!("..{}", hi),
        _ => String::new(),
    }
}

fn render_role_tabs(frame: &mut Frame, area: Rect, settings: &crate::settings::SettingsState) {
    let is_active = settings.cursor == 0;
    let mut spans = Vec::new();

    for (i, role) in ROLES.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        let is_current = i == settings.role_index;
        let style = if is_active && is_current {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let prefix = if is_active && is_current { "\u{25C0} " } else if is_current { "[" } else { "" };
        let suffix = if is_active && is_current { " \u{25B6}" } else if is_current { "]" } else { "" };
        spans.push(Span::styled(format!("{}{}{}", prefix, role_label(role), suffix), style));
    }

    let para = Paragraph::new(Line::from(spans)).alignment(Alignment::Center);
    frame.render_widget(para, area);
}

fn render_selector_modal(
    frame: &mut Frame,
    settings: &crate::settings::SettingsState,
    size: Rect,
    panel_w: u16,
) {
    let fo = settings.field_offset();
    let field = &settings.fields[fo];
    let count = settings.selector_filtered_indices.len();

    let visible = (size.height as usize).saturating_sub(8).min(15).max(3);
    let visible = visible.min(count).max(1);
    let modal_h = visible as u16 + 4;

    let x = (size.width.saturating_sub(panel_w)) / 2;
    let y = (size.height.saturating_sub(modal_h)) / 2;
    let area = Rect::new(x, y, panel_w, modal_h);

    frame.render_widget(Clear, area);

    let title = if settings.selector_filter.is_empty() {
        format!(" {} \u{2014} type to filter ", field.label())
    } else {
        format!(" {} [{}] ", field.label(), settings.selector_filter)
    };
    let block = Block::default()
        .title(title.as_str())
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let scroll = if settings.selector_cursor >= visible {
        settings.selector_cursor - visible + 1
    } else {
        0
    };

    let mut lines = Vec::new();
    for fi in scroll..count {
        if lines.len() >= visible { break; }
        let label = settings.selector_label_at(fi);
        let is_selected = fi == settings.selector_cursor;
        let style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let marker = if is_selected { "\u{25B6} " } else { "  " };
        lines.push(Line::from(Span::styled(format!("{}{}", marker, label), style)));
    }

    while lines.len() < visible {
        lines.push(Line::from(""));
    }

    let list = Paragraph::new(lines);
    frame.render_widget(list, inner);
}

fn render_secret_edit_modal(
    frame: &mut Frame,
    settings: &crate::settings::SettingsState,
    size: Rect,
    panel_w: u16,
) {
    let fo = settings.field_offset();
    let field = &settings.fields[fo];

    let modal_h = 7u16.min(size.height);
    let x = (size.width.saturating_sub(panel_w)) / 2;
    let y = (size.height.saturating_sub(modal_h)) / 2;
    let area = Rect::new(x, y, panel_w, modal_h);

    frame.render_widget(Clear, area);

    let title = format!(" {} ", field.label());
    let block = Block::default()
        .title(title.as_str())
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let display = if settings.secret_edit_buffer.is_empty() {
        Paragraph::new(Line::from(Span::styled(
            "(empty \u{2014} type or paste your API key)",
            Style::default().fg(Color::DarkGray),
        )))
    } else {
        Paragraph::new(Line::from(Span::styled(
            settings.secret_edit_buffer.clone(),
            Style::default().fg(Color::White),
        )))
        .wrap(Wrap { trim: false })
    };
    frame.render_widget(display, inner);

    // Cursor positioned at secret_edit_cursor within the text
    let text = &settings.secret_edit_buffer;
    let cursor_byte = settings.secret_edit_cursor.min(text.len());
    let text_before_cursor = &text[..cursor_byte];

    if text.is_empty() {
        frame.set_cursor_position((inner.x, inner.y));
    } else {
        // Count rows from newlines in text before cursor
        let row = text_before_cursor.lines().count().max(1) - 1;
        let last_newline = text_before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col_text = &text_before_cursor[last_newline..];
        let col = unicode_width::UnicodeWidthStr::width(col_text) as u16;

        let cx = inner.x + col.min(inner.width.saturating_sub(1));
        let cy = inner.y + (row as u16).min(inner.height.saturating_sub(1));
        frame.set_cursor_position((cx, cy));
    }
}

fn render_loading_overlay(frame: &mut Frame, size: Rect, panel_w: u16) {
    let overlay_h: u16 = 5;
    let x = (size.width.saturating_sub(panel_w)) / 2;
    let y = (size.height.saturating_sub(overlay_h)) / 2;
    let area = Rect::new(x, y, panel_w, overlay_h);

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Provider Settings ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let loading = Paragraph::new(Line::from(Span::styled(
        "Loading settings...",
        Style::default().fg(Color::Yellow),
    )))
    .block(block)
    .alignment(Alignment::Center);

    frame.render_widget(loading, area);
}
