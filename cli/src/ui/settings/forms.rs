use super::EDIT_FG;
use crate::app::{ModelEditForm, SettingsState};
use crate::ui::theme::{ACCENT, BG, DIM, MUTED, POPUP_HIGHLIGHT_BG, SETTINGS_HIGHLIGHT_BG, TEXT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub(super) fn render_model_dropdown(f: &mut Frame, area: Rect, state: &SettingsState) {
    let items = &state.model_definitions;
    if items.is_empty() {
        return;
    }

    let height = (items.len() as u16 + 2).min(area.height.saturating_sub(4));
    let width = items.iter().map(|d| d.name.len()).max().unwrap_or(20) as u16 + 6;
    let width = width.min(area.width.saturating_sub(4));

    //
    // Center the dropdown in the area.
    //

    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(" Select Model ")
        .style(Style::default().bg(BG));

    let inner = block.inner(popup_area);
    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);

    let mut lines: Vec<Line> = Vec::new();
    for (i, def) in items.iter().enumerate() {
        let selected = i == state.dropdown_selected;
        let style = if selected {
            Style::default().fg(ACCENT).bg(SETTINGS_HIGHLIGHT_BG)
        } else {
            Style::default().fg(TEXT)
        };
        let prefix = if selected { "\u{25b8} " } else { "  " };
        lines.push(Line::from(Span::styled(
            format!("{}{}", prefix, def.name),
            style,
        )));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

//
// Returns (before_cursor, after_cursor) text visible within max_width,
// scrolled to keep the cursor visible. When not editing, cursor_pos
// should be set to text length to show the tail.
//

fn scroll_field_parts(text: &str, cursor_pos: usize, max_width: usize) -> (String, String) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    if len <= max_width {
        let before: String = chars[..cursor_pos.min(len)].iter().collect();
        let after: String = chars[cursor_pos.min(len)..].iter().collect();
        return (before, after);
    }

    //
    // Need to scroll. Keep cursor visible within the window.
    // Reserve 1 char for ellipsis on whichever side is truncated.
    //

    let visible = max_width.saturating_sub(1); // leave room for ellipsis
    let cpos = cursor_pos.min(len);

    // Determine the visible window start.
    let start = if cpos <= visible { 0 } else { cpos - visible };

    let end = (start + max_width).min(len);

    let before: String = if start > 0 {
        let mut s = String::from("\u{2026}");
        s.extend(&chars[start + 1..cpos.min(end)]);
        s
    } else {
        chars[..cpos.min(end)].iter().collect()
    };

    let after: String = if end < len {
        let mut s: String = chars[cpos.min(end)..end.saturating_sub(1)].iter().collect();
        s.push('\u{2026}');
        s
    } else {
        chars[cpos.min(end)..end].iter().collect()
    };

    (before, after)
}

pub(super) fn render_model_form(f: &mut Frame, area: Rect, form: &ModelEditForm) {
    let providers = crate::app::sorted_providers();
    let provider_name = providers
        .get(form.provider_idx)
        .map(|p| p.display_name())
        .unwrap_or("?");

    let show_base_url = form.shows_base_url();
    let field_count: u16 = if show_base_url { 4 } else { 3 }; // provider + apikey + [baseurl] + model
    let base_lines: u16 = field_count + 2 + 2; // fields + blank + hints + border top/bottom
    let dropdown_extra = if form.model_dropdown_open {
        1 + form.available_models.len() as u16 // blank + model list
    } else if form.loading_models {
        1
    } else {
        0
    };
    let height = (base_lines + dropdown_extra).min(area.height.saturating_sub(4));
    let width = 60u16.min(area.width.saturating_sub(4));

    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let title = if form.edit_index.is_some() {
        " Edit Model "
    } else {
        " Add Model "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(title)
        .style(Style::default().bg(BG));

    let inner = block.inner(popup_area);
    form.model_dropdown_inner_h.set(inner.height as usize);
    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);

    let mut lines: Vec<Line> = Vec::new();

    //
    // Provider field (arrows to cycle).
    //

    let prov_sel = form.focused_field == 0;
    lines.push(Line::from(vec![
        Span::styled(
            if prov_sel { "\u{25b8} " } else { "  " },
            Style::default().fg(if prov_sel { ACCENT } else { TEXT }),
        ),
        Span::styled(
            "Provider    ",
            Style::default().fg(if prov_sel { ACCENT } else { TEXT }),
        ),
        Span::styled(
            format!("\u{25c2} {} \u{25b8}", provider_name),
            if prov_sel {
                Style::default().fg(EDIT_FG)
            } else {
                Style::default().fg(MUTED)
            },
        ),
    ]));

    //
    // API key field.
    //

    let field_max = inner.width.saturating_sub(16) as usize;
    let edit_style = Style::default().fg(EDIT_FG);
    let cursor_style = Style::default().fg(ACCENT);

    //
    // Helper to build a text field line with cursor support.
    //

    let build_field =
        |label: &str, text: &str, selected: bool, editing: bool, cursor_pos: usize| -> Line {
            let sel_fg = if selected { ACCENT } else { TEXT };
            let prefix = if selected { "\u{25b8} " } else { "  " };

            if editing && selected {
                let (before, after) = scroll_field_parts(text, cursor_pos, field_max);
                let spans = vec![
                    Span::styled(prefix, Style::default().fg(sel_fg)),
                    Span::styled(label.to_string(), Style::default().fg(sel_fg)),
                    Span::styled(before, edit_style),
                    Span::styled("\u{258f}", cursor_style),
                    Span::styled(after, edit_style),
                ];
                Line::from(spans)
            } else {
                let (before, after) = scroll_field_parts(text, text.chars().count(), field_max);
                let display = format!("{}{}", before, after);
                Line::from(vec![
                    Span::styled(prefix, Style::default().fg(sel_fg)),
                    Span::styled(label.to_string(), Style::default().fg(sel_fg)),
                    Span::styled(display, Style::default().fg(MUTED)),
                ])
            }
        };

    //
    // API key: mask when not editing. Show (optional) hint for local providers.
    //

    let key_sel = form.logical_field() == 1 && form.focused_field == 1;
    let key_text;
    let api_key_optional = providers
        .get(form.provider_idx)
        .map(|p| p.api_key_optional())
        .unwrap_or(false);
    let key_label = if api_key_optional {
        "API Key (opt)"
    } else {
        "API Key     "
    };
    let key_display = if key_sel && form.editing_text {
        &form.api_key
    } else if form.api_key.is_empty() {
        ""
    } else {
        let len = form.api_key.chars().count();
        key_text = if len <= 4 {
            form.api_key.clone()
        } else {
            let tail: String = form.api_key.chars().skip(len - 4).collect();
            format!("{}{}", "\u{2022}".repeat(len - 4), tail)
        };
        &key_text
    };

    lines.push(build_field(
        key_label,
        key_display,
        key_sel,
        form.editing_text,
        form.cursor_pos,
    ));

    //
    // Base URL field (only for local/custom providers).
    //

    if show_base_url {
        let url_sel = form.focused_field == 2;
        lines.push(build_field(
            "Base URL    ",
            &form.base_url,
            url_sel,
            form.editing_text,
            form.cursor_pos,
        ));
    }

    //
    // Model name field.
    //

    let model_field_idx = if show_base_url { 3 } else { 2 };
    let mod_sel = form.focused_field == model_field_idx;
    lines.push(build_field(
        "Model       ",
        &form.model_name,
        mod_sel,
        form.editing_text,
        form.cursor_pos,
    ));

    lines.push(Line::raw(""));

    //
    // Hints.
    //

    let mut hints = vec![
        Span::styled("  ^s", Style::default().fg(DIM)),
        Span::styled(" save  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(DIM)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ];
    if form.logical_field() == 3 && form.editing_text {
        hints.push(Span::styled("  enter", Style::default().fg(DIM)));
        hints.push(Span::styled(" load models", Style::default().fg(MUTED)));
    }
    lines.push(Line::from(hints));

    if form.loading_models {
        lines.push(Line::from(Span::styled(
            "  Loading models...",
            Style::default().fg(MUTED),
        )));
    }

    //
    // Model dropdown if open — rendered as a separate scrollable region
    // below the fixed header so scrolling doesn't push the form fields
    // off screen.
    //

    if form.model_dropdown_open && !form.available_models.is_empty() {
        lines.push(Line::raw(""));
        let header_h = lines.len() as u16;

        let header_area = Rect {
            height: header_h,
            ..inner
        };
        f.render_widget(Paragraph::new(lines), header_area);

        let dropdown_area = Rect {
            y: inner.y + header_h,
            height: inner.height.saturating_sub(header_h),
            ..inner
        };
        form.model_dropdown_inner_h
            .set(dropdown_area.height as usize);

        let mut dropdown_lines: Vec<Line> = Vec::new();
        for (i, name) in form.available_models.iter().enumerate() {
            let selected = i == form.model_dropdown_selected;
            let style = if selected {
                Style::default().fg(ACCENT).bg(POPUP_HIGHLIGHT_BG)
            } else {
                Style::default().fg(TEXT)
            };
            let prefix = if selected { "  \u{25b8} " } else { "    " };
            dropdown_lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, name),
                style,
            )));
        }

        let scroll_y = form.model_dropdown_scroll as u16;
        let paragraph = Paragraph::new(dropdown_lines).scroll((scroll_y, 0));
        f.render_widget(paragraph, dropdown_area);
    } else {
        f.render_widget(Paragraph::new(lines), inner);
    }
}
