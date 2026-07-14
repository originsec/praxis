//
// Intercept filter bar: shared filter_bar chrome plus Traffic/Rules/Matches
// placeholders and optional server-search hint.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Span;

use crate::app::intercept::InterceptTab;
use crate::app::App;
use crate::ui::filter_bar::{self, FilterBarModel, FilterPill};

pub fn render(f: &mut Frame, area: Rect, app: &App, extra_groups: &[Vec<Span<'static>>]) {
    //
    // Convert pre-built pill span groups back into FilterPill where
    // possible; Traffic still builds pills as spans for hit registration
    // width. Render via shared filter_bar for consistent `/` chrome.
    //
    let state = &app.intercept;
    let placeholder = match state.tab {
        InterceptTab::Traffic => "filter  ^\u{21b5} server search",
        InterceptTab::Rules => "filter rules",
        InterceptTab::Matches => "filter matches",
    };

    //
    // extra_groups are already painted as spans (pills + meta). Keep the
    // legacy compose path so hit boxes stay aligned: prefix + groups.
    //
    let model = FilterBarModel {
        focused: state.search_focused,
        query: &state.search_input,
        placeholder,
        extra_pills: Vec::new(),
        meta: None,
    };

    //
    // Paint base filter then append extra_groups manually so callers can
    // still pass pre-styled pill span groups.
    //
    let mut spans = vec![
        ratatui::text::Span::styled(
            "/",
            ratatui::style::Style::default().fg(crate::ui::theme::TEXT_BRIGHT),
        ),
        ratatui::text::Span::raw(" "),
    ];
    // Reuse filter_bar content rendering by calling render into a temp is hard;
    // duplicate content span logic via filter_bar::render for the full line
    // when no extras, else compose.
    if extra_groups.is_empty() {
        filter_bar::render(f, area, &model);
        return;
    }

    // Content span matching filter_bar.
    if state.search_focused {
        if state.search_input.is_empty() {
            spans.push(ratatui::text::Span::styled(
                "\u{2588}",
                ratatui::style::Style::default().fg(crate::ui::theme::ACCENT),
            ));
        } else {
            spans.push(ratatui::text::Span::styled(
                format!("{}\u{2588}", state.search_input),
                ratatui::style::Style::default().fg(crate::ui::theme::ACCENT),
            ));
        }
    } else if state.search_input.is_empty() {
        spans.push(ratatui::text::Span::styled(
            placeholder.to_string(),
            ratatui::style::Style::default()
                .fg(crate::ui::theme::DIM)
                .add_modifier(ratatui::style::Modifier::ITALIC),
        ));
    } else {
        spans.push(ratatui::text::Span::styled(
            state.search_input.clone(),
            ratatui::style::Style::default().fg(crate::ui::theme::ACCENT),
        ));
    }

    for group in extra_groups {
        spans.push(ratatui::text::Span::raw("    "));
        spans.extend(group.iter().cloned());
    }

    f.render_widget(
        ratatui::widgets::Paragraph::new(ratatui::text::Line::from(spans)),
        area,
    );
}

/// Columns occupied by the leading `/ ` + search field (before extra groups).
pub fn search_prefix_width(app: &App) -> u16 {
    let state = &app.intercept;
    let model = FilterBarModel {
        focused: state.search_focused,
        query: &state.search_input,
        placeholder: match state.tab {
            InterceptTab::Traffic => "filter  ^\u{21b5} server search",
            InterceptTab::Rules => "filter rules",
            InterceptTab::Matches => "filter matches",
        },
        extra_pills: Vec::new(),
        meta: None,
    };
    filter_bar::prefix_width(&model)
}

pub fn pill_spans(label: &str, value: &str) -> Vec<Span<'static>> {
    filter_bar::pill_spans(label, value)
}

// Keep FilterPill available for future direct use.
#[allow(dead_code)]
pub type InterceptFilterPill = FilterPill;
