import { Handle, Position } from '@xyflow/react';
import type { NodeTypes } from '@xyflow/react';
import {
  Play, Cpu, Sparkles, MessageSquare, Database, HardDriveDownload,
  RefreshCw, Clock, BrainCircuit, FolderOpen, Square, Wrench, FileText,
  CheckCircle2, XCircle, AlertCircle, Loader2,
} from 'lucide-react';

//
// Handle styles - large for easy clicking.
//
export const handleStyle = {
  width: 20,
  height: 20,
  background: 'var(--accent-info)',
  border: '3px solid var(--bg-primary)',
  borderRadius: '50%',
};

//
// Selection styles.
//
export const selectedStyle = {
  boxShadow: '0 0 0 1px var(--accent-info)',
};

export const hoverStyle = 'hover:shadow-[0_0_0_1px_var(--accent-info)]';

//
// Status indicator for execution overlays.
//
function getStatusIndicator(status: string) {
  switch (status) {
    case 'Completed':
      return { icon: CheckCircle2, color: 'var(--text-highlight)', animate: false };
    case 'Failed':
      return { icon: XCircle, color: 'var(--accent-error)', animate: false };
    case 'Running':
      return { icon: Loader2, color: 'var(--text-secondary)', animate: true };
    case 'WaitingForInputs':
      return { icon: Clock, color: 'var(--accent-warning)', animate: false };
    case 'Skipped':
      return { icon: AlertCircle, color: 'var(--text-muted)', animate: false };
    default:
      return { icon: Clock, color: 'var(--text-muted)', animate: false };
  }
}

//
// Status overlay rendered in the top-right corner of a node.
//
function StatusOverlay({ status }: { status: string }) {
  const info = getStatusIndicator(status);
  const Icon = info.icon;
  return (
    <div className="absolute top-1 right-1 bg-[var(--bg-primary)] rounded-full p-0.5">
      <Icon
        size={14}
        style={{ color: info.color }}
        className={info.animate ? 'animate-spin' : ''}
      />
    </div>
  );
}

//
// Custom node components with handles for connections.
//
function TriggerNode({ data, selected }: { data: { label: string; status?: string }; selected?: boolean }) {
  return (
    <div
      className={`ascii-box bg-[var(--bg-secondary)] px-3 py-2 relative transition-all ${!selected ? hoverStyle : ''}`}
      style={selected ? selectedStyle : undefined}
    >
      <Handle
        type="source"
        position={Position.Right}
        style={handleStyle}
      />
      {data.status && <StatusOverlay status={data.status} />}
      <Play size={18} className="text-[var(--accent-success)]" />
    </div>
  );
}

export interface OperationNodeData {
  label: string;
  operation: string;
  sessionColor?: string;
  description?: string;
  operationPrompt?: string;
  maxRuntime?: number;
  modelRef?: string;
  category?: string;
  mode?: string;
  timeout?: number;
  agentIterations?: number;
  yoloMode?: boolean;
  workingDir?: string;
  requireAllInputs?: boolean;
  status?: string;
}

function OperationNode({ data, selected }: { data: OperationNodeData; selected?: boolean }) {
  const baseStyle = data.sessionColor
    ? { borderLeft: `4px solid ${data.sessionColor}` }
    : {};
  const style = selected ? { ...baseStyle, ...selectedStyle } : baseStyle;
  return (
    <div
      className={`ascii-box bg-[var(--bg-secondary)] px-4 py-3 min-w-[220px] max-w-[280px] relative transition-all ${!selected ? hoverStyle : ''}`}
      style={style}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Right} style={handleStyle} />
      {data.status && <StatusOverlay status={data.status} />}
      <div className="flex items-center gap-2 mb-2">
        <Cpu size={14} className="text-[var(--accent-info)] shrink-0" />
        <span className="text-sm font-mono text-highlight truncate leading-none">{data.operation || 'Operation'}</span>
      </div>
      {data.description && (
        <div className="mb-1.5">
          <span className="text-[9px] tracking-wider text-[var(--text-secondary)] uppercase">Description</span>
          <div className="text-[11px] text-muted truncate" title={data.description}>{data.description}</div>
        </div>
      )}
      {data.operationPrompt && (
        <div className="mb-1.5">
          <span className="text-[9px] tracking-wider text-[var(--text-secondary)] uppercase">Prompt</span>
          <div className="text-[11px] text-muted line-clamp-2" title={data.operationPrompt}>
            {data.operationPrompt.length > 80 ? data.operationPrompt.substring(0, 80) + '...' : data.operationPrompt}
          </div>
        </div>
      )}
      <div className="flex items-center gap-1.5 flex-wrap">
        {data.mode && (
          <span className="text-[10px] px-1.5 py-0.5 bg-[var(--bg-primary)] text-[var(--text-secondary)] font-mono">{data.mode}</span>
        )}
        {data.modelRef && (
          <span className="inline-flex items-center gap-1 text-[10px] px-1.5 py-0.5 bg-[var(--accent-info)]/10 text-[var(--accent-info)] font-mono">
            <BrainCircuit size={10} />{data.modelRef.split('::').pop()}
          </span>
        )}
        {(data.maxRuntime || data.timeout) && (
          <span className="inline-flex items-center gap-1 text-[10px] px-1.5 py-0.5 bg-[var(--accent-warning)]/10 text-[var(--accent-warning)] font-mono">
            <Clock size={10} />{data.maxRuntime || data.timeout}s
          </span>
        )}
        {data.mode !== 'oneshot' && data.agentIterations && data.agentIterations > 1 && (
          <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-purple)]/10 text-[var(--accent-purple)] font-mono">
            ×{data.agentIterations}
          </span>
        )}
        {data.yoloMode && (
          <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-error)]/10 text-[var(--accent-error)] font-mono">YOLO</span>
        )}
        {data.requireAllInputs === false && (
          <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-info)]/10 text-[var(--accent-info)] font-mono">ANY</span>
        )}
      </div>
      {data.workingDir && (
        <div className="flex items-center gap-1 mt-1.5 text-[10px] text-muted font-mono truncate" title={data.workingDir}>
          <FolderOpen size={10} className="shrink-0" />{data.workingDir}
        </div>
      )}
    </div>
  );
}

interface TransformNodeData {
  label: string;
  prompt: string;
  sessionColor?: string;
  modelRef?: string;
  maxRuntime?: number;
  yoloMode?: boolean;
  workingDir?: string;
  requireAllInputs?: boolean;
  status?: string;
}

function TransformNode({ data, selected }: { data: TransformNodeData; selected?: boolean }) {
  const baseStyle = data.sessionColor
    ? { borderLeft: `4px solid ${data.sessionColor}` }
    : {};
  const style = selected ? { ...baseStyle, ...selectedStyle } : baseStyle;
  return (
    <div
      className={`ascii-box bg-[var(--bg-secondary)] px-4 py-3 min-w-[220px] max-w-[280px] relative transition-all ${!selected ? hoverStyle : ''}`}
      style={style}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Right} style={handleStyle} />
      {data.status && <StatusOverlay status={data.status} />}
      <div className="flex items-center gap-2 mb-2">
        <Sparkles size={14} className="text-[var(--accent-warning)] shrink-0" />
        <span className="text-sm font-mono text-highlight leading-none">Transform</span>
      </div>
      {data.prompt && (
        <div className="mb-1.5">
          <span className="text-[9px] tracking-wider text-[var(--text-secondary)] uppercase">Prompt</span>
          <div className="text-[11px] text-muted truncate" title={data.prompt}>
            {data.prompt.length > 50 ? data.prompt.substring(0, 50) + '...' : data.prompt}
          </div>
        </div>
      )}
      {(data.modelRef || data.maxRuntime || data.yoloMode || data.requireAllInputs === false) && (
        <div className="flex items-center gap-1.5 flex-wrap">
          {data.modelRef && (
            <span className="inline-flex items-center gap-1 text-[10px] px-1.5 py-0.5 bg-[var(--accent-info)]/10 text-[var(--accent-info)] font-mono">
              <BrainCircuit size={10} />{data.modelRef.split('::').pop()}
            </span>
          )}
          {data.maxRuntime && (
            <span className="inline-flex items-center gap-1 text-[10px] px-1.5 py-0.5 bg-[var(--accent-warning)]/10 text-[var(--accent-warning)] font-mono">
              <Clock size={10} />{data.maxRuntime}s
            </span>
          )}
          {data.yoloMode && (
            <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-error)]/10 text-[var(--accent-error)] font-mono">YOLO</span>
          )}
          {data.requireAllInputs === false && (
            <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-info)]/10 text-[var(--accent-info)] font-mono">ANY</span>
          )}
        </div>
      )}
      {data.workingDir && (
        <div className="flex items-center gap-1 mt-1.5 text-[10px] text-muted font-mono truncate" title={data.workingDir}>
          <FolderOpen size={10} className="shrink-0" />{data.workingDir}
        </div>
      )}
    </div>
  );
}

interface GenericPromptNodeData {
  label: string;
  prompt: string;
  sessionColor?: string;
  maxRuntime?: number;
  yoloMode?: boolean;
  workingDir?: string;
  requireAllInputs?: boolean;
  status?: string;
}

function GenericPromptNode({ data, selected }: { data: GenericPromptNodeData; selected?: boolean }) {
  const baseStyle = data.sessionColor
    ? { borderLeft: `4px solid ${data.sessionColor}` }
    : {};
  const style = selected ? { ...baseStyle, ...selectedStyle } : baseStyle;
  return (
    <div
      className={`ascii-box bg-[var(--bg-secondary)] px-4 py-3 min-w-[220px] max-w-[280px] relative transition-all ${!selected ? hoverStyle : ''}`}
      style={style}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Right} style={handleStyle} />
      {data.status && <StatusOverlay status={data.status} />}
      <div className="flex items-center gap-2 mb-2">
        <MessageSquare size={14} className="text-[var(--accent-purple)] shrink-0" />
        <span className="text-sm font-mono text-highlight leading-none">Prompt</span>
      </div>
      {data.prompt && (
        <div className="mb-1.5">
          <span className="text-[9px] tracking-wider text-[var(--text-secondary)] uppercase">Prompt</span>
          <div className="text-[11px] text-muted truncate" title={data.prompt}>
            {data.prompt.length > 50 ? data.prompt.substring(0, 50) + '...' : data.prompt}
          </div>
        </div>
      )}
      {(data.maxRuntime || data.yoloMode || data.requireAllInputs === false) && (
        <div className="flex items-center gap-1.5 flex-wrap">
          {data.maxRuntime && (
            <span className="inline-flex items-center gap-1 text-[10px] px-1.5 py-0.5 bg-[var(--accent-warning)]/10 text-[var(--accent-warning)] font-mono">
              <Clock size={10} />{data.maxRuntime}s
            </span>
          )}
          {data.yoloMode && (
            <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-error)]/10 text-[var(--accent-error)] font-mono">YOLO</span>
          )}
          {data.requireAllInputs === false && (
            <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-info)]/10 text-[var(--accent-info)] font-mono">ANY</span>
          )}
        </div>
      )}
      {data.workingDir && (
        <div className="flex items-center gap-1 mt-1.5 text-[10px] text-muted font-mono truncate" title={data.workingDir}>
          <FolderOpen size={10} className="shrink-0" />{data.workingDir}
        </div>
      )}
    </div>
  );
}

interface MemoryNodeData {
  label: string;
  memoryKey: string;
  memoryMode: 'Store' | 'Retrieve';
  status?: string;
}

function MemoryNode({ data, selected }: { data: MemoryNodeData; selected?: boolean }) {
  const isStore = data.memoryMode === 'Store';
  const accent = isStore ? 'var(--accent-success)' : 'var(--accent-info)';
  const style = selected ? { borderColor: accent } : undefined;
  const Icon = isStore ? Database : HardDriveDownload;
  const badge = isStore ? 'STORE' : 'LOAD';
  return (
    <div
      className="ascii-box bg-[var(--bg-secondary)] px-4 py-2 min-w-[150px] relative"
      style={style}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Right} style={handleStyle} />
      {data.status && <StatusOverlay status={data.status} />}
      <div className="flex items-center gap-2">
        <Icon size={14} style={{ color: accent }} />
        <span className="text-sm font-mono leading-none">{data.memoryKey || (isStore ? 'Store' : 'Retrieve')}</span>
        <span className="text-[10px] px-1.5 py-0.5 font-mono" style={{ backgroundColor: `color-mix(in srgb, ${accent} 20%, transparent)`, color: accent }}>{badge}</span>
      </div>
    </div>
  );
}

interface LoopNodeData {
  label: string;
  maxIterations: number;
  status?: string;
}

function LoopNode({ data, selected }: { data: LoopNodeData; selected?: boolean }) {
  const style = selected ? { borderColor: 'var(--accent-warning)' } : undefined;
  return (
    <div
      className="ascii-box bg-[var(--bg-secondary)] px-4 py-2 min-w-[150px] relative"
      style={style}
    >
      <Handle type="target" position={Position.Top} style={{ ...handleStyle, left: '50%' }} />
      <Handle type="source" position={Position.Bottom} id="0" style={{ ...handleStyle, left: '50%' }} />
      {data.status && <StatusOverlay status={data.status} />}
      <div className="flex items-center gap-2">
        <RefreshCw size={14} className="text-[var(--accent-warning)]" />
        <span className="text-sm font-mono leading-none">Loop</span>
        <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] font-mono">max {data.maxIterations}</span>
      </div>
    </div>
  );
}

interface ToolNodeData {
  label: string;
  toolName: string;
  toolDisplayName?: string;
  maxRuntime?: number;
  status?: string;
}

function ToolNode({ data, selected }: { data: ToolNodeData; selected?: boolean }) {
  const accent = 'var(--accent-info)';
  const style = selected ? { borderColor: accent } : undefined;
  return (
    <div
      className="ascii-box bg-[var(--bg-secondary)] px-4 py-2 min-w-[150px] relative"
      style={style}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Right} style={handleStyle} />
      {data.status && <StatusOverlay status={data.status} />}
      <div className="flex items-center gap-2">
        <Wrench size={14} style={{ color: accent }} />
        <span className="text-sm font-mono leading-none">{data.toolDisplayName || data.toolName || 'Tool'}</span>
        {data.maxRuntime && (
          <span className="text-[10px] px-1.5 py-0.5 font-mono" style={{ backgroundColor: `color-mix(in srgb, ${accent} 20%, transparent)`, color: accent }}>{data.maxRuntime}s</span>
        )}
      </div>
    </div>
  );
}

interface PayloadNodeData {
  label: string;
  shortname: string;
  content?: string;
  status?: string;
}

function PayloadNode({ data, selected }: { data: PayloadNodeData; selected?: boolean }) {
  const accent = 'var(--accent-warning)';
  const style = selected ? { borderColor: accent } : undefined;
  return (
    <div
      className={`ascii-box bg-[var(--bg-secondary)] px-4 py-3 min-w-[180px] max-w-[260px] relative transition-all ${!selected ? hoverStyle : ''}`}
      style={style}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Right} style={handleStyle} />
      {data.status && <StatusOverlay status={data.status} />}
      <div className="flex items-center gap-2 mb-1">
        <FileText size={14} style={{ color: accent }} className="shrink-0" />
        <span className="text-sm font-mono text-highlight leading-none">{data.shortname || 'Payload'}</span>
      </div>
      {data.content && (
        <div>
          <span className="text-[9px] tracking-wider text-[var(--text-secondary)] uppercase">Content</span>
          <div className="text-[11px] text-muted truncate" title={data.content}>
            {data.content.length > 50 ? data.content.substring(0, 50) + '...' : data.content}
          </div>
        </div>
      )}
    </div>
  );
}

interface TerminationNodeData {
  label: string;
  requireAllInputs?: boolean;
  status?: string;
}

function TerminationNode({ data, selected }: { data: TerminationNodeData; selected?: boolean }) {
  return (
    <div
      className={`ascii-box bg-[var(--bg-secondary)] px-3 py-2 relative transition-all ${!selected ? hoverStyle : ''}`}
      style={selected ? selectedStyle : undefined}
    >
      <Handle
        type="target"
        position={Position.Left}
        style={handleStyle}
      />
      {data.status && <StatusOverlay status={data.status} />}
      <Square size={18} className="text-[var(--accent-error)]" />
    </div>
  );
}

export const nodeTypes: NodeTypes = {
  trigger: TriggerNode,
  operation: OperationNode,
  transform: TransformNode,
  genericPrompt: GenericPromptNode,
  memory: MemoryNode,
  loop: LoopNode,
  tool: ToolNode,
  payload: PayloadNode,
  termination: TerminationNode,
};
