use serde::{Deserialize, Serialize};

//
// Shared types for LLM-assisted connector authoring: generating a draft Lua
// connector for an unsupported agent, validating a script with a dry run on a
// node, and proposing a repair for a connector that has stopped working. The
// node gathers inputs (`ConnectorProbe`) and runs dry runs
// (`ConnectorDryRun`); the service drives the LLM and reports a
// `ConnectorAuthorOutcome` back to the client.
//

pub const DEFAULT_MAX_REPAIRS: u32 = 2;
pub const MAX_REPAIRS_LIMIT: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectorAuthorRequest {
    Generate {
        node_id: String,
        executable: String,
        #[serde(default)]
        max_repairs: Option<u32>,
    },
    Repair {
        node_id: String,
        //
        // Script id, unique id prefix, or exact script name.
        //
        script: String,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        executable: Option<String>,
        #[serde(default)]
        max_repairs: Option<u32>,
    },
    Validate {
        node_id: String,
        #[serde(default)]
        script: Option<String>,
        #[serde(default)]
        script_content: Option<String>,
    },
}

impl ConnectorAuthorRequest {
    pub fn node_id(&self) -> &str {
        match self {
            Self::Generate { node_id, .. }
            | Self::Repair { node_id, .. }
            | Self::Validate { node_id, .. } => node_id,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Generate { .. } => "generate",
            Self::Repair { .. } => "repair",
            Self::Validate { .. } => "validate",
        }
    }
}

pub fn clamp_max_repairs(requested: Option<u32>) -> u32 {
    requested
        .unwrap_or(DEFAULT_MAX_REPAIRS)
        .min(MAX_REPAIRS_LIMIT)
}

//
// Bounded output of one `--help` / `--version` invocation.
//

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CommandCapture {
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub output: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProbedEntry {
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub file_type: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProbedDir {
    pub path: String,
    pub entries: Vec<ProbedEntry>,
    pub truncated: bool,
}

//
// A small config file whose secret-looking values were redacted on the node
// before it left the endpoint.
//

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProbedConfig {
    pub path: String,
    pub format: String,
    pub content: String,
    pub redactions: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ConnectorProbe {
    pub requested: String,
    pub os: String,
    pub executable: Option<String>,
    pub help: Option<CommandCapture>,
    pub version: Option<CommandCapture>,
    pub acp_advertised: bool,
    pub acp_evidence: Vec<String>,
    pub dirs: Vec<ProbedDir>,
    pub configs: Vec<ProbedConfig>,
    pub notes: Vec<String>,
}

impl ConnectorProbe {
    //
    // One-line description used in provenance notes and progress messages.
    //
    pub fn summary(&self) -> String {
        let entries: usize = self.dirs.iter().map(|d| d.entries.len()).sum();
        format!(
            "exe={} os={} help={} version={} acp={} dirs={} entries={} configs={}",
            self.executable.as_deref().unwrap_or("(not found)"),
            self.os,
            self.help.as_ref().map(|c| c.output.len()).unwrap_or(0),
            self.version
                .as_ref()
                .map(|c| c.output.lines().next().unwrap_or("").trim().to_string())
                .unwrap_or_default(),
            self.acp_advertised,
            self.dirs.len(),
            entries,
            self.configs.len(),
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DryRunFingerprint {
    pub available: bool,
    pub process_path: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DryRunReconSummary {
    pub config_items: usize,
    pub project_paths: usize,
    pub mcp_servers: usize,
    pub skills: usize,
    pub sessions: usize,
}

impl DryRunReconSummary {
    pub fn is_empty(&self) -> bool {
        self.config_items == 0
            && self.project_paths == 0
            && self.mcp_servers == 0
            && self.skills == 0
            && self.sessions == 0
    }
}

//
// Result of validating a script on a node: syntax → manifest/callbacks →
// fingerprint → static recon. Stops at the first failing stage, recorded in
// `failed_stage`. `sandboxed` reports whether side-effecting host APIs were
// disabled for the run.
//

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ConnectorDryRun {
    pub ok: bool,
    pub sandboxed: bool,
    pub failed_stage: Option<String>,
    pub name: Option<String>,
    pub short_name: Option<String>,
    pub callbacks: Vec<String>,
    pub fingerprint: Option<DryRunFingerprint>,
    pub recon: Option<DryRunReconSummary>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ConnectorDryRun {
    pub fn report(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Dry run: {}{}",
            if self.ok { "passed" } else { "FAILED" },
            self.failed_stage
                .as_deref()
                .map(|s| format!(" at stage '{}'", s))
                .unwrap_or_default()
        ));
        if let (Some(name), Some(short)) = (&self.name, &self.short_name) {
            lines.push(format!("Connector: {} ({})", name, short));
        }
        if let Some(fp) = &self.fingerprint {
            lines.push(format!(
                "Fingerprint: available={} path={} version={}",
                fp.available,
                fp.process_path.as_deref().unwrap_or("-"),
                fp.version.as_deref().unwrap_or("-")
            ));
        }
        if let Some(r) = &self.recon {
            lines.push(format!(
                "Recon: {} config item(s), {} project path(s), {} MCP server(s), {} skill(s), {} session(s)",
                r.config_items, r.project_paths, r.mcp_servers, r.skills, r.sessions
            ));
        }
        for e in &self.errors {
            lines.push(format!("Error: {}", e));
        }
        for w in &self.warnings {
            lines.push(format!("Warning: {}", w));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ConnectorAuthorOutcome {
    pub kind: String,
    pub ok: bool,
    pub message: String,
    //
    // The disabled user script the result was saved as (generate/repair).
    //
    pub script_id: Option<String>,
    pub script_name: Option<String>,
    pub script: Option<String>,
    //
    // Repair only: the script being repaired and a unified diff to the
    // proposed replacement.
    //
    pub original_script_id: Option<String>,
    pub original_script_name: Option<String>,
    pub diff: Option<String>,
    pub validation: Option<ConnectorDryRun>,
    pub attempts: u32,
    pub model: Option<String>,
}
