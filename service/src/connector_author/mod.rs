mod prompt;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use common::acp_ext::{EXT_PRAXIS_CONNECTOR_DRY_RUN, EXT_PRAXIS_CONNECTOR_PROBE};
use common::ai::{AiClient, Message, Provider, Role, build_message, create_ai_client};
use common::connector_author::{
    ConnectorAuthorOutcome, ConnectorAuthorRequest, ConnectorDryRun, ConnectorProbe,
    clamp_max_repairs,
};
use common::{ClientDirectMessage, LuaAgentScriptInfo, ReconResult};
use lapin::Channel;
use serde_json::json;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::acp_node_proxy::AcpNodeProxy;
use crate::config::ServiceConfig;
use crate::database::Database;
use crate::messaging::send_to_client;

//
// LLM-assisted connector authoring. Drives three workflows on behalf of a
// client:
//
// - generate: probe an unsupported agent on a node, ask the configured LLM
//   for a draft Lua connector, validate it with a dry run on that node and
//   feed failures back for up to N repair attempts;
// - repair: diagnose an existing connector with a dry run (plus a recon
//   regression check against stored recon), then ask the LLM for a patched
//   script, validated the same way, and return a diff;
// - validate: dry-run a saved script (or raw script text) on a node.
//
// Generated and repaired scripts are only ever saved as new, disabled user
// scripts with a provenance header. Nothing here overwrites an existing
// script, touches built-ins or enables anything: the operator reviews the
// result and enables it explicitly.
//

const MAX_TOKENS: u32 = 8192;
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const DRY_RUN_TIMEOUT: Duration = Duration::from_secs(120);
const PROVENANCE_PREFIX: &str = "-- praxis-provenance:";

#[derive(Clone)]
pub struct AuthorDeps {
    pub database: Arc<Database>,
    pub service_config: Arc<RwLock<ServiceConfig>>,
    pub acp_node_proxy: Arc<AcpNodeProxy>,
    pub publish_channel: Channel,
    pub client_channel: Channel,
}

pub struct ConnectorAuthorManager {
    active: Arc<RwLock<HashMap<String, CancellationToken>>>,
}

impl ConnectorAuthorManager {
    pub fn new() -> Self {
        Self {
            active: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    //
    // Start a workflow in the background. Returns immediately so the
    // dispatch loop stays free to process the matching cancel. Exactly one
    // ConnectorAuthorResult is sent for the request.
    //

    pub async fn start(
        &self,
        deps: AuthorDeps,
        client_id: String,
        request_id: String,
        request: ConnectorAuthorRequest,
    ) {
        let token = CancellationToken::new();
        self.active
            .write()
            .await
            .insert(request_id.clone(), token.clone());
        let active = self.active.clone();

        tokio::spawn(async move {
            let kind = request.label().to_string();
            let progress = Progress {
                channel: deps.client_channel.clone(),
                client_id: client_id.clone(),
                request_id: request_id.clone(),
            };
            common::log_info!(
                "Connector author {} request {} started for node {}",
                kind,
                common::short_id(&request_id),
                common::short_id(request.node_id())
            );

            //
            // Dropping the workflow future on cancel aborts any in-flight
            // LLM HTTP request and stops waiting on the node.
            //

            let outcome = tokio::select! {
                result = run(&deps, request, &progress) => result.unwrap_or_else(|e| failed(&kind, e.to_string())),
                _ = token.cancelled() => failed(&kind, "Cancelled".to_string()),
            };

            common::log_info!(
                "Connector author {} request {} finished: ok={} {}",
                kind,
                common::short_id(&request_id),
                outcome.ok,
                common::truncate_str(&outcome.message, 200)
            );
            let _ = send_to_client(
                &deps.client_channel,
                &client_id,
                ClientDirectMessage::ConnectorAuthorResult {
                    request_id: request_id.clone(),
                    outcome,
                },
            )
            .await;
            active.write().await.remove(&request_id);
        });
    }

    pub async fn cancel(&self, request_id: &str) {
        if let Some(token) = self.active.write().await.remove(request_id) {
            token.cancel();
        }
    }
}

fn failed(kind: &str, message: String) -> ConnectorAuthorOutcome {
    ConnectorAuthorOutcome {
        kind: kind.to_string(),
        ok: false,
        message,
        ..Default::default()
    }
}

struct Progress {
    channel: Channel,
    client_id: String,
    request_id: String,
}

impl Progress {
    async fn send(&self, message: impl Into<String>) {
        let _ = send_to_client(
            &self.channel,
            &self.client_id,
            ClientDirectMessage::ConnectorAuthorProgress {
                request_id: self.request_id.clone(),
                message: message.into(),
            },
        )
        .await;
    }
}

async fn run(
    deps: &AuthorDeps,
    request: ConnectorAuthorRequest,
    progress: &Progress,
) -> Result<ConnectorAuthorOutcome> {
    match request {
        ConnectorAuthorRequest::Generate {
            node_id,
            executable,
            max_repairs,
        } => {
            generate(
                deps,
                &node_id,
                &executable,
                clamp_max_repairs(max_repairs),
                progress,
            )
            .await
        }
        ConnectorAuthorRequest::Repair {
            node_id,
            script,
            error,
            executable,
            max_repairs,
        } => {
            repair(
                deps,
                &node_id,
                &script,
                error,
                executable,
                clamp_max_repairs(max_repairs),
                progress,
            )
            .await
        }
        ConnectorAuthorRequest::Validate {
            node_id,
            script,
            script_content,
        } => validate(deps, &node_id, script, script_content, progress).await,
    }
}

// ---------------------------------------------------------------------------
// Workflows
// ---------------------------------------------------------------------------

async fn generate(
    deps: &AuthorDeps,
    node_id: &str,
    executable: &str,
    max_repairs: u32,
    progress: &Progress,
) -> Result<ConnectorAuthorOutcome> {
    let executable = executable.trim();
    if executable.is_empty() {
        return Err(anyhow!("An executable name or path is required"));
    }
    let model = resolve_model(&deps.service_config).await?;

    progress
        .send(format!(
            "Probing '{}' on node {}…",
            executable,
            common::short_id(node_id)
        ))
        .await;
    let probe = node_probe(deps, node_id, executable, &[]).await?;
    if probe.executable.is_none() {
        return Err(anyhow!(
            "Executable '{}' was not found on node {}. Pass a full path if it is installed outside PATH and the usual user bin directories.",
            executable,
            common::short_id(node_id)
        ));
    }
    progress.send(format!("Probe: {}", probe.summary())).await;

    let messages = vec![
        build_message(Role::System, prompt::system_prompt()),
        build_message(Role::User, prompt::generate_prompt(&probe)),
    ];
    let backend = LiveBackend {
        deps,
        node_id,
        model: &model,
        progress,
        expected_short_name: None,
    };
    let authored = author_loop(&backend, messages, max_repairs).await?;

    let short_name = authored
        .validation
        .short_name
        .clone()
        .unwrap_or_else(|| default_short_name(executable));
    let header = provenance_header(&Provenance {
        action: "generated",
        model: &model.display,
        node_id,
        inputs: &probe.summary(),
        extra: None,
        validation: &authored.validation,
        attempts: authored.attempts,
    });
    let script = format!("{}{}", header, authored.script);
    let (script_id, script_name) = save_draft(
        &deps.database,
        &format!("{}-generated", short_name),
        &script,
    )
    .await?;

    Ok(ConnectorAuthorOutcome {
        kind: "generate".into(),
        ok: authored.validation.ok,
        message: outcome_message("Draft connector", &script_name, &authored),
        script_id: Some(script_id),
        script_name: Some(script_name),
        script: Some(script),
        validation: Some(authored.validation),
        attempts: authored.attempts,
        model: Some(model.display.clone()),
        ..Default::default()
    })
}

async fn repair(
    deps: &AuthorDeps,
    node_id: &str,
    script_ref: &str,
    operator_error: Option<String>,
    executable: Option<String>,
    max_repairs: u32,
    progress: &Progress,
) -> Result<ConnectorAuthorOutcome> {
    let scripts = deps.database.list_lua_agent_scripts().await?;
    let original = resolve_script(&scripts, script_ref)?.clone();
    let model = resolve_model(&deps.service_config).await?;

    //
    // Diagnose first. A script that is enabled already runs on nodes, so it
    // is dry-run without the sandbox (sandboxing would itself cause
    // failures for connectors that legitimately run e.g. PowerShell during
    // fingerprint); disabled scripts are unreviewed and stay sandboxed.
    //

    progress
        .send(format!(
            "Diagnosing '{}' on node {}…",
            original.name,
            common::short_id(node_id)
        ))
        .await;
    let current = node_dry_run(deps, node_id, &original.script, original.disabled).await?;
    let stored = match &current.short_name {
        Some(short) => deps
            .database
            .get_recon_result(node_id, short)
            .await
            .ok()
            .flatten()
            .map(|s| s.recon_result),
        None => None,
    };
    let detected = detect_failure(&current, stored.as_ref());
    let failure = match (operator_error.filter(|e| !e.trim().is_empty()), detected) {
        (Some(op), Some(det)) => format!("{}\n\nDry run on the node:\n{}", op, det),
        (Some(op), None) => format!("{}\n\n(The dry run on the node passed.)", op),
        (None, Some(det)) => det,
        (None, None) => {
            return Ok(ConnectorAuthorOutcome {
                kind: "repair".into(),
                ok: true,
                message: format!(
                    "No failure detected for '{}' on node {}: the dry run passed and recon returned data. Describe the problem with an error message to request a repair anyway.",
                    original.name,
                    common::short_id(node_id)
                ),
                original_script_id: Some(original.id.clone()),
                original_script_name: Some(original.name.clone()),
                validation: Some(current),
                ..Default::default()
            });
        }
    };
    progress
        .send(format!(
            "Failure: {}",
            failure.lines().next().unwrap_or_default()
        ))
        .await;

    let exe = executable
        .filter(|e| !e.trim().is_empty())
        .or_else(|| {
            current
                .fingerprint
                .as_ref()
                .and_then(|f| f.process_path.clone())
        })
        .or_else(|| current.short_name.clone())
        .unwrap_or_else(|| original.name.clone());
    progress
        .send(format!(
            "Probing '{}' on node {}…",
            exe,
            common::short_id(node_id)
        ))
        .await;
    let probe = node_probe(deps, node_id, &exe, &referenced_dot_dirs(&original.script)).await?;
    progress.send(format!("Probe: {}", probe.summary())).await;

    let original_body = strip_provenance(&original.script);
    let messages = vec![
        build_message(Role::System, prompt::system_prompt()),
        build_message(
            Role::User,
            prompt::repair_prompt(original_body, &failure, &probe),
        ),
    ];
    let backend = LiveBackend {
        deps,
        node_id,
        model: &model,
        progress,
        expected_short_name: current.short_name.clone(),
    };
    let authored = author_loop(&backend, messages, max_repairs).await?;

    let diff = unified_diff(original_body, &authored.script);
    let header = provenance_header(&Provenance {
        action: "repaired",
        model: &model.display,
        node_id,
        inputs: &probe.summary(),
        extra: Some(&format!(
            "repair of '{}' ({}); failure: {}",
            original.name,
            common::short_id(&original.id),
            failure.lines().next().unwrap_or_default()
        )),
        validation: &authored.validation,
        attempts: authored.attempts,
    });
    let script = format!("{}{}", header, authored.script);
    let (script_id, script_name) = save_draft(
        &deps.database,
        &format!("{}-repaired", repaired_base_name(&original.name)),
        &script,
    )
    .await?;

    Ok(ConnectorAuthorOutcome {
        kind: "repair".into(),
        ok: authored.validation.ok,
        message: outcome_message("Proposed repair", &script_name, &authored),
        script_id: Some(script_id),
        script_name: Some(script_name),
        script: Some(script),
        original_script_id: Some(original.id.clone()),
        original_script_name: Some(original.name.clone()),
        diff: Some(diff),
        validation: Some(authored.validation),
        attempts: authored.attempts,
        model: Some(model.display.clone()),
    })
}

async fn validate(
    deps: &AuthorDeps,
    node_id: &str,
    script_ref: Option<String>,
    script_content: Option<String>,
    progress: &Progress,
) -> Result<ConnectorAuthorOutcome> {
    let (script, sandbox, info) = match (script_content, script_ref) {
        (Some(content), _) if !content.trim().is_empty() => (content, true, None),
        (_, Some(reference)) => {
            let scripts = deps.database.list_lua_agent_scripts().await?;
            let info = resolve_script(&scripts, &reference)?.clone();
            (info.script.clone(), info.disabled, Some(info))
        }
        _ => {
            return Err(anyhow!(
                "Provide a script reference or script content to validate"
            ));
        }
    };

    progress
        .send(format!(
            "Dry run{} on node {}…",
            if sandbox { " (sandboxed)" } else { "" },
            common::short_id(node_id)
        ))
        .await;
    let validation = node_dry_run(deps, node_id, &script, sandbox).await?;
    Ok(ConnectorAuthorOutcome {
        kind: "validate".into(),
        ok: validation.ok,
        message: validation.report(),
        script_id: info.as_ref().map(|i| i.id.clone()),
        script_name: info.as_ref().map(|i| i.name.clone()),
        validation: Some(validation),
        attempts: 1,
        ..Default::default()
    })
}

fn outcome_message(label: &str, script_name: &str, authored: &Authored) -> String {
    if authored.validation.ok {
        format!(
            "{} saved as disabled user script '{}' after {} attempt(s); the dry run passed. Review it, then enable it in Settings → Agents.",
            label, script_name, authored.attempts
        )
    } else {
        format!(
            "{} saved as disabled user script '{}', but the dry run still fails after {} attempt(s) ({}). Review and fix it before enabling.",
            label,
            script_name,
            authored.attempts,
            authored
                .validation
                .errors
                .first()
                .map(|e| common::truncate_str(e, 160).to_string())
                .unwrap_or_default()
        )
    }
}

// ---------------------------------------------------------------------------
// Failure detection
// ---------------------------------------------------------------------------

//
// Decide whether a dry run of an existing connector shows it is broken.
// Returns a description for the LLM, or None when the connector looks
// healthy. Besides outright fingerprint/recon errors this flags a recon
// regression: recon now returns nothing where the stored recon for this
// node and agent previously had data, which is the usual symptom of an
// agent update moving its files.
//
// Hook point: an agent-version-change event can dry-run the affected
// connector on the node, call this with the stored recon, and offer a
// repair when it returns Some.
//

pub(crate) fn detect_failure(
    dry_run: &ConnectorDryRun,
    stored: Option<&ReconResult>,
) -> Option<String> {
    if !dry_run.ok {
        return Some(dry_run.report());
    }
    let now_empty = dry_run.recon.as_ref().is_some_and(|r| r.is_empty());
    let previously = stored.map(|r| {
        r.config.items.len()
            + r.config.project_paths.len()
            + r.tools.mcp_servers.len()
            + r.tools.skills.len()
            + r.sessions.items.len()
    });
    match previously {
        Some(count) if now_empty && count > 0 => Some(format!(
            "Recon now returns no data, but the last stored recon for this agent on this node had {} item(s) (config items, project paths, MCP servers, skills and sessions). The agent was probably updated and moved or renamed its files.\n\n{}",
            count,
            dry_run.report()
        )),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Authoring loop
// ---------------------------------------------------------------------------

//
// The two round trips the authoring loop needs. Split out so the loop's
// termination and feedback logic can be tested with a stub LLM and node.
//

#[async_trait]
trait AuthorBackend: Send + Sync {
    async fn complete(&self, messages: &[Message]) -> Result<String>;
    async fn validate(&self, script: &str) -> Result<ConnectorDryRun>;
    async fn progress(&self, _message: String) {}
}

struct Authored {
    script: String,
    validation: ConnectorDryRun,
    attempts: u32,
}

//
// Ask for a script, dry-run it, and feed failures back until it passes or
// `max_repairs` follow-up attempts are used up (so at most max_repairs + 1
// LLM calls). Returns the last script that was produced, passing or not;
// errors only when no script could be extracted at all or a round trip
// itself failed.
//

async fn author_loop(
    backend: &dyn AuthorBackend,
    mut messages: Vec<Message>,
    max_repairs: u32,
) -> Result<Authored> {
    let mut last: Option<(String, ConnectorDryRun)> = None;
    let mut attempts = 0u32;

    for attempt in 0..=max_repairs {
        attempts = attempt + 1;
        backend
            .progress(if attempt == 0 {
                "Asking the model for a connector…".to_string()
            } else {
                format!("Repair attempt {} of {}…", attempt, max_repairs)
            })
            .await;
        let reply = backend.complete(&messages).await?;
        messages.push(build_message(Role::Assistant, reply.clone()));

        let Some(script) = extract_lua_block(&reply) else {
            backend
                .progress("The reply did not contain a complete ```lua block".to_string())
                .await;
            messages.push(build_message(
                Role::User,
                "Your reply did not contain a complete fenced ```lua code block. Reply with the complete connector script in one ```lua block.".to_string(),
            ));
            continue;
        };

        backend
            .progress("Validating on the node…".to_string())
            .await;
        let validation = backend.validate(&script).await?;
        backend
            .progress(validation.report().lines().next().unwrap_or("").to_string())
            .await;
        let passed = validation.ok;
        if !passed {
            messages.push(build_message(
                Role::User,
                prompt::feedback_prompt(&validation),
            ));
        }
        last = Some((script, validation));
        if passed {
            break;
        }
    }

    let (script, validation) = last.ok_or_else(|| {
        anyhow!(
            "The model did not return a Lua script after {} attempt(s)",
            attempts
        )
    })?;
    Ok(Authored {
        script,
        validation,
        attempts,
    })
}

struct ResolvedModel {
    client: AiClient,
    model: String,
    display: String,
}

struct LiveBackend<'a> {
    deps: &'a AuthorDeps,
    node_id: &'a str,
    model: &'a ResolvedModel,
    progress: &'a Progress,
    //
    // Repairs must keep the connector's short_name: intercept targets,
    // stored recon and sessions are keyed by it.
    //
    expected_short_name: Option<String>,
}

#[async_trait]
impl AuthorBackend for LiveBackend<'_> {
    async fn complete(&self, messages: &[Message]) -> Result<String> {
        common::ai::execute_chat_completion(
            &self.model.client,
            self.model.model.clone(),
            messages.to_vec(),
            Some(MAX_TOKENS),
        )
        .await
    }

    async fn validate(&self, script: &str) -> Result<ConnectorDryRun> {
        let mut result = node_dry_run(self.deps, self.node_id, script, true).await?;
        enforce_short_name(&mut result, self.expected_short_name.as_deref());
        Ok(result)
    }

    async fn progress(&self, message: String) {
        self.progress.send(message).await;
    }
}

pub(crate) fn enforce_short_name(result: &mut ConnectorDryRun, expected: Option<&str>) {
    if let (Some(expected), Some(actual)) = (expected, result.short_name.as_deref())
        && expected != actual
    {
        result.errors.push(format!(
            "short_name changed from '{}' to '{}'; a repair must keep the original short_name",
            expected, actual
        ));
        result.ok = false;
        result
            .failed_stage
            .get_or_insert_with(|| "manifest".to_string());
    }
}

async fn resolve_model(service_config: &Arc<RwLock<ServiceConfig>>) -> Result<ResolvedModel> {
    let config = service_config.read().await;
    let def = config
        .get_connector_author_model_def()
        .or_else(|| config.get_orchestrator_model_def())
        .ok_or_else(|| {
            anyhow!("No model configured for connector authoring. Go to Settings > LLM and assign a model to Connector Author (or the Orchestrator).")
        })?;
    let provider = Provider::from_str(&def.provider)
        .ok_or_else(|| anyhow!("Unsupported provider '{}'", def.provider))?;
    if def.api_key.is_empty() && !provider.api_key_optional() {
        return Err(anyhow!(
            "No API key configured for model '{}'. Go to Settings > LLM to configure it.",
            def.name
        ));
    }
    let client = create_ai_client(provider, def.api_key.clone(), def.base_url.as_deref())?;
    Ok(ResolvedModel {
        client,
        model: def.model.clone(),
        display: def.name.clone(),
    })
}

// ---------------------------------------------------------------------------
// Node round trips
// ---------------------------------------------------------------------------

async fn node_request<T: serde::de::DeserializeOwned>(
    deps: &AuthorDeps,
    node_id: &str,
    method: &str,
    mut params: serde_json::Value,
    timeout: Duration,
) -> Result<T> {
    params["_meta"] = json!({ "praxis": { "nodeId": node_id } });
    let result = deps
        .acp_node_proxy
        .request_with_timeout(&deps.publish_channel, node_id, method, params, timeout)
        .await?;
    if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!(err.to_string()));
    }
    serde_json::from_value(result).map_err(|e| {
        anyhow!(
            "Unexpected {} response from node (is it up to date?): {}",
            method,
            e
        )
    })
}

async fn node_probe(
    deps: &AuthorDeps,
    node_id: &str,
    executable: &str,
    extra_names: &[String],
) -> Result<ConnectorProbe> {
    node_request(
        deps,
        node_id,
        EXT_PRAXIS_CONNECTOR_PROBE,
        json!({ "executable": executable, "extra_names": extra_names }),
        PROBE_TIMEOUT,
    )
    .await
}

//
// Dot-directories an existing connector references (e.g. ".factory" in
// `home_dir = ".factory"` or ".factory/settings.json"), so a repair probe
// lists the directories the connector actually depends on even when they
// are not named after the executable.
//

pub(crate) fn referenced_dot_dirs(script: &str) -> Vec<String> {
    static DOT_DIR: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"["/]\.([A-Za-z0-9][A-Za-z0-9_-]{0,39})["/]"#).unwrap()
    });
    let mut names: Vec<String> = Vec::new();
    for caps in DOT_DIR.captures_iter(script) {
        let name = caps[1].to_string();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

async fn node_dry_run(
    deps: &AuthorDeps,
    node_id: &str,
    script: &str,
    sandbox: bool,
) -> Result<ConnectorDryRun> {
    node_request(
        deps,
        node_id,
        EXT_PRAXIS_CONNECTOR_DRY_RUN,
        json!({ "script": script, "sandbox": sandbox }),
        DRY_RUN_TIMEOUT,
    )
    .await
}

// ---------------------------------------------------------------------------
// Scripts: lookup, provenance, saving
// ---------------------------------------------------------------------------

//
// Resolve a script reference: exact id, exact name, unique id prefix, then
// unique case-insensitive name.
//

pub(crate) fn resolve_script<'a>(
    scripts: &'a [LuaAgentScriptInfo],
    reference: &str,
) -> Result<&'a LuaAgentScriptInfo> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(anyhow!("A script id or name is required"));
    }
    if let Some(s) = scripts
        .iter()
        .find(|s| s.id == reference || s.name == reference)
    {
        return Ok(s);
    }
    let lower = reference.to_lowercase();
    let by_prefix: Vec<_> = scripts
        .iter()
        .filter(|s| reference.len() >= 4 && s.id.starts_with(reference))
        .collect();
    let candidates = if by_prefix.is_empty() {
        scripts
            .iter()
            .filter(|s| s.name.to_lowercase() == lower)
            .collect()
    } else {
        by_prefix
    };
    match candidates.as_slice() {
        [only] => Ok(only),
        [] => Err(anyhow!("No agent script matches '{}'", reference)),
        many => Err(anyhow!(
            "'{}' is ambiguous; matches: {}",
            reference,
            many.iter()
                .map(|s| format!("{} ({})", s.name, common::short_id(&s.id)))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn default_short_name(executable: &str) -> String {
    let base = executable
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(executable)
        .to_lowercase();
    let base = base
        .trim_end_matches(".exe")
        .trim_end_matches(".cmd")
        .to_string();
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "connector".to_string()
    } else {
        cleaned
    }
}

fn repaired_base_name(name: &str) -> &str {
    let mut base = name;
    if let Some((head, tail)) = base.rsplit_once('-')
        && !tail.is_empty()
        && tail.chars().all(|c| c.is_ascii_digit())
    {
        base = head;
    }
    base.strip_suffix("-repaired").unwrap_or(base)
}

struct Provenance<'a> {
    action: &'a str,
    model: &'a str,
    node_id: &'a str,
    inputs: &'a str,
    extra: Option<&'a str>,
    validation: &'a ConnectorDryRun,
    attempts: u32,
}

fn provenance_header(p: &Provenance) -> String {
    let validation = if p.validation.ok {
        "passed".to_string()
    } else {
        format!(
            "FAILED at {}",
            p.validation
                .failed_stage
                .as_deref()
                .unwrap_or("unknown stage")
        )
    };
    let mut lines = vec![
        format!(
            "{} {} by the Praxis connector author; saved disabled for review",
            PROVENANCE_PREFIX, p.action
        ),
        format!("{} model: {}", PROVENANCE_PREFIX, p.model),
        format!(
            "{} created: {}",
            PROVENANCE_PREFIX,
            chrono::Utc::now().to_rfc3339()
        ),
        format!("{} node: {}", PROVENANCE_PREFIX, p.node_id),
        format!("{} inputs: {}", PROVENANCE_PREFIX, p.inputs),
    ];
    if let Some(extra) = p.extra {
        lines.push(format!("{} {}", PROVENANCE_PREFIX, extra));
    }
    lines.push(format!(
        "{} dry run: {} after {} attempt(s)",
        PROVENANCE_PREFIX, validation, p.attempts
    ));
    format!("{}\n\n", lines.join("\n").replace(['\r'], ""))
}

//
// Remove a provenance header written by provenance_header so repairs of a
// generated script neither feed it to the LLM nor accumulate headers.
//

pub(crate) fn strip_provenance(script: &str) -> &str {
    let mut rest = script;
    while let Some(line_end) = rest.find('\n') {
        if rest.starts_with(PROVENANCE_PREFIX) {
            rest = &rest[line_end + 1..];
        } else {
            break;
        }
    }
    if rest.len() != script.len() {
        rest.strip_prefix('\n').unwrap_or(rest)
    } else {
        script
    }
}

//
// Save a script as a brand-new disabled, non-built-in row. A fresh id is
// always used, so no existing script (built-in or user) is ever
// overwritten, and the name gets a numeric suffix when taken.
//

pub(crate) async fn save_draft(
    db: &Database,
    base_name: &str,
    script: &str,
) -> Result<(String, String)> {
    let existing: Vec<String> = db
        .list_lua_agent_scripts()
        .await?
        .into_iter()
        .map(|s| s.name)
        .collect();
    let mut name = base_name.to_string();
    let mut n = 2;
    while existing.contains(&name) {
        name = format!("{}-{}", base_name, n);
        n += 1;
    }
    let id = uuid::Uuid::new_v4().to_string();
    db.upsert_lua_agent_script(&id, &name, script, true, false, None)
        .await?;
    Ok((id, name))
}

pub(crate) fn unified_diff(old: &str, new: &str) -> String {
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header("current", "proposed")
        .to_string()
}

//
// Pull the connector out of an LLM reply: the longest complete ```lua fenced
// block, else the longest complete fenced block that looks like a connector.
// An unterminated fence (e.g. output truncated at max_tokens) is ignored.
//

pub(crate) fn extract_lua_block(text: &str) -> Option<String> {
    let mut lua_blocks = Vec::new();
    let mut other_blocks = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let Some(info) = trimmed.strip_prefix("```") else {
            continue;
        };
        let lang = info.trim().to_lowercase();
        let mut body = Vec::new();
        let mut closed = false;
        for inner in lines.by_ref() {
            if inner.trim_start().starts_with("```") && inner.trim() == "```" {
                closed = true;
                break;
            }
            body.push(inner);
        }
        if !closed {
            break;
        }
        let code = body.join("\n");
        if lang == "lua" {
            lua_blocks.push(code);
        } else if code.contains("short_name") && code.contains("return") {
            other_blocks.push(code);
        }
    }
    let pick = |blocks: Vec<String>| blocks.into_iter().max_by_key(|b| b.len());
    pick(lua_blocks)
        .or_else(|| pick(other_blocks))
        .map(|code| format!("{}\n", code.trim_end()))
        .filter(|code| !code.trim().is_empty())
}

#[cfg(test)]
mod tests;
