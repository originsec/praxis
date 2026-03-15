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
  SelectionMode,
} from '@xyflow/react';
import type { Node, Edge, Connection, OnSelectionChangeParams } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { Play, X, Save, Copy, Download, Cpu, Maximize2, GitMerge, Sparkles, MessageSquare, Users, Database, RefreshCw, LayoutGrid, Square, Settings, Check, AlertTriangle, Wrench, FileText } from 'lucide-react';
import { ConfigModal } from '../common/ConfigModal';
import { Modal } from '../common/Modal';
import { ChainTriggerPanel } from './ChainTriggerPanel';
import type {
  BlockConfig,
  ChainDefinitionFull,
  ChainDefinitionInput,
  ChainElement,
  ChainConnection as ChainConnectionType,
  NodeState,
  OperationDefinitionInfo,
  SessionGroup,
  ToolkitToolInfo,
  PayloadInfo,
  BrowserMessage,
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
import { nodeTypes } from './ChainNodes';
import type { OperationNodeData } from './ChainNodes';

//
// Extra data tracked separately (prompts, models, session groups).
//
interface MemoryConfig {
  key: string;
  mode: 'Store' | 'Retrieve';
}

interface ToolConfig {
  tool_name: string;
  tool_params: Record<string, unknown>;
}

interface ChainExtraData {
  transformPrompts: Map<string, string>;
  transformModels: Map<string, string>;
  genericPrompts: Map<string, string>;
  sessionGroups: Map<string, SessionGroup>;
  blockConfigs: Map<string, BlockConfig>;
  memoryConfigs: Map<string, MemoryConfig>;
  loopMaxIterations: Map<string, number>;
  toolConfigs: Map<string, ToolConfig>;
  payloadConfigs: Map<string, string>;
}

//
// Convert chain definition to React Flow nodes and edges (positions computed
// via dagre).
//
function chainToFlow(chain: ChainDefinitionFull | null, operationDefs?: OperationDefinitionInfo[], payloadList?: PayloadInfo[]): { nodes: Node[]; edges: Edge[]; extraData: ChainExtraData } {
  const emptyExtraData: ChainExtraData = {
    transformPrompts: new Map(),
    transformModels: new Map(),
    genericPrompts: new Map(),
    sessionGroups: new Map(),
    blockConfigs: new Map(),
    memoryConfigs: new Map(),
    loopMaxIterations: new Map(),
    toolConfigs: new Map(),
    payloadConfigs: new Map(),
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
            requireAllInputs: elem.block_config?.require_all_inputs,
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
            requireAllInputs: elem.block_config?.require_all_inputs,
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
            requireAllInputs: elem.block_config?.require_all_inputs,
          },
        };
      case 'Memory':
        extraData.memoryConfigs.set(elem.id, { key: elem.key, mode: elem.mode });
        return {
          id: elem.id,
          type: 'memory',
          position,
          data: { label: 'Memory', memoryKey: elem.key, memoryMode: elem.mode },
        };
      case 'Loop':
        extraData.loopMaxIterations.set(elem.id, elem.max_iterations);
        return {
          id: elem.id,
          type: 'loop',
          position,
          data: { label: 'Loop', maxIterations: elem.max_iterations },
        };
      case 'Tool':
        extraData.toolConfigs.set(elem.id, { tool_name: elem.tool_name, tool_params: elem.tool_params });
        if (elem.block_config) {
          extraData.blockConfigs.set(elem.id, elem.block_config);
        }
        return {
          id: elem.id,
          type: 'tool',
          position,
          data: { label: 'Tool', toolName: elem.tool_name, maxRuntime: elem.block_config?.max_runtime },
        };
      case 'Payload': {
        extraData.payloadConfigs.set(elem.id, elem.payload_id);
        if (elem.block_config) {
          extraData.blockConfigs.set(elem.id, elem.block_config);
        }
        const plMatch = (payloadList || []).find(p => p.id === elem.payload_id);
        return {
          id: elem.id,
          type: 'payload',
          position,
          data: { label: 'Payload', shortname: plMatch?.shortname || elem.payload_id.slice(0, 8), content: plMatch?.content },
        };
      }
      case 'Termination':
        if (elem.block_config) {
          extraData.blockConfigs.set(elem.id, elem.block_config);
        }
        return {
          id: elem.id,
          type: 'termination',
          position,
          data: {
            label: 'End',
            requireAllInputs: elem.block_config?.require_all_inputs,
          },
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
      case 'memory': {
        const memCfg = extraData.memoryConfigs.get(node.id);
        return {
          element_type: 'Memory' as const,
          id: node.id,
          key: memCfg?.key || '',
          mode: memCfg?.mode || 'Store',
        };
      }
      case 'loop':
        return {
          element_type: 'Loop' as const,
          id: node.id,
          max_iterations: extraData.loopMaxIterations.get(node.id) || 3,
        };
      case 'tool': {
        const toolCfg = extraData.toolConfigs.get(node.id);
        return {
          element_type: 'Tool' as const,
          id: node.id,
          tool_name: toolCfg?.tool_name || '',
          tool_params: toolCfg?.tool_params || {},
          block_config: extraData.blockConfigs.get(node.id) || null,
        };
      }
      case 'payload':
        return {
          element_type: 'Payload' as const,
          id: node.id,
          payload_id: extraData.payloadConfigs.get(node.id) || '',
          block_config: extraData.blockConfigs.get(node.id) || null,
        };
      case 'termination':
        return {
          element_type: 'Termination' as const,
          id: node.id,
          block_config: extraData.blockConfigs.get(node.id) || null,
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
      className={`flex items-center gap-1.5 py-1.5 px-2 transition-all group ${
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
      <span className="text-[10px] tracking-wider text-[var(--text-secondary)] group-hover:text-highlight transition-colors">{label}</span>
    </div>
  );
}

interface ChainBuilderInnerProps {
  chain?: ChainDefinitionFull | null;
  onSave: (definition: ChainDefinitionInput, onResult?: (result: 'saved' | 'error') => void) => void;
  onDuplicate?: (definition: ChainDefinitionInput) => void;
  onExport?: (definition: ChainDefinitionInput) => void;
  onCancel: () => void;
  operationDefs: OperationDefinitionInfo[];
  modelDefs: ModelDefinition[];
  nodes: NodeState[];
  toolkitTools: ToolkitToolInfo[];
  payloads: PayloadInfo[];
  send: (msg: BrowserMessage) => void;
  saveStatus?: string | null;
  saveError?: string | null;
}

function ChainBuilderInner({ chain, onSave, onDuplicate, onExport, onCancel, operationDefs, modelDefs, nodes: _systemNodes, toolkitTools, payloads, send, saveStatus, saveError }: ChainBuilderInnerProps) {
  const [name, setName] = useState(chain?.name || '');
  const [description, setDescription] = useState(chain?.description || '');
  const [timeout, setChainTimeout] = useState(chain?.timeout || 1800);
  const category = 'default';

  const initialFlow = chainToFlow(chain || null, operationDefs, payloads);
  const [nodes, setNodes, onNodesChange] = useNodesState(initialFlow.nodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(initialFlow.edges);

  //
  // Track extra data (prompts, models, session groups) separately.
  //
  const [extraData, setExtraData] = useState<ChainExtraData>(() => initialFlow.extraData);

  //
  // Re-resolve payload node data when payloads list arrives after initial load.
  //
  useEffect(() => {
    if (payloads.length === 0) return;
    setNodes(nds => {
      let changed = false;
      const updated = nds.map(n => {
        if (n.type !== 'payload') return n;
        const payloadId = extraData.payloadConfigs.get(n.id);
        if (!payloadId) return n;
        const pl = payloads.find(p => p.id === payloadId);
        if (!pl) return n;
        const data = n.data as Record<string, unknown>;
        if (data.shortname === pl.shortname && data.content === pl.content) return n;
        changed = true;
        return { ...n, data: { ...data, shortname: pl.shortname, content: pl.content } };
      });
      return changed ? updated : nds;
    });
  }, [payloads, extraData.payloadConfigs, setNodes]);

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
  const [memoryMode, setMemoryMode] = useState<'Store' | 'Retrieve'>('Store');

  const [showLoopModal, setShowLoopModal] = useState(false);
  const [loopMaxIterations, setLoopMaxIterations] = useState<number>(3);

  const [showToolModal, setShowToolModal] = useState(false);
  const [toolModalToolName, setToolModalToolName] = useState('');
  const [toolModalParams, setToolModalParams] = useState<Record<string, unknown>>({});
  const [showPayloadModal, setShowPayloadModal] = useState(false);
  const [payloadModalSelectedId, setPayloadModalSelectedId] = useState<string | null>(null);
  const [payloadEditName, setPayloadEditName] = useState('');
  const [payloadEditContent, setPayloadEditContent] = useState('');
  const [payloadEditId, setPayloadEditId] = useState<string | null>(null);
  const [showPayloadForm, setShowPayloadForm] = useState(false);

  //
  // Modal state for session group configuration.
  //
  const [showSessionGroupModal, setShowSessionGroupModal] = useState(false);
  const [sessionGroupYolo, setSessionGroupYolo] = useState(false);
  const [sessionGroupWorkingDir, setSessionGroupWorkingDir] = useState('');
  const [editingSessionGroupId, setEditingSessionGroupId] = useState<string | null>(null);

  //
  // Per-block config state (shared across Operation, Transform, GenericPrompt
  // modals).
  //
  const [blockMaxRuntime, setBlockMaxRuntime] = useState<string>('');
  const [blockYoloMode, setBlockYoloMode] = useState<boolean>(false);
  const [blockWorkingDir, setBlockWorkingDir] = useState<string>('');
  const [blockRequireAllInputs, setBlockRequireAllInputs] = useState<boolean>(true);

  const advancedSectionConfig = {
    type: 'section' as const,
    title: 'Additional settings',
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
      {
        name: 'requireAllInputs',
        label: 'Require All Inputs',
        type: 'toggle' as const,
        span: 'full' as const,
        help: 'When off, runs with partial inputs at merge points where some branches don\'t fire.',
      },
    ],
  };

  const blockConfigValues = {
    maxRuntime: blockMaxRuntime,
    workingDir: blockWorkingDir,
    yoloMode: blockYoloMode,
    requireAllInputs: blockRequireAllInputs,
  };

  const handleBlockConfigChange = (name: string, value: any) => {
    if (name === 'maxRuntime') setBlockMaxRuntime(value);
    if (name === 'workingDir') setBlockWorkingDir(value);
    if (name === 'yoloMode') setBlockYoloMode(!!value);
    if (name === 'requireAllInputs') setBlockRequireAllInputs(!!value);
  };

  const resetBlockConfig = () => {
    setBlockMaxRuntime('');
    setBlockYoloMode(false);
    setBlockWorkingDir('');
    setBlockRequireAllInputs(true);
  };

  const loadBlockConfig = (nodeId: string) => {
    const existing = extraData.blockConfigs.get(nodeId);
    setBlockMaxRuntime(existing?.max_runtime ? String(existing.max_runtime) : '');
    setBlockYoloMode(existing?.yolo_mode || false);
    setBlockWorkingDir(existing?.working_dir || '');
    setBlockRequireAllInputs(existing?.require_all_inputs !== false);
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
    if (!blockRequireAllInputs) blockConfig.require_all_inputs = false;

    setExtraData(prev => {
      const newConfigs = new Map(prev.blockConfigs);
      if (blockConfig.max_runtime || blockConfig.yolo_mode || blockConfig.working_dir || blockConfig.require_all_inputs === false) {
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
  const hasTermination = nodes.some(n => n.type === 'termination');

  //
  // Check which selected nodes can be grouped (Operations, Transforms,
  // GenericPrompts only).
  //
  const groupableSelectedNodes = useMemo(() => {
    return nodes.filter(n =>
      selectedNodeIds.has(n.id) &&
      (n.type === 'operation' || n.type === 'genericPrompt')
    );
  }, [nodes, selectedNodeIds]);

  const canGroupSelection = groupableSelectedNodes.length >= 2;

  //
  // Check if all selected groupable nodes share the same session group.
  //
  const selectedSessionGroup = useMemo(() => {
    if (groupableSelectedNodes.length === 0) return null;
    const groups = groupableSelectedNodes
      .map(n => extraData.sessionGroups.get(n.id))
      .filter((g): g is SessionGroup => g != null);
    if (groups.length === 0) return null;
    const firstId = groups[0].id;
    if (groups.every(g => g.id === firstId)) return groups[0];
    return null;
  }, [groupableSelectedNodes, extraData.sessionGroups]);

  //
  // Handle selection change.
  //
  const onSelectionChange = useCallback((params: OnSelectionChangeParams) => {
    setSelectedNodeIds(new Set(params.nodes.map(n => n.id)));
  }, []);

  //
  // Group selected nodes into a session — show config modal first.
  //
  const handleGroupIntoSession = useCallback(() => {
    if (!canGroupSelection) return;
    setEditingSessionGroupId(null);
    setSessionGroupYolo(false);
    setSessionGroupWorkingDir('');
    setShowSessionGroupModal(true);
  }, [canGroupSelection]);

  //
  // Confirm session group creation/edit from modal.
  //
  const handleSessionGroupConfirm = useCallback(() => {
    if (editingSessionGroupId) {
      //
      // Editing existing session group — update all nodes in this group.
      //
      setExtraData(prev => {
        const newSessionGroups = new Map(prev.sessionGroups);
        for (const [nodeId, sg] of newSessionGroups) {
          if (sg.id === editingSessionGroupId) {
            newSessionGroups.set(nodeId, {
              ...sg,
              yolo_mode: sessionGroupYolo,
              working_dir: sessionGroupWorkingDir || undefined,
            });
          }
        }
        return { ...prev, sessionGroups: newSessionGroups };
      });
    } else {
      //
      // Creating new session group.
      //
      const usedColors = getUsedColors(
        Array.from(extraData.sessionGroups.values()).map(sg => ({ session_group: sg }))
      );
      const newColor = getNextSessionColor(usedColors);
      const newGroupId = generateUUID();

      const newSessionGroup: SessionGroup = {
        id: newGroupId,
        color: newColor,
        yolo_mode: sessionGroupYolo,
        working_dir: sessionGroupWorkingDir || undefined,
      };

      setExtraData(prev => {
        const newSessionGroups = new Map(prev.sessionGroups);
        for (const node of groupableSelectedNodes) {
          newSessionGroups.set(node.id, newSessionGroup);
        }
        return { ...prev, sessionGroups: newSessionGroups };
      });

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

      setSelectedNodeIds(new Set());
    }

    setShowSessionGroupModal(false);
    setEditingSessionGroupId(null);
  }, [editingSessionGroupId, sessionGroupYolo, sessionGroupWorkingDir, groupableSelectedNodes, extraData.sessionGroups, setNodes]);

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
    (params: Connection) => {
      //
      // Loop elements: max one incoming and one outgoing connection.
      //
      const sourceNode = nodes.find(n => n.id === params.source);
      const targetNode = nodes.find(n => n.id === params.target);
      if (sourceNode?.type === 'loop') {
        const existing = edges.filter(e => e.source === params.source);
        if (existing.length >= 1) return;
      }
      if (targetNode?.type === 'loop') {
        const existing = edges.filter(e => e.target === params.target);
        if (existing.length >= 1) return;
      }

      setEdges((eds) => addEdge({
        ...params,
        id: generateUUID(),
        type: 'smoothstep',
        markerEnd: { type: MarkerType.ArrowClosed },
        style: { stroke: 'var(--text-secondary)', strokeWidth: 2 },
      }, eds));
    },
    [setEdges, nodes, edges]
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
      // Prevent adding second trigger or termination.
      //
      if (type === 'trigger' && hasTrigger) {
        return;
      }
      if (type === 'termination' && hasTermination) {
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
      if (type === 'memory') {
        setPendingPosition(position);
        setMemoryKey('');
        setMemoryMode('Store');
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

      if (type === 'tool') {
        setPendingPosition(position);
        setToolModalToolName(toolkitTools.length > 0 ? toolkitTools[0].tool_name : '');
        if (toolkitTools.length > 0) {
          const defaults: Record<string, unknown> = {};
          for (const field of toolkitTools[0].config_schema) {
            if (field.default_value != null) defaults[field.name] = field.default_value;
          }
          setToolModalParams(defaults);
        } else {
          setToolModalParams({});
        }
        setShowToolModal(true);
        return;
      }

      //
      // For other types, create directly.
      //
      addNodeAtPosition(type, position);
    },
    [screenToFlowPosition, hasTrigger, hasTermination, toolkitTools]
  );

  const addNodeAtPosition = useCallback((type: string, position: { x: number; y: number }, nodeExtraData?: Record<string, unknown>) => {
    //
    // Prevent adding second trigger or termination.
    //
    if (type === 'trigger' && hasTrigger) {
      return;
    }
    if (type === 'termination' && hasTermination) {
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
      case 'memory': {
        const mode = (nodeExtraData?.memoryMode as 'Store' | 'Retrieve') || 'Store';
        newNode = {
          id: newId,
          type: 'memory',
          position,
          data: { label: 'Memory', memoryKey: nodeExtraData?.memoryKey || '', memoryMode: mode },
        };
        if (nodeExtraData?.memoryKey) {
          setExtraData(prev => {
            const newConfigs = new Map(prev.memoryConfigs);
            newConfigs.set(newId, { key: nodeExtraData.memoryKey as string, mode });
            return { ...prev, memoryConfigs: newConfigs };
          });
        }
        break;
      }
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
      case 'tool': {
        const toolName = (nodeExtraData?.toolName as string) || '';
        const toolParams = (nodeExtraData?.toolParams as Record<string, unknown>) || {};
        newNode = {
          id: newId,
          type: 'tool',
          position,
          data: { label: 'Tool', toolName, toolDisplayName: nodeExtraData?.toolDisplayName || toolName },
        };
        if (toolName) {
          setExtraData(prev => {
            const newConfigs = new Map(prev.toolConfigs);
            newConfigs.set(newId, { tool_name: toolName, tool_params: toolParams });
            return { ...prev, toolConfigs: newConfigs };
          });
        }
        break;
      }
      case 'payload': {
        const payloadId = (nodeExtraData?.payloadId as string) || '';
        const shortname = (nodeExtraData?.shortname as string) || '';
        const payloadContent = (nodeExtraData?.content as string) || '';
        newNode = {
          id: newId,
          type: 'payload',
          position,
          data: { label: 'Payload', shortname, content: payloadContent },
        };
        if (payloadId) {
          setExtraData(prev => {
            const newConfigs = new Map(prev.payloadConfigs);
            newConfigs.set(newId, payloadId);
            return { ...prev, payloadConfigs: newConfigs };
          });
        }
        break;
      }
      case 'termination':
        newNode = {
          id: newId,
          type: 'termination',
          position,
          data: { label: 'End', requireAllInputs: false },
        };
        setExtraData(prev => {
          const newConfigs = new Map(prev.blockConfigs);
          newConfigs.set(newId, { require_all_inputs: false });
          return { ...prev, blockConfigs: newConfigs };
        });
        break;
      default:
        return;
    }

    setNodes((nds) => [...nds, newNode]);
  }, [setNodes, hasTrigger, hasTermination, setExtraData]);

  //
  // Quick add from palette click (adds at a default position).
  //
  const handleQuickAdd = useCallback((type: string) => {
    //
    // Prevent adding second trigger or termination.
    //
    if (type === 'trigger' && hasTrigger) {
      return;
    }
    if (type === 'termination' && hasTermination) {
      return;
    }

    //
    // Place new element at the center of the current viewport.
    //
    const bounds = reactFlowWrapper.current?.getBoundingClientRect();
    const position = bounds
      ? screenToFlowPosition({ x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 })
      : { x: 100, y: 100 };

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

    if (type === 'memory') {
      setPendingPosition(position);
      setMemoryKey('');
      setMemoryMode('Store');
      setShowMemoryModal(true);
      return;
    }

    if (type === 'loop') {
      setLoopMaxIterations(3);
      setShowLoopModal(true);
      return;
    }

    if (type === 'tool') {
      setPendingPosition(position);
      setToolModalToolName(toolkitTools.length > 0 ? toolkitTools[0].tool_name : '');
      if (toolkitTools.length > 0) {
        const defaults: Record<string, unknown> = {};
        for (const field of toolkitTools[0].config_schema) {
          if (field.default_value != null) defaults[field.name] = field.default_value;
        }
        setToolModalParams(defaults);
      } else {
        setToolModalParams({});
      }
      setShowToolModal(true);
      return;
    }

    if (type === 'payload') {
      setPendingPosition(position);
      setPayloadModalSelectedId(null);
      setPayloadEditName('');
      setPayloadEditContent('');
      setPayloadEditId(null);
      setShowPayloadForm(false);
      send({ type: 'payload_list' });
      setShowPayloadModal(true);
      return;
    }

    addNodeAtPosition(type, position);
  }, [addNodeAtPosition, hasTrigger, hasTermination, screenToFlowPosition, toolkitTools, send]);

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
      requireAllInputs: blockRequireAllInputs === false ? false : undefined,
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
  }, [pendingPosition, editingNodeId, selectedOperation, setNodes, blockMaxRuntime, blockYoloMode, blockWorkingDir, blockRequireAllInputs, operationDefs]);

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
          ? { ...n, data: { ...n.data, prompt: transformPrompt, modelRef: transformModel || undefined, maxRuntime, yoloMode: blockYoloMode || undefined, workingDir: blockWorkingDir || undefined, requireAllInputs: blockRequireAllInputs === false ? false : undefined } }
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
        data: { label: 'Transform', prompt: transformPrompt, modelRef: transformModel || undefined, maxRuntime, yoloMode: blockYoloMode || undefined, workingDir: blockWorkingDir || undefined, requireAllInputs: blockRequireAllInputs === false ? false : undefined },
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
  }, [pendingPosition, editingNodeId, transformPrompt, transformModel, setNodes, blockMaxRuntime, blockYoloMode, blockWorkingDir, blockRequireAllInputs]);

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
          ? { ...n, data: { ...n.data, prompt: genericPromptText, maxRuntime, yoloMode: blockYoloMode || undefined, workingDir: blockWorkingDir || undefined, requireAllInputs: blockRequireAllInputs === false ? false : undefined } }
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
        data: { label: 'Prompt', prompt: genericPromptText, maxRuntime, yoloMode: blockYoloMode || undefined, workingDir: blockWorkingDir || undefined, requireAllInputs: blockRequireAllInputs === false ? false : undefined },
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
  }, [pendingPosition, editingNodeId, genericPromptText, setNodes, blockMaxRuntime, blockYoloMode, blockWorkingDir, blockRequireAllInputs]);

  const handleMemoryConfirm = useCallback(() => {
    if (editingNodeId) {
      setExtraData(prev => {
        const newConfigs = new Map(prev.memoryConfigs);
        newConfigs.set(editingNodeId, { key: memoryKey, mode: memoryMode });
        return { ...prev, memoryConfigs: newConfigs };
      });
      setNodes(nds => nds.map(n =>
        n.id === editingNodeId
          ? { ...n, data: { ...n.data, memoryKey, memoryMode } }
          : n
      ));
      setShowMemoryModal(false);
      setEditingNodeId(null);
      setMemoryKey('');
    } else {
      const position = pendingPosition || { x: 100, y: 100 + nodes.length * 100 };
      addNodeAtPosition('memory', position, { memoryKey, memoryMode });
      setShowMemoryModal(false);
      setPendingPosition(null);
      setMemoryKey('');
    }
  }, [pendingPosition, editingNodeId, memoryMode, memoryKey, addNodeAtPosition, setNodes, nodes.length]);

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

  const handleToolConfirm = useCallback(() => {
    const params: Record<string, unknown> = { ...toolModalParams };
    const selectedTool = toolkitTools.find(t => t.tool_name === toolModalToolName);
    const displayName = selectedTool?.display_name || toolModalToolName;

    if (editingNodeId) {
      setExtraData(prev => {
        const newConfigs = new Map(prev.toolConfigs);
        newConfigs.set(editingNodeId, { tool_name: toolModalToolName, tool_params: params });
        return { ...prev, toolConfigs: newConfigs };
      });
      setNodes(nds => nds.map(n =>
        n.id === editingNodeId
          ? { ...n, data: { ...n.data, toolName: toolModalToolName, toolDisplayName: displayName } }
          : n
      ));
      setShowToolModal(false);
      setEditingNodeId(null);
    } else {
      const position = pendingPosition || { x: 100, y: 100 + nodes.length * 100 };
      addNodeAtPosition('tool', position, { toolName: toolModalToolName, toolDisplayName: displayName, toolParams: params });
      setShowToolModal(false);
      setPendingPosition(null);
    }
    setToolModalToolName('');
    setToolModalParams({});
  }, [pendingPosition, editingNodeId, toolModalToolName, toolModalParams, toolkitTools, addNodeAtPosition, setNodes, nodes.length]);

  const handlePayloadConfirm = useCallback(() => {
    if (!payloadModalSelectedId) return;
    const payload = payloads.find(p => p.id === payloadModalSelectedId);
    const shortname = payload?.shortname || '';
    const content = payload?.content || '';

    if (editingNodeId) {
      setExtraData(prev => {
        const newConfigs = new Map(prev.payloadConfigs);
        newConfigs.set(editingNodeId, payloadModalSelectedId);
        return { ...prev, payloadConfigs: newConfigs };
      });
      setNodes(nds => nds.map(n =>
        n.id === editingNodeId
          ? { ...n, data: { ...n.data, shortname, content } }
          : n
      ));
      setShowPayloadModal(false);
      setEditingNodeId(null);
    } else {
      const position = pendingPosition || { x: 100, y: 100 + nodes.length * 100 };
      addNodeAtPosition('payload', position, { payloadId: payloadModalSelectedId, shortname, content });
      setShowPayloadModal(false);
      setPendingPosition(null);
    }
  }, [pendingPosition, editingNodeId, payloadModalSelectedId, payloads, addNodeAtPosition, setNodes, nodes.length]);

  //
  // Brief "Saved" flash when save succeeds.
  //
  const [saveFlash, setSaveFlash] = useState<'saving' | 'saved' | 'error' | null>(null);
  const saveFlashTimer = useRef<number | null>(null);

  const canSave = name.trim().length > 0;
  const [saveValidationError, setSaveValidationError] = useState<string | null>(null);

  const handleSave = () => {
    if (!canSave || saveFlash === 'saving') return;
    setSaveValidationError(null);

    if (!hasTrigger || !hasTermination) {
      const missing = [!hasTrigger && 'Trigger', !hasTermination && 'End Terminator'].filter(Boolean).join(' and ');
      setSaveValidationError(`Chain requires a ${missing}`);
      window.setTimeout(() => setSaveValidationError(null), 3000);
      return;
    }

    setSaveFlash('saving');
    if (saveFlashTimer.current) window.clearTimeout(saveFlashTimer.current);
    const definition = flowToChain(nodes, edges, name.trim(), description, category, timeout, extraData);
    onSave(definition, (result) => {
      setSaveFlash(result);
      saveFlashTimer.current = window.setTimeout(() => setSaveFlash(null), result === 'error' ? 3000 : 2000);
    });
  };

  useEffect(() => {
    if (saveFlash !== 'saving') return;
    if (!saveStatus && !saveError) return;

    const result: 'saved' | 'error' = saveError ? 'error' : 'saved';
    setSaveFlash(result);
    if (saveFlashTimer.current) window.clearTimeout(saveFlashTimer.current);
    saveFlashTimer.current = window.setTimeout(() => setSaveFlash(null), result === 'error' ? 3000 : 2000);
  }, [saveFlash, saveStatus, saveError]);

  //
  // Map React Flow node types back to ChainElement element_type for dagre.
  //
  const nodeTypeToElementType: Record<string, string> = {
    trigger: 'Trigger',
    operation: 'Operation',
    transform: 'Transform',
    genericPrompt: 'GenericPrompt',
    memory: 'Memory',
    loop: 'Loop',
    termination: 'Termination',
  };

  const handleAutoLayout = useCallback(() => {
    if (nodes.length === 0) return;

    const elements = nodes.map(n => ({
      id: n.id,
      element_type: nodeTypeToElementType[n.type || ''] || 'Operation',
    })) as import('../../api/types').ChainElement[];

    const connections = edges.map(e => ({
      id: e.id,
      from_element: e.source,
      to_element: e.target,
      from_port: e.sourceHandle ? parseInt(e.sourceHandle, 10) || 0 : 0,
      to_port: 0,
      condition: null,
    })) as import('../../api/types').ChainConnection[];

    const positions = computeLayout(elements, connections);

    setNodes(nds => nds.map(n => {
      const pos = positions.get(n.id);
      return pos ? { ...n, position: pos } : n;
    }));

    setTimeout(() => fitView({ padding: 0.2, maxZoom: 1.5 }), 50);
  }, [nodes, edges, setNodes, fitView]);

  //
  // Duplicate: prompt for new name/description, then create as new chain.
  //
  const [showDuplicateModal, setShowDuplicateModal] = useState(false);
  const [duplicateValues, setDuplicateValues] = useState<Record<string, string | boolean>>({ name: '', description: '' });

  const handleDuplicateClick = () => {
    setDuplicateValues({ name: `${name} (copy)`, description });
    setShowDuplicateModal(true);
  };

  const handleDuplicateConfirm = () => {
    const dupName = (duplicateValues.name as string).trim();
    if (!dupName || !onDuplicate) return;
    const definition = flowToChain(nodes, edges, dupName, duplicateValues.description as string, category, timeout, extraData);
    onDuplicate(definition);
    setShowDuplicateModal(false);
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
              const newMemoryConfigs = new Map(prev.memoryConfigs);
              const newLoopMaxIters = new Map(prev.loopMaxIterations);
              newSessionGroups.delete(hoveredNodeId);
              newBlockConfigs.delete(hoveredNodeId);
              newTransformPrompts.delete(hoveredNodeId);
              newTransformModels.delete(hoveredNodeId);
              newGenericPrompts.delete(hoveredNodeId);
              newMemoryConfigs.delete(hoveredNodeId);
              newLoopMaxIters.delete(hoveredNodeId);
              return {
                ...prev,
                sessionGroups: newSessionGroups,
                blockConfigs: newBlockConfigs,
                transformPrompts: newTransformPrompts,
                transformModels: newTransformModels,
                genericPrompts: newGenericPrompts,
                memoryConfigs: newMemoryConfigs,
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
  // Only available on edges originating from agent-mode operation nodes.
  //
  const onEdgeDoubleClick = useCallback((_: React.MouseEvent, edge: Edge) => {
    //
    // Check if source node is an agent-mode operation.
    //
    const sourceNode = nodes.find(n => n.id === edge.source);
    const isAgentOp = sourceNode?.type === 'operation'
      && (sourceNode.data as unknown as OperationNodeData)?.mode === 'agent';
    if (!isAgentOp) return;

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
  }, [setEdges, nodes]);

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
    } else if (node.type === 'memory') {
      setEditingNodeId(node.id);
      const cfg = extraData.memoryConfigs.get(node.id);
      setMemoryKey(cfg?.key || '');
      setMemoryMode(cfg?.mode || 'Store');
      setShowMemoryModal(true);
    } else if (node.type === 'loop') {
      setEditingNodeId(node.id);
      setLoopMaxIterations(extraData.loopMaxIterations.get(node.id) || 3);
      setShowLoopModal(true);
    } else if (node.type === 'tool') {
      setEditingNodeId(node.id);
      const cfg = extraData.toolConfigs.get(node.id);
      setToolModalToolName(cfg?.tool_name || '');
      setToolModalParams({ ...(cfg?.tool_params || {}) });
      setShowToolModal(true);
    } else if (node.type === 'payload') {
      setEditingNodeId(node.id);
      setPayloadModalSelectedId(extraData.payloadConfigs.get(node.id) || null);
      setPayloadEditName('');
      setPayloadEditContent('');
      setPayloadEditId(null);
      setShowPayloadForm(false);
      send({ type: 'payload_list' });
      setShowPayloadModal(true);
    }
  }, [extraData, send]);

  return (
    <div className="flex flex-col h-full">
      {/*
      //
      // Header.
      //
      */}
      <div className="flex items-center justify-between px-3 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)]">
        <div className="flex items-center gap-2">
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Chain name *"
            className={`bg-[var(--bg-primary)] border px-2.5 py-1 text-xs text-highlight w-48 focus:outline-none transition-colors ${
              name.trim() ? 'border-dim focus:border-subtle' : 'border-[var(--accent-error)]'
            }`}
          />
          <input
            type="text"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Description"
            className="bg-[var(--bg-primary)] border border-dim px-2.5 py-1 text-xs text-highlight flex-1 min-w-[450px] focus:outline-none focus:border-subtle transition-colors"
          />
          <div className="flex items-center gap-1">
            <label className="text-[10px] tracking-wider text-[var(--text-secondary)]">Timeout:</label>
            <input
              type="number"
              value={timeout}
              onChange={(e) => setChainTimeout(parseInt(e.target.value) || 1800)}
              min={1}
              className="bg-[var(--bg-primary)] border border-dim px-1.5 py-1 text-xs text-highlight w-16 text-center focus:outline-none focus:border-subtle transition-colors"
            />
            <span className="text-[10px]" style={{ color: 'var(--text-muted)' }}>s</span>
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <button
            onClick={onCancel}
            className="flex items-center gap-1.5 px-3 py-1 text-[10px] tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
          >
            <X size={11} />
            Close
          </button>
          {onDuplicate && (
            <button
              onClick={handleDuplicateClick}
              disabled={!canSave}
              className="inline-flex items-center gap-1.5 px-3 py-1 text-[10px] tracking-wider border border-dim text-muted hover:border-subtle hover:bg-[var(--highlight)] transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              title="Duplicate as new chain"
            >
              <Copy size={11} />
              Duplicate
            </button>
          )}
          {onExport && (
            <button
              onClick={() => {
                const definition = flowToChain(nodes, edges, name.trim(), description, category, timeout, extraData);
                onExport(definition);
              }}
              disabled={!canSave}
              className="inline-flex items-center gap-1.5 px-3 py-1 text-[10px] tracking-wider border border-dim text-muted hover:border-[var(--accent-purple)] hover:text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              title="Export as JSON"
            >
              <Download size={11} />
              Export
            </button>
          )}
          <button
            onClick={handleSave}
            disabled={!canSave || saveFlash === 'saving'}
            className={`inline-flex items-center gap-1.5 px-3 py-1 text-[10px] tracking-wider border transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
              saveFlash === 'saved'
                ? 'border-[var(--accent-success)] bg-[var(--accent-success)]/20 text-[var(--accent-success)]'
                : saveFlash === 'error'
                ? 'border-[var(--accent-error)] bg-[var(--accent-error)]/20 text-[var(--accent-error)]'
                : saveFlash === 'saving'
                ? 'border-[var(--accent-warning)] bg-[var(--accent-warning)]/20 text-[var(--accent-warning)]'
                : 'border-dim bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:border-[var(--accent-info)] hover:bg-[var(--accent-info)]/30'
            }`}
            title={!canSave ? 'Chain name is required' : undefined}
          >
            {saveFlash === 'saved' ? <Check size={11} /> : saveFlash === 'error' ? <AlertTriangle size={11} /> : <Save size={11} />}
            {saveFlash === 'saved' ? 'Saved' : saveFlash === 'error' ? 'Error' : saveFlash === 'saving' ? 'Saving...' : 'Save'}
          </button>
        </div>
      </div>

      {/*
      //
      // Flow Canvas.
      //
      */}
      <div className="flex-1 min-h-0 relative" ref={reactFlowWrapper}>
        {saveValidationError && (
          <div className="absolute top-2 left-1/2 -translate-x-1/2 z-50 px-3 py-1.5 text-[10px] bg-[var(--accent-error)]/20 border border-[var(--accent-error)] text-[var(--accent-error)]">
            {saveValidationError}
          </div>
        )}
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
          // Bottom-right controls: auto-layout + fit view.
          //
          */}
          <Panel position="bottom-right" className="!m-2">
            <div className="flex gap-1">
              <button
                onClick={handleAutoLayout}
                className="p-1.5 bg-[var(--bg-secondary)] border border-subtle rounded hover:bg-[var(--bg-tertiary)] transition-colors"
                title="Auto-layout"
              >
                <LayoutGrid size={14} className="text-[var(--text-secondary)]" />
              </button>
              <button
                onClick={() => fitView({ padding: 0.2, maxZoom: 1.5 })}
                className="p-1.5 bg-[var(--bg-secondary)] border border-subtle rounded hover:bg-[var(--bg-tertiary)] transition-colors"
                title="Fit to view"
              >
                <Maximize2 size={14} className="text-[var(--text-secondary)]" />
              </button>
            </div>
          </Panel>

          {/*
          //
          // Element Palette.
          //
          */}
          <Panel position="top-left" className="!m-2" style={{ maxHeight: 'calc(100% - 40px)' }}>
            <div
              className="bg-[var(--bg-secondary)] border border-[var(--border-color)] p-2 overflow-y-auto"
              style={{ maxHeight: 'calc(100%)', borderRadius: 2, boxShadow: '3px 3px 0 0 rgba(0,0,0,0.4)' }}
            >
              <div className="text-[10px] tracking-widest text-[var(--text-secondary)] mb-1.5 px-1" style={{ letterSpacing: '0.1em' }}>ELEMENTS</div>
              <div className="grid grid-cols-2 gap-x-1 gap-y-0">
                <PaletteItem
                  type="trigger"
                  icon={<Play size={16} className={hasTrigger ? "text-[var(--text-secondary)]" : "text-[var(--accent-success)]"} />}
                  label="Trigger"
                  disabled={hasTrigger}
                  onClick={() => handleQuickAdd('trigger')}
                />
                <PaletteItem
                  type="termination"
                  icon={<Square size={16} className={hasTermination ? "text-[var(--text-secondary)]" : "text-[var(--accent-error)]"} />}
                  label="End"
                  disabled={hasTermination}
                  onClick={() => handleQuickAdd('termination')}
                />
                <PaletteItem
                  type="operation"
                  icon={<Cpu size={16} className="text-[var(--accent-info)]" />}
                  label="Operation"
                  onClick={() => handleQuickAdd('operation')}
                />
                <PaletteItem
                  type="transform"
                  icon={<Sparkles size={16} className="text-[var(--accent-warning)]" />}
                  label="Transform"
                  onClick={() => handleQuickAdd('transform')}
                />
                <PaletteItem
                  type="genericPrompt"
                  icon={<MessageSquare size={16} className="text-[var(--accent-purple)]" />}
                  label="Prompt"
                  onClick={() => handleQuickAdd('genericPrompt')}
                />
                <PaletteItem
                  type="memory"
                  icon={<Database size={16} className="text-[var(--accent-success)]" />}
                  label="Memory"
                  onClick={() => handleQuickAdd('memory')}
                />
                <PaletteItem
                  type="loop"
                  icon={<RefreshCw size={16} className="text-[var(--accent-warning)]" />}
                  label="Loop"
                  onClick={() => handleQuickAdd('loop')}
                />
                <PaletteItem
                  type="tool"
                  icon={<Wrench size={16} className="text-[var(--accent-info)]" />}
                  label="Tool"
                  disabled={toolkitTools.length === 0}
                  onClick={() => handleQuickAdd('tool')}
                />
                <PaletteItem
                  type="payload"
                  icon={<FileText size={16} className="text-[var(--accent-warning)]" />}
                  label="Payload"
                  onClick={() => handleQuickAdd('payload')}
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
          // Ungroup / Edit Session Panel - show when selected nodes have
          // session groups.
          //
          */}
          {groupableSelectedNodes.length > 0 && groupableSelectedNodes.some(n => extraData.sessionGroups.has(n.id)) && (
            <Panel position="top-center" className="!m-2 !mt-14">
              <div className="ascii-box bg-[var(--bg-secondary)] p-2.5 flex items-center gap-2">
                {selectedSessionGroup && (
                  <button
                    onClick={() => {
                      setEditingSessionGroupId(selectedSessionGroup.id);
                      setSessionGroupYolo(selectedSessionGroup.yolo_mode);
                      setSessionGroupWorkingDir(selectedSessionGroup.working_dir || '');
                      setShowSessionGroupModal(true);
                    }}
                    className="flex items-center gap-2 px-3 py-1.5 text-xs tracking-wider border border-dim bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:border-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors"
                    title="Edit session group settings"
                  >
                    <Settings size={12} />
                    Edit Session
                  </button>
                )}
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
      // Trigger Panel (only for saved chains).
      //
      */}
      {chain?.id && (
        <ChainTriggerPanel chainId={chain.id} />
      )}

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
        title="Configure Memory"
        config={[
          {
            type: 'section',
            fields: [
              {
                name: 'memoryMode',
                label: 'Mode',
                type: 'select' as const,
                span: 'full' as const,
                options: [
                  { value: 'Store', label: 'Store' },
                  { value: 'Retrieve', label: 'Retrieve' },
                ],
              },
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
        values={{ memoryKey, memoryMode }}
        onChange={(name, value) => {
          if (name === 'memoryKey') setMemoryKey(value);
          if (name === 'memoryMode') setMemoryMode(value as 'Store' | 'Retrieve');
        }}
        onSubmit={handleMemoryConfirm}
        submitLabel={editingNodeId ? 'Update' : 'Add'}
        submitIcon={<Database size={14} />}
        submitVariant={memoryMode === 'Store' ? 'success' : 'info'}
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

      <ConfigModal
        isOpen={showToolModal}
        onClose={() => {
          setShowToolModal(false);
          setPendingPosition(null);
          setEditingNodeId(null);
          setToolModalToolName('');
          setToolModalParams({});
        }}
        size="sm"
        title="Configure Tool"
        config={(() => {
          const selectedTool = toolkitTools.find(t => t.tool_name === toolModalToolName);
          const items: Array<{ type: 'section'; fields: Array<{ name: string; label: string; type: 'text' | 'textarea' | 'select' | 'number'; span?: 'full' | 'half'; options?: Array<{ value: string; label: string }>; placeholder?: string }> }> = [];

          items.push({
            type: 'section',
            fields: [{
              name: '_tool_select',
              label: 'Tool',
              type: 'select' as const,
              span: 'full' as const,
              options: toolkitTools.map(t => ({ value: t.tool_name, label: t.display_name })),
            }],
          });

          if (selectedTool && selectedTool.config_schema.length > 0) {
            items.push({
              type: 'section',
              fields: selectedTool.config_schema.map(field => ({
                name: field.name,
                label: field.label,
                type: (field.field_type === 'select' ? 'select' : field.field_type === 'textarea' ? 'textarea' : field.field_type === 'number' ? 'number' : 'text') as 'text' | 'textarea' | 'select' | 'number',
                span: 'full' as const,
                options: field.options?.map(o => ({ value: o.value, label: o.label })) || undefined,
                placeholder: field.default_value || undefined,
              })),
            });
          }

          return items;
        })()}
        values={{
          _tool_select: toolModalToolName,
          ...Object.fromEntries(Object.entries(toolModalParams).map(([k, v]) => [k, String(v ?? '')])),
        }}
        onChange={(name, value) => {
          if (name === '_tool_select') {
            setToolModalToolName(value);
            const newTool = toolkitTools.find(t => t.tool_name === value);
            if (newTool) {
              const defaults: Record<string, unknown> = {};
              for (const field of newTool.config_schema) {
                if (field.default_value != null) defaults[field.name] = field.default_value;
              }
              setToolModalParams(defaults);
            } else {
              setToolModalParams({});
            }
          } else {
            setToolModalParams(prev => ({ ...prev, [name]: value }));
          }
        }}
        onSubmit={handleToolConfirm}
        submitLabel={editingNodeId ? 'Update' : 'Add'}
        submitIcon={<Wrench size={14} />}
        submitVariant="info"
        submitDisabled={!toolModalToolName}
      />

      <Modal
        isOpen={showPayloadModal}
        onClose={() => { setShowPayloadModal(false); setEditingNodeId(null); }}
        title="Payload"
        size="lg"
      >
        <div className="space-y-4">
          <div className="max-h-48 overflow-y-auto border border-[var(--border-color)] rounded">
            {payloads.length === 0 ? (
              <div className="p-3 text-xs text-muted text-center">No payloads yet. Create one below.</div>
            ) : payloads.map(p => (
              <div
                key={p.id}
                className={`flex items-center justify-between px-3 py-2 cursor-pointer border-b border-[var(--border-color)] last:border-b-0 hover:bg-[var(--bg-tertiary)] transition-colors ${payloadModalSelectedId === p.id ? 'bg-[var(--accent-warning)]/10' : ''}`}
                onClick={() => setPayloadModalSelectedId(p.id)}
              >
                <div className="min-w-0">
                  <div className="text-xs font-mono text-highlight">{p.shortname}</div>
                  <div className="text-xs text-muted truncate">{p.content.substring(0, 60)}{p.content.length > 60 ? '...' : ''}</div>
                </div>
                <div className="flex items-center gap-1 shrink-0 ml-2">
                  <button
                    className="p-1 text-muted hover:text-highlight"
                    title="Edit"
                    onClick={(e) => { e.stopPropagation(); setPayloadEditId(p.id); setPayloadEditName(p.shortname); setPayloadEditContent(p.content); setShowPayloadForm(true); }}
                  >✎</button>
                  <button
                    className="p-1 text-muted hover:text-[var(--accent-error)]"
                    title="Delete"
                    onClick={(e) => { e.stopPropagation(); send({ type: 'payload_delete', id: p.id }); if (payloadModalSelectedId === p.id) setPayloadModalSelectedId(null); }}
                  >✕</button>
                </div>
              </div>
            ))}
          </div>

          {showPayloadForm ? (
            <div className="border border-[var(--border-color)] rounded p-3 space-y-2">
              <div className="text-xs text-muted font-medium">{payloadEditId ? 'Edit Payload' : 'New Payload'}</div>
              <input
                type="text"
                value={payloadEditName}
                onChange={(e) => setPayloadEditName(e.target.value)}
                placeholder="Shortname (one word)"
                className="w-full bg-[var(--bg-primary)] text-xs px-2 py-1.5 border border-[var(--border-color)] font-mono focus:outline-none focus:border-[var(--accent-warning)]"
              />
              <textarea
                value={payloadEditContent}
                onChange={(e) => setPayloadEditContent(e.target.value)}
                placeholder="Payload content (markdown)"
                rows={10}
                className="w-full bg-[var(--bg-primary)] text-xs px-2 py-1.5 border border-[var(--border-color)] font-mono focus:outline-none focus:border-[var(--accent-warning)] resize-y"
              />
              <div className="flex gap-2">
                <button
                  className="px-3 py-1.5 text-xs border border-[var(--accent-warning)] text-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/10 disabled:opacity-50"
                  disabled={!payloadEditName.trim() || !payloadEditContent.trim()}
                  onClick={() => {
                    send({ type: 'payload_upsert', id: payloadEditId || undefined, shortname: payloadEditName.trim(), content: payloadEditContent });
                    setPayloadEditId(null);
                    setPayloadEditName('');
                    setPayloadEditContent('');
                    setShowPayloadForm(false);
                  }}
                >
                  {payloadEditId ? 'Update' : 'Save Payload'}
                </button>
                <button
                  className="px-3 py-1.5 text-xs border border-dim text-muted hover:text-highlight"
                  onClick={() => { setPayloadEditId(null); setPayloadEditName(''); setPayloadEditContent(''); setShowPayloadForm(false); }}
                >
                  Cancel
                </button>
              </div>
            </div>
          ) : (
            <button
              className="px-3 py-1.5 text-xs border border-dim text-muted hover:text-highlight hover:border-[var(--accent-warning)]"
              onClick={() => { setPayloadEditId(null); setPayloadEditName(''); setPayloadEditContent(''); setShowPayloadForm(true); }}
            >
              + New Payload
            </button>
          )}

          <div className="flex justify-end">
            <button
              className="inline-flex items-center gap-2 px-4 py-2 text-xs tracking-wider border border-dim transition-colors disabled:opacity-50 bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:border-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30"
              disabled={!payloadModalSelectedId}
              onClick={handlePayloadConfirm}
            >
              <FileText size={14} />
              {editingNodeId ? 'Update' : 'Add'}
            </button>
          </div>
        </div>
      </Modal>

      <ConfigModal
        isOpen={showDuplicateModal}
        onClose={() => setShowDuplicateModal(false)}
        onSubmit={handleDuplicateConfirm}
        title="Duplicate Chain"
        submitLabel="Create"
        submitIcon={<Copy size={14} />}
        size="sm"
        values={duplicateValues}
        onChange={(key, val) => setDuplicateValues(prev => ({ ...prev, [key]: val }))}
        config={[
          {
            type: 'section' as const,
            fields: [
              { name: 'name', label: 'Name', type: 'text' as const, required: true, span: 'full' as const },
              { name: 'description', label: 'Description', type: 'text' as const, span: 'full' as const },
            ],
          },
        ]}
      />

      {/*
      //
      // Session Group Configuration Modal.
      //
      */}
      <ConfigModal
        isOpen={showSessionGroupModal}
        onClose={() => {
          setShowSessionGroupModal(false);
          setEditingSessionGroupId(null);
        }}
        title={editingSessionGroupId ? 'Edit Session Group' : 'Configure Session Group'}
        size="sm"
        config={[
          {
            type: 'section',
            fields: [
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
                help: 'Auto-approve agent actions without prompting.',
              },
            ],
          },
        ]}
        values={{
          workingDir: sessionGroupWorkingDir,
          yoloMode: sessionGroupYolo,
        }}
        onChange={(name, value) => {
          if (name === 'workingDir') setSessionGroupWorkingDir(value);
          if (name === 'yoloMode') setSessionGroupYolo(!!value);
        }}
        onSubmit={handleSessionGroupConfirm}
        submitLabel={editingSessionGroupId ? 'Update' : 'Create'}
        submitIcon={<Users size={14} />}
        submitVariant="purple"
      />
    </div>
  );
}

interface ChainBuilderProps {
  chain?: ChainDefinitionFull | null;
  onSave: (definition: ChainDefinitionInput, onResult?: (result: 'saved' | 'error') => void) => void;
  onDuplicate?: (definition: ChainDefinitionInput) => void;
  onExport?: (definition: ChainDefinitionInput) => void;
  onCancel: () => void;
  operationDefs: OperationDefinitionInfo[];
  modelDefs?: ModelDefinition[];
  nodes?: NodeState[];
  toolkitTools?: ToolkitToolInfo[];
  payloads?: PayloadInfo[];
  send?: (msg: BrowserMessage) => void;
  saveStatus?: string | null;
  saveError?: string | null;
}

const noopSend = () => {};
export function ChainBuilder({ modelDefs = [], nodes = [], toolkitTools = [], payloads = [], send = noopSend, saveStatus, saveError, ...props }: ChainBuilderProps) {
  return (
    <ReactFlowProvider>
      <ChainBuilderInner {...props} modelDefs={modelDefs} nodes={nodes} toolkitTools={toolkitTools} payloads={payloads} send={send} saveStatus={saveStatus} saveError={saveError} />
    </ReactFlowProvider>
  );
}
