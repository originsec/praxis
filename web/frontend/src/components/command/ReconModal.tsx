import { useState, useEffect, useMemo } from 'react';
import {
  Loader2,
  Wrench,
  RefreshCw,
  ChevronRight,
  ChevronDown,
  FolderOpen,
  FileText,
  History,
  Sparkles,
  Cpu,
  Pencil,
  Save,
  X,
  User,
  Key,
  Settings,
  Search,
} from 'lucide-react';
import { Modal } from '../common/Modal';
import { CodeEditor, languageFromPath } from '../common/CodeEditor';
import { useApp } from '../../context/AppContext';
import type { ReconResult } from '../../api/types';

interface ReconModalProps {
  nodeId: string;
  agentShortName: string;
  onClose: () => void;
}

type ReconTab = 'config' | 'tools' | 'sessions';
type ToolsCategory = 'mcp' | 'skills' | 'internal';

export function ReconModal({ nodeId, agentShortName, onClose }: ReconModalProps) {
  const { send, sendCommand } = useApp();

  const [reconResult, setReconResult] = useState<ReconResult | null>(null);
  const [reconPerformedAt, setReconPerformedAt] = useState<string | null>(null);
  const [reconIsSemantic, setReconIsSemantic] = useState<boolean | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  const [activeTab, setActiveTab] = useState<ReconTab>('config');
  const [toolsCategory, setToolsCategory] = useState<ToolsCategory>('mcp');

  //
  // Tools tab state.
  //

  const [selectedServerIdx, setSelectedServerIdx] = useState<number | null>(null);
  const [expandedMcpContexts, setExpandedMcpContexts] = useState<Set<string>>(new Set(['Global']));

  //
  // Config tab state.
  //

  const [selectedConfigIdx, setSelectedConfigIdx] = useState<number | null>(null);
  const [configContent, setConfigContent] = useState<string | null>(null);
  const [isLoadingConfigContent, setIsLoadingConfigContent] = useState(false);
  const [configContentError, setConfigContentError] = useState<string | null>(null);
  const [editingConfigIdx, setEditingConfigIdx] = useState<number | null>(null);
  const [editingConfigContent, setEditingConfigContent] = useState('');
  const [isSavingConfig, setIsSavingConfig] = useState(false);
  const [configSaveError, setConfigSaveError] = useState<string | null>(null);
  const [expandedConfigDirs, setExpandedConfigDirs] = useState<Set<string>>(new Set());
  const [metadataCollapsed, setMetadataCollapsed] = useState(true);

  //
  // Sessions tab state.
  //

  const [selectedSessionIdx, setSelectedSessionIdx] = useState<number | null>(null);
  const [sessionContent, setSessionContent] = useState<string | null>(null);
  const [isLoadingSessionContent, setIsLoadingSessionContent] = useState(false);
  const [sessionContentError, setSessionContentError] = useState<string | null>(null);

  //
  // Fetch recon from service, trigger node recon if needed.
  //

  useEffect(() => {
    let cancelled = false;
    let pollInterval: ReturnType<typeof setInterval> | null = null;
    let reconTriggered = false;

    const requestRecon = () => {
      send({ type: 'recon_get', node_id: nodeId, agent_short_name: agentShortName });
    };

    const handleWsMessage = (event: Event) => {
      if (cancelled) return;
      const message = (event as CustomEvent).detail;
      if (message.type === 'recon_get_response' &&
          message.node_id === nodeId &&
          message.agent_short_name === agentShortName) {
        if (message.recon_result) {
          setReconResult(message.recon_result);
          setReconPerformedAt(message.performed_at);
          setReconIsSemantic(message.is_semantic);
          setIsLoading(false);
          if (pollInterval) clearInterval(pollInterval);
          window.removeEventListener('ws-message', handleWsMessage);
        } else if (!reconTriggered) {
          reconTriggered = true;
          sendCommand(nodeId, { Agent: { Select: { short_name: agentShortName } } })
            .then(() => sendCommand(nodeId, { Agent: 'Recon' }))
            .catch(() => {});
          pollInterval = setInterval(() => {
            if (!cancelled) requestRecon();
          }, 1000);
        }
      }
    };

    window.addEventListener('ws-message', handleWsMessage);
    requestRecon();

    return () => {
      cancelled = true;
      window.removeEventListener('ws-message', handleWsMessage);
      if (pollInterval) clearInterval(pollInterval);
    };
  }, [nodeId, agentShortName, send, sendCommand]);

  //
  // Trigger recon (standard or semantic). Polls until performed_at changes,
  // 60s timeout.
  //

  const handleRecon = (semantic: boolean) => {
    if (!nodeId || !agentShortName) return;

    setIsLoading(true);
    const previousPerformedAt = reconPerformedAt;
    const command = semantic ? 'ReconSemantic' : 'Recon';

    const pollInterval = setInterval(() => {
      send({ type: 'recon_get', node_id: nodeId, agent_short_name: agentShortName });
    }, 1000);

    const timeout = setTimeout(() => {
      clearInterval(pollInterval);
      window.removeEventListener('ws-message', handleWsMessage);
      setIsLoading(false);
    }, 60000);

    const handleWsMessage = (event: Event) => {
      const message = (event as CustomEvent).detail;
      if (message.type === 'recon_get_response' &&
          message.node_id === nodeId &&
          message.agent_short_name === agentShortName) {
        if (message.recon_result && message.performed_at &&
            message.performed_at !== previousPerformedAt) {
          setReconResult(message.recon_result);
          setReconPerformedAt(message.performed_at);
          setReconIsSemantic(message.is_semantic);
          setIsLoading(false);
          clearInterval(pollInterval);
          clearTimeout(timeout);
          window.removeEventListener('ws-message', handleWsMessage);
        }
      }
    };

    window.addEventListener('ws-message', handleWsMessage);

    (async () => {
      try {
        const selectResp = await sendCommand(nodeId, {
          Agent: { Select: { short_name: agentShortName } },
        });
        if ('Error' in selectResp.result) throw new Error();
        const reconResp = await sendCommand(nodeId, { Agent: command });
        if ('Error' in reconResp.result) throw new Error();
      } catch {
        clearInterval(pollInterval);
        clearTimeout(timeout);
        window.removeEventListener('ws-message', handleWsMessage);
        setIsLoading(false);
      }
    })();

    setTimeout(() => {
      send({ type: 'recon_get', node_id: nodeId, agent_short_name: agentShortName });
    }, 500);
  };

  //
  // Config content fetching.
  //

  useEffect(() => {
    if (selectedConfigIdx === null || !reconResult?.config) {
      setConfigContent(null);
      setConfigContentError(null);
      return;
    }
    const configItem = reconResult.config[selectedConfigIdx];
    if (!configItem?.path || !nodeId) return;

    if (configItem.contents) {
      setConfigContent(configItem.contents);
      setConfigContentError(null);
      setIsLoadingConfigContent(false);
      return;
    }

    let isCancelled = false;
    setIsLoadingConfigContent(true);
    setConfigContent(null);

    sendCommand(nodeId, {
      Agent: { ReadFile: { file_type: 'Config', path: configItem.path } },
    }).then(response => {
      if (isCancelled) return;
      if ('Agent' in response.result &&
          typeof response.result.Agent === 'object' &&
          response.result.Agent !== null &&
          'ReadFileResult' in response.result.Agent) {
        const result = response.result.Agent.ReadFileResult;
        if (result.content) {
          setConfigContent(result.content);
          const updatedConfig = [...reconResult.config];
          updatedConfig[selectedConfigIdx] = { ...configItem, contents: result.content };
          setReconResult({ ...reconResult, config: updatedConfig });
        } else if (result.error) {
          setConfigContentError(result.error);
        }
      } else if ('Error' in response.result) {
        setConfigContentError((response.result as { Error: { message: string } }).Error.message);
      }
    }).catch(error => {
      if (!isCancelled) setConfigContentError(String(error));
    }).finally(() => {
      if (!isCancelled) setIsLoadingConfigContent(false);
    });

    return () => { isCancelled = true; };
  }, [selectedConfigIdx, reconResult?.config, nodeId, sendCommand]);

  //
  // Session content fetching.
  //

  useEffect(() => {
    if (selectedSessionIdx === null || !reconResult?.sessions) {
      setSessionContent(null);
      setSessionContentError(null);
      return;
    }
    const session = reconResult.sessions[selectedSessionIdx];
    if (!session?.session_file || !nodeId) return;

    let isCancelled = false;
    setIsLoadingSessionContent(true);
    setSessionContentError(null);
    setSessionContent(null);

    sendCommand(nodeId, {
      Agent: { ReadFile: { file_type: 'Session', path: session.session_file } },
    }).then(response => {
      if (isCancelled) return;
      if ('Agent' in response.result &&
          typeof response.result.Agent === 'object' &&
          response.result.Agent !== null &&
          'ReadFileResult' in response.result.Agent) {
        const result = response.result.Agent.ReadFileResult;
        if (result.content) setSessionContent(result.content);
        else if (result.error) setSessionContentError(result.error);
      } else if ('Error' in response.result) {
        setSessionContentError((response.result as { Error: { message: string } }).Error.message);
      }
    }).catch(error => {
      if (!isCancelled) setSessionContentError(String(error));
    }).finally(() => {
      if (!isCancelled) setIsLoadingSessionContent(false);
    });

    return () => { isCancelled = true; };
  }, [selectedSessionIdx, reconResult?.sessions, nodeId, sendCommand]);

  //
  // Config editing handlers.
  //

  const handleStartConfigEdit = (idx: number, content: string) => {
    setEditingConfigIdx(idx);
    setEditingConfigContent(content);
    setConfigSaveError(null);
    setSelectedConfigIdx(idx);
  };

  const handleCancelConfigEdit = () => {
    setEditingConfigIdx(null);
    setEditingConfigContent('');
    setConfigSaveError(null);
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

      if ('Agent' in response.result &&
          typeof response.result.Agent === 'object' &&
          response.result.Agent !== null &&
          'WriteFileResult' in response.result.Agent) {
        const result = response.result.Agent.WriteFileResult;
        if (result.success) {
          const updatedItems = [...reconResult.config];
          updatedItems[editingConfigIdx] = { ...item, contents: editingConfigContent };
          setReconResult({ ...reconResult, config: updatedItems });
          setConfigContent(editingConfigContent);
          setEditingConfigIdx(null);
          setEditingConfigContent('');
        } else {
          setConfigSaveError(result.error || 'Failed to save');
        }
      } else if ('Error' in response.result) {
        setConfigSaveError((response.result as { Error: { message: string } }).Error.message);
      }
    } catch (error) {
      setConfigSaveError(String(error));
    } finally {
      setIsSavingConfig(false);
    }
  };

  //
  // Relative time formatting for the timestamp.
  //

  const relativeTime = useMemo(() => {
    if (!reconPerformedAt) return null;
    const diff = Date.now() - new Date(reconPerformedAt).getTime();
    const mins = Math.floor(diff / 60000);
    if (mins < 1) return 'just now';
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    return `${Math.floor(hours / 24)}d ago`;
  }, [reconPerformedAt]);

  //
  // Derived data.
  //

  const allServers = reconResult?.tools.mcp_servers ?? [];
  const mcpCount = allServers.length;
  const skillsCount = reconResult?.tools.skills.length ?? 0;
  const internalCount = reconResult?.tools.internal_tools.length ?? 0;

  //
  // MCP servers grouped by context_path.
  //

  const serversByContext = useMemo(() => {
    const grouped = allServers.reduce((acc, server, idx) => {
      const key = server.context_path ?? 'Global';
      if (!acc[key]) acc[key] = [];
      acc[key].push({ server, idx });
      return acc;
    }, {} as Record<string, { server: typeof allServers[0]; idx: number }[]>);

    const contexts = Object.keys(grouped).sort((a, b) => {
      if (a === 'Global') return -1;
      if (b === 'Global') return 1;
      return a.localeCompare(b);
    });

    return { grouped, contexts };
  }, [allServers]);

  //
  // Config files grouped by directory.
  //

  const configByDir = useMemo(() => {
    if (!reconResult?.config) return { grouped: {} as Record<string, { item: ReconResult['config'][number]; idx: number; filename: string }[]>, dirs: [] as string[] };
    const grouped = reconResult.config.reduce((acc, item, idx) => {
      const parts = item.path.split('/');
      const filename = parts.pop() || item.path;
      const dir = parts.join('/') || '/';
      if (!acc[dir]) acc[dir] = [];
      acc[dir].push({ item, idx, filename });
      return acc;
    }, {} as Record<string, { item: typeof reconResult.config[0]; idx: number; filename: string }[]>);
    return { grouped, dirs: Object.keys(grouped).sort() };
  }, [reconResult?.config]);

  //
  // Counts for tab badges.
  //

  const configCount = reconResult?.config.length ?? 0;
  const sessionsCount = reconResult?.sessions?.length ?? 0;
  const toolsCount = mcpCount + skillsCount + internalCount;

  //
  // Auto-select first available tools category when switching to tools tab.
  //

  useEffect(() => {
    if (activeTab !== 'tools' || !reconResult) return;
    if (toolsCategory === 'mcp' && mcpCount === 0) {
      if (skillsCount > 0) setToolsCategory('skills');
      else if (internalCount > 0) setToolsCategory('internal');
    } else if (toolsCategory === 'skills' && skillsCount === 0) {
      if (mcpCount > 0) setToolsCategory('mcp');
      else if (internalCount > 0) setToolsCategory('internal');
    } else if (toolsCategory === 'internal' && internalCount === 0) {
      if (mcpCount > 0) setToolsCategory('mcp');
      else if (skillsCount > 0) setToolsCategory('skills');
    }
  }, [activeTab, reconResult, toolsCategory, mcpCount, skillsCount, internalCount]);

  //
  // Session content parsing.
  //

  type ParsedMessage = { type?: string; role?: string; content?: string; timestamp?: string };

  const parsedSessionMessages = useMemo((): ParsedMessage[] => {
    if (!sessionContent) return [];
    const lines = sessionContent.split('\n').filter(l => l.trim());
    if (lines.length === 0) return [];
    try {
      const firstParsed = JSON.parse(lines[0]);
      if (typeof firstParsed === 'object' && !Array.isArray(firstParsed)) {
        return lines.map(line => {
          try { return JSON.parse(line) as ParsedMessage; }
          catch { return { content: line }; }
        });
      }
    } catch {
      try {
        const parsed = JSON.parse(sessionContent);
        return parsed.messages || [];
      } catch {
        return [{ content: sessionContent }];
      }
    }
    return [{ content: sessionContent }];
  }, [sessionContent]);

  const tabs: { id: ReconTab; label: string; count: number }[] = [
    { id: 'config', label: 'Config', count: configCount },
    { id: 'tools', label: 'Tools', count: toolsCount },
    { id: 'sessions', label: 'Sessions', count: sessionsCount },
  ];

  return (
    <Modal
      isOpen={true}
      onClose={onClose}
      title={`Recon: ${agentShortName}`}
      size="xl"
      noPadding
      resizable
      storageKey="cmd-recon"
      defaultWidth={760}
      defaultHeight={Math.round(window.innerHeight * 0.7)}
    >
      <div className="flex flex-col h-full">

        {/*
        //
        // Header bar with tabs and actions.
        //
        */}

        <div className="flex items-center gap-1 px-2.5 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)] flex-shrink-0">
          <div className="flex items-center gap-0.5">
            {tabs.map(t => (
              <button
                key={t.id}
                onClick={() => setActiveTab(t.id)}
                className={`px-2 py-0.5 text-[10px] transition-colors ${
                  activeTab === t.id
                    ? 'bg-[var(--accent-info)]/20 text-[var(--accent-info)] border border-[var(--accent-info)]/50'
                    : 'text-muted hover:text-[var(--text-primary)] hover:bg-[var(--highlight)]'
                }`}
              >
                {t.label}
                {reconResult && <span className="ml-1 opacity-70">{t.count}</span>}
              </button>
            ))}
          </div>

          <div className="ml-auto flex items-center gap-1.5">
            {relativeTime && (
              <span className="text-[9px] text-muted flex items-center gap-1">
                {reconIsSemantic && <Sparkles size={9} className="text-[var(--accent-info)]" />}
                {relativeTime}
              </span>
            )}

            <button
              onClick={() => handleRecon(true)}
              disabled={isLoading}
              className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] bg-[var(--accent-info)]/20 text-[var(--accent-info)] border border-dim hover:border-[var(--accent-info)] transition-colors disabled:opacity-50"
              title="Semantic recon (AI-powered)"
            >
              <Search size={10} />
              Discover
            </button>

            <button
              onClick={() => handleRecon(false)}
              disabled={isLoading}
              className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] border border-dim hover:border-[var(--accent-purple)] transition-colors disabled:opacity-50"
              title="Standard recon"
            >
              <RefreshCw size={10} className={isLoading ? 'animate-spin' : ''} />
              Refresh
            </button>
          </div>
        </div>

        {/*
        //
        // Content area.
        //
        */}

        {isLoading && !reconResult ? (
          <div className="flex-1 flex items-center justify-center">
            <Loader2 size={20} className="animate-spin text-muted" />
            <span className="ml-2 text-muted text-[10px]">Loading recon data...</span>
          </div>
        ) : !reconResult ? (
          <div className="flex-1 flex items-center justify-center text-muted text-[10px]">
            Failed to load recon data
          </div>
        ) : (
          <div className="flex-1 overflow-hidden flex flex-col">

            {/*
            //
            // Config tab.
            //
            */}

            {activeTab === 'config' && (
              <div className="flex-1 flex flex-col min-h-0 overflow-hidden">

                {/*
                //
                // Metadata section (collapsible).
                //
                */}

                {(reconResult.metadata?.user_identities?.length || reconResult.metadata?.api_keys?.length) ? (
                  <div className="border-b border-subtle flex-shrink-0">
                    <button
                      onClick={() => setMetadataCollapsed(!metadataCollapsed)}
                      className="w-full px-2.5 py-1 flex items-center gap-1.5 hover:bg-[var(--bg-tertiary)] transition-colors"
                    >
                      {metadataCollapsed
                        ? <ChevronRight size={10} className="text-muted" />
                        : <ChevronDown size={10} className="text-muted" />
                      }
                      <Sparkles size={9} className="text-muted opacity-40" />
                      <span className="text-[10px] text-muted">Extracted Metadata</span>
                      <span className="text-[9px] text-muted ml-auto">
                        {(reconResult.metadata?.user_identities?.length || 0) + (reconResult.metadata?.api_keys?.length || 0)} items
                      </span>
                    </button>
                    {!metadataCollapsed && (
                      <div className="px-2.5 pb-1.5 text-[10px] border-t border-dim max-h-28 overflow-y-auto scrollbar-on-hover">
                        {reconResult.metadata?.user_identities?.length ? (
                          <div className="flex items-center gap-1.5 flex-wrap mt-1.5">
                            <span className="text-muted flex items-center gap-1">
                              <User size={10} /> Identities:
                            </span>
                            {reconResult.metadata.user_identities.map((id, idx) => (
                              <span key={idx} className="px-1.5 py-0.5 font-mono bg-[var(--accent-info)]/10 text-[var(--accent-info)]">{id}</span>
                            ))}
                          </div>
                        ) : null}
                        {reconResult.metadata?.api_keys?.length ? (
                          <div className="flex items-center gap-1.5 flex-wrap mt-1">
                            <span className="text-muted flex items-center gap-1">
                              <Key size={10} /> API Keys:
                            </span>
                            {reconResult.metadata.api_keys.map((key, idx) => (
                              <span key={idx} className="px-1.5 py-0.5 font-mono bg-[var(--accent-warning)]/10 text-[var(--accent-warning)]">{key}</span>
                            ))}
                          </div>
                        ) : null}
                      </div>
                    )}
                  </div>
                ) : null}

                {reconResult.config.length === 0 ? (
                  <div className="flex-1 flex items-center justify-center text-muted text-[10px]">
                    No config files discovered
                  </div>
                ) : (
                  <div className="flex-1 flex min-h-0 overflow-hidden">

                    {/*
                    //
                    // Left panel: file tree grouped by directory.
                    //
                    */}

                    <div className="w-48 flex-shrink-0 flex flex-col border-r border-subtle overflow-hidden bg-[var(--bg-secondary)]">
                      <div className="px-2 py-1 border-b border-subtle bg-[var(--bg-tertiary)]">
                        <span className="text-[9px] text-muted uppercase tracking-wider">
                          {configCount} file{configCount !== 1 ? 's' : ''}
                        </span>
                      </div>
                      <div className="flex-1 overflow-y-auto scrollbar-on-hover">
                        {configByDir.dirs.map(dir => {
                          const files = configByDir.grouped[dir];
                          const isExpanded = expandedConfigDirs.has(dir) || configByDir.dirs.length === 1;
                          const dirDisplay = dir.split('/').slice(-2).join('/') || dir;

                          return (
                            <div key={dir}>
                              <button
                                onClick={() => {
                                  setExpandedConfigDirs(prev => {
                                    const next = new Set(prev);
                                    if (next.has(dir)) next.delete(dir);
                                    else next.add(dir);
                                    return next;
                                  });
                                }}
                                className="w-full px-2 py-1 text-left flex items-center gap-1 hover:bg-[var(--bg-tertiary)] border-b border-dim"
                              >
                                <ChevronRight
                                  size={10}
                                  className={`text-muted transition-transform ${isExpanded ? 'rotate-90' : ''}`}
                                />
                                <FolderOpen size={10} className="text-muted" />
                                <span className="text-[9px] font-mono text-muted truncate" title={dir}>
                                  {dirDisplay}
                                </span>
                                <span className="text-[9px] text-muted ml-auto">{files.length}</span>
                              </button>

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
                                    className={`w-full pl-5 pr-2 py-1 text-left transition-colors border-b border-dim last:border-0 ${
                                      isSelected
                                        ? 'bg-[var(--accent-info)]/10 border-l-2 border-l-[var(--accent-info)]'
                                        : 'hover:bg-[var(--bg-tertiary)]'
                                    }`}
                                  >
                                    <div className="flex items-center gap-1">
                                      <FileText size={10} className={isSelected ? 'text-[var(--accent-info)]' : 'text-muted'} />
                                      <span className={`text-[10px] font-mono truncate ${isSelected ? 'text-[var(--accent-info)]' : ''}`}>
                                        {filename}
                                      </span>
                                    </div>
                                    <div className="mt-0.5 pl-3.5">
                                      <span className="text-[8px] px-1 py-px bg-[var(--bg-primary)] text-muted">
                                        {item.config_type}
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
                    // Right panel: config content viewer with edit/save.
                    //
                    */}

                    <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
                      {selectedConfigIdx === null ? (
                        <div className="flex-1 flex items-center justify-center bg-[var(--bg-secondary)]">
                          <div className="text-center text-muted">
                            <FolderOpen size={24} className="mx-auto mb-1 opacity-50" />
                            <p className="text-[10px]">Select a file to view</p>
                          </div>
                        </div>
                      ) : (
                        <>
                          <div className="px-2.5 py-1 border-b border-subtle bg-[var(--bg-tertiary)] flex items-center gap-2 flex-shrink-0">
                            <span className="font-mono text-[10px] truncate text-[var(--accent-info)] min-w-0">
                              {reconResult.config[selectedConfigIdx].path}
                            </span>
                            {editingConfigIdx !== selectedConfigIdx && !isLoadingConfigContent &&
                             (configContent || reconResult.config[selectedConfigIdx].contents) && (
                              <button
                                onClick={() => handleStartConfigEdit(selectedConfigIdx, configContent ?? reconResult.config[selectedConfigIdx].contents ?? '')}
                                className="p-0.5 text-muted hover:text-[var(--accent-info)] transition-colors flex-shrink-0 ml-auto"
                                title="Edit"
                              >
                                <Pencil size={11} />
                              </button>
                            )}
                          </div>

                          <div className="flex-1 overflow-auto bg-[var(--bg-secondary)]">
                            {editingConfigIdx === selectedConfigIdx ? (
                              <div className="h-full flex flex-col p-2">
                                <CodeEditor
                                  value={editingConfigContent}
                                  onChange={setEditingConfigContent}
                                  readOnly={isSavingConfig}
                                  language={languageFromPath(reconResult.config[selectedConfigIdx].path)}
                                />
                                {configSaveError && (
                                  <div className="mt-1 text-[10px] text-[var(--accent-error)]">{configSaveError}</div>
                                )}
                                <div className="flex justify-end gap-1.5 mt-2 flex-shrink-0">
                                  <button
                                    onClick={handleCancelConfigEdit}
                                    disabled={isSavingConfig}
                                    className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted hover:text-[var(--text-primary)] transition-colors disabled:opacity-50"
                                  >
                                    <X size={10} /> Cancel
                                  </button>
                                  <button
                                    onClick={handleSaveConfig}
                                    disabled={isSavingConfig}
                                    className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors disabled:opacity-50"
                                  >
                                    {isSavingConfig ? <Loader2 size={10} className="animate-spin" /> : <Save size={10} />}
                                    {isSavingConfig ? 'Saving...' : 'Save'}
                                  </button>
                                </div>
                              </div>
                            ) : isLoadingConfigContent ? (
                              <div className="flex-1 flex items-center justify-center py-8">
                                <Loader2 size={16} className="animate-spin text-muted" />
                              </div>
                            ) : configContentError ? (
                              <div className="p-2.5 text-[var(--accent-error)] text-[10px]">
                                Error: {configContentError}
                              </div>
                            ) : (
                              <pre className="p-2.5 text-[10px] font-mono whitespace-pre-wrap text-muted">
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
            // Tools tab.
            //
            */}

            {activeTab === 'tools' && (
              <div className="flex-1 flex min-h-0 overflow-hidden">

                {/*
                //
                // Left sidebar: category buttons.
                //
                */}

                <div className="w-28 flex-shrink-0 flex flex-col border-r border-subtle bg-[var(--bg-secondary)]">
                  {[
                    { id: 'mcp' as const, label: 'MCP', icon: Wrench, count: mcpCount },
                    { id: 'skills' as const, label: 'Skills', icon: Sparkles, count: skillsCount },
                    { id: 'internal' as const, label: 'Internal', icon: Cpu, count: internalCount },
                  ].map(cat => (
                    <button
                      key={cat.id}
                      onClick={() => setToolsCategory(cat.id)}
                      disabled={cat.count === 0}
                      className={`w-full px-2 py-1.5 text-left flex items-center gap-1.5 transition-colors border-b border-dim ${
                        toolsCategory === cat.id
                          ? 'bg-[var(--accent-info)]/10 border-l-2 border-l-[var(--accent-info)] text-[var(--accent-info)]'
                          : cat.count === 0
                          ? 'text-muted opacity-40'
                          : 'text-muted hover:text-[var(--text-primary)] hover:bg-[var(--highlight)]'
                      }`}
                    >
                      <cat.icon size={11} />
                      <span className="text-[10px]">{cat.label}</span>
                      <span className="text-[9px] opacity-70 ml-auto">{cat.count}</span>
                    </button>
                  ))}
                </div>

                {/*
                //
                // Content area.
                //
                */}

                <div className="flex-1 flex flex-col min-w-0 overflow-hidden">

                  {/*
                  //
                  // MCP: server list grouped by context → tool grid.
                  //
                  */}

                  {toolsCategory === 'mcp' && (
                    mcpCount === 0 ? (
                      <div className="flex-1 flex items-center justify-center text-muted text-[10px]">No MCP servers</div>
                    ) : (
                      <div className="flex-1 flex min-h-0 overflow-hidden">

                        {/*
                        //
                        // Server list.
                        //
                        */}

                        <div className="w-44 flex-shrink-0 flex flex-col border-r border-subtle bg-[var(--bg-secondary)] overflow-hidden">
                          <div className="px-2 py-1 border-b border-subtle bg-[var(--bg-tertiary)]">
                            <span className="text-[9px] text-muted uppercase tracking-wider">
                              {mcpCount} server{mcpCount !== 1 ? 's' : ''}
                            </span>
                          </div>
                          <div className="flex-1 overflow-y-auto scrollbar-on-hover">
                            {serversByContext.contexts.map(context => {
                              const servers = serversByContext.grouped[context];
                              const isGlobal = context === 'Global';
                              const contextDisplay = isGlobal ? 'Global' : context.split('/').slice(-2).join('/');
                              const isExpanded = expandedMcpContexts.has(context);

                              return (
                                <div key={context}>
                                  <button
                                    onClick={() => {
                                      setExpandedMcpContexts(prev => {
                                        const next = new Set(prev);
                                        if (next.has(context)) next.delete(context);
                                        else next.add(context);
                                        return next;
                                      });
                                    }}
                                    className="w-full px-2 py-1 bg-[var(--bg-tertiary)] border-b border-dim flex items-center gap-1 hover:bg-[var(--highlight)] transition-colors"
                                  >
                                    <ChevronRight
                                      size={10}
                                      className={`text-muted transition-transform ${isExpanded ? 'rotate-90' : ''}`}
                                    />
                                    {isGlobal
                                      ? <Settings size={9} className="text-muted" />
                                      : <FolderOpen size={9} className="text-muted" />
                                    }
                                    <span className="text-[9px] font-mono text-muted truncate" title={context}>
                                      {contextDisplay}
                                    </span>
                                    <span className="text-[9px] text-muted ml-auto">{servers.length}</span>
                                  </button>

                                  {isExpanded && servers.map(({ server, idx }) => {
                                    const isSelected = selectedServerIdx === idx;
                                    return (
                                      <button
                                        key={`${server.name}-${idx}`}
                                        onClick={() => setSelectedServerIdx(idx)}
                                        className={`w-full pl-5 pr-2 py-1 text-left transition-colors border-b border-dim last:border-0 ${
                                          isSelected
                                            ? 'bg-[var(--accent-info)]/10 border-l-2 border-l-[var(--accent-info)]'
                                            : 'hover:bg-[var(--bg-tertiary)]'
                                        }`}
                                      >
                                        <div className="flex items-center gap-1">
                                          <Wrench size={10} className={isSelected ? 'text-[var(--accent-info)]' : 'text-muted'} />
                                          <span className={`text-[10px] font-medium truncate ${isSelected ? 'text-[var(--accent-info)]' : ''}`}>
                                            {server.name}
                                          </span>
                                          <span className="text-[9px] text-muted ml-auto">{server.tools.length}</span>
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
                        // Tool grid.
                        //
                        */}

                        <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
                          {selectedServerIdx === null ? (
                            <div className="flex-1 flex items-center justify-center bg-[var(--bg-secondary)]">
                              <div className="text-center text-muted">
                                <Wrench size={24} className="mx-auto mb-1 opacity-50" />
                                <p className="text-[10px]">Select a server</p>
                              </div>
                            </div>
                          ) : (
                            <>
                              <div className="px-2.5 py-1 border-b border-subtle bg-[var(--bg-tertiary)] flex-shrink-0">
                                <div className="flex items-center gap-1.5">
                                  <Wrench size={11} className="text-[var(--accent-info)]" />
                                  <span className="text-[10px] font-medium text-[var(--accent-info)]">
                                    {allServers[selectedServerIdx].name}
                                  </span>
                                  <span className="text-[9px] px-1 py-px bg-[var(--bg-primary)] text-muted">
                                    {allServers[selectedServerIdx].transport}
                                  </span>
                                  <span className="text-[9px] text-muted ml-auto">
                                    {allServers[selectedServerIdx].tools.length} tool{allServers[selectedServerIdx].tools.length !== 1 ? 's' : ''}
                                  </span>
                                </div>
                                {(allServers[selectedServerIdx].command || allServers[selectedServerIdx].address) && (
                                  <div className="mt-0.5 text-[9px] font-mono text-muted truncate">
                                    {allServers[selectedServerIdx].command || allServers[selectedServerIdx].address}
                                  </div>
                                )}
                              </div>
                              <div className="flex-1 overflow-y-auto bg-[var(--bg-secondary)] p-2 scrollbar-on-hover">
                                <div className="grid grid-cols-2 gap-1">
                                  {allServers[selectedServerIdx].tools.map(tool => (
                                    <div
                                      key={tool.name}
                                      className="p-1.5 bg-[var(--bg-primary)] border border-subtle hover:border-[var(--accent-info)]/50 transition-colors"
                                    >
                                      <p className="font-mono text-[10px] text-[var(--accent-info)] truncate" title={tool.name}>
                                        {tool.name}
                                      </p>
                                      <p className="text-[9px] text-muted mt-0.5 line-clamp-2" title={tool.description}>
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
                    )
                  )}

                  {/*
                  //
                  // Skills list.
                  //
                  */}

                  {toolsCategory === 'skills' && (
                    skillsCount === 0 ? (
                      <div className="flex-1 flex items-center justify-center text-muted text-[10px]">No skills</div>
                    ) : (
                      <div className="flex-1 overflow-y-auto p-2 scrollbar-on-hover">
                        <div className="space-y-1">
                          {reconResult!.tools.skills.map(skill => (
                            <div key={skill.name} className="p-1.5 border border-subtle">
                              <div className="flex items-center gap-1.5">
                                <Sparkles size={10} className="text-[var(--accent-info)]" />
                                <span className="text-[10px] font-medium">{skill.name}</span>
                              </div>
                              {skill.description && (
                                <p className="text-[9px] text-muted mt-0.5 pl-4">{skill.description}</p>
                              )}
                            </div>
                          ))}
                        </div>
                      </div>
                    )
                  )}

                  {/*
                  //
                  // Internal tools list.
                  //
                  */}

                  {toolsCategory === 'internal' && (
                    internalCount === 0 ? (
                      <div className="flex-1 flex items-center justify-center text-muted text-[10px]">No internal tools</div>
                    ) : (
                      <div className="flex-1 overflow-y-auto p-2 scrollbar-on-hover">
                        <div className="space-y-1">
                          {reconResult!.tools.internal_tools.map(tool => (
                            <div key={tool.name} className="p-1.5 border border-subtle">
                              <div className="flex items-center gap-1.5">
                                <Cpu size={10} className="text-[var(--accent-purple)]" />
                                <span className="text-[10px] font-medium">{tool.name}</span>
                              </div>
                              {tool.description && (
                                <p className="text-[9px] text-muted mt-0.5 pl-4">{tool.description}</p>
                              )}
                            </div>
                          ))}
                        </div>
                      </div>
                    )
                  )}
                </div>
              </div>
            )}

            {/*
            //
            // Sessions tab.
            //
            */}

            {activeTab === 'sessions' && (
              <div className="flex-1 flex min-h-0 overflow-hidden">
                {!reconResult.sessions || reconResult.sessions.length === 0 ? (
                  <div className="flex-1 flex items-center justify-center text-muted text-[10px]">
                    No sessions discovered
                  </div>
                ) : (
                  <>

                    {/*
                    //
                    // Left panel: session list.
                    //
                    */}

                    <div className="w-48 flex-shrink-0 flex flex-col border-r border-subtle overflow-hidden bg-[var(--bg-secondary)]">
                      <div className="px-2 py-1 border-b border-subtle bg-[var(--bg-tertiary)]">
                        <span className="text-[9px] text-muted uppercase tracking-wider">
                          {sessionsCount} session{sessionsCount !== 1 ? 's' : ''}
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
                              className={`w-full px-2 py-1 text-left transition-colors border-b border-dim last:border-0 ${
                                isSelected
                                  ? 'bg-[var(--accent-info)]/10 border-l-2 border-l-[var(--accent-info)]'
                                  : 'hover:bg-[var(--bg-tertiary)]'
                              }`}
                            >
                              <div className="flex items-center gap-1.5">
                                <History size={10} className={isSelected ? 'text-[var(--accent-info)]' : 'text-muted'} />
                                <span className={`text-[10px] font-mono ${isSelected ? 'text-[var(--accent-info)]' : ''}`}>
                                  {shortId}
                                </span>
                                <span className="text-[9px] text-muted ml-auto">
                                  {session.message_count}
                                </span>
                              </div>
                              {session.last_modified && (
                                <div className="mt-0.5 text-[9px] text-muted truncate pl-4">
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
                    // Right panel: parsed session content.
                    //
                    */}

                    <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
                      {selectedSessionIdx === null ? (
                        <div className="flex-1 flex items-center justify-center text-muted text-[10px]">
                          Select a session to view
                        </div>
                      ) : isLoadingSessionContent ? (
                        <div className="flex-1 flex items-center justify-center">
                          <Loader2 size={16} className="animate-spin text-muted" />
                        </div>
                      ) : sessionContentError ? (
                        <div className="flex-1 flex items-center justify-center p-2.5">
                          <div className="text-center">
                            <X size={16} className="mx-auto mb-1 text-[var(--accent-danger)]" />
                            <p className="text-[var(--accent-danger)] text-[10px]">{sessionContentError}</p>
                          </div>
                        </div>
                      ) : (() => {
                        const session = reconResult.sessions[selectedSessionIdx];
                        return (
                          <>
                            <div className="px-2.5 py-1 border-b border-subtle bg-[var(--bg-tertiary)] flex items-center justify-between flex-shrink-0">
                              <span className="text-[10px] font-mono text-muted truncate">
                                {session.session_id}
                              </span>
                              <span className="text-[9px] text-muted">
                                {parsedSessionMessages.length} entries
                              </span>
                            </div>
                            <div className="flex-1 overflow-y-auto scrollbar-on-hover">
                              {parsedSessionMessages.length === 0 ? (
                                <div className="p-2.5 text-muted text-[10px]">No content</div>
                              ) : (
                                <div className="p-2 space-y-1.5">
                                  {parsedSessionMessages.map((msg, idx) => {
                                    const msgType = msg.type || msg.role || 'unknown';
                                    const isUser = msgType === 'user' || msgType === 'human';
                                    const isAssistant = msgType === 'assistant' || msgType === 'gemini' || msgType === 'model';
                                    return (
                                      <div
                                        key={idx}
                                        className={`p-1.5 text-[10px] ${
                                          isUser
                                            ? 'bg-[var(--accent-info)]/10 border-l-2 border-l-[var(--accent-info)]'
                                            : isAssistant
                                            ? 'bg-[var(--bg-secondary)] border-l-2 border-l-[var(--accent-purple)]'
                                            : 'bg-[var(--bg-tertiary)] border-l-2 border-l-[var(--border-subtle)]'
                                        }`}
                                      >
                                        <div className="flex items-center gap-1.5 mb-0.5">
                                          <span className={`text-[9px] font-medium uppercase ${
                                            isUser ? 'text-[var(--accent-info)]' :
                                            isAssistant ? 'text-[var(--accent-purple)]' :
                                            'text-muted'
                                          }`}>
                                            {msgType}
                                          </span>
                                          {msg.timestamp && (
                                            <span className="text-[8px] text-muted">
                                              {new Date(msg.timestamp).toLocaleString()}
                                            </span>
                                          )}
                                        </div>
                                        <div className="whitespace-pre-wrap break-words font-mono text-[10px]">
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
                  </>
                )}
              </div>
            )}
          </div>
        )}
      </div>
    </Modal>
  );
}
