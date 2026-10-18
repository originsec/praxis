//
// Unit tests for the recon helpers in lib/helpers.lua. Each test builds a
// fixture tree under a temp dir, runs a Lua chunk against it in a real
// connector VM (so the praxis host API is the production one), and asserts
// on the JSON it returns.
//

use super::{EMBEDDED_LUA_SCRIPTS, runtime};
use serde_json::Value as JsonValue;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const STUB_CONNECTOR: &str = r#"return { name = "Stub", short_name = "stub" }"#;

pub(super) struct Fixture {
    pub root: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let root = std::env::temp_dir().join(format!("praxis-lua-helpers-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root should be created");
        Self { root }
    }

    pub fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).expect("fixture dir should be created");
        fs::write(&path, contents).expect("fixture file should be written");
        path
    }

    pub fn path(&self, rel: &str) -> String {
        slash(&self.root.join(rel))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(super) fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn run_chunk(lua: &mlua::Lua, chunk: &str) -> JsonValue {
    let json: String = lua
        .load(format!(
            "local helpers = require('praxis.helpers')\nlocal out = (function()\n{}\nend)()\nreturn praxis.json_encode(out)",
            chunk
        ))
        .eval()
        .expect("lua chunk should run");
    serde_json::from_str(&json).expect("chunk output should be JSON")
}

fn eval(chunk: &str) -> JsonValue {
    let lua = runtime::create_vm(STUB_CONNECTOR).expect("stub connector should load");
    run_chunk(&lua, chunk)
}

fn lua_str(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

//
// Load an embedded connector and point praxis.user_homes at the fixture's
// home dir. Returns the recon result as JSON (post redaction).
//

pub(super) fn connector_recon(short_name: &str, home: &str) -> (common::ReconResult, String) {
    let needle = format!("AGENT_SHORT_NAME = \"{}\"", short_name);
    let script = EMBEDDED_LUA_SCRIPTS
        .iter()
        .find(|s| s.contains(&needle))
        .expect("connector should be embedded");
    let lua = runtime::create_vm(script).expect("connector should load");
    lua.load(format!(
        "praxis.user_homes = function() return {{ {} }} end\npraxis.env_get = function() return nil end",
        lua_str(home)
    ))
    .exec()
    .expect("home override should load");
    let recon = runtime::vm_recon(&lua, false, None).expect("recon should complete");
    let json = serde_json::to_string(&recon).unwrap();
    (recon, json)
}

#[test]
fn parse_hooks_handles_nested_and_flat_shapes() {
    let out = eval(
        r#"
        local nested = helpers.parse_hooks({
          PreToolUse = {
            { matcher = "Bash", hooks = { { type = "command", command = "audit.sh", timeout = 5 } } },
          },
          Stop = { { hooks = { { type = "http", url = "https://example.test/hook" } } } },
          Interrupt = { { hooks = { { type = "mcp_tool", server = "srv", tool = "halt" } } } },
        }, { scope = "project", source_path = "/p/.claude/settings.json", context_path = "/p" })
        local flat = helpers.parse_hooks({
          beforeShellExecution = { { command = "./guard.sh", matcher = "rm" } },
          stop = { command = "notify" },
        }, {})
        return { nested = nested, flat = flat }
        "#,
    );

    let nested = out["nested"].as_array().unwrap();
    assert_eq!(nested.len(), 3);
    assert_eq!(nested[0]["event"], "Interrupt");
    assert_eq!(nested[0]["command"], "mcp:srv/halt");
    let nested = &nested[1..];
    assert_eq!(nested[0]["event"], "PreToolUse");
    assert_eq!(nested[0]["matcher"], "Bash");
    assert_eq!(nested[0]["hook_type"], "command");
    assert_eq!(nested[0]["command"], "audit.sh");
    assert_eq!(nested[0]["scope"], "project");
    assert_eq!(nested[0]["context_path"], "/p");
    assert_eq!(nested[1]["event"], "Stop");
    assert_eq!(nested[1]["command"], "https://example.test/hook");

    let flat = out["flat"].as_array().unwrap();
    assert_eq!(flat.len(), 2);
    assert_eq!(flat[0]["event"], "beforeShellExecution");
    assert_eq!(flat[0]["matcher"], "rm");
    assert_eq!(flat[0]["scope"], "user");
    assert_eq!(flat[1]["command"], "notify");
}

#[test]
fn collect_configs_extracts_hooks_from_json_and_toml() {
    let fx = Fixture::new();
    fx.write(
        "home/.agent/settings.json",
        r#"{"hooks":{"PostToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"fmt"}]}]}}"#,
    );
    fx.write(
        "home/.agent/hooks.toml",
        "[[SessionStart]]\ncommand = \"echo hi\"\n",
    );
    let out = eval(&format!(
        r#"
        local r = helpers.collect_configs({}, {{
          {{ path = ".agent/settings.json", type = "global_settings", hooks = true }},
          {{ path = ".agent/hooks.toml", type = "global_hooks", hooks = "root", hook_scope = "managed" }},
        }}, {{ scope = "home" }})
        return r.hooks
        "#,
        lua_str(&fx.path("home"))
    ));
    let hooks = out.as_array().unwrap();
    assert_eq!(hooks.len(), 2);
    assert_eq!(hooks[0]["event"], "PostToolUse");
    assert_eq!(
        hooks[0]["source_path"],
        fx.path("home/.agent/settings.json")
    );
    assert_eq!(hooks[1]["event"], "SessionStart");
    assert_eq!(hooks[1]["scope"], "managed");
}

#[test]
fn discover_skill_dirs_reads_agent_and_shared_dirs() {
    let fx = Fixture::new();
    fx.write(
        "home/.agents/skills/pdf/SKILL.md",
        "---\nname: pdf\ndescription: Work with PDFs\n---\nbody",
    );
    fx.write(
        "home/.claude/skills/review/SKILL.md",
        "---\ndescription: Review\n---\n",
    );
    fx.write(
        "proj/.github/skills/deploy/SKILL.md",
        "---\nname: deploy\ndescription: Ship it\n---\n",
    );
    fx.write("proj/.agents/skills/not-a-skill/README.md", "nope");
    let out = eval(&format!(
        r#"
        return helpers.discover_skill_dirs({}, {{ {} }}, {{
          home = {{ ".claude/skills", helpers.SHARED_SKILL_DIRS }},
          project = {{ helpers.SHARED_SKILL_DIRS, ".github/skills" }},
        }})
        "#,
        lua_str(&fx.path("home")),
        lua_str(&fx.path("proj"))
    ));
    let mut names: Vec<String> = out
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["deploy", "pdf", "review"]);
    let deploy = out
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "deploy")
        .unwrap();
    assert_eq!(deploy["context_path"], fx.path("proj"));
    assert_eq!(deploy["description"], "Ship it");
}

#[test]
fn instruction_imports_are_followed_but_code_is_ignored() {
    let fx = Fixture::new();
    fx.write(
        "proj/CLAUDE.md",
        "See @docs/style.md and @~/shared.md.\n```\n@docs/ignored.md\n```\nInline `@docs/also-ignored.md`. Mail me@example.com",
    );
    fx.write("proj/docs/style.md", "Nested: @../nested.md");
    fx.write("proj/nested.md", "leaf");
    fx.write("proj/docs/ignored.md", "x");
    fx.write("proj/docs/also-ignored.md", "x");
    fx.write("home/shared.md", "shared");
    let lua = runtime::create_vm(STUB_CONNECTOR).unwrap();
    lua.load(format!(
        "praxis.extract_user_home = function() return {} end",
        lua_str(&fx.path("home"))
    ))
    .exec()
    .unwrap();
    let out = run_chunk(
        &lua,
        &format!(
            r#"
            local r = helpers.new_recon_result()
            table.insert(r.config_items, {{ path = {}, config_type = "project_instructions:" .. {} }})
            helpers.collect_instruction_imports(r)
            return r.config_items
            "#,
            lua_str(&fx.path("proj/CLAUDE.md")),
            lua_str(&fx.path("proj"))
        ),
    );
    let paths: Vec<String> = out
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["path"].as_str().unwrap().replace("/docs/../", "/"))
        .collect();
    assert!(paths.contains(&fx.path("proj/docs/style.md")));
    assert!(paths.contains(&fx.path("home/shared.md")));
    assert!(paths.contains(&fx.path("proj/nested.md")));
    assert!(!paths.iter().any(|p| p.contains("ignored")));
    assert_eq!(paths.len(), 4);
    assert!(
        out[1]["config_type"]
            .as_str()
            .unwrap()
            .starts_with("imported_instructions:")
    );
}

#[test]
fn credential_metadata_never_carries_secret_values() {
    let fx = Fixture::new();
    fx.write(
        "home/.agent/auth.json",
        r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":"sk-proj-SECRETSECRETSECRET","tokens":{"access_token":"eyJSECRETACCESS","refresh_token":"rt_SECRETREFRESH","account_id":"3f2b8c1e-0d4a-4b6f-9a51-6f0e2d7c9b10"},"last_refresh":"2026-10-01T00:00:00Z","nulled":null}"#,
    );
    fx.write("home/.agent/settings.json", r#"{"theme":"dark"}"#);
    let out = eval(&format!(
        r#"
        return helpers.collect_credentials({}, {{
          {{ path = ".agent/auth.json", kind = "oauth_token_file",
             fields = {{ auth_mode = "auth_mode", account_id = "tokens.account_id",
                         last_refresh = "last_refresh",
                         leaked = "tokens.access_token" }},
             presence = {{ "OPENAI_API_KEY", "tokens.refresh_token", "tokens.missing", "nulled" }} }},
          {{ path = ".agent/settings.json", kind = "api_key_in_config",
             presence = {{ "apiKey" }}, require_any = true }},
          {{ path = ".agent/absent.json", kind = "oauth_token_file" }},
        }})
        "#,
        lua_str(&fx.path("home"))
    ));
    let items = out.as_array().unwrap();
    assert_eq!(
        items.len(),
        1,
        "settings without a key and absent files are skipped"
    );
    let md = &items[0]["metadata"];
    assert_eq!(md["auth_mode"], "chatgpt");
    assert_eq!(md["account_id"], "3f2b8c1e-0d4a-4b6f-9a51-6f0e2d7c9b10");
    assert_eq!(md["OPENAI_API_KEY_present"], "true");
    assert_eq!(md["refresh_token_present"], "true");
    assert!(md.get("missing_present").is_none());
    assert!(
        md.get("nulled_present").is_none(),
        "JSON null is not presence"
    );
    assert!(items[0]["last_modified"].is_string());

    //
    // The Lua allow-list copies whatever path a connector names, so a
    // misconfigured `leaked` field reaches the Rust side — which must strip it.
    //

    let mut cred: common::CredentialItem = serde_json::from_value(items[0].clone()).unwrap();
    cred.redact();
    let json = serde_json::to_string(&cred).unwrap();
    assert!(
        !json.contains("SECRET"),
        "redacted credential leaked: {}",
        json
    );
}

//
// End-to-end: a Claude Code home with hooks, a plugin, memory, a rule
// import and live-looking OAuth tokens. Asserts the new sections are
// populated and that no secret value survives into the serialized result.
//

#[test]
fn claudecode_recon_collects_depth_sections_without_secrets() {
    let fx = Fixture::new();
    let home = fx.path("home");
    let proj = fx.path("home/work/app");
    let plugin_root = fx.path("home/.claude/plugins/cache/mkt/fmt/1.0.0");

    fx.write(
        "home/.claude/settings.json",
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"audit.sh"}]}]},
            "enabledPlugins":{"fmt@mkt":true},
            "env":{"ANTHROPIC_API_KEY":"sk-ant-api03-SECRETSECRETSECRETSECRET"}}"#,
    );
    fx.write(
        "home/.claude/.credentials.json",
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-SECRETACCESS","refreshToken":"sk-ant-ort01-SECRETREFRESH","expiresAt":1790000000000,"subscriptionType":"max"}}"#,
    );
    fx.write(
        "home/.claude.json",
        &format!(
            r#"{{"oauthAccount":{{"emailAddress":"user@example.com","accountUuid":"3f2b8c1e-0d4a-4b6f-9a51-6f0e2d7c9b10"}},"projects":{{{}:{{}}}}}}"#,
            lua_str(&proj)
        ),
    );
    fx.write(
        "home/.claude/plugins/installed_plugins.json",
        &format!(
            r#"{{"version":2,"plugins":{{"fmt@mkt":[{{"scope":"user","installPath":{},"version":"1.0.0"}}]}}}}"#,
            lua_str(&plugin_root)
        ),
    );
    fx.write(
        "home/.claude/plugins/known_marketplaces.json",
        r#"{"mkt":{"source":{"source":"github","repo":"example/mkt"}}}"#,
    );
    fx.write(
        "home/.claude/plugins/cache/mkt/fmt/1.0.0/hooks/hooks.json",
        r#"{"hooks":{"PostToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"fmt.sh"}]}]}}"#,
    );
    fx.write(
        "home/.claude/projects/-home-work-app/memory/MEMORY.md",
        "- [Fact](fact.md)",
    );
    fx.write(
        "home/.claude/projects/-home-work-app/memory/fact.md",
        "fact",
    );
    fx.write("home/work/app/CLAUDE.md", "Follow @docs/rules.md");
    fx.write("home/work/app/docs/rules.md", "rules");
    fx.write(
        "home/work/app/.claude/settings.local.json",
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"notify"}]}]}}"#,
    );

    let (recon, json) = connector_recon("claudecode", &home);

    let events: Vec<(&str, &str)> = recon
        .tools
        .hooks
        .iter()
        .map(|h| (h.event.as_str(), h.scope.as_str()))
        .collect();
    assert!(events.contains(&("PreToolUse", "user")), "{:?}", events);
    assert!(events.contains(&("PostToolUse", "plugin")), "{:?}", events);
    assert!(events.contains(&("Stop", "local")), "{:?}", events);

    assert_eq!(recon.tools.plugins.len(), 1);
    assert!(recon.tools.plugins[0].enabled);
    assert_eq!(recon.tools.marketplaces[0].source, "github:example/mkt");

    assert_eq!(
        recon.config.items.iter().filter(|c| c.is_memory()).count(),
        2
    );
    assert!(
        recon
            .config
            .items
            .iter()
            .any(|c| c.path.ends_with("docs/rules.md") && c.is_instructions())
    );

    let oauth = recon
        .config
        .credentials
        .iter()
        .find(|c| c.kind == "oauth_token_file")
        .expect("oauth credential store should be reported");
    assert_eq!(oauth.metadata["subscription_type"], "max");
    assert_eq!(oauth.metadata["refreshToken_present"], "true");
    assert!(oauth.metadata["expires_at"].starts_with("2026-"));
    let account = recon
        .config
        .credentials
        .iter()
        .find(|c| c.kind == "account_config")
        .unwrap();
    assert_eq!(account.metadata["email"], "user@example.com");
    assert!(recon.config.credentials.iter().any(|c| {
        c.kind == "api_key_in_config"
            && c.metadata
                .get("ANTHROPIC_API_KEY_present")
                .map(String::as_str)
                == Some("true")
    }));

    assert!(!json.contains("SECRET"), "secret leaked into recon output");
    assert!(
        !recon
            .config
            .items
            .iter()
            .any(|c| c.path.ends_with(".credentials.json")),
        "credential files must not be readable config items"
    );
}

//
// Run every embedded connector's static recon against this machine and
// print counts only. Used to produce the PR summary:
//   cargo test -p praxis_node live_recon_counts -- --ignored --nocapture
//

#[test]
#[ignore]
fn live_recon_counts() {
    for short in [
        "claudecode",
        "codex",
        "gemini",
        "agy",
        "cursor",
        "droid",
        "pi",
        "claudedesktop",
    ] {
        let needle = format!("AGENT_SHORT_NAME = \"{}\"", short);
        let Some(script) = EMBEDDED_LUA_SCRIPTS.iter().find(|s| s.contains(&needle)) else {
            continue;
        };
        let lua = runtime::create_vm(script).unwrap();
        let process_path = runtime::vm_fingerprint_details(&lua)
            .ok()
            .and_then(|f| f.process_path);
        let r = match runtime::vm_recon(&lua, false, process_path.as_deref()) {
            Ok(r) => r,
            Err(e) => {
                println!("{short}: recon failed: {e}");
                continue;
            }
        };
        println!(
            "{short}: configs={} memory={} instructions={} projects={} skills={} hooks={} plugins={} marketplaces={} credentials={} sessions={} mcp={}",
            r.config.items.len(),
            r.config.items.iter().filter(|c| c.is_memory()).count(),
            r.config
                .items
                .iter()
                .filter(|c| c.is_instructions())
                .count(),
            r.config.project_paths.len(),
            r.tools.skills.len(),
            r.tools.hooks.len(),
            r.tools.plugins.len(),
            r.tools.marketplaces.len(),
            r.config.credentials.len(),
            r.sessions.items.len(),
            r.tools.mcp_servers.len(),
        );
        if std::env::var("PRAXIS_LIVE_CREDS").is_ok() {
            for c in &r.config.credentials {
                let keys: Vec<&String> = c.metadata.keys().collect();
                let masked: Vec<&String> = c
                    .metadata
                    .iter()
                    .filter(|(_, v)| v.as_str() == common::redact::REDACTED)
                    .map(|(k, _)| k)
                    .collect();
                println!(
                    "  cred kind={} exists={:?} keys={:?} masked={:?}",
                    c.kind, c.exists, keys, masked
                );
            }
        }
        if std::env::var("PRAXIS_LIVE_HOOKS").is_ok() {
            for h in &r.tools.hooks {
                let file = h.source_path.rsplit('/').next().unwrap_or("");
                println!(
                    "  hook {} scope={} file={} type={:?}",
                    h.event, h.scope, file, h.hook_type
                );
            }
        }
    }
}
