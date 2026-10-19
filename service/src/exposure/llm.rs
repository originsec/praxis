use common::exposure::CapabilityClass;
use semantic_parser::{ParserConfig, Provider, SemanticParser};
use serde_json::{Value, json};
use std::collections::HashMap;

use super::ExposureDeps;
use super::graph::LlmCandidate;

//
// Optional Semantic Parser refinement for capabilities no rule matched.
// Verdicts (including "no class") are cached in exposure_cache keyed by the
// capability identity, so each capability is sent to the LLM at most once
// and later reports reuse the verdict without any LLM configured. Work per
// run is capped; running again continues with the remainder.
//

const BATCH_SIZE: usize = 30;
const MAX_PER_RUN: usize = 120;

const INSTRUCTIONS: &str = "Each TEXT entry is a capability (tool, MCP server or skill) available to an AI coding agent. \
Classify each one into zero or more of these classes from the 'lethal trifecta':\n\
- private_data: it can read the user's or organisation's private data (local files, mail, chat, databases, documents, secrets).\n\
- untrusted_content: it brings content an outside party could control into the agent's context (web pages, search results, issues, tickets, emails, messages, third-party files).\n\
- exfiltration: it can send data outside the machine (network requests, messages, posting, pushing, uploading, remote APIs that receive arguments).\n\
Only assign a class when the name/description clearly indicates it; return an empty list otherwise. \
Return one item per entry, echoing its id.";

const SCHEMA: &str = r#"{"items": [{"id": "string", "classes": ["private_data | untrusted_content | exfiltration"]}]}"#;

pub(super) struct RefineOutcome {
    pub verdicts: HashMap<String, Vec<CapabilityClass>>,
    pub warnings: Vec<String>,
}

pub(super) async fn refine(
    deps: &ExposureDeps,
    candidates: &[LlmCandidate],
) -> Result<RefineOutcome, String> {
    let model_def = deps
        .service_config
        .read()
        .await
        .get_semantic_parser_model_def()
        .ok_or_else(|| {
            "LLM refinement skipped: no Semantic Parser LLM configured (Settings > LLM Providers)."
                .to_string()
        })?;
    let provider = Provider::from_str(&model_def.provider)
        .ok_or_else(|| format!("Invalid Semantic Parser provider: {}", model_def.provider))?;
    let parser = SemanticParser::new(ParserConfig {
        provider,
        api_key: model_def.api_key,
        model: model_def.model,
        max_retries: 2,
        max_tokens: Some(4096),
        base_url: model_def.base_url,
    })
    .map_err(|e| format!("Failed to create semantic parser: {}", e))?;

    let mut outcome = RefineOutcome {
        verdicts: HashMap::new(),
        warnings: Vec::new(),
    };
    let take = candidates.len().min(MAX_PER_RUN);
    if candidates.len() > take {
        outcome.warnings.push(format!(
            "LLM classified {} of {} unclassified capabilities; run again to continue.",
            take,
            candidates.len()
        ));
    }

    for batch in candidates[..take].chunks(BATCH_SIZE) {
        let entries: Vec<Value> = batch
            .iter()
            .enumerate()
            .map(|(i, c)| {
                json!({
                    "id": i.to_string(),
                    "kind": c.kind.as_str(),
                    "server": c.server,
                    "name": c.name,
                    "description": common::truncate_str(&c.description, 500),
                })
            })
            .collect();
        let text = serde_json::to_string_pretty(&entries).unwrap_or_default();
        let parsed = match parser.parse(&text, INSTRUCTIONS, SCHEMA).await {
            Ok(json) => json,
            Err(e) => {
                outcome
                    .warnings
                    .push(format!("LLM classification batch failed: {}", e));
                continue;
            }
        };
        for (index, classes) in parse_verdicts(&parsed) {
            let Some(candidate) = batch.get(index) else {
                continue;
            };
            let names: Vec<&str> = classes.iter().map(|c| c.as_str()).collect();
            let value = serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string());
            if let Err(e) = deps
                .database
                .set_exposure_cache(&candidate.key, &value)
                .await
            {
                common::log_warn!("exposure: failed to cache LLM verdict: {}", e);
            }
            outcome.verdicts.insert(candidate.key.clone(), classes);
        }
    }
    let missing = take.saturating_sub(outcome.verdicts.len());
    if missing > 0 {
        outcome.warnings.push(format!(
            "LLM returned no verdict for {} capabilities; run again to retry them.",
            missing
        ));
    }
    Ok(outcome)
}

//
// Pull (batch index, classes) pairs out of the parser's JSON, ignoring
// unknown classes and malformed items.
//

fn parse_verdicts(json: &str) -> Vec<(usize, Vec<CapabilityClass>)> {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    value
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let index = match item.get("id")? {
                        Value::String(s) => s.trim().parse().ok()?,
                        Value::Number(n) => n.as_u64()? as usize,
                        _ => return None,
                    };
                    let mut classes: Vec<CapabilityClass> = item
                        .get("classes")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .filter_map(CapabilityClass::parse)
                                .collect()
                        })
                        .unwrap_or_default();
                    classes.sort();
                    classes.dedup();
                    Some((index, classes))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_llm_verdicts() {
        let json = r#"{"items": [
            {"id": "0", "classes": ["exfiltration", "bogus", "exfiltration"]},
            {"id": 1, "classes": []},
            {"classes": ["private_data"]}
        ]}"#;
        let verdicts = parse_verdicts(json);
        assert_eq!(
            verdicts,
            vec![(0, vec![CapabilityClass::Exfiltration]), (1, vec![])]
        );
        assert!(parse_verdicts("nope").is_empty());
    }
}
