use crate::app::App;
use crate::settings::{FieldKind, ROLES, role_label};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

// Label column width for field rows
const LABEL_W: usize = 38;

pub fn render(frame: &mut Frame, app: &App) {
    let settings = match &app.settings {
        Some(s) => s,
        None => return,
    };

    let theme = &app.theme;
    let size = frame.area();
    let panel_w = size.width.min(72);

    if settings.loading {
        render_loading_overlay(frame, theme, size, panel_w);
        return;
    }

    if settings.secret_edit_open {
        render_secret_edit_modal(frame, theme, settings, size, panel_w);
        return;
    }

    if settings.selector_open {
        render_selector_modal(frame, theme, settings, size, panel_w);
        return;
    }

    // Layout: 2 border + 1 panel tabs + 1 role tabs + 1 separator + fields + 1 status
    let field_count = settings.fields.len();
    let desired_h: u16 = 6 + field_count as u16;
    let panel_h = desired_h.min(size.height);
    let max_visible_fields = (panel_h as usize).saturating_sub(6);

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

    let panel_title = match settings.active_panel {
        crate::settings::SettingsPanel::Provider => " Provider Settings ",
        crate::settings::SettingsPanel::Role => " Role Settings ",
        crate::settings::SettingsPanel::Theme => " Theme Settings ",
    };
    let block = Block::default()
        .title(panel_title)
        .title_style(theme.accent_bold())
        .borders(Borders::ALL)
        .border_style(theme.text_dim());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut constraints = vec![
        Constraint::Length(1), // panel tabs
        Constraint::Length(1), // role tabs
        Constraint::Length(1), // separator
    ];
    for _ in 0..visible_fields {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(0)); // status

    let rows = Layout::vertical(&constraints).split(inner);

    // -- Panel tabs --
    render_panel_tabs(frame, theme, rows[0], settings);

    // -- Role tabs (hidden for Theme panel) --
    if !matches!(settings.active_panel, crate::settings::SettingsPanel::Theme) {
        render_role_tabs(frame, theme, rows[1], settings);
    }

    // -- Fields (single-line each) --
    let provider_settings = settings.provider_info.as_ref()
        .map(|info| info.settings.as_slice())
        .unwrap_or(&[]);

    if settings.fields.is_empty() {
        let msg = Paragraph::new(Line::from(Span::styled(
            "  No role-specific settings",
            theme.text_dim(),
        )));
        frame.render_widget(msg, rows[2]);
    }

    for vi in 0..visible_fields {
        let fi = scroll + vi;
        if fi >= field_count { break; }
        let field = &settings.fields[fi];
        let row = rows[3 + vi];
        let is_active = settings.cursor == fi + 1;
        let is_selector = field.kind() == FieldKind::Selector;
        let is_secret = field.kind() == FieldKind::Secret;
        let is_dynamic = fi >= 4 && matches!(settings.active_panel, crate::settings::SettingsPanel::Provider);

        let label_style = if is_active { theme.selected_bold() } else { theme.text_dim() };

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
        let value_style = if is_active { theme.text() } else { theme.text_dim() };

        let mut value_str = String::new();
        let mut value_fg = value_style;

        if is_empty {
            match field.kind() {
                FieldKind::Secret => {
                    if is_active {
                        value_str = "\u{2588} enter API key...".into();
                        value_fg = theme.text_dim();
                    }
                }
                FieldKind::Text => {
                    let ph = if is_dynamic { "(default)" } else { "(optional)" };
                    value_str = if is_active { format!("\u{2588} {}", ph) } else { ph.to_string() };
                    value_fg = theme.text_dim();
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
                spans.push(Span::styled("\u{2588}", theme.text()));
            }
        } else if !value_str.is_empty() {
            spans.push(Span::styled(&value_str, value_fg));
        }

        // Hints
        if is_active {
            if is_secret {
                spans.push(Span::styled(" [Tab=edit]", theme.text_dim()));
            } else if is_selector {
                spans.push(Span::styled(" [\u{2190}\u{2192} Tab]", theme.text_dim()));
            }
        }

        let line = Paragraph::new(Line::from(spans));
        frame.render_widget(line, row);

        // Cursor position for text fields
        if is_active && !is_selector && !is_secret && !is_empty {
            let visual_w = unicode_width::UnicodeWidthStr::width(display.as_str());
            let cursor_x = LABEL_W + visual_w;
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

    let status_text = if settings.saving {
        Line::from(Span::styled(
            format!(" Saving...\u{2026}{}", scroll_hint),
            Style::default().fg(theme.warning),
        ))
    } else if settings.saved && settings.error.is_none() {
        Line::from(Span::styled(
            format!(" Saved! Press Esc to close{}", scroll_hint),
            theme.success_style(),
        ))
    } else if let Some(err) = &settings.error {
        Line::from(Span::styled(
            format!(" {}{}", err, scroll_hint),
            theme.error_style(),
        ))
    } else {
        Line::from(Span::styled(
            format!(" Enter=save  Esc=cancel  Tab=select  Shift+Tab=panel  j/k=nav{}", scroll_hint),
            theme.text_dim(),
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

fn render_panel_tabs(frame: &mut Frame, theme: &Theme, area: Rect, settings: &crate::settings::SettingsState) {
    let panels: &[(&str, crate::settings::SettingsPanel)] = &[
        ("Provider Settings", crate::settings::SettingsPanel::Provider),
        ("Role Settings", crate::settings::SettingsPanel::Role),
        ("Theme Settings", crate::settings::SettingsPanel::Theme),
    ];
    let mut spans = Vec::new();
    for (i, (label, panel)) in panels.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" | "));
        }
        let is_current = *panel == settings.active_panel;
        let style = if is_current {
            theme.accent_bold()
        } else {
            theme.text_dim()
        };
        let prefix = if is_current { "\u{25C0} " } else { "" };
        let suffix = if is_current { " \u{25B6}" } else { "" };
        spans.push(Span::styled(format!("{}{}{}", prefix, label, suffix), style));
    }
    let para = Paragraph::new(Line::from(spans)).alignment(Alignment::Center);
    frame.render_widget(para, area);
}

fn render_role_tabs(frame: &mut Frame, theme: &Theme, area: Rect, settings: &crate::settings::SettingsState) {
    let is_active = settings.cursor == 0;
    let mut spans = Vec::new();

    for (i, role) in ROLES.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        let is_current = i == settings.role_index;
        let style = if is_active && is_current {
            theme.accent_bold()
        } else if is_current {
            Style::default().fg(theme.accent)
        } else {
            theme.text_dim()
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
    theme: &Theme,
    settings: &crate::settings::SettingsState,
    size: Rect,
    panel_w: u16,
) {
    let fo = settings.field_offset();
    let field = &settings.fields[fo];
    let count = settings.selector_filtered_indices.len();

    let visible = (size.height as usize).saturating_sub(8).clamp(3, 15);
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
        .title_style(theme.accent_bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warning));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let scroll = if settings.selector_cursor >= visible {
        settings.selector_cursor - visible + 1
    } else {
        0
    };

    let mut lines = Vec::new();
    if count == 0 {
        lines.push(Line::from(Span::styled("  No matches — Esc to cancel", theme.text_dim())));
    } else {
        for fi in scroll..count {
            if lines.len() >= visible { break; }
            let label = settings.selector_label_at(fi);
            let is_selected = fi == settings.selector_cursor;
            let style = if is_selected {
                theme.selected_bold()
            } else {
                theme.text_dim()
            };
            let marker = if is_selected { "\u{25B6} " } else { "  " };
            lines.push(Line::from(Span::styled(format!("{}{}", marker, label), style)));
        }
    }

    while lines.len() < visible {
        lines.push(Line::from(""));
    }

    let list = Paragraph::new(lines);
    frame.render_widget(list, inner);
}

fn render_secret_edit_modal(
    frame: &mut Frame,
    theme: &Theme,
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
        .title_style(theme.accent_bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warning));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let display = if settings.secret_edit_buffer.is_empty() {
        Paragraph::new(Line::from(Span::styled(
            "(empty \u{2014} type or paste your API key)",
            theme.text_dim(),
        )))
    } else {
        Paragraph::new(Line::from(Span::styled(
            settings.secret_edit_buffer.clone(),
            theme.text(),
        )))
        .wrap(Wrap { trim: false })
    };
    frame.render_widget(display, inner);

    // Cursor positioned at secret_edit_cursor within the text,
    // accounting for both hard newlines and soft wrapping at inner.width.
    let text = &settings.secret_edit_buffer;
    let cursor_byte = settings.secret_edit_cursor.min(text.len());
    let text_before_cursor = &text[..cursor_byte];

    if text.is_empty() {
        frame.set_cursor_position((inner.x, inner.y));
    } else {
        let wrap_w = inner.width as usize;
        let (visual_row, visual_col) = visual_cursor_pos(text_before_cursor, wrap_w);
        let cx = inner.x + (visual_col as u16).min(inner.width.saturating_sub(1));
        let cy = inner.y + (visual_row as u16).min(inner.height.saturating_sub(1));
        frame.set_cursor_position((cx, cy));
    }
}

/// Given a string and a wrap width, compute the 0-indexed visual (row, col) of the
/// position at the end of the string, accounting for both hard newlines and soft wrapping.
fn visual_cursor_pos(text: &str, wrap_width: usize) -> (usize, usize) {
    if text.is_empty() || wrap_width == 0 {
        return (0, 0);
    }
    let mut total_rows: usize = 0;
    let mut last_col: usize = 0;

    for segment in text.split('\n') {
        let seg_w = unicode_width::UnicodeWidthStr::width(segment);
        if seg_w == 0 {
            // Empty line after a newline — still occupies one row
            total_rows += 1;
            last_col = 0;
            continue;
        }
        if seg_w <= wrap_width {
            // Fits on one line
            total_rows += 1;
            last_col = seg_w;
        } else {
            // Wraps across multiple visual lines
            let full_lines = seg_w / wrap_width;
            let remainder = seg_w % wrap_width;
            total_rows += full_lines;
            if remainder > 0 {
                total_rows += 1;
                last_col = remainder;
            } else {
                // Exactly ends at a wrap boundary — cursor is at column wrap_width
                // (the start of the next visual line)
                last_col = wrap_width;
            }
        }
    }

    let row = total_rows.saturating_sub(1);
    (row, last_col)
}

fn render_loading_overlay(frame: &mut Frame, theme: &Theme, size: Rect, panel_w: u16) {
    let overlay_h: u16 = 5;
    let x = (size.width.saturating_sub(panel_w)) / 2;
    let y = (size.height.saturating_sub(overlay_h)) / 2;
    let area = Rect::new(x, y, panel_w, overlay_h);

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Provider Settings ")
        .title_style(theme.accent_bold())
        .borders(Borders::ALL)
        .border_style(theme.text_dim());

    let loading = Paragraph::new(Line::from(Span::styled(
        "Loading settings...",
        Style::default().fg(theme.warning),
    )))
    .block(block)
    .alignment(Alignment::Center);

    frame.render_widget(loading, area);
}
