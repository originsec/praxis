import { useParams, Link, useNavigate, useSearchParams } from 'react-router-dom';
import { useState, useEffect } from 'react';
import {
  Server,
  Bot,
  Terminal as TerminalIcon,
  Shield,
  ArrowLeft,
  Clock,
  Play,
  Square,
  ChevronLeft,
  ChevronRight,
  Wifi,
  Globe,
  FileText,
  Loader2,
  Zap,
  // Radar,  // Hidden - Discovery feature not ready
} from 'lucide-react';
import { useApp } from '../context/AppContext';
import { StatusBadge, getNodeStatus } from '../components/common/StatusBadge';
import { DataTable, type ColumnDef } from '../components/common/DataTable';
import { Terminal } from '../components/terminal/Terminal';
import { Modal } from '../components/common/Modal';
import type { InterceptedTrafficEntry, TrafficLogFilters, InterceptMethod, DiscoveredAgent } from '../api/types';
import {
  ScrollableTrafficTable,
  TrafficFilterBar,
  countTrafficEntries,
  type ProtocolFilter,
} from '../components/traffic/TrafficTable';
// import { NodeDiscoveryTab } from '../components/discovery';  // Hidden - feature not ready

//
// Display limit for logical entries (HTTP + WS groups).
//
const DISPLAY_LIMIT = 100;
//
// Fetch limit for raw entries (higher to ensure we get enough after grouping).
//
const FETCH_LIMIT = 10000;

type Tab = 'agents' | 'terminal' | 'intercept';  // | 'discovery' - Hidden, feature not ready

export function NodeDetailPage() {
  const { nodeId } = useParams<{ nodeId: string }>();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const { getNode, sendCommand, state, requestTrafficLog, trackNodeAccess, enableIntercept, disableIntercept } = useApp();
  const node = nodeId ? getNode(nodeId) : undefined;

  //
  // Track node access for recent nodes list.
  //
  useEffect(() => {
    if (nodeId && node) {
      trackNodeAccess(nodeId);
    }
  }, [nodeId, node, trackNodeAccess]);

  //
  // Tab from URL or default to 'agents'.
  //
  const tabParam = searchParams.get('tab');
  const activeTab: Tab = (tabParam === 'terminal' || tabParam === 'intercept') ? tabParam : 'agents';  // discovery hidden
  const setActiveTab = (tab: Tab) => {
    setSearchParams({ tab }, { replace: true });
  };

  const [isCreatingTerminal, setIsCreatingTerminal] = useState(false);

  //
  // Terminal ID from node state (persists across navigation).
  //
  const terminalId = node?.active_terminal_id ?? null;
  const [creatingSessionFor, setCreatingSessionFor] = useState<string | null>(null);
  const [closingSessionFor, setClosingSessionFor] = useState<string | null>(null);

  //
  // Close session confirmation modal state.
  //
  const [showCloseSessionModal, setShowCloseSessionModal] = useState(false);
  const [agentToCloseSession, setAgentToCloseSession] = useState<string | null>(null);

  //
  // Check for running ops/chains on the node.
  //
  const runningNodeOps = state.operations
    .filter(op => op.node_id === nodeId && op.status === 'Running');
  const runningNodeChains = state.chains.executions
    .filter(exec => exec.node_id === nodeId && exec.status === 'Running');
  const hasRunningOpsOrChains = runningNodeOps.length > 0 || runningNodeChains.length > 0;

  //
  // Intercept traffic state.
  //
  const [trafficFilters, setTrafficFilters] = useState<TrafficLogFilters>({
    node_id: nodeId ?? null,
    agent_short_name: null,
    start_time: null,
    end_time: null,
    url_pattern: null,
    direction: null,
    limit: FETCH_LIMIT,
    offset: 0,
  });
  const [showMethodSelector, setShowMethodSelector] = useState(false);

  //
  // Fetch traffic when intercept tab is active or when requestTrafficLog
  // becomes available.
  //
  useEffect(() => {
    if (activeTab === 'intercept' && nodeId) {
      const filters = { ...trafficFilters, node_id: nodeId };
      setTrafficFilters(filters);
      requestTrafficLog(filters);
    }
  }, [activeTab, nodeId, requestTrafficLog]);

  //
  // Show loading state while system state is being fetched.
  //
  if (!state.systemState) {
    return (
      <div className="space-y-6">
        <Link
          to="/nodes"
          className="inline-flex items-center gap-2 text-muted hover:text-[var(--text-primary)]"
        >
          <ArrowLeft size={18} />
          Back to Nodes
        </Link>
        <div className="bg-card ascii-box border border-subtle p-12 text-center">
          <Server size={48} className="mx-auto mb-4 text-muted animate-pulse" />
          <h2 className="text-title font-semibold text-lg mb-2">Loading...</h2>
          <p className="text-muted">Connecting to server</p>
        </div>
      </div>
    );
  }

  if (!node) {
    return (
      <div className="space-y-6">
        <Link
          to="/nodes"
          className="inline-flex items-center gap-2 text-muted hover:text-[var(--text-primary)]"
        >
          <ArrowLeft size={18} />
          Back to Nodes
        </Link>
        <div className="bg-card ascii-box border border-subtle p-12 text-center">
          <Server size={48} className="mx-auto mb-4 text-muted opacity-50" />
          <h2 className="text-title font-semibold text-lg mb-2">Node Not Found</h2>
          <p className="text-muted">The node may have been removed or disconnected</p>
        </div>
      </div>
    );
  }

  const handleSelectAgent = async (shortName: string) => {
    await sendCommand(node.node_id, { Agent: { Select: { short_name: shortName } } });
  };

  const handleCreateSession = async (shortName: string) => {
    setCreatingSessionFor(shortName);
    try {
      await handleSelectAgent(shortName);
      await sendCommand(node.node_id, { Session: { Create: { context: { yolo_mode: false } } } });
      navigate(`/nodes/${node.node_id}/agents/${shortName}`);
    } finally {
      setCreatingSessionFor(null);
    }
  };

  const doCloseSession = async (shortName: string) => {
    setClosingSessionFor(shortName);
    try {
      await handleSelectAgent(shortName);
      await sendCommand(node.node_id, { Session: 'Close' });
    } finally {
      setClosingSessionFor(null);
    }
  };

  const handleCloseSession = (shortName: string) => {
    if (hasRunningOpsOrChains) {
      setAgentToCloseSession(shortName);
      setShowCloseSessionModal(true);
    } else {
      doCloseSession(shortName);
    }
  };

  const handleCloseSessionConfirm = () => {
    if (agentToCloseSession) {
      setShowCloseSessionModal(false);
      doCloseSession(agentToCloseSession);
      setAgentToCloseSession(null);
    }
  };

  const handleCreateTerminal = async () => {
    //
    // If terminal already exists on node, just use it (state will update
    // via node information update).
    //
    if (terminalId) {
      return;
    }
    setIsCreatingTerminal(true);
    try {
      await sendCommand(node.node_id, { Terminal: 'Create' });
      //
      // Terminal ID will be set via node state update, not from response.
      //
    } finally {
      setIsCreatingTerminal(false);
    }
  };

  const handleCloseTerminal = async () => {
    await sendCommand(node.node_id, { Terminal: 'Close' });
    //
    // Terminal ID will be cleared via node state update.
    //
  };

  //
  // Platform detection for intercept method availability.
  // - Proxy: all platforms
  // - VPN: Windows and Linux
  // - Hosts: all platforms
  // - Tproxy: Linux only
  //
  const isWindowsNode = node.os_details.toLowerCase().includes('windows');
  const isLinuxNode = node.os_details.toLowerCase().includes('linux');

  const handleToggleIntercept = async () => {
    if (node.intercept_active) {
      disableIntercept(node.node_id);
    } else {
      setShowMethodSelector(true);
    }
  };

  const handleEnableWithMethod = (method: InterceptMethod) => {
    enableIntercept(node.node_id, method);
    setShowMethodSelector(false);
  };

  const agentColumns: ColumnDef<DiscoveredAgent>[] = [
    {
      key: 'name',
      header: 'Agent',
      sortable: false,
      render: (_: unknown, agent: DiscoveredAgent) => (
        <div className="flex items-center gap-3">
          <Bot size={14} className="text-muted group-hover:text-[var(--accent-info)]" />
          <span className="font-medium text-highlight group-hover:text-[var(--accent-info)]">{agent.name}</span>
        </div>
      ),
    },
    {
      key: 'short_name',
      header: 'Short Name',
      sortable: false,
      cellClassName: 'font-mono text-muted',
    },
    {
      key: 'version',
      header: 'Version',
      sortable: false,
      render: (_: unknown, agent: DiscoveredAgent) => (
        <span className="font-mono text-muted">{agent.version || '-'}</span>
      ),
    },
    {
      key: 'session',
      header: 'Session',
      sortable: false,
      render: (_: unknown, agent: DiscoveredAgent) => {
        const isSelected = node.selected_agent?.short_name === agent.short_name;
        const hasSession = isSelected && node.selected_agent?.session_id;
        return hasSession
          ? <span className="text-[var(--accent-success)]">Active</span>
          : <span className="text-muted">-</span>;
      },
    },
    {
      key: 'actions',
      header: '',
      sortable: false,
      render: (_: unknown, agent: DiscoveredAgent) => {
        const isSelected = node.selected_agent?.short_name === agent.short_name;
        const hasSession = isSelected && node.selected_agent?.session_id;
        return (
          <div className="flex items-center gap-2 justify-end" onClick={e => e.stopPropagation()}>
            {hasSession ? (
              <button
                onClick={() => handleCloseSession(agent.short_name)}
                disabled={closingSessionFor === agent.short_name}
                className="inline-flex items-center gap-2 px-3 py-1.5 bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors disabled:opacity-50"
              >
                {closingSessionFor === agent.short_name
                  ? <><Loader2 size={14} className="animate-spin" /> Closing...</>
                  : <><Square size={14} /> Close Session</>}
              </button>
            ) : (
              <button
                onClick={() => handleCreateSession(agent.short_name)}
                disabled={!agent.available || creatingSessionFor === agent.short_name}
                style={{ cursor: agent.available && creatingSessionFor !== agent.short_name ? 'pointer' : 'not-allowed' }}
                className="inline-flex items-center gap-2 px-3 py-1.5 bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors disabled:opacity-50"
              >
                {creatingSessionFor === agent.short_name
                  ? <><Loader2 size={14} className="animate-spin" /> Starting...</>
                  : <><Play size={14} /> Start Session</>}
              </button>
            )}
          </div>
        );
      },
    },
  ];

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'agents', label: 'Agents', icon: <Bot size={18} /> },
    { id: 'terminal', label: 'Terminal', icon: <TerminalIcon size={18} /> },
    ...(node.intercept_supported ? [
      { id: 'intercept' as Tab, label: 'Intercept', icon: <Shield size={18} /> },
      // { id: 'discovery' as Tab, label: 'Discovery', icon: <Radar size={18} /> },  // Hidden - feature not ready
    ] : []),
  ];

  return (
    <div className="flex flex-col h-full gap-4 md:gap-6">
      {/*
      //
      // Back link.
      //
      */}
      <Link
        to="/nodes"
        className="inline-flex items-center gap-2 text-muted hover:text-[var(--text-primary)]"
      >
        <ArrowLeft size={18} />
        Back to Nodes
      </Link>

      {/*
      //
      // Node header.
      //
      */}
      <div className="bg-card ascii-box border border-subtle p-4 md:p-6">
        <div className="flex flex-col lg:flex-row lg:items-start lg:justify-between gap-4">
          <div className="flex items-start md:items-center gap-3 md:gap-4">
            <div className="p-3  bg-[var(--bg-secondary)]">
              <Server size={28} className="text-[var(--accent-success)]" />
            </div>
            <div>
              <h1 className="text-xl md:text-2xl font-bold text-highlight">{node.machine_name || 'Unknown'}</h1>
              <p className="text-muted text-sm mt-1">{node.os_details}</p>
              <p className="text-muted text-xs font-mono mt-1">{node.node_id}</p>
            </div>
          </div>
          <div className="flex items-start sm:items-center gap-3 md:gap-4">
            <div className="text-left sm:text-right">
              <div className="flex items-center gap-2 text-sm text-muted">
                <Clock size={14} />
                Last seen: {new Date(node.last_update).toLocaleString()}
              </div>
            </div>
            <StatusBadge status={getNodeStatus(node.last_update)} />
          </div>
        </div>
      </div>

      {/*
      //
      // Tabs.
      //
      */}
      <div className="border-b border-subtle overflow-x-auto">
        <div className="flex gap-1 min-w-max">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2 px-4 py-3 text-sm font-medium border-b-2 transition-colors ${
                activeTab === tab.id
                  ? 'border-[var(--accent-info)] text-title'
                  : 'border-transparent text-muted hover:text-[var(--text-primary)]'
              }`}
            >
              {tab.icon}
              {tab.label}
            </button>
          ))}
        </div>
      </div>

      {/*
      //
      // Tab content.
      //
      */}
      {activeTab === 'agents' && (
        <div className="bg-card ascii-box border border-subtle overflow-hidden">
          <div className="overflow-x-auto">
            <DataTable
              data={node.discovered_agents}
              columns={agentColumns}
              getRowKey={a => a.short_name}
              onRowClick={async (agent) => {
                const isSelected = node.selected_agent?.short_name === agent.short_name;
                if (!isSelected) {
                  await handleSelectAgent(agent.short_name);
                }
                navigate(`/nodes/${node.node_id}/agents/${agent.short_name}`);
              }}
              rowClassName="group"
              emptyMessage={
                <div className="py-8">
                  <Bot size={48} className="mx-auto mb-4 text-muted opacity-50" />
                  <h2 className="text-title font-semibold text-lg mb-2">No Agents Discovered</h2>
                  <p className="text-muted">This node hasn&apos;t reported any available agents</p>
                </div>
              }
            />
          </div>
        </div>
      )}

      {activeTab === 'terminal' && (
        <div className="flex-1 flex flex-col bg-card ascii-box border border-subtle overflow-hidden">
          {terminalId ? (
            <div className="flex-1 flex flex-col min-h-0">
              <div className="px-4 py-2 border-b border-subtle flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 bg-[var(--bg-secondary)]">
                <span className="text-sm text-muted font-mono">Terminal: {terminalId.slice(0, 8)}...</span>
                <button
                  onClick={handleCloseTerminal}
                  className="inline-flex items-center gap-2 px-3 py-1 rounded text-sm text-[var(--accent-error)] hover:bg-[var(--accent-error)]/10 transition-colors"
                >
                  <Square size={14} /> Close
                </button>
              </div>
              <Terminal nodeId={node.node_id} terminalId={terminalId} />
            </div>
          ) : (
            <div className="p-12 text-center">
              <TerminalIcon size={48} className="mx-auto mb-4 text-muted opacity-50" />
              <h2 className="text-title font-semibold text-lg mb-2">No Terminal Session</h2>
              <p className="text-muted mb-4">Start a terminal session to execute commands on this node</p>
              <button
                onClick={handleCreateTerminal}
                disabled={isCreatingTerminal}
                style={{ cursor: isCreatingTerminal ? 'wait' : 'pointer' }}
                className="inline-flex items-center gap-2 px-4 py-2  bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors disabled:opacity-50"
              >
                <Play size={16} />
                {isCreatingTerminal ? 'Creating...' : 'Start Terminal'}
              </button>
            </div>
          )}
        </div>
      )}

      {activeTab === 'intercept' && (
        <NodeInterceptTab
          node={node}
          onToggleIntercept={handleToggleIntercept}
          trafficLog={state.intercept.trafficLog}
          trafficTotalCount={state.intercept.trafficTotalCount}
          filters={trafficFilters}
          setFilters={setTrafficFilters}
          requestTrafficLog={requestTrafficLog}
        />
      )}

      {/* Hidden - Discovery feature not ready
      {activeTab === 'discovery' && (
        <NodeDiscoveryTab node={node} />
      )}
      */}

      {/*
      //
      // Method Selector Modal.
      //
      */}
      <Modal
        isOpen={showMethodSelector}
        onClose={() => setShowMethodSelector(false)}
        title="Select Interception Method"
        size="sm"
      >
        <div className="space-y-4">
          <p className="text-sm text-muted">
            Choose how to intercept traffic on this node.
          </p>

          <div className="space-y-2">
            <button
              onClick={() => handleEnableWithMethod('Proxy')}
              className="w-full p-3 bg-[var(--bg-secondary)] hover:bg-[var(--bg-tertiary)] transition-colors text-left"
            >
              <div className="flex items-center gap-3">
                <Globe size={20} className="text-[var(--accent-info)]" />
                <div>
                  <div className="text-title text-sm font-medium">System Proxy</div>
                  <div className="text-muted text-xs mt-0.5">
                    Uses system proxy settings
                  </div>
                </div>
              </div>
            </button>
            <button
              onClick={() => isWindowsNode && handleEnableWithMethod('Vpn')}
              disabled={!isWindowsNode}
              className={`w-full p-3 bg-[var(--bg-secondary)] transition-colors text-left ${
                isWindowsNode
                  ? 'hover:bg-[var(--bg-tertiary)] cursor-pointer'
                  : 'opacity-50 cursor-not-allowed'
              }`}
            >
              <div className="flex items-center gap-3">
                <Wifi size={20} className={isWindowsNode ? 'text-[var(--accent-info)]' : 'text-muted'} />
                <div>
                  <div className={`text-sm font-medium ${isWindowsNode ? 'text-title' : 'text-muted'}`}>VPN</div>
                  <div className="text-muted text-xs mt-0.5">
                    {isWindowsNode
                      ? 'Virtual network adapter for packet-level interception'
                      : 'Windows only'}
                  </div>
                </div>
              </div>
            </button>
            <button
              onClick={() => handleEnableWithMethod('Hosts')}
              className="w-full p-3 bg-[var(--bg-secondary)] hover:bg-[var(--bg-tertiary)] transition-colors text-left"
            >
              <div className="flex items-center gap-3">
                <FileText size={20} className="text-[var(--accent-info)]" />
                <div>
                  <div className="text-title text-sm font-medium">Hosts File</div>
                  <div className="text-muted text-xs mt-0.5">
                    Redirects domains via hosts file
                  </div>
                </div>
              </div>
            </button>
            <button
              onClick={() => isLinuxNode && handleEnableWithMethod('Tproxy')}
              disabled={!isLinuxNode}
              className={`w-full p-3 bg-[var(--bg-secondary)] transition-colors text-left ${
                isLinuxNode
                  ? 'hover:bg-[var(--bg-tertiary)] cursor-pointer'
                  : 'opacity-50 cursor-not-allowed'
              }`}
            >
              <div className="flex items-center gap-3">
                <Zap size={20} className={isLinuxNode ? 'text-[var(--accent-info)]' : 'text-muted'} />
                <div>
                  <div className={`text-sm font-medium ${isLinuxNode ? 'text-title' : 'text-muted'}`}>TPROXY</div>
                  <div className="text-muted text-xs mt-0.5">
                    {isLinuxNode
                      ? 'Transparent proxy via iptables TPROXY'
                      : 'Linux only'}
                  </div>
                </div>
              </div>
            </button>
          </div>

          <div className="flex justify-end pt-2">
            <button
              onClick={() => setShowMethodSelector(false)}
              className="px-4 py-2 text-sm border border-subtle hover:bg-[var(--bg-tertiary)] transition-colors"
            >
              Cancel
            </button>
          </div>
        </div>
      </Modal>

      {/*
      //
      // Close Session Confirmation Modal.
      //
      */}
      <Modal
        isOpen={showCloseSessionModal}
        title="Close Session"
        onClose={() => {
          setShowCloseSessionModal(false);
          setAgentToCloseSession(null);
        }}
        size="sm"
      >
        <div className="space-y-4">
          <p className="text-[var(--text-secondary)]">
            There {runningNodeOps.length + runningNodeChains.length === 1 ? 'is' : 'are'}{' '}
            <span className="text-[var(--accent-error)] font-medium">
              {runningNodeOps.length > 0 && `${runningNodeOps.length} running operation${runningNodeOps.length !== 1 ? 's' : ''}`}
              {runningNodeOps.length > 0 && runningNodeChains.length > 0 && ' and '}
              {runningNodeChains.length > 0 && `${runningNodeChains.length} running chain${runningNodeChains.length !== 1 ? 's' : ''}`}
            </span>{' '}
            on this node that will likely fail if you close the session.
          </p>
          <p className="text-[var(--text-secondary)]">
            Do you want to continue?
          </p>
          <div className="flex justify-end gap-3 pt-2">
            <button
              onClick={() => {
                setShowCloseSessionModal(false);
                setAgentToCloseSession(null);
              }}
              className="px-4 py-2 text-sm text-[var(--text-secondary)] hover:text-[var(--text-primary)] transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleCloseSessionConfirm}
              className="inline-flex items-center gap-2 px-4 py-2 text-sm bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors"
            >
              <Square size={16} />
              Close Session
            </button>
          </div>
        </div>
      </Modal>
    </div>
  );
}

//
// Node Intercept Tab Component.
//
function NodeInterceptTab({
  node,
  onToggleIntercept,
  trafficLog,
  trafficTotalCount,
  filters,
  setFilters,
  requestTrafficLog,
}: {
  node: { node_id: string; intercept_active: boolean; discovered_agents: { short_name: string }[] };
  onToggleIntercept: () => void;
  trafficLog: InterceptedTrafficEntry[];
  trafficTotalCount: number;
  filters: TrafficLogFilters;
  setFilters: (filters: TrafficLogFilters) => void;
  requestTrafficLog: (filters: TrafficLogFilters) => void;
}) {
  const [protocolFilter, setProtocolFilter] = useState<ProtocolFilter>('all');
  const [searchFilter, setSearchFilter] = useState('');

  const handleRefresh = () => {
    requestTrafficLog(filters);
  };

  //
  // Handle filter changes with auto-refresh.
  //
  const handleFilterChange = (newFilters: TrafficLogFilters) => {
    setFilters(newFilters);
    requestTrafficLog(newFilters);
  };

  const handlePrevPage = () => {
    const newOffset = Math.max(0, filters.offset - filters.limit);
    const newFilters = { ...filters, offset: newOffset };
    setFilters(newFilters);
    requestTrafficLog(newFilters);
  };

  const handleNextPage = () => {
    const newOffset = filters.offset + filters.limit;
    if (newOffset < trafficTotalCount) {
      const newFilters = { ...filters, offset: newOffset };
      setFilters(newFilters);
      requestTrafficLog(newFilters);
    }
  };

  const currentPage = Math.floor(filters.offset / filters.limit) + 1;
  const totalPages = Math.ceil(trafficTotalCount / filters.limit);
  const hasPrev = filters.offset > 0;
  const hasNext = filters.offset + filters.limit < trafficTotalCount;

  return (
    <div className="flex-1 flex flex-col gap-4 min-h-0">
      {/*
      //
      // Enable/Disable Control.
      //
      */}
      <div className="bg-card ascii-box border border-subtle p-4">
        <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
          <div className="flex items-start sm:items-center gap-3 sm:gap-4">
            <div
              className={`p-3 ${
                node.intercept_active ? 'bg-[var(--accent-warning)]/20' : 'bg-[var(--bg-secondary)]'
              }`}
            >
              <Shield
                size={24}
                className={node.intercept_active ? 'text-[var(--accent-warning)]' : 'text-muted'}
              />
            </div>
            <div>
              <h2 className="text-title font-semibold">Traffic Interception</h2>
              <p className="text-muted text-xs mt-1">
                {node.intercept_active
                  ? 'Proxy is active and capturing traffic'
                  : 'Proxy is disabled'}
              </p>
            </div>
          </div>
          <button
            onClick={onToggleIntercept}
            className={`px-4 py-2 text-sm transition-colors ${
              node.intercept_active
                ? 'bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30'
                : 'bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30'
            }`}
          >
            {node.intercept_active ? 'Disable' : 'Enable'}
          </button>
        </div>
      </div>

      {/*
      //
      // Filters.
      //
      */}
      <TrafficFilterBar
        filters={filters}
        setFilters={handleFilterChange}
        protocolFilter={protocolFilter}
        setProtocolFilter={setProtocolFilter}
        searchFilter={searchFilter}
        setSearchFilter={setSearchFilter}
        onRefresh={handleRefresh}
        showAgentSelector={true}
        fixedNodeAgents={node.discovered_agents}
      />

      {/*
      //
      // Traffic Table.
      //
      */}
      <ScrollableTrafficTable
        entries={trafficLog}
        protocolFilter={protocolFilter}
        searchFilter={searchFilter}
        expandedRow={null}
        setExpandedRow={() => {}}
        showNodeColumn={false}
        displayLimit={DISPLAY_LIMIT}
        heightMode="flex"
        emptyMessage="No traffic entries for this node"
      />

      {/*
      //
      // Pagination.
      //
      */}
      {trafficTotalCount > 0 && (
        <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 text-xs">
          <div className="text-muted">
            Showing {Math.min(countTrafficEntries(trafficLog, protocolFilter, searchFilter), DISPLAY_LIMIT)} entries (of {trafficTotalCount} total)
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={handlePrevPage}
              disabled={!hasPrev}
              className="flex items-center gap-1 px-3 py-1 text-muted hover:text-title border border-subtle hover:border-[var(--border-hover)] transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <ChevronLeft size={12} />
              PREV
            </button>
            <span className="text-muted px-2">
              {currentPage} / {totalPages || 1}
            </span>
            <button
              onClick={handleNextPage}
              disabled={!hasNext}
              className="flex items-center gap-1 px-3 py-1 text-muted hover:text-title border border-subtle hover:border-[var(--border-hover)] transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              NEXT
              <ChevronRight size={12} />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
