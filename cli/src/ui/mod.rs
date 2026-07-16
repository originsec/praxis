pub mod chain_form;
pub mod chrome;
pub mod common;
pub mod filter_bar;
pub mod form_modal;
pub mod help;
pub mod hint_row;
pub mod hits;
pub mod intercept;
pub mod list_detail;
pub mod log_query;
pub mod nodes;
pub mod operations;
pub mod orchestrator;
pub mod overlay_hits;
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
    app.hits_clear();

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
        Window::Orchestrator => orchestrator::render(f, chunks[2], app),
        Window::Nodes => nodes::render(f, chunks[2], app),
        Window::Intercept => intercept::render(f, chunks[2], app),
        Window::LogQuery => log_query::render(f, chunks[2], app),
        Window::Operations => {
            if let Some(ref opts) = app.run_options {
                popup::render_run_options(f, chunks[2], opts);
                overlay_hits::register_run_options_hits(app, chunks[2], opts);
            } else if let Some(ref tform) = app.trigger_form {
                popup::render_trigger_form(f, chunks[2], tform);
                overlay_hits::register_trigger_form_hits(app, chunks[2], tform);
            } else if let Some(ref cform) = app.chain_form {
                let hit = chain_form::render_chain_form(f, chunks[2], cform);
                *app.chain_form_hits.borrow_mut() = hit;
                if let Some(ref editor) = cform.editor {
                    overlay_hits::register_chain_editor_hits(app, chunks[2], cform, editor);
                } else {
                    let hit = app.chain_form_hits.borrow().clone();
                    overlay_hits::register_chain_form_hits(app, &hit);
                }
            } else {
                //
                // New-op form is a centered modal over the ops list
                // (same chrome as settings / intercept rule forms).
                //
                operations::render(f, chunks[2], app);
                if let Some(ref form) = app.new_op_form {
                    operations::new_op_form::render(f, chunks[2], form);
                    operations::new_op_form::register_hits(app, chunks[2], form);
                }
            }
        }
        Window::Settings => settings::render(f, chunks[2], app),
    }

    status_bar::render(f, chunks[3], app);

    let terminal = f.area();

    if let Some(ref p) = app.popup {
        popup::render(f, p);
        overlay_hits::register_popup_hits(app, terminal, p);
    }
    if let Some(ref form) = app.add_remote_node_form {
        popup::render_add_remote_node_form(f, terminal, form);
        overlay_hits::register_add_remote_hits(app, terminal, form);
    }
    if let Some(ref confirm) = app.confirm {
        popup::render_confirm(f, confirm);
        overlay_hits::register_confirm_hits(app, terminal, confirm);
    }

    //
    // Documentation-helper overlay renders on top of everything, including
    // popups, since it is summonable from any window.
    //
    if app.help.open {
        help::render(f, &app.help);
    }
}

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
