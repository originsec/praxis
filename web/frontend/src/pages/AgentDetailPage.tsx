import { useParams, Link, useSearchParams } from 'react-router-dom';
import { useState, useRef, useEffect, useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import {
  ArrowLeft,
  Bot,
  Send,
  Wrench,
  Settings,
  Cpu,
  Square,
  Loader2,
  Search,
  Play,
  RefreshCw,
  Target,
  Zap,
  Clock,
  Sparkles,
  ToggleLeft,
  ToggleRight,
  Shield,
  FolderOpen,
  Pencil,
  Save,
  X,
  User,
  Key,
  GitBranch,
  Download,
  ChevronRight,
  ChevronDown,
  FileText,
  History,
} from 'lucide-react';
import { useApp, type AgentSessionMessage } from '../context/AppContext';
import { generateUUID } from '../utils/uuid';
import { Modal } from '../components/common/Modal';
import { RunModal } from '../components/common/RunModal';
import { OperationDetailModal } from '../components/common/OperationDetailModal';
import { ChainExecutionModal } from '../components/common/ChainExecutionModal';
import { Tooltip } from '../components/common/Tooltip';
import type { SemanticOpUpdate, ReconResult, TrafficLogFilters, SessionContext, ChainDefinitionFull } from '../api/types';
import { StatusBadge, getOperationStatusColor } from '../components/common/StatusBadge';
import {
  ScrollableTrafficTable,
  TrafficFilterBar,
  type ProtocolFilter,
} from '../components/traffic/TrafficTable';
import { exportAgentSession, downloadTextFile } from '../utils/export';

type Tab = 'session' | 'ops' | 'recon' | 'intercept';
type ToolsSubTab = 'mcp' | 'skills' | 'internal';
type ReconSubTab = 'tools' | 'config' | 'sessions';

export function AgentDetailPage() {
  const { nodeId, agentShortName } = useParams<{ nodeId: string; agentShortName: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const { getNode, sendCommand, state, send, runOperation, runChain, addAgentSessionMessage, clearAgentSessionMessages, requestChainDefList, requestChain, trackNodeAccess } = useApp();
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
  // Get selected agent if it matches, otherwise find from discovered agents.
  //
  const selectedAgent = (node && node.selected_agent?.short_name === agentShortName) ? node.selected_agent : null;
  const discoveredAgent = node?.discovered_agents.find(a => a.short_name === agentShortName);
  const hasSession = !!selectedAgent?.session_id;

  //
  // Debug: Log session detection.
  //
  console.log('[AgentDetailPage] nodeId:', nodeId, 'agentShortName:', agentShortName);
  console.log('[AgentDetailPage] node?.selected_agent:', node?.selected_agent);
  console.log('[AgentDetailPage] selectedAgent:', selectedAgent, 'hasSession:', hasSession);

  //
  // Tab from URL or default based on session status.
  //
  const tabParam = searchParams.get('tab');
  const validTabs: Tab[] = ['session', 'ops', 'recon', 'intercept'];
  const defaultTab: Tab = hasSession ? 'session' : 'recon';
  const activeTab: Tab = (tabParam && validTabs.includes(tabParam as Tab)) ? (tabParam as Tab) : defaultTab;

  const updateSearchParams = (updates: Record<string, string>) => {
    const newParams: Record<string, string> = {};
    //
    // Preserve relevant params.
    //
    const tab = updates.tab ?? searchParams.get('tab') ?? '';
    if (tab) newParams.tab = tab;
    if (updates.sub !== undefined) {
      if (updates.sub) newParams.sub = updates.sub;
    } else {
      const sub = searchParams.get('sub');
      if (sub) newParams.sub = sub;
    }
    setSearchParams(newParams, { replace: true });
  };

  const setActiveTab = (tab: Tab) => {
    updateSearchParams({ tab, sub: '' });
  };

  //
  // Sub-tab from URL.
  //
  const subParam = searchParams.get('sub');
  const toolsSubTab: ToolsSubTab = (subParam === 'skills' || subParam === 'internal') ? subParam : 'mcp';
  const setToolsSubTab = (sub: ToolsSubTab) => {
    updateSearchParams({ sub });
  };

  //
  // reconSubTab: 'sessions' if sub is sessions, 'tools' if sub is any tools-related value, otherwise 'config'.
  //
  const reconSubTab: ReconSubTab =
    (subParam === 'sessions') ? 'sessions' :
    (subParam === 'tools' || subParam === 'mcp' || subParam === 'skills' || subParam === 'internal') ? 'tools' : 'config';
  const setReconSubTab = (sub: ReconSubTab) => {
    updateSearchParams({ sub });
  };

  //
  // Get messages from context keyed by session_id.
  //
  const sessionId = selectedAgent?.session_id;
  const messages: AgentSessionMessage[] = sessionId ? (state.agentSessionMessages[sessionId] || []) : [];

  const [input, setInput] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [isCreatingSession, setIsCreatingSession] = useState(false);
  const [isClosingSession, setIsClosingSession] = useState(false);
  const [isDiscoveringTools, setIsDiscoveringTools] = useState(false);
  const [selectedServerIdx, setSelectedServerIdx] = useState<number | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messageInputRef = useRef<HTMLInputElement>(null);
  const previousMessageCountRef = useRef(0);

  //
  // Recon state.
  //
  const [reconResult, setReconResult] = useState<ReconResult | null>(null);
  const [reconPerformedAt, setReconPerformedAt] = useState<string | null>(null);
  const [reconIsSemantic, setReconIsSemantic] = useState<boolean | null>(null);
  const [isLoadingRecon, setIsLoadingRecon] = useState(false);

  //
  // Run operation modal state.
  //
  const [showRunOpModal, setShowRunOpModal] = useState(false);

  //
  // Run chain modal state.
  //
  const [showRunChainModal, setShowRunChainModal] = useState(false);

  //
  // Close session confirmation modal state.
  //
  const [showCloseSessionModal, setShowCloseSessionModal] = useState(false);

  //
  // Chain execution detail modal state.
  //
  const [selectedChainExecId, setSelectedChainExecId] = useState<string | null>(null);

  //
  // Selected config file in config view (for split-view).
  //
  const [selectedConfigIdx, setSelectedConfigIdx] = useState<number | null>(null);

  //
  // Selected session in sessions view.
  //
  const [selectedSessionIdx, setSelectedSessionIdx] = useState<number | null>(null);

  //
  // Session content fetching state.
  //
  const [sessionContent, setSessionContent] = useState<string | null>(null);
  const [isLoadingSessionContent, setIsLoadingSessionContent] = useState(false);
  const [sessionContentError, setSessionContentError] = useState<string | null>(null);

  //
  // Config content fetching state.
  //
  const [configContent, setConfigContent] = useState<string | null>(null);
  const [isLoadingConfigContent, setIsLoadingConfigContent] = useState(false);
  const [configContentError, setConfigContentError] = useState<string | null>(null);

  //
  // Config file editing state.
  //
  const [editingConfigIdx, setEditingConfigIdx] = useState<number | null>(null);
  const [editingConfigContent, setEditingConfigContent] = useState<string>('');
  const [isSavingConfig, setIsSavingConfig] = useState(false);
  const [configSaveError, setConfigSaveError] = useState<string | null>(null);

  //
  // Expanded config directory groups.
  //
  const [expandedConfigDirs, setExpandedConfigDirs] = useState<Set<string>>(new Set());

  //
  // Metadata section (Identities/API Keys) collapsed state.
  //
  const [metadataCollapsed, setMetadataCollapsed] = useState(false);

  //
  // Expanded MCP server context groups.
  //
  const [expandedMcpContexts, setExpandedMcpContexts] = useState<Set<string>>(new Set(['Global']));

  //
  // Selected operation for detail modal.
  //
  const [selectedOp, setSelectedOp] = useState<SemanticOpUpdate | null>(null);

  //
  // Local YOLO mode state (used when creating sessions).
  //
  const [localYoloMode, setLocalYoloMode] = useState(selectedAgent?.yolo_mode ?? false);

  //
  // Project path selection for session context.
  //
  const [projectPaths, setProjectPaths] = useState<string[]>([]);
  const [selectedProjectPath, setSelectedProjectPath] = useState<string | null>(null);

  //
  // Warning modal for toggling YOLO with active session.
  //
  const [showYoloWarning, setShowYoloWarning] = useState(false);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [messages]);

  //
  // Auto-focus message input when session becomes active on the session tab.
  //
  useEffect(() => {
    if (hasSession && activeTab === 'session') {
      //
      // Small delay to ensure the input is rendered.
      //
      setTimeout(() => {
        messageInputRef.current?.focus();
      }, 100);
    }
  }, [hasSession, activeTab]);

  //
  // When a new assistant message arrives, return focus to the input.
  //
  useEffect(() => {
    if (!hasSession || activeTab !== 'session') {
      previousMessageCountRef.current = messages.length;
      return;
    }

    if (messages.length > previousMessageCountRef.current) {
      const lastMessage = messages[messages.length - 1];
      if (lastMessage?.role === 'assistant') {
        setTimeout(() => messageInputRef.current?.focus(), 0);
      }
    }

    previousMessageCountRef.current = messages.length;
  }, [messages, hasSession, activeTab]);

  //
  // Fetch operation definitions when modal opens.
  //
  useEffect(() => {
    if (showRunOpModal) {
      send({ type: 'op_def_list' });
    }
  }, [showRunOpModal, send]);

  //
  // Fetch chain definitions when modal opens.
  //
  useEffect(() => {
    if (showRunChainModal) {
      requestChainDefList();
    }
  }, [showRunChainModal, requestChainDefList]);

  //
  // Fetch recon data from service when viewing an agent page.
  // If no stored recon exists, trigger node recon and poll service until data arrives.
  // Note: This must be before early returns to follow Rules of Hooks.
  //
  useEffect(() => {
    const agent = discoveredAgent || selectedAgent;
    if (!agent || !nodeId || !agentShortName) return;

    let isCancelled = false;
    let pollInterval: ReturnType<typeof setInterval> | null = null;
    let reconTriggered = false;

    const requestRecon = () => {
      send({ type: 'recon_get', node_id: nodeId, agent_short_name: agentShortName });
    };

    //
    // Listen for service responses. The listener removes itself once data is
    // loaded to avoid interfering with manual refresh (handleRecon).
    //
    const handleWsMessage = (event: Event) => {
      if (isCancelled) return;
      const customEvent = event as CustomEvent;
      const message = customEvent.detail;
      if (message.type === 'recon_get_response' &&
          message.node_id === nodeId &&
          message.agent_short_name === agentShortName) {
        if (message.recon_result) {
          //
          // Found stored recon - use it, stop polling, remove listener.
          //
          setReconResult(message.recon_result);
          setReconPerformedAt(message.performed_at);
          setReconIsSemantic(message.is_semantic);
          setIsLoadingRecon(false);
          if (pollInterval) {
            clearInterval(pollInterval);
            pollInterval = null;
          }
          window.removeEventListener('ws-message', handleWsMessage);
        } else if (!reconTriggered) {
          //
          // No stored recon - trigger node to perform recon, then poll.
          //
          reconTriggered = true;
          sendCommand(nodeId, { Agent: 'Recon' }).catch(() => {});
          pollInterval = setInterval(() => {
            if (!isCancelled) {
              requestRecon();
            }
          }, 1000);
        }
      }
    };

    //
    // Initial request.
    //
    setIsLoadingRecon(true);
    window.addEventListener('ws-message', handleWsMessage);
    requestRecon();

    return () => {
      isCancelled = true;
      window.removeEventListener('ws-message', handleWsMessage);
      if (pollInterval) {
        clearInterval(pollInterval);
      }
    };
  }, [discoveredAgent?.short_name, selectedAgent?.short_name, nodeId, agentShortName, send, sendCommand]);

  //
  // Extract project paths from recon result (recon now includes sessions and
  // project_paths). Auto-select first path if none selected.
  //
  useEffect(() => {
    if (reconResult && !hasSession) {
      const paths = reconResult.project_paths || [];
      setProjectPaths(paths);
      if (paths.length > 0 && selectedProjectPath === null) {
        setSelectedProjectPath(paths[0]);
      }
    }
  }, [reconResult, hasSession, selectedProjectPath]);

  //
  // Fetch session content when a session is selected.
  // Note: This must be before early returns to follow Rules of Hooks.
  //
  useEffect(() => {
    if (selectedSessionIdx === null || !reconResult?.sessions) {
      setSessionContent(null);
      setSessionContentError(null);
      return;
    }
    const session = reconResult.sessions[selectedSessionIdx];
    if (!session?.session_file || !nodeId) return;

    //
    // Fetch session content from node.
    //

    let isCancelled = false;
    setIsLoadingSessionContent(true);
    setSessionContentError(null);
    setSessionContent(null);

    sendCommand(nodeId, {
      Agent: { ReadFile: { file_type: 'Session', path: session.session_file } },
    }).then(response => {
      if (isCancelled) return;
      if (
        'Agent' in response.result &&
        typeof response.result.Agent === 'object' &&
        response.result.Agent !== null &&
        'ReadFileResult' in response.result.Agent
      ) {
        const result = response.result.Agent.ReadFileResult;
        if (result.content) {
          setSessionContent(result.content);
        } else if (result.error) {
          setSessionContentError(result.error);
        }
      } else if ('Error' in response.result) {
        setSessionContentError((response.result as { Error: { message: string } }).Error.message);
      }
    }).catch(error => {
      if (!isCancelled) {
        setSessionContentError(String(error));
      }
    }).finally(() => {
      if (!isCancelled) {
        setIsLoadingSessionContent(false);
      }
    });

    return () => {
      isCancelled = true;
    };
  }, [selectedSessionIdx, reconResult?.sessions, nodeId, sendCommand]);

  //
  // Fetch config content when selected config changes.
  //
  useEffect(() => {
    if (selectedConfigIdx === null || !reconResult?.config) {
      setConfigContent(null);
      setConfigContentError(null);
      return;
    }
    const configItem = reconResult.config[selectedConfigIdx];
    if (!configItem?.path || !nodeId) return;

    //
    // If content is already loaded in the recon result, use it.
    //
    if (configItem.contents) {
      setConfigContent(configItem.contents);
      setConfigContentError(null);
      setIsLoadingConfigContent(false);
      return;
    }

    //
    // Fetch content from node.
    //
    let isCancelled = false;
    setIsLoadingConfigContent(true);
    setConfigContent(null);

    sendCommand(nodeId, {
      Agent: { ReadFile: { file_type: 'Config', path: configItem.path } },
    }).then(response => {
      if (isCancelled) return;
      if (
        'Agent' in response.result &&
        typeof response.result.Agent === 'object' &&
        response.result.Agent !== null &&
        'ReadFileResult' in response.result.Agent
      ) {
        const result = response.result.Agent.ReadFileResult;
        if (result.content) {
          setConfigContent(result.content);
          //
          // Update the recon result with the loaded content so we don't re-fetch.
          //
          const updatedConfig = [...reconResult.config];
          updatedConfig[selectedConfigIdx] = { ...configItem, contents: result.content };
          setReconResult({ ...reconResult, config: updatedConfig });
        } else if (result.error) {
          setConfigContentError(result.error);
        }
      } else if ('Error' in response.result) {
        setConfigContentError(response.result.Error.message);
      }
    }).catch(error => {
      if (!isCancelled) {
        setConfigContentError(String(error));
      }
    }).finally(() => {
      if (!isCancelled) {
        setIsLoadingConfigContent(false);
      }
    });

    return () => {
      isCancelled = true;
    };
  }, [selectedConfigIdx, reconResult?.config, nodeId, sendCommand]);

  //
  // Get available (non-disabled) operation definitions.
  //
  const operationDefs = state.operationDefs.filter(def => !def.disabled);

  //
  // Get available (non-disabled) chain definitions.
  //
  const chainDefs = state.chains.chains.filter(chain => !chain.disabled);

  //
  // Use selected agent info if available, otherwise use discovered agent.
  //
  const agentName = selectedAgent?.short_name ?? discoveredAgent?.short_name ?? agentShortName;

  //
  // Filter operations for this agent (must be before early returns for hooks
  // consistency).
  //
  const agentOps = state.operations
    .filter(op => op.agent_short_name === agentShortName && op.node_id === nodeId);

  //
  // Filter chain executions for this agent.
  //
  const agentChainExecs = state.chains.executions
    .filter(exec => exec.agent_short_name === agentShortName && exec.node_id === nodeId);

  //
  // Check for running ops/chains on the entire node (for close session warning).
  //
  const runningNodeOps = state.operations
    .filter(op => op.node_id === nodeId && op.status === 'Running');
  const runningNodeChains = state.chains.executions
    .filter(exec => exec.node_id === nodeId && exec.status === 'Running');
  const hasRunningOpsOrChains = runningNodeOps.length > 0 || runningNodeChains.length > 0;

  //
  // Combined and sorted list of ops and chain executions by start time desc.
  //
  const sortedOpsAndChains = useMemo(() => {
    const ops = agentOps.map(op => ({
      type: 'op' as const,
      id: op.operation_id,
      startTime: new Date(op.start_time).getTime(),
      data: op,
    }));
    const chains = agentChainExecs.map(exec => ({
      type: 'chain' as const,
      id: exec.execution_id,
      startTime: new Date(exec.started_at).getTime(),
      data: exec,
    }));
    return [...ops, ...chains].sort((a, b) => b.startTime - a.startTime);
  }, [agentOps, agentChainExecs]);

  //
  // Derive selected chain execution from current state (so it updates live).
  //
  const selectedChainExec = useMemo(() => {
    if (!selectedChainExecId) return null;
    return agentChainExecs.find(e => e.execution_id === selectedChainExecId) ?? null;
  }, [selectedChainExecId, agentChainExecs]);

  //
  // Fetch chain definition when execution is selected.
  //
  useEffect(() => {
    if (selectedChainExec) {
      requestChain(selectedChainExec.chain_id);
    }
  }, [selectedChainExec, requestChain]);

  //
  // Use cached chain definition or current chain from state.
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

  //
  // Show loading state while system state is being fetched.
  //
  if (!state.systemState) {
    return (
      <div className="space-y-6">
        <Link
          to={nodeId ? `/nodes/${nodeId}` : '/nodes'}
          className="inline-flex items-center gap-2 text-muted hover:text-[var(--text-primary)]"
        >
          <ArrowLeft size={18} />
          Back to Node
        </Link>
        <div className="bg-card ascii-box border border-subtle p-12 text-center">
          <Loader2 size={48} className="mx-auto mb-4 text-muted animate-spin" />
          <h2 className="text-title font-semibold text-lg mb-2">Loading...</h2>
          <p className="text-muted">Connecting to server</p>
        </div>
      </div>
    );
  }

  if (!node || (!selectedAgent && !discoveredAgent)) {
    return (
      <div className="space-y-6">
        <Link
          to={nodeId ? `/nodes/${nodeId}` : '/nodes'}
          className="inline-flex items-center gap-2 text-muted hover:text-[var(--text-primary)]"
        >
          <ArrowLeft size={18} />
          Back to Node
        </Link>
        <div className="bg-card ascii-box border border-subtle p-12 text-center">
          <Bot size={48} className="mx-auto mb-4 text-muted opacity-50" />
          <h2 className="text-title font-semibold text-lg mb-2">Agent Not Found</h2>
          <p className="text-muted">The agent may not be available on this node</p>
        </div>
      </div>
    );
  }

  const handleRunOpFromModal = (opFullName: string, _nodeId: string, _agentName: string) => {
    if (!nodeId || !agentShortName) return;
    const workingDir = selectedAgent?.working_dir ?? selectedProjectPath ?? undefined;
    runOperation(nodeId, agentShortName, opFullName, workingDir);
    setActiveTab('ops');
  };

  const handleRunChainFromModal = (chainId: string, _nodeId: string, _agentName: string) => {
    if (!nodeId || !agentShortName) return;
    const workingDir = selectedAgent?.working_dir ?? selectedProjectPath ?? undefined;
    runChain(chainId, nodeId, agentShortName, workingDir);
    setActiveTab('ops');
  };

  const handleRecon = (semantic: boolean): Promise<void> => {
    return new Promise((resolve) => {
      if (!nodeId || !agentShortName) {
        resolve();
        return;
      }

      setIsLoadingRecon(true);

      //
      // Capture the current performed_at - we'll wait until it changes.
      // This avoids clock sync issues since we compare service times against service times.
      //
      const previousPerformedAt = reconPerformedAt;

      //
      // Trigger recon on the intended agent, not just whichever agent is
      // currently selected on the node.
      //
      const command = semantic ? 'ReconSemantic' : 'Recon';

      //
      // Poll service until performed_at changes (meaning new data arrived).
      //
      const pollInterval = setInterval(() => {
        send({ type: 'recon_get', node_id: nodeId, agent_short_name: agentShortName });
      }, 1000);

      //
      // Timeout after 60 seconds to prevent infinite polling.
      //
      const timeout = setTimeout(() => {
        clearInterval(pollInterval);
        window.removeEventListener('ws-message', handleWsMessage);
        setIsLoadingRecon(false);
        resolve();
      }, 60000);

      const handleWsMessage = (event: Event) => {
        const customEvent = event as CustomEvent;
        const message = customEvent.detail;
        if (message.type === 'recon_get_response' &&
            message.node_id === nodeId &&
            message.agent_short_name === agentShortName) {
          if (message.recon_result && message.performed_at) {
            //
            // Only accept if performed_at has changed from what we had before.
            //
            if (message.performed_at !== previousPerformedAt) {
              setReconResult(message.recon_result);
              setReconPerformedAt(message.performed_at);
              setReconIsSemantic(message.is_semantic);
              setIsLoadingRecon(false);
              clearInterval(pollInterval);
              clearTimeout(timeout);
              window.removeEventListener('ws-message', handleWsMessage);
              resolve();
            }
          }
        }
      };

      window.addEventListener('ws-message', handleWsMessage);

      //
      // Ensure the target agent is selected before issuing recon.
      //
      (async () => {
        try {
          const selectResp = await sendCommand(nodeId, {
            Agent: { Select: { short_name: agentShortName } },
          });
          if ('Error' in selectResp.result) {
            throw new Error(selectResp.result.Error.message);
          }

          const reconResp = await sendCommand(nodeId, { Agent: command });
          if ('Error' in reconResp.result) {
            throw new Error(reconResp.result.Error.message);
          }
        } catch {
          clearInterval(pollInterval);
          clearTimeout(timeout);
          window.removeEventListener('ws-message', handleWsMessage);
          setIsLoadingRecon(false);
          resolve();
        }
      })();

      //
      // Initial poll request (in case the node is very fast).
      //
      setTimeout(() => {
        send({ type: 'recon_get', node_id: nodeId, agent_short_name: agentShortName });
      }, 500);
    });
  };

  const handleSaveConfig = async () => {
    if (editingConfigIdx === null || !reconResult || !nodeId) return;
    const item = reconResult.config[editingConfigIdx];
    setIsSavingConfig(true);
    setConfigSaveError(null);

    try {
      const response = await sendCommand(nodeId, {
        Agent: { WriteFile: { file_type: 'Config', path: item.path, contents: editingConfigContent } },
      });

      if (
        'Agent' in response.result &&
        typeof response.result.Agent === 'object' &&
        response.result.Agent !== null &&
        'WriteFileResult' in response.result.Agent
      ) {
        const result = response.result.Agent.WriteFileResult;
        if (result.success) {
          //
          // Update local state with new content.
          //
          const updatedItems = [...reconResult.config];
          updatedItems[editingConfigIdx] = { ...item, contents: editingConfigContent };
          setReconResult({ ...reconResult, config: updatedItems });
          setEditingConfigIdx(null);
          setEditingConfigContent('');
        } else {
          setConfigSaveError(result.error || 'Failed to save config file');
        }
      } else if ('Error' in response.result) {
        setConfigSaveError(response.result.Error.message);
      }
    } catch (error) {
      setConfigSaveError(String(error));
    } finally {
      setIsSavingConfig(false);
    }
  };

  const handleCancelConfigEdit = () => {
    setEditingConfigIdx(null);
    setEditingConfigContent('');
    setConfigSaveError(null);
  };

  const handleStartConfigEdit = (idx: number, content: string) => {
    setEditingConfigIdx(idx);
    setEditingConfigContent(content);
    setConfigSaveError(null);
    //
    // Ensure it's selected.
    //
    setSelectedConfigIdx(idx);
  };

  const handleSendMessage = async () => {
    if (!input.trim() || isLoading || !sessionId) return;

    const userMessage: AgentSessionMessage = {
      role: 'user',
      content: input.trim(),
      timestamp: new Date(),
    };
    addAgentSessionMessage(sessionId, userMessage);
    setInput('');
    setIsLoading(true);

    try {
      const transactionId = generateUUID();
      const response = await sendCommand(nodeId!, {
        Session: { Prompt: { text: input.trim(), transaction_id: transactionId } },
      });

      if ('Session' in response.result && typeof response.result.Session === 'object' && response.result.Session !== null && 'PromptResponse' in response.result.Session) {
        const assistantMessage: AgentSessionMessage = {
          role: 'assistant',
          content: response.result.Session.PromptResponse.response,
          timestamp: new Date(),
        };
        addAgentSessionMessage(sessionId, assistantMessage);
      } else if ('Error' in response.result) {
        const errorMessage: AgentSessionMessage = {
          role: 'assistant',
          content: `Error: ${response.result.Error.message}`,
          timestamp: new Date(),
        };
        addAgentSessionMessage(sessionId, errorMessage);
      }
    } catch (error) {
      console.error('Failed to send message:', error);
    } finally {
      setIsLoading(false);
      setTimeout(() => messageInputRef.current?.focus(), 0);
    }
  };

  const doCloseSession = async () => {
    const currentSessionId = sessionId;
    setIsClosingSession(true);
    try {
      await sendCommand(nodeId!, { Session: 'Close' });
      //
      // Clear messages for this session.
      //
      if (currentSessionId) {
        clearAgentSessionMessages(currentSessionId);
      }
    } finally {
      setIsClosingSession(false);
    }
  };

  const handleCloseSession = () => {
    if (hasRunningOpsOrChains) {
      setShowCloseSessionModal(true);
    } else {
      doCloseSession();
    }
  };

  const handleCloseSessionConfirm = () => {
    setShowCloseSessionModal(false);
    doCloseSession();
  };

  const handleCreateSession = async () => {
    setIsCreatingSession(true);
    try {
      await sendCommand(nodeId!, { Agent: { Select: { short_name: agentShortName! } } });
      const context: SessionContext = {
        yolo_mode: localYoloMode,
        working_dir: selectedProjectPath ?? undefined,
      };
      await sendCommand(nodeId!, { Session: { Create: { context } } });
      setActiveTab('session');
    } finally {
      setIsCreatingSession(false);
    }
  };

  const handleDiscoverTools = async () => {
    setIsDiscoveringTools(true);
    try {
      //
      // Semantic discovery for internal tools.
      //
      await handleRecon(true);
    } finally {
      setIsDiscoveringTools(false);
    }
  };

  const handleToggleYolo = () => {
    if (hasSession) {
      //
      // Show warning that changes won't take effect until next session.
      //
      setShowYoloWarning(true);
    } else {
      //
      // No session, toggle immediately.
      //
      setLocalYoloMode(!localYoloMode);
    }
  };

  const handleConfirmYoloToggle = () => {
    setLocalYoloMode(!localYoloMode);
    setShowYoloWarning(false);
  };

  const handleExportSession = () => {
    if (messages.length === 0 || !agentName || !node?.machine_name) return;
    const content = exportAgentSession(messages, agentName, node.machine_name);
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    downloadTextFile(content, `agent-session-${agentName}-${timestamp}.md`);
  };

  const formatOpDuration = (start: string, end: string | null) => {
    const startTime = new Date(start).getTime();
    const endTime = end ? new Date(end).getTime() : Date.now();
    const diffMs = endTime - startTime;
    const diffSecs = Math.floor(diffMs / 1000);
    const mins = Math.floor(diffSecs / 60);
    const secs = diffSecs % 60;
    return mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;
  };

  //
  // Filter traffic log for this agent.
  //
  const agentTraffic = state.intercept.trafficLog.filter(
    entry => entry.node_id === nodeId && entry.agent_short_name === agentShortName
  );

  const runningOpsCount = agentOps.filter(op => op.status === 'Running' || op.status === 'Queued').length;
  const runningChainsCount = agentChainExecs.filter(exec => exec.status === 'Running' || exec.status === 'Queued').length;
  const runningCount = runningOpsCount + runningChainsCount;

  const tabs: { id: Tab; label: string; icon: React.ReactNode; badge?: number }[] = [
    { id: 'recon', label: 'Recon', icon: <Target size={18} /> },
    { id: 'session', label: 'Session', icon: <Bot size={18} /> },
    { id: 'ops', label: 'Ops', icon: <Zap size={18} />, badge: runningCount || undefined },
    ...(node.intercept_supported ? [{ id: 'intercept' as Tab, label: 'Intercept', icon: <Shield size={18} />, badge: agentTraffic.length || undefined }] : []),
  ];

  //
  // Get MCP servers from recon result.
  //
  const allServers = reconResult?.tools.mcp_servers ?? [];

  return (
    <div className="space-y-4 md:space-y-6 md:h-full flex flex-col">
      {/*
      //
      // Back link.
      //
      */}
      <Link
        to={`/nodes/${nodeId}`}
        className="inline-flex items-center gap-2 text-muted hover:text-[var(--text-primary)]"
      >
        <ArrowLeft size={18} />
        Back to Node
      </Link>

      {/*
      //
      // Agent header.
      //
      */}
      <div className="bg-card ascii-box border border-subtle p-4 md:p-6">
        <div className="flex flex-col xl:flex-row xl:items-start xl:justify-between gap-4">
          <div className="flex items-start md:items-center gap-3 md:gap-4">
            <div className="p-3  bg-[var(--accent-success)]/20">
              <Bot size={28} className="text-[var(--accent-success)]" />
            </div>
            <div>
              <h1 className="text-xl md:text-2xl font-bold text-highlight">
                {agentName}
                {discoveredAgent?.version && (
                  <span className="text-sm font-normal text-muted ml-2">v{discoveredAgent.version}</span>
                )}
              </h1>
              <p className="text-muted text-sm mt-1">
                {node.machine_name}
                {node.os_details && <span className="text-xs ml-2 opacity-70">({node.os_details})</span>}
              </p>
              {selectedAgent?.process_name && (
                <p className="text-muted text-sm mt-1">Process: {selectedAgent.process_name}</p>
              )}
              {discoveredAgent && !selectedAgent && (
                <p className="text-muted text-xs mt-1">{discoveredAgent.name}</p>
              )}
            </div>
          </div>
          <div className="flex flex-col items-start xl:items-end gap-2 w-full xl:w-auto">
            {/*
            //
            // Top row: Run Op + Run Chain + Start/Close Session.
            //
            */}
            <div className="flex flex-wrap items-center gap-2 w-full xl:w-auto">
              <button
                onClick={() => setShowRunOpModal(true)}
                className="inline-flex items-center gap-2 px-3 md:px-4 py-2 text-sm whitespace-nowrap bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors"
              >
                <Zap size={16} /> Run Op
              </button>
              <button
                onClick={() => setShowRunChainModal(true)}
                className="inline-flex items-center gap-2 px-3 md:px-4 py-2 text-sm whitespace-nowrap bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors"
              >
                <GitBranch size={16} /> Run Chain
              </button>
              {hasSession ? (
                <button
                  onClick={handleCloseSession}
                  disabled={isClosingSession}
                  className="inline-flex items-center gap-2 px-3 md:px-4 py-2 text-sm whitespace-nowrap bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors disabled:opacity-50"
                >
                  {isClosingSession ? (
                    <><Loader2 size={16} className="animate-spin" /> Closing...</>
                  ) : (
                    <><Square size={16} /> Close Session</>
                  )}
                </button>
              ) : (
                <button
                  onClick={handleCreateSession}
                  disabled={!discoveredAgent?.available || isCreatingSession || isLoadingRecon}
                  className="inline-flex items-center gap-2 px-3 md:px-4 py-2 text-sm whitespace-nowrap bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors disabled:opacity-50"
                >
                  {isCreatingSession ? (
                    <><Loader2 size={16} className="animate-spin" /> Starting...</>
                  ) : (
                    <><Play size={16} /> Start Session</>
                  )}
                </button>
              )}
            </div>
            {/*
            //
            // Session options - always visible, disabled when session active.
            //
            */}
            {/*
            //
            // Project path selector row.
            //
            */}
            <div className={`flex flex-wrap items-center gap-2 ${hasSession ? 'opacity-50' : ''}`}>
              {isLoadingRecon ? (
                <span className="text-xs text-muted flex items-center gap-1">
                  <Loader2 size={12} className="animate-spin" />
                  Refreshing recon...
                </span>
              ) : projectPaths.length > 0 ? (
                <div className="flex items-center gap-1.5 max-w-full">
                  <FolderOpen size={14} className="text-muted" />
                  <select
                    value={selectedProjectPath ?? projectPaths[0] ?? ''}
                    onChange={(e) => setSelectedProjectPath(e.target.value || null)}
                    disabled={hasSession}
                    className="bg-[var(--bg-secondary)] border border-subtle px-2 py-1 text-xs text-[var(--text-primary)] focus:outline-none focus:border-[var(--border-active)] max-w-[58vw] sm:max-w-[260px] disabled:cursor-not-allowed"
                    title={hasSession ? "Close session to change project path" : "Select project directory for session"}
                  >
                    {projectPaths.map((path) => (
                      <option key={path} value={path}>
                        {path}
                      </option>
                    ))}
                  </select>
                </div>
              ) : null}
            </div>
            {/*
            //
            // YOLO toggle row.
            //
            */}
            <button
              onClick={handleToggleYolo}
              disabled={hasSession}
              className={`flex items-center gap-1.5 text-xs ${hasSession ? 'opacity-50 cursor-not-allowed' : ''}`}
              title={hasSession ? "Close session to change YOLO mode" : (localYoloMode ? "YOLO mode enabled - agent will auto-approve actions" : "YOLO mode disabled - agent requires approval")}
            >
              {localYoloMode ? (
                <ToggleRight size={16} className="text-[var(--accent-warning)]" />
              ) : (
                <ToggleLeft size={16} className="text-muted" />
              )}
              <span className={localYoloMode ? "text-[var(--accent-warning)]" : "text-muted"}>
                YOLO
              </span>
            </button>
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
              {tab.badge && (
                <span className="px-1.5 py-0.5 text-xs rounded-full bg-[var(--accent-info)]/20 text-[var(--accent-info)]">
                  {tab.badge}
                </span>
              )}
            </button>
          ))}
        </div>
      </div>

      {/*
      //
      // Tab content.
      //
      */}
      <div className="md:flex-1 md:min-h-0">
        {activeTab === 'session' && (
          <div className="bg-card ascii-box border border-subtle md:h-full flex flex-col overflow-auto md:overflow-hidden">
            {!hasSession ? (
              <div className="flex-1 flex items-center justify-center p-8">
                <div className="text-center">
                  <Bot size={48} className="mx-auto mb-4 text-muted opacity-50" />
                  <h2 className="text-title font-semibold text-lg mb-2">No Active Session</h2>
                  <p className="text-muted mb-4">Start a session to interact with this agent</p>
                  <button
                    onClick={handleCreateSession}
                    disabled={!discoveredAgent?.available || isCreatingSession}
                    className="inline-flex items-center gap-2 px-4 py-2  bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors disabled:opacity-50"
                  >
                    {isCreatingSession ? (
                      <><Loader2 size={16} className="animate-spin" /> Starting...</>
                    ) : (
                      <><Play size={16} /> Start Session</>
                    )}
                  </button>
                </div>
              </div>
            ) : (
              <>
            {/*
            //
            // Session info banner.
            //
            */}
            <div className="px-4 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)] flex items-center gap-4 text-[10px] text-muted">
              {selectedAgent?.process_name && (
                <span>Process: <span className="font-mono">{selectedAgent.process_name}</span></span>
              )}
              {selectedAgent?.session_id && (
                <span>Session: <span className="font-mono">{selectedAgent.session_id.slice(0, 12)}...</span></span>
              )}
              {selectedAgent?.working_dir && (
                <span>Path: <span className="font-mono">{selectedAgent.working_dir.split('/').slice(-2).join('/')}</span></span>
              )}
            </div>
            {/*
            //
            // Messages.
            //
            */}
            {messages.length === 0 && !isLoading ? (
              <div className="flex-1 flex items-center justify-center px-4 py-8 md:py-12">
                <div className="text-center">
                  <Bot size={48} className="mx-auto mb-4 text-muted opacity-50" />
                  <p className="text-muted">Send a message to start the conversation</p>
                </div>
              </div>
            ) : (
              <div className="flex-1 min-h-0 overflow-auto p-4 space-y-4">
                {messages.map((msg, idx) => (
                  <div
                    key={idx}
                    className={`flex ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}
                  >
                    <div
                      className={`max-w-[80%] px-4 py-3 ${
                        msg.role === 'user'
                          ? ''
                          : 'bg-[var(--bg-secondary)]'
                      }`}
                    >
                      {msg.role === 'user' ? (
                        <p className="whitespace-pre-wrap text-[var(--accent-info)]">{msg.content}</p>
                      ) : (
                        <div className="prose prose-invert prose-sm max-w-none break-words text-[var(--text-secondary)] prose-table:border-collapse prose-th:border prose-th:border-subtle prose-th:px-3 prose-th:py-2 prose-th:bg-[var(--bg-tertiary)] prose-td:border prose-td:border-subtle prose-td:px-3 prose-td:py-2 prose-headings:text-[var(--text-secondary)] prose-p:text-[var(--text-secondary)] prose-li:text-[var(--text-secondary)] prose-code:text-[var(--text-secondary)]">
                          <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.content}</ReactMarkdown>
                        </div>
                      )}
                      <p className="text-xs mt-2" style={{ color: 'var(--text-muted)' }}>
                        {msg.timestamp.toLocaleTimeString()}
                      </p>
                    </div>
                  </div>
                ))}
                {isLoading && (
                  <div className="flex justify-start">
                    <div className="bg-[var(--bg-secondary)] ascii-box px-4 py-3">
                      <Loader2 size={20} className="animate-spin text-[var(--accent-info)]" />
                    </div>
                  </div>
                )}
                <div ref={messagesEndRef} />
              </div>
            )}

            {/*
            //
            // Input.
            //
            */}
            <div className="p-4 border-t border-subtle">
              <div className="flex gap-3">
                <input
                  ref={messageInputRef}
                  type="text"
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  onKeyDown={(e) => e.key === 'Enter' && !e.shiftKey && handleSendMessage()}
                  placeholder="Type a message..."
                  className="flex-1 bg-[var(--bg-secondary)] border border-subtle  px-4 py-3 text-[var(--text-primary)] placeholder-[var(--text-secondary)] focus:outline-none focus:border-[var(--border-active)]"
                  disabled={isLoading}
                />
                <button
                  onClick={handleExportSession}
                  disabled={messages.length === 0}
                  className="px-4 py-3 bg-[var(--bg-secondary)] border border-subtle text-muted hover:text-[var(--text-primary)] hover:border-[var(--border-active)] transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                  title="Export session transcript"
                >
                  <Download size={20} />
                </button>
                <button
                  onClick={handleSendMessage}
                  disabled={!input.trim() || isLoading}
                  className="px-4 py-3 bg-[var(--accent-info)]/20 text-[var(--accent-info)]  hover:bg-[var(--accent-info)]/30 transition-colors disabled:opacity-50"
                >
                  <Send size={20} />
                </button>
              </div>
            </div>
              </>
            )}
          </div>
        )}


        {activeTab === 'ops' && (
          <div className="bg-card ascii-box border border-subtle overflow-hidden">
            {sortedOpsAndChains.length === 0 ? (
              <div className="p-12 text-center">
                <Zap size={48} className="mx-auto mb-4 text-muted opacity-50" />
                <h2 className="text-title font-semibold text-lg mb-2">No Operations</h2>
                <p className="text-muted">No operations have been run on this agent yet</p>
              </div>
            ) : (
              <table className="w-full text-xs">
                <thead>
                  <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
                    <th className="text-left px-4 py-2 text-muted tracking-wider">NAME</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">ID</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">TYPE</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">STARTED</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">DURATION</th>
                    <th className="text-left px-4 py-2 text-muted tracking-wider">STATUS</th>
                  </tr>
                </thead>
                <tbody>
                  {sortedOpsAndChains.map((item) => item.type === 'chain' ? (
                    <tr
                      key={item.id}
                      className="border-b border-dim last:border-0 hover:bg-[var(--highlight)] transition-colors cursor-pointer"
                      onClick={() => setSelectedChainExecId(item.data.execution_id)}
                    >
                      <td className="px-4 py-3">
                        <div className="flex items-center gap-3">
                          {item.data.status === 'Running' || item.data.status === 'Queued' ? (
                            <Loader2 size={14} className="animate-spin text-[var(--accent-info)]" />
                          ) : (
                            <GitBranch size={14} className="text-muted" />
                          )}
                          <span className="font-medium">{item.data.chain_name}</span>
                        </div>
                      </td>
                      <td className="px-4 py-3 text-muted font-mono">{item.data.execution_id.slice(0, 8)}...</td>
                      <td className="px-4 py-3">
                        <span className="text-xs text-muted flex items-center gap-1">
                          <GitBranch size={10} /> Chain
                        </span>
                      </td>
                      <td className="px-4 py-3 text-muted">
                        {new Date(item.data.started_at).toLocaleString()}
                      </td>
                      <td className="px-4 py-3">
                        <div className="flex items-center gap-1 text-muted">
                          <Clock size={12} />
                          {formatOpDuration(item.data.started_at, item.data.ended_at)}
                        </div>
                      </td>
                      <td className="px-4 py-3">
                        <StatusBadge
                          status={getOperationStatusColor(item.data.status)}
                          label={item.data.status}
                        />
                      </td>
                    </tr>
                  ) : (
                    <tr
                      key={item.id}
                      className="border-b border-dim last:border-0 hover:bg-[var(--highlight)] transition-colors cursor-pointer"
                      onClick={() => setSelectedOp(item.data)}
                    >
                      <td className="px-4 py-3">
                        <div className="flex items-center gap-3">
                          {item.data.status === 'Running' ? (
                            <Loader2 size={14} className="animate-spin text-[var(--accent-info)]" />
                          ) : (
                            <Zap size={14} className="text-muted" />
                          )}
                          <span className="font-medium">{item.data.spec.name}</span>
                        </div>
                      </td>
                      <td className="px-4 py-3 text-muted font-mono">{item.data.operation_id.slice(0, 8)}...</td>
                      <td className="px-4 py-3">
                        <span className="text-xs text-muted flex items-center gap-1">
                          <Zap size={10} /> Op
                        </span>
                      </td>
                      <td className="px-4 py-3 text-muted">
                        {new Date(item.data.start_time).toLocaleString()}
                      </td>
                      <td className="px-4 py-3">
                        <div className="flex items-center gap-1 text-muted">
                          <Clock size={12} />
                          {formatOpDuration(item.data.start_time, item.data.end_time)}
                        </div>
                      </td>
                      <td className="px-4 py-3">
                        <StatusBadge
                          status={getOperationStatusColor(item.data.status)}
                          label={item.data.status}
                        />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        )}


        {activeTab === 'recon' && (
          <div className="bg-card ascii-box border border-subtle md:h-full flex flex-col overflow-auto md:overflow-hidden">
            {/*
            //
            // Recon subtabs.
            //
            */}
            <div className="px-3 md:px-4 py-2 border-b border-subtle flex flex-col gap-2 lg:flex-row lg:items-center lg:justify-between bg-[var(--bg-secondary)]">
              <div className="flex gap-1 overflow-x-auto w-full lg:w-auto">
                <button
                  onClick={() => setReconSubTab('config')}
                  className={`px-3 py-1.5 text-sm whitespace-nowrap transition-colors ${
                    reconSubTab === 'config'
                      ? 'bg-[var(--highlight)] text-title'
                      : 'text-muted hover:text-[var(--text-primary)]'
                  }`}
                >
                  <span className="flex items-center gap-2">
                    <Settings size={14} />
                    Config
                    {(reconResult?.config?.length ?? 0) > 0 && (
                      <span className="text-[10px] opacity-70">
                        {reconResult?.config?.length}
                      </span>
                    )}
                  </span>
                </button>
                <button
                  onClick={() => setReconSubTab('tools')}
                  className={`px-3 py-1.5 text-sm whitespace-nowrap transition-colors ${
                    reconSubTab === 'tools'
                      ? 'bg-[var(--highlight)] text-title'
                      : 'text-muted hover:text-[var(--text-primary)]'
                  }`}
                >
                  <span className="flex items-center gap-2">
                    <Wrench size={14} />
                    Tools
                    {(() => {
                      const toolsCount = (reconResult?.tools?.mcp_servers?.length ?? 0) +
                        (reconResult?.tools?.skills?.length ?? 0) +
                        (reconResult?.tools?.internal_tools?.length ?? 0);
                      return toolsCount > 0 ? (
                        <span className="text-[10px] opacity-70">{toolsCount}</span>
                      ) : null;
                    })()}
                  </span>
                </button>
                <button
                  onClick={() => setReconSubTab('sessions')}
                  className={`px-3 py-1.5 text-sm whitespace-nowrap transition-colors ${
                    reconSubTab === 'sessions'
                      ? 'bg-[var(--highlight)] text-title'
                      : 'text-muted hover:text-[var(--text-primary)]'
                  }`}
                >
                  <span className="flex items-center gap-2">
                    <History size={14} />
                    Sessions
                    {(reconResult?.sessions?.length ?? 0) > 0 && (
                      <span className="text-[10px] opacity-70">
                        {reconResult?.sessions?.length}
                      </span>
                    )}
                  </span>
                </button>
              </div>
              <div className="flex items-center flex-wrap gap-2 sm:gap-3 w-full lg:w-auto lg:justify-end">
                <div className="flex flex-wrap gap-2">
                  <button
                    onClick={handleDiscoverTools}
                    disabled={isDiscoveringTools || !discoveredAgent?.available}
                    className="inline-flex items-center gap-2 px-3 py-1.5 bg-[var(--accent-info)]/20 text-[var(--accent-info)] text-sm whitespace-nowrap hover:bg-[var(--accent-info)]/30 transition-colors disabled:opacity-50"
                  >
                    {isDiscoveringTools ? (
                      <>
                        <Loader2 size={14} className="animate-spin" /> Discovering...
                      </>
                    ) : (
                      <>
                        <Search size={14} /> Discover
                      </>
                    )}
                  </button>
                  <button
                    onClick={() => handleRecon(false)}
                    disabled={isLoadingRecon || !discoveredAgent?.available}
                    className="inline-flex items-center gap-2 px-3 py-1.5 bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] text-sm whitespace-nowrap hover:bg-[var(--accent-purple)]/30 transition-colors disabled:opacity-50"
                  >
                    <RefreshCw size={14} className={isLoadingRecon ? 'animate-spin' : ''} />
                    {isLoadingRecon ? 'Refreshing...' : 'Refresh'}
                  </button>
                </div>
                {reconPerformedAt && (
                  <Tooltip content={`Last ${reconIsSemantic ? 'semantic ' : ''}recon: ${new Date(reconPerformedAt).toLocaleString()}`}>
                    <span className="text-xs text-muted flex items-center gap-1 whitespace-nowrap">
                      <Clock size={12} />
                      {(() => {
                        const date = new Date(reconPerformedAt);
                        const now = new Date();
                        const diffMs = now.getTime() - date.getTime();
                        const diffMins = Math.floor(diffMs / 60000);
                        const diffHours = Math.floor(diffMins / 60);
                        const diffDays = Math.floor(diffHours / 24);
                        if (diffMins < 1) return 'just now';
                        if (diffMins < 60) return `${diffMins}m ago`;
                        if (diffHours < 24) return `${diffHours}h ago`;
                        return `${diffDays}d ago`;
                      })()}
                      {reconIsSemantic && <Sparkles size={10} className="text-[var(--accent-info)]" />}
                    </span>
                  </Tooltip>
                )}
              </div>
            </div>

            {/*
            //
            // Tools subtab.
            //
            */}
            {reconSubTab === 'tools' && (() => {
              const hasMcp = allServers.length > 0;
              const hasSkills = (reconResult?.tools.skills?.length ?? 0) > 0;
              const hasInternal = (reconResult?.tools.internal_tools?.length ?? 0) > 0;
              const hasAnyTools = hasMcp || hasSkills || hasInternal;

              //
              // Auto-select first available category if current selection is empty.
              //

              const effectiveTab = (toolsSubTab === 'mcp' && !hasMcp && (hasSkills || hasInternal))
                ? (hasSkills ? 'skills' : 'internal')
                : (toolsSubTab === 'skills' && !hasSkills && (hasMcp || hasInternal))
                ? (hasMcp ? 'mcp' : 'internal')
                : (toolsSubTab === 'internal' && !hasInternal && (hasMcp || hasSkills))
                ? (hasMcp ? 'mcp' : 'skills')
                : toolsSubTab;

              if (!hasAnyTools) {
                return (
                  <div className="flex-1 min-h-0 flex items-center justify-center">
                    <div className="text-center">
                      <Wrench size={48} className="mx-auto mb-4 text-muted opacity-50" />
                      <p className="text-muted">No tools discovered for this agent</p>
                      <p className="text-muted text-sm mt-2">Click "Discover" to scan for tools</p>
                    </div>
                  </div>
                );
              }

              return (
                <div className="flex-1 min-h-0 flex">
                  {/*
                  //
                  // Left menu.
                  //
                  */}
                  <div className="w-36 flex-shrink-0 border-r border-subtle bg-[var(--bg-secondary)]">
                    {hasMcp && (
                      <button
                        onClick={() => setToolsSubTab('mcp')}
                        className={`w-full px-3 py-2 text-left text-xs transition-colors flex items-center justify-between ${
                          effectiveTab === 'mcp'
                            ? 'bg-[var(--accent-info)]/10 text-[var(--accent-info)] border-l-2 border-l-[var(--accent-info)]'
                            : 'text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)]'
                        }`}
                      >
                        <span className="flex items-center gap-2">
                          <Wrench size={14} />
                          MCP Servers
                        </span>
                        <span className="text-[10px] opacity-70">{allServers.length}</span>
                      </button>
                    )}
                    {hasSkills && (
                      <button
                        onClick={() => setToolsSubTab('skills')}
                        className={`w-full px-3 py-2 text-left text-xs transition-colors flex items-center justify-between ${
                          effectiveTab === 'skills'
                            ? 'bg-[var(--accent-info)]/10 text-[var(--accent-info)] border-l-2 border-l-[var(--accent-info)]'
                            : 'text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)]'
                        }`}
                      >
                        <span className="flex items-center gap-2">
                          <Sparkles size={14} />
                          Skills
                        </span>
                        <span className="text-[10px] opacity-70">{reconResult?.tools.skills?.length}</span>
                      </button>
                    )}
                    {hasInternal && (
                      <button
                        onClick={() => setToolsSubTab('internal')}
                        className={`w-full px-3 py-2 text-left text-xs transition-colors flex items-center justify-between ${
                          effectiveTab === 'internal'
                            ? 'bg-[var(--accent-info)]/10 text-[var(--accent-info)] border-l-2 border-l-[var(--accent-info)]'
                            : 'text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)]'
                        }`}
                      >
                        <span className="flex items-center gap-2">
                          <Cpu size={14} />
                          Internal
                        </span>
                        <span className="text-[10px] opacity-70">{reconResult?.tools.internal_tools?.length}</span>
                      </button>
                    )}
                  </div>

                  {/*
                  //
                  // Content area.
                  //
                  */}
                  <div className="flex-1 min-w-0">
                    {effectiveTab === 'mcp' && (() => {
                      //
                      // Group servers by context_path.
                      //
                      const serversByContext = allServers.reduce((acc, server, idx) => {
                        const key = server.context_path ?? 'Global';
                        if (!acc[key]) acc[key] = [];
                        acc[key].push({ server, idx });
                        return acc;
                      }, {} as Record<string, { server: typeof allServers[0]; idx: number }[]>);

                      //
                      // Sort contexts: Global first, then alphabetically.
                      //
                      const contexts = Object.keys(serversByContext).sort((a, b) => {
                        if (a === 'Global') return -1;
                        if (b === 'Global') return 1;
                        return a.localeCompare(b);
                      });

                      return (
                        <div className="md:h-full flex flex-col">
                          <div className="flex-1 flex flex-col xl:flex-row min-h-0 p-4 gap-4">
                            {/*
                            //
                            // Left panel: Server list grouped by context.
                            //
                            */}
                            <div className="w-full xl:w-56 flex-shrink-0 flex flex-col border border-subtle rounded overflow-hidden bg-[var(--bg-secondary)] max-h-64 xl:max-h-none">
                              <div className="px-3 py-2 border-b border-subtle bg-[var(--bg-tertiary)]">
                                <span className="text-xs text-muted uppercase tracking-wider">
                                  {allServers.length} server{allServers.length !== 1 ? 's' : ''} • {allServers.reduce((sum, s) => sum + s.tools.length, 0)} tools
                                </span>
                              </div>
                              <div className="flex-1 overflow-y-auto scrollbar-on-hover">
                                {contexts.map(context => {
                                  const servers = serversByContext[context];
                                  const isGlobal = context === 'Global';
                                  const contextDisplay = isGlobal ? 'Global' : context.split('/').slice(-2).join('/');
                                  const isExpanded = expandedMcpContexts.has(context);

                                  const toggleContext = () => {
                                    setExpandedMcpContexts(prev => {
                                      const next = new Set(prev);
                                      if (next.has(context)) next.delete(context);
                                      else next.add(context);
                                      return next;
                                    });
                                  };

                                  return (
                                    <div key={context}>
                                      {/*
                                      //
                                      // Context header (collapsible).
                                      //
                                      */}
                                      <button
                                        onClick={toggleContext}
                                        className="w-full px-2 py-1.5 bg-[var(--bg-tertiary)] border-b border-subtle flex items-center gap-1.5 hover:bg-[var(--highlight)] transition-colors"
                                      >
                                        <ChevronRight
                                          size={12}
                                          className={`text-muted transition-transform ${isExpanded ? 'rotate-90' : ''}`}
                                        />
                                        {isGlobal ? (
                                          <Settings size={10} className="text-muted" />
                                        ) : (
                                          <FolderOpen size={10} className="text-muted" />
                                        )}
                                        <span className="text-[10px] font-mono text-muted truncate" title={context}>
                                          {contextDisplay}
                                        </span>
                                        <span className="text-[9px] text-muted ml-auto">{servers.length}</span>
                                      </button>

                                      {/*
                                      //
                                      // Servers in this context (collapsible).
                                      //
                                      */}
                                      {isExpanded && servers.map(({ server, idx }) => {
                                        const isSelected = selectedServerIdx === idx;
                                        return (
                                          <button
                                            key={`${server.name}-${idx}`}
                                            onClick={() => setSelectedServerIdx(idx)}
                                            className={`w-full pl-6 pr-2 py-1.5 text-left transition-colors border-b border-dim last:border-0 ${
                                              isSelected
                                                ? 'bg-[var(--accent-info)]/10 border-l-2 border-l-[var(--accent-info)]'
                                                : 'hover:bg-[var(--bg-tertiary)]'
                                            }`}
                                          >
                                            <div className="flex items-center gap-1.5">
                                              <Wrench size={11} className={isSelected ? 'text-[var(--accent-info)]' : 'text-muted'} />
                                              <span className={`text-xs font-medium truncate ${isSelected ? 'text-[var(--accent-info)]' : ''}`}>
                                                {server.name}
                                              </span>
                                              <span className="text-[9px] text-muted ml-auto">
                                                {server.tools.length}
                                              </span>
                                            </div>
                                          </button>
                                        );
                                      })}
                                    </div>
                                  );
                                })}
                              </div>
                            </div>

                            {/*
                            //
                            // Right panel: Server tools.
                            //
                            */}
                            <div className="flex-1 flex flex-col min-w-0 border border-subtle rounded overflow-hidden">
                              {selectedServerIdx === null ? (
                                <div className="flex-1 flex items-center justify-center bg-[var(--bg-secondary)]">
                                  <div className="text-center text-muted">
                                    <Wrench size={32} className="mx-auto mb-2 opacity-50" />
                                    <p className="text-sm">Select a server to view tools</p>
                                  </div>
                                </div>
                              ) : (
                                <>
                                  <div className="px-4 py-2 border-b border-subtle bg-[var(--bg-tertiary)] flex-shrink-0">
                                    <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
                                      <div className="flex items-center gap-2">
                                        <Wrench size={16} className="text-[var(--accent-info)]" />
                                        <span className="font-medium text-[var(--accent-info)]">
                                          {allServers[selectedServerIdx].name}
                                        </span>
                                        <span className="text-[10px] px-1.5 py-0.5 bg-[var(--bg-primary)] text-muted rounded">
                                          {allServers[selectedServerIdx].transport}
                                        </span>
                                      </div>
                                      <span className="text-xs text-muted">
                                        {allServers[selectedServerIdx].tools.length} tool{allServers[selectedServerIdx].tools.length !== 1 ? 's' : ''}
                                      </span>
                                    </div>
                                    {allServers[selectedServerIdx].context_path && (
                                      <div className="mt-1 flex items-center gap-1 text-[10px] text-muted">
                                        <FolderOpen size={10} />
                                        <span className="font-mono truncate" title={allServers[selectedServerIdx].context_path || ''}>
                                          {allServers[selectedServerIdx].context_path}
                                        </span>
                                      </div>
                                    )}
                                    {(allServers[selectedServerIdx].command || allServers[selectedServerIdx].address) && (
                                      <div className="mt-1 text-[10px] font-mono text-muted truncate" title={allServers[selectedServerIdx].command || allServers[selectedServerIdx].address || ''}>
                                        {allServers[selectedServerIdx].command || allServers[selectedServerIdx].address}
                                      </div>
                                    )}
                                  </div>
                                  <div className="flex-1 overflow-y-auto bg-[var(--bg-secondary)] p-4 scrollbar-on-hover">
                                    <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
                                      {allServers[selectedServerIdx].tools.map((tool) => (
                                        <div
                                          key={tool.name}
                                          className="p-3 bg-[var(--bg-primary)] rounded border border-subtle hover:border-[var(--accent-info)]/50 transition-colors"
                                        >
                                          <p className="font-mono text-xs text-[var(--accent-info)] truncate" title={tool.name}>
                                            {tool.name}
                                          </p>
                                          <p className="text-xs text-muted mt-1.5 line-clamp-2" title={tool.description}>
                                            {tool.description || 'No description'}
                                          </p>
                                        </div>
                                      ))}
                                    </div>
                                  </div>
                                </>
                              )}
                            </div>
                          </div>
                        </div>
                      );
                    })()}

                    {effectiveTab === 'skills' && reconResult?.tools.skills && (
                      <div className="p-4 overflow-auto h-full">
                        <div className="space-y-2">
                          {reconResult.tools.skills.map((skill) => (
                            <div key={skill.name} className="p-3 border border-subtle rounded">
                              <div className="flex items-center gap-2">
                                <Sparkles size={16} className="text-[var(--accent-info)]" />
                                <p className="font-medium">{skill.name}</p>
                              </div>
                              <p className="text-xs text-muted mt-2">{skill.description}</p>
                            </div>
                          ))}
                        </div>
                      </div>
                    )}

                    {effectiveTab === 'internal' && reconResult?.tools.internal_tools && (
                      <div className="p-4 overflow-auto h-full">
                        <div className="space-y-2">
                          {reconResult.tools.internal_tools.map((tool) => (
                            <div key={tool.name} className="p-3 border border-subtle rounded">
                              <div className="flex items-center gap-2">
                                <Cpu size={16} className="text-[var(--accent-purple)]" />
                                <p className="font-medium">{tool.name}</p>
                              </div>
                              <p className="text-xs text-muted mt-2">{tool.description}</p>
                            </div>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              );
            })()}

            {/*
            //
            // Config subtab.
            //
            */}
            {reconSubTab === 'config' && (
              <div className="flex-1 min-h-0 flex flex-col">
                {/*
                //
                // Compact metadata section - collapsible.
                //
                */}
                {(reconResult?.metadata?.user_identities?.length || reconResult?.metadata?.api_keys?.length) ? (
                  <div className="mx-4 mt-4 mb-2 bg-[var(--bg-secondary)] rounded border border-subtle flex-shrink-0">
                    {/*
                    //
                    // Header with toggle.
                    //
                    */}
                    <button
                      onClick={() => setMetadataCollapsed(!metadataCollapsed)}
                      className="w-full px-2 py-1.5 flex items-center gap-2 hover:bg-[var(--bg-tertiary)] transition-colors text-xs"
                    >
                      {metadataCollapsed ? (
                        <ChevronRight size={14} className="text-muted" />
                      ) : (
                        <ChevronDown size={14} className="text-muted" />
                      )}
                      <Tooltip content="Semantically extracted using AI">
                        <svg
                          width="14"
                          height="14"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="var(--text-muted)"
                          strokeWidth="2"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          className="opacity-40"
                        >
                          <path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z" />
                          <path d="M20 3v4" />
                          <path d="M22 5h-4" />
                          <path d="M4 17v2" />
                          <path d="M5 18H3" />
                        </svg>
                      </Tooltip>
                      <span className="text-muted font-medium">
                        Extracted Metadata
                      </span>
                      <span className="text-[10px] text-muted ml-auto">
                        {(reconResult?.metadata?.user_identities?.length || 0) + (reconResult?.metadata?.api_keys?.length || 0)} items
                      </span>
                    </button>

                    {/*
                    //
                    // Content - shown when not collapsed.
                    //
                    */}
                    {!metadataCollapsed && (
                      <div className="px-2 pb-2 text-xs border-t border-subtle max-h-40 overflow-y-auto scrollbar-on-hover">
                        {reconResult?.metadata?.user_identities?.length ? (
                          <div className="flex items-center gap-2 flex-wrap mt-2">
                            <span className="text-muted flex items-center gap-1">
                              <User size={12} />
                              Identities:
                            </span>
                            {reconResult.metadata.user_identities.map((identity, idx) => (
                              <span key={idx} className="px-1.5 py-0.5 font-mono bg-[var(--accent-info)]/10 text-[var(--accent-info)] rounded break-all max-w-full">{identity}</span>
                            ))}
                          </div>
                        ) : null}
                        {reconResult?.metadata?.api_keys?.length ? (
                          <div className={`flex items-center gap-2 flex-wrap ${reconResult?.metadata?.user_identities?.length ? 'mt-1.5' : 'mt-2'}`}>
                            <span className="text-muted flex items-center gap-1">
                              <Key size={12} />
                              API Keys:
                            </span>
                            {reconResult.metadata.api_keys.map((key, idx) => (
                              <span key={idx} className="px-1.5 py-0.5 font-mono bg-[var(--accent-warning)]/10 text-[var(--accent-warning)] rounded break-all max-w-full">{key}</span>
                            ))}
                          </div>
                        ) : null}
                      </div>
                    )}
                  </div>
                ) : null}

                {!reconResult?.config?.length ? (
                  <div className="flex-1 flex items-center justify-center">
                    <div className="text-center">
                      <Settings size={48} className="mx-auto mb-4 text-muted opacity-50" />
                      <p className="text-muted">No configuration files discovered</p>
                      <p className="text-muted text-sm mt-2">Click "Refresh" to fetch config files</p>
                    </div>
                  </div>
                ) : (
                  <div className="flex-1 flex flex-col lg:flex-row min-h-0 p-4 pt-2 gap-4">
                    {/*
                    //
                    // Left panel: File list grouped by directory.
                    //
                    */}
                    <div className="w-full lg:w-64 flex-shrink-0 flex flex-col border border-subtle rounded overflow-hidden bg-[var(--bg-secondary)] max-h-56 lg:max-h-none">
                      <div className="px-3 py-2 border-b border-subtle bg-[var(--bg-tertiary)]">
                        <span className="text-[10px] text-muted uppercase tracking-wider">
                          {reconResult.config.length} file{reconResult.config.length !== 1 ? 's' : ''}
                        </span>
                      </div>
                      <div className="flex-1 overflow-y-auto scrollbar-on-hover">
                        {(() => {
                          //
                          // Group files by directory.
                          //
                          const grouped = reconResult.config.reduce((acc, item, idx) => {
                            const parts = item.path.split('/');
                            const filename = parts.pop() || item.path;
                            const dir = parts.join('/') || '/';
                            if (!acc[dir]) acc[dir] = [];
                            acc[dir].push({ item, idx, filename });
                            return acc;
                          }, {} as Record<string, { item: typeof reconResult.config[0]; idx: number; filename: string }[]>);

                          const dirs = Object.keys(grouped).sort();
                          const toggleDir = (dir: string) => {
                            setExpandedConfigDirs(prev => {
                              const next = new Set(prev);
                              if (next.has(dir)) next.delete(dir);
                              else next.add(dir);
                              return next;
                            });
                          };

                          return dirs.map(dir => {
                            const files = grouped[dir];
                            const isExpanded = expandedConfigDirs.has(dir) || dirs.length === 1;
                            const dirDisplay = dir.split('/').slice(-2).join('/') || dir;

                            return (
                              <div key={dir}>
                                {/*
                                //
                                // Directory header.
                                //
                                */}
                                <button
                                  onClick={() => toggleDir(dir)}
                                  className="w-full px-2 py-1.5 text-left flex items-center gap-1.5 hover:bg-[var(--bg-tertiary)] border-b border-subtle"
                                >
                                  <ChevronRight
                                    size={12}
                                    className={`text-muted transition-transform ${isExpanded ? 'rotate-90' : ''}`}
                                  />
                                  <FolderOpen size={12} className="text-muted" />
                                  <span className="text-[11px] font-mono text-muted truncate" title={dir}>
                                    {dirDisplay}
                                  </span>
                                  <span className="text-[10px] text-muted ml-auto">
                                    {files.length}
                                  </span>
                                </button>

                                {/*
                                //
                                // Files in directory.
                                //
                                */}
                                {isExpanded && files.map(({ item, idx, filename }) => {
                                  const isSelected = selectedConfigIdx === idx;
                                  return (
                                    <button
                                      key={idx}
                                      onClick={() => {
                                        setSelectedConfigIdx(idx);
                                        if (editingConfigIdx !== null && editingConfigIdx !== idx) {
                                          handleCancelConfigEdit();
                                        }
                                      }}
                                      className={`w-full pl-6 pr-2 py-1.5 text-left transition-colors border-b border-dim last:border-0 ${
                                        isSelected
                                          ? 'bg-[var(--accent-info)]/10 border-l-2 border-l-[var(--accent-info)]'
                                          : 'hover:bg-[var(--bg-tertiary)]'
                                      }`}
                                    >
                                      <div className="flex items-center gap-1.5">
                                        <FileText size={11} className={isSelected ? 'text-[var(--accent-info)]' : 'text-muted'} />
                                        <span className={`text-[11px] font-mono truncate ${isSelected ? 'text-[var(--accent-info)]' : ''}`}>
                                          {filename}
                                        </span>
                                      </div>
                                      <div className="mt-0.5 pl-4">
                                        <span className="text-[9px] px-1 py-0.5 bg-[var(--bg-primary)] text-muted rounded">
                                          {item.config_type}
                                        </span>
                                      </div>
                                    </button>
                                  );
                                })}
                              </div>
                            );
                          });
                        })()}
                      </div>
                    </div>

                    {/*
                    //
                    // Right panel: File content.
                    //
                    */}
                    <div className="flex-1 flex flex-col min-w-0 border border-subtle rounded overflow-hidden">
                      {selectedConfigIdx === null ? (
                        <div className="flex-1 flex items-center justify-center bg-[var(--bg-secondary)]">
                          <div className="text-center text-muted">
                            <FolderOpen size={32} className="mx-auto mb-2 opacity-50" />
                            <p className="text-sm">Select a file to view</p>
                          </div>
                        </div>
                      ) : (
                        <>
                          {/*
                          //
                          // File header.
                          //
                          */}
                            <div className="px-4 py-2 border-b border-subtle bg-[var(--bg-tertiary)] flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 flex-shrink-0">
                            <div className="flex items-center gap-2 min-w-0">
                              <span className="font-mono text-sm truncate text-[var(--accent-info)]">
                                {reconResult.config[selectedConfigIdx].path}
                              </span>
                            </div>
                            {editingConfigIdx !== selectedConfigIdx && !isLoadingConfigContent && (configContent || reconResult.config[selectedConfigIdx].contents) && (
                              <button
                                onClick={() => handleStartConfigEdit(selectedConfigIdx, configContent ?? reconResult.config[selectedConfigIdx].contents ?? '')}
                                className="p-1.5 text-muted hover:text-[var(--accent-info)] hover:bg-[var(--accent-info)]/10 rounded transition-colors flex-shrink-0"
                                title="Edit config file"
                              >
                                <Pencil size={14} />
                              </button>
                            )}
                          </div>

                          {/*
                          //
                          // File content.
                          //
                          */}
                          <div className="flex-1 overflow-auto bg-[var(--bg-secondary)]">
                            {editingConfigIdx === selectedConfigIdx ? (
                              <div className="h-full flex flex-col p-4">
                                <textarea
                                  value={editingConfigContent}
                                  onChange={(e) => setEditingConfigContent(e.target.value)}
                                  className="flex-1 w-full p-3 text-xs font-mono bg-[var(--bg-primary)] border border-subtle rounded focus:outline-none focus:border-[var(--accent-info)] resize-none"
                                  disabled={isSavingConfig}
                                />
                                {configSaveError && (
                                  <div className="mt-2 text-xs text-[var(--accent-error)]">
                                    {configSaveError}
                                  </div>
                                )}
                                <div className="flex justify-end gap-2 mt-3 flex-shrink-0">
                                  <button
                                    onClick={handleCancelConfigEdit}
                                    disabled={isSavingConfig}
                                    className="inline-flex items-center gap-1.5 px-3 py-1.5 text-sm text-muted hover:text-[var(--text-primary)] transition-colors disabled:opacity-50"
                                  >
                                    <X size={14} />
                                    Cancel
                                  </button>
                                  <button
                                    onClick={handleSaveConfig}
                                    disabled={isSavingConfig}
                                    className="inline-flex items-center gap-1.5 px-3 py-1.5 text-sm bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors disabled:opacity-50"
                                  >
                                    {isSavingConfig ? (
                                      <Loader2 size={14} className="animate-spin" />
                                    ) : (
                                      <Save size={14} />
                                    )}
                                    {isSavingConfig ? 'Saving...' : 'Save'}
                                  </button>
                                </div>
                              </div>
                            ) : isLoadingConfigContent ? (
                              <div className="flex-1 flex items-center justify-center">
                                <Loader2 size={24} className="animate-spin text-muted" />
                              </div>
                            ) : configContentError ? (
                              <div className="p-4 text-[var(--accent-error)] text-sm">
                                Error loading config: {configContentError}
                              </div>
                            ) : (
                              <pre className="p-4 text-xs font-mono whitespace-pre-wrap text-muted">
                                {configContent ?? reconResult.config[selectedConfigIdx].contents ?? 'No content available'}
                              </pre>
                            )}
                          </div>
                        </>
                      )}
                    </div>
                  </div>
                )}
              </div>
            )}

            {/*
            //
            // Sessions subtab.
            //
            */}
            {reconSubTab === 'sessions' && (
              <div className="flex-1 flex flex-col min-h-0 overflow-auto md:overflow-hidden">
                {!reconResult?.sessions || reconResult.sessions.length === 0 ? (
                  <div className="flex-1 flex items-center justify-center p-8">
                    <div className="text-center">
                      <History size={48} className="mx-auto mb-4 text-muted opacity-50" />
                      <p className="text-muted">No sessions discovered</p>
                      <p className="text-muted text-sm mt-2">Click "Refresh" to scan for session files</p>
                    </div>
                  </div>
                ) : (
                  <div className="flex-1 flex flex-col lg:flex-row min-h-0 p-4 pt-2 gap-4 overflow-hidden">
                    {/*
                    //
                    // Left panel: Session list.
                    //
                    */}
                    <div className="w-full lg:w-64 flex-shrink-0 flex flex-col border border-subtle rounded overflow-hidden bg-[var(--bg-secondary)] max-h-56 lg:max-h-none">
                      <div className="px-3 py-2 border-b border-subtle bg-[var(--bg-tertiary)]">
                        <span className="text-[10px] text-muted uppercase tracking-wider">
                          {reconResult.sessions.length} session{reconResult.sessions.length !== 1 ? 's' : ''}
                        </span>
                      </div>
                      <div className="flex-1 overflow-y-auto scrollbar-on-hover">
                        {reconResult.sessions.map((session, idx) => {
                          const isSelected = selectedSessionIdx === idx;
                          const shortId = session.session_id.slice(0, 8);
                          return (
                            <button
                              key={session.session_id}
                              onClick={() => setSelectedSessionIdx(idx)}
                              className={`w-full px-3 py-2 text-left transition-colors border-b border-dim last:border-0 ${
                                isSelected
                                  ? 'bg-[var(--accent-info)]/10 border-l-2 border-l-[var(--accent-info)]'
                                  : 'hover:bg-[var(--bg-tertiary)]'
                              }`}
                            >
                              <div className="flex items-center gap-2">
                                <History size={12} className={isSelected ? 'text-[var(--accent-info)]' : 'text-muted'} />
                                <span className={`text-[11px] font-mono ${isSelected ? 'text-[var(--accent-info)]' : ''}`}>
                                  {shortId}
                                </span>
                                <span className="text-[10px] text-muted ml-auto">
                                  {session.message_count}
                                </span>
                              </div>
                              {session.last_modified && (
                                <div className="mt-1 text-[9px] text-muted truncate">
                                  {session.last_modified}
                                </div>
                              )}
                            </button>
                          );
                        })}
                      </div>
                    </div>

                    {/*
                    //
                    // Right panel: Session content.
                    //
                    */}
                    <div className="flex-1 flex flex-col min-w-0 min-h-0 border border-subtle rounded overflow-hidden">
                      {selectedSessionIdx === null ? (
                        <div className="flex-1 flex items-center justify-center text-muted text-sm">
                          Select a session to view contents
                        </div>
                      ) : isLoadingSessionContent ? (
                        <div className="flex-1 flex items-center justify-center">
                          <Loader2 size={24} className="animate-spin text-muted" />
                        </div>
                      ) : sessionContentError ? (
                        <div className="flex-1 flex items-center justify-center p-4">
                          <div className="text-center">
                            <X size={24} className="mx-auto mb-2 text-[var(--accent-danger)]" />
                            <p className="text-[var(--accent-danger)] text-sm">{sessionContentError}</p>
                          </div>
                        </div>
                      ) : (() => {
                        const session = reconResult.sessions[selectedSessionIdx];

                        //
                        // Parse session content - handle both JSONL (Claude Code) and
                        // JSON formats (Gemini).
                        //
                        type ParsedMessage = { type?: string; role?: string; content?: string; timestamp?: string };
                        let messages: ParsedMessage[] = [];

                        if (sessionContent) {
                          //
                          // Try JSONL first (each line is a JSON object).
                          //
                          const lines = sessionContent.split('\n').filter(l => l.trim());
                          if (lines.length > 0) {
                            try {
                              //
                              // Check if first line is valid JSON object.
                              //
                              const firstParsed = JSON.parse(lines[0]);
                              if (typeof firstParsed === 'object' && !Array.isArray(firstParsed)) {
                                //
                                // Likely JSONL format.
                                //
                                messages = lines.map(line => {
                                  try {
                                    return JSON.parse(line) as ParsedMessage;
                                  } catch {
                                    return { content: line };
                                  }
                                });
                              }
                            } catch {
                              //
                              // Try single JSON object with messages array.
                              //
                              try {
                                const parsed = JSON.parse(sessionContent);
                                messages = parsed.messages || [];
                              } catch {
                                //
                                // Plain text fallback.
                                //
                                messages = [{ content: sessionContent }];
                              }
                            }
                          }
                        }

                        return (
                          <>
                            <div className="px-4 py-2 border-b border-subtle bg-[var(--bg-tertiary)] flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 flex-shrink-0">
                              <span className="text-xs font-mono text-muted truncate">
                                {session.session_id}
                              </span>
                              <span className="text-[10px] text-muted">
                                {messages.length} entries
                              </span>
                            </div>
                            <div className="flex-1 overflow-y-auto scrollbar-on-hover">
                              {messages.length === 0 ? (
                                <div className="p-4 text-muted text-sm">No content in this session</div>
                              ) : (
                                <div className="p-3 space-y-3">
                                  {messages.map((msg, idx) => {
                                    const msgType = msg.type || msg.role || 'unknown';
                                    const isUser = msgType === 'user' || msgType === 'human';
                                    const isAssistant = msgType === 'assistant' || msgType === 'gemini' || msgType === 'model';
                                    return (
                                      <div
                                        key={idx}
                                        className={`p-3 rounded text-xs ${
                                          isUser
                                            ? 'bg-[var(--accent-info)]/10 border-l-2 border-l-[var(--accent-info)]'
                                            : isAssistant
                                            ? 'bg-[var(--bg-secondary)] border-l-2 border-l-[var(--accent-purple)]'
                                            : 'bg-[var(--bg-tertiary)] border-l-2 border-l-[var(--border-subtle)]'
                                        }`}
                                      >
                                        <div className="flex items-center gap-2 mb-1">
                                          <span className={`text-[10px] font-medium uppercase ${
                                            isUser ? 'text-[var(--accent-info)]' :
                                            isAssistant ? 'text-[var(--accent-purple)]' :
                                            'text-muted'
                                          }`}>
                                            {msgType}
                                          </span>
                                          {msg.timestamp && (
                                            <span className="text-[9px] text-muted">
                                              {new Date(msg.timestamp).toLocaleString()}
                                            </span>
                                          )}
                                        </div>
                                        <div className="whitespace-pre-wrap break-words font-mono text-[11px]">
                                          {typeof msg.content === 'string' ? msg.content : JSON.stringify(msg, null, 2)}
                                        </div>
                                      </div>
                                    );
                                  })}
                                </div>
                              )}
                            </div>
                          </>
                        );
                      })()}
                    </div>
                  </div>
                )}
              </div>
            )}

          </div>
        )}

        {activeTab === 'intercept' && (
          <AgentInterceptTab
            nodeId={nodeId!}
            agentShortName={agentShortName!}
            agentTraffic={agentTraffic}
            send={send}
          />
        )}
      </div>

      {/*
      //
      // Operation Detail Modal.
      //
      */}
      <OperationDetailModal
        operation={selectedOp}
        onClose={() => setSelectedOp(null)}
      />

      {/*
      //
      // Run Operation Modal.
      //
      */}
      <RunModal
        isOpen={showRunOpModal}
        onClose={() => setShowRunOpModal(false)}
        onRun={handleRunOpFromModal}
        title="Run Operation"
        items={operationDefs.filter(d => !d.disabled).map(def => ({
          id: def.full_name,
          name: def.name,
          description: def.description,
          badge: def.category,
        }))}
        variant="operation"
        fixedNodeId={nodeId || undefined}
        fixedAgentName={agentShortName || undefined}
        warningMessage={hasSession ? 'Running an operation will close the current session.' : undefined}
      />

      {/*
      //
      // Run Chain Modal.
      //
      */}
      <RunModal
        isOpen={showRunChainModal}
        onClose={() => setShowRunChainModal(false)}
        onRun={handleRunChainFromModal}
        title="Run Chain"
        items={chainDefs.filter(c => !c.disabled).map(chain => ({
          id: chain.id,
          name: chain.name,
          description: chain.description,
          badge: `${chain.element_count} elements`,
        }))}
        variant="chain"
        fixedNodeId={nodeId || undefined}
        fixedAgentName={agentShortName || undefined}
        warningMessage={hasSession ? 'Running a chain will close the current session.' : undefined}
      />

      {/*
      //
      // YOLO Mode Warning Modal.
      //
      */}
      <Modal
        isOpen={showYoloWarning}
        onClose={() => setShowYoloWarning(false)}
        title="Change YOLO Mode"
      >
        <div className="space-y-4">
          <div className="flex items-start gap-3 p-4  bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/30">
            <span className="text-[var(--accent-warning)] text-xl mt-0.5">⚠</span>
            <div>
              <p className="text-sm font-medium text-[var(--accent-warning)]">
                Session Active
              </p>
              <p className="text-sm text-muted mt-1">
                A session is currently active. Changing YOLO mode will only take effect from the <strong>next session</strong>. The current session will continue with its existing YOLO setting.
              </p>
            </div>
          </div>

          <div className="flex justify-end gap-3 pt-2">
            <button
              onClick={() => setShowYoloWarning(false)}
              className="px-4 py-2 text-sm  border border-subtle hover:bg-[var(--bg-tertiary)] transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleConfirmYoloToggle}
              className="inline-flex items-center gap-2 px-4 py-2 text-sm  bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30 transition-colors"
            >
              Change Anyway
            </button>
          </div>
        </div>
      </Modal>

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
      />

      {/*
      //
      // Close Session Confirmation Modal.
      //
      */}
      <Modal
        isOpen={showCloseSessionModal}
        title="Close Session"
        onClose={() => setShowCloseSessionModal(false)}
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
              onClick={() => setShowCloseSessionModal(false)}
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
// Agent Intercept Tab Component using shared TrafficTable.
//
function AgentInterceptTab({
  nodeId,
  agentShortName,
  agentTraffic,
  send,
}: {
  nodeId: string;
  agentShortName: string;
  agentTraffic: ReturnType<typeof useApp>['state']['intercept']['trafficLog'];
  send: ReturnType<typeof useApp>['send'];
}) {
  const [expandedRow, setExpandedRow] = useState<number | null>(null);
  const [protocolFilter, setProtocolFilter] = useState<ProtocolFilter>('all');
  const [searchFilter, setSearchFilter] = useState('');
  const [filters, setFilters] = useState<TrafficLogFilters>({
    node_id: nodeId,
    agent_short_name: agentShortName,
    url_pattern: null,
    direction: null,
    start_time: null,
    end_time: null,
    limit: 10000,
    offset: 0,
  });

  const handleRefresh = () => {
    send({ type: 'traffic_log_request', filters });
  };

  //
  // Handle filter changes with auto-refresh.
  //
  const handleFilterChange = (newFilters: TrafficLogFilters) => {
    //
    // Preserve the fixed node_id and agent_short_name.
    //
    const updatedFilters = {
      ...newFilters,
      node_id: nodeId,
      agent_short_name: agentShortName,
    };
    setFilters(updatedFilters);
    send({ type: 'traffic_log_request', filters: updatedFilters });
  };

  //
  // Auto-refresh on mount (tab selection or page load).
  //
  useEffect(() => {
    handleRefresh();
  }, [nodeId, agentShortName, send]);

  return (
    <div className="space-y-4">
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
      />

      {/*
      //
      // Traffic Table.
      //
      */}
      {agentTraffic.length === 0 ? (
        <div className="border border-subtle ascii-box p-12 text-center">
          <Shield size={48} className="mx-auto mb-4 text-muted opacity-50" />
          <h2 className="text-title font-semibold text-lg mb-2">No Traffic Captured</h2>
          <p className="text-muted">Enable interception on this node to start capturing traffic</p>
          <p className="text-muted text-sm mt-2">
            Go to <Link to={`/nodes/${nodeId}?tab=intercept`} className="text-[var(--accent-info)] hover:underline">Node Intercept</Link> to enable
          </p>
        </div>
      ) : (
        <ScrollableTrafficTable
          entries={agentTraffic}
          protocolFilter={protocolFilter}
          searchFilter={searchFilter}
          expandedRow={expandedRow}
          setExpandedRow={setExpandedRow}
          showNodeColumn={false}
          displayLimit={100}
          heightMode="fixed"
          maxHeight="60vh"
        />
      )}
    </div>
  );
}
