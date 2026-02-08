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
  const [isMobileNavOpen, setIsMobileNavOpen] = useState(false);
  const eventLoggingEnabled = (() => {
    const value = state.config.application_logs_enabled;
    if (!value) return false;
    const normalized = value.toLowerCase();
    return !(normalized === 'false' || normalized === '0' || normalized === 'no');
  })();

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
      <div className="hidden md:block">
        <Sidebar />
      </div>

      {isMobileNavOpen && (
        <button
          className="md:hidden fixed inset-0 z-40 bg-black/50"
          onClick={() => setIsMobileNavOpen(false)}
          aria-label="Close navigation"
        />
      )}

      <div
        className={`md:hidden fixed left-0 top-0 z-50 h-full transition-transform duration-200 ${
          isMobileNavOpen ? "translate-x-0" : "-translate-x-full"
        }`}
      >
        <Sidebar onNavigate={() => setIsMobileNavOpen(false)} />
      </div>

      <div className="flex-1 flex flex-col overflow-hidden">
        <Header onOpenMobileNav={() => setIsMobileNavOpen(true)} />
        <VersionUpdateBanner />
        <ConfigWarningBanner />

        {/*
        //
        // Main content area - shrinks when event log is open.
        //
        */}
        <main
          className="flex-1 overflow-auto p-4 md:p-6"
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
                <span
                  className={`text-[10px] px-2 py-0.5 border rounded-full tracking-wider ${
                    eventLoggingEnabled
                      ? 'text-[var(--accent-success)] border-[var(--accent-success)]/40 bg-[var(--accent-success)]/10'
                      : 'text-[var(--accent-error)] border-[var(--accent-error)]/40 bg-[var(--accent-error)]/10'
                  }`}
                  title={eventLoggingEnabled ? 'Centralized logging enabled' : 'Centralized logging disabled'}
                >
                  {eventLoggingEnabled ? 'LOGGING ON' : 'LOGGING OFF'}
                </span>
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
