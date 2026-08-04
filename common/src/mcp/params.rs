use rmcp::schemars::JsonSchema;
use serde::Deserialize;

use crate::{ScheduleSpec, TargetSpec, TriggerConfig};

//
// Tool parameter types for MCP server operations.
//

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NodePrefixParams {
    pub prefix: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NodeParams {
    pub node: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionCreateParams {
    #[schemars(description = "Node ID prefix")]
    pub node: String,

    #[schemars(
        description = "Agent short name (e.g. 'claude-code', 'codex'). ACP sessions are per-agent; each session is bound to the agent it was created with."
    )]
    pub agent: String,

    #[schemars(description = "Enable YOLO mode (agent auto-approves actions)")]
    #[serde(default)]
    pub yolo: bool,

    #[schemars(description = "Working directory / project path for the session")]
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReconRunParams {
    #[schemars(description = "Node ID prefix")]
    pub node: String,

    #[schemars(description = "Agent short name whose view the recon scan is scoped to")]
    pub agent: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionPromptParams {
    pub node: String,

    #[schemars(description = "Session ID returned by session_create. Required.")]
    pub session_id: String,

    pub prompt: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionCloseParams {
    pub node: String,

    #[schemars(description = "Session ID returned by session_create. Required.")]
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub enum McpFileType {
    Config,
    Session,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteFileParams {
    pub node: String,
    pub file_type: McpFileType,
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReconListParams {
    #[schemars(description = "Node ID prefix")]
    pub node: String,

    #[schemars(description = "Agent short name")]
    pub agent: String,

    #[schemars(
        description = "Section to list: all, sessions, tools, projects, configs (default: all)"
    )]
    pub section: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReconReadParams {
    #[schemars(description = "Node ID prefix")]
    pub node: String,

    #[schemars(description = "Agent short name whose stored recon to read from")]
    pub agent: String,

    #[schemars(description = "Path to the file (omit to read all from recon)")]
    pub path: Option<String>,

    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReconGrepParams {
    #[schemars(description = "Node ID prefix")]
    pub node: String,

    #[schemars(description = "Agent short name whose stored recon to grep")]
    pub agent: String,

    #[schemars(description = "Regex pattern to search for")]
    pub pattern: String,

    #[schemars(
        description = "File path(s) to grep. Supports glob patterns (e.g. '/etc/*.conf'). Omit to grep all files from recon."
    )]
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrafficSearchParams {
    pub pattern: String,
    pub node: Option<String>,
    pub agent: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpRunParams {
    #[schemars(description = "Operation name (e.g. recon::system_info) or chain name/ID")]
    pub name: String,

    #[schemars(description = "Node ID prefix")]
    pub node: String,

    #[schemars(description = "Agent short name")]
    pub agent: String,

    #[schemars(description = "Working directory for the operation")]
    pub working_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShortIdParams {
    #[schemars(description = "Short ID to look up")]
    pub short_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NameParams {
    #[schemars(description = "Operation name (e.g. recon::system_info) or chain name")]
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpCreateParams {
    #[schemars(description = "Display name for the operation")]
    pub name: String,

    #[schemars(
        description = "Short name identifier (lowercase, no spaces). Combined with category to form the full name: category::short_name"
    )]
    pub short_name: String,

    #[schemars(description = "Category for the operation (e.g. 'recon', 'exfil', 'custom')")]
    pub category: String,

    #[schemars(description = "Human-readable description of what the operation does")]
    pub description: String,

    #[schemars(description = "The prompt to send to the remote agent")]
    pub operation_prompt: String,

    #[schemars(
        description = "Execution mode: 'one-shot' (single prompt/response) or 'agent' (iterative LLM-driven orchestration with multiple rounds). Default: 'one-shot'"
    )]
    #[serde(default = "default_mode")]
    pub mode: String,

    #[schemars(
        description = "Contextual information to enrich the semantic agent's understanding (agent mode only)"
    )]
    #[serde(default)]
    pub agent_info: String,

    #[schemars(description = "Timeout in seconds. Default: 60")]
    #[serde(default = "default_op_timeout")]
    pub timeout: u64,

    #[schemars(description = "Max iterations for agent mode. Default: 5")]
    #[serde(default = "default_agent_iterations")]
    pub agent_iterations: u32,

    #[schemars(description = "Enable YOLO mode (agent auto-approves actions). Default: false")]
    #[serde(default)]
    pub yolo_mode: bool,
}

fn default_mode() -> String {
    "one-shot".to_string()
}

fn default_op_timeout() -> u64 {
    60
}

fn default_agent_iterations() -> u32 {
    5
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpDeleteParams {
    #[schemars(
        description = "Full name (category::short_name), short_name, or display name of the operation to delete"
    )]
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChainCreateParams {
    #[schemars(description = "Display name for the chain")]
    pub name: String,

    #[schemars(
        description = "Existing operation names in execution order. Each value may be a full name, short name, or display name.",
        length(min = 1)
    )]
    pub operations: Vec<String>,

    #[schemars(description = "Human-readable description of the chain")]
    #[serde(default)]
    pub description: String,

    #[schemars(description = "Chain category. Default: custom")]
    #[serde(default = "default_chain_category")]
    pub category: String,

    #[schemars(description = "Optional timeout for the entire chain in seconds")]
    pub timeout: Option<u64>,
}

fn default_chain_category() -> String {
    "custom".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TriggerListParams {
    #[schemars(description = "Optional full chain ID. Omit to list triggers for every chain.")]
    pub chain_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerScheduleParams {
    #[serde(rename = "daily_at")]
    DailyAt {
        #[schemars(description = "UTC hour (0-23)", range(min = 0, max = 23))]
        hour: u8,

        #[schemars(description = "UTC minute (0-59)", range(min = 0, max = 59))]
        minute: u8,
    },
    Interval {
        #[schemars(
            description = "Interval length in minutes (must be at least 1)",
            range(min = 1)
        )]
        minutes: u32,
    },
}

impl TryFrom<TriggerScheduleParams> for ScheduleSpec {
    type Error = String;

    fn try_from(value: TriggerScheduleParams) -> Result<Self, Self::Error> {
        match value {
            TriggerScheduleParams::DailyAt { hour, minute } => {
                if hour > 23 {
                    return Err("daily_at hour must be between 0 and 23".to_string());
                }
                if minute > 59 {
                    return Err("daily_at minute must be between 0 and 59".to_string());
                }
                Ok(Self::DailyAt { hour, minute })
            }
            TriggerScheduleParams::Interval { minutes } => {
                if minutes == 0 {
                    return Err("interval minutes must be at least 1".to_string());
                }
                Ok(Self::Interval { minutes })
            }
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerConfigParams {
    Scheduled {
        schedule: TriggerScheduleParams,

        #[schemars(
            description = "Fire repeatedly when true; disable after the first run when false"
        )]
        #[serde(default)]
        recurring: bool,
    },
    InterceptMatch {
        #[schemars(description = "Numeric ID of the intercept rule that should fire the chain")]
        rule_id: i64,
    },
    NewNode,
}

impl TryFrom<TriggerConfigParams> for TriggerConfig {
    type Error = String;

    fn try_from(value: TriggerConfigParams) -> Result<Self, Self::Error> {
        match value {
            TriggerConfigParams::Scheduled {
                schedule,
                recurring,
            } => Ok(Self::Scheduled {
                schedule: schedule.try_into()?,
                recurring,
            }),
            TriggerConfigParams::InterceptMatch { rule_id } => {
                Ok(Self::InterceptMatch { rule_id })
            }
            TriggerConfigParams::NewNode => Ok(Self::NewNode),
        }
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct TriggerTargetParams {
    #[schemars(description = "Full node IDs to target. Empty targets all registered nodes.")]
    #[serde(default)]
    pub node_ids: Vec<String>,

    #[schemars(description = "Optional case-insensitive substring filter on node OS details")]
    pub os_filter: Option<String>,

    #[schemars(
        description = "Agent short names to target, e.g. 'codex' or 'claudecode'. Matching against discovered agents is exact and case-sensitive -- call node_list/agent_list first to confirm the real short name rather than guessing one. Empty targets all available agents."
    )]
    #[serde(default)]
    pub agent_short_names: Vec<String>,

    #[schemars(
        description = "For event triggers, include the node that caused the event even when node_ids would otherwise exclude it"
    )]
    #[serde(default)]
    pub include_triggering_node: bool,
}

impl From<TriggerTargetParams> for TargetSpec {
    fn from(value: TriggerTargetParams) -> Self {
        Self {
            node_ids: value.node_ids,
            os_filter: value.os_filter,
            agent_short_names: value.agent_short_names,
            include_triggering_node: value.include_triggering_node,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TriggerCreateParams {
    #[schemars(description = "Chain display name or chain ID prefix")]
    pub chain: String,

    #[schemars(
        description = "Trigger configuration. Use {\"type\":\"new_node\"} to fire whenever a node registers."
    )]
    pub trigger: TriggerConfigParams,

    #[schemars(description = "Nodes and agents that should execute the chain")]
    #[serde(default)]
    pub target: TriggerTargetParams,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TriggerIdParams {
    #[schemars(description = "Trigger ID prefix returned by trigger_list")]
    pub trigger_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TriggerToggleParams {
    #[schemars(description = "Trigger ID prefix returned by trigger_list")]
    pub trigger_id: String,

    #[schemars(description = "Whether the trigger should be active")]
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_new_node_trigger_with_targeting() {
        let params: TriggerCreateParams = serde_json::from_value(serde_json::json!({
            "chain": "CI/CD",
            "trigger": { "type": "new_node" },
            "target": {
                "agent_short_names": ["codex"],
                "include_triggering_node": true
            }
        }))
        .unwrap();

        assert!(matches!(params.trigger, TriggerConfigParams::NewNode));
        assert_eq!(params.target.agent_short_names, ["codex"]);
        assert!(params.target.include_triggering_node);
    }

    #[test]
    fn parses_linear_chain_with_defaults() {
        let params: ChainCreateParams = serde_json::from_value(serde_json::json!({
            "name": "CI/CD on connect",
            "operations": ["custom::cicd"]
        }))
        .unwrap();

        assert_eq!(params.operations, ["custom::cicd"]);
        assert_eq!(params.category, "custom");
        assert!(params.description.is_empty());
        assert_eq!(params.timeout, None);
    }

    #[test]
    fn parses_scheduled_trigger() {
        let params: TriggerCreateParams = serde_json::from_value(serde_json::json!({
            "chain": "Daily checks",
            "trigger": {
                "type": "scheduled",
                "schedule": { "type": "daily_at", "hour": 3, "minute": 30 },
                "recurring": true
            }
        }))
        .unwrap();

        assert!(matches!(
            params.trigger,
            TriggerConfigParams::Scheduled {
                schedule: TriggerScheduleParams::DailyAt {
                    hour: 3,
                    minute: 30
                },
                recurring: true
            }
        ));
    }

    #[test]
    fn rejects_invalid_schedules() {
        let schedule = TriggerScheduleParams::DailyAt {
            hour: 24,
            minute: 0,
        };
        assert_eq!(
            ScheduleSpec::try_from(schedule).unwrap_err(),
            "daily_at hour must be between 0 and 23"
        );

        let schedule = TriggerScheduleParams::Interval { minutes: 0 };
        assert_eq!(
            ScheduleSpec::try_from(schedule).unwrap_err(),
            "interval minutes must be at least 1"
        );
    }
}
