mod detail;
mod list;
mod session;
mod sessions_list;
mod terminal;

pub use sessions_list::sessions_list_rect;

use crate::app::NodesState;
use crate::ui::theme::{ACCENT, MUTED};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(
    f: &mut Frame,
    area: Rect,
    state: &NodesState,
    ops: &[common::SemanticOpUpdate],
    chains: &[common::ChainExecutionUpdate],
) {
    if let Some(ref term) = state.terminal {
        terminal::render_terminal(f, area, term);
        return;
    }

    if let Some(ref opts) = state.session_options {
        session::render_session_options(f, area, opts);
        return;
    }

    //
    // If a session is foregrounded, draw the chat view. Otherwise fall
    // back to the node browse view. The sessions list overlay is
    // rendered on top of whichever view is active.
    //

    if let Some(session) = state.active_session() {
        session::render_session_chat(f, area, session);
    } else {
        let outer = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);

        let chunks = Layout::horizontal([
            Constraint::Percentage(state.split_percent),
            Constraint::Percentage(100 - state.split_percent),
        ])
        .split(outer[0]);

        list::render_node_list(f, chunks[0], state);
        detail::render_node_detail(f, chunks[1], state, ops, chains);

        let has_terminal = state
            .nodes
            .get(state.selected)
            .map(|n| {
                n.capabilities.is_empty()
                    || n.capabilities.contains(&common::NodeCapability::Terminal)
            })
            .unwrap_or(false);

        let mut hint_spans = vec![Span::raw(" ")];

        if state.detail_focus {
            let has_session = state
                .nodes
                .get(state.selected)
                .map(|n| {
                    n.capabilities.is_empty()
                        || n.capabilities.contains(&common::NodeCapability::Session)
                })
                .unwrap_or(false);
            if has_session {
                hint_spans.push(Span::styled("enter", Style::default().fg(ACCENT)));
                hint_spans.push(Span::styled(" session  ", Style::default().fg(MUTED)));
            }
        } else {
            hint_spans.push(Span::styled("enter", Style::default().fg(ACCENT)));
            hint_spans.push(Span::styled(" select  ", Style::default().fg(MUTED)));
        }

        hint_spans.push(Span::styled("^r", Style::default().fg(ACCENT)));
        hint_spans.push(Span::styled(" reset", Style::default().fg(MUTED)));

        if has_terminal {
            hint_spans.push(Span::styled("  ^t", Style::default().fg(ACCENT)));
            hint_spans.push(Span::styled(" terminal", Style::default().fg(MUTED)));
        }

        let session_count = state.sessions.len();
        hint_spans.push(Span::styled("  ^w", Style::default().fg(ACCENT)));
        hint_spans.push(Span::styled(
            format!(" sessions({})", session_count),
            Style::default().fg(MUTED),
        ));
        let hints = Line::from(hint_spans);
        f.render_widget(Paragraph::new(hints), outer[1]);
    }

    if state.sessions_list_open {
        sessions_list::render(f, area, state);
    }
}
