//
// Matches tab: list of rule matches on the left, detail + summary
// on the right.
//

use chrono::Local;
use common::TrafficDirection;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

use crate::app::App;
use crate::app::intercept::SummaryStatus;
use crate::app::intercept::match_detail;
use crate::ui::common::focused_titled_panel;
use crate::ui::intercept::search_bar;
use crate::ui::theme::{ACCENT, BG_SELECTED, DIM, MUTED, STATUS_DONE, STATUS_RUNNING, TEXT_BRIGHT};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
    render_filter_bar(f, chunks[0], app);

    let pct = app.intercept.match_split_percent.clamp(20, 80);
    let split = Layout::horizontal([
        Constraint::Percentage(pct),
        Constraint::Percentage(100 - pct),
    ])
    .split(chunks[1]);
    render_list(f, split[0], app);
    render_detail(f, split[1], app);
}

fn render_filter_bar(f: &mut Frame, area: Rect, app: &App) {
    let label = match app.intercept.match_rule_filter {
        None => "all rules".to_string(),
        Some(rid) => app
            .intercept
            .rules
            .iter()
            .find(|r| r.id == rid)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| format!("rule#{}", rid)),
    };
    let groups = [
        search_bar::pill_spans("rule", &label),
        search_bar::pill_spans(
            "loaded",
            &format!(
                "{}/{}",
                app.intercept.filtered_matches_len(),
                app.intercept.match_total
            ),
        ),
    ];
    search_bar::render(f, area, app, &groups);
}

fn render_list(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        Cell::from("Time"),
        Cell::from("Rule"),
        Cell::from("Agent"),
        Cell::from("Dir"),
        Cell::from("URL"),
        Cell::from("Sum"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));
    let widths = [
        Constraint::Length(11),
        Constraint::Length(14),
        Constraint::Length(10),
        Constraint::Length(4),
        Constraint::Min(14),
        Constraint::Length(4),
    ];

    let filtered = app.intercept.filtered_matches();
    let rows: Vec<Row<'static>> = filtered
        .iter()
        .map(|m| {
            let ts = m
                .match_info
                .matched_at
                .with_timezone(&Local)
                .format("%H:%M:%S%.3f")
                .to_string();
            let sum = summary_glyph(app.intercept.summary_status(m));
            let dir = match m.traffic.direction {
                TrafficDirection::Send => "\u{2191}",
                TrafficDirection::Receive => "\u{2193}",
            };
            let preview = m
                .match_info
                .summary
                .as_deref()
                .map(|s| truncate_first_line(s, 24))
                .unwrap_or_default();
            Row::new(vec![
                Cell::from(Span::styled(ts, Style::default().fg(MUTED))),
                Cell::from(Span::styled(
                    m.match_info.rule_name.clone(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    m.traffic.agent_short_name.clone(),
                    Style::default().fg(TEXT_BRIGHT),
                )),
                Cell::from(Span::styled(dir.to_string(), Style::default().fg(DIM))),
                Cell::from(Span::styled(
                    if preview.is_empty() {
                        truncate(&m.traffic.url, 40)
                    } else {
                        format!("{} — {}", truncate(&m.traffic.url, 28), preview)
                    },
                    Style::default().fg(TEXT_BRIGHT),
                )),
                Cell::from(sum),
            ])
        })
        .collect();

    let block = focused_titled_panel(" Matches ", !app.intercept.match_detail_focus);

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = TableState::default();
    if !filtered.is_empty() {
        state.select(Some(
            app.intercept.match_selected.min(filtered.len() - 1),
        ));
    }
    f.render_stateful_widget(table, area, &mut state);
}

fn summary_glyph(status: SummaryStatus) -> Span<'static> {
    match status {
        SummaryStatus::Ready => Span::styled(
            "\u{2713}",
            Style::default()
                .fg(STATUS_DONE)
                .add_modifier(Modifier::BOLD),
        ),
        SummaryStatus::Pending => Span::styled(
            "\u{25cb}",
            Style::default().fg(STATUS_RUNNING),
        ),
        SummaryStatus::NotConfigured => Span::styled("\u{00b7}", Style::default().fg(DIM)),
    }
}

fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let filtered_len = app.intercept.filtered_matches_len();

    let Some(m) = app
        .intercept
        .filtered_match_at(app.intercept.match_selected)
    else {
        let block = focused_titled_panel(" Match detail ", app.intercept.match_detail_focus);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No match selected.",
                Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
            )))
            .block(block),
            area,
        );
        return;
    };

    let detail = match_detail::build(&app.intercept, m, app.intercept.match_highlight_index);
    let title = if detail.occurrence_count > 0 {
        format!(
            " Match {} / {}  \u{b7}  hit {} / {} ",
            app.intercept.match_selected + 1,
            filtered_len,
            app.intercept.match_highlight_index + 1,
            detail.occurrence_count
        )
    } else {
        format!(
            " Match {} / {} ",
            app.intercept.match_selected + 1,
            filtered_len
        )
    };
    let block = focused_titled_panel(&title, app.intercept.match_detail_focus);

    let inner = block.inner(area);
    //
    // line_count runs ratatui's own word-wrap so this matches the actual
    // render (a plain `.lines.len()` undercounts once any line wraps —
    // see match_detail::rendered_row_offset for why that matters here).
    // Queried before `.block(..)` is attached, so it doesn't double with
    // `inner.height` (which already excludes the block's border rows).
    //
    let para = Paragraph::new(detail.lines).wrap(Wrap { trim: false });
    let total_rows = para.line_count(inner.width) as u16;
    let max_scroll = total_rows.saturating_sub(inner.height);
    app.intercept.match_detail_max_scroll.set(max_scroll);
    app.intercept.match_detail_width.set(inner.width);
    let effective = app.intercept.match_detail_scroll.min(max_scroll);
    let para = para.scroll((effective, 0)).block(block);
    f.render_widget(para, area);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

fn truncate_first_line(s: &str, max: usize) -> String {
    truncate(s.lines().next().unwrap_or(s), max)
}

