//
// Generic session mode infrastructure for agent connectors. These modules
// provide reusable DevTools and UIAutomation session implementations that
// agent-specific code can use via adapter traits.
//

#[cfg(windows)]
pub mod devtools;
#[cfg(windows)]
pub mod uiautomation;
