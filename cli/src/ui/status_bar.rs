use crate::app::{App, Window};
use crate::keymap::global;
use crate::ui::chrome;
use crate::ui::hits::MouseAction;
use crate::ui::theme::{ACCENT, BG, DIM, MUTED, OK, STATUS_FAIL, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

//
// One run of adjacent spans in the left half of the bar; `action` is set for
// the runs that are clickable. Rendering and hit registration both walk the
// same segment list, so a click target is always derived from the width of
// the text actually drawn under it.
//

struct Segment {
    spans: Vec<Span<'static>>,
    action: Option<MouseAction>,
}

impl Segment {
    fn text(spans: Vec<Span<'static>>) -> Self {
        Self {
            spans,
            action: None,
        }
    }

    fn button(spans: Vec<Span<'static>>, action: MouseAction) -> Self {
        Self {
            spans,
            action: Some(action),
        }
    }

    fn width(&self) -> u16 {
        self.spans
            .iter()
            .map(|s| s.content.chars().count() as u16)
            .sum()
    }
}

const NAV_PAIRS: &[(&str, &str, Window)] = &[
    (global::ORCHESTRATOR, "orchestrator", Window::Orchestrator),
    (global::NODES, "nodes", Window::Nodes),
    (global::OPERATIONS, "ops", Window::Operations),
    (global::INTERCEPT, "intercept", Window::Intercept),
    (global::LOG_QUERY, "logs", Window::LogQuery),
    (global::SETTINGS, "settings", Window::Settings),
];

fn nav_label(key: &str, label: &str, active: bool) -> Vec<Span<'static>> {
    let key_style = if active {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let label_style = if active {
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };
    vec![
        Span::styled(key.to_string(), key_style),
        Span::styled(format!(" {}", label), label_style),
    ]
}

fn left_segments(
    connected: bool,
    node_count: usize,
    session_count: usize,
    active_window: Window,
) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();

    let conn_color = if connected { OK } else { STATUS_FAIL };
    let nodes_label = if node_count == 1 {
        "1 node".to_string()
    } else {
        format!("{} nodes", node_count)
    };
    segments.push(Segment::text(vec![
        Span::styled("\u{2022} ", Style::default().fg(conn_color)),
        Span::styled(nodes_label, Style::default().fg(MUTED)),
    ]));

    if session_count > 0 {
        segments.push(Segment::text(vec![
            chrome::mid_dot(),
            Span::styled(
                format!("{} sessions", session_count),
                Style::default().fg(ACCENT),
            ),
        ]));
    }

    segments.push(Segment::text(vec![Span::raw("    ")]));

    for (i, (key, label, window)) in NAV_PAIRS.iter().enumerate() {
        if i > 0 {
            segments.push(Segment::text(vec![Span::raw("  ")]));
        }
        segments.push(Segment::button(
            nav_label(key, label, active_window == *window),
            MouseAction::SwitchWindow(*window),
        ));
    }

    segments.push(Segment::text(vec![chrome::mid_dot()]));
    segments.push(Segment::button(
        chrome::dim_hint(global::HELP, "help"),
        MouseAction::OpenHelp,
    ));
    segments.push(Segment::text(vec![chrome::mid_dot()]));
    segments.push(Segment::button(
        chrome::dim_hint(global::QUIT, "quit"),
        MouseAction::Quit,
    ));

    segments
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let right = Line::from(vec![
        if app.connected {
            Span::styled(
                "connected",
                Style::default().fg(OK).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                "disconnected",
                Style::default()
                    .fg(STATUS_FAIL)
                    .add_modifier(Modifier::BOLD),
            )
        },
        Span::raw(" "),
    ]);

    let chunks = Layout::horizontal([Constraint::Min(1), Constraint::Length(right.width() as u16)])
        .split(area);

    let segments = left_segments(
        app.connected,
        app.nodes.nodes.len(),
        app.nodes.sessions.len(),
        app.active_window,
    );

    //
    // Walk the segments left to right, registering a hit rect for each
    // clickable run clipped to the visible area so a narrow terminal cannot
    // leave live targets over text it never drew.
    //

    let mut left: Vec<Span> = Vec::new();
    let mut col = 0u16;
    let end_x = chunks[0].x.saturating_add(chunks[0].width);
    for segment in segments {
        let width = segment.width();
        let Segment { spans, action } = segment;
        if let Some(action) = action {
            let x = chunks[0].x.saturating_add(col);
            if x < end_x {
                app.hits_register(Rect::new(x, chunks[0].y, width.min(end_x - x), 1), action);
            }
        }
        col = col.saturating_add(width);
        left.extend(spans);
    }

    let left_bar = Paragraph::new(Line::from(left)).style(Style::default().bg(BG));
    let right_bar = Paragraph::new(right).style(Style::default().bg(BG));

    f.render_widget(left_bar, chunks[0]);
    f.render_widget(right_bar, chunks[1]);
}

#[cfg(test)]
mod tests {
    use super::*;

    //
    // Every clickable segment must sit exactly on top of its own text. The
    // regression this guards: hit rects used to be computed by a second pass
    // that re-derived the layout and skipped the "^h help" hint entirely, so
    // the quit rect landed on the help text and clicking help quit the app.
    //

    fn clickable_text(segments: &[Segment]) -> Vec<(String, &MouseAction)> {
        let rendered: Vec<char> = segments
            .iter()
            .flat_map(|s| s.spans.iter())
            .flat_map(|s| s.content.chars())
            .collect();

        let mut col = 0usize;
        let mut clickable = Vec::new();
        for segment in segments {
            let width = segment.width() as usize;
            if let Some(action) = &segment.action {
                clickable.push((rendered[col..col + width].iter().collect(), action));
            }
            col += width;
        }
        clickable
    }

    fn assert_layout(connected: bool, node_count: usize, session_count: usize, active: Window) {
        let segments = left_segments(connected, node_count, session_count, active);
        let clickable = clickable_text(&segments);

        let expected = [
            "^o orchestrator",
            "^l nodes",
            "^p ops",
            "^t intercept",
            "^g logs",
            "^s settings",
            "^h help",
            "^q quit",
        ];
        let actual: Vec<&str> = clickable.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(actual, expected);

        assert!(matches!(clickable[6].1, MouseAction::OpenHelp));
        assert!(matches!(clickable[7].1, MouseAction::Quit));
    }

    #[test]
    fn help_and_quit_hits_cover_their_own_text() {
        assert_layout(true, 3, 2, Window::Nodes);
    }

    #[test]
    fn layout_holds_for_variable_width_prefixes() {
        //
        // The node and session counters change width, which is what used to
        // make the hand-rolled column arithmetic drift.
        //

        assert_layout(true, 1, 0, Window::Orchestrator);
        assert_layout(false, 0, 0, Window::Settings);
        assert_layout(true, 128, 4096, Window::LogQuery);
    }
}
