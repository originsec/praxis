import { Download } from 'lucide-react';
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
        <div className="space-y-6">
          {/*
          //
          // Info.
          //
          */}
          <div className="grid grid-cols-2 gap-4">
            <div className="col-span-2">
              <p className="text-xs text-muted mb-1">Operation ID</p>
              <p className="text-sm font-mono">{operation.operation_id}</p>
            </div>
            <div>
              <p className="text-xs text-muted mb-1">Status</p>
              <StatusBadge
                status={getOperationStatusColor(operation.status)}
                label={operation.status}
              />
            </div>
            <div>
              <p className="text-xs text-muted mb-1">Agent</p>
              <p className="text-sm">{operation.agent_short_name}</p>
            </div>
            <div>
              <p className="text-xs text-muted mb-1">Mode</p>
              <p className="text-sm">{operation.spec.mode}</p>
            </div>
            <div>
              <p className="text-xs text-muted mb-1">Duration</p>
              <p className="text-sm">
                {formatDuration(operation.start_time, operation.end_time)}
              </p>
            </div>
          </div>

          {/*
          //
          // Description.
          //
          */}
          <div>
            <p className="text-xs text-muted mb-1">Description</p>
            <p className="text-sm text-muted">{operation.spec.description}</p>
          </div>

          {/*
          //
          // Prompt.
          //
          */}
          <div>
            <p className="text-xs text-muted mb-1">Prompt</p>
            <div className="bg-[var(--bg-secondary)] p-3">
              <pre className="text-sm whitespace-pre-wrap font-mono">
                {operation.spec.operation_prompt}
              </pre>
            </div>
          </div>

          {/*
          //
          // Output.
          //
          */}
          {operation.output && (
            <div>
              <p className="text-xs text-muted mb-1">Output</p>
              <div className="bg-[var(--bg-secondary)] p-3 max-h-96 overflow-auto">
                <StyledOutput output={operation.output} />
              </div>
            </div>
          )}

          {/*
          //
          // Result.
          //
          */}
          {operation.result && (
            <div>
              <p className="text-xs text-muted mb-1">Result</p>
              <div className="bg-[var(--bg-secondary)] p-3 max-h-64 overflow-auto">
                <pre className="text-sm whitespace-pre-wrap font-mono">
                  {operation.result}
                </pre>
              </div>
            </div>
          )}
        </div>
      )}
    </Modal>
  );
}
