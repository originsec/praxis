//
// System-prompt + message-formatting helpers for AgentChat.
//
// The legacy agent-response action parser (AgentChatAction + parse_line +
// parse_agent_response) was removed during the ACP cut-over. Restoring it
// is part of the follow-up described in agent_chat/mod.rs.
//

/// System prompt template included at compile time.
const SYSTEM_PROMPT_TEMPLATE: &str = include_str!("prompts/system.prompt");

/// Generate the system prompt for an agent joining AgentChat
pub fn generate_system_prompt(
    nickname: &str,
    node_name: &str,
    goal: Option<&str>,
    current_channel: &str,
    topic: Option<&str>,
    other_agents: &[String],
) -> String {
    let goal_text = goal.unwrap_or("No specific goal set - collaborate freely");
    let topic_text = topic.unwrap_or("(no topic set)");
    let others_text = if other_agents.is_empty() {
        "(no other agents)".to_string()
    } else {
        other_agents.join(", ")
    };

    SYSTEM_PROMPT_TEMPLATE
        .replace("{nickname}", nickname)
        .replace("{node_name}", node_name)
        .replace("{goal}", goal_text)
        .replace("{channel}", current_channel)
        .replace("{topic}", topic_text)
        .replace("{others}", &others_text)
}

/// Format messages for delivery to an agent
pub fn format_message_delivery(
    channel_messages: &[(String, String, String)],
    direct_messages: &[(String, String, String)],
) -> String {
    let mut output = String::new();

    if !channel_messages.is_empty() {
        output.push_str("--- NEW MESSAGES ---\n");
        for (timestamp, sender, content) in channel_messages {
            output.push_str(&format!("[{}] <{}> {}\n", timestamp, sender, content));
        }
    }

    if !direct_messages.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("--- DIRECT MESSAGES ---\n");
        for (timestamp, sender, content) in direct_messages {
            output.push_str(&format!(
                "[{}] DM from <{}>: {}\n",
                timestamp, sender, content
            ));
        }
    }

    if !output.is_empty() {
        output.push_str("\n---\nRespond with message(s) and/or commands. Use /wait to pause.\n");
    }

    output
}
