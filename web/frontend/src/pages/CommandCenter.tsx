import { useState, useEffect, useMemo } from 'react';
import { Server, AlertCircle } from 'lucide-react';
import { useApp } from '../context/AppContext';
import { CommandTopBar } from '../components/command/CommandTopBar';
import { NodeCard } from '../components/command/NodeCard';
import { OrchestratorPanel } from '../components/command/OrchestratorPanel';
import { ActivityBar } from '../components/command/ActivityBar';
import { getNodeStatus } from '../components/common/StatusBadge';

const ORCHESTRATOR_PANEL_KEY = 'commandCenter.orchestratorOpen';

export function CommandCenter() {
  const { state, requestOperations, requestChainExecutions } = useApp();

  const [orchestratorOpen, setOrchestratorOpen] = useState(() => {
    const stored = localStorage.getItem(ORCHESTRATOR_PANEL_KEY);
    return stored !== null ? stored === 'true' : true;
  });

  const toggleOrchestrator = () => {
    setOrchestratorOpen(prev => {
      localStorage.setItem(ORCHESTRATOR_PANEL_KEY, String(!prev));
      return !prev;
    });
  };

  //
  // Fetch operations and chain executions on connect.
  //
  useEffect(() => {
    if (state.connected) {
      requestOperations();
      requestChainExecutions();
    }
  }, [state.connected, requestOperations, requestChainExecutions]);

  //
  // Sort nodes: online first, then by machine name.
  //
  const sortedNodes = useMemo(() => {
    const nodes = state.systemState?.nodes ?? [];
    return [...nodes].sort((a, b) => {
      const statusOrder = { online: 0, warning: 1, offline: 2 };
      const aStatus = statusOrder[getNodeStatus(a.last_update)];
      const bStatus = statusOrder[getNodeStatus(b.last_update)];
      if (aStatus !== bStatus) return aStatus - bStatus;
      return (a.machine_name || '').localeCompare(b.machine_name || '');
    });
  }, [state.systemState?.nodes]);

  return (
    <div className="flex flex-col h-screen overflow-hidden">
      <CommandTopBar
        orchestratorOpen={orchestratorOpen}
        onToggleOrchestrator={toggleOrchestrator}
      />

      <div className="flex flex-1 min-h-0">
        {/*
        //
        // Main content area — node cards grid.
        //
        */}
        <div className="flex-1 flex flex-col min-h-0 min-w-0">
          <div className="flex-1 overflow-auto p-4">
            {!state.connected ? (
              <div className="flex items-center justify-center h-full">
                <div className="bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 p-6 ascii-box max-w-md">
                  <div className="flex items-center gap-3">
                    <AlertCircle className="text-[var(--accent-error)] flex-shrink-0" size={20} />
                    <div>
                      <p className="text-xs font-medium text-[var(--accent-error)]">CONNECTION LOST</p>
                      <p className="text-xs text-muted mt-1">Attempting to reconnect...</p>
                    </div>
                  </div>
                </div>
              </div>
            ) : !state.systemState ? (
              <div className="flex items-center justify-center h-full">
                <div className="bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/30 p-6 ascii-box max-w-md">
                  <div className="flex items-center gap-3">
                    <AlertCircle className="text-[var(--accent-warning)] flex-shrink-0" size={20} />
                    <div>
                      <p className="text-xs font-medium text-[var(--accent-warning)]">SERVICE UNAVAILABLE</p>
                      <p className="text-xs text-muted mt-1">Connected to web server but Praxis service is not responding...</p>
                    </div>
                  </div>
                </div>
              </div>
            ) : sortedNodes.length === 0 ? (
              <div className="flex items-center justify-center h-full">
                <div className="text-center">
                  <Server size={48} className="mx-auto mb-4 text-muted opacity-50" />
                  <p className="text-muted text-sm">No nodes connected</p>
                  <p className="text-xs text-muted mt-1">Waiting for nodes to check in...</p>
                </div>
              </div>
            ) : (
              <div className="grid gap-4" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 380px))' }}>
                {sortedNodes.map(node => (
                  <NodeCard key={node.node_id} node={node} />
                ))}
              </div>
            )}
          </div>

          <ActivityBar />
        </div>

        {/*
        //
        // Orchestrator panel — collapsible right side.
        //
        */}
        <OrchestratorPanel isOpen={orchestratorOpen} onToggle={toggleOrchestrator} />
      </div>
    </div>
  );
}
