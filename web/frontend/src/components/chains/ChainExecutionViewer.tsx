import { useState, useMemo, useEffect, useCallback, useRef } from 'react';
import {
  ReactFlow,
  Panel,
  Background,
  BackgroundVariant,
  MarkerType,
  ReactFlowProvider,
  useReactFlow,
  useNodesState,
  useEdgesState,
} from '@xyflow/react';
import type { Node, Edge } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { Play, Clock, CheckCircle2, XCircle, AlertCircle, Loader2, Maximize2, Cpu, Sparkles, MessageSquare, ExternalLink, ChevronDown, ChevronRight, Database, RefreshCw, Square, Wrench, FileText } from 'lucide-react';
import type {
  ChainExecutionUpdate,
  ChainDefinitionFull,
  ElementExecution,
  OperationDefinitionInfo,
  PayloadInfo,
} from '../../api/types';
import { StyledOutput } from '../common/StyledOutput';
import { computeLayout } from '../../utils/dagreLayout';
import { nodeTypes } from './ChainNodes';

//
// Status colors and icons (used by step list and detail panel).
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
// Convert chain definition to React Flow nodes with execution status. Uses
// stored positions from chain definition (same as builder) and populates full
// node data so the shared node components render identically.
//
function chainToFlowWithStatus(
  chain: ChainDefinitionFull | null,
  elements: Record<string, ElementExecution>,
  operationDefs?: OperationDefinitionInfo[],
  payloads?: PayloadInfo[],
): { nodes: Node[]; edges: Edge[] } {
  if (!chain) return { nodes: [], edges: [] };

  //
  // Always use dagre auto-layout for the execution viewer so chains
  // render cleanly regardless of stored positions.
  //
  const positions = computeLayout(chain.elements, chain.connections);

  const nodes: Node[] = chain.elements.map((elem) => {
    const execStatus = elements[elem.id]?.status;
    const status = typeof execStatus === 'object'
      ? (Object.keys(execStatus)[0] as string)
      : execStatus;
    const position = positions.get(elem.id) || { x: 0, y: 0 };

    switch (elem.element_type) {
      case 'Trigger':
        return {
          id: elem.id,
          type: 'trigger',
          position,
          data: { label: 'Manual Trigger', status },
        };
      case 'Operation': {
        const opDef = operationDefs?.find(d => d.full_name === elem.operation_name);
        return {
          id: elem.id,
          type: 'operation',
          position,
          data: {
            label: 'Operation',
            operation: elem.operation_name || 'Operation',
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
            status,
          },
        };
      }
      case 'Transform':
        return {
          id: elem.id,
          type: 'transform',
          position,
          data: {
            label: 'Transform',
            prompt: elem.prompt || '',
            sessionColor: elem.session_group?.color,
            modelRef: elem.model_ref,
            maxRuntime: elem.block_config?.max_runtime,
            yoloMode: elem.block_config?.yolo_mode,
            workingDir: elem.block_config?.working_dir,
            requireAllInputs: elem.block_config?.require_all_inputs,
            status,
          },
        };
      case 'GenericPrompt':
        return {
          id: elem.id,
          type: 'genericPrompt',
          position,
          data: {
            label: 'Prompt',
            prompt: elem.prompt || '',
            sessionColor: elem.session_group?.color,
            maxRuntime: elem.block_config?.max_runtime,
            yoloMode: elem.block_config?.yolo_mode,
            workingDir: elem.block_config?.working_dir,
            requireAllInputs: elem.block_config?.require_all_inputs,
            status,
          },
        };
      case 'Memory':
        return {
          id: elem.id,
          type: 'memory',
          position,
          data: { label: 'Memory', memoryKey: elem.key, memoryMode: elem.mode, status },
        };
      case 'Loop':
        return {
          id: elem.id,
          type: 'loop',
          position,
          data: { label: 'Loop', maxIterations: elem.max_iterations, status },
        };
      case 'Tool':
        return {
          id: elem.id,
          type: 'tool',
          position,
          data: { label: 'Tool', toolName: elem.tool_name, status },
        };
      case 'Payload': {
        const plInfo = (payloads || []).find(p => p.id === elem.payload_id);
        return {
          id: elem.id,
          type: 'payload',
          position,
          data: { label: 'Payload', shortname: plInfo?.shortname || elem.payload_id.slice(0, 8), status },
        };
      }
      case 'Termination':
        return {
          id: elem.id,
          type: 'termination',
          position,
          data: {
            label: 'End',
            requireAllInputs: elem.block_config?.require_all_inputs,
            status,
          },
        };
    }
  }).filter((n): n is NonNullable<typeof n> => n != null);

  const edges: Edge[] = chain.connections.map((conn) => {
    let stroke = 'var(--text-secondary)';
    let label: string | undefined;

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
      style: { stroke, strokeWidth: 2 },
      label,
      labelStyle: label ? { fill: stroke, fontSize: 10, fontWeight: 500 } : undefined,
    };
  });

  return { nodes, edges };
}

const EMPTY_PAYLOADS: PayloadInfo[] = [];

interface ChainExecutionViewerInnerProps {
  execution: ChainExecutionUpdate;
  chain: ChainDefinitionFull | null;
  isLoading?: boolean;
  onEditChain?: (chainId: string) => void;
  operationDefs?: OperationDefinitionInfo[];
  payloads?: PayloadInfo[];
}

function ChainExecutionViewerInner({ execution, chain, isLoading, onEditChain, operationDefs, payloads = EMPTY_PAYLOADS }: ChainExecutionViewerInnerProps) {
  const [selectedElementId, setSelectedElementId] = useState<string | null>(null);
  const [outputExpanded, setOutputExpanded] = useState(true);
  const [graphExpanded, setGraphExpanded] = useState(true);
  const { fitView, setCenter, getNodes } = useReactFlow();

  //
  // Cache the chain definition so nodes don't disappear if the parent's
  // chain prop goes null (e.g. currentChain changes elsewhere).
  //
  const chainRef = useRef(chain);
  if (chain) chainRef.current = chain;
  const stableChain = chainRef.current;

  //
  // Use JSON.stringify for deep comparison since React's shallow comparison
  // may not detect changes in the elements object when updates arrive.
  //
  const elementsKey = JSON.stringify(execution.elements);
  const computedFlow = useMemo(
    () => chainToFlowWithStatus(stableChain, execution.elements, operationDefs, payloads),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [stableChain, elementsKey, operationDefs, payloads]
  );

  //
  // Use React Flow's state hooks to ensure proper node rendering. Sync
  // from computed flow whenever it changes.
  //
  const [nodes, setNodes, onNodesChange] = useNodesState(computedFlow.nodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(computedFlow.edges);
  useEffect(() => {
    setNodes(computedFlow.nodes);
    setEdges(computedFlow.edges);
  }, [computedFlow, setNodes, setEdges]);

  //
  // Auto-fit view on initial load only.
  //
  const initialFitDone = useRef(false);
  useEffect(() => {
    if (computedFlow.nodes.length > 0 && !initialFitDone.current) {
      initialFitDone.current = true;
      const timer = setTimeout(() => {
        fitView({ padding: 0.05, maxZoom: 1.5 });
      }, 50);
      return () => clearTimeout(timer);
    }
  }, [computedFlow.nodes.length, fitView]);

  //
  // Auto-zoom to the currently running element (only when it changes).
  //
  const lastRunningIdRef = useRef<string | null>(null);
  useEffect(() => {
    if (execution.status !== 'Running') return;

    const runningId = Object.entries(execution.elements).find(
      ([, elem]) => elem.status === 'Running'
    )?.[0];
    if (!runningId || runningId === lastRunningIdRef.current) return;
    lastRunningIdRef.current = runningId;

    //
    // Read node positions from React Flow's internal state to ensure
    // they're laid out.
    //
    const timer = setTimeout(() => {
      const flowNodes = getNodes();
      const target = flowNodes.find(n => n.id === runningId);
      if (!target) return;
      setCenter(
        target.position.x + (target.measured?.width ?? 200) / 2,
        target.position.y + (target.measured?.height ?? 60) / 2,
        { zoom: 0.7, duration: 400 }
      );
    }, 200);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [elementsKey, execution.status, setCenter, getNodes]);

  //
  // Get selected element's execution info.
  //
  const selectedElement = selectedElementId ? execution.elements[selectedElementId] : null;
  const selectedOutput = useMemo(() => {
    if (!selectedElement) return null;
    const status = selectedElement.status;
    if (typeof status === 'object') {
      if ('Completed' in status) return status.Completed.output;
      if ('Failed' in status) return status.Failed.error;
    }
    return null;
  }, [selectedElement]);

  //
  // Get outputs from terminal elements.
  //
  const outputs = execution.outputs;

  //
  // Auto-collapse graph when execution completes with output.
  //
  useEffect(() => {
    if (execution.status === 'Completed' && Object.keys(outputs).length > 0) {
      setGraphExpanded(false);
    }
  }, [execution.status, outputs]);

  const handleNodeClick = (_: React.MouseEvent, node: Node) => {
    setSelectedElementId(node.id);
  };

  //
  // Get step name from chain definition.
  //
  const getStepName = (elementId: string): { name: string; type: string } => {
    if (!chain) return { name: elementId.slice(0, 8), type: 'unknown' };

    const element = chain.elements.find(e => e.id === elementId);
    if (!element) return { name: elementId.slice(0, 8), type: 'unknown' };

    switch (element.element_type) {
      case 'Trigger':
        return { name: 'Trigger', type: 'trigger' };
      case 'Operation':
        return { name: element.operation_name || 'Operation', type: 'operation' };
      case 'Transform':
        return { name: 'Transform', type: 'transform' };
      case 'GenericPrompt':
        return { name: 'Prompt', type: 'genericPrompt' };
      case 'Memory':
        return {
          name: `${element.mode === 'Store' ? 'Store' : 'Load'}: ${element.key}`,
          type: 'memory',
        };
      case 'Loop':
        return { name: `Loop (max ${element.max_iterations})`, type: 'loop' };
      case 'Tool':
        return { name: 'Tool', type: 'tool' };
      case 'Payload':
        return { name: 'Payload', type: 'payload' };
      case 'Termination':
        return { name: 'End', type: 'termination' };
      default:
        return { name: elementId.slice(0, 8), type: 'unknown' };
    }
  };

  //
  // Sort elements by following connections from trigger.
  //
  const sortedElements = useMemo(() => {
    if (!chain) {
      //
      // Fallback: just return elements, no specific order.
      //
      return Object.entries(execution.elements);
    }

    //
    // Build connection graph: from_element -> [to_elements].
    //
    const connectionMap = new Map<string, string[]>();
    for (const conn of chain.connections) {
      const existing = connectionMap.get(conn.from_element) || [];
      existing.push(conn.to_element);
      connectionMap.set(conn.from_element, existing);
    }

    //
    // Find trigger element (starting point).
    //
    const trigger = chain.elements.find(e => e.element_type === 'Trigger');
    if (!trigger) {
      return Object.entries(execution.elements);
    }

    //
    // Walk connections from trigger to build execution order.
    //
    const visited = new Set<string>();
    const order: string[] = [];
    const queue = [trigger.id];

    while (queue.length > 0) {
      const current = queue.shift()!;
      if (visited.has(current)) continue;
      visited.add(current);
      order.push(current);

      const nextElements = connectionMap.get(current) || [];
      for (const next of nextElements) {
        if (!visited.has(next)) {
          queue.push(next);
        }
      }
    }

    //
    // Build order map.
    //
    const orderMap = new Map<string, number>();
    order.forEach((id, index) => {
      orderMap.set(id, index);
    });

    return Object.entries(execution.elements)
      .sort(([idA], [idB]) => {
        const orderA = orderMap.get(idA) ?? 999;
        const orderB = orderMap.get(idB) ?? 999;
        return orderA - orderB;
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [elementsKey, chain]);

  //
  // Keyboard navigation for execution steps.
  //
  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (sortedElements.length === 0) return;

    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();

      const currentIndex = selectedElementId
        ? sortedElements.findIndex(([id]) => id === selectedElementId)
        : -1;

      let newIndex: number;
      if (e.key === 'ArrowDown') {
        newIndex = currentIndex < sortedElements.length - 1 ? currentIndex + 1 : 0;
      } else {
        newIndex = currentIndex > 0 ? currentIndex - 1 : sortedElements.length - 1;
      }

      setSelectedElementId(sortedElements[newIndex][0]);
    }
  }, [sortedElements, selectedElementId]);

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  return (
    <div className="flex flex-col">
      {/*
      //
      // Execution info header.
      //
      */}
      <div className="flex items-center justify-between px-3 py-1.5 border-b border-subtle bg-[var(--bg-secondary)]">
        <div className="flex items-baseline gap-3 text-[10px] whitespace-nowrap">
          <div className="flex items-baseline min-w-0">
            <span className="text-muted">Chain:</span>
            <span className="ml-2 font-mono truncate max-w-[220px]">{execution.chain_name}</span>
            {onEditChain && chain && (
              <button
                onClick={() => onEditChain(chain.id)}
                className="ml-2 text-[var(--accent-info)] hover:text-[var(--accent-info)]/80 transition-colors self-center"
                title="Edit chain definition"
              >
                <ExternalLink size={12} />
              </button>
            )}
          </div>
          <div className="flex items-baseline">
            <span className="text-muted">Status:</span>
            <span className={`ml-2 font-mono ${
              execution.status === 'Completed' ? 'text-[var(--text-highlight)]' :
              execution.status === 'Failed' ? 'text-[var(--accent-error)]' :
              execution.status === 'Cancelled' ? 'text-[var(--accent-warning)]' :
              'text-[var(--text-secondary)]'
            }`}>{execution.status}</span>
          </div>
          <div className="flex items-baseline">
            <span className="text-muted">Started:</span>
            <span className="ml-2">{new Date(execution.started_at).toLocaleString()}</span>
          </div>
          {execution.ended_at && (
            <div className="flex items-baseline">
              <span className="text-muted">Ended:</span>
              <span className="ml-2">{new Date(execution.ended_at).toLocaleString()}</span>
            </div>
          )}
        </div>
      </div>

      {/*
      //
      // Final Output - collapsible section shown when chain completed.
      //
      */}
      {execution.status === 'Completed' && Object.keys(outputs).length > 0 && (
        <div className="border-b border-subtle">
          <button
            onClick={() => setOutputExpanded(!outputExpanded)}
            className="w-full px-3 py-1.5 flex items-center gap-2 hover:bg-[var(--bg-tertiary)] transition-colors text-left"
          >
            {outputExpanded ? (
              <ChevronDown size={12} className="text-[var(--text-secondary)]" />
            ) : (
              <ChevronRight size={12} className="text-[var(--text-secondary)]" />
            )}
            <CheckCircle2 size={12} className="text-[var(--accent-success)]" />
            <span className="text-xs font-medium text-[var(--text-highlight)]">Final Output</span>
            <span className="text-xs text-muted ml-auto">
              {Object.keys(outputs).length} output{Object.keys(outputs).length !== 1 ? 's' : ''}
            </span>
          </button>
          {outputExpanded && (
            <div className="px-3 pb-3 space-y-2">
              {Object.entries(outputs).map(([label, output]) => (
                <div key={label} className="p-2 bg-[var(--bg-secondary)] rounded text-xs max-h-64 overflow-auto border border-subtle text-[var(--text-secondary)]">
                  <StyledOutput output={output} />
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/*
      //
      // Collapsible flow graph.
      //
      */}
      <div className="border-b border-subtle">
        <button
          onClick={() => setGraphExpanded(!graphExpanded)}
          className="w-full px-3 py-1.5 flex items-center gap-2 hover:bg-[var(--bg-tertiary)] transition-colors text-left"
        >
          {graphExpanded ? (
            <ChevronDown size={12} className="text-[var(--text-secondary)]" />
          ) : (
            <ChevronRight size={12} className="text-[var(--text-secondary)]" />
          )}
          <span className="text-xs font-medium text-[var(--text-secondary)]">Graph</span>
        </button>
        {graphExpanded && <div className="h-40 min-h-[10rem]">
        {stableChain ? (
          <ReactFlow
            nodes={nodes}
            edges={edges}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            nodeTypes={nodeTypes}
            onNodeClick={handleNodeClick}
            fitView
            fitViewOptions={{ padding: 0.05, maxZoom: 1.5 }}
            minZoom={0.2}
            maxZoom={2}
            nodesDraggable={false}
            nodesConnectable={false}
            elementsSelectable={true}
            proOptions={{ hideAttribution: true }}
          >
            <Background variant={BackgroundVariant.Dots} gap={20} size={1} color="var(--text-secondary)" />
            <Panel position="bottom-right">
              <button
                onClick={() => fitView({ padding: 0.05, maxZoom: 1.5 })}
                className="p-1.5 bg-[var(--bg-secondary)] border border-subtle rounded hover:bg-[var(--bg-tertiary)] transition-colors"
                title="Fit to view"
              >
                <Maximize2 size={14} className="text-[var(--text-secondary)]" />
              </button>
            </Panel>
          </ReactFlow>
        ) : (
          <div className="h-full flex items-center justify-center text-muted text-sm gap-2">
            {isLoading ? (
              <>
                <Loader2 size={16} className="animate-spin" />
                <span>Loading chain definition...</span>
              </>
            ) : (
              <span>Chain definition not available</span>
            )}
          </div>
        )}
      </div>}
      </div>

      {/*
      //
      // Element details below - horizontal layout.
      //
      */}
      <div className="flex min-h-[300px]">
        {/*
        //
        // Steps list.
        //
        */}
        <div className="w-64 border-r border-subtle bg-[var(--bg-secondary)]">
          <div className="p-3">
            <h4 className="text-sm font-medium text-muted mb-3">Execution Steps</h4>
            <div className="space-y-1">
              {sortedElements.map(([id, elem]) => {
                const status = typeof elem.status === 'object'
                  ? Object.keys(elem.status)[0]
                  : elem.status;
                const statusInfo = getStatusIndicator(status);
                const StatusIcon = statusInfo.icon;
                const stepInfo = getStepName(id);

                return (
                  <div
                    key={id}
                    className={`p-2 rounded cursor-pointer hover:bg-[var(--bg-tertiary)] transition-colors ${
                      selectedElementId === id ? 'bg-[var(--bg-tertiary)] ring-1 ring-[var(--accent-info)]' : ''
                    }`}
                    onClick={() => setSelectedElementId(id)}
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2 min-w-0">
                        {stepInfo.type === 'trigger' && <Play size={12} className="text-[var(--accent-success)] flex-shrink-0" />}
                        {stepInfo.type === 'operation' && <Cpu size={12} className="text-[var(--accent-info)] flex-shrink-0" />}
                        {stepInfo.type === 'transform' && <Sparkles size={12} className="text-[var(--accent-warning)] flex-shrink-0" />}
                        {stepInfo.type === 'genericPrompt' && <MessageSquare size={12} className="text-[var(--accent-purple)] flex-shrink-0" />}
                        {stepInfo.type === 'memory' && <Database size={12} className="text-[var(--accent-success)] flex-shrink-0" />}
                        {stepInfo.type === 'loop' && <RefreshCw size={12} className="text-[var(--accent-warning)] flex-shrink-0" />}
                        {stepInfo.type === 'tool' && <Wrench size={12} className="text-[var(--accent-info)] flex-shrink-0" />}
                        {stepInfo.type === 'payload' && <FileText size={12} className="text-[var(--accent-warning)] flex-shrink-0" />}
                        {stepInfo.type === 'termination' && <Square size={12} className="text-[var(--accent-error)] flex-shrink-0" />}
                        {stepInfo.type === 'unknown' && <Clock size={12} className="text-[var(--text-secondary)] flex-shrink-0" />}
                        <span className="text-xs font-mono truncate">{stepInfo.name}</span>
                      </div>
                      <StatusIcon
                        size={12}
                        style={{ color: statusInfo.color }}
                        className={`flex-shrink-0 ${statusInfo.animate ? 'animate-spin' : ''}`}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        </div>

        {/*
        //
        // Element details panel.
        //
        */}
        <div className="flex-1 p-3">
          {selectedElementId && selectedElement ? (
            <div className="space-y-3">
              {(() => {
                const stepInfo = getStepName(selectedElementId);
                return (
                  <div className="flex items-baseline gap-2">
                    {stepInfo.type === 'trigger' && <Play size={14} className="text-[var(--accent-success)] self-center" />}
                    {stepInfo.type === 'operation' && <Cpu size={14} className="text-[var(--accent-info)] self-center" />}
                    {stepInfo.type === 'transform' && <Sparkles size={14} className="text-[var(--accent-warning)] self-center" />}
                    {stepInfo.type === 'genericPrompt' && <MessageSquare size={14} className="text-[var(--accent-purple)] self-center" />}
                    {stepInfo.type === 'memory' && <Database size={14} className="text-[var(--accent-success)] self-center" />}
                    {stepInfo.type === 'loop' && <RefreshCw size={14} className="text-[var(--accent-warning)] self-center" />}
                    {stepInfo.type === 'tool' && <Wrench size={14} className="text-[var(--accent-info)] self-center" />}
                    {stepInfo.type === 'payload' && <FileText size={14} className="text-[var(--accent-warning)] self-center" />}
                    {stepInfo.type === 'termination' && <Square size={14} className="text-[var(--accent-error)] self-center" />}
                    <span className="text-sm font-medium text-[var(--text-highlight)]">
                      {stepInfo.name}
                      {stepInfo.type === 'payload' && selectedElement?.config && selectedElement.config.type === 'Payload' && (() => {
                        const cfg = selectedElement.config as { type: 'Payload'; payload_id: string };
                        const plRec = payloads.find(p => p.id === cfg.payload_id);
                        return plRec ? `: ${plRec.shortname}` : '';
                      })()}
                    </span>
                    <span className="text-[10px] text-[var(--text-secondary)] font-mono">{selectedElementId.slice(0, 8)}</span>
                  </div>
                );
              })()}

              <div className="flex gap-4 text-[10px]">
                <div>
                  <span className="text-muted">Status:</span>{' '}
                  <span className="font-mono">
                    {typeof selectedElement.status === 'object'
                      ? Object.keys(selectedElement.status)[0]
                      : selectedElement.status}
                  </span>
                </div>
                {selectedElement.started_at && (
                  <div>
                    <span className="text-muted">Started:</span>{' '}
                    <span>{new Date(selectedElement.started_at).toLocaleString()}</span>
                  </div>
                )}
                {selectedElement.completed_at && (
                  <div>
                    <span className="text-muted">Completed:</span>{' '}
                    <span>{new Date(selectedElement.completed_at).toLocaleString()}</span>
                  </div>
                )}
              </div>

              {/*
              //
              // Element Configuration.
              //
              */}
              {selectedElement.config && (
                <div>
                  <span className="text-[10px] text-muted font-medium">Configuration:</span>
                  <div className="mt-1 p-2 bg-[var(--bg-secondary)] rounded text-[10px]">
                    {selectedElement.config.type === 'Trigger' && (
                      <span className="text-muted">Trigger (Manual)</span>
                    )}
                    {selectedElement.config.type === 'Operation' && (
                      <div className="space-y-1">
                        <div><span className="text-muted">Operation:</span> <span className="font-mono">{selectedElement.config.operation_name}</span></div>
                        {selectedElement.config.model_ref && (
                          <div><span className="text-muted">Model:</span> <span className="font-mono text-[var(--accent-info)]">{selectedElement.config.model_ref}</span></div>
                        )}
                      </div>
                    )}
                    {selectedElement.config.type === 'Transform' && (
                      <div className="space-y-2">
                        {selectedElement.config.model_ref && (
                          <div><span className="text-muted">Model:</span> <span className="font-mono text-[var(--accent-info)]">{selectedElement.config.model_ref}</span></div>
                        )}
                        <div>
                          <span className="text-muted">Prompt:</span>
                          <pre className="mt-1 text-[var(--text-secondary)] whitespace-pre-wrap font-mono">{selectedElement.config.prompt}</pre>
                        </div>
                      </div>
                    )}
                    {selectedElement.config.type === 'GenericPrompt' && (
                      <div>
                        <span className="text-muted">Prompt:</span>
                        <pre className="mt-1 text-[var(--text-secondary)] whitespace-pre-wrap font-mono">{selectedElement.config.prompt}</pre>
                      </div>
                    )}
                    {selectedElement.config.type === 'Memory' && (
                      <div className="space-y-1">
                        <div><span className="text-muted">Mode:</span> <span className="font-mono">{selectedElement.config.mode}</span></div>
                        <div><span className="text-muted">Key:</span> <span className={`font-mono ${selectedElement.config.mode === 'Store' ? 'text-[var(--accent-success)]' : 'text-[var(--accent-info)]'}`}>{selectedElement.config.key}</span></div>
                      </div>
                    )}
                    {selectedElement.config.type === 'Loop' && (
                      <div><span className="text-muted">Max Iterations:</span> <span className="font-mono text-[var(--accent-warning)]">{selectedElement.config.max_iterations}</span></div>
                    )}
                    {selectedElement.config.type === 'Tool' && (
                      <div className="space-y-1">
                        <div><span className="text-muted">Tool:</span> <span className="font-mono text-[var(--accent-info)]">{selectedElement.config.tool_name}</span></div>
                        {Object.entries(selectedElement.config.tool_params).filter(([k]) => k !== 'input').map(([k, v]) => (
                          <div key={k}><span className="text-muted">{k}:</span> <span className="font-mono">{String(v)}</span></div>
                        ))}
                      </div>
                    )}
                    {selectedElement.config.type === 'Payload' && (() => {
                      const cfg = selectedElement.config as { type: 'Payload'; payload_id: string };
                      const plRec = payloads.find(p => p.id === cfg.payload_id);
                      return (
                        <div><span className="text-muted">Payload:</span> <span className="font-mono text-[var(--accent-warning)]">{plRec ? plRec.shortname : cfg.payload_id.slice(0, 8)}</span></div>
                      );
                    })()}
                    {selectedElement.config.type === 'Termination' && (
                      <span className="text-muted">Termination (End)</span>
                    )}
                  </div>
                </div>
              )}

              {/*
              //
              // Element Output/Error - shown first for Operations.
              //
              */}
              {selectedOutput && (
                <div>
                  <span className="text-[10px] text-muted font-medium">
                    {typeof selectedElement.status === 'object' && 'Failed' in selectedElement.status
                      ? 'Error:'
                      : 'Output:'}
                  </span>
                  <div className="mt-1 p-2 bg-[var(--bg-secondary)] rounded text-[10px] max-h-64 overflow-auto text-[var(--text-secondary)]">
                    <StyledOutput output={selectedOutput} />
                  </div>
                </div>
              )}

              {/*
              //
              // Element Context/Input - shown after output.
              //
              */}
              {selectedElement.context && selectedElement.context.input && (
                <div>
                  <span className="text-[10px] text-muted font-medium">Input Data:</span>
                  <div className="mt-1 p-2 bg-[var(--bg-secondary)] rounded text-[10px] max-h-48 overflow-auto">
                    <pre className="whitespace-pre-wrap font-mono text-[10px] text-[var(--text-secondary)]">{selectedElement.context.input}</pre>
                  </div>
                </div>
              )}

              {/*
              //
              // Session ID - shown for any element with a session.
              //
              */}
              {selectedElement.context?.session_id && (
                <div className="text-[10px] text-muted">
                  Session: <span className="font-mono">{selectedElement.context.session_id.slice(0, 8)}</span>
                </div>
              )}
            </div>
          ) : (
            <div className="h-full flex items-center justify-center text-muted text-sm">
              Select a step to see details
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

interface ChainExecutionViewerProps {
  execution: ChainExecutionUpdate;
  chain: ChainDefinitionFull | null;
  isLoading?: boolean;
  onEditChain?: (chainId: string) => void;
  operationDefs?: OperationDefinitionInfo[];
  payloads?: PayloadInfo[];
}

export function ChainExecutionViewer(props: ChainExecutionViewerProps) {
  return (
    <ReactFlowProvider>
      <ChainExecutionViewerInner {...props} />
    </ReactFlowProvider>
  );
}
