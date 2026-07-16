//
// Intercept window render dispatcher. Owns the tab header, status
// line, and delegates content to sub-tab renderers. Rule create/edit
// is `rule_form` (uses shared `form_modal` chrome).
//

mod log;
mod matches;
mod rule_form;
mod rules;
mod search_bar;

use crate::app::App;
use crate::app::intercept::{InterceptTab, body::BodyMode};
use crate::ui::chrome;
use crate::ui::common::table_data_start_titled;
use crate::ui::hits::{split_border_rect, MouseAction, RowSelect, RowSelectKind};
use crate::ui::theme::{
    ACCENT, BORDER_SUBTLE, MUTED, OK, STATUS_FAIL, STATUS_RUNNING, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub(super) fn body_lines(bytes: &[u8], mode: BodyMode) -> Vec<ratatui::text::Line<'static>> {
    crate::app::intercept::body::render_body(bytes, mode)
}

/// Footer is always on: action hints, or a transient error/status banner.
pub fn show_footer(_app: &App) -> bool {
    true
}

/// Chrome layout below the window header — shared by render and mouse hit-tests.
pub struct InterceptChrome {
    pub body: Rect,
}

pub fn chrome_layout(area: Rect, show_footer: bool) -> InterceptChrome {
    //
    // Fixed chrome rows, then Min body so the tab content always fills
    // remaining height (same pattern as ops). No per-node status strip
    // or all-off banner — enable/disable lives in the Nodes window.
    //
    let mut constraints = vec![
        Constraint::Length(1), // tab header
        Constraint::Length(1), // divider
        Constraint::Min(1),    // tab body
    ];
    if show_footer {
        constraints.push(Constraint::Length(1));
    }
    let chunks = Layout::vertical(constraints).split(area);
    let body = chunks[2];
    InterceptChrome { body }
}

/// Filter bar + horizontal split used by Traffic and Matches tabs.
pub struct FilterSplit {
    pub filter: Rect,
    pub left: Rect,
    pub right: Rect,
}

pub fn filter_split(body: Rect, split_percent: u16) -> FilterSplit {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(body);
    let pct = split_percent.clamp(20, 80);
    let split = Layout::horizontal([
        Constraint::Percentage(pct),
        Constraint::Percentage(100 - pct),
    ])
    .split(chunks[1]);
    FilterSplit {
        filter: chunks[0],
        left: split[0],
        right: split[1],
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let footer = show_footer(app);

    //
    // Same constraint stack as chrome_layout.
    //
    let mut constraints = vec![
        Constraint::Length(1), // tab header
        Constraint::Length(1), // divider
        Constraint::Min(1),    // content
    ];
    if footer {
        constraints.push(Constraint::Length(1));
    }

    let chunks = Layout::vertical(constraints).split(area);
    render_tabs(f, chunks[0], app);
    render_divider(f, chunks[1]);
    let content = chunks[2];

    let form_open = app.intercept.rule_form.is_some();

    match app.intercept.tab {
        InterceptTab::Traffic => {
            log::render(f, content, app);
            if !form_open {
                register_traffic_hits(app, content);
            }
        }
        InterceptTab::Rules => {
            rules::render(f, content, app);
            if !form_open {
                register_rules_hits(app, content);
            }
        }
        InterceptTab::Matches => {
            matches::render(f, content, app);
            if !form_open {
                register_matches_hits(app, content);
            }
        }
    }

    if footer {
        render_status_footer(f, chunks[3], app);
    }

    //
    // Rule create/edit is a centered modal over the intercept window
    // (same chrome as settings model forms), not a split pane.
    //
    if let Some(ref rf) = app.intercept.rule_form {
        rule_form::render(f, area, rf, app);
    }
}

fn register_traffic_hits(app: &App, body: Rect) {
    let panes = filter_split(body, app.intercept.log_split_percent);
    //
    // Pane hits first; split border last so drag wins hit-test on the
    // divider (last registered = top-most).
    //
    app.hits_register(panes.right, MouseAction::InterceptLogDetailFocus);
    app.hits_register(
        panes.left,
        MouseAction::SelectRow(RowSelect {
            kind: RowSelectKind::InterceptLog,
            table_area: panes.left,
            data_start: table_data_start_titled(panes.left),
        }),
    );
    app.hits_register(
        split_border_rect(panes.left),
        MouseAction::InterceptLogSplitDragStart,
    );
}

fn register_matches_hits(app: &App, body: Rect) {
    let panes = filter_split(body, app.intercept.match_split_percent);
    app.hits_register(panes.right, MouseAction::InterceptMatchDetailFocus);
    app.hits_register(
        panes.left,
        MouseAction::SelectRow(RowSelect {
            kind: RowSelectKind::InterceptMatch,
            table_area: panes.left,
            data_start: table_data_start_titled(panes.left),
        }),
    );
    app.hits_register(
        split_border_rect(panes.left),
        MouseAction::InterceptMatchSplitDragStart,
    );
}

fn register_rules_hits(app: &App, body: Rect) {
    let panes = filter_split(body, app.intercept.rule_split_percent);
    app.hits_register(panes.right, MouseAction::InterceptRuleDetailFocus);
    app.hits_register(
        panes.left,
        MouseAction::SelectRow(RowSelect {
            kind: RowSelectKind::InterceptRule,
            table_area: panes.left,
            data_start: table_data_start_titled(panes.left),
        }),
    );
    app.hits_register(
        split_border_rect(panes.left),
        MouseAction::InterceptRuleSplitDragStart,
    );
}

fn render_tabs(f: &mut Frame, area: Rect, app: &App) {
    let count = app.intercept.buffer.len();
    let rules_count = app.intercept.filtered_rule_ids().len();
    let matches_count = app.intercept.filtered_matches_len();

    let tab_specs = [
        (InterceptTab::Traffic, InterceptTab::Traffic.label(), count),
        (InterceptTab::Rules, InterceptTab::Rules.label(), rules_count),
        (InterceptTab::Matches, InterceptTab::Matches.label(), matches_count),
    ];

    let mut x = 0u16;
    for (i, (tab, label, n)) in tab_specs.iter().enumerate() {
        let w = chrome::tab_width(label, Some(*n));
        app.hits_register(
            Rect::new(area.x.saturating_add(x), area.y, w, 1),
            MouseAction::InterceptTab(*tab),
        );
        x += w;
        if i + 1 < tab_specs.len() {
            x += chrome::tab_sep_width();
        }
    }

    let mut spans: Vec<Span> = Vec::new();
    for (i, (tab, label, n)) in tab_specs.iter().enumerate() {
        if i > 0 {
            spans.push(chrome::tab_sep());
        }
        spans.extend(chrome::tab(label, Some(*n), app.intercept.tab == *tab));
    }

    if app.intercept.paused {
        spans.push(Span::raw("    "));
        spans.push(chrome::pill("PAUSED", ACCENT));
        let pending = app.intercept.paused_pending.len();
        if pending > 0 {
            spans.push(Span::styled(
                format!(" +{}", pending),
                Style::default().fg(MUTED),
            ));
        }
    } else if app.intercept.follow_tail {
        spans.push(Span::raw("    "));
        spans.push(chrome::pill("TAIL", STATUS_RUNNING));
    }

    spans.push(Span::raw("      "));
    spans.push(Span::styled("tab", Style::default().fg(TEXT_BRIGHT)));
    spans.push(Span::styled(" switch", Style::default().fg(MUTED)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_divider(f: &mut Frame, area: Rect) {
    let line = "\u{2500}".repeat(area.width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            line,
            Style::default().fg(BORDER_SUBTLE),
        ))),
        area,
    );
}

fn render_status_footer(f: &mut Frame, area: Rect, app: &App) {
    if let Some((msg, _)) = &app.intercept.last_error {
        let line = Line::from(vec![
            Span::styled("\u{25b3} ", Style::default().fg(STATUS_FAIL)),
            Span::styled(
                msg.clone(),
                Style::default()
                    .fg(STATUS_FAIL)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }

    if let Some((msg, _)) = &app.intercept.status_message {
        let line = Line::from(vec![
            Span::styled("\u{2713} ", Style::default().fg(OK)),
            Span::styled(msg.clone(), Style::default().fg(OK)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }

    render_action_hints(f, area, app);
}

fn render_action_hints(f: &mut Frame, area: Rect, app: &App) {
    use crate::keymap::action;
    use crate::ui::hint_row::{self, HintItem};

    let items: Vec<HintItem> = match app.intercept.tab {
        InterceptTab::Traffic => vec![
            HintItem::new(action::ENTER, "detail"),
            HintItem::new("p", "pause"),
            HintItem::new("t", "tail"),
            HintItem::new(action::REFRESH, "refresh"),
            HintItem::new("y", "copy"),
            HintItem::new("b", "body"),
            HintItem::new(action::CLEAR_ALL, "clear"),
        ],
        InterceptTab::Rules => vec![
            HintItem::new(action::NEW, "new"),
            HintItem::new(action::EDIT, "edit"),
            HintItem::new(action::DELETE, "delete"),
            HintItem::new(action::SPACE, "toggle"),
            HintItem::new(action::REFRESH, "refresh"),
            HintItem::new(action::ENTER, "matches"),
        ],
        InterceptTab::Matches => vec![
            HintItem::new(action::ENTER, "detail"),
            HintItem::new("f", "rule filter"),
            HintItem::new(action::REFRESH, "refresh"),
            HintItem::new("y", "copy"),
            HintItem::new("b", "body"),
            HintItem::new(action::NEW, "rule from match"),
        ],
    };
    hint_row::render(f, area, &items, None);
}