import { useState, useEffect, useRef } from 'react';
import { ScrollText, Pause, Play, Download } from 'lucide-react';
import { useApp } from '../context/AppContext';

export function EventsPage() {
  const { state } = useApp();
  const [isPaused, setIsPaused] = useState(false);
  const [filter, setFilter] = useState('');
  const eventsEndRef = useRef<HTMLDivElement>(null);

  //
  // Use events from global state.
  //
  const events = state.events;

  useEffect(() => {
    if (!isPaused) {
      eventsEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [events, isPaused]);

  const filteredEvents = filter
    ? events.filter(
        (e) =>
          e.message_name.toLowerCase().includes(filter.toLowerCase()) ||
          e.details.toLowerCase().includes(filter.toLowerCase())
      )
    : events;

  const handleExport = () => {
    const data = events.map((e) => `${e.timestamp} [${e.message_name}] ${e.details}`).join('\n');
    const blob = new Blob([data], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `praxis-events-${new Date().toISOString().split('T')[0]}.log`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const getEventColor = (name: string) => {
    if (name.includes('Error') || name.includes('Failed')) return 'text-[var(--accent-error)]';
    if (name.includes('Warning')) return 'text-[var(--accent-warning)]';
    if (name.includes('Registration') || name.includes('Created')) return 'text-[var(--accent-success)]';
    return 'text-[var(--accent-info)]';
  };

  return (
    <div className="p-3 md:p-6 space-y-4 h-full flex flex-col">
      {/*
      //
      // Page header.
      //
      */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
        <div className="flex items-center gap-2 md:gap-3">
          <ScrollText className="text-[var(--accent-success)]" size={20} />
          <h1 className="text-lg font-bold tracking-wider text-highlight">EVENTS</h1>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setIsPaused(!isPaused)}
            className={`flex items-center gap-2 px-3 py-1 text-xs border transition-colors ${
              isPaused
                ? 'border-[var(--accent-warning)] text-[var(--accent-warning)]'
                : 'border-subtle text-muted hover:text-title hover:border-[var(--border-hover)]'
            }`}
          >
            {isPaused ? <Play size={12} /> : <Pause size={12} />}
            {isPaused ? 'RESUME' : 'PAUSE'}
          </button>
          <button
            onClick={handleExport}
            className="flex items-center gap-2 px-3 py-1 text-xs border border-subtle text-muted hover:text-title hover:border-[var(--border-hover)] transition-colors"
          >
            <Download size={12} />
            EXPORT
          </button>
        </div>
      </div>

      {/*
      //
      // Filter.
      //
      */}
      <div className="flex flex-col sm:flex-row sm:items-center gap-3 sm:gap-4 p-3 md:p-4 border border-subtle ascii-box">
        <input
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter events..."
          className="flex-1 bg-transparent border-b border-subtle text-xs text-title px-2 py-1 focus:border-[var(--accent-success)] outline-none"
        />
        <span className="text-xs text-muted">{filteredEvents.length} events</span>
      </div>

      {/*
      //
      // Events list.
      //
      */}
      <div className="flex-1 bg-card border border-subtle ascii-box overflow-hidden min-h-0">
        {filteredEvents.length === 0 ? (
          <div className="h-full flex items-center justify-center">
            <div className="text-center">
              <ScrollText size={48} className="mx-auto mb-4 text-muted opacity-50" />
              <h2 className="text-title font-semibold text-sm mb-2">NO EVENTS</h2>
              <p className="text-xs text-muted">
                {filter ? 'No events match your filter' : 'Waiting for events...'}
              </p>
            </div>
          </div>
        ) : (
          <div className="h-full overflow-auto p-4 font-mono text-xs">
            {filteredEvents.map((event, idx) => (
              <div
                key={idx}
                className="flex flex-col sm:flex-row sm:items-start gap-1 sm:gap-4 py-2 border-b border-dim last:border-0 hover:bg-[var(--bg-secondary)]"
              >
                <span className="text-muted whitespace-nowrap">
                  {new Date(event.timestamp).toLocaleTimeString()}
                </span>
                <span className={`font-medium whitespace-nowrap ${getEventColor(event.message_name)}`}>
                  [{event.message_name}]
                </span>
                <span className="text-[var(--text-primary)] break-all">{event.details}</span>
              </div>
            ))}
            <div ref={eventsEndRef} />
          </div>
        )}
      </div>

      {/*
      //
      // Status bar.
      //
      */}
      <div className="flex items-center justify-between text-xs text-muted">
        <span>Real-time system event log</span>
        {isPaused && (
          <span className="text-[var(--accent-warning)]">Auto-scroll paused</span>
        )}
      </div>
    </div>
  );
}
