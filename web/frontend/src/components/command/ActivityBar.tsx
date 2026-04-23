import { useState, useCallback, useRef, useMemo } from 'react';
import {
  Zap,
  GitBranch,
  ChevronUp,
  ChevronDown,
  BookOpen,
  Timer,
  Crosshair,
  Shield,
  Loader2,
  X,
  Trash2,
  Wrench,
} from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { getOperationStatusColor, StatusBadge } from '../common/StatusBadge';
import { OperationDetailFloating } from './OperationDetailFloating';
import { ChainExecutionFloating } from './ChainExecutionFloating';
import { LibraryModal } from './LibraryModal';
import { TriggersModal } from './TriggersModal';
import { TrafficModal } from './TrafficModal';
import { LogQueryModal } from './LogQueryModal';
import { ToolkitModal } from './ToolkitModal';


const DEFAULT_PANEL_HEIGHT = 200;
const MIN_PANEL_HEIGHT = 80;
const MAX_PANEL_HEIGHT = 600;

export function ActivityBar() {
  const { state, cancelOperation, cancelChainExecution, removeOperation, removeChainExecution, clearOperations, clearChainExecutions } = useApp();
  const nodes = state.systemState?.nodes || [];
  const nodeName = (nodeId: string) => nodes.find(n => n.node_id === nodeId)?.machine_name || nodeId.slice(0, 8);
  const [expanded, setExpanded] = useState(false);
  const [panelHeight, setPanelHeight] = useState(DEFAULT_PANEL_HEIGHT);
  const [selectedOpId, setSelectedOpId] = useState<string | null>(null);
  const selectedOp = useMemo(() => {
    if (!selectedOpId) return null;
    return state.operations.find(op => op.operation_id === selectedOpId) ?? null;
  }, [selectedOpId, state.operations]);
  const [selectedChainExecId, setSelectedChainExecId] = useState<string | null>(null);
  const [showLibrary, setShowLibrary] = useState(false);
  const [showTriggers, setShowTriggers] = useState(false);
  const [showTraffic, setShowTraffic] = useState(false);
  const [showLogQuery, setShowLogQuery] = useState(false);
  const [showToolkit, setShowToolkit] = useState(false);

  const runningOps = state.operations.filter(op => op.status === 'Running');
  const runningChains = state.chains.executions.filter(e => e.status === 'Running' || e.status === 'Queued');
  const totalRunning = runningOps.length + runningChains.length;

  //
  // Merge ops and chains into a single time-sorted list (newest first).
  //
  const allItems: ({ kind: 'op'; op: typeof state.operations[0] } | { kind: 'chain'; exec: typeof state.chains.executions[0] })[] = [
    ...state.operations.map(op => ({ kind: 'op' as const, op, time: new Date(op.start_time).getTime() })),
    ...state.chains.executions.map(exec => ({ kind: 'chain' as const, exec, time: new Date(exec.started_at).getTime() })),
  ].sort((a, b) => b.time - a.time);

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
            // Resize handle + clear button.
            //
            */}
            <div className="flex items-center flex-shrink-0 relative">
              <div
                onMouseDown={handleResizeStart}
                className="absolute inset-x-0 top-0 h-1 cursor-row-resize bg-transparent hover:bg-[var(--accent-info)]/30 active:bg-[var(--accent-info)]/50 transition-colors z-10"
              />
              {allItems.length > 0 && (
                <button
                  onClick={() => { clearOperations(); clearChainExecutions(); }}
                  className="ml-auto px-2 py-0.5 text-[9px] text-muted/50 hover:text-[var(--accent-error)] transition-colors flex-shrink-0"
                  title="Clear all finished"
                >
                  Clear
                </button>
              )}
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
              {allItems.length === 0 ? (
                <div className="text-[10px] text-muted text-center py-6">No operations or chain executions</div>
              ) : (
                <>
                  {allItems.map(item => item.kind === 'op' ? (
                    <div
                      key={item.op.operation_id}
                      onClick={() => setSelectedOpId(item.op.operation_id)}
                      className="flex items-center justify-between py-1 px-2 hover:bg-[var(--highlight)] transition-colors cursor-pointer text-[10px] group/row"
                    >
                      <div className="flex items-center gap-2 min-w-0">
                        <Zap size={10} className="text-[var(--accent-purple)] flex-shrink-0" />
                        <span className="text-highlight truncate">{item.op.spec.name}</span>
                        <span className="text-muted truncate">{item.op.agent_short_name}</span>
                        <span className="text-[9px] text-muted/50 truncate">{nodeName(item.op.node_id)}</span>
                        <span className="text-[9px] text-muted opacity-70">{new Date(item.op.start_time).toLocaleTimeString()}</span>
                      </div>
                      <div className="flex items-center gap-1.5 flex-shrink-0">
                        <StatusBadge status={getOperationStatusColor(item.op.status)} label={item.op.status} />
                        {item.op.status === 'Running' && (
                          <button
                            onClick={e => { e.stopPropagation(); cancelOperation(item.op.operation_id); }}
                            className="p-0.5 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors"
                            title="Cancel"
                          >
                            <X size={10} />
                          </button>
                        )}
                        {item.op.status !== 'Running' && (
                          <button
                            onClick={e => { e.stopPropagation(); removeOperation(item.op.operation_id); }}
                            className="p-0.5 text-muted/30 hover:text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors opacity-0 group-hover/row:opacity-100"
                            title="Remove"
                          >
                            <Trash2 size={10} />
                          </button>
                        )}
                      </div>
                    </div>
                  ) : (
                    <div
                      key={item.exec.execution_id}
                      onClick={() => setSelectedChainExecId(item.exec.execution_id)}
                      className="flex items-center justify-between py-1 px-2 hover:bg-[var(--highlight)] transition-colors cursor-pointer text-[10px] group/row"
                    >
                      <div className="flex items-center gap-2 min-w-0">
                        <GitBranch size={10} className="text-[var(--accent-info)] flex-shrink-0" />
                        <span className="text-highlight truncate">{item.exec.chain_name}</span>
                        <span className="text-muted truncate">{item.exec.agent_short_name}</span>
                        <span className="text-[9px] text-muted/50 truncate">{nodeName(item.exec.node_id)}</span>
                        <span className="text-[9px] text-muted opacity-70">{new Date(item.exec.started_at).toLocaleTimeString()}</span>
                      </div>
                      <div className="flex items-center gap-1.5 flex-shrink-0">
                        <StatusBadge status={getOperationStatusColor(item.exec.status)} label={item.exec.status} />
                        {(item.exec.status === 'Running' || item.exec.status === 'Queued') && (
                          <button
                            onClick={e => { e.stopPropagation(); cancelChainExecution(item.exec.execution_id); }}
                            className="p-0.5 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors"
                            title="Cancel"
                          >
                            <X size={10} />
                          </button>
                        )}
                        {item.exec.status !== 'Running' && item.exec.status !== 'Queued' && (
                          <button
                            onClick={e => { e.stopPropagation(); removeChainExecution(item.exec.execution_id); }}
                            className="p-0.5 text-muted/30 hover:text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors opacity-0 group-hover/row:opacity-100"
                            title="Remove"
                          >
                            <Trash2 size={10} />
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
                onClick={e => { e.stopPropagation(); setSelectedOpId(op.operation_id); }}
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
                {allItems.length} total
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
              onClick={() => setShowTriggers(true)}
              className="flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted hover:text-[var(--text-primary)] transition-colors"
            >
              <Timer size={10} /> Triggers
            </button>
            <div className="w-px h-3.5 bg-[var(--text-muted)]" />
            <button
              onClick={() => setShowTraffic(true)}
              className="flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted hover:text-[var(--text-primary)] transition-colors"
            >
              <Shield size={10} /> Traffic
            </button>
            <button
              onClick={() => setShowLogQuery(true)}
              className="flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted hover:text-[var(--text-primary)] transition-colors"
            >
              <Crosshair size={10} /> Log Query
            </button>
            <div className="w-px h-3.5 bg-[var(--text-muted)]" />
            <button
              onClick={() => setShowToolkit(true)}
              className="flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted hover:text-[var(--text-primary)] transition-colors"
            >
              <Wrench size={10} /> Toolkit
            </button>
          </div>
        </div>
      </div>

      {/*
      //
      // Detail modals.
      //
      */}
      {selectedOp && (
        <OperationDetailFloating
          operation={selectedOp}
          onClose={() => setSelectedOpId(null)}
        />
      )}

      {selectedChainExec && (
        <ChainExecutionFloating
          execution={selectedChainExec}
          onClose={() => setSelectedChainExecId(null)}
        />
      )}

      {showLibrary && (
        <LibraryModal onClose={() => setShowLibrary(false)} />
      )}

      {showTriggers && (
        <TriggersModal onClose={() => setShowTriggers(false)} />
      )}

      {showTraffic && (
        <TrafficModal onClose={() => setShowTraffic(false)} />
      )}

      {showLogQuery && (
        <LogQueryModal onClose={() => setShowLogQuery(false)} />
      )}

      {showToolkit && (
        <ToolkitModal onClose={() => setShowToolkit(false)} />
      )}
    </>
  );
}
