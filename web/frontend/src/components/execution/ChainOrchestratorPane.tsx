import { useRef, useEffect, useState } from 'react';
import {
  Send,
  Loader2,
  Sparkles,
  PlayCircle,
  StopCircle,
  Square,
  PanelRightClose,
  PanelRightOpen,
  AlertCircle,
} from 'lucide-react';
import { useApp } from '../../context/AppContext';
import type { ChainOrchMode } from '../../api/types';
import { ChatMessage } from '../common/ChatMessage';
import { StreamingMessage } from '../common/StreamingMessage';
import { PlanDisplay } from '../common/PlanDisplay';

interface ChainOrchestratorPaneProps {
  collapsed: boolean;
  onToggleCollapsed: () => void;
  workspaceContext: string;
}

const MODE_LABELS: Record<ChainOrchMode, string> = {
  build: 'Build',
  execute: 'Execute',
};

const MODE_COLORS: Record<ChainOrchMode, string> = {
  build: 'var(--accent-purple)',
  execute: 'var(--accent-warning)',
};

export function ChainOrchestratorPane({ collapsed, onToggleCollapsed, workspaceContext }: ChainOrchestratorPaneProps) {
  const { state, chainOrchStart, chainOrchStop, chainOrchCancel, chainOrchPrompt, chainOrchSetMode, getConfig } = useApp();
  const { chainOrchestrator } = state;
  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!state.connected) return;
    getConfig(['llm_feature_orchestrator', 'llm_model_definitions']);
  }, [state.connected, getConfig]);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [chainOrchestrator.messages, chainOrchestrator.streamingContent, chainOrchestrator.currentToolExecutions]);

  useEffect(() => {
    if (!chainOrchestrator.isLoading && chainOrchestrator.sessionActive) {
      inputRef.current?.focus();
    }
  }, [chainOrchestrator.isLoading, chainOrchestrator.sessionActive]);

  const handleSendMessage = () => {
    if (!input.trim() || chainOrchestrator.isLoading) return;
    chainOrchPrompt(input.trim(), workspaceContext);
    setInput('');
  };

  const isConfigured = (() => {
    const selectedModelName = state.config.llm_feature_orchestrator;
    if (!selectedModelName) return false;
    const modelDefsRaw = state.config.llm_model_definitions;
    if (!modelDefsRaw) return false;
    try {
      const defs = JSON.parse(modelDefsRaw) as Array<{ name: string }>;
      return defs.some((d) => d.name === selectedModelName);
    } catch {
      return false;
    }
  })();

  if (collapsed) {
    return (
      <div className="w-10 border-l border-subtle bg-[var(--bg-secondary)] flex flex-col items-center py-3 gap-3">
        <button
          onClick={onToggleCollapsed}
          className="text-muted hover:text-title transition-colors"
          title="Expand Chain Orchestrator"
        >
          <PanelRightOpen size={16} />
        </button>
        <div className="writing-mode-vertical text-[10px] tracking-widest text-muted rotate-180" style={{ writingMode: 'vertical-rl' }}>
          CHAIN ORCHESTRATOR
        </div>
        {chainOrchestrator.sessionActive && (
          <div className="w-2 h-2 rounded-full bg-[var(--accent-success)] animate-pulse" />
        )}
      </div>
    );
  }

  return (
    <div className="w-[400px] border-l border-subtle bg-[var(--bg-primary)] flex flex-col">
      {/*
      //
      // Header.
      //
      */}
      <div className="px-3 py-2 border-b border-subtle bg-[var(--bg-secondary)] flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Sparkles size={14} className="text-[var(--accent-purple)]" />
          <span className="text-xs font-medium text-title">Chain Orchestrator</span>
        </div>
        <div className="flex items-center gap-2">
          {chainOrchestrator.sessionActive ? (
            <button
              onClick={chainOrchStop}
              className="flex items-center gap-1 px-2 py-1 text-[10px] bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors"
            >
              <StopCircle size={10} />
              Stop
            </button>
          ) : (
            <button
              onClick={chainOrchStart}
              disabled={!isConfigured || chainOrchestrator.isStarting}
              className="flex items-center gap-1 px-2 py-1 text-[10px] bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {chainOrchestrator.isStarting ? (
                <Loader2 size={10} className="animate-spin" />
              ) : (
                <PlayCircle size={10} />
              )}
              Start
            </button>
          )}
          <button
            onClick={onToggleCollapsed}
            className="text-muted hover:text-title transition-colors"
            title="Collapse"
          >
            <PanelRightClose size={14} />
          </button>
        </div>
      </div>

      {/*
      //
      // Mode selector.
      //
      */}
      {chainOrchestrator.sessionActive && (
        <div className="px-3 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)] flex items-center gap-1">
          {(['build', 'execute'] as ChainOrchMode[]).map((m) => (
            <button
              key={m}
              onClick={() => chainOrchSetMode(m)}
              className={`flex-1 px-2 py-1.5 text-[10px] tracking-wider transition-colors cursor-pointer ${
                chainOrchestrator.mode === m
                  ? 'border-b-2'
                  : 'text-muted hover:text-title hover:bg-[var(--bg-secondary)]'
              }`}
              style={chainOrchestrator.mode === m ? { color: MODE_COLORS[m], borderColor: MODE_COLORS[m], backgroundColor: `color-mix(in srgb, ${MODE_COLORS[m]} 15%, transparent)` } : undefined}
            >
              {MODE_LABELS[m]}
            </button>
          ))}
        </div>
      )}

      {/*
      //
      // Not configured warning.
      //
      */}
      {!isConfigured && !chainOrchestrator.sessionActive && (
        <div className="m-3 p-2 bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/30 flex items-start gap-2">
          <AlertCircle size={14} className="text-[var(--accent-warning)] mt-0.5 flex-shrink-0" />
          <div>
            <p className="text-[10px] font-medium text-[var(--accent-warning)]">Not Configured</p>
            <p className="text-[10px] text-muted mt-0.5">
              Go to Settings to configure an LLM provider.
            </p>
          </div>
        </div>
      )}

      {/*
      //
      // Plan display.
      //
      */}
      {chainOrchestrator.currentPlan && chainOrchestrator.currentPlan.steps.length > 0 && (
        <div className="px-3 pt-2">
          <PlanDisplay plan={chainOrchestrator.currentPlan} />
        </div>
      )}

      {/*
      //
      // Messages.
      //
      */}
      <div className="flex-1 overflow-auto p-2 space-y-2 min-h-0">
        {chainOrchestrator.messages.map((msg) => (
          <ChatMessage key={msg.id} message={msg} agentLabel="Chain Orchestrator" compact />
        ))}

        {chainOrchestrator.isLoading && (
          <StreamingMessage
            content={chainOrchestrator.streamingContent}
            toolExecutions={chainOrchestrator.currentToolExecutions}
            agentLabel="Chain Orchestrator"
            compact
          />
        )}

        <div ref={messagesEndRef} />
      </div>

      {/*
      //
      // Token usage.
      //
      */}
      {chainOrchestrator.tokenUsage && (
        <div className="px-3 py-1 border-t border-subtle text-[10px] text-muted">
          {chainOrchestrator.tokenUsage.totalTokens.toLocaleString()} tokens
        </div>
      )}

      {/*
      //
      // Input bar.
      //
      */}
      <div className="p-2 border-t border-subtle">
        <div className="flex gap-1.5">
          <input
            ref={inputRef}
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && !e.shiftKey && handleSendMessage()}
            placeholder={
              chainOrchestrator.sessionActive
                ? 'Ask Chain Orchestrator...'
                : 'Start a session first...'
            }
            className="flex-1 bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-xs text-[var(--text-primary)] placeholder-[var(--text-secondary)] focus:outline-none focus:border-[var(--border-active)]"
            disabled={!chainOrchestrator.sessionActive || chainOrchestrator.isLoading}
          />
          {chainOrchestrator.isLoading ? (
            <button
              onClick={chainOrchCancel}
              className="px-2.5 py-2 bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors"
              title="Stop generation"
            >
              <Square size={14} />
            </button>
          ) : (
            <button
              onClick={handleSendMessage}
              disabled={!input.trim() || !chainOrchestrator.sessionActive}
              className="px-2.5 py-2 bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Send size={14} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
