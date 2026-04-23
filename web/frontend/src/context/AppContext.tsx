import { createContext, useContext, useReducer, useEffect, useCallback, useMemo, useRef, type ReactNode, type Dispatch } from 'react';
import { wsClient } from '../api/websocket';
import { generateUUID } from '../utils/uuid';
import type { OrchestratorState, OrchestratorSessionState } from './orchestratorTypes';

//
// Re-export Orchestrator types for consumers.
//
export type { OrchestratorMessage, OrchestratorToolExecution, OrchestratorSessionState } from './orchestratorTypes';
import { loadPersistedOrchestratorState, loadRecentNodes, persistRecentNodes, persistOrchestratorState } from '../utils/persistence';
import type {
  SystemState,
  NodeState,
  SemanticOpUpdate,
  CommandResponse,
  TerminalOutput,
  ServerMessage,
  CommandRequest,
  EventLogEntry,
  OperationDefinitionInfo,
  BrowserMessage,
  OrchestratorPlan,
  InterceptedTrafficEntry,
  InterceptMethod,
  InterceptRule,
  TrafficMatchWithDetails,
  InterceptStatus,
  TrafficLogFilters,
  TargetDirection,
  RuleScope,
  ChainDefinitionInfo,
  ChainDefinitionFull,
  ChainDefinitionInput,
  ChainExecutionUpdate,
  ChainTriggerInfo,
  TriggerConfig,
  TargetSpec,
  PayloadInfo,
  AgentChatAgentInfo,
  AgentChatAgentStatus,
  AgentChatChannelInfo,
  AgentChatMessageInfo,
  AgentChatSessionState,
  LuaAgentScriptInfo,
  ToolkitExecuteResult,
  ToolkitApplyOutcome,
  ToolkitModelOption,
  ToolkitReconTarget,
  ToolkitToolInfo,
} from '../api/types';

//
// Agent session message types.
//
export interface AgentSessionMessage {
  role: 'user' | 'assistant';
  content: string;
  timestamp: Date;
}

//
// ACP JSON-RPC helpers.
//

let nextAcpId = 1;

function acpRequest(method: string, params?: unknown): string {
  const id = nextAcpId++;
  return JSON.stringify({ jsonrpc: '2.0', id, method, params });
}

interface AcpJsonRpc {
  jsonrpc: string;
  id?: number | string;
  method?: string;
  params?: Record<string, unknown>;
  result?: Record<string, unknown>;
  error?: { code: number; message: string };
}

//
// Track pending ACP request IDs to correlate responses.
//
interface PendingAcpRequest {
  method: string;
  sessionId?: string;
  label?: string;
  //
  // Optional generic resolve/reject for callers using `sendAcpNodeRequest`.
  // When present, the response handler invokes these instead of (or in
  // addition to) the orchestrator-specific switch.
  //
  resolve?: (result: unknown, text: string) => void;
  reject?: (reason: unknown) => void;
  //
  // When true, accumulated `agent_message_chunk` text for the associated
  // sessionId is appended to `textBuf` and returned on resolve.
  //
  collectText?: boolean;
  textBuf?: string;
}

const initialOrchestratorState: OrchestratorState = {
  sessions: [],
  activeSessionId: null,
  isStarting: false,
  nextRequestId: 1,
};

const MAX_RECENT_NODES = 3;

//
// Intercept state.
//
interface InterceptState {
  trafficLog: InterceptedTrafficEntry[];
  trafficTotalCount: number;
  trafficMatches: TrafficMatchWithDetails[];
  matchesTotalCount: number;
  rules: InterceptRule[];
  nodeStatus: Map<string, InterceptStatus>;
  ruleError: string | null;
}

const initialInterceptState: InterceptState = {
  trafficLog: [],
  trafficTotalCount: 0,
  trafficMatches: [],
  matchesTotalCount: 0,
  rules: [],
  nodeStatus: new Map(),
  ruleError: null,
};

//
// Chain state.
//
interface ChainState {
  chains: ChainDefinitionInfo[];
  currentChain: ChainDefinitionFull | null;
  chainDefinitionsCache: Record<string, ChainDefinitionFull>;
  loadingChains: Set<string>;
  executions: ChainExecutionUpdate[];
  triggers: ChainTriggerInfo[];
  chainError: string | null;
  chainSuccess: string | null;
  lastCreatedChainId: string | null;
}

const initialChainState: ChainState = {
  chains: [],
  currentChain: null,
  chainDefinitionsCache: {},
  loadingChains: new Set(),
  executions: [],
  triggers: [],
  chainError: null,
  chainSuccess: null,
  lastCreatedChainId: null,
};

//
// Agent Chat state.
//
interface AgentChatState {
  session: AgentChatSessionState | null;
  currentChannelId: string | null;
  messages: AgentChatMessageInfo[];
  isLoading: boolean;
  error: string | null;
}

interface ToolkitState {
  tools: ToolkitToolInfo[];
  models: ToolkitModelOption[];
  reconTargets: ToolkitReconTarget[];
  executeResult: ToolkitExecuteResult | null;
  applyResults: ToolkitApplyOutcome[] | null;
  executionProgress: { current: number; total: number } | null;
  error: string | null;
}

//
// LogQuery state.
//
interface LogQueryState {
  query: string;
  isRunning: boolean;
  columns: string[];
  rows: unknown[][];
  totalCount: number;
  error: string | null;
}

const initialLogQueryState: LogQueryState = {
  query: '',
  isRunning: false,
  columns: [],
  rows: [],
  totalCount: 0,
  error: null,
};

const initialAgentChatState: AgentChatState = {
  session: null,
  currentChannelId: null,
  messages: [],
  isLoading: false,
  error: null,
};

const initialToolkitState: ToolkitState = {
  tools: [],
  models: [],
  reconTargets: [],
  executeResult: null,
  applyResults: null,
  executionProgress: null,
  error: null,
};

//
// State.
//
interface AppState {
  connected: boolean;
  clientId: string | null;
  version: string | null;
  systemState: SystemState | null;
  operations: SemanticOpUpdate[];
  operationDefs: OperationDefinitionInfo[];
  events: EventLogEntry[];
  config: Record<string, string>;
  opDefError: string | null;
  opDefSuccess: string | null;
  orchestrator: OrchestratorState;
  intercept: InterceptState;
  logQuery: LogQueryState;
  chains: ChainState;
  agentChat: AgentChatState;
  toolkit: ToolkitState;
  luaAgentScripts: LuaAgentScriptInfo[];
  payloads: PayloadInfo[];
  //
  // Agent session messages keyed by session_id.
  //
  agentSessionMessages: Record<string, AgentSessionMessage[]>;
  //
  // Session streaming state for ACP agents.
  //
  agentSessionStreaming: Record<string, {
    content: string;
    transactionId: string;
    toolCalls: Array<{ toolName: string; toolId: string; input: string; output?: string; isError?: boolean }>;
    pendingPermission: { permissionId: string; toolName: string; toolInput: string } | null;
    agentStatus: string | null;
    hadToolCall: boolean;
  }>;
  //
  // Recently accessed node IDs (most recent first).
  //
  recentlyAccessedNodeIds: string[];
  //
  // Per-node agent sessions. Keyed by `${nodeId}|${agentShortName}` so a
  // single node can host multiple concurrent sessions, one per connector.
  // Each entry stores the ACP sessionId plus the originating (nodeId,
  // agentShortName) pair for ergonomic lookup.
  //
  nodeSessions: Record<string, { nodeId: string; agentShortName: string; sessionId: string }>;
}

//
// Stable composite key for nodeSessions / agentSessionStreaming so the
// same (nodeId, agentShortName) pair always hashes to one entry.
//

export function nodeSessionKey(nodeId: string, agentShortName: string): string {
  return `${nodeId}|${agentShortName}`;
}

//
// Use a function to create initial state so we can load persisted data.
//

function createInitialState(): AppState {
  return {
    connected: false,
    clientId: null,
    version: null,
    systemState: null,
    operations: [],
    operationDefs: [],
    events: [],
    config: {},
    opDefError: null,
    opDefSuccess: null,
    orchestrator: loadPersistedOrchestratorState(initialOrchestratorState),
    intercept: initialInterceptState,
    logQuery: initialLogQueryState,
    chains: initialChainState,
    agentChat: initialAgentChatState,
    toolkit: initialToolkitState,
    luaAgentScripts: [],
    payloads: [],
    agentSessionMessages: {},
    agentSessionStreaming: {},
    recentlyAccessedNodeIds: loadRecentNodes(MAX_RECENT_NODES),
    nodeSessions: {},
  };
}

//
// Actions.
//
type Action =
  | { type: 'SET_CONNECTED'; connected: boolean; clientId?: string; version?: string }
  | { type: 'SET_STATE'; state: SystemState }
  | { type: 'SET_OPERATIONS'; operations: SemanticOpUpdate[] }
  | { type: 'UPDATE_OPERATION'; update: SemanticOpUpdate }
  | { type: 'SET_OPERATION_DEFS'; definitions: OperationDefinitionInfo[] }
  | { type: 'ADD_EVENT'; entry: EventLogEntry }
  | { type: 'CLEAR_EVENTS' }
  | { type: 'SET_CONFIG'; values: Record<string, string> }
  | { type: 'SET_OP_DEF_ERROR'; error: string | null }
  | { type: 'SET_OP_DEF_SUCCESS'; fullName: string | null }
  | { type: 'ORCHESTRATOR_CREATING_SESSION' }
  | { type: 'ORCHESTRATOR_SESSION_CREATED'; sessionId: string; label: string; loaded?: boolean; provider?: string; model?: string }
  | { type: 'ORCHESTRATOR_SESSION_STARTED'; sessionId: string; provider: string; model: string }
  | { type: 'ORCHESTRATOR_SESSION_CLOSED'; sessionId: string }
  | { type: 'ORCHESTRATOR_SESSION_LOADED'; sessionId: string }
  | { type: 'ORCHESTRATOR_SYNC_SESSIONS'; sessionIds: string[] }
  | { type: 'ORCHESTRATOR_SET_ACTIVE_SESSION'; sessionId: string | null }
  | { type: 'ORCHESTRATOR_ADD_USER_MESSAGE'; sessionId: string; message: string; promptId: string }
  | { type: 'ORCHESTRATOR_ADD_CONTENT'; sessionId: string; content: string }
  | { type: 'ORCHESTRATOR_TOOL_EXECUTING'; sessionId: string; name: string; input?: string }
  | { type: 'ORCHESTRATOR_TOOL_EXECUTED'; sessionId: string; name: string; display: string; success: boolean; result: string }
  | { type: 'ORCHESTRATOR_PLAN_UPDATED'; sessionId: string; plan: OrchestratorPlan }
  | { type: 'ORCHESTRATOR_DONE'; sessionId: string }
  | { type: 'ORCHESTRATOR_ERROR'; sessionId: string; message: string }
  | { type: 'ORCHESTRATOR_CLEAR_MESSAGES'; sessionId: string }
  | { type: 'ORCHESTRATOR_TOKEN_USAGE'; sessionId: string; promptTokens: number; completionTokens: number; totalTokens: number }
  //
  // LogQuery actions.
  //
  | { type: 'LOG_QUERY_SET_QUERY'; query: string }
  | { type: 'LOG_QUERY_START' }
  | { type: 'LOG_QUERY_RESPONSE'; columns: string[]; rows: unknown[][]; totalCount: number }
  | { type: 'LOG_QUERY_ERROR'; message: string }
  //
  // Intercept actions.
  //
  | { type: 'SET_TRAFFIC_LOG'; entries: InterceptedTrafficEntry[]; totalCount: number }
  | { type: 'SET_TRAFFIC_MATCHES'; matches: TrafficMatchWithDetails[]; totalCount: number }
  | { type: 'SET_TRAFFIC_CLEARED'; deletedCount: number }
  | { type: 'SET_INTERCEPT_RULES'; rules: InterceptRule[] }
  | { type: 'ADD_INTERCEPT_RULE'; rule: InterceptRule }
  | { type: 'UPDATE_INTERCEPT_RULE'; rule: InterceptRule }
  | { type: 'DELETE_INTERCEPT_RULE'; id: number; success: boolean }
  | { type: 'SET_INTERCEPT_RULE_ERROR'; error: string | null }
  | { type: 'SET_INTERCEPT_STATUS'; status: InterceptStatus }
  //
  // Agent session message actions.
  //
  | { type: 'AGENT_SESSION_ADD_MESSAGE'; sessionId: string; message: AgentSessionMessage }
  | { type: 'AGENT_SESSION_CLEAR_MESSAGES'; sessionId: string }
  | { type: 'AGENT_SESSION_STREAMING_UPDATE'; nodeId: string; transactionId: string; update: import('../api/types').SessionUpdateKind }
  | { type: 'AGENT_SESSION_STREAMING_COMPLETE'; nodeId: string; transactionId: string }
  | { type: 'AGENT_SESSION_STREAMING_CLEAR'; nodeId: string }
  | { type: 'AGENT_SESSION_STREAMING_CHUNK'; nodeId: string; text: string }
  | { type: 'NODE_SESSION_SET'; nodeId: string; sessionId: string; agentShortName: string }
  | { type: 'NODE_SESSION_CLEAR'; nodeId: string; agentShortName: string }
  //
  // Chain actions.
  //
  | { type: 'SET_CHAINS'; chains: ChainDefinitionInfo[] }
  | { type: 'SET_CURRENT_CHAIN'; chain: ChainDefinitionFull | null }
  | { type: 'REQUEST_CHAIN'; chain_id: string }
  | { type: 'ADD_CHAIN'; chain: ChainDefinitionInfo }
  | { type: 'UPDATE_CHAIN'; chain: ChainDefinitionInfo }
  | { type: 'DELETE_CHAIN'; chain_id: string }
  | { type: 'SET_CHAIN_EXECUTIONS'; executions: ChainExecutionUpdate[] }
  | { type: 'UPDATE_CHAIN_EXECUTION'; execution: ChainExecutionUpdate }
  | { type: 'SET_CHAIN_TRIGGERS'; triggers: ChainTriggerInfo[] }
  | { type: 'ADD_CHAIN_TRIGGER'; trigger: ChainTriggerInfo }
  | { type: 'UPDATE_CHAIN_TRIGGER'; trigger: ChainTriggerInfo }
  | { type: 'DELETE_CHAIN_TRIGGER'; trigger_id: string }
  | { type: 'SET_CHAIN_ERROR'; error: string | null }
  | { type: 'SET_CHAIN_SUCCESS'; message: string | null }
  | { type: 'SET_LAST_CREATED_CHAIN_ID'; chainId: string | null }
  //
  // Recent nodes action.
  //
  | { type: 'ACCESS_NODE'; nodeId: string }
  //
  // Agent Chat actions.
  //
  | { type: 'AGENT_CHAT_SESSION_STARTED'; sessionId: string; goal: string | null }
  | { type: 'AGENT_CHAT_SESSION_STOPPED'; sessionId: string }
  | { type: 'AGENT_CHAT_AGENT_ADDED'; sessionId: string; agent: AgentChatAgentInfo }
  | { type: 'AGENT_CHAT_AGENT_REMOVED'; sessionId: string; agentId: string }
  | { type: 'AGENT_CHAT_AGENT_STATUS_CHANGED'; sessionId: string; agentId: string; status: AgentChatAgentStatus }
  | { type: 'AGENT_CHAT_CHANNEL_CREATED'; sessionId: string; channel: AgentChatChannelInfo }
  | { type: 'AGENT_CHAT_CHANNEL_UPDATED'; sessionId: string; channel: AgentChatChannelInfo }
  | { type: 'AGENT_CHAT_AGENT_JOINED_CHANNEL'; sessionId: string; agentId: string; channelId: string }
  | { type: 'AGENT_CHAT_AGENT_LEFT_CHANNEL'; sessionId: string; agentId: string; channelId: string }
  | { type: 'AGENT_CHAT_MESSAGE'; sessionId: string; message: AgentChatMessageInfo }
  | { type: 'AGENT_CHAT_STATE_UPDATE'; session: AgentChatSessionState }
  | { type: 'AGENT_CHAT_HISTORY_RESPONSE'; sessionId: string; channelId: string | null; messages: AgentChatMessageInfo[] }
  | { type: 'AGENT_CHAT_ERROR'; message: string }
  | { type: 'AGENT_CHAT_SET_CURRENT_CHANNEL'; channelId: string | null }
  | { type: 'AGENT_CHAT_CLEAR_ERROR' }
  | { type: 'AGENT_CHAT_SET_LOADING'; loading: boolean }
  //
  // Toolkit actions.
  //
  | { type: 'TOOLKIT_LIST_RESPONSE'; tools: ToolkitToolInfo[]; models: ToolkitModelOption[] }
  | { type: 'TOOLKIT_RECON_RESPONSE'; targets: ToolkitReconTarget[] }
  | { type: 'TOOLKIT_EXECUTE_RESULT'; result: ToolkitExecuteResult }
  | { type: 'TOOLKIT_EXECUTION_PROGRESS'; current: number; total: number }
  | { type: 'TOOLKIT_APPLY_RESULT'; results: ToolkitApplyOutcome[] }
  | { type: 'TOOLKIT_ERROR'; message: string }
  //
  // Lua agent script actions.
  //
  | { type: 'SET_LUA_AGENT_SCRIPTS'; scripts: LuaAgentScriptInfo[] }
  //
  // Payload actions.
  //
  | { type: 'SET_PAYLOADS'; payloads: PayloadInfo[] };

function reduceCore(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'SET_CONNECTED':
      return { ...state, connected: action.connected, clientId: action.clientId ?? state.clientId, version: action.version ?? state.version };
    case 'SET_STATE':
      return { ...state, systemState: action.state };
    case 'SET_OPERATIONS':
      return { ...state, operations: action.operations };
    case 'UPDATE_OPERATION': {
      const index = state.operations.findIndex((op) => op.operation_id === action.update.operation_id);
      if (index >= 0) {
        const newOps = [...state.operations];
        newOps[index] = action.update;
        return { ...state, operations: newOps };
      }
      return { ...state, operations: [...state.operations, action.update] };
    }
    case 'SET_OPERATION_DEFS':
      return { ...state, operationDefs: action.definitions };
    case 'ADD_EVENT':
      //
      // Keep last 1000 events to avoid memory issues.
      //
      return { ...state, events: [...state.events.slice(-999), action.entry] };
    case 'CLEAR_EVENTS':
      return { ...state, events: [] };
    case 'SET_CONFIG':
      return { ...state, config: { ...state.config, ...action.values } };
    case 'SET_OP_DEF_ERROR':
      return { ...state, opDefError: action.error, opDefSuccess: null };
    case 'SET_OP_DEF_SUCCESS':
      return { ...state, opDefSuccess: action.fullName, opDefError: null };
    case 'SET_LUA_AGENT_SCRIPTS':
      return { ...state, luaAgentScripts: action.scripts };
    case 'SET_PAYLOADS':
      return { ...state, payloads: action.payloads };
    default:
      return null;
  }
}

//
// Helper to update a specific session within the orchestrator state.
//
function updateSession(
  state: AppState,
  sessionId: string,
  updater: (session: OrchestratorSessionState) => OrchestratorSessionState,
): AppState | null {
  const session = state.orchestrator.sessions.find(s => s.sessionId === sessionId);
  if (!session) return null;
  return {
    ...state,
    orchestrator: {
      ...state.orchestrator,
      sessions: state.orchestrator.sessions.map(s =>
        s.sessionId === sessionId ? updater(s) : s
      ),
    },
  };
}

function reduceOrchestrator(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'ORCHESTRATOR_CREATING_SESSION':
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          isStarting: true,
        },
      };
    case 'ORCHESTRATOR_SESSION_CREATED': {
      const exists = state.orchestrator.sessions.some(s => s.sessionId === action.sessionId);
      if (exists) return state;
      const newSession: OrchestratorSessionState = {
        sessionId: action.sessionId,
        label: action.label,
        loaded: action.loaded ?? true,
        provider: null,
        model: null,
        messages: action.loaded !== false ? [{
          id: generateUUID(),
          role: 'system',
          content: `Session "${action.label}" created.`,
          timestamp: new Date(),
        }] : [],
        currentPlan: null,
        isLoading: false,
        streamingContent: '',
        hadToolCall: false,
        currentToolExecutions: [],
        tokenUsage: null,
        currentPromptId: null,
      };
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          sessions: [...state.orchestrator.sessions, newSession].sort((a, b) => a.label.localeCompare(b.label)),
          activeSessionId: action.sessionId,
          isStarting: false,
        },
      };
    }
    case 'ORCHESTRATOR_SESSION_STARTED':
      return updateSession(state, action.sessionId, (s) => {
        //
        // Only add the "Session started" message once (skip on replay).
        //

        const alreadyStarted = s.provider !== null;
        return {
          ...s,
          loaded: true,
          provider: action.provider,
          model: action.model,
          messages: alreadyStarted ? s.messages : [...s.messages, {
            id: generateUUID(),
            role: 'system' as const,
            content: `Session started (${action.provider}::${action.model}).`,
            timestamp: new Date(),
          }],
        };
      });
    case 'ORCHESTRATOR_SESSION_CLOSED': {
      const remaining = state.orchestrator.sessions.filter(s => s.sessionId !== action.sessionId);
      const newActive = state.orchestrator.activeSessionId === action.sessionId
        ? (remaining.length > 0 ? remaining[remaining.length - 1].sessionId : null)
        : state.orchestrator.activeSessionId;
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          sessions: remaining,
          activeSessionId: newActive,
        },
      };
    }
    case 'ORCHESTRATOR_SET_ACTIVE_SESSION':
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          activeSessionId: action.sessionId,
        },
      };
    case 'ORCHESTRATOR_SESSION_LOADED':
      return updateSession(state, action.sessionId, (s) => ({
        ...s,
        loaded: true,
      }));
    case 'ORCHESTRATOR_SYNC_SESSIONS': {
      const keep = new Set(action.sessionIds);
      const filtered = state.orchestrator.sessions.filter(s => keep.has(s.sessionId));
      const newActive = state.orchestrator.activeSessionId && keep.has(state.orchestrator.activeSessionId)
        ? state.orchestrator.activeSessionId
        : filtered.length > 0 ? filtered[0].sessionId : null;
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          sessions: filtered,
          activeSessionId: newActive,
        },
      };
    }
    case 'ORCHESTRATOR_ADD_USER_MESSAGE':
      return updateSession(state, action.sessionId, (s) => ({
        ...s,
        messages: [...s.messages, {
          id: generateUUID(),
          role: 'user' as const,
          content: action.message,
          timestamp: new Date(),
        }],
        isLoading: true,
        streamingContent: '',
        hadToolCall: false,
        currentToolExecutions: [],
        currentPromptId: action.promptId,
      }));
    case 'ORCHESTRATOR_ADD_CONTENT':
      return updateSession(state, action.sessionId, (s) => {
        const needsSep = s.hadToolCall && s.streamingContent.length > 0
          && !s.streamingContent.endsWith('\n\n');
        const prefix = needsSep ? '\n\n' : '';
        return {
          ...s,
          streamingContent: s.streamingContent + prefix + action.content,
          hadToolCall: false,
        };
      });
    case 'ORCHESTRATOR_TOOL_EXECUTING':
      return updateSession(state, action.sessionId, (s) => ({
        ...s,
        hadToolCall: true,
        currentToolExecutions: [...s.currentToolExecutions, {
          name: action.name,
          display: 'Executing...',
          success: true,
          executing: true,
          input: action.input,
        }],
      }));
    case 'ORCHESTRATOR_TOOL_EXECUTED':
      return updateSession(state, action.sessionId, (s) => ({
        ...s,
        currentToolExecutions: s.currentToolExecutions.map((ex) =>
          ex.name === action.name && ex.executing
            ? { name: action.name, display: action.display, success: action.success, executing: false, input: ex.input, result: action.result }
            : ex
        ),
      }));
    case 'ORCHESTRATOR_PLAN_UPDATED':
      return updateSession(state, action.sessionId, (s) => ({
        ...s,
        currentPlan: action.plan,
      }));
    case 'ORCHESTRATOR_DONE':
      return updateSession(state, action.sessionId, (s) => {
        //
        // Finalize the current streaming content and tool executions into a
        // message.
        //
        const newMessages = [...s.messages];
        if (s.streamingContent || s.currentToolExecutions.length > 0) {
          newMessages.push({
            id: generateUUID(),
            role: 'assistant',
            content: s.streamingContent,
            timestamp: new Date(),
            toolExecutions: s.currentToolExecutions.length > 0
              ? [...s.currentToolExecutions]
              : undefined,
          });
        }
        return {
          ...s,
          messages: newMessages,
          isLoading: false,
          streamingContent: '',
          currentToolExecutions: [],
        };
      });
    case 'ORCHESTRATOR_ERROR':
      return updateSession(state, action.sessionId, (s) => ({
        ...s,
        messages: [...s.messages, {
          id: generateUUID(),
          role: 'system' as const,
          content: `Error: ${action.message}`,
          timestamp: new Date(),
        }],
        isLoading: false,
        streamingContent: '',
        currentToolExecutions: [],
      }));
    case 'ORCHESTRATOR_CLEAR_MESSAGES':
      return updateSession(state, action.sessionId, (s) => ({
        ...s,
        messages: [],
        currentPlan: null,
        streamingContent: '',
        currentToolExecutions: [],
        tokenUsage: null,
      }));
    case 'ORCHESTRATOR_TOKEN_USAGE':
      return updateSession(state, action.sessionId, (s) => ({
        ...s,
        tokenUsage: {
          promptTokens: action.promptTokens,
          completionTokens: action.completionTokens,
          totalTokens: action.totalTokens,
        },
      }));
    default:
      return null;
  }
}

function reduceIntercept(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'SET_TRAFFIC_LOG':
      return {
        ...state,
        intercept: {
          ...state.intercept,
          trafficLog: action.entries,
          trafficTotalCount: action.totalCount,
        },
      };
    case 'SET_TRAFFIC_MATCHES':
      return {
        ...state,
        intercept: {
          ...state.intercept,
          trafficMatches: action.matches,
          matchesTotalCount: action.totalCount,
        },
      };
    case 'SET_TRAFFIC_CLEARED':
      return {
        ...state,
        intercept: {
          ...state.intercept,
          trafficLog: [],
          trafficTotalCount: 0,
        },
      };
    case 'SET_INTERCEPT_RULES':
      return {
        ...state,
        intercept: {
          ...state.intercept,
          rules: action.rules,
        },
      };
    case 'ADD_INTERCEPT_RULE':
      return {
        ...state,
        intercept: {
          ...state.intercept,
          rules: [...state.intercept.rules, action.rule],
          ruleError: null,
        },
      };
    case 'UPDATE_INTERCEPT_RULE': {
      const updatedRules = state.intercept.rules.map((r) =>
        r.id === action.rule.id ? action.rule : r
      );
      return {
        ...state,
        intercept: {
          ...state.intercept,
          rules: updatedRules,
          ruleError: null,
        },
      };
    }
    case 'DELETE_INTERCEPT_RULE':
      if (action.success) {
        return {
          ...state,
          intercept: {
            ...state.intercept,
            rules: state.intercept.rules.filter((r) => r.id !== action.id),
            ruleError: null,
          },
        };
      }
      return state;
    case 'SET_INTERCEPT_RULE_ERROR':
      return {
        ...state,
        intercept: {
          ...state.intercept,
          ruleError: action.error,
        },
      };
    case 'SET_INTERCEPT_STATUS': {
      const newStatus = new Map(state.intercept.nodeStatus);
      newStatus.set(action.status.node_id, action.status);
      return {
        ...state,
        intercept: {
          ...state.intercept,
          nodeStatus: newStatus,
        },
      };
    }
    default:
      return null;
  }
}

function reduceLogQuery(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'LOG_QUERY_SET_QUERY':
      return {
        ...state,
        logQuery: { ...state.logQuery, query: action.query },
      };
    case 'LOG_QUERY_START':
      return {
        ...state,
        logQuery: { ...state.logQuery, isRunning: true, error: null },
      };
    case 'LOG_QUERY_RESPONSE':
      return {
        ...state,
        logQuery: {
          ...state.logQuery,
          isRunning: false,
          columns: action.columns,
          rows: action.rows,
          totalCount: action.totalCount,
          error: null,
        },
      };
    case 'LOG_QUERY_ERROR':
      return {
        ...state,
        logQuery: { ...state.logQuery, isRunning: false, error: action.message },
      };
    default:
      return null;
  }
}

function reduceAgentSessions(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'AGENT_SESSION_ADD_MESSAGE': {
      const existingMessages = state.agentSessionMessages[action.sessionId] || [];
      return {
        ...state,
        agentSessionMessages: {
          ...state.agentSessionMessages,
          [action.sessionId]: [...existingMessages, action.message],
        },
      };
    }
    case 'AGENT_SESSION_CLEAR_MESSAGES': {
      const { [action.sessionId]: _, ...rest } = state.agentSessionMessages;
      return {
        ...state,
        agentSessionMessages: rest,
      };
    }
    case 'AGENT_SESSION_STREAMING_UPDATE': {
      const key = action.nodeId;
      const existing = state.agentSessionStreaming[key] || {
        content: '',
        transactionId: action.transactionId,
        toolCalls: [],
        pendingPermission: null,
        agentStatus: null,
        hadToolCall: false,
      };
      const update = action.update;

      if ('TextChunk' in update) {
        const needsSeparator = existing.hadToolCall
          && existing.content.length > 0
          && !existing.content.endsWith('\n\n');
        const prefix = needsSeparator ? '\n\n' : '';
        return {
          ...state,
          agentSessionStreaming: {
            ...state.agentSessionStreaming,
            [key]: { ...existing, content: existing.content + prefix + update.TextChunk.text, hadToolCall: false },
          },
        };
      } else if ('ToolCall' in update) {
        return {
          ...state,
          agentSessionStreaming: {
            ...state.agentSessionStreaming,
            [key]: {
              ...existing,
              hadToolCall: true,
              toolCalls: [...existing.toolCalls, {
                toolName: update.ToolCall.tool_name,
                toolId: update.ToolCall.tool_id,
                input: update.ToolCall.input,
              }],
            },
          },
        };
      } else if ('ToolResult' in update) {
        const updatedCalls = existing.toolCalls.map(tc =>
          tc.toolId === update.ToolResult.tool_id
            ? { ...tc, output: update.ToolResult.output, isError: update.ToolResult.is_error }
            : tc
        );
        return {
          ...state,
          agentSessionStreaming: {
            ...state.agentSessionStreaming,
            [key]: { ...existing, toolCalls: updatedCalls },
          },
        };
      } else if ('PermissionRequest' in update) {
        return {
          ...state,
          agentSessionStreaming: {
            ...state.agentSessionStreaming,
            [key]: {
              ...existing,
              pendingPermission: {
                permissionId: update.PermissionRequest.permission_id,
                toolName: update.PermissionRequest.tool_name,
                toolInput: update.PermissionRequest.tool_input,
              },
            },
          },
        };
      } else if ('AgentStatus' in update) {
        return {
          ...state,
          agentSessionStreaming: {
            ...state.agentSessionStreaming,
            [key]: { ...existing, agentStatus: update.AgentStatus.status },
          },
        };
      }
      return state;
    }
    case 'AGENT_SESSION_STREAMING_CLEAR': {
      const { [action.nodeId]: _, ...rest } = state.agentSessionStreaming;
      return { ...state, agentSessionStreaming: rest };
    }
    case 'AGENT_SESSION_STREAMING_CHUNK': {
      const key = action.nodeId;
      const existing = state.agentSessionStreaming[key] || {
        content: '',
        transactionId: '',
        toolCalls: [],
        pendingPermission: null,
        agentStatus: null,
        hadToolCall: false,
      };
      return {
        ...state,
        agentSessionStreaming: {
          ...state.agentSessionStreaming,
          [key]: { ...existing, content: existing.content + action.text, hadToolCall: false },
        },
      };
    }
    case 'NODE_SESSION_SET': {
      const key = nodeSessionKey(action.nodeId, action.agentShortName);
      return {
        ...state,
        nodeSessions: {
          ...state.nodeSessions,
          [key]: {
            nodeId: action.nodeId,
            agentShortName: action.agentShortName,
            sessionId: action.sessionId,
          },
        },
      };
    }
    case 'NODE_SESSION_CLEAR': {
      const key = nodeSessionKey(action.nodeId, action.agentShortName);
      const { [key]: _, ...rest } = state.nodeSessions;
      return { ...state, nodeSessions: rest };
    }
    default:
      return null;
  }
}

function reduceChains(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'SET_CHAINS':
      return { ...state, chains: { ...state.chains, chains: action.chains } };
    case 'REQUEST_CHAIN': {
      const newLoadingChains = new Set(state.chains.loadingChains);
      newLoadingChains.add(action.chain_id);
      return { ...state, chains: { ...state.chains, loadingChains: newLoadingChains } };
    }
    case 'SET_CURRENT_CHAIN': {
      if (!action.chain) {
        return { ...state, chains: { ...state.chains, currentChain: null } };
      }
      const newLoadingChains = new Set(state.chains.loadingChains);
      newLoadingChains.delete(action.chain.id);
      const newCache = { ...state.chains.chainDefinitionsCache, [action.chain.id]: action.chain };
      return {
        ...state,
        chains: {
          ...state.chains,
          currentChain: action.chain,
          chainDefinitionsCache: newCache,
          loadingChains: newLoadingChains,
        },
      };
    }
    case 'ADD_CHAIN':
      return { ...state, chains: { ...state.chains, chains: [...state.chains.chains, action.chain] } };
    case 'UPDATE_CHAIN': {
      const updatedChains = state.chains.chains.map(c => c.id === action.chain.id ? action.chain : c);
      //
      // Invalidate cached full definition so next load fetches fresh data
      // with updated block_config and other settings.
      //
      const { [action.chain.id]: _, ...remainingCache } = state.chains.chainDefinitionsCache;
      const clearedCurrentChain = state.chains.currentChain?.id === action.chain.id
        ? null
        : state.chains.currentChain;
      return { ...state, chains: { ...state.chains, chains: updatedChains, currentChain: clearedCurrentChain, chainDefinitionsCache: remainingCache } };
    }
    case 'DELETE_CHAIN':
      return { ...state, chains: { ...state.chains, chains: state.chains.chains.filter(c => c.id !== action.chain_id) } };
    case 'SET_CHAIN_EXECUTIONS':
      return { ...state, chains: { ...state.chains, executions: action.executions } };
    case 'UPDATE_CHAIN_EXECUTION': {
      const index = state.chains.executions.findIndex(e => e.execution_id === action.execution.execution_id);
      if (index >= 0) {
        const newExecs = [...state.chains.executions];
        newExecs[index] = action.execution;
        return { ...state, chains: { ...state.chains, executions: newExecs } };
      }
      return { ...state, chains: { ...state.chains, executions: [...state.chains.executions, action.execution] } };
    }
    case 'SET_CHAIN_TRIGGERS':
      return { ...state, chains: { ...state.chains, triggers: action.triggers } };
    case 'ADD_CHAIN_TRIGGER':
      return { ...state, chains: { ...state.chains, triggers: [...state.chains.triggers, action.trigger] } };
    case 'UPDATE_CHAIN_TRIGGER': {
      const triggerIndex = state.chains.triggers.findIndex(t => t.id === action.trigger.id);
      if (triggerIndex >= 0) {
        const newTriggers = [...state.chains.triggers];
        newTriggers[triggerIndex] = action.trigger;
        return { ...state, chains: { ...state.chains, triggers: newTriggers } };
      }
      return { ...state, chains: { ...state.chains, triggers: [...state.chains.triggers, action.trigger] } };
    }
    case 'DELETE_CHAIN_TRIGGER':
      return { ...state, chains: { ...state.chains, triggers: state.chains.triggers.filter(t => t.id !== action.trigger_id) } };
    case 'SET_CHAIN_ERROR':
      return { ...state, chains: { ...state.chains, chainError: action.error, chainSuccess: null } };
    case 'SET_CHAIN_SUCCESS':
      return { ...state, chains: { ...state.chains, chainSuccess: action.message, chainError: null } };
    case 'SET_LAST_CREATED_CHAIN_ID':
      return { ...state, chains: { ...state.chains, lastCreatedChainId: action.chainId } };
    default:
      return null;
  }
}

function reduceRecentNodes(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'ACCESS_NODE': {
      //
      // Move the accessed node to the front, remove duplicates, and limit to
      // MAX_RECENT_NODES.
      //
      const filtered = state.recentlyAccessedNodeIds.filter(id => id !== action.nodeId);
      const updated = [action.nodeId, ...filtered].slice(0, MAX_RECENT_NODES);
      persistRecentNodes(updated);
      return { ...state, recentlyAccessedNodeIds: updated };
    }
    default:
      return null;
  }
}

function reduceAgentChat(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'AGENT_CHAT_SESSION_STARTED': {
      const newSession: AgentChatSessionState = {
        id: action.sessionId,
        goal: action.goal,
        status: 'active',
        agents: [],
        channels: [],
        created_at: new Date().toISOString(),
      };
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          session: newSession,
          messages: [],
          error: null,
        },
      };
    }
    case 'AGENT_CHAT_SESSION_STOPPED':
      return {
        ...state,
        agentChat: {
          ...initialAgentChatState,
        },
      };
    case 'AGENT_CHAT_AGENT_ADDED':
      if (!state.agentChat.session || state.agentChat.session.id !== action.sessionId) return state;
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          session: {
            ...state.agentChat.session,
            agents: [...state.agentChat.session.agents, action.agent],
          },
        },
      };
    case 'AGENT_CHAT_AGENT_REMOVED':
      if (!state.agentChat.session || state.agentChat.session.id !== action.sessionId) return state;
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          session: {
            ...state.agentChat.session,
            agents: state.agentChat.session.agents.filter(a => a.id !== action.agentId),
          },
        },
      };
    case 'AGENT_CHAT_AGENT_STATUS_CHANGED':
      if (!state.agentChat.session || state.agentChat.session.id !== action.sessionId) return state;
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          session: {
            ...state.agentChat.session,
            agents: state.agentChat.session.agents.map(a =>
              a.id === action.agentId ? { ...a, status: action.status } : a
            ),
          },
        },
      };
    case 'AGENT_CHAT_CHANNEL_CREATED':
      if (!state.agentChat.session || state.agentChat.session.id !== action.sessionId) return state;
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          session: {
            ...state.agentChat.session,
            channels: [...state.agentChat.session.channels, action.channel],
          },
          //
          // Auto-select the first channel if none selected.
          //
          currentChannelId: state.agentChat.currentChannelId ?? action.channel.id,
        },
      };
    case 'AGENT_CHAT_CHANNEL_UPDATED':
      if (!state.agentChat.session || state.agentChat.session.id !== action.sessionId) return state;
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          session: {
            ...state.agentChat.session,
            channels: state.agentChat.session.channels.map(c =>
              c.id === action.channel.id ? action.channel : c
            ),
          },
        },
      };
    case 'AGENT_CHAT_AGENT_JOINED_CHANNEL':
      if (!state.agentChat.session || state.agentChat.session.id !== action.sessionId) return state;
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          session: {
            ...state.agentChat.session,
            agents: state.agentChat.session.agents.map(a =>
              a.id === action.agentId ? { ...a, current_channel_id: action.channelId } : a
            ),
          },
        },
      };
    case 'AGENT_CHAT_AGENT_LEFT_CHANNEL':
      if (!state.agentChat.session || state.agentChat.session.id !== action.sessionId) return state;
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          session: {
            ...state.agentChat.session,
            agents: state.agentChat.session.agents.map(a =>
              a.id === action.agentId && a.current_channel_id === action.channelId
                ? { ...a, current_channel_id: null }
                : a
            ),
          },
        },
      };
    case 'AGENT_CHAT_MESSAGE':
      if (!state.agentChat.session || state.agentChat.session.id !== action.sessionId) return state;
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          messages: [...state.agentChat.messages, action.message],
        },
      };
    case 'AGENT_CHAT_STATE_UPDATE':
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          session: action.session,
          currentChannelId: state.agentChat.currentChannelId ?? action.session.channels[0]?.id ?? null,
        },
      };
    case 'AGENT_CHAT_HISTORY_RESPONSE':
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          messages: action.messages,
        },
      };
    case 'AGENT_CHAT_ERROR':
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          error: action.message,
          isLoading: false,
        },
      };
    case 'AGENT_CHAT_SET_CURRENT_CHANNEL':
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          currentChannelId: action.channelId,
        },
      };
    case 'AGENT_CHAT_CLEAR_ERROR':
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          error: null,
        },
      };
    case 'AGENT_CHAT_SET_LOADING':
      return {
        ...state,
        agentChat: {
          ...state.agentChat,
          isLoading: action.loading,
        },
      };
    default:
      return null;
  }
}

function reduceToolkit(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'TOOLKIT_LIST_RESPONSE':
      return {
        ...state,
        toolkit: { ...state.toolkit, tools: action.tools, models: action.models, error: null },
      };
    case 'TOOLKIT_RECON_RESPONSE':
      return {
        ...state,
        toolkit: { ...state.toolkit, reconTargets: action.targets, error: null },
      };
    case 'TOOLKIT_EXECUTE_RESULT':
      return {
        ...state,
        toolkit: { ...state.toolkit, executeResult: action.result, applyResults: null, executionProgress: null, error: null },
      };
    case 'TOOLKIT_EXECUTION_PROGRESS':
      return {
        ...state,
        toolkit: { ...state.toolkit, executionProgress: { current: action.current, total: action.total } },
      };
    case 'TOOLKIT_APPLY_RESULT':
      return {
        ...state,
        toolkit: { ...state.toolkit, applyResults: action.results, error: null },
      };
    case 'TOOLKIT_ERROR':
      return {
        ...state,
        toolkit: { ...state.toolkit, error: action.message, executionProgress: null },
      };
    default:
      return null;
  }
}

function reducer(state: AppState, action: Action): AppState {
  return (
    reduceCore(state, action)
    ?? reduceOrchestrator(state, action)
    ?? reduceIntercept(state, action)
    ?? reduceLogQuery(state, action)
    ?? reduceAgentSessions(state, action)
    ?? reduceChains(state, action)
    ?? reduceRecentNodes(state, action)
    ?? reduceAgentChat(state, action)
    ?? reduceToolkit(state, action)
    ?? state
  );
}

//
// Context.
//
interface AppContextValue {
  state: AppState;
  //
  // Raw reducer dispatch — exposed for components that need to update
  // multi-slice state (e.g. per-node session tracking) without us adding a
  // one-off setter for each action.
  //
  dispatch: Dispatch<Action>;
  //
  // Helpers.
  //
  getNode: (nodeId: string) => NodeState | undefined;
  //
  // Commands.
  //
  sendCommand: (nodeId: string, command: CommandRequest['command']) => Promise<CommandResponse>;
  //
  // Send an ACP JSON-RPC request targeted at a specific node. The node is
  // identified via `_meta.praxis.nodeId` in the params (the service proxy
  // routes any ACP frame carrying that marker to the owning node). When
  // `collectText` is true the helper also buffers streamed
  // `agent_message_chunk` text for the session and returns it in `text`.
  //
  sendAcpNodeRequest: (
    nodeId: string,
    method: string,
    params: Record<string, unknown>,
    collectText?: boolean,
  ) => Promise<{ result: unknown; text: string }>;
  //
  // Send an ACP JSON-RPC notification (no response) to a specific node.
  //
  sendAcpNodeNotification: (nodeId: string, method: string, params: Record<string, unknown>) => void;
  //
  // Terminal.
  //
  registerTerminalHandler: (nodeId: string, terminalId: string, handler: (output: TerminalOutput) => void) => () => void;
  sendTerminalInput: (nodeId: string, terminalId: string, data: number[]) => void;
  //
  // Semantic Operations.
  //
  requestOperations: () => void;
  runOperation: (nodeId: string, agentShortName: string, operationName: string, workingDir?: string) => void;
  cancelOperation: (operationId: string) => void;
  removeOperation: (operationId: string) => void;
  clearOperations: () => void;
  clearEventLog: () => void;
  //
  // Node Management.
  //
  removeNode: (nodeId: string) => void;
  resetNode: (nodeId: string) => void;
  //
  // Config.
  //
  getConfig: (keys: string[]) => void;
  setConfig: (values: Record<string, string>) => void;
  //
  // Operation Definitions.
  //
  clearOpDefStatus: () => void;
  //
  // Orchestrator (multi-session ACP).
  //
  orchestratorCreateSession: (modelRef?: string) => void;
  orchestratorCloseSession: (sessionId: string) => void;
  orchestratorCancelPrompt: (sessionId: string) => void;
  orchestratorSendPrompt: (sessionId: string, message: string) => void;
  orchestratorSetActiveSession: (sessionId: string | null) => void;
  orchestratorClearMessages: (sessionId: string) => void;
  //
  // Generic send.
  //
  send: (message: BrowserMessage) => void;
  //
  // Traffic Interception.
  //
  requestTrafficLog: (filters: TrafficLogFilters) => void;
  requestTrafficMatches: (ruleId: number | null, limit: number, offset: number) => void;
  clearTraffic: () => void;
  requestInterceptRules: () => void;
  createInterceptRule: (name: string, regexPattern: string, targetDirection: TargetDirection, scope: RuleScope, summarizationPrompt?: string | null) => void;
  updateInterceptRule: (id: number, updates: { name?: string; regex_pattern?: string; target_direction?: TargetDirection; scope?: RuleScope; enabled?: boolean; summarization_prompt?: string | null }) => void;
  deleteInterceptRule: (id: number) => void;
  enableIntercept: (nodeId: string, method?: InterceptMethod) => void;
  disableIntercept: (nodeId: string) => void;
  clearInterceptRuleError: () => void;
  //
  // Agent session messages.
  //
  addAgentSessionMessage: (sessionId: string, message: AgentSessionMessage) => void;
  clearAgentSessionMessages: (sessionId: string) => void;
  clearAgentSessionStreaming: (nodeId: string) => void;
  //
  // Chain operations.
  //
  requestChainDefList: () => void;
  requestChain: (chainId: string) => void;
  createChain: (definition: ChainDefinitionInput) => void;
  updateChain: (chainId: string, definition: ChainDefinitionInput) => void;
  deleteChain: (chainId: string) => void;
  runChain: (chainId: string, nodeId: string, agentShortName: string, workingDir?: string, targetSpec?: TargetSpec) => void;
  cancelChainExecution: (executionId: string) => void;
  removeChainExecution: (executionId: string) => void;
  //
  // Recent nodes tracking.
  //
  trackNodeAccess: (nodeId: string) => void;
  clearChainExecutions: () => void;
  requestChainExecutions: () => void;
  clearChainStatus: () => void;
  clearLastCreatedChain: () => void;
  //
  // Chain triggers.
  //
  requestChainTriggers: (chainId?: string) => void;
  createChainTrigger: (chainId: string, triggerConfig: TriggerConfig, targetSpec: TargetSpec) => void;
  updateChainTrigger: (triggerId: string, updates: { enabled?: boolean; trigger_config?: TriggerConfig; target_spec?: TargetSpec }) => void;
  deleteChainTrigger: (triggerId: string) => void;
  //
  // Agent Chat.
  //
  agentChatStart: (goal: string | null, yoloMode: boolean) => void;
  agentChatStop: () => void;
  agentChatAddAgent: (nodeId: string, agentShortName: string) => void;
  agentChatRemoveAgent: (agentId: string) => void;
  agentChatReorderAgents: (agentIds: string[]) => void;
  agentChatSendMessage: (content: string, channelId?: string, recipientNickname?: string) => void;
  agentChatJoinChannel: (channelName: string) => void;
  agentChatGetHistory: (channelId?: string, limit?: number) => void;
  agentChatGetState: () => void;
  agentChatSetCurrentChannel: (channelId: string | null) => void;
  agentChatClearError: () => void;
  //
  // LogQuery.
  //
  logQuerySetQuery: (query: string) => void;
  logQueryRun: (query: string) => void;
  //
  // Lua agent scripts.
  //
  listLuaAgentScripts: () => void;
  addLuaAgentScript: (name: string, script: string) => void;
  updateLuaAgentScript: (scriptId: string, name: string, script: string) => void;
  deleteLuaAgentScript: (scriptId: string) => void;
  resetLuaAgentScriptDefaults: () => void;
  toggleLuaAgentScriptDisabled: (scriptId: string, disabled: boolean) => void;
}

const AppContext = createContext<AppContextValue | null>(null);

export function AppProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, null, createInitialState);

  //
  // Use refs for callback maps to avoid stale closure issues.
  //
  const pendingCommandsRef = useRef<Map<string, (response: CommandResponse) => void>>(new Map());
  const terminalHandlersRef = useRef<Map<string, (output: TerminalOutput) => void>>(new Map());
  const clientIdRef = useRef<string | null>(null);
  const pendingAcpRequestsRef = useRef<Map<number | string, PendingAcpRequest>>(new Map());

  //
  // Keep clientId ref in sync.
  //
  useEffect(() => {
    clientIdRef.current = state.clientId;
  }, [state.clientId]);

  //
  // Persist Orchestrator state to sessionStorage whenever it changes.
  //
  useEffect(() => {
    persistOrchestratorState(state.orchestrator);
  }, [state.orchestrator]);

  //
  // Handle WebSocket messages - only set up once.
  //
  useEffect(() => {
    const handleMessage = (message: ServerMessage) => {
      switch (message.type) {
        case 'connected': {
          dispatch({ type: 'SET_CONNECTED', connected: true, clientId: message.client_id, version: message.version });
          wsClient.send({ type: 'config_get', keys: ['prompt_timeout_secs'] });

          //
          // Fetch existing orchestrator sessions from the service.
          //

          const listRpc = acpRequest('session/list');
          wsClient.send({ type: 'acp_message', json_rpc: listRpc });
          const listParsed = JSON.parse(listRpc);
          pendingAcpRequestsRef.current.set(listParsed.id, { method: 'session/list' });
          break;
        }
        case 'state_update':
          dispatch({ type: 'SET_STATE', state: message.state });
          break;
        case 'command_response': {
          const resolver = pendingCommandsRef.current.get(message.response.command_id);
          if (resolver) {
            resolver(message.response);
            pendingCommandsRef.current.delete(message.response.command_id);
          }
          break;
        }
        case 'terminal_output': {
          const key = `${message.output.node_id}:${message.output.terminal_id}`;
          const handler = terminalHandlersRef.current.get(key);
          if (handler) {
            handler(message.output);
          }
          break;
        }
        case 'semantic_op_update':
          dispatch({ type: 'UPDATE_OPERATION', update: message.update });
          break;
        case 'semantic_op_list':
          dispatch({ type: 'SET_OPERATIONS', operations: message.operations });
          break;
        case 'config_response':
          dispatch({ type: 'SET_CONFIG', values: message.values });
          break;
        case 'config_saved':
          //
          // Refresh centralized logging state after a config update.
          //
          wsClient.send({ type: 'config_get', keys: ['application_logs_enabled', 'prompt_timeout_secs'] });
          break;
        case 'op_def_list':
          dispatch({ type: 'SET_OPERATION_DEFS', definitions: message.definitions });
          break;
        case 'op_def_error':
          dispatch({ type: 'SET_OP_DEF_ERROR', error: message.message });
          break;
        case 'op_def_added':
          dispatch({ type: 'SET_OP_DEF_SUCCESS', fullName: message.full_name });
          break;
        //
        // ACP JSON-RPC messages from the service. Parse and dispatch
        // appropriate orchestrator actions.
        //
        case 'acp_message': {
          try {
            const rpc = JSON.parse(message.json_rpc) as AcpJsonRpc;

            if (rpc.method === 'session/update' && rpc.params) {
              //
              // Session update notification from the server.
              //
              const sessionId = rpc.params.sessionId as string;
              const update = rpc.params.update as Record<string, unknown>;
              if (!sessionId || !update) break;

              const extractText = (u: Record<string, unknown>): string => {
                const content = u.content as { type: string; text?: string } | Array<{ type: string; text?: string }> | undefined;
                if (!content) return '';
                if (Array.isArray(content)) {
                  return content.filter(b => b.type === 'text' && b.text).map(b => b.text!).join('');
                }
                return (content as { text?: string }).text || '';
              };

              const sessionUpdate = update.sessionUpdate as string;

              switch (sessionUpdate) {
                case 'session_info_update': {
                  const meta = update._meta as Record<string, unknown> | undefined;
                  if (meta) {
                    if (meta.promptTokens !== undefined) {
                      dispatch({
                        type: 'ORCHESTRATOR_TOKEN_USAGE',
                        sessionId,
                        promptTokens: (meta.promptTokens as number) || 0,
                        completionTokens: (meta.completionTokens as number) || 0,
                        totalTokens: (meta.totalTokens as number) || 0,
                      });
                    }
                    if (meta.provider && meta.model) {
                      dispatch({
                        type: 'ORCHESTRATOR_SESSION_STARTED',
                        sessionId,
                        provider: meta.provider as string,
                        model: meta.model as string,
                      });
                    }
                  }
                  break;
                }
                case 'user_message_chunk': {
                  const promptText = extractText(update);
                  if (promptText) {
                    dispatch({
                      type: 'ORCHESTRATOR_ADD_USER_MESSAGE',
                      sessionId,
                      message: promptText,
                      promptId: generateUUID(),
                    });
                  }
                  break;
                }
                case 'agent_message_chunk': {
                  const text = extractText(update);
                  if (text) {
                    //
                    // Dispatch to orchestrator for WEB_ prefixed orchestrator
                    // sessions (matches existing behavior). Node-bound agent
                    // sessions feed the per-node streaming UI instead.
                    //
                    const nodeEntry = Object.values(nodeSessionsRef.current)
                      .find((s) => s.sessionId === sessionId);
                    if (nodeEntry) {
                      dispatch({
                        type: 'AGENT_SESSION_STREAMING_CHUNK',
                        nodeId: nodeEntry.nodeId,
                        text,
                      });
                    } else {
                      dispatch({
                        type: 'ORCHESTRATOR_ADD_CONTENT',
                        sessionId,
                        content: text,
                      });
                    }
                    //
                    // Buffer into any pending ACP request with `collectText`
                    // targeting this session so `sendAcpNodeRequest` can
                    // return the concatenated reply.
                    //
                    for (const [, pending] of pendingAcpRequestsRef.current) {
                      if (pending.collectText && pending.sessionId === sessionId) {
                        pending.textBuf = (pending.textBuf ?? '') + text;
                      }
                    }
                  }
                  break;
                }
                case 'tool_call': {
                  const toolName = (update.title as string) || '';
                  if (toolName !== 'report_plan') {
                    dispatch({
                      type: 'ORCHESTRATOR_TOOL_EXECUTING',
                      sessionId,
                      name: toolName,
                      input: update.toolCallId as string | undefined,
                    });
                  }
                  break;
                }
                case 'tool_call_update': {
                  const tcId = (update.toolCallId as string) || '';
                  const status = (update.status as string) || '';
                  if (status === 'completed' || status === 'failed') {
                    const resultText = extractText(update);
                    dispatch({
                      type: 'ORCHESTRATOR_TOOL_EXECUTED',
                      sessionId,
                      name: tcId,
                      display: tcId,
                      success: status !== 'failed',
                      result: resultText,
                    });
                  }
                  break;
                }
                case 'plan': {
                  const entries = update.entries as Array<{ content: string; status: string }> | undefined;
                  if (entries) {
                    const plan: OrchestratorPlan = {
                      steps: entries.map(e => ({
                        description: e.content,
                        status: e.status === 'completed' ? 'done' as const
                          : e.status === 'in_progress' ? 'in_progress' as const
                          : 'not_started' as const,
                      })),
                      summary: undefined,
                      current_step_description: undefined,
                    };
                    dispatch({
                      type: 'ORCHESTRATOR_PLAN_UPDATED',
                      sessionId,
                      plan,
                    });
                  }
                  break;
                }
              }
            } else if (rpc.id !== undefined && !rpc.method) {
              //
              // Response to a pending request.
              //
              const pending = pendingAcpRequestsRef.current.get(rpc.id);
              if (!pending) break;
              pendingAcpRequestsRef.current.delete(rpc.id);

              //
              // Generic resolver path used by `sendAcpNodeRequest`. Matches
              // before the orchestrator-specific switch so callers outside
              // the orchestrator chat flow get their promise settled.
              //
              if (pending.resolve || pending.reject) {
                if (rpc.error) {
                  pending.reject?.(new Error(rpc.error.message));
                } else {
                  pending.resolve?.(rpc.result ?? null, pending.textBuf ?? '');
                }
                break;
              }

              if (rpc.error) {
                const sessionId = pending.sessionId;
                if (sessionId) {
                  dispatch({ type: 'ORCHESTRATOR_ERROR', sessionId, message: rpc.error.message });
                }
                break;
              }

              switch (pending.method) {
                case 'session/list': {
                  const rawSessions = rpc.result?.sessions as Array<{ sessionId: string; title?: string; cwd?: string }> | undefined;
                  if (rawSessions && rawSessions.length > 0) {
                    //
                    // Filter to only WEB_ prefixed sessions (by session ID).
                    //

                    const webSessions = rawSessions.filter(s => s.sessionId.startsWith('WEB_'));
                    const serverIds = webSessions.map(s => s.sessionId);
                    const serverSet = new Set(serverIds);
                    const currentSessions = orchestratorSessionsRef.current;
                    const existing = currentSessions.filter(s => serverSet.has(s.sessionId));
                    if (existing.length !== currentSessions.length) {
                      dispatch({ type: 'ORCHESTRATOR_SYNC_SESSIONS', sessionIds: serverIds });
                    }

                    let loadTriggered = false;
                    for (const sess of webSessions) {
                      const label = `Session ${webSessionCounter.current++}`;
                      const alreadyExists = orchestratorSessionsRef.current.some(s => s.sessionId === sess.sessionId);
                      if (!alreadyExists) {
                        dispatch({
                          type: 'ORCHESTRATOR_SESSION_CREATED',
                          sessionId: sess.sessionId,
                          label,
                          loaded: false,
                        });

                        //
                        // Trigger session/load for the first new session when
                        // no session is currently active (reconnect scenario).
                        //

                        if (!loadTriggered && !orchestratorActiveIdRef.current) {
                          loadTriggered = true;
                          const loadRpc = acpRequest('session/load', { sessionId: sess.sessionId, cwd: '.', mcpServers: [] });
                          wsClient.send({ type: 'acp_message', json_rpc: loadRpc });
                          const loadParsed = JSON.parse(loadRpc);
                          pendingAcpRequestsRef.current.set(loadParsed.id, { method: 'session/load', sessionId: sess.sessionId });
                        }
                      }
                    }
                  }
                  break;
                }
                case 'session/new': {
                  const sessionId = rpc.result?.sessionId as string;
                  if (sessionId) {
                    dispatch({
                      type: 'ORCHESTRATOR_SESSION_CREATED',
                      sessionId,
                      label: `Session ${webSessionCounter.current++}`,
                    });
                  }
                  break;
                }
                case 'session/prompt': {
                  const sessionId = pending.sessionId;
                  if (sessionId) {
                    dispatch({ type: 'ORCHESTRATOR_DONE', sessionId });
                  }
                  break;
                }
                case 'session/load': {
                  const sessionId = pending.sessionId;
                  if (sessionId) {
                    dispatch({ type: 'ORCHESTRATOR_SESSION_LOADED', sessionId });
                  }
                  break;
                }
                case 'session/close': {
                  const sessionId = pending.sessionId;
                  if (sessionId) {
                    dispatch({ type: 'ORCHESTRATOR_SESSION_CLOSED', sessionId });
                  }
                  break;
                }
              }
            }
          } catch (e) {
            console.warn('Failed to parse ACP message:', e);
          }
          break;
        }
        //
        // Traffic interception messages.
        //
        case 'traffic_log_response':
          dispatch({ type: 'SET_TRAFFIC_LOG', entries: message.entries, totalCount: message.total_count });
          break;
        case 'traffic_matches_response':
          dispatch({ type: 'SET_TRAFFIC_MATCHES', matches: message.matches, totalCount: message.total_count });
          break;
        case 'traffic_cleared':
          dispatch({ type: 'SET_TRAFFIC_CLEARED', deletedCount: message.deleted_count });
          break;
        case 'intercept_rule_list':
          dispatch({ type: 'SET_INTERCEPT_RULES', rules: message.rules });
          break;
        case 'intercept_rule_created':
          dispatch({ type: 'ADD_INTERCEPT_RULE', rule: message.rule });
          break;
        case 'intercept_rule_updated':
          dispatch({ type: 'UPDATE_INTERCEPT_RULE', rule: message.rule });
          break;
        case 'intercept_rule_deleted':
          dispatch({ type: 'DELETE_INTERCEPT_RULE', id: message.id, success: message.success });
          break;
        case 'intercept_rule_error':
          dispatch({ type: 'SET_INTERCEPT_RULE_ERROR', error: message.message });
          break;
        case 'intercept_status_update':
          dispatch({ type: 'SET_INTERCEPT_STATUS', status: message.status });
          break;

        //
        // Chain messages.
        //
        case 'chain_def_list':
          dispatch({ type: 'SET_CHAINS', chains: message.chains });
          break;
        case 'chain_get_response':
          dispatch({ type: 'SET_CURRENT_CHAIN', chain: message.chain });
          break;
        case 'chain_created':
          dispatch({ type: 'ADD_CHAIN', chain: message.chain });
          dispatch({ type: 'SET_CHAIN_SUCCESS', message: `Chain '${message.chain.name}' created` });
          dispatch({ type: 'SET_LAST_CREATED_CHAIN_ID', chainId: message.chain.id });
          break;
        case 'chain_updated':
          dispatch({ type: 'UPDATE_CHAIN', chain: message.chain });
          dispatch({ type: 'SET_CHAIN_SUCCESS', message: `Chain '${message.chain.name}' updated` });
          break;
        case 'chain_deleted':
          if (message.success) {
            dispatch({ type: 'DELETE_CHAIN', chain_id: message.chain_id });
          }
          break;
        case 'chain_error':
          dispatch({ type: 'SET_CHAIN_ERROR', error: message.message });
          break;
        case 'chain_execution_started':
          //
          // TODO: Handle execution started.
          //
          break;
        case 'chain_execution_update':
          dispatch({ type: 'UPDATE_CHAIN_EXECUTION', execution: message.execution });
          break;
        case 'chain_execution_list':
          dispatch({ type: 'SET_CHAIN_EXECUTIONS', executions: message.executions });
          break;

        //
        // Chain trigger messages.
        //
        case 'chain_trigger_created':
          dispatch({ type: 'ADD_CHAIN_TRIGGER', trigger: message.trigger });
          break;
        case 'chain_trigger_updated':
          dispatch({ type: 'UPDATE_CHAIN_TRIGGER', trigger: message.trigger });
          break;
        case 'chain_trigger_deleted':
          dispatch({ type: 'DELETE_CHAIN_TRIGGER', trigger_id: message.trigger_id });
          break;
        case 'chain_trigger_list_response':
          dispatch({ type: 'SET_CHAIN_TRIGGERS', triggers: message.triggers });
          break;

        //
        // LogQuery messages.
        //
        case 'log_query_response':
          dispatch({ type: 'LOG_QUERY_RESPONSE', columns: message.columns, rows: message.rows, totalCount: message.total_count });
          break;
        case 'log_query_error':
          dispatch({ type: 'LOG_QUERY_ERROR', message: message.message });
          break;

        //
        // Recon messages.
        //
        case 'recon_get_response':
          //
          // Dispatch as custom event so UI components can catch recon responses.
          //
          window.dispatchEvent(new CustomEvent('ws-message', { detail: message }));
          break;
        case 'toolkit_list_response':
          dispatch({ type: 'TOOLKIT_LIST_RESPONSE', tools: message.tools, models: message.models });
          break;
        case 'toolkit_recon_response':
          dispatch({ type: 'TOOLKIT_RECON_RESPONSE', targets: message.targets });
          break;
        case 'toolkit_execution_result':
          dispatch({ type: 'TOOLKIT_EXECUTE_RESULT', result: message.result });
          break;
        case 'toolkit_execution_progress':
          dispatch({ type: 'TOOLKIT_EXECUTION_PROGRESS', current: message.current, total: message.total });
          break;
        case 'toolkit_apply_result':
          dispatch({ type: 'TOOLKIT_APPLY_RESULT', results: message.results });
          break;
        case 'toolkit_error':
          dispatch({ type: 'TOOLKIT_ERROR', message: message.message });
          break;

        //
        // Lua agent script messages.
        //
        //
        // Payload messages.
        //
        case 'payload_list_response':
          dispatch({ type: 'SET_PAYLOADS', payloads: message.payloads });
          break;
        case 'payload_upserted':
        case 'payload_deleted':
          wsClient.send({ type: 'payload_list' });
          break;
        case 'payload_error':
          break;

        case 'lua_agent_script_added':
        case 'lua_agent_script_updated':
        case 'lua_agent_script_deleted':
        case 'lua_agent_script_defaults_reset':
        case 'lua_agent_script_disabled_toggled':
          wsClient.send({ type: 'lua_agent_script_list' });
          break;
        case 'lua_agent_script_list':
          dispatch({ type: 'SET_LUA_AGENT_SCRIPTS', scripts: message.scripts });
          break;

        //
        // Agent Chat messages.
        //
        case 'agent_chat_session_started':
          dispatch({ type: 'AGENT_CHAT_SESSION_STARTED', sessionId: message.session_id, goal: message.goal });
          break;
        case 'agent_chat_session_stopped':
          dispatch({ type: 'AGENT_CHAT_SESSION_STOPPED', sessionId: message.session_id });
          break;
        case 'agent_chat_agent_added':
          dispatch({ type: 'AGENT_CHAT_AGENT_ADDED', sessionId: message.session_id, agent: message.agent });
          break;
        case 'agent_chat_agent_removed':
          dispatch({ type: 'AGENT_CHAT_AGENT_REMOVED', sessionId: message.session_id, agentId: message.agent_id });
          break;
        case 'agent_chat_agent_status_changed':
          dispatch({ type: 'AGENT_CHAT_AGENT_STATUS_CHANGED', sessionId: message.session_id, agentId: message.agent_id, status: message.status });
          break;
        case 'agent_chat_channel_created':
          dispatch({ type: 'AGENT_CHAT_CHANNEL_CREATED', sessionId: message.session_id, channel: message.channel });
          break;
        case 'agent_chat_channel_updated':
          dispatch({ type: 'AGENT_CHAT_CHANNEL_UPDATED', sessionId: message.session_id, channel: message.channel });
          break;
        case 'agent_chat_agent_joined_channel':
          dispatch({ type: 'AGENT_CHAT_AGENT_JOINED_CHANNEL', sessionId: message.session_id, agentId: message.agent_id, channelId: message.channel_id });
          break;
        case 'agent_chat_agent_left_channel':
          dispatch({ type: 'AGENT_CHAT_AGENT_LEFT_CHANNEL', sessionId: message.session_id, agentId: message.agent_id, channelId: message.channel_id });
          break;
        case 'agent_chat_message':
          dispatch({ type: 'AGENT_CHAT_MESSAGE', sessionId: message.session_id, message: message.message });
          break;
        case 'agent_chat_state_update':
          dispatch({ type: 'AGENT_CHAT_STATE_UPDATE', session: message.session });
          break;
        case 'agent_chat_history_response':
          dispatch({ type: 'AGENT_CHAT_HISTORY_RESPONSE', sessionId: message.session_id, channelId: message.channel_id, messages: message.messages });
          break;
        case 'agent_chat_error':
          dispatch({ type: 'AGENT_CHAT_ERROR', message: message.message });
          break;

        //
        // Session streaming updates (ACP agent sessions).
        //
        case 'session_update':
          dispatch({
            type: 'AGENT_SESSION_STREAMING_UPDATE',
            nodeId: message.update.node_id,
            transactionId: message.update.transaction_id,
            update: message.update.update,
          });
          break;
      }
    };

    const unsubscribe = wsClient.addHandler(handleMessage);

    //
    // Connect to WebSocket.
    //

    wsClient.connect().catch(console.error);

    //
    // Poll session/list every 5 seconds to stay in sync.
    //

    const pollInterval = setInterval(() => {
      const rpc = acpRequest('session/list');
      wsClient.send({ type: 'acp_message', json_rpc: rpc });
      const parsed = JSON.parse(rpc);
      pendingAcpRequestsRef.current.set(parsed.id, { method: 'session/list' });
    }, 5000);

    return () => {
      unsubscribe();
      clearInterval(pollInterval);
    };
  //
  // Empty deps - only run once.
  //
  }, []);

  //
  // Helpers.
  //
  const getNode = useCallback(
    (nodeId: string) => state.systemState?.nodes.find((n) => n.node_id === nodeId),
    [state.systemState]
  );

  //
  // Send command and wait for response.
  //
  const sendCommand = useCallback(
    (nodeId: string, command: CommandRequest['command']): Promise<CommandResponse> => {
      return new Promise((resolve) => {
        const commandId = generateUUID();
        const request: CommandRequest = {
          command_id: commandId,
          client_id: clientIdRef.current ?? '',
          node_id: nodeId,
          command,
        };

        pendingCommandsRef.current.set(commandId, resolve);
        wsClient.send({ type: 'command', payload: request });
      });
    },
    []
  );

  //
  // Terminal handlers.
  //
  const registerTerminalHandler = useCallback(
    (nodeId: string, terminalId: string, handler: (output: TerminalOutput) => void) => {
      const key = `${nodeId}:${terminalId}`;
      terminalHandlersRef.current.set(key, handler);
      return () => {
        terminalHandlersRef.current.delete(key);
      };
    },
    []
  );

  const sendTerminalInput = useCallback((nodeId: string, terminalId: string, data: number[]) => {
    wsClient.send({ type: 'terminal_write', node_id: nodeId, terminal_id: terminalId, data });
  }, []);

  //
  // Semantic operations - request list.
  //
  const requestOperations = useCallback(() => {
    wsClient.send({ type: 'semantic_op_list_request' });
  }, []);

  //
  // Semantic operations - run by operation name (service looks up definition).
  //
  const runOperation = useCallback(
    (nodeId: string, agentShortName: string, operationName: string, workingDir?: string) => {
      wsClient.send({
        type: 'semantic_op_run',
        node_id: nodeId,
        agent_short_name: agentShortName,
        operation_name: operationName,
        working_dir: workingDir ?? null,
      });
    },
    []
  );

  const cancelOperation = useCallback((operationId: string) => {
    wsClient.send({ type: 'semantic_op_cancel', operation_id: operationId });
  }, []);

  const removeOperation = useCallback((operationId: string) => {
    wsClient.send({ type: 'semantic_op_remove', operation_id: operationId });
  }, []);

  const clearOperations = useCallback(() => {
    wsClient.send({ type: 'semantic_op_clear' });
  }, []);

  const clearEventLog = useCallback(() => {
    wsClient.send({ type: 'application_log_clear', node_id: null });
    dispatch({ type: 'CLEAR_EVENTS' });
  }, []);

  //
  // Node management.
  //
  const removeNode = useCallback((nodeId: string) => {
    wsClient.send({ type: 'remove_node', node_id: nodeId });
  }, []);

  const resetNode = useCallback((nodeId: string) => {
    wsClient.send({ type: 'reset_node', node_id: nodeId });
  }, []);

  //
  // Config.
  //
  const getConfig = useCallback((keys: string[]) => {
    wsClient.send({ type: 'config_get', keys });
  }, []);

  const setConfig = useCallback((values: Record<string, string>) => {
    wsClient.send({ type: 'config_set', values });
    //
    // Optimistically update local state so UI reflects changes immediately.
    //
    dispatch({ type: 'SET_CONFIG', values });
  }, []);

  //
  // Generic send for any browser message.
  //
  const send = useCallback((message: BrowserMessage) => {
    wsClient.send(message);
  }, []);

  //
  // Clear operation definition status (error/success).
  //
  const clearOpDefStatus = useCallback(() => {
    dispatch({ type: 'SET_OP_DEF_ERROR', error: null });
  }, []);

  //
  // Orchestrator functions (multi-session ACP).
  //

  const webSessionCounter = useRef(1);
  const orchestratorCreateSession = useCallback((modelRef?: string) => {
    dispatch({ type: 'ORCHESTRATOR_CREATING_SESSION' });
    const params: Record<string, unknown> = { cwd: '.', mcpServers: [] };
    if (modelRef) {
      params._meta = { modelRef };
    }
    const jsonRpc = acpRequest('session/new', params);
    const parsed = JSON.parse(jsonRpc);
    pendingAcpRequestsRef.current.set(parsed.id, { method: 'session/new' });
    wsClient.send({ type: 'acp_message', json_rpc: jsonRpc });
  }, []);

  const orchestratorCloseSession = useCallback((sessionId: string) => {
    const jsonRpc = acpRequest('session/close', { sessionId });
    const parsed = JSON.parse(jsonRpc);
    pendingAcpRequestsRef.current.set(parsed.id, { method: 'session/close', sessionId });
    wsClient.send({ type: 'acp_message', json_rpc: jsonRpc });
  }, []);

  const orchestratorCancelPrompt = useCallback((sessionId: string) => {
    const jsonRpc = JSON.stringify({ jsonrpc: '2.0', method: 'session/cancel', params: { sessionId } });
    wsClient.send({ type: 'acp_message', json_rpc: jsonRpc });
    dispatch({ type: 'ORCHESTRATOR_DONE', sessionId });
  }, []);

  const orchestratorSendPrompt = useCallback((sessionId: string, message: string) => {
    const promptId = generateUUID();
    dispatch({ type: 'ORCHESTRATOR_ADD_USER_MESSAGE', sessionId, message, promptId });
    const jsonRpc = acpRequest('session/prompt', {
      sessionId,
      prompt: [{ type: 'text', text: message }],
    });
    const parsed = JSON.parse(jsonRpc);
    pendingAcpRequestsRef.current.set(parsed.id, { method: 'session/prompt', sessionId });
    wsClient.send({ type: 'acp_message', json_rpc: jsonRpc });
  }, []);

  const orchestratorSessionsRef = useRef(state.orchestrator.sessions);
  orchestratorSessionsRef.current = state.orchestrator.sessions;
  const orchestratorActiveIdRef = useRef(state.orchestrator.activeSessionId);
  orchestratorActiveIdRef.current = state.orchestrator.activeSessionId;
  const nodeSessionsRef = useRef(state.nodeSessions);
  nodeSessionsRef.current = state.nodeSessions;

  const orchestratorSetActiveSession = useCallback((sessionId: string | null) => {
    dispatch({ type: 'ORCHESTRATOR_SET_ACTIVE_SESSION', sessionId });

    //
    // If the session hasn't been loaded yet, send session/load to get history.
    //

    if (sessionId) {
      const session = orchestratorSessionsRef.current.find(s => s.sessionId === sessionId);
      if (session && !session.loaded) {
        const jsonRpc = acpRequest('session/load', { sessionId, cwd: '.', mcpServers: [] });
        wsClient.send({ type: 'acp_message', json_rpc: jsonRpc });
        const parsed = JSON.parse(jsonRpc);
        pendingAcpRequestsRef.current.set(parsed.id, { method: 'session/load', sessionId });
      }
    }
  }, []);

  const orchestratorClearMessages = useCallback((sessionId: string) => {
    dispatch({ type: 'ORCHESTRATOR_CLEAR_MESSAGES', sessionId });
  }, []);

  //
  // Send an ACP JSON-RPC request targeted at a specific node. Merges the
  // provided params with `_meta.praxis.nodeId` so the service proxy routes
  // the frame to the owning node. Returns the response result alongside any
  // streamed `agent_message_chunk` text collected while the request was in
  // flight (when `collectText` is true).
  //
  const sendAcpNodeRequest = useCallback(
    (
      nodeId: string,
      method: string,
      params: Record<string, unknown>,
      collectText = false,
    ): Promise<{ result: unknown; text: string }> => {
      const existingMeta = (typeof params._meta === 'object' && params._meta !== null)
        ? (params._meta as Record<string, unknown>)
        : {};
      const existingPraxis = (typeof existingMeta.praxis === 'object' && existingMeta.praxis !== null)
        ? (existingMeta.praxis as Record<string, unknown>)
        : {};
      const merged = {
        ...params,
        _meta: {
          ...existingMeta,
          praxis: { ...existingPraxis, nodeId },
        },
      };
      const jsonRpc = acpRequest(method, merged);
      const parsed = JSON.parse(jsonRpc);
      const sessionId = typeof params.sessionId === 'string' ? params.sessionId : undefined;

      return new Promise<{ result: unknown; text: string }>((resolve, reject) => {
        pendingAcpRequestsRef.current.set(parsed.id, {
          method,
          sessionId,
          collectText,
          textBuf: '',
          resolve: (result, text) => resolve({ result, text }),
          reject,
        });
        wsClient.send({ type: 'acp_message', json_rpc: jsonRpc });
      });
    },
    [],
  );

  const sendAcpNodeNotification = useCallback(
    (nodeId: string, method: string, params: Record<string, unknown>) => {
      const existingMeta = (typeof params._meta === 'object' && params._meta !== null)
        ? (params._meta as Record<string, unknown>)
        : {};
      const existingPraxis = (typeof existingMeta.praxis === 'object' && existingMeta.praxis !== null)
        ? (existingMeta.praxis as Record<string, unknown>)
        : {};
      const merged = {
        ...params,
        _meta: {
          ...existingMeta,
          praxis: { ...existingPraxis, nodeId },
        },
      };
      const jsonRpc = JSON.stringify({ jsonrpc: '2.0', method, params: merged });
      wsClient.send({ type: 'acp_message', json_rpc: jsonRpc });
    },
    [],
  );

  //
  // Traffic interception functions.
  //
  const requestTrafficLog = useCallback((filters: TrafficLogFilters) => {
    wsClient.send({ type: 'traffic_log_request', filters });
  }, []);

  const requestTrafficMatches = useCallback((ruleId: number | null, limit: number, offset: number) => {
    wsClient.send({ type: 'traffic_matches_request', rule_id: ruleId, limit, offset });
  }, []);

  const clearTraffic = useCallback(() => {
    wsClient.send({ type: 'traffic_clear' });
  }, []);

  const requestInterceptRules = useCallback(() => {
    wsClient.send({ type: 'intercept_rule_list' });
  }, []);

  const createInterceptRule = useCallback((
    name: string,
    regexPattern: string,
    targetDirection: TargetDirection,
    scope: RuleScope,
    summarizationPrompt?: string | null
  ) => {
    wsClient.send({
      type: 'intercept_rule_create',
      name,
      regex_pattern: regexPattern,
      target_direction: targetDirection,
      scope,
      summarization_prompt: summarizationPrompt,
    });
  }, []);

  const updateInterceptRule = useCallback((
    id: number,
    updates: {
      name?: string;
      regex_pattern?: string;
      target_direction?: TargetDirection;
      scope?: RuleScope;
      enabled?: boolean;
      summarization_prompt?: string | null;
    }
  ) => {
    wsClient.send({
      type: 'intercept_rule_update',
      id,
      ...updates,
    });
  }, []);

  const deleteInterceptRule = useCallback((id: number) => {
    wsClient.send({ type: 'intercept_rule_delete', id });
  }, []);

  const enableIntercept = useCallback((nodeId: string, method?: InterceptMethod) => {
    wsClient.send({ type: 'intercept_enable', node_id: nodeId, method: method ?? null });
  }, []);

  const disableIntercept = useCallback((nodeId: string) => {
    wsClient.send({ type: 'intercept_disable', node_id: nodeId });
  }, []);

  const clearInterceptRuleError = useCallback(() => {
    dispatch({ type: 'SET_INTERCEPT_RULE_ERROR', error: null });
  }, []);

  //
  // Agent session message helpers.
  //
  const addAgentSessionMessage = useCallback((sessionId: string, message: AgentSessionMessage) => {
    dispatch({ type: 'AGENT_SESSION_ADD_MESSAGE', sessionId, message });
  }, []);

  const clearAgentSessionMessages = useCallback((sessionId: string) => {
    dispatch({ type: 'AGENT_SESSION_CLEAR_MESSAGES', sessionId });
  }, []);

  const clearAgentSessionStreaming = useCallback((nodeId: string) => {
    dispatch({ type: 'AGENT_SESSION_STREAMING_CLEAR', nodeId });
  }, []);

  //
  // Chain operations.
  //
  const requestChainDefList = useCallback(() => {
    wsClient.send({ type: 'chain_def_list' });
  }, []);

  const requestChain = useCallback((chainId: string) => {
    dispatch({ type: 'REQUEST_CHAIN', chain_id: chainId });
    wsClient.send({ type: 'chain_get', chain_id: chainId });
  }, []);

  const createChain = useCallback((definition: ChainDefinitionInput) => {
    wsClient.send({ type: 'chain_create', definition });
  }, []);

  const updateChain = useCallback((chainId: string, definition: ChainDefinitionInput) => {
    wsClient.send({ type: 'chain_update', chain_id: chainId, definition });
  }, []);

  const deleteChain = useCallback((chainId: string) => {
    wsClient.send({ type: 'chain_delete', chain_id: chainId });
  }, []);

  const runChain = useCallback((chainId: string, nodeId: string, agentShortName: string, workingDir?: string, targetSpec?: TargetSpec) => {
    wsClient.send({
      type: 'chain_run',
      chain_id: chainId,
      node_id: nodeId,
      agent_short_name: agentShortName,
      working_dir: workingDir ?? null,
      target_spec: targetSpec ?? null,
    });
  }, []);

  const cancelChainExecution = useCallback((executionId: string) => {
    wsClient.send({ type: 'chain_cancel', execution_id: executionId });
  }, []);

  const removeChainExecution = useCallback((executionId: string) => {
    wsClient.send({ type: 'chain_execution_remove', execution_id: executionId });
    //
    // Optimistically remove from local state.
    //
    dispatch({
      type: 'SET_CHAIN_EXECUTIONS',
      executions: state.chains.executions.filter(e => e.execution_id !== executionId),
    });
  }, [state.chains.executions]);

  const clearChainExecutions = useCallback(() => {
    wsClient.send({ type: 'chain_execution_clear' });
    //
    // Optimistically remove finished from local state.
    //
    dispatch({
      type: 'SET_CHAIN_EXECUTIONS',
      executions: state.chains.executions.filter(e =>
        e.status === 'Running' || e.status === 'Queued'
      ),
    });
  }, [state.chains.executions]);

  const requestChainExecutions = useCallback(() => {
    wsClient.send({ type: 'chain_execution_list' });
  }, []);

  const clearChainStatus = useCallback(() => {
    dispatch({ type: 'SET_CHAIN_ERROR', error: null });
    dispatch({ type: 'SET_CHAIN_SUCCESS', message: null });
  }, []);

  const clearLastCreatedChain = useCallback(() => {
    dispatch({ type: 'SET_LAST_CREATED_CHAIN_ID', chainId: null });
  }, []);

  //
  // Chain trigger methods.
  //

  const requestChainTriggers = useCallback((chainId?: string) => {
    wsClient.send({ type: 'chain_trigger_list', chain_id: chainId ?? null });
  }, []);

  const createChainTrigger = useCallback((chainId: string, triggerConfig: TriggerConfig, targetSpec: TargetSpec) => {
    wsClient.send({ type: 'chain_trigger_create', chain_id: chainId, trigger_config: triggerConfig, target_spec: targetSpec });
  }, []);

  const updateChainTrigger = useCallback((triggerId: string, updates: { enabled?: boolean; trigger_config?: TriggerConfig; target_spec?: TargetSpec }) => {
    wsClient.send({ type: 'chain_trigger_update', trigger_id: triggerId, ...updates });
  }, []);

  const deleteChainTrigger = useCallback((triggerId: string) => {
    wsClient.send({ type: 'chain_trigger_delete', trigger_id: triggerId });
  }, []);

  const trackNodeAccess = useCallback((nodeId: string) => {
    dispatch({ type: 'ACCESS_NODE', nodeId });
  }, []);

  //
  // Agent Chat functions.
  //
  const agentChatStart = useCallback((goal: string | null, yoloMode: boolean) => {
    dispatch({ type: 'AGENT_CHAT_SET_LOADING', loading: true });
    wsClient.send({ type: 'agent_chat_start', goal, yolo_mode: yoloMode });
  }, []);

  const agentChatStop = useCallback(() => {
    if (state.agentChat.session) {
      wsClient.send({ type: 'agent_chat_stop', session_id: state.agentChat.session.id });
    }
  }, [state.agentChat.session]);

  const agentChatAddAgent = useCallback((nodeId: string, agentShortName: string) => {
    if (state.agentChat.session) {
      wsClient.send({
        type: 'agent_chat_add_agent',
        session_id: state.agentChat.session.id,
        node_id: nodeId,
        agent_short_name: agentShortName,
      });
    }
  }, [state.agentChat.session]);

  const agentChatRemoveAgent = useCallback((agentId: string) => {
    if (state.agentChat.session) {
      wsClient.send({
        type: 'agent_chat_remove_agent',
        session_id: state.agentChat.session.id,
        agent_id: agentId,
      });
    }
  }, [state.agentChat.session]);

  const agentChatReorderAgents = useCallback((agentIds: string[]) => {
    if (state.agentChat.session) {
      wsClient.send({
        type: 'agent_chat_reorder_agents',
        session_id: state.agentChat.session.id,
        agent_ids: agentIds,
      });
    }
  }, [state.agentChat.session]);

  const agentChatSendMessage = useCallback((content: string, channelId?: string, recipientNickname?: string) => {
    if (state.agentChat.session) {
      wsClient.send({
        type: 'agent_chat_send_message',
        session_id: state.agentChat.session.id,
        content,
        channel_id: channelId ?? state.agentChat.currentChannelId ?? null,
        recipient_nickname: recipientNickname ?? null,
      });
    }
  }, [state.agentChat.session, state.agentChat.currentChannelId]);

  const agentChatJoinChannel = useCallback((channelName: string) => {
    if (state.agentChat.session) {
      wsClient.send({
        type: 'agent_chat_join_channel',
        session_id: state.agentChat.session.id,
        channel_name: channelName,
      });
    }
  }, [state.agentChat.session]);

  const agentChatGetHistory = useCallback((channelId?: string, limit?: number) => {
    if (state.agentChat.session) {
      wsClient.send({
        type: 'agent_chat_get_history',
        session_id: state.agentChat.session.id,
        channel_id: channelId ?? state.agentChat.currentChannelId ?? null,
        limit: limit ?? 100,
      });
    }
  }, [state.agentChat.session, state.agentChat.currentChannelId]);

  const agentChatGetState = useCallback(() => {
    wsClient.send({
      type: 'agent_chat_get_state',
      session_id: state.agentChat.session?.id ?? null,
    });
  }, [state.agentChat.session]);

  const agentChatSetCurrentChannel = useCallback((channelId: string | null) => {
    dispatch({ type: 'AGENT_CHAT_SET_CURRENT_CHANNEL', channelId });
  }, []);

  const agentChatClearError = useCallback(() => {
    dispatch({ type: 'AGENT_CHAT_CLEAR_ERROR' });
  }, []);

  //
  // LogQuery functions.
  //
  const logQuerySetQuery = useCallback((query: string) => {
    dispatch({ type: 'LOG_QUERY_SET_QUERY', query });
  }, []);

  const logQueryRun = useCallback((query: string) => {
    dispatch({ type: 'LOG_QUERY_START' });
    wsClient.send({ type: 'log_query', query });
  }, []);

  //
  // Lua agent script functions.
  //
  const listLuaAgentScripts = useCallback(() => {
    wsClient.send({ type: 'lua_agent_script_list' });
  }, []);

  const addLuaAgentScript = useCallback((name: string, script: string) => {
    wsClient.send({ type: 'lua_agent_script_add', name, script });
  }, []);

  const updateLuaAgentScript = useCallback((scriptId: string, name: string, script: string) => {
    wsClient.send({ type: 'lua_agent_script_update', script_id: scriptId, name, script });
  }, []);

  const deleteLuaAgentScript = useCallback((scriptId: string) => {
    wsClient.send({ type: 'lua_agent_script_delete', script_id: scriptId });
  }, []);

  const resetLuaAgentScriptDefaults = useCallback(() => {
    wsClient.send({ type: 'lua_agent_script_reset_defaults' });
  }, []);

  const toggleLuaAgentScriptDisabled = useCallback((scriptId: string, disabled: boolean) => {
    wsClient.send({ type: 'lua_agent_script_toggle_disabled', script_id: scriptId, disabled });
  }, []);

  const value = useMemo<AppContextValue>(() => ({
    state,
    dispatch,
    getNode,
    sendCommand,
    sendAcpNodeRequest,
    sendAcpNodeNotification,
    registerTerminalHandler,
    sendTerminalInput,
    requestOperations,
    runOperation,
    cancelOperation,
    removeOperation,
    clearOperations,
    clearEventLog,
    removeNode,
    resetNode,
    getConfig,
    setConfig,
    clearOpDefStatus,
    orchestratorCreateSession,
    orchestratorCloseSession,
    orchestratorCancelPrompt,
    orchestratorSendPrompt,
    orchestratorSetActiveSession,
    orchestratorClearMessages,
    send,
    requestTrafficLog,
    requestTrafficMatches,
    clearTraffic,
    requestInterceptRules,
    createInterceptRule,
    updateInterceptRule,
    deleteInterceptRule,
    enableIntercept,
    disableIntercept,
    clearInterceptRuleError,
    addAgentSessionMessage,
    clearAgentSessionMessages,
    clearAgentSessionStreaming,
    //
    // Chain operations.
    //
    requestChainDefList,
    requestChain,
    createChain,
    updateChain,
    deleteChain,
    runChain,
    cancelChainExecution,
    removeChainExecution,
    clearChainExecutions,
    requestChainExecutions,
    clearChainStatus,
    clearLastCreatedChain,
    //
    // Chain triggers.
    //
    requestChainTriggers,
    createChainTrigger,
    updateChainTrigger,
    deleteChainTrigger,
    trackNodeAccess,
    //
    // Agent Chat.
    //
    agentChatStart,
    agentChatStop,
    agentChatAddAgent,
    agentChatRemoveAgent,
    agentChatReorderAgents,
    agentChatSendMessage,
    agentChatJoinChannel,
    agentChatGetHistory,
    agentChatGetState,
    agentChatSetCurrentChannel,
    agentChatClearError,
    //
    // LogQuery.
    //
    logQuerySetQuery,
    logQueryRun,
    //
    // Lua agent scripts.
    //
    listLuaAgentScripts,
    addLuaAgentScript,
    updateLuaAgentScript,
    deleteLuaAgentScript,
    resetLuaAgentScriptDefaults,
    toggleLuaAgentScriptDisabled,
  }), [
    state,
    dispatch,
    getNode,
    sendCommand,
    sendAcpNodeRequest,
    sendAcpNodeNotification,
    registerTerminalHandler,
    sendTerminalInput,
    requestOperations,
    runOperation,
    cancelOperation,
    removeOperation,
    clearOperations,
    clearEventLog,
    removeNode,
    resetNode,
    getConfig,
    setConfig,
    clearOpDefStatus,
    orchestratorCreateSession,
    orchestratorCloseSession,
    orchestratorCancelPrompt,
    orchestratorSendPrompt,
    orchestratorSetActiveSession,
    orchestratorClearMessages,
    send,
    requestTrafficLog,
    requestTrafficMatches,
    clearTraffic,
    requestInterceptRules,
    createInterceptRule,
    updateInterceptRule,
    deleteInterceptRule,
    enableIntercept,
    disableIntercept,
    clearInterceptRuleError,
    addAgentSessionMessage,
    clearAgentSessionMessages,
    clearAgentSessionStreaming,
    requestChainDefList,
    requestChain,
    createChain,
    updateChain,
    deleteChain,
    runChain,
    cancelChainExecution,
    removeChainExecution,
    clearChainExecutions,
    requestChainExecutions,
    clearChainStatus,
    clearLastCreatedChain,
    requestChainTriggers,
    createChainTrigger,
    updateChainTrigger,
    deleteChainTrigger,
    trackNodeAccess,
    agentChatStart,
    agentChatStop,
    agentChatAddAgent,
    agentChatRemoveAgent,
    agentChatReorderAgents,
    agentChatSendMessage,
    agentChatJoinChannel,
    agentChatGetHistory,
    agentChatGetState,
    agentChatSetCurrentChannel,
    agentChatClearError,
    logQuerySetQuery,
    logQueryRun,
    listLuaAgentScripts,
    addLuaAgentScript,
    updateLuaAgentScript,
    deleteLuaAgentScript,
    resetLuaAgentScriptDefaults,
    toggleLuaAgentScriptDisabled,
  ]);

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}

export function useApp() {
  const context = useContext(AppContext);
  if (!context) {
    throw new Error('useApp must be used within AppProvider');
  }
  return context;
}
