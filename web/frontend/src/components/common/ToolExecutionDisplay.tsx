import { useState } from 'react';
import {
  Loader2,
  CheckCircle,
  XCircle,
  Wrench,
  ChevronRight,
  ChevronDown,
} from 'lucide-react';
import type { OrchestratorToolExecution } from '../../context/orchestratorTypes';

function ToolExecutionItem({ exec }: { exec: OrchestratorToolExecution }) {
  const [expanded, setExpanded] = useState(false);
  const canExpand = exec.input || exec.result;

  return (
    <div
      className={`text-[10px] px-2 py-1 rounded cursor-pointer ${
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
        {!exec.executing && <span className="text-[var(--text-highlight)]/60">- {exec.display}</span>}
      </div>
      {expanded && (
        <div className="mt-2 ml-5 space-y-2">
          {exec.input && (
            <div className="p-2 bg-[var(--bg-primary)] rounded border border-subtle text-[var(--text-muted)] font-mono text-[10px] overflow-x-auto max-h-48 overflow-y-auto whitespace-pre-wrap break-all">
              <span className="text-[var(--text-highlight)]/40 select-none">input: </span>
              {(() => {
                try {
                  return JSON.stringify(JSON.parse(exec.input), null, 2);
                } catch {
                  return exec.input;
                }
              })()}
            </div>
          )}
          {exec.result && (
            <div className="p-2 bg-[var(--bg-primary)] rounded border border-subtle text-[var(--text-muted)] font-mono text-[10px] overflow-x-auto max-h-48 overflow-y-auto whitespace-pre-wrap break-all">
              <span className="text-[var(--text-highlight)]/40 select-none">result: </span>
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
      )}
    </div>
  );
}

export function ToolExecutionDisplay({
  executions,
  collapsible = false,
}: {
  executions: OrchestratorToolExecution[];
  collapsible?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  if (executions.length === 0) return null;

  if (!collapsible) {
    return (
      <div className="space-y-1 mb-2">
        {executions.map((exec, idx) => (
          <ToolExecutionItem key={idx} exec={exec} />
        ))}
      </div>
    );
  }

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
