import { useState, useEffect, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { Zap, X, Trash2, Clock, Square, Loader2, Play, GitBranch, ChevronDown, ChevronRight, Wifi, MonitorSmartphone } from 'lucide-react';
import { useApp } from '../context/AppContext';
import { StatusBadge, getOperationStatusColor } from '../components/common/StatusBadge';
import { DataTable, type ColumnDef, type RowAction } from '../components/common/DataTable';
import { RunModal } from '../components/common/RunModal';
import { OperationDetailModal } from '../components/common/OperationDetailModal';
import { ChainExecutionModal } from '../components/common/ChainExecutionModal';
import { LibraryTab } from '../components/library/LibraryTab';
import type { OperationDefinitionInfo, ChainDefinitionFull, ChainTriggerInfo, TriggerConfig, SemanticOpUpdate, ChainExecutionUpdate } from '../api/types';

type FilterStatus = 'all' | 'Running' | 'Completed' | 'Failed' | 'Cancelled' | 'Queued';
type MainTab = 'runs' | 'library' | 'triggers';

//
// Helpers for trigger display.
//

function triggerConfigSummary(config: TriggerConfig): string {
  switch (config.type) {
    case 'Scheduled': {
      const sched = config.schedule;
      const schedText = sched.type === 'DailyAt'
        ? `Daily at ${String(sched.hour).padStart(2, '0')}:${String(sched.minute).padStart(2, '0')}`
        : `Every ${sched.minutes}m`;
      return `${schedText}${config.recurring ? '' : ' (once)'}`;
    }
    case 'InterceptMatch':
      return `Rule #${config.rule_id}`;
    case 'NewNode':
      return 'New node';
  }
}

function targetSpecSummary(spec: import('../api/types').TargetSpec): string {
  const parts: string[] = [];
  if (spec.node_ids.length > 0) {
    parts.push(`${spec.node_ids.length} node${spec.node_ids.length > 1 ? 's' : ''}`);
  } else {
    parts.push('All nodes');
  }
  if (spec.agent_short_names.length > 0) {
    parts.push(spec.agent_short_names.join(', '));
  }
  if (spec.os_filter) {
    parts.push(`OS: ${spec.os_filter}`);
  }
  return parts.join(' / ');
}

function TriggerTypeIcon({ config }: { config: TriggerConfig }) {
  switch (config.type) {
    case 'Scheduled':
      return <Clock size={12} className="text-[var(--accent-warning)]" />;
    case 'InterceptMatch':
      return <Wifi size={12} className="text-[var(--accent-info)]" />;
    case 'NewNode':
      return <MonitorSmartphone size={12} className="text-[var(--accent-success)]" />;
  }
}

interface TriggersTabProps {
  triggers: ChainTriggerInfo[];
  chains: import('../api/types').ChainDefinitionInfo[];
  onToggleEnabled: (trigger: ChainTriggerInfo) => void;
  onDelete: (triggerId: string) => void;
}

function TriggersTab({ triggers, chains, onToggleEnabled, onDelete }: TriggersTabProps) {
  const getChainName = (chainId: string) => {
    const chain = chains.find(c => c.id === chainId);
    return chain?.name || chainId.slice(0, 8) + '...';
  };

  if (triggers.length === 0) {
    return (
      <div className="bg-card ascii-box border border-subtle p-8 text-center">
        <Zap size={32} className="mx-auto mb-3 text-muted opacity-50" />
        <p className="text-muted text-sm">No triggers configured</p>
        <p className="text-xs mt-1" style={{ color: 'var(--text-muted)' }}>
          Add triggers from the chain editor
        </p>
      </div>
    );
  }

  const triggerColumns: ColumnDef<ChainTriggerInfo>[] = [
    {
      key: 'chain_id',
      header: 'Chain',
      sortable: false,
      render: (_: unknown, t: ChainTriggerInfo) => (
        <div className="flex items-center gap-2">
          <GitBranch size={12} className="text-muted" />
          <span className="font-medium text-highlight">{getChainName(t.chain_id)}</span>
        </div>
      ),
    },
    {
      key: 'type',
      header: 'Type',
      sortable: false,
      render: (_: unknown, t: ChainTriggerInfo) => (
        <div className="flex items-center gap-1.5">
          <TriggerTypeIcon config={t.trigger_config} />
          <span>{t.trigger_config.type}</span>
        </div>
      ),
    },
    {
      key: 'config',
      header: 'Config',
      sortable: false,
      render: (_: unknown, t: ChainTriggerInfo) => (
        <span className="text-muted">{triggerConfigSummary(t.trigger_config)}</span>
      ),
    },
    {
      key: 'target',
      header: 'Target',
      sortable: false,
      render: (_: unknown, t: ChainTriggerInfo) => (
        <span className="text-muted">{targetSpecSummary(t.target_spec)}</span>
      ),
    },
    {
      key: 'enabled',
      header: 'Enabled',
      sortable: false,
      render: (_: unknown, t: ChainTriggerInfo) => (
        <button
          onClick={(e) => { e.stopPropagation(); onToggleEnabled(t); }}
          className={`px-2 py-0.5 text-[10px] tracking-wider border transition-colors ${
            t.enabled
              ? 'border-[var(--accent-success)]/40 text-[var(--accent-success)] bg-[var(--accent-success)]/10'
              : 'border-dim text-muted'
          }`}
        >
          {t.enabled ? 'ON' : 'OFF'}
        </button>
      ),
    },
    {
      key: 'last_fired_at',
      header: 'Last Fired',
      sortable: false,
      render: (_: unknown, t: ChainTriggerInfo) => (
        <span className="text-muted">{t.last_fired_at ? new Date(t.last_fired_at).toLocaleString() : '-'}</span>
      ),
    },
    {
      key: 'next_fire_at',
      header: 'Next Fire',
      sortable: false,
      render: (_: unknown, t: ChainTriggerInfo) => (
        <span className="text-muted">{t.next_fire_at ? new Date(t.next_fire_at).toLocaleString() : '-'}</span>
      ),
    },
  ];

  const triggerActions: RowAction<ChainTriggerInfo>[] = [
    {
      icon: <Trash2 size={14} />,
      label: 'Delete trigger',
      onClick: (t) => onDelete(t.id),
      hoverColor: 'var(--accent-error)',
    },
  ];

  return (
    <div className="border border-subtle ascii-box overflow-x-auto">
      <DataTable
        data={triggers}
        columns={triggerColumns}
        getRowKey={t => t.id}
        actions={triggerActions}
        pinnedActions
      />
    </div>
  );
}

export function OperationsPage() {
  const { state, send, cancelOperation, removeOperation, clearOperations, runChain, cancelChainExecution, removeChainExecution, clearChainExecutions, requestChainExecutions, requestChainDefList, requestChain, requestOperations, requestChainTriggers, updateChainTrigger, deleteChainTrigger } = useApp();
  const operations = state.operations;
  const [searchParams, setSearchParams] = useSearchParams();

  //
  // Tab from URL or default.
  //
  const tabParam = searchParams.get('tab');
  const mainTab: MainTab = tabParam === 'library' ? 'library' : tabParam === 'triggers' ? 'triggers' : 'runs';
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

  //
  // Fetch all triggers when on triggers tab.
  //
  useEffect(() => {
    if (isConnected && mainTab === 'triggers') {
      requestChainTriggers();
    }
  }, [mainTab, isConnected, requestChainTriggers]);

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

  const handleRunOperationAdvanced = (opFullName: string, targetSpec: import('../api/types').TargetSpec) => {
    const allNodes = state.systemState?.nodes || [];

    //
    // Resolve targets from spec: filter nodes, then agents per node.
    //
    const filteredNodes = targetSpec.node_ids.length > 0
      ? allNodes.filter(n => targetSpec.node_ids.includes(n.node_id))
      : targetSpec.os_filter
        ? allNodes.filter(n => n.os_details.toLowerCase().includes(targetSpec.os_filter!.toLowerCase()))
        : allNodes;

    for (const node of filteredNodes) {
      const agents = targetSpec.agent_short_names.length > 0
        ? node.discovered_agents.filter(a => targetSpec.agent_short_names.includes(a.short_name))
        : node.selected_agent
          ? [{ short_name: node.selected_agent.short_name }]
          : node.discovered_agents.slice(0, 1);

      for (const agent of agents) {
        send({
          type: 'semantic_op_run',
          node_id: node.node_id,
          agent_short_name: agent.short_name,
          operation_name: opFullName,
          working_dir: null,
        });
      }
    }
    setMainTab('runs');
  };

  const handleRunChainAdvanced = (chainId: string, targetSpec: import('../api/types').TargetSpec) => {
    const allNodes = state.systemState?.nodes || [];
    const primaryNode = allNodes[0];
    if (!primaryNode) return;
    const agentName = primaryNode.selected_agent?.short_name || primaryNode.discovered_agents?.[0]?.short_name || '';
    runChain(chainId, primaryNode.node_id, agentName, undefined, targetSpec);
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
  // Fetch chain definition once when a new execution is selected. Depend on
  // chain_id only so status updates don't re-trigger the fetch.
  //
  const selectedChainId = selectedChainExec?.chain_id ?? null;
  useEffect(() => {
    if (selectedChainId) {
      requestChain(selectedChainId);
    }
  }, [selectedChainId, requestChain]);

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

  const formatDuration = (start: string, end: string | null, status: string) => {
    if (status === 'Queued') return '—';
    const startTime = new Date(start).getTime();
    const endTime = end ? new Date(end).getTime() : Date.now();
    const diffMs = endTime - startTime;
    const diffSecs = Math.floor(diffMs / 1000);
    const mins = Math.floor(diffSecs / 60);
    const secs = diffSecs % 60;
    return mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;
  };

  //
  // Column definitions for operations and chain executions tables.
  //

  const chainExecColumns: ColumnDef<ChainExecutionUpdate>[] = [
    {
      key: 'chain_name',
      header: 'Chain',
      sortable: false,
      render: (_: unknown, exec: ChainExecutionUpdate) => (
        <div className="flex items-center gap-3">
          {exec.status === 'Running' || exec.status === 'Queued'
            ? <Loader2 size={14} className="flex-shrink-0 animate-spin text-[var(--accent-info)]" />
            : <GitBranch size={14} className="flex-shrink-0 text-muted" />}
          <span className="font-medium text-highlight truncate">{exec.chain_name}</span>
        </div>
      ),
    },
    {
      key: 'execution_id',
      header: 'ID',
      sortable: false,
      cellClassName: 'text-muted font-mono',
    },
    { key: 'agent_short_name', header: 'Agent', sortable: false },
    {
      key: 'node_id',
      header: 'Node',
      sortable: false,
      cellClassName: 'text-muted font-mono',
    },
    {
      key: 'started_at',
      header: 'Started',
      sortable: false,
      render: (_: unknown, exec: ChainExecutionUpdate) => (
        <span className="text-muted">{new Date(exec.started_at).toLocaleString()}</span>
      ),
    },
    {
      key: 'duration',
      header: 'Duration',
      sortable: false,
      render: (_: unknown, exec: ChainExecutionUpdate) => (
        <div className="flex items-center gap-1 text-muted">
          <Clock size={12} />
          {formatDuration(exec.started_at, exec.ended_at, exec.status)}
        </div>
      ),
    },
    {
      key: 'status',
      header: 'Status',
      sortable: false,
      render: (_: unknown, exec: ChainExecutionUpdate) => (
        <StatusBadge
          status={exec.status === 'Running' || exec.status === 'Queued' ? 'info' : exec.status === 'Completed' ? 'online' : exec.status === 'Failed' ? 'offline' : 'warning'}
          label={exec.status}
        />
      ),
    },
  ];

  const chainExecActions: RowAction<ChainExecutionUpdate>[] = [
    {
      icon: <Square size={14} />,
      label: 'Cancel',
      onClick: (exec) => cancelChainExecution(exec.execution_id),
      visible: (exec) => exec.status === 'Running' || exec.status === 'Queued',
      hoverColor: 'var(--accent-error)',
    },
    {
      icon: <X size={14} />,
      label: 'Remove',
      onClick: (exec) => removeChainExecution(exec.execution_id),
      visible: (exec) => exec.status !== 'Running' && exec.status !== 'Queued',
      hoverColor: 'var(--accent-error)',
    },
  ];

  const opColumns: ColumnDef<SemanticOpUpdate>[] = [
    {
      key: 'name',
      header: 'Operation',
      sortable: false,
      render: (_: unknown, op: SemanticOpUpdate) => (
        <div className="flex items-center gap-3">
          {op.status === 'Running'
            ? <Loader2 size={14} className="flex-shrink-0 animate-spin text-[var(--accent-info)]" />
            : <Zap size={14} className="flex-shrink-0 text-muted" />}
          <span className="font-medium text-highlight truncate">{op.spec.name}</span>
        </div>
      ),
    },
    {
      key: 'operation_id',
      header: 'ID',
      sortable: false,
      cellClassName: 'text-muted font-mono',
    },
    { key: 'agent_short_name', header: 'Agent', sortable: false },
    {
      key: 'node_id',
      header: 'Node',
      sortable: false,
      cellClassName: 'text-muted font-mono',
    },
    {
      key: 'start_time',
      header: 'Started',
      sortable: false,
      render: (_: unknown, op: SemanticOpUpdate) => (
        <span className="text-muted">{new Date(op.start_time).toLocaleString()}</span>
      ),
    },
    {
      key: 'duration',
      header: 'Duration',
      sortable: false,
      render: (_: unknown, op: SemanticOpUpdate) => (
        <div className="flex items-center gap-1 text-muted">
          <Clock size={12} />
          {formatDuration(op.start_time, op.end_time, op.status)}
        </div>
      ),
    },
    {
      key: 'status',
      header: 'Status',
      sortable: false,
      render: (_: unknown, op: SemanticOpUpdate) => (
        <StatusBadge status={getOperationStatusColor(op.status)} label={op.status} />
      ),
    },
  ];

  const opActions: RowAction<SemanticOpUpdate>[] = [
    {
      icon: <Square size={14} />,
      label: 'Cancel',
      onClick: (op) => cancelOperation(op.operation_id),
      visible: (op) => op.status === 'Running' || op.status === 'Queued',
      hoverColor: 'var(--accent-error)',
    },
    {
      icon: <X size={14} />,
      label: 'Remove',
      onClick: (op) => removeOperation(op.operation_id),
      visible: (op) => op.status === 'Completed' || op.status === 'Failed' || op.status === 'Cancelled',
      hoverColor: 'var(--accent-error)',
    },
  ];

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
        <button
          onClick={() => setMainTab('triggers')}
          className={`pb-3 px-1 text-sm font-medium transition-colors border-b-2 ${
            mainTab === 'triggers'
              ? 'text-title border-[var(--accent-info)]'
              : 'text-muted hover:text-[var(--text-primary)] border-transparent'
          }`}
        >
          Triggers
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
              <DataTable
                data={filteredChainExecutions}
                columns={chainExecColumns}
                getRowKey={e => e.execution_id}
                actions={chainExecActions}
                onRowClick={(exec) => setSelectedChainExecId(exec.execution_id)}
                resizable
                pinnedActions
              />
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
              <DataTable
                data={filteredOperations}
                columns={opColumns}
                getRowKey={op => op.operation_id}
                actions={opActions}
                onRowClick={(op) => setSelectedOpId(op.operation_id)}
                resizable
                pinnedActions
              />
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
      // Triggers tab content.
      //
      */}
      {mainTab === 'triggers' && (
        <TriggersTab
          triggers={state.chains.triggers}
          chains={chains}
          onToggleEnabled={(trigger) => updateChainTrigger(trigger.id, { enabled: !trigger.enabled })}
          onDelete={(triggerId) => deleteChainTrigger(triggerId)}
        />
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
        onRunAdvanced={handleRunOperationAdvanced}
        title="Run Operation"
        items={definitions.filter(d => !d.disabled).sort((a, b) => (a.category || '').localeCompare(b.category || '') || a.name.localeCompare(b.name)).map(def => ({
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
        onRunAdvanced={handleRunChainAdvanced}
        title="Run Chain"
        items={chains.filter(c => !c.disabled).sort((a, b) => a.name.localeCompare(b.name)).map(chain => ({
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
        operationDefs={definitions}
        payloads={state.payloads}
      />
    </div>
  );
}
