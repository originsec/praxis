import type { NexusMessage, NexusState } from '../context/nexusTypes';

const NEXUS_SESSION_STORAGE_KEY = 'praxis_nexus_session';
const RECENT_NODES_STORAGE_KEY = 'praxis_recent_nodes';

function serializeNexusState(state: NexusState): string {
  return JSON.stringify({
    ...state,
    messages: state.messages.map((msg) => ({
      ...msg,
      timestamp: msg.timestamp.toISOString(),
    })),
  });
}

function deserializeNexusState(json: string): NexusState | null {
  try {
    const parsed = JSON.parse(json);
    return {
      ...parsed,
      messages: parsed.messages.map((msg: NexusMessage & { timestamp: string }) => ({
        ...msg,
        timestamp: new Date(msg.timestamp),
      })),
    };
  } catch {
    return null;
  }
}

export function loadPersistedNexusState(initial: NexusState): NexusState {
  try {
    const stored = sessionStorage.getItem(NEXUS_SESSION_STORAGE_KEY);
    if (stored) {
      const state = deserializeNexusState(stored);
      if (state) {
        //
        // Reset transient states that shouldn't persist across page loads.
        //
        return {
          ...state,
          isStarting: false,
          isLoading: false,
          streamingContent: '',
          currentToolExecutions: [],
        };
      }
    }
  } catch {
    //
    // sessionStorage might not be available.
    //
  }
  return initial;
}

export function persistNexusState(state: NexusState): void {
  try {
    if (state.sessionActive) {
      sessionStorage.setItem(NEXUS_SESSION_STORAGE_KEY, serializeNexusState(state));
    } else {
      //
      // Clear storage when session is stopped.
      //
      sessionStorage.removeItem(NEXUS_SESSION_STORAGE_KEY);
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
