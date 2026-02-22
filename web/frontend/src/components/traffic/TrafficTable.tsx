import { useState, useMemo } from 'react';
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
import { DataTable, type ColumnDef } from '../common/DataTable';
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

type RowItem =
  | { type: 'http'; entry: InterceptedTrafficEntry; key: string }
  | { type: 'ws_group'; group: WebSocketGroup; key: string }
  | { type: 'h2_group'; group: H2Group; key: string };

export interface TrafficFilterBarProps {
  filters: TrafficLogFilters;
  setFilters: (filters: TrafficLogFilters) => void;
  protocolFilter: ProtocolFilter;
  setProtocolFilter: (filter: ProtocolFilter) => void;
  searchFilter: string;
  setSearchFilter: (filter: string) => void;
  onRefresh: () => void;
  onClear?: () => void;
  nodes?: { node_id: string; machine_name: string; discovered_agents: { short_name: string }[] }[];
  showNodeSelector?: boolean;
  showAgentSelector?: boolean;
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

  const matchText = (text: string, regex: RegExp | null, filterLower: string): boolean => {
    if (regex) return regex.test(text);
    return text.toLowerCase().includes(filterLower);
  };

  const decodeBody = (body: number[] | null | undefined): string | null => {
    if (!body) return null;
    try { return new TextDecoder().decode(new Uint8Array(body)); }
    catch { return null; }
  };

  const headersToString = (headers: Record<string, string> | null | undefined): string | null => {
    if (!headers) return null;
    return Object.entries(headers).map(([k, v]) => `${k}: ${v}`).join('\n');
  };

  let regex: RegExp | null = null;
  let filterLower = '';
  try { regex = new RegExp(searchFilter, 'i'); }
  catch { filterLower = searchFilter.toLowerCase(); }

  if (matchText(entry.url, regex, filterLower)) return true;
  const reqHeaders = headersToString(entry.request_headers);
  if (reqHeaders && matchText(reqHeaders, regex, filterLower)) return true;
  const respHeaders = headersToString(entry.response_headers);
  if (respHeaders && matchText(respHeaders, regex, filterLower)) return true;
  const reqBody = decodeBody(entry.request_body);
  if (reqBody && matchText(reqBody, regex, filterLower)) return true;
  const respBody = decodeBody(entry.response_body);
  if (respBody && matchText(respBody, regex, filterLower)) return true;

  return false;
}

//
// Utility: Count entries (HTTP entries + WS groups, not individual frames).
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
        wsGroupKeys.add(`${entry.node_id}:${entry.url}`);
      }
    } else if (isH2) {
      if (protocolFilter === 'all' || protocolFilter === 'h2') {
        h2GroupKeys.add(`${entry.node_id}:${entry.url}`);
      }
    } else {
      if (protocolFilter === 'all' || protocolFilter === 'http') httpCount++;
    }
  });

  return httpCount + wsGroupKeys.size + h2GroupKeys.size;
}

//
// Build grouped RowItem[] from raw entries.
//

function buildTrafficRows(
  entries: InterceptedTrafficEntry[],
  protocolFilter: ProtocolFilter,
  searchFilter: string,
  displayLimit?: number,
): RowItem[] {
  const httpEntries: InterceptedTrafficEntry[] = [];
  const wsFrames: InterceptedTrafficEntry[] = [];
  const h2Frames: InterceptedTrafficEntry[] = [];

  entries.forEach((entry) => {
    if (!matchesSearchFilter(entry, searchFilter)) return;
    const isWs = entry.method?.startsWith('WS_');
    const isH2 = entry.method?.startsWith('H2_');
    if (isWs) wsFrames.push(entry);
    else if (isH2) h2Frames.push(entry);
    else httpEntries.push(entry);
  });

  const wsGroups = new Map<string, WebSocketGroup>();
  wsFrames.forEach((frame) => {
    const groupKey = `${frame.node_id}:${frame.url}`;
    if (!wsGroups.has(groupKey)) {
      wsGroups.set(groupKey, {
        url: frame.url, nodeId: frame.node_id, agent: frame.agent_short_name,
        frames: [], firstTimestamp: frame.timestamp, lastTimestamp: frame.timestamp,
        sendCount: 0, recvCount: 0, totalBytes: 0,
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

  const h2Groups = new Map<string, H2Group>();
  h2Frames.forEach((frame) => {
    const groupKey = `${frame.node_id}:${frame.url}`;
    if (!h2Groups.has(groupKey)) {
      h2Groups.set(groupKey, {
        url: frame.url, nodeId: frame.node_id, agent: frame.agent_short_name,
        frames: [], firstTimestamp: frame.timestamp, lastTimestamp: frame.timestamp,
        sendCount: 0, recvCount: 0, totalBytes: 0,
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

  const rows: RowItem[] = [];

  if (protocolFilter === 'all' || protocolFilter === 'http') {
    httpEntries.forEach((entry) => {
      rows.push({ type: 'http', entry, key: `http-${entry.id ?? entry.timestamp}` });
    });
  }
  if (protocolFilter === 'all' || protocolFilter === 'websocket') {
    wsGroups.forEach((group, key) => {
      rows.push({ type: 'ws_group', group, key: `ws-${key}` });
    });
  }
  if (protocolFilter === 'all' || protocolFilter === 'h2') {
    h2Groups.forEach((group, key) => {
      rows.push({ type: 'h2_group', group, key: `h2-${key}` });
    });
  }

  rows.sort((a, b) => {
    const tsA = a.type === 'http' ? a.entry.timestamp : a.group.lastTimestamp;
    const tsB = b.type === 'http' ? b.entry.timestamp : b.group.lastTimestamp;
    return tsB.localeCompare(tsA);
  });

  return displayLimit ? rows.slice(0, displayLimit) : rows;
}

//
// Column definitions for the traffic DataTable.
//

function getTrafficColumns(showNodeColumn: boolean): ColumnDef<RowItem>[] {
  const cols: ColumnDef<RowItem>[] = [
    {
      key: 'timestamp',
      header: 'Timestamp',
      sortable: false,
      render: (_: unknown, row: RowItem) => {
        const ts = row.type === 'http' ? row.entry.timestamp : row.group.lastTimestamp;
        return <span className="text-muted font-mono">{new Date(ts).toLocaleString()}</span>;
      },
    },
  ];

  if (showNodeColumn) {
    cols.push({
      key: 'node',
      header: 'Node',
      sortable: false,
      render: (_: unknown, row: RowItem) => {
        const nodeId = row.type === 'http' ? row.entry.node_id : row.group.nodeId;
        return <span className="text-title font-mono">{nodeId}</span>;
      },
    });
  }

  cols.push(
    {
      key: 'agent',
      header: 'Agent',
      sortable: false,
      render: (_: unknown, row: RowItem) => {
        const agent = row.type === 'http' ? row.entry.agent_short_name : row.group.agent;
        return <span className="text-highlight">{agent}</span>;
      },
    },
    {
      key: 'method',
      header: 'Method',
      sortable: false,
      render: (_: unknown, row: RowItem) => {
        if (row.type === 'ws_group') {
          return (
            <div className="flex items-center gap-1">
              <Plug size={12} className="flex-shrink-0 text-[var(--accent-info)]" />
              <span className="text-[var(--accent-info)] font-mono">WS</span>
              <span className="text-muted text-[10px] ml-1">({row.group.frames.length})</span>
            </div>
          );
        }
        if (row.type === 'h2_group') {
          return (
            <div className="flex items-center gap-1">
              <span className="text-[var(--accent-purple)] font-mono">H2</span>
              <span className="text-muted text-[10px] ml-1">({row.group.frames.length})</span>
            </div>
          );
        }
        return <span className="text-title font-mono">{row.entry.method ?? '-'}</span>;
      },
    },
    {
      key: 'url',
      header: 'URL',
      sortable: false,
      render: (_: unknown, row: RowItem) => {
        const url = row.type === 'http' ? row.entry.url : row.group.url;
        return <span className="text-title font-mono" title={url}>{url}</span>;
      },
    },
    {
      key: 'status',
      header: 'Status',
      sortable: false,
      render: (_: unknown, row: RowItem) => {
        if (row.type !== 'http') {
          const group = row.group;
          return (
            <div className="flex items-center gap-2 text-[10px] font-mono">
              <span className="text-[var(--accent-warning)]">
                <ArrowUp size={10} className="inline" /> {group.sendCount}
              </span>
              <span className="text-[var(--accent-success)]">
                <ArrowDown size={10} className="inline" /> {group.recvCount}
              </span>
              <span className="text-muted">{formatBytes(group.totalBytes)}</span>
            </div>
          );
        }
        if (!row.entry.response_status) return <span className="text-muted">-</span>;
        const s = row.entry.response_status;
        const color = s >= 400 ? 'text-[var(--accent-alert)]' : s >= 300 ? 'text-[var(--accent-warning)]' : 'text-[var(--accent-success)]';
        return <span className={`font-mono ${color}`}>{s}</span>;
      },
    },
  );

  return cols;
}

//
// Expanded row renderers.
//

function ExpandedHttpRow({ entry }: { entry: InterceptedTrafficEntry }) {
  return (
    <div className="space-y-4">
      <div>
        <div className="text-muted mb-2 tracking-wider">FULL URL</div>
        <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto break-all whitespace-pre-wrap">
          {entry.method ?? 'GET'} {entry.url}
        </pre>
      </div>
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
  );
}

function ExpandedFrameList({ frames, label }: { frames: InterceptedTrafficEntry[]; label: string }) {
  const [expandedFrameId, setExpandedFrameId] = useState<number | null>(null);
  const sorted = [...frames].sort((a, b) => b.timestamp.localeCompare(a.timestamp));

  return (
    <div className="space-y-1">
      {sorted.map((frame) => {
        const id = frame.id ?? 0;
        const isExpanded = expandedFrameId === id;
        const timestamp = new Date(frame.timestamp).toLocaleTimeString();
        const isSend = frame.direction === 'send';
        const frameType = frame.method?.replace(`${label}_`, '') ?? '';
        const payload = isSend ? frame.request_body : frame.response_body;

        const getPreview = (): string | null => {
          if (!payload || payload.length === 0) return null;
          try {
            const decoded = new TextDecoder().decode(new Uint8Array(payload));
            const printable = decoded.replace(/^[\x00-\x1F]+/, '').slice(0, 60);
            if (printable.length > 0) return printable;
          } catch { /* ignore */ }
          return null;
        };
        const preview = getPreview();

        return (
          <div key={id}>
            <div
              className="flex items-center gap-3 px-3 py-1 hover:bg-[var(--highlight)] cursor-pointer"
              onClick={() => setExpandedFrameId(isExpanded ? null : id)}
            >
              <span className="text-muted">
                {isExpanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
              </span>
              <span className="text-muted font-mono text-[10px] w-20">{timestamp}</span>
              <div className="flex items-center gap-1 w-20">
                {isSend
                  ? <ArrowUp size={10} className="text-[var(--accent-warning)]" />
                  : <ArrowDown size={10} className="text-[var(--accent-success)]" />}
                <span className={`font-mono text-[10px] ${isSend ? 'text-[var(--accent-warning)]' : 'text-[var(--accent-success)]'}`}>
                  {frameType}
                </span>
              </div>
              <span className="text-muted font-mono text-[10px] flex-1 truncate">
                {preview
                  ? <span className="italic">{preview}{preview.length >= 60 ? '...' : ''}</span>
                  : <span>{payload?.length ?? 0} bytes</span>}
              </span>
            </div>
            {isExpanded && (
              <div className="px-3 py-2 ml-6 bg-[var(--bg-tertiary)]">
                <div className="text-muted mb-1 text-[10px] tracking-wider">
                  {frameType === 'TEXT' ? 'MESSAGE CONTENT' : `${frameType} FRAME (${payload?.length ?? 0} bytes)`}
                </div>
                <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64 whitespace-pre-wrap">
                  {payload ? tryPrettyPrintJson(payload) : '[No payload]'}
                </pre>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
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
  const getAgentOptions = () => {
    if (fixedNodeAgents) return fixedNodeAgents;
    if (nodes && filters.node_id) {
      const selectedNode = nodes.find(n => n.node_id === filters.node_id);
      return selectedNode?.discovered_agents ?? [];
    }
    if (nodes) {
      const allAgents = nodes.flatMap(n => n.discovered_agents);
      return allAgents.filter((agent, idx, arr) =>
        arr.findIndex(a => a.short_name === agent.short_name) === idx
      );
    }
    return [];
  };

  const agentOptions = getAgentOptions();

  return (
    <div className="flex flex-col lg:flex-row lg:items-center gap-3 lg:gap-4 p-4 border border-subtle ascii-box">
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

      {showNodeSelector && nodes && (
        <select
          className="bg-[var(--bg-tertiary)] border border-subtle text-xs text-title px-2 py-1 outline-none"
          value={filters.node_id ?? ''}
          onChange={(e) => {
            setFilters({ ...filters, node_id: e.target.value || null, agent_short_name: null, offset: 0 });
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

      {showAgentSelector && (
        <select
          className="bg-[var(--bg-tertiary)] border border-subtle text-xs text-title px-2 py-1 outline-none"
          value={filters.agent_short_name ?? ''}
          onChange={(e) => {
            setFilters({ ...filters, agent_short_name: e.target.value || null, offset: 0 });
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

      <div className="flex items-center gap-2 lg:ml-auto">
        <button
          onClick={onRefresh}
          className="flex items-center gap-2 px-3 py-1 text-xs text-muted hover:text-[var(--accent-info)] border border-subtle hover:border-[var(--accent-info)] hover:bg-[var(--accent-info)]/10 transition-colors"
        >
          <RefreshCw size={12} />
          REFRESH
        </button>
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
// ScrollableTrafficTable — now backed by DataTable.
//

export interface ScrollableTrafficTableProps {
  entries: InterceptedTrafficEntry[];
  protocolFilter: ProtocolFilter;
  searchFilter: string;
  expandedRow: number | null;
  setExpandedRow: (id: number | null) => void;
  showNodeColumn?: boolean;
  displayLimit?: number;
  heightMode?: 'flex' | 'fixed';
  maxHeight?: string;
  emptyMessage?: string;
}

export function ScrollableTrafficTable({
  entries,
  protocolFilter,
  searchFilter,
  showNodeColumn = true,
  displayLimit,
  heightMode = 'fixed',
  maxHeight = '70vh',
  emptyMessage = 'No traffic entries',
}: ScrollableTrafficTableProps) {
  const rows = useMemo(
    () => buildTrafficRows(entries, protocolFilter, searchFilter, displayLimit),
    [entries, protocolFilter, searchFilter, displayLimit],
  );

  const columns = useMemo(() => getTrafficColumns(showNodeColumn), [showNodeColumn]);

  const containerClass = heightMode === 'flex'
    ? 'flex-1 min-h-0 border border-subtle ascii-box flex flex-col overflow-hidden'
    : 'border border-subtle ascii-box flex flex-col overflow-hidden';
  const containerStyle = heightMode === 'fixed' ? { maxHeight } : undefined;

  return (
    <div className={containerClass} style={containerStyle}>
      <DataTable
        data={rows}
        columns={columns}
        getRowKey={row => row.key}
        resizable
        stickyHeader
        expandable={{
          render: (row) => {
            if (row.type === 'http') return <ExpandedHttpRow entry={row.entry} />;
            const label = row.type === 'ws_group' ? 'WS' : 'H2';
            return <ExpandedFrameList frames={row.group.frames} label={label} />;
          },
        }}
        rowClassName={(row) =>
          row.type !== 'http' ? 'bg-[var(--bg-tertiary)]/50' : ''
        }
        emptyMessage={emptyMessage}
        className="flex flex-col flex-1 min-h-0 overflow-hidden"
      />
    </div>
  );
}
