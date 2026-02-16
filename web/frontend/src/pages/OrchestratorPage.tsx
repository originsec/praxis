import { useRef, useEffect, useMemo, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import {
  Bot,
  Send,
  Loader2,
  Sparkles,
  PlayCircle,
  StopCircle,
  Square,
  CheckCircle,
  XCircle,
  Circle,
  Wrench,
  ListTodo,
  AlertCircle,
  ChevronRight,
  ChevronDown,
  Download,
  Activity,
} from 'lucide-react';
import { exportOrchestratorSession, downloadTextFile } from '../utils/export';
import { useApp, type OrchestratorMessage, type OrchestratorToolExecution } from '../context/AppContext';
import type { OrchestratorPlan, PlanStep, NodeState } from '../api/types';
import type { OrchestratorState } from '../context/orchestratorTypes';
import { getFeatureFlags } from '../utils/featureFlags';

//
// Plan step status icon.
//
function PlanStepIcon({ status }: { status: PlanStep['status'] }) {
  switch (status) {
    case 'done':
      return <CheckCircle size={10} className="text-[var(--accent-success)]" />;
    case 'in_progress':
      return <Loader2 size={10} className="text-[var(--accent-warning)] animate-spin" />;
    case 'not_started':
    default:
      return <Circle size={10} className="text-muted" />;
  }
}

//
// Plan display component.
//
function PlanDisplay({ plan }: { plan: OrchestratorPlan }) {
  const doneCount = plan.steps.filter((s) => s.status === 'done').length;
  const totalCount = plan.steps.length;
  const progressPercent = totalCount > 0 ? (doneCount / totalCount) * 100 : 0;

  return (
    <div className="bg-[var(--bg-tertiary)] p-3 mb-3 border border-subtle">
      <div className="flex items-center gap-2 mb-2">
        <ListTodo size={12} className="text-[var(--accent-purple)]" />
        <span className="font-medium text-xs">Plan</span>
        <span className="text-[10px] text-muted ml-auto">
          {doneCount}/{totalCount}
        </span>
      </div>

      {/*
      //
      // Progress bar.
      //
      */}
      <div className="h-0.5 bg-[var(--bg-secondary)] rounded-full mb-2 overflow-hidden">
        <div
          className="h-full bg-[var(--accent-purple)]/60 transition-all duration-300"
          style={{ width: `${progressPercent}%` }}
        />
      </div>

      {/*
      //
      // Current step description.
      //
      */}
      {plan.current_step_description && (
        <div className="text-xs text-[var(--accent-warning)] mb-2 font-medium">
          {plan.current_step_description}
        </div>
      )}

      {/*
      //
      // Steps.
      //
      */}
      <div className="space-y-1">
        {plan.steps.map((step, idx) => (
          <div
            key={idx}
            className={`flex items-start gap-1.5 text-xs ${
              step.status === 'done'
                ? 'text-muted line-through'
                : step.status === 'in_progress'
                ? 'text-[var(--text-primary)]'
                : 'text-[var(--text-secondary)]'
            }`}
          >
            <div className="mt-0.5">
              <PlanStepIcon status={step.status} />
            </div>
            <span>{step.description}</span>
          </div>
        ))}
      </div>

      {/*
      //
      // Summary.
      //
      */}
      {plan.summary && (
        <div className="mt-2 pt-2 border-t border-subtle text-xs text-[var(--text-highlight)]/50 italic">
          {plan.summary}
        </div>
      )}
    </div>
  );
}

//
// Single tool execution item.
//
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

//
// Tool execution display - collapsible for completed messages.
//
function ToolExecutionDisplay({
  executions,
  collapsible = false,
}: {
  executions: OrchestratorToolExecution[];
  collapsible?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  if (executions.length === 0) return null;

  //
  // For streaming (not collapsible), always show all.
  //
  if (!collapsible) {
    return (
      <div className="space-y-1 mb-2">
        {executions.map((exec, idx) => (
          <ToolExecutionItem key={idx} exec={exec} />
        ))}
      </div>
    );
  }

  //
  // For completed messages (collapsible), show summary with expand option.
  //
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

//
// Message component.
//
function ChatMessage({ message }: { message: OrchestratorMessage }) {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';

  return (
    <div
      className={`flex ${isUser ? 'justify-end' : isSystem ? 'justify-center' : 'justify-start'}`}
    >
      <div
        className={`w-full md:max-w-[85%] ascii-box px-3 md:px-4 py-3 ${
          isUser
            ? 'bg-[var(--accent-purple)]/20 text-[var(--text-primary)]'
            : isSystem
            ? 'bg-[var(--bg-tertiary)] text-muted text-sm'
            : 'bg-[var(--bg-secondary)] text-[var(--text-highlight)]/80'
        }`}
      >
        {!isUser && !isSystem && (
          <div className="flex items-center gap-2 mb-2 text-[var(--accent-success)]">
            <Bot size={16} />
            <span className="text-xs font-medium">Orchestrator</span>
          </div>
        )}

        {/*
        //
        // Tool executions - collapsible for completed assistant messages.
        //
        */}
        {message.toolExecutions && (
          <ToolExecutionDisplay executions={message.toolExecutions} collapsible={true} />
        )}

        {/*
        //
        // Content.
        //
        */}
        {isUser || isSystem ? (
          <div className="whitespace-pre-wrap break-words">{message.content}</div>
        ) : (
          <div className="prose prose-invert prose-sm max-w-none break-words prose-table:border-collapse prose-th:border prose-th:border-subtle prose-th:px-3 prose-th:py-2 prose-th:bg-[var(--bg-tertiary)] prose-td:border prose-td:border-subtle prose-td:px-3 prose-td:py-2">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{message.content}</ReactMarkdown>
          </div>
        )}

        <p className="text-xs text-muted mt-2">{message.timestamp.toLocaleTimeString()}</p>
      </div>
    </div>
  );
}

//
// Streaming message display.
//
function StreamingMessage({
  content,
  toolExecutions,
}: {
  content: string;
  toolExecutions: OrchestratorToolExecution[];
}) {
  return (
    <div className="flex justify-start">
      <div className="w-full md:max-w-[85%] ascii-box px-3 md:px-4 py-3 bg-[var(--bg-secondary)] text-[var(--text-highlight)]/80">
        <div className="flex items-center gap-2 mb-2 text-[var(--accent-success)]">
          <Bot size={16} />
          <span className="text-xs font-medium">Orchestrator</span>
          <Loader2 size={12} className="animate-spin ml-auto" />
        </div>

        <ToolExecutionDisplay executions={toolExecutions} />

        {content && (
          <div className="prose prose-invert prose-sm max-w-none break-words prose-table:border-collapse prose-th:border prose-th:border-subtle prose-th:px-3 prose-th:py-2 prose-th:bg-[var(--bg-tertiary)] prose-td:border prose-td:border-subtle prose-td:px-3 prose-td:py-2">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
          </div>
        )}

        {!content && toolExecutions.length === 0 && (
          <div className="flex items-center gap-2 text-muted text-sm">
            <Loader2 size={14} className="animate-spin" />
            <span>Thinking...</span>
          </div>
        )}
      </div>
    </div>
  );
}

type VizNodeKind = 'orchestrator' | 'plan' | 'tool' | 'node' | 'agent';
type VizNodeStatus = 'idle' | 'running' | 'done' | 'error';
type VizEdgeKind = 'flow' | 'relation' | 'impact';

interface VizNode {
  id: string;
  label: string;
  subtitle?: string;
  kind: VizNodeKind;
  status: VizNodeStatus;
  x: number;
  y: number;
  z: number;
  size: number;
}

interface VizEdge {
  id: string;
  source: string;
  target: string;
  kind: VizEdgeKind;
}

function safeParseJson(value?: string): unknown | null {
  if (!value) return null;
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}

function collectExecutionTargets(
  value: unknown,
  acc: { nodeIds: Set<string>; agentShortNames: Set<string> },
  depth = 0
) {
  if (value == null || depth > 5) return;
  if (Array.isArray(value)) {
    for (const item of value) collectExecutionTargets(item, acc, depth + 1);
    return;
  }
  if (typeof value !== 'object') return;

  const obj = value as Record<string, unknown>;
  for (const [keyRaw, inner] of Object.entries(obj)) {
    const key = keyRaw.toLowerCase();
    if (typeof inner === 'string') {
      if (key === 'node_id' && inner.trim()) acc.nodeIds.add(inner);
      if ((key === 'agent_short_name' || key === 'short_name') && inner.trim()) {
        acc.agentShortNames.add(inner);
      }
    }
    collectExecutionTargets(inner, acc, depth + 1);
  }
}

function toNodeIdKey(nodeId: string): string {
  return `node:${nodeId}`;
}

function toAgentIdKey(nodeId: string, shortName: string): string {
  return `agent:${nodeId}:${shortName}`;
}

function OrchestratorLiveMap({
  orchestrator,
  nodes,
}: {
  orchestrator: OrchestratorState;
  nodes: NodeState[];
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [rotation, setRotation] = useState(0);
  const [viewport, setViewport] = useState({ width: 920, height: 320 });

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const width = Math.max(640, Math.floor(entry.contentRect.width));
      setViewport({ width, height: 320 });
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    let rafId = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const dt = Math.min(80, now - last);
      last = now;
      setRotation((prev) => (prev + dt * 0.00035) % (Math.PI * 2));
      rafId = requestAnimationFrame(tick);
    };
    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, []);

  const latestAssistantTools = useMemo(() => {
    const latest = [...orchestrator.messages].reverse().find((m) => m.role === 'assistant' && m.toolExecutions?.length);
    return latest?.toolExecutions ?? [];
  }, [orchestrator.messages]);

  const activeTools = orchestrator.currentToolExecutions.length > 0
    ? orchestrator.currentToolExecutions
    : latestAssistantTools;

  const hasVizData = orchestrator.sessionActive
    || orchestrator.isLoading
    || (orchestrator.currentPlan?.steps.length ?? 0) > 0
    || activeTools.length > 0;

  const viz = useMemo(() => {
    const vizNodes: VizNode[] = [];
    const vizEdges: VizEdge[] = [];
    const nodeById = new Map<string, VizNode>();

    const addNode = (node: VizNode) => {
      if (!nodeById.has(node.id)) {
        nodeById.set(node.id, node);
        vizNodes.push(node);
      }
    };
    const addEdge = (edge: VizEdge) => {
      if (edge.source === edge.target) return;
      if (!vizEdges.find((e) => e.source === edge.source && e.target === edge.target && e.kind === edge.kind)) {
        vizEdges.push(edge);
      }
    };

    const placeLayer = (layer: VizNode[], x: number, spreadY: number, spreadZ: number) => {
      if (layer.length === 0) return;
      const center = (layer.length - 1) / 2;
      for (let i = 0; i < layer.length; i += 1) {
        const yOffset = (i - center) * spreadY;
        const zOffset = ((i % 2 === 0 ? 1 : -1) * ((Math.floor(i / 2) + 1) * spreadZ));
        layer[i].x = x;
        layer[i].y = yOffset;
        layer[i].z = zOffset;
      }
    };

    const layerOrchestrator: VizNode[] = [];
    const layerPlan: VizNode[] = [];
    const layerTools: VizNode[] = [];
    const layerNodes: VizNode[] = [];
    const layerAgents: VizNode[] = [];

    const coreNode: VizNode = {
      id: 'orchestrator:core',
      label: 'Orchestrator',
      subtitle: orchestrator.isLoading ? 'running' : 'idle',
      kind: 'orchestrator',
      status: orchestrator.isLoading ? 'running' : orchestrator.sessionActive ? 'idle' : 'done',
      x: 0,
      y: 0,
      z: 0,
      size: 12,
    };
    addNode(coreNode);
    layerOrchestrator.push(coreNode);

    const plan = orchestrator.currentPlan;
    const fullSteps = plan?.steps ?? [];
    const activeIndex = fullSteps.findIndex((s) => s.status === 'in_progress');
    const windowStart = activeIndex >= 0 ? Math.max(0, activeIndex - 1) : Math.max(0, fullSteps.length - 3);
    const visiblePlan = fullSteps.slice(windowStart, windowStart + 4);
    const planStepIds: string[] = [];
    for (let i = 0; i < visiblePlan.length; i += 1) {
      const sourceIndex = windowStart + i;
      const step = visiblePlan[i];
      const stepId = `plan:${sourceIndex}`;
      planStepIds.push(stepId);
      const stepNode: VizNode = {
        id: stepId,
        label: `S${sourceIndex + 1}`,
        subtitle: step.description,
        kind: 'plan',
        status: step.status === 'done' ? 'done' : step.status === 'in_progress' ? 'running' : 'idle',
        x: 0,
        y: 0,
        z: 0,
        size: 10,
      };
      addNode(stepNode);
      layerPlan.push(stepNode);
      addEdge({
        id: `edge:plan-link:${sourceIndex}`,
        source: i === 0 ? 'orchestrator:core' : `plan:${windowStart + i - 1}`,
        target: stepId,
        kind: 'flow',
      });
    }

    const focusedNodeIds = new Set<string>();
    const focusedAgents = new Set<string>();

    const visibleTools = activeTools.slice(-8);
    for (let i = 0; i < visibleTools.length; i += 1) {
      const tool = visibleTools[i];
      const toolId = `tool:${i}:${tool.name}`;
      const toolNode: VizNode = {
        id: toolId,
        label: tool.name,
        subtitle: tool.executing ? 'Pending' : tool.display,
        kind: 'tool',
        status: tool.executing ? 'running' : tool.success ? 'done' : 'error',
        x: 0,
        y: 0,
        z: 0,
        size: 10,
      };
      addNode(toolNode);
      layerTools.push(toolNode);
      addEdge({
        id: `edge:tool:source:${i}`,
        source: planStepIds[planStepIds.length - 1] || 'orchestrator:core',
        target: toolId,
        kind: 'impact',
      });

      const targets = { nodeIds: new Set<string>(), agentShortNames: new Set<string>() };
      collectExecutionTargets(safeParseJson(tool.input), targets);
      collectExecutionTargets(safeParseJson(tool.result), targets);

      for (const nodeId of targets.nodeIds) {
        focusedNodeIds.add(nodeId);
      }
      for (const shortName of targets.agentShortNames) {
        focusedAgents.add(shortName);
      }
    }

    let shownNodes = 0;
    const maxNodes = 8;
    const maxAgents = 10;
    let shownAgents = 0;

    const sortedNodes = [...nodes].sort((a, b) => {
      const aHit = focusedNodeIds.has(a.node_id) ? 1 : 0;
      const bHit = focusedNodeIds.has(b.node_id) ? 1 : 0;
      if (aHit !== bHit) return bHit - aHit;
      return (b.selected_agent ? 1 : 0) - (a.selected_agent ? 1 : 0);
    });

    for (const node of sortedNodes) {
      if (shownNodes >= maxNodes) break;
      const hasFocus = focusedNodeIds.size === 0 || focusedNodeIds.has(node.node_id) || !!node.selected_agent;
      if (!hasFocus && shownNodes >= 4) continue;

      const nodeKey = toNodeIdKey(node.node_id);
      const nodeNode: VizNode = {
        id: nodeKey,
        label: node.machine_name || node.node_id.slice(0, 8),
        subtitle: node.node_id.slice(0, 8),
        kind: 'node',
        status: focusedNodeIds.has(node.node_id) ? 'running' : 'idle',
        x: 0,
        y: 0,
        z: 0,
        size: 11,
      };
      addNode(nodeNode);
      layerNodes.push(nodeNode);
      addEdge({ id: `edge:core:${nodeKey}`, source: 'orchestrator:core', target: nodeKey, kind: 'relation' });
      shownNodes += 1;

      const selected = node.selected_agent?.short_name;
      if (selected && shownAgents < maxAgents) {
        const selectedKey = toAgentIdKey(node.node_id, selected);
        const selectedAgentNode: VizNode = {
          id: selectedKey,
          label: selected,
          subtitle: focusedAgents.has(selected) ? 'In tool path' : 'Selected',
          kind: 'agent',
          status: focusedAgents.has(selected) ? 'running' : 'idle',
          x: 0,
          y: 0,
          z: 0,
          size: 9,
        };
        addNode(selectedAgentNode);
        layerAgents.push(selectedAgentNode);
        addEdge({ id: `edge:${nodeKey}:${selectedKey}`, source: nodeKey, target: selectedKey, kind: 'relation' });
        shownAgents += 1;
      }

      const interestingAgents = node.discovered_agents.filter((agent) => focusedAgents.has(agent.short_name)).slice(0, 2);
      for (const agent of interestingAgents) {
        if (shownAgents >= maxAgents) break;
        if (agent.short_name === selected) continue;
        const agentKey = toAgentIdKey(node.node_id, agent.short_name);
        const agentNode: VizNode = {
          id: agentKey,
          label: agent.short_name,
          subtitle: 'Referenced',
          kind: 'agent',
          status: agent.available ? 'running' : 'error',
          x: 0,
          y: 0,
          z: 0,
          size: 9,
        };
        addNode(agentNode);
        layerAgents.push(agentNode);
        addEdge({ id: `edge:${nodeKey}:${agentKey}`, source: nodeKey, target: agentKey, kind: 'relation' });
        shownAgents += 1;
      }
    }

    for (const edge of vizEdges) {
      const sourceNode = nodeById.get(edge.source);
      const targetNode = nodeById.get(edge.target);
      if (!sourceNode || !targetNode) continue;
      if (sourceNode.kind === 'tool' && targetNode.kind === 'node') {
        sourceNode.status = sourceNode.status === 'error' ? 'error' : 'running';
        targetNode.status = 'running';
      }
      if (sourceNode.kind === 'tool' && targetNode.kind === 'agent') {
        targetNode.status = 'running';
      }
    }

    placeLayer(layerOrchestrator, -280, 120, 50);
    placeLayer(layerPlan, -110, 85, 35);
    placeLayer(layerTools, 60, 70, 30);
    placeLayer(layerNodes, 260, 82, 35);
    placeLayer(layerAgents, 430, 70, 28);

    return {
      nodes: vizNodes,
      edges: vizEdges,
      toolCount: visibleTools.length,
      pendingTools: visibleTools.filter((t) => t.executing).length,
    };
  }, [activeTools, nodes, orchestrator.currentPlan, orchestrator.isLoading, orchestrator.sessionActive]);

  const edgeColor = (kind: VizEdgeKind): string => {
    if (kind === 'flow') return 'var(--accent-info)';
    if (kind === 'impact') return 'var(--accent-warning)';
    return 'var(--text-secondary)';
  };

  const kindColor = (kind: VizNodeKind): string => {
    if (kind === 'orchestrator') return 'var(--accent-purple)';
    if (kind === 'plan') return 'var(--accent-info)';
    if (kind === 'tool') return 'var(--accent-warning)';
    if (kind === 'node') return 'var(--accent-success)';
    return 'var(--text-highlight)';
  };

  const projected = useMemo(() => {
    const cos = Math.cos(rotation);
    const sin = Math.sin(rotation);
    const fov = 780;
    const depthOffset = 980;
    const cx = viewport.width / 2;
    const cy = viewport.height / 2;

    const nodeMap = new Map<string, { x: number; y: number; r: number; depth: number; node: VizNode }>();
    for (const node of viz.nodes) {
      const rx = node.x * cos - node.z * sin;
      const rz = node.x * sin + node.z * cos;
      const scale = fov / Math.max(240, depthOffset + rz);
      nodeMap.set(node.id, {
        x: cx + rx * scale,
        y: cy + node.y * scale,
        r: Math.max(3.5, node.size * scale),
        depth: rz,
        node,
      });
    }

    const edges = viz.edges
      .map((edge) => {
        const source = nodeMap.get(edge.source);
        const target = nodeMap.get(edge.target);
        if (!source || !target) return null;
        return {
          edge,
          source,
          target,
          depth: (source.depth + target.depth) / 2,
        };
      })
      .filter((e): e is NonNullable<typeof e> => !!e)
      .sort((a, b) => a.depth - b.depth);

    const nodesSorted = [...nodeMap.values()].sort((a, b) => a.depth - b.depth);
    return { edges, nodes: nodesSorted };
  }, [rotation, viewport.height, viewport.width, viz.edges, viz.nodes]);

  if (!hasVizData) return null;

  return (
    <div className="mb-4 bg-card ascii-box border border-subtle">
      <div className="px-3 py-2 border-b border-subtle flex items-center gap-3 text-xs">
        <div className="flex items-center gap-2 text-[var(--text-primary)]">
          <Activity size={13} className={orchestrator.isLoading ? 'animate-pulse text-[var(--accent-warning)]' : 'text-[var(--accent-info)]'} />
          <span className="font-medium">Execution Topology</span>
        </div>
        <span className="text-muted">{viz.toolCount} tool calls</span>
        {viz.pendingTools > 0 && <span className="text-[var(--accent-warning)]">{viz.pendingTools} pending</span>}
        <span className="text-muted ml-auto">Live map of plan, tools, nodes, and agents</span>
      </div>
      <div ref={containerRef} className="relative h-[320px] overflow-hidden bg-[radial-gradient(ellipse_at_center,var(--bg-secondary),var(--bg-primary))]">
        <svg width={viewport.width} height={viewport.height} className="absolute inset-0">
          {projected.edges.map(({ edge, source, target }) => (
            <line
              key={edge.id}
              x1={source.x}
              y1={source.y}
              x2={target.x}
              y2={target.y}
              stroke={edgeColor(edge.kind)}
              strokeWidth={edge.kind === 'impact' ? 1.8 : 1.1}
              opacity={edge.kind === 'impact' ? 0.85 : 0.45}
            />
          ))}
          {projected.nodes.map(({ node, x, y, r, depth }) => {
            const depthOpacity = Math.max(0.45, Math.min(1, 1 - (depth + 520) / 1700));
            const fill = node.status === 'error'
              ? 'var(--accent-error)'
              : node.status === 'running'
              ? 'var(--accent-warning)'
              : node.status === 'done'
              ? 'var(--accent-success)'
              : kindColor(node.kind);
            return (
              <g key={node.id} opacity={depthOpacity}>
                <circle cx={x} cy={y} r={r + 2.5} fill={fill} opacity={0.18} />
                <circle cx={x} cy={y} r={r} fill={fill} stroke="var(--bg-primary)" strokeWidth={1.2} />
                <title>{`${node.label}${node.subtitle ? ` • ${node.subtitle}` : ''}`}</title>
                <text
                  x={x}
                  y={y - (r + 5)}
                  textAnchor="middle"
                  fontSize={10}
                  fill="var(--text-primary)"
                  style={{ pointerEvents: 'none' }}
                >
                  {node.label}
                </text>
              </g>
            );
          })}
        </svg>
        <div className="absolute bottom-2 left-3 text-[10px] text-muted">
          rotating 3D projection • focused on active plan/tools
        </div>
      </div>
    </div>
  );
}

export function OrchestratorPage() {
  const { state, orchestratorStart, orchestratorStop, orchestratorCancel, orchestratorPrompt, getConfig } = useApp();
  const { orchestrator } = state;
  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const showExecutionTopology = getFeatureFlags().orchestratorExecutionTopology;

  //
  // Fetch config on mount to check if Orchestrator is configured.
  //
  useEffect(() => {
    if (!state.connected) return;
    getConfig(['llm_feature_orchestrator', 'llm_model_definitions']);
  }, [state.connected, getConfig]);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [orchestrator.messages, orchestrator.streamingContent, orchestrator.currentToolExecutions]);

  //
  // Focus input when loading completes.
  //
  useEffect(() => {
    if (!orchestrator.isLoading && orchestrator.sessionActive) {
      inputRef.current?.focus();
    }
  }, [orchestrator.isLoading, orchestrator.sessionActive]);

  const handleSendMessage = () => {
    if (!input.trim() || orchestrator.isLoading) return;
    orchestratorPrompt(input.trim());
    setInput('');
  };

  const handleNewSession = () => {
    orchestratorStart();
  };

  const handleStopSession = () => {
    orchestratorStop();
  };

  const handleExport = () => {
    if (orchestrator.messages.length === 0) return;
    const content = exportOrchestratorSession(orchestrator.messages, orchestrator.tokenUsage);
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    downloadTextFile(content, `orchestrator-session-${timestamp}.md`);
  };

  //
  // Check if Orchestrator is configured via the LLM feature system.
  //
  const orchestratorConfig = (() => {
    const selectedModelName = state.config.llm_feature_orchestrator;
    if (!selectedModelName) return null;

    const modelDefsRaw = state.config.llm_model_definitions;
    if (!modelDefsRaw) return null;

    try {
      const defs = JSON.parse(modelDefsRaw) as Array<{ name: string; provider: string; model: string }>;
      const def = defs.find((d) => d.name === selectedModelName);
      if (def) {
        return { provider: def.provider, model: def.model };
      }
    } catch {
      // Parse error
    }
    return null;
  })();

  const isConfigured = !!orchestratorConfig;

  return (
    <div className="h-full flex flex-col">
      {/*
      //
      // Header.
      //
      */}
      <div className="flex flex-col lg:flex-row lg:items-center lg:justify-between gap-3 mb-4 md:mb-6">
        <div className="flex items-start md:items-center gap-3">
          <div className="p-3 bg-[var(--accent-purple)]/20">
            <Sparkles size={32} className="text-[var(--accent-purple)]" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h1 className="text-xl md:text-2xl font-bold text-highlight">Orchestrator</h1>
              <span className="px-2 py-0.5 text-xs font-medium bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] rounded">
                Experimental
              </span>
            </div>
            <p className="text-muted mt-1">
              AI-powered red teaming orchestration
              {orchestratorConfig && (
                <span className="ml-2 text-[var(--accent-info)]">
                  · {orchestratorConfig.provider}/{orchestratorConfig.model}
                </span>
              )}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2 md:gap-3">
          {/*
          //
          // Export button.
          //
          */}
          <button
            onClick={handleExport}
            disabled={orchestrator.messages.length === 0}
            className="flex items-center gap-2 px-3 py-2 bg-[var(--bg-secondary)] border border-subtle text-muted hover:text-[var(--text-primary)] hover:border-[var(--border-active)] transition-colors text-sm disabled:opacity-50 disabled:cursor-not-allowed"
            title="Export session transcript"
          >
            <Download size={16} />
          </button>

          {orchestrator.sessionActive ? (
            <button
              onClick={handleStopSession}
              className="flex items-center gap-2 px-4 py-2 bg-[var(--accent-error)]/20 text-[var(--accent-error)]  hover:bg-[var(--accent-error)]/30 transition-colors text-sm"
            >
              <StopCircle size={16} />
              Stop Session
            </button>
          ) : (
            <button
              onClick={handleNewSession}
              disabled={!isConfigured || orchestrator.isStarting}
              className="flex items-center gap-2 px-4 py-2 bg-[var(--accent-purple)]/20 text-[var(--accent-purple)]  hover:bg-[var(--accent-purple)]/30 transition-colors text-sm disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {orchestrator.isStarting ? (
                <>
                  <Loader2 size={16} className="animate-spin" />
                  Starting...
                </>
              ) : (
                <>
                  <PlayCircle size={16} />
                  New Session
                </>
              )}
            </button>
          )}
        </div>
      </div>

      {/*
      //
      // Not configured warning.
      //
      */}
      {!isConfigured && (
        <div className="mb-4 p-3 md:p-4 bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/30  flex items-start gap-3">
          <AlertCircle size={20} className="text-[var(--accent-warning)] mt-0.5 flex-shrink-0" />
          <div>
            <p className="text-sm font-medium text-[var(--accent-warning)]">
              Orchestrator Not Configured
            </p>
            <p className="text-xs text-muted mt-1">
              Go to Settings &gt; Orchestrator to configure an LLM provider and API key.
            </p>
          </div>
        </div>
      )}

      {showExecutionTopology && (
        <OrchestratorLiveMap orchestrator={orchestrator} nodes={state.systemState?.nodes ?? []} />
      )}

      {/*
      //
      // Plan display.
      //
      */}
      {orchestrator.currentPlan && orchestrator.currentPlan.steps.length > 0 && (
        <PlanDisplay plan={orchestrator.currentPlan} />
      )}

      {/*
      //
      // Chat area.
      //
      */}
      <div className="flex-1 bg-card ascii-box border border-subtle flex flex-col min-h-0">
        {/*
        //
        // Messages.
        //
        */}
        <div className="flex-1 overflow-auto p-3 md:p-6 space-y-4">
          {orchestrator.messages.map((msg) => (
            <ChatMessage key={msg.id} message={msg} />
          ))}

          {/*
          //
          // Streaming content.
          //
          */}
          {orchestrator.isLoading && (
            <StreamingMessage
              content={orchestrator.streamingContent}
              toolExecutions={orchestrator.currentToolExecutions}
            />
          )}

          <div ref={messagesEndRef} />
        </div>

        {/*
        //
        // Input.
        //
        */}
        <div className="p-4 border-t border-subtle">
          <div className="flex gap-2 md:gap-3">
            <input
              ref={inputRef}
              type="text"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && !e.shiftKey && handleSendMessage()}
              placeholder={
                orchestrator.sessionActive
                  ? 'Ask Orchestrator anything...'
                  : 'Start a session to begin chatting...'
              }
              className="flex-1 bg-[var(--bg-secondary)] border border-subtle  px-4 py-3 text-[var(--text-primary)] placeholder-[var(--text-secondary)] focus:outline-none focus:border-[var(--border-active)]"
              disabled={!orchestrator.sessionActive || orchestrator.isLoading}
            />
            {orchestrator.isLoading ? (
              <button
                onClick={orchestratorCancel}
                className="px-4 py-3 bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors"
                title="Stop generation"
              >
                <Square size={20} />
              </button>
            ) : (
              <button
                onClick={handleSendMessage}
                disabled={!input.trim() || !orchestrator.sessionActive}
                className="px-4 py-3 bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <Send size={20} />
              </button>
            )}
          </div>
        </div>
      </div>

      {/*
      //
      // Status footer.
      //
      */}
      <div className="mt-3 md:mt-4 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 text-xs md:text-sm text-muted">
        <div className="flex flex-wrap items-center gap-2 md:gap-4">
          <span>{state.systemState?.nodes.length ?? 0} nodes connected</span>
          <span>
            {state.operations.filter((op) => op.status === 'Running').length} operations running
          </span>
          {orchestrator.sessionActive && (
            <span className="text-[var(--accent-purple)]">Orchestrator session active</span>
          )}
          {orchestrator.tokenUsage && (
            <span className="text-[var(--accent-info)]" title={`Prompt: ${orchestrator.tokenUsage.promptTokens.toLocaleString()} | Completion: ${orchestrator.tokenUsage.completionTokens.toLocaleString()}`}>
              {orchestrator.tokenUsage.totalTokens.toLocaleString()} tokens
            </span>
          )}
        </div>
        <span className={state.connected ? 'text-[var(--accent-success)]' : 'text-[var(--accent-error)]'}>
          {state.connected ? 'Connected' : 'Disconnected'}
        </span>
      </div>
    </div>
  );
}
