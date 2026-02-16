import { useState, useRef, useEffect } from 'react';
import { Download, ChevronDown, ChevronRight } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Modal } from './Modal';
import { StatusBadge, getOperationStatusColor } from './StatusBadge';
import { StyledOutput } from './StyledOutput';
import { exportOperationResult, downloadTextFile } from '../../utils/export';
import type { SemanticOpUpdate } from '../../api/types';

interface OperationDetailModalProps {
  operation: SemanticOpUpdate | null;
  onClose: () => void;
}

function formatDuration(start: string, end: string | null): string {
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
          <div className="grid grid-cols-4 gap-x-4 gap-y-1 text-[11px]">
            <div className="col-span-4">
              <span className="text-muted">ID:</span>{' '}
              <span className="font-mono">{operation.operation_id}</span>
            </div>
            <div>
              <span className="text-muted">Status:</span>{' '}
              <StatusBadge
                status={getOperationStatusColor(operation.status)}
                label={operation.status}
              />
            </div>
            <div>
              <span className="text-muted">Agent:</span>{' '}
              <span>{operation.agent_short_name}</span>
            </div>
            <div>
              <span className="text-muted">Mode:</span>{' '}
              <span>{operation.spec.mode}</span>
            </div>
            <div>
              <span className="text-muted">Duration:</span>{' '}
              <span>{formatDuration(operation.start_time, operation.end_time)}</span>
            </div>
            {operation.spec.description && (
              <div className="col-span-4 mt-1">
                <span className="text-muted">{operation.spec.description}</span>
              </div>
            )}
          </div>

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
              <div className="bg-[var(--bg-secondary)] p-3">
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
                  className="bg-[var(--bg-secondary)] p-3 max-h-96 overflow-auto"
                >
                  <StyledOutput output={operation.output} />
                </div>
              )}
            </div>
          )}

          {/*
          //
          // Summary (collapsible) with Result tag.
          //
          */}
          {(operation.summary || operation.result) && (
            <div>
              <button
                onClick={() => setSummaryCollapsed(!summaryCollapsed)}
                className="flex items-center gap-2 text-xs text-muted mb-1 hover:text-[var(--text-primary)] transition-colors"
              >
                {summaryCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
                Summary
                {operation.result && (
                  <span className="px-1.5 py-0.5 text-[10px] font-mono bg-[var(--bg-tertiary)] border border-dim">
                    {operation.result}
                  </span>
                )}
              </button>
              {!summaryCollapsed && operation.summary && (
                <div className="bg-[var(--bg-secondary)] p-3">
                  <p className="text-xs">{operation.summary}</p>
                </div>
              )}
            </div>
          )}

        </div>
      )}
    </Modal>
  );
}
