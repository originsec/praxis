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
  const [promptCollapsed, setPromptCollapsed] = useState(false);
  const [outputCollapsed, setOutputCollapsed] = useState(false);
  const [resultCollapsed, setResultCollapsed] = useState(false);

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
            <div className="col-span-4 mt-1">
              <span className="text-muted">{operation.spec.description}</span>
            </div>
          </div>

          {/*
          //
          // Prompt (collapsible).
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
                <pre className="text-sm whitespace-pre-wrap font-mono">
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
          // Result - actual findings/data/output (collapsible).
          //
          */}
          {operation.result && (
            <div>
              <button
                onClick={() => setResultCollapsed(!resultCollapsed)}
                className="flex items-center gap-1 text-xs text-muted mb-1 hover:text-[var(--text-primary)] transition-colors"
              >
                {resultCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
                Result
              </button>
              {!resultCollapsed && (
                <div className="bg-[var(--bg-secondary)] p-3 max-h-64 overflow-auto prose prose-sm prose-invert max-w-none [&_p]:my-1 [&_ul]:my-1 [&_li]:my-0 [&_h2]:text-base [&_h2]:mt-3 [&_h2]:mb-2 [&_h3]:text-sm [&_h3]:mt-2 [&_h3]:mb-1">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>
                    {operation.result}
                  </ReactMarkdown>
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </Modal>
  );
}
