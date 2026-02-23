import { useState } from 'react';
import {
  Zap,
  GitBranch,
  ChevronUp,
  ChevronDown,
  BookOpen,
  Crosshair,
  Shield,
  Loader2,
  X,
} from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { getOperationStatusColor, StatusBadge } from '../common/StatusBadge';
import { OperationDetailModal } from '../common/OperationDetailModal';
import { ChainExecutionModal } from '../common/ChainExecutionModal';
import { LibraryModal } from './LibraryModal';
import { TrafficModal } from './TrafficModal';
import { HuntingModal } from './HuntingModal';
import type { SemanticOpUpdate } from '../../api/types';

export function ActivityBar() {
  const { state, cancelOperation, cancelChainExecution } = useApp();
  const [expanded, setExpanded] = useState(false);
  const [selectedOp, setSelectedOp] = useState<SemanticOpUpdate | null>(null);
  const [selectedChainExecId, setSelectedChainExecId] = useState<string | null>(null);
  const [showLibrary, setShowLibrary] = useState(false);
  const [showTraffic, setShowTraffic] = useState(false);
  const [showHunting, setShowHunting] = useState(false);

  const runningOps = state.operations.filter(op => op.status === 'Running');
  const recentOps = state.operations.slice(0, 10);
  const runningChains = state.chains.executions.filter(e => e.status === 'Running' || e.status === 'Queued');
  const recentChains = state.chains.executions.slice(0, 10);
  const totalRunning = runningOps.length + runningChains.length;

  const selectedChainExec = selectedChainExecId
    ? state.chains.executions.find(e => e.execution_id === selectedChainExecId) ?? null
    : null;

  return (
    <>
      <div className="border-t border-subtle bg-[var(--bg-secondary)] flex-shrink-0">
        {/*
        //
        // Collapsed bar.
        //
        */}
        <div className="flex items-center justify-between px-3 py-1.5">
          <div className="flex items-center gap-3 overflow-hidden">
            {/*
            //
            // Running ops/chains indicators.
            //
            */}
            {runningOps.slice(0, 3).map(op => (
              <button
                key={op.operation_id}
                onClick={() => setSelectedOp(op)}
                className="flex items-center gap-1.5 text-[10px] text-[var(--accent-info)] hover:text-[var(--text-primary)] transition-colors truncate max-w-[200px]"
              >
                <Loader2 size={10} className="animate-spin flex-shrink-0" />
                <Zap size={9} className="flex-shrink-0" />
                <span className="truncate">{op.spec.name}</span>
              </button>
            ))}

            {runningChains.slice(0, 3).map(exec => (
              <button
                key={exec.execution_id}
                onClick={() => setSelectedChainExecId(exec.execution_id)}
                className="flex items-center gap-1.5 text-[10px] text-[var(--accent-info)] hover:text-[var(--text-primary)] transition-colors truncate max-w-[200px]"
              >
                <Loader2 size={10} className="animate-spin flex-shrink-0" />
                <GitBranch size={9} className="flex-shrink-0" />
                <span className="truncate">{exec.chain_name}</span>
              </button>
            ))}

            {totalRunning === 0 && (
              <span className="text-[10px] text-muted">No active operations</span>
            )}
          </div>

          <div className="flex items-center gap-2">
            {/*
            //
            // Quick links.
            //
            */}
            <button
              onClick={() => setShowLibrary(true)}
              className="flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted hover:text-[var(--text-primary)] transition-colors"
            >
              <BookOpen size={10} /> Library
            </button>
            <button
              onClick={() => setShowHunting(true)}
              className="flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted hover:text-[var(--text-primary)] transition-colors"
            >
              <Crosshair size={10} /> Hunting
            </button>
            <button
              onClick={() => setShowTraffic(true)}
              className="flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted hover:text-[var(--text-primary)] transition-colors"
            >
              <Shield size={10} /> Traffic
            </button>

            <button
              onClick={() => setExpanded(!expanded)}
              className="p-1 text-muted hover:text-[var(--text-primary)] transition-colors"
              title={expanded ? 'Collapse' : 'Expand'}
            >
              {expanded ? <ChevronDown size={12} /> : <ChevronUp size={12} />}
            </button>
          </div>
        </div>

        {/*
        //
        // Expanded panel with full activity list.
        //
        */}
        {expanded && (
          <div className="border-t border-subtle max-h-48 overflow-auto px-3 py-2 space-y-1">
            {recentOps.length === 0 && recentChains.length === 0 ? (
              <div className="text-[10px] text-muted text-center py-3">No recent operations</div>
            ) : (
              <>
                {recentOps.map(op => (
                  <div
                    key={op.operation_id}
                    onClick={() => setSelectedOp(op)}
                    className="flex items-center justify-between py-1 px-2 hover:bg-[var(--highlight)] transition-colors cursor-pointer text-[10px]"
                  >
                    <div className="flex items-center gap-2 min-w-0">
                      <Zap size={10} className="text-[var(--accent-purple)] flex-shrink-0" />
                      <span className="text-highlight truncate">{op.spec.name}</span>
                      <span className="text-muted truncate">{op.agent_short_name}</span>
                    </div>
                    <div className="flex items-center gap-2 flex-shrink-0">
                      <StatusBadge status={getOperationStatusColor(op.status)} label={op.status} />
                      {op.status === 'Running' && (
                        <button
                          onClick={e => { e.stopPropagation(); cancelOperation(op.operation_id); }}
                          className="p-0.5 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors"
                          title="Cancel"
                        >
                          <X size={10} />
                        </button>
                      )}
                    </div>
                  </div>
                ))}
                {recentChains.map(exec => (
                  <div
                    key={exec.execution_id}
                    onClick={() => setSelectedChainExecId(exec.execution_id)}
                    className="flex items-center justify-between py-1 px-2 hover:bg-[var(--highlight)] transition-colors cursor-pointer text-[10px]"
                  >
                    <div className="flex items-center gap-2 min-w-0">
                      <GitBranch size={10} className="text-[var(--accent-info)] flex-shrink-0" />
                      <span className="text-highlight truncate">{exec.chain_name}</span>
                      <span className="text-muted truncate">{exec.agent_short_name}</span>
                    </div>
                    <div className="flex items-center gap-2 flex-shrink-0">
                      <StatusBadge status={getOperationStatusColor(exec.status)} label={exec.status} />
                      {(exec.status === 'Running' || exec.status === 'Queued') && (
                        <button
                          onClick={e => { e.stopPropagation(); cancelChainExecution(exec.execution_id); }}
                          className="p-0.5 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors"
                          title="Cancel"
                        >
                          <X size={10} />
                        </button>
                      )}
                    </div>
                  </div>
                ))}
              </>
            )}
          </div>
        )}
      </div>

      {/*
      //
      // Detail modals.
      //
      */}
      <OperationDetailModal
        operation={selectedOp}
        onClose={() => setSelectedOp(null)}
      />

      <ChainExecutionModal
        execution={selectedChainExec}
        chain={null}
        onClose={() => setSelectedChainExecId(null)}
      />

      {showLibrary && (
        <LibraryModal onClose={() => setShowLibrary(false)} />
      )}

      {showTraffic && (
        <TrafficModal onClose={() => setShowTraffic(false)} />
      )}

      {showHunting && (
        <HuntingModal onClose={() => setShowHunting(false)} />
      )}
    </>
  );
}
