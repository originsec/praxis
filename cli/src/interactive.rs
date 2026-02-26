use anyhow::Result;
use clap::{CommandFactory, Parser};
use colored::Colorize;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::history::DefaultHistory;
use rustyline::{CompletionType, Config, Editor, Helper};
use std::sync::{Arc, Mutex};

use crate::client::CliClient;
use crate::output::{format_short_id, OutputFormat};
use crate::Commands;

#[derive(Parser)]
#[command(
    name = "praxis",
    no_binary_name = true,
    disable_help_flag = true,
    disable_help_subcommand = true,
)]
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

//
// REPL selection state — tracks which node/agent is selected and
// whether there's an active session.
//

#[derive(Default)]
struct ReplState {
    selected_node: Option<String>,
    selected_machine_name: Option<String>,
    selected_agent: Option<String>,
    has_session: bool,
}

impl ReplState {

    //
    // Build plain-text and ANSI-colored prompts separately. The plain version
    // is passed to rustyline for correct width calculation; the colored one is
    // returned by Highlighter::highlight_prompt for display.
    //

    fn build_prompt(&self) -> (String, String) {
        if self.selected_node.is_none() {
            return (
                "praxis ❯ ".to_string(),
                format!("{} {} ", "praxis".bold(), "❯".bold()),
            );
        }

        let node_display = self
            .selected_machine_name
            .as_deref()
            .unwrap_or_else(|| {
                self.selected_node
                    .as_deref()
                    .map(|id| &id[..8.min(id.len())])
                    .unwrap_or("?")
            });

        let (inner_plain, inner_colored) = if let Some(ref agent) = self.selected_agent {
            if self.has_session {
                (
                    format!("{}:{} *", node_display, agent),
                    format!("{}:{} {}", node_display.cyan(), agent.green(), "*".yellow()),
                )
            } else {
                (
                    format!("{}:{}", node_display, agent),
                    format!("{}:{}", node_display.cyan(), agent.green()),
                )
            }
        } else {
            (
                node_display.to_string(),
                format!("{}", node_display.cyan()),
            )
        };

        (
            format!("praxis [{}] ❯ ", inner_plain),
            format!("{} [{}] {} ", "praxis".bold(), inner_colored, "❯".bold()),
        )
    }
}

//
// Completion cache — populated after each command, consumed by the
// synchronous Completer trait.
//

#[derive(Default)]
struct CompletionCache {
    node_ids: Vec<String>,
    agent_names: Vec<String>,
    op_names: Vec<String>,
    op_short_ids: Vec<String>,
    chain_names: Vec<String>,
    chain_exec_ids: Vec<String>,
    project_paths: Vec<String>,
    config_paths: Vec<String>,
    session_paths: Vec<String>,
}

async fn refresh_completion_cache(
    client: &CliClient,
    cache: &Arc<Mutex<CompletionCache>>,
    selected_node: Option<&str>,
    selected_agent: Option<&str>,
) {
    let mut node_ids = Vec::new();
    let mut agent_names = Vec::new();
    let mut full_node_id = None;

    if let Some(state) = client.get_state().await {
        for node in &state.nodes {
            let short = format_short_id(&node.node_id);
            node_ids.push(short.clone());

            //
            // Only show agents from the selected node. If no node is selected,
            // show agents from all nodes.
            //

            let show_agents = selected_node
                .map(|sel| node.node_id.starts_with(sel) || short == sel)
                .unwrap_or(true);

            if show_agents {
                if selected_node.is_some() {
                    full_node_id = Some(node.node_id.clone());
                }
                for agent in &node.discovered_agents {
                    if !agent_names.contains(&agent.short_name) {
                        agent_names.push(agent.short_name.clone());
                    }
                }
            }
        }
    }

    //
    // Fire a non-blocking recon request so project paths are available
    // for the next completion cycle. Read whatever is already cached.
    //

    if let (Some(ref nid), Some(agent)) = (full_node_id, selected_agent) {
        client.request_recon_result(nid, agent).await;
    }
    let project_paths = client.get_cached_project_paths().await;
    let config_paths = client.get_cached_config_paths().await;
    let session_paths = client.get_cached_session_paths().await;

    let op_defs = client.get_operation_definitions().await;
    let op_names: Vec<String> = op_defs.iter()
        .filter(|op| !op.disabled)
        .map(|op| op.full_name.clone())
        .collect();

    let ops = client.get_operations().await;
    let op_short_ids: Vec<String> = ops.iter()
        .map(|op| format_short_id(&op.operation_id))
        .collect();

    let chain_defs = client.get_chain_definitions().await;
    let chain_names: Vec<String> = chain_defs.iter()
        .filter(|c| !c.disabled)
        .map(|c| c.name.clone())
        .collect();

    let execs = client.get_chain_executions().await;
    let chain_exec_ids: Vec<String> = execs.iter()
        .map(|e| format_short_id(&e.execution_id))
        .collect();

    if let Ok(mut c) = cache.lock() {
        c.node_ids = node_ids;
        c.agent_names = agent_names;
        c.op_names = op_names;
        c.op_short_ids = op_short_ids;
        c.chain_names = chain_names;
        c.chain_exec_ids = chain_exec_ids;
        c.project_paths = project_paths;
        c.config_paths = config_paths;
        c.session_paths = session_paths;
    }
}

//
// Determine what kind of dynamic value we should complete based on
// the current input tokens.
//

enum CompletionContext {
    Command,
    NodeId,
    AgentName,
    OpName,
    ShortId,
    ProjectPath,
    ConfigPath,
    SessionPath,
    ReconSection,
}

fn detect_context(tokens: &[&str], trailing_space: bool) -> CompletionContext {
    let completing_idx = if trailing_space { tokens.len() } else { tokens.len().saturating_sub(1) };

    //
    // If the previous token is a flag, complete the flag value.
    //

    if completing_idx > 0 {
        let prev = tokens[completing_idx - 1];
        if prev == "-n" || prev == "--node" {
            return CompletionContext::NodeId;
        }
        if prev == "-a" || prev == "--agent" {
            return CompletionContext::AgentName;
        }
        if prev == "-p" || prev == "--project" {
            return CompletionContext::ProjectPath;
        }
    }

    //
    // Check command path for positional argument context.
    //

    if completing_idx >= 2 {
        let (cmd, sub) = (tokens[0], tokens[1]);
        match (cmd, sub) {
            ("node", "select") if completing_idx == 2 => return CompletionContext::NodeId,
            ("node", "reset") if completing_idx == 2 => return CompletionContext::NodeId,
            ("agent", "select") if completing_idx >= 2 => return CompletionContext::AgentName,
            ("op", "run") if completing_idx == 2 => return CompletionContext::OpName,
            ("op", "info") | ("op", "cancel") if completing_idx == 2 => return CompletionContext::ShortId,
            ("session", "create") if completing_idx == 2 => return CompletionContext::ProjectPath,
            ("recon", "config-read") | ("recon", "config-grep") if completing_idx == 2 => {
                return CompletionContext::ConfigPath;
            }
            ("recon", "session-read") | ("recon", "session-grep") if completing_idx == 2 => {
                return CompletionContext::SessionPath;
            }
            ("recon", "list") if completing_idx == 2 => {
                return CompletionContext::ReconSection;
            }
            _ => {}
        }
    }

    CompletionContext::Command
}

struct PraxisCompleter {
    commands: Vec<Vec<String>>,
    cache: Arc<Mutex<CompletionCache>>,
    colored_prompt: Arc<Mutex<String>>,
}

impl PraxisCompleter {
    fn new(cache: Arc<Mutex<CompletionCache>>, colored_prompt: Arc<Mutex<String>>) -> Self {
        let cmd = ReplCli::command();
        let mut paths = Vec::new();
        Self::collect_paths(&cmd, &mut Vec::new(), &mut paths);

        for builtin in ["help", "exit", "quit", "clear"] {
            paths.push(vec![builtin.to_string()]);
        }

        Self { commands: paths, cache, colored_prompt }
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

    fn dynamic_candidates(&self, context: &CompletionContext) -> Vec<String> {
        let Ok(cache) = self.cache.lock() else {
            return Vec::new();
        };
        match context {
            CompletionContext::NodeId => cache.node_ids.clone(),
            CompletionContext::AgentName => cache.agent_names.clone(),
            CompletionContext::OpName => {
                let mut names = cache.op_names.clone();
                names.extend(cache.chain_names.clone());
                names
            }
            CompletionContext::ShortId => {
                let mut ids = cache.op_short_ids.clone();
                ids.extend(cache.chain_exec_ids.clone());
                ids
            }
            CompletionContext::ProjectPath => cache.project_paths.clone(),
            CompletionContext::ConfigPath => cache.config_paths.clone(),
            CompletionContext::SessionPath => cache.session_paths.clone(),
            CompletionContext::ReconSection => {
                vec!["all", "sessions", "tools", "projects", "configs"]
                    .into_iter().map(|s| s.to_string()).collect()
            }
            CompletionContext::Command => Vec::new(),
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

        //
        // Check if we should complete a dynamic value instead of a command.
        //

        let all_tokens: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
        let context = detect_context(&all_tokens, trailing_space);

        if !matches!(context, CompletionContext::Command) {
            let candidates = self.dynamic_candidates(&context);
            let start = pos - partial.len();
            let pairs = candidates
                .into_iter()
                .filter(|c| c.starts_with(partial))
                .map(|c| Pair {
                    display: c.clone(),
                    replacement: c,
                })
                .collect();
            return Ok((start, pairs));
        }

        //
        // Command name completion.
        //

        let mut candidates: Vec<String> = Vec::new();

        for path in &self.commands {
            if path.len() <= depth {
                continue;
            }

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
impl Highlighter for PraxisCompleter {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> std::borrow::Cow<'b, str> {
        if prompt.starts_with("praxis") {
            let colored = self.colored_prompt.lock().unwrap().clone();
            std::borrow::Cow::Owned(colored)
        } else {
            std::borrow::Cow::Borrowed(prompt)
        }
    }
}
impl Validator for PraxisCompleter {}
impl Helper for PraxisCompleter {}

fn history_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".praxis").join("history"))
}

fn print_banner(client_id_short: &str, node_count: usize, rabbitmq_url: &str) {
    println!(
        r#"
    ____                  _
   / __ \_________ __  __(_)____
  / /_/ / ___/ __ `/ |/_/ / ___/
 / ____/ /  / /_/ />  </ (__  )
/_/   /_/   \__,_/_/|_/_/____/
"#
    );
    println!(
        "  {} {} | client {} | {} node(s)",
        "praxis".bold(),
        env!("CARGO_PKG_VERSION"),
        client_id_short.cyan(),
        node_count
    );
    println!("  {}", rabbitmq_url.dimmed());
    println!("  Type {} for commands, {} (or {}) to quit\n", "help".bold(), "exit".bold(), "ctrl+d".bold());
}

fn print_help() {
    println!("\n{}\n", "Commands".bold().underline());

    let cmds = [
        ("node list", "List connected nodes"),
        ("node select <node>", "Select a node"),
        ("node reset <node>", "Reset a node (cancel ops, re-register)"),
        ("agent list", "List agents on selected node"),
        ("agent select <agent>", "Select an agent"),
        ("agent update", "Request agent info update"),
        ("recon run", "Run recon on selected node"),
        ("recon run-semantic", "Run semantic recon on selected node"),
        ("recon list", "List all stored recon data"),
        ("recon list <section>", "List sessions/tools/projects/configs"),
        ("recon config-read [path]", "Read config file (omit for all)"),
        ("recon session-read [path]", "Read session file (omit for all)"),
        ("recon config-grep <pattern> [path]", "Grep config file (omit for all)"),
        ("recon session-grep <pattern> [path]", "Grep session file (omit for all)"),
        ("session create", "Create a session"),
        ("session prompt <text>", "Send a prompt"),
        ("session prompt", "Interactive prompt mode"),
        ("session close", "Close a session"),
        ("traffic search <pattern>", "Search intercepted traffic"),
        ("op available", "List available operations and chains"),
        ("op run <name>", "Run an operation or chain"),
        ("op list", "List tracked operations/chains"),
        ("op info <id>", "Show operation/chain info"),
        ("op cancel <id>", "Cancel an operation/chain"),
        ("op definition <name>", "Show operation or chain definition"),
        ("orchestrate", "Interactive LLM orchestrator session"),
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

//
// Determine whether a command token sequence needs a -n (node) flag.
// Agent and session commands always need it. Op only needs it for "run".
//

fn needs_node_flag(tokens: &[String]) -> bool {
    if tokens.len() < 2 {
        return false; // no subcommand yet, don't inject flags
    }
    let cmd = tokens[0].as_str();
    let sub = tokens[1].as_str();
    match cmd {
        "agent" | "session" | "recon" => true,
        "op" => sub == "run",
        _ => false,
    }
}

//
// Determine whether a command token sequence needs an -a (agent) flag.
//

fn needs_agent_flag(tokens: &[String]) -> bool {
    if tokens.len() < 2 {
        return false;
    }
    tokens[0] == "op" && tokens[1] == "run"
}

fn inject_defaults(tokens: &mut Vec<String>, state: &ReplState) {
    let has_node = tokens.iter().any(|t| t == "-n" || t == "--node");
    let has_agent = tokens.iter().any(|t| t == "-a" || t == "--agent");

    if !has_node && needs_node_flag(tokens) {
        if let Some(ref node) = state.selected_node {
            tokens.push("-n".to_string());
            tokens.push(format_short_id(node));
        }
    }
    if !has_agent && needs_agent_flag(tokens) {
        if let Some(ref agent) = state.selected_agent {
            tokens.push("-a".to_string());
            tokens.push(agent.clone());
        }
    }
}

//
// After a successful command, update ReplState based on what was run.
//

//
// Handle node select from tokens (client-side state only).
//

fn handle_node_select(
    tokens: &[String],
    state: &mut ReplState,
    sys_state: Option<&common::SystemState>,
) {
    if let Some(prefix) = tokens.get(2) {
        if let Some(sys_state) = sys_state {
            let search = prefix.to_lowercase();
            if let Some(node) = sys_state.nodes.iter()
                .find(|n| n.node_id.to_lowercase().starts_with(&search))
            {
                state.selected_node = Some(node.node_id.clone());
                state.selected_machine_name = Some(node.machine_name.clone());
            }
        }
    }
}

//
// Sync REPL state (agent, session) from the server's SystemState.
// This ensures the prompt always reflects reality regardless of
// whether commands succeeded or failed.
//

fn sync_repl_state(state: &mut ReplState, sys_state: Option<&common::SystemState>) {
    let Some(sys_state) = sys_state else { return };
    let Some(ref node_id) = state.selected_node else {
        state.selected_machine_name = None;
        state.selected_agent = None;
        state.has_session = false;
        return;
    };

    let Some(node) = sys_state.nodes.iter().find(|n| n.node_id == *node_id) else {
        state.selected_node = None;
        state.selected_machine_name = None;
        state.selected_agent = None;
        state.has_session = false;
        return;
    };

    state.selected_machine_name = Some(node.machine_name.clone());

    if let Some(ref agent) = node.selected_agent {
        state.selected_agent = Some(agent.short_name.clone());
        state.has_session = agent.session_id.is_some();
    } else {
        state.selected_agent = None;
        state.has_session = false;
    }
}

fn is_session_create(tokens: &[String]) -> bool {
    tokens.first().map(|s| s.as_str()) == Some("session")
        && tokens.get(1).map(|s| s.as_str()) == Some("create")
}

fn has_project_flag(tokens: &[String]) -> bool {
    tokens.iter().any(|t| t == "-p" || t == "--project")
}

//
// Handle interactive project selection for "session create":
// - Bare positional path: "session create /foo" → inject -p flag
// - No project: show picker from recon data
//

async fn handle_session_create_project(
    tokens: &mut Vec<String>,
    repl_state: &ReplState,
    client: &crate::client::CliClient,
    rl: &mut Editor<PraxisCompleter, DefaultHistory>,
) {
    if has_project_flag(tokens) {
        return;
    }

    //
    // Check for bare positional: "session create /some/path" or
    // "session create ~/project". Walk tokens after "create", skipping
    // flags and their values, to find a bare positional argument.
    //

    let known_flags = ["-n", "--node", "-y", "--yolo", "-p", "--project"];
    let mut positional: Option<(usize, String)> = None;
    let mut i = 2;
    while i < tokens.len() {
        let t = &tokens[i];
        if t.starts_with('-') {
            let takes_value = known_flags.contains(&t.as_str()) && *t != "-y" && *t != "--yolo";
            if takes_value {
                i += 1; // skip the flag's value
            }
        } else {
            positional = Some((i, t.clone()));
            break;
        }
        i += 1;
    }

    if let Some((idx, path)) = positional {
        tokens.remove(idx);
        tokens.push("-p".to_string());
        tokens.push(path);
        return;
    }

    //
    // No project specified — try to show an interactive picker from the
    // latest recon result.
    //

    let (Some(node_id), Some(agent)) = (&repl_state.selected_node, &repl_state.selected_agent) else {
        return;
    };

    let recon = match client.get_recon_result(node_id, agent).await {
        Ok(Some(r)) => r,
        _ => return,
    };

    if recon.project_paths.is_empty() {
        return;
    }

    println!();
    println!("  {}", "Select a project:".bold());
    for (i, path) in recon.project_paths.iter().enumerate() {
        println!("  {}  {}", format!("[{}]", i + 1).cyan(), path);
    }
    println!("  {}  {}", "[0]".cyan(), "(no project)");
    println!();

    let selection: String = match rl.readline("  Choice: ") {
        Ok(line) => line.trim().to_string(),
        Err(_) => return,
    };

    if selection.is_empty() || selection == "0" {
        return;
    }

    if let Ok(idx) = selection.parse::<usize>() {
        if idx >= 1 && idx <= recon.project_paths.len() {
            let path = recon.project_paths[idx - 1].clone();
            tokens.push("-p".to_string());
            tokens.push(path);
        }
    }
}

//
// Generic interactive picker — shows a numbered list and returns the chosen
// value, or None if the user cancels.
//

fn interactive_pick(
    items: &[String],
    prompt_msg: &str,
    rl: &mut Editor<PraxisCompleter, DefaultHistory>,
) -> Option<String> {
    if items.is_empty() {
        return None;
    }

    println!();
    println!("  {}", prompt_msg.bold());
    for (i, item) in items.iter().enumerate() {
        println!("  {}  {}", format!("[{}]", i + 1).cyan(), item);
    }
    println!("  {}  {}", "[0]".cyan(), "(cancel)");
    println!();

    let selection: String = match rl.readline("  Choice: ") {
        Ok(line) => line.trim().to_string(),
        Err(_) => return None,
    };

    if selection.is_empty() || selection == "0" {
        return None;
    }

    if let Ok(idx) = selection.parse::<usize>() {
        if idx >= 1 && idx <= items.len() {
            return Some(items[idx - 1].clone());
        }
    }
    None
}

//
// Intercept recon config-read/session-read/config-grep/session-grep — if the
// positional path arg is missing, show an interactive picker from cached paths.
//

fn is_recon_needs_path(tokens: &[String]) -> Option<&'static str> {
    if tokens.first().map(|s| s.as_str()) != Some("recon") {
        return None;
    }
    match tokens.get(1).map(|s| s.as_str()) {
        Some("config-read") | Some("config-grep") => Some("config"),
        Some("session-read") | Some("session-grep") => Some("session"),
        _ => None,
    }
}

fn count_positionals(tokens: &[String]) -> usize {
    let mut count = 0;
    let mut i = 2;
    while i < tokens.len() {
        let t = &tokens[i];
        if t.starts_with('-') {
            if t == "-n" || t == "--node"
                || t == "-a" || t == "--agent"
                || t == "--line-start" || t == "--line-end"
            {
                i += 1; // skip flag value
            }
        } else {
            count += 1;
        }
        i += 1;
    }
    count
}

fn is_grep_command(tokens: &[String]) -> bool {
    matches!(tokens.get(1).map(|s| s.as_str()), Some("config-grep") | Some("session-grep"))
}

//
// For read commands: path is the first positional, so missing when count == 0.
// For grep commands: pattern is first, path is second, so missing when count < 2.
//

fn needs_path_picker(tokens: &[String]) -> bool {
    if is_recon_needs_path(tokens).is_none() {
        return false;
    }
    let n = count_positionals(tokens);
    if is_grep_command(tokens) {
        n == 1 // has pattern, needs path
    } else {
        n == 0 // needs path
    }
}

async fn handle_recon_path_picker(
    tokens: &mut Vec<String>,
    repl_state: &ReplState,
    client: &CliClient,
    rl: &mut Editor<PraxisCompleter, DefaultHistory>,
) {
    let Some(path_type) = is_recon_needs_path(tokens) else { return };
    if !needs_path_picker(tokens) { return; }

    let (Some(node_id), Some(agent)) = (&repl_state.selected_node, &repl_state.selected_agent) else {
        return;
    };

    let recon = match client.get_recon_result(node_id, agent).await {
        Ok(Some(r)) => r,
        _ => return,
    };

    let file_paths: Vec<String> = match path_type {
        "config" => recon.config.iter().map(|c| c.path.clone()).collect(),
        "session" => recon.sessions.iter().map(|s| s.session_file.clone()).collect(),
        _ => return,
    };

    //
    // Build picker items with * (all) at the top.
    //

    let mut items = vec!["* (all)".to_string()];
    items.extend(file_paths);

    let label = if path_type == "config" { "Select a config file:" } else { "Select a session file:" };
    if let Some(picked) = interactive_pick(&items, label, rl) {
        if !picked.starts_with("* ") {
            tokens.push(picked);
        }
        // picking * leaves path absent → handler reads/greps all files
    }
}

//
// Intercept "recon list" with no section — show a picker.
//

fn handle_recon_list_picker(
    tokens: &mut Vec<String>,
    rl: &mut Editor<PraxisCompleter, DefaultHistory>,
) {
    if tokens.first().map(|s| s.as_str()) != Some("recon") { return; }
    if tokens.get(1).map(|s| s.as_str()) != Some("list") { return; }
    if count_positionals(tokens) > 0 { return; }

    let sections = vec![
        "all".to_string(),
        "sessions".to_string(),
        "tools".to_string(),
        "projects".to_string(),
        "configs".to_string(),
    ];

    if let Some(section) = interactive_pick(&sections, "Select a section:", rl) {
        tokens.push(section);
    }
}

//
// Intercept "agent select" with no short_name — show a picker.
//

async fn handle_agent_select_picker(
    tokens: &mut Vec<String>,
    repl_state: &ReplState,
    client: &CliClient,
    rl: &mut Editor<PraxisCompleter, DefaultHistory>,
) {
    if tokens.first().map(|s| s.as_str()) != Some("agent") { return; }
    if tokens.get(1).map(|s| s.as_str()) != Some("select") { return; }

    //
    // Check if there's already a positional agent name.
    //

    if count_positionals(tokens) > 0 { return; }

    let Some(ref node_id) = repl_state.selected_node else { return };

    let state = match client.get_state().await {
        Some(s) => s,
        None => return,
    };

    let Some(node) = state.nodes.iter().find(|n| n.node_id == *node_id) else { return };

    let agents: Vec<String> = node.discovered_agents.iter()
        .filter(|a| a.available)
        .map(|a| a.short_name.clone())
        .collect();

    if let Some(name) = interactive_pick(&agents, "Select an agent:", rl) {
        tokens.push(name);
    }
}

//
// Intercept "node select" with no prefix — show a picker.
//

async fn handle_node_select_picker(
    tokens: &mut Vec<String>,
    client: &CliClient,
    rl: &mut Editor<PraxisCompleter, DefaultHistory>,
) {
    if tokens.first().map(|s| s.as_str()) != Some("node") { return; }
    if tokens.get(1).map(|s| s.as_str()) != Some("select") { return; }
    if tokens.len() > 2 { return; }

    let state = match client.get_state().await {
        Some(s) => s,
        None => return,
    };

    let nodes: Vec<String> = state.nodes.iter()
        .map(|n| {
            let short = &n.node_id[..8.min(n.node_id.len())];
            format!("{} ({})", short, n.machine_name)
        })
        .collect();

    let node_ids: Vec<String> = state.nodes.iter()
        .map(|n| format_short_id(&n.node_id))
        .collect();

    if nodes.is_empty() { return; }

    println!();
    println!("  {}", "Select a node:".bold());
    for (i, label) in nodes.iter().enumerate() {
        println!("  {}  {}", format!("[{}]", i + 1).cyan(), label);
    }
    println!("  {}  {}", "[0]".cyan(), "(cancel)");
    println!();

    let selection: String = match rl.readline("  Choice: ") {
        Ok(line) => line.trim().to_string(),
        Err(_) => return,
    };

    if selection.is_empty() || selection == "0" { return; }

    if let Ok(idx) = selection.parse::<usize>() {
        if idx >= 1 && idx <= node_ids.len() {
            tokens.push(node_ids[idx - 1].clone());
        }
    }
}

pub async fn run_repl(rabbitmq_url: &str, timeout: u64, output: OutputFormat) -> Result<()> {
    //
    // Install the SIGINT handler early so Ctrl+C during command execution
    // doesn't terminate the process. Rustyline saves/restores signal handlers
    // around readline(), so this must be installed before rustyline starts
    // to stay in the handler chain.
    //

    #[cfg(unix)]
    let _sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    let mut cli_state = crate::state::CliState::load()?;
    let client_id = cli_state.get_or_create_client_id()?;
    let short_id = client_id[..8.min(client_id.len())].to_string();

    let mut client = CliClient::connect(rabbitmq_url, timeout, client_id).await?;

    let system_state = client.get_state().await;
    let node_count = system_state.as_ref().map(|s| s.nodes.len()).unwrap_or(0);

    print_banner(&short_id, node_count, rabbitmq_url);

    let cache = Arc::new(Mutex::new(CompletionCache::default()));
    let colored_prompt = Arc::new(Mutex::new(String::new()));
    let mut repl_state = ReplState::default();

    refresh_completion_cache(&client, &cache, repl_state.selected_node.as_deref(), repl_state.selected_agent.as_deref()).await;

    let config = Config::builder()
        .completion_type(CompletionType::List)
        .build();

    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(PraxisCompleter::new(Arc::clone(&cache), Arc::clone(&colored_prompt))));

    if let Some(path) = history_path() {
        let _ = rl.load_history(&path);
    }

    //
    // Auto-select if there's exactly one active node (seen within 60s).
    // Populate agent and session state from it if available.
    //

    if let Some(ref state) = system_state {
        let now = chrono::Utc::now();
        let active_nodes: Vec<_> = state.nodes.iter()
            .filter(|n| {
                let age = now.signed_duration_since(n.last_update);
                age.num_seconds() < 60
            })
            .collect();

        if active_nodes.len() == 1 {
            let node = active_nodes[0];
            repl_state.selected_node = Some(node.node_id.clone());
            repl_state.selected_machine_name = Some(node.machine_name.clone());

            if let Some(ref agent) = node.selected_agent {
                repl_state.selected_agent = Some(agent.short_name.clone());
                repl_state.has_session = agent.session_id.is_some();
            }
        }
    }

    loop {
        let (plain_prompt, color_prompt) = repl_state.build_prompt();
        *colored_prompt.lock().unwrap() = color_prompt;

        match rl.readline(&plain_prompt) {
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

                let mut tokens = shell_split(trimmed);
                inject_defaults(&mut tokens, &repl_state);

                //
                // Intercept "session create" for interactive project selection
                // and bare positional path support.
                //

                //
                // Interactive pickers for commands missing positional args.
                //

                if is_session_create(&tokens) {
                    handle_session_create_project(
                        &mut tokens,
                        &repl_state,
                        &client,
                        &mut rl,
                    ).await;
                }

                handle_recon_path_picker(&mut tokens, &repl_state, &client, &mut rl).await;
                handle_recon_list_picker(&mut tokens, &mut rl);
                handle_agent_select_picker(&mut tokens, &repl_state, &client, &mut rl).await;
                handle_node_select_picker(&mut tokens, &client, &mut rl).await;

                match ReplCli::try_parse_from(&tokens) {
                    Ok(parsed) => {
                        println!();
                        let result = parsed.command.execute(&mut client, &output).await;

                        let sys_state = client.get_state().await;

                        if result.is_ok() {
                            println!();
                            if tokens.first().map(|s| s.as_str()) == Some("node")
                                && tokens.get(1).map(|s| s.as_str()) == Some("select")
                            {
                                handle_node_select(&tokens, &mut repl_state, sys_state.as_ref());
                            }
                        }

                        //
                        // Always sync agent/session state from the server so
                        // the prompt reflects reality after every command.
                        //

                        sync_repl_state(&mut repl_state, sys_state.as_ref());

                        if let Err(e) = result {
                            crate::output::print_error(&e.to_string());
                            println!();
                        }
                    }
                    Err(e) => {
                        let kind = e.kind();
                        if matches!(kind, clap::error::ErrorKind::InvalidSubcommand) {
                            println!("Unknown command. Type 'help' for available commands.");
                        } else {
                            //
                            // Print clap's usage/help, filtering out lines
                            // that reference --help (not relevant in the REPL).
                            //
                            let msg = e.to_string();
                            for line in msg.lines() {
                                let trimmed = line.trim();
                                if trimmed.starts_with("-h,")
                                    || trimmed == "Options:"
                                    || trimmed.contains("try '--help'")
                                    || trimmed.contains("try 'help'")
                                {
                                    continue;
                                }
                                println!("{}", line);
                            }
                        }
                    }
                }

                refresh_completion_cache(&client, &cache, repl_state.selected_node.as_deref(), repl_state.selected_agent.as_deref()).await;
            }
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => {
                println!();
                break;
            }
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
