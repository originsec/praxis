mod config_tab;
mod sessions_tab;
mod tools_tab;
pub mod tree;

use crate::app::{App, ReconOverlay, ReconTab};
use crate::ui::chrome;
use crate::ui::common::short_id;
use crate::ui::hits::{split_border_rect, HintRegistrar, MouseAction, ReconHintAction};
use crate::ui::theme::{
    ACCENT, BORDER_SUBTLE, DIM, MUTED, STATUS_FAIL, STATUS_RUNNING, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render_recon(f: &mut Frame, area: Rect, app: &App, overlay: &ReconOverlay) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // divider
        Constraint::Length(1), // tabs
        Constraint::Length(1), // filter
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(area);

    render_header(f, chunks[0], overlay);
    render_divider(f, chunks[1]);
    render_tab_bar(f, chunks[2], overlay);
    register_tab_hits(app, chunks[2], overlay);
    render_filter_bar(f, chunks[3], overlay);
    app.hits_register(chunks[3], MouseAction::ReconFilterBar);

    //
    // Base pane hits first; tab renderers layer per-row hits on top
    // so tree row / chevron clicks win the hit test. Split border is
    // registered last so drag still wins on the divider column.
    //
    register_content_hits(app, chunks[4], overlay);
    match overlay.active_tab {
        ReconTab::Config => config_tab::render(f, chunks[4], app, overlay),
        ReconTab::Tools => tools_tab::render(f, chunks[4], app, overlay),
        ReconTab::Sessions => sessions_tab::render(f, chunks[4], app, overlay),
    }
    register_split_border_hit(app, chunks[4], overlay);
    register_hint_hits(app, chunks[5], overlay);

    render_hints(f, chunks[5], overlay);
}

fn register_tab_hits(app: &App, area: Rect, overlay: &ReconOverlay) {
    let counts = [
        overlay
            .recon_result
            .as_ref()
            .map_or(0, |r| r.config.items.len()),
        overlay.recon_result.as_ref().map_or(0, |r| {
            r.tools.mcp_servers.len() + r.tools.skills.len() + r.tools.internal_tools.len()
        }),
        overlay
            .recon_result
            .as_ref()
            .map_or(0, |r| r.sessions.items.len()),
    ];
    let labels = ["Config", "Tools", "Sessions"];
    let tabs = [ReconTab::Config, ReconTab::Tools, ReconTab::Sessions];
    let mut x = 0u16;
    for i in 0..3 {
        let w = chrome::tab_width(labels[i], Some(counts[i]));
        app.hits_register(
            Rect::new(area.x.saturating_add(x), area.y, w, 1),
            MouseAction::ReconTab(tabs[i]),
        );
        x = x.saturating_add(w);
        if i < 2 {
            x = x.saturating_add(chrome::tab_sep_width());
        }
    }
}

fn register_content_hits(app: &App, content: Rect, overlay: &ReconOverlay) {
    let (left, right) = common_two_pane_layout(content, overlay.recon_split_percent);
    app.hits_register(right, MouseAction::ReconRightPane);
    //
    // Per-row hits are registered by each tab's left-pane renderer.
    // Keep a fallback pane hit under them for focus-only clicks.
    //
    app.hits_register(left, MouseAction::ReconLeftPane);
}

fn register_split_border_hit(app: &App, content: Rect, overlay: &ReconOverlay) {
    let (left, _) = common_two_pane_layout(content, overlay.recon_split_percent);
    app.hits_register(split_border_rect(left), MouseAction::ReconSplitDragStart);
}

fn register_hint_hits(app: &App, area: Rect, overlay: &ReconOverlay) {
    use crate::keymap::action;
    let mut reg = HintRegistrar::new(app, area);
    reg.chip(action::REFRESH, MouseAction::ReconHint(ReconHintAction::Refresh));
    reg.chip(" refresh", MouseAction::ReconHint(ReconHintAction::Refresh));
    reg.gap(4);
    reg.chip(action::DISCOVER, MouseAction::ReconHint(ReconHintAction::Discover));
    reg.chip(" discover", MouseAction::ReconHint(ReconHintAction::Discover));
    if overlay.active_tab == ReconTab::Config {
        reg.gap(4);
        reg.chip(action::EDIT, MouseAction::ReconHint(ReconHintAction::Edit));
        reg.chip(" edit", MouseAction::ReconHint(ReconHintAction::Edit));
    }
    reg.gap(4);
    reg.chip(action::ESC, MouseAction::ReconHint(ReconHintAction::Close));
    reg.chip(" close", MouseAction::ReconHint(ReconHintAction::Close));
}

fn render_header(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    let mut spans = vec![
        chrome::diamond(ACCENT),
        Span::raw(" "),
        Span::styled(
            "Recon",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        chrome::mid_dot(),
        Span::styled(&overlay.agent_short_name, Style::default().fg(ACCENT)),
        chrome::mid_dot(),
        Span::styled(
            format!("@ {}", short_id(&overlay.node_id)),
            Style::default().fg(DIM),
        ),
    ];

    if overlay.is_semantic {
        spans.push(Span::raw("  "));
        spans.push(chrome::pill("AI", ACCENT));
    }
    if let Some((ref msg, at)) = overlay.config_edit_status {
        if at.elapsed() < std::time::Duration::from_secs(3) {
            spans.push(Span::raw("  "));
            let style = if msg == "Saved" || msg == "No changes" {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(STATUS_FAIL)
                    .add_modifier(Modifier::BOLD)
            };
            spans.push(Span::styled(msg.clone(), style));
        }
    }
    if overlay.is_loading {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            "loading…",
            Style::default()
                .fg(STATUS_RUNNING)
                .add_modifier(Modifier::ITALIC),
        ));
    } else if let Some(ref error) = overlay.error {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("error: {}", error),
            Style::default()
                .fg(STATUS_FAIL)
                .add_modifier(Modifier::BOLD),
        ));
    } else if let Some(ref at) = overlay.performed_at {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(format!("[{}]", at), Style::default().fg(DIM)));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_divider(f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(area.width as usize),
            Style::default().fg(BORDER_SUBTLE),
        ))),
        area,
    );
}

fn render_filter_bar(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    use crate::ui::filter_bar::{self, FilterBarModel};
    filter_bar::render(
        f,
        area,
        &FilterBarModel {
            focused: overlay.filter_focused,
            query: &overlay.filter,
            placeholder: "filter",
            extra_pills: Vec::new(),
            meta: None,
        },
    );
}

fn render_hints(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    use crate::keymap::action;
    use crate::ui::hint_row::{self, HintItem};
    use crate::ui::hits::ReconHintAction;

    let mut items = vec![
        HintItem::with_action(
            action::REFRESH,
            "refresh",
            MouseAction::ReconHint(ReconHintAction::Refresh),
        ),
        HintItem::with_action(
            action::DISCOVER,
            "discover",
            MouseAction::ReconHint(ReconHintAction::Discover),
        ),
    ];
    if overlay.active_tab == ReconTab::Config {
        items.push(HintItem::with_action(
            action::EDIT,
            "edit",
            MouseAction::ReconHint(ReconHintAction::Edit),
        ));
    }
    items.push(HintItem::with_action(
        action::ESC,
        "close",
        MouseAction::ReconHint(ReconHintAction::Close),
    ));
    hint_row::render(f, area, &items, None);
}

fn render_tab_bar(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    let config_count = overlay
        .recon_result
        .as_ref()
        .map_or(0, |r| r.config.items.len());
    let tools_count = overlay.recon_result.as_ref().map_or(0, |r| {
        r.tools.mcp_servers.len() + r.tools.skills.len() + r.tools.internal_tools.len()
    });
    let sessions_count = overlay
        .recon_result
        .as_ref()
        .map_or(0, |r| r.sessions.items.len());

    let mut spans = Vec::new();
    spans.extend(chrome::tab(
        "Config",
        Some(config_count),
        overlay.active_tab == ReconTab::Config,
    ));
    spans.push(chrome::tab_sep());
    spans.extend(chrome::tab(
        "Tools",
        Some(tools_count),
        overlay.active_tab == ReconTab::Tools,
    ));
    spans.push(chrome::tab_sep());
    spans.extend(chrome::tab(
        "Sessions",
        Some(sessions_count),
        overlay.active_tab == ReconTab::Sessions,
    ));
    spans.push(Span::raw("      "));
    spans.push(Span::styled("tab", Style::default().fg(TEXT_BRIGHT)));
    spans.push(Span::styled(" switch", Style::default().fg(MUTED)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn common_two_pane_layout(area: Rect, split_percent: u16) -> (Rect, Rect) {
    crate::ui::list_detail::two_pane(area, split_percent)
}

//
// Layout of the recon overlay's vertical sections within the area that
// nodes::render hands to render_recon. Mirrors the Layout::vertical
// split in render_recon — keep these in sync.
//

pub struct ReconAreas {
    pub content: Rect,
}

pub fn recon_areas(area: Rect) -> ReconAreas {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // divider
        Constraint::Length(1), // tabs
        Constraint::Length(1), // filter
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(area);
    ReconAreas {
        content: chunks[4],
    }
}

//
// Shared left-tree renderer used by all three tabs.
//

pub fn render_tree_left(
    f: &mut Frame,
    area: Rect,
    app: &App,
    overlay: &ReconOverlay,
    title: &str,
) {
    use crate::ui::common::focused_titled_panel;

    let block = focused_titled_panel(title, !overlay.right_pane_focused);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if overlay.recon_result.is_none() {
        let msg = if overlay.is_loading {
            " Loading recon data..."
        } else if overlay.error.is_some() {
            " Error loading recon"
        } else {
            " No recon data available"
        };
        let style = if overlay.is_loading {
            Style::default().fg(STATUS_RUNNING)
        } else {
            Style::default().fg(STATUS_FAIL)
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, style))),
            inner,
        );
        return;
    }

    let rows = tree::build_visible_rows(overlay);
    if rows.is_empty() {
        let msg = if overlay.filter.trim().is_empty() {
            " Nothing discovered"
        } else {
            " No matches"
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                msg,
                Style::default().fg(DIM),
            ))),
            inner,
        );
        return;
    }

    //
    // Selection/scroll are mutated on the overlay through Cell-free
    // interior — we only read here. Callers that navigate sync via
    // tree::sync_selection before render when needed. For display we
    // recompute a local scroll so the selected row stays visible.
    //

    let sel_idx = tree::selected_visible_index(overlay, &rows).unwrap_or(0);
    let h = inner.height as usize;
    let mut scroll = overlay.tree_scroll as usize;
    if sel_idx < scroll {
        scroll = sel_idx;
    } else if h > 0 && sel_idx >= scroll + h {
        scroll = sel_idx + 1 - h;
    }

    let mut lines: Vec<Line> = Vec::new();
    for (vis_i, row) in rows.iter().enumerate().skip(scroll).take(h) {
        let is_selected = overlay.selected.as_ref() == Some(&row.id);
        let is_hovered = overlay.hovered_row == Some(vis_i);
        lines.push(tree::row_line(row, is_selected, is_hovered, inner.width));

        //
        // Register per-row mouse targets. Chevron occupies the first
        // few columns after the cursor glyph; rest of the row selects.
        //

        let row_y = inner.y.saturating_add((vis_i - scroll) as u16);
        let chevron_w = 4u16.saturating_add((row.depth as u16).saturating_mul(2));
        // Full row first, then chevron on top (last registered wins).
        app.hits_register(
            Rect::new(inner.x, row_y, inner.width, 1),
            MouseAction::ReconTreeRow { row: vis_i },
        );
        if row.expandable {
            app.hits_register(
                Rect::new(inner.x, row_y, chevron_w.min(inner.width), 1),
                MouseAction::ReconTreeChevron { row: vis_i },
            );
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}
