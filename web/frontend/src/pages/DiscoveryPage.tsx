import { useState, useEffect } from 'react';
import { RefreshCw, Filter } from 'lucide-react';
import { useApp } from '../context/AppContext';
import { DiscoveryTable } from '../components/discovery';

export function DiscoveryPage() {
  const {
    state,
    requestDiscoveredEndpoints,
  } = useApp();

  const [nodeFilter, setNodeFilter] = useState<string | null>(null);

  //
  // Fetch all endpoints when page loads.
  //
  useEffect(() => {
    requestDiscoveredEndpoints();
  }, [requestDiscoveredEndpoints]);

  //
  // Filter endpoints by node if selected.
  //
  const filteredEndpoints = nodeFilter
    ? state.discovery.endpoints.filter((e) => e.node_id === nodeFilter)
    : state.discovery.endpoints;

  //
  // Get unique nodes from endpoints.
  //
  const uniqueNodeIds = [...new Set(state.discovery.endpoints.map((e) => e.node_id))];

  //
  // Get node names from state.
  //
  const getNodeName = (nodeId: string) => {
    const node = state.systemState?.nodes.find((n) => n.node_id === nodeId);
    return node?.machine_name || nodeId.slice(0, 8) + '...';
  };

  const handleRefresh = () => {
    requestDiscoveredEndpoints();
  };

  return (
    <div className="space-y-6">
      {/*
      //
      // Header.
      //
      */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-highlight">Agent Discovery</h1>
          <p className="text-muted mt-1">Discovered LLM endpoints across all nodes</p>
        </div>
        <button
          onClick={handleRefresh}
          disabled={state.discovery.isLoading}
          className="flex items-center gap-2 px-4 py-2 text-sm text-muted hover:text-title border border-subtle hover:border-[var(--border-hover)] transition-colors disabled:opacity-50"
        >
          <RefreshCw size={14} className={state.discovery.isLoading ? 'animate-spin' : ''} />
          Refresh
        </button>
      </div>

      {/*
      //
      // Filter bar.
      //
      */}
      {uniqueNodeIds.length > 1 && (
        <div className="flex items-center gap-4">
          <div className="flex items-center gap-2 text-sm text-muted">
            <Filter size={14} />
            Filter by node:
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              onClick={() => setNodeFilter(null)}
              className={`px-3 py-1 text-xs transition-colors ${
                nodeFilter === null
                  ? 'bg-[var(--accent-info)]/20 text-[var(--accent-info)]'
                  : 'bg-[var(--bg-secondary)] text-muted hover:text-title'
              }`}
            >
              All Nodes
            </button>
            {uniqueNodeIds.map((nodeId) => (
              <button
                key={nodeId}
                onClick={() => setNodeFilter(nodeId)}
                className={`px-3 py-1 text-xs transition-colors ${
                  nodeFilter === nodeId
                    ? 'bg-[var(--accent-info)]/20 text-[var(--accent-info)]'
                    : 'bg-[var(--bg-secondary)] text-muted hover:text-title'
                }`}
              >
                {getNodeName(nodeId)}
              </button>
            ))}
          </div>
        </div>
      )}

      {/*
      //
      // Error display.
      //
      */}
      {state.discovery.error && (
        <div className="bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 p-4 text-[var(--accent-error)] text-sm">
          {state.discovery.error}
        </div>
      )}

      {/*
      //
      // Discovery Table.
      //
      */}
      <div className="bg-card ascii-box border border-subtle overflow-hidden">
        <div className="px-4 py-3 border-b border-subtle bg-[var(--bg-tertiary)] flex items-center justify-between">
          <h3 className="text-title font-semibold">
            Discovered Endpoints
            {filteredEndpoints.length > 0 && (
              <span className="ml-2 text-muted font-normal">({filteredEndpoints.length})</span>
            )}
          </h3>
        </div>
        <DiscoveryTable
          endpoints={filteredEndpoints}
          showNodeColumn={true}
          isLoading={state.discovery.isLoading}
        />
      </div>
    </div>
  );
}
