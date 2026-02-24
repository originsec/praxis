use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::HashMap;

use super::{Database, DatabasePool};

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

/// Session group for elements that share a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionGroup {
    /// Unique identifier for this session group
    pub id: String,
    /// Color for visual identification (hex format like "#8B5CF6")
    pub color: String,
    /// Whether YOLO mode is enabled for the session
    pub yolo_mode: bool,
    /// Working directory override for this session group
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

/// Per-block configuration overrides
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlockConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runtime: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yolo_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// When false, the element runs as soon as any input fires (for merge
    /// points with conditional branches). Default (None/true): wait for all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_all_inputs: Option<bool>,
}

/// Memory element mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryMode {
    Store,
    Retrieve,
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
        /// Per-block configuration overrides
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block_config: Option<BlockConfig>,
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
        /// Per-block configuration overrides
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block_config: Option<BlockConfig>,
    },
    /// Generic prompt element - sends prompt to agent via session
    GenericPrompt {
        id: ElementId,
        /// Prompt to send to agent
        prompt: String,
        /// Session group for shared session execution
        session_group: Option<SessionGroup>,
        /// Per-block configuration overrides
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block_config: Option<BlockConfig>,
    },
    /// Memory element - stores or retrieves data by key
    Memory {
        id: ElementId,
        key: String,
        mode: MemoryMode,
    },
    /// Loop element - retries via port 0 until max_iterations, then exits via port 1
    Loop {
        id: ElementId,
        max_iterations: u32,
    },
    /// Tool element - invokes a registered toolkit tool
    Tool {
        id: ElementId,
        tool_name: String,
        tool_params: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block_config: Option<BlockConfig>,
    },
    /// Payload element - outputs static content from a stored payload
    Payload {
        id: ElementId,
        payload_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block_config: Option<BlockConfig>,
    },
    /// Termination element - explicit end of chain (exactly one per chain)
    Termination {
        id: ElementId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block_config: Option<BlockConfig>,
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
            ChainElement::Memory { id, .. } => id,
            ChainElement::Loop { id, .. } => id,
            ChainElement::Tool { id, .. } => id,
            ChainElement::Payload { id, .. } => id,
            ChainElement::Termination { id, .. } => id,
        }
    }

    /// Get the element's block config (if any)
    pub fn block_config(&self) -> Option<&BlockConfig> {
        match self {
            ChainElement::Operation { block_config, .. } => block_config.as_ref(),
            ChainElement::Transform { block_config, .. } => block_config.as_ref(),
            ChainElement::GenericPrompt { block_config, .. } => block_config.as_ref(),
            ChainElement::Tool { block_config, .. } => block_config.as_ref(),
            ChainElement::Payload { block_config, .. } => block_config.as_ref(),
            ChainElement::Termination { block_config, .. } => block_config.as_ref(),
            ChainElement::Trigger { .. }
            | ChainElement::Memory { .. }
            | ChainElement::Loop { .. } => None,
        }
    }

    /// Get the element's session group (if any)
    #[allow(dead_code)]
    pub fn session_group(&self) -> Option<&SessionGroup> {
        match self {
            ChainElement::Operation { session_group, .. } => session_group.as_ref(),
            ChainElement::Transform { session_group, .. } => session_group.as_ref(),
            ChainElement::GenericPrompt { session_group, .. } => session_group.as_ref(),
            ChainElement::Trigger { .. }
            | ChainElement::Memory { .. }
            | ChainElement::Loop { .. }
            | ChainElement::Tool { .. }
            | ChainElement::Payload { .. }
            | ChainElement::Termination { .. } => None,
        }
    }
}

/// Condition for when a connection fires
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionCondition {
    OnSuccess,
    OnFailure,
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
    /// Optional condition for when this connection fires
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<ConnectionCondition>,
}

/// Element position for visual layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementPosition {
    pub x: f64,
    pub y: f64,
}

/// Complete chain definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainDefinition {
    /// Unique chain ID (UUID)
    pub id: String,
    pub name: String,
    pub description: String,
    /// Category for organization
    pub category: String,
    /// All elements in the chain
    pub elements: Vec<ChainElement>,
    /// All connections between elements
    pub connections: Vec<ChainConnection>,
    /// Whether the chain is disabled (stored in table column, not in JSON).
    #[serde(default, skip_serializing)]
    pub disabled: bool,
    /// Timeout for the entire chain execution in seconds
    pub timeout: Option<u64>,
    /// Visual positions of elements (element_id -> position).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub positions: HashMap<String, ElementPosition>,
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
    #[serde(default)]
    pub trigger_count: usize,
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
            trigger_count: 0,
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
        // Must have exactly one Termination element.
        //
        let terminations: Vec<_> = self
            .elements
            .iter()
            .filter(|e| matches!(e, ChainElement::Termination { .. }))
            .collect();

        if terminations.is_empty() {
            return Err("Chain must have exactly one termination element".to_string());
        }
        if terminations.len() > 1 {
            return Err("Chain cannot have more than one termination element".to_string());
        }

        //
        // Termination element must not have outgoing connections.
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

        //
        // Validate Loop elements: max_iterations >= 1, at most one incoming
        // connection, at most two outgoing (port 0 = retry, port 1 = exit).
        //
        for element in &self.elements {
            if let ChainElement::Loop { max_iterations, .. } = element {
                if *max_iterations < 1 {
                    return Err("Loop element max_iterations must be >= 1".to_string());
                }
                let incoming = self.connections.iter().filter(|c| &c.to_element == element.id()).count();
                let outgoing = self.connections.iter().filter(|c| &c.from_element == element.id()).count();
                if incoming > 1 {
                    return Err(format!(
                        "Loop element can have at most one incoming connection (has {})", incoming
                    ));
                }
                if outgoing > 2 {
                    return Err(format!(
                        "Loop element can have at most two outgoing connections (has {})", outgoing
                    ));
                }
            }
        }

        //
        // Validate cycles: find SCCs using Tarjan's algorithm. Each SCC with
        // >1 node must contain at least one Loop element.
        //
        {
            let element_ids: Vec<&str> = self.elements.iter().map(|e| e.id().as_str()).collect();
            let adj: std::collections::HashMap<&str, Vec<&str>> = {
                let mut map: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
                for id in &element_ids {
                    map.entry(id).or_default();
                }
                for conn in &self.connections {
                    map.entry(conn.from_element.as_str())
                        .or_default()
                        .push(conn.to_element.as_str());
                }
                map
            };

            let sccs = tarjan_scc(&element_ids, &adj);
            for scc in &sccs {
                if scc.len() > 1 {
                    let has_loop = scc.iter().any(|id| {
                        self.elements.iter().any(|e| {
                            e.id().as_str() == *id && matches!(e, ChainElement::Loop { .. })
                        })
                    });
                    if !has_loop {
                        return Err(
                            "Chain contains a cycle without a Loop element. Add a Loop block to control iteration."
                                .to_string(),
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

/// Tarjan's SCC algorithm to find all strongly connected components.
fn tarjan_scc<'a>(
    nodes: &[&'a str],
    adj: &std::collections::HashMap<&'a str, Vec<&'a str>>,
) -> Vec<Vec<&'a str>> {
    use std::collections::HashMap;

    struct State<'a> {
        index_counter: usize,
        stack: Vec<&'a str>,
        on_stack: HashMap<&'a str, bool>,
        index: HashMap<&'a str, usize>,
        lowlink: HashMap<&'a str, usize>,
        result: Vec<Vec<&'a str>>,
    }

    fn strongconnect<'a>(
        v: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        state: &mut State<'a>,
    ) {
        state.index.insert(v, state.index_counter);
        state.lowlink.insert(v, state.index_counter);
        state.index_counter += 1;
        state.stack.push(v);
        state.on_stack.insert(v, true);

        if let Some(neighbors) = adj.get(v) {
            for w in neighbors {
                if !state.index.contains_key(w) {
                    strongconnect(w, adj, state);
                    let w_low = state.lowlink[w];
                    let v_low = state.lowlink[v];
                    if w_low < v_low {
                        state.lowlink.insert(v, w_low);
                    }
                } else if *state.on_stack.get(w).unwrap_or(&false) {
                    let w_idx = state.index[w];
                    let v_low = state.lowlink[v];
                    if w_idx < v_low {
                        state.lowlink.insert(v, w_idx);
                    }
                }
            }
        }

        if state.lowlink[v] == state.index[v] {
            let mut component = Vec::new();
            loop {
                let w = state.stack.pop().unwrap();
                state.on_stack.insert(w, false);
                component.push(w);
                if w == v {
                    break;
                }
            }
            state.result.push(component);
        }
    }

    let mut state = State {
        index_counter: 0,
        stack: Vec::new(),
        on_stack: HashMap::new(),
        index: HashMap::new(),
        lowlink: HashMap::new(),
        result: Vec::new(),
    };

    for node in nodes {
        if !state.index.contains_key(node) {
            strongconnect(node, adj, &mut state);
        }
    }

    state.result
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
    // Check if we need to migrate old-style Termination elements (ones with
    // termination_type field). New-style Termination elements (no
    // termination_type) are left alone.
    //
    let has_termination = {
        let elements = value.get("elements")?.as_array()?;
        elements.iter().any(|e| {
            e.get("element_type")
                .and_then(|v| v.as_str())
                .map(|t| t == "Termination")
                .unwrap_or(false)
                && e.get("termination_type").is_some()
        })
    };

    //
    // If no SessionBox elements, no positions, and no Termination — already
    // in new format.
    //
    if session_boxes.is_empty() && !has_positions && !has_termination {
        return None;
    }

    //
    // If no SessionBox but has positions or Termination, just handle those.
    //
    if session_boxes.is_empty() {
        if has_positions {
            let elements = value.get_mut("elements")?.as_array_mut()?;
            for element in elements.iter_mut() {
                if let Some(obj) = element.as_object_mut() {
                    obj.remove("position");
                    obj.remove("size");
                }
            }
            common::log_info!("Migrated chain: removed position fields from elements");
        }
        migrate_termination_elements(&mut value);
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

    //
    // Fall through to Termination migration below.
    //
    migrate_termination_elements(&mut value);

    serde_json::to_string(&value).ok()
}

/// Second migration pass: convert or remove old-style Termination elements
/// (ones with termination_type field).
/// - Raw Termination: remove element + connections pointing to it.
/// - Semantic Termination: convert to Transform with same id, prompt, model_ref.
/// New-style Termination elements (no termination_type) are left untouched.
fn migrate_termination_elements(value: &mut serde_json::Value) {
    let (raw_term_ids, semantic_conversions) = {
        let elements = match value.get("elements").and_then(|e| e.as_array()) {
            Some(e) => e,
            None => return,
        };

        let mut raw_ids: Vec<String> = Vec::new();
        let mut semantic: Vec<(String, String, Option<String>)> = Vec::new();

        for e in elements {
            let element_type = match e.get("element_type").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => continue,
            };
            if element_type != "Termination" {
                continue;
            }

            //
            // Only migrate old-style Termination elements that have a
            // termination_type field. New-style ones are valid as-is.
            //
            if e.get("termination_type").is_none() {
                continue;
            }

            let id = match e.get("id").and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };

            let term_type = e
                .get("termination_type")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("Raw");

            if term_type == "Semantic" {
                let prompt = e
                    .get("termination_type")
                    .and_then(|v| v.get("prompt"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let model_ref = e
                    .get("termination_type")
                    .and_then(|v| v.get("model_ref"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                semantic.push((id, prompt, model_ref));
            } else {
                raw_ids.push(id);
            }
        }

        (raw_ids, semantic)
    };

    if raw_term_ids.is_empty() && semantic_conversions.is_empty() {
        return;
    }

    //
    // Remove connections pointing to Raw Termination elements.
    //
    if !raw_term_ids.is_empty() {
        if let Some(connections) = value.get_mut("connections").and_then(|c| c.as_array_mut()) {
            connections.retain(|conn| {
                let to = conn
                    .get("to_element")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                !raw_term_ids.contains(&to.to_string())
            });
        }
    }

    //
    // Process elements: remove Raw Termination, convert Semantic to Transform.
    //
    if let Some(elements) = value.get_mut("elements").and_then(|e| e.as_array_mut()) {
        //
        // Remove Raw Termination elements.
        //
        elements.retain(|e| {
            let id = e.get("id").and_then(|v| v.as_str()).unwrap_or("");
            !raw_term_ids.contains(&id.to_string())
        });

        //
        // Convert Semantic Termination elements to Transform.
        //
        for element in elements.iter_mut() {
            if let Some(obj) = element.as_object_mut() {
                let id = obj
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Some((_, prompt, model_ref)) =
                    semantic_conversions.iter().find(|(sid, _, _)| *sid == id)
                {
                    obj.insert(
                        "element_type".to_string(),
                        serde_json::Value::String("Transform".to_string()),
                    );
                    obj.insert(
                        "prompt".to_string(),
                        serde_json::Value::String(prompt.clone()),
                    );
                    if let Some(mref) = model_ref {
                        obj.insert(
                            "model_ref".to_string(),
                            serde_json::Value::String(mref.clone()),
                        );
                    }
                    obj.remove("termination_type");
                    obj.remove("label");
                }
            }
        }
    }

    let migrated_count = raw_term_ids.len() + semantic_conversions.len();
    if migrated_count > 0 {
        common::log_info!(
            "Migrated chain: converted {} Termination elements ({} removed, {} converted to Transform)",
            migrated_count,
            raw_term_ids.len(),
            semantic_conversions.len()
        );
    }
}

impl Database {
    /// Insert or update a chain definition
    pub async fn upsert_chain(&self, chain: &ChainDefinition) -> Result<()> {
        let definition_json = serde_json::to_string(&chain)?;

        let sql = "INSERT INTO operation_chains (id, name, description, category, definition, disabled, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 description = excluded.description,
                 category = excluded.category,
                 definition = excluded.definition,
                 disabled = excluded.disabled,
                 updated_at = excluded.updated_at";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&chain.id)
                    .bind(&chain.name)
                    .bind(&chain.description)
                    .bind(&chain.category)
                    .bind(&definition_json)
                    .bind(if chain.disabled { 1i32 } else { 0i32 })
                    .bind(chain.created_at.to_rfc3339())
                    .bind(chain.updated_at.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(&chain.id)
                    .bind(&chain.name)
                    .bind(&chain.description)
                    .bind(&chain.category)
                    .bind(&definition_json)
                    .bind(if chain.disabled { 1i16 } else { 0i16 })
                    .bind(chain.created_at.to_rfc3339())
                    .bind(chain.updated_at.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
        }

        self.prune_old_chains().await?;

        Ok(())
    }

    /// Get a chain definition by ID
    /// Automatically migrates old format (SessionBox, positions) to new format
    pub async fn get_chain(&self, id: &str) -> Result<Option<ChainDefinition>> {
        let sql = "SELECT definition, disabled FROM operation_chains WHERE id = $1";

        let row_opt: Option<(String, bool)> = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(id)
                    .fetch_optional(pool)
                    .await?
                    .map(|r| (r.get(0), r.get(1)))
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(id)
                    .fetch_optional(pool)
                    .await?
                    .map(|r| {
                        let disabled: i16 = r.get(1);
                        (r.get(0), disabled != 0)
                    })
            }
        };

        match row_opt {
            Some((json, disabled)) => {
                let mut chain = match serde_json::from_str::<ChainDefinition>(&json) {
                    Ok(c) => c,
                    Err(_) => {
                        if let Some(migrated_json) = migrate_chain_json(&json) {
                            serde_json::from_str(&migrated_json)?
                        } else {
                            serde_json::from_str(&json)?
                        }
                    }
                };
                chain.disabled = disabled;
                Ok(Some(chain))
            }
            None => Ok(None),
        }
    }

    /// List all chain definitions (returns summary info)
    /// Automatically handles migration of old format chains
    pub async fn list_chains(&self) -> Result<Vec<ChainDefinitionInfo>> {
        let sql = "SELECT definition, disabled FROM operation_chains ORDER BY category, name";

        let rows: Vec<(String, bool)> = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                rows.iter().map(|r| (r.get(0), r.get(1))).collect()
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                rows.iter().map(|r| {
                    let disabled: i16 = r.get(1);
                    (r.get(0), disabled != 0)
                }).collect()
            }
        };

        let chains: Vec<ChainDefinitionInfo> = rows
            .into_iter()
            .filter_map(|(json, disabled)| {
                serde_json::from_str::<ChainDefinition>(&json)
                    .ok()
                    .or_else(|| {
                        migrate_chain_json(&json)
                            .and_then(|migrated| serde_json::from_str::<ChainDefinition>(&migrated).ok())
                    })
                    .map(|mut c| { c.disabled = disabled; c })
            })
            .map(|c| c.to_info())
            .collect();

        Ok(chains)
    }

    /// List chain definitions by category
    /// Automatically handles migration of old format chains
    #[allow(dead_code)]
    pub async fn list_chains_by_category(&self, category: &str) -> Result<Vec<ChainDefinitionInfo>> {
        let sql = "SELECT definition, disabled FROM operation_chains WHERE category = $1 ORDER BY name";

        let rows: Vec<(String, bool)> = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).bind(category).fetch_all(pool).await?;
                rows.iter().map(|r| (r.get(0), r.get(1))).collect()
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).bind(category).fetch_all(pool).await?;
                rows.iter().map(|r| {
                    let disabled: i16 = r.get(1);
                    (r.get(0), disabled != 0)
                }).collect()
            }
        };

        let chains: Vec<ChainDefinitionInfo> = rows
            .into_iter()
            .filter_map(|(json, disabled)| {
                serde_json::from_str::<ChainDefinition>(&json)
                    .ok()
                    .or_else(|| {
                        migrate_chain_json(&json)
                            .and_then(|migrated| serde_json::from_str::<ChainDefinition>(&migrated).ok())
                    })
                    .map(|mut c| { c.disabled = disabled; c })
            })
            .map(|c| c.to_info())
            .collect();

        Ok(chains)
    }

    /// Delete a chain definition by ID (cascade-deletes associated triggers)
    pub async fn delete_chain(&self, id: &str) -> Result<bool> {
        //
        // Cascade delete associated triggers first.
        //
        let _ = self.delete_chain_triggers_for_chain(id).await;

        let sql = "DELETE FROM operation_chains WHERE id = $1";

        let count = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(id)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(id)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
        };

        Ok(count > 0)
    }

    /// Set the disabled flag on a chain definition
    pub async fn set_chain_disabled(&self, id: &str, disabled: bool) -> Result<bool> {
        let sql = "UPDATE operation_chains SET disabled = $1, updated_at = $2 WHERE id = $3";
        let now = chrono::Utc::now().to_rfc3339();

        let count = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(if disabled { 1i32 } else { 0i32 })
                    .bind(&now)
                    .bind(id)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(if disabled { 1i16 } else { 0i16 })
                    .bind(&now)
                    .bind(id)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
        };

        Ok(count > 0)
    }

    /// Count chain definitions
    pub async fn count_chains(&self) -> Result<usize> {
        let sql = "SELECT COUNT(*) FROM operation_chains";

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

    /// Prune old chain definitions (keep only MAX_CHAINS)
    async fn prune_old_chains(&self) -> Result<usize> {
        let count = self.count_chains().await?;

        if count <= MAX_CHAINS {
            return Ok(0);
        }

        let to_delete = count - MAX_CHAINS;

        let sql = "DELETE FROM operation_chains WHERE id IN (
                SELECT id FROM operation_chains
                ORDER BY updated_at ASC LIMIT $1
            )";

        let deleted = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(to_delete as i64)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(to_delete as i64)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
        };

        Ok(deleted as usize)
    }
}
