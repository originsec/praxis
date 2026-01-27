use super::types::Tool;

/// Extend a base system prompt with tool documentation and calling instructions
///
/// This instructs the AI to output tool calls in a specific JSON format that we can parse,
/// working around native function calling API limitations.
///
/// If include_completion_instructions is true, also adds instructions for signaling task completion.
///
/// # Arguments
///
/// * `base_prompt` - The base system prompt to extend
/// * `tools` - Array of Tool definitions to document
/// * `include_completion_instructions` - Whether to include task completion signaling instructions
///
/// # Returns
///
/// Extended system prompt with tool documentation and calling instructions
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

    prompt.push_str("\n## Tool Calling Format\n\n");
    prompt.push_str("To call a tool, output a JSON code block in this exact format:\n\n");
    prompt.push_str("```json\n{\"tool\": \"tool_name\", \"args\": {\"param1\": \"value1\"}}\n```\n\n");
    prompt.push_str("CRITICAL RULES - YOU MUST FOLLOW THESE EXACTLY:\n\n");
    prompt.push_str("1. OUTPUT ONLY ONE TOOL CALL PER MESSAGE - Never output multiple tool calls in the same response\n");
    prompt.push_str("2. STOP IMMEDIATELY after outputting a tool call JSON block - do not write anything after it\n");
    prompt.push_str("3. NEVER simulate, guess, or hallucinate tool results - you do not know what the tool will return\n");
    prompt.push_str("4. WAIT for the actual tool result in the next message before continuing\n");
    prompt.push_str("5. You may include brief explanatory text BEFORE the tool call, but NOTHING after it\n\n");
    prompt.push_str("WRONG (do not do this):\n");
    prompt.push_str("```json\n{\"tool\": \"session_prompt\", \"args\": {\"text\": \"hello\"}}\n```\n");
    prompt.push_str("The agent responded with... [WRONG - you cannot know the response yet!]\n\n");
    prompt.push_str("CORRECT:\n");
    prompt.push_str("I will ask the agent for information.\n");
    prompt.push_str("```json\n{\"tool\": \"session_prompt\", \"args\": {\"text\": \"hello\"}}\n```\n");
    prompt.push_str("[STOP HERE - wait for result]\n\n");

    if include_completion_instructions {
        prompt.push_str("\n## Task Completion\n\n");
        prompt.push_str("When you have completed the task and have gathered all necessary information, ");
        prompt.push_str("signal completion by outputting this exact JSON block:\n\n");
        prompt.push_str("```json\n{\"complete\": true, \"summary\": \"Brief summary of what was accomplished\"}\n```\n\n");
        prompt.push_str("IMPORTANT:\n");
        prompt.push_str("- Only signal completion when the task is truly finished\n");
        prompt.push_str("- The summary should be a concise description of what was accomplished\n");
        prompt.push_str("- Do NOT signal completion if you need to call more tools or gather more information\n");
        prompt.push_str("- Signal completion as soon as the task objective is met\n\n");
    }

    prompt
}

/// Extend a base system prompt with tool documentation and calling instructions
///
/// This is a convenience wrapper that doesn't include completion instructions.
/// Use this for agents that don't need to signal task completion.
///
/// # Examples
///
/// ```
/// use common::ai::{Tool, get_system_prompt_with_tools};
///
/// let base = "You are a helpful assistant.";
/// let tools = vec![]; // Your tools here
/// let prompt = get_system_prompt_with_tools(base, &tools);
/// ```
pub fn get_system_prompt_with_tools(base_prompt: &str, tools: &[Tool]) -> String {
    get_system_prompt_with_tools_impl(base_prompt, tools, false)
}

/// Extend a base system prompt with tool documentation, calling instructions, and completion signaling
///
/// Use this variant for agents that need to signal when their task is complete.
/// This is particularly useful for autonomous agents that execute multi-step workflows.
///
/// # Examples
///
/// ```
/// use common::ai::{Tool, get_system_prompt_with_tools_and_completion};
///
/// let base = "You are an autonomous agent that performs tasks.";
/// let tools = vec![]; // Your tools here
/// let prompt = get_system_prompt_with_tools_and_completion(base, &tools);
/// ```
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
        assert!(prompt.contains("signal completion"));
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
