//
// Matches tab detail-pane content: the same field set as service
// matching (url, host, method, status, headers, bodies), plus a live
// regex-match highlight overlay so the operator can see exactly where
// the rule's pattern hit and jump between occurrences with n/p.
//

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use regex::Regex;

use common::TrafficMatchWithDetails;

use super::{InterceptState, SummaryStatus};
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MATCH_HIGHLIGHT, MUTED, STATUS_RUNNING, TEXT, TEXT_BRIGHT,
};

pub struct MatchDetail {
    pub lines: Vec<Line<'static>>,
    /// Total regex-match occurrences found across the rendered content.
    pub occurrence_count: usize,
    /// Line index of the current (bright) occurrence, if any.
    pub current_line: Option<usize>,
}

pub fn build(state: &InterceptState, m: &TrafficMatchWithDetails, highlight_index: usize) -> MatchDetail {
    let lines = content_lines(state, m);

    let regex = state
        .rules
        .iter()
        .find(|r| r.id == m.match_info.rule_id)
        .and_then(|r| Regex::new(&r.regex_pattern).ok());

    let Some(regex) = regex else {
        return MatchDetail {
            lines,
            occurrence_count: 0,
            current_line: None,
        };
    };

    overlay_highlights(lines, &regex, highlight_index)
}

//
// `Paragraph::scroll` operates in rendered (post-wrap) row units once
// `Wrap` is set, not logical-`Line` units — a single long line (e.g. a
// JSON string value packed with escaped `\n`s) can word-wrap into dozens
// of rows. Ratatui's wrapper treats each logical line as a hard boundary
// (it never merges trailing/leading content across lines, always
// emitting at least one row per line — see ratatui-widgets' reflow.rs),
// so rendered-row counts are prefix-additive: the row where logical line
// `target_line` begins equals the rendered row *count* of everything
// before it. Building a throwaway Paragraph from just that prefix and
// asking its `line_count` (the `unstable-rendered-line-info` feature,
// already enabled in Cargo.toml) gives that count using ratatui's own
// wrapping algorithm — no need to reimplement word-wrap ourselves.
//
// The `Wrap` here must match the real detail-pane Paragraph's wrap
// setting exactly (see `render_detail` in ui/intercept/matches.rs) or
// the row math diverges from what's actually on screen.
//
pub fn rendered_row_offset(lines: &[Line<'static>], target_line: usize, width: u16) -> u16 {
    let prefix: Vec<Line<'static>> = lines[..target_line.min(lines.len())].to_vec();
    Paragraph::new(prefix)
        .wrap(Wrap { trim: false })
        .line_count(width) as u16
}

fn content_lines(state: &InterceptState, m: &TrafficMatchWithDetails) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::from(vec![
        Span::styled("rule: ", Style::default().fg(MUTED)),
        Span::styled(
            m.match_info.rule_name.clone(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    ]));
    out.push(Line::from(vec![
        Span::styled("url:  ", Style::default().fg(MUTED)),
        Span::styled(m.traffic.url.clone(), Style::default().fg(TEXT_BRIGHT)),
    ]));
    out.push(Line::from(vec![
        Span::styled("host: ", Style::default().fg(MUTED)),
        Span::styled(m.traffic.host.clone(), Style::default().fg(TEXT_BRIGHT)),
    ]));
    out.push(Line::from(vec![
        Span::styled("agent:", Style::default().fg(MUTED)),
        Span::styled(
            format!(" {} ", m.traffic.agent_short_name),
            Style::default().fg(TEXT_BRIGHT),
        ),
        Span::styled("dir:", Style::default().fg(MUTED)),
        Span::styled(
            format!(" {:?} ", m.traffic.direction).to_lowercase(),
            Style::default().fg(DIM),
        ),
        Span::styled("method:", Style::default().fg(MUTED)),
        Span::styled(
            format!(" {}", m.traffic.method.as_deref().unwrap_or("-")),
            Style::default().fg(DIM),
        ),
    ]));
    if let Some(s) = m.traffic.response_status {
        out.push(Line::from(vec![
            Span::styled("stat: ", Style::default().fg(MUTED)),
            Span::styled(s.to_string(), Style::default().fg(TEXT_BRIGHT)),
        ]));
    }
    out.push(Line::from(Span::styled(
        "  (o) open in Traffic tab",
        Style::default().fg(DIM),
    )));
    out.push(Line::raw(""));

    match state.summary_status(m) {
        SummaryStatus::Ready => {
            if let Some(ref summary) = m.match_info.summary {
                out.push(Line::from(Span::styled(
                    "AI SUMMARY",
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                )));
                for line in summary.lines() {
                    out.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(TEXT),
                    )));
                }
                out.push(Line::raw(""));
            }
        }
        SummaryStatus::Pending => {
            out.push(Line::from(Span::styled(
                "AI SUMMARY (generating…)",
                Style::default().fg(STATUS_RUNNING),
            )));
            out.push(Line::raw(""));
        }
        SummaryStatus::NotConfigured => {
            out.push(Line::from(Span::styled(
                "(no summarization configured for this rule)",
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )));
            out.push(Line::raw(""));
        }
    }

    push_headers(&mut out, "REQUEST HEADERS", m.traffic.request_headers.as_ref());
    if let Some(body) = state.request_body_for(&m.traffic) {
        out.push(Line::from(Span::styled(
            format!(
                "REQUEST BODY ({} bytes, {})",
                body.len(),
                state.body_mode.label()
            ),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        out.extend(super::body::render_body(body, state.body_mode));
        out.push(Line::raw(""));
    } else if m.traffic.id.is_some() && state.body_needs_fetch(&m.traffic) {
        out.push(Line::from(Span::styled(
            "REQUEST BODY (fetching…)",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
        out.push(Line::raw(""));
    }

    push_headers(&mut out, "RESPONSE HEADERS", m.traffic.response_headers.as_ref());
    if let Some(body) = state.response_body_for(&m.traffic) {
        out.push(Line::from(Span::styled(
            format!(
                "RESPONSE BODY ({} bytes, {})",
                body.len(),
                state.body_mode.label()
            ),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        out.extend(super::body::render_body(body, state.body_mode));
    } else if m.traffic.id.is_some() && state.body_needs_fetch(&m.traffic) {
        out.push(Line::from(Span::styled(
            "RESPONSE BODY (fetching…)",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
    }
    out
}

fn push_headers(
    out: &mut Vec<Line<'static>>,
    heading: &str,
    headers: Option<&indexmap::IndexMap<String, String>>,
) {
    let Some(headers) = headers else {
        return;
    };
    if headers.is_empty() {
        return;
    }
    out.push(Line::from(Span::styled(
        heading.to_string(),
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )));
    for (k, v) in headers {
        out.push(Line::from(vec![
            Span::styled(format!("{}: ", k), Style::default().fg(MUTED)),
            Span::styled(v.clone(), Style::default().fg(TEXT)),
        ]));
    }
    out.push(Line::raw(""));
}

//
// Overlays regex-match highlighting onto already-styled detail lines:
// every occurrence gets a faded tag, the `highlight_index`-th one
// (0-based, in on-screen reading order) gets the bright/current style.
// Runs against the rendered plain text (post JSON pretty-print/hex/raw
// formatting) rather than raw field bytes, so offsets always line up
// with what's actually on screen regardless of body_mode.
//
fn overlay_highlights(lines: Vec<Line<'static>>, regex: &Regex, highlight_index: usize) -> MatchDetail {
    let mut occurrence_count = 0usize;
    let mut current_line = None;
    let mut out = Vec::with_capacity(lines.len());

    for (line_idx, line) in lines.into_iter().enumerate() {
        let plain: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let hits: Vec<(usize, usize)> = regex
            .find_iter(&plain)
            .map(|h| (h.start(), h.end()))
            .filter(|(s, e)| s < e)
            .collect();
        if hits.is_empty() {
            out.push(line);
            continue;
        }

        //
        // Rebuild spans at the union of the original span boundaries and
        // the hit boundaries, so untouched text keeps its original style
        // (JSON colouring, etc.) and each hit gets its own override —
        // even when a hit straddles more than one original span.
        //
        let runs: Vec<(usize, usize, Style)> = {
            let mut pos = 0;
            line.spans
                .iter()
                .map(|s| {
                    let start = pos;
                    pos += s.content.len();
                    (start, pos, s.style)
                })
                .collect()
        };

        let mut cuts: Vec<usize> = vec![0, plain.len()];
        cuts.extend(runs.iter().flat_map(|r| [r.0, r.1]));
        cuts.extend(hits.iter().flat_map(|h| [h.0, h.1]));
        cuts.sort_unstable();
        cuts.dedup();

        let mut spans = Vec::new();
        for w in cuts.windows(2) {
            let (s, e) = (w[0], w[1]);
            if s >= e {
                continue;
            }
            let text = plain[s..e].to_string();
            match hits.iter().position(|(hs, he)| *hs <= s && e <= *he) {
                Some(idx) => {
                    let global_idx = occurrence_count + idx;
                    let style = if global_idx == highlight_index {
                        current_line = Some(line_idx);
                        Style::default()
                            .bg(MATCH_HIGHLIGHT)
                            .fg(TEXT_BRIGHT)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().bg(BG_SELECTED).fg(MATCH_HIGHLIGHT)
                    };
                    spans.push(Span::styled(text, style));
                }
                None => {
                    let base = runs
                        .iter()
                        .find(|r| r.0 <= s && e <= r.1)
                        .map(|r| r.2)
                        .unwrap_or_default();
                    spans.push(Span::styled(text, base));
                }
            }
        }
        out.push(Line::from(spans));
        occurrence_count += hits.len();
    }

    MatchDetail {
        lines: out,
        occurrence_count,
        current_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use common::{
        InterceptMethod, InterceptRule, InterceptedTrafficEntry, RuleScope, TargetDirection,
        TrafficDirection, TrafficMatch,
    };

    fn sample_rule(id: i64, pattern: &str) -> InterceptRule {
        let now = Utc::now();
        InterceptRule {
            id,
            name: "test-rule".into(),
            regex_pattern: pattern.into(),
            target_direction: TargetDirection::Both,
            scope: RuleScope::All,
            enabled: true,
            summarization_prompt: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn sample_match(rule_id: i64, entry: InterceptedTrafficEntry) -> TrafficMatchWithDetails {
        TrafficMatchWithDetails {
            match_info: TrafficMatch {
                id: 1,
                traffic_id: entry.id.unwrap_or(1),
                rule_id,
                rule_name: "test-rule".into(),
                matched_at: Utc::now(),
                summary: None,
            },
            traffic: entry,
        }
    }

    fn base_entry() -> InterceptedTrafficEntry {
        InterceptedTrafficEntry {
            id: Some(1),
            timestamp: Utc::now(),
            node_id: "n1".into(),
            agent_short_name: "claude".into(),
            intercept_method: InterceptMethod::Proxy,
            direction: TrafficDirection::Send,
            method: Some("POST".into()),
            url: "https://api.example.com/v1/messages".into(),
            host: "api.example.com".into(),
            request_headers: None,
            request_body: None,
            response_status: None,
            response_headers: None,
            response_body: None,
        }
    }

    fn state_with_rule(rule: InterceptRule) -> InterceptState {
        let mut state = InterceptState::default();
        state.rules = vec![rule];
        state
    }

    fn bright_style() -> Style {
        Style::default()
            .bg(MATCH_HIGHLIGHT)
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD)
    }

    fn faded_style() -> Style {
        Style::default().bg(BG_SELECTED).fg(MATCH_HIGHLIGHT)
    }

    fn styled_spans(lines: &[Line<'static>]) -> Vec<(String, Style)> {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| (s.content.to_string(), s.style))
            .collect()
    }

    #[test]
    fn single_occurrence_in_url_is_highlighted() {
        let state = state_with_rule(sample_rule(1, "secret"));
        let mut entry = base_entry();
        entry.url = "https://api.example.com/secret/path".into();
        let m = sample_match(1, entry);

        let detail = build(&state, &m, 0);
        assert_eq!(detail.occurrence_count, 1);
        assert!(detail.current_line.is_some());

        let bright: Vec<_> = styled_spans(&detail.lines)
            .into_iter()
            .filter(|(_, s)| *s == bright_style())
            .collect();
        assert_eq!(bright.len(), 1);
        assert_eq!(bright[0].0, "secret");
    }

    #[test]
    fn multiple_occurrences_current_is_bright_others_faded() {
        let state = state_with_rule(sample_rule(1, "hit"));
        let mut entry = base_entry();
        entry.url = "https://example.com/hit-one".into();
        entry.request_body = Some(br#"{"note":"hit-two and hit-three"}"#.to_vec());
        let m = sample_match(1, entry);

        //
        // Three occurrences total: one in the url line, two inside the
        // pretty-printed JSON body string value.
        //
        let detail = build(&state, &m, 1);
        assert_eq!(detail.occurrence_count, 3);

        let bright: Vec<_> = styled_spans(&detail.lines)
            .into_iter()
            .filter(|(_, s)| *s == bright_style())
            .collect();
        assert_eq!(bright.len(), 1, "exactly one occurrence must be bright");

        let faded: Vec<_> = styled_spans(&detail.lines)
            .into_iter()
            .filter(|(_, s)| *s == faded_style())
            .collect();
        assert_eq!(faded.len(), 2, "the other two occurrences must be faded");
    }

    #[test]
    fn hit_spanning_json_span_boundary_counts_as_one_occurrence() {
        //
        // Pattern straddles the closing quote + colon of a JSON key, which
        // highlight_json_line renders as separate spans (key, then punct) —
        // exercises the multi-run cut/merge path in overlay_highlights.
        //
        let state = state_with_rule(sample_rule(1, r#""note":"#));
        let mut entry = base_entry();
        entry.request_body = Some(br#"{"note": "value"}"#.to_vec());
        let m = sample_match(1, entry);

        let detail = build(&state, &m, 0);
        assert_eq!(
            detail.occurrence_count, 1,
            "a hit crossing multiple JSON-coloured spans must still count once"
        );
    }

    #[test]
    fn request_headers_are_scanned_for_highlights() {
        let state = state_with_rule(sample_rule(1, "sk-secret"));
        let mut entry = base_entry();
        let mut headers = indexmap::IndexMap::new();
        headers.insert("X-Api-Key".to_string(), "sk-secret-token".to_string());
        entry.request_headers = Some(headers);
        let m = sample_match(1, entry);

        let detail = build(&state, &m, 0);
        assert_eq!(detail.occurrence_count, 1);
        let bright: Vec<_> = styled_spans(&detail.lines)
            .into_iter()
            .filter(|(_, s)| *s == bright_style())
            .collect();
        assert_eq!(bright[0].0, "sk-secret");
    }

    #[test]
    fn rendered_row_offset_accounts_for_wrapped_lines() {
        //
        // Line 1 is a single 100-char unbreakable token — at a narrow
        // width it must word-wrap into multiple rendered rows. If
        // rendered_row_offset() still counted logical lines 1:1 (the bug
        // this exists to fix), the offset for line 2 would just be 2.
        //
        let lines: Vec<Line<'static>> = vec![
            Line::raw("short"),
            Line::raw("a".repeat(100)),
            Line::raw("also short"),
        ];
        let width = 10u16;

        let offset_0 = rendered_row_offset(&lines, 0, width);
        let offset_1 = rendered_row_offset(&lines, 1, width);
        let offset_2 = rendered_row_offset(&lines, 2, width);

        assert_eq!(offset_0, 0, "nothing precedes the first line");
        assert_eq!(offset_1, 1, "one short line renders as exactly one row");
        assert!(
            offset_2 - offset_1 > 1,
            "the 100-char line must occupy more than one rendered row at width {width}, \
             got {} rows (offset_1={offset_1}, offset_2={offset_2})",
            offset_2 - offset_1
        );
    }

    #[test]
    fn missing_rule_yields_no_highlights_but_still_renders() {
        let state = InterceptState::default();
        let m = sample_match(1, base_entry());

        let detail = build(&state, &m, 0);
        assert_eq!(detail.occurrence_count, 0);
        assert!(detail.current_line.is_none());
        assert!(!detail.lines.is_empty());
    }

    #[test]
    fn invalid_regex_on_rule_yields_no_highlights_but_still_renders() {
        let state = state_with_rule(sample_rule(1, "(unterminated"));
        let m = sample_match(1, base_entry());

        let detail = build(&state, &m, 0);
        assert_eq!(detail.occurrence_count, 0);
        assert!(detail.current_line.is_none());
        assert!(!detail.lines.is_empty());
    }
}
