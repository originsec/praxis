import { useState, useEffect, useMemo } from 'react';
import {
  Server,
  Bot,
  Play,
  Square,
  Loader2,
  Shield,
  Zap,
  GitBranch,
  Search,
  Terminal as TerminalIcon,
  ChevronDown,
  ChevronRight,
  Globe,
  Wifi,
  FileText,
  FolderOpen,
} from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { StatusBadge, getNodeStatus } from '../common/StatusBadge';
import { RunModal, type RunItem } from '../common/RunModal';
import { Modal } from '../common/Modal';
import { ReconModal } from './ReconModal';
import { TerminalModal } from './TerminalModal';
import { AgentSessionModal } from './AgentSessionModal';
import type { NodeState, InterceptMethod } from '../../api/types';

interface NodeCardProps {
  node: NodeState;
}

export function NodeCard({ node }: NodeCardProps) {
  const {
    state,
    sendCommand,
    runOperation,
    runChain,
    enableIntercept,
    disableIntercept,
    requestChainDefList,
    send,
  } = useApp();

  const [agentsExpanded, setAgentsExpanded] = useState(node.discovered_agents.length <= 3);
  const [creatingSessionFor, setCreatingSessionFor] = useState<string | null>(null);
  const [closingSessionFor, setClosingSessionFor] = useState<string | null>(null);

  //
  // Session creation with working directory picker.
  //
  const [sessionCreateAgent, setSessionCreateAgent] = useState<string | null>(null);
  const [sessionProjectPaths, setSessionProjectPaths] = useState<string[]>([]);
  const [sessionSelectedPath, setSessionSelectedPath] = useState<string | null>(null);
  const [sessionPathsLoading, setSessionPathsLoading] = useState(false);

  //
  // Modal state.
  //
  const [showRunOpModal, setShowRunOpModal] = useState(false);
  const [showRunChainModal, setShowRunChainModal] = useState(false);
  const [showMethodSelector, setShowMethodSelector] = useState(false);
  const [showReconModal, setShowReconModal] = useState<{ agentShortName: string } | null>(null);
  const [showTerminalModal, setShowTerminalModal] = useState(false);
  const [showSessionModal, setShowSessionModal] = useState<{ agentShortName: string } | null>(null);

  //
  // Wrap node in array for RunModal — node is pre-selected but agent is choosable.
  //
  const singleNodeList = useMemo(() => [node], [node]);

  //
  // Fetch op/chain definitions when modals open.
  //
  useEffect(() => {
    if (showRunOpModal) send({ type: 'op_def_list' });
  }, [showRunOpModal, send]);

  useEffect(() => {
    if (showRunChainModal) requestChainDefList();
  }, [showRunChainModal, requestChainDefList]);

  const handleSelectAgent = async (shortName: string) => {
    await sendCommand(node.node_id, { Agent: { Select: { short_name: shortName } } });
  };

  //
  // Initiate session creation — fetch recon for project paths first. If paths
  // are found, show the picker modal. Otherwise create immediately.
  //
  const handleInitCreateSession = (shortName: string) => {
    setSessionCreateAgent(shortName);
    setSessionProjectPaths([]);
    setSessionSelectedPath(null);
    setSessionPathsLoading(true);

    let resolved = false;
    let pollInterval: ReturnType<typeof setInterval> | null = null;
    let reconTriggered = false;

    const handleWsMessage = (event: Event) => {
      if (resolved) return;
      const message = (event as CustomEvent).detail;
      if (message.type === 'recon_get_response' &&
          message.node_id === node.node_id &&
          message.agent_short_name === shortName) {
        if (message.recon_result) {
          resolved = true;
          if (pollInterval) clearInterval(pollInterval);
          window.removeEventListener('ws-message', handleWsMessage);
          const paths: string[] = message.recon_result.project_paths || [];
          setSessionPathsLoading(false);
          if (paths.length > 0) {
            setSessionProjectPaths(paths);
            setSessionSelectedPath(paths[0]);
          } else {
            doCreateSession(shortName, undefined);
          }
        } else if (!reconTriggered) {
          reconTriggered = true;
          sendCommand(node.node_id, { Agent: 'Recon' }).catch(() => {});
          pollInterval = setInterval(() => {
            if (!resolved) {
              send({ type: 'recon_get', node_id: node.node_id, agent_short_name: shortName });
            }
          }, 1000);

          //
          // Timeout — if no recon after 5s, just create without a path.
          //
          setTimeout(() => {
            if (!resolved) {
              resolved = true;
              if (pollInterval) clearInterval(pollInterval);
              window.removeEventListener('ws-message', handleWsMessage);
              setSessionPathsLoading(false);
              doCreateSession(shortName, undefined);
            }
          }, 5000);
        }
      }
    };

    window.addEventListener('ws-message', handleWsMessage);
    send({ type: 'recon_get', node_id: node.node_id, agent_short_name: shortName });
  };

  const doCreateSession = async (shortName: string, workingDir: string | undefined) => {
    setSessionCreateAgent(null);
    setCreatingSessionFor(shortName);
    try {
      await handleSelectAgent(shortName);
      await sendCommand(node.node_id, {
        Session: { Create: { context: { yolo_mode: false, working_dir: workingDir } } },
      });
    } finally {
      setCreatingSessionFor(null);
    }
  };

  const handleConfirmCreateSession = () => {
    if (!sessionCreateAgent) return;
    doCreateSession(sessionCreateAgent, sessionSelectedPath ?? undefined);
  };

  const handleCloseSession = async (shortName: string) => {
    setClosingSessionFor(shortName);
    try {
      await handleSelectAgent(shortName);
      await sendCommand(node.node_id, { Session: 'Close' });
    } finally {
      setClosingSessionFor(null);
    }
  };

  const handleToggleIntercept = () => {
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

  const isWindowsNode = node.os_details.toLowerCase().includes('windows');
  const isLinuxNode = node.os_details.toLowerCase().includes('linux');

  const status = getNodeStatus(node.last_update);
  const agents = node.discovered_agents;
  const visibleAgents = agentsExpanded ? agents : agents.slice(0, 3);
  const hasHiddenAgents = agents.length > 3 && !agentsExpanded;

  const opItems: RunItem[] = state.operationDefs.map(d => ({
    id: d.full_name,
    name: d.name,
    description: d.description || undefined,
    badge: d.category || undefined,
  }));

  const chainItems: RunItem[] = state.chains.chains.map(c => ({
    id: c.id,
    name: c.name,
    description: c.description || undefined,
    badge: `${c.element_count} steps`,
  }));

  return (
    <>
      <div className="bg-card ascii-box border border-subtle flex flex-col">
        {/*
        //
        // Card header — machine name, OS, status.
        //
        */}
        <div className="px-3 py-2 border-b border-subtle bg-[var(--bg-tertiary)] flex items-center justify-between">
          <div className="flex items-center gap-2 min-w-0">
            <Server size={14} className="text-muted flex-shrink-0" />
            <span className="font-medium text-highlight text-sm truncate">{node.machine_name || 'Unknown'}</span>
          </div>
          <StatusBadge status={status} />
        </div>

        {/*
        //
        // Node info row.
        //
        */}
        <div className="px-3 py-2 flex items-center gap-3 text-xs text-muted border-b border-subtle">
          <span className="truncate">{node.os_details}</span>
          <span className="font-mono text-[10px] truncate ml-auto">{node.node_id.slice(0, 12)}...</span>
        </div>

        {/*
        //
        // Intercept status.
        //
        */}
        {node.intercept_supported && (
          <div className="px-3 py-1.5 flex items-center justify-between border-b border-subtle">
            <div className="flex items-center gap-1.5 text-xs">
              <Shield size={12} className={node.intercept_active ? 'text-[var(--accent-warning)]' : 'text-muted'} />
              <span className={node.intercept_active ? 'text-[var(--accent-warning)]' : 'text-muted'}>
                Intercept {node.intercept_active ? 'ON' : 'OFF'}
              </span>
            </div>
            <button
              onClick={handleToggleIntercept}
              className={`px-2 py-0.5 text-[10px] transition-colors ${
                node.intercept_active
                  ? 'bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30'
                  : 'bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30'
              }`}
            >
              {node.intercept_active ? 'Disable' : 'Enable'}
            </button>
          </div>
        )}

        {/*
        //
        // Agents list.
        //
        */}
        <div className="flex-1 px-3 py-2">
          <div className="flex items-center justify-between mb-1.5">
            <span className="text-[10px] text-muted tracking-wider">AGENTS ({agents.length})</span>
            {agents.length > 3 && (
              <button
                onClick={() => setAgentsExpanded(!agentsExpanded)}
                className="text-[10px] text-muted hover:text-[var(--text-primary)] flex items-center gap-0.5"
              >
                {agentsExpanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
                {agentsExpanded ? 'Less' : 'More'}
              </button>
            )}
          </div>

          <div className="space-y-1">
            {visibleAgents.map(agent => {
              const isSelected = node.selected_agent?.short_name === agent.short_name;
              const hasSession = isSelected && !!node.selected_agent?.session_id;

              return (
                <div
                  key={agent.short_name}
                  className="flex items-center justify-between py-1 group"
                >
                  <div className="flex items-center gap-2 min-w-0">
                    <Bot size={11} className={hasSession ? 'text-[var(--accent-success)]' : agent.available ? 'text-muted' : 'text-[var(--accent-error)]'} />
                    <span className="text-xs text-highlight truncate">{agent.short_name}</span>
                    {hasSession && <span className="text-[9px] text-[var(--accent-success)]">LIVE</span>}
                    {agent.version && <span className="text-[9px] text-muted">{agent.version}</span>}
                  </div>

                  <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                    {hasSession ? (
                      <>
                        <button
                          onClick={() => setShowSessionModal({ agentShortName: agent.short_name })}
                          className="p-0.5 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/20 transition-colors"
                          title="Open session"
                        >
                          <Bot size={11} />
                        </button>
                        <button
                          onClick={() => handleCloseSession(agent.short_name)}
                          disabled={closingSessionFor === agent.short_name}
                          className="p-0.5 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors disabled:opacity-50"
                          title="Close session"
                        >
                          {closingSessionFor === agent.short_name
                            ? <Loader2 size={11} className="animate-spin" />
                            : <Square size={11} />}
                        </button>
                      </>
                    ) : (
                      <button
                        onClick={() => handleInitCreateSession(agent.short_name)}
                        disabled={!agent.available || creatingSessionFor === agent.short_name}
                        className="p-0.5 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/20 transition-colors disabled:opacity-50"
                        title="Start session"
                      >
                        {creatingSessionFor === agent.short_name
                          ? <Loader2 size={11} className="animate-spin" />
                          : <Play size={11} />}
                      </button>
                    )}
                    <button
                      onClick={() => setShowReconModal({ agentShortName: agent.short_name })}
                      className="p-0.5 text-muted hover:text-[var(--accent-info)] hover:bg-[var(--accent-info)]/20 transition-colors"
                      title="Recon"
                    >
                      <Search size={11} />
                    </button>
                  </div>
                </div>
              );
            })}
            {hasHiddenAgents && (
              <div className="text-[10px] text-muted">+{agents.length - 3} more...</div>
            )}
          </div>
        </div>

        {/*
        //
        // Quick actions bar.
        //
        */}
        <div className="px-3 py-2 border-t border-subtle flex flex-wrap gap-1.5">
          <button
            onClick={() => setShowRunOpModal(true)}
            className="inline-flex items-center gap-1 px-2 py-1 text-[10px] bg-[var(--accent-purple)]/10 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/20 transition-colors"
            title="Run Operation"
          >
            <Zap size={10} /> Op
          </button>
          <button
            onClick={() => setShowRunChainModal(true)}
            className="inline-flex items-center gap-1 px-2 py-1 text-[10px] bg-[var(--accent-info)]/10 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/20 transition-colors"
            title="Run Chain"
          >
            <GitBranch size={10} /> Chain
          </button>
          <button
            onClick={() => setShowTerminalModal(true)}
            className="inline-flex items-center gap-1 px-2 py-1 text-[10px] bg-[var(--bg-secondary)] text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)] transition-colors"
            title="Terminal"
          >
            <TerminalIcon size={10} /> Term
          </button>
        </div>
      </div>

      {/*
      //
      // Modals.
      //
      */}
      <RunModal
        isOpen={showRunOpModal}
        onClose={() => setShowRunOpModal(false)}
        title="Run Operation"
        items={opItems}
        variant="operation"
        nodes={singleNodeList}
        onRun={(itemId, nodeId, agentName) => {
          runOperation(nodeId, agentName, itemId);
        }}
      />

      <RunModal
        isOpen={showRunChainModal}
        onClose={() => setShowRunChainModal(false)}
        title="Run Chain"
        items={chainItems}
        variant="chain"
        nodes={singleNodeList}
        onRun={(itemId, nodeId, agentName) => {
          runChain(itemId, nodeId, agentName);
        }}
      />

      {/*
      //
      // Intercept method selector modal.
      //
      */}
      <Modal
        isOpen={showMethodSelector}
        onClose={() => setShowMethodSelector(false)}
        title="Select Interception Method"
        size="sm"
      >
        <div className="space-y-3">
          <p className="text-sm text-muted">Choose how to intercept traffic on this node.</p>
          <div className="space-y-2">
            <button onClick={() => handleEnableWithMethod('Proxy')} className="w-full p-3 bg-[var(--bg-secondary)] hover:bg-[var(--bg-tertiary)] transition-colors text-left">
              <div className="flex items-center gap-3">
                <Globe size={18} className="text-[var(--accent-info)]" />
                <div>
                  <div className="text-title text-sm font-medium">System Proxy</div>
                  <div className="text-muted text-xs">Uses system proxy settings</div>
                </div>
              </div>
            </button>
            <button
              onClick={() => isWindowsNode && handleEnableWithMethod('Vpn')}
              disabled={!isWindowsNode}
              className={`w-full p-3 bg-[var(--bg-secondary)] transition-colors text-left ${isWindowsNode ? 'hover:bg-[var(--bg-tertiary)]' : 'opacity-50 cursor-not-allowed'}`}
            >
              <div className="flex items-center gap-3">
                <Wifi size={18} className={isWindowsNode ? 'text-[var(--accent-info)]' : 'text-muted'} />
                <div>
                  <div className={`text-sm font-medium ${isWindowsNode ? 'text-title' : 'text-muted'}`}>VPN</div>
                  <div className="text-muted text-xs">{isWindowsNode ? 'Virtual network adapter' : 'Windows only'}</div>
                </div>
              </div>
            </button>
            <button onClick={() => handleEnableWithMethod('Hosts')} className="w-full p-3 bg-[var(--bg-secondary)] hover:bg-[var(--bg-tertiary)] transition-colors text-left">
              <div className="flex items-center gap-3">
                <FileText size={18} className="text-[var(--accent-info)]" />
                <div>
                  <div className="text-title text-sm font-medium">Hosts File</div>
                  <div className="text-muted text-xs">Redirects domains via hosts file</div>
                </div>
              </div>
            </button>
            <button
              onClick={() => isLinuxNode && handleEnableWithMethod('Tproxy')}
              disabled={!isLinuxNode}
              className={`w-full p-3 bg-[var(--bg-secondary)] transition-colors text-left ${isLinuxNode ? 'hover:bg-[var(--bg-tertiary)]' : 'opacity-50 cursor-not-allowed'}`}
            >
              <div className="flex items-center gap-3">
                <Zap size={18} className={isLinuxNode ? 'text-[var(--accent-info)]' : 'text-muted'} />
                <div>
                  <div className={`text-sm font-medium ${isLinuxNode ? 'text-title' : 'text-muted'}`}>TPROXY</div>
                  <div className="text-muted text-xs">{isLinuxNode ? 'Transparent proxy via iptables' : 'Linux only'}</div>
                </div>
              </div>
            </button>
          </div>
        </div>
      </Modal>

      {showReconModal && (
        <ReconModal
          nodeId={node.node_id}
          agentShortName={showReconModal.agentShortName}
          onClose={() => setShowReconModal(null)}
        />
      )}

      {showTerminalModal && (
        <TerminalModal
          nodeId={node.node_id}
          node={node}
          onClose={() => setShowTerminalModal(false)}
        />
      )}

      {showSessionModal && (
        <AgentSessionModal
          nodeId={node.node_id}
          agentShortName={showSessionModal.agentShortName}
          node={node}
          onClose={() => setShowSessionModal(null)}
        />
      )}

      {/*
      //
      // Working directory picker for session creation.
      //
      */}
      <Modal
        isOpen={sessionCreateAgent !== null && sessionProjectPaths.length > 0}
        onClose={() => setSessionCreateAgent(null)}
        title={`Start Session: ${sessionCreateAgent}`}
        size="sm"
      >
        <div className="space-y-4">
          <div>
            <label className="block text-xs text-muted tracking-wider mb-2">WORKING DIRECTORY</label>
            <div className="space-y-1 max-h-48 overflow-auto">
              {sessionProjectPaths.map(path => (
                <button
                  key={path}
                  onClick={() => setSessionSelectedPath(path)}
                  className={`w-full flex items-center gap-2 px-3 py-2 text-left text-xs transition-colors ${
                    sessionSelectedPath === path
                      ? 'bg-[var(--accent-info)]/20 border border-[var(--accent-info)]'
                      : 'bg-[var(--bg-secondary)] border border-subtle hover:border-[var(--border-hover)]'
                  }`}
                >
                  <FolderOpen size={12} className={sessionSelectedPath === path ? 'text-[var(--accent-info)]' : 'text-muted'} />
                  <span className="font-mono truncate text-highlight">{path}</span>
                </button>
              ))}
            </div>
          </div>

          <div className="flex justify-between items-center pt-2">
            <button
              onClick={() => {
                if (sessionCreateAgent) doCreateSession(sessionCreateAgent, undefined);
              }}
              className="px-3 py-2 text-xs text-muted hover:text-[var(--text-primary)] transition-colors"
            >
              Skip (no directory)
            </button>
            <div className="flex gap-2">
              <button
                onClick={() => setSessionCreateAgent(null)}
                className="px-4 py-2 text-xs text-muted border border-dim hover:border-subtle transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleConfirmCreateSession}
                className="inline-flex items-center gap-2 px-4 py-2 text-xs bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors"
              >
                <Play size={12} /> Start
              </button>
            </div>
          </div>
        </div>
      </Modal>

      {/*
      //
      // Loading overlay when fetching recon for session creation.
      //
      */}
      <Modal
        isOpen={sessionCreateAgent !== null && sessionPathsLoading}
        onClose={() => setSessionCreateAgent(null)}
        title="Starting Session"
        size="sm"
      >
        <div className="flex items-center justify-center py-6 gap-3">
          <Loader2 size={16} className="animate-spin text-muted" />
          <span className="text-sm text-muted">Checking for project directories...</span>
        </div>
      </Modal>
    </>
  );
}
