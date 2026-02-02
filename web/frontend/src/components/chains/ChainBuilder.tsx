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
import { Play, Zap, X, Save, CircleStop, FileOutput, Cpu, Maximize2, GitMerge, Sparkles, MessageSquare, Users } from 'lucide-react';
import { Modal } from '../common/Modal';
import { ConfigModal } from '../common/ConfigModal';
import type {
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
function TriggerNode({ data, selected }: { data: { label: string }; selected?: boolean }) {
  return (
    <div
      className={`ascii-box bg-[var(--bg-secondary)] px-4 py-2 min-w-[120px] relative transition-all ${!selected ? hoverStyle : ''}`}
      style={selected ? selectedStyle : undefined}
    >
      <Handle
        type="source"
        position={Position.Right}
        style={handleStyle}
      />
      <div className="flex items-center gap-2">
        <Play size={14} className="text-[var(--accent-success)]" />
        <span className="text-sm font-mono">{data.label}</span>
      </div>
    </div>
  );
}

function OperationNode({ data, selected }: { data: { label: string; operation: string; sessionColor?: string }; selected?: boolean }) {
  const baseStyle = data.sessionColor
    ? { borderLeft: `4px solid ${data.sessionColor}` }
    : {};
  const style = selected ? { ...baseStyle, ...selectedStyle } : baseStyle;
  return (
    <div
      className={`ascii-box bg-[var(--bg-secondary)] px-4 py-2 min-w-[150px] relative transition-all ${!selected ? hoverStyle : ''}`}
      style={style}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Right} style={handleStyle} />
      <div className="flex items-center gap-2">
        <Cpu size={14} className="text-[var(--accent-info)]" />
        <div className="flex flex-col">
          <span className="text-sm font-mono">{data.label}</span>
          <span className="text-xs text-[var(--text-secondary)]">{data.operation}</span>
        </div>
      </div>
    </div>
  );
}

function TransformNode({ data, selected }: { data: { label: string; prompt: string; sessionColor?: string }; selected?: boolean }) {
  const baseStyle = data.sessionColor
    ? { borderLeft: `4px solid ${data.sessionColor}` }
    : {};
  const style = selected ? { ...baseStyle, ...selectedStyle } : baseStyle;
  return (
    <div
      className={`ascii-box bg-[var(--bg-secondary)] px-4 py-2 min-w-[150px] relative transition-all ${!selected ? hoverStyle : ''}`}
      style={style}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Right} style={handleStyle} />
      <div className="flex items-center gap-2">
        <Sparkles size={14} className="text-[var(--accent-warning)]" />
        <div className="flex flex-col">
          <span className="text-sm font-mono">{data.label}</span>
          <span className="text-xs text-[var(--text-secondary)] truncate max-w-[150px]" title={data.prompt}>
            {data.prompt.length > 30 ? data.prompt.substring(0, 30) + '...' : data.prompt}
          </span>
        </div>
      </div>
    </div>
  );
}

function GenericPromptNode({ data, selected }: { data: { label: string; prompt: string; sessionColor?: string }; selected?: boolean }) {
  const baseStyle = data.sessionColor
    ? { borderLeft: `4px solid ${data.sessionColor}` }
    : {};
  const style = selected ? { ...baseStyle, ...selectedStyle } : baseStyle;
  return (
    <div
      className={`ascii-box bg-[var(--bg-secondary)] px-4 py-2 min-w-[150px] relative transition-all ${!selected ? hoverStyle : ''}`}
      style={style}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <Handle type="source" position={Position.Right} style={handleStyle} />
      <div className="flex items-center gap-2">
        <MessageSquare size={14} className="text-[var(--accent-purple)]" />
        <div className="flex flex-col">
          <span className="text-sm font-mono">{data.label}</span>
          <span className="text-xs text-[var(--text-secondary)] truncate max-w-[150px]" title={data.prompt}>
            {data.prompt.length > 30 ? data.prompt.substring(0, 30) + '...' : data.prompt}
          </span>
        </div>
      </div>
    </div>
  );
}

function TerminationNode({ data, selected }: { data: { label: string; termType: string }; selected?: boolean }) {
  return (
    <div
      className={`ascii-box bg-[var(--bg-secondary)] px-4 py-2 min-w-[120px] relative transition-all ${!selected ? hoverStyle : ''}`}
      style={selected ? selectedStyle : undefined}
    >
      <Handle type="target" position={Position.Left} style={handleStyle} />
      <div className="flex items-center gap-2">
        <CircleStop size={14} className="text-[var(--accent-error)]" />
        <div className="flex flex-col">
          <span className="text-sm font-mono">{data.label}</span>
          <span className="text-xs text-[var(--text-secondary)]">{data.termType}</span>
        </div>
      </div>
    </div>
  );
}

const nodeTypes: NodeTypes = {
  trigger: TriggerNode,
  operation: OperationNode,
  transform: TransformNode,
  genericPrompt: GenericPromptNode,
  termination: TerminationNode,
};

//
// Extra data tracked separately (prompts, models, session groups).
//
interface ChainExtraData {
  terminationPrompts: Map<string, string>;
  terminationModels: Map<string, string>;
  transformPrompts: Map<string, string>;
  transformModels: Map<string, string>;
  genericPrompts: Map<string, string>;
  sessionGroups: Map<string, SessionGroup>;
}

//
// Convert chain definition to React Flow nodes and edges (positions computed
// via dagre).
//
function chainToFlow(chain: ChainDefinitionFull | null): { nodes: Node[]; edges: Edge[]; extraData: ChainExtraData } {
  const emptyExtraData: ChainExtraData = {
    terminationPrompts: new Map(),
    terminationModels: new Map(),
    transformPrompts: new Map(),
    transformModels: new Map(),
    genericPrompts: new Map(),
    sessionGroups: new Map(),
  };

  if (!chain) return { nodes: [], edges: [], extraData: emptyExtraData };

  //
  // Compute positions using dagre layout.
  //
  const positions = computeLayout(chain.elements, chain.connections);

  const extraData = { ...emptyExtraData };

  const nodes: Node[] = chain.elements.map((elem) => {
    const position = positions.get(elem.id) || { x: 0, y: 0 };

    switch (elem.element_type) {
      case 'Trigger':
        return {
          id: elem.id,
          type: 'trigger',
          position,
          data: { label: 'Manual Trigger' },
        };
      case 'Operation':
        if (elem.session_group) {
          extraData.sessionGroups.set(elem.id, elem.session_group);
        }
        return {
          id: elem.id,
          type: 'operation',
          position,
          data: {
            label: 'Operation',
            operation: elem.operation_name,
            sessionColor: elem.session_group?.color,
          },
        };
      case 'Transform':
        extraData.transformPrompts.set(elem.id, elem.prompt);
        if (elem.model_ref) {
          extraData.transformModels.set(elem.id, elem.model_ref);
        }
        if (elem.session_group) {
          extraData.sessionGroups.set(elem.id, elem.session_group);
        }
        return {
          id: elem.id,
          type: 'transform',
          position,
          data: {
            label: 'Transform',
            prompt: elem.prompt,
            sessionColor: elem.session_group?.color,
          },
        };
      case 'GenericPrompt':
        extraData.genericPrompts.set(elem.id, elem.prompt);
        if (elem.session_group) {
          extraData.sessionGroups.set(elem.id, elem.session_group);
        }
        return {
          id: elem.id,
          type: 'genericPrompt',
          position,
          data: {
            label: 'Prompt',
            prompt: elem.prompt,
            sessionColor: elem.session_group?.color,
          },
        };
      case 'Termination':
        //
        // Extract prompt and model_ref from Semantic terminations.
        //
        if (elem.termination_type.type === 'Semantic' && 'prompt' in elem.termination_type) {
          extraData.terminationPrompts.set(elem.id, elem.termination_type.prompt);
          if (elem.termination_type.model_ref) {
            extraData.terminationModels.set(elem.id, elem.termination_type.model_ref);
          }
        }
        return {
          id: elem.id,
          type: 'termination',
          position,
          data: {
            label: elem.label,
            termType: elem.termination_type.type,
          },
        };
    }
  });

  const edges: Edge[] = chain.connections.map((conn) => ({
    id: conn.id,
    source: conn.from_element,
    target: conn.to_element,
    markerEnd: { type: MarkerType.ArrowClosed },
    style: { stroke: 'var(--text-secondary)' },
  }));

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
  const elements: ChainElement[] = nodes.map((node) => {
    //
    // Note: We don't store positions - dagre computes them on load.
    //
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
        };
      case 'transform':
        return {
          element_type: 'Transform' as const,
          id: node.id,
          prompt: extraData.transformPrompts.get(node.id) || '',
          model_ref: extraData.transformModels.get(node.id) || null,
          session_group: extraData.sessionGroups.get(node.id) || null,
        };
      case 'genericPrompt':
        return {
          element_type: 'GenericPrompt' as const,
          id: node.id,
          prompt: extraData.genericPrompts.get(node.id) || '',
          session_group: extraData.sessionGroups.get(node.id) || null,
        };
      case 'termination':
        const prompt = extraData.terminationPrompts.get(node.id) || '';
        const modelRef = extraData.terminationModels.get(node.id) || null;
        return {
          element_type: 'Termination' as const,
          id: node.id,
          termination_type: node.data?.termType === 'Raw'
            ? { type: 'Raw' as const }
            : { type: 'Semantic' as const, prompt, model_ref: modelRef },
          label: (node.data?.label as string) || 'Output',
        };
      default:
        throw new Error(`Unknown node type: ${node.type}`);
    }
  });

  const connections: ChainConnectionType[] = edges.map((edge) => ({
    id: edge.id,
    from_element: edge.source,
    to_element: edge.target,
    from_port: 0,
    to_port: 0,
  }));

  return {
    name,
    description,
    category,
    elements,
    connections,
    disabled: false,
    timeout,
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
  const [timeout, setTimeout] = useState(chain?.timeout || 300);
  const category = 'default';

  const initialFlow = chainToFlow(chain || null);
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
  // Modal state for termination configuration.
  //
  const [showTerminationModal, setShowTerminationModal] = useState(false);
  const [terminationType, setTerminationType] = useState<'Raw' | 'Semantic'>('Raw');
  const [terminationPrompt, setTerminationPrompt] = useState('');
  const [terminationModel, setTerminationModel] = useState<string>('');

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

  const reactFlowWrapper = useRef<HTMLDivElement>(null);
  const { screenToFlowPosition, fitView } = useReactFlow();

  //
  // Check if trigger/termination already exists.
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
      markerEnd: { type: MarkerType.ArrowClosed },
      style: { stroke: 'var(--text-secondary)' },
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

      //
      // Prevent adding second termination.
      //
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
        setShowTransformModal(true);
        return;
      }

      //
      // For generic prompt, show the configuration modal.
      //
      if (type === 'genericPrompt') {
        setPendingPosition(position);
        setGenericPromptText('');
        setShowGenericPromptModal(true);
        return;
      }

      //
      // For termination, show the configuration modal.
      //
      if (type === 'termination') {
        setPendingPosition(position);
        setTerminationType('Raw');
        setTerminationPrompt('');
        setTerminationModel('');
        setShowTerminationModal(true);
        return;
      }

      //
      // For other types, create directly.
      //
      addNodeAtPosition(type, position);
    },
    [screenToFlowPosition, hasTrigger, hasTermination]
  );

  const addNodeAtPosition = useCallback((type: string, position: { x: number; y: number }, nodeExtraData?: Record<string, unknown>) => {
    //
    // Prevent adding second trigger.
    //
    if (type === 'trigger' && hasTrigger) {
      return;
    }

    //
    // Prevent adding second termination.
    //
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
      case 'operation':
        newNode = {
          id: newId,
          type: 'operation',
          position,
          data: { label: 'Operation', operation: nodeExtraData?.operation || '' },
        };
        break;
      case 'transform':
        newNode = {
          id: newId,
          type: 'transform',
          position,
          data: {
            label: 'Transform',
            prompt: nodeExtraData?.prompt || '',
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
      case 'termination':
        newNode = {
          id: newId,
          type: 'termination',
          position,
          data: {
            label: nodeExtraData?.label || 'Output',
            termType: nodeExtraData?.termType || 'Raw'
          },
        };
        //
        // Store prompt and model if Semantic.
        //
        if (nodeExtraData?.termType === 'Semantic') {
          setExtraData(prev => {
            const newTermPrompts = new Map(prev.terminationPrompts);
            const newTermModels = new Map(prev.terminationModels);
            if (nodeExtraData?.prompt) {
              newTermPrompts.set(newId, nodeExtraData.prompt as string);
            }
            if (nodeExtraData?.modelRef) {
              newTermModels.set(newId, nodeExtraData.modelRef as string);
            }
            return { ...prev, terminationPrompts: newTermPrompts, terminationModels: newTermModels };
          });
        }
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
    // Prevent adding second trigger.
    //
    if (type === 'trigger' && hasTrigger) {
      return;
    }

    //
    // Prevent adding second termination.
    //
    if (type === 'termination' && hasTermination) {
      return;
    }

    const position = { x: 100 + nodes.length * 30, y: 100 + nodes.length * 30 };

    if (type === 'operation') {
      setPendingPosition(position);
      setShowOperationModal(true);
      return;
    }

    if (type === 'transform') {
      setPendingPosition(position);
      setTransformPrompt('');
      setTransformModel('');
      setShowTransformModal(true);
      return;
    }

    if (type === 'genericPrompt') {
      setPendingPosition(position);
      setGenericPromptText('');
      setShowGenericPromptModal(true);
      return;
    }

    if (type === 'termination') {
      setPendingPosition(position);
      setTerminationType('Raw');
      setTerminationPrompt('');
      setTerminationModel('');
      setShowTerminationModal(true);
      return;
    }

    addNodeAtPosition(type, position);
  }, [nodes.length, addNodeAtPosition, hasTrigger, hasTermination]);

  const handleOperationSelect = useCallback(() => {
    if (pendingPosition && selectedOperation) {
      addNodeAtPosition('operation', pendingPosition, { operation: selectedOperation });
      setShowOperationModal(false);
      setPendingPosition(null);
      setSelectedOperation('');
    }
  }, [pendingPosition, selectedOperation, addNodeAtPosition]);

  const handleTransformConfirm = useCallback(() => {
    if (transformPrompt.trim()) {
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
            ? { ...n, data: { ...n.data, prompt: transformPrompt } }
            : n
        ));
      } else if (pendingPosition) {
        //
        // Add new node.
        //
        addNodeAtPosition('transform', pendingPosition, {
          prompt: transformPrompt,
          modelRef: transformModel || undefined,
        });
      }
      setShowTransformModal(false);
      setPendingPosition(null);
      setEditingNodeId(null);
      setTransformPrompt('');
      setTransformModel('');
    }
  }, [pendingPosition, editingNodeId, transformPrompt, transformModel, addNodeAtPosition, setNodes]);

  const handleGenericPromptConfirm = useCallback(() => {
    if (genericPromptText.trim()) {
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
            ? { ...n, data: { ...n.data, prompt: genericPromptText } }
            : n
        ));
      } else if (pendingPosition) {
        //
        // Add new node.
        //
        addNodeAtPosition('genericPrompt', pendingPosition, {
          prompt: genericPromptText,
        });
      }
      setShowGenericPromptModal(false);
      setPendingPosition(null);
      setEditingNodeId(null);
      setGenericPromptText('');
    }
  }, [pendingPosition, editingNodeId, genericPromptText, addNodeAtPosition, setNodes]);

  const handleTerminationConfirm = useCallback(() => {
    if (editingNodeId) {
      //
      // Update existing node.
      //
      setExtraData(prev => {
        const newTermPrompts = new Map(prev.terminationPrompts);
        const newTermModels = new Map(prev.terminationModels);
        if (terminationType === 'Semantic') {
          newTermPrompts.set(editingNodeId, terminationPrompt);
          if (terminationModel) {
            newTermModels.set(editingNodeId, terminationModel);
          } else {
            newTermModels.delete(editingNodeId);
          }
        } else {
          newTermPrompts.delete(editingNodeId);
          newTermModels.delete(editingNodeId);
        }
        return { ...prev, terminationPrompts: newTermPrompts, terminationModels: newTermModels };
      });
      setNodes(nds => nds.map(n =>
        n.id === editingNodeId
          ? {
              ...n,
              data: {
                ...n.data,
                label: terminationType === 'Raw' ? 'Raw Output' : 'Semantic Output',
                termType: terminationType,
              },
            }
          : n
      ));
      setShowTerminationModal(false);
      setEditingNodeId(null);
      setTerminationPrompt('');
      setTerminationModel('');
    } else if (pendingPosition) {
      //
      // Add new node.
      //
      addNodeAtPosition('termination', pendingPosition, {
        label: terminationType === 'Raw' ? 'Raw Output' : 'Semantic Output',
        termType: terminationType,
        prompt: terminationPrompt,
        modelRef: terminationModel || undefined,
      });
      setShowTerminationModal(false);
      setPendingPosition(null);
      setTerminationPrompt('');
      setTerminationModel('');
    }
  }, [pendingPosition, editingNodeId, terminationType, terminationPrompt, terminationModel, addNodeAtPosition, setNodes]);

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
              const newTermPrompts = new Map(prev.terminationPrompts);
              const newTermModels = new Map(prev.terminationModels);
              const newTransformPrompts = new Map(prev.transformPrompts);
              const newTransformModels = new Map(prev.transformModels);
              const newGenericPrompts = new Map(prev.genericPrompts);
              newSessionGroups.delete(hoveredNodeId);
              newTermPrompts.delete(hoveredNodeId);
              newTermModels.delete(hoveredNodeId);
              newTransformPrompts.delete(hoveredNodeId);
              newTransformModels.delete(hoveredNodeId);
              newGenericPrompts.delete(hoveredNodeId);
              return {
                ...prev,
                sessionGroups: newSessionGroups,
                terminationPrompts: newTermPrompts,
                terminationModels: newTermModels,
                transformPrompts: newTransformPrompts,
                transformModels: newTransformModels,
                genericPrompts: newGenericPrompts,
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
  const initialFitDone = useRef(false);
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

  const onEdgeMouseLeave = useCallback((_: React.MouseEvent, edge: Edge) => {
    setHoveredEdgeId(null);
    //
    // Reset edge style.
    //
    setEdges(eds => eds.map(e =>
      e.id === edge.id
        ? { ...e, style: { ...e.style, stroke: 'var(--text-secondary)', strokeWidth: 2 } }
        : e
    ));
  }, [setEdges]);

  //
  // Handle node click for selection.
  //
  const onNodeClick = useCallback((event: React.MouseEvent, node: Node) => {
    event.stopPropagation();

    //
    // Check if Ctrl/Meta is held for multi-select.
    //
    const isMultiSelect = event.ctrlKey || event.metaKey;

    setNodes(nds =>
      nds.map(n => {
        if (n.id === node.id) {
          //
          // Clicked node: select it (or toggle if multi-select and already
          // selected).
          //
          return { ...n, selected: isMultiSelect ? !n.selected : true };
        }
        //
        // Other nodes: keep selection if multi-select, clear if single select.
        //
        return isMultiSelect ? n : { ...n, selected: false };
      })
    );
  }, [setNodes]);

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
    if (node.type === 'transform') {
      setEditingNodeId(node.id);
      setTransformPrompt(extraData.transformPrompts.get(node.id) || '');
      setTransformModel(extraData.transformModels.get(node.id) || '');
      setShowTransformModal(true);
    } else if (node.type === 'termination') {
      setEditingNodeId(node.id);
      const termType = (node.data?.termType as string) || 'Raw';
      setTerminationType(termType as 'Raw' | 'Semantic');
      setTerminationPrompt(extraData.terminationPrompts.get(node.id) || '');
      setTerminationModel(extraData.terminationModels.get(node.id) || '');
      setShowTerminationModal(true);
    } else if (node.type === 'genericPrompt') {
      setEditingNodeId(node.id);
      setGenericPromptText(extraData.genericPrompts.get(node.id) || '');
      setShowGenericPromptModal(true);
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
              onChange={(e) => setTimeout(parseInt(e.target.value) || 300)}
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
      <div className="flex-1" ref={reactFlowWrapper}>
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
          onPaneClick={onPaneClick}
          onSelectionChange={onSelectionChange}
          nodeTypes={nodeTypes}
          fitView
          fitViewOptions={{ padding: 0.5, maxZoom: 1 }}
          minZoom={0.2}
          maxZoom={2}
          defaultViewport={{ x: 0, y: 0, zoom: 0.8 }}
          deleteKeyCode={['Delete', 'Backspace']}
          connectionLineStyle={{ stroke: 'var(--accent-info)', strokeWidth: 2 }}
          defaultEdgeOptions={{
            style: { stroke: 'var(--text-secondary)', strokeWidth: 2 },
            markerEnd: { type: MarkerType.ArrowClosed },
          }}
          snapToGrid
          snapGrid={[10, 10]}
          selectionMode={SelectionMode.Partial}
          selectionOnDrag
          selectionKeyCode={['Control', 'Meta']}
          multiSelectionKeyCode={null}
          panOnDrag
          panOnScroll={false}
          elementsSelectable={false}
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
          <Panel position="top-left" className="!m-2">
            <div className="ascii-box bg-[var(--bg-secondary)] p-3 flex flex-col gap-0.5">
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
                  type="termination"
                  icon={<CircleStop size={20} className={hasTermination ? "text-[var(--text-secondary)]" : "text-[var(--accent-error)]"} />}
                  label="Output"
                  disabled={hasTermination}
                  onClick={() => handleQuickAdd('termination')}
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
              Drag from handles to connect • Ctrl+Click to multi-select • Delete to remove
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
        }}
        title="Select Operation"
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
        ]}
        values={{ operation: selectedOperation }}
        onChange={(_name, value) => setSelectedOperation(value)}
        onSubmit={handleOperationSelect}
        submitLabel="Add"
        submitIcon={<Cpu size={14} />}
        submitVariant="info"
        submitDisabled={!selectedOperation}
      />

      {/*
      //
      // Output Configuration Modal.
      //
      */}
      <Modal
        isOpen={showTerminationModal}
        onClose={() => {
          setShowTerminationModal(false);
          setPendingPosition(null);
          setTerminationPrompt('');
          setTerminationModel('');
        }}
        title="Configure Output"
        size="md"
      >
        <div className="space-y-0">
          {/*
          //
          // Type selector section.
          //
          */}
          <div className="p-2.5 bg-[var(--bg-secondary)]">
            <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Type</label>
            <div className="flex gap-2">
              <button
                onClick={() => setTerminationType('Raw')}
                className={`flex-1 flex items-center justify-center gap-2 px-3 py-2.5 text-sm border transition-colors ${
                  terminationType === 'Raw'
                    ? 'bg-[var(--accent-success)]/20 text-[var(--accent-success)] border-[var(--accent-success)]'
                    : 'bg-[var(--bg-primary)] border-dim hover:border-subtle'
                }`}
              >
                <FileOutput size={14} />
                Raw
              </button>
              <button
                onClick={() => setTerminationType('Semantic')}
                className={`flex-1 flex items-center justify-center gap-2 px-3 py-2.5 text-sm border transition-colors ${
                  terminationType === 'Semantic'
                    ? 'bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] border-[var(--accent-purple)]'
                    : 'bg-[var(--bg-primary)] border-dim hover:border-subtle'
                }`}
              >
                <Zap size={14} />
                Semantic
              </button>
            </div>
            <p className="text-xs mt-2 leading-relaxed" style={{ color: 'var(--text-muted)' }}>
              {terminationType === 'Raw'
                ? 'Raw outputs the accumulated data directly without processing'
                : 'Semantic processes the data with an LLM using the prompt below'}
            </p>
          </div>

          {/*
          //
          // Prompt and Model fields for Semantic type.
          //
          */}
          {terminationType === 'Semantic' && (
            <>
              <div className="p-2.5 bg-[var(--bg-secondary)]">
                <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Model</label>
                <select
                  value={terminationModel}
                  onChange={(e) => setTerminationModel(e.target.value)}
                  className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                >
                  <option value="">Use default model</option>
                  {modelDefs.map((m) => (
                    <option key={m.name} value={m.name}>
                      {m.name}
                    </option>
                  ))}
                </select>
                <p className="text-xs mt-2 leading-relaxed" style={{ color: 'var(--text-muted)' }}>
                  {modelDefs.length === 0
                    ? 'No models configured. Configure models in Settings.'
                    : 'Select a model or use the default semantic operations model.'}
                </p>
              </div>
              <div className="p-2.5 bg-[var(--bg-secondary)]">
                <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">
                  Prompt<span className="text-[var(--accent-error)]/70"> *</span>
                </label>
                <textarea
                  value={terminationPrompt}
                  onChange={(e) => setTerminationPrompt(e.target.value)}
                  placeholder="Enter the prompt for processing the accumulated data..."
                  className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight font-mono min-h-[100px] resize-none focus:outline-none focus:border-subtle transition-colors"
                />
              </div>
            </>
          )}

          {/*
          //
          // Actions.
          //
          */}
          <div className="p-2.5 bg-[var(--bg-secondary)]">
            <div className="flex justify-end gap-2">
              <button
                onClick={() => {
                  setShowTerminationModal(false);
                  setPendingPosition(null);
                  setTerminationPrompt('');
                  setTerminationModel('');
                }}
                className="px-4 py-2 text-xs tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleTerminationConfirm}
                disabled={terminationType === 'Semantic' && !terminationPrompt.trim()}
                className="inline-flex items-center gap-2 px-4 py-2 text-xs tracking-wider border border-dim bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:border-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <CircleStop size={14} />
                Add
              </button>
            </div>
          </div>
        </div>
      </Modal>

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
          setTransformPrompt('');
          setTransformModel('');
        }}
        title="Configure Transform"
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
        ]}
        values={{
          model: transformModel,
          prompt: transformPrompt,
        }}
        onChange={(name, value) => {
          if (name === 'model') setTransformModel(value);
          if (name === 'prompt') setTransformPrompt(value);
        }}
        onSubmit={handleTransformConfirm}
        submitLabel="Add"
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
          setGenericPromptText('');
        }}
        title="Configure Prompt"
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
        ]}
        values={{ prompt: genericPromptText }}
        onChange={(_name, value) => setGenericPromptText(value)}
        onSubmit={handleGenericPromptConfirm}
        submitLabel="Add"
        submitIcon={<MessageSquare size={14} />}
        submitVariant="purple"
        submitDisabled={!genericPromptText.trim()}
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
