//
// Unified search filter bar used on Traffic, Rules, and Matches tabs.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::intercept::InterceptTab;
use crate::app::App;
use crate::ui::chrome;
use crate::ui::theme::{ACCENT, DIM, MUTED, TEXT_BRIGHT};

pub fn render(f: &mut Frame, area: Rect, app: &App, extra_groups: &[Vec<Span<'static>>]) {
    let state = &app.intercept;
    let search_span = if state.search_focused {
        if state.search_input.is_empty() {
            Span::styled("\u{2588}", Style::default().fg(ACCENT))
        } else {
            Span::styled(
                format!("{}\u{2588}", state.search_input),
                Style::default().fg(ACCENT),
            )
        }
    } else if state.search_input.is_empty() {
        let hint = match state.tab {
            InterceptTab::Traffic => "/ search  ^\u{21b5} server",
            InterceptTab::Rules => "/ search rules",
            InterceptTab::Matches => "/ search matches",
        };
        Span::styled(hint, Style::default().fg(DIM).add_modifier(Modifier::ITALIC))
    } else {
        Span::styled(state.search_input.clone(), Style::default().fg(ACCENT))
    };

    let mut spans = vec![
        Span::styled("/", Style::default().fg(TEXT_BRIGHT)),
        Span::raw(" "),
        search_span,
    ];

    for group in extra_groups {
        spans.push(Span::raw("    "));
        spans.extend(group.iter().cloned());
    }

    if !state.search_input.is_empty() && !state.search_focused {
        spans.push(Span::raw("    "));
        spans.push(Span::styled("esc", Style::default().fg(MUTED)));
        spans.push(Span::styled(" clear", Style::default().fg(DIM)));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn pill_spans(label: &str, value: &str) -> Vec<Span<'static>> {
    chrome::pill_two_tone(label, value, ACCENT)
}