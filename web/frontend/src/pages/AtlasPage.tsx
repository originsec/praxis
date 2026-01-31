import { useRef, useEffect, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import {
  Bot,
  Send,
  Loader2,
  Sparkles,
  PlayCircle,
  StopCircle,
  Square,
  CheckCircle,
  XCircle,
  Circle,
  Wrench,
  ListTodo,
  AlertCircle,
  ChevronRight,
  ChevronDown,
  Download,
} from 'lucide-react';
import { exportAtlasSession, downloadTextFile } from '../utils/export';
import { useApp, type AtlasMessage, type AtlasToolExecution } from '../context/AppContext';
import type { AtlasPlan, PlanStep } from '../api/types';

//
// Plan step status icon.
//
function PlanStepIcon({ status }: { status: PlanStep['status'] }) {
  switch (status) {
    case 'done':
      return <CheckCircle size={10} className="text-[var(--accent-success)]" />;
    case 'in_progress':
      return <Loader2 size={10} className="text-[var(--accent-warning)] animate-spin" />;
    case 'not_started':
    default:
      return <Circle size={10} className="text-muted" />;
  }
}

//
// Plan display component.
//
function PlanDisplay({ plan }: { plan: AtlasPlan }) {
  const doneCount = plan.steps.filter((s) => s.status === 'done').length;
  const totalCount = plan.steps.length;
  const progressPercent = totalCount > 0 ? (doneCount / totalCount) * 100 : 0;

  return (
    <div className="bg-[var(--bg-tertiary)] p-3 mb-3 border border-subtle">
      <div className="flex items-center gap-2 mb-2">
        <ListTodo size={12} className="text-[var(--accent-purple)]" />
        <span className="font-medium text-xs">Plan</span>
        <span className="text-[10px] text-muted ml-auto">
          {doneCount}/{totalCount}
        </span>
      </div>

      {/*
      //
      // Progress bar.
      //
      */}
      <div className="h-0.5 bg-[var(--bg-secondary)] rounded-full mb-2 overflow-hidden">
        <div
          className="h-full bg-[var(--accent-purple)]/60 transition-all duration-300"
          style={{ width: `${progressPercent}%` }}
        />
      </div>

      {/*
      //
      // Current step description.
      //
      */}
      {plan.current_step_description && (
        <div className="text-xs text-[var(--accent-warning)] mb-2 font-medium">
          {plan.current_step_description}
        </div>
      )}

      {/*
      //
      // Steps.
      //
      */}
      <div className="space-y-1">
        {plan.steps.map((step, idx) => (
          <div
            key={idx}
            className={`flex items-start gap-1.5 text-xs ${
              step.status === 'done'
                ? 'text-muted line-through'
                : step.status === 'in_progress'
                ? 'text-[var(--text-primary)]'
                : 'text-[var(--text-secondary)]'
            }`}
          >
            <div className="mt-0.5">
              <PlanStepIcon status={step.status} />
            </div>
            <span>{step.description}</span>
          </div>
        ))}
      </div>

      {/*
      //
      // Summary.
      //
      */}
      {plan.summary && (
        <div className="mt-2 pt-2 border-t border-subtle text-xs text-[var(--text-highlight)]/50 italic">
          {plan.summary}
        </div>
      )}
    </div>
  );
}

//
// Single tool execution item.
//
function ToolExecutionItem({ exec }: { exec: AtlasToolExecution }) {
  const [expanded, setExpanded] = useState(false);
  const canExpand = !exec.executing && exec.result;

  return (
    <div
      className={`text-[10px] px-2 py-1 rounded ${
        exec.executing
          ? 'bg-[var(--accent-warning)]/5 text-[var(--accent-warning)]/80'
          : exec.success
          ? 'bg-[var(--accent-success)]/5 text-[var(--accent-success)]/80'
          : 'bg-[var(--accent-error)]/5 text-[var(--accent-error)]/80'
      } ${canExpand ? 'cursor-pointer hover:bg-[var(--bg-tertiary)]' : ''}`}
      onClick={() => canExpand && setExpanded(!expanded)}
    >
      <div className="flex items-center gap-2">
        {exec.executing ? (
          <Loader2 size={10} className="animate-spin" />
        ) : exec.success ? (
          <CheckCircle size={10} />
        ) : (
          <XCircle size={10} />
        )}
        <Wrench size={10} />
        <span className="font-mono">{exec.name}</span>
        {!exec.executing && <span className="text-[var(--text-highlight)]/60">- {exec.display}</span>}
      </div>
      {exec.input && (
        <div className="mt-1 ml-5 pl-2 border-l border-current/30 text-[var(--text-highlight)]/60 italic">
          {exec.input}
        </div>
      )}
      {expanded && exec.result && (
        <div className="mt-2 ml-5 p-2 bg-[var(--bg-primary)] rounded border border-subtle text-[var(--text-muted)] font-mono text-[10px] overflow-x-auto max-h-48 overflow-y-auto whitespace-pre-wrap break-all">
          {(() => {
            try {
              return JSON.stringify(JSON.parse(exec.result), null, 2);
            } catch {
              return exec.result;
            }
          })()}
        </div>
      )}
    </div>
  );
}

//
// Tool execution display - collapsible for completed messages.
//
function ToolExecutionDisplay({
  executions,
  collapsible = false,
}: {
  executions: AtlasToolExecution[];
  collapsible?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  if (executions.length === 0) return null;

  //
  // For streaming (not collapsible), always show all.
  //
  if (!collapsible) {
    return (
      <div className="space-y-1 mb-2">
        {executions.map((exec, idx) => (
          <ToolExecutionItem key={idx} exec={exec} />
        ))}
      </div>
    );
  }

  //
  // For completed messages (collapsible), show summary with expand option.
  //
  const successCount = executions.filter((e) => e.success).length;
  const failCount = executions.filter((e) => !e.success && !e.executing).length;

  return (
    <div className="mb-2">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 text-xs px-3 py-1.5 rounded bg-[var(--bg-tertiary)] text-muted hover:bg-[var(--bg-secondary)] transition-colors w-full text-left"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Wrench size={12} />
        <span>
          {executions.length} tool call{executions.length !== 1 ? 's' : ''}
        </span>
        {successCount > 0 && (
          <span className="text-[var(--accent-success)]">
            <CheckCircle size={10} className="inline mr-1" />
            {successCount}
          </span>
        )}
        {failCount > 0 && (
          <span className="text-[var(--accent-error)]">
            <XCircle size={10} className="inline mr-1" />
            {failCount}
          </span>
        )}
      </button>
      {expanded && (
        <div className="space-y-1 mt-1 pl-2 border-l border-subtle">
          {executions.map((exec, idx) => (
            <ToolExecutionItem key={idx} exec={exec} />
          ))}
        </div>
      )}
    </div>
  );
}

//
// Message component.
//
function ChatMessage({ message }: { message: AtlasMessage }) {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';

  return (
    <div
      className={`flex ${isUser ? 'justify-end' : isSystem ? 'justify-center' : 'justify-start'}`}
    >
      <div
        className={`max-w-[85%] ascii-box px-4 py-3 ${
          isUser
            ? 'bg-[var(--accent-purple)]/20 text-[var(--text-primary)]'
            : isSystem
            ? 'bg-[var(--bg-tertiary)] text-muted text-sm'
            : 'bg-[var(--bg-secondary)] text-[var(--text-highlight)]/80'
        }`}
      >
        {!isUser && !isSystem && (
          <div className="flex items-center gap-2 mb-2 text-[var(--accent-success)]">
            <Bot size={16} />
            <span className="text-xs font-medium">Atlas</span>
          </div>
        )}

        {/*
        //
        // Tool executions - collapsible for completed assistant messages.
        //
        */}
        {message.toolExecutions && (
          <ToolExecutionDisplay executions={message.toolExecutions} collapsible={true} />
        )}

        {/*
        //
        // Content.
        //
        */}
        {isUser || isSystem ? (
          <div className="whitespace-pre-wrap break-words">{message.content}</div>
        ) : (
          <div className="prose prose-invert prose-sm max-w-none break-words prose-table:border-collapse prose-th:border prose-th:border-subtle prose-th:px-3 prose-th:py-2 prose-th:bg-[var(--bg-tertiary)] prose-td:border prose-td:border-subtle prose-td:px-3 prose-td:py-2">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{message.content}</ReactMarkdown>
          </div>
        )}

        <p className="text-xs text-muted mt-2">{message.timestamp.toLocaleTimeString()}</p>
      </div>
    </div>
  );
}

//
// Streaming message display.
//
function StreamingMessage({
  content,
  toolExecutions,
}: {
  content: string;
  toolExecutions: AtlasToolExecution[];
}) {
  return (
    <div className="flex justify-start">
      <div className="max-w-[85%] ascii-box px-4 py-3 bg-[var(--bg-secondary)] text-[var(--text-highlight)]/80">
        <div className="flex items-center gap-2 mb-2 text-[var(--accent-success)]">
          <Bot size={16} />
          <span className="text-xs font-medium">Atlas</span>
          <Loader2 size={12} className="animate-spin ml-auto" />
        </div>

        <ToolExecutionDisplay executions={toolExecutions} />

        {content && (
          <div className="prose prose-invert prose-sm max-w-none break-words prose-table:border-collapse prose-th:border prose-th:border-subtle prose-th:px-3 prose-th:py-2 prose-th:bg-[var(--bg-tertiary)] prose-td:border prose-td:border-subtle prose-td:px-3 prose-td:py-2">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
          </div>
        )}

        {!content && toolExecutions.length === 0 && (
          <div className="flex items-center gap-2 text-muted text-sm">
            <Loader2 size={14} className="animate-spin" />
            <span>Thinking...</span>
          </div>
        )}
      </div>
    </div>
  );
}

export function AtlasPage() {
  const { state, atlasStart, atlasStop, atlasCancel, atlasPrompt, getConfig } = useApp();
  const { atlas } = state;
  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  //
  // Fetch config on mount to check if Atlas is configured.
  //
  useEffect(() => {
    getConfig(['llm_feature_atlas', 'llm_model_definitions']);
  }, [getConfig]);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [atlas.messages, atlas.streamingContent, atlas.currentToolExecutions]);

  //
  // Focus input when loading completes.
  //
  useEffect(() => {
    if (!atlas.isLoading && atlas.sessionActive) {
      inputRef.current?.focus();
    }
  }, [atlas.isLoading, atlas.sessionActive]);

  const handleSendMessage = () => {
    if (!input.trim() || atlas.isLoading) return;
    atlasPrompt(input.trim());
    setInput('');
  };

  const handleNewSession = () => {
    atlasStart();
  };

  const handleStopSession = () => {
    atlasStop();
  };

  const handleExport = () => {
    if (atlas.messages.length === 0) return;
    const content = exportAtlasSession(atlas.messages, atlas.tokenUsage);
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    downloadTextFile(content, `atlas-session-${timestamp}.md`);
  };

  //
  // Check if Atlas is configured via the LLM feature system.
  //
  const atlasConfig = (() => {
    const selectedModelName = state.config.llm_feature_atlas;
    if (!selectedModelName) return null;

    const modelDefsRaw = state.config.llm_model_definitions;
    if (!modelDefsRaw) return null;

    try {
      const defs = JSON.parse(modelDefsRaw) as Array<{ name: string; provider: string; model: string }>;
      const def = defs.find((d) => d.name === selectedModelName);
      if (def) {
        return { provider: def.provider, model: def.model };
      }
    } catch {
      // Parse error
    }
    return null;
  })();

  const isConfigured = !!atlasConfig;

  return (
    <div className="h-full flex flex-col">
      {/*
      //
      // Header.
      //
      */}
      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-3">
          <div className="p-3 bg-[var(--accent-purple)]/20">
            <Sparkles size={32} className="text-[var(--accent-purple)]" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h1 className="text-2xl font-bold text-highlight">Atlas</h1>
              <span className="px-2 py-0.5 text-xs font-medium bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] rounded">
                Experimental
              </span>
            </div>
            <p className="text-muted mt-1">
              AI-powered red teaming orchestration
              {atlasConfig && (
                <span className="ml-2 text-[var(--accent-info)]">
                  · {atlasConfig.provider}/{atlasConfig.model}
                </span>
              )}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          {/*
          //
          // Export button.
          //
          */}
          <button
            onClick={handleExport}
            disabled={atlas.messages.length === 0}
            className="flex items-center gap-2 px-3 py-2 bg-[var(--bg-secondary)] border border-subtle text-muted hover:text-[var(--text-primary)] hover:border-[var(--border-active)] transition-colors text-sm disabled:opacity-50 disabled:cursor-not-allowed"
            title="Export session transcript"
          >
            <Download size={16} />
          </button>

          {atlas.sessionActive ? (
            <button
              onClick={handleStopSession}
              className="flex items-center gap-2 px-4 py-2 bg-[var(--accent-error)]/20 text-[var(--accent-error)]  hover:bg-[var(--accent-error)]/30 transition-colors text-sm"
            >
              <StopCircle size={16} />
              Stop Session
            </button>
          ) : (
            <button
              onClick={handleNewSession}
              disabled={!isConfigured || atlas.isStarting}
              className="flex items-center gap-2 px-4 py-2 bg-[var(--accent-purple)]/20 text-[var(--accent-purple)]  hover:bg-[var(--accent-purple)]/30 transition-colors text-sm disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {atlas.isStarting ? (
                <>
                  <Loader2 size={16} className="animate-spin" />
                  Starting...
                </>
              ) : (
                <>
                  <PlayCircle size={16} />
                  New Session
                </>
              )}
            </button>
          )}
        </div>
      </div>

      {/*
      //
      // Not configured warning.
      //
      */}
      {!isConfigured && (
        <div className="mb-4 p-4 bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/30  flex items-start gap-3">
          <AlertCircle size={20} className="text-[var(--accent-warning)] mt-0.5 flex-shrink-0" />
          <div>
            <p className="text-sm font-medium text-[var(--accent-warning)]">
              Atlas Not Configured
            </p>
            <p className="text-xs text-muted mt-1">
              Go to Settings &gt; Atlas to configure an LLM provider and API key.
            </p>
          </div>
        </div>
      )}

      {/*
      //
      // Plan display.
      //
      */}
      {atlas.currentPlan && atlas.currentPlan.steps.length > 0 && (
        <PlanDisplay plan={atlas.currentPlan} />
      )}

      {/*
      //
      // Chat area.
      //
      */}
      <div className="flex-1 bg-card ascii-box border border-subtle flex flex-col min-h-0">
        {/*
        //
        // Messages.
        //
        */}
        <div className="flex-1 overflow-auto p-6 space-y-4">
          {atlas.messages.map((msg) => (
            <ChatMessage key={msg.id} message={msg} />
          ))}

          {/*
          //
          // Streaming content.
          //
          */}
          {atlas.isLoading && (
            <StreamingMessage
              content={atlas.streamingContent}
              toolExecutions={atlas.currentToolExecutions}
            />
          )}

          <div ref={messagesEndRef} />
        </div>

        {/*
        //
        // Input.
        //
        */}
        <div className="p-4 border-t border-subtle">
          <div className="flex gap-3">
            <input
              ref={inputRef}
              type="text"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && !e.shiftKey && handleSendMessage()}
              placeholder={
                atlas.sessionActive
                  ? 'Ask Atlas anything...'
                  : 'Start a session to begin chatting...'
              }
              className="flex-1 bg-[var(--bg-secondary)] border border-subtle  px-4 py-3 text-[var(--text-primary)] placeholder-[var(--text-secondary)] focus:outline-none focus:border-[var(--border-active)]"
              disabled={!atlas.sessionActive || atlas.isLoading}
            />
            {atlas.isLoading ? (
              <button
                onClick={atlasCancel}
                className="px-4 py-3 bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors"
                title="Stop generation"
              >
                <Square size={20} />
              </button>
            ) : (
              <button
                onClick={handleSendMessage}
                disabled={!input.trim() || !atlas.sessionActive}
                className="px-4 py-3 bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <Send size={20} />
              </button>
            )}
          </div>
        </div>
      </div>

      {/*
      //
      // Status footer.
      //
      */}
      <div className="mt-4 flex items-center justify-between text-sm text-muted">
        <div className="flex items-center gap-4">
          <span>{state.systemState?.nodes.length ?? 0} nodes connected</span>
          <span>
            {state.operations.filter((op) => op.status === 'Running').length} operations running
          </span>
          {atlas.sessionActive && (
            <span className="text-[var(--accent-purple)]">Atlas session active</span>
          )}
          {atlas.tokenUsage && (
            <span className="text-[var(--accent-info)]" title={`Prompt: ${atlas.tokenUsage.promptTokens.toLocaleString()} | Completion: ${atlas.tokenUsage.completionTokens.toLocaleString()}`}>
              {atlas.tokenUsage.totalTokens.toLocaleString()} tokens
            </span>
          )}
        </div>
        <span className={state.connected ? 'text-[var(--accent-success)]' : 'text-[var(--accent-error)]'}>
          {state.connected ? 'Connected' : 'Disconnected'}
        </span>
      </div>
    </div>
  );
}
