mod detail;
mod list;
mod session;
mod sessions_list;
mod terminal;

pub use sessions_list::sessions_list_rect;

use crate::app::App;
use crate::ui::common::table_data_start_margin_header;
use crate::ui::hits::{split_border_rect, HintRegistrar, MouseAction, NodesHintAction, RowSelect, RowSelectKind};
use crate::ui::recon;
use crate::ui::theme::{MUTED, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.nodes;
    let ops = &app.operations.operations;
    let chains = &app.operations.chain_executions;

    if let Some(ref term) = state.terminal {
        terminal::render_terminal(f, area, term);
        return;
    }

    if let Some(ref opts) = state.session_options {
        session::render_session_options(f, area, opts);
        let dir_count = if opts.working_dirs.is_empty() {
            1
        } else {
            1 + opts.working_dirs.len()
        };
        crate::ui::overlay_hits::register_session_options_hits(app, area, dir_count);
        return;
    }

    if let Some(ref recon) = state.recon {
        recon::render_recon(f, area, app, recon);
        return;
    }

    //
    // If a session is foregrounded, draw the chat view. Otherwise fall
    // back to the node browse view. The sessions list overlay is
    // rendered on top of whichever view is active.
    //

    if let Some(session) = state.active_session() {
        session::render_session_chat(f, area, session);
        crate::ui::overlay_hits::register_session_chat_hits(app, area);
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

        register_browse_hits(app, chunks[0], chunks[1], outer[1]);

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
        let mut reg = HintRegistrar::new(app, outer[1]);

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
                reg.chip("\u{21b5}", MouseAction::NodesHint(NodesHintAction::StartSession));
                reg.chip(" session", MouseAction::NodesHint(NodesHintAction::StartSession));
                reg.gap(4);
                hint_spans.push(Span::styled("\u{21B5}", key_style));
                hint_spans.push(Span::styled(" session", label_style));
                hint_spans.push(Span::raw("    "));
            }
            reg.chip("r", MouseAction::NodesHint(NodesHintAction::Recon));
            reg.chip(" recon", MouseAction::NodesHint(NodesHintAction::Recon));
            hint_spans.push(Span::styled("r", key_style));
            hint_spans.push(Span::styled(" recon", label_style));
        } else {
            reg.chip("\u{21b5}", MouseAction::NodesHint(NodesHintAction::SelectDetail));
            reg.chip(" select", MouseAction::NodesHint(NodesHintAction::SelectDetail));
            hint_spans.push(Span::styled("\u{21B5}", key_style));
            hint_spans.push(Span::styled(" select", label_style));
        }

        reg.gap(4);
        reg.chip("^r", MouseAction::NodesHint(NodesHintAction::Reset));
        reg.chip(" reset", MouseAction::NodesHint(NodesHintAction::Reset));
        reg.gap(4);
        reg.chip("^d", MouseAction::NodesHint(NodesHintAction::Remove));
        reg.chip(" remove", MouseAction::NodesHint(NodesHintAction::Remove));
        reg.gap(4);
        reg.chip("^n", MouseAction::NodesHint(NodesHintAction::AddRemote));
        reg.chip(" add remote", MouseAction::NodesHint(NodesHintAction::AddRemote));

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
            reg.gap(4);
            reg.chip("^y", MouseAction::NodesHint(NodesHintAction::Terminal));
            reg.chip(" terminal", MouseAction::NodesHint(NodesHintAction::Terminal));
            hint_spans.push(Span::raw("    "));
            hint_spans.push(Span::styled("^y", key_style));
            hint_spans.push(Span::styled(" terminal", label_style));
        }

        let session_count = state.sessions.len();
        let sessions_label = format!(" sessions ({})", session_count);
        reg.gap(4);
        reg.chip("^w", MouseAction::NodesHint(NodesHintAction::Sessions));
        reg.chip(&sessions_label, MouseAction::NodesHint(NodesHintAction::Sessions));
        hint_spans.push(Span::raw("    "));
        hint_spans.push(Span::styled("^w", key_style));
        hint_spans.push(Span::styled(sessions_label, label_style));
        let hints = Line::from(hint_spans);
        f.render_widget(Paragraph::new(hints), outer[1]);
    }

    if state.sessions_list_open {
        sessions_list::render(f, area, state);
        crate::ui::overlay_hits::register_sessions_list_hits(app, area, state.sessions.len());
    }
}

fn register_browse_hits(app: &App, list_area: Rect, detail_area: Rect, _hints_area: Rect) {
    app.hits_register(
        split_border_rect(list_area),
        MouseAction::NodesSplitDragStart,
    );
    app.hits_register(
        list_area,
        MouseAction::SelectRow(RowSelect {
            kind: RowSelectKind::NodesList,
            table_area: list_area,
            data_start: table_data_start_margin_header(list_area),
        }),
    );
    app.hits_register(detail_area, MouseAction::NodesDetailFocus);
    let agents_start = detail_area.y.saturating_add(1) + 5;
    app.hits_register(
        detail_area,
        MouseAction::NodesAgentRow { agents_start },
    );
}
