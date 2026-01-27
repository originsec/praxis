import type { SkynetMessage, SkynetState } from '../context/skynetTypes';

const SKYNET_SESSION_STORAGE_KEY = 'praxis_skynet_session';
const RECENT_NODES_STORAGE_KEY = 'praxis_recent_nodes';

function serializeSkynetState(state: SkynetState): string {
  return JSON.stringify({
    ...state,
    messages: state.messages.map((msg) => ({
      ...msg,
      timestamp: msg.timestamp.toISOString(),
    })),
  });
}

function deserializeSkynetState(json: string): SkynetState | null {
  try {
    const parsed = JSON.parse(json);
    return {
      ...parsed,
      messages: parsed.messages.map((msg: SkynetMessage & { timestamp: string }) => ({
        ...msg,
        timestamp: new Date(msg.timestamp),
      })),
    };
  } catch {
    return null;
  }
}

export function loadPersistedSkynetState(initial: SkynetState): SkynetState {
  try {
    const stored = sessionStorage.getItem(SKYNET_SESSION_STORAGE_KEY);
    if (stored) {
      const state = deserializeSkynetState(stored);
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

export function persistSkynetState(state: SkynetState): void {
  try {
    if (state.sessionActive) {
      sessionStorage.setItem(SKYNET_SESSION_STORAGE_KEY, serializeSkynetState(state));
    } else {
      //
      // Clear storage when session is stopped.
      //
      sessionStorage.removeItem(SKYNET_SESSION_STORAGE_KEY);
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
