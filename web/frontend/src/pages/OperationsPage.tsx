import { useState, useEffect, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { Zap, X, Trash2, Clock, Square, Loader2, Play, GitBranch, ChevronDown, ChevronRight } from 'lucide-react';
import { useApp } from '../context/AppContext';
import { StatusBadge, getOperationStatusColor } from '../components/common/StatusBadge';
import { RunModal } from '../components/common/RunModal';
import { OperationDetailModal } from '../components/common/OperationDetailModal';
import { ChainExecutionModal } from '../components/common/ChainExecutionModal';
import { LibraryTab } from '../components/library/LibraryTab';
import type { OperationDefinitionInfo, ChainDefinitionFull } from '../api/types';

type FilterStatus = 'all' | 'Running' | 'Completed' | 'Failed' | 'Cancelled' | 'Queued';
type MainTab = 'runs' | 'library';

export function OperationsPage() {
  const { state, send, cancelOperation, removeOperation, clearOperations, runChain, cancelChainExecution, removeChainExecution, clearChainExecutions, requestChainExecutions, requestChainDefList, requestChain, requestOperations } = useApp();
  const operations = state.operations;
  const [searchParams, setSearchParams] = useSearchParams();

  //
  // Tab from URL or default.
  //
  const tabParam = searchParams.get('tab');
  const mainTab: MainTab = tabParam === 'library' ? 'library' : 'runs';
  const setMainTab = (tab: MainTab) => {
    setSearchParams({ tab }, { replace: true });
  };
  const [filter, setFilter] = useState<FilterStatus>('all');

  //
  // Operation detail modal state - store just the ID, derive actual data from
  // state so it updates live.
  //
  const [selectedOpId, setSelectedOpId] = useState<string | null>(null);

  //
  // Derive selected operation from current state (so it updates live).
  //
  const selectedOp = useMemo(() => {
    if (!selectedOpId) return null;
    return operations.find(op => op.operation_id === selectedOpId) ?? null;
  }, [selectedOpId, operations]);

  //
  // Library tab state (uses operationDefs from context).
  //
  const definitions = state.operationDefs;
  const [showRunModal, setShowRunModal] = useState(false);
  const [preSelectedOpDef, setPreSelectedOpDef] = useState<OperationDefinitionInfo | null>(null);

  //
  // Chain run modal state.
  //
  const [showRunChainModal, setShowRunChainModal] = useState(false);

  //
  // Chain execution detail modal state - store just the ID, derive actual data
  // from state.
  //
  const [selectedChainExecId, setSelectedChainExecId] = useState<string | null>(null);

  //
  // Collapsible sections state for runs tab.
  //
  const [chainsCollapsed, setChainsCollapsed] = useState(false);
  const [opsCollapsed, setOpsCollapsed] = useState(false);

  //
  // Get chain executions.
  //
  const chainExecutions = state.chains.executions;

  //
  // Derive selected execution from current state (so it updates live).
  //
  const selectedChainExec = useMemo(() => {
    if (!selectedChainExecId) return null;
    return chainExecutions.find(e => e.execution_id === selectedChainExecId) ?? null;
  }, [selectedChainExecId, chainExecutions]);
  const chains = state.chains.chains;
  const isConnected = state.connected;

  //
  // Fetch definitions and chains when connected.
  //
  useEffect(() => {
    if (isConnected) {
      send({ type: 'op_def_list' });
      requestChainDefList();
    }
  }, [isConnected, send, requestChainDefList]);

  //
  // Fetch chain executions and operations when on runs tab and connected.
  //
  useEffect(() => {
    if (isConnected && mainTab === 'runs') {
      requestChainExecutions();
      requestOperations();
    }
  }, [mainTab, isConnected, requestChainExecutions, requestOperations]);

  const handleRunOperation = (opFullName: string, nodeId: string, agentName: string) => {
    send({
      type: 'semantic_op_run',
      node_id: nodeId,
      agent_short_name: agentName,
      operation_name: opFullName,
      working_dir: null,
    });
    setMainTab('runs');
  };

  const handleRunChainFromModal = (chainId: string, nodeId: string, agentName: string) => {
    runChain(chainId, nodeId, agentName);
    setMainTab('runs');
  };

  const filteredOperations = (filter === 'all'
      ? operations
      : operations.filter((op) => op.status === filter)
    ).sort((a, b) => new Date(b.start_time).getTime() - new Date(a.start_time).getTime());

  //
  // Filter chain executions using the same status filter.
  //
  const filteredChainExecutions = (filter === 'all'
      ? chainExecutions
      : chainExecutions.filter((exec) => exec.status === filter)
    ).sort((a, b) => new Date(b.started_at).getTime() - new Date(a.started_at).getTime());

  //
  // Fetch chain definition when execution is selected.
  //
  useEffect(() => {
    if (selectedChainExec) {
      requestChain(selectedChainExec.chain_id);
    }
  }, [selectedChainExec, requestChain]);

  //
  // Use the cached chain definition or current chain from state.
  //
  const selectedChainDef = useMemo((): ChainDefinitionFull | null => {
    if (!selectedChainExec) return null;
    //
    // First check the cache for the chain definition.
    //
    const cached = state.chains.chainDefinitionsCache[selectedChainExec.chain_id];
    if (cached) return cached;
    //
    // Fall back to current chain if it matches.
    //
    if (state.chains.currentChain?.id === selectedChainExec.chain_id) {
      return state.chains.currentChain;
    }
    return null;
  }, [selectedChainExec, state.chains.chainDefinitionsCache, state.chains.currentChain]);

  //
  // Check if chain is currently loading.
  //
  const isChainLoading = useMemo(() => {
    if (!selectedChainExec) return false;
    return state.chains.loadingChains.has(selectedChainExec.chain_id);
  }, [selectedChainExec, state.chains.loadingChains]);

  const formatDuration = (start: string, end: string | null) => {
    const startTime = new Date(start).getTime();
    const endTime = end ? new Date(end).getTime() : Date.now();
    const diffMs = endTime - startTime;
    const diffSecs = Math.floor(diffMs / 1000);
    const mins = Math.floor(diffSecs / 60);
    const secs = diffSecs % 60;
    return mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;
  };

  const filters: { value: FilterStatus; label: string }[] = [
    { value: 'all', label: 'All' },
    { value: 'Running', label: 'Running' },
    { value: 'Completed', label: 'Completed' },
    { value: 'Failed', label: 'Failed' },
    { value: 'Cancelled', label: 'Cancelled' },
    { value: 'Queued', label: 'Queued' },
  ];

  return (
    <div className="space-y-4 md:space-y-6">
      {/*
      //
      // Page header.
      //
      */}
      <div className="flex flex-col lg:flex-row lg:items-center lg:justify-between gap-3">
        <div>
          <h1 className="text-2xl font-bold text-highlight">Operations</h1>
          <p className="text-muted mt-1">Semantic operations and automation tasks</p>
        </div>
        <div className="flex flex-wrap items-center gap-1.5 sm:gap-2">
          {mainTab === 'runs' && (
            <>
              <button
                onClick={() => {
                  setPreSelectedOpDef(null);
                  setShowRunModal(true);
                }}
                disabled={definitions.length === 0}
                className="inline-flex items-center gap-1.5 sm:gap-2 px-2.5 sm:px-3 py-1.5 sm:py-2 text-xs sm:text-sm bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors disabled:opacity-50"
              >
                <Play size={14} />
                Run Op
              </button>
              <button
                onClick={() => setShowRunChainModal(true)}
                disabled={chains.length === 0}
                className="inline-flex items-center gap-1.5 sm:gap-2 px-2.5 sm:px-3 py-1.5 sm:py-2 text-xs sm:text-sm bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors disabled:opacity-50"
              >
                <GitBranch size={14} />
                Run Chain
              </button>
              <button
                onClick={() => { clearOperations(); clearChainExecutions(); }}
                className="inline-flex items-center gap-1.5 sm:gap-2 px-2.5 sm:px-3 py-1.5 sm:py-2 text-xs sm:text-sm border border-subtle hover:bg-[var(--bg-tertiary)] text-muted hover:text-[var(--text-primary)] transition-colors"
              >
                <Trash2 size={14} />
                Clear Finished
              </button>
            </>
          )}
        </div>
      </div>

      {/*
      //
      // Main tabs.
      //
      */}
      <div className="flex gap-4 border-b border-subtle overflow-x-auto">
        <button
          onClick={() => setMainTab('runs')}
          className={`pb-3 px-1 text-sm font-medium transition-colors border-b-2 ${
            mainTab === 'runs'
              ? 'text-title border-[var(--accent-info)]'
              : 'text-muted hover:text-[var(--text-primary)] border-transparent'
          }`}
        >
          Runs
        </button>
        <button
          onClick={() => setMainTab('library')}
          className={`pb-3 px-1 text-sm font-medium transition-colors border-b-2 ${
            mainTab === 'library'
              ? 'text-title border-[var(--accent-info)]'
              : 'text-muted hover:text-[var(--text-primary)] border-transparent'
          }`}
        >
          Library
        </button>
      </div>

      {mainTab === 'runs' && (
        <>
          {/*
          //
          // Filters.
          //
          */}
      <div className="flex flex-wrap gap-2">
        {filters.map((f) => {
          const opCount = operations.filter((op) => op.status === f.value).length;
          const chainCount = chainExecutions.filter((exec) => exec.status === f.value).length;
          const totalCount = opCount + chainCount;
          return (
            <button
              key={f.value}
              onClick={() => setFilter(f.value)}
              className={`px-3 py-1.5 text-sm transition-colors ${
                filter === f.value
                  ? 'bg-[var(--accent-info)]/20 text-[var(--accent-info)] border border-[var(--accent-info)]/50'
                  : 'text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)]'
              }`}
            >
              {f.label}
              {f.value !== 'all' && (
                <span className="ml-1.5">
                  ({totalCount})
                </span>
              )}
            </button>
          );
        })}
      </div>

      {/*
      //
      // Chain Executions section.
      //
      */}
      <div>
        <button
          onClick={() => setChainsCollapsed(!chainsCollapsed)}
          className="text-sm font-medium text-muted mb-3 flex items-center gap-2 hover:text-[var(--text-primary)] transition-colors"
        >
          {chainsCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
          <GitBranch size={14} />
          Chain Executions ({filteredChainExecutions.length})
        </button>
        {!chainsCollapsed && (
          filteredChainExecutions.length === 0 ? (
            <div className="bg-card ascii-box border border-subtle p-8 text-center">
              <GitBranch size={32} className="mx-auto mb-3 text-muted opacity-50" />
              <p className="text-muted text-sm">
                {filter === 'all'
                  ? 'No chain executions have been run yet'
                  : `No ${filter.toLowerCase()} chain executions`}
              </p>
            </div>
          ) : (
            <div className="border border-subtle ascii-box overflow-x-auto">
              <table className="w-full min-w-[980px] text-xs">
                <thead>
                  <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
                    <th className="text-left px-4 py-2 text-muted tracking-wider">CHAIN</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">ID</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">AGENT</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">NODE</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">STARTED</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">STATUS</th>
                    <th className="px-4 py-2"></th>
                  </tr>
                </thead>
                <tbody>
                  {filteredChainExecutions.map((exec) => (
                    <tr
                      key={exec.execution_id}
                      className="border-b border-dim last:border-0 hover:bg-[var(--highlight)] transition-colors cursor-pointer"
                      onClick={() => setSelectedChainExecId(exec.execution_id)}
                    >
                      <td className="px-4 py-2">
                        <div className="flex items-center gap-3">
                          {exec.status === 'Running' || exec.status === 'Queued' ? (
                            <Loader2 size={14} className="animate-spin text-[var(--accent-info)]" />
                          ) : (
                            <GitBranch size={14} className="text-muted" />
                          )}
                          <span className="font-medium text-highlight">{exec.chain_name}</span>
                        </div>
                      </td>
                      <td className="px-4 py-2 text-muted font-mono">{exec.execution_id.slice(0, 8)}...</td>
                      <td className="px-4 py-2">{exec.agent_short_name}</td>
                      <td className="px-4 py-2 text-muted">
                        {exec.node_id.slice(0, 8)}...
                      </td>
                      <td className="px-4 py-2 text-muted">
                        {new Date(exec.started_at).toLocaleString()}
                      </td>
                      <td className="px-4 py-2">
                        <StatusBadge
                          status={exec.status === 'Running' || exec.status === 'Queued' ? 'info' : exec.status === 'Completed' ? 'online' : exec.status === 'Failed' ? 'offline' : 'warning'}
                          label={exec.status}
                        />
                      </td>
                      <td className="px-4 py-2">
                        <div className="flex items-center gap-2 justify-end" onClick={(e) => e.stopPropagation()}>
                          {(exec.status === 'Running' || exec.status === 'Queued') && (
                            <button
                              onClick={() => cancelChainExecution(exec.execution_id)}
                              className="p-2 hover:bg-[var(--accent-error)]/10 text-muted hover:text-[var(--accent-error)] transition-colors"
                              title="Cancel"
                            >
                              <Square size={14} />
                            </button>
                          )}
                          {exec.status !== 'Running' && exec.status !== 'Queued' && (
                            <button
                              onClick={() => removeChainExecution(exec.execution_id)}
                              className="p-2 hover:bg-[var(--accent-error)]/10 text-muted hover:text-[var(--accent-error)] transition-colors"
                              title="Remove"
                            >
                              <X size={14} />
                            </button>
                          )}
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )
        )}
      </div>

      {/*
      //
      // Semantic Operations section.
      //
      */}
      <div className="mt-6">
        <button
          onClick={() => setOpsCollapsed(!opsCollapsed)}
          className="text-sm font-medium text-muted mb-3 flex items-center gap-2 hover:text-[var(--text-primary)] transition-colors"
        >
          {opsCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
          <Zap size={14} />
          Semantic Operations ({filteredOperations.length})
        </button>
        {!opsCollapsed && (
          filteredOperations.length === 0 ? (
            <div className="bg-card ascii-box border border-subtle p-8 text-center">
              <Zap size={32} className="mx-auto mb-3 text-muted opacity-50" />
              <p className="text-muted text-sm">
                {filter === 'all'
                  ? 'No semantic operations have been run yet'
                  : `No ${filter.toLowerCase()} operations`}
              </p>
            </div>
          ) : (
            <div className="border border-subtle ascii-box overflow-x-auto">
              <table className="w-full min-w-[1080px] text-xs">
                <thead>
                  <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
                    <th className="text-left px-4 py-2 text-muted tracking-wider">OPERATION</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">ID</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">AGENT</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">NODE</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">STARTED</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">DURATION</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">STATUS</th>
                    <th className="px-4 py-2"></th>
                  </tr>
                </thead>
                <tbody>
                  {filteredOperations.map((op) => (
                    <tr
                      key={op.operation_id}
                      className="border-b border-dim last:border-0 hover:bg-[var(--highlight)] transition-colors cursor-pointer"
                      onClick={() => setSelectedOpId(op.operation_id)}
                    >
                      <td className="px-4 py-2">
                        <div className="flex items-center gap-3">
                          {op.status === 'Running' ? (
                            <Loader2 size={14} className="animate-spin text-[var(--accent-info)]" />
                          ) : (
                            <Zap size={14} className="text-muted" />
                          )}
                          <span className="font-medium text-highlight">{op.spec.name}</span>
                        </div>
                      </td>
                      <td className="px-4 py-2 text-muted font-mono">{op.operation_id.slice(0, 8)}...</td>
                      <td className="px-4 py-2">{op.agent_short_name}</td>
                      <td className="px-4 py-2 text-muted">
                        {op.node_id.slice(0, 8)}...
                      </td>
                      <td className="px-4 py-2 text-muted">
                        {new Date(op.start_time).toLocaleString()}
                      </td>
                      <td className="px-4 py-2">
                        <div className="flex items-center gap-1 text-muted">
                          <Clock size={12} />
                          {formatDuration(op.start_time, op.end_time)}
                        </div>
                      </td>
                      <td className="px-4 py-2">
                        <StatusBadge
                          status={getOperationStatusColor(op.status)}
                          label={op.status}
                        />
                      </td>
                      <td className="px-4 py-2">
                        <div className="flex items-center gap-2 justify-end" onClick={(e) => e.stopPropagation()}>
                          {op.status === 'Running' && (
                            <button
                              onClick={() => cancelOperation(op.operation_id)}
                              className="p-2  hover:bg-[var(--accent-error)]/10 text-muted hover:text-[var(--accent-error)] transition-colors"
                              title="Cancel"
                            >
                              <Square size={14} />
                            </button>
                          )}
                          {(op.status === 'Completed' ||
                            op.status === 'Failed' ||
                            op.status === 'Cancelled') && (
                            <button
                              onClick={() => removeOperation(op.operation_id)}
                              className="p-2  hover:bg-[var(--accent-error)]/10 text-muted hover:text-[var(--accent-error)] transition-colors"
                              title="Remove"
                            >
                              <X size={14} />
                            </button>
                          )}
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )
        )}
      </div>
        </>
      )}

      {/*
      //
      // Library tab content.
      //
      */}
      {mainTab === 'library' && (
        <LibraryTab nodes={state.systemState?.nodes || []} />
      )}

      {/*
      //
      // Operation detail modal.
      //
      */}
      <OperationDetailModal
        operation={selectedOp}
        onClose={() => setSelectedOpId(null)}
      />

      {/*
      //
      // Run Operation modal.
      //
      */}
      <RunModal
        isOpen={showRunModal}
        onClose={() => {
          setShowRunModal(false);
          setPreSelectedOpDef(null);
        }}
        onRun={handleRunOperation}
        title="Run Operation"
        items={definitions.filter(d => !d.disabled).map(def => ({
          id: def.full_name,
          name: def.name,
          description: def.description,
          badge: def.category,
        }))}
        nodes={state.systemState?.nodes || []}
        variant="operation"
        preSelectedItem={preSelectedOpDef ? {
          id: preSelectedOpDef.full_name,
          name: preSelectedOpDef.name,
          description: preSelectedOpDef.description,
          badge: preSelectedOpDef.category,
        } : null}
      />

      {/*
      //
      // Run Chain modal.
      //
      */}
      <RunModal
        isOpen={showRunChainModal}
        onClose={() => setShowRunChainModal(false)}
        onRun={handleRunChainFromModal}
        title="Run Chain"
        items={chains.filter(c => !c.disabled).map(chain => ({
          id: chain.id,
          name: chain.name,
          description: chain.description,
          badge: `${chain.element_count} elements`,
        }))}
        nodes={state.systemState?.nodes || []}
        variant="chain"
      />

      {/*
      //
      // Chain Execution Detail Modal.
      //
      */}
      <ChainExecutionModal
        execution={selectedChainExec}
        chain={selectedChainDef}
        isLoading={isChainLoading}
        onClose={() => setSelectedChainExecId(null)}
        onEditChain={() => {
          setSelectedChainExecId(null);
          setMainTab('library');
        }}
      />
    </div>
  );
}
