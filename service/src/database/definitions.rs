use anyhow::Result;
use chrono::{DateTime, Utc};
use common::{OperationDefinitionInfo, SemanticOperationSpec};
use sqlx::Row;

use super::{Database, DatabasePool, MAX_OPERATION_DEFINITIONS};

/// Database record for an operation definition
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OperationDefinition {
    /// Full name: category::short_name (primary key)
    pub full_name: String,
    /// Category (e.g., "recon", "exfiltration")
    pub category: String,
    /// Short name within the category
    pub short_name: String,
    /// Display name
    pub name: String,
    pub description: String,
    /// Information for semantic agents to enrich their understanding
    pub agent_info: String,
    /// Timeout in seconds
    pub timeout: u64,
    /// The prompt to run for this operation
    pub operation_prompt: String,
    /// Execution mode: "one-shot" or "agent"
    pub mode: String,
    /// Maximum iterations for agent mode
    pub agent_iterations: u32,
    /// DEPRECATED: List of operations to run before this one - use chains instead
    #[serde(default)]
    pub operation_chain: Vec<String>,
    /// Whether this operation is disabled
    pub disabled: bool,
    /// Whether to run the agent session in YOLO mode (auto-approve actions)
    pub yolo_mode: bool,
    /// Optional model override (format: "provider::model")
    #[serde(default)]
    pub model_ref: Option<String>,
    /// When the definition was created
    pub created_at: DateTime<Utc>,
    /// When the definition was last updated
    pub updated_at: DateTime<Utc>,
}

impl OperationDefinition {
    /// Convert to OperationDefinitionInfo for sending to clients
    pub fn to_info(&self) -> OperationDefinitionInfo {
        OperationDefinitionInfo {
            full_name: self.full_name.clone(),
            category: self.category.clone(),
            short_name: self.short_name.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            agent_info: self.agent_info.clone(),
            timeout: self.timeout,
            operation_prompt: self.operation_prompt.clone(),
            mode: self.mode.clone(),
            agent_iterations: self.agent_iterations,
            //
            // DEPRECATED: operation_chain is no longer used - use chains
            // instead.
            //
            operation_chain: vec![],
            disabled: self.disabled,
            yolo_mode: self.yolo_mode,
            model_ref: self.model_ref.clone(),
        }
    }

    pub fn from_json(json_content: &str) -> Result<Self, String> {
        #[derive(serde::Deserialize)]
        struct JsonOp {
            #[serde(default)]
            item_type: Option<String>,
            name: String,
            #[serde(default)]
            short_name: Option<String>,
            #[serde(default)]
            category: Option<String>,
            description: String,
            agent_info: String,
            #[serde(default = "default_timeout")]
            timeout: u64,
            operation_prompt: String,
            #[serde(default = "default_mode")]
            mode: String,
            #[serde(default = "default_agent_iterations")]
            agent_iterations: u32,
            #[serde(default)]
            disabled: bool,
            #[serde(default)]
            yolo_mode: bool,
            #[serde(default)]
            model_ref: Option<String>,
        }

        fn default_timeout() -> u64 {
            60
        }
        fn default_mode() -> String {
            "one-shot".to_string()
        }
        fn default_agent_iterations() -> u32 {
            5
        }

        let parsed: JsonOp = serde_json::from_str(json_content)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;

        //
        // Validate item_type if present.
        //
        if let Some(ref item_type) = parsed.item_type {
            if item_type != "operation" {
                return Err(format!(
                    "Invalid item_type '{}'. Expected 'operation' for operation definitions.",
                    item_type
                ));
            }
        }

        let category = parsed
            .category
            .ok_or_else(|| "JSON must contain 'category' field".to_string())?;
        let short_name = parsed
            .short_name
            .ok_or_else(|| "JSON must contain 'short_name' field".to_string())?;

        let full_name = format!("{}::{}", category, short_name);
        let now = Utc::now();

        Ok(OperationDefinition {
            full_name,
            category,
            short_name,
            name: parsed.name,
            description: parsed.description,
            agent_info: parsed.agent_info,
            timeout: parsed.timeout,
            operation_prompt: parsed.operation_prompt,
            mode: parsed.mode,
            agent_iterations: parsed.agent_iterations,
            operation_chain: vec![],
            disabled: parsed.disabled,
            yolo_mode: parsed.yolo_mode,
            model_ref: parsed.model_ref,
            created_at: now,
            updated_at: now,
        })
    }

    //
    // Export to JSON format (includes item_type for import detection).
    //

    #[allow(dead_code)]
    pub fn to_json(&self) -> String {
        #[derive(serde::Serialize)]
        struct JsonExport {
            item_type: &'static str,
            name: String,
            short_name: String,
            category: String,
            description: String,
            agent_info: String,
            timeout: u64,
            operation_prompt: String,
            mode: String,
            agent_iterations: u32,
            disabled: bool,
            yolo_mode: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            model_ref: Option<String>,
        }

        let export = JsonExport {
            item_type: "operation",
            name: self.name.clone(),
            short_name: self.short_name.clone(),
            category: self.category.clone(),
            description: self.description.clone(),
            agent_info: self.agent_info.clone(),
            timeout: self.timeout,
            operation_prompt: self.operation_prompt.clone(),
            mode: self.mode.clone(),
            agent_iterations: self.agent_iterations,
            disabled: self.disabled,
            yolo_mode: self.yolo_mode,
            model_ref: self.model_ref.clone(),
        };

        serde_json::to_string_pretty(&export).unwrap_or_default()
    }

    /// Convert to SemanticOperationSpec for running the operation
    pub fn to_spec(&self) -> SemanticOperationSpec {
        SemanticOperationSpec {
            name: self.name.clone(),
            description: self.description.clone(),
            agent_info: self.agent_info.clone(),
            timeout: self.timeout,
            operation_prompt: self.operation_prompt.clone(),
            mode: self.mode.clone(),
            agent_iterations: self.agent_iterations,
            yolo_mode: self.yolo_mode,
            model_ref: self.model_ref.clone(),
        }
    }
}

impl Database {
    /// Insert or update an operation definition
    pub async fn upsert_operation_definition(
        &self,
        definition: &OperationDefinition,
    ) -> Result<()> {
        let sql = "INSERT INTO operation_definitions (full_name, category, short_name, name, description, agent_info, timeout, operation_prompt, mode, agent_iterations, operation_chain, disabled, yolo_mode, model_ref, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
             ON CONFLICT(full_name) DO UPDATE SET
                 category = excluded.category,
                 short_name = excluded.short_name,
                 name = excluded.name,
                 description = excluded.description,
                 agent_info = excluded.agent_info,
                 timeout = excluded.timeout,
                 operation_prompt = excluded.operation_prompt,
                 mode = excluded.mode,
                 agent_iterations = excluded.agent_iterations,
                 operation_chain = excluded.operation_chain,
                 disabled = excluded.disabled,
                 yolo_mode = excluded.yolo_mode,
                 model_ref = excluded.model_ref,
                 updated_at = excluded.updated_at";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&definition.full_name)
                    .bind(&definition.category)
                    .bind(&definition.short_name)
                    .bind(&definition.name)
                    .bind(&definition.description)
                    .bind(&definition.agent_info)
                    .bind(definition.timeout as i64)
                    .bind(&definition.operation_prompt)
                    .bind(&definition.mode)
                    .bind(definition.agent_iterations as i64)
                    .bind("[]") // DEPRECATED: operation_chain is always empty now
                    .bind(definition.disabled)
                    .bind(definition.yolo_mode)
                    .bind(&definition.model_ref)
                    .bind(definition.created_at.to_rfc3339())
                    .bind(definition.updated_at.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(&definition.full_name)
                    .bind(&definition.category)
                    .bind(&definition.short_name)
                    .bind(&definition.name)
                    .bind(&definition.description)
                    .bind(&definition.agent_info)
                    .bind(definition.timeout as i64)
                    .bind(&definition.operation_prompt)
                    .bind(&definition.mode)
                    .bind(definition.agent_iterations as i64)
                    .bind("[]") // DEPRECATED: operation_chain is always empty now
                    .bind(if definition.disabled { 1i16 } else { 0i16 })
                    .bind(if definition.yolo_mode { 1i16 } else { 0i16 })
                    .bind(&definition.model_ref)
                    .bind(definition.created_at.to_rfc3339())
                    .bind(definition.updated_at.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
        }

        //
        // Auto-prune old definitions.
        //
        self.prune_old_definitions().await?;

        Ok(())
    }

    /// Get an operation definition by full_name
    pub async fn get_operation_definition(
        &self,
        full_name: &str,
    ) -> Result<Option<OperationDefinition>> {
        let sql = "SELECT full_name, category, short_name, name, description, agent_info, timeout, operation_prompt, mode, agent_iterations, operation_chain, disabled, yolo_mode, model_ref, created_at, updated_at
             FROM operation_definitions WHERE full_name = $1";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query(sql)
                    .bind(full_name)
                    .fetch_optional(pool)
                    .await?;
                match row {
                    Some(row) => Ok(Some(parse_definition_row_sqlite(&row)?)),
                    None => Ok(None),
                }
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(sql)
                    .bind(full_name)
                    .fetch_optional(pool)
                    .await?;
                match row {
                    Some(row) => Ok(Some(parse_definition_row_postgres(&row)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// List all operation definitions
    pub async fn list_operation_definitions(&self) -> Result<Vec<OperationDefinition>> {
        let sql = "SELECT full_name, category, short_name, name, description, agent_info, timeout, operation_prompt, mode, agent_iterations, operation_chain, disabled, yolo_mode, model_ref, created_at, updated_at
             FROM operation_definitions ORDER BY category, short_name";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                let mut definitions = Vec::new();
                for row in rows {
                    definitions.push(parse_definition_row_sqlite(&row)?);
                }
                Ok(definitions)
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                let mut definitions = Vec::new();
                for row in rows {
                    definitions.push(parse_definition_row_postgres(&row)?);
                }
                Ok(definitions)
            }
        }
    }

    /// List operation definitions by category
    #[allow(dead_code)]
    pub async fn list_operation_definitions_by_category(
        &self,
        category: &str,
    ) -> Result<Vec<OperationDefinition>> {
        let sql = "SELECT full_name, category, short_name, name, description, agent_info, timeout, operation_prompt, mode, agent_iterations, operation_chain, disabled, yolo_mode, model_ref, created_at, updated_at
             FROM operation_definitions WHERE category = $1 ORDER BY short_name";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).bind(category).fetch_all(pool).await?;
                let mut definitions = Vec::new();
                for row in rows {
                    definitions.push(parse_definition_row_sqlite(&row)?);
                }
                Ok(definitions)
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).bind(category).fetch_all(pool).await?;
                let mut definitions = Vec::new();
                for row in rows {
                    definitions.push(parse_definition_row_postgres(&row)?);
                }
                Ok(definitions)
            }
        }
    }

    /// Delete an operation definition by full_name
    pub async fn delete_operation_definition(&self, full_name: &str) -> Result<bool> {
        let sql = "DELETE FROM operation_definitions WHERE full_name = $1";

        let count = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query(sql)
                .bind(full_name)
                .execute(pool)
                .await?
                .rows_affected(),
            DatabasePool::Postgres(pool) => sqlx::query(sql)
                .bind(full_name)
                .execute(pool)
                .await?
                .rows_affected(),
        };

        Ok(count > 0)
    }

    /// Set the disabled flag on an operation definition
    pub async fn set_operation_definition_disabled(
        &self,
        full_name: &str,
        disabled: bool,
    ) -> Result<bool> {
        let sql =
            "UPDATE operation_definitions SET disabled = $1, updated_at = $2 WHERE full_name = $3";
        let now = chrono::Utc::now().to_rfc3339();

        let count = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query(sql)
                .bind(if disabled { 1i16 } else { 0i16 })
                .bind(&now)
                .bind(full_name)
                .execute(pool)
                .await?
                .rows_affected(),
            DatabasePool::Postgres(pool) => sqlx::query(sql)
                .bind(disabled)
                .bind(&now)
                .bind(full_name)
                .execute(pool)
                .await?
                .rows_affected(),
        };

        Ok(count > 0)
    }

    /// Count operation definitions
    pub async fn count_operation_definitions(&self) -> Result<usize> {
        let sql = "SELECT COUNT(*) FROM operation_definitions";

        let count: i64 = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query(sql).fetch_one(pool).await?;
                row.get(0)
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(sql).fetch_one(pool).await?;
                row.get(0)
            }
        };

        Ok(count as usize)
    }

    /// Prune old operation definitions (keep only MAX_OPERATION_DEFINITIONS)
    async fn prune_old_definitions(&self) -> Result<usize> {
        let count = self.count_operation_definitions().await?;

        if count <= MAX_OPERATION_DEFINITIONS {
            return Ok(0);
        }

        let to_delete = count - MAX_OPERATION_DEFINITIONS;

        let sql = "DELETE FROM operation_definitions WHERE full_name IN (
                SELECT full_name FROM operation_definitions
                ORDER BY updated_at ASC LIMIT $1
            )";

        let deleted = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query(sql)
                .bind(to_delete as i64)
                .execute(pool)
                .await?
                .rows_affected(),
            DatabasePool::Postgres(pool) => sqlx::query(sql)
                .bind(to_delete as i64)
                .execute(pool)
                .await?
                .rows_affected(),
        };

        Ok(deleted as usize)
    }
}

//
// Helper functions.
//

fn parse_definition_row_sqlite(row: &sqlx::sqlite::SqliteRow) -> Result<OperationDefinition> {
    let full_name: String = row.get(0);
    let category: String = row.get(1);
    let short_name: String = row.get(2);
    let name: String = row.get(3);
    let description: String = row.get(4);
    let agent_info: String = row.get(5);
    let timeout: i64 = row.get(6);
    let operation_prompt: String = row.get(7);
    let mode: String = row.get(8);
    let agent_iterations: i64 = row.get(9);
    let _operation_chain_json: String = row.get(10); // DEPRECATED: ignored
    let disabled: bool = row.get(11);
    let yolo_mode: bool = row.get(12);
    let model_ref: Option<String> = row.get(13);
    let created_at_str: String = row.get(14);
    let updated_at_str: String = row.get(15);

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)?.with_timezone(&Utc);

    Ok(OperationDefinition {
        full_name,
        category,
        short_name,
        name,
        description,
        agent_info,
        timeout: timeout as u64,
        operation_prompt,
        mode,
        agent_iterations: agent_iterations as u32,
        //
        // DEPRECATED: always empty.
        //
        operation_chain: vec![],
        disabled,
        yolo_mode,
        model_ref,
        created_at,
        updated_at,
    })
}

fn parse_definition_row_postgres(row: &sqlx::postgres::PgRow) -> Result<OperationDefinition> {
    let full_name: String = row.get(0);
    let category: String = row.get(1);
    let short_name: String = row.get(2);
    let name: String = row.get(3);
    let description: String = row.get(4);
    let agent_info: String = row.get(5);
    let timeout: i64 = row.get(6);
    let operation_prompt: String = row.get(7);
    let mode: String = row.get(8);
    let agent_iterations: i64 = row.get(9);
    let _operation_chain_json: String = row.get(10); // DEPRECATED: ignored
    let disabled: i16 = row.get(11);
    let yolo_mode: i16 = row.get(12);
    let model_ref: Option<String> = row.get(13);
    let created_at_str: String = row.get(14);
    let updated_at_str: String = row.get(15);

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)?.with_timezone(&Utc);

    Ok(OperationDefinition {
        full_name,
        category,
        short_name,
        name,
        description,
        agent_info,
        timeout: timeout as u64,
        operation_prompt,
        mode,
        agent_iterations: agent_iterations as u32,
        //
        // DEPRECATED: always empty.
        //
        operation_chain: vec![],
        disabled: disabled != 0,
        yolo_mode: yolo_mode != 0,
        model_ref,
        created_at,
        updated_at,
    })
}
