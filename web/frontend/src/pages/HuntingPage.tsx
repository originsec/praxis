import { useState, useCallback, useRef, useEffect } from 'react';
import { Play, Loader2, AlertTriangle, BookOpen, ChevronRight, GripHorizontal } from 'lucide-react';
import { useApp } from '../context/AppContext';
import { KqlCodeEditor } from '../components/hunting/KqlCodeEditor';
import { HuntingResultsTable } from '../components/hunting/HuntingResultsTable';

const DEFAULT_EDITOR_HEIGHT = 200;
const MIN_EDITOR_HEIGHT = 80;
const MAX_EDITOR_HEIGHT = 600;

interface TableInfo {
  name: string;
  description: string;
  source: string;
  columns: { name: string; description: string }[];
}

const TABLES: TableInfo[] = [
  {
    name: 'AgentLogs',
    description: 'Discovered agents across nodes',
    source: 'In-memory',
    columns: [
      { name: 'timestamp', description: 'Last update' },
      { name: 'node_id', description: 'Node identifier' },
      { name: 'agent_short_name', description: 'Agent short name' },
      { name: 'agent_name', description: 'Display name' },
      { name: 'version', description: 'Agent version' },
    ],
  },
  {
    name: 'EventLogs',
    description: 'System event log entries',
    source: 'Database',
    columns: [
      { name: 'timestamp', description: 'Event time' },
      { name: 'source', description: 'Event source' },
      { name: 'level', description: 'Log level' },
      { name: 'target', description: 'Log target module' },
      { name: 'message', description: 'Log message' },
    ],
  },
  {
    name: 'NodeLogs',
    description: 'Connected nodes',
    source: 'In-memory',
    columns: [
      { name: 'timestamp', description: 'Last update' },
      { name: 'node_id', description: 'Node identifier' },
      { name: 'machine_name', description: 'Hostname' },
      { name: 'os_details', description: 'OS info' },
      { name: 'intercept_active', description: 'Interception active' },
    ],
  },
  {
    name: 'ReconLogs',
    description: 'Recon summary per node+agent',
    source: 'Database',
    columns: [
      { name: 'timestamp', description: 'Recon time' },
      { name: 'node_id', description: 'Node identifier' },
      { name: 'agent_short_name', description: 'Agent short name' },
      { name: 'is_semantic', description: 'Semantic recon' },
      { name: 'mcp_server_count', description: 'MCP servers found' },
      { name: 'skill_count', description: 'Skills found' },
      { name: 'internal_tool_count', description: 'Internal tools found' },
      { name: 'config_count', description: 'Config items found' },
      { name: 'session_count', description: 'Sessions found' },
      { name: 'project_path_count', description: 'Project paths found' },
    ],
  },
  {
    name: 'ReconMetadataLogs',
    description: 'User identities and API keys from recon',
    source: 'Database',
    columns: [
      { name: 'timestamp', description: 'Recon time' },
      { name: 'node_id', description: 'Node identifier' },
      { name: 'agent_short_name', description: 'Agent short name' },
      { name: 'entry_type', description: 'user_identity or api_key' },
      { name: 'value', description: 'The identity or key' },
    ],
  },
  {
    name: 'ReconSessionLogs',
    description: 'Sessions discovered during recon',
    source: 'Database',
    columns: [
      { name: 'timestamp', description: 'Recon time' },
      { name: 'node_id', description: 'Node identifier' },
      { name: 'agent_short_name', description: 'Agent short name' },
      { name: 'session_id', description: 'Session identifier' },
      { name: 'context_path', description: 'Project/context path' },
      { name: 'last_modified', description: 'Session last modified' },
      { name: 'message_count', description: 'Messages in session' },
    ],
  },
  {
    name: 'ReconToolLogs',
    description: 'Individual tools from recon (MCP, skills, internal)',
    source: 'Database',
    columns: [
      { name: 'timestamp', description: 'Recon time' },
      { name: 'node_id', description: 'Node identifier' },
      { name: 'agent_short_name', description: 'Agent short name' },
      { name: 'tool_type', description: 'mcp, skill, or internal' },
      { name: 'server_name', description: 'MCP server (null for skills)' },
      { name: 'tool_name', description: 'Tool name' },
      { name: 'tool_description', description: 'Tool description' },
      { name: 'transport', description: 'MCP transport type' },
    ],
  },
  {
    name: 'TrafficLogs',
    description: 'Intercepted HTTP traffic',
    source: 'Database',
    columns: [
      { name: 'timestamp', description: 'Capture time' },
      { name: 'traffic_id', description: 'Traffic entry ID' },
      { name: 'node_id', description: 'Capturing node' },
      { name: 'agent_short_name', description: 'Associated agent' },
      { name: 'intercept_method', description: 'proxy, vpn, hosts, tproxy' },
      { name: 'direction', description: 'send or receive' },
      { name: 'method', description: 'HTTP method' },
      { name: 'url', description: 'Full URL' },
      { name: 'host', description: 'Host/domain' },
      { name: 'request_headers', description: 'Headers (JSON)' },
      { name: 'request_body', description: 'Body (text)' },
      { name: 'response_status', description: 'HTTP status code' },
      { name: 'response_headers', description: 'Headers (JSON)' },
      { name: 'response_body', description: 'Body (text)' },
    ],
  },
  {
    name: 'TrafficMatchLogs',
    description: 'Traffic matching intercept rules',
    source: 'Database',
    columns: [
      { name: 'timestamp', description: 'Match time' },
      { name: 'traffic_id', description: 'Correlates to TrafficLogs.id' },
      { name: 'node_id', description: 'Capturing node' },
      { name: 'agent_short_name', description: 'Associated agent' },
      { name: 'rule_id', description: 'Matching rule ID' },
      { name: 'rule_name', description: 'Matching rule name' },
      { name: 'summary', description: 'LLM summary (if configured)' },
      { name: 'method', description: 'HTTP method' },
      { name: 'url', description: 'Full URL' },
      { name: 'host', description: 'Host/domain' },
      { name: 'direction', description: 'send or receive' },
      { name: 'response_status', description: 'HTTP status code' },
    ],
  },
];

function TableReference() {
  const [expandedTable, setExpandedTable] = useState<string | null>(null);

  return (
    <div className="flex flex-col h-full">
      <div className="px-3 py-2 border-b border-subtle bg-[var(--bg-tertiary)]">
        <div className="flex items-center gap-2 text-[10px] text-muted tracking-wider">
          <BookOpen size={10} />
          TABLES
        </div>
      </div>
      <div className="flex-1 overflow-y-auto">
        {TABLES.map((table) => {
          const isExpanded = expandedTable === table.name;
          return (
            <div key={table.name}>
              <button
                onClick={() => setExpandedTable(isExpanded ? null : table.name)}
                className={`w-full flex items-start gap-2 px-3 py-2 text-left transition-colors border-b border-dim ${
                  isExpanded ? 'bg-[var(--highlight)]' : 'hover:bg-[var(--highlight)]'
                }`}
              >
                <ChevronRight
                  size={10}
                  className={`text-muted flex-shrink-0 mt-0.5 transition-transform ${isExpanded ? 'rotate-90' : ''}`}
                />
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5">
                    <span className="text-xs text-highlight font-mono">
                      {table.name}
                    </span>
                    <span className="text-[9px] text-muted opacity-60">
                      {table.source === 'Database' ? 'DB' : 'MEM'}
                    </span>
                  </div>
                  <div className="text-[10px] text-muted mt-0.5">{table.description}</div>
                </div>
              </button>
              {isExpanded && (
                <div className="px-3 pl-7 py-2 border-b border-dim bg-[var(--bg-tertiary)]/50">
                  <div className="space-y-1">
                    {table.columns.map((col) => (
                      <div key={col.name} className="flex items-baseline gap-2">
                        <code className="text-[10px] text-highlight font-mono flex-shrink-0">{col.name}</code>
                        <span className="text-[10px] text-muted leading-tight">{col.description}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

//
// Draggable horizontal separator between query editor and results.
//

function HorizontalResizeHandle({ onDrag }: { onDrag: (deltaY: number) => void }) {
  const draggingRef = useRef(false);
  const startYRef = useRef(0);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    draggingRef.current = true;
    startYRef.current = e.clientY;

    const onMouseMove = (ev: MouseEvent) => {
      if (!draggingRef.current) return;
      const delta = ev.clientY - startYRef.current;
      if (delta !== 0) {
        startYRef.current = ev.clientY;
        onDrag(delta);
      }
    };

    const onMouseUp = () => {
      draggingRef.current = false;
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', onMouseUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };

    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
    document.body.style.cursor = 'row-resize';
    document.body.style.userSelect = 'none';
  }, [onDrag]);

  return (
    <div
      onMouseDown={handleMouseDown}
      className="flex-shrink-0 h-[7px] cursor-row-resize group relative border-b border-subtle bg-[var(--bg-secondary)] hover:bg-[var(--highlight)] transition-colors"
    >
      <div className="absolute inset-x-0 top-1/2 -translate-y-1/2 flex justify-center">
        <GripHorizontal size={12} className="text-[var(--border-subtle)] group-hover:text-[var(--text-muted)] transition-colors" />
      </div>
    </div>
  );
}

export function HuntingPage() {
  const { state, huntingQuery, huntingSetQuery } = useApp();
  const query = state.hunting.query;
  const [showReference, setShowReference] = useState(false);
  const [editorHeight, setEditorHeight] = useState(DEFAULT_EDITOR_HEIGHT);
  const resultsRef = useRef<HTMLDivElement>(null);
  const [resultsMaxHeight, setResultsMaxHeight] = useState('70vh');

  const handleRun = useCallback(() => {
    if (query.trim() && !state.hunting.isRunning) {
      huntingQuery(query.trim());
    }
  }, [query, state.hunting.isRunning, huntingQuery]);

  const handleEditorResize = useCallback((deltaY: number) => {
    setEditorHeight((prev) => Math.min(MAX_EDITOR_HEIGHT, Math.max(MIN_EDITOR_HEIGHT, prev + deltaY)));
  }, []);

  //
  // Compute the results table max height so it never extends below the
  // viewport. Recalculate when the editor height changes or the window
  // resizes.
  //

  useEffect(() => {
    const update = () => {
      if (resultsRef.current) {
        const top = resultsRef.current.getBoundingClientRect().top;
        const padding = 24; // bottom padding matching page padding
        const available = window.innerHeight - top - padding;
        setResultsMaxHeight(`${Math.max(200, available)}px`);
      }
    };
    update();
    window.addEventListener('resize', update);
    return () => window.removeEventListener('resize', update);
  }, [editorHeight, state.hunting.error]);

  return (
    <div className="space-y-4 md:space-y-6">
      {/*
      //
      // Header — matches intercept page style.
      //
      */}
      <div>
        <h1 className="text-2xl font-bold text-highlight">Hunting</h1>
        <p className="text-muted mt-1">KQL query interface</p>
      </div>

      {/*
      //
      // Editor + Reference panel.
      //
      */}
      <div className="flex border border-subtle ascii-box" style={{ height: editorHeight }}>
        <div className="flex-1 flex flex-col min-w-0">
          <div className="flex items-center gap-2 px-4 py-1.5 border-b border-subtle bg-[var(--bg-secondary)]">
            <button
              onClick={handleRun}
              disabled={state.hunting.isRunning || !query.trim()}
              className="flex items-center gap-1.5 px-3 py-1 text-xs bg-[var(--accent-success)]/20 text-[var(--accent-success)] border border-[var(--accent-success)]/30 hover:bg-[var(--accent-success)]/30 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
              title="Run query (Ctrl+Enter)"
            >
              {state.hunting.isRunning ? (
                <Loader2 size={12} className="animate-spin" />
              ) : (
                <Play size={12} />
              )}
              RUN
            </button>
            <span className="text-[10px] text-muted tracking-wider">CTRL+ENTER</span>
            <div className="flex-1" />
            <button
              onClick={() => setShowReference(!showReference)}
              className={`flex items-center gap-1.5 px-2 py-1 text-[10px] tracking-wider transition-colors ${
                showReference
                  ? 'text-title bg-[var(--highlight)] border border-subtle'
                  : 'text-muted hover:text-title border border-transparent'
              }`}
            >
              <BookOpen size={10} />
              SCHEMA
            </button>
          </div>
          <KqlCodeEditor
            value={query}
            onChange={huntingSetQuery}
            onCtrlEnter={handleRun}
            readOnly={state.hunting.isRunning}
          />
        </div>

        {showReference && (
          <div className="w-72 border-l border-subtle flex-shrink-0">
            <TableReference />
          </div>
        )}
      </div>

      {/*
      //
      // Draggable separator between editor and results.
      //
      */}
      <HorizontalResizeHandle onDrag={handleEditorResize} />

      {/*
      //
      // Error display.
      //
      */}
      {state.hunting.error && (
        <div className="flex items-center gap-2 px-4 py-2 bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 ascii-box text-xs text-[var(--accent-error)]">
          <AlertTriangle size={12} />
          {state.hunting.error}
        </div>
      )}

      {/*
      //
      // Results table.
      //
      */}
      <div ref={resultsRef} className="border border-subtle ascii-box flex flex-col overflow-hidden" style={{ maxHeight: resultsMaxHeight }}>
        <HuntingResultsTable
          columns={state.hunting.columns}
          rows={state.hunting.rows}
          totalCount={state.hunting.totalCount}
        />
      </div>
    </div>
  );
}
