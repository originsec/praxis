use super::types::Tool;

//
// Include prompt files at compile time.
//
const TOOL_CALLING_PROMPT: &str = include_str!("../prompts/tool_calling.prompt");
const TASK_COMPLETION_PROMPT: &str = include_str!("../prompts/task_completion.prompt");

/// Extend a base system prompt with tool documentation and calling instructions
///
/// This instructs the AI to output tool calls in a specific JSON format that we can parse,
/// working around native function calling API limitations.
///
/// If include_completion_instructions is true, also adds instructions for signaling task completion.
pub fn get_system_prompt_with_tools_impl(
    base_prompt: &str,
    tools: &[Tool],
    include_completion_instructions: bool,
) -> String {
    let mut prompt = base_prompt.to_string();
    prompt.push_str("\n\n## Available Tools\n\n");
    prompt.push_str("You have access to the following tools:\n\n");

    for tool in tools {
        prompt.push_str(&format!("### {}\n", tool.name));
        if let Some(desc) = &tool.description {
            prompt.push_str(&format!("{}\n\n", desc));
        }
        if let Some(params) = &tool.parameters {
            prompt.push_str(&format!("Parameters: {}\n\n", params));
        }
    }

    prompt.push('\n');
    prompt.push_str(TOOL_CALLING_PROMPT);

    if include_completion_instructions {
        prompt.push('\n');
        prompt.push_str(TASK_COMPLETION_PROMPT);
    }

    prompt
}

/// Extend a base system prompt with tool documentation and calling instructions
///
/// This is a convenience wrapper that doesn't include completion instructions.
/// Use this for agents that don't need to signal task completion.
pub fn get_system_prompt_with_tools(base_prompt: &str, tools: &[Tool]) -> String {
    get_system_prompt_with_tools_impl(base_prompt, tools, false)
}

/// Extend a base system prompt with tool documentation, calling instructions, and completion signaling
///
/// Use this variant for agents that need to signal when their task is complete.
/// This is particularly useful for autonomous agents that execute multi-step workflows.
pub fn get_system_prompt_with_tools_and_completion(base_prompt: &str, tools: &[Tool]) -> String {
    get_system_prompt_with_tools_impl(base_prompt, tools, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_without_completion() {
        let base = "You are a helpful assistant.";
        let tools = vec![];
        let prompt = get_system_prompt_with_tools(base, &tools);

        assert!(prompt.contains(base));
        assert!(prompt.contains("## Available Tools"));
        assert!(prompt.contains("## Tool Calling Format"));
        assert!(!prompt.contains("## Task Completion"));
    }

    #[test]
    fn test_prompt_with_completion() {
        let base = "You are a helpful assistant.";
        let tools = vec![];
        let prompt = get_system_prompt_with_tools_and_completion(base, &tools);

        assert!(prompt.contains(base));
        assert!(prompt.contains("## Available Tools"));
        assert!(prompt.contains("## Tool Calling Format"));
        assert!(prompt.contains("## Task Completion"));
        assert!(prompt.contains("Signal completion"));
    }

    #[test]
    fn test_prompt_includes_tool_info() {
        let base = "You are a helpful assistant.";
        let tool = Tool {
            name: "test_tool".to_string(),
            description: Some("A test tool".to_string()),
            parameters: Some(serde_json::json!({})),
        };
        let tools = vec![tool];
        let prompt = get_system_prompt_with_tools(base, &tools);

        assert!(prompt.contains("test_tool"));
        assert!(prompt.contains("A test tool"));
    }
}
