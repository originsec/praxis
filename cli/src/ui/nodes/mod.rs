mod detail;
mod list;
mod session;
mod sessions_list;
mod terminal;

pub use sessions_list::sessions_list_rect;

use crate::app::{App, NodesState};
use crate::ui::recon;
use crate::ui::theme::{MUTED, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub enum NodesHintAction {
    SelectDetail,
    StartSession,
    Recon,
    Reset,
    Remove,
    AddRemote,
    Terminal,
    Sessions,
}

fn push_region(
    regions: &mut Vec<(u16, u16, NodesHintAction)>,
    x: &mut u16,
    text: &str,
    action: NodesHintAction,
) {
    let w = text.chars().count() as u16;
    regions.push((*x, *x + w, action));
    *x += w;
}

/// Hit-test the nodes hint bar. `base_x` is the hint row's x coordinate.
pub fn hint_action_at(app: &App, base_x: u16, col: u16) -> Option<NodesHintAction> {
    let rel = col.saturating_sub(base_x);
    let mut regions: Vec<(u16, u16, NodesHintAction)> = Vec::new();
    let mut x = 0u16;

    if app.nodes.detail_focus {
        let has_session = app
            .nodes
            .nodes
            .get(app.nodes.selected)
            .map(|n| {
                n.capabilities.is_empty()
                    || n.capabilities.contains(&common::NodeCapability::Session)
            })
            .unwrap_or(false);
        if has_session {
            push_region(&mut regions, &mut x, "\u{21b5}", NodesHintAction::StartSession);
            push_region(&mut regions, &mut x, " session", NodesHintAction::StartSession);
            x += 4;
        }
        push_region(&mut regions, &mut x, "r", NodesHintAction::Recon);
        push_region(&mut regions, &mut x, " recon", NodesHintAction::Recon);
    } else {
        push_region(&mut regions, &mut x, "\u{21b5}", NodesHintAction::SelectDetail);
        push_region(&mut regions, &mut x, " select", NodesHintAction::SelectDetail);
    }

    x += 4;
    push_region(&mut regions, &mut x, "^r", NodesHintAction::Reset);
    push_region(&mut regions, &mut x, " reset", NodesHintAction::Reset);
    x += 4;
    push_region(&mut regions, &mut x, "^d", NodesHintAction::Remove);
    push_region(&mut regions, &mut x, " remove", NodesHintAction::Remove);
    x += 4;
    push_region(&mut regions, &mut x, "^n", NodesHintAction::AddRemote);
    push_region(&mut regions, &mut x, " add remote", NodesHintAction::AddRemote);

    let has_terminal = app
        .nodes
        .nodes
        .get(app.nodes.selected)
        .map(|n| {
            n.capabilities.is_empty()
                || n.capabilities.contains(&common::NodeCapability::Terminal)
        })
        .unwrap_or(false);
    if has_terminal {
        x += 4;
        push_region(&mut regions, &mut x, "^t", NodesHintAction::Terminal);
        push_region(&mut regions, &mut x, " terminal", NodesHintAction::Terminal);
    }

    let session_count = app.nodes.sessions.len();
    x += 4;
    push_region(&mut regions, &mut x, "^w", NodesHintAction::Sessions);
    push_region(
        &mut regions,
        &mut x,
        &format!(" sessions ({})", session_count),
        NodesHintAction::Sessions,
    );

    for (start, end, action) in regions {
        if rel >= start && rel < end {
            return Some(action);
        }
    }
    None
}

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
