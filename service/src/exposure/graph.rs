use common::exposure::{
    AgentExposure, AgentModifier, CapabilityClass, CapabilityKind, ClassReason,
    ClassifiedCapability, EdgeKind, EntityKind, ExposureGraph, ExposureReport, GraphEdge,
    GraphEntity, ModifierKind, RiskLevel, SharedEntity, TrifectaPath,
};
use common::{McpServer, ReconResult};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::rules::{RuleSet, Subject};

//
// Pure report construction: stored recon + traffic hosts + cached verdicts
// and modifiers in, ExposureReport out. Kept free of I/O so it can be unit
// tested on synthetic multi-node fixtures.
//

pub struct AgentInput {
    pub node_id: String,
    pub agent_short_name: String,
    pub performed_at: String,
    pub is_semantic: bool,
    pub recon: ReconResult,
}

pub struct TrafficHost {
    pub node_id: String,
    /// Captured agent label; may name several agents separated by '|'.
    pub agent_label: String,
    pub host: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedModifiers {
    pub probed_at: String,
    pub modifiers: Vec<AgentModifier>,
}

#[derive(Default)]
pub struct Inputs {
    pub agents: Vec<AgentInput>,
    pub machine_names: HashMap<String, String>,
    pub traffic_hosts: Vec<TrafficHost>,
    /// (node_id, agent_short_name) -> probed modifiers.
    pub modifiers: HashMap<(String, String), CachedModifiers>,
    /// LLM cache key -> classes (empty = benign verdict).
    pub llm_verdicts: HashMap<String, Vec<CapabilityClass>>,
    pub warnings: Vec<String>,
}

//
// A capability no rule matched and no cached LLM verdict covers.
//

#[derive(Debug, Clone)]
pub struct LlmCandidate {
    pub key: String,
    pub kind: CapabilityKind,
    pub server: Option<String>,
    pub name: String,
    pub description: String,
}

fn llm_key(
    kind: CapabilityKind,
    server: Option<&str>,
    name: &str,
    description: &str,
) -> String {
    format!(
        "llm:{}:{}:{}:{}",
        kind.as_str(),
        server.unwrap_or(""),
        name,
        common::truncate_str(description.trim(), 300)
    )
}

fn agent_entity_id(node_id: &str, agent: &str) -> String {
    format!("agent:{}/{}", node_id, agent)
}

// ---------------------------------------------------------------------------
// MCP server identity
// ---------------------------------------------------------------------------

//
// Identity used to deduplicate the same MCP server across agents and nodes:
// the normalised URL for remote servers, otherwise the normalised command
// line, falling back to the lower-cased name when neither is known.
// Loopback URLs point at something local to each node, so they are scoped
// to the node and never merged across nodes.
//

fn mcp_server_key(server: &McpServer, node_id: &str) -> String {
    let non_empty = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    if let Some(addr) = non_empty(&server.address) {
        let key = format!("url:{}", normalize_url(&addr));
        return match url_host(&addr) {
            Some(host) if is_loopback(&host) => format!("{}@{}", key, node_id),
            _ => key,
        };
    }
    if let Some(cmd) = non_empty(&server.command) {
        return format!("cmd:{}", normalize_command(&cmd));
    }
    format!("name:{}", server.name.trim().to_lowercase())
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    host == "localhost"
        || host.ends_with(".localhost")
        || host == "::1"
        || host == "0.0.0.0"
        || host.starts_with("127.")
}

fn host_entity_id(host: &str, node_id: &str) -> String {
    if is_loopback(host) {
        format!("host:{}@{}", host, node_id)
    } else {
        format!("host:{}", host)
    }
}

//
// host[:port]/path with scheme, credentials, query and fragment dropped —
// query strings frequently carry API keys.
//

fn normalize_url(raw: &str) -> String {
    match reqwest::Url::parse(raw.trim()) {
        Ok(url) => {
            let host = url.host_str().unwrap_or("").to_lowercase();
            let port = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
            let path = url.path().trim_end_matches('/');
            format!("{}{}{}", host, port, path)
        }
        Err(_) => raw
            .trim()
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .trim_end_matches('/')
            .to_lowercase(),
    }
}

fn url_host(raw: &str) -> Option<String> {
    reqwest::Url::parse(raw.trim())
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .filter(|h| !h.is_empty())
}

const LAUNCHERS: &[&str] = &[
    "npx", "uvx", "bunx", "pnpx", "pnpm", "npm", "bun", "yarn", "uv",
];
const LAUNCHER_NOISE: &[&str] = &["-y", "--yes", "-q", "--quiet", "--silent"];

fn is_secret_name(name: &str) -> bool {
    let n = name.trim_start_matches('-').to_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "passwd",
        "auth",
        "credential",
    ]
    .iter()
    .any(|s| n.contains(s))
}

//
// Program basename without extension, launcher noise flags removed, package
// version pins dropped (`pkg@1.2.3`, `@scope/pkg@latest`) and obvious secret
// values redacted so the key is safe to show and export.
//

fn normalize_command(cmd: &str) -> String {
    let mut tokens = cmd.split_whitespace();
    let Some(first) = tokens.next() else {
        return String::new();
    };
    let base = first
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(first)
        .to_lowercase();
    let program = [".exe", ".cmd", ".bat", ".ps1"]
        .iter()
        .find_map(|ext| base.strip_suffix(ext))
        .unwrap_or(&base)
        .to_string();
    let is_launcher = LAUNCHERS.contains(&program.as_str());

    let mut out = vec![program];
    let mut redact_next = false;
    for token in tokens {
        if redact_next {
            out.push("***".to_string());
            redact_next = false;
            continue;
        }
        if is_launcher && LAUNCHER_NOISE.contains(&token) {
            continue;
        }
        if let Some((lhs, _)) = token.split_once('=')
            && is_secret_name(lhs)
        {
            out.push(format!("{}=***", lhs));
            continue;
        }
        if token.starts_with('-') {
            redact_next = is_secret_name(token);
            out.push(token.to_string());
            continue;
        }
        out.push(strip_package_version(token).replace('\\', "/"));
    }
    out.join(" ")
}

fn strip_package_version(token: &str) -> &str {
    let (scope_len, rest) = match token.strip_prefix('@') {
        Some(rest) => (1, rest),
        None => (0, token),
    };
    match rest.rfind('@') {
        Some(at) => {
            let version = &rest[at + 1..];
            let looks_like_version = version
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || c == '^' || c == '~')
                || matches!(version, "latest" | "next");
            if looks_like_version {
                &token[..scope_len + at]
            } else {
                token
            }
        }
        None => token,
    }
}

// ---------------------------------------------------------------------------
// Report construction
// ---------------------------------------------------------------------------

struct GraphBuilder {
    entities: BTreeMap<String, GraphEntity>,
    edges: BTreeSet<GraphEdge>,
}

impl GraphBuilder {
    fn entity(&mut self, id: &str, kind: EntityKind, label: &str) -> &mut GraphEntity {
        self.entities
            .entry(id.to_string())
            .or_insert_with(|| GraphEntity {
                id: id.to_string(),
                kind,
                label: label.to_string(),
                classes: Vec::new(),
                attrs: BTreeMap::new(),
            })
    }

    fn edge(&mut self, from: &str, to: &str, kind: EdgeKind) {
        self.edges.insert(GraphEdge {
            from: from.to_string(),
            to: to.to_string(),
            kind,
        });
    }

    fn add_classes(&mut self, id: &str, classes: impl Iterator<Item = CapabilityClass>) {
        if let Some(entity) = self.entities.get_mut(id) {
            for class in classes {
                if !entity.classes.contains(&class) {
                    entity.classes.push(class);
                }
            }
            entity.classes.sort();
        }
    }
}

struct Classifier<'a> {
    rules: &'a RuleSet,
    llm_verdicts: &'a HashMap<String, Vec<CapabilityClass>>,
    candidates: BTreeMap<String, LlmCandidate>,
    llm_classified: usize,
}

impl Classifier<'_> {
    fn classify(&mut self, subject: Subject, context_path: Option<String>) -> ClassifiedCapability {
        let result = self.rules.classify(&subject);
        let mut cap = ClassifiedCapability {
            kind: subject.kind,
            name: subject.name.to_string(),
            server: subject.server.map(str::to_string),
            context_path,
            classes: result.classes,
            verdict: None,
        };
        if let Some(rule) = result.matched {
            if cap.classes.is_empty() {
                cap.verdict = Some(rule);
            }
            return cap;
        }

        let key = llm_key(
            subject.kind,
            subject.server,
            subject.name,
            subject.description,
        );
        match self.llm_verdicts.get(&key) {
            Some(classes) => {
                self.llm_classified += 1;
                cap.classes = classes
                    .iter()
                    .map(|c| ClassReason {
                        class: *c,
                        reason: "llm".to_string(),
                    })
                    .collect();
                if cap.classes.is_empty() {
                    cap.verdict = Some("llm".to_string());
                }
            }
            None => {
                self.candidates.entry(key.clone()).or_insert(LlmCandidate {
                    key,
                    kind: subject.kind,
                    server: subject.server.map(str::to_string),
                    name: subject.name.to_string(),
                    description: subject.description.to_string(),
                });
            }
        }
        cap
    }
}

//
// Approval modifiers are matched against capabilities by name: Claude-style
// `mcp__<server>__<tool>` entries, Gemini `mcpServers.<server>.trust`, and
// `Tool` / `Tool(args)` entries for built-ins. A bare (or wildcard) entry
// approves the whole tool. A scoped entry such as `Bash(ls:*)` only counts
// when its command can reach the network or run arbitrary code on its own
// (UNATTENDED_COMMANDS), so routine allow-lists do not mark every agent
// critical. Other scoped entries are still reported as modifiers.
//

const UNATTENDED_COMMANDS: &[&str] = &[
    "curl",
    "wget",
    "nc",
    "ncat",
    "netcat",
    "ssh",
    "scp",
    "sftp",
    "rsync",
    "ftp",
    "telnet",
    "git push",
    "gh",
    "http",
    "https",
    "httpie",
    "python",
    "python3",
    "node",
    "npx",
    "deno",
    "bun",
    "bash",
    "sh",
    "zsh",
    "pwsh",
    "powershell",
    "ruby",
    "perl",
    "php",
    "eval",
];

fn scoped_command_is_unattended(args: &str) -> bool {
    let args = args
        .trim()
        .trim_end_matches('*')
        .trim_end_matches(':')
        .trim()
        .to_lowercase();
    args.is_empty()
        || UNATTENDED_COMMANDS.iter().any(|cmd| {
            args == *cmd
                || args.starts_with(&format!("{} ", cmd))
                || args.starts_with(&format!("{}:", cmd))
        })
}

fn auto_approved(cap: &ClassifiedCapability, detail: &str) -> bool {
    let detail = detail.trim();
    let server_matches = |server: &str| match cap.kind {
        CapabilityKind::McpTool => cap.server.as_deref() == Some(server),
        CapabilityKind::McpServer => cap.name == server,
        _ => false,
    };
    if let Some(rest) = detail.strip_prefix("mcp__") {
        let mut parts = rest.splitn(2, "__");
        let server = parts.next().unwrap_or("");
        return match parts.next() {
            None | Some("*") => server_matches(server),
            Some(tool) => {
                server_matches(server)
                    && (cap.kind == CapabilityKind::McpServer || cap.name == tool)
            }
        };
    }
    if let Some(rest) = detail.strip_prefix("mcpServers.") {
        return server_matches(rest.split('.').next().unwrap_or(""));
    }
    let (tool, args) = match detail.split_once('(') {
        Some((tool, rest)) => (tool.trim(), Some(rest.trim_end_matches(')'))),
        None => (detail, None),
    };
    matches!(cap.kind, CapabilityKind::Builtin | CapabilityKind::Internal)
        && !tool.is_empty()
        && cap.name.eq_ignore_ascii_case(tool)
        && args.is_none_or(scoped_command_is_unattended)
}

fn assess(
    caps: &[ClassifiedCapability],
    modifiers: &[AgentModifier],
) -> (bool, RiskLevel, Vec<String>) {
    let present: BTreeSet<CapabilityClass> = caps
        .iter()
        .flat_map(|c| c.classes.iter().map(|r| r.class))
        .collect();
    let trifecta = present.len() == CapabilityClass::ALL.len();

    let mut unattended: Vec<String> = modifiers
        .iter()
        .filter(|m| m.kind == ModifierKind::Yolo)
        .map(|m| m.detail.clone())
        .collect();
    for cap in caps.iter().filter(|c| !c.classes.is_empty()) {
        let approved = modifiers
            .iter()
            .filter(|m| m.kind == ModifierKind::AutoApprove)
            .any(|m| auto_approved(cap, &m.detail));
        if approved {
            unattended.push(format!("auto-approved {}", cap.label()));
        }
    }

    let risk = match (trifecta, unattended.is_empty(), present.len()) {
        (true, false, _) => RiskLevel::Critical,
        (true, true, _) => RiskLevel::High,
        (false, _, 2) => RiskLevel::Medium,
        _ => RiskLevel::Low,
    };
    (trifecta, risk, unattended)
}

fn push_unique(caps: &mut Vec<ClassifiedCapability>, cap: ClassifiedCapability) {
    let exists = caps
        .iter()
        .any(|c| c.kind == cap.kind && c.server == cap.server && c.name == cap.name);
    if !exists {
        caps.push(cap);
    }
}

pub fn build_report(rules: &RuleSet, inputs: &Inputs) -> (ExposureReport, Vec<LlmCandidate>) {
    let mut graph = GraphBuilder {
        entities: BTreeMap::new(),
        edges: BTreeSet::new(),
    };
    let mut classifier = Classifier {
        rules,
        llm_verdicts: &inputs.llm_verdicts,
        candidates: BTreeMap::new(),
        llm_classified: 0,
    };
    let mut agents = Vec::new();
    let mut unattended_by_agent: HashMap<String, Vec<String>> = HashMap::new();

    let mut ordered: Vec<&AgentInput> = inputs.agents.iter().collect();
    ordered
        .sort_by(|a, b| (&a.node_id, &a.agent_short_name).cmp(&(&b.node_id, &b.agent_short_name)));

    for input in ordered {
        let node_id = input.node_id.as_str();
        let agent = input.agent_short_name.as_str();
        let machine_name = inputs
            .machine_names
            .get(node_id)
            .cloned()
            .unwrap_or_default();
        let node_entity = format!("node:{}", node_id);
        let agent_id = agent_entity_id(node_id, agent);
        let node_label = if machine_name.is_empty() {
            common::short_id(node_id).to_string()
        } else {
            machine_name.clone()
        };

        graph
            .entity(&node_entity, EntityKind::Node, &node_label)
            .attrs
            .insert("node_id".to_string(), node_id.to_string());
        graph.entity(
            &agent_id,
            EntityKind::Agent,
            &format!("{}@{}", agent, node_label),
        );
        graph.edge(&node_entity, &agent_id, EdgeKind::RunsAgent);

        let mut caps: Vec<ClassifiedCapability> = Vec::new();

        for server in &input.recon.tools.mcp_servers {
            let server_id = format!("mcp:{}", mcp_server_key(server, node_id));
            let url = server
                .address
                .as_deref()
                .map(str::trim)
                .filter(|a| !a.is_empty());
            let command = server.command.as_deref();
            {
                let entity = graph.entity(&server_id, EntityKind::McpServer, &server.name);
                entity
                    .attrs
                    .insert("transport".to_string(), server.transport.to_string());
                entity.attrs.insert(
                    "endpoint".to_string(),
                    server_id.trim_start_matches("mcp:").to_string(),
                );
                if !entity.label.split(" | ").any(|l| l == server.name) {
                    entity.label = format!("{} | {}", entity.label, server.name);
                }
            }
            graph.edge(&agent_id, &server_id, EdgeKind::UsesMcpServer);
            if let Some(host) = url.and_then(url_host) {
                let host_id = host_entity_id(&host, node_id);
                graph.entity(&host_id, EntityKind::Host, &host);
                graph.edge(&server_id, &host_id, EdgeKind::ConnectsTo);
            }

            let mut server_caps = Vec::new();
            if server.tools.is_empty() {
                server_caps.push(classifier.classify(
                    Subject {
                        kind: CapabilityKind::McpServer,
                        agent,
                        name: &server.name,
                        description: "",
                        server: Some(&server.name),
                        command,
                        url,
                    },
                    server.context_path.clone(),
                ));
            }
            for tool in &server.tools {
                server_caps.push(
                    classifier.classify(
                        Subject {
                            kind: CapabilityKind::McpTool,
                            agent,
                            name: &tool.name,
                            description: &tool.description,
                            server: Some(&server.name),
                            command,
                            url,
                        },
                        tool.context_path
                            .clone()
                            .or_else(|| server.context_path.clone()),
                    ),
                );
            }
            graph.add_classes(
                &server_id,
                server_caps
                    .iter()
                    .flat_map(|c| c.classes.iter().map(|r| r.class)),
            );
            for cap in server_caps {
                push_unique(&mut caps, cap);
            }
        }

        for skill in &input.recon.tools.skills {
            let cap = classifier.classify(
                Subject {
                    kind: CapabilityKind::Skill,
                    agent,
                    name: &skill.name,
                    description: &skill.description,
                    server: None,
                    command: None,
                    url: None,
                },
                skill.context_path.clone(),
            );
            if !cap.classes.is_empty() {
                let skill_id = format!("skill:{}/{}/{}", node_id, agent, skill.name);
                graph.entity(&skill_id, EntityKind::Skill, &skill.name);
                graph.add_classes(&skill_id, cap.classes.iter().map(|r| r.class));
                graph.edge(&agent_id, &skill_id, EdgeKind::HasSkill);
            }
            push_unique(&mut caps, cap);
        }

        if input.recon.tools.internal_tools.is_empty() {
            for name in rules.builtins_for(agent) {
                let cap = classifier.classify(
                    Subject {
                        kind: CapabilityKind::Builtin,
                        agent,
                        name,
                        description: "",
                        server: None,
                        command: None,
                        url: None,
                    },
                    None,
                );
                push_unique(&mut caps, cap);
            }
        } else {
            for tool in &input.recon.tools.internal_tools {
                let cap = classifier.classify(
                    Subject {
                        kind: CapabilityKind::Internal,
                        agent,
                        name: &tool.name,
                        description: &tool.description,
                        server: None,
                        command: None,
                        url: None,
                    },
                    tool.context_path.clone(),
                );
                push_unique(&mut caps, cap);
            }
        }

        let observed_hosts: Vec<String> = inputs
            .traffic_hosts
            .iter()
            .filter(|t| {
                t.node_id == node_id && common::traffic_agent_matches(&t.agent_label, agent)
            })
            .map(|t| t.host.to_lowercase())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for host in &observed_hosts {
            let host_id = host_entity_id(host, node_id);
            graph.entity(&host_id, EntityKind::Host, host);
            graph.edge(&agent_id, &host_id, EdgeKind::ObservedTraffic);
        }

        let cached = inputs
            .modifiers
            .get(&(node_id.to_string(), agent.to_string()));
        let modifiers = cached.map(|c| c.modifiers.clone()).unwrap_or_default();
        let (trifecta, risk, unattended) = assess(&caps, &modifiers);
        unattended_by_agent.insert(agent_id.clone(), unattended);

        let exposure = AgentExposure {
            id: agent_id.clone(),
            node_id: node_id.to_string(),
            machine_name,
            agent_short_name: agent.to_string(),
            performed_at: input.performed_at.clone(),
            is_semantic: input.is_semantic,
            trifecta,
            risk,
            capabilities: caps,
            modifiers,
            modifiers_probed_at: cached.map(|c| c.probed_at.clone()),
            observed_hosts,
        };
        graph.add_classes(&agent_id, exposure.classes_present().into_iter());
        if let Some(entity) = graph.entities.get_mut(&agent_id) {
            entity
                .attrs
                .insert("risk".to_string(), risk.as_str().to_string());
        }
        agents.push(exposure);
    }

    let paths = trifecta_paths(&agents, &unattended_by_agent);
    let graph = ExposureGraph {
        entities: graph.entities.into_values().collect(),
        edges: graph.edges.into_iter().collect(),
    };
    let shared = shared_entities(&graph, &agents);

    let report = ExposureReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        rule_count: rules.rules.len(),
        llm_classified: classifier.llm_classified,
        probed: false,
        agents,
        graph,
        paths,
        shared,
        warnings: inputs.warnings.clone(),
    };
    (report, classifier.candidates.into_values().collect())
}

fn trifecta_paths(
    agents: &[AgentExposure],
    unattended: &HashMap<String, Vec<String>>,
) -> Vec<TrifectaPath> {
    let labels = |a: &AgentExposure, class: CapabilityClass| -> Vec<String> {
        a.contributors(class).iter().map(|c| c.label()).collect()
    };
    let mut paths: Vec<TrifectaPath> = agents
        .iter()
        .filter(|a| a.trifecta)
        .map(|a| TrifectaPath {
            agent_id: a.id.clone(),
            node_id: a.node_id.clone(),
            machine_name: a.machine_name.clone(),
            agent_short_name: a.agent_short_name.clone(),
            risk: a.risk,
            sources: labels(a, CapabilityClass::UntrustedContent),
            private_data: labels(a, CapabilityClass::PrivateData),
            exfiltration: labels(a, CapabilityClass::Exfiltration),
            unattended: unattended.get(&a.id).cloned().unwrap_or_default(),
        })
        .collect();
    paths.sort_by(|a, b| {
        b.risk
            .cmp(&a.risk)
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
    paths
}

//
// MCP servers and hosts linked to two or more agents. A host is linked to
// an agent directly (observed traffic) or through an MCP server the agent
// uses that connects to it.
//

fn shared_entities(graph: &ExposureGraph, agents: &[AgentExposure]) -> Vec<SharedEntity> {
    let by_id: HashMap<&str, &AgentExposure> = agents.iter().map(|a| (a.id.as_str(), a)).collect();
    let mut linked: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for edge in &graph.edges {
        match edge.kind {
            EdgeKind::UsesMcpServer | EdgeKind::ObservedTraffic => {
                linked.entry(&edge.to).or_default().insert(&edge.from);
            }
            _ => {}
        }
    }
    for edge in graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::ConnectsTo)
    {
        let users: Vec<&str> = linked
            .get(edge.from.as_str())
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        linked.entry(&edge.to).or_default().extend(users);
    }

    let mut shared: Vec<SharedEntity> = linked
        .into_iter()
        .filter(|(_, agent_ids)| agent_ids.len() >= 2)
        .filter_map(|(entity_id, agent_ids)| {
            let entity = graph.entity(entity_id)?;
            let members: Vec<&AgentExposure> = agent_ids
                .iter()
                .filter_map(|id| by_id.get(id).copied())
                .collect();
            let nodes: BTreeSet<&str> = members.iter().map(|a| a.node_id.as_str()).collect();
            Some(SharedEntity {
                entity_id: entity_id.to_string(),
                kind: entity.kind,
                label: entity.label.clone(),
                classes: entity.classes.clone(),
                agents: agent_ids.iter().map(|s| s.to_string()).collect(),
                cross_node: nodes.len() > 1,
                nodes: nodes.into_iter().map(str::to_string).collect(),
                reaches_trifecta: members.iter().any(|a| a.trifecta),
            })
        })
        .collect();
    shared.sort_by(|a, b| {
        b.reaches_trifecta
            .cmp(&a.reaches_trifecta)
            .then(b.cross_node.cmp(&a.cross_node))
            .then(b.agents.len().cmp(&a.agents.len()))
            .then_with(|| a.label.cmp(&b.label))
    });
    shared
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exposure::rules;
    use common::{AgentTool, McpTransport, ReconTools};

    fn tool(name: &str, description: &str) -> AgentTool {
        AgentTool {
            name: name.to_string(),
            description: description.to_string(),
            context_path: None,
        }
    }

    fn stdio(name: &str, command: &str, tools: Vec<AgentTool>) -> McpServer {
        McpServer {
            name: name.to_string(),
            transport: McpTransport::Stdio,
            address: None,
            command: Some(command.to_string()),
            tools,
            context_path: None,
        }
    }

    fn remote(name: &str, url: &str, tools: Vec<AgentTool>) -> McpServer {
        McpServer {
            name: name.to_string(),
            transport: McpTransport::Sse,
            address: Some(url.to_string()),
            command: None,
            tools,
            context_path: None,
        }
    }

    fn agent(node: &str, short: &str, tools: ReconTools) -> AgentInput {
        AgentInput {
            node_id: node.to_string(),
            agent_short_name: short.to_string(),
            performed_at: "2026-10-18T00:00:00Z".to_string(),
            is_semantic: false,
            recon: ReconResult {
                tools,
                ..Default::default()
            },
        }
    }

    //
    // Two nodes, three agents:
    //   node-a/claudecode  built-ins (full trifecta) + github via npx
    //   node-a/gemini      only a fetch tool (U X) via a remote server
    //   node-b/codex       a pinned github package (same server as node-a)
    //                      and a read-only database tool (P)
    //
    fn fixture() -> Inputs {
        let github = || {
            vec![
                tool("get_issue", "Get an issue"),
                tool("create_issue", "Create an issue"),
            ]
        };
        Inputs {
            agents: vec![
                agent(
                    "node-a",
                    "claudecode",
                    ReconTools {
                        mcp_servers: vec![stdio(
                            "github",
                            "/usr/bin/npx -y @modelcontextprotocol/server-github",
                            github(),
                        )],
                        ..Default::default()
                    },
                ),
                agent(
                    "node-a",
                    "gemini",
                    ReconTools {
                        mcp_servers: vec![remote(
                            "web",
                            "https://mcp.example.com/sse?token=abc",
                            vec![tool("fetch", "Fetch a URL")],
                        )],
                        internal_tools: vec![tool("save_memory", "")],
                        ..Default::default()
                    },
                ),
                agent(
                    "node-b",
                    "codex",
                    ReconTools {
                        mcp_servers: vec![
                            stdio(
                                "gh",
                                "npx @modelcontextprotocol/server-github@1.2.0",
                                github(),
                            ),
                            stdio("db", "uvx mcp-server-postgres", vec![tool("query", "")]),
                        ],
                        internal_tools: vec![tool("mystery_tool", "Does a thing")],
                        ..Default::default()
                    },
                ),
            ],
            machine_names: HashMap::from([
                ("node-a".to_string(), "alpha".to_string()),
                ("node-b".to_string(), "beta".to_string()),
            ]),
            traffic_hosts: vec![
                TrafficHost {
                    node_id: "node-a".to_string(),
                    agent_label: "claudecode|claudedesktop".to_string(),
                    host: "api.anthropic.com".to_string(),
                },
                TrafficHost {
                    node_id: "node-a".to_string(),
                    agent_label: "gemini".to_string(),
                    host: "MCP.example.com".to_string(),
                },
            ],
            ..Default::default()
        }
    }

    fn build(inputs: &Inputs) -> (ExposureReport, Vec<LlmCandidate>) {
        let rules = rules::parse(rules::default_text()).unwrap();
        build_report(&rules, inputs)
    }

    #[test]
    fn normalizes_commands() {
        assert_eq!(
            normalize_command("/usr/bin/npx -y @modelcontextprotocol/server-github"),
            "npx @modelcontextprotocol/server-github"
        );
        assert_eq!(
            normalize_command("C:\\nodejs\\npx.cmd @modelcontextprotocol/server-github@latest"),
            "npx @modelcontextprotocol/server-github"
        );
        assert_eq!(
            normalize_command("uvx mcp-server-fetch@0.6.2 --api-key sk-123 --mode=fast"),
            "uvx mcp-server-fetch --api-key *** --mode=fast"
        );
        assert_eq!(
            normalize_command(
                "docker run -i --rm -e GITHUB_TOKEN=ghp_x ghcr.io/github/github-mcp-server"
            ),
            "docker run -i --rm -e GITHUB_TOKEN=*** ghcr.io/github/github-mcp-server"
        );
        // -y is only launcher noise for package runners.
        assert_eq!(normalize_command("mytool -y"), "mytool -y");
    }

    #[test]
    fn normalizes_urls() {
        assert_eq!(
            normalize_url("HTTPS://user:pw@MCP.Example.com:443/sse/?token=abc#x"),
            "mcp.example.com/sse"
        );
        assert_eq!(
            normalize_url("http://localhost:8080/mcp"),
            "localhost:8080/mcp"
        );
        assert_eq!(
            url_host("https://Api.GitHub.com/x").as_deref(),
            Some("api.github.com")
        );
    }

    #[test]
    fn dedupes_mcp_servers_across_agents_and_nodes() {
        let (report, _) = build(&fixture());
        let github: Vec<&GraphEntity> = report
            .graph
            .entities
            .iter()
            .filter(|e| e.kind == EntityKind::McpServer && e.id.contains("server-github"))
            .collect();
        assert_eq!(github.len(), 1);
        assert_eq!(
            github[0].id,
            "mcp:cmd:npx @modelcontextprotocol/server-github"
        );
        assert_eq!(github[0].label, "github | gh");

        let shared = report
            .shared
            .iter()
            .find(|s| s.entity_id == github[0].id)
            .expect("github server shared");
        assert!(shared.cross_node);
        assert!(shared.reaches_trifecta);
        assert_eq!(shared.nodes, vec!["node-a", "node-b"]);
    }

    #[test]
    fn trifecta_evaluation() {
        let (report, _) = build(&fixture());
        let claude = report.find_agent("node-a", "claudecode").unwrap();
        assert!(claude.trifecta);
        assert_eq!(claude.risk, RiskLevel::High);

        let gemini = report.find_agent("node-a", "gemini").unwrap();
        assert!(!gemini.trifecta);
        assert_eq!(gemini.risk, RiskLevel::Medium);
        assert_eq!(
            gemini.classes_present(),
            vec![
                CapabilityClass::UntrustedContent,
                CapabilityClass::Exfiltration
            ]
        );
        // Semantic internal tools replace the assumed built-ins.
        assert!(
            gemini
                .capabilities
                .iter()
                .all(|c| c.kind != CapabilityKind::Builtin)
        );

        let codex = report.find_agent("node-b", "codex").unwrap();
        assert!(codex.trifecta, "github tools + db give codex all three");
    }

    #[test]
    fn computes_paths_and_hosts() {
        let (report, candidates) = build(&fixture());
        assert_eq!(report.paths.len(), 2);
        let path = report
            .paths
            .iter()
            .find(|p| p.agent_short_name == "claudecode")
            .unwrap();
        assert!(path.sources.contains(&"builtin:WebFetch".to_string()));
        assert!(path.sources.contains(&"mcp:github/get_issue".to_string()));
        assert!(path.private_data.contains(&"builtin:Read".to_string()));
        assert!(
            path.exfiltration
                .contains(&"mcp:github/create_issue".to_string())
        );
        assert!(path.unattended.is_empty());

        // Traffic labelled "claudecode|claudedesktop" lands on claudecode.
        let claude = report.find_agent("node-a", "claudecode").unwrap();
        assert_eq!(claude.observed_hosts, vec!["api.anthropic.com"]);

        // Remote MCP host and observed traffic converge on one host entity.
        let host = report.graph.entity("host:mcp.example.com").unwrap();
        assert_eq!(host.kind, EntityKind::Host);
        assert!(
            report
                .graph
                .edges
                .iter()
                .any(|e| e.kind == EdgeKind::ConnectsTo && e.to == "host:mcp.example.com")
        );
        assert!(
            report
                .graph
                .edges
                .iter()
                .any(|e| e.kind == EdgeKind::ObservedTraffic && e.to == "host:mcp.example.com")
        );

        // Only the unknown internal tool is left for the LLM.
        let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["mystery_tool"]);
    }

    #[test]
    fn llm_verdicts_and_modifiers_apply() {
        let mut inputs = fixture();
        inputs.llm_verdicts.insert(
            llm_key(
                CapabilityKind::Internal,
                None,
                "mystery_tool",
                "Does a thing",
            ),
            vec![CapabilityClass::Exfiltration],
        );
        inputs.modifiers.insert(
            ("node-a".to_string(), "claudecode".to_string()),
            CachedModifiers {
                probed_at: "2026-10-18T01:00:00Z".to_string(),
                modifiers: vec![AgentModifier {
                    kind: ModifierKind::AutoApprove,
                    detail: "mcp__github__create_issue".to_string(),
                    source: "/home/u/.claude/settings.json".to_string(),
                    reason: "rule:claude-allow-list".to_string(),
                }],
            },
        );
        let (report, candidates) = build(&inputs);
        assert!(candidates.is_empty());
        assert_eq!(report.llm_classified, 1);

        let codex = report.find_agent("node-b", "codex").unwrap();
        let mystery = codex
            .capabilities
            .iter()
            .find(|c| c.name == "mystery_tool")
            .unwrap();
        assert_eq!(mystery.classes[0].reason, "llm");

        let claude = report.find_agent("node-a", "claudecode").unwrap();
        assert_eq!(claude.risk, RiskLevel::Critical);
        let path = report.paths_for_agent(&claude.id)[0];
        assert_eq!(
            path.unattended,
            vec!["auto-approved mcp:github/create_issue"]
        );
        assert_eq!(report.paths[0].agent_id, claude.id, "critical sorts first");
    }

    #[test]
    fn scoped_approvals_only_count_for_unattended_commands() {
        let bash = ClassifiedCapability {
            kind: CapabilityKind::Builtin,
            name: "Bash".to_string(),
            server: None,
            context_path: None,
            classes: Vec::new(),
            verdict: None,
        };
        assert!(auto_approved(&bash, "Bash"));
        assert!(auto_approved(&bash, "Bash(*)"));
        assert!(auto_approved(&bash, "Bash(git push:*)"));
        assert!(auto_approved(&bash, "Bash(curl https://x)"));
        assert!(!auto_approved(&bash, "Bash(ls:*)"));
        assert!(!auto_approved(&bash, "Bash(git status)"));
        assert!(!auto_approved(&bash, "WebFetch"));

        let fetch = ClassifiedCapability {
            name: "WebFetch".to_string(),
            ..bash.clone()
        };
        assert!(!auto_approved(&fetch, "WebFetch(domain:example.com)"));
    }

    #[test]
    fn loopback_endpoints_are_scoped_per_node() {
        let local = || {
            remote(
                "notes",
                "http://127.0.0.1:27123/mcp",
                vec![tool("fetch", "Fetch a URL")],
            )
        };
        let inputs = Inputs {
            agents: vec![
                agent(
                    "n1",
                    "claudecode",
                    ReconTools {
                        mcp_servers: vec![local()],
                        ..Default::default()
                    },
                ),
                agent(
                    "n2",
                    "claudecode",
                    ReconTools {
                        mcp_servers: vec![local()],
                        ..Default::default()
                    },
                ),
            ],
            ..Default::default()
        };
        let (report, _) = build(&inputs);
        let servers = report
            .graph
            .entities
            .iter()
            .filter(|e| e.kind == EntityKind::McpServer)
            .count();
        assert_eq!(servers, 2);
        assert!(report.graph.entity("host:127.0.0.1@n1").is_some());
        assert!(report.shared.iter().all(|s| !s.cross_node));
    }

    #[test]
    fn unlisted_server_becomes_server_capability() {
        let inputs = Inputs {
            agents: vec![agent(
                "n",
                "claudecode",
                ReconTools {
                    mcp_servers: vec![stdio("slack", "npx slack-mcp", Vec::new())],
                    ..Default::default()
                },
            )],
            ..Default::default()
        };
        let (report, _) = build(&inputs);
        let a = &report.agents[0];
        let server = a
            .capabilities
            .iter()
            .find(|c| c.kind == CapabilityKind::McpServer)
            .unwrap();
        assert_eq!(server.label(), "mcp-server:slack");
        assert_eq!(server.classes.len(), 3);
    }
}
