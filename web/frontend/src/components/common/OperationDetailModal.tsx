import { useState, useRef, useEffect } from 'react';
import { Download, ChevronDown, ChevronRight } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import remarkBreaks from 'remark-breaks';
import { Modal } from './Modal';
import { StyledOutput } from './StyledOutput';
import { exportOperationResult, downloadTextFile } from '../../utils/export';
import type { SemanticOpUpdate } from '../../api/types';

interface OperationDetailModalProps {
  operation: SemanticOpUpdate | null;
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

export function OperationDetailModal({ operation, onClose }: OperationDetailModalProps) {
  const outputRef = useRef<HTMLDivElement>(null);
  const [summaryCollapsed, setSummaryCollapsed] = useState(false);
  const [promptCollapsed, setPromptCollapsed] = useState(true);
  const [outputCollapsed, setOutputCollapsed] = useState(false);

  //
  // Autoscroll output when it changes (for live updates during execution).
  //
  useEffect(() => {
    if (outputRef.current && operation?.status === 'Running') {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [operation?.output, operation?.status]);

  const handleExport = () => {
    if (!operation) return;
    const content = exportOperationResult(operation);
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    downloadTextFile(content, `operation-${operation.spec.name}-${timestamp}.md`);
  };

  return (
    <Modal
      isOpen={operation !== null}
      onClose={onClose}
      title={`Semantic Operation: ${operation?.spec.name ?? ''}`}
      size="lg"
      headerActions={operation && (
        <button
          onClick={handleExport}
          className="p-1 hover:bg-[var(--bg-tertiary)] text-muted hover:text-[var(--text-primary)] transition-colors"
          title="Export as Markdown"
        >
          <Download size={20} />
        </button>
      )}
    >
      {operation && (
        <div className="space-y-4">
          {/*
          //
          // Info.
          //
          */}
          <div className="flex items-baseline gap-4 text-[11px] whitespace-nowrap flex-wrap py-1.5">
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
          </div>
          <div className="text-[10px] font-mono text-muted/50">{operation.operation_id}</div>

          {/*
          //
          // Summary (collapsible) with Result tag, markdown content.
          //
          */}
          {(operation.summary || operation.result) && (
            <div>
              <button
                onClick={() => setSummaryCollapsed(!summaryCollapsed)}
                className="flex items-center gap-2 text-xs text-muted hover:text-[var(--text-primary)] transition-colors"
              >
                {summaryCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
                Summary
                {operation.result && operation.spec.mode !== 'one-shot' && (
                  <span className="px-1.5 py-0.5 text-[10px] font-mono bg-[var(--bg-tertiary)] border border-dim">
                    {operation.result}
                  </span>
                )}
              </button>
              {!summaryCollapsed && (operation.summary || operation.result) && (
                <div className="mt-2 w-full bg-[var(--bg-secondary)] p-3 max-h-64 overflow-auto prose prose-sm prose-invert max-w-none text-xs text-[var(--text-secondary)] [&_p]:my-1 [&_ul]:my-1 [&_li]:my-0 [&_h2]:text-sm [&_h2]:mt-2 [&_h2]:mb-1 [&_h3]:text-xs [&_h3]:mt-1 [&_h3]:mb-0.5 [&_pre]:whitespace-pre [&_pre]:font-mono">
                  <ReactMarkdown remarkPlugins={[remarkGfm, remarkBreaks]}>
                    {(operation.summary || operation.result)!}
                  </ReactMarkdown>
                </div>
              )}
            </div>
          )}

          {/*
          //
          // Prompt (collapsible, collapsed by default).
          //
          */}
          <div>
            <button
              onClick={() => setPromptCollapsed(!promptCollapsed)}
              className="flex items-center gap-1 text-xs text-muted mb-1 hover:text-[var(--text-primary)] transition-colors"
            >
              {promptCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
              Prompt
            </button>
            {!promptCollapsed && (
              <div className="bg-[var(--bg-secondary)] p-3 text-[var(--text-secondary)]">
                <pre className="text-xs whitespace-pre-wrap font-mono">
                  {operation.spec.operation_prompt}
                </pre>
              </div>
            )}
          </div>

          {/*
          //
          // Output (collapsible, with autoscroll during execution).
          //
          */}
          {operation.output && (
            <div>
              <button
                onClick={() => setOutputCollapsed(!outputCollapsed)}
                className="flex items-center gap-1 text-xs text-muted mb-1 hover:text-[var(--text-primary)] transition-colors"
              >
                {outputCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
                Output
              </button>
              {!outputCollapsed && (
                <div
                  ref={outputRef}
                  className="bg-[var(--bg-secondary)] p-3 max-h-96 overflow-auto text-[var(--text-secondary)]"
                >
                  <StyledOutput output={operation.output} />
                </div>
              )}
            </div>
          )}


        </div>
      )}
    </Modal>
  );
}
