use chrono::{DateTime, Utc};
use rusqlite::{params, Result as SqliteResult};
use serde::{Deserialize, Serialize};

use super::Database;

/// Maximum number of chain definitions to store
const MAX_CHAINS: usize = 200;

/// Unique identifier for chain elements
pub type ElementId = String;

/// Trigger element types (start of chain)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TriggerType {
    /// Manual trigger via UI
    Manual,
}

/// Model reference for LLM operations (format: "provider::model")
pub type ModelRef = String;

/// Termination element types (end of chain)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TerminationType {
    /// Raw dump - outputs the accumulated input data
    Raw,
    /// Semantic termination - runs LLM with prompt on accumulated data
    Semantic {
        prompt: String,
        /// Optional model override (format: "provider::model")
        model_ref: Option<ModelRef>,
    },
}

/// Session group for elements that share a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionGroup {
    /// Unique identifier for this session group
    pub id: String,
    /// Color for visual identification (hex format like "#8B5CF6")
    pub color: String,
    /// Whether YOLO mode is enabled for the session
    pub yolo_mode: bool,
}

/// Chain element variants
/// Note: Positions are not stored - they are computed dynamically using Dagre layout
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "element_type")]
pub enum ChainElement {
    /// Trigger element - start of chain (exactly one per chain)
    Trigger {
        id: ElementId,
        trigger_type: TriggerType,
    },
    /// Semantic operation block - executes an existing operation definition
    Operation {
        id: ElementId,
        /// Full name of the operation definition (category::short_name)
        operation_name: String,
        /// Optional model/provider override (format: "provider::model")
        model_ref: Option<ModelRef>,
        /// Session group for shared session execution
        session_group: Option<SessionGroup>,
    },
    /// Transform element - runs LLM on input and passes result to next element
    Transform {
        id: ElementId,
        /// Prompt for LLM processing
        prompt: String,
        /// Model to use (format: "provider::model")
        model_ref: Option<ModelRef>,
        /// Session group for shared session execution
        session_group: Option<SessionGroup>,
    },
    /// Generic prompt element - sends prompt to agent via session
    GenericPrompt {
        id: ElementId,
        /// Prompt to send to agent
        prompt: String,
        /// Session group for shared session execution
        session_group: Option<SessionGroup>,
    },
    /// Termination element - end of a branch
    Termination {
        id: ElementId,
        termination_type: TerminationType,
        /// Label for this output
        label: String,
    },
}

impl ChainElement {
    /// Get the element's unique ID
    pub fn id(&self) -> &ElementId {
        match self {
            ChainElement::Trigger { id, .. } => id,
            ChainElement::Operation { id, .. } => id,
            ChainElement::Transform { id, .. } => id,
            ChainElement::GenericPrompt { id, .. } => id,
            ChainElement::Termination { id, .. } => id,
        }
    }

    /// Get the element's session group (if any)
    #[allow(dead_code)]
    pub fn session_group(&self) -> Option<&SessionGroup> {
        match self {
            ChainElement::Operation { session_group, .. } => session_group.as_ref(),
            ChainElement::Transform { session_group, .. } => session_group.as_ref(),
            ChainElement::GenericPrompt { session_group, .. } => session_group.as_ref(),
            _ => None,
        }
    }
}

/// Connection between two elements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConnection {
    /// Unique connection ID
    pub id: String,
    /// Source element ID
    pub from_element: ElementId,
    /// Target element ID
    pub to_element: ElementId,
    /// Output port index (for elements with multiple outputs)
    pub from_port: u32,
    /// Input port index (for elements with multiple inputs)
    pub to_port: u32,
}

/// Complete chain definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainDefinition {
    /// Unique chain ID (UUID)
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// Category for organization
    pub category: String,
    /// All elements in the chain
    pub elements: Vec<ChainElement>,
    /// All connections between elements
    pub connections: Vec<ChainConnection>,
    /// Whether the chain is disabled
    pub disabled: bool,
    /// Timeout for the entire chain execution in seconds
    pub timeout: Option<u64>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

/// Summary info about a chain (for list views)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainDefinitionInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub disabled: bool,
    pub timeout: Option<u64>,
    pub element_count: usize,
    pub operation_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ChainDefinition {
    /// Convert to summary info
    pub fn to_info(&self) -> ChainDefinitionInfo {
        //
        // Count operations, transforms, and generic prompts as "operation
        // count".
        //
        let operation_count = self
            .elements
            .iter()
            .filter(|e| matches!(e,
                ChainElement::Operation { .. } |
                ChainElement::Transform { .. } |
                ChainElement::GenericPrompt { .. }
            ))
            .count();

        ChainDefinitionInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            category: self.category.clone(),
            disabled: self.disabled,
            timeout: self.timeout,
            element_count: self.elements.len(),
            operation_count,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    /// Validate chain structure
    pub fn validate(&self) -> Result<(), String> {
        //
        // Must have at least one trigger.
        //
        let triggers: Vec<_> = self
            .elements
            .iter()
            .filter(|e| matches!(e, ChainElement::Trigger { .. }))
            .collect();

        if triggers.is_empty() {
            return Err("Chain must have exactly one trigger element".to_string());
        }
        if triggers.len() > 1 {
            return Err("Chain cannot have more than one trigger element".to_string());
        }

        //
        // Must have at least one termination.
        //
        let terminations: Vec<_> = self
            .elements
            .iter()
            .filter(|e| matches!(e, ChainElement::Termination { .. }))
            .collect();

        if terminations.is_empty() {
            return Err("Chain must have at least one termination element".to_string());
        }

        //
        // Validate all connections reference existing elements.
        //
        for conn in &self.connections {
            if !self.elements.iter().any(|e| e.id() == &conn.from_element) {
                return Err(format!(
                    "Connection {} references non-existent source element {}",
                    conn.id, conn.from_element
                ));
            }
            if !self.elements.iter().any(|e| e.id() == &conn.to_element) {
                return Err(format!(
                    "Connection {} references non-existent target element {}",
                    conn.id, conn.to_element
                ));
            }
        }

        //
        // Trigger should have no incoming connections.
        //
        for trigger in &triggers {
            if self
                .connections
                .iter()
                .any(|c| &c.to_element == trigger.id())
            {
                return Err("Trigger element cannot have incoming connections".to_string());
            }
        }

        //
        // Termination should have no outgoing connections.
        //
        for term in &terminations {
            if self
                .connections
                .iter()
                .any(|c| &c.from_element == term.id())
            {
                return Err("Termination element cannot have outgoing connections".to_string());
            }
        }

        Ok(())
    }
}

/// Session group colors for migration
const SESSION_GROUP_COLORS: &[&str] = &[
    //
    // Purple.
    //
    "#8B5CF6",
    //
    // Emerald.
    //
    "#10B981",
    //
    // Amber.
    //
    "#F59E0B",
    //
    // Red.
    //
    "#EF4444",
    //
    // Blue.
    //
    "#3B82F6",
    //
    // Pink.
    //
    "#EC4899",
    //
    // Teal.
    //
    "#14B8A6",
    //
    // Orange.
    //
    "#F97316",
];

/// Migrate old chain format (with SessionBox and positions) to new format
/// Returns the migrated JSON string if migration was performed, otherwise None
fn migrate_chain_json(json: &str) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(json).ok()?;

    //
    // First pass: read elements to find SessionBox elements (immutable borrow).
    //
    let session_boxes: Vec<(String, bool, Vec<String>)> = {
        let elements = value.get("elements")?.as_array()?;
        elements
            .iter()
            .filter_map(|e| {
                let element_type = e.get("element_type")?.as_str()?;
                if element_type == "SessionBox" {
                    let id = e.get("id")?.as_str()?.to_string();
                    let yolo_mode = e.get("yolo_mode").and_then(|v| v.as_bool()).unwrap_or(false);
                    let contained = e.get("contained_elements")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default();
                    Some((id, yolo_mode, contained))
                } else {
                    None
                }
            })
            .collect()
    };

    //
    // Check if we need to remove position fields.
    //
    let has_positions = {
        let elements = value.get("elements")?.as_array()?;
        elements.iter().any(|e| e.get("position").is_some())
    };

    //
    // If no SessionBox elements and no positions, already in new format.
    //
    if session_boxes.is_empty() && !has_positions {
        return None;
    }

    //
    // If no SessionBox but has positions, just remove position fields.
    //
    if session_boxes.is_empty() {
        let elements = value.get_mut("elements")?.as_array_mut()?;
        for element in elements.iter_mut() {
            if let Some(obj) = element.as_object_mut() {
                obj.remove("position");
                obj.remove("size");
            }
        }
        common::log_info!("Migrated chain: removed position fields from elements");
        return serde_json::to_string(&value).ok();
    }

    //
    // Build session group info with colors.
    //
    let mut used_colors = std::collections::HashSet::new();
    let mut session_groups: Vec<(String, String, bool, Vec<String>)> = Vec::new();

    for (id, yolo_mode, contained) in session_boxes {
        let color = SESSION_GROUP_COLORS
            .iter()
            .find(|c| !used_colors.contains(*c))
            .unwrap_or(&SESSION_GROUP_COLORS[0]);
        used_colors.insert(*color);
        session_groups.push((id, color.to_string(), yolo_mode, contained));
    }

    //
    // Build a map of element_id -> session_group.
    //
    let mut element_to_group: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();

    for (group_id, color, yolo_mode, contained_elements) in &session_groups {
        let session_group = serde_json::json!({
            "id": group_id,
            "color": color,
            "yolo_mode": yolo_mode
        });
        for elem_id in contained_elements {
            element_to_group.insert(elem_id.clone(), session_group.clone());
        }
    }

    let session_box_ids: std::collections::HashSet<String> = session_groups
        .iter()
        .map(|(id, _, _, _)| id.clone())
        .collect();

    //
    // Remove connections involving SessionBox elements.
    //
    if let Some(connections) = value.get_mut("connections").and_then(|c| c.as_array_mut()) {
        connections.retain(|conn| {
            let from = conn.get("from_element").and_then(|v| v.as_str()).unwrap_or("");
            let to = conn.get("to_element").and_then(|v| v.as_str()).unwrap_or("");
            !session_box_ids.contains(from) && !session_box_ids.contains(to)
        });
    }

    //
    // Update elements: remove SessionBox, remove positions, add session_group.
    //
    if let Some(elements) = value.get_mut("elements").and_then(|e| e.as_array_mut()) {
        //
        // Remove SessionBox elements.
        //
        elements.retain(|e| {
            e.get("element_type")
                .and_then(|v| v.as_str())
                .map(|t| t != "SessionBox")
                .unwrap_or(true)
        });

        //
        // Update remaining elements.
        //
        for element in elements.iter_mut() {
            if let Some(obj) = element.as_object_mut() {
                obj.remove("position");
                obj.remove("size");

                if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
                    if let Some(session_group) = element_to_group.get(id) {
                        obj.insert("session_group".to_string(), session_group.clone());
                    }
                }
            }
        }
    }

    common::log_info!(
        "Migrated chain: converted {} SessionBox elements to session groups",
        session_groups.len()
    );

    serde_json::to_string(&value).ok()
}

impl Database {
    /// Initialize the chains schema
    pub fn init_chains_schema(conn: &rusqlite::Connection) -> SqliteResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS operation_chains (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                category TEXT NOT NULL,
                definition TEXT NOT NULL,
                disabled INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chains_category ON operation_chains(category)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chains_name ON operation_chains(name)",
            [],
        )?;

        Ok(())
    }

    /// Insert or update a chain definition
    pub fn upsert_chain(&self, chain: &ChainDefinition) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        let definition_json = serde_json::to_string(&chain)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

        conn.execute(
            "INSERT INTO operation_chains (id, name, description, category, definition, disabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 description = excluded.description,
                 category = excluded.category,
                 definition = excluded.definition,
                 disabled = excluded.disabled,
                 updated_at = excluded.updated_at",
            params![
                chain.id,
                chain.name,
                chain.description,
                chain.category,
                definition_json,
                if chain.disabled { 1 } else { 0 },
                chain.created_at.to_rfc3339(),
                chain.updated_at.to_rfc3339(),
            ],
        )?;

        drop(conn);
        self.prune_old_chains()?;

        Ok(())
    }

    /// Get a chain definition by ID
    /// Automatically migrates old format (SessionBox, positions) to new format
    pub fn get_chain(&self, id: &str) -> SqliteResult<Option<ChainDefinition>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT definition FROM operation_chains WHERE id = ?1",
        )?;

        let result = match stmt.query_row(params![id], |row| {
            let definition_json: String = row.get(0)?;
            Ok(definition_json)
        }) {
            Ok(json) => {
                //
                // Try to deserialize directly first.
                //
                match serde_json::from_str::<ChainDefinition>(&json) {
                    Ok(chain) => Some(chain),
                    Err(_) => {
                        //
                        // Try migration for old format.
                        //
                        if let Some(migrated_json) = migrate_chain_json(&json) {
                            let chain: ChainDefinition = serde_json::from_str(&migrated_json)
                                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
                            Some(chain)
                        } else {
                            //
                            // Migration failed, try original error.
                            //
                            let chain: ChainDefinition = serde_json::from_str(&json)
                                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
                            Some(chain)
                        }
                    }
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e),
        };

        Ok(result)
    }

    /// List all chain definitions (returns summary info)
    /// Automatically handles migration of old format chains
    pub fn list_chains(&self) -> SqliteResult<Vec<ChainDefinitionInfo>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT definition FROM operation_chains ORDER BY category, name",
        )?;

        let chains = stmt
            .query_map([], |row| {
                let definition_json: String = row.get(0)?;
                Ok(definition_json)
            })?
            .filter_map(|r| r.ok())
            .filter_map(|json| {
                //
                // Try direct deserialization first.
                //
                serde_json::from_str::<ChainDefinition>(&json)
                    .ok()
                    .or_else(|| {
                        //
                        // Try migration for old format.
                        //
                        migrate_chain_json(&json)
                            .and_then(|migrated| serde_json::from_str::<ChainDefinition>(&migrated).ok())
                    })
            })
            .map(|c| c.to_info())
            .collect();

        Ok(chains)
    }

    /// List chain definitions by category
    /// Automatically handles migration of old format chains
    #[allow(dead_code)]
    pub fn list_chains_by_category(&self, category: &str) -> SqliteResult<Vec<ChainDefinitionInfo>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT definition FROM operation_chains WHERE category = ?1 ORDER BY name",
        )?;

        let chains = stmt
            .query_map(params![category], |row| {
                let definition_json: String = row.get(0)?;
                Ok(definition_json)
            })?
            .filter_map(|r| r.ok())
            .filter_map(|json| {
                //
                // Try direct deserialization first.
                //
                serde_json::from_str::<ChainDefinition>(&json)
                    .ok()
                    .or_else(|| {
                        //
                        // Try migration for old format.
                        //
                        migrate_chain_json(&json)
                            .and_then(|migrated| serde_json::from_str::<ChainDefinition>(&migrated).ok())
                    })
            })
            .map(|c| c.to_info())
            .collect();

        Ok(chains)
    }

    /// Delete a chain definition by ID
    pub fn delete_chain(&self, id: &str) -> SqliteResult<bool> {
        let conn = self.conn().lock().unwrap();

        let count = conn.execute(
            "DELETE FROM operation_chains WHERE id = ?1",
            params![id],
        )?;

        Ok(count > 0)
    }

    /// Count chain definitions
    pub fn count_chains(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM operation_chains",
            [],
            |row| row.get(0),
        )?;

        Ok(count as usize)
    }

    /// Prune old chain definitions (keep only MAX_CHAINS)
    fn prune_old_chains(&self) -> SqliteResult<usize> {
        let count = self.count_chains()?;

        if count <= MAX_CHAINS {
            return Ok(0);
        }

        let to_delete = count - MAX_CHAINS;
        let conn = self.conn().lock().unwrap();

        let deleted = conn.execute(
            "DELETE FROM operation_chains WHERE id IN (
                SELECT id FROM operation_chains
                ORDER BY updated_at ASC LIMIT ?1
            )",
            params![to_delete],
        )?;

        Ok(deleted)
    }
}
