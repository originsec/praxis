pub mod chain_form;
pub mod chrome;
pub mod common;
pub mod intercept;
pub mod log_query;
pub mod nodes;
pub mod operations;
pub mod orchestrator;
pub mod popup;
pub mod recon;
pub mod settings;
pub mod status_bar;
pub mod theme;

use crate::app::{App, Window};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use theme::{DIM, TEXT_BRIGHT};

pub use theme::BG;

pub fn render(f: &mut Frame, app: &App) {
    f.render_widget(Block::default().style(Style::default().bg(BG)), f.area());

    let inner = f.area().inner(Margin {
        vertical: 1,
        horizontal: 2,
    });

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    render_header(f, chunks[0], app);

    match app.active_window {
        Window::Orchestrator => orchestrator::render(f, chunks[2], &app.orchestrator),
        Window::Nodes => nodes::render(
            f,
            chunks[2],
            &app.nodes,
            &app.operations.operations,
            &app.operations.chain_executions,
        ),
        Window::Intercept => intercept::render(f, chunks[2], app),
        Window::LogQuery => log_query::render(f, chunks[2], &app.log_query),
        Window::Operations => {
            if let Some(ref form) = app.new_op_form {
                popup::render_new_op_form(f, chunks[2], form);
            } else if let Some(ref opts) = app.run_options {
                popup::render_run_options(f, chunks[2], opts);
            } else if let Some(ref tform) = app.trigger_form {
                popup::render_trigger_form(f, chunks[2], tform);
            } else if let Some(ref cform) = app.chain_form {
                let hit = chain_form::render_chain_form(f, chunks[2], cform);
                *app.chain_form_hits.borrow_mut() = hit;
            } else {
                operations::render(f, chunks[2], &app.operations);
            }
        }
        Window::Settings => settings::render(f, chunks[2], &app.settings),
    }

    status_bar::render(f, chunks[3], app);

    //
    // Render popup overlay on top of everything.
    //
    if let Some(ref p) = app.popup {
        popup::render(f, p);
    }
    if let Some(ref form) = app.add_remote_node_form {
        popup::render_add_remote_node_form(f, f.area(), form);
    }
    if let Some(ref confirm) = app.confirm {
        popup::render_confirm(f, confirm);
    }
}

//
// Top header. Left side: brand sigil + word + version + connection
// dot. Right side: active window crumb. Borrows opencode's "dot in
// success-green when connected" idiom.
//

fn render_header(f: &mut Frame, area: ratatui::layout::Rect, _app: &App) {
    let version = env!("CARGO_PKG_VERSION");

    let right = Line::from(vec![
        Span::styled(
            "praxis",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(format!("v{}", version), Style::default().fg(DIM)),
    ])
    .alignment(Alignment::Right);

    f.render_widget(Paragraph::new(right), area);
}
