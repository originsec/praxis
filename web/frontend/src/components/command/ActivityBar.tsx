import { useState, useCallback, useRef } from 'react';
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
  GripHorizontal,
} from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { getOperationStatusColor, StatusBadge } from '../common/StatusBadge';
import { OperationDetailModal } from '../common/OperationDetailModal';
import { ChainExecutionModal } from '../common/ChainExecutionModal';
import { LibraryModal } from './LibraryModal';
import { TrafficModal } from './TrafficModal';
import { HuntingModal } from './HuntingModal';
import type { SemanticOpUpdate } from '../../api/types';

const DEFAULT_PANEL_HEIGHT = 200;
const MIN_PANEL_HEIGHT = 80;
const MAX_PANEL_HEIGHT = 600;

export function ActivityBar() {
  const { state, cancelOperation, cancelChainExecution } = useApp();
  const [expanded, setExpanded] = useState(false);
  const [panelHeight, setPanelHeight] = useState(DEFAULT_PANEL_HEIGHT);
  const [selectedOp, setSelectedOp] = useState<SemanticOpUpdate | null>(null);
  const [selectedChainExecId, setSelectedChainExecId] = useState<string | null>(null);
  const [showLibrary, setShowLibrary] = useState(false);
  const [showTraffic, setShowTraffic] = useState(false);
  const [showHunting, setShowHunting] = useState(false);

  const runningOps = state.operations.filter(op => op.status === 'Running');
  const allOps = state.operations;
  const runningChains = state.chains.executions.filter(e => e.status === 'Running' || e.status === 'Queued');
  const allChains = state.chains.executions;
  const totalRunning = runningOps.length + runningChains.length;

  const selectedChainExec = selectedChainExecId
    ? state.chains.executions.find(e => e.execution_id === selectedChainExecId) ?? null
    : null;

  //
  // Drag-to-resize the expanded panel (drag upward to grow).
  //
  const isDragging = useRef(false);
  const dragStartY = useRef(0);
  const dragStartHeight = useRef(0);

  const handleResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    isDragging.current = true;
    dragStartY.current = e.clientY;
    dragStartHeight.current = panelHeight;
    document.body.style.cursor = 'row-resize';
    document.body.style.userSelect = 'none';

    const handleMouseMove = (ev: MouseEvent) => {
      if (!isDragging.current) return;
      const delta = dragStartY.current - ev.clientY;
      setPanelHeight(Math.max(MIN_PANEL_HEIGHT, Math.min(MAX_PANEL_HEIGHT, dragStartHeight.current + delta)));
    };

    const handleMouseUp = () => {
      isDragging.current = false;
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
  }, [panelHeight]);

  return (
    <>
      <div className="border-t border-subtle bg-[var(--bg-secondary)] flex-shrink-0 flex flex-col">
        {/*
        //
        // Expanded panel — above the summary bar, with resize handle on top.
        //
        */}
        {expanded && (
          <>
            {/*
            //
            // Resize handle.
            //
            */}
            <div
              onMouseDown={handleResizeStart}
              className="flex-shrink-0 h-[5px] cursor-row-resize group relative bg-[var(--bg-secondary)] hover:bg-[var(--highlight)] transition-colors border-b border-subtle"
            >
              <div className="absolute inset-x-0 top-1/2 -translate-y-1/2 flex justify-center">
                <GripHorizontal size={10} className="text-[var(--border-subtle)] group-hover:text-[var(--text-muted)] transition-colors" />
              </div>
            </div>

            {/*
            //
            // Scrollable operations list.
            //
            */}
            <div
              className="overflow-auto px-3 py-2 space-y-0.5"
              style={{ height: panelHeight }}
            >
              {allOps.length === 0 && allChains.length === 0 ? (
                <div className="text-[10px] text-muted text-center py-6">No operations or chain executions</div>
              ) : (
                <>
                  {allOps.map(op => (
                    <div
                      key={op.operation_id}
                      onClick={() => setSelectedOp(op)}
                      className="flex items-center justify-between py-1 px-2 hover:bg-[var(--highlight)] transition-colors cursor-pointer text-[10px]"
                    >
                      <div className="flex items-center gap-2 min-w-0">
                        <Zap size={10} className="text-[var(--accent-purple)] flex-shrink-0" />
                        <span className="text-highlight truncate">{op.spec.name}</span>
                        <span className="text-muted truncate">{op.agent_short_name}</span>
                        <span className="text-[9px] text-muted/50">{new Date(op.start_time).toLocaleTimeString()}</span>
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
                  {allChains.map(exec => (
                    <div
                      key={exec.execution_id}
                      onClick={() => setSelectedChainExecId(exec.execution_id)}
                      className="flex items-center justify-between py-1 px-2 hover:bg-[var(--highlight)] transition-colors cursor-pointer text-[10px]"
                    >
                      <div className="flex items-center gap-2 min-w-0">
                        <GitBranch size={10} className="text-[var(--accent-info)] flex-shrink-0" />
                        <span className="text-highlight truncate">{exec.chain_name}</span>
                        <span className="text-muted truncate">{exec.agent_short_name}</span>
                        <span className="text-[9px] text-muted/50">{new Date(exec.started_at).toLocaleTimeString()}</span>
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
          </>
        )}

        {/*
        //
        // Summary bar — always visible. Arrow on left, status clickable.
        //
        */}
        <div className="flex items-center px-3 py-1.5 border-t border-subtle gap-2">
          <button
            onClick={() => setExpanded(!expanded)}
            className="p-1 text-muted hover:text-[var(--text-primary)] transition-colors flex-shrink-0"
            title={expanded ? 'Collapse' : 'Expand'}
          >
            {expanded ? <ChevronDown size={12} /> : <ChevronUp size={12} />}
          </button>

          <div
            onClick={() => setExpanded(!expanded)}
            className="flex items-center gap-3 overflow-hidden cursor-pointer flex-1 min-w-0"
          >
            {runningOps.slice(0, 3).map(op => (
              <span
                key={op.operation_id}
                onClick={e => { e.stopPropagation(); setSelectedOp(op); }}
                className="flex items-center gap-1.5 text-[10px] text-[var(--accent-info)] hover:text-[var(--text-primary)] transition-colors truncate max-w-[200px] cursor-pointer"
              >
                <Loader2 size={10} className="animate-spin flex-shrink-0" />
                <Zap size={9} className="flex-shrink-0" />
                <span className="truncate">{op.spec.name}</span>
              </span>
            ))}

            {runningChains.slice(0, 3).map(exec => (
              <span
                key={exec.execution_id}
                onClick={e => { e.stopPropagation(); setSelectedChainExecId(exec.execution_id); }}
                className="flex items-center gap-1.5 text-[10px] text-[var(--accent-info)] hover:text-[var(--text-primary)] transition-colors truncate max-w-[200px] cursor-pointer"
              >
                <Loader2 size={10} className="animate-spin flex-shrink-0" />
                <GitBranch size={9} className="flex-shrink-0" />
                <span className="truncate">{exec.chain_name}</span>
              </span>
            ))}

            {totalRunning === 0 && (
              <span className="text-[10px] text-muted">No active operations</span>
            )}

            {expanded && (
              <span className="text-[9px] text-muted ml-2">
                {allOps.length + allChains.length} total
              </span>
            )}
          </div>

          <div className="flex items-center gap-2 flex-shrink-0">
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
          </div>
        </div>
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
