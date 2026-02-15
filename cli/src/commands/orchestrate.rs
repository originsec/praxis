use anyhow::Result;
use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{Config, Editor};
use std::io::Write;
use tokio::sync::mpsc;

use common::{ClientDirectMessage, OrchestratorPlan, PlanStepStatus};

use crate::client::CliClient;
use crate::spinner::Spinner;

pub async fn execute(client: &mut CliClient) -> Result<()> {
    //
    // Subscribe to orchestrator events before starting the session.
    //
    let mut event_rx = client.subscribe_orchestrator_events();

    client.start_orchestrator().await?;

    //
    // Wait for OrchestratorStarted or OrchestratorError.
    //
    let started = wait_for_started(&mut event_rx).await;
    if !started {
        client.unsubscribe_orchestrator_events().await;
        return Ok(());
    }

    println!("  {}", "Type your prompt, Ctrl+C to cancel inference, Ctrl+D to exit".dimmed());
    println!();

    let config = Config::builder().build();
    let mut rl: Editor<(), DefaultHistory> = Editor::with_config(config)?;

    loop {
        let line = rl.readline(&format!("  {} ", "▸".bold()));

        match line {
            Ok(input) => {
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(trimmed);

                client.send_orchestrator_prompt(trimmed.to_string()).await?;

                //
                // Process events until Done.
                //
                process_events_until_done(client, &mut event_rx).await;
            }
            Err(ReadlineError::Interrupted) => {
                //
                // Ctrl+C at prompt — exit.
                //
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

    //
    // Clean up.
    //
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
        Ok(Some(ClientDirectMessage::OrchestratorError { message })) => {
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
) {
    let mut spinner: Option<Spinner> = None;
    let mut accumulated_content = String::new();
    let mut total_prompt_tokens: u32 = 0;
    let mut total_completion_tokens: u32 = 0;
    let mut total_tokens: u32 = 0;

    //
    // Install Ctrl+C handler for cancelling inference.
    //
    let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_flag_clone = cancel_flag.clone();

    let ctrlc_handle = tokio::spawn(async move {
        loop {
            if tokio::signal::ctrl_c().await.is_ok() {
                cancel_flag_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                break;
            }
        }
    });

    loop {
        tokio::select! {
            event = event_rx.recv() => {
                let Some(event) = event else { break };

                match event {
                    ClientDirectMessage::OrchestratorContent { content } => {
                        if let Some(s) = spinner.take() {
                            s.finish().await;
                        }
                        accumulated_content.push_str(&content);
                    }
                    ClientDirectMessage::OrchestratorToolExecuting { name, input: _ } => {
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
                            spinner = Some(Spinner::start_with_elapsed(&format!("◆ {}", name)));
                        }
                    }
                    ClientDirectMessage::OrchestratorToolExecuted { name, display, success, .. } => {
                        if name != "report_plan" {
                            if let Some(s) = spinner.take() {
                                s.finish().await;
                            }
                            let icon = if success { "✓".green() } else { "✗".red() };
                            println!("  {} {} {}", icon, name.dimmed(), display);
                        }
                    }
                    ClientDirectMessage::OrchestratorPlanUpdated { plan } => {
                        if let Some(s) = spinner.take() {
                            s.finish().await;
                        }
                        print!("\r\x1B[2K");
                        let _ = std::io::stdout().flush();
                        render_plan(&plan);
                    }
                    ClientDirectMessage::OrchestratorTokenUsage { prompt_tokens, completion_tokens, total_tokens: batch_total } => {
                        total_prompt_tokens += prompt_tokens;
                        total_completion_tokens += completion_tokens;
                        total_tokens += batch_total;

                        //
                        // Update token counter in-place below the prompt line.
                        //
                        let usage = format!("  tokens: {} prompt + {} completion = {}", total_prompt_tokens, total_completion_tokens, total_tokens);
                        print!("\r\x1B[2K{}", usage.dimmed());
                        let _ = std::io::stdout().flush();
                    }
                    ClientDirectMessage::OrchestratorError { message } => {
                        if let Some(s) = spinner.take() {
                            s.finish().await;
                        }
                        eprintln!("  {} {}", "✗".red(), message);
                    }
                    ClientDirectMessage::OrchestratorDone => {
                        if let Some(s) = spinner.take() {
                            s.finish().await;
                        }

                        //
                        // Finalize the in-place token counter line.
                        //
                        if total_tokens > 0 {
                            println!();
                        }

                        //
                        // Render accumulated content as markdown.
                        //
                        if !accumulated_content.trim().is_empty() {
                            println!();
                            render_markdown(&accumulated_content);
                            println!();
                        }

                        accumulated_content.clear();
                        break;
                    }
                    ClientDirectMessage::OrchestratorStopped => {
                        if let Some(s) = spinner.take() {
                            s.finish().await;
                        }
                        break;
                    }
                    _ => {}
                }
            }
            _ = async {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    if cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                }
            } => {
                //
                // Ctrl+C during inference — cancel and continue loop.
                //
                if let Some(s) = spinner.take() {
                    s.finish().await;
                }
                let _ = client.cancel_orchestrator().await;
                println!("  {}", "Cancelled".yellow());

                //
                // Drain until Done.
                //
                while let Some(event) = event_rx.recv().await {
                    if matches!(event, ClientDirectMessage::OrchestratorDone | ClientDirectMessage::OrchestratorStopped) {
                        break;
                    }
                }

                accumulated_content.clear();
                break;
            }
        }
    }

    ctrlc_handle.abort();
}

fn render_plan(plan: &OrchestratorPlan) {
    println!();

    if let Some(ref desc) = plan.current_step_description {
        println!("  {} {}", "▸".bold(), desc.as_str().bold());
    }

    for step in &plan.steps {
        let (icon, style) = match step.status {
            PlanStepStatus::Done => ("✓".to_string().green(), step.description.as_str().dimmed()),
            PlanStepStatus::InProgress => ("●".to_string().yellow(), step.description.as_str().normal()),
            PlanStepStatus::NotStarted => ("○".to_string().dimmed(), step.description.as_str().dimmed()),
        };
        println!("  {} {}", icon, style);
    }

    if let Some(ref summary) = plan.summary {
        println!("  {}", summary.as_str().dimmed());
    }

    println!();
}

fn render_markdown(content: &str) {
    let skin = termimad::MadSkin::default();
    let rendered = skin.text(content, None);
    //
    // Indent each line for consistent layout.
    //
    for line in rendered.to_string().lines() {
        println!("  {}", line);
    }
}
