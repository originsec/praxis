use anyhow::{Result, anyhow};
use clap::{Args, Subcommand, ValueEnum};
use common::exposure::{
    AgentExposure, CapabilityClass, ExposureOptions, ExposureReport, RiskLevel, SharedEntity,
    TrifectaPath,
};

use crate::client::Client;
use crate::output::{format_short_id, print_header, print_success};

#[derive(Args, Clone, Copy)]
pub struct RefreshArgs {
    /// Classify capabilities no rule matched with the Semantic Parser LLM (cached)
    #[arg(long)]
    llm: bool,
    /// Read agent settings on online nodes to detect YOLO / auto-approve (cached)
    #[arg(long)]
    probe: bool,
}

impl From<RefreshArgs> for ExposureOptions {
    fn from(args: RefreshArgs) -> Self {
        Self {
            llm: args.llm,
            probe_configs: args.probe,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ExportFormat {
    Dot,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum RiskArg {
    Low,
    Medium,
    High,
    Critical,
}

impl From<RiskArg> for RiskLevel {
    fn from(r: RiskArg) -> Self {
        match r {
            RiskArg::Low => RiskLevel::Low,
            RiskArg::Medium => RiskLevel::Medium,
            RiskArg::High => RiskLevel::High,
            RiskArg::Critical => RiskLevel::Critical,
        }
    }
}

#[derive(Subcommand)]
pub enum ExposureCommand {
    /// Lethal-trifecta summary across all agents with stored recon
    Summary {
        #[command(flatten)]
        refresh: RefreshArgs,
        /// Print JSON instead of text
        #[arg(long)]
        json: bool,
    },

    /// Capabilities, classes, modifiers and paths for one agent
    Agent {
        /// Node ID prefix
        node: String,
        /// Agent short name
        agent: String,
        #[command(flatten)]
        refresh: RefreshArgs,
        #[arg(long)]
        json: bool,
    },

    /// Trifecta attack paths and MCP servers / hosts shared across agents
    Paths {
        /// Only show paths at or above this risk
        #[arg(long, value_enum)]
        min_risk: Option<RiskArg>,
        #[command(flatten)]
        refresh: RefreshArgs,
        #[arg(long)]
        json: bool,
    },

    /// Export the exposure graph (Graphviz DOT) or full report (JSON)
    Export {
        #[arg(long, value_enum, default_value = "dot")]
        format: ExportFormat,
        /// Write to this file instead of stdout
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
        #[command(flatten)]
        refresh: RefreshArgs,
    },

    /// Show, replace or reset the exposure rules file
    Rules {
        #[command(subcommand)]
        command: RulesCommand,
    },
}

#[derive(Subcommand)]
pub enum RulesCommand {
    /// Print the current rules file
    Show,
    /// Replace the rules file with the contents of FILE (validated first)
    Set { file: std::path::PathBuf },
    /// Restore the built-in default rules
    Reset,
}

pub async fn execute(client: &Client, command: ExposureCommand) -> Result<()> {
    match command {
        ExposureCommand::Summary { refresh, json } => {
            let report = client.request_exposure_report(refresh.into()).await?;
            if json {
                print_json(&serde_json::json!({
                    "summary": report.summary(),
                    "agents": report.agents.iter().map(agent_row_json).collect::<Vec<_>>(),
                    "warnings": report.warnings,
                }))
            } else {
                print_summary(&report);
                Ok(())
            }
        }
        ExposureCommand::Agent {
            node,
            agent,
            refresh,
            json,
        } => {
            let report = client.request_exposure_report(refresh.into()).await?;
            let found = report.find_agent(&node, &agent).map_err(|e| anyhow!(e))?;
            if json {
                print_json(&serde_json::json!({
                    "agent": found,
                    "paths": report.paths_for_agent(&found.id),
                    "shared": report.shared_for_agent(&found.id),
                }))
            } else {
                print_agent(&report, found);
                Ok(())
            }
        }
        ExposureCommand::Paths {
            min_risk,
            refresh,
            json,
        } => {
            let report = client.request_exposure_report(refresh.into()).await?;
            let min = min_risk.map(RiskLevel::from).unwrap_or(RiskLevel::Low);
            let paths: Vec<&TrifectaPath> = report.paths.iter().filter(|p| p.risk >= min).collect();
            if json {
                print_json(&serde_json::json!({ "paths": paths, "shared": report.shared }))
            } else {
                print_paths(&paths, &report.shared.iter().collect::<Vec<_>>());
                Ok(())
            }
        }
        ExposureCommand::Export {
            format,
            output,
            refresh,
        } => {
            let report = client.request_exposure_report(refresh.into()).await?;
            let text = match format {
                ExportFormat::Dot => report.to_dot(),
                ExportFormat::Json => serde_json::to_string_pretty(&report)?,
            };
            match output {
                Some(path) => {
                    std::fs::write(&path, text)?;
                    print_success(&format!("Wrote {}", path.display()));
                }
                None => print!("{}", text),
            }
            Ok(())
        }
        ExposureCommand::Rules { command } => rules(client, command).await,
    }
}

async fn rules(client: &Client, command: RulesCommand) -> Result<()> {
    let state = match command {
        RulesCommand::Show => {
            let state = client.get_exposure_rules().await?;
            print!("{}", state.text);
            if let Some(e) = state.error {
                return Err(anyhow!("Stored rules do not parse: {}", e));
            }
            return Ok(());
        }
        RulesCommand::Set { file } => {
            let text = std::fs::read_to_string(&file)
                .map_err(|e| anyhow!("Failed to read {}: {}", file.display(), e))?;
            client.set_exposure_rules(text).await?
        }
        RulesCommand::Reset => client.reset_exposure_rules().await?,
    };
    match state.error {
        Some(e) => Err(anyhow!("Rules rejected: {}", e)),
        None => {
            print_success(&format!(
                "Exposure rules saved ({} rules)",
                state.rule_count
            ));
            Ok(())
        }
    }
}

fn print_json(value: &serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn agent_row_json(a: &AgentExposure) -> serde_json::Value {
    serde_json::json!({
        "node_id": a.node_id,
        "machine_name": a.machine_name,
        "agent": a.agent_short_name,
        "risk": a.risk,
        "trifecta": a.trifecta,
        "classes": a.badges(),
        "capabilities": a.capabilities.len(),
        "unclassified": a.unclassified_count(),
        "modifiers": a.modifiers.len(),
    })
}

fn node_label(node_id: &str, machine: &str) -> String {
    if machine.is_empty() {
        format_short_id(node_id)
    } else {
        format!("{} ({})", format_short_id(node_id), machine)
    }
}

fn print_summary(report: &ExposureReport) {
    let s = report.summary();
    print_header("Agent Exposure (lethal trifecta)");
    println!(
        "  nodes {}  agents {}  trifecta {}  [critical {}, high {}, medium {}, low {}]",
        s.nodes, s.agents, s.trifecta_agents, s.critical, s.high, s.medium, s.low
    );
    println!(
        "  MCP servers {}  hosts {}  skills {}  shared {} ({} cross-node)",
        s.mcp_servers, s.hosts, s.skills, s.shared_entities, s.cross_node_links
    );
    println!(
        "  capabilities {} (unclassified {}, llm {})  modifiers {}  rules {}{}",
        s.capabilities,
        s.unclassified,
        report.llm_classified,
        s.modifiers,
        report.rule_count,
        if report.probed { "  [probed]" } else { "" }
    );
    println!();

    let mut agents: Vec<&AgentExposure> = report.agents.iter().collect();
    agents.sort_by(|a, b| b.risk.cmp(&a.risk).then_with(|| a.id.cmp(&b.id)));
    if agents.is_empty() {
        println!("  No agents with stored recon. Run recon first.");
    } else {
        println!("  {:<9} {:<6} {:<16} NODE", "RISK", "P U X", "AGENT");
        for a in agents {
            println!(
                "  {:<9} {:<6} {:<16} {}",
                a.risk.as_str(),
                a.badges(),
                a.agent_short_name,
                node_label(&a.node_id, &a.machine_name)
            );
        }
    }
    print_warnings(&report.warnings);
}

fn print_agent(report: &ExposureReport, a: &AgentExposure) {
    print_header(&format!(
        "{} @ {}",
        a.agent_short_name,
        node_label(&a.node_id, &a.machine_name)
    ));
    println!(
        "  risk {}  trifecta {}  recon {} ({})",
        a.risk.as_str(),
        if a.trifecta { "yes" } else { "no" },
        a.performed_at,
        if a.is_semantic { "semantic" } else { "static" }
    );
    for class in CapabilityClass::ALL {
        let contributors = a.contributors(class);
        println!();
        println!("  {} ({})", class.label(), contributors.len());
        for cap in contributors {
            let reason = cap
                .classes
                .iter()
                .find(|c| c.class == class)
                .map(|c| c.reason.as_str())
                .unwrap_or("");
            println!("    {:<48} {}", cap.label(), reason);
        }
    }

    println!();
    match &a.modifiers_probed_at {
        Some(at) => println!("  modifiers (probed {})", at),
        None => println!("  modifiers (configs not probed; use --probe)"),
    }
    for m in &a.modifiers {
        println!(
            "    {:<12} {}  [{} in {}]",
            m.kind.as_str(),
            m.detail,
            m.reason,
            m.source
        );
    }

    let unclassified: Vec<String> = a
        .capabilities
        .iter()
        .filter(|c| c.is_unclassified())
        .map(|c| c.label())
        .collect();
    if !unclassified.is_empty() {
        println!();
        println!("  unclassified ({}; refine with --llm)", unclassified.len());
        for label in unclassified {
            println!("    {}", label);
        }
    }
    if !a.observed_hosts.is_empty() {
        println!();
        println!("  observed hosts: {}", a.observed_hosts.join(", "));
    }

    let paths: Vec<&TrifectaPath> = report.paths_for_agent(&a.id);
    let shared = report.shared_for_agent(&a.id);
    if !paths.is_empty() || !shared.is_empty() {
        print_paths(&paths, &shared);
    }
}

fn print_paths(paths: &[&TrifectaPath], shared: &[&SharedEntity]) {
    print_header("Trifecta paths");
    if paths.is_empty() {
        println!("  none");
    }
    for p in paths {
        println!(
            "  [{}] {} @ {}",
            p.risk.as_str(),
            p.agent_short_name,
            node_label(&p.node_id, &p.machine_name)
        );
        println!("    untrusted in  {}", p.sources.join(", "));
        println!("    private data  {}", p.private_data.join(", "));
        println!("    exfil out     {}", p.exfiltration.join(", "));
        if !p.unattended.is_empty() {
            println!("    unattended    {}", p.unattended.join(", "));
        }
    }

    print_header("Shared MCP servers and hosts");
    if shared.is_empty() {
        println!("  none");
    }
    for s in shared {
        let mut tags = Vec::new();
        if s.cross_node {
            tags.push("cross-node");
        }
        if s.reaches_trifecta {
            tags.push("reaches trifecta");
        }
        println!(
            "  {:<10} {:<32} {} agents, {} nodes {}",
            s.kind.as_str(),
            s.label,
            s.agents.len(),
            s.nodes.len(),
            if tags.is_empty() {
                String::new()
            } else {
                format!("[{}]", tags.join(", "))
            }
        );
    }
}

fn print_warnings(warnings: &[String]) {
    if warnings.is_empty() {
        return;
    }
    println!();
    for w in warnings {
        println!("  ! {}", w);
    }
}
