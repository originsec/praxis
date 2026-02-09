import { useState, useEffect, useCallback } from 'react';
import { Search, RefreshCw, Trash2, AlertCircle, AlertTriangle, Info, Bug, FileText } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import type { ApplicationLogEntry, NodeState } from '../../api/types';

const LOG_LEVELS = ['error', 'warn', 'info', 'debug', 'trace'] as const;
type LogLevel = typeof LOG_LEVELS[number];

const LEVEL_CONFIG: Record<LogLevel, { icon: React.ReactNode; color: string; bgColor: string }> = {
  error: { icon: <AlertCircle size={14} />, color: 'text-[var(--accent-error)]', bgColor: 'bg-[var(--accent-error)]/10' },
  warn: { icon: <AlertTriangle size={14} />, color: 'text-[var(--accent-warning)]', bgColor: 'bg-[var(--accent-warning)]/10' },
  info: { icon: <Info size={14} />, color: 'text-[var(--accent-info)]', bgColor: 'bg-[var(--accent-info)]/10' },
  debug: { icon: <Bug size={14} />, color: 'text-[var(--accent-purple)]', bgColor: 'bg-[var(--accent-purple)]/10' },
  trace: { icon: <FileText size={14} />, color: 'text-[var(--text-muted)]', bgColor: 'bg-[var(--text-muted)]/10' },
};

interface ApplicationLogTabProps {
  nodeId: string | null;
  nodes?: NodeState[];
  selectedNodeId?: string;
  onNodeChange?: (nodeId: string) => void;
}

type SourceType = 'service' | 'web' | 'node';

export function ApplicationLogTab({ nodeId, nodes, selectedNodeId, onNodeChange }: ApplicationLogTabProps) {
  const { send } = useApp();
  const [entries, setEntries] = useState<ApplicationLogEntry[]>([]);
  const [totalCount, setTotalCount] = useState(0);
  const [loading, setLoading] = useState(false);
  const [levelFilter, setLevelFilter] = useState<Set<LogLevel>>(new Set(['error', 'warn', 'info']));
  const [sourceFilter, setSourceFilter] = useState<Set<SourceType>>(new Set(['service', 'web', 'node']));
  const [regexFilter, setRegexFilter] = useState('');
  const [regexInput, setRegexInput] = useState('');
  const [offset, setOffset] = useState(0);
  const [autoRefresh, setAutoRefresh] = useState(false);
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const limit = 100;

  //
  // Request event log entries.
  // Always query all sources and filter client-side for flexibility.
  //
  const fetchLogs = useCallback(() => {
    setLoading(true);
    send({
      type: 'application_log_request',
      node_id: '',  // Always query all sources, filter client-side
      level_filter: levelFilter.size > 0 ? Array.from(levelFilter) : null,
      regex_filter: regexFilter || null,
      limit,
      offset,
    });

    //
    // TODO: Wire up response handling via AppContext.
    // For now, simulate a timeout to reset loading state.
    //
    setTimeout(() => setLoading(false), 1000);
  }, [levelFilter, regexFilter, offset, limit, send]);

  //
  // Listen for node event log responses via WebSocket.
  // This is a temporary approach until proper AppContext integration.
  //
  useEffect(() => {
    const handleWsMessage = (event: CustomEvent<{ type: string; node_id?: string; entries?: ApplicationLogEntry[]; total_count?: number; deleted_count?: number }>) => {
      const message = event.detail;
      if (message.type === 'application_log_response') {
        //
        // If nodeId is null (all nodes), accept any response.
        // Otherwise, only accept responses for the selected node.
        //
        if (nodeId === null || message.node_id === nodeId) {
          setEntries(message.entries || []);
          setTotalCount(message.total_count || 0);
          setLoading(false);
        }
      } else if (message.type === 'application_log_cleared') {
        setEntries([]);
        setTotalCount(0);
        setLoading(false);
      }
    };

    window.addEventListener('ws-message' as keyof WindowEventMap, handleWsMessage as EventListener);
    return () => {
      window.removeEventListener('ws-message' as keyof WindowEventMap, handleWsMessage as EventListener);
    };
  }, [nodeId]);

  //
  // Fetch on mount and when filters change.
  //
  useEffect(() => {
    fetchLogs();
  }, [fetchLogs]);

  //
  // Auto-refresh interval.
  //
  useEffect(() => {
    if (!autoRefresh) return;
    const interval = setInterval(fetchLogs, 5000);
    return () => clearInterval(interval);
  }, [autoRefresh, fetchLogs]);

  //
  // Apply regex filter with debounce.
  //
  useEffect(() => {
    const timeout = setTimeout(() => {
      setRegexFilter(regexInput);
      setOffset(0);
    }, 500);
    return () => clearTimeout(timeout);
  }, [regexInput]);

  //
  // Toggle level filter.
  //
  const toggleLevel = (level: LogLevel) => {
    setLevelFilter(prev => {
      const next = new Set(prev);
      if (next.has(level)) {
        next.delete(level);
      } else {
        next.add(level);
      }
      return next;
    });
    setOffset(0);
  };

  //
  // Toggle source filter.
  //
  const toggleSource = (source: SourceType) => {
    setSourceFilter(prev => {
      const next = new Set(prev);
      if (next.has(source)) {
        next.delete(source);
      } else {
        next.add(source);
      }
      return next;
    });
    setOffset(0);
  };

  //
  // Determine source type from entry.
  //
  const getSourceType = (source: string): SourceType => {
    if (source === 'service') return 'service';
    if (source === 'web') return 'web';
    return 'node';
  };

  //
  // Filter entries by source type, node selection, level, and regex.
  //
  const filteredEntries = entries.filter(entry => {
    const sourceType = getSourceType(entry.source);

    //
    // Check if this source type is enabled.
    //
    if (!sourceFilter.has(sourceType)) {
      return false;
    }

    //
    // Check if this log level is enabled.
    //
    if (levelFilter.size > 0 && !levelFilter.has(entry.level as LogLevel)) {
      return false;
    }

    //
    // If a specific node is selected and this is a node log, only show logs
    // from that node.
    //
    if (nodeId !== null && sourceType === 'node' && entry.source !== nodeId) {
      return false;
    }

    //
    // Apply regex filter if present.
    //
    if (regexFilter) {
      try {
        const regex = new RegExp(regexFilter, 'i');
        const searchText = `${entry.level} ${entry.target || ''} ${entry.message}`.toLowerCase();
        if (!regex.test(searchText)) {
          return false;
        }
      } catch {
        //
        // Invalid regex, use literal match.
        //
        const filterLower = regexFilter.toLowerCase();
        const searchText = `${entry.level} ${entry.target || ''} ${entry.message}`.toLowerCase();
        if (!searchText.includes(filterLower)) {
          return false;
        }
      }
    }

    return true;
  });

  //
  // Clear logs.
  //
  const handleClear = () => {
    setShowClearConfirm(true);
  };

  const confirmClear = () => {
    //
    // nodeId === null means "All Nodes" - clear all logs including service and web.
    //
    send({ type: 'application_log_clear', node_id: nodeId });
    setShowClearConfirm(false);
  };

  //
  // Format timestamp.
  //
  const formatTime = (timestamp: string) => {
    const date = new Date(timestamp);
    return date.toLocaleTimeString('en-US', {
      hour12: false,
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      fractionalSecondDigits: 3,
    });
  };

  //
  // Note: Pagination shows total from server, but we filter client-side.
  // This is fine since we're working with a limited page of results.
  //
  const displayedCount = filteredEntries.length;
  const totalPages = Math.ceil(totalCount / limit);
  const currentPage = Math.floor(offset / limit) + 1;

  return (
    <div className="flex flex-col h-full">
      {/*
      // Filters toolbar - styled like traffic table
      */}
      <div className="flex items-center gap-4 p-4 border border-subtle ascii-box mb-3">
        {/* Source type filters */}
        {(['service', 'web', 'node'] as const).map(source => {
          const isActive = sourceFilter.has(source);
          return (
            <button
              key={source}
              onClick={() => toggleSource(source)}
              className={`px-3 py-1.5 text-xs font-medium transition-colors ${
                isActive
                  ? 'bg-[var(--accent-info)]/20 text-[var(--accent-info)] border border-[var(--accent-info)]/30'
                  : 'bg-[var(--bg-secondary)] text-muted border border-transparent hover:border-subtle'
              }`}
            >
              {source.toUpperCase()}
            </button>
          );
        })}

        {/* Node selector (if nodes provided) */}
        {nodes && nodes.length > 0 && onNodeChange && (
          <select
            value={selectedNodeId || 'all'}
            onChange={(e) => onNodeChange(e.target.value)}
            className="bg-[var(--bg-tertiary)] border border-subtle text-xs text-title px-2 py-1 outline-none"
          >
            <option value="all">All Nodes</option>
            {nodes.map((node) => (
              <option key={node.node_id} value={node.node_id}>
                {node.machine_name || 'Unknown'}
              </option>
            ))}
          </select>
        )}

        {/* Level filters */}
        {LOG_LEVELS.map(level => {
          const config = LEVEL_CONFIG[level];
          const isActive = levelFilter.has(level);
          return (
            <button
              key={level}
              onClick={() => toggleLevel(level)}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium transition-colors ${
                isActive
                  ? `${config.bgColor} ${config.color} border border-current/30`
                  : 'bg-[var(--bg-secondary)] text-muted border border-transparent hover:border-subtle'
              }`}
            >
              {config.icon}
              {level.toUpperCase()}
            </button>
          );
        })}

        {/* Search */}
        <div className="flex items-center gap-2 flex-1">
          <Search size={14} className="text-muted" />
          <input
            type="text"
            placeholder="Search..."
            className="bg-transparent border-b border-subtle text-xs text-title px-2 py-1 flex-1 focus:border-[var(--accent-success)] outline-none"
            value={regexInput}
            onChange={(e) => setRegexInput(e.target.value)}
          />
        </div>

        {/* Auto-refresh toggle */}
        <button
          onClick={() => setAutoRefresh(!autoRefresh)}
          className={`p-1.5 transition-colors ${
            autoRefresh
              ? 'text-[var(--accent-info)] bg-[var(--accent-info)]/10'
              : 'text-muted hover:text-[var(--accent-info)] hover:bg-[var(--accent-info)]/10'
          }`}
          title={autoRefresh ? 'Disable auto-refresh (5s)' : 'Enable auto-refresh (5s)'}
        >
          <RefreshCw size={16} className={autoRefresh ? 'animate-spin' : ''} />
        </button>

        {/* Manual refresh */}
        <button
          onClick={fetchLogs}
          disabled={loading}
          className="p-1.5 text-muted hover:text-[var(--accent-info)] hover:bg-[var(--accent-info)]/10 transition-colors disabled:opacity-50"
          title="Refresh"
        >
          <RefreshCw size={16} className={loading ? 'animate-spin' : ''} />
        </button>

        {/* Clear logs */}
        <button
          onClick={handleClear}
          className="p-1.5 text-muted hover:text-[var(--accent-error)] hover:bg-[var(--accent-error)]/10 transition-colors"
          title="Clear logs"
        >
          <Trash2 size={16} />
        </button>
      </div>

      {/*
      // Log entries table - styled like nodes table
      */}
      <div className="flex-1 border border-subtle ascii-box overflow-hidden flex flex-col">
        <div className="flex-1 overflow-auto">
          <table className="w-full text-xs">
            <thead className="sticky top-0 bg-[var(--bg-tertiary)]">
              <tr className="border-b border-subtle">
                <th className="text-left px-4 py-2 text-muted tracking-wider w-32">TIMESTAMP</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider w-40">SOURCE</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider w-24">LEVEL</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider w-48">TARGET</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">MESSAGE</th>
              </tr>
            </thead>
          <tbody>
            {filteredEntries.length === 0 ? (
              <tr>
                <td colSpan={5} className="px-4 py-8 text-center text-muted">
                  {loading ? 'Loading...' : entries.length === 0 ? 'No log entries found' : 'No entries match the selected filters'}
                </td>
              </tr>
            ) : (
              filteredEntries.map((entry, idx) => {
                const config = LEVEL_CONFIG[entry.level as LogLevel] || LEVEL_CONFIG.info;
                const node = nodes?.find(n => n.node_id === entry.source);

                return (
                  <tr
                    key={idx}
                    className="border-b border-dim last:border-0 hover:bg-[var(--highlight)] transition-colors"
                  >
                    <td className="px-4 py-2 text-muted font-mono whitespace-nowrap">
                      {formatTime(entry.timestamp)}
                    </td>
                    <td className="px-4 py-2 text-highlight whitespace-nowrap" title={node?.machine_name || entry.source}>
                      {entry.source === 'service' || entry.source === 'web'
                        ? entry.source
                        : node?.machine_name || entry.source.slice(0, 8)}
                    </td>
                    <td className="px-4 py-2 whitespace-nowrap">
                      <span className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded ${config.bgColor} ${config.color}`}>
                        {config.icon}
                        {entry.level.toUpperCase()}
                      </span>
                    </td>
                    <td className="px-4 py-2 text-muted font-mono truncate" title={entry.target || ''}>
                      {entry.target || '-'}
                    </td>
                    <td className="px-4 py-2 text-highlight font-mono break-all">
                      {entry.message}
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
          </table>
        </div>
      </div>

      {/*
      // Pagination
      */}
      {totalPages > 1 && (
        <div className="flex items-center justify-between text-xs mt-3 px-4">
          <span className="text-muted font-mono">
            Showing {displayedCount} of {totalCount} entries (page {currentPage}/{totalPages})
          </span>
          <div className="flex items-center gap-2">
            <button
              onClick={() => setOffset(Math.max(0, offset - limit))}
              disabled={offset === 0}
              className="px-3 py-1 text-xs text-muted hover:text-[var(--accent-info)] border border-subtle hover:border-[var(--accent-info)] transition-colors disabled:opacity-50"
            >
              Previous
            </button>
            <button
              onClick={() => setOffset(offset + limit)}
              disabled={offset + limit >= totalCount}
              className="px-3 py-1 text-xs text-muted hover:text-[var(--accent-info)] border border-subtle hover:border-[var(--accent-info)] transition-colors disabled:opacity-50"
            >
              Next
            </button>
          </div>
        </div>
      )}

      {/*
      // Clear confirmation modal
      */}
      {showClearConfirm && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-[var(--bg-secondary)] border border-subtle ascii-box p-6 max-w-md">
            <h3 className="text-title font-mono mb-4">Clear Application Logs</h3>
            <p className="text-muted mb-6">
              {nodeId === null
                ? 'Clear all application logs from all sources (nodes, service, web)?'
                : `Clear all logs for ${nodes?.find(n => n.node_id === nodeId)?.machine_name || 'this source'}?`}
            </p>
            <div className="flex gap-3 justify-end">
              <button
                onClick={() => setShowClearConfirm(false)}
                className="px-4 py-2 text-xs text-muted border border-subtle hover:border-[var(--accent-info)] hover:text-[var(--accent-info)] transition-colors"
              >
                CANCEL
              </button>
              <button
                onClick={confirmClear}
                className="px-4 py-2 text-xs text-[var(--accent-error)] border border-[var(--accent-error)] hover:bg-[var(--accent-error)]/10 transition-colors"
              >
                CLEAR ALL
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
