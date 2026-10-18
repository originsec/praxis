use super::{EMBEDDED_LUA_SCRIPTS, runtime};
use mlua::Lua;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

//
// Fixture-driven tests for connectors whose targets can't run in CI (or on
// Linux at all). Each test loads the embedded script, points
// praxis.user_homes at a temp home and, where needed, overrides
// praxis.os_name so macOS/Windows path logic runs on any host.
//

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("praxis-connector-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root should be created");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let path = self.home().join(rel);
        fs::create_dir_all(path.parent().unwrap()).expect("fixture dir should be created");
        fs::write(&path, contents).expect("fixture file should be written");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn norm(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn load_connector(short_name: &str, home: &Path, os_name: Option<&str>) -> Lua {
    let marker = format!("AGENT_SHORT_NAME = \"{}\"", short_name);
    let script = EMBEDDED_LUA_SCRIPTS
        .iter()
        .find(|script| script.contains(&marker))
        .unwrap_or_else(|| panic!("{} connector should be embedded", short_name));
    let lua = runtime::create_vm(script).expect("connector should load");

    let home_json = serde_json::to_string(&norm(home)).unwrap();
    lua.load(format!(
        "praxis.user_homes = function() return {{ {} }} end",
        home_json
    ))
    .exec()
    .expect("home override should load");

    if let Some(os_name) = os_name {
        lua.load(format!(
            "praxis.os_name = function() return \"{}\" end",
            os_name
        ))
        .exec()
        .expect("os override should load");
    }
    lua
}

fn config_types(recon: &common::ReconResult) -> Vec<String> {
    recon
        .config
        .items
        .iter()
        .map(|item| item.config_type.clone())
        .collect()
}

fn sqlite3_available() -> bool {
    std::process::Command::new("sqlite3")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn json5_config_is_normalised() {
    let fixture = Fixture::new();
    let lua = load_connector("openclaw", &fixture.home(), None);
    let parsed: String = lua
        .load(
            r#"
            local helpers = require("praxis.helpers")
            local cfg = helpers.parse_json5([[
              // comment
              {
                gateway: { url: "http://127.0.0.1:18789/ws", },
                /* block */ list: ['a', "b\"c", 'it\'s',],
                n: -1.5e3, ok: true, none: null,
              }
            ]])
            return praxis.json_encode(cfg)
            "#,
        )
        .eval()
        .expect("json5 parse should run");
    let value: serde_json::Value = serde_json::from_str(&parsed).unwrap();
    assert_eq!(value["gateway"]["url"], "http://127.0.0.1:18789/ws");
    assert_eq!(value["list"], serde_json::json!(["a", "b\"c", "it's"]));
    assert_eq!(value["n"], -1500.0);
    assert_eq!(value["ok"], true);
    assert!(value["none"].is_null());
}

#[test]
fn openclaw_recon_reads_config_workspace_and_legacy_sessions() {
    let fixture = Fixture::new();
    fixture.write(
        ".openclaw/openclaw.json",
        r#"{
          // JSON5, as hand-edited configs are
          models: { providers: { vllm: { baseUrl: "http://127.0.0.1:8000/v1", apiKey: "x" } } },
          agents: {
            defaults: { model: { primary: "vllm/local-1" } },
            list: [{ id: "work", workspace: "~/.openclaw/workspace-work" }],
          },
          channels: { telegram: { enabled: true }, slack: {} },
          mcp: { servers: { files: { command: "npx", args: ["-y", "server-fs"] } } },
        }"#,
    );
    fixture.write(".openclaw/workspace/AGENTS.md", "# agents");
    fixture.write(".openclaw/workspace/MEMORY.md", "facts");
    fixture.write(
        ".openclaw/workspace/skills/inbox/SKILL.md",
        "---\nname: inbox-triage\ndescription: Sort mail\n---\n",
    );
    fixture.write(".openclaw/workspace-work/memory/2026-10-01.md", "notes");
    fixture.write(
        ".openclaw/skills/weather/SKILL.md",
        "---\nname: weather\ndescription: Forecasts\n---\n",
    );
    fixture.write(
        ".openclaw/agents/main/sessions/legacy-1.jsonl",
        "{\"type\":\"session\",\"id\":\"legacy-1\"}\n{\"type\":\"message\"}\n",
    );

    let lua = load_connector("openclaw", &fixture.home(), None);
    let recon = runtime::vm_recon(&lua, false, None).expect("recon should complete");

    let workspace = norm(&fixture.home().join(".openclaw/workspace"));
    let workspace_work = norm(&fixture.home().join(".openclaw/workspace-work"));
    let paths: Vec<String> = recon
        .config
        .project_paths
        .iter()
        .map(|p| p.replace('\\', "/"))
        .collect();
    assert!(paths.contains(&workspace), "default workspace: {:?}", paths);
    assert!(
        paths.contains(&workspace_work),
        "agent workspace: {:?}",
        paths
    );

    let types = config_types(&recon);
    for expected in [
        "global_settings",
        "model_provider:vllm",
        "default_model:vllm/local-1",
        "channel:slack",
        "channel:telegram",
    ] {
        assert!(
            types.iter().any(|t| t == expected),
            "{} in {:?}",
            expected,
            types
        );
    }
    assert!(
        types
            .iter()
            .any(|t| t.starts_with("workspace_instructions:"))
    );
    assert!(types.iter().filter(|t| t.starts_with("memory:")).count() >= 2);

    let mcp = &recon.tools.mcp_servers;
    assert_eq!(mcp.len(), 1);
    assert_eq!(mcp[0].name, "files");
    assert_eq!(mcp[0].command.as_deref(), Some("npx -y server-fs"));

    let skills: Vec<&str> = recon.tools.skills.iter().map(|s| s.name.as_str()).collect();
    assert!(skills.contains(&"inbox-triage"), "{:?}", skills);
    assert!(skills.contains(&"weather"), "{:?}", skills);

    let session = recon
        .sessions
        .items
        .iter()
        .find(|s| s.session_id == "legacy-1")
        .expect("legacy JSONL session should be discovered");
    assert_eq!(session.context_path.replace('\\', "/"), workspace);
}

#[test]
fn openclaw_reads_sqlite_sessions() {
    if !sqlite3_available() {
        eprintln!("sqlite3 not on PATH; skipping");
        return;
    }
    let fixture = Fixture::new();
    fixture.write(".openclaw/openclaw.json", "{}");
    let db = fixture
        .home()
        .join(".openclaw/agents/main/agent/openclaw-agent.sqlite");
    fs::create_dir_all(db.parent().unwrap()).unwrap();

    //
    // Minimal subset of the agent schema (2026.10.x) the connector reads.
    //

    let status = std::process::Command::new("sqlite3")
        .arg(&db)
        .arg(
            "CREATE TABLE session_windows (session_id TEXT PRIMARY KEY, session_key TEXT, updated_at INTEGER);
             CREATE TABLE transcript_events (session_id TEXT, seq INTEGER, event_json TEXT, event_zstd BLOB);
             INSERT INTO session_windows VALUES ('s-1', 'agent:main:explicit:s-1', 1792347348570);
             INSERT INTO transcript_events VALUES ('s-1', 0, '{\"type\":\"session\",\"id\":\"s-1\"}', NULL);
             INSERT INTO transcript_events VALUES ('s-1', 1, '{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}', NULL);
             INSERT INTO transcript_events VALUES ('s-1', 2, NULL, x'00');",
        )
        .status()
        .expect("sqlite3 should run");
    assert!(status.success());

    let lua = load_connector("openclaw", &fixture.home(), None);
    let recon = runtime::vm_recon(&lua, false, None).expect("recon should complete");
    let session = recon
        .sessions
        .items
        .iter()
        .find(|s| s.session_id == "s-1")
        .expect("sqlite session should be discovered");
    assert_eq!(session.message_count, 2);
    assert!(session.session_file.ends_with("openclaw-agent.sqlite::s-1"));

    let content = runtime::vm_read_session_content(&lua, &session.session_file)
        .expect("read should run")
        .expect("content should be returned");
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[1].contains("\"role\":\"user\""));
    assert!(lines[2].contains("praxis_compressed_event"));
}

#[test]
fn claudechrome_detects_extension_in_chromium_profiles() {
    let fixture = Fixture::new();
    let lua = load_connector("claudechrome", &fixture.home(), Some("linux"));
    let details = runtime::vm_fingerprint_details(&lua).expect("fingerprint should run");
    assert!(!details.available);

    let profile = ".config/chromium/Profile 1";
    fixture.write(&format!("{}/Preferences", profile), "{}");
    fixture.write(
        &format!(
            "{}/Extensions/fcoeoabgfenejglbffodgkkbkcdhcgfn/1.0.101_0/manifest.json",
            profile
        ),
        r#"{"name":"Claude","version":"1.0.101"}"#,
    );
    fixture.write(
        ".config/chromium/NativeMessagingHosts/com.anthropic.claude_code_browser_extension.json",
        "{}",
    );

    let details = runtime::vm_fingerprint_details(&lua).expect("fingerprint should run");
    assert!(details.available);
    assert_eq!(details.version.as_deref(), Some("1.0.101"));

    let recon = runtime::vm_recon(&lua, false, None).expect("recon should complete");
    let types = config_types(&recon);
    assert!(types.contains(&"extension_manifest:Chromium/Profile 1".to_string()));
    assert!(types.contains(&"browser_preferences:Chromium/Profile 1".to_string()));
    assert!(types.contains(&"native_messaging_host:Chromium".to_string()));
}

#[test]
fn claudedesktop_macos_recon_includes_cowork_state() {
    let fixture = Fixture::new();
    let support = "Library/Application Support/Claude";
    fixture.write(
        &format!("{}/claude_desktop_config.json", support),
        r#"{"mcpServers":{"notes":{"command":"notes-mcp"}}}"#,
    );
    let account = format!("{}/local-agent-mode-sessions/acct/org", support);
    fixture.write(
        &format!("{}/local_1234.json", account),
        r#"{"messages":[{},{}]}"#,
    );
    fixture.write(&format!("{}/scheduled-tasks.json", account), "[]");
    fixture.write(&format!("{}/memory/CLAUDE.md", account), "remember");
    fixture.write(
        &format!(
            "{}/cowork_plugins/sales/.claude-plugin/plugin.json",
            support
        ),
        r#"{"name":"sales"}"#,
    );
    fixture.write(
        &format!("{}/cowork_plugins/sales/skills/forecast/SKILL.md", support),
        "---\nname: forecast\ndescription: Pipeline forecast\n---\n",
    );
    fixture.write("Library/Logs/Claude/main.log", "log");

    let lua = load_connector("claudedesktop", &fixture.home(), Some("macos"));
    let recon = runtime::vm_recon(&lua, false, None).expect("recon should complete");
    let types = config_types(&recon);
    for expected in [
        "global_settings",
        "cowork_scheduled_tasks",
        "cowork_memory",
        "cowork_plugin",
        "log",
    ] {
        assert!(
            types.iter().any(|t| t == expected),
            "{} in {:?}",
            expected,
            types
        );
    }
    assert_eq!(recon.tools.mcp_servers.len(), 1);
    assert!(recon.tools.skills.iter().any(|s| s.name == "forecast"));

    let session = recon
        .sessions
        .items
        .iter()
        .find(|s| s.session_id == "1234")
        .expect("Cowork session should be discovered");
    assert_eq!(session.message_count, 2);
}

#[test]
fn codexdesktop_recon_reports_only_app_specific_state() {
    let fixture = Fixture::new();
    fixture.write(".codex/config.toml", "model = \"gpt-5\"");
    fixture.write(".codex/.codex-global-state.json", "{}");
    fixture.write(".codex/worktrees/a1b2/repo/.git", "gitdir: /x");

    let lua = load_connector("codexdesktop", &fixture.home(), Some("macos"));
    let recon = runtime::vm_recon(&lua, false, None).expect("recon should complete");
    let types = config_types(&recon);
    assert_eq!(types, vec!["app_global_state".to_string()]);
    let worktree = norm(&fixture.home().join(".codex/worktrees/a1b2/repo"));
    assert_eq!(
        recon
            .config
            .project_paths
            .iter()
            .map(|p| p.replace('\\', "/"))
            .collect::<Vec<_>>(),
        vec![worktree]
    );
}
