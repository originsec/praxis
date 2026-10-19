use anyhow::{Result, anyhow};
use chrono::{DateTime, SecondsFormat, Utc};
use common::config_change::{
    ChangeKind, ChangeQuery, ChangeState, ConfigChangeRecord, ConfigFileKind, DriftStatus,
};

use super::Database;
use super::exec::{Arg, DbRow, db_args};

//
// Persistence for the agent configuration change ledger. Each row is one
// write Praxis made to a config file on a node (kind = apply) or one
// rollback of such a write (kind = revert). Before/after content is kept so
// changes can be restored and diffed long after the fact.
//

const LIST_COLUMNS: &str = "id, created_at, kind, state, node_id, agent_short_name, path,
    file_kind, config_type, op_json, summary, operator, engagement, note,
    before_exists, before_sha256, after_exists, after_sha256, undo_json,
    reverts_change_id, reverted_by, reverted_at, drift_status, drift_checked_at";

const DEFAULT_LIST_LIMIT: usize = 500;

//
// Timestamps are stored with fixed microsecond precision so lexical order
// in TEXT columns matches chronological order on both backends.
//

fn ts(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

impl Database {
    pub async fn insert_config_change(&self, r: &ConfigChangeRecord) -> Result<()> {
        let op_json = r.op.as_ref().map(serde_json::to_string).transpose()?;
        let undo_json = r.undo.as_ref().map(serde_json::to_string).transpose()?;
        let sql = "INSERT INTO config_changes (
                id, created_at, kind, state, node_id, agent_short_name, path,
                file_kind, config_type, op_json, summary, operator, engagement, note,
                before_exists, before_sha256, before_content,
                after_exists, after_sha256, after_content, undo_json,
                reverts_change_id, reverted_by, reverted_at, drift_status, drift_checked_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
                $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26)";

        self.db_execute(
            sql,
            db_args![
                &r.id,
                ts(&r.created_at),
                r.kind.as_str(),
                r.state.as_str(),
                &r.node_id,
                r.agent_short_name.as_deref(),
                &r.path,
                r.file_kind.as_str(),
                r.config_type.as_deref(),
                op_json,
                &r.summary,
                &r.operator,
                r.engagement.as_deref(),
                r.note.as_deref(),
                r.before_exists,
                r.before_sha256.as_deref(),
                r.before_content.as_deref(),
                r.after_exists,
                r.after_sha256.as_deref(),
                r.after_content.as_deref(),
                undo_json,
                r.reverts_change_id.as_deref(),
                r.reverted_by.as_deref(),
                r.reverted_at.as_ref().map(ts),
                r.drift_status.map(|d| d.as_str()),
                r.drift_checked_at.as_ref().map(ts),
            ],
        )
        .await?;
        Ok(())
    }

    //
    // Full record including before/after content.
    //

    pub async fn get_config_change(&self, id: &str) -> Result<Option<ConfigChangeRecord>> {
        let sql = format!(
            "SELECT {}, before_content, after_content FROM config_changes WHERE id = $1",
            LIST_COLUMNS
        );
        let row = self.db_fetch_optional(&sql, db_args![id]).await?;
        row.map(|r| parse_row(&r, true)).transpose()
    }

    //
    // Resolve a full id or unique prefix (as shown by short ids in the UI).
    //

    pub async fn resolve_config_change_id(&self, id_or_prefix: &str) -> Result<String> {
        let prefix = id_or_prefix.trim().to_lowercase();
        if prefix.is_empty() {
            return Err(anyhow!("change id is empty"));
        }
        let pattern = format!("{}%", prefix.replace(['%', '_'], ""));
        let rows = self
            .db_fetch_all(
                "SELECT id FROM config_changes WHERE id LIKE $1 ORDER BY created_at DESC LIMIT 3",
                db_args![pattern],
            )
            .await?;
        let ids: Vec<String> = rows.iter().map(|r| r.get::<String, _>(0)).collect();
        if ids.contains(&prefix) {
            return Ok(prefix);
        }
        match ids.as_slice() {
            [] => Err(anyhow!("no change matches '{}'", id_or_prefix)),
            [only] => Ok(only.clone()),
            _ => Err(anyhow!("change id '{}' is ambiguous", id_or_prefix)),
        }
    }

    //
    // List changes newest first. Content columns are only loaded when
    // `with_content` is set (they can be large).
    //

    pub async fn list_config_changes(
        &self,
        query: &ChangeQuery,
        with_content: bool,
    ) -> Result<Vec<ConfigChangeRecord>> {
        let mut clauses: Vec<String> = Vec::new();
        let mut args: Vec<Arg> = Vec::new();
        let mut push = |clause: &str, value: String, args: &mut Vec<Arg>| {
            args.push(Arg::Str(value));
            clauses.push(format!("{} ${}", clause, args.len()));
        };

        if let Some(v) = &query.node_id {
            push("node_id =", v.clone(), &mut args);
        }
        if let Some(v) = &query.agent_short_name {
            push("agent_short_name =", v.clone(), &mut args);
        }
        if let Some(v) = &query.engagement {
            push("engagement =", v.clone(), &mut args);
        }
        if let Some(v) = &query.path {
            push("path =", v.clone(), &mut args);
        }
        if let Some(v) = query.state {
            push("state =", v.as_str().to_string(), &mut args);
        }
        if let Some(v) = query.kind {
            push("kind =", v.as_str().to_string(), &mut args);
        }
        if !query.change_ids.is_empty() {
            let start = args.len();
            let placeholders: Vec<String> = query
                .change_ids
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    args.push(Arg::Str(id.clone()));
                    format!("${}", start + i + 1)
                })
                .collect();
            clauses.push(format!("id IN ({})", placeholders.join(", ")));
        }

        let where_clause = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        let content_cols = if with_content {
            ", before_content, after_content"
        } else {
            ""
        };
        let limit = query.limit.unwrap_or(DEFAULT_LIST_LIMIT).max(1) as i64;
        args.push(Arg::I64(limit));
        let sql = format!(
            "SELECT {}{} FROM config_changes {} ORDER BY created_at DESC LIMIT ${}",
            LIST_COLUMNS,
            content_cols,
            where_clause,
            args.len()
        );

        let rows = self.db_fetch_all(&sql, args).await?;
        rows.iter().map(|r| parse_row(r, with_content)).collect()
    }

    //
    // Flip an active change to reverted. Returns false if it was not active
    // (already reverted concurrently), which callers treat as a conflict.
    //

    pub async fn mark_config_change_reverted(
        &self,
        id: &str,
        reverted_by: &str,
        reverted_at: &DateTime<Utc>,
    ) -> Result<bool> {
        let affected = self
            .db_execute(
                "UPDATE config_changes SET state = 'reverted', reverted_by = $2, reverted_at = $3
                 WHERE id = $1 AND state = 'active'",
                db_args![id, reverted_by, ts(reverted_at)],
            )
            .await?;
        Ok(affected == 1)
    }

    pub async fn update_config_change_drift(
        &self,
        id: &str,
        status: DriftStatus,
        checked_at: &DateTime<Utc>,
    ) -> Result<()> {
        self.db_execute(
            "UPDATE config_changes SET drift_status = $2, drift_checked_at = $3 WHERE id = $1",
            db_args![id, status.as_str(), ts(checked_at)],
        )
        .await?;
        Ok(())
    }
}

fn parse_enum<T: serde::de::DeserializeOwned>(raw: &str, what: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(raw.to_string()))
        .map_err(|_| anyhow!("invalid {} '{}' in config_changes", what, raw))
}

fn opt_ts(row: &DbRow, idx: usize) -> Result<Option<DateTime<Utc>>> {
    let raw: Option<String> = row.get(idx);
    raw.map(|s| Ok(DateTime::parse_from_rfc3339(&s)?.with_timezone(&Utc)))
        .transpose()
}

fn parse_row(row: &DbRow, with_content: bool) -> Result<ConfigChangeRecord> {
    let kind: String = row.get(2);
    let state: String = row.get(3);
    let file_kind: String = row.get(7);
    let op_json: Option<String> = row.get(9);
    let undo_json: Option<String> = row.get(18);
    let drift: Option<String> = row.get(22);

    Ok(ConfigChangeRecord {
        id: row.get(0),
        created_at: row.get_timestamp(1)?,
        kind: parse_enum::<ChangeKind>(&kind, "kind")?,
        state: parse_enum::<ChangeState>(&state, "state")?,
        node_id: row.get(4),
        agent_short_name: row.get(5),
        path: row.get(6),
        file_kind: ConfigFileKind::parse(&file_kind).unwrap_or(ConfigFileKind::Text),
        config_type: row.get(8),
        op: op_json.and_then(|j| serde_json::from_str(&j).ok()),
        summary: row.get(10),
        operator: row.get(11),
        engagement: row.get(12),
        note: row.get(13),
        before_exists: row.get_bool(14),
        before_sha256: row.get(15),
        after_exists: row.get_bool(16),
        after_sha256: row.get(17),
        undo: undo_json.and_then(|j| serde_json::from_str(&j).ok()),
        reverts_change_id: row.get(19),
        reverted_by: row.get(20),
        reverted_at: opt_ts(row, 21)?,
        drift_status: drift.as_deref().and_then(DriftStatus::parse),
        drift_checked_at: opt_ts(row, 23)?,
        before_content: if with_content { row.get(24) } else { None },
        after_content: if with_content { row.get(25) } else { None },
    })
}
