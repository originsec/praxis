import { Wifi, WifiOff, RefreshCw } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { useLocation } from 'react-router-dom';

export function Header() {
  const { state } = useApp();
  const location = useLocation();
  const nodeCount = state.systemState?.nodes.length ?? 0;
  const runningOps = state.operations.filter((op) => op.status === 'Running').length;

  //
  // Get current page title.
  //
  const getPageTitle = () => {
    const path = location.pathname;
    if (path === '/') return 'DASHBOARD';
    if (path.startsWith('/nodes/') && path.includes('/agents/')) return 'AGENT SESSION';
    if (path.startsWith('/nodes/')) return 'NODE DETAILS';
    if (path === '/nodes') return 'NODES';
    if (path === '/nexus') return 'NEXUS';
    if (path === '/operations') return 'OPERATIONS';
    if (path === '/events') return 'EVENTS';
    if (path === '/settings') return 'SETTINGS';
    return 'PRAXIS';
  };

  return (
    <header className="h-10 bg-[var(--bg-secondary)] border-b border-subtle flex items-center justify-between px-4">
      {/*
      //
      // Left side - page title.
      //
      */}
      <div className="flex items-center gap-4">
        <span className="text-xs text-muted tracking-wider">{getPageTitle()}</span>
      </div>

      {/*
      //
      // Right side - status.
      //
      */}
      <div className="flex items-center gap-6">
        {/*
        //
        // Stats.
        //
        */}
        <div className="flex items-center gap-4 text-xs">
          <div className="flex items-center gap-2">
            <span className="text-muted">NODES:</span>
            <span className="text-highlight font-medium">{nodeCount}</span>
          </div>
          {runningOps > 0 && (
            <div className="flex items-center gap-2">
              <RefreshCw size={12} className="animate-spin text-[var(--accent-info)]" />
              <span className="text-muted">OPS:</span>
              <span className="text-highlight font-medium">{runningOps}</span>
            </div>
          )}
        </div>

        {/*
        //
        // Connection status.
        //
        */}
        <div className="flex items-center gap-2 text-xs">
          {state.connected ? (
            <>
              <Wifi size={12} className="status-online" />
              <span className="status-online tracking-wider">ONLINE</span>
            </>
          ) : (
            <>
              <WifiOff size={12} className="status-offline" />
              <span className="status-offline tracking-wider">OFFLINE</span>
            </>
          )}
        </div>
      </div>
    </header>
  );
}
