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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct MissionBudgetUpdate {
    #[serde(default)]
    pub max_iterations: Option<u32>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub max_active_seconds: Option<u64>,
}

impl MissionBudgetUpdate {
    pub fn is_empty(&self) -> bool {
        self.max_iterations.is_none()
            && self.max_tokens.is_none()
            && self.max_active_seconds.is_none()
    }

    pub fn apply(self, budget: &mut MissionBudget) {
        if let Some(value) = self.max_iterations {
            budget.max_iterations = value;
        }
        if let Some(value) = self.max_tokens {
            budget.max_tokens = value;
        }
        if let Some(value) = self.max_active_seconds {
            budget.max_active_seconds = value;
        }
    }
}

impl From<MissionBudget> for MissionBudgetUpdate {
    fn from(budget: MissionBudget) -> Self {
        Self {
            max_iterations: Some(budget.max_iterations),
            max_tokens: Some(budget.max_tokens),
            max_active_seconds: Some(budget.max_active_seconds),
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct MissionSpecUpdate {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub clear_model: bool,
    #[serde(default)]
    pub budget: Option<MissionBudgetUpdate>,
    #[serde(default)]
    pub verify: Option<bool>,
}

impl MissionSpecUpdate {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.objective.is_none()
            && self.scope.is_none()
            && self.model.is_none()
            && !self.clear_model
            && self.budget.is_none()
            && self.verify.is_none()
    }

    pub fn apply(self, spec: &mut MissionSpec) {
        if let Some(name) = self.name {
            spec.name = name;
        }
        if let Some(objective) = self.objective {
            spec.objective = objective;
        }
        if let Some(scope) = self.scope {
            spec.scope = scope;
        }
        if self.clear_model {
            spec.model = None;
        }
        if let Some(model) = self.model {
            spec.model = Some(model);
        }
        if let Some(budget) = self.budget {
            budget.apply(&mut spec.budget);
        }
        if let Some(verify) = self.verify {
            spec.verify = verify;
        }
    }
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
    Update {
        id: String,
        changes: MissionSpecUpdate,
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
        budget: Option<MissionBudgetUpdate>,
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
