import { createContext, useContext, useReducer, useEffect, useCallback, useRef, type ReactNode } from 'react';
import { wsClient } from '../api/websocket';
import { generateUUID } from '../utils/uuid';
import type { AtlasState } from './atlasTypes';

//
// Re-export Atlas types for consumers.
//
export type { AtlasMessage, AtlasToolExecution } from './atlasTypes';
import { loadPersistedAtlasState, loadRecentNodes, persistRecentNodes, persistAtlasState } from '../utils/persistence';
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
  AtlasPlan,
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
  DiscoveredLlmEndpoint,
  NexusAgentInfo,
  NexusAgentStatus,
  NexusChannelInfo,
  NexusMessageInfo,
  NexusSessionState,
} from '../api/types';

//
// Agent session message types.
//
export interface AgentSessionMessage {
  role: 'user' | 'assistant';
  content: string;
  timestamp: Date;
}

const initialAtlasState: AtlasState = {
  sessionActive: false,
  isStarting: false,
  messages: [],
  currentPlan: null,
  isLoading: false,
  streamingContent: '',
  currentToolExecutions: [],
  tokenUsage: null,
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
  chainError: string | null;
  chainSuccess: string | null;
}

const initialChainState: ChainState = {
  chains: [],
  currentChain: null,
  chainDefinitionsCache: {},
  loadingChains: new Set(),
  executions: [],
  chainError: null,
  chainSuccess: null,
};

//
// Agent discovery state.
//
interface DiscoveryState {
  endpoints: DiscoveredLlmEndpoint[];
  isLoading: boolean;
  error: string | null;
}

const initialDiscoveryState: DiscoveryState = {
  endpoints: [],
  isLoading: false,
  error: null,
};

//
// Nexus state.
//
interface NexusState {
  session: NexusSessionState | null;
  currentChannelId: string | null;
  messages: NexusMessageInfo[];
  isLoading: boolean;
  error: string | null;
}

const initialNexusState: NexusState = {
  session: null,
  currentChannelId: null,
  messages: [],
  isLoading: false,
  error: null,
};

//
// Event log panel UI state.
//
interface EventLogPanelState {
  isOpen: boolean;
  height: number;
}

const initialEventLogPanelState: EventLogPanelState = {
  isOpen: false,
  height: 300,
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
  atlas: AtlasState;
  intercept: InterceptState;
  chains: ChainState;
  discovery: DiscoveryState;
  nexus: NexusState;
  eventLogPanel: EventLogPanelState;
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
    atlas: loadPersistedAtlasState(initialAtlasState),
    intercept: initialInterceptState,
    chains: initialChainState,
    discovery: initialDiscoveryState,
    nexus: initialNexusState,
    eventLogPanel: initialEventLogPanelState,
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
  | { type: 'SET_CONFIG'; values: Record<string, string> }
  | { type: 'SET_OP_DEF_ERROR'; error: string | null }
  | { type: 'SET_OP_DEF_SUCCESS'; fullName: string | null }
  | { type: 'ATLAS_STARTING' }
  | { type: 'ATLAS_STARTED' }
  | { type: 'ATLAS_STOPPED' }
  | { type: 'ATLAS_ADD_USER_MESSAGE'; message: string }
  | { type: 'ATLAS_ADD_CONTENT'; content: string }
  | { type: 'ATLAS_TOOL_EXECUTING'; name: string; input?: string }
  | { type: 'ATLAS_TOOL_EXECUTED'; name: string; display: string; success: boolean; result: string }
  | { type: 'ATLAS_PLAN_UPDATED'; plan: AtlasPlan }
  | { type: 'ATLAS_DONE' }
  | { type: 'ATLAS_ERROR'; message: string }
  | { type: 'ATLAS_CLEAR_MESSAGES' }
  | { type: 'ATLAS_SET_LOADING'; loading: boolean }
  | { type: 'ATLAS_TOKEN_USAGE'; promptTokens: number; completionTokens: number; totalTokens: number }
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
  | { type: 'SET_CHAIN_ERROR'; error: string | null }
  | { type: 'SET_CHAIN_SUCCESS'; message: string | null }
  //
  // Recent nodes action.
  //
  | { type: 'ACCESS_NODE'; nodeId: string }
  //
  // Agent discovery actions.
  //
  | { type: 'SET_DISCOVERED_ENDPOINTS'; endpoints: DiscoveredLlmEndpoint[] }
  | { type: 'SET_DISCOVERY_LOADING'; loading: boolean }
  | { type: 'SET_DISCOVERY_ERROR'; error: string | null }
  //
  // Event log panel actions.
  //
  | { type: 'TOGGLE_EVENT_LOG_PANEL' }
  | { type: 'SET_EVENT_LOG_PANEL_HEIGHT'; height: number }
  //
  // Nexus actions.
  //
  | { type: 'NEXUS_SESSION_STARTED'; sessionId: string; goal: string | null }
  | { type: 'NEXUS_SESSION_STOPPED'; sessionId: string }
  | { type: 'NEXUS_AGENT_ADDED'; sessionId: string; agent: NexusAgentInfo }
  | { type: 'NEXUS_AGENT_REMOVED'; sessionId: string; agentId: string }
  | { type: 'NEXUS_AGENT_STATUS_CHANGED'; sessionId: string; agentId: string; status: NexusAgentStatus }
  | { type: 'NEXUS_CHANNEL_CREATED'; sessionId: string; channel: NexusChannelInfo }
  | { type: 'NEXUS_CHANNEL_UPDATED'; sessionId: string; channel: NexusChannelInfo }
  | { type: 'NEXUS_AGENT_JOINED_CHANNEL'; sessionId: string; agentId: string; channelId: string }
  | { type: 'NEXUS_AGENT_LEFT_CHANNEL'; sessionId: string; agentId: string; channelId: string }
  | { type: 'NEXUS_MESSAGE'; sessionId: string; message: NexusMessageInfo }
  | { type: 'NEXUS_STATE_UPDATE'; session: NexusSessionState }
  | { type: 'NEXUS_HISTORY_RESPONSE'; sessionId: string; channelId: string | null; messages: NexusMessageInfo[] }
  | { type: 'NEXUS_ERROR'; message: string }
  | { type: 'NEXUS_SET_CURRENT_CHANNEL'; channelId: string | null }
  | { type: 'NEXUS_CLEAR_ERROR' }
  | { type: 'NEXUS_SET_LOADING'; loading: boolean };

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
    case 'SET_CONFIG':
      return { ...state, config: { ...state.config, ...action.values } };
    case 'SET_OP_DEF_ERROR':
      return { ...state, opDefError: action.error, opDefSuccess: null };
    case 'SET_OP_DEF_SUCCESS':
      return { ...state, opDefSuccess: action.fullName, opDefError: null };
    default:
      return null;
  }
}

function reduceAtlas(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'ATLAS_STARTING':
      return {
        ...state,
        atlas: {
          ...state.atlas,
          isStarting: true,
        },
      };
    case 'ATLAS_STARTED':
      return {
        ...state,
        atlas: {
          ...initialAtlasState,
          sessionActive: true,
          isStarting: false,
          messages: [{
            id: generateUUID(),
            role: 'system',
            content: 'Atlas session started.',
            timestamp: new Date(),
          }],
        },
      };
    case 'ATLAS_STOPPED':
      return {
        ...state,
        atlas: {
          ...state.atlas,
          sessionActive: false,
          isStarting: false,
          isLoading: false,
        },
      };
    case 'ATLAS_ADD_USER_MESSAGE':
      return {
        ...state,
        atlas: {
          ...state.atlas,
          messages: [...state.atlas.messages, {
            id: generateUUID(),
            role: 'user',
            content: action.message,
            timestamp: new Date(),
          }],
          isLoading: true,
          streamingContent: '',
          currentToolExecutions: [],
        },
      };
    case 'ATLAS_ADD_CONTENT':
      return {
        ...state,
        atlas: {
          ...state.atlas,
          streamingContent: state.atlas.streamingContent + action.content,
        },
      };
    case 'ATLAS_TOOL_EXECUTING':
      return {
        ...state,
        atlas: {
          ...state.atlas,
          currentToolExecutions: [...state.atlas.currentToolExecutions, {
            name: action.name,
            display: 'Executing...',
            success: true,
            executing: true,
            input: action.input,
          }],
        },
      };
    case 'ATLAS_TOOL_EXECUTED': {
      const executions = state.atlas.currentToolExecutions.map((ex) =>
        ex.name === action.name && ex.executing
          ? { name: action.name, display: action.display, success: action.success, executing: false, input: ex.input, result: action.result }
          : ex
      );
      return {
        ...state,
        atlas: {
          ...state.atlas,
          currentToolExecutions: executions,
        },
      };
    }
    case 'ATLAS_PLAN_UPDATED':
      return {
        ...state,
        atlas: {
          ...state.atlas,
          currentPlan: action.plan,
        },
      };
    case 'ATLAS_DONE': {
      //
      // Finalize the current streaming content and tool executions into a
      // message.
      //
      const newMessages = [...state.atlas.messages];
      if (state.atlas.streamingContent || state.atlas.currentToolExecutions.length > 0) {
        newMessages.push({
          id: generateUUID(),
          role: 'assistant',
          content: state.atlas.streamingContent,
          timestamp: new Date(),
          toolExecutions: state.atlas.currentToolExecutions.length > 0
            ? [...state.atlas.currentToolExecutions]
            : undefined,
        });
      }
      return {
        ...state,
        atlas: {
          ...state.atlas,
          messages: newMessages,
          isLoading: false,
          streamingContent: '',
          currentToolExecutions: [],
        },
      };
    }
    case 'ATLAS_ERROR': {
      const newMessages = [...state.atlas.messages, {
        id: generateUUID(),
        role: 'system' as const,
        content: `Error: ${action.message}`,
        timestamp: new Date(),
      }];
      return {
        ...state,
        atlas: {
          ...state.atlas,
          messages: newMessages,
          isStarting: false,
          isLoading: false,
          streamingContent: '',
          currentToolExecutions: [],
        },
      };
    }
    case 'ATLAS_CLEAR_MESSAGES':
      return {
        ...state,
        atlas: {
          ...state.atlas,
          messages: [],
          currentPlan: null,
        },
      };
    case 'ATLAS_SET_LOADING':
      return {
        ...state,
        atlas: {
          ...state.atlas,
          isLoading: action.loading,
        },
      };
    case 'ATLAS_TOKEN_USAGE':
      return {
        ...state,
        atlas: {
          ...state.atlas,
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
      return { ...state, chains: { ...state.chains, chains: updatedChains } };
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
    case 'SET_CHAIN_ERROR':
      return { ...state, chains: { ...state.chains, chainError: action.error, chainSuccess: null } };
    case 'SET_CHAIN_SUCCESS':
      return { ...state, chains: { ...state.chains, chainSuccess: action.message, chainError: null } };
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

function reduceDiscovery(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'SET_DISCOVERED_ENDPOINTS':
      return {
        ...state,
        discovery: {
          ...state.discovery,
          endpoints: action.endpoints,
          isLoading: false,
        },
      };
    case 'SET_DISCOVERY_LOADING':
      return {
        ...state,
        discovery: {
          ...state.discovery,
          isLoading: action.loading,
        },
      };
    case 'SET_DISCOVERY_ERROR':
      return {
        ...state,
        discovery: {
          ...state.discovery,
          error: action.error,
          isLoading: false,
        },
      };
    default:
      return null;
  }
}

function reduceEventLogPanel(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'TOGGLE_EVENT_LOG_PANEL':
      return {
        ...state,
        eventLogPanel: {
          ...state.eventLogPanel,
          isOpen: !state.eventLogPanel.isOpen,
        },
      };
    case 'SET_EVENT_LOG_PANEL_HEIGHT':
      return {
        ...state,
        eventLogPanel: {
          ...state.eventLogPanel,
          height: action.height,
        },
      };
    default:
      return null;
  }
}

function reduceNexus(state: AppState, action: Action): AppState | null {
  switch (action.type) {
    case 'NEXUS_SESSION_STARTED': {
      const newSession: NexusSessionState = {
        id: action.sessionId,
        goal: action.goal,
        status: 'active',
        agents: [],
        channels: [],
        created_at: new Date().toISOString(),
      };
      return {
        ...state,
        nexus: {
          ...state.nexus,
          session: newSession,
          messages: [],
          error: null,
        },
      };
    }
    case 'NEXUS_SESSION_STOPPED':
      return {
        ...state,
        nexus: {
          ...initialNexusState,
        },
      };
    case 'NEXUS_AGENT_ADDED':
      if (!state.nexus.session || state.nexus.session.id !== action.sessionId) return state;
      return {
        ...state,
        nexus: {
          ...state.nexus,
          session: {
            ...state.nexus.session,
            agents: [...state.nexus.session.agents, action.agent],
          },
        },
      };
    case 'NEXUS_AGENT_REMOVED':
      if (!state.nexus.session || state.nexus.session.id !== action.sessionId) return state;
      return {
        ...state,
        nexus: {
          ...state.nexus,
          session: {
            ...state.nexus.session,
            agents: state.nexus.session.agents.filter(a => a.id !== action.agentId),
          },
        },
      };
    case 'NEXUS_AGENT_STATUS_CHANGED':
      if (!state.nexus.session || state.nexus.session.id !== action.sessionId) return state;
      return {
        ...state,
        nexus: {
          ...state.nexus,
          session: {
            ...state.nexus.session,
            agents: state.nexus.session.agents.map(a =>
              a.id === action.agentId ? { ...a, status: action.status } : a
            ),
          },
        },
      };
    case 'NEXUS_CHANNEL_CREATED':
      if (!state.nexus.session || state.nexus.session.id !== action.sessionId) return state;
      return {
        ...state,
        nexus: {
          ...state.nexus,
          session: {
            ...state.nexus.session,
            channels: [...state.nexus.session.channels, action.channel],
          },
          //
          // Auto-select the first channel if none selected.
          //
          currentChannelId: state.nexus.currentChannelId ?? action.channel.id,
        },
      };
    case 'NEXUS_CHANNEL_UPDATED':
      if (!state.nexus.session || state.nexus.session.id !== action.sessionId) return state;
      return {
        ...state,
        nexus: {
          ...state.nexus,
          session: {
            ...state.nexus.session,
            channels: state.nexus.session.channels.map(c =>
              c.id === action.channel.id ? action.channel : c
            ),
          },
        },
      };
    case 'NEXUS_AGENT_JOINED_CHANNEL':
      if (!state.nexus.session || state.nexus.session.id !== action.sessionId) return state;
      return {
        ...state,
        nexus: {
          ...state.nexus,
          session: {
            ...state.nexus.session,
            agents: state.nexus.session.agents.map(a =>
              a.id === action.agentId ? { ...a, current_channel_id: action.channelId } : a
            ),
          },
        },
      };
    case 'NEXUS_AGENT_LEFT_CHANNEL':
      if (!state.nexus.session || state.nexus.session.id !== action.sessionId) return state;
      return {
        ...state,
        nexus: {
          ...state.nexus,
          session: {
            ...state.nexus.session,
            agents: state.nexus.session.agents.map(a =>
              a.id === action.agentId && a.current_channel_id === action.channelId
                ? { ...a, current_channel_id: null }
                : a
            ),
          },
        },
      };
    case 'NEXUS_MESSAGE':
      if (!state.nexus.session || state.nexus.session.id !== action.sessionId) return state;
      return {
        ...state,
        nexus: {
          ...state.nexus,
          messages: [...state.nexus.messages, action.message],
        },
      };
    case 'NEXUS_STATE_UPDATE':
      return {
        ...state,
        nexus: {
          ...state.nexus,
          session: action.session,
          currentChannelId: state.nexus.currentChannelId ?? action.session.channels[0]?.id ?? null,
        },
      };
    case 'NEXUS_HISTORY_RESPONSE':
      return {
        ...state,
        nexus: {
          ...state.nexus,
          messages: action.messages,
        },
      };
    case 'NEXUS_ERROR':
      return {
        ...state,
        nexus: {
          ...state.nexus,
          error: action.message,
          isLoading: false,
        },
      };
    case 'NEXUS_SET_CURRENT_CHANNEL':
      return {
        ...state,
        nexus: {
          ...state.nexus,
          currentChannelId: action.channelId,
        },
      };
    case 'NEXUS_CLEAR_ERROR':
      return {
        ...state,
        nexus: {
          ...state.nexus,
          error: null,
        },
      };
    case 'NEXUS_SET_LOADING':
      return {
        ...state,
        nexus: {
          ...state.nexus,
          isLoading: action.loading,
        },
      };
    default:
      return null;
  }
}

function reducer(state: AppState, action: Action): AppState {
  return (
    reduceCore(state, action)
    ?? reduceAtlas(state, action)
    ?? reduceIntercept(state, action)
    ?? reduceAgentSessions(state, action)
    ?? reduceChains(state, action)
    ?? reduceRecentNodes(state, action)
    ?? reduceDiscovery(state, action)
    ?? reduceEventLogPanel(state, action)
    ?? reduceNexus(state, action)
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
  //
  // Node Management.
  //
  removeNode: (nodeId: string) => void;
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
  // Atlas.
  //
  atlasStart: () => void;
  atlasStop: () => void;
  atlasCancel: () => void;
  atlasPrompt: (message: string) => void;
  atlasClearMessages: () => void;
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
  runChain: (chainId: string, nodeId: string, agentShortName: string, workingDir?: string) => void;
  cancelChainExecution: (executionId: string) => void;
  removeChainExecution: (executionId: string) => void;
  //
  // Recent nodes tracking.
  //
  trackNodeAccess: (nodeId: string) => void;
  clearChainExecutions: () => void;
  requestChainExecutions: () => void;
  clearChainStatus: () => void;
  //
  // Agent discovery.
  //
  enableAgentDiscovery: (nodeId: string) => void;
  disableAgentDiscovery: (nodeId: string) => void;
  requestDiscoveredEndpoints: (nodeId?: string) => void;
  createDynamicAgent: (nodeId: string, endpointId: string, agentName: string, shortName: string) => void;
  deleteDynamicAgent: (nodeId: string, shortName: string) => void;
  clearDiscoveryError: () => void;
  //
  // Event log panel.
  //
  toggleEventLogPanel: () => void;
  setEventLogPanelHeight: (height: number) => void;
  //
  // Nexus.
  //
  nexusStart: (goal: string | null, yoloMode: boolean) => void;
  nexusStop: () => void;
  nexusAddAgent: (nodeId: string, agentShortName: string) => void;
  nexusRemoveAgent: (agentId: string) => void;
  nexusReorderAgents: (agentIds: string[]) => void;
  nexusSendMessage: (content: string, channelId?: string, recipientNickname?: string) => void;
  nexusJoinChannel: (channelName: string) => void;
  nexusGetHistory: (channelId?: string, limit?: number) => void;
  nexusGetState: () => void;
  nexusSetCurrentChannel: (channelId: string | null) => void;
  nexusClearError: () => void;
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
  // Persist Atlas state to sessionStorage whenever it changes.
  //
  useEffect(() => {
    persistAtlasState(state.atlas);
  }, [state.atlas]);

  //
  // Handle WebSocket messages - only set up once.
  //
  useEffect(() => {
    const handleMessage = (message: ServerMessage) => {
      switch (message.type) {
        case 'connected':
          dispatch({ type: 'SET_CONNECTED', connected: true, clientId: message.client_id, version: message.version });
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
        // Atlas messages.
        //
        case 'atlas_started':
          dispatch({ type: 'ATLAS_STARTED' });
          break;
        case 'atlas_stopped':
          dispatch({ type: 'ATLAS_STOPPED' });
          break;
        case 'atlas_content':
          dispatch({ type: 'ATLAS_ADD_CONTENT', content: message.content });
          break;
        case 'atlas_tool_executing':
          dispatch({ type: 'ATLAS_TOOL_EXECUTING', name: message.name, input: message.input });
          break;
        case 'atlas_tool_executed':
          dispatch({ type: 'ATLAS_TOOL_EXECUTED', name: message.name, display: message.display, success: message.success, result: message.result });
          break;
        case 'atlas_plan_updated':
          dispatch({ type: 'ATLAS_PLAN_UPDATED', plan: message.plan });
          break;
        case 'atlas_done':
          dispatch({ type: 'ATLAS_DONE' });
          break;
        case 'atlas_error':
          dispatch({ type: 'ATLAS_ERROR', message: message.message });
          break;
        case 'atlas_token_usage':
          dispatch({
            type: 'ATLAS_TOKEN_USAGE',
            promptTokens: message.prompt_tokens,
            completionTokens: message.completion_tokens,
            totalTokens: message.total_tokens,
          });
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
        // Agent discovery messages.
        //
        case 'discovered_endpoints_list':
          dispatch({ type: 'SET_DISCOVERED_ENDPOINTS', endpoints: message.endpoints });
          break;
        case 'dynamic_agent_created':
          //
          // TODO: Show success toast.
          //
          break;
        case 'dynamic_agent_deleted':
          //
          // TODO: Show success toast.
          //
          break;
        case 'agent_discovery_error':
          dispatch({ type: 'SET_DISCOVERY_ERROR', error: message.message });
          break;

        //
        // Application log messages.
        //
        case 'application_log_response':
          //
          // Dispatch as custom event for ApplicationLogTab to catch.
          //
          window.dispatchEvent(new CustomEvent('ws-message', { detail: message }));
          break;
        case 'application_log_cleared':
          //
          // Dispatch as custom event for ApplicationLogTab to catch.
          //
          window.dispatchEvent(new CustomEvent('ws-message', { detail: message }));
          break;

        //
        // Recon messages.
        //
        case 'recon_get_response':
          //
          // Dispatch as custom event for AgentDetailPage to catch.
          //
          window.dispatchEvent(new CustomEvent('ws-message', { detail: message }));
          break;

        //
        // Nexus messages.
        //
        case 'nexus_session_started':
          dispatch({ type: 'NEXUS_SESSION_STARTED', sessionId: message.session_id, goal: message.goal });
          break;
        case 'nexus_session_stopped':
          dispatch({ type: 'NEXUS_SESSION_STOPPED', sessionId: message.session_id });
          break;
        case 'nexus_agent_added':
          dispatch({ type: 'NEXUS_AGENT_ADDED', sessionId: message.session_id, agent: message.agent });
          break;
        case 'nexus_agent_removed':
          dispatch({ type: 'NEXUS_AGENT_REMOVED', sessionId: message.session_id, agentId: message.agent_id });
          break;
        case 'nexus_agent_status_changed':
          dispatch({ type: 'NEXUS_AGENT_STATUS_CHANGED', sessionId: message.session_id, agentId: message.agent_id, status: message.status });
          break;
        case 'nexus_channel_created':
          dispatch({ type: 'NEXUS_CHANNEL_CREATED', sessionId: message.session_id, channel: message.channel });
          break;
        case 'nexus_channel_updated':
          dispatch({ type: 'NEXUS_CHANNEL_UPDATED', sessionId: message.session_id, channel: message.channel });
          break;
        case 'nexus_agent_joined_channel':
          dispatch({ type: 'NEXUS_AGENT_JOINED_CHANNEL', sessionId: message.session_id, agentId: message.agent_id, channelId: message.channel_id });
          break;
        case 'nexus_agent_left_channel':
          dispatch({ type: 'NEXUS_AGENT_LEFT_CHANNEL', sessionId: message.session_id, agentId: message.agent_id, channelId: message.channel_id });
          break;
        case 'nexus_message':
          dispatch({ type: 'NEXUS_MESSAGE', sessionId: message.session_id, message: message.message });
          break;
        case 'nexus_state_update':
          dispatch({ type: 'NEXUS_STATE_UPDATE', session: message.session });
          break;
        case 'nexus_history_response':
          dispatch({ type: 'NEXUS_HISTORY_RESPONSE', sessionId: message.session_id, channelId: message.channel_id, messages: message.messages });
          break;
        case 'nexus_error':
          dispatch({ type: 'NEXUS_ERROR', message: message.message });
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

  //
  // Node management.
  //
  const removeNode = useCallback((nodeId: string) => {
    wsClient.send({ type: 'remove_node', node_id: nodeId });
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
  // Atlas functions.
  //
  const atlasStart = useCallback(() => {
    dispatch({ type: 'ATLAS_STARTING' });
    wsClient.send({ type: 'atlas_start' });
  }, []);

  const atlasStop = useCallback(() => {
    wsClient.send({ type: 'atlas_stop' });
    dispatch({ type: 'ATLAS_STOPPED' });
  }, []);

  const atlasCancel = useCallback(() => {
    wsClient.send({ type: 'atlas_cancel' });
    dispatch({ type: 'ATLAS_DONE' });
  }, []);

  const atlasPrompt = useCallback((message: string) => {
    dispatch({ type: 'ATLAS_ADD_USER_MESSAGE', message });
    wsClient.send({ type: 'atlas_prompt', message });
  }, []);

  const atlasClearMessages = useCallback(() => {
    dispatch({ type: 'ATLAS_CLEAR_MESSAGES' });
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

  const runChain = useCallback((chainId: string, nodeId: string, agentShortName: string, workingDir?: string) => {
    wsClient.send({
      type: 'chain_run',
      chain_id: chainId,
      node_id: nodeId,
      agent_short_name: agentShortName,
      working_dir: workingDir ?? null,
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

  const trackNodeAccess = useCallback((nodeId: string) => {
    dispatch({ type: 'ACCESS_NODE', nodeId });
  }, []);

  //
  // Agent discovery functions.
  //
  const enableAgentDiscovery = useCallback((nodeId: string) => {
    wsClient.send({ type: 'agent_discovery_enable', node_id: nodeId });
  }, []);

  const disableAgentDiscovery = useCallback((nodeId: string) => {
    wsClient.send({ type: 'agent_discovery_disable', node_id: nodeId });
  }, []);

  const requestDiscoveredEndpoints = useCallback((nodeId?: string) => {
    dispatch({ type: 'SET_DISCOVERY_LOADING', loading: true });
    wsClient.send({ type: 'discovered_endpoints_request', node_id: nodeId ?? null });
  }, []);

  const createDynamicAgent = useCallback((
    nodeId: string,
    endpointId: string,
    agentName: string,
    shortName: string
  ) => {
    wsClient.send({
      type: 'create_dynamic_agent',
      node_id: nodeId,
      endpoint_id: endpointId,
      agent_name: agentName,
      short_name: shortName,
    });
  }, []);

  const deleteDynamicAgent = useCallback((nodeId: string, shortName: string) => {
    wsClient.send({
      type: 'delete_dynamic_agent',
      node_id: nodeId,
      short_name: shortName,
    });
  }, []);

  const clearDiscoveryError = useCallback(() => {
    dispatch({ type: 'SET_DISCOVERY_ERROR', error: null });
  }, []);

  //
  // Event log panel functions.
  //
  const toggleEventLogPanel = useCallback(() => {
    dispatch({ type: 'TOGGLE_EVENT_LOG_PANEL' });
  }, []);

  const setEventLogPanelHeight = useCallback((height: number) => {
    dispatch({ type: 'SET_EVENT_LOG_PANEL_HEIGHT', height });
  }, []);

  //
  // Nexus functions.
  //
  const nexusStart = useCallback((goal: string | null, yoloMode: boolean) => {
    dispatch({ type: 'NEXUS_SET_LOADING', loading: true });
    wsClient.send({ type: 'nexus_start', goal, yolo_mode: yoloMode });
  }, []);

  const nexusStop = useCallback(() => {
    if (state.nexus.session) {
      wsClient.send({ type: 'nexus_stop', session_id: state.nexus.session.id });
    }
  }, [state.nexus.session]);

  const nexusAddAgent = useCallback((nodeId: string, agentShortName: string) => {
    if (state.nexus.session) {
      wsClient.send({
        type: 'nexus_add_agent',
        session_id: state.nexus.session.id,
        node_id: nodeId,
        agent_short_name: agentShortName,
      });
    }
  }, [state.nexus.session]);

  const nexusRemoveAgent = useCallback((agentId: string) => {
    if (state.nexus.session) {
      wsClient.send({
        type: 'nexus_remove_agent',
        session_id: state.nexus.session.id,
        agent_id: agentId,
      });
    }
  }, [state.nexus.session]);

  const nexusReorderAgents = useCallback((agentIds: string[]) => {
    if (state.nexus.session) {
      wsClient.send({
        type: 'nexus_reorder_agents',
        session_id: state.nexus.session.id,
        agent_ids: agentIds,
      });
    }
  }, [state.nexus.session]);

  const nexusSendMessage = useCallback((content: string, channelId?: string, recipientNickname?: string) => {
    if (state.nexus.session) {
      wsClient.send({
        type: 'nexus_send_message',
        session_id: state.nexus.session.id,
        content,
        channel_id: channelId ?? state.nexus.currentChannelId ?? null,
        recipient_nickname: recipientNickname ?? null,
      });
    }
  }, [state.nexus.session, state.nexus.currentChannelId]);

  const nexusJoinChannel = useCallback((channelName: string) => {
    if (state.nexus.session) {
      wsClient.send({
        type: 'nexus_join_channel',
        session_id: state.nexus.session.id,
        channel_name: channelName,
      });
    }
  }, [state.nexus.session]);

  const nexusGetHistory = useCallback((channelId?: string, limit?: number) => {
    if (state.nexus.session) {
      wsClient.send({
        type: 'nexus_get_history',
        session_id: state.nexus.session.id,
        channel_id: channelId ?? state.nexus.currentChannelId ?? null,
        limit: limit ?? 100,
      });
    }
  }, [state.nexus.session, state.nexus.currentChannelId]);

  const nexusGetState = useCallback(() => {
    wsClient.send({
      type: 'nexus_get_state',
      session_id: state.nexus.session?.id ?? null,
    });
  }, [state.nexus.session]);

  const nexusSetCurrentChannel = useCallback((channelId: string | null) => {
    dispatch({ type: 'NEXUS_SET_CURRENT_CHANNEL', channelId });
  }, []);

  const nexusClearError = useCallback(() => {
    dispatch({ type: 'NEXUS_CLEAR_ERROR' });
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
    removeNode,
    getConfig,
    setConfig,
    clearOpDefStatus,
    atlasStart,
    atlasStop,
    atlasCancel,
    atlasPrompt,
    atlasClearMessages,
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
    trackNodeAccess,
    //
    // Agent discovery.
    //
    enableAgentDiscovery,
    disableAgentDiscovery,
    requestDiscoveredEndpoints,
    createDynamicAgent,
    deleteDynamicAgent,
    clearDiscoveryError,
    //
    // Event log panel.
    //
    toggleEventLogPanel,
    setEventLogPanelHeight,
    //
    // Nexus.
    //
    nexusStart,
    nexusStop,
    nexusAddAgent,
    nexusRemoveAgent,
    nexusReorderAgents,
    nexusSendMessage,
    nexusJoinChannel,
    nexusGetHistory,
    nexusGetState,
    nexusSetCurrentChannel,
    nexusClearError,
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
