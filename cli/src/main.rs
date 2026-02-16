mod client;
mod commands;
mod interactive;
mod mcp;
mod output;
mod spinner;
mod state;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

use commands::{
    agent::AgentCommand,
    node::NodeCommand,
    op::OpCommand,
    recon::ReconCommand,
    session::SessionCommand,
    traffic::TrafficCommand,
};
use output::OutputFormat;

#[derive(Parser)]
#[command(name = "praxis_cli")]
#[command(about = "Praxis CLI - command-line interface for the Praxis C2 framework")]
#[command(version)]
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

    /// Run a single command and exit
    #[arg(short = 'C', long = "command")]
    command_str: Option<String>,

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
pub(crate) enum Commands {
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

    /// Reconnaissance operations
    Recon {
        #[command(subcommand)]
        command: ReconCommand,
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

    /// Operation and chain workflow commands
    Op {
        #[command(subcommand)]
        command: OpCommand,
    },

    /// Interactive LLM orchestrator session
    Orchestrate,
}

impl Commands {
    pub(crate) async fn execute(
        self,
        client: &mut client::CliClient,
        output: &OutputFormat,
    ) -> Result<()> {
        match self {
            Commands::Node { command } => commands::node::execute(client, command, output).await,
            Commands::Agent { command } => commands::agent::execute(client, command, output).await,
            Commands::Recon { command } => commands::recon::execute(client, command, output).await,
            Commands::Session { command } => {
                commands::session::execute(client, command, output).await
            }
            Commands::Traffic { command } => {
                commands::traffic::execute(client, command, output).await
            }
            Commands::Op { command } => commands::op::execute(client, command, output).await,
            Commands::Orchestrate => commands::orchestrate::execute(client).await,
        }
    }
}

fn print_fullhelp() {
    let mut cmd = Cli::command();

    println!("================================================================================");
    println!("PRAXIS CLI - COMPREHENSIVE HELP");
    println!("================================================================================");
    println!();

    println!("MAIN COMMAND");
    println!("------------");
    cmd.print_help().ok();
    println!();
    println!();

    let subcommands = ["node", "agent", "recon", "session", "traffic", "op", "orchestrate"];

    for sub_name in subcommands {
        println!("================================================================================");
        println!("COMMAND: praxis_cli {}", sub_name);
        println!("================================================================================");
        println!();

        if let Some(sub) = cmd.find_subcommand_mut(sub_name) {
            sub.print_help().ok();
            println!();
            println!();

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

    if cli.fullhelp {
        print_fullhelp();
        return Ok(());
    }

    if cli.clear {
        state::CliState::clear()?;
        output::print_success("Local state cleared");
        return Ok(());
    }

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

    if cli.mcp {
        return mcp::run_server(&cli.rabbitmq_url, cli.timeout).await;
    }

    //
    // -C "command string": parse, execute, exit.
    //
    if let Some(cmd_str) = cli.command_str {
        let tokens = interactive::shell_split(&cmd_str);
        let repl_cli = interactive::ReplCli::try_parse_from(&tokens)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut cli_state = state::CliState::load()?;
        let client_id = cli_state.get_or_create_client_id()?;
        let mut client =
            client::CliClient::connect(&cli.rabbitmq_url, cli.timeout, client_id).await?;

        let result = repl_cli.command.execute(&mut client, &cli.output).await;

        client.disconnect().await;
        return result;
    }

    //
    // Explicit subcommand (backwards compatibility).
    //
    if let Some(command) = cli.command {
        let mut cli_state = state::CliState::load()?;
        let client_id = cli_state.get_or_create_client_id()?;
        let mut client =
            client::CliClient::connect(&cli.rabbitmq_url, cli.timeout, client_id).await?;

        let result = command.execute(&mut client, &cli.output).await;

        client.disconnect().await;
        return result;
    }

    //
    // No command, no flags: enter interactive REPL.
    //
    interactive::run_repl(&cli.rabbitmq_url, cli.timeout, cli.output).await
}
