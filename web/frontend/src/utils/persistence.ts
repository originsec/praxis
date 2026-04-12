import type { OrchestratorMessage, OrchestratorState, OrchestratorSessionState } from '../context/orchestratorTypes';

const ORCHESTRATOR_SESSION_STORAGE_KEY = 'praxis_orchestrator_session';
const RECENT_NODES_STORAGE_KEY = 'praxis_recent_nodes';

function serializeSessionMessages(messages: OrchestratorMessage[]): object[] {
  return messages.map((msg) => ({
    ...msg,
    timestamp: msg.timestamp.toISOString(),
  }));
}

function deserializeSessionMessages(messages: Array<OrchestratorMessage & { timestamp: string }>): OrchestratorMessage[] {
  return messages.map((msg) => ({
    ...msg,
    timestamp: new Date(msg.timestamp),
  }));
}

function serializeOrchestratorState(state: OrchestratorState): string {
  return JSON.stringify({
    ...state,
    sessions: state.sessions.map((session) => ({
      ...session,
      messages: serializeSessionMessages(session.messages),
    })),
  });
}

function deserializeOrchestratorState(json: string): OrchestratorState | null {
  try {
    const parsed = JSON.parse(json);
    return {
      ...parsed,
      sessions: (parsed.sessions || []).map((session: OrchestratorSessionState & { messages: Array<OrchestratorMessage & { timestamp: string }> }) => ({
        ...session,
        messages: deserializeSessionMessages(session.messages),
        //
        // Reset transient states that shouldn't persist across page loads.
        //
        isLoading: false,
        streamingContent: '',
        hadToolCall: false,
        currentToolExecutions: [],
      })),
      isStarting: false,
    };
  } catch {
    return null;
  }
}

export function loadPersistedOrchestratorState(initial: OrchestratorState): OrchestratorState {
  try {
    const stored = sessionStorage.getItem(ORCHESTRATOR_SESSION_STORAGE_KEY);
    if (stored) {
      const state = deserializeOrchestratorState(stored);
      if (state) {
        return state;
      }
    }
  } catch {
    //
    // sessionStorage might not be available.
    //
  }
  return initial;
}

export function persistOrchestratorState(state: OrchestratorState): void {
  try {
    if (state.sessions.length > 0) {
      sessionStorage.setItem(ORCHESTRATOR_SESSION_STORAGE_KEY, serializeOrchestratorState(state));
    } else {
      sessionStorage.removeItem(ORCHESTRATOR_SESSION_STORAGE_KEY);
    }
  } catch {
    //
    // sessionStorage might not be available or quota exceeded.
    //
  }
}

export function loadRecentNodes(maxCount: number): string[] {
  try {
    const stored = localStorage.getItem(RECENT_NODES_STORAGE_KEY);
    if (stored) {
      const parsed = JSON.parse(stored);
      if (Array.isArray(parsed)) {
        return parsed.slice(0, maxCount);
      }
    }
  } catch {
    //
    // Ignore parse errors.
    //
  }
  return [];
}

export function persistRecentNodes(nodes: string[]): void {
  try {
    localStorage.setItem(RECENT_NODES_STORAGE_KEY, JSON.stringify(nodes));
  } catch {
    //
    // Ignore storage errors.
    //
  }
}
