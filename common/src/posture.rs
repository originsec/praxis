use serde::{Deserialize, Serialize};

//
// Endpoint posture: how an endpoint's AI agents are governed and observed.
// The node evaluates posture (managed policy, telemetry, sandbox settings
// per fingerprinted agent, plus host security products) and returns a
// PostureResult; the service persists it per node. Security product
// signatures are owned by the service (editable virtual file) and sent with
// each posture request so the node never needs its own copy.
//

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PostureCategory {
    Telemetry,
    Sandbox,
}

impl PostureCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            PostureCategory::Telemetry => "telemetry",
            PostureCategory::Sandbox => "sandbox",
        }
    }
}

//
// Where a setting came from. Ordering matches typical vendor precedence
// (managed policy beats user config beats shipped/system defaults); the
// definition file decides the real per-agent order.
//

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SettingScope {
    Managed,
    User,
    Defaults,
    Process,
}

impl SettingScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            SettingScope::Managed => "managed",
            SettingScope::User => "user",
            SettingScope::Defaults => "defaults",
            SettingScope::Process => "process",
        }
    }
}

/// One top-level key enforced by a managed policy source, summarised.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PolicyKey {
    pub key: String,
    /// Coarse grouping (permissions, mcp, hooks, model, telemetry, ...).
    pub category: String,
    pub summary: String,
}

/// A managed/system policy location checked on the endpoint.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PolicySource {
    /// File path, `plist:<domain>` or `registry:<key>\<value>`.
    pub location: String,
    pub scope: Option<SettingScope>,
    pub present: bool,
    #[serde(default)]
    pub keys: Vec<PolicyKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Raw policy content (capped) so it can be inspected without a
    /// second round-trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

/// A telemetry/sandbox setting observed in one source.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PostureSetting {
    pub category: PostureCategory,
    /// Dotted key path (e.g. `permissions.defaultMode`) or `$VAR` for
    /// process environment variables.
    pub key: String,
    /// Sanitised value: URLs reduced to scheme://host, secrets redacted.
    pub value: String,
    pub scope: SettingScope,
    /// File path / plist / registry location, or `node process environment`.
    pub source: String,
    /// True when no higher-precedence source sets the same key.
    pub effective: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TelemetrySummary {
    /// Some(true) when an effective "enabled" key is truthy.
    pub enabled: Option<bool>,
    /// Export destinations, host only.
    #[serde(default)]
    pub endpoints: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AgentPosture {
    pub agent_short_name: String,
    pub agent_name: String,
    /// True when at least one managed (higher-precedence) policy exists.
    pub managed_policy: bool,
    #[serde(default)]
    pub policy_sources: Vec<PolicySource>,
    #[serde(default)]
    pub settings: Vec<PostureSetting>,
    #[serde(default)]
    pub telemetry: TelemetrySummary,
    #[serde(default)]
    pub notes: Vec<String>,
}

//
// Per-OS string lists used by security product signatures.
//

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct OsList {
    #[serde(default)]
    pub windows: Vec<String>,
    #[serde(default)]
    pub linux: Vec<String>,
    #[serde(default)]
    pub macos: Vec<String>,
}

impl OsList {
    pub fn for_os(&self, os: &str) -> &[String] {
        match os {
            "windows" => &self.windows,
            "macos" => &self.macos,
            _ => &self.linux,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SecurityProductSignature {
    pub id: String,
    pub name: String,
    /// e.g. "edr", "monitoring", "ai_governance".
    pub category: String,
    #[serde(default)]
    pub processes: OsList,
    #[serde(default)]
    pub services: OsList,
    #[serde(default)]
    pub paths: OsList,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SecurityProductHit {
    pub id: String,
    pub name: String,
    pub category: String,
    /// True when a signature process is currently running.
    pub running: bool,
    /// Human-readable evidence, e.g. `process: falcon-sensor`.
    pub evidence: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct HostPosture {
    pub os: String,
    /// Number of signatures the node evaluated (0 means none were supplied,
    /// so an empty product list is not evidence of absence).
    pub signatures_evaluated: usize,
    #[serde(default)]
    pub security_products: Vec<SecurityProductHit>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PostureResult {
    pub host: HostPosture,
    #[serde(default)]
    pub agents: Vec<AgentPosture>,
}

impl PostureResult {
    //
    // View used by the CLI and MCP surfaces: optionally narrow to one agent
    // and drop raw policy contents (which can be large) unless requested.
    //

    pub fn filtered(&self, agent: Option<&str>, include_raw: bool) -> PostureResult {
        let mut out = self.clone();
        if let Some(agent) = agent {
            out.agents
                .retain(|a| a.agent_short_name.eq_ignore_ascii_case(agent));
        }
        if !include_raw {
            for a in &mut out.agents {
                for src in &mut a.policy_sources {
                    src.raw = None;
                }
            }
        }
        out
    }
}

//
// Stored posture as returned by the service. `error` is set when a refresh
// failed; `posture` may then still hold the previous (stale) result.
//

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PostureLookup {
    pub posture: Option<PostureResult>,
    pub performed_at: Option<String>,
    pub error: Option<String>,
}

//
// Request parameters for `_praxis/posture`. The service fills both fields;
// `agents` restricts evaluation to the node's fingerprinted agents.
//

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PostureParams {
    #[serde(default)]
    pub agents: Option<Vec<String>>,
    #[serde(default)]
    pub signatures: Vec<SecurityProductSignature>,
}

//
// Host lookups needed to evaluate signatures. Kept as a trait so matching
// is a pure function that tests can drive with a fake host.
//

pub trait HostProbe {
    fn process_running(&self, name: &str) -> bool;
    fn service_installed(&self, name: &str) -> bool;
    fn path_exists(&self, path: &str) -> bool;
}

pub fn current_os() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        _ => "linux",
    }
}

pub fn match_signatures(
    signatures: &[SecurityProductSignature],
    os: &str,
    probe: &dyn HostProbe,
) -> Vec<SecurityProductHit> {
    signatures
        .iter()
        .filter_map(|sig| match_signature(sig, os, probe))
        .collect()
}

fn match_signature(
    sig: &SecurityProductSignature,
    os: &str,
    probe: &dyn HostProbe,
) -> Option<SecurityProductHit> {
    let mut evidence = Vec::new();
    let mut running = false;
    for p in sig.processes.for_os(os) {
        if probe.process_running(p) {
            running = true;
            evidence.push(format!("process: {}", p));
        }
    }
    for s in sig.services.for_os(os) {
        if probe.service_installed(s) {
            evidence.push(format!("service: {}", s));
        }
    }
    for p in sig.paths.for_os(os) {
        if probe.path_exists(p) {
            evidence.push(format!("path: {}", p));
        }
    }
    if evidence.is_empty() {
        return None;
    }
    Some(SecurityProductHit {
        id: sig.id.clone(),
        name: sig.name.clone(),
        category: sig.category.clone(),
        running,
        evidence,
    })
}

//
// Normalise a process name for comparison: case-insensitive and without a
// Windows `.exe` suffix, so signatures can list either form.
//

pub fn normalize_process_name(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    struct FakeHost {
        processes: HashSet<String>,
        services: HashSet<String>,
        paths: HashSet<String>,
    }

    impl HostProbe for FakeHost {
        fn process_running(&self, name: &str) -> bool {
            self.processes.contains(&normalize_process_name(name))
        }
        fn service_installed(&self, name: &str) -> bool {
            self.services.contains(name)
        }
        fn path_exists(&self, path: &str) -> bool {
            self.paths.contains(path)
        }
    }

    fn host(processes: &[&str], services: &[&str], paths: &[&str]) -> FakeHost {
        FakeHost {
            processes: processes
                .iter()
                .map(|p| normalize_process_name(p))
                .collect(),
            services: services.iter().map(|s| s.to_string()).collect(),
            paths: paths.iter().map(|p| p.to_string()).collect(),
        }
    }

    fn sig() -> SecurityProductSignature {
        SecurityProductSignature {
            id: "edr".into(),
            name: "Example EDR".into(),
            category: "edr".into(),
            processes: OsList {
                windows: vec!["ExampleSvc.exe".into()],
                linux: vec!["example-sensor".into()],
                macos: vec![],
            },
            services: OsList {
                linux: vec!["example-sensor".into()],
                ..Default::default()
            },
            paths: OsList {
                linux: vec!["/opt/example".into()],
                ..Default::default()
            },
        }
    }

    #[test]
    fn no_evidence_no_hit() {
        let hits = match_signatures(&[sig()], "linux", &host(&["bash"], &[], &[]));
        assert!(hits.is_empty());
    }

    #[test]
    fn installed_but_not_running() {
        let hits = match_signatures(&[sig()], "linux", &host(&[], &[], &["/opt/example"]));
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].running);
        assert_eq!(hits[0].evidence, vec!["path: /opt/example"]);
    }

    #[test]
    fn running_process_and_service() {
        let hits = match_signatures(
            &[sig()],
            "linux",
            &host(&["example-sensor"], &["example-sensor"], &[]),
        );
        assert!(hits[0].running);
        assert_eq!(hits[0].evidence.len(), 2);
    }

    #[test]
    fn os_specific_lists_and_exe_suffix() {
        //
        // The Windows signature lists `ExampleSvc.exe`; a running
        // `examplesvc` (no suffix, different case) still matches, while the
        // Linux-only path is ignored on Windows.
        //
        let probe = host(&["examplesvc"], &[], &["/opt/example"]);
        let hits = match_signatures(&[sig()], "windows", &probe);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].evidence, vec!["process: ExampleSvc.exe"]);
        assert!(match_signatures(&[sig()], "macos", &probe).is_empty());
    }
}
