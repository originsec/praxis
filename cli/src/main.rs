mod acp;
mod app;
mod client;
mod commands;
mod config;
mod event;
mod markdown;
mod output;
mod session_store;
mod state;
mod ui;

use anyhow::Result;
use app::App;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use event::EventHandler;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::sync::Arc;

#[derive(Parser)]
#[command(
    about = "Praxis CLI - terminal interface for the Praxis C2 framework",
)]
#[command(version)]
struct Cli {
    /// Connection and command timeout in seconds
    #[arg(short = 't', long = "timeout", default_value = "600")]
    timeout: u64,

    /// Run a single command string and exit
    #[arg(short = 'C', long = "command")]
    command_string: Option<String>,

    /// Clear local state (client ID)
    #[arg(long = "clear")]
    clear: bool,

    /// Check service connection status
    #[arg(long = "status")]
    status: bool,

    /// Run as ACP stdio proxy (forward JSON-RPC over stdin/stdout to service via RabbitMQ)
    #[arg(long = "acp")]
    acp: bool,

    /// Resume a saved orchestrator session — pick from a list of local sessions.
    #[arg(long = "resume")]
    resume: bool,

    /// Continue the most recent local orchestrator session.
    #[arg(long = "continue")]
    continue_last: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Node management commands
    Node {
        #[command(subcommand)]
        command: commands::node::NodeCommand,
    },

    /// Agent management commands
    Agent {
        #[command(subcommand)]
        command: commands::agent::AgentCommand,
    },

    /// Session management commands
    Session {
        #[command(subcommand)]
        command: commands::session::SessionCommand,
    },

    /// Persist the RabbitMQ URL to ~/.config/praxis/config
    #[command(name = "set-rabbitmqurl")]
    SetRabbitmqUrl {
        /// e.g. amqp://user:pass@host:5672
        url: String,
    },

    /// Print resolved CLI config (RabbitMQ URL + source).
    #[command(name = "config")]
    Config,
}

impl Commands {
    async fn execute(self, client: &client::Client) -> Result<()> {
        match self {
            Commands::Node { command } => commands::node::execute(client, command).await,
            Commands::Agent { command } => commands::agent::execute(client, command).await,
            Commands::Session { command } => commands::session::execute(client, command).await,
            Commands::SetRabbitmqUrl { .. } | Commands::Config => unreachable!(
                "config subcommands handled before connecting to a client"
            ),
        }
    }
}

#[derive(Parser)]
#[command(name = "praxis_cli", no_binary_name = true)]
struct CommandStringCli {
    #[command(subcommand)]
    command: Commands,
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(error) = run().await {
        output::print_error(&error.to_string());
        std::process::exit(1);
    }
    Ok(())
}

fn argv0_basename() -> String {
    std::env::args_os()
        .next()
        .and_then(|raw| {
            std::path::PathBuf::from(raw)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "praxis".to_string())
}

fn parse_cli() -> Result<Cli> {
    //
    // clap's name/bin_name require `'static` strings. The binary name
    // is determined once at startup and lives for the rest of the
    // process, so we leak it to satisfy the lifetime.
    //

    let bin: &'static str = Box::leak(argv0_basename().into_boxed_str());
    let after_help: &'static str = Box::leak(
        format!(
            "Without an action flag or subcommand, {bin} starts the interactive terminal UI.\n\n\
             The RabbitMQ URL is read from ~/.config/praxis/config (key PRAXIS_RABBITMQ_URL).\n\
             Use `{bin} set-rabbitmqurl <url>` to set it.\n\n\
             Examples:\n  \
             {bin}\n  \
             {bin} set-rabbitmqurl amqp://praxis:praxis@localhost:5672\n  \
             {bin} --status\n  \
             {bin} -C \"node list\"\n  \
             {bin} session create --node abc123 --yolo",
        )
        .into_boxed_str(),
    );

    let cmd = <Cli as CommandFactory>::command()
        .name(bin)
        .bin_name(bin)
        .after_help(after_help);
    let matches = cmd.get_matches();
    Ok(Cli::from_arg_matches(&matches)?)
}

async fn run() -> Result<()> {
    let cli = parse_cli()?;

    if cli.clear {
        state::CliState::clear()?;
        output::print_success("Local state cleared");
        return Ok(());
    }

    //
    // Config-only subcommands run before we resolve a URL or connect
    // to anything, so they work even with no service available.
    //

    if let Some(Commands::SetRabbitmqUrl { url }) = &cli.command {
        let path = config::set("PRAXIS_RABBITMQ_URL", url)?;
        output::print_success(&format!("Wrote PRAXIS_RABBITMQ_URL to {}", path.display()));
        return Ok(());
    }

    if let Some(Commands::Config) = &cli.command {
        let url = config::resolve_rabbitmq_url();
        let source = if config::get("PRAXIS_RABBITMQ_URL").is_some() {
            "config file"
        } else {
            "default"
        };
        let path = config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        println!("Config file: {}", path);
        println!("RabbitMQ URL: {} ({})", url, source);
        return Ok(());
    }

    let rabbitmq_url = config::resolve_rabbitmq_url();

    if cli.status {
        return run_status(&rabbitmq_url, cli.timeout).await;
    }

    if let Some(command_string) = cli.command_string.as_deref() {
        return run_command_string(&rabbitmq_url, cli.timeout, command_string).await;
    }

    if let Some(command) = cli.command {
        return run_command(&rabbitmq_url, cli.timeout, command).await;
    }

    if cli.acp {
        return run_acp_proxy(&rabbitmq_url, cli.timeout).await;
    }

    let resume = if cli.continue_last {
        session_store::most_recent()?
    } else if cli.resume {
        select_session_interactive()?
    } else {
        None
    };

    run_tui(&rabbitmq_url, cli.timeout, resume).await
}

fn select_session_interactive() -> Result<Option<session_store::StoredSession>> {
    let sessions = session_store::list()?;
    if sessions.is_empty() {
        eprintln!("No saved orchestrator sessions found in ~/.praxis/sessions/");
        return Ok(None);
    }

    println!("Saved orchestrator sessions:");
    for (i, s) in sessions.iter().enumerate() {
        let preview = s
            .first_user_text()
            .map(|t| {
                let t = t.replace('\n', " ");
                if t.len() > 60 {
                    format!("{}…", &t[..60])
                } else {
                    t
                }
            })
            .unwrap_or_else(|| "(empty)".to_string());
        let when = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(
            s.updated_at_ms as i64,
        )
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
        println!("  [{}] {}  {}", i + 1, when, preview);
    }
    print!("Select session (1-{}, or empty to cancel): ", sessions.len());
    use std::io::Write;
    std::io::stdout().flush().ok();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }
    let idx: usize = match input.parse::<usize>() {
        Ok(n) if n >= 1 && n <= sessions.len() => n - 1,
        _ => {
            eprintln!("Invalid selection");
            return Ok(None);
        }
    };
    Ok(Some(sessions.into_iter().nth(idx).unwrap()))
}

async fn run_status(rabbitmq_url: &str, timeout: u64) -> Result<()> {
    let mut cli_state = state::CliState::load()?;
    let client_id = cli_state.get_or_create_client_id()?;
    let short_id = output::format_short_id(&client_id);

    let client = client::Client::connect(rabbitmq_url, timeout, client_id).await?;
    let system_state = client.get_state().await;
    client.disconnect().await;

    output::print_success(&format!("Connected to service (client: {})", short_id));
    if let Some(state) = system_state {
        println!("  Nodes: {}", state.nodes.len());
    }
    Ok(())
}

async fn run_command_string(rabbitmq_url: &str, timeout: u64, command_string: &str) -> Result<()> {
    let tokens = shell_split(command_string);
    let parsed = CommandStringCli::try_parse_from(&tokens)?;
    run_command(rabbitmq_url, timeout, parsed.command).await
}

async fn run_command(rabbitmq_url: &str, timeout: u64, command: Commands) -> Result<()> {
    let mut cli_state = state::CliState::load()?;
    let client_id = cli_state.get_or_create_client_id()?;
    let client = client::Client::connect(rabbitmq_url, timeout, client_id).await?;
    let result = command.execute(&client).await;
    client.disconnect().await;
    result
}

async fn run_acp_proxy(rabbitmq_url: &str, timeout: u64) -> Result<()> {
    let uid = uuid::Uuid::new_v4().to_string();
    let client_id = format!("acp_{}", &uid[..8]);
    let client = Arc::new(client::Client::connect(rabbitmq_url, timeout, client_id).await?);

    let mut acp_rx = client.subscribe_acp_events();

    //
    // Track request IDs originated from stdin so we only forward
    // responses that belong to this proxy session.
    //

    let pending_ids: Arc<std::sync::Mutex<std::collections::HashSet<serde_json::Value>>> =
        Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));

    //
    // ACP responses from service written to stdout as NDJSON.
    // Only forward notifications (no id) and responses to our requests.
    //

    let pending_ids_rx = pending_ids.clone();
    let stdout_task = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(json_rpc) = acp_rx.recv().await {
            //
            // Parse to check if this is a response we should forward.
            //

            let should_forward = if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&json_rpc) {
                if let Some(id) = msg.get("id") {
                    if msg.get("method").is_some() {
                        true // server-initiated request — forward
                    } else {
                        pending_ids_rx.lock().unwrap().remove(id) // response — only if we sent the request
                    }
                } else {
                    true // notification (no id) — always forward
                }
            } else {
                true // parse error — forward anyway
            };

            if !should_forward {
                continue;
            }

            use tokio::io::AsyncWriteExt;
            if stdout.write_all(json_rpc.as_bytes()).await.is_err() {
                break;
            }
            if stdout.write_all(b"\n").await.is_err() {
                break;
            }
            if stdout.flush().await.is_err() {
                break;
            }
        }
    });

    //
    // NDJSON lines from stdin forwarded as ACP requests to service.
    // Track request IDs so we can filter responses.
    //

    let client_clone = client.clone();
    let pending_ids_tx = pending_ids.clone();
    let stdin_task = tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let reader = BufReader::new(tokio::io::stdin());
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }

            //
            // Track the request ID if present.
            //

            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(id) = msg.get("id") {
                    pending_ids_tx.lock().unwrap().insert(id.clone());
                }
            }

            if let Err(e) = client_clone.send_acp_message(line).await {
                eprintln!("Failed to send ACP message: {}", e);
                break;
            }
        }
    });

    //
    // Wait for stdin to close (client disconnected).
    //
    let _ = stdin_task.await;
    stdout_task.abort();

    if let Ok(client) = Arc::try_unwrap(client) {
        client.disconnect().await;
    }
    Ok(())
}

async fn run_tui(
    rabbitmq_url: &str,
    timeout: u64,
    resume: Option<session_store::StoredSession>,
) -> Result<()> {
    let mut cli_state = state::CliState::load()?;
    let client_id = cli_state.get_or_create_client_id()?;

    eprintln!("Connecting to {}...", rabbitmq_url);
    let client = client::Client::connect(rabbitmq_url, timeout, client_id.clone()).await?;
    let client = Arc::new(client);

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            crossterm::event::PopKeyboardEnhancementFlags,
            LeaveAlternateScreen,
            DisableMouseCapture,
        );
        original_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        crossterm::event::PushKeyboardEnhancementFlags(
            crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
        ),
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let terminal_paused = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let terminal_resume = Arc::new(tokio::sync::Notify::new());
    let mut events = EventHandler::new(
        client.clone(),
        terminal_paused.clone(),
        terminal_resume.clone(),
    );
    let mut app = App::new(
        client.clone(),
        rabbitmq_url.to_string(),
        client_id,
        events.sender(),
    );
    app.terminal_paused = terminal_paused;
    app.terminal_resume = terminal_resume;
    if let Some(stored) = resume {
        app.seed_orchestrator_resume(stored);
    }
    app.init().await;
    let mut should_draw = true;

    loop {
        if app.needs_full_redraw {
            app.needs_full_redraw = false;
            terminal.clear()?;
            should_draw = true;
        }

        if should_draw {
            //
            // Pre-render housekeeping: rebuild the intercept display
            // rows when filters or buffer changed, and expire stale
            // error banners. Done here so render() can stay
            // &App-pure.
            //
            app.intercept.rebuild_display();
            app.intercept.clear_stale_error();

            terminal.draw(|f| {
                app.terminal_width = f.area().width;
                ui::render(f, &app);
            })?;
            should_draw = false;
        }

        if let Some(event) = events.next().await {
            should_draw |= app.handle_event(event).await;
        } else {
            break;
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::event::PopKeyboardEnhancementFlags,
        LeaveAlternateScreen,
        DisableMouseCapture,
    )?;

    if let Ok(client) = Arc::try_unwrap(client) {
        client.disconnect().await;
    }

    Ok(())
}

fn shell_split(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;

    for ch in input.chars() {
        if escape_next {
            current.push(ch);
            escape_next = false;
            continue;
        }

        if ch == '\\' && !in_single_quote {
            escape_next = true;
            continue;
        }

        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            continue;
        }

        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            continue;
        }

        if ch.is_whitespace() && !in_single_quote && !in_double_quote {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }

        current.push(ch);
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}
