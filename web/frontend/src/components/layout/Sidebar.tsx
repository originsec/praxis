import { NavLink } from 'react-router-dom';
import {
  LayoutDashboard,
  Server,
  Bot,
  Zap,
  Shield,
  Crosshair,
  MessageSquare,
  // Radar,  // Hidden - Discovery feature not ready
  Settings,
  Wrench,
} from 'lucide-react';
import { useApp } from '../../context/AppContext';

interface SidebarProps {
  onNavigate?: () => void;
}

export function Sidebar({ onNavigate }: SidebarProps) {
  const { state } = useApp();
  const nodes = state.systemState?.nodes ?? [];

  const navItems = [
    { to: '/', icon: LayoutDashboard, label: 'DASHBOARD', end: true },
    { to: '/nodes', icon: Server, label: 'NODES', end: false },
    { to: '/operations', icon: Zap, label: 'OPERATIONS', end: false },
    { to: '/intercept', icon: Shield, label: 'INTERCEPT', end: false },
    { to: '/hunting', icon: Crosshair, label: 'HUNTING', end: false },
    // { to: '/discovery', icon: Radar, label: 'DISCOVERY', end: false },  // Hidden - feature not ready
    { to: '/orchestrator', icon: Bot, label: 'ORCHESTRATOR', end: false },
    { to: '/agent-chat', icon: MessageSquare, label: 'AGENT CHAT', end: false },
    { to: '/toolkit', icon: Wrench, label: 'TOOLKIT', end: false },
    { to: '/settings', icon: Settings, label: 'SETTINGS', end: false },
  ];

  return (
    <div className="w-64 h-full flex flex-col border-r border-subtle bg-[var(--bg-secondary)]">
      {/*
      //
      // Logo.
      //
      */}
      <div className="p-4 border-b border-subtle">
        <div className="flex items-center justify-between">
          <span className="text-highlight font-bold tracking-widest text-sm">[Ø] PRAXIS</span>
          <span className="text-[10px] text-muted">v{state.version ?? '?.?.?'}</span>
        </div>
        <div className="text-[10px] text-muted tracking-wider mt-1">Command & Control</div>
      </div>

      {/*
      //
      // Navigation.
      //
      */}
      <nav className="flex-1 p-2 space-y-1">
        {navItems.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            end={item.end}
            onClick={onNavigate}
            className={({ isActive }) =>
              `flex items-center gap-3 px-3 py-2 text-xs tracking-wider transition-colors ${
                isActive
                  ? 'text-title bg-[var(--highlight)] border-l-2 border-[var(--accent-success)]'
                  : 'text-muted hover:text-title hover:bg-[var(--highlight)]'
              }`
            }
          >
            <item.icon size={14} />
            {item.label}
          </NavLink>
        ))}
      </nav>

      {/*
      //
      // Recent Nodes Panel.
      //
      */}
      <div className="border-t border-subtle">
        <div className="px-3 py-2 text-xs text-muted tracking-wider">
          <span>RECENT NODES</span>
        </div>
        <div className="px-2 pb-2">
          {state.recentlyAccessedNodeIds
            .map(nodeId => nodes.find(n => n.node_id === nodeId))
            .filter((node): node is NonNullable<typeof node> => node !== undefined)
            .map((node) => (
            <NavLink
              key={node.node_id}
              to={`/nodes/${node.node_id}`}
              onClick={onNavigate}
              className={({ isActive }) =>
                `block mb-1 p-2 border border-subtle text-xs transition-colors ascii-box ${
                  isActive
                    ? 'border-[var(--accent-success)]'
                    : 'hover:border-[var(--border-hover)]'
                }`
              }
            >
              <div className="flex items-center gap-2">
                <span className="text-title">●</span>
                <span className="font-medium truncate text-highlight">{node.machine_name || 'Unknown'}</span>
              </div>
              <div className="text-muted mt-1 flex items-center gap-1">
                <span className="label-upper">OS</span>
                <span>{node.os_details?.split(' ')[0] || 'unknown'}</span>
              </div>
              <div className="text-muted font-mono text-[10px] truncate">
                <span className="label-upper">ID</span> {node.node_id.slice(0, 20)}...
              </div>
              {node.selected_agent && (
                <div className="mt-1 flex items-center gap-1 text-title">
                  <span className="text-[10px]">▶ [{node.node_id.slice(0, 8)}] {node.selected_agent.short_name}</span>
                </div>
              )}
            </NavLink>
          ))}
          {nodes.length === 0 && (
            <div className="px-3 py-4 text-xs text-muted text-center border border-subtle ascii-box">
              No nodes connected
            </div>
          )}
        </div>
      </div>

      {/*
      //
      // Footer.
      //
      */}
      <div className="p-3 border-t border-subtle text-center">
        <span className="footer-text">©2026 ORIGIN</span>
      </div>
    </div>
  );
}
