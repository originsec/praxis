import { useState, useEffect, useRef } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import {
  Bot,
  Loader2,
  CheckCircle,
  XCircle,
  Circle,
  Wrench,
  ListTodo,
  ChevronRight,
  ChevronDown,
  ChevronUp,
  Brain,
  User,
} from 'lucide-react';
import type { OrchestratorMessage, OrchestratorToolExecution } from '../../context/orchestratorTypes';
import type { OrchestratorPlan, PlanStep } from '../../api/types';

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
// Extract thinking content from <think> tags.
//
function parseThinkingContent(content: string): { thinking: string[]; response: string } {
  const startTag = '<think>';
  const endTag = '</think>';
  const thinking: string[] = [];
  let remaining = content;

  while (true) {
    const startIdx = remaining.indexOf(startTag);
    const endIdx = remaining.indexOf(endTag);

    if (startIdx === -1 || endIdx === -1 || startIdx > endIdx) {
      break;
    }

    const block = remaining.substring(startIdx + startTag.length, endIdx).trim();
    if (block) {
      thinking.push(block);
    }
    remaining = remaining.substring(0, startIdx) + remaining.substring(endIdx + endTag.length);
  }

  let response = remaining.trim();

  //
  // Strip code fences that contain markdown formatting. LLMs sometimes
  // wrap markdown tables, headers, or bold text inside ``` blocks which
  // prevents ReactMarkdown from rendering them properly.
  //

  response = stripMarkdownCodeFences(response);

  return { thinking, response };
}

function stripMarkdownCodeFences(text: string): string {
  //
  // Match ``` blocks and check if their content looks like markdown
  // (contains tables, headers, or bold). If so, unwrap them.
  //

  return text.replace(
    /```[a-z]*\n([\s\S]*?)```/g,
    (_match, inner: string) => {
      const hasTable = /^\s*\|.*\|/m.test(inner);
      const hasHeader = /^#{1,6}\s/m.test(inner);
      const hasBold = /\*\*[^*]+\*\*/.test(inner);
      if (hasTable || hasHeader || hasBold) {
        return inner;
      }
      return _match;
    }
  );
}

//
// Collapsible thinking block.
//
function ThinkingBlock({ content, autoExpand = false }: { content: string; autoExpand?: boolean }) {
  const [show, setShow] = useState(false);

  useEffect(() => {
    setShow(autoExpand);
  }, [autoExpand]);

  return (
    <div>
      <button
        onClick={() => setShow(!show)}
        className="flex items-center gap-1.5 text-xs text-muted/30 hover:text-muted/50 transition-colors"
      >
        {show ? <ChevronUp size={12} /> : <ChevronRight size={12} />}
        <span>Thinking</span>
      </button>
      {show && (
        <div className="mt-1 ml-4 text-[11px] text-muted/25 whitespace-pre-wrap max-h-48 overflow-y-auto">
          {content}
        </div>
      )}
    </div>
  );
}

function ThinkingDisplay({ blocks, collapsible = false }: { blocks: string[]; collapsible?: boolean }) {
  const [expanded, setExpanded] = useState(false);

  if (blocks.length === 0) return null;

  if (!collapsible) {
    return (
      <div className="mb-3 space-y-2">
        {blocks.map((t, i) => (
          <ThinkingBlock key={i} content={t} autoExpand={i === blocks.length - 1} />
        ))}
      </div>
    );
  }

  return (
    <div className="mb-2">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 text-xs px-3 py-1.5 rounded bg-[var(--bg-tertiary)] text-muted hover:bg-[var(--bg-secondary)] transition-colors w-full text-left"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Brain size={12} />
        <span>
          {blocks.length} thinking block{blocks.length !== 1 ? 's' : ''}
        </span>
      </button>
      {expanded && (
        <div className="space-y-2 mt-1 pl-2 border-l border-subtle">
          {blocks.map((t, i) => (
            <ThinkingBlock key={i} content={t} />
          ))}
        </div>
      )}
    </div>
  );
}

//
// Single tool execution item.
//
function ToolExecutionItem({ exec, compact = false }: { exec: OrchestratorToolExecution; compact?: boolean }) {
  const [expanded, setExpanded] = useState(false);
  const canExpand = exec.input || exec.result;
  const fontSize = compact ? 'text-[9px]' : 'text-[10px]';

  return (
    <div
      className={`${fontSize} px-2 py-1 rounded cursor-pointer ${
        exec.executing
          ? 'bg-[var(--accent-warning)]/5 text-[var(--accent-warning)]/80'
          : exec.success
          ? 'bg-[var(--accent-success)]/5 text-[var(--accent-success)]/80'
          : 'bg-[var(--accent-error)]/5 text-[var(--accent-error)]/80'
      } hover:bg-[var(--bg-tertiary)]`}
      onClick={() => canExpand && setExpanded(!expanded)}
    >
      <div className="flex items-center gap-2">
        {canExpand && (
          expanded
            ? <ChevronDown size={10} className="flex-shrink-0" />
            : <ChevronRight size={10} className="flex-shrink-0" />
        )}
        {exec.executing ? (
          <Loader2 size={10} className="animate-spin flex-shrink-0" />
        ) : exec.success ? (
          <CheckCircle size={10} className="flex-shrink-0" />
        ) : (
          <XCircle size={10} className="flex-shrink-0" />
        )}
        <Wrench size={10} className="flex-shrink-0" />
        <span className="font-mono">{exec.name}</span>
        {!exec.executing && <span className="text-[var(--text-highlight)]/60 truncate">- {exec.display}</span>}
      </div>
      {expanded && (
        <div className="mt-2 ml-5 space-y-2">
          {exec.input && (
            <div className="p-2 bg-[var(--bg-primary)] rounded border border-subtle text-[var(--text-muted)] font-mono text-[10px] overflow-x-auto max-h-48 overflow-y-auto whitespace-pre-wrap break-all">
              <span className="text-[var(--text-highlight)]/40 select-none">input: </span>
              {(() => {
                try { return JSON.stringify(JSON.parse(exec.input), null, 2); }
                catch { return exec.input; }
              })()}
            </div>
          )}
          {exec.result && (
            <div className="p-2 bg-[var(--bg-primary)] rounded border border-subtle text-[var(--text-muted)] font-mono text-[10px] overflow-x-auto max-h-48 overflow-y-auto whitespace-pre-wrap break-all">
              <span className="text-[var(--text-highlight)]/40 select-none">result: </span>
              {(() => {
                try { return JSON.stringify(JSON.parse(exec.result), null, 2); }
                catch { return exec.result; }
              })()}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

//
// Tool execution display.
//
function ToolExecutionDisplay({
  executions,
  collapsible = false,
  compact = false,
}: {
  executions: OrchestratorToolExecution[];
  collapsible?: boolean;
  compact?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  if (executions.length === 0) return null;

  if (!collapsible) {
    return (
      <div className="space-y-1 mb-2">
        {executions.map((exec, idx) => (
          <ToolExecutionItem key={idx} exec={exec} compact={compact} />
        ))}
      </div>
    );
  }

  const successCount = executions.filter(e => e.success).length;
  const failCount = executions.filter(e => !e.success && !e.executing).length;

  return (
    <div className="mb-2">
      <button
        onClick={() => setExpanded(!expanded)}
        className={`flex items-center gap-2 ${compact ? 'text-[10px]' : 'text-xs'} px-3 py-1.5 rounded bg-[var(--bg-tertiary)] text-muted hover:bg-[var(--bg-secondary)] transition-colors w-full text-left`}
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Wrench size={12} />
        <span>{executions.length} tool call{executions.length !== 1 ? 's' : ''}</span>
        {successCount > 0 && (
          <span className="text-[var(--accent-success)]">
            <CheckCircle size={10} className="inline mr-1" />{successCount}
          </span>
        )}
        {failCount > 0 && (
          <span className="text-[var(--accent-error)]">
            <XCircle size={10} className="inline mr-1" />{failCount}
          </span>
        )}
      </button>
      {expanded && (
        <div className="space-y-1 mt-1 pl-2 border-l border-subtle">
          {executions.map((exec, idx) => (
            <ToolExecutionItem key={idx} exec={exec} compact={compact} />
          ))}
        </div>
      )}
    </div>
  );
}

//
// Plan display component. Compact collapsible bar that shows current step
// or final status, expanding to reveal full plan on click.
//
export function PlanDisplay({ plan, compact = false }: { plan: OrchestratorPlan; compact?: boolean }) {
  const [expanded, setExpanded] = useState(true);
  const prevStepsRef = useRef<string | null>(null);
  const doneCount = plan.steps.filter(s => s.status === 'done').length;
  const totalCount = plan.steps.length;
  const allDone = doneCount === totalCount;

  //
  // Auto-expand when a new plan arrives, auto-collapse when complete.
  //

  const planFingerprint = plan.steps.map(s => s.description).join('\0');
  useEffect(() => {
    if (prevStepsRef.current !== null && prevStepsRef.current !== planFingerprint) {
      setExpanded(true);
    }
    prevStepsRef.current = planFingerprint;
  }, [planFingerprint]);

  useEffect(() => {
    if (allDone && totalCount > 0) {
      setExpanded(false);
    }
  }, [allDone, totalCount]);
  const progressPercent = totalCount > 0 ? (doneCount / totalCount) * 100 : 0;

  //
  // Determine the summary line: final status, current step, or progress.
  //
  const currentStep = plan.steps.find(s => s.status === 'in_progress');
  const summaryText = allDone
    ? (plan.summary || 'Complete')
    : currentStep
    ? currentStep.description
    : plan.current_step_description || `${doneCount}/${totalCount} steps`;

  const statusColor = allDone
    ? 'text-[var(--accent-success)]'
    : currentStep
    ? 'text-[var(--accent-warning)]'
    : 'text-muted';

  return (
    <div className="border-t border-subtle flex-shrink-0">
      {/*
      //
      // Collapsed bar: clickable current step + progress.
      //
      */}
      <div
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 px-3 py-1.5 cursor-pointer hover:bg-[var(--highlight)] transition-colors"
      >
        <ListTodo size={10} className="text-[var(--accent-purple)] flex-shrink-0" />
        <div className="flex-1 min-w-0 flex items-center gap-2">
          <span className={`${compact ? 'text-[9px]' : 'text-[10px]'} ${statusColor} truncate`}>
            {summaryText}
          </span>
        </div>
        <span className={`${compact ? 'text-[8px]' : 'text-[9px]'} text-muted flex-shrink-0`}>
          {doneCount}/{totalCount}
        </span>
        {expanded ? <ChevronDown size={10} className="text-muted flex-shrink-0" /> : <ChevronUp size={10} className="text-muted flex-shrink-0" />}
      </div>

      {/*
      //
      // Expanded: full step list with progress bar.
      //
      */}
      {expanded && (
        <div className="px-3 pb-2 space-y-1.5">
          <div className="h-0.5 bg-[var(--bg-secondary)] overflow-hidden">
            <div
              className="h-full bg-[var(--accent-purple)]/60 transition-all duration-300"
              style={{ width: `${progressPercent}%` }}
            />
          </div>

          <div className="space-y-0.5">
            {plan.steps.map((step, idx) => (
              <div
                key={idx}
                className={`flex items-start gap-1.5 ${compact ? 'text-[9px]' : 'text-[10px]'} ${
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

          {plan.summary && (
            <div className={`pt-1 border-t border-subtle ${compact ? 'text-[9px]' : 'text-[10px]'} text-[var(--text-highlight)]/50 italic`}>
              {plan.summary}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

//
// Chat message component. Supports compact mode for the side panel.
//
export function ChatMessage({ message, compact = false }: { message: OrchestratorMessage; compact?: boolean }) {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';
  const isAssistant = !isUser && !isSystem;
  const { thinking, response } = isAssistant
    ? parseThinkingContent(message.content)
    : { thinking: [], response: message.content };

  const padding = compact ? 'px-2 py-1.5' : 'px-3 md:px-4 py-2';

  //
  // Left border accent differentiates message roles at a glance.
  //
  const borderAccent = isUser
    ? 'border-l-2 border-l-[var(--accent-purple)]'
    : isSystem
    ? ''
    : 'border-l-2 border-l-[var(--accent-success)]';

  //
  // System messages render as a small centered inline notice, not a chat bubble.
  //
  if (isSystem) {
    //
    // Split model info (in parentheses) onto its own line.
    //
    const parenMatch = message.content.match(/^(.+?)\s*\(([^)]+)\)\.?$/);
    const mainText = parenMatch ? parenMatch[1] : message.content;
    const modelText = parenMatch ? parenMatch[2] : null;

    return (
      <div className="flex justify-center">
        <div className={`text-center ${compact ? 'px-2 py-0.5' : 'px-3 py-1'} ${compact ? 'text-[9px]' : 'text-[10px]'} text-muted/60 italic`}>
          <div className="flex items-center justify-center gap-2">
            <span className="text-muted/30">—</span>
            <span>{mainText}</span>
            <span className="text-muted/30">—</span>
          </div>
          {modelText && (
            <div className={`${compact ? 'text-[8px]' : 'text-[9px]'} text-muted/40 font-mono mt-0.5`}>
              {modelText}
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className={`flex ${isUser ? 'justify-end' : 'justify-start'}`}>
      <div
        className={`w-full ${compact ? '' : 'md:max-w-[85%]'} ascii-box ${padding} ${borderAccent} ${
          isUser
            ? 'bg-[var(--accent-purple)]/10 text-[var(--text-primary)]'
            : 'bg-[var(--bg-secondary)] text-[var(--text-highlight)]/80'
        }`}
      >
        {/*
        //
        // Role label — user or assistant.
        //
        */}
        {isUser && (
          <div className={`flex items-center gap-1.5 mb-1.5 text-[var(--accent-purple)]`}>
            <User size={compact ? 10 : 12} />
            <span className={`${compact ? 'text-[9px]' : 'text-[10px]'} font-medium`}>You</span>
          </div>
        )}
        {isAssistant && (
          <div className="flex items-center gap-1.5 mb-1.5 text-[var(--accent-success)]">
            <Bot size={compact ? 10 : 12} />
            <span className={`${compact ? 'text-[9px]' : 'text-[10px]'} font-medium`}>Orchestrator</span>
          </div>
        )}

        {message.toolExecutions && (
          <ToolExecutionDisplay executions={message.toolExecutions} collapsible={true} compact={compact} />
        )}

        <ThinkingDisplay blocks={thinking} collapsible={true} />

        {isUser ? (
          <div className={`whitespace-pre-wrap break-words ${compact ? 'text-[10px]' : 'text-[11px]'}`}>{message.content}</div>
        ) : response ? (
          <div className={`prose prose-invert max-w-none break-words ${compact ? 'text-[10px] leading-relaxed [&_p]:my-1 [&_li]:my-0.5 [&_pre]:text-[9px] [&_code]:text-[9px]' : 'text-[11px] leading-relaxed [&_p]:my-1.5 [&_li]:my-0.5 [&_pre]:text-[10px] [&_code]:text-[10px]'} prose-table:border-collapse prose-th:border prose-th:border-subtle prose-th:px-2 prose-th:py-1 prose-th:bg-[var(--bg-tertiary)] prose-th:text-[10px] prose-td:border prose-td:border-subtle prose-td:px-2 prose-td:py-1 prose-td:text-[10px]`}>
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{response}</ReactMarkdown>
          </div>
        ) : null}

        <p className={`${compact ? 'text-[8px]' : 'text-[10px]'} text-muted/50 mt-1`}>{message.timestamp.toLocaleTimeString()}</p>
      </div>
    </div>
  );
}

//
// Streaming message display.
//
export function StreamingMessage({
  content,
  toolExecutions,
  compact = false,
}: {
  content: string;
  toolExecutions: OrchestratorToolExecution[];
  compact?: boolean;
}) {
  const { thinking, response } = parseThinkingContent(content);
  const padding = compact ? 'px-2 py-2' : 'px-3 md:px-4 py-3';

  return (
    <div className="flex justify-start">
      <div className={`w-full ${compact ? '' : 'md:max-w-[85%]'} ascii-box ${padding} bg-[var(--bg-secondary)] text-[var(--text-highlight)]/80`}>
        <div className="flex items-center gap-2 mb-2 text-[var(--accent-success)]">
          <Bot size={compact ? 12 : 16} />
          <span className={`${compact ? 'text-[10px]' : 'text-xs'} font-medium`}>Orchestrator</span>
          <Loader2 size={12} className="animate-spin ml-auto" />
        </div>

        <ToolExecutionDisplay executions={toolExecutions} compact={compact} />
        <ThinkingDisplay blocks={thinking} />

        {response && (
          <div className={`prose prose-invert max-w-none break-words ${compact ? 'text-[10px] leading-relaxed [&_p]:my-1' : 'text-[11px] leading-relaxed [&_p]:my-1.5'}`}>
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{response}</ReactMarkdown>
          </div>
        )}

        {!content && toolExecutions.length === 0 && (
          <div className={`flex items-center gap-2 text-muted ${compact ? 'text-[10px]' : 'text-xs'}`}>
            <Loader2 size={14} className="animate-spin" />
            <span>Thinking...</span>
          </div>
        )}
      </div>
    </div>
  );
}
