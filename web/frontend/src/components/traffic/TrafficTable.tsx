import { useState } from 'react';
import {
  ChevronDown,
  ChevronRight,
  Plug,
  ArrowUp,
  ArrowDown,
  Search,
  RefreshCw,
  Trash2,
} from 'lucide-react';
import type { InterceptedTrafficEntry, TrafficLogFilters } from '../../api/types';

//
// Types.
//

export interface WebSocketGroup {
  url: string;
  nodeId: string;
  agent: string;
  frames: InterceptedTrafficEntry[];
  firstTimestamp: string;
  lastTimestamp: string;
  sendCount: number;
  recvCount: number;
  totalBytes: number;
}

export interface H2Group {
  url: string;
  nodeId: string;
  agent: string;
  frames: InterceptedTrafficEntry[];
  firstTimestamp: string;
  lastTimestamp: string;
  sendCount: number;
  recvCount: number;
  totalBytes: number;
}

export type ProtocolFilter = 'all' | 'http' | 'websocket' | 'h2';

export interface TrafficTableProps {
  entries: InterceptedTrafficEntry[];
  protocolFilter: ProtocolFilter;
  searchFilter: string;
  expandedRow: number | null;
  setExpandedRow: (id: number | null) => void;
  showNodeColumn?: boolean;
  //
  // Limit number of logical entries (HTTP + WS groups) displayed.
  //
  displayLimit?: number;
}

export interface TrafficFilterBarProps {
  //
  // Filter state.
  //
  filters: TrafficLogFilters;
  setFilters: (filters: TrafficLogFilters) => void;
  protocolFilter: ProtocolFilter;
  setProtocolFilter: (filter: ProtocolFilter) => void;
  searchFilter: string;
  setSearchFilter: (filter: string) => void;
  //
  // Actions.
  //
  onRefresh: () => void;
  onClear?: () => void;
  //
  // Data for dropdowns.
  //
  nodes?: { node_id: string; machine_name: string; discovered_agents: { short_name: string }[] }[];
  //
  // Visibility controls.
  //
  showNodeSelector?: boolean;
  showAgentSelector?: boolean;
  //
  // For node-scoped agent selector (when node is fixed).
  //
  fixedNodeAgents?: { short_name: string }[];
}

//
// Helper Functions.
//

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}

export function tryDecodeBody(body: number[]): string {
  try {
    return new TextDecoder().decode(new Uint8Array(body));
  } catch {
    return `[Binary data: ${body.length} bytes]`;
  }
}

export function tryPrettyPrintJson(body: number[]): string {
  try {
    const text = new TextDecoder().decode(new Uint8Array(body));
    //
    // Trim whitespace and remove null bytes that might cause JSON.parse to
    // fail.
    //
    const trimmed = text.trim().replace(/\0/g, '');
    const parsed = JSON.parse(trimmed);
    return JSON.stringify(parsed, null, 2);
  } catch (e) {
    console.info('[tryPrettyPrintJson] JSON parse failed:', e);
    try {
      return new TextDecoder().decode(new Uint8Array(body));
    } catch (e2) {
      console.info('[tryPrettyPrintJson] TextDecoder failed:', e2);
      return `[Binary data: ${body.length} bytes]`;
    }
  }
}

function matchesSearchFilter(entry: InterceptedTrafficEntry, searchFilter: string): boolean {
  if (!searchFilter) return true;

  //
  // Helper to check if text matches (regex or literal).
  //
  const matchText = (text: string, regex: RegExp | null, filterLower: string): boolean => {
    if (regex) {
      return regex.test(text);
    }
    return text.toLowerCase().includes(filterLower);
  };

  //
  // Helper to decode body.
  //
  const decodeBody = (body: number[] | null | undefined): string | null => {
    if (!body) return null;
    try {
      return new TextDecoder().decode(new Uint8Array(body));
    } catch { return null; }
  };

  //
  // Helper to stringify headers.
  //
  const headersToString = (headers: Record<string, string> | null | undefined): string | null => {
    if (!headers) return null;
    return Object.entries(headers).map(([k, v]) => `${k}: ${v}`).join('\n');
  };

  let regex: RegExp | null = null;
  let filterLower = '';
  try {
    regex = new RegExp(searchFilter, 'i');
  } catch {
    //
    // Invalid regex, use literal match.
    //
    filterLower = searchFilter.toLowerCase();
  }

  //
  // Check URL.
  //
  if (matchText(entry.url, regex, filterLower)) return true;

  //
  // Check request headers.
  //
  const reqHeaders = headersToString(entry.request_headers);
  if (reqHeaders && matchText(reqHeaders, regex, filterLower)) return true;

  //
  // Check response headers.
  //
  const respHeaders = headersToString(entry.response_headers);
  if (respHeaders && matchText(respHeaders, regex, filterLower)) return true;

  //
  // Check request body.
  //
  const reqBody = decodeBody(entry.request_body);
  if (reqBody && matchText(reqBody, regex, filterLower)) return true;

  //
  // Check response body.
  //
  const respBody = decodeBody(entry.response_body);
  if (respBody && matchText(respBody, regex, filterLower)) return true;

  return false;
}

//
// Utility: Count entries (HTTP entries + WS groups, not individual WS frames).
//

export function countTrafficEntries(
  entries: InterceptedTrafficEntry[],
  protocolFilter: ProtocolFilter,
  searchFilter: string
): number {
  let httpCount = 0;
  const wsGroupKeys = new Set<string>();
  const h2GroupKeys = new Set<string>();

  entries.forEach((entry) => {
    if (!matchesSearchFilter(entry, searchFilter)) return;

    const isWs = entry.method?.startsWith('WS_');
    const isH2 = entry.method?.startsWith('H2_');
    if (isWs) {
      if (protocolFilter === 'all' || protocolFilter === 'websocket') {
        const groupKey = `${entry.node_id}:${entry.url}`;
        wsGroupKeys.add(groupKey);
      }
    } else if (isH2) {
      if (protocolFilter === 'all' || protocolFilter === 'h2') {
        const groupKey = `${entry.node_id}:${entry.url}`;
        h2GroupKeys.add(groupKey);
      }
    } else {
      if (protocolFilter === 'all' || protocolFilter === 'http') {
        httpCount++;
      }
    }
  });

  return httpCount + wsGroupKeys.size + h2GroupKeys.size;
}

//
// Traffic Filter Bar Component.
//

export function TrafficFilterBar({
  filters,
  setFilters,
  protocolFilter,
  setProtocolFilter,
  searchFilter,
  setSearchFilter,
  onRefresh,
  onClear,
  nodes,
  showNodeSelector = false,
  showAgentSelector = false,
  fixedNodeAgents,
}: TrafficFilterBarProps) {
  //
  // Get agents for dropdown based on context.
  //
  const getAgentOptions = () => {
    if (fixedNodeAgents) {
      //
      // Node-scoped: show agents for that specific node.
      //
      return fixedNodeAgents;
    }
    if (nodes && filters.node_id) {
      //
      // Global with node selected: show agents for selected node.
      //
      const selectedNode = nodes.find(n => n.node_id === filters.node_id);
      return selectedNode?.discovered_agents ?? [];
    }
    if (nodes) {
      //
      // Global with no node selected: show all unique agents.
      //
      const allAgents = nodes.flatMap(n => n.discovered_agents);
      const uniqueAgents = allAgents.filter((agent, idx, arr) =>
        arr.findIndex(a => a.short_name === agent.short_name) === idx
      );
      return uniqueAgents;
    }
    return [];
  };

  const agentOptions = getAgentOptions();

  return (
    <div className="flex flex-col lg:flex-row lg:items-center gap-3 lg:gap-4 p-4 border border-subtle ascii-box">
      {/*
      //
      // Unified Search Filter.
      //
      */}
      <div className="flex items-center gap-2 w-full lg:w-auto">
        <Search size={14} className="text-muted" />
        <input
          type="text"
          placeholder="Search..."
          className="bg-transparent border-b border-subtle text-xs text-title px-2 py-1 w-full lg:w-48 focus:border-[var(--accent-success)] outline-none"
          value={searchFilter}
          onChange={(e) => setSearchFilter(e.target.value)}
        />
      </div>

      {/*
      //
      // Node Selector.
      //
      */}
      {showNodeSelector && nodes && (
        <select
          className="bg-[var(--bg-tertiary)] border border-subtle text-xs text-title px-2 py-1 outline-none"
          value={filters.node_id ?? ''}
          onChange={(e) => {
            const newFilters = { ...filters, node_id: e.target.value || null, agent_short_name: null, offset: 0 };
            setFilters(newFilters);
          }}
        >
          <option value="">All Nodes</option>
          {nodes.map((node) => (
            <option key={node.node_id} value={node.node_id}>
              {node.machine_name || node.node_id.slice(0, 8)}
            </option>
          ))}
        </select>
      )}

      {/*
      //
      // Agent Selector.
      //
      */}
      {showAgentSelector && (
        <select
          className="bg-[var(--bg-tertiary)] border border-subtle text-xs text-title px-2 py-1 outline-none"
          value={filters.agent_short_name ?? ''}
          onChange={(e) => {
            const newFilters = { ...filters, agent_short_name: e.target.value || null, offset: 0 };
            setFilters(newFilters);
          }}
        >
          <option value="">All Agents</option>
          {agentOptions.map((agent) => (
            <option key={agent.short_name} value={agent.short_name}>
              {agent.short_name}
            </option>
          ))}
        </select>
      )}

      {/*
      //
      // Protocol Filter.
      //
      */}
      <select
        className="bg-[var(--bg-tertiary)] border border-subtle text-xs text-title px-2 py-1 outline-none"
        value={protocolFilter}
        onChange={(e) => setProtocolFilter(e.target.value as ProtocolFilter)}
      >
        <option value="all">All Protocols</option>
        <option value="http">HTTP Only</option>
        <option value="websocket">WebSocket Only</option>
        <option value="h2">HTTP/2 Only</option>
      </select>

      <div className="hidden lg:block flex-1" />

      {/*
      //
      // Refresh Button.
      //
      */}
      <div className="flex items-center gap-2 lg:ml-auto">
        <button
          onClick={onRefresh}
          className="flex items-center gap-2 px-3 py-1 text-xs text-muted hover:text-[var(--accent-info)] border border-subtle hover:border-[var(--accent-info)] hover:bg-[var(--accent-info)]/10 transition-colors"
        >
          <RefreshCw size={12} />
          REFRESH
        </button>

      {/*
      //
      // Clear Button (optional).
      //
      */}
        {onClear && (
          <button
            onClick={onClear}
            className="flex items-center gap-2 px-3 py-1 text-xs text-muted hover:text-[var(--accent-error)] border border-subtle hover:border-[var(--accent-error)] hover:bg-[var(--accent-error)]/10 transition-colors"
          >
            <Trash2 size={12} />
            CLEAR
          </button>
        )}
      </div>
    </div>
  );
}

//
// Grouped Traffic Rows Component.
//

export function GroupedTrafficRows({
  entries,
  protocolFilter,
  searchFilter,
  expandedRow,
  setExpandedRow,
  showNodeColumn = true,
  displayLimit,
}: TrafficTableProps) {
  const [expandedWsGroups, setExpandedWsGroups] = useState<Set<string>>(new Set());
  const [expandedH2Groups, setExpandedH2Groups] = useState<Set<string>>(new Set());

  //
  // Separate HTTP, WebSocket, and HTTP/2 entries, applying search filter.
  //
  const httpEntries: InterceptedTrafficEntry[] = [];
  const wsFrames: InterceptedTrafficEntry[] = [];
  const h2Frames: InterceptedTrafficEntry[] = [];

  entries.forEach((entry) => {
    if (!matchesSearchFilter(entry, searchFilter)) return;

    const isWs = entry.method?.startsWith('WS_');
    const isH2 = entry.method?.startsWith('H2_');
    if (isWs) {
      wsFrames.push(entry);
    } else if (isH2) {
      h2Frames.push(entry);
    } else {
      httpEntries.push(entry);
    }
  });

  //
  // Group WebSocket frames by URL + node.
  //
  const wsGroups = new Map<string, WebSocketGroup>();
  wsFrames.forEach((frame) => {
    const groupKey = `${frame.node_id}:${frame.url}`;
    if (!wsGroups.has(groupKey)) {
      wsGroups.set(groupKey, {
        url: frame.url,
        nodeId: frame.node_id,
        agent: frame.agent_short_name,
        frames: [],
        firstTimestamp: frame.timestamp,
        lastTimestamp: frame.timestamp,
        sendCount: 0,
        recvCount: 0,
        totalBytes: 0,
      });
    }
    const group = wsGroups.get(groupKey)!;
    group.frames.push(frame);
    if (frame.timestamp < group.firstTimestamp) group.firstTimestamp = frame.timestamp;
    if (frame.timestamp > group.lastTimestamp) group.lastTimestamp = frame.timestamp;
    if (frame.direction === 'send') {
      group.sendCount++;
      group.totalBytes += frame.request_body?.length ?? 0;
    } else {
      group.recvCount++;
      group.totalBytes += frame.response_body?.length ?? 0;
    }
  });

  //
  // Group HTTP/2 frames by URL + node.
  //
  const h2Groups = new Map<string, H2Group>();
  h2Frames.forEach((frame) => {
    const groupKey = `${frame.node_id}:${frame.url}`;
    if (!h2Groups.has(groupKey)) {
      h2Groups.set(groupKey, {
        url: frame.url,
        nodeId: frame.node_id,
        agent: frame.agent_short_name,
        frames: [],
        firstTimestamp: frame.timestamp,
        lastTimestamp: frame.timestamp,
        sendCount: 0,
        recvCount: 0,
        totalBytes: 0,
      });
    }
    const group = h2Groups.get(groupKey)!;
    group.frames.push(frame);
    if (frame.timestamp < group.firstTimestamp) group.firstTimestamp = frame.timestamp;
    if (frame.timestamp > group.lastTimestamp) group.lastTimestamp = frame.timestamp;
    if (frame.direction === 'send') {
      group.sendCount++;
      group.totalBytes += frame.request_body?.length ?? 0;
    } else {
      group.recvCount++;
      group.totalBytes += frame.response_body?.length ?? 0;
    }
  });

  //
  // Build combined list maintaining order by timestamp.
  //
  type RowItem =
    | { type: 'http'; entry: InterceptedTrafficEntry }
    | { type: 'ws_group'; group: WebSocketGroup; key: string }
    | { type: 'h2_group'; group: H2Group; key: string };

  const rows: RowItem[] = [];

  //
  // Add HTTP entries.
  //
  if (protocolFilter === 'all' || protocolFilter === 'http') {
    httpEntries.forEach((entry) => {
      rows.push({ type: 'http', entry });
    });
  }

  //
  // Add WebSocket groups (using first timestamp for ordering).
  //
  if (protocolFilter === 'all' || protocolFilter === 'websocket') {
    wsGroups.forEach((group, key) => {
      rows.push({ type: 'ws_group', group, key });
    });
  }

  //
  // Add HTTP/2 groups.
  //
  if (protocolFilter === 'all' || protocolFilter === 'h2') {
    h2Groups.forEach((group, key) => {
      rows.push({ type: 'h2_group', group, key });
    });
  }

  //
  // Sort by timestamp (descending - newest first).
  //
  rows.sort((a, b) => {
    const tsA = a.type === 'http' ? a.entry.timestamp : a.group.lastTimestamp;
    const tsB = b.type === 'http' ? b.entry.timestamp : b.group.lastTimestamp;
    return tsB.localeCompare(tsA);
  });

  //
  // Apply display limit if specified.
  //
  const displayedRows = displayLimit ? rows.slice(0, displayLimit) : rows;

  const toggleWsGroup = (key: string) => {
    setExpandedWsGroups((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  };

  const toggleH2Group = (key: string) => {
    setExpandedH2Groups((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  };

  const colSpan = showNodeColumn ? 7 : 6;

  return (
    <>
      {displayedRows.map((row) => {
        if (row.type === 'http') {
          return (
            <TrafficRow
              key={row.entry.id ?? row.entry.timestamp}
              entry={row.entry}
              expanded={expandedRow === row.entry.id}
              onToggle={() => setExpandedRow(expandedRow === row.entry.id ? null : row.entry.id)}
              showNodeColumn={showNodeColumn}
            />
          );
        } else if (row.type === 'ws_group') {
          const isExpanded = expandedWsGroups.has(row.key);
          return (
            <WebSocketGroupRow
              key={row.key}
              group={row.group}
              isExpanded={isExpanded}
              onToggle={() => toggleWsGroup(row.key)}
              expandedFrameId={expandedRow}
              setExpandedFrameId={setExpandedRow}
              showNodeColumn={showNodeColumn}
              colSpan={colSpan}
            />
          );
        } else {
          const isExpanded = expandedH2Groups.has(row.key);
          return (
            <H2GroupRow
              key={row.key}
              group={row.group}
              isExpanded={isExpanded}
              onToggle={() => toggleH2Group(row.key)}
              expandedFrameId={expandedRow}
              setExpandedFrameId={setExpandedRow}
              showNodeColumn={showNodeColumn}
              colSpan={colSpan}
            />
          );
        }
      })}
    </>
  );
}

//
// WebSocket Group Row Component.
//

function WebSocketGroupRow({
  group,
  isExpanded,
  onToggle,
  expandedFrameId,
  setExpandedFrameId,
  showNodeColumn,
  colSpan,
}: {
  group: WebSocketGroup;
  isExpanded: boolean;
  onToggle: () => void;
  expandedFrameId: number | null;
  setExpandedFrameId: (id: number | null) => void;
  showNodeColumn: boolean;
  colSpan: number;
}) {
  const timestamp = new Date(group.lastTimestamp).toLocaleString();
  const frameCount = group.frames.length;

  return (
    <>
      {/*
      //
      // Group header row.
      //
      */}
      <tr
        className="border-b border-dim hover:bg-[var(--highlight)] cursor-pointer bg-[var(--bg-tertiary)]/50"
        onClick={onToggle}
      >
        <td className="px-4 py-2">
          {isExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </td>
        <td className="px-4 py-2 text-muted font-mono">{timestamp}</td>
        {showNodeColumn && (
          <td className="px-4 py-2 text-title">{group.nodeId.slice(0, 8)}</td>
        )}
        <td className="px-4 py-2 text-highlight">{group.agent}</td>
        <td className="px-4 py-2">
          <div className="flex items-center gap-1">
            <Plug size={12} className="text-[var(--accent-info)]" />
            <span className="text-[var(--accent-info)] font-mono">WS</span>
            <span className="text-muted text-[10px] ml-1">
              ({frameCount} frames)
            </span>
          </div>
        </td>
        <td className="px-4 py-2 text-title font-mono truncate max-w-xs" title={group.url}>
          {group.url}
        </td>
        <td className="px-4 py-2">
          <div className="flex items-center gap-2 text-[10px] font-mono">
            <span className="text-[var(--accent-warning)]">
              <ArrowUp size={10} className="inline" /> {group.sendCount}
            </span>
            <span className="text-[var(--accent-success)]">
              <ArrowDown size={10} className="inline" /> {group.recvCount}
            </span>
            <span className="text-muted">
              {formatBytes(group.totalBytes)}
            </span>
          </div>
        </td>
      </tr>

      {/*
      //
      // Expanded frames.
      //
      */}
      {isExpanded && (
        <tr className="bg-[var(--bg-primary)]">
          <td colSpan={colSpan} className="p-0 pl-8">
            <div className="py-2 space-y-1">
              {group.frames
                .sort((a, b) => b.timestamp.localeCompare(a.timestamp))
                .map((frame) => (
                  <WebSocketFrameRow
                    key={frame.id ?? frame.timestamp}
                    frame={frame}
                    expanded={expandedFrameId === frame.id}
                    onToggle={() =>
                      setExpandedFrameId(expandedFrameId === frame.id ? null : frame.id)
                    }
                  />
                ))}
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

//
// WebSocket Frame Row Component.
//

function WebSocketFrameRow({
  frame,
  expanded,
  onToggle,
}: {
  frame: InterceptedTrafficEntry;
  expanded: boolean;
  onToggle: () => void;
}) {
  const timestamp = new Date(frame.timestamp).toLocaleTimeString();
  const isSend = frame.direction === 'send';
  const wsType = frame.method?.replace('WS_', '') ?? '';
  const payload = isSend ? frame.request_body : frame.response_body;
  const preview = payload && wsType === 'TEXT'
    ? tryDecodeBody(payload).slice(0, 60)
    : null;

  return (
    <div>
      <div
        className="flex items-center gap-3 px-3 py-1 hover:bg-[var(--highlight)] cursor-pointer"
        onClick={onToggle}
      >
        <span className="text-muted">
          {expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
        </span>
        <span className="text-muted font-mono text-[10px] w-20">{timestamp}</span>
        <div className="flex items-center gap-1 w-16">
          {isSend ? (
            <ArrowUp size={10} className="text-[var(--accent-warning)]" />
          ) : (
            <ArrowDown size={10} className="text-[var(--accent-success)]" />
          )}
          <span className={`font-mono text-[10px] ${isSend ? 'text-[var(--accent-warning)]' : 'text-[var(--accent-success)]'}`}>
            {wsType}
          </span>
        </div>
        <span className="text-muted font-mono text-[10px] flex-1 truncate">
          {preview ? (
            <span className="italic">{preview}{preview.length >= 60 ? '...' : ''}</span>
          ) : (
            <span>{payload?.length ?? 0} bytes</span>
          )}
        </span>
      </div>
      {expanded && (
        <div className="px-3 py-2 ml-6 bg-[var(--bg-tertiary)]">
          <div className="text-muted mb-1 text-[10px] tracking-wider">
            {wsType === 'TEXT' ? 'MESSAGE CONTENT' : 'BINARY DATA'}
          </div>
          <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64 whitespace-pre-wrap">
            {wsType === 'TEXT' && payload ? tryPrettyPrintJson(payload) : payload ? tryDecodeBody(payload) : '[No payload]'}
          </pre>
        </div>
      )}
    </div>
  );
}

//
// HTTP/2 Group Row Component.
//

function H2GroupRow({
  group,
  isExpanded,
  onToggle,
  expandedFrameId,
  setExpandedFrameId,
  showNodeColumn,
  colSpan,
}: {
  group: H2Group;
  isExpanded: boolean;
  onToggle: () => void;
  expandedFrameId: number | null;
  setExpandedFrameId: (id: number | null) => void;
  showNodeColumn: boolean;
  colSpan: number;
}) {
  const timestamp = new Date(group.lastTimestamp).toLocaleString();
  const frameCount = group.frames.length;

  return (
    <>
      {/*
      //
      // Group header row.
      //
      */}
      <tr
        className="border-b border-dim hover:bg-[var(--highlight)] cursor-pointer bg-[var(--bg-tertiary)]/50"
        onClick={onToggle}
      >
        <td className="px-4 py-2">
          {isExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </td>
        <td className="px-4 py-2 text-muted font-mono">{timestamp}</td>
        {showNodeColumn && (
          <td className="px-4 py-2 text-title">{group.nodeId.slice(0, 8)}</td>
        )}
        <td className="px-4 py-2 text-highlight">{group.agent}</td>
        <td className="px-4 py-2">
          <div className="flex items-center gap-1">
            <span className="text-[var(--accent-purple)] font-mono">H2</span>
            <span className="text-muted text-[10px] ml-1">
              ({frameCount} frames)
            </span>
          </div>
        </td>
        <td className="px-4 py-2 text-title font-mono truncate max-w-xs" title={group.url}>
          {group.url}
        </td>
        <td className="px-4 py-2">
          <div className="flex items-center gap-2 text-[10px] font-mono">
            <span className="text-[var(--accent-warning)]">
              <ArrowUp size={10} className="inline" /> {group.sendCount}
            </span>
            <span className="text-[var(--accent-success)]">
              <ArrowDown size={10} className="inline" /> {group.recvCount}
            </span>
            <span className="text-muted">
              {formatBytes(group.totalBytes)}
            </span>
          </div>
        </td>
      </tr>

      {/*
      //
      // Expanded frames.
      //
      */}
      {isExpanded && (
        <tr className="bg-[var(--bg-primary)]">
          <td colSpan={colSpan} className="p-0 pl-8">
            <div className="py-2 space-y-1">
              {group.frames
                .sort((a, b) => b.timestamp.localeCompare(a.timestamp))
                .map((frame) => (
                  <H2FrameRow
                    key={frame.id ?? frame.timestamp}
                    frame={frame}
                    expanded={expandedFrameId === frame.id}
                    onToggle={() =>
                      setExpandedFrameId(expandedFrameId === frame.id ? null : frame.id)
                    }
                  />
                ))}
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

//
// HTTP/2 Frame Row Component.
//

function H2FrameRow({
  frame,
  expanded,
  onToggle,
}: {
  frame: InterceptedTrafficEntry;
  expanded: boolean;
  onToggle: () => void;
}) {
  const timestamp = new Date(frame.timestamp).toLocaleTimeString();
  const isSend = frame.direction === 'send';
  const h2Type = frame.method?.replace('H2_', '') ?? '';
  const payload = isSend ? frame.request_body : frame.response_body;

  //
  // Try to decode and preview the payload.
  //
  const getPreview = (): string | null => {
    if (!payload || payload.length === 0) return null;
    try {
      const decoded = new TextDecoder().decode(new Uint8Array(payload));
      //
      // Skip non-printable characters at the start (gRPC length prefix).
      //
      const printable = decoded.replace(/^[\x00-\x1F]+/, '').slice(0, 60);
      if (printable.length > 0) return printable;
    } catch {
      // Ignore decode errors.
    }
    return null;
  };

  const preview = getPreview();

  return (
    <div>
      <div
        className="flex items-center gap-3 px-3 py-1 hover:bg-[var(--highlight)] cursor-pointer"
        onClick={onToggle}
      >
        <span className="text-muted">
          {expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
        </span>
        <span className="text-muted font-mono text-[10px] w-20">{timestamp}</span>
        <div className="flex items-center gap-1 w-20">
          {isSend ? (
            <ArrowUp size={10} className="text-[var(--accent-warning)]" />
          ) : (
            <ArrowDown size={10} className="text-[var(--accent-success)]" />
          )}
          <span className={`font-mono text-[10px] ${isSend ? 'text-[var(--accent-warning)]' : 'text-[var(--accent-success)]'}`}>
            {h2Type}
          </span>
        </div>
        <span className="text-muted font-mono text-[10px] flex-1 truncate">
          {preview ? (
            <span className="italic">{preview}{preview.length >= 60 ? '...' : ''}</span>
          ) : (
            <span>{payload?.length ?? 0} bytes</span>
          )}
        </span>
      </div>
      {expanded && (
        <div className="px-3 py-2 ml-6 bg-[var(--bg-tertiary)]">
          <div className="text-muted mb-1 text-[10px] tracking-wider">
            {h2Type} FRAME ({payload?.length ?? 0} bytes)
          </div>
          <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64 whitespace-pre-wrap">
            {payload ? tryPrettyPrintJson(payload) : '[No payload]'}
          </pre>
        </div>
      )}
    </div>
  );
}

//
// HTTP Traffic Row Component.
//

function TrafficRow({
  entry,
  expanded,
  onToggle,
  showNodeColumn,
}: {
  entry: InterceptedTrafficEntry;
  expanded: boolean;
  onToggle: () => void;
  showNodeColumn: boolean;
}) {
  const timestamp = new Date(entry.timestamp).toLocaleString();
  const colSpan = showNodeColumn ? 7 : 6;

  return (
    <>
      <tr
        className="border-b border-dim hover:bg-[var(--highlight)] cursor-pointer"
        onClick={onToggle}
      >
        <td className="px-4 py-2">
          {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </td>
        <td className="px-4 py-2 text-muted font-mono">{timestamp}</td>
        {showNodeColumn && (
          <td className="px-4 py-2 text-title">{entry.node_id.slice(0, 8)}</td>
        )}
        <td className="px-4 py-2 text-highlight">{entry.agent_short_name}</td>
        <td className="px-4 py-2">
          <span className="text-title font-mono">{entry.method ?? '-'}</span>
        </td>
        <td className="px-4 py-2 text-title font-mono truncate max-w-xs" title={entry.url}>
          {entry.url}
        </td>
        <td className="px-4 py-2">
          {entry.response_status ? (
            <span
              className={`font-mono ${
                entry.response_status >= 400
                  ? 'text-[var(--accent-alert)]'
                  : entry.response_status >= 300
                  ? 'text-[var(--accent-warning)]'
                  : 'text-[var(--accent-success)]'
              }`}
            >
              {entry.response_status}
            </span>
          ) : (
            <span className="text-muted">-</span>
          )}
        </td>
      </tr>
      {expanded && (
        <tr className="bg-[var(--bg-tertiary)]">
          <td colSpan={colSpan} className="px-4 py-4">
            <div className="space-y-4">
              {/*
              //
              // Full URL.
              //
              */}
              <div>
                <div className="text-muted mb-2 tracking-wider">FULL URL</div>
                <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto break-all whitespace-pre-wrap">
                  {entry.method ?? 'GET'} {entry.url}
                </pre>
              </div>

              {/*
              //
              // HTTP request/response content.
              //
              */}
              <div className="grid grid-cols-2 gap-4">
                {entry.request_headers && (
                  <div>
                    <div className="text-muted mb-2 tracking-wider">REQUEST HEADERS</div>
                    <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64">
                      {JSON.stringify(entry.request_headers, null, 2)}
                    </pre>
                  </div>
                )}
                {entry.request_body && (
                  <div>
                    <div className="text-muted mb-2 tracking-wider">REQUEST BODY</div>
                    <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64 whitespace-pre-wrap">
                      {tryPrettyPrintJson(entry.request_body)}
                    </pre>
                  </div>
                )}
                {entry.response_headers && (
                  <div>
                    <div className="text-muted mb-2 tracking-wider">RESPONSE HEADERS</div>
                    <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64">
                      {JSON.stringify(entry.response_headers, null, 2)}
                    </pre>
                  </div>
                )}
                {entry.response_body && (
                  <div>
                    <div className="text-muted mb-2 tracking-wider">RESPONSE BODY</div>
                    <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64 whitespace-pre-wrap">
                      {tryPrettyPrintJson(entry.response_body)}
                    </pre>
                  </div>
                )}
              </div>
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

//
// Traffic Table Header Component.
//

export function TrafficTableHeader({ showNodeColumn = true }: { showNodeColumn?: boolean }) {
  return (
    <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
      <th className="text-left px-4 py-2 text-muted tracking-wider"></th>
      <th className="text-left px-4 py-2 text-muted tracking-wider">TIMESTAMP</th>
      {showNodeColumn && (
        <th className="text-left px-4 py-2 text-muted tracking-wider">NODE</th>
      )}
      <th className="text-left px-4 py-2 text-muted tracking-wider">AGENT</th>
      <th className="text-left px-4 py-2 text-muted tracking-wider">METHOD</th>
      <th className="text-left px-4 py-2 text-muted tracking-wider">URL</th>
      <th className="text-left px-4 py-2 text-muted tracking-wider">STATUS</th>
    </tr>
  );
}

//
// Scrollable Traffic Table Component.
// Reusable wrapper that provides fixed header and scrollable body.
//

export interface ScrollableTrafficTableProps extends TrafficTableProps {
  //
  // Height mode: 'flex' fills available space in flex container,
  // 'fixed' uses maxHeight for non-flex contexts.
  //
  heightMode?: 'flex' | 'fixed';
  //
  // Max height for fixed mode (default: 70vh).
  //
  maxHeight?: string;
  //
  // Empty state message.
  //
  emptyMessage?: string;
}

export function ScrollableTrafficTable({
  entries,
  protocolFilter,
  searchFilter,
  expandedRow,
  setExpandedRow,
  showNodeColumn = true,
  displayLimit,
  heightMode = 'fixed',
  maxHeight = '70vh',
  emptyMessage = 'No traffic entries',
}: ScrollableTrafficTableProps) {
  const colSpan = showNodeColumn ? 7 : 6;

  const containerClass = heightMode === 'flex'
    ? 'flex-1 min-h-0 border border-subtle ascii-box flex flex-col overflow-hidden'
    : `border border-subtle ascii-box flex flex-col overflow-hidden`;

  const containerStyle = heightMode === 'fixed' ? { maxHeight } : undefined;

  return (
    <div className={containerClass} style={containerStyle}>
      <div className="overflow-x-auto">
      <table className="w-full min-w-[920px] text-xs table-fixed">
        <thead>
          <TrafficTableHeader showNodeColumn={showNodeColumn} />
        </thead>
      </table>
      </div>
      <div className="flex-1 overflow-y-auto">
        <div className="overflow-x-auto">
        <table className="w-full min-w-[920px] text-xs table-fixed">
          <tbody>
            <GroupedTrafficRows
              entries={entries}
              protocolFilter={protocolFilter}
              searchFilter={searchFilter}
              expandedRow={expandedRow}
              setExpandedRow={setExpandedRow}
              showNodeColumn={showNodeColumn}
              displayLimit={displayLimit}
            />
            {entries.length === 0 && (
              <tr>
                <td colSpan={colSpan} className="px-4 py-8 text-center text-muted">
                  {emptyMessage}
                </td>
              </tr>
            )}
          </tbody>
        </table>
        </div>
      </div>
    </div>
  );
}
