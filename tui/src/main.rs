mod app;
mod client;
mod event;
mod markdown;
mod state;
mod ui;

use anyhow::Result;
use app::App;
use clap::Parser;
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
#[command(name = "praxis_tui")]
#[command(about = "Praxis TUI - terminal user interface for the Praxis C2 framework")]
#[command(version)]
struct Args {
    /// RabbitMQ URL
    #[arg(short = 'r', long = "rabbitmq", env = "PRAXIS_RABBITMQ_URL")]
    #[arg(default_value = "amqp://praxis:praxis@localhost:5672")]
    rabbitmq_url: String,

    /// Connection timeout in seconds
    #[arg(short = 't', long = "timeout", default_value = "10")]
    timeout: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    //
    // Load or create persistent client ID.
    //
    let mut cli_state = state::CliState::load()?;
    let client_id = cli_state.get_or_create_client_id()?;

    //
    // Connect to the service via RabbitMQ.
    //
    eprintln!("Connecting to {}...", args.rabbitmq_url);
    let client = client::Client::connect(&args.rabbitmq_url, args.timeout, client_id).await?;
    let client = Arc::new(client);

    //
    // Install panic hook to restore terminal on crash.
    //
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

    //
    // Enter TUI mode.
    //
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        crossterm::event::PushKeyboardEnhancementFlags(
            crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | crossterm::event::KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES,
        ),
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    //
    // Build app and event handler.
    //
    let mut app = App::new(client.clone());
    app.init().await;
    let mut events = EventHandler::new(client.clone());
    app.event_tx = Some(events.sender());

    //
    // Main render loop.
    //
    loop {
        terminal.draw(|f| {
            app.terminal_width = f.area().width;
            ui::render(f, &app);
        })?;

        if let Some(event) = events.next().await {
            app.handle_event(event).await;
        } else {
            break;
        }

        if app.should_quit {
            break;
        }
    }

    //
    // Restore terminal.
    //
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::event::PopKeyboardEnhancementFlags,
        LeaveAlternateScreen,
        DisableMouseCapture,
    )?;

    //
    // Graceful disconnect — only possible if we have sole ownership.
    //
    if let Ok(client) = Arc::try_unwrap(client) {
        client.disconnect().await;
    }

    Ok(())
}
