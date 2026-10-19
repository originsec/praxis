use std::path::{Path, PathBuf};

use common::connector_author::{
    CommandCapture, ConnectorProbe, ProbedConfig, ProbedDir, ProbedEntry,
};
use regex::Regex;
use serde_json::{Value as JsonValue, json};

//
// Input gathering for LLM-assisted connector authoring. Everything here is
// bounded: the agent binary is only ever run with `--help` / `--version`
// under a short timeout, directory listings are depth- and count-limited and
// carry names, sizes and types only, and the only file contents that leave
// the node are small, clearly-config JSON/TOML/YAML files with secret-looking
// values redacted.
//

const COMMAND_TIMEOUT_SECS: u64 = 5;
const MAX_COMMAND_OUTPUT: usize = 16 * 1024;
const MAX_HOMES: usize = 8;
const MAX_WALK_DEPTH: usize = 3;
const MAX_ENTRIES_PER_DIR: usize = 150;
const MAX_ENTRIES_TOTAL: usize = 400;
const MAX_CONFIG_FILE_BYTES: u64 = 16 * 1024;
const MAX_CONFIG_DEPTH: usize = 2;
const MAX_CONFIGS: usize = 6;
const MAX_CONFIG_BYTES_TOTAL: usize = 48 * 1024;
const MAX_ACP_EVIDENCE: usize = 5;
const REDACTED: &str = "<redacted>";

//
// Directories, relative to a user home, where agents commonly keep config
// and state. `{}` is replaced with the agent name. All platforms' layouts are
// checked everywhere; non-existent paths are simply skipped.
//

const CANDIDATE_DIRS: &[&str] = &[
    ".{}",
    ".config/{}",
    ".local/share/{}",
    ".local/state/{}",
    "Library/Application Support/{}",
    "AppData/Roaming/{}",
    "AppData/Local/{}",
];

//
// User-level install locations searched when the executable is not on the
// node's own PATH (nodes often run as a service with a minimal PATH).
//

const HOME_BIN_DIRS: &[&str] = &[
    ".local/bin",
    "bin",
    ".local/share/mise/shims",
    ".cargo/bin",
    ".bun/bin",
    ".npm-global/bin",
    "go/bin",
    "AppData/Roaming/npm",
    "AppData/Local/Programs",
];

const GLOBAL_BIN_DIRS: &[&str] = &["/usr/local/bin", "/usr/bin", "/opt/homebrew/bin"];

const SKIP_DESCEND: &[&str] = &["node_modules", ".git", "cache", "Cache", ".cache"];
const MAX_EXTRA_NAMES: usize = 4;

//
// Shared parent directories that must never be listed wholesale as if they
// belonged to the agent (".config" would otherwise expose every app's name).
//

const GENERIC_DIR_NAMES: &[&str] = &["config", "local", "cache", "ssh", "gnupg"];

//
// Probe `requested` (an executable name or path). `extra_names` are further
// config-directory base names to look for, e.g. the dot-dirs an existing
// connector references when it is being repaired.
//

pub fn probe(requested: &str, extra_names: &[String]) -> ConnectorProbe {
    let homes = crate::agent_connectors::utils::enumerate_user_homes();
    probe_with_homes(requested, extra_names, &homes)
}

pub(crate) fn probe_with_homes(
    requested: &str,
    extra_names: &[String],
    homes: &[PathBuf],
) -> ConnectorProbe {
    let homes: Vec<PathBuf> = homes.iter().take(MAX_HOMES).cloned().collect();
    let mut result = ConnectorProbe {
        requested: requested.to_string(),
        os: std::env::consts::OS.to_string(),
        ..Default::default()
    };

    let executable = resolve_executable(requested, &homes);
    if let Some(exe) = &executable {
        let exe_str = exe.to_string_lossy().to_string();
        let help = capture(&exe_str, &["--help"]);
        result.acp_evidence = detect_acp(&help.output);
        result.acp_advertised = !result.acp_evidence.is_empty();
        result.help = Some(help);
        result.version = Some(capture(&exe_str, &["--version"]));
        result.executable = Some(exe_str);
    } else {
        result.notes.push(format!(
            "Executable '{}' was not found on the node",
            requested
        ));
    }

    let names = agent_names(requested, executable.as_deref(), extra_names);
    let mut total_entries = 0usize;
    let mut config_bytes = 0usize;
    for home in &homes {
        for template in CANDIDATE_DIRS {
            for name in &names {
                let dir = home.join(template.replace("{}", name));
                if !dir.is_dir() || result.dirs.iter().any(|d| Path::new(&d.path) == dir) {
                    continue;
                }
                let listed = list_dir(&dir, MAX_ENTRIES_TOTAL.saturating_sub(total_entries));
                total_entries += listed.entries.len();
                collect_configs(&dir, &listed, name, &mut result.configs, &mut config_bytes);
                result.dirs.push(listed);
            }
        }
    }
    if total_entries >= MAX_ENTRIES_TOTAL {
        result.notes.push(format!(
            "Directory listing truncated at {} entries",
            MAX_ENTRIES_TOTAL
        ));
    }

    result
}

//
// Candidate base names for config directories: the requested name (when it
// is a bare name), the executable's file stem, lowercased, without Windows
// launcher extensions, and up to MAX_EXTRA_NAMES caller-supplied names.
// Extra names are restricted to plain [a-z0-9_-] so they cannot escape the
// home directory.
//

fn agent_names(requested: &str, executable: Option<&Path>, extra: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    let mut push = |raw: &str| {
        let lower = raw.to_lowercase();
        let stem = lower
            .trim_end_matches(".exe")
            .trim_end_matches(".cmd")
            .trim_end_matches(".bat")
            .to_string();
        if !stem.is_empty() && !names.contains(&stem) {
            names.push(stem);
        }
    };
    if let Some(name) = Path::new(requested).file_name() {
        push(&name.to_string_lossy());
    }
    if let Some(name) = executable.and_then(|p| p.file_name()) {
        push(&name.to_string_lossy());
    }
    for name in extra
        .iter()
        .map(|n| n.trim_start_matches('.'))
        .filter(|n| {
            !n.is_empty()
                && n.len() <= 40
                && !GENERIC_DIR_NAMES.contains(&n.to_lowercase().as_str())
                && n.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .take(MAX_EXTRA_NAMES)
    {
        push(name);
    }
    names
}

fn resolve_executable(requested: &str, homes: &[PathBuf]) -> Option<PathBuf> {
    let requested = requested.trim();
    if requested.is_empty() {
        return None;
    }
    if requested.contains('/') || requested.contains('\\') {
        let path = PathBuf::from(crate::agent_connectors::utils::expand_path(requested));
        return path.is_file().then_some(path);
    }

    if let Some(found) = crate::utils::find_all_executables_in_path(requested)
        .into_iter()
        .next()
    {
        return Some(PathBuf::from(found));
    }

    let file_names: Vec<String> = if cfg!(windows) {
        vec![
            format!("{}.exe", requested),
            format!("{}.cmd", requested),
            requested.to_string(),
        ]
    } else {
        vec![requested.to_string()]
    };

    let dirs = homes
        .iter()
        .flat_map(|home| HOME_BIN_DIRS.iter().map(move |d| home.join(d)))
        .chain(GLOBAL_BIN_DIRS.iter().map(PathBuf::from));
    for dir in dirs {
        for file_name in &file_names {
            let candidate = dir.join(file_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn capture(executable: &str, args: &[&str]) -> CommandCapture {
    let spec = json!({
        "program": executable,
        "args": args,
        "timeout_secs": COMMAND_TIMEOUT_SECS,
        "env": { "NO_COLOR": "1", "TERM": "dumb", "CI": "1" },
    });
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    match crate::agent_connectors::lua::runtime::run_command(&spec, None) {
        Ok(value) => {
            let stdout = value.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
            let stderr = value.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
            let timed_out = value
                .get("timed_out")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            //
            // Many CLIs print help to stderr; keep both, stdout first.
            //
            let combined = match (
                stdout.trim().is_empty(),
                stderr.trim().is_empty() || timed_out,
            ) {
                (false, false) => format!("{}\n{}", stdout, stderr),
                (false, true) => stdout.to_string(),
                (true, false) => stderr.to_string(),
                (true, true) => String::new(),
            };
            let (output, truncated) = truncate(&strip_ansi(&combined), MAX_COMMAND_OUTPUT);
            CommandCapture {
                args,
                exit_code: if timed_out {
                    None
                } else {
                    value
                        .get("status")
                        .and_then(|v| v.as_i64())
                        .map(|c| c as i32)
                },
                timed_out,
                output,
                truncated,
            }
        }
        Err(e) => CommandCapture {
            args,
            output: format!("failed to run: {}", e),
            ..Default::default()
        },
    }
}

fn truncate(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_string(), false);
    }
    (common::truncate_str(text, max).to_string(), true)
}

fn strip_ansi(text: &str) -> String {
    static ANSI: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap());
    ANSI.replace_all(text, "").to_string()
}

//
// Lines of help text that advertise an Agent Client Protocol mode.
//

pub(crate) fn detect_acp(help: &str) -> Vec<String> {
    static ACP: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"(?i)(\bacp\b|agent[ -]client[ -]protocol)").unwrap()
    });
    help.lines()
        .filter(|line| ACP.is_match(line))
        .map(|line| common::truncate_str(line.trim(), 200).to_string())
        .take(MAX_ACP_EVIDENCE)
        .collect()
}

fn list_dir(root: &Path, budget: usize) -> ProbedDir {
    let mut dir = ProbedDir {
        path: root.to_string_lossy().to_string(),
        ..Default::default()
    };
    let limit = budget.min(MAX_ENTRIES_PER_DIR);
    let walker = walkdir::WalkDir::new(root)
        .min_depth(1)
        .max_depth(MAX_WALK_DEPTH)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| {
            e.depth() <= 1
                || e.path()
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| !SKIP_DESCEND.contains(&n.to_string_lossy().as_ref()))
                    .unwrap_or(true)
        });
    for entry in walker.filter_map(|e| e.ok()) {
        if dir.entries.len() >= limit {
            dir.truncated = true;
            break;
        }
        let file_type = entry.file_type();
        let kind = if file_type.is_symlink() {
            "symlink"
        } else if file_type.is_dir() {
            "dir"
        } else {
            "file"
        };
        let size = if file_type.is_file() {
            entry.metadata().map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        dir.entries.push(ProbedEntry {
            path: rel,
            kind: kind.to_string(),
            size,
            file_type: entry
                .path()
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
        });
    }
    dir
}

//
// A file is "clearly config" when it is a small JSON/TOML/YAML file near the
// top of the agent directory, named like a config file (or after the agent),
// and not named like something that holds credentials or history.
//

fn config_format(rel_path: &str, size: u64, agent_name: &str) -> Option<&'static str> {
    static SECRET_NAME: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"(?i)(auth|cred|secret|token|key|session|history|cookie|oauth|passw|\.env)")
            .unwrap()
    });
    static CONFIG_NAME: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"(?i)(config|settings|preferences|prefs|options|mcp)").unwrap()
    });

    if size == 0 || size > MAX_CONFIG_FILE_BYTES {
        return None;
    }
    if rel_path.split('/').count() > MAX_CONFIG_DEPTH {
        return None;
    }
    let file_name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let lower = file_name.to_lowercase();
    let (stem, ext) = lower.rsplit_once('.')?;
    let format = match ext {
        "json" | "jsonc" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        _ => return None,
    };
    if SECRET_NAME.is_match(file_name) {
        return None;
    }
    if !CONFIG_NAME.is_match(file_name) && stem != agent_name {
        return None;
    }
    Some(format)
}

fn collect_configs(
    root: &Path,
    listed: &ProbedDir,
    agent_name: &str,
    out: &mut Vec<ProbedConfig>,
    total_bytes: &mut usize,
) {
    for entry in &listed.entries {
        if out.len() >= MAX_CONFIGS || *total_bytes >= MAX_CONFIG_BYTES_TOTAL {
            return;
        }
        if entry.kind != "file" {
            continue;
        }
        let Some(format) = config_format(&entry.path, entry.size, agent_name) else {
            continue;
        };
        let path = root.join(&entry.path);
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some((content, redactions)) = redact_config(format, &raw) else {
            continue;
        };
        *total_bytes += content.len();
        out.push(ProbedConfig {
            path: path.to_string_lossy().to_string(),
            format: format.to_string(),
            content,
            redactions,
        });
    }
}

fn is_secret_key(key: &str) -> bool {
    static SECRET_KEY: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(
            r"(?i)(api[_-]?key|apikey|token|secret|passw|pwd|auth|credential|bearer|cookie|private[_-]?key|access[_-]?key|signature|license)",
        )
        .unwrap()
    });
    SECRET_KEY.is_match(key)
}

//
// Values that look like credentials no matter which key holds them: known
// token prefixes, JWTs, URLs with embedded credentials, and long opaque
// strings with mixed letters and digits.
//

fn is_secret_value(value: &str) -> bool {
    static PREFIX: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(
            r"^(sk-|sk_|rk_|pk_live|ghp_|gho_|ghu_|ghs_|github_pat_|xox[abprs]-|AKIA|ASIA|AIza|eyJ|glpat-|hf_|r8_|npm_|ya29\.)",
        )
        .unwrap()
    });
    static URL_CREDS: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"://[^/\s:@]+:[^/\s@]+@").unwrap());

    let v = value.trim();
    if PREFIX.is_match(v) || URL_CREDS.is_match(v) {
        return true;
    }
    v.len() >= 32
        && !v.contains(char::is_whitespace)
        && !v.contains('/')
        && !v.contains('\\')
        && v.chars().any(|c| c.is_ascii_digit())
        && v.chars().any(|c| c.is_ascii_alphabetic())
}

fn redact_json(value: &mut JsonValue, key_is_secret: bool, count: &mut usize) {
    match value {
        JsonValue::String(s) => {
            if key_is_secret || is_secret_value(s) {
                *s = REDACTED.to_string();
                *count += 1;
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                redact_json(item, key_is_secret, count);
            }
        }
        JsonValue::Object(map) => {
            for (k, v) in map.iter_mut() {
                redact_json(v, key_is_secret || is_secret_key(k), count);
            }
        }
        _ => {}
    }
}

fn redact_toml(value: &mut toml::Value, key_is_secret: bool, count: &mut usize) {
    match value {
        toml::Value::String(s) => {
            if key_is_secret || is_secret_value(s) {
                *s = REDACTED.to_string();
                *count += 1;
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                redact_toml(item, key_is_secret, count);
            }
        }
        toml::Value::Table(table) => {
            for (k, v) in table.iter_mut() {
                redact_toml(v, key_is_secret || is_secret_key(k), count);
            }
        }
        _ => {}
    }
}

//
// YAML is redacted line by line (no YAML parser on the node). A `key:` line
// whose key looks secret has its value replaced; if the value is a block
// scalar or nested mapping, every more-indented line that follows is
// withheld as well. Any other scalar that looks like a credential is also
// replaced.
//

fn redact_yaml(raw: &str, count: &mut usize) -> String {
    static KV: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r#"^(\s*(?:-\s+)?)(["']?)([^"':#]+)(["']?)\s*:(.*)$"#).unwrap()
    });
    static ITEM: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"^(\s*-\s+)(.+)$").unwrap());

    let mut out = Vec::new();
    let mut secret_block_indent: Option<usize> = None;
    for line in raw.lines() {
        let indent = line.len() - line.trim_start().len();
        if let Some(block) = secret_block_indent {
            if line.trim().is_empty() || indent > block {
                if !line.trim().is_empty() {
                    *count += 1;
                    out.push(format!("{}{}", &line[..indent], REDACTED));
                }
                continue;
            }
            secret_block_indent = None;
        }
        if let Some(caps) = KV.captures(line) {
            let key = caps[3].trim();
            let rest = caps[5].trim();
            let value = rest.split(" #").next().unwrap_or("").trim();
            let unquoted = value.trim_matches(|c| c == '"' || c == '\'');
            if is_secret_key(key) {
                if value.is_empty()
                    || value == "|"
                    || value == ">"
                    || value.starts_with('|')
                    || value.starts_with('>')
                {
                    secret_block_indent = Some(indent);
                    out.push(line.to_string());
                } else {
                    *count += 1;
                    out.push(format!(
                        "{}{}{}{}: {}",
                        &caps[1], &caps[2], &caps[3], &caps[4], REDACTED
                    ));
                }
                continue;
            }
            if !unquoted.is_empty() && is_secret_value(unquoted) {
                *count += 1;
                out.push(format!(
                    "{}{}{}{}: {}",
                    &caps[1], &caps[2], &caps[3], &caps[4], REDACTED
                ));
                continue;
            }
            out.push(line.to_string());
            continue;
        }
        if let Some(caps) = ITEM.captures(line) {
            let value = caps[2].trim().trim_matches(|c| c == '"' || c == '\'');
            if is_secret_value(value) {
                *count += 1;
                out.push(format!("{}{}", &caps[1], REDACTED));
                continue;
            }
        }
        out.push(line.to_string());
    }
    out.join("\n")
}

//
// Redact a config file. Returns None when a JSON/TOML file cannot be parsed:
// unparseable content is withheld rather than sent unredacted.
//

pub(crate) fn redact_config(format: &str, raw: &str) -> Option<(String, usize)> {
    let mut count = 0usize;
    match format {
        "json" => {
            let mut value: JsonValue = serde_json::from_str(raw).ok()?;
            redact_json(&mut value, false, &mut count);
            Some((serde_json::to_string_pretty(&value).ok()?, count))
        }
        "toml" => {
            let mut value: toml::Value = toml::from_str(raw).ok()?;
            redact_toml(&mut value, false, &mut count);
            Some((toml::to_string(&value).ok()?, count))
        }
        "yaml" => {
            let content = redact_yaml(raw, &mut count);
            Some((content, count))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("praxis-probe-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn json_redaction_hides_secret_keys_and_values() {
        let raw = r#"{
            "model": "gpt-5",
            "max_tokens": 4096,
            "providers": { "openai": { "api_key": "abc", "base_url": "https://api.openai.com/v1" } },
            "mcp": { "github": { "env": { "GITHUB_TOKEN": "x" }, "args": ["ghp_0123456789abcdef"] } },
            "headers": { "Authorization": "Bearer zzz" },
            "proxy": "https://user:hunter2@proxy.local"
        }"#;
        let (out, count) = redact_config("json", raw).unwrap();
        assert!(!out.contains("\"abc\""));
        assert!(!out.contains("ghp_0123"));
        assert!(!out.contains("Bearer zzz"));
        assert!(!out.contains("hunter2"));
        assert!(!out.contains("\"x\""));
        assert!(out.contains("gpt-5"));
        assert!(out.contains("4096"));
        assert!(out.contains("https://api.openai.com/v1"));
        assert_eq!(count, 5);
    }

    #[test]
    fn toml_redaction_hides_secrets() {
        let raw = "model = \"o3\"\n[auth]\nmode = \"oauth\"\n[mcp.x]\nenv = { API_KEY = \"k\" }\nsecret_value = \"AKIAABCDEFGHIJKLMNOP\"\n";
        let (out, count) = redact_config("toml", raw).unwrap();
        assert!(out.contains("o3"));
        assert!(!out.contains("oauth"));
        assert!(!out.contains("\"k\""));
        assert!(!out.contains("AKIA"));
        assert_eq!(count, 3);
    }

    #[test]
    fn yaml_redaction_hides_keys_blocks_and_values() {
        let raw = "model: sonnet\napi_key: sk-123\ncredentials:\n  user: bob\n  pass: x\nlist:\n  - eyJhbGciOiJIUzI1NiJ9.abc\nname: crush\n";
        let (out, count) = redact_config("yaml", raw).unwrap();
        assert!(out.contains("model: sonnet"));
        assert!(out.contains("name: crush"));
        assert!(!out.contains("sk-123"));
        assert!(!out.contains("bob"));
        assert!(!out.contains("eyJ"));
        assert_eq!(count, 4);
    }

    #[test]
    fn unparseable_json_is_withheld() {
        assert!(redact_config("json", "{ \"api_key\": \"oops\", ").is_none());
    }

    #[test]
    fn acp_detection_finds_mode_lines() {
        let help = "Usage: crush [flags]\n  run   Run a prompt\n  --acp   Start in Agent Client Protocol mode\n  acp   serve ACP over stdio\n";
        let evidence = detect_acp(help);
        assert_eq!(evidence.len(), 2);
        assert!(detect_acp("Usage: tool --capacity 3").is_empty());
    }

    #[test]
    fn config_selection_skips_secret_named_and_deep_files() {
        assert_eq!(config_format("config.json", 10, "x"), Some("json"));
        assert_eq!(config_format("x.yaml", 10, "x"), Some("yaml"));
        assert_eq!(config_format("auth.json", 10, "x"), None);
        assert_eq!(
            config_format("settings.json", MAX_CONFIG_FILE_BYTES + 1, "x"),
            None
        );
        assert_eq!(config_format("a/b/settings.json", 10, "x"), None);
        assert_eq!(config_format("random.json", 10, "x"), None);
        assert_eq!(config_format("history.jsonl", 10, "x"), None);
    }

    #[test]
    fn listing_is_bounded_and_contents_are_redacted() {
        let root = temp_root();
        let home = root.join("home");
        let agent_dir = home.join(".config").join("fakeagent");
        fs::create_dir_all(agent_dir.join("sessions")).unwrap();
        for i in 0..(MAX_ENTRIES_PER_DIR + 20) {
            fs::write(
                agent_dir.join("sessions").join(format!("s{:04}.jsonl", i)),
                "{}",
            )
            .unwrap();
        }
        fs::write(
            agent_dir.join("fakeagent.json"),
            r#"{"provider":"x","api_key":"supersecret"}"#,
        )
        .unwrap();
        fs::write(agent_dir.join("auth.json"), r#"{"token":"t"}"#).unwrap();

        let probe = probe_with_homes("fakeagent-not-installed", &[], std::slice::from_ref(&home));
        assert!(probe.executable.is_none());
        assert!(probe.dirs.is_empty(), "name mismatch should find nothing");

        let extra = vec![
            "../etc".to_string(),
            ".config".to_string(),
            ".fakeagent".to_string(),
        ];
        let probe = probe_with_homes("other", &extra, std::slice::from_ref(&home));
        assert_eq!(
            probe.dirs.len(),
            1,
            "extra names find the agent dir, unsafe ones are ignored"
        );

        let probe = probe_with_homes("fakeagent", &[], &[home]);
        assert_eq!(probe.dirs.len(), 1);
        let dir = &probe.dirs[0];
        assert!(dir.truncated);
        assert_eq!(dir.entries.len(), MAX_ENTRIES_PER_DIR);
        assert_eq!(probe.configs.len(), 1);
        assert!(!probe.configs[0].content.contains("supersecret"));
        assert!(probe.configs.iter().all(|c| !c.path.ends_with("auth.json")));

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn help_output_is_captured_bounded_and_acp_detected() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root();
        let exe = root.join("noisyagent");
        fs::write(
            &exe,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'noisyagent 1.2.3'; exit 0; fi\necho '  acp    Run as an ACP server'\nhead -c 40000 /dev/zero | tr '\\0' 'x'\n",
        )
        .unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();

        let probe = probe_with_homes(&exe.to_string_lossy(), &[], &[]);
        let help = probe.help.unwrap();
        assert!(help.truncated);
        assert!(help.output.len() <= MAX_COMMAND_OUTPUT);
        assert!(probe.acp_advertised);
        assert!(probe.version.unwrap().output.contains("1.2.3"));

        fs::remove_dir_all(root).unwrap();
    }
}
