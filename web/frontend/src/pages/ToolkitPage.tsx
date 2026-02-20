import { useEffect, useMemo, useState } from 'react';
import { Code2, Copy, Eye, FileDiff, ShieldAlert } from 'lucide-react';
import { Modal } from '../components/common/Modal';
import { useApp } from '../context/AppContext';
import type { SessionItem, ToolkitToolInfo } from '../api/types';

function toolIcon(toolName: string) {
  if (toolName === 'session_history_poisoning') return ShieldAlert;
  if (toolName === 'message_encoder') return Code2;
  return Eye;
}

function prettyPrintSession(content: string): string {
  const trimmed = content.trim();
  if (!trimmed) return content;

  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    // Continue to line-level JSONL pretty-print fallback.
  }

  const lines = content.split('\n');
  let hasJsonLine = false;
  const formatted = lines.map((line) => {
    const t = line.trim();
    if (!t) return line;
    try {
      const parsed = JSON.parse(t);
      hasJsonLine = true;
      return JSON.stringify(parsed, null, 2);
    } catch {
      return line;
    }
  });

  return hasJsonLine ? formatted.join('\n') : content;
}

type DiffRow = {
  leftLineNo: number | null;
  rightLineNo: number | null;
  left: string;
  right: string;
  kind: 'same' | 'changed' | 'added' | 'removed';
};

type DiffDisplayRow =
  | { type: 'row'; row: DiffRow }
  | { type: 'separator'; key: string };

function buildLineDiff(originalText: string, updatedText: string): DiffRow[] {
  const left = prettyPrintSession(originalText).split('\n');
  const right = prettyPrintSession(updatedText).split('\n');
  const max = Math.max(left.length, right.length);
  const rows: DiffRow[] = [];

  for (let i = 0; i < max; i += 1) {
    const l = left[i];
    const r = right[i];
    if (l === r) {
      rows.push({ leftLineNo: i + 1, rightLineNo: i + 1, left: l ?? '', right: r ?? '', kind: 'same' });
      continue;
    }
    if (l === undefined) {
      rows.push({ leftLineNo: null, rightLineNo: i + 1, left: '', right: r ?? '', kind: 'added' });
      continue;
    }
    if (r === undefined) {
      rows.push({ leftLineNo: i + 1, rightLineNo: null, left: l, right: '', kind: 'removed' });
      continue;
    }
    rows.push({ leftLineNo: i + 1, rightLineNo: i + 1, left: l, right: r, kind: 'changed' });
  }

  return rows;
}

function buildGitStyleDiffRows(rows: DiffRow[], contextLines = 3): DiffDisplayRow[] {
  const changedIndexes = rows
    .map((row, idx) => (row.kind === 'same' ? -1 : idx))
    .filter((idx) => idx >= 0);

  if (changedIndexes.length === 0) {
    return rows.map((row) => ({ type: 'row', row }));
  }

  const include = new Set<number>();
  for (const idx of changedIndexes) {
    const start = Math.max(0, idx - contextLines);
    const end = Math.min(rows.length - 1, idx + contextLines);
    for (let i = start; i <= end; i += 1) include.add(i);
  }

  const result: DiffDisplayRow[] = [];
  let prevIncluded: number | null = null;
  for (let i = 0; i < rows.length; i += 1) {
    if (!include.has(i)) continue;
    if (prevIncluded !== null && i - prevIncluded > 1) {
      result.push({ type: 'separator', key: `sep-${prevIncluded}-${i}` });
    }
    result.push({ type: 'row', row: rows[i] });
    prevIncluded = i;
  }

  return result;
}

interface SessionHistoryPoisoningModalProps {
  isOpen: boolean;
  onClose: () => void;
  description: string;
}

function SessionHistoryPoisoningModal({ isOpen, onClose, description }: SessionHistoryPoisoningModalProps) {
  const { state, send, sendCommand } = useApp();

  const [selectedNodeId, setSelectedNodeId] = useState('');
  const [selectedAgent, setSelectedAgent] = useState('');
  const [selectedSessionFile, setSelectedSessionFile] = useState('');
  const [selectedModelRef, setSelectedModelRef] = useState('');
  const [loadingRecon, setLoadingRecon] = useState(false);
  const [loadingRun, setLoadingRun] = useState(false);
  const [loadingApply, setLoadingApply] = useState(false);
  const [originalContent, setOriginalContent] = useState('');
  const [toolError, setToolError] = useState<string | null>(null);

  const nodes = state.systemState?.nodes ?? [];
  const selectedNode = nodes.find((n) => n.node_id === selectedNodeId);
  const agents = selectedNode?.discovered_agents.filter((a) => a.available) ?? [];

  const reconTarget = state.toolkit.reconTargets.find(
    (t) => t.node_id === selectedNodeId && t.agent_short_name === selectedAgent
  );
  const sessions: SessionItem[] = reconTarget?.sessions ?? [];
  const selectedSession = sessions.find((s) => s.session_file === selectedSessionFile) ?? null;

  const execution = state.toolkit.execution?.tool_name === 'session_history_poisoning' ? state.toolkit.execution : null;
  const preview = execution?.previews[0];
  const diffRows = useMemo(
    () => (preview?.preview_content ? buildLineDiff(originalContent, preview.preview_content) : []),
    [originalContent, preview?.preview_content]
  );
  const visibleDiffRows = useMemo(() => buildGitStyleDiffRows(diffRows, 3), [diffRows]);

  useEffect(() => {
    if (!isOpen) return;
    setToolError(null);
  }, [isOpen]);

  const runRecon = async () => {
    if (!selectedNodeId || !selectedAgent) return;
    setLoadingRecon(true);
    setToolError(null);
    send({
      type: 'toolkit_recon',
      tool_name: 'session_history_poisoning',
      target_spec: {
        node_ids: [selectedNodeId],
        os_filter: null,
        agent_short_names: [selectedAgent],
        include_triggering_node: false,
      },
    });
    setTimeout(() => setLoadingRecon(false), 400);
  };

  const runPreview = async () => {
    if (!selectedNodeId || !selectedAgent || !selectedSession || !selectedModelRef) return;

    setLoadingRun(true);
    setToolError(null);
    try {
      await sendCommand(selectedNodeId, { Agent: { Select: { short_name: selectedAgent } } });
      const readResp = await sendCommand(selectedNodeId, {
        Agent: { ReadFile: { file_type: 'Session', path: selectedSession.session_file } },
      });

      if ('Agent' in readResp.result && typeof readResp.result.Agent === 'object' && readResp.result.Agent !== null
        && 'ReadFileResult' in readResp.result.Agent) {
        const result = readResp.result.Agent.ReadFileResult;
        if (result.content) {
          setOriginalContent(result.content);
        } else {
          throw new Error(result.error || 'Failed to read session content');
        }
      }

      send({
        type: 'toolkit_execute',
        tool_name: 'session_history_poisoning',
        target_spec: {
          node_ids: [selectedNodeId],
          os_filter: null,
          agent_short_names: [selectedAgent],
          include_triggering_node: false,
        },
        params: {
          model_ref: selectedModelRef,
          targets: [{
            node_id: selectedNodeId,
            agent_short_name: selectedAgent,
            session_id: selectedSession.session_id,
            session_file: selectedSession.session_file,
          }],
        },
      });
    } catch (error) {
      setToolError(String(error));
    } finally {
      setLoadingRun(false);
    }
  };

  const applyChanges = () => {
    if (!execution) return;
    setLoadingApply(true);
    send({
      type: 'toolkit_apply',
      execution_id: execution.execution_id,
      decisions: preview ? [{ target: preview.target, accepted: true }] : [],
    });
    setTimeout(() => setLoadingApply(false), 400);
  };

  const changedCount = diffRows.filter((r) => r.kind !== 'same').length;

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="Session History Poisoning" size="full" noPadding>
      <div className="p-4 border-b border-subtle bg-[var(--bg-tertiary)]">
        <p className="text-sm text-muted">{description}</p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-[360px,1fr] h-[80vh]">
        <div className="border-r border-subtle p-4 space-y-3 overflow-auto">
          <h3 className="text-xs font-semibold text-title">Targeting</h3>

          <div>
            <label className="text-xs text-muted">Node</label>
            <select
              className="w-full mt-1 bg-[var(--surface-2)] border border-subtle rounded px-2 py-1"
              value={selectedNodeId}
              onChange={(e) => {
                setSelectedNodeId(e.target.value);
                setSelectedAgent('');
                setSelectedSessionFile('');
              }}
            >
              <option value="">Select node</option>
              {nodes.map((n) => (
                <option key={n.node_id} value={n.node_id}>{n.machine_name} ({n.node_id.slice(0, 8)})</option>
              ))}
            </select>
          </div>

          <div>
            <label className="text-xs text-muted">Agent</label>
            <select
              className="w-full mt-1 bg-[var(--surface-2)] border border-subtle rounded px-2 py-1"
              value={selectedAgent}
              onChange={(e) => {
                setSelectedAgent(e.target.value);
                setSelectedSessionFile('');
              }}
            >
              <option value="">Select agent</option>
              {agents.map((a) => (
                <option key={a.short_name} value={a.short_name}>{a.name}</option>
              ))}
            </select>
          </div>

          <button
            className="w-full px-3 py-2 rounded bg-[var(--accent-info)] text-black text-sm disabled:opacity-50"
            disabled={!selectedNodeId || !selectedAgent || loadingRecon}
            onClick={runRecon}
          >
            {loadingRecon ? 'Running Recon...' : 'Static Recon'}
          </button>

          <div>
            <label className="text-xs text-muted">Session</label>
            <select
              className="w-full mt-1 bg-[var(--surface-2)] border border-subtle rounded px-2 py-1"
              value={selectedSessionFile}
              onChange={(e) => setSelectedSessionFile(e.target.value)}
            >
              <option value="">Select session</option>
              {sessions.map((s) => (
                <option key={s.session_file} value={s.session_file}>{s.session_id} ({s.message_count} msgs)</option>
              ))}
            </select>
          </div>

          <div>
            <label className="text-xs text-muted">Model Definition</label>
            <select
              className="w-full mt-1 bg-[var(--surface-2)] border border-subtle rounded px-2 py-1"
              value={selectedModelRef}
              onChange={(e) => setSelectedModelRef(e.target.value)}
            >
              <option value="">Select model</option>
              {state.toolkit.models.map((m) => (
                <option key={m.name} value={m.name}>{m.name}</option>
              ))}
            </select>
          </div>

          <button
            className="w-full px-3 py-2 rounded bg-[var(--accent-warning)] text-black text-sm disabled:opacity-50"
            disabled={!selectedNodeId || !selectedAgent || !selectedSession || !selectedModelRef || loadingRun}
            onClick={runPreview}
          >
            {loadingRun ? 'Generating...' : 'Run Tool'}
          </button>

          <button
            className="w-full px-3 py-2 rounded bg-[var(--accent-success)] text-black text-sm disabled:opacity-50"
            disabled={!execution || !preview?.preview_content || loadingApply}
            onClick={applyChanges}
          >
            {loadingApply ? 'Applying...' : 'Accept and Overwrite'}
          </button>

          {(toolError || state.toolkit.error) && (
            <p className="text-xs text-[var(--accent-error)]">{toolError || state.toolkit.error}</p>
          )}
        </div>

        <div className="p-4 overflow-auto space-y-3">
          <div className="flex items-center justify-between">
            <h3 className="text-xs font-semibold text-title flex items-center gap-2">
              <FileDiff size={14} /> Preview Diff
            </h3>
            <span className="text-xs text-muted">Changed lines: {changedCount}</span>
          </div>

          {!preview?.preview_content && (
            <div className="text-sm text-muted border border-subtle rounded p-4">Run the tool to generate a preview.</div>
          )}

          {preview?.preview_content && (
            <div className="border border-subtle rounded overflow-hidden font-mono text-xs">
              <div className="grid grid-cols-2 bg-[var(--bg-tertiary)] border-b border-subtle">
                <div className="px-3 py-2 text-muted">Original</div>
                <div className="px-3 py-2 text-muted border-l border-subtle">Proposed</div>
              </div>
              <div className="max-h-[62vh] overflow-auto">
                {visibleDiffRows.map((entry, idx) => {
                  if (entry.type === 'separator') {
                    return (
                      <div
                        key={entry.key}
                        className="grid grid-cols-2 bg-[var(--bg-tertiary)]/70 border-y border-subtle"
                      >
                        <div className="px-3 py-1 text-muted">...</div>
                        <div className="px-3 py-1 text-muted border-l border-subtle">...</div>
                      </div>
                    );
                  }
                  const row = entry.row;
                  const bg = row.kind === 'same'
                    ? 'bg-transparent'
                    : row.kind === 'added'
                      ? 'bg-[var(--accent-success)]/12'
                      : row.kind === 'removed'
                        ? 'bg-[var(--accent-error)]/12'
                        : 'bg-[var(--accent-warning)]/12';
                  const marker = row.kind === 'added' ? '+' : row.kind === 'removed' ? '-' : row.kind === 'changed' ? '~' : ' ';
                  return (
                    <div key={`${idx}-${row.leftLineNo}-${row.rightLineNo}`} className={`grid grid-cols-2 ${bg}`}>
                      <div className="px-2 py-1.5 pr-3 whitespace-pre-wrap break-words">
                        <span className="text-muted mr-2 inline-block w-4 text-center">{marker}</span>
                        <span className="text-muted mr-2 inline-block w-8 text-right">{row.leftLineNo ?? ''}</span>
                        {row.left}
                      </div>
                      <div className="px-2 py-1.5 pl-3 whitespace-pre-wrap break-words border-l border-subtle/70">
                        <span className="text-muted mr-2 inline-block w-4 text-center">{marker}</span>
                        <span className="text-muted mr-2 inline-block w-8 text-right">{row.rightLineNo ?? ''}</span>
                        {row.right}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </div>
      </div>
    </Modal>
  );
}

interface MessageEncoderModalProps {
  isOpen: boolean;
  onClose: () => void;
  description: string;
}

function MessageEncoderModal({ isOpen, onClose, description }: MessageEncoderModalProps) {
  const { state, send } = useApp();
  const [input, setInput] = useState('');
  const [encoding, setEncoding] = useState('braille_us_type2');
  const [copied, setCopied] = useState(false);

  const execution = state.toolkit.execution?.tool_name === 'message_encoder' ? state.toolkit.execution : null;
  const output = execution?.previews[0]?.preview_content ?? '';

  const runEncode = () => {
    if (!input.trim()) return;
    send({
      type: 'toolkit_execute',
      tool_name: 'message_encoder',
      target_spec: {
        node_ids: [],
        os_filter: null,
        agent_short_names: [],
        include_triggering_node: false,
      },
      params: {
        input_text: input,
        encoding,
      },
    });
  };

  const copyOutput = async () => {
    if (!output) return;
    await navigator.clipboard.writeText(output);
    setCopied(true);
    setTimeout(() => setCopied(false), 1000);
  };

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="Message Encoder" size="lg">
      <p className="text-sm text-muted mb-4">{description}</p>

      <div className="space-y-4">
        <div>
          <label className="text-xs text-muted">Encoding</label>
          <select
            className="w-full mt-1 bg-[var(--surface-2)] border border-subtle rounded px-2 py-1"
            value={encoding}
            onChange={(e) => setEncoding(e.target.value)}
          >
            <option value="braille_us_type2">Braille (US Type 2)</option>
          </select>
        </div>

        <div>
          <label className="text-xs text-muted">Input</label>
          <textarea
            className="w-full h-36 mt-1 bg-[var(--surface-2)] border border-subtle rounded p-2 text-sm"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder="Type text to encode..."
          />
        </div>

        <button
          className="px-3 py-2 rounded bg-[var(--accent-warning)] text-black text-sm disabled:opacity-50"
          disabled={!input.trim()}
          onClick={runEncode}
        >
          Encode Message
        </button>

        <div>
          <div className="flex items-center justify-between mb-1">
            <label className="text-xs text-muted">Encoded Output</label>
            <button
              className="inline-flex items-center gap-1 px-2 py-1 text-xs rounded border border-subtle hover:bg-[var(--bg-secondary)]"
              onClick={copyOutput}
              disabled={!output}
            >
              <Copy size={12} /> {copied ? 'Copied' : 'Copy'}
            </button>
          </div>
          <textarea
            className="w-full h-36 bg-[var(--surface-2)] border border-subtle rounded p-2 text-sm font-mono"
            readOnly
            value={output}
          />
        </div>
      </div>
    </Modal>
  );
}

export function ToolkitPage() {
  const { state, send } = useApp();
  const [activeTool, setActiveTool] = useState<string | null>(null);

  useEffect(() => {
    send({ type: 'toolkit_list' });
  }, [send]);

  const tools = state.toolkit.tools;

  const openTool = (toolName: string) => setActiveTool(toolName);
  const closeTool = () => setActiveTool(null);

  const descriptionFor = (toolName: string) =>
    tools.find((t) => t.tool_name === toolName)?.description ?? '';

  return (
    <div className="space-y-6 h-full overflow-auto pb-8">
      <div>
        <h1 className="text-2xl font-bold text-highlight">Toolkit</h1>
        <p className="text-muted mt-1">Specialized offensive tools</p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
        {tools.map((tool: ToolkitToolInfo) => {
          const Icon = toolIcon(tool.tool_name);
          return (
            <button
              key={tool.tool_name}
              onClick={() => openTool(tool.tool_name)}
              className="text-left rounded border border-subtle bg-[var(--surface-1)] hover:bg-[var(--surface-2)] transition-colors p-4 ascii-box"
            >
              <div className="flex items-center gap-2 mb-2">
                <Icon size={18} className="text-[var(--accent-info)]" />
                <h2 className="text-sm font-semibold text-title">{tool.display_name}</h2>
              </div>
              <p className="text-xs text-muted leading-relaxed">{tool.description}</p>
            </button>
          );
        })}
      </div>

      {activeTool === 'session_history_poisoning' && (
        <SessionHistoryPoisoningModal
          isOpen
          onClose={closeTool}
          description={descriptionFor('session_history_poisoning')}
        />
      )}

      {activeTool === 'message_encoder' && (
        <MessageEncoderModal
          isOpen
          onClose={closeTool}
          description={descriptionFor('message_encoder')}
        />
      )}
    </div>
  );
}
