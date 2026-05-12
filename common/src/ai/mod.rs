pub mod client;
pub mod models;
pub mod output;
pub mod parsing;
pub mod prompts;
pub mod provider;
pub mod providers;
pub mod types;

pub use provider::Provider;

pub use types::{
    AiResponse, ChatCompletionChoice, ChatCompletionDelta, ChatCompletionRequest,
    ChatCompletionResponse, Content, Message, Role, Tool, Usage,
};

pub use client::{
    AiClient, build_message, create_ai_client, execute_chat_completion, execute_with_tool_parsing,
};

pub use parsing::{parse_completion_signal, parse_manual_tool_call};

pub use prompts::{get_system_prompt_with_tools, get_system_prompt_with_tools_and_completion};

pub use output::{
    OutputLineType, fmt_agent_start, fmt_complete, fmt_error, fmt_incoming, fmt_iteration,
    fmt_outgoing, fmt_section,
};

pub use models::{fetch_models_for_provider, probe_openai_compatible_endpoint};
