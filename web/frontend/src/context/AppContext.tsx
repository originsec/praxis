import { createContext, useContext, useReducer, useEffect, useCallback, useRef, type ReactNode } from 'react';
import { wsClient } from '../api/websocket';
import { generateUUID } from '../utils/uuid';
import type { OrchestratorState } from './orchestratorTypes';

//
// Re-export Orchestrator types for consumers.
//
export type { OrchestratorMessage, OrchestratorToolExecution } from './orchestratorTypes';
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

const initialOrchestratorState: OrchestratorState = {
  sessionActive: false,
  isStarting: false,
  provider: null,
  model: null,
  messages: [],
  currentPlan: null,
  isLoading: false,
  streamingContent: '',
  currentToolExecutions: [],
  tokenUsage: null,
  currentPromptId: null,
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
// Hunting state.
//
interface HuntingState {
  query: string;
  isRunning: boolean;
  columns: string[];
  rows: unknown[][];
  totalCount: number;
  error: string | null;
}

const initialHuntingState: HuntingState = {
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
  hunting: HuntingState;
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
  // Recently accessed node IDs (most recent first).
  //
  recentlyAccessedNodeIds: string[];
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
    hunting: initialHuntingState,
    chains: initialChainState,
    agentChat: initialAgentChatState,
    toolkit: initialToolkitState,
    luaAgentScripts: [],
    payloads: [],
    agentSessionMessages: {},
    recentlyAccessedNodeIds: loadRecentNodes(MAX_RECENT_NODES),
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
  | { type: 'ORCHESTRATOR_STARTING' }
  | { type: 'ORCHESTRATOR_STARTED'; provider: string; model: string }
  | { type: 'ORCHESTRATOR_STOPPED' }
  | { type: 'ORCHESTRATOR_ADD_USER_MESSAGE'; message: string; promptId: string }
  | { type: 'ORCHESTRATOR_ADD_CONTENT'; content: string }
  | { type: 'ORCHESTRATOR_TOOL_EXECUTING'; name: string; input?: string }
  | { type: 'ORCHESTRATOR_TOOL_EXECUTED'; name: string; display: string; success: boolean; result: string }
  | { type: 'ORCHESTRATOR_PLAN_UPDATED'; plan: OrchestratorPlan }
  | { type: 'ORCHESTRATOR_DONE' }
  | { type: 'ORCHESTRATOR_ERROR'; message: string }
  | { type: 'ORCHESTRATOR_CLEAR_MESSAGES' }
  | { type: 'ORCHESTRATOR_SET_LOADING'; loading: boolean }
  | { type: 'ORCHESTRATOR_TOKEN_USAGE'; promptTokens: number; completionTokens: number; totalTokens: number }
  //
  // Hunting actions.
  //
  | { type: 'HUNTING_SET_QUERY'; query: string }
  | { type: 'HUNTING_QUERY_START' }
  | { type: 'HUNTING_QUERY_RESPONSE'; columns: string[]; rows: unknown[][]; totalCount: number }
  | { type: 'HUNTING_QUERY_ERROR'; message: string }
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

function reduceOrchestrator(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'ORCHESTRATOR_STARTING':
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          isStarting: true,
        },
      };
    case 'ORCHESTRATOR_STARTED':
      return {
        ...state,
        orchestrator: {
          ...initialOrchestratorState,
          sessionActive: true,
          isStarting: false,
          provider: action.provider,
          model: action.model,
          messages: [{
            id: generateUUID(),
            role: 'system',
            content: `Orchestrator session started (${action.provider}::${action.model}).`,
            timestamp: new Date(),
          }],
        },
      };
    case 'ORCHESTRATOR_STOPPED':
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          sessionActive: false,
          isStarting: false,
          isLoading: false,
          messages: state.orchestrator.sessionActive
            ? [
                ...state.orchestrator.messages,
                {
                  id: generateUUID(),
                  role: 'system',
                  content: 'Orchestrator session stopped.',
                  timestamp: new Date(),
                },
              ]
            : state.orchestrator.messages,
        },
      };
    case 'ORCHESTRATOR_ADD_USER_MESSAGE':
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          messages: [...state.orchestrator.messages, {
            id: generateUUID(),
            role: 'user',
            content: action.message,
            timestamp: new Date(),
          }],
          isLoading: true,
          streamingContent: '',
          currentToolExecutions: [],
          currentPromptId: action.promptId,
        },
      };
    case 'ORCHESTRATOR_ADD_CONTENT': {
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          streamingContent: state.orchestrator.streamingContent + action.content,
        },
      };
    }
    case 'ORCHESTRATOR_TOOL_EXECUTING':
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          currentToolExecutions: [...state.orchestrator.currentToolExecutions, {
            name: action.name,
            display: 'Executing...',
            success: true,
            executing: true,
            input: action.input,
          }],
        },
      };
    case 'ORCHESTRATOR_TOOL_EXECUTED': {
      const executions = state.orchestrator.currentToolExecutions.map((ex) =>
        ex.name === action.name && ex.executing
          ? { name: action.name, display: action.display, success: action.success, executing: false, input: ex.input, result: action.result }
          : ex
      );
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          currentToolExecutions: executions,
        },
      };
    }
    case 'ORCHESTRATOR_PLAN_UPDATED':
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          currentPlan: action.plan,
        },
      };
    case 'ORCHESTRATOR_DONE': {
      //
      // Finalize the current streaming content and tool executions into a
      // message.
      //
      const newMessages = [...state.orchestrator.messages];
      if (state.orchestrator.streamingContent || state.orchestrator.currentToolExecutions.length > 0) {
        newMessages.push({
          id: generateUUID(),
          role: 'assistant',
          content: state.orchestrator.streamingContent,
          timestamp: new Date(),
          toolExecutions: state.orchestrator.currentToolExecutions.length > 0
            ? [...state.orchestrator.currentToolExecutions]
            : undefined,
        });
      }
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          messages: newMessages,
          isLoading: false,
          streamingContent: '',
          currentToolExecutions: [],
        },
      };
    }
    case 'ORCHESTRATOR_ERROR': {
      const newMessages = [...state.orchestrator.messages, {
        id: generateUUID(),
        role: 'system' as const,
        content: `Error: ${action.message}`,
        timestamp: new Date(),
      }];
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          messages: newMessages,
          isStarting: false,
          isLoading: false,
          streamingContent: '',
          currentToolExecutions: [],
        },
      };
    }
    case 'ORCHESTRATOR_CLEAR_MESSAGES':
      return {
        ...state,
        orchestrator: {
          ...initialOrchestratorState,
          sessionActive: state.orchestrator.sessionActive,
          provider: state.orchestrator.provider,
          model: state.orchestrator.model,
        },
      };
    case 'ORCHESTRATOR_SET_LOADING':
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          isLoading: action.loading,
        },
      };
    case 'ORCHESTRATOR_TOKEN_USAGE':
      return {
        ...state,
        orchestrator: {
          ...state.orchestrator,
          tokenUsage: {
            promptTokens: action.promptTokens,
            completionTokens: action.completionTokens,
            totalTokens: action.totalTokens,
          },
        },
      };
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

function reduceHunting(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'HUNTING_SET_QUERY':
      return {
        ...state,
        hunting: { ...state.hunting, query: action.query },
      };
    case 'HUNTING_QUERY_START':
      return {
        ...state,
        hunting: { ...state.hunting, isRunning: true, error: null },
      };
    case 'HUNTING_QUERY_RESPONSE':
      return {
        ...state,
        hunting: {
          ...state.hunting,
          isRunning: false,
          columns: action.columns,
          rows: action.rows,
          totalCount: action.totalCount,
          error: null,
        },
      };
    case 'HUNTING_QUERY_ERROR':
      return {
        ...state,
        hunting: { ...state.hunting, isRunning: false, error: action.message },
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
    ?? reduceHunting(state, action)
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
  // Helpers.
  //
  getNode: (nodeId: string) => NodeState | undefined;
  //
  // Commands.
  //
  sendCommand: (nodeId: string, command: CommandRequest['command']) => Promise<CommandResponse>;
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
  // Orchestrator.
  //
  orchestratorStart: () => void;
  orchestratorStop: () => void;
  orchestratorCancel: () => void;
  orchestratorPrompt: (message: string) => void;
  orchestratorClearMessages: () => void;
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
  // Hunting.
  //
  huntingSetQuery: (query: string) => void;
  huntingQuery: (query: string) => void;
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
        case 'connected':
          dispatch({ type: 'SET_CONNECTED', connected: true, clientId: message.client_id, version: message.version });
          wsClient.send({ type: 'config_get', keys: ['prompt_timeout_secs'] });
          break;
        case 'state_update':
          //
          // Debug: Log selected_agent info from state updates.
          //
          if (message.state.nodes?.length > 0) {
            const nodeWithSession = message.state.nodes.find(n => n.selected_agent?.session_id);
            if (nodeWithSession) {
              console.log('[state_update] Node with session:', nodeWithSession.node_id, 'selected_agent:', nodeWithSession.selected_agent);
            }
          }
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
        // Orchestrator messages. Events carry a prompt_id; discard stale
        // events that don't match the current prompt.
        //
        case 'orchestrator_started':
          dispatch({ type: 'ORCHESTRATOR_STARTED', provider: message.provider, model: message.model });
          break;
        case 'orchestrator_stopped':
          dispatch({ type: 'ORCHESTRATOR_STOPPED' });
          break;
        case 'orchestrator_content':
          if (message.prompt_id === String(orchestratorPromptSeq.current)) {
            dispatch({ type: 'ORCHESTRATOR_ADD_CONTENT', content: message.content });
          }
          break;
        case 'orchestrator_tool_executing':
          if (message.prompt_id === String(orchestratorPromptSeq.current) && message.name !== 'report_plan') {
            dispatch({ type: 'ORCHESTRATOR_TOOL_EXECUTING', name: message.name, input: message.input });
          }
          break;
        case 'orchestrator_tool_executed':
          if (message.prompt_id === String(orchestratorPromptSeq.current) && message.name !== 'report_plan') {
            dispatch({ type: 'ORCHESTRATOR_TOOL_EXECUTED', name: message.name, display: message.display, success: message.success, result: message.result });
          }
          break;
        case 'orchestrator_plan_updated':
          if (message.prompt_id === String(orchestratorPromptSeq.current)) {
            dispatch({ type: 'ORCHESTRATOR_PLAN_UPDATED', plan: message.plan });
          }
          break;
        case 'orchestrator_done':
          if (message.prompt_id === String(orchestratorPromptSeq.current)) {
            dispatch({ type: 'ORCHESTRATOR_DONE' });
          }
          break;
        case 'orchestrator_error':
          if (message.prompt_id === String(orchestratorPromptSeq.current)) {
            dispatch({ type: 'ORCHESTRATOR_ERROR', message: message.message });
          }
          break;
        case 'orchestrator_token_usage':
          if (message.prompt_id === String(orchestratorPromptSeq.current)) {
            dispatch({
              type: 'ORCHESTRATOR_TOKEN_USAGE',
              promptTokens: message.prompt_tokens,
              completionTokens: message.completion_tokens,
              totalTokens: message.total_tokens,
            });
          }
          break;
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
        // Hunting messages.
        //
        case 'hunting_query_response':
          dispatch({ type: 'HUNTING_QUERY_RESPONSE', columns: message.columns, rows: message.rows, totalCount: message.total_count });
          break;
        case 'hunting_query_error':
          dispatch({ type: 'HUNTING_QUERY_ERROR', message: message.message });
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
      }
    };

    const unsubscribe = wsClient.addHandler(handleMessage);

    //
    // Connect to WebSocket.
    //
    wsClient.connect().catch(console.error);

    return () => {
      unsubscribe();
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
  // Orchestrator functions.
  //
  const orchestratorStart = useCallback(() => {
    dispatch({ type: 'ORCHESTRATOR_STARTING' });
    wsClient.send({ type: 'orchestrator_start' });
  }, []);

  const orchestratorStop = useCallback(() => {
    wsClient.send({ type: 'orchestrator_stop' });
    dispatch({ type: 'ORCHESTRATOR_STOPPED' });
  }, []);

  const orchestratorCancel = useCallback(() => {
    wsClient.send({ type: 'orchestrator_cancel' });
    dispatch({ type: 'ORCHESTRATOR_DONE' });
  }, []);

  const orchestratorPromptSeq = useRef(0);

  const orchestratorPrompt = useCallback((message: string) => {
    orchestratorPromptSeq.current += 1;
    const promptId = String(orchestratorPromptSeq.current);
    dispatch({ type: 'ORCHESTRATOR_ADD_USER_MESSAGE', message, promptId });
    wsClient.send({ type: 'orchestrator_prompt', prompt_id: promptId, message });
  }, []);

  const orchestratorClearMessages = useCallback(() => {
    dispatch({ type: 'ORCHESTRATOR_CLEAR_MESSAGES' });
  }, []);

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
  // Hunting functions.
  //
  const huntingSetQuery = useCallback((query: string) => {
    dispatch({ type: 'HUNTING_SET_QUERY', query });
  }, []);

  const huntingQuery = useCallback((query: string) => {
    dispatch({ type: 'HUNTING_QUERY_START' });
    wsClient.send({ type: 'hunting_query', query });
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

  const value: AppContextValue = {
    state,
    getNode,
    sendCommand,
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
    orchestratorStart,
    orchestratorStop,
    orchestratorCancel,
    orchestratorPrompt,
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
    // Hunting.
    //
    huntingSetQuery,
    huntingQuery,
    //
    // Lua agent scripts.
    //
    listLuaAgentScripts,
    addLuaAgentScript,
    updateLuaAgentScript,
    deleteLuaAgentScript,
    resetLuaAgentScriptDefaults,
    toggleLuaAgentScriptDisabled,
  };

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}

export function useApp() {
  const context = useContext(AppContext);
  if (!context) {
    throw new Error('useApp must be used within AppProvider');
  }
  return context;
}
