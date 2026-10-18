mod eval;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use common::ConfigItem;
use common::posture::{
    AgentPosture, HostPosture, HostProbe, PostureCategory, PostureParams, PostureResult,
    PostureSetting, SettingScope, current_os, match_signatures, normalize_process_name,
};

use crate::agent_connectors::utils::enumerate_user_homes;

use eval::{AgentDef, LoadedSource, SourceDef};

const DEFINITIONS_TOML: &str = include_str!("agents.toml");

fn definitions() -> &'static HashMap<String, AgentDef> {
    static DEFS: OnceLock<HashMap<String, AgentDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        eval::parse_definitions(DEFINITIONS_TOML).unwrap_or_else(|e| {
            tracing::error!("Invalid built-in posture definitions: {}", e);
            HashMap::new()
        })
    })
}

//
// Evaluate posture for the given agents (short_name, display name) plus
// the host. Blocking: reads files, the registry and the process table, so
// callers run it on a blocking thread.
//

pub fn evaluate(agents: &[(String, String)], params: &PostureParams) -> PostureResult {
    let os = current_os();
    let homes = enumerate_user_homes();

    let agents = agents
        .iter()
        .map(|(short_name, name)| match definitions().get(short_name) {
            Some(def) => evaluate_agent(short_name, name, def, os, &homes),
            None => AgentPosture {
                agent_short_name: short_name.clone(),
                agent_name: name.clone(),
                notes: vec!["No posture definition for this agent.".to_string()],
                ..Default::default()
            },
        })
        .collect();

    let probe = LiveHost::new();
    PostureResult {
        host: HostPosture {
            os: os.to_string(),
            signatures_evaluated: params.signatures.len(),
            security_products: match_signatures(&params.signatures, os, &probe),
        },
        agents,
    }
}

fn evaluate_agent(
    short_name: &str,
    name: &str,
    def: &AgentDef,
    os: &str,
    homes: &[PathBuf],
) -> AgentPosture {
    let sources = load_sources(def, os, homes);

    let mut tracked: Vec<(PostureCategory, String, bool)> = Vec::new();
    let mut env_keys: Vec<(PostureCategory, String, bool)> = Vec::new();
    let mut push = |category, key: &String, is_endpoint| {
        let entry = (category, key.clone(), is_endpoint);
        if key.starts_with('$') {
            env_keys.push(entry);
        } else {
            tracked.push(entry);
        }
    };
    for k in &def.telemetry.enabled {
        push(PostureCategory::Telemetry, k, false);
    }
    for k in &def.telemetry.endpoints {
        push(PostureCategory::Telemetry, k, true);
    }
    for k in &def.telemetry.keys {
        push(PostureCategory::Telemetry, k, false);
    }
    for k in &def.sandbox.keys {
        push(PostureCategory::Sandbox, k, false);
    }

    let mut settings = eval::evaluate_settings(&sources, &tracked);
    for (category, key, is_endpoint) in env_keys {
        if let Ok(value) = std::env::var(&key[1..])
            && !value.is_empty()
        {
            settings.push(PostureSetting {
                category,
                value: eval::sanitize_value(&key, &serde_json::Value::String(value), is_endpoint),
                key,
                scope: SettingScope::Process,
                source: "node process environment".to_string(),
                effective: true,
            });
        }
    }

    let policy_sources: Vec<_> = sources
        .iter()
        .filter(|s| s.scope != SettingScope::User)
        .map(|s| eval::policy_source(s, &def.categories))
        .collect();
    let managed_policy = sources
        .iter()
        .any(|s| s.scope == SettingScope::Managed && s.present);
    let telemetry = eval::telemetry_summary(&settings, &def.telemetry);

    AgentPosture {
        agent_short_name: short_name.to_string(),
        agent_name: name.to_string(),
        managed_policy,
        policy_sources,
        settings,
        telemetry,
        notes: def.notes.clone(),
    }
}

//
// Resolve and load every source applicable to this OS. Per-user (`~/`)
// sources expand to one entry per home; only present per-user sources are
// kept, while global sources are always kept so the report shows which
// policy locations were checked.
//

fn load_sources(def: &AgentDef, os: &str, homes: &[PathBuf]) -> Vec<LoadedSource> {
    let mut out = Vec::new();
    for (rank, src) in def.sources.iter().enumerate() {
        if !src.applies_to(os) {
            continue;
        }
        let format = src.format.as_deref().unwrap_or(&def.format);
        if let Some(path) = resolved_path(src) {
            if let Some(rel) = path.strip_prefix("~/") {
                for home in homes {
                    let full = join_rel(home, rel);
                    if full.exists() {
                        out.push(load_file(
                            rank,
                            src.scope,
                            Some(home.clone()),
                            &full,
                            format,
                        ));
                    }
                }
            } else if path.contains('*') {
                //
                // Drop-in directories (e.g. managed-settings.d/*.json):
                // every match is a source at the same precedence. Report
                // the pattern itself as absent when nothing matches.
                //
                let matches: Vec<PathBuf> = glob::glob(&path)
                    .map(|it| it.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();
                if matches.is_empty() {
                    out.push(load_file(rank, src.scope, None, Path::new(&path), format));
                }
                for m in matches {
                    out.push(load_file(rank, src.scope, None, &m, format));
                }
            } else {
                out.push(load_file(rank, src.scope, None, Path::new(&path), format));
            }
        } else if let Some(domain) = &src.plist {
            out.extend(load_plists(rank, src, domain, format));
        } else if let Some(key) = &src.registry {
            let mut loaded = load_registry(rank, src.scope, key, src.value.as_deref(), format);
            loaded.content = loaded.content.map(eval::unflatten_dotted_keys);
            out.push(loaded);
        }
    }
    out
}

fn resolved_path(src: &SourceDef) -> Option<String> {
    if let Some(var) = &src.env_override
        && let Ok(p) = std::env::var(var)
        && !p.is_empty()
    {
        return Some(p);
    }
    src.path.as_deref().map(expand_env)
}

fn join_rel(home: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .fold(home.to_path_buf(), |p, part| p.join(part))
}

//
// Expand `%VAR%` and `${VAR}` from the node environment. Unset variables
// are left as-is so the resulting path simply does not exist.
//

fn expand_env(template: &str) -> String {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| regex::Regex::new(r"%([A-Za-z0-9_]+)%|\$\{([A-Za-z0-9_]+)\}").unwrap());
    re.replace_all(template, |caps: &regex::Captures| {
        let name = caps
            .get(1)
            .or_else(|| caps.get(2))
            .map_or("", |m| m.as_str());
        std::env::var(name).unwrap_or_else(|_| caps[0].to_string())
    })
    .into_owned()
}

fn load_file(
    rank: usize,
    scope: SettingScope,
    home: Option<PathBuf>,
    path: &Path,
    format: &str,
) -> LoadedSource {
    let mut src = LoadedSource {
        rank,
        scope,
        home,
        location: path.display().to_string(),
        present: path.exists(),
        content: None,
        raw: None,
        error: None,
    };
    if !src.present {
        return src;
    }
    match std::fs::read_to_string(path) {
        Ok(raw) => {
            match eval::parse_content(&raw, format) {
                Ok(v) => src.content = Some(v),
                Err(e) => src.error = Some(e),
            }
            if src.scope != SettingScope::User {
                src.raw = Some(eval::cap_raw(&raw));
            }
        }
        Err(e) => src.error = Some(format!("Unreadable: {}", e)),
    }
    src
}

//
// macOS configuration profiles deliver managed preferences as plists under
// /Library/Managed Preferences (computer-wide) and
// /Library/Managed Preferences/<user>/ (per-user). `plutil` converts them
// to JSON so they go through the same evaluation as file-based policy.
// With `value`, only that key is used; `base64 = true` means the key holds
// a base64-encoded document in `format` (Codex's *_toml_base64 keys).
//

fn load_plists(rank: usize, src: &SourceDef, domain: &str, format: &str) -> Vec<LoadedSource> {
    let base = "/Library/Managed Preferences";
    let mut paths = vec![PathBuf::from(format!("{}/{}.plist", base, domain))];
    if let Ok(it) = glob::glob(&format!("{}/*/{}.plist", base, domain)) {
        paths.extend(it.filter_map(|r| r.ok()));
    }
    let mut out: Vec<LoadedSource> = paths
        .iter()
        .filter(|p| p.exists())
        .map(|p| load_plist(rank, src, p, format))
        .collect();
    if out.is_empty() {
        out.push(LoadedSource {
            rank,
            scope: src.scope,
            home: None,
            location: plist_location(&paths[0], src.value.as_deref()),
            present: false,
            content: None,
            raw: None,
            error: None,
        });
    }
    out
}

fn plist_location(path: &Path, key: Option<&str>) -> String {
    match key {
        Some(k) => format!("{} [{}]", path.display(), k),
        None => path.display().to_string(),
    }
}

fn load_plist(rank: usize, def: &SourceDef, path: &Path, format: &str) -> LoadedSource {
    let mut src = LoadedSource {
        rank,
        scope: def.scope,
        home: None,
        location: plist_location(path, def.value.as_deref()),
        present: true,
        content: None,
        raw: None,
        error: None,
    };
    let mut cmd = crate::utils::silent_command("plutil");
    cmd.arg("-convert").arg("json").arg("-o").arg("-").arg(path);
    let out = match crate::utils::output_with_timeout(&mut cmd, std::time::Duration::from_secs(10))
    {
        Ok(out) if out.status.success() => out,
        Ok(out) => {
            src.error = Some(format!(
                "plutil failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
            return src;
        }
        Err(e) => {
            src.error = Some(format!("plutil failed: {}", e));
            return src;
        }
    };
    let json = String::from_utf8_lossy(&out.stdout).to_string();
    let parsed = match eval::parse_content(&json, "json") {
        Ok(v) => v,
        Err(e) => {
            src.error = Some(e);
            return src;
        }
    };
    let Some(key) = def.value.as_deref() else {
        src.raw = Some(eval::cap_raw(&json));
        src.content = Some(eval::unflatten_dotted_keys(parsed));
        return src;
    };
    let Some(text) = parsed.get(key).and_then(|v| v.as_str()) else {
        src.present = false;
        return src;
    };
    let decoded = if def.base64 {
        use base64::Engine;
        match base64::engine::general_purpose::STANDARD.decode(text.trim()) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
            Err(e) => {
                src.error = Some(format!("Invalid base64 in {}: {}", key, e));
                return src;
            }
        }
    } else {
        text.to_string()
    };
    match eval::parse_content(&decoded, format) {
        Ok(v) => src.content = Some(v),
        Err(e) => src.error = Some(e),
    }
    src.raw = Some(eval::cap_raw(&decoded));
    src
}

#[cfg(windows)]
fn load_registry(
    rank: usize,
    scope: SettingScope,
    key: &str,
    value: Option<&str>,
    format: &str,
) -> LoadedSource {
    use winreg::RegKey;
    use winreg::enums::*;

    let location = match value {
        Some(v) => format!("{}\\{}", key, v),
        None => key.to_string(),
    };
    let mut src = LoadedSource {
        rank,
        scope,
        home: None,
        location,
        present: false,
        content: None,
        raw: None,
        error: None,
    };
    let (hive, sub) = match key.split_once('\\') {
        Some(("HKLM", sub)) => (HKEY_LOCAL_MACHINE, sub),
        Some(("HKCU", sub)) => (HKEY_CURRENT_USER, sub),
        _ => {
            src.error = Some("Unsupported registry hive".to_string());
            return src;
        }
    };
    let Ok(reg) = RegKey::predef(hive).open_subkey(sub) else {
        return src;
    };

    //
    // With a value name, the value holds the whole policy document
    // (JSON/TOML text). Without one, each value under the key is a policy
    // key in its own right.
    //
    match value {
        Some(name) => {
            let Ok(raw) = reg.get_value::<String, _>(name) else {
                return src;
            };
            src.present = true;
            match eval::parse_content(&raw, format) {
                Ok(v) => src.content = Some(v),
                Err(e) => src.error = Some(e),
            }
            src.raw = Some(eval::cap_raw(&raw));
        }
        None => {
            let mut map = serde_json::Map::new();
            for (name, data) in reg.enum_values().flatten() {
                map.insert(name, serde_json::Value::String(data.to_string()));
            }
            src.present = !map.is_empty();
            if src.present {
                let v = serde_json::Value::Object(map);
                src.raw = Some(eval::cap_raw(
                    &serde_json::to_string_pretty(&v).unwrap_or_default(),
                ));
                src.content = Some(v);
            }
        }
    }
    src
}

#[cfg(not(windows))]
fn load_registry(
    rank: usize,
    scope: SettingScope,
    key: &str,
    _value: Option<&str>,
    _format: &str,
) -> LoadedSource {
    LoadedSource {
        rank,
        scope,
        home: None,
        location: key.to_string(),
        present: false,
        content: None,
        raw: None,
        error: None,
    }
}

//
// File-based managed/system policy locations for an agent on this OS,
// resolved to concrete paths. Used to surface policy files in the recon
// config view and to allow reading them outside user home directories.
//

fn global_policy_files(def: &AgentDef, os: &str) -> Vec<(SettingScope, PathBuf)> {
    let mut out = Vec::new();
    for src in def
        .sources
        .iter()
        .filter(|s| s.applies_to(os) && s.scope != SettingScope::User)
    {
        let Some(path) = resolved_path(src).filter(|p| !p.starts_with("~/")) else {
            continue;
        };
        if path.contains('*') {
            if let Ok(it) = glob::glob(&path) {
                out.extend(it.filter_map(|r| r.ok()).map(|p| (src.scope, p)));
            }
        } else {
            out.push((src.scope, PathBuf::from(path)));
        }
    }
    out.retain(|(_, p)| p.is_file());
    out
}

pub fn policy_config_items(agent_short_name: &str) -> Vec<ConfigItem> {
    let Some(def) = definitions().get(agent_short_name) else {
        return Vec::new();
    };
    global_policy_files(def, current_os())
        .into_iter()
        .map(|(scope, path)| ConfigItem {
            path: path.display().to_string(),
            contents: None,
            config_type: match scope {
                SettingScope::Defaults => "system_defaults".to_string(),
                _ => "managed_policy".to_string(),
            },
        })
        .collect()
}

//
// True when `canonical` is one of the known managed/system policy files.
// These live outside user homes (e.g. /etc, /Library, ProgramData) and are
// read-only from Praxis' point of view.
//

pub fn is_known_policy_path(canonical: &Path) -> bool {
    let os = current_os();
    definitions().values().any(|def| {
        global_policy_files(def, os)
            .iter()
            .filter_map(|(_, p)| p.canonicalize().ok())
            .any(|p| p == canonical)
    })
}

//
// Live host probe for signature matching. The process table is snapshotted
// once per evaluation.
//

struct LiveHost {
    processes: HashSet<String>,
}

impl LiveHost {
    fn new() -> Self {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            false,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
        //
        // Linux truncates the kernel process name to 15 characters, so the
        // executable's file name is recorded too (when readable).
        //
        let mut processes = HashSet::new();
        for p in sys.processes().values() {
            processes.insert(normalize_process_name(&p.name().to_string_lossy()));
            if let Some(file) = p.exe().and_then(|e| e.file_name()) {
                processes.insert(normalize_process_name(&file.to_string_lossy()));
            }
        }
        Self { processes }
    }
}

impl HostProbe for LiveHost {
    fn process_running(&self, name: &str) -> bool {
        self.processes.contains(&normalize_process_name(name))
    }

    fn service_installed(&self, name: &str) -> bool {
        service_installed(name)
    }

    fn path_exists(&self, path: &str) -> bool {
        Path::new(&expand_env(path)).exists()
    }
}

#[cfg(windows)]
fn service_installed(name: &str) -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(format!("SYSTEM\\CurrentControlSet\\Services\\{}", name))
        .is_ok()
}

#[cfg(target_os = "macos")]
fn service_installed(name: &str) -> bool {
    ["/Library/LaunchDaemons", "/Library/LaunchAgents"]
        .iter()
        .any(|dir| Path::new(dir).join(format!("{}.plist", name)).exists())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn service_installed(name: &str) -> bool {
    let unit = if name.contains('.') {
        name.to_string()
    } else {
        format!("{}.service", name)
    };
    [
        "/etc/systemd/system",
        "/usr/lib/systemd/system",
        "/lib/systemd/system",
        "/run/systemd/system",
    ]
    .iter()
    .any(|dir| Path::new(dir).join(&unit).exists())
        || Path::new("/etc/init.d").join(name).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_definitions_parse() {
        let defs = eval::parse_definitions(DEFINITIONS_TOML).expect("agents.toml parses");
        assert!(defs.contains_key("claudecode"));
        for (name, def) in &defs {
            for src in &def.sources {
                let kinds = [
                    src.path.is_some(),
                    src.plist.is_some(),
                    src.registry.is_some(),
                ];
                assert_eq!(
                    kinds.iter().filter(|k| **k).count(),
                    1,
                    "{}: each source needs exactly one of path/plist/registry",
                    name
                );
            }
        }
    }

    #[test]
    fn user_sources_expand_per_home_and_missing_are_skipped() {
        let dir = std::env::temp_dir().join(format!("praxis_posture_{}", std::process::id()));
        let home_a = dir.join("a");
        let home_b = dir.join("b");
        std::fs::create_dir_all(home_a.join(".tool")).unwrap();
        std::fs::create_dir_all(&home_b).unwrap();
        std::fs::write(home_a.join(".tool/settings.json"), r#"{"mode":"x"}"#).unwrap();
        let managed = dir.join("managed.json");
        std::fs::write(&managed, r#"{"mode":"locked"}"#).unwrap();

        let def: AgentDef = toml::from_str(&format!(
            r#"
[[sources]]
scope = "managed"
path = '{}'
[[sources]]
scope = "user"
path = "~/.tool/settings.json"
[sandbox]
keys = ["mode"]
"#,
            managed.display()
        ))
        .unwrap();

        let posture = evaluate_agent("t", "T", &def, current_os(), &[home_a.clone(), home_b]);
        assert!(posture.managed_policy);
        assert_eq!(posture.policy_sources.len(), 1);
        assert!(posture.policy_sources[0].raw.is_some());
        assert_eq!(posture.settings.len(), 2);
        let user = posture
            .settings
            .iter()
            .find(|s| s.scope == SettingScope::User)
            .unwrap();
        assert!(!user.effective, "managed value overrides the user file");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn expand_env_handles_both_syntaxes() {
        // SAFETY: test-local variable name, no concurrent readers.
        unsafe { std::env::set_var("PRAXIS_POSTURE_TEST_DIR", "/x") };
        assert_eq!(expand_env("%PRAXIS_POSTURE_TEST_DIR%/a"), "/x/a");
        assert_eq!(expand_env("${PRAXIS_POSTURE_TEST_DIR}/a"), "/x/a");
        assert_eq!(
            expand_env("%PRAXIS_UNSET_VAR_ZZ%/a"),
            "%PRAXIS_UNSET_VAR_ZZ%/a"
        );
    }
}
