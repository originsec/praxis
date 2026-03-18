import { useEffect, useRef, useState, useCallback, type ReactNode } from 'react';
import { Copy } from 'lucide-react';
import { Modal } from '../common/Modal';
import { useApp } from '../../context/AppContext';
import type { SessionItem, ToolkitDiffHunk, ToolkitDiffLine, ToolkitTargetPreview } from '../../api/types';

//
// Word-level inline diff. Splits two strings into words (preserving whitespace
// tokens) and runs a simple LCS diff to find which segments actually changed.
// Returns ReactNode fragments with changed words wrapped in a highlight span.
//

type WordSpan = { text: string; changed: boolean };

function diffWords(oldStr: string, newStr: string): { oldSpans: WordSpan[]; newSpans: WordSpan[] } {
  const tokenize = (s: string) => s.match(/\S+|\s+/g) || [];
  const oldToks = tokenize(oldStr);
  const newToks = tokenize(newStr);

  //
  // LCS table to find common subsequence.
  //

  const m = oldToks.length;
  const n = newToks.length;
  const dp: number[][] = Array.from({ length: m + 1 }, () => Array(n + 1).fill(0));
  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      dp[i][j] = oldToks[i - 1] === newToks[j - 1]
        ? dp[i - 1][j - 1] + 1
        : Math.max(dp[i - 1][j], dp[i][j - 1]);
    }
  }

  //
  // Backtrack to mark which tokens are common.
  //

  const oldChanged = new Array(m).fill(true);
  const newChanged = new Array(n).fill(true);
  let i = m, j = n;
  while (i > 0 && j > 0) {
    if (oldToks[i - 1] === newToks[j - 1]) {
      oldChanged[i - 1] = false;
      newChanged[j - 1] = false;
      i--; j--;
    } else if (dp[i - 1][j] >= dp[i][j - 1]) {
      i--;
    } else {
      j--;
    }
  }

  const oldSpans = oldToks.map((text, idx) => ({ text, changed: oldChanged[idx] }));
  const newSpans = newToks.map((text, idx) => ({ text, changed: newChanged[idx] }));
  return { oldSpans, newSpans };
}

function renderSpans(spans: WordSpan[], highlightClass: string): ReactNode {
  return spans.map((span, i) =>
    span.changed
      ? <span key={i} className={highlightClass}>{span.text}</span>
      : <span key={i}>{span.text}</span>
  );
}

//
// Side-by-side diff view rendered from server-computed diff_hunks. Pairs
// removed/added lines into change rows with word-level highlights.
//

type SideBySideRow =
  | { type: 'context'; lineNo: number; content: string }
  | { type: 'change'; oldLineNo: number | null; oldContent: string; newLineNo: number | null; newContent: string }
  | { type: 'separator' };

function hunkToRows(hunk: ToolkitDiffHunk): SideBySideRow[] {
  const rows: SideBySideRow[] = [];
  const lines = hunk.lines;
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    if (line.kind === 'Context') {
      rows.push({ type: 'context', lineNo: line.old_line_no ?? line.new_line_no ?? 0, content: line.content });
      i++;
      continue;
    }

    //
    // Collect a contiguous block of Removed then Added lines and pair them.
    //

    const removed: ToolkitDiffLine[] = [];
    const added: ToolkitDiffLine[] = [];

    while (i < lines.length && lines[i].kind === 'Removed') {
      removed.push(lines[i]);
      i++;
    }
    while (i < lines.length && lines[i].kind === 'Added') {
      added.push(lines[i]);
      i++;
    }

    const maxLen = Math.max(removed.length, added.length);
    for (let k = 0; k < maxLen; k++) {
      const r = removed[k];
      const a = added[k];
      rows.push({
        type: 'change',
        oldLineNo: r?.old_line_no ?? null,
        oldContent: r?.content ?? '',
        newLineNo: a?.new_line_no ?? null,
        newContent: a?.content ?? '',
      });
    }
  }

  return rows;
}

const GUTTER = 'text-muted/40 w-8 text-right pr-1 select-none shrink-0 border-r border-dim';

function DiffHunkView({ hunks }: { hunks: ToolkitDiffHunk[] }) {
  if (hunks.length === 0) {
    return <div className="p-6 text-center text-sm text-muted">No differences found.</div>;
  }

  return (
    <div className="font-mono text-xs leading-[1.6]">
      {hunks.map((hunk, hunkIdx) => {
        const rows = hunkToRows(hunk);

        return (
          <div key={hunkIdx}>
            {hunkIdx > 0 && (
              <div className="bg-[var(--bg-primary)] px-3 py-0.5 text-muted text-center select-none">
                &#8942;
              </div>
            )}

            <div className="grid grid-cols-2 bg-[var(--bg-tertiary)] border-y border-dim select-none">
              <div className="px-3 py-1 text-muted">
                @@ -{hunk.old_start},{hunk.old_len} @@
              </div>
              <div className="px-3 py-1 text-muted border-l border-dim">
                @@ +{hunk.new_start},{hunk.new_len} @@
              </div>
            </div>

            {rows.map((row, rowIdx) => {
              if (row.type === 'separator') {
                return (
                  <div key={rowIdx} className="grid grid-cols-2 border-y border-dim">
                    <div className="px-3 py-0.5 text-muted text-center">...</div>
                    <div className="px-3 py-0.5 text-muted text-center border-l border-dim">...</div>
                  </div>
                );
              }

              if (row.type === 'context') {
                return (
                  <div key={rowIdx} className="grid grid-cols-2">
                    <div className="flex">
                      <span className={GUTTER}>{row.lineNo}</span>
                      <span className="flex-1 whitespace-pre-wrap break-all px-1.5 text-highlight">{row.content}</span>
                    </div>
                    <div className="flex border-l border-dim">
                      <span className={GUTTER}>{row.lineNo}</span>
                      <span className="flex-1 whitespace-pre-wrap break-all px-1.5 text-highlight">{row.content}</span>
                    </div>
                  </div>
                );
              }

              //
              // Change row — do word-level diff if both sides have content.
              //

              const hasOld = row.oldContent !== '';
              const hasNew = row.newContent !== '';
              const hasBoth = hasOld && hasNew;

              let oldNode: ReactNode = row.oldContent;
              let newNode: ReactNode = row.newContent;

              if (hasBoth) {
                const { oldSpans, newSpans } = diffWords(row.oldContent, row.newContent);
                oldNode = renderSpans(oldSpans, 'bg-[var(--accent-error)]/30 text-[var(--accent-error)]');
                newNode = renderSpans(newSpans, 'bg-[var(--accent-success)]/30 text-[var(--accent-success)]');
              }

              return (
                <div key={rowIdx} className="grid grid-cols-2">
                  <div className={`flex ${hasOld ? 'bg-[var(--accent-error)]/8' : ''}`}>
                    <span className={GUTTER}>{row.oldLineNo ?? ''}</span>
                    <span className={`flex-1 whitespace-pre-wrap break-all px-1.5 ${hasOld ? 'text-[var(--accent-error)]' : ''}`}>
                      {oldNode}
                    </span>
                  </div>
                  <div className={`flex border-l border-dim ${hasNew ? 'bg-[var(--accent-success)]/8' : ''}`}>
                    <span className={GUTTER}>{row.newLineNo ?? ''}</span>
                    <span className={`flex-1 whitespace-pre-wrap break-all px-1.5 ${hasNew ? 'text-[var(--accent-success)]' : ''}`}>
                      {newNode}
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
        );
      })}
    </div>
  );
}

interface SessionHistoryPoisoningModalProps {
  isOpen: boolean;
  onClose: () => void;
  description: string;
}

export function SessionHistoryPoisoningModal({ isOpen, onClose, description }: SessionHistoryPoisoningModalProps) {
  const { state, send } = useApp();

  const [selectedNodeId, setSelectedNodeId] = useState('');
  const [selectedAgent, setSelectedAgent] = useState('');
  const [selectedSessionFile, setSelectedSessionFile] = useState('');
  const [selectedModelRef, setSelectedModelRef] = useState('');
  const [maxTokens, setMaxTokens] = useState(50000);
  const [loadingRecon, setLoadingRecon] = useState(false);
  const [loadingRun, setLoadingRun] = useState(false);
  const [loadingApply, setLoadingApply] = useState(false);

  const nodes = state.systemState?.nodes ?? [];
  const selectedNode = nodes.find((n) => n.node_id === selectedNodeId);
  const agents = selectedNode?.discovered_agents.filter((a) => a.available) ?? [];

  const reconTarget = state.toolkit.reconTargets.find(
    (t) => t.node_id === selectedNodeId && t.agent_short_name === selectedAgent
  );
  const sessions: SessionItem[] = reconTarget?.sessions ?? [];
  const selectedSession = sessions.find((s) => s.session_file === selectedSessionFile) ?? null;

  const execResult = state.toolkit.executeResult?.tool_name === 'session_history_poisoning'
    ? state.toolkit.executeResult : null;
  const preview: ToolkitTargetPreview | undefined = execResult?.previews[0];
  const diffHunks: ToolkitDiffHunk[] = preview?.diff_hunks ?? [];
  const applyResults = state.toolkit.applyResults;

  //
  // Auto-trigger recon when both node and agent are selected.
  //

  const reconTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const triggerRecon = useCallback(() => {
    if (!selectedNodeId || !selectedAgent) return;
    setLoadingRecon(true);
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
  }, [selectedNodeId, selectedAgent, send]);

  useEffect(() => {
    if (!selectedNodeId || !selectedAgent) return;
    if (reconTimerRef.current) clearTimeout(reconTimerRef.current);
    reconTimerRef.current = setTimeout(triggerRecon, 300);
    return () => { if (reconTimerRef.current) clearTimeout(reconTimerRef.current); };
  }, [selectedNodeId, selectedAgent, triggerRecon]);

  //
  // Clear loading states on actual responses.
  //

  useEffect(() => {
    if (reconTarget || state.toolkit.error) setLoadingRecon(false);
  }, [reconTarget, state.toolkit.error]);

  useEffect(() => {
    if (execResult || state.toolkit.error) setLoadingRun(false);
  }, [execResult, state.toolkit.error]);

  useEffect(() => {
    if (applyResults || state.toolkit.error) setLoadingApply(false);
  }, [applyResults, state.toolkit.error]);

  const canExecute = selectedNodeId && selectedAgent && selectedSession && selectedModelRef && !loadingRun;
  const canApply = execResult && preview?.preview_content && !loadingApply;

  const runPreview = () => {
    if (!canExecute) return;
    setLoadingRun(true);
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
        max_tokens: maxTokens,
        targets: [{
          node_id: selectedNodeId,
          agent_short_name: selectedAgent,
          session_id: selectedSession!.session_id,
          session_file: selectedSession!.session_file,
        }],
      },
    });
  };

  const applyChanges = () => {
    if (!canApply || !preview) return;
    setLoadingApply(true);
    send({
      type: 'toolkit_apply',
      tool_name: 'session_history_poisoning',
      execution_id: execResult!.execution_id,
      targets: [{
        target: preview.target,
        content: preview.preview_content!,
      }],
    });
  };

  const selectCls = 'w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-sm text-highlight focus:outline-none focus:border-subtle';
  const labelCls = 'block text-[11px] tracking-wider text-[var(--text-secondary)] uppercase mb-1';

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="Session History Poisoning" size="full" noPadding>
      <div className="flex h-[80vh]">

        {/* Left sidebar */}
        <div className="w-[300px] shrink-0 border-r border-subtle flex flex-col overflow-auto">

          <div className="p-3 text-xs text-muted leading-relaxed border-b border-dim">
            {description}
          </div>

          <div className="p-3 space-y-2.5 flex-1">
            <div>
              <label className={labelCls}>Node</label>
              <select
                className={selectCls}
                value={selectedNodeId}
                onChange={(e) => {
                  setSelectedNodeId(e.target.value);
                  setSelectedAgent('');
                  setSelectedSessionFile('');
                }}
              >
                <option value="">-</option>
                {nodes.map((n) => (
                  <option key={n.node_id} value={n.node_id}>{n.machine_name} ({n.node_id.slice(0, 8)})</option>
                ))}
              </select>
            </div>

            <div>
              <label className={labelCls}>Agent</label>
              <select
                className={selectCls}
                value={selectedAgent}
                onChange={(e) => {
                  setSelectedAgent(e.target.value);
                  setSelectedSessionFile('');
                }}
              >
                <option value="">-</option>
                {agents.map((a) => (
                  <option key={a.short_name} value={a.short_name}>{a.name}</option>
                ))}
              </select>
            </div>

            <div>
              <label className={labelCls}>Session</label>
              <select
                className={selectCls}
                value={selectedSessionFile}
                onChange={(e) => setSelectedSessionFile(e.target.value)}
                disabled={sessions.length === 0 && !loadingRecon}
              >
                <option value="">{loadingRecon ? 'Scanning...' : sessions.length === 0 ? (selectedAgent ? 'No sessions' : '-') : 'Select session'}</option>
                {sessions.map((s) => (
                  <option key={s.session_file} value={s.session_file}>{s.session_id.slice(0, 12)}... ({s.message_count} msgs)</option>
                ))}
              </select>
            </div>

            <div>
              <label className={labelCls}>Model</label>
              <select
                className={selectCls}
                value={selectedModelRef}
                onChange={(e) => setSelectedModelRef(e.target.value)}
              >
                <option value="">-</option>
                {state.toolkit.models.map((m) => (
                  <option key={m.name} value={m.name}>{m.name}</option>
                ))}
              </select>
            </div>

            <div>
              <label className={labelCls}>Max Tokens</label>
              <input
                type="number"
                className={selectCls}
                value={maxTokens}
                onChange={(e) => setMaxTokens(Math.max(1, parseInt(e.target.value) || 50000))}
                min={1}
              />
            </div>
          </div>

          <div className="p-3 space-y-2 border-t border-dim">
            <button
              className="w-full px-3 py-1.5 text-xs tracking-wider border border-dim bg-[var(--accent-warning)]/15 text-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/25 hover:border-[var(--accent-warning)]/50 transition-colors disabled:opacity-40 disabled:pointer-events-none"
              disabled={!canExecute}
              onClick={runPreview}
            >
              {loadingRun ? 'Executing...' : 'Execute'}
            </button>

            <button
              className="w-full px-3 py-1.5 text-xs tracking-wider border border-dim bg-[var(--accent-success)]/15 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/25 hover:border-[var(--accent-success)]/50 transition-colors disabled:opacity-40 disabled:pointer-events-none"
              disabled={!canApply}
              onClick={applyChanges}
            >
              {loadingApply ? 'Applying...' : 'Apply'}
            </button>
          </div>

          {state.toolkit.error && (
            <div className="px-3 py-2 bg-[var(--accent-error)]/10 border-t border-[var(--accent-error)]/30 text-[var(--accent-error)] text-xs">
              {state.toolkit.error}
            </div>
          )}

          {applyResults && (
            <div className="px-3 py-2 border-t border-dim">
              {applyResults.map((r, i) => (
                <div key={i} className={`text-xs ${r.success ? 'text-[var(--accent-success)]' : 'text-[var(--accent-error)]'}`}>
                  {r.success ? 'Applied successfully' : `Failed: ${r.error}`}
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Right panel - diff view */}
        <div className="flex-1 flex flex-col min-w-0 overflow-hidden">

          <div className="flex items-center justify-between px-3 py-1.5 bg-[var(--bg-tertiary)] border-b border-dim shrink-0">
            <span className="text-[11px] tracking-wider text-[var(--text-secondary)] uppercase">Diff Preview</span>
            {loadingRun && state.toolkit.executionProgress && (
              <span className="text-[10px] tracking-wider text-[var(--accent-warning)]">
                Processing message {state.toolkit.executionProgress.current}/{state.toolkit.executionProgress.total}...
              </span>
            )}
            {loadingRun && !state.toolkit.executionProgress && (
              <span className="text-[10px] tracking-wider text-[var(--accent-warning)]">running...</span>
            )}
            {applyResults && applyResults.every(r => r.success) && (
              <span className="text-[10px] tracking-wider text-[var(--accent-success)]">applied</span>
            )}
          </div>

          <div className="flex-1 overflow-auto">
            {!preview && !loadingRun && (
              <div className="flex items-center justify-center h-full text-sm text-muted">
                Select targets and execute to preview changes.
              </div>
            )}

            {loadingRun && !preview && (
              <div className="flex items-center justify-center h-full text-sm text-muted">
                {state.toolkit.executionProgress
                  ? `Processing message ${state.toolkit.executionProgress.current}/${state.toolkit.executionProgress.total}...`
                  : 'Running LLM transform...'}
              </div>
            )}

            {preview?.error && (
              <div className="m-3 p-2.5 bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 text-[var(--accent-error)] text-xs">
                {preview.error}
              </div>
            )}

            {preview?.preview_content && diffHunks.length > 0 && (
              <DiffHunkView hunks={diffHunks} />
            )}

            {preview?.preview_content && diffHunks.length === 0 && (
              <div className="flex items-center justify-center h-full text-sm text-muted">
                No changes detected.
              </div>
            )}
          </div>
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

export function MessageEncoderModal({ isOpen, onClose, description }: MessageEncoderModalProps) {
  const { state, send } = useApp();
  const [input, setInput] = useState('');

  const encodingOptions = state.toolkit.tools
    .find(t => t.tool_name === 'message_encoder')
    ?.config_schema.find(f => f.name === 'encoding')
    ?.options ?? [];

  const [encoding, setEncoding] = useState(() =>
    encodingOptions[0]?.value ?? 'base64'
  );
  const [copied, setCopied] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const execResult = state.toolkit.executeResult?.tool_name === 'message_encoder'
    ? state.toolkit.executeResult : null;
  const output = execResult?.previews[0]?.preview_content ?? '';

  //
  // Auto-encode on input or encoding change (debounced).
  //

  useEffect(() => {
    if (!input.trim()) return;
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      send({
        type: 'toolkit_execute',
        tool_name: 'message_encoder',
        target_spec: {
          node_ids: [],
          os_filter: null,
          agent_short_names: [],
          include_triggering_node: false,
        },
        params: { input_text: input, encoding },
      });
    }, 150);
    return () => { if (debounceRef.current) clearTimeout(debounceRef.current); };
  }, [input, encoding, send]);

  const copyOutput = async () => {
    if (!output) return;
    await navigator.clipboard.writeText(output);
    setCopied(true);
    setTimeout(() => setCopied(false), 1000);
  };

  const labelCls = 'block text-[10px] tracking-wider text-[var(--text-secondary)] uppercase mb-1';
  const inputCls = 'w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle';

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="Message Encoder" size="md">
      <p className="text-[10px] text-muted mb-3 leading-relaxed">{description}</p>

      <div className="space-y-2.5">
        <div>
          <label className={labelCls}>Encoding</label>
          <select className={inputCls} value={encoding} onChange={(e) => setEncoding(e.target.value)}>
            {encodingOptions.map(opt => (
              <option key={opt.value} value={opt.value}>{opt.label}</option>
            ))}
          </select>
        </div>

        <div>
          <label className={labelCls}>Input</label>
          <textarea
            className={`${inputCls} h-24 resize-none`}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder="Type text to encode..."
          />
        </div>

        <div>
          <div className="flex items-center justify-between mb-1">
            <label className={labelCls}>Output</label>
            <button
              className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] tracking-wider border border-dim text-muted hover:text-highlight hover:border-subtle transition-colors disabled:opacity-40"
              onClick={copyOutput}
              disabled={!output}
            >
              <Copy size={10} /> {copied ? 'Copied' : 'Copy'}
            </button>
          </div>
          <textarea
            className={`${inputCls} h-24 font-mono resize-none`}
            readOnly
            value={output}
          />
        </div>

        {output && (
          <p className="text-[10px] text-muted/60 leading-relaxed">
            Some encodings produce invisible characters. The output may appear empty — use Copy to transfer it.
          </p>
        )}
      </div>
    </Modal>
  );
}
