mod detail;
mod list;
mod session;
mod sessions_list;
mod terminal;

pub use sessions_list::sessions_list_rect;

use crate::app::NodesState;
use crate::ui::recon;
use crate::ui::theme::{MUTED, TEXT_BRIGHT};
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

    if let Some(ref recon) = state.recon {
        recon::render_recon(f, area, recon);
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

        //
        // Default split: detail pane fixed at 30 cols, node list fills
        // the rest. If the user has resized the split, honour their
        // pick (state.split_percent != 0).
        //
        let chunks = if state.split_percent_user_set {
            Layout::horizontal([
                Constraint::Percentage(state.split_percent),
                Constraint::Percentage(100 - state.split_percent),
            ])
            .split(outer[0])
        } else {
            Layout::horizontal([Constraint::Min(20), Constraint::Length(30)]).split(outer[0])
        };

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

        let key_style = Style::default().fg(TEXT_BRIGHT);
        let label_style = Style::default().fg(MUTED);
        let mut hint_spans: Vec<Span> = Vec::new();

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
                hint_spans.push(Span::styled("\u{21B5}", key_style));
                hint_spans.push(Span::styled(" session", label_style));
                hint_spans.push(Span::raw("    "));
            }
            hint_spans.push(Span::styled("r", key_style));
            hint_spans.push(Span::styled(" recon", label_style));
        } else {
            hint_spans.push(Span::styled("\u{21B5}", key_style));
            hint_spans.push(Span::styled(" select", label_style));
        }

        hint_spans.push(Span::raw("    "));
        hint_spans.push(Span::styled("^r", key_style));
        hint_spans.push(Span::styled(" reset", label_style));
        hint_spans.push(Span::raw("    "));
        hint_spans.push(Span::styled("^d", key_style));
        hint_spans.push(Span::styled(" remove", label_style));
        hint_spans.push(Span::raw("    "));
        hint_spans.push(Span::styled("^n", key_style));
        hint_spans.push(Span::styled(" add remote", label_style));

        if has_terminal {
            hint_spans.push(Span::raw("    "));
            hint_spans.push(Span::styled("^t", key_style));
            hint_spans.push(Span::styled(" terminal", label_style));
        }

        let session_count = state.sessions.len();
        hint_spans.push(Span::raw("    "));
        hint_spans.push(Span::styled("^w", key_style));
        hint_spans.push(Span::styled(
            format!(" sessions ({})", session_count),
            label_style,
        ));
        let hints = Line::from(hint_spans);
        f.render_widget(Paragraph::new(hints), outer[1]);
    }

    if state.sessions_list_open {
        sessions_list::render(f, area, state);
    }
}
