import { useState, useEffect } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import {
  Bot,
  ChevronRight,
  ChevronDown,
  ChevronUp,
  Brain,
} from 'lucide-react';
import type { OrchestratorMessage } from '../../context/orchestratorTypes';
import { ToolExecutionDisplay } from './ToolExecutionDisplay';

//
// Extract thinking content from <think> tags (supports multiple).
//
export function parseThinkingContent(content: string): { thinking: string[]; response: string } {
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

  return { thinking, response: remaining.trim() };
}

export function ThinkingBlock({ content, autoExpand = false }: { content: string; autoExpand?: boolean }) {
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

export function ThinkingDisplay({ blocks, collapsible = false }: { blocks: string[]; collapsible?: boolean }) {
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

interface ChatMessageProps {
  message: OrchestratorMessage;
  agentLabel?: string;
  compact?: boolean;
}

export function ChatMessage({ message, agentLabel = 'Orchestrator', compact }: ChatMessageProps) {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';
  const isAssistant = !isUser && !isSystem;
  const { thinking, response } = isAssistant
    ? parseThinkingContent(message.content)
    : { thinking: [], response: message.content };

  const padding = compact ? 'px-2 py-2' : 'px-3 md:px-4 py-3';
  const width = compact ? 'w-full' : 'w-full md:max-w-[85%]';
  const textSize = compact ? 'text-xs' : '';

  return (
    <div
      className={`flex ${isUser ? 'justify-end' : isSystem ? 'justify-center' : 'justify-start'}`}
    >
      <div
        className={`${width} ascii-box ${padding} ${textSize} ${
          isUser
            ? 'bg-[var(--highlight)] text-[var(--text-primary)] border-l-2 border-l-[var(--accent-purple)]'
            : isSystem
            ? 'bg-[var(--bg-tertiary)] text-muted text-sm'
            : 'bg-[var(--bg-secondary)] text-[var(--text-highlight)]/80'
        }`}
      >
        {isUser && (
          <div className={`flex items-center gap-1.5 mb-1 text-[var(--accent-purple)]`}>
            <span className={`font-medium ${compact ? 'text-[10px]' : 'text-xs'}`}>You</span>
          </div>
        )}
        {!isUser && !isSystem && (
          <div className="flex items-center gap-2 mb-1.5 text-[var(--accent-success)]">
            <Bot size={compact ? 12 : 16} />
            <span className={`font-medium ${compact ? 'text-[10px]' : 'text-xs'}`}>{agentLabel}</span>
          </div>
        )}

        {message.toolExecutions && (
          <ToolExecutionDisplay executions={message.toolExecutions} collapsible={true} />
        )}

        <ThinkingDisplay blocks={thinking} collapsible={true} />

        {isUser || isSystem ? (
          <div className="whitespace-pre-wrap break-words">{message.content}</div>
        ) : response ? (
          <div className={`prose prose-invert max-w-none break-words ${compact ? 'prose-xs' : 'prose-sm'} prose-table:border-collapse prose-th:border prose-th:border-subtle prose-th:px-3 prose-th:py-2 prose-th:bg-[var(--bg-tertiary)] prose-td:border prose-td:border-subtle prose-td:px-3 prose-td:py-2`}>
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{response}</ReactMarkdown>
          </div>
        ) : null}

        <p className={`text-muted mt-1.5 ${compact ? 'text-[9px]' : 'text-xs'}`}>{message.timestamp.toLocaleTimeString()}</p>
      </div>
    </div>
  );
}
