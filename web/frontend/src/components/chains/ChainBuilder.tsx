import { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import type { DragEvent } from 'react';
import {
  ReactFlow,
  Background,
  BackgroundVariant,
  useNodesState,
  useEdgesState,
  addEdge,
  MarkerType,
  Panel,
  useReactFlow,
  ReactFlowProvider,
  Handle,
  Position,
  SelectionMode,
} from '@xyflow/react';
import type { Node, Edge, Connection, NodeTypes, OnSelectionChangeParams } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { Play, X, Save, Cpu, Maximize2, GitMerge, Sparkles, MessageSquare, Users, Database, HardDriveDownload, RefreshCw, Clock, BrainCircuit, FolderOpen } from 'lucide-react';
import { ConfigModal } from '../common/ConfigModal';
import type {
  BlockConfig,
  ChainDefinitionFull,
  ChainDefinitionInput,
  ChainElement,
  ChainConnection as ChainConnectionType,
  OperationDefinitionInfo,
  SessionGroup,
} from '../../api/types';
import { computeLayout } from '../../utils/dagreLayout';
import { getNextSessionColor, getUsedColors } from '../../utils/sessionColors';

//
// Model definition type (matches SettingsPage).
//
interface ModelDefinition {
  //
  // provider::model format.
  //
  name: string;
  provider: string;
  model: string;
  apiKey: string;
}
import { generateUUID } from '../../utils/uuid';

//
// Handle styles - large for easy clicking.
//
const handleStyle = {
  width: 20,
  height: 20,
  background: 'var(--accent-info)',
  border: '3px solid var(--bg-primary)',
  borderRadius: '50%',
};

//
// Selection styles.
//
const selectedStyle = {
  boxShadow: '0 0 0 1px var(--accent-info)',
};

const hoverStyle = 'hover:shadow-[0_0_0_1px_var(--accent-info)]';

//
// Custom node components with handles for connections.
//
function TriggerNode({ selected }: { data: { label: string }; selected?: boolean }) {
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
      <Play size={18} className="text-[var(--accent-success)]" />
    </div>
  );
}

interface OperationNodeData {
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
      </div>
      {data.workingDir && (
        <div className="flex items-center gap-1 mt-1.5 text-[10px] text-muted font-mono truncate" title={data.workingDir}>
          <FolderOpen size={10} className="shrink-0" />{data.workingDir}
        </div>
      )}
    </div>
  );
}

function TransformNode({ data, selected }: { data: { label: string; prompt: string; sessionColor?: string; modelRef?: string; maxRuntime?: number; yoloMode?: boolean; workingDir?: string }; selected?: boolean }) {
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
      {(data.modelRef || data.maxRuntime || data.yoloMode) && (
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

function GenericPromptNode({ data, selected }: { data: { label: string; prompt: string; sessionColor?: string; maxRuntime?: number; yoloMode?: boolean; workingDir?: string }; selected?: boolean }) {
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
      {(data.maxRuntime || data.yoloMode) && (
        <div className="flex items-center gap-1.5 flex-wrap">
          {data.maxRuntime && (
            <span className="inline-flex items-center gap-1 text-[10px] px-1.5 py-0.5 bg-[var(--accent-warning)]/10 text-[var(--accent-warning)] font-mono">
              <Clock size={10} />{data.maxRuntime}s
            </span>
          )}
          {data.yoloMode && (
            <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-error)]/10 text-[var(--accent-error)] font-mono">YOLO</span>
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

function MemoryStoreNode({ data, selected }: { data: { label: string; memoryKey: string }; selected?: boolean }) {
  const style = selected ? { borderColor: 'var(--accent-success)' } : undefined;
  return (
    <div
      className="ascii-box bg-[var(--bg-secondary)] px-4 py-2 min-w-[150px] relative"
      style={style}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Right} style={handleStyle} />
      <div className="flex items-center gap-2">
        <Database size={14} className="text-[var(--accent-success)]" />
        <span className="text-sm font-mono leading-none">{data.memoryKey || 'Store'}</span>
        <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-success)]/20 text-[var(--accent-success)] font-mono">STORE</span>
      </div>
    </div>
  );
}

function MemoryRetrieveNode({ data, selected }: { data: { label: string; memoryKey: string }; selected?: boolean }) {
  const style = selected ? { borderColor: 'var(--accent-info)' } : undefined;
  return (
    <div
      className="ascii-box bg-[var(--bg-secondary)] px-4 py-2 min-w-[150px] relative"
      style={style}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Right} style={handleStyle} />
      <div className="flex items-center gap-2">
        <HardDriveDownload size={14} className="text-[var(--accent-info)]" />
        <span className="text-sm font-mono">{data.memoryKey || 'Retrieve'}</span>
        <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-info)]/20 text-[var(--accent-info)] font-mono">RETRIEVE</span>
      </div>
    </div>
  );
}

function LoopNode({ data, selected }: { data: { label: string; maxIterations: number }; selected?: boolean }) {
  const style = selected ? { borderColor: 'var(--accent-warning)' } : undefined;
  return (
    <div
      className="ascii-box bg-[var(--bg-secondary)] px-4 py-3 min-w-[180px] relative"
      style={{ ...style, minHeight: 70 }}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Bottom} id="0" style={{ ...handleStyle, left: '50%' }} />
      <Handle type="source" position={Position.Right} id="1" style={handleStyle} />
      <div className="flex items-center gap-2 pr-16">
        <RefreshCw size={14} className="text-[var(--accent-warning)]" />
        <span className="text-sm font-mono leading-none">Loop</span>
        <span className="text-[10px] px-1.5 py-0.5 bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] font-mono">max {data.maxIterations}</span>
      </div>
      <span className="absolute text-[9px] text-[var(--accent-warning)]" style={{ bottom: -16, left: '50%', transform: 'translateX(-50%)' }}>↻ retry</span>
      <span className="absolute text-[9px] text-muted" style={{ right: 28, top: '50%', transform: 'translateY(-50%)' }}>→ done</span>
    </div>
  );
}

const nodeTypes: NodeTypes = {
  trigger: TriggerNode,
  operation: OperationNode,
  transform: TransformNode,
  genericPrompt: GenericPromptNode,
  memoryStore: MemoryStoreNode,
  memoryRetrieve: MemoryRetrieveNode,
  loop: LoopNode,
};

//
// Extra data tracked separately (prompts, models, session groups).
//
interface ChainExtraData {
  transformPrompts: Map<string, string>;
  transformModels: Map<string, string>;
  genericPrompts: Map<string, string>;
  sessionGroups: Map<string, SessionGroup>;
  blockConfigs: Map<string, BlockConfig>;
  memoryKeys: Map<string, string>;
  loopMaxIterations: Map<string, number>;
}

//
// Convert chain definition to React Flow nodes and edges (positions computed
// via dagre).
//
function chainToFlow(chain: ChainDefinitionFull | null, operationDefs?: OperationDefinitionInfo[]): { nodes: Node[]; edges: Edge[]; extraData: ChainExtraData } {
  const emptyExtraData: ChainExtraData = {
    transformPrompts: new Map(),
    transformModels: new Map(),
    genericPrompts: new Map(),
    sessionGroups: new Map(),
    blockConfigs: new Map(),
    memoryKeys: new Map(),
    loopMaxIterations: new Map(),
  };

  if (!chain) return { nodes: [], edges: [], extraData: emptyExtraData };

  //
  // Use stored positions if available, otherwise compute via dagre.
  //
  const hasStoredPositions = chain.positions && Object.keys(chain.positions).length > 0;
  const dagrePositions = hasStoredPositions ? null : computeLayout(chain.elements, chain.connections);

  const extraData = { ...emptyExtraData };

  const nodes: Node[] = chain.elements.map((elem) => {
    const position = hasStoredPositions
      ? (chain.positions![elem.id] || { x: 0, y: 0 })
      : (dagrePositions!.get(elem.id) || { x: 0, y: 0 });

    switch (elem.element_type) {
      case 'Trigger':
        return {
          id: elem.id,
          type: 'trigger',
          position,
          data: { label: 'Manual Trigger' },
        };
      case 'Operation': {
        if (elem.session_group) {
          extraData.sessionGroups.set(elem.id, elem.session_group);
        }
        if (elem.block_config) {
          extraData.blockConfigs.set(elem.id, elem.block_config);
        }
        const opDef = operationDefs?.find(d => d.full_name === elem.operation_name);
        return {
          id: elem.id,
          type: 'operation',
          position,
          data: {
            label: 'Operation',
            operation: elem.operation_name,
            sessionColor: elem.session_group?.color,
            description: opDef?.description,
            operationPrompt: opDef?.operation_prompt,
            maxRuntime: elem.block_config?.max_runtime,
            modelRef: elem.model_ref || opDef?.model_ref,
            category: opDef?.category,
            mode: opDef?.mode,
            timeout: opDef?.timeout,
            agentIterations: opDef?.agent_iterations,
            yoloMode: elem.block_config?.yolo_mode || opDef?.yolo_mode,
            workingDir: elem.block_config?.working_dir,
          },
        };
      }
      case 'Transform':
        extraData.transformPrompts.set(elem.id, elem.prompt);
        if (elem.model_ref) {
          extraData.transformModels.set(elem.id, elem.model_ref);
        }
        if (elem.session_group) {
          extraData.sessionGroups.set(elem.id, elem.session_group);
        }
        if (elem.block_config) {
          extraData.blockConfigs.set(elem.id, elem.block_config);
        }
        return {
          id: elem.id,
          type: 'transform',
          position,
          data: {
            label: 'Transform',
            prompt: elem.prompt,
            sessionColor: elem.session_group?.color,
            modelRef: elem.model_ref,
            maxRuntime: elem.block_config?.max_runtime,
            yoloMode: elem.block_config?.yolo_mode,
            workingDir: elem.block_config?.working_dir,
          },
        };
      case 'GenericPrompt':
        extraData.genericPrompts.set(elem.id, elem.prompt);
        if (elem.session_group) {
          extraData.sessionGroups.set(elem.id, elem.session_group);
        }
        if (elem.block_config) {
          extraData.blockConfigs.set(elem.id, elem.block_config);
        }
        return {
          id: elem.id,
          type: 'genericPrompt',
          position,
          data: {
            label: 'Prompt',
            prompt: elem.prompt,
            sessionColor: elem.session_group?.color,
            maxRuntime: elem.block_config?.max_runtime,
            yoloMode: elem.block_config?.yolo_mode,
            workingDir: elem.block_config?.working_dir,
          },
        };
      case 'MemoryStore':
        extraData.memoryKeys.set(elem.id, elem.key);
        return {
          id: elem.id,
          type: 'memoryStore',
          position,
          data: { label: 'Memory Store', memoryKey: elem.key },
        };
      case 'MemoryRetrieve':
        extraData.memoryKeys.set(elem.id, elem.key);
        return {
          id: elem.id,
          type: 'memoryRetrieve',
          position,
          data: { label: 'Memory Retrieve', memoryKey: elem.key },
        };
      case 'Loop':
        extraData.loopMaxIterations.set(elem.id, elem.max_iterations);
        return {
          id: elem.id,
          type: 'loop',
          position,
          data: { label: 'Loop', maxIterations: elem.max_iterations },
        };
    }
  }).filter((n): n is NonNullable<typeof n> => n != null);

  const edges: Edge[] = chain.connections.map((conn) => {
    let stroke = 'var(--text-secondary)';
    let label: string | undefined;
    let strokeDasharray: string | undefined;

    if (conn.condition === 'OnSuccess') {
      stroke = 'var(--accent-success)';
      label = 'Success';
    } else if (conn.condition === 'OnFailure') {
      stroke = 'var(--accent-error)';
      label = 'Failure';
    }

    return {
      id: conn.id,
      source: conn.from_element,
      target: conn.to_element,
      sourceHandle: conn.from_port > 0 ? String(conn.from_port) : undefined,
      type: 'smoothstep',
      markerEnd: { type: MarkerType.ArrowClosed },
      style: { stroke, strokeDasharray, strokeWidth: 2 },
      label,
      labelStyle: { fill: stroke, fontSize: 10, fontWeight: 500 },
      data: { condition: conn.condition || null },
    };
  });

  return { nodes, edges, extraData };
}

//
// Convert React Flow nodes and edges back to chain definition.
//
function flowToChain(
  nodes: Node[],
  edges: Edge[],
  name: string,
  description: string,
  category: string,
  timeout: number,
  extraData: ChainExtraData
): ChainDefinitionInput {
  //
  // Store visual positions for each element.
  //
  const positions: Record<string, { x: number; y: number }> = {};
  for (const node of nodes) {
    positions[node.id] = { x: node.position.x, y: node.position.y };
  }

  const elements: ChainElement[] = nodes.map((node) => {
    switch (node.type) {
      case 'trigger':
        return {
          element_type: 'Trigger' as const,
          id: node.id,
          trigger_type: { type: 'Manual' as const },
        };
      case 'operation':
        return {
          element_type: 'Operation' as const,
          id: node.id,
          operation_name: (node.data?.operation as string) || '',
          model_ref: null,
          session_group: extraData.sessionGroups.get(node.id) || null,
          block_config: extraData.blockConfigs.get(node.id) || null,
        };
      case 'transform':
        return {
          element_type: 'Transform' as const,
          id: node.id,
          prompt: extraData.transformPrompts.get(node.id) || '',
          model_ref: extraData.transformModels.get(node.id) || null,
          session_group: extraData.sessionGroups.get(node.id) || null,
          block_config: extraData.blockConfigs.get(node.id) || null,
        };
      case 'genericPrompt':
        return {
          element_type: 'GenericPrompt' as const,
          id: node.id,
          prompt: extraData.genericPrompts.get(node.id) || '',
          session_group: extraData.sessionGroups.get(node.id) || null,
          block_config: extraData.blockConfigs.get(node.id) || null,
        };
      case 'memoryStore':
        return {
          element_type: 'MemoryStore' as const,
          id: node.id,
          key: extraData.memoryKeys.get(node.id) || '',
        };
      case 'memoryRetrieve':
        return {
          element_type: 'MemoryRetrieve' as const,
          id: node.id,
          key: extraData.memoryKeys.get(node.id) || '',
        };
      case 'loop':
        return {
          element_type: 'Loop' as const,
          id: node.id,
          max_iterations: extraData.loopMaxIterations.get(node.id) || 3,
        };
      default:
        throw new Error(`Unknown node type: ${node.type}`);
    }
  });

  const connections: ChainConnectionType[] = edges.map((edge) => ({
    id: edge.id,
    from_element: edge.source,
    to_element: edge.target,
    from_port: edge.sourceHandle ? parseInt(edge.sourceHandle, 10) || 0 : 0,
    to_port: 0,
    condition: (edge.data as Record<string, unknown>)?.condition as ChainConnectionType['condition'] || null,
  }));

  return {
    name,
    description,
    category,
    elements,
    connections,
    disabled: false,
    timeout,
    positions,
  };
}

//
// Element palette item component.
//
interface PaletteItemProps {
  type: string;
  icon: React.ReactNode;
  label: string;
  disabled?: boolean;
  onClick?: () => void;
}

function PaletteItem({ type, icon, label, disabled, onClick }: PaletteItemProps) {
  const onDragStart = (event: DragEvent, nodeType: string) => {
    if (disabled) {
      event.preventDefault();
      return;
    }
    event.dataTransfer.setData('application/reactflow', nodeType);
    event.dataTransfer.effectAllowed = 'move';
  };

  return (
    <div
      className={`flex flex-col items-center gap-2 py-3 px-2 transition-all group ${
        disabled
          ? 'opacity-30 cursor-not-allowed'
          : 'cursor-grab hover:bg-[var(--bg-primary)]/50 active:scale-95'
      }`}
      draggable={!disabled}
      onDragStart={(e) => onDragStart(e, type)}
      onClick={disabled ? undefined : onClick}
      title={disabled ? `${label} (already added)` : label}
    >
      <div className={`transition-transform ${disabled ? '' : 'group-hover:scale-110'}`}>
        {icon}
      </div>
      <span className="text-[10px] tracking-widest text-[var(--text-secondary)] group-hover:text-highlight transition-colors" style={{ letterSpacing: '0.08em' }}>{label}</span>
    </div>
  );
}

interface ChainBuilderInnerProps {
  chain?: ChainDefinitionFull | null;
  onSave: (definition: ChainDefinitionInput) => void;
  onCancel: () => void;
  operationDefs: OperationDefinitionInfo[];
  modelDefs: ModelDefinition[];
}

function ChainBuilderInner({ chain, onSave, onCancel, operationDefs, modelDefs }: ChainBuilderInnerProps) {
  const [name, setName] = useState(chain?.name || '');
  const [description, setDescription] = useState(chain?.description || '');
  const [timeout, setTimeout] = useState(chain?.timeout || 1800);
  const category = 'default';

  const initialFlow = chainToFlow(chain || null, operationDefs);
  const [nodes, setNodes, onNodesChange] = useNodesState(initialFlow.nodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(initialFlow.edges);

  //
  // Track extra data (prompts, models, session groups) separately.
  //
  const [extraData, setExtraData] = useState<ChainExtraData>(() => initialFlow.extraData);

  //
  // Track hovered node for delete-on-hover.
  //
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);

  //
  // Track hovered edge for delete-on-hover.
  //
  const [hoveredEdgeId, setHoveredEdgeId] = useState<string | null>(null);

  //
  // Selection state for multi-select grouping.
  //
  const [selectedNodeIds, setSelectedNodeIds] = useState<Set<string>>(new Set());

  //
  // Modal state for operation selection.
  //
  const [showOperationModal, setShowOperationModal] = useState(false);
  const [pendingPosition, setPendingPosition] = useState<{ x: number; y: number } | null>(null);
  const [selectedOperation, setSelectedOperation] = useState<string>('');

  //
  // Modal state for transform configuration.
  //
  const [showTransformModal, setShowTransformModal] = useState(false);
  const [transformPrompt, setTransformPrompt] = useState('');
  const [transformModel, setTransformModel] = useState<string>('');

  //
  // Modal state for generic prompt configuration.
  //
  const [showGenericPromptModal, setShowGenericPromptModal] = useState(false);
  const [genericPromptText, setGenericPromptText] = useState('');

  //
  // Track which node is being edited (null means adding new).
  //
  const [editingNodeId, setEditingNodeId] = useState<string | null>(null);

  //
  // Modal state for memory key configuration.
  //
  const [showMemoryModal, setShowMemoryModal] = useState(false);
  const [memoryKey, setMemoryKey] = useState('');
  const [pendingMemoryType, setPendingMemoryType] = useState<'memoryStore' | 'memoryRetrieve'>('memoryStore');

  const [showLoopModal, setShowLoopModal] = useState(false);
  const [loopMaxIterations, setLoopMaxIterations] = useState<number>(3);

  //
  // Per-block config state (shared across Operation, Transform, GenericPrompt
  // modals).
  //
  const [blockMaxRuntime, setBlockMaxRuntime] = useState<string>('');
  const [blockYoloMode, setBlockYoloMode] = useState<boolean>(false);
  const [blockWorkingDir, setBlockWorkingDir] = useState<string>('');

  const advancedSectionConfig = {
    type: 'section' as const,
    title: 'Advanced',
    collapsible: true,
    fields: [
      {
        name: 'maxRuntime',
        label: 'Max Runtime (seconds)',
        type: 'text' as const,
        placeholder: 'Default',
        span: 'full' as const,
      },
      {
        name: 'workingDir',
        label: 'Working Directory',
        type: 'text' as const,
        placeholder: 'Default',
        span: 'full' as const,
      },
      {
        name: 'yoloMode',
        label: 'YOLO Mode',
        type: 'toggle' as const,
        span: 'full' as const,
      },
    ],
  };

  const blockConfigValues = {
    maxRuntime: blockMaxRuntime,
    workingDir: blockWorkingDir,
    yoloMode: blockYoloMode,
  };

  const handleBlockConfigChange = (name: string, value: any) => {
    if (name === 'maxRuntime') setBlockMaxRuntime(value);
    if (name === 'workingDir') setBlockWorkingDir(value);
    if (name === 'yoloMode') setBlockYoloMode(!!value);
  };

  const resetBlockConfig = () => {
    setBlockMaxRuntime('');
    setBlockYoloMode(false);
    setBlockWorkingDir('');
  };

  const loadBlockConfig = (nodeId: string) => {
    const existing = extraData.blockConfigs.get(nodeId);
    setBlockMaxRuntime(existing?.max_runtime ? String(existing.max_runtime) : '');
    setBlockYoloMode(existing?.yolo_mode || false);
    setBlockWorkingDir(existing?.working_dir || '');
  };

  //
  // Build a BlockConfig from current state and save it to extraData for the
  // given node ID. Clears the entry if no fields are set.
  //
  const saveBlockConfig = (nodeId: string) => {
    const blockConfig: BlockConfig = {};
    if (blockMaxRuntime) blockConfig.max_runtime = parseInt(blockMaxRuntime) || null;
    if (blockYoloMode) blockConfig.yolo_mode = true;
    if (blockWorkingDir) blockConfig.working_dir = blockWorkingDir;

    setExtraData(prev => {
      const newConfigs = new Map(prev.blockConfigs);
      if (blockConfig.max_runtime || blockConfig.yolo_mode || blockConfig.working_dir) {
        newConfigs.set(nodeId, blockConfig);
      } else {
        newConfigs.delete(nodeId);
      }
      return { ...prev, blockConfigs: newConfigs };
    });
  };

  const reactFlowWrapper = useRef<HTMLDivElement>(null);
  const { screenToFlowPosition, fitView } = useReactFlow();

  //
  // Check if trigger already exists.
  //
  const hasTrigger = nodes.some(n => n.type === 'trigger');

  //
  // Check which selected nodes can be grouped (Operations, Transforms,
  // GenericPrompts only).
  //
  const groupableSelectedNodes = useMemo(() => {
    return nodes.filter(n =>
      selectedNodeIds.has(n.id) &&
      (n.type === 'operation' || n.type === 'transform' || n.type === 'genericPrompt')
    );
  }, [nodes, selectedNodeIds]);

  const canGroupSelection = groupableSelectedNodes.length >= 2;

  //
  // Handle selection change.
  //
  const onSelectionChange = useCallback((params: OnSelectionChangeParams) => {
    setSelectedNodeIds(new Set(params.nodes.map(n => n.id)));
  }, []);

  //
  // Group selected nodes into a session.
  //
  const handleGroupIntoSession = useCallback(() => {
    if (!canGroupSelection) return;

    const usedColors = getUsedColors(
      Array.from(extraData.sessionGroups.values()).map(sg => ({ session_group: sg }))
    );
    const newColor = getNextSessionColor(usedColors);
    const newGroupId = generateUUID();

    const newSessionGroup: SessionGroup = {
      id: newGroupId,
      color: newColor,
      yolo_mode: false,
    };

    //
    // Update extra data with new session group for all selected nodes.
    //
    setExtraData(prev => {
      const newSessionGroups = new Map(prev.sessionGroups);
      for (const node of groupableSelectedNodes) {
        newSessionGroups.set(node.id, newSessionGroup);
      }
      return { ...prev, sessionGroups: newSessionGroups };
    });

    //
    // Update node data to show session color.
    //
    setNodes(nds =>
      nds.map(n => {
        if (groupableSelectedNodes.some(gn => gn.id === n.id)) {
          return {
            ...n,
            data: { ...n.data, sessionColor: newColor },
          };
        }
        return n;
      })
    );

    //
    // Clear selection.
    //
    setSelectedNodeIds(new Set());
  }, [canGroupSelection, groupableSelectedNodes, extraData.sessionGroups, setNodes]);

  //
  // Remove session group from selected nodes.
  //
  const handleUngroupSelection = useCallback(() => {
    if (groupableSelectedNodes.length === 0) return;

    //
    // Get the session group IDs of nodes being removed.
    //
    const affectedGroupIds = new Set<string>();
    for (const node of groupableSelectedNodes) {
      const group = extraData.sessionGroups.get(node.id);
      if (group) {
        affectedGroupIds.add(group.id);
      }
    }

    //
    // Build new session groups map, removing selected nodes.
    //
    const newSessionGroups = new Map(extraData.sessionGroups);
    const selectedIds = new Set(groupableSelectedNodes.map(n => n.id));
    for (const nodeId of selectedIds) {
      newSessionGroups.delete(nodeId);
    }

    //
    // Check each affected group - if only 1 node remains, remove it too.
    //
    const nodesToRemoveColor = new Set(selectedIds);
    for (const groupId of affectedGroupIds) {
      const remainingNodesInGroup: string[] = [];
      for (const [nodeId, group] of newSessionGroups) {
        if (group.id === groupId) {
          remainingNodesInGroup.push(nodeId);
        }
      }
      //
      // If only 1 node left in this group, remove it from the group.
      //
      if (remainingNodesInGroup.length === 1) {
        newSessionGroups.delete(remainingNodesInGroup[0]);
        nodesToRemoveColor.add(remainingNodesInGroup[0]);
      }
    }

    setExtraData(prev => ({ ...prev, sessionGroups: newSessionGroups }));

    //
    // Update node data to remove session color.
    //
    setNodes(nds =>
      nds.map(n => {
        if (nodesToRemoveColor.has(n.id)) {
          const { sessionColor, ...restData } = n.data as Record<string, unknown>;
          return { ...n, data: restData };
        }
        return n;
      })
    );

    setSelectedNodeIds(new Set());
  }, [groupableSelectedNodes, extraData.sessionGroups, setNodes]);

  const onConnect = useCallback(
    (params: Connection) => setEdges((eds) => addEdge({
      ...params,
      id: generateUUID(),
      type: 'smoothstep',
      markerEnd: { type: MarkerType.ArrowClosed },
      style: { stroke: 'var(--text-secondary)', strokeWidth: 2 },
    }, eds)),
    [setEdges]
  );

  const onDragOver = useCallback((event: DragEvent) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
  }, []);

  const onDrop = useCallback(
    (event: DragEvent) => {
      event.preventDefault();

      const type = event.dataTransfer.getData('application/reactflow');
      if (!type || !reactFlowWrapper.current) return;

      //
      // Prevent adding second trigger.
      //
      if (type === 'trigger' && hasTrigger) {
        return;
      }

      const position = screenToFlowPosition({
        x: event.clientX,
        y: event.clientY,
      });

      //
      // For operations, show the selection modal.
      //
      if (type === 'operation') {
        setPendingPosition(position);
        resetBlockConfig();
        setShowOperationModal(true);
        return;
      }

      //
      // For transform, show the configuration modal.
      //
      if (type === 'transform') {
        setPendingPosition(position);
        setTransformPrompt('');
        setTransformModel('');
        resetBlockConfig();
        setShowTransformModal(true);
        return;
      }

      //
      // For generic prompt, show the configuration modal.
      //
      if (type === 'genericPrompt') {
        setPendingPosition(position);
        setGenericPromptText('');
        resetBlockConfig();
        setShowGenericPromptModal(true);
        return;
      }

      //
      // For memory nodes, show the memory key modal.
      //
      if (type === 'memoryStore' || type === 'memoryRetrieve') {
        setPendingPosition(position);
        setPendingMemoryType(type as 'memoryStore' | 'memoryRetrieve');
        setMemoryKey('');
        setShowMemoryModal(true);
        return;
      }

      //
      // For loop nodes, show the loop configuration modal.
      //
      if (type === 'loop') {
        setPendingPosition(position);
        setLoopMaxIterations(3);
        setShowLoopModal(true);
        return;
      }

      //
      // For other types, create directly.
      //
      addNodeAtPosition(type, position);
    },
    [screenToFlowPosition, hasTrigger]
  );

  const addNodeAtPosition = useCallback((type: string, position: { x: number; y: number }, nodeExtraData?: Record<string, unknown>) => {
    //
    // Prevent adding second trigger.
    //
    if (type === 'trigger' && hasTrigger) {
      return;
    }

    const newId = generateUUID();
    let newNode: Node;

    switch (type) {
      case 'trigger':
        newNode = {
          id: newId,
          type: 'trigger',
          position,
          data: { label: 'Manual Trigger' },
        };
        break;
      case 'operation': {
        const opDef = operationDefs.find(d => d.full_name === (nodeExtraData?.operation as string));
        newNode = {
          id: newId,
          type: 'operation',
          position,
          data: {
            label: 'Operation',
            operation: nodeExtraData?.operation || '',
            description: opDef?.description,
            modelRef: opDef?.model_ref,
            maxRuntime: nodeExtraData?.maxRuntime,
          },
        };
        break;
      }
      case 'transform':
        newNode = {
          id: newId,
          type: 'transform',
          position,
          data: {
            label: 'Transform',
            prompt: nodeExtraData?.prompt || '',
            modelRef: nodeExtraData?.modelRef,
            maxRuntime: nodeExtraData?.maxRuntime,
          },
        };
        //
        // Store prompt and model in extraData.
        //
        if (nodeExtraData?.prompt) {
          setExtraData(prev => {
            const newTransformPrompts = new Map(prev.transformPrompts);
            newTransformPrompts.set(newId, nodeExtraData.prompt as string);
            const newTransformModels = new Map(prev.transformModels);
            if (nodeExtraData?.modelRef) {
              newTransformModels.set(newId, nodeExtraData.modelRef as string);
            }
            return { ...prev, transformPrompts: newTransformPrompts, transformModels: newTransformModels };
          });
        }
        break;
      case 'genericPrompt':
        newNode = {
          id: newId,
          type: 'genericPrompt',
          position,
          data: {
            label: 'Prompt',
            prompt: nodeExtraData?.prompt || '',
            maxRuntime: nodeExtraData?.maxRuntime,
          },
        };
        //
        // Store prompt in extraData.
        //
        if (nodeExtraData?.prompt) {
          setExtraData(prev => {
            const newGenericPrompts = new Map(prev.genericPrompts);
            newGenericPrompts.set(newId, nodeExtraData.prompt as string);
            return { ...prev, genericPrompts: newGenericPrompts };
          });
        }
        break;
      case 'memoryStore':
        newNode = {
          id: newId,
          type: 'memoryStore',
          position,
          data: { label: 'Memory Store', memoryKey: nodeExtraData?.memoryKey || '' },
        };
        if (nodeExtraData?.memoryKey) {
          setExtraData(prev => {
            const newKeys = new Map(prev.memoryKeys);
            newKeys.set(newId, nodeExtraData.memoryKey as string);
            return { ...prev, memoryKeys: newKeys };
          });
        }
        break;
      case 'memoryRetrieve':
        newNode = {
          id: newId,
          type: 'memoryRetrieve',
          position,
          data: { label: 'Memory Retrieve', memoryKey: nodeExtraData?.memoryKey || '' },
        };
        if (nodeExtraData?.memoryKey) {
          setExtraData(prev => {
            const newKeys = new Map(prev.memoryKeys);
            newKeys.set(newId, nodeExtraData.memoryKey as string);
            return { ...prev, memoryKeys: newKeys };
          });
        }
        break;
      case 'loop':
        newNode = {
          id: newId,
          type: 'loop',
          position,
          data: { label: 'Loop', maxIterations: nodeExtraData?.maxIterations || 3 },
        };
        setExtraData(prev => {
          const newMap = new Map(prev.loopMaxIterations);
          newMap.set(newId, (nodeExtraData?.maxIterations as number) || 3);
          return { ...prev, loopMaxIterations: newMap };
        });
        break;
      default:
        return;
    }

    setNodes((nds) => [...nds, newNode]);
  }, [setNodes, hasTrigger, setExtraData]);

  //
  // Quick add from palette click (adds at a default position).
  //
  const handleQuickAdd = useCallback((type: string) => {
    //
    // Prevent adding second trigger.
    //
    if (type === 'trigger' && hasTrigger) {
      return;
    }

    const position = { x: 100 + nodes.length * 30, y: 100 + nodes.length * 30 };

    if (type === 'operation') {
      setPendingPosition(position);
      resetBlockConfig();
      setShowOperationModal(true);
      return;
    }

    if (type === 'transform') {
      setPendingPosition(position);
      setTransformPrompt('');
      setTransformModel('');
      resetBlockConfig();
      setShowTransformModal(true);
      return;
    }

    if (type === 'genericPrompt') {
      setPendingPosition(position);
      setGenericPromptText('');
      resetBlockConfig();
      setShowGenericPromptModal(true);
      return;
    }

    if (type === 'memoryStore' || type === 'memoryRetrieve') {
      setPendingMemoryType(type as 'memoryStore' | 'memoryRetrieve');
      setMemoryKey('');
      setShowMemoryModal(true);
      return;
    }

    if (type === 'loop') {
      setLoopMaxIterations(3);
      setShowLoopModal(true);
      return;
    }

    addNodeAtPosition(type, position);
  }, [nodes.length, addNodeAtPosition, hasTrigger]);

  const handleOperationSelect = useCallback(() => {
    if (!selectedOperation) return;

    const opDef = operationDefs.find(d => d.full_name === selectedOperation);
    const maxRuntime = blockMaxRuntime ? parseInt(blockMaxRuntime, 10) : undefined;
    const opNodeData = {
      label: 'Operation',
      operation: selectedOperation,
      description: opDef?.description,
      operationPrompt: opDef?.operation_prompt,
      modelRef: opDef?.model_ref,
      maxRuntime,
      category: opDef?.category,
      mode: opDef?.mode,
      timeout: opDef?.timeout,
      agentIterations: opDef?.agent_iterations,
      yoloMode: blockYoloMode || opDef?.yolo_mode,
      workingDir: blockWorkingDir || undefined,
    };

    if (editingNodeId) {
      //
      // Update existing operation node.
      //
      setNodes(nds => nds.map(n =>
        n.id === editingNodeId
          ? { ...n, data: { ...n.data, ...opNodeData } }
          : n
      ));
      saveBlockConfig(editingNodeId);
    } else if (pendingPosition) {
      const newNodeId = generateUUID();
      const newNode: Node = {
        id: newNodeId,
        type: 'operation',
        position: pendingPosition,
        data: opNodeData,
      };
      setNodes(nds => [...nds, newNode]);
      saveBlockConfig(newNodeId);
    }

    setShowOperationModal(false);
    setPendingPosition(null);
    setEditingNodeId(null);
    setSelectedOperation('');
    resetBlockConfig();
  }, [pendingPosition, editingNodeId, selectedOperation, setNodes, blockMaxRuntime, blockYoloMode, blockWorkingDir, operationDefs]);

  const handleTransformConfirm = useCallback(() => {
    if (!transformPrompt.trim()) return;

    const maxRuntime = blockMaxRuntime ? parseInt(blockMaxRuntime, 10) : undefined;

    if (editingNodeId) {
      //
      // Update existing node.
      //
      setExtraData(prev => {
        const newTransformPrompts = new Map(prev.transformPrompts);
        const newTransformModels = new Map(prev.transformModels);
        newTransformPrompts.set(editingNodeId, transformPrompt);
        if (transformModel) {
          newTransformModels.set(editingNodeId, transformModel);
        } else {
          newTransformModels.delete(editingNodeId);
        }
        return { ...prev, transformPrompts: newTransformPrompts, transformModels: newTransformModels };
      });
      setNodes(nds => nds.map(n =>
        n.id === editingNodeId
          ? { ...n, data: { ...n.data, prompt: transformPrompt, modelRef: transformModel || undefined, maxRuntime, yoloMode: blockYoloMode || undefined, workingDir: blockWorkingDir || undefined } }
          : n
      ));
      saveBlockConfig(editingNodeId);
    } else if (pendingPosition) {
      //
      // Add new node.
      //
      const newNodeId = generateUUID();
      const newNode: Node = {
        id: newNodeId,
        type: 'transform',
        position: pendingPosition,
        data: { label: 'Transform', prompt: transformPrompt, modelRef: transformModel || undefined, maxRuntime, yoloMode: blockYoloMode || undefined, workingDir: blockWorkingDir || undefined },
      };
      setNodes(nds => [...nds, newNode]);
      setExtraData(prev => {
        const newTransformPrompts = new Map(prev.transformPrompts);
        newTransformPrompts.set(newNodeId, transformPrompt);
        const newTransformModels = new Map(prev.transformModels);
        if (transformModel) {
          newTransformModels.set(newNodeId, transformModel);
        }
        return { ...prev, transformPrompts: newTransformPrompts, transformModels: newTransformModels };
      });
      saveBlockConfig(newNodeId);
    }

    setShowTransformModal(false);
    setPendingPosition(null);
    setEditingNodeId(null);
    setTransformPrompt('');
    setTransformModel('');
    resetBlockConfig();
  }, [pendingPosition, editingNodeId, transformPrompt, transformModel, setNodes, blockMaxRuntime, blockYoloMode, blockWorkingDir]);

  const handleGenericPromptConfirm = useCallback(() => {
    if (!genericPromptText.trim()) return;

    const maxRuntime = blockMaxRuntime ? parseInt(blockMaxRuntime, 10) : undefined;

    if (editingNodeId) {
      //
      // Update existing node.
      //
      setExtraData(prev => {
        const newGenericPrompts = new Map(prev.genericPrompts);
        newGenericPrompts.set(editingNodeId, genericPromptText);
        return { ...prev, genericPrompts: newGenericPrompts };
      });
      setNodes(nds => nds.map(n =>
        n.id === editingNodeId
          ? { ...n, data: { ...n.data, prompt: genericPromptText, maxRuntime, yoloMode: blockYoloMode || undefined, workingDir: blockWorkingDir || undefined } }
          : n
      ));
      saveBlockConfig(editingNodeId);
    } else if (pendingPosition) {
      //
      // Add new node.
      //
      const newNodeId = generateUUID();
      const newNode: Node = {
        id: newNodeId,
        type: 'genericPrompt',
        position: pendingPosition,
        data: { label: 'Prompt', prompt: genericPromptText, maxRuntime, yoloMode: blockYoloMode || undefined, workingDir: blockWorkingDir || undefined },
      };
      setNodes(nds => [...nds, newNode]);
      setExtraData(prev => {
        const newGenericPrompts = new Map(prev.genericPrompts);
        newGenericPrompts.set(newNodeId, genericPromptText);
        return { ...prev, genericPrompts: newGenericPrompts };
      });
      saveBlockConfig(newNodeId);
    }

    setShowGenericPromptModal(false);
    setPendingPosition(null);
    setEditingNodeId(null);
    setGenericPromptText('');
    resetBlockConfig();
  }, [pendingPosition, editingNodeId, genericPromptText, setNodes, blockMaxRuntime, blockYoloMode, blockWorkingDir]);

  const handleMemoryConfirm = useCallback(() => {
    if (editingNodeId) {
      setExtraData(prev => {
        const newKeys = new Map(prev.memoryKeys);
        newKeys.set(editingNodeId, memoryKey);
        return { ...prev, memoryKeys: newKeys };
      });
      setNodes(nds => nds.map(n =>
        n.id === editingNodeId
          ? { ...n, data: { ...n.data, memoryKey } }
          : n
      ));
      setShowMemoryModal(false);
      setEditingNodeId(null);
      setMemoryKey('');
    } else {
      const position = pendingPosition || { x: 100, y: 100 + nodes.length * 100 };
      addNodeAtPosition(pendingMemoryType, position, { memoryKey });
      setShowMemoryModal(false);
      setPendingPosition(null);
      setMemoryKey('');
    }
  }, [pendingPosition, editingNodeId, pendingMemoryType, memoryKey, addNodeAtPosition, setNodes, nodes.length]);

  const handleLoopConfirm = useCallback(() => {
    if (editingNodeId) {
      setExtraData(prev => {
        const newMap = new Map(prev.loopMaxIterations);
        newMap.set(editingNodeId, loopMaxIterations);
        return { ...prev, loopMaxIterations: newMap };
      });
      setNodes(nds => nds.map(n =>
        n.id === editingNodeId
          ? { ...n, data: { ...n.data, maxIterations: loopMaxIterations } }
          : n
      ));
      setShowLoopModal(false);
      setEditingNodeId(null);
    } else {
      const position = pendingPosition || { x: 100, y: 100 + nodes.length * 100 };
      addNodeAtPosition('loop', position, { maxIterations: loopMaxIterations });
      setShowLoopModal(false);
      setPendingPosition(null);
    }
  }, [pendingPosition, editingNodeId, loopMaxIterations, addNodeAtPosition, setNodes, nodes.length]);

  const canSave = name.trim().length > 0;

  const handleSave = () => {
    if (!canSave) return;
    const definition = flowToChain(nodes, edges, name.trim(), description, category, timeout, extraData);
    onSave(definition);
  };

  //
  // Handle keyboard shortcuts.
  //
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      //
      // Delete/Backspace removes hovered node or edge (if no text input is
      // focused).
      //
      if ((event.key === 'Delete' || event.key === 'Backspace') && (hoveredNodeId || hoveredEdgeId)) {
        const activeElement = document.activeElement;
        const isInputFocused = activeElement instanceof HTMLInputElement ||
                               activeElement instanceof HTMLTextAreaElement ||
                               activeElement instanceof HTMLSelectElement;
        if (!isInputFocused) {
          event.preventDefault();

          //
          // Delete hovered edge.
          //
          if (hoveredEdgeId) {
            setEdges((eds) => eds.filter((e) => e.id !== hoveredEdgeId));
            setHoveredEdgeId(null);
            return;
          }

          //
          // Delete hovered node.
          //
          if (hoveredNodeId) {
            //
            // Also remove from extraData.
            //
            setExtraData(prev => {
              const newSessionGroups = new Map(prev.sessionGroups);
              const newBlockConfigs = new Map(prev.blockConfigs);
              const newTransformPrompts = new Map(prev.transformPrompts);
              const newTransformModels = new Map(prev.transformModels);
              const newGenericPrompts = new Map(prev.genericPrompts);
              const newMemoryKeys = new Map(prev.memoryKeys);
              const newLoopMaxIters = new Map(prev.loopMaxIterations);
              newSessionGroups.delete(hoveredNodeId);
              newBlockConfigs.delete(hoveredNodeId);
              newTransformPrompts.delete(hoveredNodeId);
              newTransformModels.delete(hoveredNodeId);
              newGenericPrompts.delete(hoveredNodeId);
              newMemoryKeys.delete(hoveredNodeId);
              newLoopMaxIters.delete(hoveredNodeId);
              return {
                ...prev,
                sessionGroups: newSessionGroups,
                blockConfigs: newBlockConfigs,
                transformPrompts: newTransformPrompts,
                transformModels: newTransformModels,
                genericPrompts: newGenericPrompts,
                memoryKeys: newMemoryKeys,
                loopMaxIterations: newLoopMaxIters,
              };
            });
            setNodes((nds) => nds.filter((n) => n.id !== hoveredNodeId));
            //
            // Also remove edges connected to this node.
            //
            setEdges((eds) => eds.filter((e) => e.source !== hoveredNodeId && e.target !== hoveredNodeId));
            setHoveredNodeId(null);
          }
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [hoveredNodeId, hoveredEdgeId, setNodes, setEdges, setExtraData]);

  //
  // Auto-fit view only on initial load (when entering edit mode with existing
  // chain).
  //
  const initialFitDone = useRef(!chain);
  useEffect(() => {
    if (nodes.length > 0 && !initialFitDone.current) {
      initialFitDone.current = true;
      const timer = window.setTimeout(() => {
        fitView({ padding: 0.2, maxZoom: 1.5 });
      }, 50);
      return () => window.clearTimeout(timer);
    }
  }, [nodes.length, fitView]);

  //
  // Node hover handlers.
  //
  const onNodeMouseEnter = useCallback((_: React.MouseEvent, node: Node) => {
    setHoveredNodeId(node.id);
  }, []);

  const onNodeMouseLeave = useCallback(() => {
    setHoveredNodeId(null);
  }, []);

  //
  // Edge hover handlers.
  //
  const onEdgeMouseEnter = useCallback((_: React.MouseEvent, edge: Edge) => {
    setHoveredEdgeId(edge.id);
    //
    // Update edge style to highlight.
    //
    setEdges(eds => eds.map(e =>
      e.id === edge.id
        ? { ...e, style: { ...e.style, stroke: 'var(--accent-error)', strokeWidth: 3 } }
        : e
    ));
  }, [setEdges]);

  //
  // Double-click edge to cycle condition: None → OnSuccess → OnFailure → None.
  //
  const onEdgeDoubleClick = useCallback((_: React.MouseEvent, edge: Edge) => {
    setEdges(eds => eds.map(e => {
      if (e.id !== edge.id) return e;
      const currentCondition = (e.data as Record<string, unknown>)?.condition as string | null;
      let nextCondition: string | null;
      let stroke: string;
      let label: string | undefined;

      if (!currentCondition) {
        nextCondition = 'OnSuccess';
        stroke = 'var(--accent-success)';
        label = 'Success';
      } else if (currentCondition === 'OnSuccess') {
        nextCondition = 'OnFailure';
        stroke = 'var(--accent-error)';
        label = 'Failure';
      } else {
        nextCondition = null;
        stroke = 'var(--text-secondary)';
        label = undefined;
      }

      return {
        ...e,
        style: { ...e.style, stroke },
        label,
        labelStyle: label ? { fill: stroke, fontSize: 10, fontWeight: 500 } : undefined,
        data: { ...((e.data as object) || {}), condition: nextCondition },
      };
    }));
  }, [setEdges]);

  const onEdgeMouseLeave = useCallback((_: React.MouseEvent, edge: Edge) => {
    setHoveredEdgeId(null);

    //
    // Reset edge style, preserving condition-based colors.
    //

    setEdges(eds => eds.map(e => {
      if (e.id !== edge.id) return e;
      const condition = (e.data as Record<string, unknown>)?.condition as string | null;
      let stroke = 'var(--text-secondary)';
      if (condition === 'OnSuccess') stroke = 'var(--accent-success)';
      else if (condition === 'OnFailure') stroke = 'var(--accent-error)';
      return { ...e, style: { ...e.style, stroke, strokeWidth: 2 } };
    }));
  }, [setEdges]);

  //
  // Handle node click for selection.
  //
  const onNodeClick = useCallback((_event: React.MouseEvent, _node: Node) => {
    //
    // Selection is handled natively by React Flow via multiSelectionKeyCode.
    //
  }, []);

  //
  // Handle click on empty canvas to deselect all.
  //
  const onPaneClick = useCallback(() => {
    setNodes(nds => nds.map(n => ({ ...n, selected: false })));
  }, [setNodes]);

  //
  // Handle double-click on nodes to open configuration modal.
  //
  const onNodeDoubleClick = useCallback((_event: React.MouseEvent, node: Node) => {
    if (node.type === 'operation') {
      setEditingNodeId(node.id);
      setSelectedOperation((node.data as Record<string, unknown>)?.operation as string || '');
      loadBlockConfig(node.id);
      setShowOperationModal(true);
    } else if (node.type === 'transform') {
      setEditingNodeId(node.id);
      setTransformPrompt(extraData.transformPrompts.get(node.id) || '');
      setTransformModel(extraData.transformModels.get(node.id) || '');
      loadBlockConfig(node.id);
      setShowTransformModal(true);
    } else if (node.type === 'genericPrompt') {
      setEditingNodeId(node.id);
      setGenericPromptText(extraData.genericPrompts.get(node.id) || '');
      loadBlockConfig(node.id);
      setShowGenericPromptModal(true);
    } else if (node.type === 'memoryStore' || node.type === 'memoryRetrieve') {
      setEditingNodeId(node.id);
      setPendingMemoryType(node.type as 'memoryStore' | 'memoryRetrieve');
      setMemoryKey(extraData.memoryKeys.get(node.id) || '');
      setShowMemoryModal(true);
    } else if (node.type === 'loop') {
      setEditingNodeId(node.id);
      setLoopMaxIterations(extraData.loopMaxIterations.get(node.id) || 3);
      setShowLoopModal(true);
    }
  }, [extraData]);

  return (
    <div className="flex flex-col h-full">
      {/*
      //
      // Header.
      //
      */}
      <div className="flex items-center justify-between px-4 py-2.5 border-b border-subtle bg-[var(--bg-tertiary)]">
        <div className="flex items-center gap-3">
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Chain name *"
            className={`bg-[var(--bg-primary)] border px-3 py-1.5 text-sm text-highlight w-40 focus:outline-none transition-colors ${
              name.trim() ? 'border-dim focus:border-subtle' : 'border-[var(--accent-error)]'
            }`}
          />
          <input
            type="text"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Description"
            className="bg-[var(--bg-primary)] border border-dim px-3 py-1.5 text-sm text-highlight flex-1 min-w-[150px] focus:outline-none focus:border-subtle transition-colors"
          />
          <div className="flex items-center gap-1.5">
            <label className="text-xs tracking-wider text-[var(--text-secondary)]">Timeout:</label>
            <input
              type="number"
              value={timeout}
              onChange={(e) => setTimeout(parseInt(e.target.value) || 1800)}
              min={1}
              className="bg-[var(--bg-primary)] border border-dim px-2 py-1.5 text-sm text-highlight w-20 text-center focus:outline-none focus:border-subtle transition-colors"
            />
            <span className="text-xs" style={{ color: 'var(--text-muted)' }}>s</span>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={onCancel}
            className="flex items-center gap-2 px-4 py-2 text-xs tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
          >
            <X size={14} />
            Cancel
          </button>
          <button
            onClick={handleSave}
            disabled={!canSave}
            className="inline-flex items-center gap-2 px-4 py-2 text-xs tracking-wider border border-dim bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:border-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            title={!canSave ? 'Chain name is required' : undefined}
          >
            <Save size={14} />
            Save
          </button>
        </div>
      </div>

      {/*
      //
      // Flow Canvas.
      //
      */}
      <div className="flex-1 min-h-0" ref={reactFlowWrapper}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onDragOver={onDragOver}
          onDrop={onDrop}
          onNodeMouseEnter={onNodeMouseEnter}
          onNodeMouseLeave={onNodeMouseLeave}
          onNodeClick={onNodeClick}
          onNodeDoubleClick={onNodeDoubleClick}
          onEdgeMouseEnter={onEdgeMouseEnter}
          onEdgeMouseLeave={onEdgeMouseLeave}
          onEdgeDoubleClick={onEdgeDoubleClick}
          onPaneClick={onPaneClick}
          onSelectionChange={onSelectionChange}
          nodeTypes={nodeTypes}
          minZoom={0.2}
          maxZoom={2}
          defaultViewport={{ x: 0, y: 0, zoom: 0.8 }}
          deleteKeyCode={['Delete', 'Backspace']}
          connectionLineStyle={{ stroke: 'var(--accent-info)', strokeWidth: 2 }}
          defaultEdgeOptions={{
            type: 'smoothstep',
            style: { stroke: 'var(--text-secondary)', strokeWidth: 2 },
            markerEnd: { type: MarkerType.ArrowClosed },
          }}
          snapToGrid
          snapGrid={[10, 10]}
          selectionMode={SelectionMode.Partial}
          selectionOnDrag
          selectionKeyCode={['Control', 'Meta']}
          multiSelectionKeyCode={['Control', 'Meta']}
          panOnDrag
          panOnScroll={false}
          selectNodesOnDrag={false}
          proOptions={{ hideAttribution: true }}
        >
          <Background variant={BackgroundVariant.Dots} gap={20} size={1} color="var(--text-secondary)" />

          {/*
          //
          // Fit View Button.
          //
          */}
          <Panel position="bottom-right" className="!m-2">
            <button
              onClick={() => fitView({ padding: 0.2, maxZoom: 1.5 })}
              className="p-1.5 bg-[var(--bg-secondary)] border border-subtle rounded hover:bg-[var(--bg-tertiary)] transition-colors"
              title="Fit to view"
            >
              <Maximize2 size={14} className="text-[var(--text-secondary)]" />
            </button>
          </Panel>

          {/*
          //
          // Element Palette.
          //
          */}
          <Panel position="top-left" className="!m-2" style={{ maxHeight: 'calc(100% - 40px)' }}>
            <div
              className="bg-[var(--bg-secondary)] border border-[var(--border-color)] p-3 flex flex-col gap-0.5 overflow-y-auto"
              style={{ maxHeight: 'calc(100%)', borderRadius: 2, boxShadow: '3px 3px 0 0 rgba(0,0,0,0.4)' }}
            >
              <div className="text-[11px] tracking-widest text-[var(--text-secondary)] mb-2 px-1" style={{ letterSpacing: '0.1em' }}>ELEMENTS</div>
              <div className="flex flex-col gap-0.5">
                <PaletteItem
                  type="trigger"
                  icon={<Play size={20} className={hasTrigger ? "text-[var(--text-secondary)]" : "text-[var(--accent-success)]"} />}
                  label="Trigger"
                  disabled={hasTrigger}
                  onClick={() => handleQuickAdd('trigger')}
                />
                <PaletteItem
                  type="operation"
                  icon={<Cpu size={20} className="text-[var(--accent-info)]" />}
                  label="Operation"
                  onClick={() => handleQuickAdd('operation')}
                />
                <PaletteItem
                  type="transform"
                  icon={<Sparkles size={20} className="text-[var(--accent-warning)]" />}
                  label="Transform"
                  onClick={() => handleQuickAdd('transform')}
                />
                <PaletteItem
                  type="genericPrompt"
                  icon={<MessageSquare size={20} className="text-[var(--accent-purple)]" />}
                  label="Prompt"
                  onClick={() => handleQuickAdd('genericPrompt')}
                />
                <PaletteItem
                  type="memoryStore"
                  icon={<Database size={20} className="text-[var(--accent-success)]" />}
                  label="Mem Store"
                  onClick={() => handleQuickAdd('memoryStore')}
                />
                <PaletteItem
                  type="memoryRetrieve"
                  icon={<HardDriveDownload size={20} className="text-[var(--accent-info)]" />}
                  label="Mem Load"
                  onClick={() => handleQuickAdd('memoryRetrieve')}
                />
                <PaletteItem
                  type="loop"
                  icon={<RefreshCw size={20} className="text-[var(--accent-warning)]" />}
                  label="Loop"
                  onClick={() => handleQuickAdd('loop')}
                />
              </div>
            </div>
          </Panel>

          {/*
          //
          // Session Grouping Panel.
          //
          */}
          {canGroupSelection && (
            <Panel position="top-center" className="!m-2">
              <div className="ascii-box bg-[var(--bg-secondary)] p-2.5 flex items-center gap-2">
                <span className="text-xs tracking-wider text-[var(--text-secondary)]">
                  {groupableSelectedNodes.length} nodes selected
                </span>
                <button
                  onClick={handleGroupIntoSession}
                  className="flex items-center gap-2 px-3 py-1.5 text-xs tracking-wider border border-dim bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:border-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors"
                  title="Group selected nodes into a shared session"
                >
                  <Users size={12} />
                  Group into Session
                </button>
              </div>
            </Panel>
          )}

          {/*
          //
          // Ungroup Panel - show when selected nodes have session groups.
          //
          */}
          {groupableSelectedNodes.length > 0 && groupableSelectedNodes.some(n => extraData.sessionGroups.has(n.id)) && (
            <Panel position="top-center" className="!m-2 !mt-14">
              <div className="ascii-box bg-[var(--bg-secondary)] p-2.5 flex items-center gap-2">
                <button
                  onClick={handleUngroupSelection}
                  className="flex items-center gap-2 px-3 py-1.5 text-xs tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
                  title="Remove selected nodes from their session group"
                >
                  <GitMerge size={12} />
                  Remove from Session
                </button>
              </div>
            </Panel>
          )}

          {/*
          //
          // Help Text.
          //
          */}
          <Panel position="bottom-left" className="!m-2">
            <div className="text-[10px] tracking-wide border border-dim bg-[var(--bg-secondary)]/95 px-2.5 py-1.5" style={{ color: 'var(--text-muted)' }}>
              Drag from handles to connect • Double-click connection for Success/Failure • Ctrl+Click to multi-select • Delete to remove
            </div>
          </Panel>
        </ReactFlow>
      </div>

      {/*
      //
      // Operation Selection Modal.
      //
      */}
      <ConfigModal
        isOpen={showOperationModal}
        onClose={() => {
          setShowOperationModal(false);
          setPendingPosition(null);
          setSelectedOperation('');
          setEditingNodeId(null);
          resetBlockConfig();
        }}
        title={editingNodeId ? 'Edit Operation' : 'Select Operation'}
        size="sm"
        config={[
          {
            type: 'section',
            fields: [
              {
                name: 'operation',
                label: 'Operation',
                type: 'select',
                required: true,
                span: 'full',
                options: [
                  { value: '', label: 'Select an operation...' },
                  ...operationDefs.map((op) => ({
                    value: op.full_name,
                    label: `${op.name} (${op.full_name})`,
                  })),
                ],
              },
            ],
          },
          advancedSectionConfig,
        ]}
        values={{ operation: selectedOperation, ...blockConfigValues }}
        onChange={(name, value) => {
          if (name === 'operation') setSelectedOperation(value);
          else handleBlockConfigChange(name, value);
        }}
        onSubmit={handleOperationSelect}
        submitLabel={editingNodeId ? 'Update' : 'Add'}
        submitIcon={<Cpu size={14} />}
        submitVariant="info"
        submitDisabled={!selectedOperation}
      />

      {/*
      //
      // Transform Configuration Modal.
      //
      */}
      <ConfigModal
        isOpen={showTransformModal}
        onClose={() => {
          setShowTransformModal(false);
          setPendingPosition(null);
          setEditingNodeId(null);
          setTransformPrompt('');
          setTransformModel('');
          resetBlockConfig();
        }}
        title={editingNodeId ? 'Edit Transform' : 'Configure Transform'}
        size="sm"
        config={[
          {
            type: 'section',
            fields: [
              {
                name: 'model',
                label: 'Model',
                type: 'select',
                options: [
                  { value: '', label: 'Use default model' },
                  ...modelDefs.map((m) => ({ value: m.name, label: m.name })),
                ],
                span: 'full',
                help: modelDefs.length === 0
                  ? 'No models configured. Configure models in Settings.'
                  : 'Select a model or use the default semantic operations model.',
              },
              {
                name: 'prompt',
                label: 'Prompt',
                type: 'textarea',
                required: true,
                rows: 6,
                placeholder: 'Enter the prompt for transforming the input data...',
                span: 'full',
                help: 'The LLM will process the input with this prompt and pass the result forward.',
              },
            ],
          },
          advancedSectionConfig,
        ]}
        values={{
          model: transformModel,
          prompt: transformPrompt,
          ...blockConfigValues,
        }}
        onChange={(name, value) => {
          if (name === 'model') setTransformModel(value);
          else if (name === 'prompt') setTransformPrompt(value);
          else handleBlockConfigChange(name, value);
        }}
        onSubmit={handleTransformConfirm}
        submitLabel={editingNodeId ? 'Update' : 'Add'}
        submitIcon={<Sparkles size={14} />}
        submitVariant="warning"
        submitDisabled={!transformPrompt.trim()}
      />

      {/*
      //
      // Generic Prompt Configuration Modal.
      //
      */}
      <ConfigModal
        isOpen={showGenericPromptModal}
        onClose={() => {
          setShowGenericPromptModal(false);
          setPendingPosition(null);
          setEditingNodeId(null);
          setGenericPromptText('');
          resetBlockConfig();
        }}
        title={editingNodeId ? 'Edit Prompt' : 'Configure Prompt'}
        size="sm"
        config={[
          {
            type: 'section',
            fields: [
              {
                name: 'prompt',
                label: 'Prompt',
                type: 'textarea',
                placeholder: 'Enter the prompt to send to the agent...',
                required: true,
                rows: 6,
                span: 'full',
                help: 'This prompt will be sent to the agent via the session. If first in a session group, input data will be included.',
              },
            ],
          },
          advancedSectionConfig,
        ]}
        values={{ prompt: genericPromptText, ...blockConfigValues }}
        onChange={(name, value) => {
          if (name === 'prompt') setGenericPromptText(value);
          else handleBlockConfigChange(name, value);
        }}
        onSubmit={handleGenericPromptConfirm}
        submitLabel={editingNodeId ? 'Update' : 'Add'}
        submitIcon={<MessageSquare size={14} />}
        submitVariant="purple"
        submitDisabled={!genericPromptText.trim()}
      />

      {/*
      //
      // Memory Key Configuration Modal.
      //
      */}
      <ConfigModal
        isOpen={showMemoryModal}
        onClose={() => {
          setShowMemoryModal(false);
          setPendingPosition(null);
          setMemoryKey('');
          setEditingNodeId(null);
        }}
        size="sm"
        title={pendingMemoryType === 'memoryStore' ? 'Configure Memory Store' : 'Configure Memory Retrieve'}
        config={[
          {
            type: 'section',
            fields: [
              {
                name: 'memoryKey',
                label: 'Memory Key',
                type: 'text' as const,
                placeholder: 'Enter a unique key for this memory slot...',
                span: 'full' as const,
              },
            ],
          },
        ]}
        values={{ memoryKey }}
        onChange={(_name, value) => setMemoryKey(value)}
        onSubmit={handleMemoryConfirm}
        submitLabel={editingNodeId ? 'Update' : 'Add'}
        submitIcon={pendingMemoryType === 'memoryStore' ? <Database size={14} /> : <HardDriveDownload size={14} />}
        submitVariant={pendingMemoryType === 'memoryStore' ? 'success' : 'info'}
        submitDisabled={!memoryKey.trim()}
      />

      {/*
      //
      // Loop Configuration Modal.
      //
      */}
      <ConfigModal
        isOpen={showLoopModal}
        onClose={() => {
          setShowLoopModal(false);
          setPendingPosition(null);
          setEditingNodeId(null);
        }}
        size="sm"
        title="Configure Loop"
        config={[
          {
            type: 'section',
            fields: [
              {
                name: 'loopMaxIterations',
                label: 'Max Iterations',
                type: 'text' as const,
                placeholder: 'Maximum number of loop iterations...',
                span: 'full' as const,
              },
            ],
          },
        ]}
        values={{ loopMaxIterations: String(loopMaxIterations) }}
        onChange={(_name, value) => setLoopMaxIterations(parseInt(value) || 3)}
        onSubmit={handleLoopConfirm}
        submitLabel={editingNodeId ? 'Update' : 'Add'}
        submitIcon={<RefreshCw size={14} />}
        submitVariant="warning"
        submitDisabled={loopMaxIterations < 1}
      />
    </div>
  );
}

interface ChainBuilderProps {
  chain?: ChainDefinitionFull | null;
  onSave: (definition: ChainDefinitionInput) => void;
  onCancel: () => void;
  operationDefs: OperationDefinitionInfo[];
  modelDefs?: ModelDefinition[];
}

export function ChainBuilder({ modelDefs = [], ...props }: ChainBuilderProps) {
  return (
    <ReactFlowProvider>
      <ChainBuilderInner {...props} modelDefs={modelDefs} />
    </ReactFlowProvider>
  );
}
