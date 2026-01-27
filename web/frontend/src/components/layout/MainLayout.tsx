import { Outlet } from 'react-router-dom';
import { useState, useEffect } from 'react';
import { ScrollText, X } from 'lucide-react';
import { Sidebar } from './Sidebar';
import { Header } from './Header';
import { ConfigWarningBanner } from './ConfigWarningBanner';
import { VersionUpdateBanner } from './VersionUpdateBanner';
import { useApp } from '../../context/AppContext';
import { GlobalEventLogPanel } from '../event-log/GlobalEventLogPanel';

export function MainLayout() {
  const { state, toggleEventLogPanel, setEventLogPanelHeight } = useApp();
  const [isResizing, setIsResizing] = useState(false);

  //
  // Handle event log panel resizing.
  //
  useEffect(() => {
    if (!isResizing) return;

    const handleMouseMove = (e: MouseEvent) => {
      const newHeight = window.innerHeight - e.clientY;
      setEventLogPanelHeight(Math.max(150, Math.min(newHeight, window.innerHeight - 200)));
    };

    const handleMouseUp = () => {
      setIsResizing(false);
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isResizing, setEventLogPanelHeight]);

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar />
      <div className="flex-1 flex flex-col overflow-hidden">
        <Header />
        <VersionUpdateBanner />
        <ConfigWarningBanner />

        {/*
        //
        // Main content area - shrinks when event log is open.
        //
        */}
        <main
          className="flex-1 overflow-auto p-6"
          style={state.eventLogPanel.isOpen ? {
            height: `calc(100% - ${state.eventLogPanel.height}px)`
          } : undefined}
        >
          <Outlet />
        </main>

        {/*
        //
        // Event Log Panel (bottom of page, resizable, pushes content up).
        //
        */}
        {state.eventLogPanel.isOpen && (
          <div
            className="bg-card border-t border-subtle"
            style={{ height: `${state.eventLogPanel.height}px`, flexShrink: 0 }}
          >
            {/*
            //
            // Resize handle.
            //
            */}
            <div
              className="h-0.5 cursor-ns-resize hover:bg-[var(--accent-info)] bg-[var(--accent-info)]/20 transition-colors relative group"
              onMouseDown={() => setIsResizing(true)}
              title="Drag to resize"
            >
              <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-12 h-0.5 bg-[var(--accent-info)]/60 rounded-full group-hover:bg-[var(--accent-info)]" />
            </div>

            {/*
            //
            // Panel header.
            //
            */}
            <div className="flex items-center justify-between px-4 py-2.5 border-b border-subtle bg-[var(--bg-tertiary)]">
              <div className="flex items-center gap-2">
                <ScrollText size={16} className="text-[var(--accent-info)]" />
                <h3 className="text-sm font-semibold text-title">Event Log</h3>
              </div>
              <button
                onClick={toggleEventLogPanel}
                className="text-muted hover:text-[var(--text-primary)] transition-colors"
                title="Close Event Log"
              >
                <X size={16} />
              </button>
            </div>

            {/*
            //
            // Panel content.
            //
            */}
            <div className="overflow-auto" style={{ height: `${state.eventLogPanel.height - 44}px` }}>
              <GlobalEventLogPanel />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
