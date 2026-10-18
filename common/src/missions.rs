use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MissionBudget {
    pub max_iterations: u32,
    pub max_tokens: u64,
    pub max_active_seconds: u64,
}

impl Default for MissionBudget {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            max_tokens: 500_000,
            max_active_seconds: 3600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MissionSpec {
    pub name: String,
    pub objective: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub budget: MissionBudget,
    #[serde(default = "default_verify")]
    pub verify: bool,
}

fn default_verify() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Running,
    Paused,
    Interrupted,
    NeedsReview,
    Completed,
    Cancelled,
    Failed,
    BudgetExceeded,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct MissionCheckpoint {
    pub summary: String,
    pub findings: Vec<String>,
    pub evidence: Vec<String>,
    pub outstanding: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mission {
    pub id: String,
    pub spec: MissionSpec,
    pub status: MissionStatus,
    pub checkpoint: MissionCheckpoint,
    pub iterations: u32,
    pub tokens_used: u64,
    pub active_seconds: u64,
    pub pending_tools: Vec<String>,
    pub result: Option<String>,
    pub verification: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionEvent {
    pub mission_id: String,
    pub sequence: i64,
    pub kind: String,
    pub data: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MissionCommand {
    Create {
        spec: MissionSpec,
    },
    List,
    Get {
        id: String,
    },
    Events {
        id: String,
        #[serde(default)]
        after: i64,
    },
    Resume {
        id: String,
        #[serde(default)]
        reconciliation: Option<String>,
        #[serde(default)]
        budget: Option<MissionBudget>,
    },
    Steer {
        id: String,
        instruction: String,
    },
    Pause {
        id: String,
    },
    Cancel {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum MissionResponse {
    Mission(Mission),
    Missions(Vec<Mission>),
    Events(Vec<MissionEvent>),
}
