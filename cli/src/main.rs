mod client;
mod commands;
mod mcp;
mod output;
mod state;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

use commands::{
    agent::AgentCommand,
    chain::ChainCommand,
    node::NodeCommand,
    op::OpCommand,
    session::SessionCommand,
    traffic::TrafficCommand,
};
use output::OutputFormat;

#[derive(Parser)]
#[command(name = "praxis_cli")]
#[command(about = "Praxis CLI - command-line interface for the Praxis C2 framework")]
#[command(version)]
#[command(arg_required_else_help = true)]
struct Cli {
    /// RabbitMQ URL
    #[arg(short = 'r', long = "rabbitmq", env = "PRAXIS_RABBITMQ_URL")]
    #[arg(default_value = "amqp://praxis:praxis@localhost:5672")]
    rabbitmq_url: String,

    /// Output format
    #[arg(short = 'o', long = "output", default_value = "text")]
    output: OutputFormat,

    /// Command timeout in seconds
    #[arg(short = 't', long = "timeout", default_value = "300")]
    timeout: u64,

    /// Clear local state (client ID)
    #[arg(long = "clear")]
    clear: bool,

    /// Check service connection status
    #[arg(long = "status")]
    status: bool,

    /// Run as MCP server (stdio)
    #[arg(long = "mcp")]
    mcp: bool,

    /// Show comprehensive help for all commands
    #[arg(long = "fullhelp", display_order = 1000)]
    fullhelp: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Node management commands
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },

    /// Agent management commands
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },

    /// Session management commands
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },

    /// Traffic search commands
    Traffic {
        #[command(subcommand)]
        command: TrafficCommand,
    },

    /// Semantic operation commands
    Op {
        #[command(subcommand)]
        command: OpCommand,
    },

    /// Chain workflow commands
    Chain {
        #[command(subcommand)]
        command: ChainCommand,
    },
}

fn print_fullhelp() {
    let mut cmd = Cli::command();

    println!("================================================================================");
    println!("PRAXIS CLI - COMPREHENSIVE HELP");
    println!("================================================================================");
    println!();

    //
    // Print main help.
    //
    println!("MAIN COMMAND");
    println!("------------");
    cmd.print_help().ok();
    println!();
    println!();

    //
    // Print help for each subcommand.
    //
    let subcommands = ["node", "agent", "session", "traffic", "op", "chain"];

    for sub_name in subcommands {
        println!("================================================================================");
        println!("COMMAND: praxis_cli {}", sub_name);
        println!("================================================================================");
        println!();

        if let Some(sub) = cmd.find_subcommand_mut(sub_name) {
            sub.print_help().ok();
            println!();
            println!();

            //
            // Print help for nested subcommands.
            //
            let nested: Vec<String> = sub
                .get_subcommands()
                .map(|s| s.get_name().to_string())
                .collect();

            for nested_name in nested {
                if let Some(nested_sub) = sub.find_subcommand_mut(&nested_name) {
                    println!("  SUBCOMMAND: praxis_cli {} {}", sub_name, nested_name);
                    println!("  {}", "-".repeat(60));
                    let help = nested_sub.render_help().to_string();
                    for line in help.lines() {
                        println!("    {}", line);
                    }
                    println!();
                }
            }
        }
    }

    println!("================================================================================");
    println!("EXAMPLES");
    println!("================================================================================");
    println!();
    println!("  # List connected nodes");
    println!("  praxis_cli node list");
    println!();
    println!("  # Select a node and list agents");
    println!("  praxis_cli agent list --node abc123");
    println!();
    println!("  # Select agent and create session");
    println!("  praxis_cli agent select --node abc123 claudecode");
    println!("  praxis_cli session create --node abc123 --yolo");
    println!();
    println!("  # Send a prompt");
    println!("  praxis_cli session prompt --node abc123 \"list files in current directory\"");
    println!();
    println!("  # Run a semantic operation");
    println!("  praxis_cli op run recon::system_info --node abc123 --agent claudecode");
    println!();
    println!("  # Search intercepted traffic");
    println!("  praxis_cli traffic search \"api\\.openai\\.com\" --limit 10");
    println!();
    println!("  # Use JSON output for scripting");
    println!("  praxis_cli --output json node list");
    println!();
}

fn main() {
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run());

    if let Err(e) = result {
        output::print_error(&e.to_string());
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    //
    // Handle --fullhelp early.
    //
    if cli.fullhelp {
        print_fullhelp();
        return Ok(());
    }

    //
    // Handle --clear early.
    //
    if cli.clear {
        state::CliState::clear()?;
        output::print_success("Local state cleared");
        return Ok(());
    }

    //
    // Handle --status early.
    //
    if cli.status {
        let mut cli_state = state::CliState::load()?;
        let client_id = cli_state.get_or_create_client_id()?;
        let short_id = client_id[..8.min(client_id.len())].to_string();

        let client = client::CliClient::connect(&cli.rabbitmq_url, 10, client_id).await?;
        let system_state = client.get_state().await;
        client.disconnect().await;

        match &cli.output {
            OutputFormat::Json => {
                let node_count = system_state.as_ref().map(|s| s.nodes.len()).unwrap_or(0);
                output::print_json(&serde_json::json!({
                    "status": "connected",
                    "client_id": short_id,
                    "rabbitmq_url": cli.rabbitmq_url,
                    "node_count": node_count
                }));
            }
            OutputFormat::Text => {
                output::print_success(&format!("Connected to service (client: {})", short_id));
                if let Some(state) = system_state {
                    println!("  Nodes: {}", state.nodes.len());
                }
            }
        }
        return Ok(());
    }

    //
    // Handle --mcp early.
    //
    if cli.mcp {
        return mcp::run_server(&cli.rabbitmq_url, cli.timeout).await;
    }

    //
    // Require a command if not --fullhelp, --clear, --status, or --mcp.
    //
    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            Cli::command().print_help().ok();
            return Ok(());
        }
    };

    //
    // Load or create persistent client ID.
    //
    let mut cli_state = state::CliState::load()?;
    let client_id = cli_state.get_or_create_client_id()?;

    let mut client = client::CliClient::connect(&cli.rabbitmq_url, cli.timeout, client_id).await?;

    let result = match command {
        Commands::Node { command } => commands::node::execute(&mut client, command, &cli.output).await,
        Commands::Agent { command } => commands::agent::execute(&mut client, command, &cli.output).await,
        Commands::Session { command } => commands::session::execute(&mut client, command, &cli.output).await,
        Commands::Traffic { command } => commands::traffic::execute(&mut client, command, &cli.output).await,
        Commands::Op { command } => commands::op::execute(&mut client, command, &cli.output).await,
        Commands::Chain { command } => commands::chain::execute(&mut client, command, &cli.output).await,
    };

    client.disconnect().await;
    result
}
