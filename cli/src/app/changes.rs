use super::*;
use common::acp_ext::{
    EXT_PRAXIS_CHANGES_CANCEL, EXT_PRAXIS_CHANGES_GET, EXT_PRAXIS_CHANGES_LIST,
    EXT_PRAXIS_CHANGES_REVERT, EXT_PRAXIS_CHANGES_REVERT_ALL, EXT_PRAXIS_CHANGES_STATUS,
};
use common::config_change::{
    ChangeDetail, ChangeKind, ChangeListResult, ChangeQuery, ChangeRevertAllParams,
    ChangeRevertAllResult, ChangeRevertOutcome, ChangeRevertParams, ChangeState,
    ChangeStatusParams, ChangeStatusResult, ConfigChangeRecord, ConflictPolicy, DriftStatus,
    default_engagement, default_operator,
};
use serde_json::json;

//
// Changes tab (Operations window): the agent configuration change ledger.
// Lists every config write made through Praxis with its drift status, shows
// the before/after diff, and reverts single changes or every visible active
// change. Reverts that hit a modified file surface the conflict diff and
// let the operator pick a structured undo or a forced restore.
//

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ChangeStatusFilter {
    #[default]
    All,
    Active,
    Drifted,
    Reverted,
}

impl ChangeStatusFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Active,
            Self::Active => Self::Drifted,
            Self::Drifted => Self::Reverted,
            Self::Reverted => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Active => "active",
            Self::Drifted => "drifted",
            Self::Reverted => "reverted",
        }
    }

    fn matches(self, c: &ConfigChangeRecord) -> bool {
        match self {
            Self::All => true,
            Self::Active => c.is_revertable(),
            Self::Drifted => {
                c.is_revertable()
                    && matches!(
                        c.drift_status,
                        Some(DriftStatus::Modified) | Some(DriftStatus::Missing)
                    )
            }
            Self::Reverted => c.kind == ChangeKind::Revert || c.state == ChangeState::Reverted,
        }
    }
}

pub struct ChangesInFlight {
    pub label: String,
    pub cancel_token: String,
}

pub struct ChangesState {
    pub entries: Vec<ConfigChangeRecord>,
    pub selected: usize,
    pub status_filter: ChangeStatusFilter,
    pub detail: Option<ChangeDetail>,
    pub detail_scroll: u16,
    //
    // Conflict returned by the last single revert of the selected change;
    // its diff replaces the detail pane until resolved or dismissed.
    //
    pub conflict: Option<ChangeRevertOutcome>,
    //
    // Engagement label attached to config writes made from the TUI
    // (recon editor). Defaults to $PRAXIS_ENGAGEMENT.
    //
    pub engagement: Option<String>,
    pub engagement_input: Option<String>,
    pub in_flight: Option<ChangesInFlight>,
    pub message: Option<(String, bool)>,
    pub loading: bool,
}

impl Default for ChangesState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            selected: 0,
            status_filter: ChangeStatusFilter::default(),
            detail: None,
            detail_scroll: 0,
            conflict: None,
            engagement: default_engagement(),
            engagement_input: None,
            in_flight: None,
            message: None,
            loading: false,
        }
    }
}

pub enum ChangesEvent {
    Listed(Result<Vec<ConfigChangeRecord>, String>),
    Drift(Result<ChangeStatusResult, String>),
    Detail(Box<Result<ChangeDetail, String>>),
    Reverted(Result<ChangeRevertOutcome, String>),
    RevertedAll(Result<ChangeRevertAllResult, String>),
}

impl App {
    //
    // Indices into `changes.entries` that pass the status filter and the
    // shared Operations filter text.
    //

    pub fn filtered_changes(&self) -> Vec<usize> {
        let state = &self.operations.changes;
        let filter = self.operations.filter.to_lowercase();
        state
            .entries
            .iter()
            .enumerate()
            .filter(|(_, c)| state.status_filter.matches(c))
            .filter(|(_, c)| {
                if filter.is_empty() {
                    return true;
                }
                [
                    Some(c.id.as_str()),
                    Some(c.node_id.as_str()),
                    c.agent_short_name.as_deref(),
                    c.engagement.as_deref(),
                    Some(c.path.as_str()),
                    Some(c.summary.as_str()),
                    Some(c.operator.as_str()),
                    c.note.as_deref(),
                ]
                .into_iter()
                .flatten()
                .any(|v| v.to_lowercase().contains(&filter))
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn selected_change(&self) -> Option<&ConfigChangeRecord> {
        let idx = *self
            .filtered_changes()
            .get(self.operations.changes.selected)?;
        self.operations.changes.entries.get(idx)
    }

    //
    // Reload the ledger; with `check_drift`, also re-read every active
    // change's file on its node (cancellable with ^c).
    //

    pub(crate) fn refresh_changes(&mut self, check_drift: bool) {
        let Some(tx) = self.event_tx.clone() else {
            return;
        };
        let client = self.client.clone();
        self.operations.changes.loading = true;
        let token = uuid::Uuid::new_v4().to_string();
        if check_drift {
            self.operations.changes.in_flight = Some(ChangesInFlight {
                label: "checking drift".to_string(),
                cancel_token: token.clone(),
            });
        }
        tokio::spawn(async move {
            let listed = client
                .service_request(EXT_PRAXIS_CHANGES_LIST, json!({ "limit": 1000 }))
                .await
                .and_then(|v| Ok(serde_json::from_value::<ChangeListResult>(v)?))
                .map(|r| r.changes)
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::Changes(ChangesEvent::Listed(listed)));
            if check_drift {
                let params = ChangeStatusParams {
                    query: ChangeQuery::default(),
                    cancel_token: Some(token),
                };
                let drift = client
                    .service_request(EXT_PRAXIS_CHANGES_STATUS, json!(params))
                    .await
                    .and_then(|v| Ok(serde_json::from_value::<ChangeStatusResult>(v)?))
                    .map_err(|e| e.to_string());
                let _ = tx.send(AppEvent::Changes(ChangesEvent::Drift(drift)));
            }
        });
    }

    fn load_selected_change_detail(&mut self) {
        let Some(id) = self.selected_change().map(|c| c.id.clone()) else {
            self.operations.changes.detail = None;
            return;
        };
        if self
            .operations
            .changes
            .detail
            .as_ref()
            .is_some_and(|d| d.change.id == id)
        {
            return;
        }
        let Some(tx) = self.event_tx.clone() else {
            return;
        };
        let client = self.client.clone();
        tokio::spawn(async move {
            let detail = client
                .service_request(EXT_PRAXIS_CHANGES_GET, json!({ "change_id": id }))
                .await
                .and_then(|v| Ok(serde_json::from_value::<ChangeDetail>(v)?))
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::Changes(ChangesEvent::Detail(Box::new(detail))));
        });
    }

    fn change_selection_moved(&mut self) {
        self.operations.changes.detail_scroll = 0;
        self.operations.changes.conflict = None;
        self.load_selected_change_detail();
    }

    pub(crate) fn changes_select(&mut self, idx: usize) {
        let total = self.filtered_changes().len();
        if idx < total && idx != self.operations.changes.selected {
            self.operations.changes.selected = idx;
            self.change_selection_moved();
        }
    }

    pub(crate) fn changes_move(&mut self, down: bool) {
        let sel = self.operations.changes.selected;
        let next = if down {
            sel.saturating_add(1)
        } else {
            sel.saturating_sub(1)
        };
        self.changes_select(next);
    }

    pub(crate) fn changes_filter_changed(&mut self) {
        self.operations.changes.selected = 0;
        self.change_selection_moved();
    }

    fn set_changes_message(&mut self, text: impl Into<String>, error: bool) {
        self.operations.changes.message = Some((text.into(), error));
    }

    pub(crate) fn confirm_revert_selected_change(&mut self) {
        let Some(c) = self.selected_change() else {
            return;
        };
        if !c.is_revertable() {
            let msg = if c.kind == ChangeKind::Revert {
                "revert records cannot be reverted"
            } else {
                "change is already reverted"
            };
            self.set_changes_message(msg, true);
            return;
        }
        self.confirm = Some(ConfirmAction {
            message: format!(
                "Revert change {} ({} on {})?",
                common::short_id(&c.id),
                c.summary,
                c.path
            ),
            action: ConfirmKind::RevertChange(c.id.clone()),
        });
    }

    pub(crate) fn confirm_revert_visible_changes(&mut self) {
        let ids: Vec<String> = self
            .filtered_changes()
            .into_iter()
            .filter_map(|i| self.operations.changes.entries.get(i))
            .filter(|c| c.is_revertable())
            .map(|c| c.id.clone())
            .collect();
        if ids.is_empty() {
            self.set_changes_message("no active changes in view", true);
            return;
        }
        self.confirm = Some(ConfirmAction {
            message: format!(
                "Revert all {} active change(s) in view, newest first? Conflicting files are skipped.",
                ids.len()
            ),
            action: ConfirmKind::RevertChanges(ids),
        });
    }

    pub(crate) fn start_change_revert(&mut self, change_id: String, policy: ConflictPolicy) {
        let Some(tx) = self.event_tx.clone() else {
            return;
        };
        if self.operations.changes.in_flight.is_some() {
            self.set_changes_message("another ledger operation is running (^c cancels)", true);
            return;
        }
        let token = uuid::Uuid::new_v4().to_string();
        self.operations.changes.in_flight = Some(ChangesInFlight {
            label: format!("reverting {}", common::short_id(&change_id)),
            cancel_token: token.clone(),
        });
        self.operations.changes.conflict = None;
        let client = self.client.clone();
        tokio::spawn(async move {
            let params = ChangeRevertParams {
                change_id,
                on_conflict: policy,
                operator: Some(default_operator("tui")),
                note: None,
                cancel_token: Some(token),
            };
            let result = client
                .service_request(EXT_PRAXIS_CHANGES_REVERT, json!(params))
                .await
                .and_then(|v| Ok(serde_json::from_value::<ChangeRevertOutcome>(v)?))
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::Changes(ChangesEvent::Reverted(result)));
        });
    }

    pub(crate) fn start_change_revert_all(&mut self, ids: Vec<String>) {
        let Some(tx) = self.event_tx.clone() else {
            return;
        };
        if self.operations.changes.in_flight.is_some() {
            self.set_changes_message("another ledger operation is running (^c cancels)", true);
            return;
        }
        let token = uuid::Uuid::new_v4().to_string();
        self.operations.changes.in_flight = Some(ChangesInFlight {
            label: format!("reverting {} change(s)", ids.len()),
            cancel_token: token.clone(),
        });
        let client = self.client.clone();
        tokio::spawn(async move {
            let params = ChangeRevertAllParams {
                query: ChangeQuery {
                    change_ids: ids,
                    ..Default::default()
                },
                all: false,
                on_conflict: ConflictPolicy::Abort,
                operator: Some(default_operator("tui")),
                note: None,
                cancel_token: Some(token),
            };
            let result = client
                .service_request(EXT_PRAXIS_CHANGES_REVERT_ALL, json!(params))
                .await
                .and_then(|v| Ok(serde_json::from_value::<ChangeRevertAllResult>(v)?))
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::Changes(ChangesEvent::RevertedAll(result)));
        });
    }

    //
    // Ask the service to stop the in-flight ledger request. The pending
    // response still arrives (marked cancelled) and clears the state.
    //

    pub(crate) fn cancel_change_op(&mut self) {
        let Some(in_flight) = self.operations.changes.in_flight.as_ref() else {
            return;
        };
        let token = in_flight.cancel_token.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let _ = client
                .service_notification(EXT_PRAXIS_CHANGES_CANCEL, json!({ "cancel_token": token }))
                .await;
        });
        self.set_changes_message("cancelling\u{2026}", false);
    }

    //
    // Keys specific to the Changes tab. Returns true when consumed so the
    // generic Operations handler is skipped.
    //

    pub(crate) fn handle_changes_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if let Some(input) = self.operations.changes.engagement_input.as_mut() {
            match key.code {
                KeyCode::Enter => {
                    let value = input.trim().to_string();
                    self.operations.changes.engagement = (!value.is_empty()).then_some(value);
                    self.operations.changes.engagement_input = None;
                }
                KeyCode::Esc => self.operations.changes.engagement_input = None,
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) if !ctrl => input.push(c),
                _ => {}
            }
            return true;
        }

        if self.operations.changes.conflict.is_some() {
            match key.code {
                KeyCode::Char('u') if !ctrl => {
                    let c = self.operations.changes.conflict.as_ref().unwrap();
                    if c.structured_undo_available {
                        let id = c.change_id.clone();
                        self.start_change_revert(id, ConflictPolicy::Structured);
                    } else {
                        self.set_changes_message(
                            "no structured undo for this change; use f to force",
                            true,
                        );
                    }
                    return true;
                }
                KeyCode::Char('f') if !ctrl => {
                    let id = self
                        .operations
                        .changes
                        .conflict
                        .as_ref()
                        .unwrap()
                        .change_id
                        .clone();
                    self.start_change_revert(id, ConflictPolicy::Force);
                    return true;
                }
                KeyCode::Esc => {
                    self.operations.changes.conflict = None;
                    return true;
                }
                _ => {}
            }
        }

        if self.operations.filter_focused {
            return false;
        }

        match key.code {
            KeyCode::Char('c') if ctrl => {
                self.cancel_change_op();
                true
            }
            KeyCode::Char('z') if ctrl => {
                self.confirm_revert_selected_change();
                true
            }
            KeyCode::Char('x') if ctrl => {
                self.confirm_revert_visible_changes();
                true
            }
            KeyCode::Char('r') if !ctrl => {
                self.refresh_changes(true);
                true
            }
            KeyCode::Char('s') if !ctrl => {
                self.operations.changes.status_filter =
                    self.operations.changes.status_filter.next();
                self.changes_filter_changed();
                true
            }
            KeyCode::Char('e') if !ctrl => {
                self.operations.changes.engagement_input = Some(
                    self.operations
                        .changes
                        .engagement
                        .clone()
                        .unwrap_or_default(),
                );
                true
            }
            KeyCode::Up | KeyCode::Down if self.operations.detail_focus => {
                let s = &mut self.operations.changes.detail_scroll;
                *s = if key.code == KeyCode::Up {
                    s.saturating_sub(1)
                } else {
                    s.saturating_add(1)
                };
                true
            }
            KeyCode::PageUp | KeyCode::PageDown if self.operations.detail_focus => {
                let s = &mut self.operations.changes.detail_scroll;
                *s = if key.code == KeyCode::PageUp {
                    s.saturating_sub(10)
                } else {
                    s.saturating_add(10)
                };
                true
            }
            KeyCode::Up | KeyCode::Down => {
                self.changes_move(key.code == KeyCode::Down);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn handle_changes_event(&mut self, event: ChangesEvent) {
        let state = &mut self.operations.changes;
        match event {
            ChangesEvent::Listed(Ok(entries)) => {
                //
                // Keep the selection on the same change across reloads.
                //
                let selected_id = self.selected_change().map(|c| c.id.clone());
                let state = &mut self.operations.changes;
                state.entries = entries;
                state.loading = false;
                if state.in_flight.is_none() {
                    state.message = None;
                }
                let filtered = self.filtered_changes();
                let new_idx = selected_id
                    .and_then(|id| {
                        filtered
                            .iter()
                            .position(|&i| self.operations.changes.entries[i].id == id)
                    })
                    .unwrap_or(0);
                self.operations.changes.selected = new_idx.min(filtered.len().saturating_sub(1));
                self.operations.changes.detail = None;
                self.load_selected_change_detail();
            }
            ChangesEvent::Listed(Err(e)) => {
                state.loading = false;
                state.message = Some((format!("load failed: {}", e), true));
            }
            ChangesEvent::Drift(result) => {
                state.in_flight = None;
                match result {
                    Ok(r) => {
                        for entry in &r.entries {
                            if let Some(c) =
                                state.entries.iter_mut().find(|c| c.id == entry.change_id)
                            {
                                c.drift_status = Some(entry.status);
                            }
                        }
                        let drifted = r
                            .entries
                            .iter()
                            .filter(|e| {
                                matches!(e.status, DriftStatus::Modified | DriftStatus::Missing)
                            })
                            .count();
                        let unreachable = r
                            .entries
                            .iter()
                            .filter(|e| e.status == DriftStatus::Unreachable)
                            .count();
                        state.message = Some((
                            if r.cancelled {
                                "drift check cancelled".to_string()
                            } else {
                                format!(
                                    "{} active change(s) checked: {} drifted, {} unreachable",
                                    r.entries.len(),
                                    drifted,
                                    unreachable
                                )
                            },
                            drifted > 0,
                        ));
                    }
                    Err(e) => state.message = Some((format!("drift check failed: {}", e), true)),
                }
            }
            ChangesEvent::Detail(detail) => match *detail {
                Ok(detail) => {
                    if self
                        .selected_change()
                        .is_some_and(|c| c.id == detail.change.id)
                    {
                        self.operations.changes.detail = Some(detail);
                    }
                }
                Err(e) => {
                    self.operations.changes.message = Some((format!("detail failed: {}", e), true));
                }
            },
            ChangesEvent::Reverted(result) => {
                state.in_flight = None;
                match result {
                    Ok(o) if o.outcome == common::config_change::RevertOutcomeKind::Conflict => {
                        state.message = Some((
                            format!("{}: {}", common::short_id(&o.change_id), o.message),
                            true,
                        ));
                        state.detail_scroll = 0;
                        state.conflict = Some(o);
                    }
                    Ok(o) => {
                        state.message = Some((
                            format!(
                                "{} {}: {}",
                                common::short_id(&o.change_id),
                                o.outcome.as_str(),
                                o.message
                            ),
                            !o.outcome.is_success(),
                        ));
                        self.refresh_changes(false);
                    }
                    Err(e) => state.message = Some((format!("revert failed: {}", e), true)),
                }
            }
            ChangesEvent::RevertedAll(result) => {
                state.in_flight = None;
                match result {
                    Ok(r) => {
                        let ok = r.outcomes.iter().filter(|o| o.outcome.is_success()).count();
                        let conflicts = r
                            .outcomes
                            .iter()
                            .filter(|o| {
                                o.outcome == common::config_change::RevertOutcomeKind::Conflict
                            })
                            .count();
                        state.message = Some((
                            format!(
                                "{}reverted {} of {}; {} conflict(s){}",
                                if r.cancelled { "cancelled: " } else { "" },
                                ok,
                                r.outcomes.len(),
                                conflicts,
                                if conflicts > 0 {
                                    " (select one and ^z to resolve)"
                                } else {
                                    ""
                                }
                            ),
                            ok < r.outcomes.len(),
                        ));
                        self.refresh_changes(false);
                    }
                    Err(e) => state.message = Some((format!("revert all failed: {}", e), true)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::config_change::ConfigFileKind;

    fn record(
        kind: ChangeKind,
        state: ChangeState,
        drift: Option<DriftStatus>,
    ) -> ConfigChangeRecord {
        ConfigChangeRecord {
            id: "id".into(),
            created_at: Utc::now(),
            kind,
            state,
            node_id: "n".into(),
            agent_short_name: None,
            path: "/p".into(),
            file_kind: ConfigFileKind::Json,
            config_type: None,
            op: None,
            summary: "set /x".into(),
            operator: "o".into(),
            engagement: None,
            note: None,
            before_exists: true,
            before_sha256: None,
            before_content: None,
            after_exists: true,
            after_sha256: None,
            after_content: None,
            undo: None,
            reverts_change_id: None,
            reverted_by: None,
            reverted_at: None,
            drift_status: drift,
            drift_checked_at: None,
        }
    }

    #[test]
    fn status_filter_buckets() {
        let active = record(
            ChangeKind::Apply,
            ChangeState::Active,
            Some(DriftStatus::InSync),
        );
        let drifted = record(
            ChangeKind::Apply,
            ChangeState::Active,
            Some(DriftStatus::Modified),
        );
        let reverted = record(ChangeKind::Apply, ChangeState::Reverted, None);
        let revert_row = record(ChangeKind::Revert, ChangeState::Active, None);

        let f = ChangeStatusFilter::Active;
        assert!(f.matches(&active) && f.matches(&drifted));
        assert!(!f.matches(&reverted) && !f.matches(&revert_row));

        let f = ChangeStatusFilter::Drifted;
        assert!(f.matches(&drifted) && !f.matches(&active));

        let f = ChangeStatusFilter::Reverted;
        assert!(f.matches(&reverted) && f.matches(&revert_row) && !f.matches(&active));

        assert_eq!(ChangeStatusFilter::Reverted.next(), ChangeStatusFilter::All);
    }
}
