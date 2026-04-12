pub mod messaging;
pub mod ai;
pub mod config;
pub mod logging;
pub mod mcp;

pub use messaging::*;
pub use logging::{init as init_logging, send_event, is_initialized as is_logging_initialized};

pub use ai::{
    Provider,
    Role,
    create_ai_client,
    execute_chat_completion,
    build_message,
    AiResponse,
    execute_with_tool_parsing,
    parse_manual_tool_call,
    parse_completion_signal,
    get_system_prompt_with_tools,
    get_system_prompt_with_tools_and_completion,
};

pub use config::{FileConfig, find_config_file, load_from_paths};

pub use mcp::{McpClient, PraxisServer, run_stdio_server};

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
