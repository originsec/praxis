mod app;
mod client;
mod commands;
mod event;
mod markdown;
mod output;
mod state;
mod ui;

use anyhow::Result;
use app::App;
use clap::{Parser, Subcommand};
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
#[command(name = "praxis_cli")]
#[command(
    about = "Praxis CLI - terminal interface for the Praxis C2 framework",
    after_help = "Without an action flag or subcommand, praxis_cli starts the interactive terminal UI.\n\nExamples:\n  praxis_cli\n  praxis_cli --rabbitmq amqp://praxis:praxis@localhost:5672\n  praxis_cli --status\n  praxis_cli -C \"node list\"\n  praxis_cli session create --node abc123 --yolo"
)]
#[command(version)]
struct Cli {
    /// RabbitMQ URL
    #[arg(short = 'r', long = "rabbitmq", env = "PRAXIS_RABBITMQ_URL")]
    #[arg(default_value = "amqp://praxis:praxis@localhost:5672")]
    rabbitmq_url: String,

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
}

impl Commands {
    async fn execute(self, client: &client::Client) -> Result<()> {
        match self {
            Commands::Node { command } => commands::node::execute(client, command).await,
            Commands::Agent { command } => commands::agent::execute(client, command).await,
            Commands::Session { command } => commands::session::execute(client, command).await,
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

async fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.clear {
        state::CliState::clear()?;
        output::print_success("Local state cleared");
        return Ok(());
    }

    if cli.status {
        return run_status(&cli.rabbitmq_url, cli.timeout).await;
    }

    if let Some(command_string) = cli.command_string.as_deref() {
        return run_command_string(&cli.rabbitmq_url, cli.timeout, command_string).await;
    }

    if let Some(command) = cli.command {
        return run_command(&cli.rabbitmq_url, cli.timeout, command).await;
    }

    run_tui(&cli.rabbitmq_url, cli.timeout).await
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

async fn run_tui(rabbitmq_url: &str, timeout: u64) -> Result<()> {
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

    let mut app = App::new(client.clone(), rabbitmq_url.to_string(), client_id);
    app.init().await;
    let mut events = EventHandler::new(client.clone());
    app.event_tx = Some(events.sender());
    let mut should_draw = true;

    loop {
        if should_draw {
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
