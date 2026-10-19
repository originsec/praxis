use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::connector_author::DryRunReconSummary;

use super::*;
use crate::database::DatabaseConfig;

const SCRIPT: &str = "return { name = \"X\", short_name = \"x\" }";

fn reply_with(script: &str) -> String {
    format!("Here you go:\n\n```lua\n{}\n```\n\nAssumes x.", script)
}

fn passing() -> ConnectorDryRun {
    ConnectorDryRun {
        ok: true,
        short_name: Some("x".into()),
        recon: Some(DryRunReconSummary {
            config_items: 1,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn failing(stage: &str) -> ConnectorDryRun {
    ConnectorDryRun {
        ok: false,
        failed_stage: Some(stage.into()),
        errors: vec![format!("{} broke", stage)],
        ..Default::default()
    }
}

//
// Stub LLM + node: replies are served in order (the last one repeats) and
// each validation result likewise.
//

struct Stub {
    replies: Vec<String>,
    validations: Vec<ConnectorDryRun>,
    completions: AtomicUsize,
    validations_run: AtomicUsize,
    last_messages: Mutex<Vec<Message>>,
}

impl Stub {
    fn new(replies: Vec<String>, validations: Vec<ConnectorDryRun>) -> Self {
        Self {
            replies,
            validations,
            completions: AtomicUsize::new(0),
            validations_run: AtomicUsize::new(0),
            last_messages: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl AuthorBackend for Stub {
    async fn complete(&self, messages: &[Message]) -> Result<String> {
        *self.last_messages.lock().unwrap() = messages.to_vec();
        let n = self.completions.fetch_add(1, Ordering::SeqCst);
        Ok(self.replies[n.min(self.replies.len() - 1)].clone())
    }

    async fn validate(&self, _script: &str) -> Result<ConnectorDryRun> {
        let n = self.validations_run.fetch_add(1, Ordering::SeqCst);
        Ok(self.validations[n.min(self.validations.len() - 1)].clone())
    }
}

fn seed() -> Vec<Message> {
    vec![build_message(Role::User, "write it".into())]
}

#[tokio::test]
async fn loop_stops_on_first_pass() {
    let stub = Stub::new(vec![reply_with(SCRIPT)], vec![passing()]);
    let out = author_loop(&stub, seed(), 2).await.unwrap();
    assert!(out.validation.ok);
    assert_eq!(out.attempts, 1);
    assert_eq!(stub.completions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn loop_repairs_until_pass() {
    let stub = Stub::new(
        vec![reply_with("bad"), reply_with(SCRIPT)],
        vec![failing("syntax"), passing()],
    );
    let out = author_loop(&stub, seed(), 2).await.unwrap();
    assert!(out.validation.ok);
    assert_eq!(out.attempts, 2);
    assert_eq!(out.script.trim(), SCRIPT);

    //
    // The second request carries the failed reply and the dry-run errors.
    //
    let messages = stub.last_messages.lock().unwrap();
    assert_eq!(messages.len(), 3);
    assert!(messages[2].text().contains("syntax broke"));
}

#[tokio::test]
async fn loop_terminates_after_max_repairs() {
    let stub = Stub::new(vec![reply_with("bad")], vec![failing("recon")]);
    let out = author_loop(&stub, seed(), 2).await.unwrap();
    assert!(!out.validation.ok);
    assert_eq!(out.attempts, 3);
    assert_eq!(stub.completions.load(Ordering::SeqCst), 3);

    let stub = Stub::new(vec![reply_with("bad")], vec![failing("recon")]);
    let out = author_loop(&stub, seed(), 0).await.unwrap();
    assert_eq!(out.attempts, 1);
    assert_eq!(stub.completions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn loop_errors_when_no_script_is_ever_returned() {
    let stub = Stub::new(vec!["I cannot help with that.".into()], vec![passing()]);
    let err = match author_loop(&stub, seed(), 1).await {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("did not return a Lua script"));
    assert_eq!(stub.completions.load(Ordering::SeqCst), 2);
    assert_eq!(stub.validations_run.load(Ordering::SeqCst), 0);
}

#[test]
fn extracts_longest_lua_block() {
    let text = "Snippet:\n```lua\nlocal x = 1\n```\nFull:\n```lua\nlocal helpers = require(\"praxis.helpers\")\nreturn { short_name = \"a\" }\n```\n";
    let code = extract_lua_block(text).unwrap();
    assert!(code.starts_with("local helpers"));
    assert!(code.ends_with("}\n"));
}

#[test]
fn extracts_untagged_connector_block_and_ignores_truncated() {
    let untagged = "```\nreturn { name = \"A\", short_name = \"a\" }\n```";
    assert!(extract_lua_block(untagged).is_some());
    assert!(extract_lua_block("```\nnot a connector\n```").is_none());
    assert!(extract_lua_block("```lua\nreturn {\n  name = \"cut off").is_none());
    assert!(extract_lua_block("no code here").is_none());
}

#[test]
fn provenance_round_trips() {
    let header = provenance_header(&Provenance {
        action: "generated",
        model: "m",
        node_id: "n",
        inputs: "exe=/x",
        extra: Some("extra"),
        validation: &passing(),
        attempts: 1,
    });
    let script = format!("{}{}", header, SCRIPT);
    assert!(script.starts_with(PROVENANCE_PREFIX));
    assert_eq!(strip_provenance(&script), SCRIPT);
    assert_eq!(strip_provenance(SCRIPT), SCRIPT);
}

#[test]
fn detect_failure_flags_errors_and_recon_regressions() {
    assert!(detect_failure(&failing("fingerprint"), None).is_some());
    assert!(detect_failure(&passing(), None).is_none());

    let mut empty = passing();
    empty.recon = Some(DryRunReconSummary::default());
    assert!(detect_failure(&empty, None).is_none());

    let mut stored = ReconResult::default();
    assert!(detect_failure(&empty, Some(&stored)).is_none());
    stored.config.project_paths.push("/p".into());
    let msg = detect_failure(&empty, Some(&stored)).unwrap();
    assert!(msg.contains("no data"));
}

#[test]
fn repaired_names_do_not_stack() {
    assert_eq!(repaired_base_name("crush"), "crush");
    assert_eq!(repaired_base_name("crush-repaired"), "crush");
    assert_eq!(repaired_base_name("crush-repaired-3"), "crush");
    assert_eq!(default_short_name("/home/u/.local/bin/Crush.exe"), "crush");
}

#[test]
fn guide_sections_and_helper_reference_are_extracted() {
    let doc = "# T\n## Lua Connector (Recommended)\nbody\n```\n## not a heading\n```\n## Next\nx\n";
    let section = prompt::markdown_section(doc, "## Lua Connector").unwrap();
    assert!(section.contains("not a heading"));
    assert!(!section.contains("## Next"));

    let reference = prompt::helper_api_reference(prompt_helpers());
    assert!(reference.contains("function M.run_standard_recon(ctx, config) ... end"));
    assert!(reference.contains("Standard recon orchestration"));
    assert!(!reference.contains("local result = M.new_recon_result()"));
}

fn prompt_helpers() -> &'static str {
    include_str!("../../../node/src/agent_connectors/lua/lib/helpers.lua")
}

fn script_info(id: &str, name: &str) -> LuaAgentScriptInfo {
    LuaAgentScriptInfo {
        id: id.into(),
        name: name.into(),
        script: SCRIPT.into(),
        disabled: false,
        is_builtin: false,
        version: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

#[test]
fn resolves_scripts_by_id_name_and_prefix() {
    let scripts = vec![
        script_info("aaaa1111-x", "gemini"),
        script_info("aaaa2222-y", "crush"),
    ];
    assert_eq!(resolve_script(&scripts, "gemini").unwrap().id, "aaaa1111-x");
    assert_eq!(resolve_script(&scripts, "aaaa2").unwrap().name, "crush");
    assert_eq!(resolve_script(&scripts, "CRUSH").unwrap().name, "crush");
    assert!(resolve_script(&scripts, "aaaa").is_err());
    assert!(resolve_script(&scripts, "nope").is_err());
}

#[tokio::test]
async fn drafts_are_saved_disabled_and_never_overwrite_builtins() {
    let path = std::env::temp_dir().join(format!("praxis_author_{}.db", uuid::Uuid::new_v4()));
    let db = Database::new(&DatabaseConfig::Sqlite { path: path.clone() })
        .await
        .unwrap();
    db.upsert_lua_agent_script(
        "builtin-id",
        "crush-generated",
        "builtin body",
        false,
        true,
        Some("v1"),
    )
    .await
    .unwrap();

    let (id, name) = save_draft(&db, "crush-generated", "draft body")
        .await
        .unwrap();
    assert_ne!(id, "builtin-id");
    assert_eq!(name, "crush-generated-2");
    let (_, name3) = save_draft(&db, "crush-generated", "draft body")
        .await
        .unwrap();
    assert_eq!(name3, "crush-generated-3");

    let scripts = db.list_lua_agent_scripts().await.unwrap();
    let builtin = scripts.iter().find(|s| s.id == "builtin-id").unwrap();
    assert_eq!(builtin.script, "builtin body");
    assert!(builtin.is_builtin && !builtin.disabled);
    let draft = scripts.iter().find(|s| s.id == id).unwrap();
    assert!(draft.disabled, "drafts must be saved disabled");
    assert!(!draft.is_builtin, "drafts must be user scripts");
    assert_eq!(draft.script, "draft body");

    //
    // Disabled drafts are never distributed to nodes.
    //
    let distributed = db.get_all_lua_scripts().await.unwrap();
    assert!(!distributed.iter().any(|s| s == "draft body"));

    drop(db);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
    }
}

#[test]
fn finds_referenced_dot_dirs() {
    let script = r#"
local recon_config = {
  home_dir = ".factory",
  home_configs = { { path = ".factory/settings.json" }, { path = ".config/x/y.json" } },
  sessions_relpath = ".factory/sessions",
}
local v = "1.2.3"
"#;
    assert_eq!(referenced_dot_dirs(script), vec!["factory", "config"]);
}

#[test]
fn repairs_must_keep_short_name() {
    let mut result = passing();
    enforce_short_name(&mut result, Some("x"));
    assert!(result.ok);
    enforce_short_name(&mut result, Some("droid"));
    assert!(!result.ok);
    assert_eq!(result.failed_stage.as_deref(), Some("manifest"));
    assert!(result.errors[0].contains("short_name changed"));
}
