import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import { createPortal } from 'react-dom';
import {
  Server,
  Bot,
  Play,
  Square,
  Loader2,
  Shield,
  ShieldCheck,
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
  X,
  MessageSquare,
  RotateCcw,
} from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { StatusBadge, getNodeStatus } from '../common/StatusBadge';
import { RunModal, type RunItem } from '../common/RunModal';
import { Modal } from '../common/Modal';
import { ReconModal } from './ReconModal';
import { TerminalModal } from './TerminalModal';
import { AgentSessionModal } from './AgentSessionModal';
import { StyledOutput } from '../common/StyledOutput';
import type { NodeState, NodeCapability, InterceptMethod, SemanticOpUpdate } from '../../api/types';

interface NodeCardProps {
  node: NodeState;
}

function ActiveOpEntry({ op, onHoverChange }: { op: SemanticOpUpdate; onHoverChange?: (id: string, hovered: boolean) => void }) {
  const [hovered, setHovered] = useState(false);
  const triggerRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);

  const isRunning = op.status === 'Running';

  const setHoverState = useCallback((val: boolean) => {
    setHovered(val);
    onHoverChange?.(op.operation_id, val);
  }, [op.operation_id, onHoverChange]);

  const updatePos = useCallback(() => {
    if (!triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    setPos({ top: rect.top, left: rect.left });
  }, []);

  useEffect(() => {
    if (!hovered) return;
    updatePos();
    window.addEventListener('scroll', updatePos, true);
    return () => window.removeEventListener('scroll', updatePos, true);
  }, [hovered, updatePos]);

  useEffect(() => {
    if (hovered && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [hovered, op.output]);

  const statusColor = isRunning
    ? 'var(--accent-purple)'
    : op.status === 'Completed' ? 'var(--accent-success)'
    : op.status === 'Failed' ? 'var(--accent-error)'
    : 'var(--text-secondary)';

  return (
    <div
      ref={triggerRef}
      onMouseEnter={() => setHoverState(true)}
      onMouseLeave={() => setHoverState(false)}
    >
      <div className="flex items-center gap-1.5 text-[10px]">
        {isRunning && <Loader2 size={10} className="animate-spin flex-shrink-0 text-[var(--accent-purple)]" />}
        <Zap size={10} className="flex-shrink-0 text-[var(--accent-purple)]" />
        <span className="text-highlight truncate">{op.spec.name}</span>
        <span className="text-muted">· {op.agent_short_name}</span>
        {!isRunning && (
          <span className="text-[9px] tracking-wider" style={{ color: statusColor }}>{op.status.toUpperCase()}</span>
        )}
      </div>

      {hovered && pos && createPortal(
        <div
          className="fixed z-[9999] w-80 overflow-auto scrollbar-on-hover bg-[var(--bg-primary)] border border-subtle shadow-lg"
          style={{ left: pos.left, bottom: window.innerHeight - pos.top + 4, maxHeight: pos.top - 12 }}
          onMouseEnter={() => setHoverState(true)}
          onMouseLeave={() => setHoverState(false)}
        >
          <div className="px-2.5 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)]">
            <div className="flex items-center gap-1.5 text-[10px]">
              {isRunning
                ? <Loader2 size={10} className="animate-spin flex-shrink-0 text-[var(--accent-purple)]" />
                : <Zap size={10} className="flex-shrink-0 text-[var(--accent-purple)]" />}
              <span className="text-highlight font-medium">{op.spec.name}</span>
              <span className="text-muted">· {op.agent_short_name}</span>
              {!isRunning && (
                <span className="ml-auto text-[9px] tracking-wider" style={{ color: statusColor }}>{op.status.toUpperCase()}</span>
              )}
            </div>
          </div>

          {op.spec.operation_prompt && (
            <div className="px-2.5 py-1.5 border-b border-subtle">
              <div className="text-[9px] text-muted tracking-wider mb-0.5">PROMPT</div>
              <div className="text-[10px] text-[var(--text-secondary)] font-mono whitespace-pre-wrap break-words max-h-20 overflow-auto scrollbar-on-hover">
                {op.spec.operation_prompt}
              </div>
            </div>
          )}

          {op.output && (
            <div className="px-2.5 py-1.5 border-b border-subtle">
              <div className="text-[9px] text-muted tracking-wider mb-0.5">OUTPUT</div>
              <div ref={scrollRef} className="max-h-48 overflow-auto scrollbar-on-hover text-[10px]">
                <StyledOutput output={op.output} />
              </div>
            </div>
          )}

          {(op.summary || op.result) && (
            <div className="px-2.5 py-1.5 border-b border-subtle">
              <div className="text-[9px] text-muted tracking-wider mb-0.5">RESULT</div>
              <div className="text-[10px] text-[var(--text-secondary)] whitespace-pre-wrap break-words max-h-32 overflow-auto scrollbar-on-hover">
                {op.result || op.summary}
              </div>
            </div>
          )}

          <div className="px-2.5 py-1.5 flex items-center justify-end gap-3 text-[9px] text-muted font-mono">
            {op.spec.model_ref && <span>{op.spec.model_ref.toUpperCase()}</span>}
            <span>ITERATIONS: {op.spec.agent_iterations}</span>
          </div>
        </div>,
        document.body,
      )}
    </div>
  );
}

function ActivePromptEntry({ promptText, agentName }: { promptText: string | null; agentName: string }) {
  const [hovered, setHovered] = useState(false);
  const triggerRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);

  const display = promptText
    ? promptText.length > 40 ? promptText.slice(0, 40) + '…' : promptText
    : 'Prompt';

  const updatePos = useCallback(() => {
    if (!triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    setPos({ top: rect.top, left: rect.left });
  }, []);

  useEffect(() => {
    if (!hovered) return;
    updatePos();
    window.addEventListener('scroll', updatePos, true);
    return () => window.removeEventListener('scroll', updatePos, true);
  }, [hovered, updatePos]);

  return (
    <div
      ref={triggerRef}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <div className="flex items-center gap-1.5 text-[10px]">
        <Loader2 size={10} className="animate-spin text-[var(--accent-purple)] flex-shrink-0" />
        <MessageSquare size={10} className="text-[var(--accent-purple)] flex-shrink-0" />
        <span className="text-highlight truncate">{display}</span>
        <span className="text-muted flex-shrink-0">· {agentName}</span>
      </div>

      {hovered && pos && promptText && createPortal(
        <div
          className="fixed z-[9999] w-80 overflow-auto scrollbar-on-hover bg-[var(--bg-primary)] border border-subtle shadow-lg"
          style={{ left: pos.left, bottom: window.innerHeight - pos.top + 4, maxHeight: pos.top - 12 }}
          onMouseEnter={() => setHovered(true)}
          onMouseLeave={() => setHovered(false)}
        >
          <div className="px-2.5 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)]">
            <div className="flex items-center gap-1.5 text-[10px]">
              <Loader2 size={10} className="animate-spin text-[var(--accent-purple)] flex-shrink-0" />
              <MessageSquare size={10} className="text-[var(--accent-purple)] flex-shrink-0" />
              <span className="text-highlight font-medium">Session Prompt</span>
              <span className="text-muted">· {agentName}</span>
            </div>
          </div>
          <div className="px-2.5 py-1.5">
            <div className="text-[9px] text-muted tracking-wider mb-0.5">PROMPT</div>
            <div className="text-[10px] text-[var(--text-secondary)] font-mono whitespace-pre-wrap break-words max-h-40 overflow-auto scrollbar-on-hover">
              {promptText}
            </div>
          </div>
        </div>,
        document.body,
      )}
    </div>
  );
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
    removeNode,
    resetNode,
    send,
  } = useApp();

  //
  // Capability check — empty list (legacy node) means all capabilities.
  //
  const hasCapability = useCallback(
    (cap: NodeCapability) => !node.capabilities?.length || node.capabilities.includes(cap),
    [node.capabilities],
  );

  const [agentsExpanded, setAgentsExpanded] = useState(node.discovered_agents.length <= 3);
  const [collapsed, setCollapsed] = useState(false);
  const [creatingSessionFor, setCreatingSessionFor] = useState<string | null>(null);
  const [closingSessionFor, setClosingSessionFor] = useState<string | null>(null);

  //
  // Session creation with working directory picker and YOLO toggle.
  //
  const [sessionCreateAgent, setSessionCreateAgent] = useState<string | null>(null);
  const [sessionProjectPaths, setSessionProjectPaths] = useState<string[]>([]);
  const [sessionSelectedPath, setSessionSelectedPath] = useState<string | null>(null);
  const [sessionPathsLoading, setSessionPathsLoading] = useState(false);
  const [sessionYoloMode, setSessionYoloMode] = useState(false);

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
    setSessionYoloMode(false);
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

  const doCreateSession = async (shortName: string, workingDir: string | undefined, yoloMode: boolean = false) => {
    setSessionCreateAgent(null);
    setCreatingSessionFor(shortName);
    try {
      await handleSelectAgent(shortName);
      await sendCommand(node.node_id, {
        Session: { Create: { context: { yolo_mode: yoloMode, working_dir: workingDir } } },
      });
      setShowSessionModal({ agentShortName: shortName });
    } finally {
      setCreatingSessionFor(null);
    }
  };

  const handleConfirmCreateSession = () => {
    if (!sessionCreateAgent) return;
    doCreateSession(sessionCreateAgent, sessionSelectedPath ?? undefined, sessionYoloMode);
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

  const opItems: RunItem[] = state.operationDefs
    .filter(d => !d.disabled)
    .sort((a, b) => (a.category || '').localeCompare(b.category || '') || a.name.localeCompare(b.name))
    .map(d => ({
      id: d.full_name,
      name: d.name,
      description: d.description || undefined,
      badge: d.category || undefined,
    }));

  const chainItems: RunItem[] = state.chains.chains
    .filter(c => !c.disabled)
    .sort((a, b) => a.name.localeCompare(b.name))
    .map(c => ({
      id: c.id,
      name: c.name,
      description: c.description || undefined,
      badge: `${c.element_count} steps`,
    }));

  //
  // Track which op popovers are open so they stay visible after completion.
  //

  const [hoveredOpIds, setHoveredOpIds] = useState<Set<string>>(new Set());

  const handleOpHover = useCallback((id: string, hovered: boolean) => {
    setHoveredOpIds(prev => {
      const next = new Set(prev);
      if (hovered) next.add(id); else next.delete(id);
      return next;
    });
  }, []);

  const activeOps = useMemo(
    () => state.operations.filter(op =>
      op.node_id === node.node_id && (op.status === 'Running' || hoveredOpIds.has(op.operation_id)),
    ),
    [state.operations, node.node_id, hoveredOpIds],
  );

  const activeChains = useMemo(
    () => state.chains.executions.filter(ex => ex.node_id === node.node_id && ex.status === 'Running'),
    [state.chains.executions, node.node_id],
  );

  const hasActivePrompt = !!node.selected_agent?.active_transaction_id;

  const hasActiveWork = activeOps.length > 0 || activeChains.length > 0 || hasActivePrompt;

  return (
    <>
      <div className="bg-card ascii-box border border-subtle flex flex-col">
        {/*
        //
        // Card header — machine name, OS, status, delete.
        //
        */}
        <div
          className={`px-3 py-2 ${!collapsed ? 'border-b border-subtle' : ''} bg-[var(--bg-tertiary)] flex items-center justify-between group/header cursor-pointer`}
          onClick={() => setCollapsed(!collapsed)}
        >
          <div className="flex items-center gap-2 min-w-0">
            {collapsed
              ? <ChevronRight size={12} className="text-muted flex-shrink-0" />
              : <ChevronDown size={12} className="text-muted flex-shrink-0" />}
            <Server size={14} className="text-muted flex-shrink-0" />
            <span className="font-medium text-highlight text-xs truncate">{node.machine_name || 'Unknown'}</span>
            {collapsed && hasActiveWork && (() => {
              const chain = activeChains[0];
              const op = activeOps[0];
              if (chain) return (
                <span className="inline-flex items-center gap-1 text-[9px] text-[var(--accent-info)] min-w-0">
                  <Loader2 size={9} className="animate-spin flex-shrink-0" />
                  <GitBranch size={8} className="flex-shrink-0" />
                  <span className="truncate">{chain.chain_name}</span>
                </span>
              );
              if (op) return (
                <span className="inline-flex items-center gap-1 text-[9px] text-[var(--accent-purple)] min-w-0">
                  <Loader2 size={9} className="animate-spin flex-shrink-0" />
                  <Zap size={8} className="flex-shrink-0" />
                  <span className="truncate">{op.spec.name}</span>
                </span>
              );
              return (
                <span className="inline-flex items-center gap-1 text-[9px] text-[var(--accent-purple)] min-w-0">
                  <Loader2 size={9} className="animate-spin flex-shrink-0" />
                  <MessageSquare size={8} className="flex-shrink-0" />
                  <span className="truncate">{node.selected_agent?.active_prompt_text?.slice(0, 30) || 'Prompt'}</span>
                </span>
              );
            })()}
          </div>
          <div className="flex items-center gap-2" onClick={e => e.stopPropagation()}>
            <StatusBadge status={status} />
            <button
              onClick={() => resetNode(node.node_id)}
              className="p-0.5 text-muted/30 hover:text-[var(--accent-warning)] transition-colors opacity-0 group-hover/header:opacity-100"
              title="Reset node"
            >
              <RotateCcw size={11} />
            </button>
            <button
              onClick={() => removeNode(node.node_id)}
              className="p-0.5 text-muted/30 hover:text-[var(--accent-error)] transition-colors opacity-0 group-hover/header:opacity-100"
              title="Remove node"
            >
              <X size={12} />
            </button>
          </div>
        </div>

        {!collapsed && (<>
        {/*
        //
        // Node info row.
        //
        */}
        <div className="px-3 py-2 flex items-center gap-3 text-[10px] text-muted border-b border-subtle">
          {node.node_type && (
            <span className="inline-flex items-center px-1.5 py-0.5 text-[9px] tracking-wider bg-[var(--accent-info)]/15 text-[var(--accent-info)] flex-shrink-0 uppercase">
              {node.node_type}
            </span>
          )}
          <span className="truncate">{node.os_details}</span>
          {node.privileged && (
            <span className="inline-flex items-center gap-1 px-1.5 py-0.5 text-[9px] tracking-wider bg-[var(--accent-warning)]/15 text-[var(--accent-warning)] flex-shrink-0">
              <ShieldCheck size={9} /> ROOT
            </span>
          )}
          <span className="font-mono text-[9px] truncate ml-auto">{node.node_id.slice(0, 12)}...</span>
        </div>

        {/*
        //
        // Intercept status.
        //
        */}
        {node.intercept_supported && hasCapability('Interception') && (
          <div className="px-3 py-1.5 flex items-center justify-between border-b border-subtle">
            <div className="flex items-center gap-1.5 text-[10px]">
              <Shield size={11} className={node.intercept_active ? 'text-[var(--accent-warning)]' : 'text-muted'} />
              <span className={node.intercept_active ? 'text-[var(--accent-warning)]' : 'text-muted'}>
                Intercept {node.intercept_active ? 'ON' : 'OFF'}
              </span>
            </div>
            <button
              onClick={handleToggleIntercept}
              disabled={!node.intercept_active && !node.privileged}
              title={!node.intercept_active && !node.privileged ? 'Node must be running as root/admin to enable interception' : undefined}
              className={`px-2 py-0.5 text-[9px] transition-colors ${
                node.intercept_active
                  ? 'bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30'
                  : !node.privileged
                    ? 'bg-[var(--bg-secondary)] text-muted cursor-not-allowed opacity-50'
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
                        disabled={!agent.available || creatingSessionFor === agent.short_name || !hasCapability('Session')}
                        className="p-0.5 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/20 transition-colors disabled:opacity-50"
                        title={hasCapability('Session') ? 'Start session' : 'Node does not support sessions'}
                      >
                        {creatingSessionFor === agent.short_name
                          ? <Loader2 size={11} className="animate-spin" />
                          : <Play size={11} />}
                      </button>
                    )}
                    <button
                      onClick={() => setShowReconModal({ agentShortName: agent.short_name })}
                      disabled={!hasCapability('Recon')}
                      className="p-0.5 text-muted hover:text-[var(--accent-info)] hover:bg-[var(--accent-info)]/20 transition-colors disabled:opacity-50"
                      title={hasCapability('Recon') ? 'Recon' : 'Node does not support recon'}
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
        // Active operations / chain executions on this node.
        //
        */}
        {hasActiveWork && (
          <div className="px-3 py-2 border-t border-subtle space-y-1.5">
            <span className="text-[10px] text-[var(--accent-info)] tracking-wider">ACTIVE ({activeOps.length + activeChains.length + (hasActivePrompt ? 1 : 0)})</span>

            {activeOps.map(op => (
              <ActiveOpEntry key={op.operation_id} op={op} onHoverChange={handleOpHover} />
            ))}

            {activeChains.map(ex => (
              <div key={ex.execution_id} className="flex items-center gap-1.5 text-[10px]">
                <Loader2 size={10} className="animate-spin text-[var(--accent-info)] flex-shrink-0" />
                <GitBranch size={10} className="text-[var(--accent-info)] flex-shrink-0" />
                <span className="text-highlight truncate">{ex.chain_name}</span>
                <span className="text-muted">· {ex.agent_short_name}</span>
              </div>
            ))}

            {hasActivePrompt && (
              <ActivePromptEntry
                promptText={node.selected_agent!.active_prompt_text ?? null}
                agentName={node.selected_agent!.short_name}
              />
            )}
          </div>
        )}

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
            disabled={!hasCapability('Terminal')}
            className={`inline-flex items-center gap-1 px-2 py-1 text-[10px] transition-colors ${
              !hasCapability('Terminal')
                ? 'bg-[var(--bg-secondary)] text-muted cursor-not-allowed opacity-50'
                : node.active_terminal_id
                  ? 'bg-[var(--accent-success)]/10 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/20'
                  : 'bg-[var(--bg-secondary)] text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)]'
            }`}
            title={hasCapability('Terminal') ? 'Terminal' : 'Node does not support terminal'}
          >
            <TerminalIcon size={10} /> Term
          </button>
        </div>
        </>)}

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
        initialTargetSpec={{ node_ids: [node.node_id], os_filter: null, agent_short_names: [], include_triggering_node: false }}
        onRun={(itemId, spec) => {
          const targetAgents = spec.agent_short_names.length > 0
            ? node.discovered_agents.filter(a => spec.agent_short_names.includes(a.short_name))
            : node.discovered_agents;
          for (const agent of targetAgents) {
            runOperation(node.node_id, agent.short_name, itemId);
          }
        }}
      />

      <RunModal
        isOpen={showRunChainModal}
        onClose={() => setShowRunChainModal(false)}
        title="Run Chain"
        items={chainItems}
        variant="chain"
        nodes={singleNodeList}
        initialTargetSpec={{ node_ids: [node.node_id], os_filter: null, agent_short_names: [], include_triggering_node: false }}
        onRun={(itemId, spec) => {
          const agentName = spec.agent_short_names.length > 0
            ? spec.agent_short_names[0]
            : node.selected_agent?.short_name || node.discovered_agents[0]?.short_name || '';
          runChain(itemId, node.node_id, agentName, undefined, spec);
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
        title={`Start Session · ${sessionCreateAgent}`}
        size="sm"
        noPadding
      >
        <div className="space-y-0">
          {/*
          //
          // Project directory selection.
          //
          */}
          <div className="p-2 bg-[var(--bg-secondary)]">
            <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">PROJECT DIRECTORY</label>
            <div className="space-y-0.5 max-h-40 overflow-auto scrollbar-on-hover">
              {sessionProjectPaths.map(path => (
                <div
                  key={path}
                  onClick={() => setSessionSelectedPath(path)}
                  className={`flex items-center gap-2 px-2.5 py-1.5 cursor-pointer transition-colors border ${
                    sessionSelectedPath === path
                      ? 'bg-[var(--accent-info)]/10 border-[var(--accent-info)]/30'
                      : 'bg-[var(--bg-primary)] border-dim hover:border-subtle'
                  }`}
                >
                  <FolderOpen size={10} className={sessionSelectedPath === path ? 'text-[var(--accent-info)]' : 'text-muted'} />
                  <span className="font-mono text-[10px] truncate text-highlight">{path}</span>
                </div>
              ))}
              <div
                onClick={() => setSessionSelectedPath(null)}
                className={`flex items-center gap-2 px-2.5 py-1.5 cursor-pointer transition-colors border ${
                  sessionSelectedPath === null
                    ? 'bg-[var(--accent-info)]/10 border-[var(--accent-info)]/30'
                    : 'bg-[var(--bg-primary)] border-dim hover:border-subtle'
                }`}
              >
                <X size={10} className={sessionSelectedPath === null ? 'text-[var(--accent-info)]' : 'text-muted'} />
                <span className="text-[10px] text-muted italic">No working directory</span>
              </div>
            </div>
          </div>

          {/*
          //
          // YOLO mode toggle.
          //
          */}
          <div className="px-2 py-1.5 bg-[var(--bg-secondary)]">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={sessionYoloMode}
                onChange={(e) => setSessionYoloMode(e.target.checked)}
                className="accent-[var(--accent-warning)]"
              />
              <Zap size={10} className={sessionYoloMode ? 'text-[var(--accent-warning)]' : 'text-muted'} />
              <span className={`text-[10px] ${sessionYoloMode ? 'text-[var(--accent-warning)]' : 'text-[var(--text-secondary)]'}`}>
                YOLO mode
              </span>
            </label>
          </div>

          {/*
          //
          // Actions.
          //
          */}
          <div className="p-2 bg-[var(--bg-secondary)]">
            <div className="flex justify-end gap-1.5">
              <button
                onClick={() => setSessionCreateAgent(null)}
                className="px-3 py-1.5 text-[10px] tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleConfirmCreateSession}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 text-[10px] tracking-wider bg-[var(--accent-success)]/20 text-[var(--accent-success)] border border-dim hover:border-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors"
              >
                <Play size={11} /> Start
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
