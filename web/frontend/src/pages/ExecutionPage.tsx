import { useReducer, useEffect, useState, useCallback, useMemo, useRef } from 'react';
import {
  Play,
  Loader2,
  AlertCircle,
} from 'lucide-react';
import { useApp } from '../context/AppContext';
import type { ExecutionTab } from '../context/executionTypes';
import type {
  ChainDefinitionInput,
  ChainDefinitionFull,
  ChainExecutionUpdate,
  ChainExecutionEvent,
  OperationDefinitionInfo,
  NodeState,
} from '../api/types';
import { generateUUID } from '../utils/uuid';
import { TabBar } from '../components/execution/TabBar';
import { ExecutionTabContent } from '../components/execution/ExecutionTabContent';
import { InlineOpCreator } from '../components/execution/InlineOpCreator';
import { ChainOrchestratorPane } from '../components/execution/ChainOrchestratorPane';
import { RunModal } from '../components/common/RunModal';
import type { RunItem } from '../components/common/RunModal';

function makeTab(name: string): ExecutionTab {
  return {
    id: generateUUID(),
    name,
    chainId: null,
    localChain: null,
    executionId: null,
    executionEvents: [],
    isDirty: false,
  };
}

//
// Tab reducer actions.
//

type TabAction =
  | { type: 'ADD_TAB'; tab: ExecutionTab }
  | { type: 'CLOSE_TAB'; tabId: string }
  | { type: 'RENAME_TAB'; tabId: string; name: string }
  | { type: 'SET_ACTIVE'; tabId: string }
  | { type: 'SET_CHAIN'; tabId: string; chainId: string }
  | { type: 'SET_EXECUTION'; tabId: string; executionId: string | null }
  | { type: 'SET_LOCAL_CHAIN'; tabId: string; localChain: ChainDefinitionInput | null }
  | { type: 'SET_DIRTY'; tabId: string; dirty: boolean }
  | { type: 'ADD_EXECUTION_EVENT'; tabId: string; event: ChainExecutionEvent };

interface TabState {
  tabs: ExecutionTab[];
  activeTabId: string;
}

function tabReducer(state: TabState, action: TabAction): TabState {
  switch (action.type) {
    case 'ADD_TAB':
      return {
        ...state,
        tabs: [...state.tabs, action.tab],
        activeTabId: action.tab.id,
      };
    case 'CLOSE_TAB': {
      const remaining = state.tabs.filter(t => t.id !== action.tabId);
      if (remaining.length === 0) {
        return { tabs: [], activeTabId: '' };
      }
      const newActive = state.activeTabId === action.tabId
        ? remaining[remaining.length - 1].id
        : state.activeTabId;
      return { tabs: remaining, activeTabId: newActive };
    }
    case 'RENAME_TAB':
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.tabId ? { ...t, name: action.name } : t),
      };
    case 'SET_ACTIVE':
      return { ...state, activeTabId: action.tabId };
    case 'SET_CHAIN':
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.tabId ? { ...t, chainId: action.chainId } : t),
      };
    case 'SET_EXECUTION':
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.tabId ? { ...t, executionId: action.executionId } : t),
      };
    case 'SET_LOCAL_CHAIN':
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.tabId ? { ...t, localChain: action.localChain } : t),
      };
    case 'SET_DIRTY':
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.tabId ? { ...t, isDirty: action.dirty } : t),
      };
    case 'ADD_EXECUTION_EVENT':
      return {
        ...state,
        tabs: state.tabs.map(t => t.id === action.tabId
          ? { ...t, executionEvents: [...t.executionEvents, action.event] }
          : t
        ),
      };
    default:
      return state;
  }
}

export function ExecutionPage() {
  const {
    state,
    send,
    getConfig,
    requestChainDefList,
    requestChain,
    createChain,
    updateChain,
    runChain,
    cancelChainExecution,
    requestChainExecutions,
  } = useApp();

  const initialTab = makeTab('Untitled');
  const [tabState, tabDispatch] = useReducer(tabReducer, {
    tabs: [initialTab],
    activeTabId: initialTab.id,
  });

  const [orchCollapsed, setOrchCollapsed] = useState(true);
  const [showOpCreator, setShowOpCreator] = useState(false);
  const [showRunModal, setShowRunModal] = useState(false);
  const [showOpenChainModal, setShowOpenChainModal] = useState(false);
  const [externalChainUpdate, setExternalChainUpdate] = useState<ChainDefinitionInput | null>(null);

  const activeTab = tabState.tabs.find(t => t.id === tabState.activeTabId) ?? tabState.tabs[0] ?? null;

  //
  // Load data on mount.
  //
  useEffect(() => {
    if (!state.connected) return;
    requestChainDefList();
    requestChainExecutions();
    send({ type: 'op_def_list' });
    getConfig(['llm_model_definitions', 'llm_feature_orchestrator']);
    send({ type: 'payload_list' });
  }, [state.connected, requestChainDefList, requestChainExecutions, send, getConfig]);

  //
  // Parse model definitions from config.
  //
  const modelDefs = useMemo(() => {
    try {
      const raw = state.config.llm_model_definitions;
      if (!raw) return [];
      return JSON.parse(raw) as Array<{ name: string; provider: string; model: string; apiKey: string }>;
    } catch {
      return [];
    }
  }, [state.config.llm_model_definitions]);

  //
  // Get the chain definition for the active tab.
  //
  const activeChain = useMemo((): ChainDefinitionFull | null => {
    if (!activeTab?.chainId) return null;
    return state.chains.chainDefinitionsCache[activeTab.chainId] || state.chains.currentChain;
  }, [activeTab?.chainId, state.chains.chainDefinitionsCache, state.chains.currentChain]);

  //
  // Get execution for the active tab.
  //
  const activeExecution = useMemo((): ChainExecutionUpdate | null => {
    if (!activeTab?.executionId) return null;
    return state.chains.executions.find(e => e.execution_id === activeTab.executionId) || null;
  }, [activeTab?.executionId, state.chains.executions]);

  //
  // Get execution events for the active tab.
  //
  const activeEvents = useMemo((): ChainExecutionEvent[] => {
    if (!activeTab?.executionId) return [];
    return state.executionEvents[activeTab.executionId] || [];
  }, [activeTab?.executionId, state.executionEvents]);

  //
  // Track known tab IDs via ref to avoid stale closure issues when
  // multiple workspace updates arrive in quick succession.
  //
  const knownTabIdsRef = useRef(new Set(tabState.tabs.map(t => t.id)));
  useEffect(() => {
    knownTabIdsRef.current = new Set(tabState.tabs.map(t => t.id));
  }, [tabState.tabs]);

  //
  // Listen for chain orchestrator workspace updates. If the tab_id doesn't
  // match any existing tab, create a new tab for it.
  //
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (!detail) return;

      const tabId = detail.tabId as string | undefined;
      const chainDef = detail.chainDefinition as ChainDefinitionInput | undefined;
      if (!chainDef) return;

      if (tabId && knownTabIdsRef.current.has(tabId)) {
        setExternalChainUpdate(chainDef);
      } else {
        const newId = tabId || generateUUID();
        knownTabIdsRef.current.add(newId);
        const newTab: ExecutionTab = {
          id: newId,
          name: chainDef.name || 'New Tab',
          chainId: null,
          localChain: chainDef,
          executionId: null,
          executionEvents: [],
          isDirty: true,
        };
        tabDispatch({ type: 'ADD_TAB', tab: newTab });
      }
    };
    window.addEventListener('chain-orch-workspace-update', handler);
    return () => window.removeEventListener('chain-orch-workspace-update', handler);
  }, []);

  //
  // Clear external update after it's been consumed.
  //
  useEffect(() => {
    if (externalChainUpdate) {
      const timer = setTimeout(() => setExternalChainUpdate(null), 100);
      return () => clearTimeout(timer);
    }
  }, [externalChainUpdate]);

  //
  // Request chain definition when active tab's chainId changes.
  //
  useEffect(() => {
    if (activeTab?.chainId && !state.chains.chainDefinitionsCache[activeTab.chainId]) {
      requestChain(activeTab.chainId);
    }
  }, [activeTab?.chainId, requestChain, state.chains.chainDefinitionsCache]);

  //
  // Watch for new chain creation and assign to tab.
  //
  useEffect(() => {
    if (activeTab && state.chains.lastCreatedChainId && !activeTab.chainId) {
      tabDispatch({ type: 'SET_CHAIN', tabId: activeTab.id, chainId: state.chains.lastCreatedChainId });
      tabDispatch({ type: 'SET_DIRTY', tabId: activeTab.id, dirty: false });
    }
  }, [state.chains.lastCreatedChainId, activeTab?.chainId, activeTab?.id, activeTab]);

  const handleAddTab = useCallback(() => {
    const count = tabState.tabs.length + 1;
    tabDispatch({ type: 'ADD_TAB', tab: makeTab(`Tab ${count}`) });
  }, [tabState.tabs.length]);

  const handleOpenChain = useCallback((chainId: string) => {
    const chainInfo = state.chains.chains.find(c => c.id === chainId);
    if (!chainInfo) return;
    const newTab: ExecutionTab = {
      id: generateUUID(),
      name: chainInfo.name,
      chainId: chainInfo.id,
      localChain: null,
      executionId: null,
      executionEvents: [],
      isDirty: false,
    };
    tabDispatch({ type: 'ADD_TAB', tab: newTab });
    requestChain(chainInfo.id);
    setShowOpenChainModal(false);
  }, [state.chains.chains, requestChain]);

  const handleSaveChain = useCallback((definition: ChainDefinitionInput) => {
    if (!activeTab) return;
    if (activeTab.chainId) {
      updateChain(activeTab.chainId, definition);
    } else {
      createChain(definition);
    }
    tabDispatch({ type: 'SET_DIRTY', tabId: activeTab.id, dirty: false });

    //
    // Tab name always matches chain name.
    //
    if (definition.name && definition.name !== activeTab.name) {
      tabDispatch({ type: 'RENAME_TAB', tabId: activeTab.id, name: definition.name });
    }
  }, [activeTab, createChain, updateChain]);

  const handleDuplicateChain = useCallback((definition: ChainDefinitionInput) => {
    createChain({ ...definition, name: `${definition.name} (copy)` });
  }, [createChain]);

  const handleRunChain = useCallback(() => {
    if (!activeTab?.chainId) return;
    setShowRunModal(true);
  }, [activeTab?.chainId]);

  const handleRunConfirm = useCallback((itemId: string, nodeId: string, agentShortName: string) => {
    if (!activeTab?.chainId) return;
    runChain(activeTab.chainId, nodeId, agentShortName);
  }, [activeTab?.chainId, runChain]);

  //
  // Watch for new executions matching active tab's chain.
  //
  const lastKnownExecRef = useRef<string | null>(null);
  useEffect(() => {
    if (!activeTab?.chainId || activeTab.executionId) return;

    const matching = state.chains.executions
      .filter(e => e.chain_id === activeTab.chainId && (e.status === 'Running' || e.status === 'Queued'))
      .sort((a, b) => new Date(b.started_at).getTime() - new Date(a.started_at).getTime());

    if (matching.length > 0 && matching[0].execution_id !== lastKnownExecRef.current) {
      lastKnownExecRef.current = matching[0].execution_id;
      tabDispatch({ type: 'SET_EXECUTION', tabId: activeTab.id, executionId: matching[0].execution_id });
    }
  }, [state.chains.executions, activeTab?.chainId, activeTab?.executionId, activeTab?.id]);

  const handleCancelExecution = useCallback(() => {
    if (activeTab?.executionId) {
      cancelChainExecution(activeTab.executionId);
    }
  }, [activeTab?.executionId, cancelChainExecution]);

  const handleClearExecution = useCallback(() => {
    if (!activeTab) return;
    tabDispatch({ type: 'SET_EXECUTION', tabId: activeTab.id, executionId: null });
    lastKnownExecRef.current = null;
  }, [activeTab?.id, activeTab]);

  const handleLocalChainChange = useCallback((chain: ChainDefinitionInput | null) => {
    if (!activeTab) return;
    tabDispatch({ type: 'SET_LOCAL_CHAIN', tabId: activeTab.id, localChain: chain });
    if (chain) {
      tabDispatch({ type: 'SET_DIRTY', tabId: activeTab.id, dirty: true });
    }
  }, [activeTab?.id, activeTab]);

  //
  // Build workspace context for the chain orchestrator. Summarizes the
  // active tab's chain definition so the orchestrator knows what the
  // user is working on.
  //
  //
  // Build workspace context injected into every orchestrator prompt. Includes
  // the active tab ID (so update_workspace targets the right tab) and the
  // full chain definition so the agent understands what's currently built.
  //
  const workspaceContext = useMemo(() => {
    const ctx: Record<string, unknown> = {
      active_tab_id: activeTab?.id ?? null,
      active_tab_name: activeTab?.name ?? null,
    };

    if (activeChain) {
      ctx.chain = {
        id: activeChain.id,
        name: activeChain.name,
        description: activeChain.description,
        category: activeChain.category,
        elements: activeChain.elements,
        connections: activeChain.connections,
        timeout: activeChain.timeout,
      };
    } else if (activeTab?.localChain) {
      ctx.chain = activeTab.localChain;
    } else {
      ctx.chain = null;
    }

    if (activeExecution) {
      ctx.execution = {
        status: activeExecution.status,
        element_count: Object.keys(activeExecution.elements).length,
      };
    }

    ctx.available_operations = state.operationDefs.map(d => d.full_name);
    ctx.available_chains = state.chains.chains.map(c => ({ id: c.id, name: c.name }));
    ctx.connected_nodes = (state.systemState?.nodes ?? []).map(n => ({
      node_id: n.node_id,
      machine_name: n.machine_name,
      agents: n.discovered_agents.filter(a => a.available).map(a => a.short_name),
    }));

    return JSON.stringify(ctx);
  }, [activeTab, activeChain, activeExecution, state.operationDefs, state.chains.chains, state.systemState?.nodes]);

  //
  // Chain list items for the run modal.
  //
  const runItems = useMemo((): RunItem[] => {
    if (!activeTab?.chainId) return [];
    const chainInfo = state.chains.chains.find(c => c.id === activeTab.chainId);
    if (!chainInfo) return [];
    return [{
      id: chainInfo.id,
      name: chainInfo.name,
      description: chainInfo.description,
      badge: `${chainInfo.element_count} elements`,
    }];
  }, [activeTab?.chainId, state.chains.chains]);

  const nodes = state.systemState?.nodes ?? [];

  //
  // Handle op creator callback: refresh op defs after creation.
  //
  const handleOpCreated = useCallback((_fullName: string) => {
    send({ type: 'op_def_list' });
  }, [send]);

  return (
    <div className="h-full flex relative">
      {/*
      //
      // Main content area.
      //
      */}
      <div className="flex-1 flex flex-col min-w-0">
        <TabBar
          tabs={tabState.tabs}
          activeTabId={tabState.activeTabId}
          onSelectTab={(tabId) => tabDispatch({ type: 'SET_ACTIVE', tabId })}
          onCloseTab={(tabId) => tabDispatch({ type: 'CLOSE_TAB', tabId })}
          onRenameTab={(tabId, name) => tabDispatch({ type: 'RENAME_TAB', tabId, name })}
          onAddTab={handleAddTab}
          onOpenChain={() => setShowOpenChainModal(true)}
        />

        {/*
        //
        // Tab content.
        //
        */}
        <div className="flex-1 min-h-0 relative">
          {activeTab ? (
            <ExecutionTabContent
              tab={activeTab}
              chain={activeChain}
              execution={activeExecution}
              executionEvents={activeEvents}
              operationDefs={state.operationDefs}
              modelDefs={modelDefs}
              nodes={nodes}
              toolkitTools={[]}
              payloads={state.payloads}
              send={send}
              saveStatus={state.chains.chainSuccess}
              saveError={state.chains.chainError}
              onSaveChain={handleSaveChain}
              onDuplicateChain={handleDuplicateChain}
              onRunChain={handleRunChain}
              onCancelExecution={handleCancelExecution}
              onCreateOp={() => setShowOpCreator(true)}
              onClearExecution={handleClearExecution}
              externalChainUpdate={externalChainUpdate}
              onLocalChainChange={handleLocalChainChange}
            />
          ) : (
            <div className="h-full flex items-center justify-center">
              <div className="text-center space-y-3">
                <Play size={32} className="mx-auto text-muted" />
                <p className="text-sm text-muted">No tabs open</p>
                <button
                  onClick={handleAddTab}
                  className="px-4 py-2 text-xs border border-dim text-muted hover:text-title hover:border-subtle transition-colors"
                >
                  New Tab
                </button>
              </div>
            </div>
          )}

          {/*
          //
          // Inline operation creator slide-out.
          //
          */}
          {showOpCreator && (
            <InlineOpCreator
              onClose={() => setShowOpCreator(false)}
              onCreated={handleOpCreated}
            />
          )}
        </div>
      </div>

      {/*
      //
      // Chain Orchestrator pane.
      //
      */}
      <ChainOrchestratorPane
        collapsed={orchCollapsed}
        onToggleCollapsed={() => setOrchCollapsed(!orchCollapsed)}
        workspaceContext={workspaceContext}
      />

      {/*
      //
      // Run chain modal.
      //
      */}
      <RunModal
        isOpen={showRunModal}
        onClose={() => setShowRunModal(false)}
        onRun={handleRunConfirm}
        title="Run Chain"
        items={runItems}
        variant="chain"
        preSelectedItem={runItems[0] || null}
        nodes={nodes}
      />

      {/*
      //
      // Open chain from library modal.
      //
      */}
      {showOpenChainModal && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={() => setShowOpenChainModal(false)}>
          <div className="bg-[var(--bg-primary)] border border-subtle ascii-box w-[450px] max-h-[500px] flex flex-col" onClick={e => e.stopPropagation()}>
            <div className="px-4 py-3 border-b border-subtle">
              <h3 className="text-sm font-medium text-title">Open Chain</h3>
            </div>
            <div className="flex-1 overflow-auto">
              {state.chains.chains.length === 0 ? (
                <div className="p-6 text-center text-sm text-muted">No saved chains</div>
              ) : (
                <div className="divide-y divide-subtle">
                  {state.chains.chains.map(c => (
                    <button
                      key={c.id}
                      onClick={() => handleOpenChain(c.id)}
                      className="w-full text-left px-4 py-3 hover:bg-[var(--bg-tertiary)] transition-colors"
                    >
                      <div className="text-sm text-title">{c.name}</div>
                      {c.description && (
                        <div className="text-xs text-muted mt-0.5 truncate">{c.description}</div>
                      )}
                      <div className="text-[10px] text-muted mt-1">
                        {c.element_count} elements · {c.category}
                      </div>
                    </button>
                  ))}
                </div>
              )}
            </div>
            <div className="px-4 py-2 border-t border-subtle flex justify-end">
              <button
                onClick={() => setShowOpenChainModal(false)}
                className="px-3 py-1.5 text-xs text-muted border border-dim hover:border-subtle transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
