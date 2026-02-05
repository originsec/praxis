import { useState, useEffect, useRef } from 'react';
import { Zap, GitBranch } from 'lucide-react';
import { Modal } from './Modal';
import type { NodeState } from '../../api/types';

export interface RunItem {
  id: string;
  name: string;
  description?: string;
  //
  // e.g., category for ops, element count for chains.
  //
  badge?: string;
}

interface RunModalProps {
  isOpen: boolean;
  onClose: () => void;
  onRun: (itemId: string, nodeId: string, agentName: string) => void;
  title: string;
  items: RunItem[];
  variant: 'operation' | 'chain';
  //
  // For single-select mode (from row click), pass the pre-selected item.
  //
  preSelectedItem?: RunItem | null;
  //
  // For node/agent selection - provide nodes array.
  //
  nodes?: NodeState[];
  //
  // For fixed node/agent (e.g., agent detail page) - skip selection UI.
  //
  fixedNodeId?: string;
  fixedAgentName?: string;
  //
  // Optional warning message (e.g., "Running will close current session").
  //
  warningMessage?: string;
}

export function RunModal({
  isOpen,
  onClose,
  onRun,
  title,
  items,
  variant,
  preSelectedItem,
  nodes = [],
  fixedNodeId,
  fixedAgentName,
  warningMessage,
}: RunModalProps) {
  const [selectedItemId, setSelectedItemId] = useState<string | null>(null);
  const [nodeId, setNodeId] = useState<string>('');
  const [agentName, setAgentName] = useState<string>('');

  const Icon = variant === 'operation' ? Zap : GitBranch;
  const isOperation = variant === 'operation';
  const prevIsOpen = useRef(false);

  //
  // When fixed node/agent provided, use those values.
  //
  const hasFixedTarget = !!(fixedNodeId && fixedAgentName);
  const effectiveNodeId = hasFixedTarget ? fixedNodeId : nodeId;
  const effectiveAgentName = hasFixedTarget ? fixedAgentName : agentName;

  //
  // Reset state only when modal first opens (not on every nodes change).
  //
  useEffect(() => {
    const justOpened = isOpen && !prevIsOpen.current;
    prevIsOpen.current = isOpen;

    if (justOpened) {
      setSelectedItemId(preSelectedItem?.id ?? null);
      //
      // Only set node/agent if not using fixed values.
      //
      if (!hasFixedTarget) {
        if (nodes.length > 0) {
          setNodeId(nodes[0].node_id);
          const agent = nodes[0].selected_agent?.short_name || nodes[0].discovered_agents?.[0]?.short_name || '';
          setAgentName(agent);
        } else {
          setNodeId('');
          setAgentName('');
        }
      }
    }
  }, [isOpen, preSelectedItem, nodes, hasFixedTarget]);

  //
  // Update agent when node changes.
  //
  const handleNodeChange = (newNodeId: string) => {
    setNodeId(newNodeId);
    const node = nodes.find(n => n.node_id === newNodeId);
    if (node) {
      const agent = node.selected_agent?.short_name || node.discovered_agents?.[0]?.short_name || '';
      setAgentName(agent);
    } else {
      setAgentName('');
    }
  };

  const handleRun = () => {
    if (selectedItemId && effectiveNodeId && effectiveAgentName) {
      onRun(selectedItemId, effectiveNodeId, effectiveAgentName);
      onClose();
    }
  };

  const isSingleSelect = !!preSelectedItem;

  return (
    <Modal isOpen={isOpen} onClose={onClose} title={title} size="md">
      <div className="space-y-0">
        {/*
        //
        // Warning message.
        //
        */}
        {warningMessage && (
          <div className="p-2.5 bg-[var(--bg-secondary)]">
            <div className="flex items-start gap-2 p-3 bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/30">
              <span className="text-[var(--accent-warning)] mt-0.5">⚠</span>
              <p className="text-sm text-[var(--accent-warning)]">{warningMessage}</p>
            </div>
          </div>
        )}

        {/*
        //
        // Item selection section.
        //
        */}
        <div className="p-2.5 bg-[var(--bg-secondary)]">
          {isSingleSelect && preSelectedItem ? (
            <div className={`p-3 border ${isOperation ? 'bg-[var(--accent-purple)]/10 border-[var(--accent-purple)]/30' : 'bg-[var(--accent-info)]/10 border-[var(--accent-info)]/30'}`}>
              <div className="flex items-center justify-between">
                <span className="font-medium text-sm text-highlight">{preSelectedItem.name}</span>
                {preSelectedItem.badge && (
                  <span className="text-xs" style={{ color: 'var(--text-muted)' }}>{preSelectedItem.badge}</span>
                )}
              </div>
              {preSelectedItem.description && (
                <p className="text-xs mt-1" style={{ color: 'var(--text-muted)' }}>{preSelectedItem.description}</p>
              )}
            </div>
          ) : (
            <>
              {/*
              //
              // Item selector.
              //
              */}
              {items.length === 0 ? (
                <div className="p-6 text-center">
                  <Icon size={32} className="mx-auto mb-3 text-muted opacity-50" />
                  <p className="text-muted text-sm">No {variant} definitions available</p>
                  <p className="text-xs mt-1" style={{ color: 'var(--text-muted)' }}>Add {variant === 'operation' ? 'operations' : 'chains'} in the Operations page</p>
                </div>
              ) : (
                <div className="space-y-2 max-h-48 overflow-y-auto scrollbar-on-hover">
                  {items.map((item) => (
                    <div
                      key={item.id}
                      onClick={() => setSelectedItemId(item.id)}
                      className={`p-3 cursor-pointer transition-colors border ${
                        selectedItemId === item.id
                          ? isOperation
                            ? 'bg-[var(--accent-purple)]/20 border-[var(--accent-purple)]'
                            : 'bg-[var(--accent-info)]/20 border-[var(--accent-info)]'
                          : 'bg-[var(--bg-primary)] border-dim hover:border-subtle'
                      }`}
                    >
                      <div className="flex items-center justify-between">
                        <span className="font-medium text-sm text-highlight">{item.name}</span>
                        {item.badge && (
                          <span className="text-xs" style={{ color: 'var(--text-muted)' }}>{item.badge}</span>
                        )}
                      </div>
                      {item.description && (
                        <p className="text-xs mt-1 line-clamp-2" style={{ color: 'var(--text-muted)' }}>{item.description}</p>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </>
          )}
        </div>

        {/*
        //
        // Node/Agent selectors - only show if not using fixed target.
        //
        */}
        {!hasFixedTarget && (
          <div className="p-2.5 bg-[var(--bg-secondary)]">
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Node</label>
                <select
                  value={nodeId}
                  onChange={(e) => handleNodeChange(e.target.value)}
                  className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                >
                  <option value="">Select node</option>
                  {nodes.map((node) => (
                    <option key={node.node_id} value={node.node_id}>
                      {node.machine_name}
                    </option>
                  ))}
                </select>
              </div>

              <div>
                <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Agent</label>
                <select
                  value={agentName}
                  onChange={(e) => setAgentName(e.target.value)}
                  className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                >
                  <option value="">Select agent</option>
                  {nodeId && nodes.find(n => n.node_id === nodeId)?.discovered_agents?.map(agent => (
                    <option key={agent.short_name} value={agent.short_name}>
                      {agent.short_name}
                    </option>
                  ))}
                </select>
              </div>
            </div>
          </div>
        )}

        {/*
        //
        // Actions.
        //
        */}
        <div className="p-2.5 bg-[var(--bg-secondary)]">
          <div className="flex justify-end gap-2">
            <button
              onClick={onClose}
              className="px-4 py-2 text-xs tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleRun}
              disabled={!selectedItemId || !effectiveNodeId || !effectiveAgentName}
              className={`inline-flex items-center gap-2 px-4 py-2 text-xs tracking-wider border border-dim transition-colors disabled:opacity-50 ${
                isOperation
                  ? 'bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:border-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30'
                  : 'bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:border-[var(--accent-info)] hover:bg-[var(--accent-info)]/30'
              }`}
            >
              <Icon size={14} />
              Run
            </button>
          </div>
        </div>
      </div>
    </Modal>
  );
}
