use common::acp_ext::EXT_PRAXIS_READ_FILE;
use common::exposure::AgentModifier;
use futures_util::future::join_all;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::Duration;

use super::graph::{CachedModifiers, Inputs};
use super::rules::RuleSet;
use super::{ExposureDeps, MODIFIERS_CACHE_PREFIX};

//
// Opt-in modifier probe. Stored recon does not carry config file contents,
// so detecting YOLO / auto-approve settings means reading the agent's
// settings files on the node (the same `_praxis/read_file` path the recon
// overlay uses). Only online nodes are probed, every read has a short
// timeout and the whole pass is bounded, so a slow node cannot hold the
// report hostage. Results are cached per agent and reused by later reports.
//

const READ_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_BUDGET: Duration = Duration::from_secs(45);

struct Target {
    node_id: String,
    agent: String,
    /// (path, recon config type) of each settings file to read.
    files: Vec<(String, String)>,
}

//
// Outcome of one settings read. A transport failure (node unreachable,
// request timed out) ends probing for that agent; a file-level failure
// (missing, unreadable) only skips the file.
//

enum ReadError {
    Transport(String),
    File(String),
}

async fn read_config(
    deps: &ExposureDeps,
    node_id: &str,
    agent: &str,
    path: &str,
) -> Result<String, ReadError> {
    let params = json!({
        "agent_short_name": agent,
        "file_type": common::AgentFileType::Config,
        "path": path,
        "_meta": { "praxis": { "nodeId": node_id } },
    });
    let result = deps
        .acp_node_proxy
        .request_with_timeout(
            &deps.channel,
            node_id,
            EXT_PRAXIS_READ_FILE,
            params,
            READ_TIMEOUT,
        )
        .await
        .map_err(|e| ReadError::Transport(e.to_string()))?;
    match result.get("content").and_then(Value::as_str) {
        Some(content) => Ok(content.to_string()),
        None => Err(ReadError::File(
            result
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("no content returned")
                .to_string(),
        )),
    }
}

pub(super) fn parse_settings(path: &str, content: &str) -> Option<Value> {
    if path.to_lowercase().ends_with(".toml") {
        toml::from_str::<Value>(content).ok()
    } else {
        serde_json::from_str::<Value>(content).ok()
    }
}

async fn probe_agent(
    deps: &ExposureDeps,
    rules: &RuleSet,
    target: &Target,
) -> (Vec<AgentModifier>, Option<String>) {
    let mut modifiers = Vec::new();
    let mut failed = 0;
    let mut first_failure = None;
    for (path, config_type) in &target.files {
        let doc = match read_config(deps, &target.node_id, &target.agent, path).await {
            Ok(content) => parse_settings(path, &content)
                .ok_or_else(|| format!("{}: not valid JSON/TOML", path)),
            Err(ReadError::File(e)) => Err(format!("{}: {}", path, e)),
            Err(ReadError::Transport(e)) => {
                failed += 1;
                first_failure.get_or_insert(format!("node unreachable ({}); probe stopped", e));
                break;
            }
        };
        let doc = match doc {
            Ok(doc) => doc,
            Err(e) => {
                failed += 1;
                first_failure.get_or_insert(e);
                continue;
            }
        };

        //
        // The same allow-list entry commonly repeats across many project
        // settings files; keep the first occurrence of each (kind, detail).
        //
        for rule in rules.modifiers_for(&target.agent, config_type) {
            for m in rule.evaluate(&doc, path) {
                if !modifiers
                    .iter()
                    .any(|x: &AgentModifier| x.kind == m.kind && x.detail == m.detail)
                {
                    modifiers.push(m);
                }
            }
        }
    }
    let warning = first_failure.map(|first| {
        format!(
            "Probe {}@{}: {} of {} settings file(s) not used (first: {})",
            target.agent,
            common::short_id(&target.node_id),
            failed,
            target.files.len(),
            first
        )
    });
    (modifiers, warning)
}

pub(super) async fn run(
    deps: &ExposureDeps,
    rules: &RuleSet,
    online: &HashSet<String>,
    inputs: &mut Inputs,
) {
    let mut targets = Vec::new();
    let mut skipped = 0;
    for agent in &inputs.agents {
        if !online.contains(&agent.node_id) {
            skipped += 1;
            continue;
        }
        let files = agent
            .recon
            .config
            .items
            .iter()
            .filter(|item| {
                rules
                    .modifiers_for(&agent.agent_short_name, &item.config_type)
                    .next()
                    .is_some()
            })
            .map(|item| (item.path.clone(), item.config_type.clone()))
            .collect();
        targets.push(Target {
            node_id: agent.node_id.clone(),
            agent: agent.agent_short_name.clone(),
            files,
        });
    }
    if skipped > 0 {
        inputs.warnings.push(format!(
            "{} agent(s) on offline nodes were not probed; cached modifiers (if any) are shown.",
            skipped
        ));
    }

    let probes = targets
        .iter()
        .map(|target| probe_agent(deps, rules, target));
    let results = match tokio::time::timeout(PROBE_BUDGET, join_all(probes)).await {
        Ok(results) => results,
        Err(_) => {
            inputs.warnings.push(format!(
                "Config probe exceeded {}s and was abandoned; cached modifiers are shown.",
                PROBE_BUDGET.as_secs()
            ));
            return;
        }
    };

    let probed_at = chrono::Utc::now().to_rfc3339();
    for (target, (modifiers, warning)) in targets.iter().zip(results) {
        inputs.warnings.extend(warning);
        let cached = CachedModifiers {
            probed_at: probed_at.clone(),
            modifiers,
        };
        let key = format!(
            "{}{}:{}",
            MODIFIERS_CACHE_PREFIX, target.node_id, target.agent
        );
        if let Ok(json) = serde_json::to_string(&cached)
            && let Err(e) = deps.database.set_exposure_cache(&key, &json).await
        {
            common::log_warn!("exposure: failed to cache modifiers for {}: {}", key, e);
        }
        inputs
            .modifiers
            .insert((target.node_id.clone(), target.agent.clone()), cached);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_and_toml_settings() {
        let json = parse_settings("/h/.claude/settings.json", r#"{"a": {"b": 1}}"#).unwrap();
        assert_eq!(json["a"]["b"], 1);
        let toml =
            parse_settings("/h/.codex/config.toml", "approval_policy = \"never\"\n").unwrap();
        assert_eq!(toml["approval_policy"], "never");
        assert!(parse_settings("/x.json", "not json").is_none());
    }
}
