//
// Agent configuration change ledger.
//
// Every config write made through Praxis (recon editor, `_praxis/write_file`,
// MCP/CLI) is routed here. The ledger snapshots the file on the node,
// computes the new content (raw overwrite or a structured edit), writes it
// back with a compare-and-swap on the before-hash, and records who changed
// what, where and why together with the full before/after content. Changes
// can later be reverted individually or in bulk (newest first), and checked
// for drift against what Praxis last wrote.
//

pub mod edit;
pub mod node_files;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use chrono::Utc;
use common::AgentFileType;
use common::acp_ext::{
    EXT_PRAXIS_CHANGES_APPLY, EXT_PRAXIS_CHANGES_CANCEL, EXT_PRAXIS_CHANGES_GET,
    EXT_PRAXIS_CHANGES_LIST, EXT_PRAXIS_CHANGES_REVERT, EXT_PRAXIS_CHANGES_REVERT_ALL,
    EXT_PRAXIS_CHANGES_STATUS,
};
use common::config_change::ChangeOp;
use common::config_change::{
    ChangeApplyParams, ChangeApplyResult, ChangeCancelParams, ChangeDetail, ChangeGetParams,
    ChangeKind, ChangeListResult, ChangeQuery, ChangeRevertAllParams, ChangeRevertAllResult,
    ChangeRevertOutcome, ChangeRevertParams, ChangeState, ChangeStatusEntry, ChangeStatusParams,
    ChangeStatusResult, ConfigChangeRecord, ConfigFileKind, ConflictPolicy, DriftStatus,
    RevertOutcomeKind, sha256_hex,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::database::Database;
use edit::{apply_op, apply_undo, unified_diff};
use node_files::{FileSnapshot, NodeFiles, WriteResult};

//
// Snapshot → write races are retried this many times before giving up.
//

const CAS_ATTEMPTS: usize = 3;

const BULK_LIMIT: usize = 10_000;

pub struct ChangeLedger {
    db: Arc<Database>,
    files: Arc<dyn NodeFiles>,
    //
    // Caller-chosen cancel tokens (scoped by client id) for in-flight
    // requests. A std mutex: it is never held across an await.
    //
    cancels: Mutex<HashMap<String, CancellationToken>>,
}

//
// Removes a cancel token from the registry when the request finishes.
//

struct CancelRegistration<'a> {
    ledger: &'a ChangeLedger,
    key: Option<String>,
}

impl Drop for CancelRegistration<'_> {
    fn drop(&mut self) {
        if let Some(key) = self.key.take()
            && let Ok(mut map) = self.ledger.cancels.lock()
        {
            map.remove(&key);
        }
    }
}

#[derive(Debug)]
struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cancelled")
    }
}

impl std::error::Error for Cancelled {}

impl ChangeLedger {
    pub fn new(db: Arc<Database>, files: Arc<dyn NodeFiles>) -> Arc<Self> {
        Arc::new(Self {
            db,
            files,
            cancels: Mutex::new(HashMap::new()),
        })
    }

    // -----------------------------------------------------------------------
    // Request routing
    // -----------------------------------------------------------------------

    //
    // Handle one `_praxis/changes/*` request. Returns the JSON result.
    //

    pub async fn handle_request(
        &self,
        client_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        match method {
            EXT_PRAXIS_CHANGES_LIST => {
                let mut query: ChangeQuery = parse(params)?;
                self.resolve_ids(&mut query).await?;
                to_value(ChangeListResult {
                    changes: self.db.list_config_changes(&query, false).await?,
                })
            }
            EXT_PRAXIS_CHANGES_GET => {
                let p: ChangeGetParams = parse(params)?;
                to_value(self.get(&p.change_id).await?)
            }
            EXT_PRAXIS_CHANGES_APPLY => {
                let p: ChangeApplyParams = parse(params)?;
                let (token, _reg) = self.register(client_id, p.cancel_token.as_deref());
                to_value(self.apply(client_id, p, &token).await?)
            }
            EXT_PRAXIS_CHANGES_REVERT => {
                let p: ChangeRevertParams = parse(params)?;
                let (token, _reg) = self.register(client_id, p.cancel_token.as_deref());
                to_value(self.revert(client_id, &p, &token).await?)
            }
            EXT_PRAXIS_CHANGES_REVERT_ALL => {
                let mut p: ChangeRevertAllParams = parse(params)?;
                self.resolve_ids(&mut p.query).await?;
                let (token, _reg) = self.register(client_id, p.cancel_token.as_deref());
                to_value(self.revert_all(client_id, p, &token).await?)
            }
            EXT_PRAXIS_CHANGES_STATUS => {
                let mut p: ChangeStatusParams = parse(params)?;
                self.resolve_ids(&mut p.query).await?;
                let (token, _reg) = self.register(client_id, p.cancel_token.as_deref());
                to_value(self.status(p.query, &token).await?)
            }
            EXT_PRAXIS_CHANGES_CANCEL => {
                let p: ChangeCancelParams = parse(params)?;
                let cancelled = self.cancel(client_id, &p.cancel_token);
                Ok(json!({ "cancelled": cancelled }))
            }
            other => Err(anyhow!("Method not found: {}", other)),
        }
    }

    //
    // Ledger-backed replacement for the raw `_praxis/write_file` path:
    // records the overwrite and answers in the legacy WriteFileResult shape
    // (plus `change_id`) so existing callers keep working.
    //

    pub async fn handle_legacy_write(&self, client_id: &str, params: Value) -> Value {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let result = async {
            let node_id = params
                .get("_meta")
                .and_then(|m| m.get("praxis"))
                .and_then(|p| p.get("nodeId"))
                .and_then(|v| v.as_str())
                .or_else(|| params.get("node_id").and_then(|v| v.as_str()))
                .ok_or_else(|| anyhow!("missing node id"))?
                .to_string();
            let contents = params
                .get("contents")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing contents"))?
                .to_string();
            let s = |k: &str| params.get(k).and_then(|v| v.as_str()).map(String::from);
            let apply = ChangeApplyParams {
                node_id,
                agent_short_name: s("agent_short_name"),
                path: path.clone(),
                file_kind: None,
                config_type: s("config_type"),
                op: ChangeOp::Overwrite { contents },
                engagement: s("engagement"),
                note: s("note"),
                operator: s("operator"),
                dry_run: false,
                cancel_token: s("cancel_token"),
            };
            let (token, _reg) = self.register(client_id, apply.cancel_token.as_deref());
            self.apply(client_id, apply, &token).await
        }
        .await;

        match result {
            Ok(r) => json!({
                "file_type": AgentFileType::Config,
                "path": path,
                "success": true,
                "error": null,
                "change_id": r.change.map(|c| c.id),
                "no_op": r.no_op,
            }),
            Err(e) => json!({
                "file_type": AgentFileType::Config,
                "path": path,
                "success": false,
                "error": e.to_string(),
            }),
        }
    }

    //
    // Expand short change ids in a query to full ids.
    //

    async fn resolve_ids(&self, query: &mut ChangeQuery) -> Result<()> {
        let mut full = Vec::with_capacity(query.change_ids.len());
        for id in &query.change_ids {
            full.push(self.db.resolve_config_change_id(id).await?);
        }
        query.change_ids = full;
        Ok(())
    }

    fn register(
        &self,
        client_id: &str,
        cancel_token: Option<&str>,
    ) -> (CancellationToken, CancelRegistration<'_>) {
        let token = CancellationToken::new();
        let key = cancel_token.map(|t| format!("{}:{}", client_id, t));
        if let Some(k) = &key
            && let Ok(mut map) = self.cancels.lock()
        {
            map.insert(k.clone(), token.clone());
        }
        (token, CancelRegistration { ledger: self, key })
    }

    fn cancel(&self, client_id: &str, cancel_token: &str) -> bool {
        let key = format!("{}:{}", client_id, cancel_token);
        let token = self.cancels.lock().ok().and_then(|mut m| m.remove(&key));
        match token {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    // -----------------------------------------------------------------------
    // Node I/O with cancellation
    // -----------------------------------------------------------------------

    async fn snapshot_one(
        &self,
        node_id: &str,
        path: &str,
        token: &CancellationToken,
    ) -> Result<FileSnapshot> {
        let paths = vec![path.to_string()];
        let snaps = tokio::select! {
            _ = token.cancelled() => return Err(Cancelled.into()),
            r = self.files.snapshot(node_id, &paths) => r?,
        };
        let snap = snaps
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("node returned no snapshot for {}", path))?;
        if let Some(err) = &snap.error {
            bail!("{}", err);
        }
        Ok(snap)
    }

    async fn write_cas(
        &self,
        node_id: &str,
        path: &str,
        expected: Option<&str>,
        content: Option<&str>,
        token: &CancellationToken,
    ) -> Result<WriteResult> {
        tokio::select! {
            _ = token.cancelled() => Err(Cancelled.into()),
            r = self.files.write(node_id, path, expected, content) => r,
        }
    }

    // -----------------------------------------------------------------------
    // Get
    // -----------------------------------------------------------------------

    pub async fn get(&self, id_or_prefix: &str) -> Result<ChangeDetail> {
        let id = self.db.resolve_config_change_id(id_or_prefix).await?;
        let change = self
            .db
            .get_config_change(&id)
            .await?
            .ok_or_else(|| anyhow!("change {} not found", id))?;
        let diff = unified_diff(
            change.before_content.as_deref().unwrap_or(""),
            change.after_content.as_deref().unwrap_or(""),
            &label("before", change.before_exists, &change.path),
            &label("after", change.after_exists, &change.path),
        );
        Ok(ChangeDetail { change, diff })
    }

    // -----------------------------------------------------------------------
    // Apply
    // -----------------------------------------------------------------------

    pub async fn apply(
        &self,
        client_id: &str,
        mut p: ChangeApplyParams,
        token: &CancellationToken,
    ) -> Result<ChangeApplyResult> {
        if p.node_id.trim().is_empty() {
            bail!("node id is required");
        }
        if p.path.trim().is_empty() {
            bail!("path is required");
        }
        let kind = p
            .file_kind
            .unwrap_or_else(|| ConfigFileKind::from_path(&p.path));
        if p.agent_short_name.is_none() || p.config_type.is_none() {
            self.fill_from_recon(&mut p).await;
        }

        let id = uuid::Uuid::new_v4().to_string();
        for _ in 0..CAS_ATTEMPTS {
            let before = self.snapshot_one(&p.node_id, &p.path, token).await?;
            let before_content = before
                .exists
                .then(|| before.content.clone().unwrap_or_default());
            let outcome = apply_op(kind, before_content.as_deref(), &p.op, &id)?;

            if before.exists && before_content.as_deref() == Some(outcome.content.as_str()) {
                return Ok(ChangeApplyResult {
                    change: None,
                    diff: String::new(),
                    no_op: true,
                    dry_run: p.dry_run,
                });
            }

            let diff = unified_diff(
                before_content.as_deref().unwrap_or(""),
                &outcome.content,
                &label("before", before.exists, &p.path),
                &label("after", true, &p.path),
            );
            if p.dry_run {
                return Ok(ChangeApplyResult {
                    change: None,
                    diff,
                    no_op: false,
                    dry_run: true,
                });
            }

            let expected = before.exists.then(|| before.sha256.clone()).flatten();
            match self
                .write_cas(
                    &p.node_id,
                    &p.path,
                    expected.as_deref(),
                    Some(&outcome.content),
                    token,
                )
                .await?
            {
                WriteResult::Conflict => continue,
                WriteResult::Written => {}
            }

            let after_sha = sha256_hex(outcome.content.as_bytes());
            let record = ConfigChangeRecord {
                id: id.clone(),
                created_at: Utc::now(),
                kind: ChangeKind::Apply,
                state: ChangeState::Active,
                node_id: p.node_id.clone(),
                agent_short_name: p.agent_short_name.clone(),
                path: p.path.clone(),
                file_kind: kind,
                config_type: p.config_type.clone(),
                summary: p.op.summary(),
                op: Some(p.op.clone()),
                operator: operator_or_default(p.operator.as_deref(), client_id),
                engagement: clean(p.engagement.as_deref()),
                note: clean(p.note.as_deref()),
                before_exists: before.exists,
                before_sha256: expected.clone(),
                before_content: before_content.clone(),
                after_exists: true,
                after_sha256: Some(after_sha.clone()),
                after_content: Some(outcome.content.clone()),
                undo: outcome.undo,
                reverts_change_id: None,
                reverted_by: None,
                reverted_at: None,
                drift_status: Some(DriftStatus::InSync),
                drift_checked_at: Some(Utc::now()),
            };

            if let Err(e) = self.db.insert_config_change(&record).await {
                //
                // Never leave an unrecorded write behind: put the original
                // back before reporting the failure.
                //
                let restore = self
                    .files
                    .write(
                        &p.node_id,
                        &p.path,
                        Some(&after_sha),
                        before_content.as_deref(),
                    )
                    .await;
                common::log_error!(
                    "Change ledger insert failed for {} on node {} ({}); restore: {:?}",
                    p.path,
                    common::short_id(&p.node_id),
                    e,
                    restore
                );
                bail!("failed to record change; the write was rolled back: {}", e);
            }

            common::log_info!(
                "Config change {} recorded: {} {} on node {} by {}",
                common::short_id(&record.id),
                record.summary,
                record.path,
                common::short_id(&record.node_id),
                record.operator
            );
            return Ok(ChangeApplyResult {
                change: Some(strip_content(record)),
                diff,
                no_op: false,
                dry_run: false,
            });
        }
        bail!(
            "{} kept changing while applying the edit; try again",
            p.path
        )
    }

    //
    // Attribute a write to an agent / config type using the stored recon
    // results for the node when the caller did not say.
    //

    async fn fill_from_recon(&self, p: &mut ChangeApplyParams) {
        let Ok(results) = self.db.get_recon_results_for_node(&p.node_id).await else {
            return;
        };
        for r in results {
            if p.agent_short_name
                .as_ref()
                .is_some_and(|agent| *agent != r.agent_short_name)
            {
                continue;
            }
            if let Some(item) = r
                .recon_result
                .config
                .items
                .iter()
                .find(|i| i.path == p.path)
            {
                p.agent_short_name.get_or_insert(r.agent_short_name.clone());
                p.config_type.get_or_insert(item.config_type.clone());
                return;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Revert
    // -----------------------------------------------------------------------

    pub async fn revert(
        &self,
        client_id: &str,
        p: &ChangeRevertParams,
        token: &CancellationToken,
    ) -> Result<ChangeRevertOutcome> {
        let id = self.db.resolve_config_change_id(&p.change_id).await?;
        let record = self
            .db
            .get_config_change(&id)
            .await?
            .ok_or_else(|| anyhow!("change {} not found", id))?;
        if record.kind == ChangeKind::Revert {
            bail!(
                "change {} is itself a revert and cannot be reverted",
                common::short_id(&id)
            );
        }
        let operator = operator_or_default(p.operator.as_deref(), client_id);
        Ok(self
            .revert_record(&record, p.on_conflict, &operator, p.note.as_deref(), token)
            .await)
    }

    async fn revert_record(
        &self,
        record: &ConfigChangeRecord,
        policy: ConflictPolicy,
        operator: &str,
        note: Option<&str>,
        token: &CancellationToken,
    ) -> ChangeRevertOutcome {
        let outcome = |kind: RevertOutcomeKind, message: String| ChangeRevertOutcome {
            change_id: record.id.clone(),
            node_id: record.node_id.clone(),
            path: record.path.clone(),
            outcome: kind,
            message,
            diff: None,
            structured_undo_available: false,
            revert_change_id: None,
        };

        if record.state == ChangeState::Reverted {
            return outcome(RevertOutcomeKind::Skipped, "already reverted".to_string());
        }

        match self.try_revert(record, policy, operator, note, token).await {
            Ok(o) => o,
            Err(e) if token.is_cancelled() || e.is::<Cancelled>() => {
                outcome(RevertOutcomeKind::Cancelled, "cancelled".to_string())
            }
            Err(e) => outcome(RevertOutcomeKind::Failed, e.to_string()),
        }
    }

    async fn try_revert(
        &self,
        record: &ConfigChangeRecord,
        policy: ConflictPolicy,
        operator: &str,
        note: Option<&str>,
        token: &CancellationToken,
    ) -> Result<ChangeRevertOutcome> {
        let structured_available = record.undo.as_ref().is_some_and(|u| !u.is_empty());

        for _ in 0..CAS_ATTEMPTS {
            let current = self
                .snapshot_one(&record.node_id, &record.path, token)
                .await?;
            let current_content = current
                .exists
                .then(|| current.content.clone().unwrap_or_default());

            let matches_after =
                current.exists == record.after_exists && current.sha256 == record.after_sha256;
            let matches_before =
                current.exists == record.before_exists && current.sha256 == record.before_sha256;

            //
            // Decide the target state of the file.
            //

            let (target, outcome_kind, how): (Option<String>, RevertOutcomeKind, &str) =
                if matches_after || matches_before {
                    (
                        record
                            .before_content
                            .clone()
                            .filter(|_| record.before_exists),
                        RevertOutcomeKind::Reverted,
                        "exact",
                    )
                } else {
                    match policy {
                        ConflictPolicy::Abort => {
                            return Ok(ChangeRevertOutcome {
                                change_id: record.id.clone(),
                                node_id: record.node_id.clone(),
                                path: record.path.clone(),
                                outcome: RevertOutcomeKind::Conflict,
                                message: if current.exists {
                                    "file was modified after this change".to_string()
                                } else {
                                    "file no longer exists".to_string()
                                },
                                diff: Some(unified_diff(
                                    record.after_content.as_deref().unwrap_or(""),
                                    current_content.as_deref().unwrap_or(""),
                                    &label("recorded", record.after_exists, &record.path),
                                    &label("current", current.exists, &record.path),
                                )),
                                structured_undo_available: structured_available && current.exists,
                                revert_change_id: None,
                            });
                        }
                        ConflictPolicy::Structured => {
                            let Some(undo) = record.undo.as_ref().filter(|u| !u.is_empty()) else {
                                bail!(
                                    "file changed since this change and a structured undo is not available ({}); use force to restore the original",
                                    record.summary
                                );
                            };
                            let Some(text) = current_content.as_deref() else {
                                bail!("file no longer exists; use force to restore the original");
                            };
                            (
                                Some(apply_undo(record.file_kind, text, undo)?),
                                RevertOutcomeKind::RevertedStructured,
                                "structured",
                            )
                        }
                        ConflictPolicy::Force => (
                            record
                                .before_content
                                .clone()
                                .filter(|_| record.before_exists),
                            RevertOutcomeKind::RevertedForced,
                            "forced",
                        ),
                    }
                };

            //
            // Write unless the file is already in the target state.
            //

            let already = match &target {
                Some(t) => current_content.as_deref() == Some(t.as_str()),
                None => !current.exists,
            };
            if !already {
                let expected = current.sha256.clone().filter(|_| current.exists);
                match self
                    .write_cas(
                        &record.node_id,
                        &record.path,
                        expected.as_deref(),
                        target.as_deref(),
                        token,
                    )
                    .await?
                {
                    WriteResult::Conflict => continue,
                    WriteResult::Written => {}
                }
            }

            let now = Utc::now();
            let revert = ConfigChangeRecord {
                id: uuid::Uuid::new_v4().to_string(),
                created_at: now,
                kind: ChangeKind::Revert,
                state: ChangeState::Active,
                node_id: record.node_id.clone(),
                agent_short_name: record.agent_short_name.clone(),
                path: record.path.clone(),
                file_kind: record.file_kind,
                config_type: record.config_type.clone(),
                op: None,
                summary: format!("revert {} ({})", common::short_id(&record.id), how),
                operator: operator.to_string(),
                engagement: record.engagement.clone(),
                note: clean(note),
                before_exists: current.exists,
                before_sha256: current.sha256.clone().filter(|_| current.exists),
                before_content: current_content.clone(),
                after_exists: target.is_some(),
                after_sha256: target.as_deref().map(|t| sha256_hex(t.as_bytes())),
                after_content: target.clone(),
                undo: None,
                reverts_change_id: Some(record.id.clone()),
                reverted_by: None,
                reverted_at: None,
                drift_status: None,
                drift_checked_at: None,
            };
            self.db.insert_config_change(&revert).await?;
            if !self
                .db
                .mark_config_change_reverted(&record.id, &revert.id, &now)
                .await?
            {
                common::log_warn!(
                    "Change {} was already marked reverted when revert {} completed",
                    common::short_id(&record.id),
                    common::short_id(&revert.id)
                );
            }
            common::log_info!(
                "Config change {} reverted ({}) by {} on node {}",
                common::short_id(&record.id),
                how,
                operator,
                common::short_id(&record.node_id)
            );

            return Ok(ChangeRevertOutcome {
                change_id: record.id.clone(),
                node_id: record.node_id.clone(),
                path: record.path.clone(),
                outcome: outcome_kind,
                message: if already {
                    "file already matched the target state".to_string()
                } else if target.is_none() {
                    "file removed (it did not exist before the change)".to_string()
                } else {
                    format!("restored ({})", how)
                },
                diff: None,
                structured_undo_available: structured_available,
                revert_change_id: Some(revert.id),
            });
        }
        bail!("{} kept changing while reverting; try again", record.path)
    }

    //
    // Revert every active change matching the query, newest first, so
    // stacked changes to the same file unwind in order. Conflicts follow
    // `on_conflict`; a skipped newer change makes older changes to the same
    // file conflict too, so nothing is clobbered out of order.
    //

    pub async fn revert_all(
        &self,
        client_id: &str,
        p: ChangeRevertAllParams,
        token: &CancellationToken,
    ) -> Result<ChangeRevertAllResult> {
        if !p.query.has_scope() && !p.all {
            bail!(
                "refusing to revert every change without an explicit scope; pass a node, agent, engagement, change ids, or all=true"
            );
        }
        let mut query = p.query.clone();
        query.kind = Some(ChangeKind::Apply);
        query.state = Some(ChangeState::Active);
        query.limit = Some(BULK_LIMIT);
        let records = self.db.list_config_changes(&query, true).await?;
        let operator = operator_or_default(p.operator.as_deref(), client_id);

        let mut outcomes = Vec::with_capacity(records.len());
        let mut cancelled = false;
        for record in &records {
            if token.is_cancelled() {
                cancelled = true;
                break;
            }
            let o = self
                .revert_record(record, p.on_conflict, &operator, p.note.as_deref(), token)
                .await;
            if o.outcome == RevertOutcomeKind::Cancelled {
                cancelled = true;
            }
            outcomes.push(o);
            if cancelled {
                break;
            }
        }
        Ok(ChangeRevertAllResult {
            outcomes,
            cancelled,
        })
    }

    // -----------------------------------------------------------------------
    // Drift status
    // -----------------------------------------------------------------------

    pub async fn status(
        &self,
        mut query: ChangeQuery,
        token: &CancellationToken,
    ) -> Result<ChangeStatusResult> {
        query.kind = Some(ChangeKind::Apply);
        query.state = Some(ChangeState::Active);
        query.limit = Some(query.limit.unwrap_or(BULK_LIMIT));
        let records = self.db.list_config_changes(&query, false).await?;

        //
        // Newest active after-hash per (node, path), to tell "changed again
        // by a later Praxis change" apart from foreign modifications. The
        // scope may be narrower than the file's full history, so look at
        // every active change for the affected files.
        //

        let mut latest: HashMap<(String, String), (chrono::DateTime<Utc>, Option<String>)> =
            HashMap::new();
        let mut by_node: HashMap<String, Vec<String>> = HashMap::new();
        for r in &records {
            let paths = by_node.entry(r.node_id.clone()).or_default();
            if !paths.contains(&r.path) {
                paths.push(r.path.clone());
                let siblings = self
                    .db
                    .list_config_changes(
                        &ChangeQuery {
                            node_id: Some(r.node_id.clone()),
                            path: Some(r.path.clone()),
                            kind: Some(ChangeKind::Apply),
                            state: Some(ChangeState::Active),
                            limit: Some(1),
                            ..Default::default()
                        },
                        false,
                    )
                    .await?;
                if let Some(s) = siblings.first() {
                    latest.insert(
                        (s.node_id.clone(), s.path.clone()),
                        (s.created_at, s.after_sha256.clone()),
                    );
                }
            }
        }

        //
        // Snapshot every node concurrently so one slow or offline node only
        // costs a single request timeout.
        //

        let requests = by_node.iter().map(|(node_id, paths)| async move {
            (node_id, paths, self.files.snapshot(node_id, paths).await)
        });
        let (results, cancelled) = tokio::select! {
            _ = token.cancelled() => (Vec::new(), true),
            r = futures_util::future::join_all(requests) => (r, false),
        };
        let mut snapshots: HashMap<(String, String), std::result::Result<FileSnapshot, String>> =
            HashMap::new();
        for (node_id, paths, result) in results {
            match result {
                Ok(snaps) => {
                    for s in snaps {
                        snapshots.insert((node_id.clone(), s.path.clone()), Ok(s));
                    }
                }
                Err(e) => {
                    for path in paths {
                        snapshots.insert((node_id.clone(), path.clone()), Err(e.to_string()));
                    }
                }
            }
        }

        let now = Utc::now();
        let mut entries = Vec::new();
        for r in &records {
            let key = (r.node_id.clone(), r.path.clone());
            let Some(snap) = snapshots.get(&key) else {
                continue;
            };
            let (status, current_sha, detail) = match snap {
                Err(e) => (DriftStatus::Unreachable, None, Some(e.clone())),
                Ok(s) if s.error.is_some() => (DriftStatus::Unreachable, None, s.error.clone()),
                Ok(s) if !s.exists => (DriftStatus::Missing, None, None),
                Ok(s) if s.sha256 == r.after_sha256 => {
                    (DriftStatus::InSync, s.sha256.clone(), None)
                }
                Ok(s) => {
                    let superseded = latest.get(&key).is_some_and(|(at, sha)| {
                        *at > r.created_at && sha.is_some() && *sha == s.sha256
                    });
                    if superseded {
                        (DriftStatus::Superseded, s.sha256.clone(), None)
                    } else {
                        (DriftStatus::Modified, s.sha256.clone(), None)
                    }
                }
            };
            if let Err(e) = self
                .db
                .update_config_change_drift(&r.id, status, &now)
                .await
            {
                common::log_warn!("Failed to store drift status for {}: {}", r.id, e);
            }
            entries.push(ChangeStatusEntry {
                change_id: r.id.clone(),
                node_id: r.node_id.clone(),
                path: r.path.clone(),
                status,
                current_sha256: current_sha,
                detail,
            });
        }

        Ok(ChangeStatusResult { entries, cancelled })
    }
}

fn parse<T: DeserializeOwned>(params: Value) -> Result<T> {
    serde_json::from_value(params).map_err(|e| anyhow!("invalid params: {}", e))
}

fn to_value<T: Serialize>(v: T) -> Result<Value> {
    Ok(serde_json::to_value(v)?)
}

fn clean(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

fn operator_or_default(operator: Option<&str>, client_id: &str) -> String {
    clean(operator).unwrap_or_else(|| format!("client:{}", common::short_id(client_id)))
}

fn label(side: &str, exists: bool, path: &str) -> String {
    if exists {
        format!("{} {}", side, path)
    } else {
        format!("{} (file did not exist)", side)
    }
}

fn strip_content(mut r: ConfigChangeRecord) -> ConfigChangeRecord {
    r.before_content = None;
    r.after_content = None;
    r
}

#[cfg(test)]
mod tests {
    use super::node_files::local::LocalFiles;
    use super::*;
    use crate::database::DatabaseConfig;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    const NODE: &str = "node-0001";

    //
    // Scratch directory + SQLite ledger per test; removed on drop. Only
    // files created here are ever written.
    //

    struct Fixture {
        dir: PathBuf,
        ledger: Arc<ChangeLedger>,
        files: Arc<LocalFiles>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    async fn fixture() -> Fixture {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "praxis_change_ledger_test_{}_{}",
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::new(&DatabaseConfig::Sqlite {
            path: dir.join("ledger.db"),
        })
        .await
        .unwrap();
        let files = Arc::new(LocalFiles::new());
        let ledger = ChangeLedger::new(Arc::new(db), files.clone());
        Fixture { dir, ledger, files }
    }

    impl Fixture {
        fn file(&self, name: &str, content: Option<&str>) -> String {
            let p = self.dir.join(name);
            if let Some(c) = content {
                std::fs::write(&p, c).unwrap();
            }
            p.to_string_lossy().into_owned()
        }

        async fn apply(&self, path: &str, op: ChangeOp, engagement: &str) -> ConfigChangeRecord {
            let r = self
                .ledger
                .apply(
                    "client-1",
                    ChangeApplyParams {
                        node_id: NODE.into(),
                        agent_short_name: Some("claude-code".into()),
                        path: path.into(),
                        file_kind: None,
                        config_type: None,
                        op,
                        engagement: Some(engagement.into()),
                        note: Some("test".into()),
                        operator: Some("tester".into()),
                        dry_run: false,
                        cancel_token: None,
                    },
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
            r.change.expect("change recorded")
        }

        async fn revert(&self, id: &str, policy: ConflictPolicy) -> ChangeRevertOutcome {
            self.ledger
                .revert(
                    "client-1",
                    &ChangeRevertParams {
                        change_id: id.into(),
                        on_conflict: policy,
                        operator: None,
                        note: None,
                        cancel_token: None,
                    },
                    &CancellationToken::new(),
                )
                .await
                .unwrap()
        }

        async fn status(&self) -> HashMap<String, DriftStatus> {
            self.ledger
                .status(ChangeQuery::default(), &CancellationToken::new())
                .await
                .unwrap()
                .entries
                .into_iter()
                .map(|e| (e.change_id, e.status))
                .collect()
        }
    }

    fn set(path: &str, v: Value) -> ChangeOp {
        ChangeOp::Set {
            key_path: path.into(),
            value: v,
        }
    }

    const SETTINGS: &str = "{\n  // keep me\n  \"model\": \"sonnet\"\n}\n";

    #[tokio::test]
    async fn apply_records_before_after_and_hashes() {
        let f = fixture().await;
        let path = f.file("settings.json", Some(SETTINGS));
        let rec = f
            .apply(&path, set("/env/PRAXIS_DEMO", json!("1")), "eng-a")
            .await;

        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("// keep me"));
        assert!(on_disk.contains("PRAXIS_DEMO"));

        let full = f.ledger.get(&rec.id[..8]).await.unwrap();
        assert_eq!(full.change.before_content.as_deref(), Some(SETTINGS));
        assert_eq!(full.change.after_content.as_deref(), Some(on_disk.as_str()));
        assert_eq!(
            full.change.before_sha256.as_deref(),
            Some(sha256_hex(SETTINGS.as_bytes()).as_str())
        );
        assert_eq!(
            full.change.after_sha256.as_deref(),
            Some(sha256_hex(on_disk.as_bytes()).as_str())
        );
        assert_eq!(full.change.operator, "tester");
        assert_eq!(full.change.engagement.as_deref(), Some("eng-a"));
        assert!(full.diff.contains("+"));
        assert!(full.change.undo.is_some());
    }

    #[tokio::test]
    async fn dry_run_and_no_op_do_not_write_or_record() {
        let f = fixture().await;
        let path = f.file("settings.json", Some(SETTINGS));
        let mut params = ChangeApplyParams {
            node_id: NODE.into(),
            agent_short_name: None,
            path: path.clone(),
            file_kind: None,
            config_type: None,
            op: set("/model", json!("opus")),
            engagement: None,
            note: None,
            operator: None,
            dry_run: true,
            cancel_token: None,
        };
        let r = f
            .ledger
            .apply("c", params.clone(), &CancellationToken::new())
            .await
            .unwrap();
        assert!(r.dry_run && r.change.is_none() && r.diff.contains("opus"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SETTINGS);

        params.dry_run = false;
        params.op = set("/model", json!("sonnet"));
        let r = f
            .ledger
            .apply("c", params, &CancellationToken::new())
            .await
            .unwrap();
        assert!(r.no_op && r.change.is_none());
        let all = f
            .ledger
            .db
            .list_config_changes(&ChangeQuery::default(), false)
            .await
            .unwrap();
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn revert_exact_restores_original_bytes() {
        let f = fixture().await;
        let path = f.file("settings.json", Some(SETTINGS));
        let rec = f.apply(&path, set("/env/A", json!("1")), "e").await;

        let out = f.revert(&rec.id, ConflictPolicy::Abort).await;
        assert_eq!(out.outcome, RevertOutcomeKind::Reverted);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SETTINGS);

        let original = f
            .ledger
            .db
            .get_config_change(&rec.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(original.state, ChangeState::Reverted);
        let revert_id = out.revert_change_id.unwrap();
        assert_eq!(original.reverted_by.as_deref(), Some(revert_id.as_str()));
        let revert_row = f
            .ledger
            .db
            .get_config_change(&revert_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(revert_row.kind, ChangeKind::Revert);
        assert_eq!(
            revert_row.reverts_change_id.as_deref(),
            Some(rec.id.as_str())
        );

        //
        // A second revert is a no-op, and revert rows cannot be reverted.
        //

        assert_eq!(
            f.revert(&rec.id, ConflictPolicy::Abort).await.outcome,
            RevertOutcomeKind::Skipped
        );
        assert!(
            f.ledger
                .revert(
                    "c",
                    &ChangeRevertParams {
                        change_id: revert_id,
                        on_conflict: ConflictPolicy::Abort,
                        operator: None,
                        note: None,
                        cancel_token: None
                    },
                    &CancellationToken::new()
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn revert_of_created_file_deletes_it() {
        let f = fixture().await;
        let path = f.file("AGENTS.md", None);
        let rec = f
            .apply(
                &path,
                ChangeOp::AppendBlock {
                    content: "<!-- harmless example -->".into(),
                },
                "e",
            )
            .await;
        assert!(!rec.before_exists);
        assert!(std::path::Path::new(&path).exists());
        let out = f.revert(&rec.id, ConflictPolicy::Abort).await;
        assert_eq!(out.outcome, RevertOutcomeKind::Reverted);
        assert!(!std::path::Path::new(&path).exists());
    }

    #[tokio::test]
    async fn revert_with_intervening_modification() {
        let f = fixture().await;
        let path = f.file("settings.json", Some(SETTINGS));
        let rec = f.apply(&path, set("/env/A", json!("1")), "e").await;

        //
        // Someone else edits the file after Praxis.
        //

        let after = std::fs::read_to_string(&path).unwrap();
        let foreign = after.replace("\"sonnet\"", "\"opus\"");
        std::fs::write(&path, &foreign).unwrap();

        let out = f.revert(&rec.id, ConflictPolicy::Abort).await;
        assert_eq!(out.outcome, RevertOutcomeKind::Conflict);
        assert!(out.structured_undo_available);
        assert!(
            out.diff
                .as_deref()
                .unwrap()
                .contains("+  \"model\": \"opus\"")
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), foreign);

        let out = f.revert(&rec.id, ConflictPolicy::Structured).await;
        assert_eq!(out.outcome, RevertOutcomeKind::RevertedStructured);
        let now = std::fs::read_to_string(&path).unwrap();
        assert!(now.contains("\"opus\""), "foreign edit kept");
        assert!(!now.contains("\"A\""), "praxis change removed");
        assert!(now.contains("// keep me"));
    }

    #[tokio::test]
    async fn force_revert_restores_before_content() {
        let f = fixture().await;
        let path = f.file("config.toml", Some("model = \"o3\"\n"));
        let rec = f
            .apply(
                &path,
                ChangeOp::Overwrite {
                    contents: "model = \"x\"\n".into(),
                },
                "e",
            )
            .await;
        std::fs::write(&path, "model = \"y\"\n").unwrap();

        //
        // Raw overwrites have no structured undo.
        //

        let out = f.revert(&rec.id, ConflictPolicy::Structured).await;
        assert_eq!(out.outcome, RevertOutcomeKind::Failed);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "model = \"y\"\n");

        let out = f.revert(&rec.id, ConflictPolicy::Force).await;
        assert_eq!(out.outcome, RevertOutcomeKind::RevertedForced);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "model = \"o3\"\n");
    }

    #[tokio::test]
    async fn revert_all_unwinds_newest_first() {
        let f = fixture().await;
        let a = f.file("settings.json", Some(SETTINGS));
        let b = f.file("config.toml", Some("# codex\nmodel = \"o3\"\n"));
        let c1 = f.apply(&a, set("/env/ONE", json!("1")), "eng").await;
        let c2 = f.apply(&b, set("/env/TWO", json!("2")), "eng").await;
        let c3 = f
            .apply(
                &a,
                ChangeOp::ListAppend {
                    key_path: "/permissions/allow".into(),
                    value: json!("Bash(ls)"),
                },
                "eng",
            )
            .await;
        let other = f
            .apply(&a, set("/env/OTHER", json!("x")), "other-eng")
            .await;

        //
        // Reverting an older change underneath a newer one on the same
        // file conflicts instead of clobbering the newer change.
        //

        let out = f.revert(&c1.id, ConflictPolicy::Abort).await;
        assert_eq!(out.outcome, RevertOutcomeKind::Conflict);

        let result = f
            .ledger
            .revert_all(
                "c",
                ChangeRevertAllParams {
                    query: ChangeQuery {
                        node_id: Some(NODE.into()),
                        ..Default::default()
                    },
                    all: false,
                    on_conflict: ConflictPolicy::Abort,
                    operator: None,
                    note: None,
                    cancel_token: None,
                },
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let order: Vec<&str> = result
            .outcomes
            .iter()
            .map(|o| o.change_id.as_str())
            .collect();
        assert_eq!(
            order,
            vec![
                other.id.as_str(),
                c3.id.as_str(),
                c2.id.as_str(),
                c1.id.as_str()
            ]
        );
        assert!(
            result
                .outcomes
                .iter()
                .all(|o| o.outcome == RevertOutcomeKind::Reverted)
        );
        assert_eq!(std::fs::read_to_string(&a).unwrap(), SETTINGS);
        assert_eq!(
            std::fs::read_to_string(&b).unwrap(),
            "# codex\nmodel = \"o3\"\n"
        );
    }

    #[tokio::test]
    async fn revert_all_requires_scope_and_filters_by_engagement() {
        let f = fixture().await;
        let a = f.file("settings.json", Some(SETTINGS));
        let keep = f.apply(&a, set("/env/KEEP", json!("1")), "other").await;
        let b = f.file("notes.md", Some("# Notes\n"));
        let gone = f
            .apply(
                &b,
                ChangeOp::AppendBlock {
                    content: "x".into(),
                },
                "eng",
            )
            .await;

        let unscoped = ChangeRevertAllParams {
            query: ChangeQuery::default(),
            all: false,
            on_conflict: ConflictPolicy::Abort,
            operator: None,
            note: None,
            cancel_token: None,
        };
        assert!(
            f.ledger
                .revert_all("c", unscoped.clone(), &CancellationToken::new())
                .await
                .is_err()
        );

        let mut scoped = unscoped;
        scoped.query.engagement = Some("eng".into());
        let r = f
            .ledger
            .revert_all("c", scoped, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(r.outcomes.len(), 1);
        assert_eq!(r.outcomes[0].change_id, gone.id);
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "# Notes\n");
        let kept = f
            .ledger
            .db
            .get_config_change(&keep.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(kept.state, ChangeState::Active);
    }

    #[tokio::test]
    async fn drift_detection() {
        let f = fixture().await;
        let a = f.file("settings.json", Some(SETTINGS));
        let b = f.file("other.json", Some("{}"));
        let c = f.file("gone.json", Some("{}"));
        let a1 = f.apply(&a, set("/x", json!(1)), "e").await;
        let a2 = f.apply(&a, set("/y", json!(2)), "e").await;
        let b1 = f.apply(&b, set("/x", json!(1)), "e").await;
        let c1 = f.apply(&c, set("/x", json!(1)), "e").await;

        let s = f.status().await;
        assert_eq!(s[&a1.id], DriftStatus::Superseded);
        assert_eq!(s[&a2.id], DriftStatus::InSync);
        assert_eq!(s[&b1.id], DriftStatus::InSync);

        std::fs::write(&b, "{\"x\": 2}").unwrap();
        std::fs::remove_file(&c).unwrap();
        let s = f.status().await;
        assert_eq!(s[&b1.id], DriftStatus::Modified);
        assert_eq!(s[&c1.id], DriftStatus::Missing);
        assert_eq!(s[&a1.id], DriftStatus::Superseded);

        let stored = f
            .ledger
            .db
            .get_config_change(&b1.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.drift_status, Some(DriftStatus::Modified));

        f.files
            .offline
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let s = f.status().await;
        assert_eq!(s[&a2.id], DriftStatus::Unreachable);
    }

    #[tokio::test]
    async fn cancelled_requests_stop_promptly() {
        let f = fixture().await;
        let a = f.file("settings.json", Some(SETTINGS));
        let rec = f.apply(&a, set("/x", json!(1)), "e").await;

        let token = CancellationToken::new();
        token.cancel();
        let r = f
            .ledger
            .revert_all(
                "c",
                ChangeRevertAllParams {
                    query: ChangeQuery::default(),
                    all: true,
                    on_conflict: ConflictPolicy::Abort,
                    operator: None,
                    note: None,
                    cancel_token: None,
                },
                &token,
            )
            .await
            .unwrap();
        assert!(r.cancelled);
        assert!(r.outcomes.is_empty());
        let still = f
            .ledger
            .db
            .get_config_change(&rec.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(still.state, ChangeState::Active);

        //
        // The cancel request from the same client trips the registered
        // token; another client's cancel does not.
        //

        let (tok, _reg) = f.ledger.register("client-x", Some("t1"));
        assert!(!f.ledger.cancel("client-y", "t1"));
        assert!(f.ledger.cancel("client-x", "t1"));
        assert!(tok.is_cancelled());
    }

    #[tokio::test]
    async fn legacy_write_is_recorded() {
        let f = fixture().await;
        let path = f.file("CLAUDE.md", Some("old\n"));
        let reply = f
            .ledger
            .handle_legacy_write(
                "client-z",
                json!({
                    "file_type": "Config",
                    "path": path,
                    "contents": "new\n",
                    "_meta": { "praxis": { "nodeId": NODE } },
                    "engagement": "eng-legacy",
                }),
            )
            .await;
        assert_eq!(reply["success"], true);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
        let id = reply["change_id"].as_str().unwrap();
        let rec = f.ledger.db.get_config_change(id).await.unwrap().unwrap();
        assert_eq!(rec.engagement.as_deref(), Some("eng-legacy"));
        assert_eq!(rec.before_content.as_deref(), Some("old\n"));
        assert!(rec.operator.starts_with("client:"));
    }

    #[tokio::test]
    async fn ledger_is_queryable_via_log_query() {
        let f = fixture().await;
        let path = f.file("settings.json", Some(SETTINGS));
        let rec = f.apply(&path, set("/x", json!(1)), "eng-q").await;
        f.apply(&path, set("/y", json!(1)), "other").await;

        let config = Arc::new(tokio::sync::RwLock::new(
            crate::config::ServiceConfig::new(f.ledger.db.clone())
                .await
                .unwrap(),
        ));
        let result = crate::log_query::execute_log_query(
            "ConfigChangeLogs | where engagement == \"eng-q\" | project change_id, before_exists, summary",
            &f.ledger.db,
            &Arc::new(crate::state::NodeRegistry::new()),
            &config,
        )
        .await
        .unwrap();
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], json!(rec.id));
        assert_eq!(result.rows[0][1], json!(1));
        assert_eq!(result.rows[0][2], json!("set /x"));
    }

    #[tokio::test]
    async fn migrations_are_idempotent() {
        let f = fixture().await;
        let path = f.file("settings.json", Some(SETTINGS));
        let rec = f.apply(&path, set("/x", json!(1)), "e").await;

        //
        // Re-open the same database twice: schema init + migrations run
        // again and must neither fail nor touch existing rows.
        //

        for _ in 0..2 {
            let db = Database::new(&DatabaseConfig::Sqlite {
                path: f.dir.join("ledger.db"),
            })
            .await
            .unwrap();
            let again = db.get_config_change(&rec.id).await.unwrap().unwrap();
            assert_eq!(again.after_sha256, rec.after_sha256);
        }
    }
}
