import { Wifi, WifiOff, RefreshCw, Sun, Moon, Menu } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { useTheme } from '../../context/ThemeContext';
import { useLocation } from 'react-router-dom';

interface HeaderProps {
  onOpenMobileNav?: () => void;
}

export function Header({ onOpenMobileNav }: HeaderProps) {
  const { state } = useApp();
  const { isDark, toggleTheme } = useTheme();
  const location = useLocation();
  const nodeCount = state.systemState?.nodes.length ?? 0;
  const runningOps = state.operations.filter((op) => op.status === 'Running').length;

  //
  // Get current page title.
  //
  const getPageTitle = () => {
    const path = location.pathname;
    if (path === '/') return 'COMMAND CENTER';
    if (path === '/dashboard') return 'DASHBOARD';
    if (path.startsWith('/nodes/') && path.includes('/agents/')) return 'AGENT SESSION';
    if (path.startsWith('/nodes/')) return 'NODE DETAILS';
    if (path === '/nodes') return 'NODES';
    if (path === '/orchestrator') return 'ORCHESTRATOR';
    if (path === '/agent-chat') return 'AGENT CHAT';
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
      <div className="flex items-center gap-2 md:gap-4">
        {onOpenMobileNav && (
          <button
            onClick={onOpenMobileNav}
            className="md:hidden p-1 rounded hover:bg-[var(--highlight)] transition-colors"
            title="Open navigation"
          >
            <Menu size={16} className="text-muted" />
          </button>
        )}
        <span className="text-xs text-muted tracking-wider">{getPageTitle()}</span>
      </div>

      {/*
      //
      // Right side - status.
      //
      */}
      <div className="flex items-center gap-2 md:gap-6">
        {/*
        //
        // Stats.
        //
        */}
        <div className="hidden md:flex items-center gap-4 text-xs">
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
        <div className="flex items-center gap-1.5 md:gap-2 text-[10px] md:text-xs">
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

        {/*
        //
        // Theme toggle.
        //
        */}
        <button
          onClick={toggleTheme}
          className="p-1 rounded hover:bg-[var(--highlight)] transition-colors"
          title={isDark ? 'Switch to light theme' : 'Switch to dark theme'}
        >
          {isDark ? (
            <Sun size={14} className="text-muted hover:text-primary" />
          ) : (
            <Moon size={14} className="text-muted hover:text-primary" />
          )}
        </button>
      </div>
    </header>
  );
}
