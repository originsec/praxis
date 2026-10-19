use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

//
// Agent exposure analysis types shared by the service (which computes the
// report from stored recon), the CLI (which renders and exports it) and the
// MCP server. Classification follows Simon Willison's "lethal trifecta"
// (https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/): an agent
// that combines access to private data, exposure to untrusted content and
// the ability to communicate externally can be steered into exfiltrating
// that data.
//

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityClass {
    PrivateData,
    UntrustedContent,
    Exfiltration,
}

impl CapabilityClass {
    pub const ALL: [CapabilityClass; 3] = [
        CapabilityClass::PrivateData,
        CapabilityClass::UntrustedContent,
        CapabilityClass::Exfiltration,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityClass::PrivateData => "private_data",
            CapabilityClass::UntrustedContent => "untrusted_content",
            CapabilityClass::Exfiltration => "exfiltration",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            CapabilityClass::PrivateData => "private data",
            CapabilityClass::UntrustedContent => "untrusted content",
            CapabilityClass::Exfiltration => "exfiltration",
        }
    }

    /// Single-letter badge used in compact listings (P / U / X).
    pub fn badge(&self) -> char {
        match self {
            CapabilityClass::PrivateData => 'P',
            CapabilityClass::UntrustedContent => 'U',
            CapabilityClass::Exfiltration => 'X',
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s.trim())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    McpTool,
    McpServer,
    Skill,
    /// Built-in tool discovered by semantic recon.
    Internal,
    /// Well-known built-in tool assumed from the rule file because recon
    /// did not enumerate internal tools (static recon).
    Builtin,
}

impl CapabilityKind {
    pub const ALL: [CapabilityKind; 5] = [
        CapabilityKind::McpTool,
        CapabilityKind::McpServer,
        CapabilityKind::Skill,
        CapabilityKind::Internal,
        CapabilityKind::Builtin,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityKind::McpTool => "mcp_tool",
            CapabilityKind::McpServer => "mcp_server",
            CapabilityKind::Skill => "skill",
            CapabilityKind::Internal => "internal",
            CapabilityKind::Builtin => "builtin",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s.trim())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassReason {
    pub class: CapabilityClass,
    /// `rule:<id>` for rule-file matches, `llm` for Semantic Parser verdicts.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedCapability {
    pub kind: CapabilityKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_path: Option<String>,
    #[serde(default)]
    pub classes: Vec<ClassReason>,

    //
    // Why the capability ended up with no classes, when it has none:
    // `rule:<id>` for an explicit benign verdict, `llm` for a Semantic
    // Parser verdict, or None when nothing matched (unclassified).
    //
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
}

impl ClassifiedCapability {
    pub fn has(&self, class: CapabilityClass) -> bool {
        self.classes.iter().any(|c| c.class == class)
    }

    pub fn is_unclassified(&self) -> bool {
        self.classes.is_empty() && self.verdict.is_none()
    }

    /// Stable, human-readable label such as `mcp:github/create_issue`.
    pub fn label(&self) -> String {
        match (self.kind, &self.server) {
            (CapabilityKind::McpTool, Some(server)) => format!("mcp:{}/{}", server, self.name),
            (CapabilityKind::McpServer, _) => format!("mcp-server:{}", self.name),
            (CapabilityKind::Skill, _) => format!("skill:{}", self.name),
            (CapabilityKind::Internal, _) => format!("internal:{}", self.name),
            (CapabilityKind::Builtin, _) => format!("builtin:{}", self.name),
            (CapabilityKind::McpTool, None) => format!("mcp:{}", self.name),
        }
    }

    pub fn badges(&self) -> String {
        badges(self.classes.iter().map(|c| c.class))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModifierKind {
    /// Permission prompts disabled for the agent as a whole.
    Yolo,
    /// Specific tools/servers allowed to run without a prompt.
    AutoApprove,
}

impl ModifierKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModifierKind::Yolo => "yolo",
            ModifierKind::AutoApprove => "auto_approve",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentModifier {
    pub kind: ModifierKind,
    /// What was found, e.g. `permissions.defaultMode = bypassPermissions`
    /// or a single allow-list entry such as `Bash(git push:*)`.
    pub detail: String,
    /// Config file the modifier was read from.
    pub source: String,
    /// `rule:<id>` of the modifier rule that matched.
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
            RiskLevel::Critical => "critical",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            RiskLevel::Low,
            RiskLevel::Medium,
            RiskLevel::High,
            RiskLevel::Critical,
        ]
        .into_iter()
        .find(|r| r.as_str().eq_ignore_ascii_case(s.trim()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExposure {
    /// Graph entity id of this agent (`agent:<node_id>/<short_name>`).
    pub id: String,
    pub node_id: String,
    pub machine_name: String,
    pub agent_short_name: String,
    /// When the underlying recon was performed.
    pub performed_at: String,
    pub is_semantic: bool,
    pub trifecta: bool,
    pub risk: RiskLevel,
    pub capabilities: Vec<ClassifiedCapability>,
    #[serde(default)]
    pub modifiers: Vec<AgentModifier>,
    /// When agent configs were last probed for modifiers (None = never).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifiers_probed_at: Option<String>,
    #[serde(default)]
    pub observed_hosts: Vec<String>,
}

impl AgentExposure {
    pub fn contributors(&self, class: CapabilityClass) -> Vec<&ClassifiedCapability> {
        self.capabilities.iter().filter(|c| c.has(class)).collect()
    }

    pub fn classes_present(&self) -> Vec<CapabilityClass> {
        CapabilityClass::ALL
            .into_iter()
            .filter(|class| self.capabilities.iter().any(|c| c.has(*class)))
            .collect()
    }

    pub fn badges(&self) -> String {
        badges(self.classes_present().into_iter())
    }

    pub fn unclassified_count(&self) -> usize {
        self.capabilities
            .iter()
            .filter(|c| c.is_unclassified())
            .count()
    }
}

//
// Graph model. Entity and edge kinds are open enums so later recon (e.g.
// credential presence, identities) can add `Credential`/`Identity` nodes
// and edges without reshaping existing ones; extra per-entity facts go in
// `attrs`.
//

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Node,
    Agent,
    McpServer,
    Host,
    Skill,
}

impl EntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityKind::Node => "node",
            EntityKind::Agent => "agent",
            EntityKind::McpServer => "mcp_server",
            EntityKind::Host => "host",
            EntityKind::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// node -> agent
    RunsAgent,
    /// agent -> mcp_server
    UsesMcpServer,
    /// agent -> skill
    HasSkill,
    /// mcp_server -> host (remote MCP endpoint)
    ConnectsTo,
    /// agent -> host (intercepted traffic)
    ObservedTraffic,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::RunsAgent => "runs",
            EdgeKind::UsesMcpServer => "uses",
            EdgeKind::HasSkill => "has_skill",
            EdgeKind::ConnectsTo => "connects_to",
            EdgeKind::ObservedTraffic => "observed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEntity {
    pub id: String,
    pub kind: EntityKind,
    pub label: String,
    #[serde(default)]
    pub classes: Vec<CapabilityClass>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attrs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExposureGraph {
    pub entities: Vec<GraphEntity>,
    pub edges: Vec<GraphEdge>,
}

impl ExposureGraph {
    pub fn entity(&self, id: &str) -> Option<&GraphEntity> {
        self.entities.iter().find(|e| e.id == id)
    }
}

//
// Untrusted-input source -> agent -> private-data tool -> exfil channel.
// One path per trifecta agent, listing every contributing capability per
// role rather than enumerating the cross product.
//

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrifectaPath {
    pub agent_id: String,
    pub node_id: String,
    pub machine_name: String,
    pub agent_short_name: String,
    pub risk: RiskLevel,
    pub sources: Vec<String>,
    pub private_data: Vec<String>,
    pub exfiltration: Vec<String>,
    /// Modifier details that remove the human from the loop on this path.
    #[serde(default)]
    pub unattended: Vec<String>,
}

//
// An MCP server or host reachable from more than one agent. These are the
// links that tie otherwise separate agents (and nodes) together.
//

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedEntity {
    pub entity_id: String,
    pub kind: EntityKind,
    pub label: String,
    pub classes: Vec<CapabilityClass>,
    pub agents: Vec<String>,
    pub nodes: Vec<String>,
    pub cross_node: bool,
    /// At least one linked agent has the full trifecta.
    pub reaches_trifecta: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExposureOptions {
    /// Ask the Semantic Parser LLM to classify capabilities no rule matched.
    #[serde(default)]
    pub llm: bool,
    /// Read agent settings files on online nodes to detect YOLO and
    /// auto-approve modifiers (results are cached).
    #[serde(default)]
    pub probe_configs: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExposureReport {
    pub generated_at: String,
    pub rule_count: usize,
    #[serde(default)]
    pub llm_classified: usize,
    #[serde(default)]
    pub probed: bool,
    pub agents: Vec<AgentExposure>,
    pub graph: ExposureGraph,
    pub paths: Vec<TrifectaPath>,
    pub shared: Vec<SharedEntity>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExposureSummary {
    pub nodes: usize,
    pub agents: usize,
    pub trifecta_agents: usize,
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub with_private_data: usize,
    pub with_untrusted_content: usize,
    pub with_exfiltration: usize,
    pub capabilities: usize,
    pub unclassified: usize,
    pub mcp_servers: usize,
    pub hosts: usize,
    pub skills: usize,
    pub shared_entities: usize,
    pub cross_node_links: usize,
    pub modifiers: usize,
}

impl ExposureReport {
    pub fn summary(&self) -> ExposureSummary {
        let count_kind = |kind: EntityKind| {
            self.graph
                .entities
                .iter()
                .filter(|e| e.kind == kind)
                .count()
        };
        let count_risk = |risk: RiskLevel| self.agents.iter().filter(|a| a.risk == risk).count();
        let count_class = |class: CapabilityClass| {
            self.agents
                .iter()
                .filter(|a| a.capabilities.iter().any(|c| c.has(class)))
                .count()
        };
        ExposureSummary {
            nodes: count_kind(EntityKind::Node),
            agents: self.agents.len(),
            trifecta_agents: self.agents.iter().filter(|a| a.trifecta).count(),
            critical: count_risk(RiskLevel::Critical),
            high: count_risk(RiskLevel::High),
            medium: count_risk(RiskLevel::Medium),
            low: count_risk(RiskLevel::Low),
            with_private_data: count_class(CapabilityClass::PrivateData),
            with_untrusted_content: count_class(CapabilityClass::UntrustedContent),
            with_exfiltration: count_class(CapabilityClass::Exfiltration),
            capabilities: self.agents.iter().map(|a| a.capabilities.len()).sum(),
            unclassified: self.agents.iter().map(|a| a.unclassified_count()).sum(),
            mcp_servers: count_kind(EntityKind::McpServer),
            hosts: count_kind(EntityKind::Host),
            skills: count_kind(EntityKind::Skill),
            shared_entities: self.shared.len(),
            cross_node_links: self.shared.iter().filter(|s| s.cross_node).count(),
            modifiers: self.agents.iter().map(|a| a.modifiers.len()).sum(),
        }
    }

    //
    // Look up one agent by node-id prefix (case-insensitive) and exact
    // agent short name. Ambiguous prefixes are rejected.
    //

    pub fn find_agent(&self, node_prefix: &str, agent: &str) -> Result<&AgentExposure, String> {
        let prefix = node_prefix.to_lowercase();
        let matches: Vec<&AgentExposure> = self
            .agents
            .iter()
            .filter(|a| {
                a.node_id.to_lowercase().starts_with(&prefix)
                    && a.agent_short_name.eq_ignore_ascii_case(agent)
            })
            .collect();
        let nodes: BTreeSet<&str> = matches.iter().map(|a| a.node_id.as_str()).collect();
        match (matches.first(), nodes.len()) {
            (None, _) => Err(format!(
                "No exposure data for agent '{}' on node '{}'. Run recon for it first.",
                agent, node_prefix
            )),
            (Some(found), 1) => Ok(found),
            _ => Err(format!(
                "Node prefix '{}' is ambiguous; matches: {}",
                node_prefix,
                nodes.into_iter().collect::<Vec<_>>().join(", ")
            )),
        }
    }

    pub fn paths_for_agent(&self, agent_id: &str) -> Vec<&TrifectaPath> {
        self.paths
            .iter()
            .filter(|p| p.agent_id == agent_id)
            .collect()
    }

    pub fn shared_for_agent(&self, agent_id: &str) -> Vec<&SharedEntity> {
        self.shared
            .iter()
            .filter(|s| s.agents.iter().any(|a| a == agent_id))
            .collect()
    }

    //
    // Graphviz DOT rendering of the exposure graph. Output is deterministic
    // (entities and edges sorted by id) so it can be diffed and snapshot
    // tested. Agents are filled by risk; class badges ([P U X]) are shown
    // on agents, MCP servers and skills.
    //

    pub fn to_dot(&self) -> String {
        let risk_by_agent: BTreeMap<&str, RiskLevel> = self
            .agents
            .iter()
            .map(|a| (a.id.as_str(), a.risk))
            .collect();

        let mut entities: Vec<&GraphEntity> = self.graph.entities.iter().collect();
        entities.sort_by(|a, b| a.id.cmp(&b.id));
        let mut edges: Vec<&GraphEdge> = self.graph.edges.iter().collect();
        edges.sort();

        let mut out = String::new();
        out.push_str("digraph exposure {\n");
        out.push_str("  rankdir=LR;\n");
        out.push_str("  node [fontname=\"Helvetica\", fontsize=10];\n");
        out.push_str("  edge [fontname=\"Helvetica\", fontsize=8];\n");

        for entity in entities {
            let badge = if entity.classes.is_empty() {
                String::new()
            } else {
                format!("\\n[{}]", badges(entity.classes.iter().copied()))
            };
            let (shape, fill) = match entity.kind {
                EntityKind::Node => ("box3d", None),
                EntityKind::Agent => {
                    let fill = match risk_by_agent.get(entity.id.as_str()) {
                        Some(RiskLevel::Critical) => "#e06666",
                        Some(RiskLevel::High) => "#f6b26b",
                        Some(RiskLevel::Medium) => "#ffe599",
                        _ => "#d9ead3",
                    };
                    ("box", Some(fill))
                }
                EntityKind::McpServer => ("component", None),
                EntityKind::Host => ("ellipse", None),
                EntityKind::Skill => ("note", None),
            };
            let risk = risk_by_agent
                .get(entity.id.as_str())
                .map(|r| format!(" {}", r.as_str()))
                .unwrap_or_default();
            let _ = write!(
                out,
                "  \"{}\" [label=\"{}{}{}\", shape={}",
                dot_escape(&entity.id),
                dot_escape(&entity.label),
                badge,
                risk,
                shape
            );
            if let Some(fill) = fill {
                let _ = write!(out, ", style=filled, fillcolor=\"{}\"", fill);
            }
            out.push_str("];\n");
        }

        for edge in edges {
            let _ = writeln!(
                out,
                "  \"{}\" -> \"{}\" [label=\"{}\"];",
                dot_escape(&edge.from),
                dot_escape(&edge.to),
                edge.kind.as_str()
            );
        }

        out.push_str("}\n");
        out
    }
}

/// Render a set of classes as `P U X`-style badges (absent classes as `-`).
pub fn badges(classes: impl Iterator<Item = CapabilityClass>) -> String {
    let present: BTreeSet<CapabilityClass> = classes.collect();
    CapabilityClass::ALL
        .iter()
        .map(|c| if present.contains(c) { c.badge() } else { '-' })
        .map(String::from)
        .collect::<Vec<_>>()
        .join(" ")
}

fn dot_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(kind: CapabilityKind, name: &str, classes: &[CapabilityClass]) -> ClassifiedCapability {
        ClassifiedCapability {
            kind,
            name: name.to_string(),
            server: None,
            context_path: None,
            classes: classes
                .iter()
                .map(|c| ClassReason {
                    class: *c,
                    reason: "rule:test".to_string(),
                })
                .collect(),
            verdict: None,
        }
    }

    fn sample_report() -> ExposureReport {
        let agent = AgentExposure {
            id: "agent:n1/claudecode".to_string(),
            node_id: "n1".to_string(),
            machine_name: "box\"1".to_string(),
            agent_short_name: "claudecode".to_string(),
            performed_at: "2026-10-18T00:00:00Z".to_string(),
            is_semantic: false,
            trifecta: true,
            risk: RiskLevel::High,
            capabilities: vec![
                cap(
                    CapabilityKind::Builtin,
                    "Bash",
                    &[CapabilityClass::PrivateData, CapabilityClass::Exfiltration],
                ),
                cap(
                    CapabilityKind::Builtin,
                    "WebFetch",
                    &[CapabilityClass::UntrustedContent],
                ),
                cap(CapabilityKind::Skill, "/noop", &[]),
            ],
            modifiers: Vec::new(),
            modifiers_probed_at: None,
            observed_hosts: Vec::new(),
        };
        ExposureReport {
            generated_at: "2026-10-18T00:00:00Z".to_string(),
            rule_count: 1,
            agents: vec![agent],
            graph: ExposureGraph {
                entities: vec![
                    GraphEntity {
                        id: "node:n1".to_string(),
                        kind: EntityKind::Node,
                        label: "box\"1".to_string(),
                        classes: Vec::new(),
                        attrs: BTreeMap::new(),
                    },
                    GraphEntity {
                        id: "agent:n1/claudecode".to_string(),
                        kind: EntityKind::Agent,
                        label: "claudecode".to_string(),
                        classes: CapabilityClass::ALL.to_vec(),
                        attrs: BTreeMap::new(),
                    },
                    GraphEntity {
                        id: "host:api.github.com".to_string(),
                        kind: EntityKind::Host,
                        label: "api.github.com".to_string(),
                        classes: Vec::new(),
                        attrs: BTreeMap::new(),
                    },
                ],
                edges: vec![
                    GraphEdge {
                        from: "node:n1".to_string(),
                        to: "agent:n1/claudecode".to_string(),
                        kind: EdgeKind::RunsAgent,
                    },
                    GraphEdge {
                        from: "agent:n1/claudecode".to_string(),
                        to: "host:api.github.com".to_string(),
                        kind: EdgeKind::ObservedTraffic,
                    },
                ],
            },
            ..Default::default()
        }
    }

    #[test]
    fn dot_export_snapshot() {
        let expected = r##"digraph exposure {
  rankdir=LR;
  node [fontname="Helvetica", fontsize=10];
  edge [fontname="Helvetica", fontsize=8];
  "agent:n1/claudecode" [label="claudecode\n[P U X] high", shape=box, style=filled, fillcolor="#f6b26b"];
  "host:api.github.com" [label="api.github.com", shape=ellipse];
  "node:n1" [label="box\"1", shape=box3d];
  "agent:n1/claudecode" -> "host:api.github.com" [label="observed"];
  "node:n1" -> "agent:n1/claudecode" [label="runs"];
}
"##;
        assert_eq!(sample_report().to_dot(), expected);
    }

    #[test]
    fn summary_counts() {
        let summary = sample_report().summary();
        assert_eq!(summary.nodes, 1);
        assert_eq!(summary.agents, 1);
        assert_eq!(summary.trifecta_agents, 1);
        assert_eq!(summary.high, 1);
        assert_eq!(summary.hosts, 1);
        assert_eq!(summary.capabilities, 3);
        assert_eq!(summary.unclassified, 1);
        assert_eq!(summary.with_untrusted_content, 1);
    }

    #[test]
    fn find_agent_by_prefix() {
        let report = sample_report();
        assert!(report.find_agent("N", "claudecode").is_ok());
        assert!(report.find_agent("n1", "codex").is_err());
    }

    #[test]
    fn badges_render_missing_classes() {
        assert_eq!(badges([CapabilityClass::Exfiltration].into_iter()), "- - X");
        assert_eq!(badges(CapabilityClass::ALL.into_iter()), "P U X");
    }
}
