pub mod acp_ext;
pub mod ai;
pub mod clear_epoch;
pub mod client_transport;
pub mod config;
pub mod id;
pub mod intercept_match;
pub mod log_query_schema;
pub mod logging;
pub mod mcp;
pub mod messaging;
pub mod remote_nodes;

pub use logging::{init as init_logging, is_initialized as is_logging_initialized, send_event};
pub use messaging::*;
pub use intercept_match::{pattern_matches_entry, rule_matches_entry};

pub use ai::{
    AiResponse, Provider, Role, build_message, create_ai_client, execute_chat_completion,
    execute_with_tool_parsing, get_system_prompt_with_tools,
    get_system_prompt_with_tools_and_completion, parse_completion_signal, parse_manual_tool_call,
    parse_manual_tool_calls,
};

pub use config::{FileConfig, find_config_file, load_from_paths};

pub use client_transport::ClientTransport;

pub use id::short_id;

pub use mcp::{McpClient, PraxisServer, run_stdio_server};

pub use remote_nodes::{REMOTE_NODE_KINDS, RemoteNodeKindInfo};

/// Truncate a string to at most `max_bytes` without panicking on multibyte
/// character boundaries. Rounds down to the nearest char boundary.
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
