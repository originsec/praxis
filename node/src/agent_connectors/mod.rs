#[cfg(not(windows))]
#[allow(dead_code)]
pub mod clawdbot;
pub mod claudecode;
#[cfg(any(target_os = "linux", windows))]
pub mod codex;
#[cfg(target_os = "linux")]
pub mod cursor;
pub mod dummy;
pub mod lua;
#[cfg(windows)]
pub mod m365copilot;
#[cfg(windows)]
pub mod modes;

mod factory;
mod registry;
mod traits;
pub mod utils;

#[allow(unused_imports)]
pub use common::{AgentTool, McpServer, McpTransport, ReconResult, ReconTools};

#[cfg(not(windows))]
#[allow(unused_imports)]
pub use clawdbot::ClawdbotAgent;
#[allow(unused_imports)]
pub use claudecode::ClaudeCodeAgent;
#[cfg(any(target_os = "linux", windows))]
#[allow(unused_imports)]
pub use codex::CodexAgent;
#[cfg(target_os = "linux")]
#[allow(unused_imports)]
pub use cursor::CursorAgent;
#[allow(unused_imports)]
pub use dummy::DummyAgent;
#[allow(unused_imports)]
pub use lua::LuaAgent;
#[cfg(windows)]
#[allow(unused_imports)]
pub use m365copilot::M365CopilotAgent;

pub use factory::AgentFactory;
pub use registry::AgentRegistry;
pub use traits::Agent;
#[allow(unused_imports)]
pub use traits::{AgentIntercept, AgentMode, AgentSession};
