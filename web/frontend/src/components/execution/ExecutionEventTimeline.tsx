import { useRef, useEffect, useState } from 'react';
import {
  Play,
  CheckCircle,
  XCircle,
  Send,
  MessageSquare,
  Wrench,
  Loader2,
  Cpu,
  ChevronDown,
  ChevronRight,
  Link2,
  Unlink,
} from 'lucide-react';
import type { ChainExecutionEvent, ChainExecutionEventKind } from '../../api/types';

interface ExecutionEventTimelineProps {
  events: ChainExecutionEvent[];
  filterElementId?: string | null;
  autoScroll?: boolean;
}

function EventIcon({ kind }: { kind: ChainExecutionEventKind }) {
  switch (kind.type) {
    case 'ElementStarted':
      return <Play size={10} className="text-[var(--accent-info)]" />;
    case 'ElementCompleted':
      return <CheckCircle size={10} className="text-[var(--accent-success)]" />;
    case 'ElementFailed':
      return <XCircle size={10} className="text-[var(--accent-error)]" />;
    case 'PromptSent':
      return <Send size={10} className="text-[var(--accent-purple)]" />;
    case 'ResponseReceived':
      return <MessageSquare size={10} className="text-[var(--accent-info)]" />;
    case 'ToolCallStarted':
      return <Wrench size={10} className="text-[var(--accent-warning)]" />;
    case 'ToolCallCompleted':
      return <Wrench size={10} className={kind.success ? 'text-[var(--accent-success)]' : 'text-[var(--accent-error)]'} />;
    case 'AgentIteration':
      return <Loader2 size={10} className="text-[var(--accent-info)] animate-spin" />;
    case 'LlmCallStarted':
      return <Cpu size={10} className="text-[var(--accent-purple)]" />;
    case 'LlmCallCompleted':
      return <Cpu size={10} className="text-[var(--accent-success)]" />;
    case 'SessionCreated':
      return <Link2 size={10} className="text-[var(--accent-info)]" />;
    case 'SessionClosed':
      return <Unlink size={10} className="text-muted" />;
    case 'OutputChunk':
      return <MessageSquare size={10} className="text-muted" />;
  }
}

function eventTitle(kind: ChainExecutionEventKind): string {
  switch (kind.type) {
    case 'ElementStarted': return `Started: ${kind.element_label} (${kind.element_type})`;
    case 'ElementCompleted': return 'Element completed';
    case 'ElementFailed': return 'Element failed';
    case 'PromptSent': return 'Prompt sent';
    case 'ResponseReceived': return 'Response received';
    case 'ToolCallStarted': return `Tool: ${kind.tool_name}`;
    case 'ToolCallCompleted': return `Tool done: ${kind.tool_name}`;
    case 'AgentIteration': return `Iteration ${kind.iteration}/${kind.total}`;
    case 'LlmCallStarted': return `LLM call (${kind.model})`;
    case 'LlmCallCompleted': return `LLM done (${kind.tokens_used} tokens)`;
    case 'SessionCreated': return `Session created`;
    case 'SessionClosed': return `Session closed`;
    case 'OutputChunk': return 'Output chunk';
  }
}

function eventPreview(kind: ChainExecutionEventKind): string | null {
  switch (kind.type) {
    case 'ElementCompleted': return kind.output_preview;
    case 'ElementFailed': return kind.error;
    case 'PromptSent': return kind.prompt_preview;
    case 'ResponseReceived': return kind.response_preview;
    case 'ToolCallStarted': return kind.input_preview;
    case 'ToolCallCompleted': return kind.result_preview;
    case 'OutputChunk': return kind.chunk;
    default: return null;
  }
}

function EventItem({ event }: { event: ChainExecutionEvent }) {
  const [expanded, setExpanded] = useState(false);
  const preview = eventPreview(event.kind);
  const hasPreview = preview && preview.length > 0;

  const time = new Date(event.timestamp).toLocaleTimeString();

  return (
    <div
      className={`px-2 py-1.5 text-[10px] hover:bg-[var(--bg-tertiary)] transition-colors ${hasPreview ? 'cursor-pointer' : ''}`}
      onClick={() => hasPreview && setExpanded(!expanded)}
    >
      <div className="flex items-center gap-2">
        {hasPreview ? (
          expanded
            ? <ChevronDown size={8} className="flex-shrink-0 text-muted" />
            : <ChevronRight size={8} className="flex-shrink-0 text-muted" />
        ) : (
          <span className="w-2 flex-shrink-0" />
        )}
        <EventIcon kind={event.kind} />
        <span className="text-[var(--text-primary)] flex-1 truncate">{eventTitle(event.kind)}</span>
        <span className="text-muted flex-shrink-0">{time}</span>
      </div>
      {expanded && preview && (
        <div className="mt-1 ml-6 p-2 bg-[var(--bg-primary)] border border-subtle text-[var(--text-secondary)] font-mono text-[10px] max-h-32 overflow-auto whitespace-pre-wrap break-all">
          {preview}
        </div>
      )}
    </div>
  );
}

export function ExecutionEventTimeline({ events, filterElementId, autoScroll = true }: ExecutionEventTimelineProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  const filteredEvents = filterElementId
    ? events.filter(e => {
        const kind = e.kind;
        if ('element_id' in kind && kind.element_id === filterElementId) return true;
        if (kind.type === 'SessionClosed') return true;
        return false;
      })
    : events;

  useEffect(() => {
    if (autoScroll && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [filteredEvents.length, autoScroll]);

  if (filteredEvents.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-xs text-muted">
        {events.length === 0 ? 'No events yet' : 'No events for selected element'}
      </div>
    );
  }

  return (
    <div ref={scrollRef} className="h-full overflow-auto">
      <div className="divide-y divide-subtle">
        {filteredEvents.map((event, idx) => (
          <EventItem key={idx} event={event} />
        ))}
      </div>
    </div>
  );
}
