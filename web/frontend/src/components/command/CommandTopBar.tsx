import { useState } from 'react';
import { Wifi, WifiOff, Sun, Moon, RefreshCw, Settings, PanelRightOpen, PanelRightClose, Search, Plus } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { useTheme } from '../../context/ThemeContext';
import { SettingsModal } from './SettingsModal';
import { AddRemoteNodeModal } from './AddRemoteNodeModal';

interface CommandTopBarProps {
  orchestratorOpen: boolean;
  onToggleOrchestrator: () => void;
  filterText: string;
  onFilterChange: (value: string) => void;
}

export function CommandTopBar({ orchestratorOpen, onToggleOrchestrator, filterText, onFilterChange }: CommandTopBarProps) {
  const { state } = useApp();
  const { isDark, toggleTheme } = useTheme();
  const [showSettings, setShowSettings] = useState(false);
  const [showAddRemote, setShowAddRemote] = useState(false);
  const nodeCount = state.systemState?.nodes.length ?? 0;
  const runningOps = state.operations.filter(op => op.status === 'Running').length;
  const runningChains = state.chains.executions.filter(e => e.status === 'Running').length;
  const activeSessions = (state.systemState?.nodes ?? []).filter(n => n.selected_agent?.session_id).length;

  return (
    <>
      <header className="cc-text-noscale h-10 bg-[var(--bg-secondary)] border-b border-subtle flex items-center justify-between px-4 flex-shrink-0">
        <div className="flex items-center gap-4">
          <span className="text-highlight font-bold tracking-widest text-sm">[&Oslash;] PRAXIS</span>
          <span className="text-[10px] text-muted">v{state.version ?? '?.?.?'}</span>

          <div className="hidden md:flex items-center gap-1 ml-4 text-muted">
            <Search size={10} />
            <input
              type="text"
              value={filterText}
              onChange={e => onFilterChange(e.target.value)}
              placeholder="Filter nodes..."
              className="bg-transparent text-[10px] text-[var(--text-primary)] placeholder:text-muted/50 focus:outline-none w-24 focus:w-40 transition-all"
            />
          </div>
        </div>

        <div className="flex items-center gap-4">

          {/*
          //
          // Activity summary.
          //
          */}

          <div className="hidden md:flex items-center gap-4 text-xs">
            <span className="text-muted">
              <span className="text-highlight font-medium">{nodeCount}</span> nodes
            </span>
            <span className="text-muted">
              <span className="text-highlight font-medium">{activeSessions}</span> sessions
            </span>
            {(runningOps + runningChains) > 0 && (
              <span className="flex items-center gap-1.5 text-[var(--accent-info)]">
                <RefreshCw size={11} className="animate-spin" />
                <span className="font-medium">{runningOps + runningChains}</span> running
              </span>
            )}
          </div>

          {/*
          //
          // Connection status.
          //
          */}

          <div className="flex items-center gap-1.5 text-[10px]">
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
          // Add remote node.
          //
          */}

          <button
            onClick={() => setShowAddRemote(true)}
            className="p-1 rounded hover:bg-[var(--highlight)] transition-colors"
            title="Add Remote Node"
          >
            <Plus size={14} className="text-muted hover:text-primary" />
          </button>

          {/*
          //
          // Orchestrator toggle.
          //
          */}

          <button
            onClick={onToggleOrchestrator}
            className="p-1 rounded hover:bg-[var(--highlight)] transition-colors"
            title={orchestratorOpen ? 'Hide Orchestrator' : 'Show Orchestrator'}
          >
            {orchestratorOpen
              ? <PanelRightClose size={14} className="text-muted hover:text-primary" />
              : <PanelRightOpen size={14} className="text-muted hover:text-primary" />}
          </button>

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
            {isDark
              ? <Sun size={14} className="text-muted hover:text-primary" />
              : <Moon size={14} className="text-muted hover:text-primary" />}
          </button>

          {/*
          //
          // Settings button.
          //
          */}

          <button
            onClick={() => setShowSettings(true)}
            className="p-1 rounded hover:bg-[var(--highlight)] transition-colors"
            title="Settings"
          >
            <Settings size={14} className="text-muted hover:text-primary" />
          </button>
        </div>
      </header>

      {showSettings && <SettingsModal onClose={() => setShowSettings(false)} />}
      <AddRemoteNodeModal isOpen={showAddRemote} onClose={() => setShowAddRemote(false)} />
    </>
  );
}
