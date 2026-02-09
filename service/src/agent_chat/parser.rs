//
// Parser for agent responses in AgentChat chat.
//
// Parses IRC-style commands and messages from agent output.
//

/// System prompt template included at compile time.
const SYSTEM_PROMPT_TEMPLATE: &str = include_str!("prompts/system.prompt");

/// Parsed action from agent response
#[derive(Debug, Clone)]
pub enum AgentChatAction {
    /// Join or create a channel
    JoinChannel { channel_name: String },
    /// Leave the current channel
    LeaveChannel,
    /// Set the topic of the current channel
    SetTopic { topic: String },
    /// List all channels
    ListChannels,
    /// Send a direct message to another agent
    DirectMessage { nickname: String, message: String },
    /// Wait for more input before responding
    Wait,
    /// Send a message to the current channel
    SendMessage { content: String },
}

/// Parse a single line from agent output
fn parse_line(line: &str) -> Option<AgentChatAction> {
    let trimmed = line.trim();

    //
    // Skip empty lines.
    //
    if trimmed.is_empty() {
        return None;
    }

    //
    // Check for commands (start with /).
    //
    if trimmed.starts_with('/') {
        let parts: Vec<&str> = trimmed[1..].splitn(2, ' ').collect();
        let command = parts[0].to_lowercase();
        let args = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match command.as_str() {
            "join" => {
                if !args.is_empty() {
                    //
                    // Ensure channel name starts with #.
                    //
                    let channel_name = if args.starts_with('#') {
                        args.to_string()
                    } else {
                        format!("#{}", args)
                    };
                    return Some(AgentChatAction::JoinChannel { channel_name });
                }
            }
            "leave" | "part" => {
                return Some(AgentChatAction::LeaveChannel);
            }
            "topic" => {
                if !args.is_empty() {
                    return Some(AgentChatAction::SetTopic { topic: args.to_string() });
                }
            }
            "channels" | "list" => {
                return Some(AgentChatAction::ListChannels);
            }
            "dm" | "msg" | "privmsg" => {
                //
                // Format: /dm <nickname> <message>
                //
                let dm_parts: Vec<&str> = args.splitn(2, ' ').collect();
                if dm_parts.len() == 2 {
                    return Some(AgentChatAction::DirectMessage {
                        nickname: dm_parts[0].to_string(),
                        message: dm_parts[1].to_string(),
                    });
                }
            }
            "wait" => {
                //
                // Only recognize /wait as a command if it's alone on the line.
                // This prevents "/wait" embedded in prose from triggering wait.
                //
                if args.is_empty() {
                    return Some(AgentChatAction::Wait);
                }
            }
            _ => {
                //
                // Unknown command, treat as message.
                //
            }
        }
    }

    //
    // Regular message to current channel.
    //
    Some(AgentChatAction::SendMessage {
        content: trimmed.to_string(),
    })
}

/// Check if accumulated message ends with sentence-ending punctuation
fn ends_with_sentence_end(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?')
        || trimmed.ends_with(':') || trimmed.is_empty()
}

/// Parse agent response into a list of actions
pub fn parse_agent_response(response: &str) -> Vec<AgentChatAction> {
    let mut actions = Vec::new();
    let mut current_message = String::new();

    for line in response.lines() {
        if let Some(action) = parse_line(line) {
            match action {
                AgentChatAction::SendMessage { content } => {
                    //
                    // Accumulate consecutive message lines.
                    //
                    if !current_message.is_empty() {
                        current_message.push('\n');
                    }
                    current_message.push_str(&content);
                }
                AgentChatAction::Wait => {
                    //
                    // Only treat /wait as a command if it's at the start or
                    // after a complete sentence. Otherwise it's likely embedded
                    // in prose like "we can /wait for someone".
                    //
                    if ends_with_sentence_end(&current_message) {
                        if !current_message.is_empty() {
                            actions.push(AgentChatAction::SendMessage {
                                content: std::mem::take(&mut current_message),
                            });
                        }
                        actions.push(AgentChatAction::Wait);
                    } else {
                        //
                        // Treat as prose - append to current message.
                        //
                        if !current_message.is_empty() {
                            current_message.push('\n');
                        }
                        current_message.push_str("/wait");
                    }
                }
                other => {
                    //
                    // Flush accumulated message before adding other action.
                    //
                    if !current_message.is_empty() {
                        actions.push(AgentChatAction::SendMessage {
                            content: std::mem::take(&mut current_message),
                        });
                    }
                    actions.push(other);
                }
            }
        }
    }

    //
    // Flush any remaining message.
    //
    if !current_message.is_empty() {
        actions.push(AgentChatAction::SendMessage {
            content: current_message,
        });
    }

    actions
}

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
            output.push_str(&format!("[{}] DM from <{}>: {}\n", timestamp, sender, content));
        }
    }

    if !output.is_empty() {
        output.push_str("\n---\nRespond with message(s) and/or commands. Use /wait to pause.\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_join_command() {
        let actions = parse_agent_response("/join #general");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            AgentChatAction::JoinChannel { channel_name } => {
                assert_eq!(channel_name, "#general");
            }
            _ => panic!("Expected JoinChannel action"),
        }
    }

    #[test]
    fn test_parse_join_without_hash() {
        let actions = parse_agent_response("/join planning");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            AgentChatAction::JoinChannel { channel_name } => {
                assert_eq!(channel_name, "#planning");
            }
            _ => panic!("Expected JoinChannel action"),
        }
    }

    #[test]
    fn test_parse_dm_command() {
        let actions = parse_agent_response("/dm alice Hello there!");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            AgentChatAction::DirectMessage { nickname, message } => {
                assert_eq!(nickname, "alice");
                assert_eq!(message, "Hello there!");
            }
            _ => panic!("Expected DirectMessage action"),
        }
    }

    #[test]
    fn test_parse_mixed_response() {
        let response = r#"Hello everyone!
I'm here to help.
/join #planning
Let me check that out."#;

        let actions = parse_agent_response(response);
        assert_eq!(actions.len(), 3);

        match &actions[0] {
            AgentChatAction::SendMessage { content } => {
                assert!(content.contains("Hello everyone!"));
                assert!(content.contains("I'm here to help."));
            }
            _ => panic!("Expected SendMessage action"),
        }

        match &actions[1] {
            AgentChatAction::JoinChannel { channel_name } => {
                assert_eq!(channel_name, "#planning");
            }
            _ => panic!("Expected JoinChannel action"),
        }

        match &actions[2] {
            AgentChatAction::SendMessage { content } => {
                assert_eq!(content, "Let me check that out.");
            }
            _ => panic!("Expected SendMessage action"),
        }
    }

    #[test]
    fn test_parse_wait_command() {
        let actions = parse_agent_response("/wait");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            AgentChatAction::Wait => {}
            _ => panic!("Expected Wait action"),
        }
    }

    #[test]
    fn test_wait_after_sentence() {
        let actions = parse_agent_response("I'll wait for more input.\n/wait");
        assert_eq!(actions.len(), 2);
        match &actions[0] {
            AgentChatAction::SendMessage { content } => {
                assert_eq!(content, "I'll wait for more input.");
            }
            _ => panic!("Expected SendMessage action"),
        }
        match &actions[1] {
            AgentChatAction::Wait => {}
            _ => panic!("Expected Wait action"),
        }
    }

    #[test]
    fn test_wait_embedded_in_sentence() {
        //
        // /wait in the middle of a sentence should be treated as prose.
        //
        let actions = parse_agent_response("We can\n/wait\nfor someone to bring us a task");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            AgentChatAction::SendMessage { content } => {
                assert!(content.contains("/wait"));
                assert!(content.contains("We can"));
            }
            _ => panic!("Expected SendMessage action"),
        }
    }
}
