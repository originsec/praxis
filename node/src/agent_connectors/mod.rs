#[cfg(not(windows))]
pub mod clawdbot;
pub mod claudecode;
pub mod dummy;
pub mod dynamic;
pub mod gemini;
#[cfg(windows)]
pub mod m365copilot;
#[cfg(windows)]
pub mod modes;

mod factory;
mod registry;
mod traits;
pub mod utils;

#[allow(unused_imports)]
pub use common::{AgentTool, McpServer, McpTransport, ReconConfig, ReconResult, ReconTools};

#[cfg(not(windows))]
#[allow(unused_imports)]
pub use clawdbot::ClawdbotAgent;
#[allow(unused_imports)]
pub use claudecode::ClaudeCodeAgent;
#[allow(unused_imports)]
pub use dummy::DummyAgent;
#[allow(unused_imports)]
pub use dynamic::DynamicAgent;
#[allow(unused_imports)]
pub use gemini::GeminiAgent;
#[cfg(windows)]
#[allow(unused_imports)]
pub use m365copilot::M365CopilotAgent;

pub use factory::AgentFactory;
pub use registry::AgentRegistry;
pub use traits::Agent;
#[allow(unused_imports)]
pub use traits::{AgentIntercept, AgentMode, AgentSession};
