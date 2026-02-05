import { useEffect } from 'react';
import { Radar, RefreshCw } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { DiscoveryTable } from './DiscoveryTable';
import type { NodeState } from '../../api/types';

interface NodeDiscoveryTabProps {
  node: NodeState;
}

export function NodeDiscoveryTab({ node }: NodeDiscoveryTabProps) {
  const {
    state,
    enableAgentDiscovery,
    disableAgentDiscovery,
    requestDiscoveredEndpoints,
    clearDiscoveryError,
  } = useApp();

  //
  // Filter endpoints for this node.
  //
  const nodeEndpoints = state.discovery.endpoints.filter(
    (e) => e.node_id === node.node_id
  );

  //
  // Fetch endpoints when tab is mounted.
  //
  useEffect(() => {
    requestDiscoveredEndpoints(node.node_id);
  }, [node.node_id, requestDiscoveredEndpoints]);

  const handleToggleDiscovery = () => {
    //
    // Clear any existing error before attempting to toggle.
    //
    clearDiscoveryError();
    if (node.agent_discovery_enabled) {
      disableAgentDiscovery(node.node_id);
    } else {
      enableAgentDiscovery(node.node_id);
    }
  };

  const handleRefresh = () => {
    requestDiscoveredEndpoints(node.node_id);
  };

  return (
    <div className="space-y-4">
      {/*
      //
      // Enable/Disable Control.
      //
      */}
      <div className="bg-card ascii-box border border-subtle p-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div
              className={`p-3 ${
                node.agent_discovery_enabled ? 'bg-[var(--accent-info)]/20' : 'bg-[var(--bg-secondary)]'
              }`}
            >
              <Radar
                size={24}
                className={node.agent_discovery_enabled ? 'text-[var(--accent-info)]' : 'text-muted'}
              />
            </div>
            <div>
              <h2 className="text-title font-semibold">Agent Discovery</h2>
              <p className="text-muted text-xs mt-1">
                {node.agent_discovery_enabled
                  ? 'Actively probing connections for LLM endpoints'
                  : 'Discovery is disabled - enable to probe for LLM endpoints'}
              </p>
              {!node.intercept_active && (
                <p className="text-[var(--accent-warning)] text-xs mt-1">
                  Note: Proxy must be enabled to discover endpoints
                </p>
              )}
            </div>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={handleRefresh}
              disabled={state.discovery.isLoading}
              className="px-3 py-2 text-sm text-muted hover:text-title border border-subtle hover:border-[var(--border-hover)] transition-colors disabled:opacity-50"
            >
              <RefreshCw size={14} className={state.discovery.isLoading ? 'animate-spin' : ''} />
            </button>
            <button
              onClick={handleToggleDiscovery}
              disabled={!node.intercept_active}
              className={`px-4 py-2 text-sm transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
                node.agent_discovery_enabled
                  ? 'bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30'
                  : 'bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30'
              }`}
            >
              {node.agent_discovery_enabled ? 'Disable' : 'Enable'}
            </button>
          </div>
        </div>
      </div>

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
      // Discovery Results.
      //
      */}
      <div className="bg-card ascii-box border border-subtle overflow-hidden">
        <div className="px-4 py-3 border-b border-subtle bg-[var(--bg-tertiary)] flex items-center justify-between">
          <h3 className="text-title font-semibold">
            Discovered Endpoints
            {nodeEndpoints.length > 0 && (
              <span className="ml-2 text-muted font-normal">({nodeEndpoints.length})</span>
            )}
          </h3>
        </div>
        <DiscoveryTable
          endpoints={nodeEndpoints}
          showNodeColumn={false}
          isLoading={state.discovery.isLoading}
        />
      </div>
    </div>
  );
}
