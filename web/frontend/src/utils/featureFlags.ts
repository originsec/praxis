//
// Feature flags that can be enabled via devtools console.
// Usage: window.praxis.enableOrchestrator() or window.praxis.disableOrchestrator()
//

const STORAGE_KEY = 'praxis_feature_flags';

export interface FeatureFlags {
  orchestrator: boolean;
  agentChat: boolean;
  orchestratorExecutionTopology: boolean;
}

const defaultFlags: FeatureFlags = {
  orchestrator: true,
  agentChat: false,
  orchestratorExecutionTopology: false,
};

//
// Load flags from localStorage.
//
function loadFlags(): FeatureFlags {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      return { ...defaultFlags, ...JSON.parse(stored) };
    }
  } catch (e) {
    console.error('Failed to load feature flags:', e);
  }
  return { ...defaultFlags };
}

//
// Save flags to localStorage.
//
function saveFlags(flags: FeatureFlags): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(flags));
  } catch (e) {
    console.error('Failed to save feature flags:', e);
  }
}

//
// Current flags state.
//
let currentFlags = loadFlags();

//
// Listeners for flag changes.
//
type FlagChangeListener = (flags: FeatureFlags) => void;
const listeners: Set<FlagChangeListener> = new Set();

function notifyListeners() {
  listeners.forEach(listener => listener(currentFlags));
}

//
// Public API for getting flags.
//
export function getFeatureFlags(): FeatureFlags {
  return { ...currentFlags };
}

//
// Subscribe to flag changes.
//
export function subscribeToFlags(listener: FlagChangeListener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

//
// Devtools API exposed on window.praxis.
//
interface PraxisDevtools {
  enableOrchestrator: () => void;
  disableOrchestrator: () => void;
  enableAgentChat: () => void;
  disableAgentChat: () => void;
  enableOrchestratorExecutionTopology: () => void;
  disableOrchestratorExecutionTopology: () => void;
  getFlags: () => FeatureFlags;
}

const devtools: PraxisDevtools = {
  enableOrchestrator: () => {
    currentFlags = { ...currentFlags, orchestrator: true };
    saveFlags(currentFlags);
    notifyListeners();
    console.log('%c[Praxis] Orchestrator enabled. Refresh the page to see changes.', 'color: #22c55e');
  },
  disableOrchestrator: () => {
    currentFlags = { ...currentFlags, orchestrator: false };
    saveFlags(currentFlags);
    notifyListeners();
    console.log('%c[Praxis] Orchestrator disabled. Refresh the page to see changes.', 'color: #ef4444');
  },
  enableAgentChat: () => {
    currentFlags = { ...currentFlags, agentChat: true };
    saveFlags(currentFlags);
    notifyListeners();
    console.log('%c[Praxis] Agent Chat enabled. Refresh the page to see changes.', 'color: #22c55e');
  },
  disableAgentChat: () => {
    currentFlags = { ...currentFlags, agentChat: false };
    saveFlags(currentFlags);
    notifyListeners();
    console.log('%c[Praxis] Agent Chat disabled. Refresh the page to see changes.', 'color: #ef4444');
  },
  enableOrchestratorExecutionTopology: () => {
    currentFlags = { ...currentFlags, orchestratorExecutionTopology: true };
    saveFlags(currentFlags);
    notifyListeners();
    console.log('%c[Praxis] Orchestrator execution topology enabled. Refresh the page to see changes.', 'color: #22c55e');
  },
  disableOrchestratorExecutionTopology: () => {
    currentFlags = { ...currentFlags, orchestratorExecutionTopology: false };
    saveFlags(currentFlags);
    notifyListeners();
    console.log('%c[Praxis] Orchestrator execution topology disabled. Refresh the page to see changes.', 'color: #ef4444');
  },
  getFlags: () => getFeatureFlags(),
};

//
// Expose on window for devtools console access.
//
declare global {
  interface Window {
    praxis: PraxisDevtools;
  }
}

export function initFeatureFlags(): void {
  window.praxis = devtools;
  console.log(
    '%c[Praxis] Devtools available. Use window.praxis.enableOrchestrator() to enable the orchestrator feature.',
    'color: #3b82f6'
  );
}
