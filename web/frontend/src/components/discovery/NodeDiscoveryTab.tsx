import { useState, useEffect } from 'react';
import { Radar, RefreshCw, X } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { DiscoveryTable } from './DiscoveryTable';
import type { NodeState, DiscoveredLlmEndpoint } from '../../api/types';

interface NodeDiscoveryTabProps {
  node: NodeState;
}

export function NodeDiscoveryTab({ node }: NodeDiscoveryTabProps) {
  const {
    state,
    enableAgentDiscovery,
    disableAgentDiscovery,
    requestDiscoveredEndpoints,
    createDynamicAgent,
    clearDiscoveryError,
  } = useApp();

  const [showCreateModal, setShowCreateModal] = useState(false);
  const [selectedEndpoint, setSelectedEndpoint] = useState<DiscoveredLlmEndpoint | null>(null);
  const [agentName, setAgentName] = useState('');
  const [shortName, setShortName] = useState('');

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

  const handleCreateAgent = (endpoint: DiscoveredLlmEndpoint) => {
    setSelectedEndpoint(endpoint);
    //
    // Pre-fill agent name from domain or IP.
    //
    const baseName = endpoint.domain || endpoint.ip_address;
    setAgentName(`${baseName} Agent`);
    setShortName(baseName.replace(/[^a-zA-Z0-9]/g, '_').toLowerCase().slice(0, 20));
    setShowCreateModal(true);
  };

  const handleConfirmCreate = () => {
    if (selectedEndpoint && agentName && shortName) {
      createDynamicAgent(node.node_id, selectedEndpoint.id, agentName, shortName);
      setShowCreateModal(false);
      setSelectedEndpoint(null);
      setAgentName('');
      setShortName('');
    }
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
                  ? 'bg-red-500/20 text-[var(--accent-error)] hover:bg-red-500/30'
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
        <div className="bg-red-500/10 border border-red-500/30 p-4 text-[var(--accent-error)] text-sm">
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
          onCreateAgent={handleCreateAgent}
          isLoading={state.discovery.isLoading}
        />
      </div>

      {/*
      //
      // Create Agent Modal.
      //
      */}
      {showCreateModal && selectedEndpoint && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-card ascii-box border border-subtle p-6 max-w-md w-full mx-4">
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-title font-semibold text-lg">Create Dynamic Agent</h3>
              <button
                onClick={() => setShowCreateModal(false)}
                className="text-muted hover:text-title transition-colors"
              >
                <X size={20} />
              </button>
            </div>
            <p className="text-muted text-sm mb-4">
              Create an agent from the discovered endpoint at{' '}
              <span className="font-mono text-highlight">{selectedEndpoint.base_url}</span>
            </p>
            <div className="space-y-4">
              <div>
                <label className="block text-sm text-muted mb-1">Agent Name</label>
                <input
                  type="text"
                  value={agentName}
                  onChange={(e) => setAgentName(e.target.value)}
                  className="w-full px-3 py-2 bg-[var(--bg-secondary)] border border-subtle text-title focus:border-[var(--accent-info)] outline-none"
                  placeholder="My LLM Agent"
                />
              </div>
              <div>
                <label className="block text-sm text-muted mb-1">Short Name (identifier)</label>
                <input
                  type="text"
                  value={shortName}
                  onChange={(e) => setShortName(e.target.value.replace(/[^a-zA-Z0-9_]/g, '').toLowerCase())}
                  className="w-full px-3 py-2 bg-[var(--bg-secondary)] border border-subtle text-title font-mono focus:border-[var(--accent-info)] outline-none"
                  placeholder="my_llm_agent"
                />
                <p className="text-xs text-muted mt-1">Lowercase letters, numbers, and underscores only</p>
              </div>
              {selectedEndpoint.models.length > 0 && (
                <div>
                  <label className="block text-sm text-muted mb-1">Available Models</label>
                  <div className="flex flex-wrap gap-1">
                    {selectedEndpoint.models.slice(0, 5).map((model) => (
                      <span
                        key={model}
                        className="px-2 py-0.5 bg-[var(--bg-tertiary)] text-muted text-xs"
                      >
                        {model}
                      </span>
                    ))}
                    {selectedEndpoint.models.length > 5 && (
                      <span className="text-muted text-xs">+{selectedEndpoint.models.length - 5} more</span>
                    )}
                  </div>
                </div>
              )}
              {!selectedEndpoint.api_key && (
                <div className="bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/30 p-3 text-xs text-[var(--accent-warning)]">
                  No API key was detected. The agent may not work without credentials.
                </div>
              )}
            </div>
            <div className="flex justify-end gap-3 mt-6">
              <button
                onClick={() => setShowCreateModal(false)}
                className="px-4 py-2 text-sm text-muted hover:text-title transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleConfirmCreate}
                disabled={!agentName || !shortName}
                className="px-4 py-2 text-sm bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                Create Agent
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
