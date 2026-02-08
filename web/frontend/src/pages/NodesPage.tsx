import { useMemo, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { Server, Trash2, Bot, Shield, Clock } from 'lucide-react';
import { useApp } from '../context/AppContext';
import { StatusBadge, getNodeStatus } from '../components/common/StatusBadge';

export function NodesPage() {
  const navigate = useNavigate();
  const { state, removeNode } = useApp();
  const rawNodes = state.systemState?.nodes ?? [];

  //
  // Track the set of node IDs to detect additions/removals.
  //
  const nodeIds = rawNodes.map(n => n.node_id).sort().join(',');

  //
  // Store the sorted order of node IDs - only update when nodes are
  // added/removed.
  //
  const sortedNodeIdsRef = useRef<string[]>([]);

  //
  // Update the sort order only when the set of node IDs changes (not when
  // properties change).
  //
  useMemo(() => {
    const sorted = [...rawNodes].sort(
      (a, b) => new Date(b.last_update).getTime() - new Date(a.last_update).getTime()
    );
    sortedNodeIdsRef.current = sorted.map(n => n.node_id);
    //
    // eslint-disable-next-line react-hooks/exhaustive-deps.
    //
  }, [nodeIds]);

  //
  // Get current node data in the stable sorted order.
  //
  const nodes = useMemo(() => {
    const nodeMap = new Map(rawNodes.map(n => [n.node_id, n]));
    return sortedNodeIdsRef.current
      .map(id => nodeMap.get(id))
      .filter((n): n is NonNullable<typeof n> => n !== undefined);
  }, [rawNodes]);

  const formatLastSeen = (timestamp: string) => {
    const date = new Date(timestamp);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffSecs = Math.floor(diffMs / 1000);
    const diffMins = Math.floor(diffSecs / 60);

    if (diffSecs < 60) return `${diffSecs}s ago`;
    if (diffMins < 60) return `${diffMins}m ago`;
    return date.toLocaleTimeString();
  };

  return (
    <div className="space-y-6">
      {/*
      //
      // Page header.
      //
      */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
        <div>
          <h1 className="text-2xl font-bold text-highlight">Nodes</h1>
          <p className="text-muted mt-1">
            <span className="text-highlight font-medium">{nodes.length}</span> node{nodes.length !== 1 ? 's' : ''} connected
          </p>
        </div>
        <div className="flex items-center gap-2 sm:gap-4">
          {nodes.some(n => getNodeStatus(n.last_update) === 'offline') && (
            <button
              onClick={() => {
                nodes
                  .filter(n => getNodeStatus(n.last_update) === 'offline')
                  .forEach(n => removeNode(n.node_id));
              }}
              className="flex items-center gap-1.5 text-[10px] px-2 py-1 border border-subtle text-muted hover:text-[var(--accent-error)] hover:border-[var(--accent-error)]/50 hover:bg-[var(--accent-error)]/10 rounded transition-colors"
            >
              <Trash2 size={10} />
              Clear expired
            </button>
          )}
        </div>
      </div>

      {/*
      //
      // Nodes list.
      //
      */}
      {nodes.length === 0 ? (
        <div className="bg-card border border-subtle ascii-box p-12 text-center">
          <Server size={48} className="mx-auto mb-4 text-muted opacity-50" />
          <h2 className="text-title font-semibold text-sm mb-2">NO NODES CONNECTED</h2>
          <p className="text-xs text-muted">
            Start a Praxis node to see it appear here
          </p>
        </div>
      ) : (
        <>
        <div className="md:hidden space-y-3">
          {nodes.map((node) => (
            <div
              key={node.node_id}
              onClick={() => navigate(`/nodes/${node.node_id}`)}
              className="border border-subtle ascii-box p-3 bg-card cursor-pointer"
            >
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <p className="font-medium text-highlight truncate">{node.machine_name || 'Unknown'}</p>
                  <p className="text-muted text-xs mt-0.5 truncate">{node.os_details}</p>
                  <p className="text-muted font-mono text-[10px] mt-1 truncate">{node.node_id}</p>
                  <div className="mt-2">
                    <StatusBadge status={getNodeStatus(node.last_update)} />
                  </div>
                </div>
                <div onClick={(e) => e.stopPropagation()}>
                  <button
                    onClick={() => removeNode(node.node_id)}
                    className="p-1 hover:bg-[var(--accent-error)]/10 text-muted hover:text-[var(--accent-error)] transition-colors"
                    title="Remove node"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>

              <div className="mt-2 grid grid-cols-2 gap-2 text-xs">
                <div className="text-muted">
                  Agents:{' '}
                  <span className="text-highlight">
                    {node.discovered_agents.filter((a) => a.available).length}
                  </span>
                </div>
                <div className="text-muted">
                  Session:{' '}
                  <span className={node.selected_agent?.session_id ? 'text-[var(--accent-success)]' : 'text-muted'}>
                    {node.selected_agent?.session_id ? node.selected_agent.short_name : '-'}
                  </span>
                </div>
                <div className="text-muted">
                  Intercept:{' '}
                  <span className="text-title">
                    {!node.intercept_supported ? 'Unsupported' : node.intercept_active ? 'Active' : '-'}
                  </span>
                </div>
                <div className="text-muted flex items-center gap-1">
                  <Clock size={11} />
                  <span>{formatLastSeen(node.last_update)}</span>
                </div>
              </div>

            </div>
          ))}
        </div>

        <div className="hidden md:block border border-subtle ascii-box overflow-x-auto">
          <table className="w-full min-w-[820px] text-xs">
            <thead>
              <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
                <th className="text-left px-4 py-2 text-muted tracking-wider">NODE</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">OS</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">AGENTS</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">SESSION</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">INTERCEPT</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">LAST SEEN</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">STATUS</th>
                <th className="px-4 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {nodes.map((node) => (
                <tr
                  key={node.node_id}
                  onClick={() => navigate(`/nodes/${node.node_id}`)}
                  className="border-b border-dim last:border-0 hover:bg-[var(--highlight)] transition-colors cursor-pointer group"
                >
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-3">
                      <Server size={14} className="text-muted group-hover:text-[var(--accent-info)]" />
                      <div>
                        <p className="font-medium text-highlight group-hover:text-[var(--accent-info)]">
                          {node.machine_name || 'Unknown'}
                        </p>
                        <p className="text-muted font-mono">{node.node_id.slice(0, 12)}...</p>
                      </div>
                    </div>
                  </td>
                  <td className="px-4 py-3 text-muted">{node.os_details}</td>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-2">
                      <Bot size={12} className="text-muted" />
                      <span>
                        {node.discovered_agents.filter((a) => a.available).length}
                      </span>
                    </div>
                  </td>
                  <td className="px-4 py-3">
                    {node.selected_agent?.session_id ? (
                      <span className="text-[var(--accent-success)]">
                        {node.selected_agent.short_name}
                      </span>
                    ) : (
                      <span className="text-muted">-</span>
                    )}
                  </td>
                  <td className="px-4 py-3">
                    {!node.intercept_supported ? (
                      <span className="text-muted opacity-50">UNSUPPORTED</span>
                    ) : node.intercept_active ? (
                      <span className="flex items-center gap-1 text-[var(--accent-warning)]">
                        <Shield size={12} /> Active
                      </span>
                    ) : (
                      <span className="text-muted">-</span>
                    )}
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-1 text-muted">
                      <Clock size={12} />
                      {formatLastSeen(node.last_update)}
                    </div>
                  </td>
                  <td className="px-4 py-3">
                    <StatusBadge status={getNodeStatus(node.last_update)} />
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-2" onClick={(e) => e.stopPropagation()}>
                      <button
                        onClick={() => removeNode(node.node_id)}
                        className="p-1 hover:bg-[var(--accent-error)]/10 text-muted hover:text-[var(--accent-error)] transition-colors"
                      >
                        <Trash2 size={14} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        </>
      )}
    </div>
  );
}
