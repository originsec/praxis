import { useState, useRef, useEffect } from 'react';
import { Download, ChevronDown, ChevronRight } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import remarkBreaks from 'remark-breaks';
import { FloatingPanel } from './FloatingPanel';
import { StyledOutput } from '../common/StyledOutput';
import { exportOperationResult, downloadTextFile } from '../../utils/export';
import type { SemanticOpUpdate } from '../../api/types';

interface Props {
  operation: SemanticOpUpdate;
  onClose: () => void;
}

function statusColor(status: string): string {
  switch (status) {
    case 'Completed': return 'text-[var(--text-highlight)]';
    case 'Failed': return 'text-[var(--accent-error)]';
    case 'Cancelled': return 'text-[var(--accent-warning)]';
    case 'Running': return 'text-[var(--accent-info)]';
    default: return 'text-[var(--text-secondary)]';
  }
}

function formatDuration(start: string, end: string | null, status: string): string {
  if (status === 'Queued') return '—';
  const startTime = new Date(start).getTime();
  const endTime = end ? new Date(end).getTime() : Date.now();
  const diffMs = endTime - startTime;
  const diffSecs = Math.floor(diffMs / 1000);
  const mins = Math.floor(diffSecs / 60);
  const secs = diffSecs % 60;
  return mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;
}

export function OperationDetailFloating({ operation, onClose }: Props) {
  const outputRef = useRef<HTMLDivElement>(null);
  const [summaryCollapsed, setSummaryCollapsed] = useState(false);
  const [promptCollapsed, setPromptCollapsed] = useState(true);
  const [outputCollapsed, setOutputCollapsed] = useState(false);

  useEffect(() => {
    if (outputRef.current && operation.status === 'Running') {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [operation.output, operation.status]);

  const handleExport = () => {
    const content = exportOperationResult(operation);
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    downloadTextFile(content, `operation-${operation.spec.name}-${timestamp}.md`);
  };

  return (
    <FloatingPanel
      title={`Op: ${operation.spec.name}`}
      onClose={onClose}
      defaultWidth={540}
      defaultHeight={460}
      headerActions={
        <button
          onClick={handleExport}
          className="p-1 text-muted hover:text-[var(--text-primary)] transition-colors"
          title="Export"
        >
          <Download size={11} />
        </button>
      }
    >
      {/*
      //
      // Info bar — matches chain execution header style.
      //
      */}
      <div className="px-3 py-1.5 border-b border-subtle bg-[var(--bg-secondary)] flex-shrink-0 overflow-hidden">
        <div className="flex items-baseline gap-3 text-[10px] whitespace-nowrap">
          <div className="flex items-baseline">
            <span className="text-muted">Status:</span>
            <span className={`ml-2 font-mono ${statusColor(operation.status)}`}>{operation.status}</span>
          </div>
          <div className="flex items-baseline">
            <span className="text-muted">Agent:</span>
            <span className="ml-2 font-mono">{operation.agent_short_name}</span>
          </div>
          <div className="flex items-baseline">
            <span className="text-muted">Mode:</span>
            <span className="ml-2">{operation.spec.mode}</span>
          </div>
          <div className="flex items-baseline">
            <span className="text-muted">Duration:</span>
            <span className="ml-2">{formatDuration(operation.start_time, operation.end_time, operation.status)}</span>
          </div>
          <span className="text-[9px] font-mono text-muted/30 ml-auto truncate min-w-0">{operation.operation_id}</span>
        </div>
      </div>

      <div className="flex-1 overflow-auto p-3 space-y-3">

        {/*
        //
        // Summary.
        //
        */}
        {(operation.summary || operation.result) && (
          <div>
            <button
              onClick={() => setSummaryCollapsed(!summaryCollapsed)}
              className="flex items-center gap-1.5 text-[10px] text-muted hover:text-[var(--text-primary)] transition-colors"
            >
              {summaryCollapsed ? <ChevronRight size={11} /> : <ChevronDown size={11} />}
              Summary
              {operation.result && operation.spec.mode !== 'one-shot' && (
                <span className="px-1 py-0.5 text-[9px] font-mono bg-[var(--bg-tertiary)] border border-dim">
                  {operation.result}
                </span>
              )}
            </button>
            {!summaryCollapsed && (operation.summary || operation.result) && (
              <div className="mt-1.5 w-full bg-[var(--bg-secondary)] p-2 max-h-48 overflow-auto prose prose-invert max-w-none text-[10px] text-[var(--text-secondary)] leading-relaxed [&_p]:my-0.5 [&_ul]:my-0.5 [&_li]:my-0 [&_h2]:text-[11px] [&_h3]:text-[10px] [&_pre]:whitespace-pre [&_pre]:font-mono">
                <ReactMarkdown remarkPlugins={[remarkGfm, remarkBreaks]}>{(operation.summary || operation.result)!}</ReactMarkdown>
              </div>
            )}
          </div>
        )}

        {/*
        //
        // Prompt.
        //
        */}
        <div>
          <button
            onClick={() => setPromptCollapsed(!promptCollapsed)}
            className="flex items-center gap-1.5 text-[10px] text-muted mb-1 hover:text-[var(--text-primary)] transition-colors"
          >
            {promptCollapsed ? <ChevronRight size={11} /> : <ChevronDown size={11} />}
            Prompt
          </button>
          {!promptCollapsed && (
            <div className="bg-[var(--bg-secondary)] p-2 text-[var(--text-secondary)]">
              <pre className="text-[10px] whitespace-pre-wrap font-mono">{operation.spec.operation_prompt}</pre>
            </div>
          )}
        </div>

        {/*
        //
        // Output.
        //
        */}
        {operation.output && (
          <div>
            <button
              onClick={() => setOutputCollapsed(!outputCollapsed)}
              className="flex items-center gap-1.5 text-[10px] text-muted mb-1 hover:text-[var(--text-primary)] transition-colors"
            >
              {outputCollapsed ? <ChevronRight size={11} /> : <ChevronDown size={11} />}
              Output
            </button>
            {!outputCollapsed && (
              <div ref={outputRef} className="bg-[var(--bg-secondary)] p-2 max-h-64 overflow-auto text-[var(--text-secondary)]">
                <StyledOutput output={operation.output} />
              </div>
            )}
          </div>
        )}
      </div>
    </FloatingPanel>
  );
}
