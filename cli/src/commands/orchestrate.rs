use anyhow::Result;
use colored::Colorize;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use rustyline::error::ReadlineError;
use std::io::{IsTerminal, Write};
use tokio::sync::mpsc;

use common::{ClientDirectMessage, OrchestratorPlan, PlanStepStatus};

use crate::client::CliClient;
use crate::spinner::Spinner;

//
// Raw-mode-safe println: uses \r\n so output renders correctly regardless of
// the terminal OPOST setting. Harmless in cooked mode (\r before \r\n is
// redundant but invisible).
//

macro_rules! rprintln {
    () => {{
        print!("\r\n");
        let _ = std::io::stdout().flush();
    }};
    ($($arg:tt)*) => {{
        print!("{}\r\n", format!($($arg)*));
        let _ = std::io::stdout().flush();
    }};
}

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> Option<Self> {
        crossterm::terminal::enable_raw_mode().ok().map(|_| Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

pub async fn execute(client: &mut CliClient) -> Result<()> {
    let mut event_rx = client.subscribe_orchestrator_events();

    client.start_orchestrator().await?;

    let started = wait_for_started(&mut event_rx).await;
    if !started {
        client.unsubscribe_orchestrator_events().await;
        return Ok(());
    }

    let mut expanded = false;

    println!(
        "  {}",
        "Type your prompt, Ctrl+C to cancel, Ctrl+O to toggle tool details, Ctrl+D to exit"
            .dimmed()
    );
    println!();

    let plain_prompt = "  ▸ ".to_string();
    let colored_prompt = format!("  {} ", "▸".bold());
    let (mut rl, _) = crate::prompt::editor_with_colored_prompt(&plain_prompt, colored_prompt)?;
    let mut prompt_seq: u64 = 0;

    loop {
        let line = rl.readline(&plain_prompt);

        match line {
            Ok(input) => {
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(trimmed);

                prompt_seq += 1;
                let prompt_id = format!("{}", prompt_seq);
                client.send_orchestrator_prompt(prompt_id.clone(), trimmed.to_string()).await?;

                process_events_until_done(client, &mut event_rx, &mut expanded, &prompt_id).await;
            }
            Err(ReadlineError::Interrupted) => {
                break;
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(e) => {
                eprintln!("  {} Input error: {}", "✗".red(), e);
                break;
            }
        }
    }

    client.stop_orchestrator().await?;
    client.unsubscribe_orchestrator_events().await;
    println!();
    println!("  {} {}", "●".dimmed(), "Orchestrator session ended".dimmed());
    println!();

    Ok(())
}

async fn wait_for_started(event_rx: &mut mpsc::UnboundedReceiver<ClientDirectMessage>) -> bool {
    let timeout = tokio::time::Duration::from_secs(30);
    match tokio::time::timeout(timeout, event_rx.recv()).await {
        Ok(Some(ClientDirectMessage::OrchestratorStarted { provider, model })) => {
            println!(
                "  {} {} {}",
                "●".green(),
                "Orchestrator session started".bold(),
                format!("({}::{})", provider, model).dimmed()
            );
            true
        }
        Ok(Some(ClientDirectMessage::OrchestratorError { message, .. })) => {
            eprintln!("  {} {}", "✗".red(), message);
            false
        }
        Ok(Some(_)) => true,
        Ok(None) => {
            eprintln!("  {} Event channel closed unexpectedly", "✗".red());
            false
        }
        Err(_) => {
            eprintln!("  {} Timed out waiting for orchestrator to start", "✗".red());
            false
        }
    }
}

async fn process_events_until_done(
    client: &CliClient,
    event_rx: &mut mpsc::UnboundedReceiver<ClientDirectMessage>,
    expanded: &mut bool,
    expected_prompt_id: &str,
) {
    let mut spinner: Option<Spinner> = None;
    let mut cursor: Option<Spinner> = Some(Spinner::start_cursor());
    let mut accumulated_content = String::new();
    let mut pending_thinking = String::new();
    let mut tool_calls: Vec<(String, bool)> = Vec::new();
    let mut total_prompt_tokens: u32 = 0;
    let mut total_completion_tokens: u32 = 0;
    let mut total_tokens: u32 = 0;
    let mut output_lines: usize = 0;
    let mut current_plan: Option<OrchestratorPlan> = None;
    let mut current_tool: Option<String> = None;
    let mut has_token_line = false;

    //
    // Enable raw mode for key event detection (Ctrl+O toggle, Ctrl+C
    // cancel). Falls back to signal-based Ctrl+C when not a TTY.
    //

    let _raw_guard = if std::io::stdin().is_terminal() {
        RawModeGuard::enable()
    } else {
        None
    };
    let interactive = _raw_guard.is_some();

    let mut term_events = if interactive {
        Some(EventStream::new())
    } else {
        None
    };

    loop {
        let key_event = async {
            match &mut term_events {
                Some(s) => s.next().await,
                None => std::future::pending().await,
            }
        };

        tokio::select! {
            event = event_rx.recv() => {
                let Some(event) = event else { break };

                //
                // Discard events from a different prompt (stale/cancelled).
                //

                match &event {
                    ClientDirectMessage::OrchestratorContent { prompt_id, .. }
                    | ClientDirectMessage::OrchestratorToolExecuting { prompt_id, .. }
                    | ClientDirectMessage::OrchestratorToolExecuted { prompt_id, .. }
                    | ClientDirectMessage::OrchestratorPlanUpdated { prompt_id, .. }
                    | ClientDirectMessage::OrchestratorDone { prompt_id }
                    | ClientDirectMessage::OrchestratorError { prompt_id, .. }
                    | ClientDirectMessage::OrchestratorTokenUsage { prompt_id, .. }
                        if prompt_id != expected_prompt_id => continue,
                    _ => {}
                }

                if let Some(c) = cursor.take() {
                    c.finish().await;
                }

                match event {
                    ClientDirectMessage::OrchestratorContent { content, .. } => {
                        if let Some(s) = spinner.take() {
                            s.finish().await;
                        }
                        current_tool = None;
                        accumulated_content.push_str(&content);

                        // Accumulate thinking across chunks and display when complete
                        let start_tag = "<think>";
                        let end_tag = "</think>";

                        // Build the string to process: pending_thinking + new content
                        let to_process = if !pending_thinking.is_empty() {
                            let combined = format!("{}{}", pending_thinking, content);
                            pending_thinking.clear();
                            combined
                        } else {
                            content.clone()
                        };

                        let mut remaining: String = to_process;

                        loop {
                            // Find the first opening tag
                            let start_match = remaining.find(start_tag);
                            // Find the first closing tag
                            let end_match = remaining.find(end_tag);

                            match (start_match, end_match) {
                                // Both tags found - check if opening comes first
                                (Some(start), Some(end)) if start < end => {
                                    // Get content between tags and display
                                    let thinking_block = remaining[start + start_tag.len()..end].trim();
                                    if !thinking_block.is_empty() {
                                        render_thinking(thinking_block);
                                    }
                                    remaining = remaining[end + end_tag.len()..].to_string();
                                }
                                // Opening tag found but no closing yet - accumulate
                                (Some(_), None) => {
                                    pending_thinking = remaining;
                                    break;
                                }
                                // No opening tag or closing comes first - done
                                _ => {
                                    break;
                                }
                            }
                        }
                    }
                    ClientDirectMessage::OrchestratorToolExecuting { name, input: _, .. } => {
                        //
                        // Hide report_plan — the plan is shown via
                        // OrchestratorPlanUpdated.
                        //
                        if name != "report_plan" {
                            if let Some(s) = spinner.take() {
                                s.finish().await;
                            }
                            print!("\r\x1B[2K");
                            let _ = std::io::stdout().flush();

                            current_tool = Some(name.clone());

                            let label = spinner_label(&name, tool_calls.len(), *expanded);
                            spinner = Some(Spinner::start_with_elapsed(&label));
                        }
                    }
                    ClientDirectMessage::OrchestratorToolExecuted { name, success, .. } => {
                        if name != "report_plan" {
                            if let Some(s) = spinner.take() {
                                s.finish().await;
                            }
                            current_tool = None;

                            if *expanded {
                                let icon = if success { "\u{2713}".green() } else { "\u{2717}".red() };
                                rprintln!("  {} {}", icon, name);
                                output_lines += 1;
                            }

                            tool_calls.push((name, success));
                        }
                    }
                    ClientDirectMessage::OrchestratorPlanUpdated { plan, .. } => {
                        if let Some(s) = spinner.take() {
                            s.finish().await;
                        }
                        print!("\r\x1B[2K");
                        let _ = std::io::stdout().flush();

                        //
                        // Clear previous output and re-render with the
                        // updated plan.
                        //

                        clear_output(output_lines);
                        output_lines = 0;

                        if has_token_line {
                            render_token_line(total_prompt_tokens, total_completion_tokens, total_tokens);
                            output_lines += 1;
                        }

                        current_plan = Some(plan);
                        output_lines += render_plan(current_plan.as_ref().unwrap());

                        if *expanded {
                            output_lines += render_expanded_tool_calls(&tool_calls);
                        }

                        if let Some(ref name) = current_tool {
                            let label = spinner_label(name, tool_calls.len(), *expanded);
                            spinner = Some(Spinner::start_with_elapsed(&label));
                        }
                    }
                    ClientDirectMessage::OrchestratorTokenUsage { prompt_tokens, completion_tokens, total_tokens: batch_total, .. } => {
                        total_prompt_tokens += prompt_tokens;
                        total_completion_tokens += completion_tokens;
                        total_tokens += batch_total;

                        if let Some(s) = spinner.take() {
                            s.finish().await;
                        }

                        if !has_token_line {
                            render_token_line(total_prompt_tokens, total_completion_tokens, total_tokens);
                            output_lines += 1;
                            has_token_line = true;
                        } else {
                            //
                            // Navigate up to the token line (first tracked
                            // line), update in-place, then return.
                            //

                            print!("\r\x1B[2K");
                            if output_lines > 0 {
                                print!("\x1B[{}A", output_lines);
                            }
                            print!("\r\x1B[2K  {}", format_token_usage(total_prompt_tokens, total_completion_tokens, total_tokens).dimmed());
                            if output_lines > 0 {
                                print!("\x1B[{}B", output_lines);
                            }
                            print!("\r");
                            let _ = std::io::stdout().flush();
                        }

                        if let Some(ref name) = current_tool {
                            let label = spinner_label(name, tool_calls.len(), *expanded);
                            spinner = Some(Spinner::start_with_elapsed(&label));
                        }
                    }
                    ClientDirectMessage::OrchestratorError { message, .. } => {
                        if let Some(s) = spinner.take() {
                            s.finish().await;
                        }
                        rprintln!("  {} {}", "\u{2717}".red(), message);
                        output_lines += 1;
                    }
                    ClientDirectMessage::OrchestratorDone { .. } => {
                        if let Some(s) = spinner.take() {
                            s.finish().await;
                        }

                        if !tool_calls.is_empty() {
                            if *expanded {
                                let total = tool_calls.len();
                                let label = if total == 1 { "tool call" } else { "tool calls" };
                                rprintln!("  {} {} {}", "\u{2500}\u{2500}".dimmed(), total, label);
                            } else {
                                render_tool_summary(&tool_calls);
                            }
                            tool_calls.clear();
                        }

                        if !accumulated_content.trim().is_empty() {
                            rprintln!();

                            if !pending_thinking.is_empty() {
                                let trimmed = pending_thinking.trim();
                                if !trimmed.is_empty() {
                                    render_thinking(trimmed);
                                }
                                pending_thinking.clear();
                            }

                            let response = strip_thinking(&accumulated_content);

                            if !response.trim().is_empty() {
                                render_markdown(&response);
                            }
                            rprintln!();
                        }

                        accumulated_content.clear();
                        break;
                    }
                    ClientDirectMessage::OrchestratorStopped => {
                        if let Some(s) = spinner.take() {
                            s.finish().await;
                        }

                        if !pending_thinking.is_empty() {
                            let trimmed = pending_thinking.trim();
                            if !trimmed.is_empty() {
                                render_thinking(trimmed);
                            }
                            pending_thinking.clear();
                        }

                        break;
                    }
                    _ => {}
                }
            }

            Some(Ok(term_event)) = key_event, if interactive => {
                if let Event::Key(KeyEvent {
                    code, modifiers, kind: KeyEventKind::Press, ..
                }) = term_event {
                    match (code, modifiers) {
                        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                            if let Some(c) = cursor.take() {
                                c.finish().await;
                            }
                            if let Some(s) = spinner.take() {
                                s.finish().await;
                            }
                            let _ = client.cancel_orchestrator().await;
                            rprintln!("  {}", "Cancelled".yellow());

                            while let Some(event) = event_rx.recv().await {
                                if matches!(
                                    event,
                                    ClientDirectMessage::OrchestratorDone { .. }
                                        | ClientDirectMessage::OrchestratorStopped
                                ) {
                                    break;
                                }
                            }

                            accumulated_content.clear();
                            break;
                        }
                        (KeyCode::Char('o'), m) if m.contains(KeyModifiers::CONTROL) => {
                            if let Some(s) = spinner.take() {
                                s.finish().await;
                            }
                            print!("\r\x1B[2K");
                            let _ = std::io::stdout().flush();

                            clear_output(output_lines);

                            *expanded = !*expanded;
                            output_lines = 0;

                            if has_token_line {
                                render_token_line(total_prompt_tokens, total_completion_tokens, total_tokens);
                                output_lines += 1;
                            }

                            if let Some(ref plan) = current_plan {
                                output_lines += render_plan(plan);
                            }

                            if *expanded {
                                output_lines += render_expanded_tool_calls(&tool_calls);
                            }

                            if let Some(ref name) = current_tool {
                                let label = spinner_label(name, tool_calls.len(), *expanded);
                                spinner = Some(Spinner::start_with_elapsed(&label));
                            }
                        }
                        _ => {}
                    }
                }
            }

            _ = tokio::signal::ctrl_c(), if !interactive => {
                if let Some(c) = cursor.take() {
                    c.finish().await;
                }
                if let Some(s) = spinner.take() {
                    s.finish().await;
                }
                let _ = client.cancel_orchestrator().await;
                rprintln!("  {}", "Cancelled".yellow());

                while let Some(event) = event_rx.recv().await {
                    if matches!(
                        event,
                        ClientDirectMessage::OrchestratorDone { .. }
                            | ClientDirectMessage::OrchestratorStopped
                    ) {
                        break;
                    }
                }

                accumulated_content.clear();
                break;
            }
        }
    }
}

fn format_token_usage(prompt: u32, completion: u32, total: u32) -> String {
    format!("tokens: {} prompt + {} completion = {}", prompt, completion, total)
}

fn render_token_line(prompt: u32, completion: u32, total: u32) {
    rprintln!("  {}", format_token_usage(prompt, completion, total).dimmed());
}

fn spinner_label(name: &str, tool_count: usize, expanded: bool) -> String {
    if expanded || tool_count == 0 {
        format!("\u{25c6} {}", name)
    } else {
        format!("\u{25c6} {} ({})", name, tool_count)
    }
}

fn clear_output(lines: usize) {
    if lines > 0 {
        print!("\x1B[{}A\x1B[J", lines);
        let _ = std::io::stdout().flush();
    }
}

fn render_expanded_tool_calls(tool_calls: &[(String, bool)]) -> usize {
    for (name, success) in tool_calls {
        let icon = if *success { "\u{2713}".green() } else { "\u{2717}".red() };
        rprintln!("  {} {}", icon, name);
    }
    tool_calls.len()
}

fn render_tool_summary(tool_calls: &[(String, bool)]) {
    let total = tool_calls.len();
    let failures = tool_calls.iter().filter(|(_, ok)| !ok).count();

    //
    // Count occurrences in first-seen order.
    //

    let mut counts: Vec<(&str, usize)> = Vec::new();
    for (name, _) in tool_calls {
        if let Some(entry) = counts.iter_mut().find(|(n, _)| *n == name) {
            entry.1 += 1;
        } else {
            counts.push((name, 1));
        }
    }

    let parts: Vec<String> = counts.iter().map(|(name, count)| {
        if *count > 1 {
            format!("{} \u{00d7}{}", name, count)
        } else {
            name.to_string()
        }
    }).collect();

    let icon = if failures == 0 { "\u{2713}".green() } else { "\u{2717}".red() };
    let label = if total == 1 { "tool call" } else { "tool calls" };

    if failures > 0 {
        rprintln!(
            "  {} {} {} ({}) \u{00b7} {} failed",
            icon, total, label, parts.join(", "), failures
        );
    } else {
        rprintln!("  {} {} {} ({})", icon, total, label, parts.join(", "));
    }
}

fn render_plan(plan: &OrchestratorPlan) -> usize {
    let mut lines = 0;

    rprintln!();
    lines += 1;

    if let Some(ref desc) = plan.current_step_description {
        rprintln!("  {} {}", "\u{25b8}".bold(), desc.as_str().bold());
        lines += 1;
    }

    for step in &plan.steps {
        let (icon, style) = match step.status {
            PlanStepStatus::Done => ("\u{2713}".to_string().green(), step.description.as_str().dimmed()),
            PlanStepStatus::InProgress => ("\u{25cf}".to_string().yellow(), step.description.as_str().normal()),
            PlanStepStatus::NotStarted => ("\u{25cb}".to_string().dimmed(), step.description.as_str().dimmed()),
        };
        rprintln!("  {} {}", icon, style);
        lines += 1;
    }

    if let Some(ref summary) = plan.summary {
        rprintln!("  {}", summary.as_str().dimmed());
        lines += 1;
    }

    rprintln!();
    lines += 1;

    lines
}

fn render_markdown(content: &str) {
    let skin = termimad::MadSkin::default();
    let rendered = skin.text(content, None);

    for line in rendered.to_string().lines() {
        rprintln!("  {}", line);
    }
}

fn render_thinking(text: &str) {
    rprintln!();
    rprintln!("  {} {}", "\u{00B7}".bold(), "Thinking:".bold().dimmed());
    for line in text.lines() {
        rprintln!("    {}", line.dimmed());
    }
    rprintln!();
}

fn strip_thinking(content: &str) -> String {
    let start_tag = "<think>";
    let end_tag = "</think>";
    let mut result = content.to_string();

    while let Some(start) = result.find(start_tag) {
        if let Some(end) = result[start..].find(end_tag) {
            result = format!("{}{}", &result[..start], &result[start + end + end_tag.len()..]);
        } else {
            break;
        }
    }

    result.trim().to_string()
}
