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
