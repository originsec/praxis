const RECENT_NODES_STORAGE_KEY = 'praxis_recent_nodes';

//
// Orchestrator session state is intentionally not persisted: the
// service holds no orchestrator state, and the web client opens a
// single ephemeral session per page load.
//

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
