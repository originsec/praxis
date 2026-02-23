import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Bot, Loader2 } from 'lucide-react';
import type { OrchestratorToolExecution } from '../../context/orchestratorTypes';
import { ToolExecutionDisplay } from './ToolExecutionDisplay';
import { parseThinkingContent, ThinkingDisplay } from './ChatMessage';

interface StreamingMessageProps {
  content: string;
  toolExecutions: OrchestratorToolExecution[];
  agentLabel?: string;
  compact?: boolean;
}

export function StreamingMessage({
  content,
  toolExecutions,
  agentLabel = 'Orchestrator',
  compact,
}: StreamingMessageProps) {
  const { thinking, response } = parseThinkingContent(content);

  const padding = compact ? 'px-2 py-2' : 'px-3 md:px-4 py-3';
  const width = compact ? 'w-full' : 'w-full md:max-w-[85%]';
  const textSize = compact ? 'text-xs' : '';

  return (
    <div className="flex justify-start">
      <div className={`${width} ascii-box ${padding} ${textSize} bg-[var(--bg-secondary)] text-[var(--text-highlight)]/80`}>
        <div className="flex items-center gap-2 mb-1.5 text-[var(--accent-success)]">
          <Bot size={compact ? 12 : 16} />
          <span className={`font-medium ${compact ? 'text-[10px]' : 'text-xs'}`}>{agentLabel}</span>
          <Loader2 size={compact ? 10 : 12} className="animate-spin ml-auto" />
        </div>

        <ToolExecutionDisplay executions={toolExecutions} />

        <ThinkingDisplay blocks={thinking} />

        {response && (
          <div className={`prose prose-invert max-w-none break-words ${compact ? 'prose-xs' : 'prose-sm'} prose-table:border-collapse prose-th:border prose-th:border-subtle prose-th:px-3 prose-th:py-2 prose-th:bg-[var(--bg-tertiary)] prose-td:border prose-td:border-subtle prose-td:px-3 prose-td:py-2`}>
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{response}</ReactMarkdown>
          </div>
        )}

        {!content && toolExecutions.length === 0 && (
          <div className={`flex items-center gap-2 text-muted ${compact ? 'text-xs' : 'text-sm'}`}>
            <Loader2 size={compact ? 10 : 14} className="animate-spin" />
            <span>Thinking...</span>
          </div>
        )}
      </div>
    </div>
  );
}
