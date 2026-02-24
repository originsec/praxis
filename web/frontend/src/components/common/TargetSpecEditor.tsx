import { useState, useMemo, useRef, useEffect } from 'react';
import { X, ChevronDown } from 'lucide-react';
import type { TargetSpec, NodeState } from '../../api/types';

interface TargetSpecEditorProps {
  value: TargetSpec;
  onChange: (spec: TargetSpec) => void;
  nodes: NodeState[];
  showTriggeringNodeOption?: boolean;
}

export function TargetSpecEditor({ value, onChange, nodes, showTriggeringNodeOption = false }: TargetSpecEditorProps) {
  const [nodeDropdownOpen, setNodeDropdownOpen] = useState(false);
  const [agentDropdownOpen, setAgentDropdownOpen] = useState(false);
  const nodeDropdownRef = useRef<HTMLDivElement>(null);
  const agentDropdownRef = useRef<HTMLDivElement>(null);

  //
  // Close dropdowns on click outside.
  //

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (nodeDropdownRef.current && !nodeDropdownRef.current.contains(e.target as Node)) {
        setNodeDropdownOpen(false);
      }
      if (agentDropdownRef.current && !agentDropdownRef.current.contains(e.target as Node)) {
        setAgentDropdownOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  //
  // Collect all unique agent short_names across all nodes.
  //

  const allAgentNames = useMemo(() => {
    const names = new Set<string>();
    for (const node of nodes) {
      if (node.discovered_agents) {
        for (const agent of node.discovered_agents) {
          names.add(agent.short_name);
        }
      }
    }
    return Array.from(names).sort();
  }, [nodes]);

  const handleAddNode = (nodeId: string) => {
    if (!value.node_ids.includes(nodeId)) {
      onChange({ ...value, node_ids: [...value.node_ids, nodeId] });
    }
    setNodeDropdownOpen(false);
  };

  const handleRemoveNode = (nodeId: string) => {
    onChange({ ...value, node_ids: value.node_ids.filter(id => id !== nodeId) });
  };

  const handleAddAgent = (agentName: string) => {
    if (!value.agent_short_names.includes(agentName)) {
      onChange({ ...value, agent_short_names: [...value.agent_short_names, agentName] });
    }
    setAgentDropdownOpen(false);
  };

  const handleRemoveAgent = (agentName: string) => {
    onChange({ ...value, agent_short_names: value.agent_short_names.filter(n => n !== agentName) });
  };

  const availableNodes = nodes.filter(n => !value.node_ids.includes(n.node_id));
  const availableAgents = allAgentNames.filter(n => !value.agent_short_names.includes(n));

  return (
    <div className="space-y-2.5">
      {/*
      //
      // Node multi-select.
      //
      */}
      <div>
        <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Target Nodes</label>
        <div className="flex flex-wrap gap-1 mb-1">
          {value.node_ids.length === 0 && (
            <span className="text-[10px] text-muted italic">All nodes</span>
          )}
          {value.node_ids.map(nodeId => {
            const node = nodes.find(n => n.node_id === nodeId);
            return (
              <span
                key={nodeId}
                className="inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] bg-[var(--accent-info)]/15 text-[var(--accent-info)] border border-[var(--accent-info)]/30"
              >
                {node?.machine_name || nodeId.slice(0, 8)}
                <button
                  type="button"
                  onClick={() => handleRemoveNode(nodeId)}
                  className="hover:text-highlight transition-colors"
                >
                  <X size={9} />
                </button>
              </span>
            );
          })}
        </div>
        <div className="relative" ref={nodeDropdownRef}>
          <button
            type="button"
            onClick={() => { setNodeDropdownOpen(!nodeDropdownOpen); setAgentDropdownOpen(false); }}
            disabled={availableNodes.length === 0}
            className="flex items-center gap-1.5 px-2 py-1 text-[10px] text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors w-full disabled:opacity-50"
          >
            <ChevronDown size={11} />
            {availableNodes.length === 0 ? 'No more nodes' : 'Add node'}
          </button>
          {nodeDropdownOpen && availableNodes.length > 0 && (
            <div className="absolute z-50 top-full left-0 w-full mt-0.5 bg-[var(--bg-primary)] border border-subtle max-h-40 overflow-y-auto shadow-lg">
              {availableNodes.map(node => (
                <button
                  key={node.node_id}
                  type="button"
                  onClick={() => handleAddNode(node.node_id)}
                  className="block w-full text-left px-2 py-1 text-[10px] text-highlight hover:bg-[var(--highlight)] transition-colors"
                >
                  {node.machine_name} <span className="text-muted">({node.os_details})</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {/*
      //
      // OS filter.
      //
      */}
      <div>
        <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">OS Filter</label>
        <input
          type="text"
          value={value.os_filter || ''}
          onChange={(e) => onChange({ ...value, os_filter: e.target.value || null })}
          placeholder="e.g. Windows, Linux (empty = all)"
          className="w-full bg-[var(--bg-primary)] border border-dim px-2 py-1 text-[10px] text-highlight focus:outline-none focus:border-subtle transition-colors"
        />
      </div>

      {/*
      //
      // Agent multi-select.
      //
      */}
      <div>
        <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Target Agents</label>
        <div className="flex flex-wrap gap-1 mb-1">
          {value.agent_short_names.length === 0 && (
            <span className="text-[10px] text-muted italic">All agents</span>
          )}
          {value.agent_short_names.map(name => (
            <span
              key={name}
              className="inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] bg-[var(--accent-purple)]/15 text-[var(--accent-purple)] border border-[var(--accent-purple)]/30"
            >
              {name}
              <button
                type="button"
                onClick={() => handleRemoveAgent(name)}
                className="hover:text-highlight transition-colors"
              >
                <X size={9} />
              </button>
            </span>
          ))}
        </div>
        <div className="relative" ref={agentDropdownRef}>
          <button
            type="button"
            onClick={() => { setAgentDropdownOpen(!agentDropdownOpen); setNodeDropdownOpen(false); }}
            disabled={availableAgents.length === 0}
            className="flex items-center gap-1.5 px-2 py-1 text-[10px] text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors w-full disabled:opacity-50"
          >
            <ChevronDown size={11} />
            {availableAgents.length === 0 ? 'No more agents' : 'Add agent'}
          </button>
          {agentDropdownOpen && availableAgents.length > 0 && (
            <div className="absolute z-50 top-full left-0 w-full mt-0.5 bg-[var(--bg-primary)] border border-subtle max-h-40 overflow-y-auto shadow-lg">
              {availableAgents.map(name => (
                <button
                  key={name}
                  type="button"
                  onClick={() => handleAddAgent(name)}
                  className="block w-full text-left px-2 py-1 text-[10px] text-highlight hover:bg-[var(--highlight)] transition-colors"
                >
                  {name}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {/*
      //
      // Include triggering node checkbox.
      //
      */}
      {showTriggeringNodeOption && (
        <label className="flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            checked={value.include_triggering_node}
            onChange={(e) => onChange({ ...value, include_triggering_node: e.target.checked })}
            className="accent-[var(--accent-info)]"
          />
          <span className="text-[10px] text-[var(--text-secondary)]">Include triggering node</span>
        </label>
      )}
    </div>
  );
}
