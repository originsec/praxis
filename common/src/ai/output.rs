/// Prefix for outgoing prompts/requests to agents
pub const PREFIX_OUTGOING: &str = ">>>";

/// Prefix for incoming responses from agents
pub const PREFIX_INCOMING: &str = "<<<";

/// Prefix for error messages
pub const PREFIX_ERROR: &str = "!!!";

/// Prefix for section headers
pub const PREFIX_SECTION: &str = "===";

/// Prefix for iteration markers
pub const PREFIX_ITERATION: &str = "---";

/// Format an outgoing prompt message
pub fn fmt_outgoing(label: &str, content: &str) -> String {
    format!("{} {}:\n{}\n\n", PREFIX_OUTGOING, label, content)
}

/// Format an incoming response message
pub fn fmt_incoming(label: &str, content: &str) -> String {
    format!("{} {}:\n{}\n\n", PREFIX_INCOMING, label, content)
}

/// Format an error message
pub fn fmt_error(message: &str) -> String {
    format!("{} {}\n\n", PREFIX_ERROR, message)
}

/// Format a section header
pub fn fmt_section(title: &str) -> String {
    format!("{} {} {}\n", PREFIX_SECTION, title, PREFIX_SECTION)
}

/// Format an iteration marker
pub fn fmt_iteration(current: usize, total: usize) -> String {
    format!("{} Iteration {}/{} {}\n", PREFIX_ITERATION, current, total, PREFIX_ITERATION)
}

/// Format the agent mode start header
pub fn fmt_agent_start(provider: &str, model: &str, max_iterations: usize) -> String {
    format!(
        "{} Starting Agent Mode {}\nProvider: {}\nModel: {}\nMax iterations: {}\n\n",
        PREFIX_SECTION, PREFIX_SECTION, provider, model, max_iterations
    )
}

/// Format the operation complete message
pub fn fmt_complete(summary: &str) -> String {
    format!(
        "{} Operation Complete {}\nSummary: {}\n\n",
        PREFIX_SECTION, PREFIX_SECTION, summary
    )
}

/// Determine the output line type from its prefix
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputLineType {
    Outgoing,
    Incoming,
    Error,
    Section,
    Iteration,
    Regular,
}

impl OutputLineType {
    /// Detect the line type from its content
    pub fn detect(line: &str) -> Self {
        if line.starts_with(PREFIX_OUTGOING) {
            Self::Outgoing
        } else if line.starts_with(PREFIX_INCOMING) {
            Self::Incoming
        } else if line.starts_with(PREFIX_ERROR) {
            Self::Error
        } else if line.starts_with(PREFIX_SECTION) {
            Self::Section
        } else if line.starts_with(PREFIX_ITERATION) {
            Self::Iteration
        } else {
            Self::Regular
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fmt_outgoing() {
        let result = fmt_outgoing("Sending prompt", "Hello world");
        assert!(result.starts_with(PREFIX_OUTGOING));
        assert!(result.contains("Sending prompt"));
        assert!(result.contains("Hello world"));
    }

    #[test]
    fn test_fmt_incoming() {
        let result = fmt_incoming("Agent response", "Hi there");
        assert!(result.starts_with(PREFIX_INCOMING));
        assert!(result.contains("Agent response"));
        assert!(result.contains("Hi there"));
    }

    #[test]
    fn test_fmt_error() {
        let result = fmt_error("Something went wrong");
        assert!(result.starts_with(PREFIX_ERROR));
        assert!(result.contains("Something went wrong"));
    }

    #[test]
    fn test_line_type_detection() {
        assert_eq!(OutputLineType::detect(">>> Sending"), OutputLineType::Outgoing);
        assert_eq!(OutputLineType::detect("<<< Response"), OutputLineType::Incoming);
        assert_eq!(OutputLineType::detect("!!! Error"), OutputLineType::Error);
        assert_eq!(OutputLineType::detect("=== Header ==="), OutputLineType::Section);
        assert_eq!(OutputLineType::detect("--- Iteration ---"), OutputLineType::Iteration);
        assert_eq!(OutputLineType::detect("Regular text"), OutputLineType::Regular);
    }
}
