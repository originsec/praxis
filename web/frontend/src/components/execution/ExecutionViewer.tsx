import { useState, useMemo, useEffect, useRef } from 'react';
import {
  ReactFlow,
  Background,
  BackgroundVariant,
  MarkerType,
  ReactFlowProvider,
  useReactFlow,
  useNodesState,
  useEdgesState,
  Panel,
} from '@xyflow/react';
import type { Node, Edge } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import {
  Maximize2,
  Square,
  ArrowLeft,
} from 'lucide-react';
import type {
  ChainExecutionUpdate,
  ChainDefinitionFull,
  ChainExecutionEvent,
  OperationDefinitionInfo,
  PayloadInfo,
} from '../../api/types';
import { computeLayout } from '../../utils/dagreLayout';
import { nodeTypes } from '../chains/ChainNodes';
import { ExecutionEventTimeline } from './ExecutionEventTimeline';

//
// Convert chain definition to React Flow nodes with execution status overlays.
// Uses the shared node components from ChainNodes.tsx so rendering matches
// the builder and the ChainExecutionViewer.
//
function chainToFlowWithStatus(
  chain: ChainDefinitionFull | null,
  execution: ChainExecutionUpdate,
  operationDefs?: OperationDefinitionInfo[],
  payloads?: PayloadInfo[],
): { nodes: Node[]; edges: Edge[] } {
  if (!chain) return { nodes: [], edges: [] };

  const positions = computeLayout(chain.elements, chain.connections);

  const nodes: Node[] = chain.elements.map((elem) => {
    const execElem = execution.elements[elem.id];
    const execStatus = execElem?.status;
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
          data: { label: 'Tool', toolName: elem.tool_name, maxRuntime: elem.block_config?.max_runtime, status },
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

interface ExecutionViewerInnerProps {
  execution: ChainExecutionUpdate;
  chain: ChainDefinitionFull | null;
  executionEvents: ChainExecutionEvent[];
  operationDefs?: OperationDefinitionInfo[];
  payloads?: PayloadInfo[];
  onCancel?: () => void;
  onBack?: () => void;
}

function ExecutionViewerInner({ execution, chain, executionEvents, operationDefs, payloads, onCancel, onBack }: ExecutionViewerInnerProps) {
  const [selectedElementId, setSelectedElementId] = useState<string | null>(null);
  const { fitView, setCenter, getNodes } = useReactFlow();

  const isRunning = execution.status === 'Running' || execution.status === 'Queued';

  //
  // Cache the chain definition so nodes don't disappear if the parent's
  // chain prop goes null.
  //
  const chainRef = useRef(chain);
  if (chain) chainRef.current = chain;
  const stableChain = chainRef.current;

  const elementsKey = JSON.stringify(execution.elements);
  const computedFlow = useMemo(
    () => chainToFlowWithStatus(stableChain, execution, operationDefs, payloads),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [stableChain, elementsKey, operationDefs, payloads]
  );

  const [nodes, setNodes, onNodesChange] = useNodesState(computedFlow.nodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(computedFlow.edges);
  useEffect(() => {
    setNodes(computedFlow.nodes);
    setEdges(computedFlow.edges);
  }, [computedFlow, setNodes, setEdges]);

  //
  // Auto-fit view on initial load.
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
  // Auto-zoom to currently running element.
  //
  const lastRunningIdRef = useRef<string | null>(null);
  useEffect(() => {
    if (execution.status !== 'Running') return;

    const runningId = Object.entries(execution.elements).find(
      ([, elem]) => elem.status === 'Running'
    )?.[0];
    if (!runningId || runningId === lastRunningIdRef.current) return;
    lastRunningIdRef.current = runningId;

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

  const statusColor = execution.status === 'Completed'
    ? 'text-[var(--accent-success)]'
    : execution.status === 'Failed'
    ? 'text-[var(--accent-error)]'
    : execution.status === 'Running'
    ? 'text-[var(--accent-warning)]'
    : 'text-muted';

  return (
    <div className="h-full flex flex-col">
      {/*
      //
      // Header bar.
      //
      */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-subtle bg-[var(--bg-secondary)]">
        <div className="flex items-center gap-3">
          {onBack && (
            <button
              onClick={onBack}
              className="text-muted hover:text-title transition-colors"
              title="Back to editor"
            >
              <ArrowLeft size={14} />
            </button>
          )}
          <span className="text-sm font-medium text-title">{execution.chain_name}</span>
          <span className={`text-xs ${statusColor}`}>{execution.status}</span>
          <span className="text-[10px] text-muted">
            {Object.keys(execution.elements).length} elements
          </span>
        </div>
        <div className="flex items-center gap-2">
          {isRunning && onCancel && (
            <button
              onClick={onCancel}
              className="flex items-center gap-2 px-3 py-1.5 text-xs bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors"
            >
              <Square size={12} />
              Cancel
            </button>
          )}
        </div>
      </div>

      {/*
      //
      // Flow graph using shared node components from ChainNodes.tsx.
      //
      */}
      <div className="h-48 border-b border-subtle">
        {stableChain ? (
          <ReactFlow
            nodes={nodes}
            edges={edges}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            nodeTypes={nodeTypes}
            fitView
            fitViewOptions={{ padding: 0.05, maxZoom: 1.5 }}
            minZoom={0.2}
            maxZoom={2}
            nodesDraggable={false}
            nodesConnectable={false}
            elementsSelectable={true}
            panOnDrag
            zoomOnScroll
            proOptions={{ hideAttribution: true }}
            onNodeClick={(_, node) => setSelectedElementId(node.id)}
          >
            <Background variant={BackgroundVariant.Dots} gap={20} size={1} color="var(--text-secondary)" />
            <Panel position="bottom-right" className="!m-1">
              <button
                onClick={() => fitView({ padding: 0.05, maxZoom: 1.5 })}
                className="p-1 bg-[var(--bg-secondary)] border border-subtle rounded hover:bg-[var(--bg-tertiary)]"
              >
                <Maximize2 size={10} className="text-muted" />
              </button>
            </Panel>
          </ReactFlow>
        ) : (
          <div className="h-full flex items-center justify-center text-muted text-sm">
            Chain definition not available
          </div>
        )}
      </div>

      {/*
      //
      // Event timeline.
      //
      */}
      <div className="flex-1 min-h-0 flex flex-col">
        <div className="px-3 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)] flex items-center justify-between">
          <span className="text-[10px] tracking-wider text-muted">
            EVENTS {selectedElementId ? `(filtered)` : ''} · {executionEvents.length} total
          </span>
          {selectedElementId && (
            <button
              onClick={() => setSelectedElementId(null)}
              className="text-[10px] text-[var(--accent-info)] hover:underline"
            >
              Show all
            </button>
          )}
        </div>
        <div className="flex-1 min-h-0">
          <ExecutionEventTimeline
            events={executionEvents}
            filterElementId={selectedElementId}
          />
        </div>
      </div>

      {/*
      //
      // Outputs section (when complete).
      //
      */}
      {Object.keys(execution.outputs).length > 0 && (
        <div className="border-t border-subtle max-h-48 overflow-auto">
          <div className="px-3 py-1.5 bg-[var(--bg-tertiary)] text-[10px] tracking-wider text-muted">
            OUTPUTS
          </div>
          {Object.entries(execution.outputs).map(([label, output]) => (
            <div key={label} className="px-3 py-2 border-b border-subtle">
              <div className="text-xs font-medium text-title mb-1">{label}</div>
              <div className="text-xs text-[var(--text-secondary)] font-mono whitespace-pre-wrap max-h-24 overflow-auto">
                {output.length > 500 ? output.substring(0, 500) + '...' : output}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

interface ExecutionViewerProps {
  execution: ChainExecutionUpdate;
  chain: ChainDefinitionFull | null;
  executionEvents: ChainExecutionEvent[];
  operationDefs?: OperationDefinitionInfo[];
  payloads?: PayloadInfo[];
  onCancel?: () => void;
  onBack?: () => void;
}

export function ExecutionViewer(props: ExecutionViewerProps) {
  return (
    <ReactFlowProvider>
      <ExecutionViewerInner {...props} />
    </ReactFlowProvider>
  );
}
