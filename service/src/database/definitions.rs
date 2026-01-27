use chrono::{DateTime, Utc};
use common::{SemanticOperationSpec, OperationDefinitionInfo};
use rusqlite::{params, Result as SqliteResult};

use super::{Database, MAX_OPERATION_DEFINITIONS};

/// Database record for an operation definition (parsed from YAML)
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
    /// Description
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

    /// Parse from YAML content
    pub fn from_yaml(yaml_content: &str) -> Result<Self, String> {
        #[derive(serde::Deserialize)]
        struct YamlOp {
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
            /// DEPRECATED: ignored, use chains instead
            #[serde(default)]
            operation_chain: Vec<String>,
            #[serde(default)]
            disabled: bool,
            #[serde(default)]
            yolo: bool,
            /// Optional model override (format: "provider::model")
            #[serde(default)]
            model_ref: Option<String>,
        }

        fn default_timeout() -> u64 { 60 }
        fn default_mode() -> String { "one-shot".to_string() }
        fn default_agent_iterations() -> u32 { 5 }

        let parsed: YamlOp = serde_yaml::from_str(yaml_content)
            .map_err(|e| format!("Failed to parse YAML: {}", e))?;

        //
        // Warn if deprecated operation_chain is used.
        //
        if !parsed.operation_chain.is_empty() {
            eprintln!("Warning: 'operation_chain' is deprecated and will be ignored. Use chains instead.");
        }

        let category = parsed.category
            .ok_or_else(|| "YAML must contain 'category' field".to_string())?;
        let short_name = parsed.short_name
            .ok_or_else(|| "YAML must contain 'short_name' field".to_string())?;

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
            //
            // DEPRECATED: always empty now, chains should be used instead.
            //
            operation_chain: vec![],
            disabled: parsed.disabled,
            yolo_mode: parsed.yolo,
            model_ref: parsed.model_ref,
            created_at: now,
            updated_at: now,
        })
    }

    //
    // Parse from JSON content.
    //

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

        fn default_timeout() -> u64 { 60 }
        fn default_mode() -> String { "one-shot".to_string() }
        fn default_agent_iterations() -> u32 { 5 }

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

        let category = parsed.category
            .ok_or_else(|| "JSON must contain 'category' field".to_string())?;
        let short_name = parsed.short_name
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
    pub fn upsert_operation_definition(&self, definition: &OperationDefinition) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        conn.execute(
            "INSERT INTO operation_definitions (full_name, category, short_name, name, description, agent_info, timeout, operation_prompt, mode, agent_iterations, operation_chain, disabled, yolo_mode, model_ref, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
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
                 updated_at = excluded.updated_at",
            params![
                definition.full_name,
                definition.category,
                definition.short_name,
                definition.name,
                definition.description,
                definition.agent_info,
                definition.timeout as i64,
                definition.operation_prompt,
                definition.mode,
                definition.agent_iterations as i64,
                //
                // DEPRECATED: operation_chain is always empty now.
                //
                "[]",
                if definition.disabled { 1 } else { 0 },
                if definition.yolo_mode { 1 } else { 0 },
                definition.model_ref,
                definition.created_at.to_rfc3339(),
                definition.updated_at.to_rfc3339(),
            ],
        )?;

        drop(conn);
        self.prune_old_definitions()?;

        Ok(())
    }

    /// Get an operation definition by full_name
    pub fn get_operation_definition(&self, full_name: &str) -> SqliteResult<Option<OperationDefinition>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT full_name, category, short_name, name, description, agent_info, timeout, operation_prompt, mode, agent_iterations, operation_chain, disabled, yolo_mode, model_ref, created_at, updated_at
             FROM operation_definitions WHERE full_name = ?1",
        )?;

        let result = match stmt.query_row(params![full_name], parse_definition_row) {
            Ok(record) => Some(record),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e),
        };

        Ok(result)
    }

    /// List all operation definitions
    pub fn list_operation_definitions(&self) -> SqliteResult<Vec<OperationDefinition>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT full_name, category, short_name, name, description, agent_info, timeout, operation_prompt, mode, agent_iterations, operation_chain, disabled, yolo_mode, model_ref, created_at, updated_at
             FROM operation_definitions ORDER BY category, short_name",
        )?;

        let definitions = stmt
            .query_map([], parse_definition_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(definitions)
    }

    /// List operation definitions by category
    #[allow(dead_code)]
    pub fn list_operation_definitions_by_category(&self, category: &str) -> SqliteResult<Vec<OperationDefinition>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT full_name, category, short_name, name, description, agent_info, timeout, operation_prompt, mode, agent_iterations, operation_chain, disabled, yolo_mode, model_ref, created_at, updated_at
             FROM operation_definitions WHERE category = ?1 ORDER BY short_name",
        )?;

        let definitions = stmt
            .query_map(params![category], parse_definition_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(definitions)
    }

    /// Delete an operation definition by full_name
    pub fn delete_operation_definition(&self, full_name: &str) -> SqliteResult<bool> {
        let conn = self.conn().lock().unwrap();

        let count = conn.execute(
            "DELETE FROM operation_definitions WHERE full_name = ?1",
            params![full_name],
        )?;

        Ok(count > 0)
    }

    /// Count operation definitions
    pub fn count_operation_definitions(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM operation_definitions",
            [],
            |row| row.get(0),
        )?;

        Ok(count as usize)
    }

    /// Prune old operation definitions (keep only MAX_OPERATION_DEFINITIONS)
    fn prune_old_definitions(&self) -> SqliteResult<usize> {
        let count = self.count_operation_definitions()?;

        if count <= MAX_OPERATION_DEFINITIONS {
            return Ok(0);
        }

        let to_delete = count - MAX_OPERATION_DEFINITIONS;
        let conn = self.conn().lock().unwrap();

        let deleted = conn.execute(
            "DELETE FROM operation_definitions WHERE full_name IN (
                SELECT full_name FROM operation_definitions
                ORDER BY updated_at ASC LIMIT ?1
            )",
            params![to_delete],
        )?;

        Ok(deleted)
    }
}

fn parse_definition_row(row: &rusqlite::Row) -> SqliteResult<OperationDefinition> {
    let full_name: String = row.get(0)?;
    let category: String = row.get(1)?;
    let short_name: String = row.get(2)?;
    let name: String = row.get(3)?;
    let description: String = row.get(4)?;
    let agent_info: String = row.get(5)?;
    let timeout: i64 = row.get(6)?;
    let operation_prompt: String = row.get(7)?;
    let mode: String = row.get(8)?;
    let agent_iterations: i64 = row.get(9)?;
    //
    // DEPRECATED: ignored.
    //
    let _operation_chain_json: String = row.get(10)?;
    let disabled: i64 = row.get(11)?;
    let yolo_mode: i64 = row.get(12)?;
    let model_ref: Option<String> = row.get(13)?;
    let created_at_str: String = row.get(14)?;
    let updated_at_str: String = row.get(15)?;

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(14, rusqlite::types::Type::Text, Box::new(e)))?
        .with_timezone(&Utc);

    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(15, rusqlite::types::Type::Text, Box::new(e)))?
        .with_timezone(&Utc);

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
