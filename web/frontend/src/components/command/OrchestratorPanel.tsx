import { useState, useRef, useEffect, useCallback, useMemo } from 'react';
import {
  Bot,
  Send,
  Loader2,
  PlayCircle,
  Square,
  AlertCircle,
  Download,
  PanelRightClose,
  Plus,
  X,
} from 'lucide-react';
import { useApp } from '../../context/AppContext';
import {
  ChatMessage,
  StreamingMessage,
  PlanDisplay,
} from '../orchestrator/OrchestratorChat';
import { exportOrchestratorSession, downloadTextFile } from '../../utils/export';

const MIN_WIDTH = 280;
const MAX_WIDTH = 800;
const DEFAULT_WIDTH = 380;
const PANEL_WIDTH_KEY = 'commandCenter.orchestratorWidth';

interface ModelDef {
  name: string;
  provider: string;
  model: string;
}

interface OrchestratorPanelProps {
  isOpen: boolean;
  onToggle: () => void;
}

export function OrchestratorPanel({ isOpen, onToggle }: OrchestratorPanelProps) {
  const {
    state,
    orchestratorCreateSession,
    orchestratorCloseSession,
    orchestratorCancelPrompt,
    orchestratorSendPrompt,
    orchestratorSetActiveSession,
    orchestratorClearMessages,
    getConfig,
    setConfig,
  } = useApp();
  const { orchestrator } = state;
  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  //
  // Active session derived from state.
  //
  const activeSession = useMemo(
    () => orchestrator.sessions.find(s => s.sessionId === orchestrator.activeSessionId) ?? null,
    [orchestrator.sessions, orchestrator.activeSessionId]
  );

  //
  // Panel width with drag-to-resize. Persisted in localStorage.
  //
  const [panelWidth, setPanelWidth] = useState(() => {
    const stored = localStorage.getItem(PANEL_WIDTH_KEY);
    return stored ? Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, parseInt(stored, 10))) : DEFAULT_WIDTH;
  });
  const [isResizing, setIsResizing] = useState(false);
  const dragStartX = useRef(0);
  const dragStartWidth = useRef(0);

  const handleDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsResizing(true);
    dragStartX.current = e.clientX;
    dragStartWidth.current = panelWidth;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }, [panelWidth]);

  useEffect(() => {
    if (!isResizing) return;

    const handleMouseMove = (e: MouseEvent) => {
      const delta = dragStartX.current - e.clientX;
      const newWidth = Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, dragStartWidth.current + delta));
      setPanelWidth(newWidth);
    };

    const handleMouseUp = () => {
      setIsResizing(false);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isResizing]);

  //
  // Persist width after drag ends.
  //
  useEffect(() => {
    if (!isResizing) {
      localStorage.setItem(PANEL_WIDTH_KEY, String(panelWidth));
    }
  }, [isResizing, panelWidth]);

  //
  // Fetch config on mount.
  //
  useEffect(() => {
    if (!state.connected) return;
    getConfig(['llm_feature_orchestrator', 'llm_model_definitions']);
  }, [state.connected, getConfig]);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [activeSession?.messages, activeSession?.streamingContent, activeSession?.currentToolExecutions]);

  useEffect(() => {
    if (activeSession && !activeSession.isLoading && isOpen) {
      inputRef.current?.focus();
    }
  }, [activeSession?.isLoading, isOpen, activeSession]);

  const handleSendMessage = () => {
    if (!input.trim() || !activeSession || activeSession.isLoading) return;
    if (input.trim() === '/clear') {
      orchestratorClearMessages(activeSession.sessionId);
      setInput('');
      return;
    }
    orchestratorSendPrompt(activeSession.sessionId, input.trim());
    setInput('');
  };

  const handleExport = () => {
    if (!activeSession || activeSession.messages.length === 0) return;
    const content = exportOrchestratorSession(activeSession.messages, activeSession.tokenUsage);
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    downloadTextFile(content, `orchestrator-session-${timestamp}.md`);
  };

  //
  // Parse model definitions from config.
  //
  const modelDefs: ModelDef[] = useMemo(() => {
    const raw = state.config.llm_model_definitions;
    if (!raw) return [];
    try {
      const defs = JSON.parse(raw);
      return Array.isArray(defs) ? defs : [];
    } catch {
      return [];
    }
  }, [state.config.llm_model_definitions]);

  const selectedModelName = state.config.llm_feature_orchestrator || '';
  const isConfigured = modelDefs.some(d => d.name === selectedModelName);

  const handleModelChange = (name: string) => {
    setConfig({ llm_feature_orchestrator: name });
  };

  //
  // Model for the active session, resolved from session state or config default.
  //
  const activeSessionModelName = useMemo(() => {
    if (!activeSession?.provider || !activeSession?.model) return selectedModelName;
    return modelDefs.find(
      d => d.provider === activeSession.provider && d.model === activeSession.model
    )?.name ?? selectedModelName;
  }, [activeSession, modelDefs, selectedModelName]);

  const handleSessionModelChange = (name: string) => {
    if (!activeSession) return;
    orchestratorCloseSession(activeSession.sessionId);
    orchestratorCreateSession(name);
  };

  const handleCreateSession = () => {
    if (!isConfigured && modelDefs.length > 0) {
      setConfig({ llm_feature_orchestrator: modelDefs[0].name });
    }
    orchestratorCreateSession();
  };

  const hasSessions = orchestrator.sessions.length > 0;

  return (
    <div
      className={`flex-shrink-0 flex ${
        isResizing ? '' : 'transition-all duration-200'
      } ${
        isOpen ? '' : 'w-0 overflow-hidden'
      }`}
      style={isOpen ? { width: panelWidth } : undefined}
    >
      {isOpen && (
        <>
          {/*
          //
          // Drag handle for resizing.
          //
          */}
          <div
            onMouseDown={handleDragStart}
            className="w-1 cursor-col-resize bg-transparent hover:bg-[var(--accent-info)]/30 active:bg-[var(--accent-info)]/50 transition-colors flex-shrink-0 border-l border-subtle"
          />

          <div className="flex-1 flex flex-col min-w-0 bg-[var(--bg-secondary)]">
            {/*
            //
            // Panel header.
            //
            */}
            <div className="px-3 py-2 border-b border-subtle flex items-center justify-between flex-shrink-0">
              <div className="flex items-center gap-2">
                <Bot size={14} className="text-[var(--accent-purple)]" />
                <span className="text-xs font-medium text-highlight">Orchestrator</span>
                {hasSessions && (
                  <span className="text-[9px] px-1.5 py-0.5 bg-[var(--accent-success)]/20 text-[var(--accent-success)]">
                    {orchestrator.sessions.length} session{orchestrator.sessions.length !== 1 ? 's' : ''}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-1">
                <button
                  onClick={handleExport}
                  disabled={!activeSession || activeSession.messages.length === 0}
                  className="p-1 text-muted hover:text-[var(--text-primary)] transition-colors disabled:opacity-30"
                  title="Export transcript"
                >
                  <Download size={12} />
                </button>
                <button
                  onClick={handleCreateSession}
                  disabled={!isConfigured || orchestrator.isStarting}
                  className="p-1 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/20 transition-colors disabled:opacity-30"
                  title="New session"
                >
                  {orchestrator.isStarting
                    ? <Loader2 size={12} className="animate-spin" />
                    : <PlayCircle size={12} />}
                </button>
                <button
                  onClick={onToggle}
                  className="p-1 text-muted hover:text-[var(--text-primary)] transition-colors"
                  title="Close panel"
                >
                  <PanelRightClose size={12} />
                </button>
              </div>
            </div>

            {/*
            //
            // Session tab bar.
            //
            */}
            {hasSessions && (
              <div className="flex border-b border-subtle bg-[var(--bg-primary)] overflow-x-auto flex-shrink-0">
                {orchestrator.sessions.map((session) => {
                  const isActive = session.sessionId === orchestrator.activeSessionId;
                  return (
                    <button
                      key={session.sessionId}
                      onClick={() => orchestratorSetActiveSession(session.sessionId)}
                      className={`group flex items-center gap-1 px-3 py-1 text-[10px] whitespace-nowrap border-b-2 transition-colors ${
                        isActive
                          ? 'bg-[var(--bg-secondary)] text-highlight border-[var(--accent-purple)]'
                          : 'text-muted hover:text-highlight border-transparent hover:border-[var(--accent-purple)]/30'
                      }`}
                    >
                      <span className="truncate">{session.label}</span>
                      {session.isLoading && <Loader2 size={8} className="animate-spin flex-shrink-0" />}
                      <span
                        onClick={(e) => {
                          e.stopPropagation();
                          orchestratorCloseSession(session.sessionId);
                        }}
                        className="ml-1 opacity-0 group-hover:opacity-100 hover:text-[var(--accent-error)] transition-opacity cursor-pointer"
                      >
                        <X size={10} />
                      </span>
                    </button>
                  );
                })}
                <button
                  onClick={handleCreateSession}
                  disabled={!isConfigured || orchestrator.isStarting}
                  className="px-2 py-1 text-muted hover:text-highlight transition-colors disabled:opacity-30 flex-shrink-0"
                  title="New session"
                >
                  <Plus size={12} />
                </button>
              </div>
            )}

            {/*
            //
            // Model selector -- always visible when models are defined.
            //
            */}
            {modelDefs.length > 0 && (
              <div className="px-3 py-1.5 border-b border-subtle flex items-center gap-2 flex-shrink-0">
                <span className="text-[9px] text-muted tracking-wider">MODEL</span>
                <select
                  value={activeSession ? activeSessionModelName : selectedModelName}
                  onChange={e => {
                    if (activeSession) {
                      handleSessionModelChange(e.target.value);
                    } else {
                      handleModelChange(e.target.value);
                    }
                  }}
                  className="flex-1 bg-[var(--bg-primary)] border border-subtle px-1.5 py-0.5 text-[10px] text-highlight focus:outline-none focus:border-[var(--border-active)] truncate"
                >
                  {!selectedModelName && !activeSession && <option value="">Select model...</option>}
                  {modelDefs.map(d => (
                    <option key={d.name} value={d.name}>{d.name}</option>
                  ))}
                </select>
              </div>
            )}

            {/*
            //
            // Not configured warning.
            //
            */}
            {!isConfigured && modelDefs.length === 0 && (
              <div className="px-3 py-2 bg-[var(--accent-warning)]/10 border-b border-[var(--accent-warning)]/30 flex items-start gap-2">
                <AlertCircle size={12} className="text-[var(--accent-warning)] mt-0.5 flex-shrink-0" />
                <p className="text-[10px] text-[var(--accent-warning)]">
                  No models configured. Go to Settings.
                </p>
              </div>
            )}

            {/*
            //
            // Messages area.
            //
            */}
            <div className="flex-1 overflow-auto p-2 space-y-2">
              {activeSession ? (
                <>
                  {activeSession.messages.map(msg => (
                    <ChatMessage key={msg.id} message={msg} compact />
                  ))}

                  {activeSession.isLoading && (
                    <StreamingMessage
                      content={activeSession.streamingContent}
                      toolExecutions={activeSession.currentToolExecutions}
                      compact
                    />
                  )}
                </>
              ) : (
                <div className="flex items-center justify-center h-full text-[10px] text-muted">
                  {hasSessions ? 'Select a session' : 'Create a session to get started'}
                </div>
              )}

              <div ref={messagesEndRef} />
            </div>

            {/*
            //
            // Plan display -- compact bar above input.
            //
            */}
            {activeSession?.currentPlan && activeSession.currentPlan.steps.length > 0 && (
              <PlanDisplay plan={activeSession.currentPlan} compact />
            )}

            {/*
            //
            // Token usage footer.
            //
            */}
            {activeSession?.tokenUsage && (
              <div className="px-3 py-1 border-t border-subtle text-[9px] text-muted flex-shrink-0">
                {activeSession.tokenUsage.totalTokens.toLocaleString()} tokens
              </div>
            )}

            {/*
            //
            // Input.
            //
            */}
            <div className="px-2 py-2 border-t border-subtle flex-shrink-0">
              <div className="flex gap-1">
                <input
                  ref={inputRef}
                  type="text"
                  value={input}
                  onChange={e => setInput(e.target.value)}
                  onKeyDown={e => e.key === 'Enter' && !e.shiftKey && handleSendMessage()}
                  placeholder={activeSession ? 'Ask...' : 'Create a session first'}
                  className="flex-1 bg-[var(--bg-primary)] border border-subtle px-2 py-1.5 text-xs text-[var(--text-primary)] placeholder-[var(--text-secondary)] focus:outline-none focus:border-[var(--border-active)]"
                  disabled={!activeSession || activeSession.isLoading}
                />
                {activeSession?.isLoading ? (
                  <button
                    onClick={() => orchestratorCancelPrompt(activeSession.sessionId)}
                    className="px-2 py-1.5 bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors"
                    title="Stop generation"
                  >
                    <Square size={14} />
                  </button>
                ) : (
                  <button
                    onClick={handleSendMessage}
                    disabled={!input.trim() || !activeSession}
                    className="px-2 py-1.5 bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors disabled:opacity-30"
                  >
                    <Send size={14} />
                  </button>
                )}
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
