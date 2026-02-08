export interface NodeState {
  node_id: string;
  machine_name: string;
  os_details: string;
  discovered_agents: DiscoveredAgent[];
  selected_agent: SelectedAgent | null;
  intercept_active: boolean;
  intercept_supported: boolean;
  agent_discovery_enabled: boolean;
  discovered_endpoints_count: number;
  //
  // ISO datetime.
  //
  last_update: string;
  //
  // Active terminal session ID (if any).
  //
  active_terminal_id?: string | null;
}

export interface DiscoveredAgent {
  name: string;
  short_name: string;
  available: boolean;
  version?: string;
}

export interface SelectedAgent {
  short_name: string;
  session_id: string | null;
  process_name: string | null;
  yolo_mode: boolean;
  working_dir: string | null;
}

//
// Recon types - tool and config discovery.
//
export interface AgentTool {
  name: string;
  description: string;
  context_path?: string | null;
}

export interface ReconTools {
  mcp_servers: McpServer[];
  skills: AgentTool[];
  internal_tools: AgentTool[];
}

export interface ConfigItem {
  path: string;
  contents?: string;
  config_type: string;
}

export interface ReconMetadata {
  user_identities?: string[];
  api_keys?: string[];
}

export interface ReconResult {
  tools: ReconTools;
  config: ConfigItem[];
  sessions: SessionItem[];
  project_paths: string[];
  metadata?: ReconMetadata;
}

export interface McpServer {
  name: string;
  transport: McpTransport;
  address: string | null;
  command: string | null;
  tools: AgentTool[];
  context_path?: string | null;
}

export type McpTransport = 'Stdio' | 'Sse' | 'WebSocket';

export interface SystemState {
  //
  // ISO datetime.
  //
  timestamp: string;
  nodes: NodeState[];
}

//
// Session item (for recon).
//
export interface SessionItem {
  session_id: string;
  context_path: string;
  session_file: string;
  last_modified: string;
  message_count: number;
  content?: string;
}

//
// Session Context for creating sessions with specific parameters.
//
export interface SessionContext {
  working_dir?: string;
  yolo_mode?: boolean;
}

//
// Commands.
//
export type NodeCommand =
  | { Agent: AgentCommand }
  | { Session: SessionCommand }
  | { Intercept: InterceptCommand }
  | { Terminal: TerminalCommand }
  | { Config: ConfigCommand }
  | { AgentRegistry: AgentRegistryCommand };

export type AgentRegistryCommand =
  | { Update: { scripts: string[] } }
  | 'List';

export type AgentCommand =
  | 'Update'
  | 'Recon'
  | 'ReconSemantic'
  | { Select: { short_name: string } }
  | { UpdateConfigFile: { path: string; contents: string } }
  | { GetSessionContent: { session_file: string } }
  | { GetConfigContent: { config_path: string } };

export type SessionCommand =
  | { Create: { context: SessionContext } }
  | 'Close'
  | { Prompt: { text: string; transaction_id: string } }
  | { CancelTransaction: { transaction_id: string } };

//
// Interception method. Proxy works on all platforms. VPN works on Windows and
// Linux. Hosts works on all platforms. Tproxy is Linux-only.
//
export type InterceptMethod = 'Proxy' | 'Vpn' | 'Hosts' | 'Tproxy';

export type InterceptCommand =
  | { Enable: { method: InterceptMethod | null } }
  | 'Disable';

export type TerminalCommand =
  | 'Create'
  | { Write: { data: number[] } }
  | { Resize: { rows: number; cols: number } }
  | 'Close';

export type ConfigCommand = { SetReportInterval: { interval_secs: number } };

export interface CommandRequest {
  command_id: string;
  client_id: string;
  node_id: string;
  command: NodeCommand;
}

//
// Command Results.
//
export type NodeCommandResult =
  | { Agent: AgentCommandResult }
  | { Session: SessionCommandResult }
  | { Intercept: InterceptCommandResult }
  | { Terminal: TerminalCommandResult }
  | { Config: ConfigCommandResult }
  | { AgentRegistry: AgentRegistryCommandResult }
  | { Error: { message: string } };

export interface LuaRegisteredAgentInfo {
  name: string;
  short_name: string;
  source: string;
  source_path?: string | null;
  loaded_at: string;
}

export type AgentRegistryCommandResult =
  | { Updated: { agent_count: number } }
  | { Listed: { agents: LuaRegisteredAgentInfo[] } };

export interface LuaAgentScriptInfo {
  id: string;
  name: string;
  script: string;
  created_at: string;
  updated_at: string;
}

export type AgentCommandResult =
  | 'UpdateSent'
  | { ReconComplete: { result: ReconResult } }
  | { Selected: { short_name: string } }
  | { YoloSet: { enabled: boolean } }
  | { ConfigFileUpdated: { success: boolean; error?: string } }
  | { SessionContent: { session_file: string; content?: string; error?: string } }
  | { ConfigContent: { config_path: string; content?: string; error?: string } };

export type SessionCommandResult =
  | { Created: { session_id: string } }
  | 'Closed'
  | { PromptResponse: { transaction_id: string; response: string } }
  | { TransactionCancelled: { transaction_id: string } };

export type InterceptCommandResult =
  | { Enabled: { method: InterceptMethod } }
  | 'Disabled';

export type TerminalCommandResult =
  | { Created: { terminal_id: string } }
  | 'Written'
  | 'Resized'
  | 'Closed';

export type ConfigCommandResult = { ReportIntervalSet: { interval_secs: number } };

export interface CommandResponse {
  command_id: string;
  node_id: string;
  result: NodeCommandResult;
}

export interface TerminalOutput {
  node_id: string;
  terminal_id: string;
  client_id: string;
  data: number[];
}

//
// Event Log.
//
export interface EventLogEntry {
  //
  // ISO datetime.
  //
  timestamp: string;
  message_name: string;
  details: string;
}

//
// Semantic Operations
// Note: LLM provider config (api_key, provider, model) is managed service-side.
//
export interface SemanticOperationSpec {
  name: string;
  description: string;
  agent_info: string;
  timeout: number;
  operation_prompt: string;
  mode: string;
  agent_iterations: number;
  yolo_mode: boolean;
  model_ref?: string | null;
}

export type SemanticOpStatus = 'Queued' | 'Running' | 'Completed' | 'Failed' | 'Cancelled';

export interface SemanticOpUpdate {
  operation_id: string;
  node_id: string;
  agent_short_name: string;
  spec: SemanticOperationSpec;
  status: SemanticOpStatus;
  start_time: string;
  end_time: string | null;
  /** Brief summary of actions taken (for display in UI header) */
  summary: string | null;
  /** Actual findings/data/output from the operation */
  result: string | null;
  queue_position: number | null;
  output: string | null;
}

//
// Library Item types - unified view of operations and chains.
//

export type LibraryItemType = 'operation' | 'chain';

export interface LibraryItem {
  id: string;
  type: LibraryItemType;
  name: string;
  description: string;
  category: string;
  shortName?: string;
  disabled: boolean;
  //
  // For operations: mode, timeout, yolo_mode.
  // For chains: element_count, operation_count.
  //
  mode?: string;
  timeout?: number;
  yoloMode?: boolean;
  elementCount?: number;
  operationCount?: number;
}

//
// Operation Definition (stored in service database).
//
export interface OperationDefinitionInfo {
  full_name: string;
  category: string;
  short_name: string;
  name: string;
  description: string;
  agent_info: string;
  timeout: number;
  operation_prompt: string;
  mode: string;
  agent_iterations: number;
  //
  // DEPRECATED: use chains instead.
  //
  operation_chain: string[];
  disabled: boolean;
  yolo_mode: boolean;
  model_ref?: string | null;
}

//
// Chain Definitions - Visual workflow chains of semantic operations.
//

export type ChainTriggerType = { type: 'Manual' };

export type ChainTerminationType =
  | { type: 'Raw' }
  | { type: 'Semantic'; prompt: string; model_ref?: string | null };

//
// Session group for elements that share a session.
//
export interface SessionGroup {
  id: string;
  color: string;
  yolo_mode: boolean;
}

//
// Note: Positions are not stored - they are computed dynamically using Dagre
// layout.
//
export type ChainElement =
  | { element_type: 'Trigger'; id: string; trigger_type: ChainTriggerType }
  | { element_type: 'Operation'; id: string; operation_name: string; model_ref?: string | null; session_group?: SessionGroup | null }
  | { element_type: 'Transform'; id: string; prompt: string; model_ref?: string | null; session_group?: SessionGroup | null }
  | { element_type: 'GenericPrompt'; id: string; prompt: string; session_group?: SessionGroup | null }
  | { element_type: 'Termination'; id: string; termination_type: ChainTerminationType; label: string };

export interface ChainConnection {
  id: string;
  from_element: string;
  to_element: string;
  from_port: number;
  to_port: number;
}

export interface ChainDefinitionInput {
  name: string;
  description: string;
  category: string;
  elements: ChainElement[];
  connections: ChainConnection[];
  disabled?: boolean;
  timeout?: number;
}

export interface ChainDefinitionFull {
  id: string;
  name: string;
  description: string;
  category: string;
  elements: ChainElement[];
  connections: ChainConnection[];
  disabled: boolean;
  timeout?: number;
  created_at: string;
  updated_at: string;
}

export interface ChainDefinitionInfo {
  id: string;
  name: string;
  description: string;
  category: string;
  disabled: boolean;
  timeout?: number;
  element_count: number;
  operation_count: number;
  created_at: string;
  updated_at: string;
}

export type ChainExecutionStatus = 'Queued' | 'Running' | 'Completed' | 'Failed' | 'Cancelled';

export type ElementExecutionStatus =
  | 'Pending'
  | 'WaitingForInputs'
  | 'Running'
  | { Completed: { output: string } }
  | { Failed: { error: string } }
  | 'Skipped';

//
// Element configuration (static, from chain definition).
//
export type ElementConfig =
  | { type: 'Trigger' }
  | { type: 'Operation'; operation_name: string; model_ref?: string | null }
  | { type: 'Transform'; prompt: string; model_ref?: string | null }
  | { type: 'GenericPrompt'; prompt: string }
  | { type: 'RawOutput' }
  | { type: 'SemanticOutput'; prompt: string; model_ref?: string | null };

//
// Element runtime context (dynamic, during execution).
//
export interface ElementContext {
  input: string;
  session_id?: string | null;
  yolo_mode: boolean;
  is_first_in_session?: boolean;
}

export interface ElementExecution {
  element_id: string;
  status: ElementExecutionStatus;
  config?: ElementConfig | null;
  context?: ElementContext | null;
  started_at: string | null;
  completed_at: string | null;
}

export interface ChainExecutionUpdate {
  execution_id: string;
  chain_id: string;
  chain_name: string;
  node_id: string;
  agent_short_name: string;
  status: ChainExecutionStatus;
  elements: Record<string, ElementExecution>;
  started_at: string;
  ended_at: string | null;
  outputs: Record<string, string>;
}

//
// Orchestrator Plan types.
//
export type PlanStepStatus = 'not_started' | 'in_progress' | 'done';

export interface PlanStep {
  description: string;
  status: PlanStepStatus;
}

export interface OrchestratorPlan {
  steps: PlanStep[];
  summary?: string;
  current_step_description?: string;
}

//
// Traffic Interception Types.
//
export type TrafficDirection = 'send' | 'receive';
export type TargetDirection = 'send' | 'receive' | 'both';
export type RuleScope =
  | 'all'
  | { node: { node_id: string } }
  | { agent: { node_id: string; agent_short_name: string } };

export interface InterceptedTrafficEntry {
  id: number | null;
  timestamp: string;
  node_id: string;
  agent_short_name: string;
  intercept_method: InterceptMethod;
  direction: TrafficDirection;
  method: string | null;
  url: string;
  host: string;
  request_headers: Record<string, string> | null;
  request_body: number[] | null;
  response_status: number | null;
  response_headers: Record<string, string> | null;
  response_body: number[] | null;
}

export interface InterceptRule {
  id: number | null;
  name: string;
  regex_pattern: string;
  target_direction: TargetDirection;
  scope: RuleScope;
  enabled: boolean;
  summarization_prompt: string | null;
  created_at: string;
  updated_at: string;
}

export interface TrafficMatch {
  id: number;
  traffic_id: number;
  rule_id: number;
  rule_name: string;
  matched_at: string;
  summary: string | null;
}

export interface TrafficMatchWithDetails {
  match_info: TrafficMatch;
  traffic: InterceptedTrafficEntry;
}

export interface TrafficLogFilters {
  node_id: string | null;
  agent_short_name: string | null;
  start_time: string | null;
  end_time: string | null;
  url_pattern: string | null;
  direction: TrafficDirection | null;
  limit: number;
  offset: number;
}

export interface InterceptStatus {
  node_id: string;
  enabled: boolean;
  method: InterceptMethod | null;
  proxy_port: number | null;
  intercepted_domains: string[];
}

//
// Agent Discovery Types.
//
// Node Event Log entry.
//
export interface ApplicationLogEntry {
  source: string;
  level: string;
  message: string;
  target: string | null;
  timestamp: string;
}

export interface DiscoveredLlmEndpoint {
  id: string;
  ip_address: string;
  domain: string | null;
  port: number;
  is_https: boolean;
  models: string[];
  base_url: string;
  api_key: string | null;
  discovered_at: string;
  node_id: string;
}

//
// Agent Chat types - IRC-style multi-agent chat system.
//
export type AgentChatAgentStatus = 'Initializing' | 'Ready' | 'Waiting' | 'Prompting' | 'Disconnected';

export interface AgentChatAgentInfo {
  id: string;
  node_id: string;
  agent_short_name: string;
  nickname: string;
  precedence: number;
  current_channel_id: string | null;
  status: AgentChatAgentStatus;
}

export interface AgentChatChannelInfo {
  id: string;
  name: string;
  topic: string | null;
  member_count: number;
  created_by: string;
}

export type AgentChatMessageType = 'Channel' | 'DirectMessage' | 'System' | 'CommandResult';

export interface AgentChatMessageInfo {
  id: number;
  channel_id: string | null;
  sender_nickname: string;
  recipient_nickname: string | null;
  message_type: AgentChatMessageType;
  content: string;
  timestamp: string;
}

export interface AgentChatSessionState {
  id: string;
  goal: string | null;
  status: string;
  agents: AgentChatAgentInfo[];
  channels: AgentChatChannelInfo[];
  created_at: string;
}

//
// WebSocket Messages (Browser -> Server).
//
export type BrowserMessage =
  | { type: 'command'; payload: CommandRequest }
  | { type: 'terminal_write'; node_id: string; terminal_id: string; data: number[] }
  | { type: 'semantic_op_run'; node_id: string; agent_short_name: string; operation_name: string; working_dir: string | null }
  | { type: 'semantic_op_cancel'; operation_id: string }
  | { type: 'semantic_op_remove'; operation_id: string }
  | { type: 'semantic_op_clear' }
  | { type: 'semantic_op_list_request' }
  | { type: 'remove_node'; node_id: string }
  | { type: 'config_get'; keys: string[] }
  | { type: 'config_set'; values: Record<string, string> }
  | { type: 'op_def_add'; content: string }
  | { type: 'op_def_list' }
  | { type: 'op_def_delete'; full_name: string }
  | { type: 'op_def_get'; full_name: string }
  | { type: 'orchestrator_start' }
  | { type: 'orchestrator_prompt'; message: string }
  | { type: 'orchestrator_stop' }
  | { type: 'orchestrator_cancel' }
  //
  // Traffic interception messages.
  //
  | { type: 'traffic_log_request'; filters: TrafficLogFilters }
  | { type: 'traffic_matches_request'; rule_id: number | null; limit: number; offset: number }
  | { type: 'traffic_clear' }
  | { type: 'intercept_rule_list' }
  | { type: 'intercept_rule_create'; name: string; regex_pattern: string; target_direction: TargetDirection; scope: RuleScope; summarization_prompt?: string | null }
  | { type: 'intercept_rule_update'; id: number; name?: string; regex_pattern?: string; target_direction?: TargetDirection; scope?: RuleScope; enabled?: boolean; summarization_prompt?: string | null }
  | { type: 'intercept_rule_delete'; id: number }
  | { type: 'intercept_enable'; node_id: string; method?: InterceptMethod | null }
  | { type: 'intercept_disable'; node_id: string }
  //
  // Chain messages.
  //
  | { type: 'chain_def_list' }
  | { type: 'chain_get'; chain_id: string }
  | { type: 'chain_create'; definition: ChainDefinitionInput }
  | { type: 'chain_update'; chain_id: string; definition: ChainDefinitionInput }
  | { type: 'chain_delete'; chain_id: string }
  | { type: 'chain_run'; chain_id: string; node_id: string; agent_short_name: string; working_dir: string | null }
  | { type: 'chain_cancel'; execution_id: string }
  | { type: 'chain_execution_list' }
  | { type: 'chain_execution_remove'; execution_id: string }
  | { type: 'chain_execution_clear' }
  //
  // Agent discovery messages.
  //
  | { type: 'agent_discovery_enable'; node_id: string }
  | { type: 'agent_discovery_disable'; node_id: string }
  | { type: 'discovered_endpoints_request'; node_id: string | null }
  //
  // Node event log messages.
  //
  | { type: 'application_log_request'; node_id: string; level_filter: string[] | null; regex_filter: string | null; limit: number; offset: number }
  | { type: 'application_log_clear'; node_id: string | null }
  //
  // Recon messages.
  //
  | { type: 'recon_get'; node_id: string; agent_short_name: string }
  //
  // Lua agent script messages.
  //
  | { type: 'lua_agent_script_add'; name: string; script: string }
  | { type: 'lua_agent_script_update'; script_id: string; name: string; script: string }
  | { type: 'lua_agent_script_delete'; script_id: string }
  | { type: 'lua_agent_script_reset_defaults' }
  | { type: 'lua_agent_script_list' }
  //
  // Agent Chat messages.
  //
  | { type: 'agent_chat_start'; goal: string | null; yolo_mode: boolean }
  | { type: 'agent_chat_stop'; session_id: string }
  | { type: 'agent_chat_add_agent'; session_id: string; node_id: string; agent_short_name: string }
  | { type: 'agent_chat_remove_agent'; session_id: string; agent_id: string }
  | { type: 'agent_chat_reorder_agents'; session_id: string; agent_ids: string[] }
  | { type: 'agent_chat_send_message'; session_id: string; content: string; channel_id: string | null; recipient_nickname: string | null }
  | { type: 'agent_chat_join_channel'; session_id: string; channel_name: string }
  | { type: 'agent_chat_get_history'; session_id: string; channel_id: string | null; limit: number }
  | { type: 'agent_chat_get_state'; session_id: string | null };

//
// WebSocket Messages (Server -> Browser).
//
export type ServerMessage =
  | { type: 'connected'; client_id: string; version: string }
  | { type: 'state_update'; state: SystemState }
  | { type: 'command_response'; response: CommandResponse }
  | { type: 'terminal_output'; output: TerminalOutput }
  | { type: 'semantic_op_update'; update: SemanticOpUpdate }
  | { type: 'semantic_op_list'; operations: SemanticOpUpdate[] }
  | { type: 'semantic_op_queued'; operation_id: string; queue_position: number }
  | { type: 'config_response'; values: Record<string, string> }
  | { type: 'config_saved' }
  | { type: 'event_log'; entry: EventLogEntry }
  | { type: 'error'; message: string }
  | { type: 'op_def_list'; definitions: OperationDefinitionInfo[] }
  | { type: 'op_def_get_response'; definition: OperationDefinitionInfo | null }
  | { type: 'op_def_added'; full_name: string }
  | { type: 'op_def_deleted'; full_name: string; success: boolean }
  | { type: 'op_def_error'; message: string }
  | { type: 'orchestrator_started' }
  | { type: 'orchestrator_content'; content: string }
  | { type: 'orchestrator_tool_executing'; name: string; input?: string }
  | { type: 'orchestrator_tool_executed'; name: string; display: string; success: boolean; result: string }
  | { type: 'orchestrator_plan_updated'; plan: OrchestratorPlan }
  | { type: 'orchestrator_done' }
  | { type: 'orchestrator_stopped' }
  | { type: 'orchestrator_error'; message: string }
  | { type: 'orchestrator_token_usage'; prompt_tokens: number; completion_tokens: number; total_tokens: number }
  //
  // Traffic interception messages.
  //
  | { type: 'traffic_log_response'; entries: InterceptedTrafficEntry[]; total_count: number }
  | { type: 'traffic_matches_response'; matches: TrafficMatchWithDetails[]; total_count: number }
  | { type: 'traffic_cleared'; deleted_count: number }
  | { type: 'intercept_rule_list'; rules: InterceptRule[] }
  | { type: 'intercept_rule_created'; rule: InterceptRule }
  | { type: 'intercept_rule_updated'; rule: InterceptRule }
  | { type: 'intercept_rule_deleted'; id: number; success: boolean }
  | { type: 'intercept_rule_error'; message: string }
  | { type: 'intercept_status_update'; status: InterceptStatus }
  //
  // Chain messages.
  //
  | { type: 'chain_def_list'; chains: ChainDefinitionInfo[] }
  | { type: 'chain_get_response'; chain: ChainDefinitionFull | null }
  | { type: 'chain_created'; chain: ChainDefinitionInfo }
  | { type: 'chain_updated'; chain: ChainDefinitionInfo }
  | { type: 'chain_deleted'; chain_id: string; success: boolean }
  | { type: 'chain_error'; message: string }
  | { type: 'chain_execution_started'; execution_id: string; chain_id: string }
  | { type: 'chain_execution_update'; execution: ChainExecutionUpdate }
  | { type: 'chain_execution_list'; executions: ChainExecutionUpdate[] }
  //
  // Agent discovery messages.
  //
  | { type: 'discovered_endpoints_list'; endpoints: DiscoveredLlmEndpoint[] }
  | { type: 'agent_discovery_error'; message: string }
  //
  // Node event log messages.
  //
  | { type: 'application_log_response'; node_id: string; entries: ApplicationLogEntry[]; total_count: number }
  | { type: 'application_log_cleared'; deleted_count: number }
  //
  // Recon messages.
  //
  | { type: 'recon_get_response'; node_id: string; agent_short_name: string; recon_result: ReconResult | null; performed_at: string | null; is_semantic: boolean | null }
  //
  // Lua agent script messages.
  //
  | { type: 'lua_agent_script_added'; id: string; name: string }
  | { type: 'lua_agent_script_updated'; id: string; name: string }
  | { type: 'lua_agent_script_deleted'; script_id: string; success: boolean }
  | { type: 'lua_agent_script_defaults_reset'; count: number }
  | { type: 'lua_agent_script_list'; scripts: LuaAgentScriptInfo[] }
  //
  // Agent Chat messages.
  //
  | { type: 'agent_chat_session_started'; session_id: string; goal: string | null }
  | { type: 'agent_chat_session_stopped'; session_id: string }
  | { type: 'agent_chat_agent_added'; session_id: string; agent: AgentChatAgentInfo }
  | { type: 'agent_chat_agent_removed'; session_id: string; agent_id: string }
  | { type: 'agent_chat_agent_status_changed'; session_id: string; agent_id: string; status: AgentChatAgentStatus }
  | { type: 'agent_chat_channel_created'; session_id: string; channel: AgentChatChannelInfo }
  | { type: 'agent_chat_channel_updated'; session_id: string; channel: AgentChatChannelInfo }
  | { type: 'agent_chat_agent_joined_channel'; session_id: string; agent_id: string; channel_id: string }
  | { type: 'agent_chat_agent_left_channel'; session_id: string; agent_id: string; channel_id: string }
  | { type: 'agent_chat_message'; session_id: string; message: AgentChatMessageInfo }
  | { type: 'agent_chat_state_update'; session: AgentChatSessionState }
  | { type: 'agent_chat_history_response'; session_id: string; channel_id: string | null; messages: AgentChatMessageInfo[] }
  | { type: 'agent_chat_error'; message: string };
