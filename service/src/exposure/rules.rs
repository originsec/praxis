use common::exposure::{AgentModifier, CapabilityClass, CapabilityKind, ClassReason, ModifierKind};
use indexmap::IndexMap;
use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use serde_json::Value;

//
// Parsing and evaluation of the exposure-rules virtual file. The service
// persists the raw TOML in service_config under `exposure_rules_toml` and
// falls back to the embedded defaults while the key is unset, so shipped
// default improvements reach installs that never edited the file.
//

const DEFAULT_TEXT: &str = include_str!("default_exposure_rules.toml");

pub const SERVICE_CONFIG_KEY: &str = "exposure_rules_toml";

pub fn default_text() -> &'static str {
    DEFAULT_TEXT
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    builtins: IndexMap<String, Vec<String>>,
    #[serde(default, rename = "rule")]
    rules: Vec<RawRule>,
    #[serde(default, rename = "modifier")]
    modifiers: Vec<RawModifier>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    id: String,
    classes: Vec<String>,
    #[serde(default)]
    kinds: Vec<String>,
    #[serde(default)]
    agents: Vec<String>,
    name: Option<String>,
    description: Option<String>,
    server: Option<String>,
    command: Option<String>,
    url: Option<String>,
    #[serde(default)]
    stop: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModifier {
    id: String,
    kind: String,
    #[serde(default)]
    agents: Vec<String>,
    config_types: Vec<String>,
    key: String,
    #[serde(default)]
    equals: Vec<String>,
}

#[derive(Debug)]
pub struct Rule {
    pub id: String,
    classes: Vec<CapabilityClass>,
    kinds: Vec<CapabilityKind>,
    agents: Vec<String>,
    name: Option<Regex>,
    description: Option<Regex>,
    server: Option<Regex>,
    command: Option<Regex>,
    url: Option<Regex>,
    stop: bool,
}

#[derive(Debug)]
pub struct ModifierRule {
    pub id: String,
    kind: ModifierKind,
    agents: Vec<String>,
    pub config_types: Vec<String>,
    key: Vec<String>,
    equals: Vec<String>,
}

#[derive(Debug, Default)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
    pub builtins: IndexMap<String, Vec<String>>,
    pub modifiers: Vec<ModifierRule>,
}

//
// What a rule is evaluated against. MCP fields are set for MCP tools and
// servers; `command` / `url` come from the owning server definition.
//

pub struct Subject<'a> {
    pub kind: CapabilityKind,
    pub agent: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub server: Option<&'a str>,
    pub command: Option<&'a str>,
    pub url: Option<&'a str>,
}

//
// Outcome of running the rule list over one subject. `matched` is the id of
// the first rule that matched (even one with no classes), so explicit
// benign verdicts can be told apart from "nothing matched".
//

#[derive(Debug, Default)]
pub struct Classification {
    pub classes: Vec<ClassReason>,
    pub matched: Option<String>,
}

fn compile(field: &str, rule_id: &str, pattern: Option<String>) -> Result<Option<Regex>, String> {
    pattern
        .map(|p| {
            RegexBuilder::new(&p)
                .case_insensitive(true)
                .build()
                .map_err(|e| format!("Rule '{}': invalid {} regex: {}", rule_id, field, e))
        })
        .transpose()
}

pub fn parse(text: &str) -> Result<RuleSet, String> {
    let raw: RawFile = toml::from_str(text).map_err(|e| format!("TOML parse error: {}", e))?;
    let mut ids = std::collections::HashSet::new();

    let mut rules = Vec::with_capacity(raw.rules.len());
    for r in raw.rules {
        let id = r.id.trim().to_string();
        if id.is_empty() {
            return Err("Every [[rule]] needs a non-empty id.".to_string());
        }
        if !ids.insert(format!("rule:{}", id)) {
            return Err(format!("Duplicate rule id '{}'.", id));
        }
        let classes = r
            .classes
            .iter()
            .map(|c| {
                CapabilityClass::parse(c).ok_or_else(|| {
                    format!(
                        "Rule '{}': unknown class '{}' (expected private_data, untrusted_content or exfiltration).",
                        id, c
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let kinds = r
            .kinds
            .iter()
            .map(|k| {
                CapabilityKind::parse(k).ok_or_else(|| {
                    format!(
                        "Rule '{}': unknown kind '{}' (expected mcp_tool, mcp_server, skill, internal or builtin).",
                        id, k
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let rule = Rule {
            name: compile("name", &id, r.name)?,
            description: compile("description", &id, r.description)?,
            server: compile("server", &id, r.server)?,
            command: compile("command", &id, r.command)?,
            url: compile("url", &id, r.url)?,
            id,
            classes,
            kinds,
            agents: r.agents,
            stop: r.stop,
        };
        if rule.name.is_none()
            && rule.description.is_none()
            && rule.server.is_none()
            && rule.command.is_none()
            && rule.url.is_none()
        {
            return Err(format!(
                "Rule '{}': at least one of name, description, server, command or url is required.",
                rule.id
            ));
        }
        rules.push(rule);
    }

    let mut modifiers = Vec::with_capacity(raw.modifiers.len());
    for m in raw.modifiers {
        let id = m.id.trim().to_string();
        if id.is_empty() {
            return Err("Every [[modifier]] needs a non-empty id.".to_string());
        }
        if !ids.insert(format!("modifier:{}", id)) {
            return Err(format!("Duplicate modifier id '{}'.", id));
        }
        let kind = match m.kind.trim() {
            "yolo" => ModifierKind::Yolo,
            "auto_approve" => ModifierKind::AutoApprove,
            other => {
                return Err(format!(
                    "Modifier '{}': unknown kind '{}' (expected yolo or auto_approve).",
                    id, other
                ));
            }
        };
        let key: Vec<String> = m.key.split('.').map(|s| s.trim().to_string()).collect();
        if key.iter().any(|segment| segment.is_empty()) {
            return Err(format!("Modifier '{}': invalid key '{}'.", id, m.key));
        }
        if m.config_types.is_empty() {
            return Err(format!(
                "Modifier '{}': config_types must list at least one type.",
                id
            ));
        }
        modifiers.push(ModifierRule {
            id,
            kind,
            agents: m.agents,
            config_types: m.config_types,
            key,
            equals: m.equals,
        });
    }

    Ok(RuleSet {
        rules,
        builtins: raw.builtins,
        modifiers,
    })
}

impl Rule {
    fn matches(&self, s: &Subject) -> bool {
        if !self.kinds.is_empty() && !self.kinds.contains(&s.kind) {
            return false;
        }
        if !self.agents.is_empty() && !self.agents.iter().any(|a| a == s.agent) {
            return false;
        }
        let check = |re: &Option<Regex>, value: Option<&str>| match re {
            None => true,
            Some(re) => value.is_some_and(|v| re.is_match(v)),
        };
        check(&self.name, Some(s.name))
            && check(&self.description, Some(s.description))
            && check(&self.server, s.server)
            && check(&self.command, s.command)
            && check(&self.url, s.url)
    }
}

impl RuleSet {
    pub fn classify(&self, subject: &Subject) -> Classification {
        let mut out = Classification::default();
        for rule in &self.rules {
            if !rule.matches(subject) {
                continue;
            }
            if out.matched.is_none() {
                out.matched = Some(format!("rule:{}", rule.id));
            }
            for class in &rule.classes {
                if !out.classes.iter().any(|c| c.class == *class) {
                    out.classes.push(ClassReason {
                        class: *class,
                        reason: format!("rule:{}", rule.id),
                    });
                }
            }
            if rule.stop {
                break;
            }
        }
        out.classes.sort_by_key(|c| c.class);
        out
    }

    pub fn builtins_for(&self, agent: &str) -> &[String] {
        self.builtins.get(agent).map(Vec::as_slice).unwrap_or(&[])
    }

    //
    // Modifier rules that want a config file of the given recon config
    // type (the part before ':' for project-scoped types) for this agent.
    //

    pub fn modifiers_for<'a>(
        &'a self,
        agent: &'a str,
        config_type: &'a str,
    ) -> impl Iterator<Item = &'a ModifierRule> + 'a {
        let base = config_type.split(':').next().unwrap_or(config_type);
        self.modifiers.iter().filter(move |m| {
            (m.agents.is_empty() || m.agents.iter().any(|a| a == agent))
                && m.config_types.iter().any(|t| t == base)
        })
    }
}

impl ModifierRule {
    //
    // Evaluate this rule against a parsed settings document. Array values
    // yield one modifier per string entry so allow-list entries can later
    // be matched against individual capabilities.
    //

    pub fn evaluate(&self, doc: &Value, source: &str) -> Vec<AgentModifier> {
        let mut hits = Vec::new();
        resolve(doc, &self.key, String::new(), &mut hits);

        let mut out = Vec::new();
        let mut push = |detail: String| {
            out.push(AgentModifier {
                kind: self.kind,
                detail: tidy_detail(&detail),
                source: source.to_string(),
                reason: format!("rule:{}", self.id),
            })
        };
        for (path, value) in hits {
            if !self.equals.is_empty() {
                let scalar = match value {
                    Value::String(s) => s.clone(),
                    Value::Bool(b) => b.to_string(),
                    Value::Number(n) => n.to_string(),
                    _ => continue,
                };
                if self.equals.iter().any(|e| e == &scalar) {
                    push(format!("{} = {}", path, scalar));
                }
                continue;
            }
            match value {
                Value::Bool(true) => push(format!("{} = true", path)),
                Value::String(s) if !s.trim().is_empty() => push(format!("{} = {}", path, s)),
                Value::Array(items) => {
                    for item in items.iter().filter_map(|v| v.as_str()) {
                        push(item.to_string());
                    }
                }
                _ => {}
            }
        }
        out
    }
}

//
// Allow-list entries can embed whole scripts (heredoc commit messages and
// the like); keep the first line, capped, so listings stay readable.
//

fn tidy_detail(detail: &str) -> String {
    let first = detail.lines().next().unwrap_or("").trim();
    let capped = common::truncate_str(first, 160);
    if capped.len() < detail.trim().len() {
        format!("{}…", capped)
    } else {
        capped.to_string()
    }
}

fn resolve<'a>(value: &'a Value, key: &[String], path: String, out: &mut Vec<(String, &'a Value)>) {
    let Some((head, rest)) = key.split_first() else {
        out.push((path, value));
        return;
    };
    let Some(obj) = value.as_object() else {
        return;
    };
    let join = |k: &str| {
        if path.is_empty() {
            k.to_string()
        } else {
            format!("{}.{}", path, k)
        }
    };
    if head == "*" {
        for (k, v) in obj {
            resolve(v, rest, join(k), out);
        }
    } else if let Some(v) = obj.get(head) {
        resolve(v, rest, join(head), out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn defaults() -> RuleSet {
        parse(default_text()).expect("default rules parse")
    }

    fn subject<'a>(kind: CapabilityKind, agent: &'a str, name: &'a str) -> Subject<'a> {
        Subject {
            kind,
            agent,
            name,
            description: "",
            server: None,
            command: None,
            url: None,
        }
    }

    fn classes(c: &Classification) -> Vec<CapabilityClass> {
        c.classes.iter().map(|r| r.class).collect()
    }

    use CapabilityClass::{Exfiltration as X, PrivateData as P, UntrustedContent as U};

    #[test]
    fn default_rules_parse_and_have_builtins() {
        let rules = defaults();
        assert!(!rules.rules.is_empty());
        assert!(rules.builtins_for("claudecode").iter().any(|b| b == "Bash"));
        assert!(!rules.modifiers.is_empty());
    }

    #[test]
    fn private_data_fixtures() {
        let rules = defaults();
        let read = rules.classify(&subject(CapabilityKind::Builtin, "claudecode", "Read"));
        assert_eq!(classes(&read), vec![P, U]);
        assert_eq!(read.classes[0].reason, "rule:file-read-private");

        let query = rules.classify(&subject(CapabilityKind::McpTool, "codex", "execute_sql"));
        assert_eq!(classes(&query), vec![P]);

        let mut secrets = subject(CapabilityKind::McpTool, "codex", "lookup");
        secrets.description = "Look up a secret in the team vault";
        assert_eq!(classes(&rules.classify(&secrets)), vec![P]);
    }

    #[test]
    fn untrusted_content_fixtures() {
        let rules = defaults();
        let fetch = rules.classify(&subject(CapabilityKind::Builtin, "claudecode", "WebFetch"));
        assert_eq!(classes(&fetch), vec![U, X]);

        let issues = rules.classify(&subject(
            CapabilityKind::McpTool,
            "claudecode",
            "list_issues",
        ));
        assert_eq!(classes(&issues), vec![P, U]);

        let mut skill = subject(CapabilityKind::Skill, "claudecode", "/triage");
        skill.description = "Triage new GitHub issues and label them";
        assert_eq!(classes(&rules.classify(&skill)), vec![U]);
    }

    #[test]
    fn exfiltration_fixtures() {
        let rules = defaults();
        let send = rules.classify(&subject(
            CapabilityKind::McpTool,
            "claudecode",
            "send_email",
        ));
        assert_eq!(classes(&send), vec![X]);

        let comment = rules.classify(&subject(
            CapabilityKind::McpTool,
            "claudecode",
            "add_issue_comment",
        ));
        assert!(classes(&comment).contains(&X));

        let get_post = rules.classify(&subject(CapabilityKind::McpTool, "claudecode", "get_post"));
        assert!(!classes(&get_post).contains(&X));

        let mut remote = subject(CapabilityKind::McpTool, "claudecode", "lookup");
        remote.server = Some("vendor");
        remote.url = Some("https://mcp.vendor.example.com/sse");
        assert_eq!(classes(&rules.classify(&remote)), vec![X]);

        let mut local = subject(CapabilityKind::McpTool, "claudecode", "lookup");
        local.url = Some("http://localhost:8080/mcp");
        assert!(rules.classify(&local).matched.is_none());
    }

    #[test]
    fn browser_and_code_exec_fixtures() {
        let rules = defaults();
        let mut click = subject(CapabilityKind::McpTool, "claudecode", "click");
        click.server = Some("chrome-devtools");
        assert_eq!(classes(&rules.classify(&click)), vec![U, X]);

        let js = rules.classify(&subject(CapabilityKind::McpTool, "codex", "js"));
        assert_eq!(classes(&js), vec![P, U, X]);
    }

    #[test]
    fn shell_is_full_trifecta() {
        let rules = defaults();
        for (agent, name) in [
            ("claudecode", "Bash"),
            ("gemini", "run_shell_command"),
            ("codex", "shell"),
            ("cursor", "run_terminal_cmd"),
        ] {
            let c = rules.classify(&subject(CapabilityKind::Builtin, agent, name));
            assert_eq!(classes(&c), vec![P, U, X], "{}:{}", agent, name);
        }
    }

    #[test]
    fn benign_and_unclassified_are_distinct() {
        let rules = defaults();
        let write = rules.classify(&subject(CapabilityKind::Builtin, "claudecode", "Write"));
        assert!(write.classes.is_empty());
        assert_eq!(write.matched.as_deref(), Some("rule:file-write"));

        let unknown = rules.classify(&subject(
            CapabilityKind::McpTool,
            "claudecode",
            "frobnicate",
        ));
        assert!(unknown.classes.is_empty());
        assert!(unknown.matched.is_none());
    }

    #[test]
    fn server_rules_only_apply_to_server_kind() {
        let rules = defaults();
        let mut server = subject(CapabilityKind::McpServer, "claudecode", "github");
        server.command = Some("npx @modelcontextprotocol/server-github");
        assert_eq!(classes(&rules.classify(&server)), vec![P, U, X]);

        let mut tool = subject(CapabilityKind::McpTool, "claudecode", "frobnicate");
        tool.server = Some("github");
        tool.command = Some("npx @modelcontextprotocol/server-github");
        assert!(rules.classify(&tool).matched.is_none());
    }

    #[test]
    fn stop_prevents_later_rules() {
        let text = r#"
[[rule]]
id = "first"
classes = []
name = "^x$"
stop = true

[[rule]]
id = "second"
classes = ["exfiltration"]
name = "x"
"#;
        let rules = parse(text).unwrap();
        let c = rules.classify(&subject(CapabilityKind::McpTool, "a", "x"));
        assert!(c.classes.is_empty());
        assert_eq!(c.matched.as_deref(), Some("rule:first"));
    }

    #[test]
    fn invalid_rules_are_rejected() {
        assert!(parse("[[rule]]\nid = \"a\"\nclasses = [\"bogus\"]\nname = \"x\"\n").is_err());
        assert!(parse("[[rule]]\nid = \"a\"\nclasses = []\n").is_err());
        assert!(parse("[[rule]]\nid = \"a\"\nclasses = []\nname = \"(\"\n").is_err());
        assert!(
            parse("[[rule]]\nid = \"a\"\nclasses = []\nname = \"x\"\n[[rule]]\nid = \"a\"\nclasses = []\nname = \"y\"\n")
                .is_err()
        );
        assert!(parse("[[rule]]\nid = \"a\"\nclasses = []\nname = \"x\"\nbogus = 1\n").is_err());
    }

    #[test]
    fn modifier_rules_evaluate_json() {
        let rules = defaults();
        let doc = json!({
            "permissions": {
                "defaultMode": "bypassPermissions",
                "allow": ["Bash(git push:*)", "mcp__github__create_issue"]
            }
        });
        let found: Vec<AgentModifier> = rules
            .modifiers_for("claudecode", "project_settings:/home/u/proj")
            .flat_map(|m| m.evaluate(&doc, "/home/u/proj/.claude/settings.json"))
            .collect();
        assert!(found.iter().any(|m| m.kind == ModifierKind::Yolo
            && m.detail == "permissions.defaultMode = bypassPermissions"));
        let allowed: Vec<&str> = found
            .iter()
            .filter(|m| m.kind == ModifierKind::AutoApprove)
            .map(|m| m.detail.as_str())
            .collect();
        assert_eq!(
            allowed,
            vec!["Bash(git push:*)", "mcp__github__create_issue"]
        );
    }

    #[test]
    fn modifier_wildcard_key() {
        let rules = defaults();
        let doc =
            json!({ "mcpServers": { "github": { "trust": true }, "local": { "trust": false } } });
        let found: Vec<AgentModifier> = rules
            .modifiers_for("gemini", "global_settings")
            .flat_map(|m| m.evaluate(&doc, "settings.json"))
            .collect();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].detail, "mcpServers.github.trust = true");
    }
}
