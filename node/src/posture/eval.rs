use std::collections::HashMap;
use std::path::PathBuf;

use common::posture::{
    PolicyKey, PolicySource, PostureCategory, PostureSetting, SettingScope, TelemetrySummary,
};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

//
// Data-driven posture definitions, keyed by agent short_name. Each agent
// lists its config sources in precedence order (highest first) plus the
// telemetry and sandbox keys worth reporting. Evaluation is pure: callers
// load the sources (files, plists, registry) and this module decides which
// value is effective and how it is summarised.
//

#[derive(Debug, Deserialize)]
pub struct AgentDef {
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub sources: Vec<SourceDef>,
    #[serde(default)]
    pub telemetry: TelemetryDef,
    #[serde(default)]
    pub sandbox: SandboxDef,
    //
    // Top-level policy key -> coarse category label, used when summarising
    // what a managed policy enforces.
    //
    #[serde(default)]
    pub categories: HashMap<String, String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

fn default_format() -> String {
    "json".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct SourceDef {
    pub scope: SettingScope,
    #[serde(default)]
    pub os: Option<String>,
    //
    // Exactly one of path / plist / registry is expected. `path` may start
    // with `~/` (evaluated once per user home) and may contain `%VAR%` or
    // `${VAR}` references to the node's environment.
    //
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub plist: Option<String>,
    #[serde(default)]
    pub registry: Option<String>,
    //
    // Registry value name, or the key to read inside a plist.
    //
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub base64: bool,
    #[serde(default)]
    pub format: Option<String>,
    //
    // Environment variable that, when set in the node's environment,
    // replaces `path` (e.g. GEMINI_CLI_SYSTEM_SETTINGS_PATH).
    //
    #[serde(default)]
    pub env_override: Option<String>,
}

impl SourceDef {
    pub fn applies_to(&self, os: &str) -> bool {
        self.os.as_deref().is_none_or(|o| o == os)
    }
}

//
// Key lists use dotted paths into the agent's settings; entries starting
// with `$` name environment variables read from the node's own process
// environment (which agent sessions launched by the node inherit).
//

#[derive(Debug, Deserialize, Default)]
pub struct TelemetryDef {
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub endpoints: Vec<String>,
    #[serde(default)]
    pub keys: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct SandboxDef {
    #[serde(default)]
    pub keys: Vec<String>,
}

pub fn parse_definitions(text: &str) -> Result<HashMap<String, AgentDef>, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}

//
// A source after loading. `rank` is the index in the definition (lower is
// higher precedence); `home` is set for per-user sources so effective
// values can be computed per user.
//

#[derive(Debug, Clone)]
pub struct LoadedSource {
    pub rank: usize,
    pub scope: SettingScope,
    pub home: Option<PathBuf>,
    pub location: String,
    pub present: bool,
    pub content: Option<Value>,
    pub raw: Option<String>,
    pub error: Option<String>,
}

const RAW_CAP: usize = 64 * 1024;
const VALUE_CAP: usize = 200;

pub fn parse_content(raw: &str, format: &str) -> Result<Value, String> {
    match format {
        "toml" => toml::from_str::<toml::Value>(raw)
            .map_err(|e| format!("TOML parse error: {}", e))
            .and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        _ => serde_json::from_str(raw).map_err(|e| format!("JSON parse error: {}", e)),
    }
}

//
// OS policy stores (Copilot's plist/registry values, for example) use flat
// dotted names like `sandbox.enabled`. Nest them so dotted key lookups work
// the same as for JSON/TOML documents.
//

pub fn unflatten_dotted_keys(value: Value) -> Value {
    let Value::Object(map) = value else {
        return value;
    };
    let mut out = serde_json::Map::new();
    for (key, v) in map {
        if !key.contains('.') {
            out.insert(key, v);
            continue;
        }
        let parts: Vec<&str> = key.split('.').collect();
        let mut cur = &mut out;
        for part in &parts[..parts.len() - 1] {
            let entry = cur
                .entry(part.to_string())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            if !entry.is_object() {
                *entry = Value::Object(serde_json::Map::new());
            }
            cur = entry.as_object_mut().unwrap();
        }
        cur.insert(parts[parts.len() - 1].to_string(), v);
    }
    Value::Object(out)
}

pub fn cap_raw(raw: &str) -> String {
    if raw.len() <= RAW_CAP {
        return raw.to_string();
    }
    format!(
        "{}\n... (truncated, {} bytes total)",
        common::truncate_str(raw, RAW_CAP),
        raw.len()
    )
}

pub fn lookup<'a>(content: &'a Value, dotted: &str) -> Option<&'a Value> {
    let mut cur = content;
    for part in dotted.split('.') {
        cur = cur.as_object()?.get(part)?;
    }
    if cur.is_null() { None } else { Some(cur) }
}

fn is_sensitive_key(key: &str) -> bool {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(header|token|secret|password|passwd|api[_-]?key|authorization|credential)",
        )
        .unwrap()
    })
    .is_match(key)
}

fn url_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"[a-zA-Z][a-zA-Z0-9+.-]*://[^\s"',;]+"#).unwrap())
}

//
// Reduce a URL to its host[:port]. Credentials, paths and queries are
// dropped so telemetry destinations can be reported without leaking tokens.
//

pub fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

pub fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

//
// Sanitise a setting value for reporting. Endpoint keys collapse to host
// list; sensitive keys are redacted; anything else is truncated with URLs
// reduced to scheme://host.
//

pub fn sanitize_value(key: &str, value: &Value, is_endpoint: bool) -> String {
    let text = value_to_string(value);
    if is_endpoint {
        let hosts: Vec<String> = url_regex()
            .find_iter(&text)
            .filter_map(|m| url_host(m.as_str()))
            .collect();
        if !hosts.is_empty() {
            return hosts.join(", ");
        }
        return common::truncate_str(&text, VALUE_CAP).to_string();
    }
    if is_sensitive_key(key) {
        return "<redacted>".to_string();
    }
    if let Value::Array(items) = value {
        return summarize_array(key, items);
    }
    let replaced = url_regex().replace_all(&text, |caps: &regex::Captures| {
        let url = &caps[0];
        let scheme = url.split("://").next().unwrap_or("");
        match url_host(url) {
            Some(h) => format!("{}://{}", scheme, h),
            None => url.to_string(),
        }
    });
    common::truncate_str(&replaced, VALUE_CAP).to_string()
}

pub fn is_truthy(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "no" | "off" | "none" | "null" | "disabled"
    )
}

//
// Compute every observed value of each tracked key and mark which ones are
// effective. Global sources (managed/defaults, no home) apply to all users;
// per-user sources only to their home. A value is effective when no
// higher-ranked applicable source sets the same key. A global value under a
// per-user source is effective only if some user does not override it (or
// no user sources were found at all).
//

pub fn evaluate_settings(
    sources: &[LoadedSource],
    tracked: &[(PostureCategory, String, bool)],
) -> Vec<PostureSetting> {
    let mut homes: Vec<&PathBuf> = sources.iter().filter_map(|s| s.home.as_ref()).collect();
    homes.sort();
    homes.dedup();

    let mut out = Vec::new();
    for (category, key, is_endpoint) in tracked {
        let has = |s: &LoadedSource| s.content.as_ref().and_then(|c| lookup(c, key)).is_some();

        for src in sources {
            let Some(value) = src.content.as_ref().and_then(|c| lookup(c, key)) else {
                continue;
            };
            let shadowed_by_global = sources
                .iter()
                .any(|o| o.rank < src.rank && o.home.is_none() && has(o));

            let effective = if shadowed_by_global {
                false
            } else if let Some(home) = &src.home {
                !sources
                    .iter()
                    .any(|o| o.rank < src.rank && o.home.as_ref() == Some(home) && has(o))
            } else {
                //
                // Global source: effective for any user whose own
                // higher-ranked sources leave the key unset.
                //
                homes.is_empty()
                    || homes.iter().any(|h| {
                        !sources
                            .iter()
                            .any(|o| o.rank < src.rank && o.home.as_ref() == Some(*h) && has(o))
                    })
            };

            out.push(PostureSetting {
                category: *category,
                key: key.clone(),
                value: sanitize_value(key, value, *is_endpoint),
                scope: src.scope,
                source: src.location.clone(),
                effective,
            });
        }
    }
    out
}

pub fn summarize_policy_keys(
    content: &Value,
    categories: &HashMap<String, String>,
) -> Vec<PolicyKey> {
    let Some(obj) = content.as_object() else {
        return Vec::new();
    };
    obj.iter()
        .map(|(key, value)| PolicyKey {
            key: key.clone(),
            category: categories
                .get(key)
                .cloned()
                .unwrap_or_else(|| "other".to_string()),
            summary: summarize_value(key, value),
        })
        .collect()
}

fn summarize_array(key: &str, items: &[Value]) -> String {
    if items.is_empty() {
        return "0 items".to_string();
    }
    let preview: Vec<String> = items
        .iter()
        .take(3)
        .map(|v| common::truncate_str(&sanitize_value(key, v, false), 60).to_string())
        .collect();
    let more = if items.len() > 3 { ", ..." } else { "" };
    format!("{} item(s): {}{}", items.len(), preview.join(", "), more)
}

fn summarize_value(key: &str, value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let keys: Vec<&str> = map.keys().take(6).map(|k| k.as_str()).collect();
            let more = if map.len() > 6 { ", ..." } else { "" };
            format!("{{{}{}}}", keys.join(", "), more)
        }
        _ => sanitize_value(key, value, false),
    }
}

pub fn telemetry_summary(settings: &[PostureSetting], def: &TelemetryDef) -> TelemetrySummary {
    let mut summary = TelemetrySummary::default();
    for s in settings.iter().filter(|s| s.effective) {
        if def.enabled.contains(&s.key) {
            summary.enabled = Some(summary.enabled.unwrap_or(false) || is_truthy(&s.value));
        }
        if def.endpoints.contains(&s.key) {
            for host in s.value.split(", ") {
                if !host.is_empty() && !summary.endpoints.iter().any(|h| h == host) {
                    summary.endpoints.push(host.to_string());
                }
            }
        }
    }
    summary
}

pub fn policy_source(src: &LoadedSource, categories: &HashMap<String, String>) -> PolicySource {
    PolicySource {
        location: src.location.clone(),
        scope: Some(src.scope),
        present: src.present,
        keys: src
            .content
            .as_ref()
            .map(|c| summarize_policy_keys(c, categories))
            .unwrap_or_default(),
        error: src.error.clone(),
        raw: src.raw.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn src(rank: usize, scope: SettingScope, home: Option<&str>, content: Value) -> LoadedSource {
        LoadedSource {
            rank,
            scope,
            home: home.map(PathBuf::from),
            location: format!("src{}{}", rank, home.unwrap_or("")),
            present: true,
            content: Some(content),
            raw: None,
            error: None,
        }
    }

    fn tracked(key: &str) -> Vec<(PostureCategory, String, bool)> {
        vec![(PostureCategory::Sandbox, key.to_string(), false)]
    }

    #[test]
    fn managed_overrides_user() {
        let sources = vec![
            src(
                0,
                SettingScope::Managed,
                None,
                json!({"permissions": {"defaultMode": "plan"}}),
            ),
            src(
                1,
                SettingScope::User,
                Some("/home/a"),
                json!({"permissions": {"defaultMode": "bypassPermissions"}}),
            ),
        ];
        let out = evaluate_settings(&sources, &tracked("permissions.defaultMode"));
        assert_eq!(out.len(), 2);
        assert!(out[0].effective && out[0].value == "plan");
        assert!(!out[1].effective);
    }

    #[test]
    fn user_wins_when_managed_silent() {
        let sources = vec![
            src(0, SettingScope::Managed, None, json!({"model": "x"})),
            src(
                1,
                SettingScope::User,
                Some("/home/a"),
                json!({"permissions": {"defaultMode": "acceptEdits"}}),
            ),
        ];
        let out = evaluate_settings(&sources, &tracked("permissions.defaultMode"));
        assert_eq!(out.len(), 1);
        assert!(out[0].effective);
        assert_eq!(out[0].scope, SettingScope::User);
    }

    #[test]
    fn defaults_effective_only_for_users_without_override() {
        //
        // Gemini-style ordering: system settings > user > system defaults.
        // User a overrides the default, user b does not, so the default is
        // still effective (for b).
        //
        let sources = vec![
            src(0, SettingScope::Managed, None, json!({})),
            src(
                1,
                SettingScope::User,
                Some("/home/a"),
                json!({"tools": {"sandbox": "docker"}}),
            ),
            src(1, SettingScope::User, Some("/home/b"), json!({})),
            src(
                2,
                SettingScope::Defaults,
                None,
                json!({"tools": {"sandbox": false}}),
            ),
        ];
        let out = evaluate_settings(&sources, &tracked("tools.sandbox"));
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|s| s.effective));

        let sources = vec![
            src(
                1,
                SettingScope::User,
                Some("/home/a"),
                json!({"tools": {"sandbox": "docker"}}),
            ),
            src(
                2,
                SettingScope::Defaults,
                None,
                json!({"tools": {"sandbox": false}}),
            ),
        ];
        let out = evaluate_settings(&sources, &tracked("tools.sandbox"));
        assert!(out[0].effective);
        assert!(!out[1].effective, "only user overrides the default");
    }

    #[test]
    fn higher_ranked_user_file_shadows_lower_for_same_home() {
        let sources = vec![
            src(
                1,
                SettingScope::User,
                Some("/home/a"),
                json!({"sandbox_mode": "read-only"}),
            ),
            src(
                2,
                SettingScope::User,
                Some("/home/a"),
                json!({"sandbox_mode": "danger-full-access"}),
            ),
            src(
                2,
                SettingScope::User,
                Some("/home/b"),
                json!({"sandbox_mode": "workspace-write"}),
            ),
        ];
        let out = evaluate_settings(&sources, &tracked("sandbox_mode"));
        let eff: Vec<bool> = out.iter().map(|s| s.effective).collect();
        assert_eq!(eff, vec![true, false, true]);
    }

    #[test]
    fn endpoints_reduced_to_host_and_secrets_redacted() {
        let v = json!("https://user:pw@otel.example.com:4318/v1/traces?token=abc");
        assert_eq!(
            sanitize_value("env.OTEL_EXPORTER_OTLP_ENDPOINT", &v, true),
            "otel.example.com:4318"
        );
        assert_eq!(
            sanitize_value(
                "env.OTEL_EXPORTER_OTLP_HEADERS",
                &json!("Authorization=Bearer x"),
                false
            ),
            "<redacted>"
        );
        assert_eq!(
            sanitize_value(
                "otel.exporter",
                &json!({"otlp-http": {"endpoint": "https://c.example/v1/logs"}}),
                false
            ),
            r#"{"otlp-http":{"endpoint":"https://c.example"}}"#
        );
    }

    #[test]
    fn telemetry_summary_from_effective_settings() {
        let def = TelemetryDef {
            enabled: vec!["env.CLAUDE_CODE_ENABLE_TELEMETRY".into()],
            endpoints: vec![
                "env.OTEL_EXPORTER_OTLP_ENDPOINT".into(),
                "$OTEL_EXPORTER_OTLP_ENDPOINT".into(),
            ],
            keys: vec![],
        };
        let mk = |key: &str, value: &str, effective: bool| PostureSetting {
            category: PostureCategory::Telemetry,
            key: key.into(),
            value: value.into(),
            scope: SettingScope::Managed,
            source: "x".into(),
            effective,
        };
        let s = telemetry_summary(
            &[
                mk("env.CLAUDE_CODE_ENABLE_TELEMETRY", "1", true),
                mk(
                    "env.OTEL_EXPORTER_OTLP_ENDPOINT",
                    "collector.corp:4317",
                    true,
                ),
                mk("$OTEL_EXPORTER_OTLP_ENDPOINT", "other.corp", true),
                mk("env.OTEL_EXPORTER_OTLP_ENDPOINT", "ignored.corp", false),
            ],
            &def,
        );
        assert_eq!(s.enabled, Some(true));
        assert_eq!(s.endpoints, vec!["collector.corp:4317", "other.corp"]);
    }

    #[test]
    fn policy_keys_summarised_with_categories() {
        let mut cats = HashMap::new();
        cats.insert("permissions".to_string(), "permissions".to_string());
        let keys = summarize_policy_keys(
            &json!({"permissions": {"deny": ["Bash(curl:*)"], "allow": []}, "apiKeyHelper": "/bin/x", "allowedMcpServers": [{"serverName": "a"}, {"serverName": "b"}]}),
            &cats,
        );
        let perm = keys.iter().find(|k| k.key == "permissions").unwrap();
        assert_eq!(perm.category, "permissions");
        assert!(perm.summary.contains("deny"));
        let helper = keys.iter().find(|k| k.key == "apiKeyHelper").unwrap();
        assert_eq!(helper.summary, "<redacted>");
        let mcp = keys.iter().find(|k| k.key == "allowedMcpServers").unwrap();
        assert!(mcp.summary.starts_with("2 item(s)"));
    }

    #[test]
    fn dotted_policy_names_are_nested() {
        let v = unflatten_dotted_keys(
            json!({"sandbox.enabled": "true", "model": "x", "telemetry.capture.prompts": "false"}),
        );
        assert_eq!(lookup(&v, "sandbox.enabled"), Some(&json!("true")));
        assert_eq!(
            lookup(&v, "telemetry.capture.prompts"),
            Some(&json!("false"))
        );
        assert_eq!(lookup(&v, "model"), Some(&json!("x")));
    }

    #[test]
    fn toml_sources_parse_to_json() {
        let v = parse_content(
            "sandbox_mode = \"read-only\"\n[otel]\nexporter = \"none\"\n",
            "toml",
        )
        .unwrap();
        assert_eq!(lookup(&v, "otel.exporter"), Some(&json!("none")));
    }
}
