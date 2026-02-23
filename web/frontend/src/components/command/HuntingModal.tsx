import { useState, useCallback, useRef } from 'react';
import { Play, Loader2, AlertTriangle, BookOpen, ChevronRight, GripHorizontal } from 'lucide-react';
import { Modal } from '../common/Modal';
import { useApp } from '../../context/AppContext';
import { KqlCodeEditor } from '../hunting/KqlCodeEditor';
import { HuntingResultsTable } from '../hunting/HuntingResultsTable';

const DEFAULT_EDITOR_HEIGHT = 160;
const MIN_EDITOR_HEIGHT = 60;
const MAX_EDITOR_HEIGHT = 400;

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
      { name: 'config_count', description: 'Config items found' },
    ],
  },
  {
    name: 'ReconToolLogs',
    description: 'Individual tools from recon',
    source: 'Database',
    columns: [
      { name: 'timestamp', description: 'Recon time' },
      { name: 'node_id', description: 'Node identifier' },
      { name: 'agent_short_name', description: 'Agent short name' },
      { name: 'tool_type', description: 'mcp, skill, or internal' },
      { name: 'server_name', description: 'MCP server name' },
      { name: 'tool_name', description: 'Tool name' },
    ],
  },
  {
    name: 'TrafficLogs',
    description: 'Intercepted HTTP traffic',
    source: 'Database',
    columns: [
      { name: 'timestamp', description: 'Capture time' },
      { name: 'node_id', description: 'Capturing node' },
      { name: 'method', description: 'HTTP method' },
      { name: 'url', description: 'Full URL' },
      { name: 'host', description: 'Host/domain' },
      { name: 'response_status', description: 'HTTP status code' },
    ],
  },
  {
    name: 'TrafficMatchLogs',
    description: 'Traffic matching intercept rules',
    source: 'Database',
    columns: [
      { name: 'timestamp', description: 'Match time' },
      { name: 'rule_name', description: 'Matching rule name' },
      { name: 'summary', description: 'LLM summary' },
      { name: 'method', description: 'HTTP method' },
      { name: 'url', description: 'Full URL' },
    ],
  },
];

function TableReference() {
  const [expandedTable, setExpandedTable] = useState<string | null>(null);

  return (
    <div className="flex flex-col h-full">
      <div className="px-2 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)]">
        <div className="flex items-center gap-1.5 text-[9px] text-muted tracking-wider">
          <BookOpen size={9} />
          TABLES
        </div>
      </div>
      <div className="flex-1 overflow-y-auto">
        {TABLES.map(table => {
          const isExpanded = expandedTable === table.name;
          return (
            <div key={table.name}>
              <button
                onClick={() => setExpandedTable(isExpanded ? null : table.name)}
                className={`w-full flex items-start gap-1.5 px-2 py-1.5 text-left transition-colors border-b border-dim ${
                  isExpanded ? 'bg-[var(--highlight)]' : 'hover:bg-[var(--highlight)]'
                }`}
              >
                <ChevronRight
                  size={9}
                  className={`text-muted flex-shrink-0 mt-0.5 transition-transform ${isExpanded ? 'rotate-90' : ''}`}
                />
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1">
                    <span className="text-[10px] text-highlight font-mono">{table.name}</span>
                    <span className="text-[8px] text-muted opacity-60">
                      {table.source === 'Database' ? 'DB' : 'MEM'}
                    </span>
                  </div>
                  <div className="text-[9px] text-muted mt-0.5">{table.description}</div>
                </div>
              </button>
              {isExpanded && (
                <div className="px-2 pl-5 py-1.5 border-b border-dim bg-[var(--bg-tertiary)]/50">
                  <div className="space-y-0.5">
                    {table.columns.map(col => (
                      <div key={col.name} className="flex items-baseline gap-1.5">
                        <code className="text-[9px] text-highlight font-mono flex-shrink-0">{col.name}</code>
                        <span className="text-[9px] text-muted">{col.description}</span>
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
      className="flex-shrink-0 h-[5px] cursor-row-resize group relative bg-[var(--bg-secondary)] hover:bg-[var(--highlight)] transition-colors"
    >
      <div className="absolute inset-x-0 top-1/2 -translate-y-1/2 flex justify-center">
        <GripHorizontal size={10} className="text-[var(--border-subtle)] group-hover:text-[var(--text-muted)] transition-colors" />
      </div>
    </div>
  );
}

interface HuntingModalProps {
  onClose: () => void;
}

export function HuntingModal({ onClose }: HuntingModalProps) {
  const { state, huntingQuery, huntingSetQuery } = useApp();
  const query = state.hunting.query;
  const [showReference, setShowReference] = useState(false);
  const [editorHeight, setEditorHeight] = useState(DEFAULT_EDITOR_HEIGHT);

  const handleRun = useCallback(() => {
    if (query.trim() && !state.hunting.isRunning) {
      huntingQuery(query.trim());
    }
  }, [query, state.hunting.isRunning, huntingQuery]);

  const handleEditorResize = useCallback((deltaY: number) => {
    setEditorHeight(prev => Math.min(MAX_EDITOR_HEIGHT, Math.max(MIN_EDITOR_HEIGHT, prev + deltaY)));
  }, []);

  return (
    <Modal
      isOpen={true}
      onClose={onClose}
      title="Hunting"
      size="full"
      noPadding
    >
      <div className="flex flex-col h-[80vh]">
        {/*
        //
        // Editor + schema reference panel.
        //
        */}
        <div className="flex border-b border-subtle flex-shrink-0" style={{ height: editorHeight }}>
          <div className="flex-1 flex flex-col min-w-0">
            <div className="flex items-center gap-2 px-3 py-1 border-b border-subtle bg-[var(--bg-secondary)]">
              <button
                onClick={handleRun}
                disabled={state.hunting.isRunning || !query.trim()}
                className="flex items-center gap-1 px-2 py-0.5 text-[10px] bg-[var(--accent-success)]/20 text-[var(--accent-success)] border border-[var(--accent-success)]/30 hover:bg-[var(--accent-success)]/30 disabled:opacity-30 transition-colors"
                title="Run query (Ctrl+Enter)"
              >
                {state.hunting.isRunning
                  ? <Loader2 size={10} className="animate-spin" />
                  : <Play size={10} />}
                RUN
              </button>
              <span className="text-[9px] text-muted">CTRL+ENTER</span>
              <div className="flex-1" />
              <button
                onClick={() => setShowReference(!showReference)}
                className={`flex items-center gap-1 px-2 py-0.5 text-[9px] tracking-wider transition-colors ${
                  showReference
                    ? 'text-title bg-[var(--highlight)] border border-subtle'
                    : 'text-muted hover:text-title border border-transparent'
                }`}
              >
                <BookOpen size={9} />
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
            <div className="w-56 border-l border-subtle flex-shrink-0">
              <TableReference />
            </div>
          )}
        </div>

        {/*
        //
        // Draggable separator.
        //
        */}
        <HorizontalResizeHandle onDrag={handleEditorResize} />

        {/*
        //
        // Error.
        //
        */}
        {state.hunting.error && (
          <div className="flex items-center gap-2 px-3 py-1.5 bg-[var(--accent-error)]/10 border-b border-[var(--accent-error)]/30 text-[10px] text-[var(--accent-error)] flex-shrink-0">
            <AlertTriangle size={10} />
            {state.hunting.error}
          </div>
        )}

        {/*
        //
        // Results table.
        //
        */}
        <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
          <HuntingResultsTable
            columns={state.hunting.columns}
            rows={state.hunting.rows}
            totalCount={state.hunting.totalCount}
          />
        </div>
      </div>
    </Modal>
  );
}
