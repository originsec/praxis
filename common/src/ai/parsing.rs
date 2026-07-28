use regex::Regex;
use serde_json::Value;

/// Find matching closing brace using brace counting, respecting JSON string literals
fn find_matching_brace(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if start >= bytes.len() || bytes[start] != b'{' {
        return None;
    }

    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut i = start;

    while i < bytes.len() {
        let c = bytes[i];

        if escape_next {
            escape_next = false;
            i += 1;
            continue;
        }

        if c == b'\\' && in_string {
            escape_next = true;
            i += 1;
            continue;
        }

        if c == b'"' {
            in_string = !in_string;
        } else if !in_string {
            if c == b'{' {
                depth += 1;
            } else if c == b'}' {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }

        i += 1;
    }

    None
}

//
// Some models (GLM, Qwen, and other Hermes-style tool-calling templates)
// wrap a call in `<tool_call>...</tool_call>` by default, even though
// `tool_calling.prompt` asks for bare JSON with no wrapper. Strip the tags
// unconditionally before searching so the JSON underneath is found the
// same way regardless of which convention the model actually used.
//
fn strip_tool_call_tags(text: &str) -> String {
    text.replace("<tool_call>", "").replace("</tool_call>", "")
}

/// Parse manual tool calls from AI response text
///
/// Returns: (tool_name, tool_args, remaining_text_without_tool_call)
///
/// Looks for a JSON block in the documented `{"tool": "tool_name", "args":
/// {...}}` format (see `tool_calling.prompt`), in either code-fenced or
/// plain form, optionally wrapped in `<tool_call>` tags. A call missing
/// the `args` object is not recognized here — see
/// `parse_manual_tool_call_lenient` for callers that need to tolerate
/// that.
pub fn parse_manual_tool_call(text: &str) -> Option<(String, Value, String)> {
    parse_manual_tool_call_with(text, extract_tool_call_parts)
}

//
// Some models occasionally flatten a tool call's arguments onto the
// top-level object (e.g. `{"tool": "search_docs", "query": "..."}`) despite
// every consumer being prompted with the strict `args`-wrapped contract.
// Tolerance for that drift is scoped to callers that have actually observed
// it, rather than loosened globally — a stray JSON object carrying a "tool"
// key should not be misread as a call by consumers that never asked for the
// lenient form.
//

/// Like `parse_manual_tool_call`, but also accepts a tool call whose
/// arguments are flattened onto the top-level object instead of nested
/// under `"args"`.
pub fn parse_manual_tool_call_lenient(text: &str) -> Option<(String, Value, String)> {
    parse_manual_tool_call_with(text, extract_tool_call_parts_lenient)
}

fn parse_manual_tool_call_with(
    text: &str,
    extract: impl Fn(&Value) -> Option<(String, Value)>,
) -> Option<(String, Value, String)> {
    let text = strip_tool_call_tags(text);
    let text = text.as_str();

    //
    // Try to find a tool call pattern using brace-counting for robustness
    // This handles nested braces inside JSON string values correctly.
    //

    //
    // First, look for code-fenced JSON blocks.
    //
    let code_fence_re = Regex::new(r#"```(?:json)?\s*\n?"#).ok()?;

    for fence_match in code_fence_re.find_iter(text) {
        let after_fence = fence_match.end();

        //
        // Find the start of the JSON object.
        //
        let json_start = text[after_fence..].find('{').map(|i| after_fence + i)?;

        //
        // Use brace counting to find the end.
        //
        if let Some(json_end) = find_matching_brace(text, json_start) {
            let json_str = &text[json_start..=json_end];

            //
            // Try to parse as tool call.
            //
            if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
                if let Some((tool_name, args)) = extract(&parsed) {
                    //
                    // Find the closing fence.
                    //
                    let remaining_text_start = text[json_end + 1..]
                        .find("```")
                        .map(|i| json_end + 1 + i + 3)
                        .unwrap_or(json_end + 1);

                    let before = &text[..fence_match.start()];
                    let after = &text[remaining_text_start..];
                    let remaining_text = format!("{}{}", before, after).trim().to_string();

                    return Some((tool_name, args, remaining_text));
                }
            }
        }
    }

    //
    // Also try without code block markers (plain JSON)
    // Look for {"tool": pattern.
    //
    let tool_pattern = r#"\{\s*"tool"\s*:"#;
    let tool_re = Regex::new(tool_pattern).ok()?;

    for tool_match in tool_re.find_iter(text) {
        let json_start = tool_match.start();

        //
        // Use brace counting to find the end.
        //
        if let Some(json_end) = find_matching_brace(text, json_start) {
            let json_str = &text[json_start..=json_end];

            //
            // Try to parse as tool call.
            //
            if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
                if let Some((tool_name, args)) = extract(&parsed) {
                    let before = &text[..json_start];
                    let after = &text[json_end + 1..];
                    let remaining_text = format!("{}{}", before, after).trim().to_string();

                    return Some((tool_name, args, remaining_text));
                }
            }
        }
    }

    None
}

///
/// Collect every tool-call JSON object in `text`, left-to-right.
///
/// Returns `(calls, remaining_text)` where `calls` is ordered by appearance
/// and `remaining_text` is the original text with all tool-call blocks
/// removed. An empty `calls` vec means no tool calls were found.
///
pub fn parse_manual_tool_calls(text: &str) -> (Vec<(String, Value)>, String) {
    let mut calls = Vec::new();
    let mut remaining = text.to_string();
    while let Some((name, args, rest)) = parse_manual_tool_call(&remaining) {
        calls.push((name, args));
        remaining = rest;
    }
    (calls, remaining)
}

fn extract_tool_call_parts(parsed: &Value) -> Option<(String, Value)> {
    let tool_name = parsed.get("tool")?.as_str()?.to_string();
    let args = parsed.get("args")?.clone();
    Some((tool_name, args))
}

fn extract_tool_call_parts_lenient(parsed: &Value) -> Option<(String, Value)> {
    let tool_name = parsed.get("tool")?.as_str()?.to_string();

    if let Some(args) = parsed.get("args") {
        return Some((tool_name, args.clone()));
    }

    let mut args = parsed.as_object()?.clone();
    args.remove("tool");
    Some((tool_name, Value::Object(args)))
}

//
// Detect a manual tool-call JSON block that was cut short (e.g. by a
// provider's output-token limit) before its closing brace, and strip it so
// truncated scaffolding is never shown to the operator. Only the plain
// (non-code-fenced) `{"tool": ...}` form is checked: a fenced block cut off
// mid-JSON has no closing fence either, so the same brace-counting failure
// applies without needing a separate scan.
//

/// Strip a trailing, unterminated tool-call JSON fragment from `text`.
/// Returns the text with the fragment (and any trailing whitespace before
/// it) removed, and whether a fragment was found. A dangling `<tool_call>`
/// wrapper tag (with or without a JSON fragment after it) is always
/// removed too, since it is never meaningful to show the operator.
pub fn strip_incomplete_tool_call(text: &str) -> (String, bool) {
    let text = strip_tool_call_tags(text);

    let Ok(tool_re) = Regex::new(r#"\{\s*"tool"\s*:"#) else {
        return (text.trim_end().to_string(), false);
    };

    for tool_match in tool_re.find_iter(&text) {
        let json_start = tool_match.start();
        if find_matching_brace(&text, json_start).is_none() {
            return (text[..json_start].trim_end().to_string(), true);
        }
    }

    (text.trim_end().to_string(), false)
}

/// Parse completion signal from AI response text
///
/// Returns: (is_complete, summary, result, remaining_text, success)
///
/// Looks for JSON blocks in format:
/// {"complete": true, "summary": "...", "result": "...", "success": true}
///
/// The 'summary' field should be a brief description of actions taken.
/// The 'result' field should contain the actual findings/data/output.
/// The 'success' field indicates whether the objective was achieved.
pub fn parse_completion_signal(text: &str) -> Option<(bool, String, String, String, Option<bool>)> {
    //
    // Use JSON parsing for robustness instead of regex.
    // First try code-fenced JSON blocks, then plain JSON.
    //

    //
    // Look for code-fenced JSON blocks.
    //
    let code_fence_re = Regex::new(r#"```(?:json)?\s*\n?"#).ok()?;

    for fence_match in code_fence_re.find_iter(text) {
        let after_fence = fence_match.end();

        //
        // Find the start of the JSON object.
        //
        if let Some(rel_start) = text[after_fence..].find('{') {
            let json_start = after_fence + rel_start;

            //
            // Use brace counting to find the end.
            //
            if let Some(json_end) = find_matching_brace(text, json_start) {
                let json_str = &text[json_start..=json_end];

                //
                // Try to parse as completion signal.
                //
                if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
                    if let Some(complete) = parsed.get("complete").and_then(|v| v.as_bool()) {
                        let summary = parsed
                            .get("summary")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let (result, success) = match parsed.get("result").and_then(|v| v.as_bool())
                        {
                            Some(true) => ("success".to_string(), Some(true)),
                            Some(false) => ("failure".to_string(), Some(false)),
                            None => ("".to_string(), None),
                        };

                        //
                        // Find the closing fence.
                        //
                        let remaining_text_start = text[json_end + 1..]
                            .find("```")
                            .map(|i| json_end + 1 + i + 3)
                            .unwrap_or(json_end + 1);

                        let before = &text[..fence_match.start()];
                        let after = &text[remaining_text_start..];
                        let remaining_text = format!("{}{}", before, after).trim().to_string();

                        return Some((complete, summary, result, remaining_text, success));
                    }
                }
            }
        }
    }

    //
    // Also try without code block markers (plain JSON).
    // Look for {"complete": pattern.
    //
    let complete_pattern = r#"\{\s*"complete"\s*:"#;
    let complete_re = Regex::new(complete_pattern).ok()?;

    for complete_match in complete_re.find_iter(text) {
        let json_start = complete_match.start();

        //
        // Use brace counting to find the end.
        //
        if let Some(json_end) = find_matching_brace(text, json_start) {
            let json_str = &text[json_start..=json_end];

            //
            // Try to parse as completion signal.
            //
            if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
                if let Some(complete) = parsed.get("complete").and_then(|v| v.as_bool()) {
                    let summary = parsed
                        .get("summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let (result, success) = match parsed.get("result").and_then(|v| v.as_bool()) {
                        Some(true) => ("success".to_string(), Some(true)),
                        Some(false) => ("failure".to_string(), Some(false)),
                        None => ("".to_string(), None),
                    };

                    let before = &text[..json_start];
                    let after = &text[json_end + 1..];
                    let remaining_text = format!("{}{}", before, after).trim().to_string();

                    return Some((complete, summary, result, remaining_text, success));
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_call_with_code_fence() {
        let text = r#"I need to check something.

```json
{"tool": "session_prompt", "args": {"text": "test prompt"}}
```

This will help me understand."#;

        let result = parse_manual_tool_call(text);
        assert!(result.is_some());

        let (tool_name, args, remaining) = result.unwrap();
        assert_eq!(tool_name, "session_prompt");
        assert_eq!(args["text"], "test prompt");
        assert!(remaining.contains("I need to check something"));
        assert!(remaining.contains("This will help me understand"));
    }

    #[test]
    fn test_parse_tool_call_plain_json() {
        let text = r#"I'll use this tool: {"tool": "node_list", "args": {}} to get the list."#;

        let result = parse_manual_tool_call(text);
        assert!(result.is_some());

        let (tool_name, _args, remaining) = result.unwrap();
        assert_eq!(tool_name, "node_list");
        assert!(remaining.contains("I'll use this tool"));
        assert!(remaining.contains("to get the list"));
    }

    #[test]
    fn test_parse_tool_call_wrapped_in_tool_call_tags() {
        //
        // GLM/Qwen/Hermes-style models default to this wrapper regardless
        // of the bare-JSON format the prompt asks for.
        //
        let text = r#"I'll check available operations.
<tool_call>{"tool": "op_available", "args": {}}</tool_call>"#;

        let result = parse_manual_tool_call(text);
        assert!(result.is_some());

        let (tool_name, _args, remaining) = result.unwrap();
        assert_eq!(tool_name, "op_available");
        assert!(remaining.contains("I'll check available operations"));
        assert!(!remaining.contains("tool_call"));
    }

    #[test]
    fn test_parse_tool_call_with_direct_arguments_lenient() {
        let text = r#"I'll check the Interception documentation for that.
{"tool": "search_docs", "query": "node not showing in intercept window"}"#;

        let result = parse_manual_tool_call_lenient(text);
        assert!(result.is_some());

        let (tool_name, args, remaining) = result.unwrap();
        assert_eq!(tool_name, "search_docs");
        assert_eq!(args["query"], "node not showing in intercept window");
        assert_eq!(
            remaining,
            "I'll check the Interception documentation for that."
        );
    }

    #[test]
    fn test_strict_parser_rejects_direct_arguments() {
        //
        // Consumers other than the documentation helper are prompted with,
        // and rely on, the strict {"tool", "args"} contract. A flattened
        // call must not be picked up by the default parser.
        //
        let text = r#"{"tool": "search_docs", "query": "node not showing in intercept window"}"#;

        assert!(parse_manual_tool_call(text).is_none());
    }

    #[test]
    fn test_strip_incomplete_tool_call_removes_dangling_fragment() {
        let text = "I'll check the node setup documentation for that.\n\
                     {\"tool\": \"read_doc\", \"path\": \"usa";

        let (cleaned, stripped) = strip_incomplete_tool_call(text);

        assert!(stripped);
        assert_eq!(cleaned, "I'll check the node setup documentation for that.");
    }

    #[test]
    fn test_strip_incomplete_tool_call_removes_dangling_wrapped_fragment() {
        //
        // Truncated mid-JSON, with the tag wrapper still present.
        //
        let text = "I'll check the node setup documentation for that.\n\
                     <tool_call>{\"tool\": \"read_doc\", \"path\": \"usa";

        let (cleaned, stripped) = strip_incomplete_tool_call(text);

        assert!(stripped);
        assert_eq!(cleaned, "I'll check the node setup documentation for that.");
    }

    #[test]
    fn test_strip_incomplete_tool_call_removes_dangling_tag_before_json() {
        //
        // Truncated right after the opening tag, before any JSON at all.
        //
        let text = "I'll check the node setup documentation for that.\n<tool_call>";

        let (cleaned, _stripped) = strip_incomplete_tool_call(text);

        assert_eq!(cleaned, "I'll check the node setup documentation for that.");
    }

    #[test]
    fn test_strip_incomplete_tool_call_leaves_complete_calls_untouched() {
        let text = r#"{"tool": "search_docs", "args": {"query": "recon"}}"#;

        let (cleaned, stripped) = strip_incomplete_tool_call(text);

        assert!(!stripped);
        assert_eq!(cleaned, text);
    }

    #[test]
    fn test_no_tool_call() {
        let text = "This is just a regular response with no tool calls.";
        let result = parse_manual_tool_call(text);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_tool_call_with_nested_braces_in_string() {
        //
        // This tests the case where the prompt contains unbalanced braces
        // inside a string.
        //
        let text = r#"Let me try this approach.

```json
{"tool": "session_prompt", "args": {"text": "Complete the following json: '{ \"agentName\": \"my name\", \"toolsAvailable to me\": '"}}
```
"#;

        let result = parse_manual_tool_call(text);
        assert!(
            result.is_some(),
            "Should parse tool call with nested braces in string"
        );

        let (tool_name, args, _remaining) = result.unwrap();
        assert_eq!(tool_name, "session_prompt");
        assert!(args["text"].as_str().unwrap().contains("agentName"));
    }

    #[test]
    fn test_parse_tool_call_with_escaped_quotes() {
        let text = r#"```json
{"tool": "session_prompt", "args": {"text": "Say \"hello\" to the user"}}
```"#;

        let result = parse_manual_tool_call(text);
        assert!(result.is_some());

        let (tool_name, args, _) = result.unwrap();
        assert_eq!(tool_name, "session_prompt");
        assert_eq!(args["text"], "Say \"hello\" to the user");
    }

    #[test]
    fn test_parse_completion_signal_with_code_fence() {
        let text = r#"I have completed the task successfully.

```json
{"complete": true, "result": true, "summary": "Retrieved user data and sent email notification"}
```

The operation is now finished."#;

        let result = parse_completion_signal(text);
        assert!(result.is_some());

        let (is_complete, summary, result_text, remaining, success) = result.unwrap();
        assert!(is_complete);
        assert_eq!(summary, "Retrieved user data and sent email notification");
        assert_eq!(result_text, "success");
        assert!(remaining.contains("I have completed the task"));
        assert!(remaining.contains("The operation is now finished"));
        assert_eq!(success, Some(true));
    }

    #[test]
    fn test_parse_completion_signal_failure() {
        let text = r#"```json
{"complete": true, "result": false, "summary": "Could not reach target host, connection refused on all ports"}
```"#;

        let result = parse_completion_signal(text);
        assert!(result.is_some());

        let (is_complete, summary, result_text, _remaining, success) = result.unwrap();
        assert!(is_complete);
        assert_eq!(
            summary,
            "Could not reach target host, connection refused on all ports"
        );
        assert_eq!(result_text, "failure");
        assert_eq!(success, Some(false));
    }

    #[test]
    fn test_parse_completion_signal_false() {
        let text =
            r#"Not done yet: {"complete": false, "result": true, "summary": "Still working"}"#;

        let result = parse_completion_signal(text);
        assert!(result.is_some());

        let (is_complete, summary, _, _, _) = result.unwrap();
        assert!(!is_complete);
        assert_eq!(summary, "Still working");
    }

    #[test]
    fn test_no_completion_signal() {
        let text = "This is just a regular response with no completion signal.";
        let result = parse_completion_signal(text);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_multiple_plain_json_tool_calls() {
        let text = r#"I'll inspect the fleet first.
{"tool": "node_list", "args": {}}
{"tool": "agent_list", "args": {"node_id": "abc"}}
{"tool": "node_list", "args": {"refresh": true}}
"#;

        let (calls, remaining) = parse_manual_tool_calls(text);
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, "node_list");
        assert_eq!(calls[0].1, serde_json::json!({}));
        assert_eq!(calls[1].0, "agent_list");
        assert_eq!(calls[1].1["node_id"], "abc");
        assert_eq!(calls[2].0, "node_list");
        assert_eq!(calls[2].1["refresh"], true);
        assert!(remaining.contains("I'll inspect the fleet first"));
        assert!(!remaining.contains("\"tool\""));
    }

    #[test]
    fn test_parse_multiple_tool_calls_wrapped_in_tags() {
        //
        // Reproduces the GLM-via-OpenRouter shape: multiple calls emitted
        // together inside a single <tool_call> wrapper.
        //
        let text = r#"I'll check operations and current nodes.
<tool_call>{"tool": "op_available", "args": {}},{"tool": "node_list", "args": {}}</tool_call>"#;

        let (calls, remaining) = parse_manual_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "op_available");
        assert_eq!(calls[1].0, "node_list");
        assert!(remaining.contains("I'll check operations and current nodes"));
        assert!(!remaining.contains("tool_call"));
    }

    #[test]
    fn test_parse_multiple_code_fenced_tool_calls() {
        let text = r#"Gathering context.

```json
{"tool": "node_list", "args": {}}
```

```json
{"tool": "op_available", "args": {}}
```
"#;

        let (calls, remaining) = parse_manual_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "node_list");
        assert_eq!(calls[1].0, "op_available");
        assert!(remaining.contains("Gathering context"));
        assert!(!remaining.contains("node_list"));
    }

    #[test]
    fn test_parse_multiple_mixed_prose_and_tools() {
        let text = r#"Checking nodes then agents.
{"tool": "node_list", "args": {}}
Next I'll list agents on the first node.
{"tool": "agent_list", "args": {"node_id": "n1"}}
Done requesting."#;

        let (calls, remaining) = parse_manual_tool_calls(text);
        assert!(calls.len() >= 2);
        assert_eq!(calls[0].0, "node_list");
        assert_eq!(calls[1].0, "agent_list");
        assert_eq!(calls[1].1["node_id"], "n1");
        assert!(remaining.contains("Checking nodes then agents"));
        assert!(remaining.contains("Done requesting"));
    }

    #[test]
    fn test_parse_manual_tool_calls_empty() {
        let (calls, remaining) = parse_manual_tool_calls("No tools here.");
        assert!(calls.is_empty());
        assert_eq!(remaining, "No tools here.");
    }
}
