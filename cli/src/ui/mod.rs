pub mod common;
pub mod intercept;
pub mod log_query;
pub mod nodes;
pub mod operations;
pub mod orchestrator;
pub mod popup;
pub mod settings;
pub mod status_bar;
pub mod theme;

use crate::app::{App, Window};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use theme::{ACCENT, DIM};

pub use theme::BG;

pub fn render(f: &mut Frame, app: &App) {
    f.render_widget(Block::default().style(Style::default().bg(BG)), f.area());

    let inner = f.area().inner(Margin {
        vertical: 1,
        horizontal: 2,
    });

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    render_header(f, chunks[0]);

    match app.active_window {
        Window::Orchestrator => orchestrator::render(f, chunks[1], &app.orchestrator),
        Window::Nodes => nodes::render(
            f,
            chunks[1],
            &app.nodes,
            &app.operations.operations,
            &app.operations.chain_executions,
        ),
        Window::Intercept => intercept::render(f, chunks[1], app),
        Window::LogQuery => log_query::render(f, chunks[1], &app.log_query),
        Window::Operations => {
            if let Some(ref form) = app.new_op_form {
                popup::render_new_op_form(f, chunks[1], form);
            } else if let Some(ref opts) = app.run_options {
                popup::render_run_options(f, chunks[1], opts);
            } else if let Some(ref tform) = app.trigger_form {
                popup::render_trigger_form(f, chunks[1], tform);
            } else {
                operations::render(f, chunks[1], &app.operations);
            }
        }
        Window::Settings => settings::render(f, chunks[1], &app.settings),
    }

    status_bar::render(f, chunks[2], app);

    //
    // Render popup overlay on top of everything.
    //
    if let Some(ref p) = app.popup {
        popup::render(f, p);
    }
    if let Some(ref confirm) = app.confirm {
        popup::render_confirm(f, confirm);
    }
    if let Some(ref picker) = app.intercept_method_picker {
        popup::render_intercept_method_picker(f, picker);
    }
}

fn render_header(f: &mut Frame, area: ratatui::layout::Rect) {
    let version = env!("CARGO_PKG_VERSION");

    let line = Line::from(vec![
        Span::styled("[\u{00d8}]", Style::default().fg(ACCENT)),
        Span::styled(
            " PRAXIS ",
            Style::default()
                .fg(Color::Rgb(200, 200, 200))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  v{} ", version), Style::default().fg(DIM)),
    ]);

    let paragraph = Paragraph::new(line).alignment(Alignment::Right);
    f.render_widget(paragraph, area);
}
