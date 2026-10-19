pub mod graph;
mod llm;
mod probe;
pub mod rules;

use anyhow::Result;
use common::NodeStatus;
use common::exposure::{CapabilityClass, ExposureOptions, ExposureReport};
use lapin::Channel;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::acp_node_proxy::AcpNodeProxy;
use crate::config::ServiceConfig;
use crate::database::Database;
use crate::state::NodeRegistry;
use graph::{AgentInput, CachedModifiers, Inputs, TrafficHost};
use rules::RuleSet;

//
// Agent exposure analysis ("BloodHound for agents"). Everything is computed
// on demand from stored recon, intercepted-traffic hosts and two caches
// (LLM verdicts and probed modifiers) — there is no persistent graph. The
// pure construction lives in `graph`, the rule engine in `rules`; this
// module gathers inputs and runs the optional probe / LLM passes.
//

const MODIFIERS_CACHE_PREFIX: &str = "modifiers:";
const LLM_CACHE_PREFIX: &str = "llm:";
const LLM_BUDGET: std::time::Duration = std::time::Duration::from_secs(150);

pub struct ExposureDeps {
    pub database: Arc<Database>,
    pub node_registry: Arc<NodeRegistry>,
    pub service_config: Arc<RwLock<ServiceConfig>>,
    pub acp_node_proxy: Arc<AcpNodeProxy>,
    pub channel: Channel,
}

pub async fn load_rules(database: &Database) -> Result<RuleSet, String> {
    let text = database
        .get_exposure_rules_text()
        .await
        .map_err(|e| format!("Failed to load exposure rules: {}", e))?;
    rules::parse(&text).map_err(|e| format!("Exposure rules are invalid: {}", e))
}

async fn load_inputs(database: &Database, registry: &NodeRegistry) -> Result<Inputs> {
    let mut inputs = Inputs::default();

    let stored = database.list_all_recon_results().await?;
    let with_recon: HashSet<(String, String)> = stored
        .iter()
        .map(|r| (r.node_id.clone(), r.agent_short_name.clone()))
        .collect();
    inputs.agents = stored
        .into_iter()
        .map(|r| AgentInput {
            node_id: r.node_id,
            agent_short_name: r.agent_short_name,
            performed_at: r.performed_at,
            is_semantic: r.is_semantic,
            recon: r.recon_result,
        })
        .collect();

    let mut missing = 0;
    for node in registry.list().await {
        if let Some(update) = &node.last_update {
            missing += update
                .discovered_agents
                .iter()
                .filter(|a| a.available)
                .filter(|a| !with_recon.contains(&(node.id.clone(), a.short_name.clone())))
                .count();
        }
        inputs.machine_names.insert(node.id, node.machine_name);
    }
    if missing > 0 {
        inputs.warnings.push(format!(
            "{} discovered agent(s) have no stored recon and are not included; run recon on them first.",
            missing
        ));
    }

    inputs.traffic_hosts = database
        .list_traffic_hosts()
        .await?
        .into_iter()
        .map(|(node_id, agent_label, host)| TrafficHost {
            node_id,
            agent_label,
            host,
        })
        .collect();

    for (key, value) in database.list_exposure_cache(MODIFIERS_CACHE_PREFIX).await? {
        let Some((node_id, agent)) = key
            .strip_prefix(MODIFIERS_CACHE_PREFIX)
            .and_then(|rest| rest.rsplit_once(':'))
        else {
            continue;
        };
        if let Ok(cached) = serde_json::from_str::<CachedModifiers>(&value) {
            inputs
                .modifiers
                .insert((node_id.to_string(), agent.to_string()), cached);
        }
    }

    for (key, value) in database.list_exposure_cache(LLM_CACHE_PREFIX).await? {
        if let Ok(classes) = serde_json::from_str::<Vec<String>>(&value) {
            let classes = classes
                .iter()
                .filter_map(|c| CapabilityClass::parse(c))
                .collect();
            inputs.llm_verdicts.insert(key, classes);
        }
    }

    Ok(inputs)
}

fn online_nodes(nodes: &[crate::state::registries::RegisteredNode]) -> HashSet<String> {
    let now = chrono::Utc::now();
    nodes
        .iter()
        .filter(|n| {
            NodeStatus::from_age_seconds((now - n.last_update_received).num_seconds())
                == NodeStatus::Online
        })
        .map(|n| n.id.clone())
        .collect()
}

//
// Report from stored data and caches only: no node I/O and no LLM calls.
// Used by the ExposureLogs log-query table.
//

pub async fn compute_cached(
    database: &Database,
    registry: &NodeRegistry,
) -> Result<ExposureReport, String> {
    let rules = load_rules(database).await?;
    let inputs = load_inputs(database, registry)
        .await
        .map_err(|e| format!("Failed to load exposure inputs: {}", e))?;
    Ok(graph::build_report(&rules, &inputs).0)
}

pub async fn generate(
    deps: &ExposureDeps,
    options: &ExposureOptions,
) -> Result<ExposureReport, String> {
    let rules = load_rules(&deps.database).await?;
    let mut inputs = load_inputs(&deps.database, &deps.node_registry)
        .await
        .map_err(|e| format!("Failed to load exposure inputs: {}", e))?;

    if options.probe_configs {
        let online = online_nodes(&deps.node_registry.list().await);
        probe::run(deps, &rules, &online, &mut inputs).await;
    }

    let (mut report, candidates) = graph::build_report(&rules, &inputs);
    if options.llm && !candidates.is_empty() {
        //
        // Bound the LLM pass so a slow provider cannot hold the reply
        // indefinitely. Verdicts are cached batch by batch, so work done
        // before the budget runs out is kept for the next run.
        //
        match tokio::time::timeout(LLM_BUDGET, llm::refine(deps, &candidates)).await {
            Ok(Ok(outcome)) => {
                inputs.llm_verdicts.extend(outcome.verdicts);
                inputs.warnings.extend(outcome.warnings);
                report = graph::build_report(&rules, &inputs).0;
            }
            Ok(Err(e)) => report.warnings.push(e),
            Err(_) => report.warnings.push(format!(
                "LLM classification exceeded {}s; finished batches are cached, run again to continue.",
                LLM_BUDGET.as_secs()
            )),
        }
    }
    report.probed = options.probe_configs;
    Ok(report)
}
