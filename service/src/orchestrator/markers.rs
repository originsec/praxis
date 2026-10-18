//
// Display filtering for prompt-based (manual) tool calling. The model emits
// tool calls as inline JSON, so streamed text is held back from the operator
// once a tool-call marker appears. Native tool calling never needs this.
//

#[derive(Debug, PartialEq, Eq)]
enum ToolMarkerState {
    Complete,
    Prefix,
    None,
}

fn plain_tool_marker_state(candidate: &[u8]) -> ToolMarkerState {
    if candidate.first() != Some(&b'{') {
        return ToolMarkerState::None;
    }

    let mut index = 1;
    while index < candidate.len() && candidate[index].is_ascii_whitespace() {
        index += 1;
    }
    if index == candidate.len() {
        return ToolMarkerState::Prefix;
    }

    let expected = b"\"tool\"";
    let available = candidate.len() - index;
    let compare_len = available.min(expected.len());
    if candidate[index..index + compare_len] != expected[..compare_len] {
        return ToolMarkerState::None;
    }
    if available < expected.len() {
        return ToolMarkerState::Prefix;
    }
    index += expected.len();

    while index < candidate.len() && candidate[index].is_ascii_whitespace() {
        index += 1;
    }
    if index == candidate.len() {
        return ToolMarkerState::Prefix;
    }

    if candidate[index] == b':' {
        ToolMarkerState::Complete
    } else {
        ToolMarkerState::None
    }
}

//
// Some models (GLM, Qwen, and other Hermes-style tool-calling templates)
// wrap a call in `<tool_call>...</tool_call>` by default, even though
// orchestrator.prompt asks for bare JSON with no wrapper. Treat the
// opening tag as its own marker so it is held back from the display the
// same way `{"tool":` is — once it appears, Rule 2 (stop talking after
// the call) means nothing meaningful ever follows it anyway.
//
const TOOL_CALL_TAG: &[u8] = b"<tool_call>";

fn tag_marker_state(candidate: &[u8]) -> ToolMarkerState {
    if candidate.first() != Some(&b'<') {
        return ToolMarkerState::None;
    }

    let compare_len = candidate.len().min(TOOL_CALL_TAG.len());
    if candidate[..compare_len] != TOOL_CALL_TAG[..compare_len] {
        return ToolMarkerState::None;
    }

    if candidate.len() < TOOL_CALL_TAG.len() {
        ToolMarkerState::Prefix
    } else {
        ToolMarkerState::Complete
    }
}

fn marker_state_at(bytes: &[u8], index: usize) -> ToolMarkerState {
    match bytes[index] {
        b'{' => plain_tool_marker_state(&bytes[index..]),
        b'<' => tag_marker_state(&bytes[index..]),
        _ => ToolMarkerState::None,
    }
}

fn earliest_marker(text: &str, want: ToolMarkerState) -> Option<usize> {
    let bytes = text.as_bytes();
    bytes
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == b'{' || **byte == b'<')
        .find_map(|(index, _)| (marker_state_at(bytes, index) == want).then_some(index))
}

fn streaming_tool_marker_position(text: &str) -> Option<usize> {
    let marker = earliest_marker(text, ToolMarkerState::Complete);
    match (marker, text.find("```")) {
        (Some(marker), Some(fence)) => Some(marker.min(fence)),
        (Some(marker), None) => Some(marker),
        (None, Some(fence)) => Some(fence),
        (None, None) => None,
    }
}

fn trailing_tool_marker_prefix_start(text: &str) -> Option<usize> {
    let marker = earliest_marker(text, ToolMarkerState::Prefix);
    let fence = if text.ends_with("``") {
        Some(text.len() - 2)
    } else if text.ends_with('`') {
        Some(text.len() - 1)
    } else {
        None
    };

    match (marker, fence) {
        (Some(marker), Some(fence)) => Some(marker.min(fence)),
        (Some(marker), None) => Some(marker),
        (None, Some(fence)) => Some(fence),
        (None, None) => None,
    }
}

//
// Incremental filter over streamed manual-mode text. `push` returns the
// chunks that are safe to show; once a tool marker is seen everything after
// it is held back. `finish` flushes any buffered tail, and `unsent_tail`
// returns what was held back when the response turned out to contain no
// tool call after all (e.g. an ordinary code fence).
//

#[derive(Default)]
pub(super) struct ManualTextFilter {
    send_buffer: String,
    held_back: bool,
    bytes_sent: usize,
}

impl ManualTextFilter {
    pub(super) fn push(&mut self, delta: &str) -> Vec<String> {
        let mut out = Vec::new();
        if self.held_back {
            return out;
        }
        self.send_buffer.push_str(delta);

        if let Some(marker_pos) = streaming_tool_marker_position(&self.send_buffer) {
            if marker_pos > 0 {
                let pre_tool = &self.send_buffer[..marker_pos];
                let cleaned = pre_tool.trim_end_matches(['`', '\n', '\r']);
                let cleaned = cleaned.trim_end_matches("json").trim_end_matches('`');
                if !cleaned.trim().is_empty() {
                    self.bytes_sent += cleaned.len();
                    out.push(cleaned.to_string());
                }
            }
            self.held_back = true;
            self.send_buffer.clear();
        } else if self.send_buffer.len() >= 50 || delta.contains('\n') {
            if let Some(split) = trailing_tool_marker_prefix_start(&self.send_buffer) {
                if split > 0 {
                    self.bytes_sent += split;
                    out.push(self.send_buffer[..split].to_string());
                }
                self.send_buffer = self.send_buffer[split..].to_string();
            } else {
                self.bytes_sent += self.send_buffer.len();
                out.push(std::mem::take(&mut self.send_buffer));
            }
        }
        out
    }

    pub(super) fn finish(&mut self) -> Option<String> {
        if self.held_back || self.send_buffer.is_empty() {
            return None;
        }
        self.bytes_sent += self.send_buffer.len();
        Some(std::mem::take(&mut self.send_buffer))
    }

    pub(super) fn unsent_tail<'a>(&self, full: &'a str) -> Option<&'a str> {
        let start = self.bytes_sent;
        if full.len() > start && full.is_char_boundary(start) {
            Some(&full[start..]).filter(|s| !s.is_empty())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_tool_marker_detects_plain_and_spaced_calls() {
        assert_eq!(
            streaming_tool_marker_position(
                "Trigger created. {\"tool\": \"report_plan\", \"args\": {}}"
            ),
            Some(17)
        );
        assert_eq!(
            streaming_tool_marker_position("Trigger created. { \n \"tool\" : \"report_plan\""),
            Some(17)
        );
    }

    #[test]
    fn streaming_tool_marker_survives_a_chunk_boundary() {
        let first_chunk = "Trigger created. Let me verify it's active. {";
        let marker_start = trailing_tool_marker_prefix_start(first_chunk)
            .expect("the trailing opening brace must be retained");
        let visible = &first_chunk[..marker_start];
        let retained = &first_chunk[marker_start..];

        assert_eq!(visible, "Trigger created. Let me verify it's active. ");

        let next_buffer =
            format!("{retained}\"tool\": \"report_plan\", \"args\": {{\"steps\": []}}}}");
        assert_eq!(streaming_tool_marker_position(&next_buffer), Some(0));
    }

    #[test]
    fn streaming_tool_marker_does_not_hold_ordinary_braces() {
        assert_eq!(trailing_tool_marker_prefix_start("Result: {done"), None);
        assert_eq!(streaming_tool_marker_position("Result: {done}"), None);
    }

    #[test]
    fn streaming_tool_marker_detects_tool_call_tag() {
        assert_eq!(
            streaming_tool_marker_position(
                "Checking nodes. <tool_call>{\"tool\": \"node_list\", \"args\": {}}"
            ),
            Some(16)
        );
    }

    #[test]
    fn streaming_tool_marker_tag_survives_a_chunk_boundary() {
        let first_chunk = "Checking nodes and operations available. <tool_call";
        let marker_start = trailing_tool_marker_prefix_start(first_chunk)
            .expect("the trailing partial tag must be retained");
        let visible = &first_chunk[..marker_start];
        let retained = &first_chunk[marker_start..];

        assert_eq!(visible, "Checking nodes and operations available. ");

        let next_buffer = format!("{retained}>{{\"tool\": \"node_list\", \"args\": {{}}}}");
        assert_eq!(streaming_tool_marker_position(&next_buffer), Some(0));
    }

    #[test]
    fn streaming_tool_marker_does_not_hold_ordinary_angle_brackets() {
        assert_eq!(trailing_tool_marker_prefix_start("Result: <done"), None);
        assert_eq!(streaming_tool_marker_position("Result: <done>"), None);
    }

    #[test]
    fn manual_filter_holds_back_tool_call() {
        let mut filter = ManualTextFilter::default();
        let mut shown = String::new();
        let full = "Listing nodes now.\n{\"tool\": \"node_list\", \"args\": {}}";
        for chunk in [
            "Listing nodes now.\n",
            "{\"to",
            "ol\": \"node_list\", \"args\": {}}",
        ] {
            for s in filter.push(chunk) {
                shown.push_str(&s);
            }
        }
        assert!(filter.finish().is_none());
        assert_eq!(shown, "Listing nodes now.\n");
        assert!(filter.unsent_tail(full).unwrap().starts_with("{\"tool\""));
    }

    #[test]
    fn manual_filter_passes_plain_text_through() {
        let mut filter = ManualTextFilter::default();
        let mut shown = String::new();
        for chunk in ["Hello ", "world"] {
            for s in filter.push(chunk) {
                shown.push_str(&s);
            }
        }
        shown.push_str(&filter.finish().unwrap());
        assert_eq!(shown, "Hello world");
        assert_eq!(filter.unsent_tail("Hello world"), None);
    }
}
