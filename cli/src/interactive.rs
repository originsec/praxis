use anyhow::Result;
use clap::{CommandFactory, Parser};
use colored::Colorize;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config, Editor, Helper};

use crate::client::CliClient;
use crate::output::OutputFormat;
use crate::Commands;

#[derive(Parser)]
#[command(name = "praxis", no_binary_name = true)]
pub(crate) struct ReplCli {
    #[command(subcommand)]
    pub command: Commands,
}

//
// Split input string into tokens, respecting quoted strings and escapes.
//
pub(crate) fn shell_split(input: &str) -> Vec<String> {
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

struct PraxisCompleter {
    commands: Vec<Vec<String>>,
}

impl PraxisCompleter {
    fn new() -> Self {
        let cmd = ReplCli::command();
        let mut paths = Vec::new();
        Self::collect_paths(&cmd, &mut Vec::new(), &mut paths);
        Self { commands: paths }
    }

    //
    // Recursively walk the clap Command tree and collect all subcommand
    // paths as token sequences (e.g. ["node", "list"], ["agent", "config", "get"]).
    //
    fn collect_paths(cmd: &clap::Command, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        let subs: Vec<_> = cmd.get_subcommands().collect();
        if subs.is_empty() && !prefix.is_empty() {
            out.push(prefix.clone());
            return;
        }
        for sub in subs {
            prefix.push(sub.get_name().to_string());
            let nested: Vec<_> = sub.get_subcommands().collect();
            if nested.is_empty() {
                out.push(prefix.clone());
            } else {
                Self::collect_paths(sub, prefix, out);
            }
            prefix.pop();
        }
    }
}

impl Completer for PraxisCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let input = &line[..pos];
        let tokens = shell_split(input);
        let trailing_space = input.ends_with(' ');

        //
        // Determine which token index we're completing. If there's a trailing
        // space, we're starting a new token; otherwise we're completing the
        // last one.
        //
        let (depth, partial) = if trailing_space {
            (tokens.len(), "")
        } else {
            let partial = tokens.last().map(|s| s.as_str()).unwrap_or("");
            (tokens.len().saturating_sub(1), partial)
        };

        let prefix_tokens: Vec<&str> = tokens.iter().take(depth).map(|s| s.as_str()).collect();

        let mut candidates: Vec<String> = Vec::new();

        for path in &self.commands {
            if path.len() <= depth {
                continue;
            }

            //
            // Check that all preceding tokens match.
            //
            let matches = prefix_tokens
                .iter()
                .zip(path.iter())
                .all(|(input, cmd)| *input == cmd.as_str());

            if !matches {
                continue;
            }

            let candidate = &path[depth];
            if candidate.starts_with(partial) && !candidates.contains(candidate) {
                candidates.push(candidate.clone());
            }
        }

        let start = pos - partial.len();
        let pairs = candidates
            .into_iter()
            .map(|c| Pair {
                display: c.clone(),
                replacement: c,
            })
            .collect();

        Ok((start, pairs))
    }
}

impl Hinter for PraxisCompleter {
    type Hint = String;
}
impl Highlighter for PraxisCompleter {}
impl Validator for PraxisCompleter {}
impl Helper for PraxisCompleter {}

fn history_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".praxis").join("history"))
}

fn print_banner(client_id_short: &str, node_count: usize) {
    println!(
        r#"
  ___  ____   __   _  _  __  ____
 / __)(  _ \ / _\ ( \/ )(  )/ ___)
( (__  )   //    \ )  (  )( \___ \
 \___)(__\_)\_/\_/(_/\_)(__)(____/
"#
    );
    println!(
        "  {} {} | client {} | {} node(s)",
        "praxis".bold(),
        env!("CARGO_PKG_VERSION"),
        client_id_short.cyan(),
        node_count
    );
    println!("  Type {} for commands, {} to quit\n", "help".bold(), "exit".bold());
}

fn print_help() {
    println!("\n{}\n", "Commands".bold().underline());

    let cmds = [
        ("node list", "List connected nodes"),
        ("node select <prefix>", "Select a node"),
        ("agent list -n <node>", "List agents on a node"),
        ("agent select -n <node> <agent>", "Select an agent"),
        ("agent recon -n <node>", "Reconnaisance on a node"),
        ("agent recon-semantic -n <node>", "Semantic recon on a node"),
        ("agent update -n <node>", "Request agent info update"),
        ("session create -n <node>", "Create a session"),
        ("session prompt -n <node> <text>", "Send a prompt"),
        ("session close -n <node>", "Close a session"),
        ("traffic search <pattern>", "Search intercepted traffic"),
        ("op list", "List available operations"),
        ("op run <name> -n <node> -a <agent>", "Run an operation"),
        ("op running", "List running operations"),
        ("op status <id>", "Check operation status"),
        ("op cancel <id>", "Cancel an operation"),
        ("chain list", "List available chains"),
        ("chain run <id> -n <node> -a <agent>", "Run a chain"),
        ("chain running", "List running chains"),
        ("chain status <id>", "Check chain status"),
        ("chain cancel <id>", "Cancel a chain"),
        ("", ""),
        ("help", "Show this help"),
        ("clear", "Clear the screen"),
        ("exit / quit", "Exit the REPL"),
    ];

    for (cmd, desc) in cmds {
        if cmd.is_empty() {
            println!();
        } else {
            println!("  {:<40} {}", cmd.green(), desc);
        }
    }
    println!();
}

pub async fn run_repl(rabbitmq_url: &str, timeout: u64, output: OutputFormat) -> Result<()> {
    let mut cli_state = crate::state::CliState::load()?;
    let client_id = cli_state.get_or_create_client_id()?;
    let short_id = client_id[..8.min(client_id.len())].to_string();

    let mut client = CliClient::connect(rabbitmq_url, timeout, client_id).await?;

    let node_count = client
        .get_state()
        .await
        .map(|s| s.nodes.len())
        .unwrap_or(0);

    print_banner(&short_id, node_count);

    let config = Config::builder()
        .completion_type(CompletionType::List)
        .build();

    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(PraxisCompleter::new()));

    if let Some(path) = history_path() {
        let _ = rl.load_history(&path);
    }

    loop {
        match rl.readline("praxis > ") {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(trimmed);

                match trimmed {
                    "exit" | "quit" => break,
                    "clear" => {
                        print!("\x1B[2J\x1B[1;1H");
                        continue;
                    }
                    "help" => {
                        print_help();
                        continue;
                    }
                    _ => {}
                }

                let tokens = shell_split(trimmed);

                match ReplCli::try_parse_from(&tokens) {
                    Ok(parsed) => {
                        if let Err(e) =
                            parsed.command.execute(&mut client, &output).await
                        {
                            crate::output::print_error(&e.to_string());
                        }
                    }
                    Err(e) => {
                        //
                        // Clap errors include --help output; print them
                        // without the "error" prefix for help requests.
                        //
                        println!("{}", e);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                crate::output::print_error(&format!("Input error: {}", e));
                break;
            }
        }
    }

    if let Some(path) = history_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = rl.save_history(&path);
    }

    client.disconnect().await;
    Ok(())
}
