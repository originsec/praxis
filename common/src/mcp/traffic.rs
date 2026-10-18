use base64::Engine;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

//
// Classification of intercepted HTTP traffic as Model Context Protocol
// (MCP) JSON-RPC. Two signals are used, in order:
//
// 1. Standard request headers. Revision 2026-07-28 requires `Mcp-Method`
//    (and `Mcp-Name` for tools/call, resources/read, prompts/get) on every
//    Streamable HTTP POST, so gateways can route without parsing bodies.
// 2. The JSON-RPC body. Older revisions (2025-11-25 and earlier) carry no
//    method header, so the body's `method` is matched against the MCP method
//    namespace. Methods shared with other JSON-RPC protocols (`initialize`
//    is also ACP and LSP, `tasks/*` is also A2A, `ping` is generic) only
//    count when some other MCP marker corroborates them.
//

pub const HEADER_MCP_METHOD: &str = "mcp-method";
pub const HEADER_MCP_NAME: &str = "mcp-name";
pub const HEADER_MCP_PROTOCOL_VERSION: &str = "mcp-protocol-version";
pub const HEADER_MCP_SESSION_ID: &str = "mcp-session-id";

const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_KEY_PREFIX: &str = "io.modelcontextprotocol/";

/// MCP fields derived from one intercepted HTTP request or response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpTrafficInfo {
    /// JSON-RPC method (e.g. `tools/call`).
    pub method: String,
    /// Tool/prompt name or resource URI the request targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Protocol version from the header, `_meta`, or initialize params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    /// Legacy (pre-2026-07-28) `Mcp-Session-Id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// JSON-RPC request id, rendered as a string. None for notifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jsonrpc_id: Option<String>,
    /// Set on responses: true for a JSON-RPC error or a tool result with
    /// `isError: true`. None when no response message was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Classify an HTTP request as MCP. Header path first, body fallback.
pub fn classify_mcp_request(
    headers: Option<&IndexMap<String, String>>,
    body: Option<&[u8]>,
) -> Option<McpTrafficInfo> {
    let header_method = header(headers, HEADER_MCP_METHOD).filter(|m| !m.is_empty());
    let message = body.and_then(parse_request_message);

    let body_method = message.as_ref().and_then(|m| {
        let method = m.get("method")?.as_str()?;
        is_mcp_method(method, m.get("params"), headers).then(|| method.to_string())
    });

    let method = header_method.map(str::to_string).or(body_method)?;
    let params = message.as_ref().and_then(|m| m.get("params"));

    let name = header(headers, HEADER_MCP_NAME)
        .map(decode_header_value)
        .or_else(|| params.and_then(target_name));

    let protocol_version = header(headers, HEADER_MCP_PROTOCOL_VERSION)
        .map(str::to_string)
        .or_else(|| {
            let params = params?;
            params
                .get("_meta")
                .and_then(|meta| meta.get(META_PROTOCOL_VERSION))
                .or_else(|| (method == "initialize").then(|| params.get("protocolVersion"))?)
                .and_then(Value::as_str)
                .map(str::to_string)
        });

    Some(McpTrafficInfo {
        method,
        name,
        protocol_version,
        session_id: header(headers, HEADER_MCP_SESSION_ID).map(str::to_string),
        jsonrpc_id: message
            .as_ref()
            .and_then(|m| m.get("id"))
            .and_then(id_string),
        is_error: None,
    })
}

impl McpTrafficInfo {
    //
    // HTTP/2 carries headers and body in separate frames. Fill the
    // body-only fields (JSON-RPC id, and name/version when the headers had
    // none) from a DATA frame on a stream already classified by its headers.
    //

    pub fn with_request_body(&self, body: &[u8]) -> McpTrafficInfo {
        let Some(from_body) = classify_mcp_request(None, Some(body)) else {
            return self.clone();
        };
        McpTrafficInfo {
            name: self.name.clone().or(from_body.name),
            protocol_version: self.protocol_version.clone().or(from_body.protocol_version),
            jsonrpc_id: from_body.jsonrpc_id,
            ..self.clone()
        }
    }

    //
    // Derive the info for the response to this request. The response body
    // may be plain JSON or an SSE stream of JSON-RPC messages; the message
    // answering our request id (or, failing that, the last response seen)
    // decides `is_error`. A legacy initialize response also carries the
    // session id header and the negotiated protocol version.
    //

    pub fn for_response(
        &self,
        headers: Option<&IndexMap<String, String>>,
        body: Option<&[u8]>,
    ) -> McpTrafficInfo {
        let mut info = self.clone();

        if info.session_id.is_none() {
            info.session_id = header(headers, HEADER_MCP_SESSION_ID).map(str::to_string);
        }

        let responses = body.map(parse_response_messages).unwrap_or_default();
        let response = responses
            .iter()
            .rev()
            .find(|m| {
                info.jsonrpc_id.is_some() && m.get("id").and_then(id_string) == info.jsonrpc_id
            })
            .or_else(|| responses.last());

        if let Some(response) = response {
            let result = response.get("result");
            info.is_error = Some(
                response.get("error").is_some()
                    || result
                        .and_then(|r| r.get("isError"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
            );
            if info.protocol_version.is_none() {
                info.protocol_version = result
                    .and_then(|r| r.get("protocolVersion"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
        }

        info
    }
}

fn header<'a>(headers: Option<&'a IndexMap<String, String>>, name: &str) -> Option<&'a str> {
    headers?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim())
}

//
// `Mcp-Name` values that are not header-safe are sent as
// `=?base64?<b64>?=`. Undecodable values are kept verbatim.
//

fn decode_header_value(value: &str) -> String {
    value
        .strip_prefix("=?base64?")
        .and_then(|v| v.strip_suffix("?="))
        .and_then(|b64| base64::engine::general_purpose::STANDARD.decode(b64).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_else(|| value.to_string())
}

//
// A request body is a single JSON-RPC message or (2025-03-26 only) a batch
// array; for a batch the first element carrying a method is used.
//

fn parse_request_message(body: &[u8]) -> Option<Value> {
    //
    // Every captured request body reaches this, including large LLM API
    // payloads; skip the full JSON parse unless it can be JSON-RPC.
    //

    if !body.windows(9).any(|w| w == b"\"jsonrpc\"") {
        return None;
    }
    let value: Value = serde_json::from_slice(body).ok()?;
    let message = match value {
        Value::Array(items) => items.into_iter().find(|m| m.get("method").is_some())?,
        other => other,
    };
    (message.get("jsonrpc").and_then(Value::as_str) == Some("2.0")).then_some(message)
}

fn parse_response_messages(body: &[u8]) -> Vec<Value> {
    let is_response = |m: &Value| m.get("result").is_some() || m.get("error").is_some();
    let flatten = |value: Value| match value {
        Value::Array(items) => items,
        other => vec![other],
    };

    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        return flatten(value).into_iter().filter(is_response).collect();
    }

    //
    // SSE: each event's `data:` lines join with newlines into one payload.
    //

    let Ok(text) = std::str::from_utf8(body) else {
        return Vec::new();
    };
    let mut messages = Vec::new();
    let mut data = String::new();
    for line in text.lines().chain(std::iter::once("")) {
        if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        } else if line.is_empty() && !data.is_empty() {
            if let Ok(value) = serde_json::from_str::<Value>(&data) {
                messages.extend(flatten(value).into_iter().filter(is_response));
            }
            data.clear();
        }
    }
    messages
}

fn is_mcp_method(
    method: &str,
    params: Option<&Value>,
    headers: Option<&IndexMap<String, String>>,
) -> bool {
    const PREFIXES: &[&str] = &["tools/", "resources/", "prompts/", "notifications/"];
    const METHODS: &[&str] = &[
        "server/discover",
        "subscriptions/listen",
        "completion/complete",
        "logging/setLevel",
        "sampling/createMessage",
        "elicitation/create",
        "roots/list",
    ];

    if METHODS.contains(&method) || PREFIXES.iter().any(|p| method.starts_with(p)) {
        return true;
    }

    let ambiguous = matches!(method, "initialize" | "ping") || method.starts_with("tasks/");
    ambiguous && has_mcp_marker(method, params, headers)
}

//
// Corroborating evidence for methods other JSON-RPC protocols also use:
// an MCP transport header, an `io.modelcontextprotocol/*` `_meta` key, or
// (for initialize) a date-shaped protocolVersion string. ACP's initialize
// uses an integer protocolVersion and LSP's has none.
//

fn has_mcp_marker(
    method: &str,
    params: Option<&Value>,
    headers: Option<&IndexMap<String, String>>,
) -> bool {
    if header(headers, HEADER_MCP_PROTOCOL_VERSION).is_some()
        || header(headers, HEADER_MCP_SESSION_ID).is_some()
    {
        return true;
    }
    let Some(params) = params else {
        return false;
    };
    let has_meta_key = params
        .get("_meta")
        .and_then(Value::as_object)
        .is_some_and(|meta| meta.keys().any(|k| k.starts_with(META_KEY_PREFIX)));
    let date_version = method == "initialize"
        && params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .is_some_and(is_date_version);
    has_meta_key || date_version
}

fn is_date_version(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn target_name(params: &Value) -> Option<String> {
    params
        .get("name")
        .or_else(|| params.get("uri"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn id_string(id: &Value) -> Option<String> {
    match id {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn headers(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn body(value: Value) -> Vec<u8> {
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn modern_request_classified_from_headers() {
        let h = headers(&[
            ("MCP-Protocol-Version", "2026-07-28"),
            ("Mcp-Method", "tools/call"),
            ("Mcp-Name", "get_weather"),
        ]);
        let b = body(json!({
            "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": {
                "name": "get_weather",
                "arguments": {"location": "Seattle"},
                "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}
            }
        }));

        let info = classify_mcp_request(Some(&h), Some(&b)).unwrap();
        assert_eq!(info.method, "tools/call");
        assert_eq!(info.name.as_deref(), Some("get_weather"));
        assert_eq!(info.protocol_version.as_deref(), Some("2026-07-28"));
        assert_eq!(info.jsonrpc_id.as_deref(), Some("7"));
        assert_eq!(info.session_id, None);
    }

    #[test]
    fn header_path_works_without_a_body() {
        let h = headers(&[("mcp-method", "server/discover")]);
        let info = classify_mcp_request(Some(&h), None).unwrap();
        assert_eq!(info.method, "server/discover");
        assert_eq!(info.jsonrpc_id, None);
    }

    #[test]
    fn base64_mcp_name_is_decoded() {
        let h = headers(&[
            ("Mcp-Method", "resources/read"),
            ("Mcp-Name", "=?base64?SGVsbG8sIOS4lueVjA==?="),
        ]);
        let info = classify_mcp_request(Some(&h), None).unwrap();
        assert_eq!(info.name.as_deref(), Some("Hello, 世界"));
    }

    #[test]
    fn modern_body_without_headers_uses_meta_version() {
        let b = body(json!({
            "jsonrpc": "2.0", "id": "a", "method": "resources/read",
            "params": {
                "uri": "file:///tmp/x",
                "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}
            }
        }));
        let info = classify_mcp_request(None, Some(&b)).unwrap();
        assert_eq!(info.method, "resources/read");
        assert_eq!(info.name.as_deref(), Some("file:///tmp/x"));
        assert_eq!(info.protocol_version.as_deref(), Some("2026-07-28"));
        assert_eq!(info.jsonrpc_id.as_deref(), Some("a"));
    }

    #[test]
    fn legacy_initialize_classified_from_body() {
        let b = body(json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "c", "version": "1"}
            }
        }));
        let info = classify_mcp_request(Some(&headers(&[])), Some(&b)).unwrap();
        assert_eq!(info.method, "initialize");
        assert_eq!(info.protocol_version.as_deref(), Some("2025-11-25"));
        assert_eq!(info.jsonrpc_id.as_deref(), Some("0"));
    }

    #[test]
    fn legacy_tools_call_picks_up_session_and_version_headers() {
        let h = headers(&[
            ("Mcp-Session-Id", "sess-1"),
            ("MCP-Protocol-Version", "2025-06-18"),
        ]);
        let b = body(json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "search", "arguments": {}}
        }));
        let info = classify_mcp_request(Some(&h), Some(&b)).unwrap();
        assert_eq!(info.method, "tools/call");
        assert_eq!(info.name.as_deref(), Some("search"));
        assert_eq!(info.session_id.as_deref(), Some("sess-1"));
        assert_eq!(info.protocol_version.as_deref(), Some("2025-06-18"));
    }

    #[test]
    fn legacy_batch_uses_first_request() {
        let b = body(json!([
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 1, "method": "tools/list"}
        ]));
        let info = classify_mcp_request(None, Some(&b)).unwrap();
        assert_eq!(info.method, "notifications/initialized");
        assert_eq!(info.jsonrpc_id, None);
    }

    #[test]
    fn acp_initialize_is_not_mcp() {
        let b = body(json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": 1, "clientCapabilities": {"fs": {}}}
        }));
        assert_eq!(classify_mcp_request(None, Some(&b)), None);
    }

    #[test]
    fn acp_session_methods_are_not_mcp() {
        for method in [
            "session/new",
            "session/prompt",
            "session/update",
            "fs/read_text_file",
        ] {
            let b = body(json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": {}}));
            assert_eq!(classify_mcp_request(None, Some(&b)), None, "{method}");
        }
    }

    #[test]
    fn lsp_messages_are_not_mcp() {
        let init = body(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"processId": 1, "rootUri": null, "capabilities": {}}
        }));
        assert_eq!(classify_mcp_request(None, Some(&init)), None);

        for method in [
            "initialized",
            "textDocument/didOpen",
            "$/cancelRequest",
            "shutdown",
        ] {
            let b = body(json!({"jsonrpc": "2.0", "method": method, "params": {}}));
            assert_eq!(classify_mcp_request(None, Some(&b)), None, "{method}");
        }
    }

    #[test]
    fn a2a_tasks_without_mcp_marker_are_not_mcp() {
        let b = body(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tasks/get", "params": {"id": "t1"}
        }));
        assert_eq!(classify_mcp_request(None, Some(&b)), None);

        let h = headers(&[("MCP-Protocol-Version", "2026-07-28")]);
        let info = classify_mcp_request(Some(&h), Some(&b)).unwrap();
        assert_eq!(info.method, "tasks/get");
    }

    #[test]
    fn non_json_rpc_bodies_are_not_mcp() {
        let openai = body(json!({"model": "gpt", "messages": [], "method": "tools/call"}));
        assert_eq!(classify_mcp_request(None, Some(&openai)), None);
        assert_eq!(classify_mcp_request(None, Some(b"not json")), None);
        assert_eq!(classify_mcp_request(None, None), None);
    }

    #[test]
    fn json_response_sets_is_error_from_tool_result() {
        let req = McpTrafficInfo {
            method: "tools/call".into(),
            jsonrpc_id: Some("7".into()),
            ..Default::default()
        };
        let ok = body(json!({"jsonrpc": "2.0", "id": 7, "result": {"content": []}}));
        assert_eq!(req.for_response(None, Some(&ok)).is_error, Some(false));

        let tool_err = body(json!({
            "jsonrpc": "2.0", "id": 7, "result": {"content": [], "isError": true}
        }));
        assert_eq!(req.for_response(None, Some(&tool_err)).is_error, Some(true));

        let rpc_err = body(json!({
            "jsonrpc": "2.0", "id": 7,
            "error": {"code": -32022, "message": "Unsupported protocol version"}
        }));
        assert_eq!(req.for_response(None, Some(&rpc_err)).is_error, Some(true));
    }

    #[test]
    fn sse_response_matches_request_id() {
        let req = McpTrafficInfo {
            method: "tools/call".into(),
            jsonrpc_id: Some("2".into()),
            ..Default::default()
        };
        let sse = "event: message\n\
                   data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{}}\n\
                   \n\
                   event: message\n\
                   data: {\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{\"code\":-32602,\"message\":\"bad\"}}\n\
                   \n";
        let info = req.for_response(None, Some(sse.as_bytes()));
        assert_eq!(info.is_error, Some(true));
        assert_eq!(info.method, "tools/call");
    }

    #[test]
    fn legacy_initialize_response_captures_session_and_version() {
        let req = McpTrafficInfo {
            method: "initialize".into(),
            jsonrpc_id: Some("0".into()),
            ..Default::default()
        };
        let h = headers(&[("Mcp-Session-Id", "abc")]);
        let b = body(json!({
            "jsonrpc": "2.0", "id": 0,
            "result": {"protocolVersion": "2025-11-25", "capabilities": {}}
        }));
        let info = req.for_response(Some(&h), Some(&b));
        assert_eq!(info.session_id.as_deref(), Some("abc"));
        assert_eq!(info.protocol_version.as_deref(), Some("2025-11-25"));
        assert_eq!(info.is_error, Some(false));
    }

    #[test]
    fn h2_body_frame_fills_jsonrpc_id() {
        let h = headers(&[("Mcp-Method", "tools/call"), ("Mcp-Name", "search")]);
        let from_headers = classify_mcp_request(Some(&h), None).unwrap();
        let b = body(json!({
            "jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": {
                "name": "search",
                "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}
            }
        }));
        let info = from_headers.with_request_body(&b);
        assert_eq!(info.method, "tools/call");
        assert_eq!(info.name.as_deref(), Some("search"));
        assert_eq!(info.jsonrpc_id.as_deref(), Some("9"));
        assert_eq!(info.protocol_version.as_deref(), Some("2026-07-28"));
        assert_eq!(from_headers.with_request_body(b"{partial"), from_headers);
    }

    #[test]
    fn empty_response_leaves_is_error_unset() {
        let req = McpTrafficInfo {
            method: "notifications/initialized".into(),
            ..Default::default()
        };
        assert_eq!(req.for_response(None, None).is_error, None);
    }
}
