import { useEffect, useMemo, useState } from 'react';
import { useApp } from '../context/AppContext';
import type { SessionItem, ToolkitApplyDecision, ToolkitTargetRef } from '../api/types';

export function ToolkitPage() {
  const { state, send } = useApp();
  const [selectedTool, setSelectedTool] = useState<string>('session_history_poisoning');
  const [selectedNodeId, setSelectedNodeId] = useState<string>('');
  const [selectedAgent, setSelectedAgent] = useState<string>('');
  const [selectedSessionFile, setSelectedSessionFile] = useState<string>('');
  const [selectedModelRef, setSelectedModelRef] = useState<string>('');
  const [encoderInput, setEncoderInput] = useState<string>('');
  const [encoderType, setEncoderType] = useState<string>('braille_us_type2');
  const [acceptMap, setAcceptMap] = useState<Record<string, boolean>>({});

  useEffect(() => {
    send({ type: 'toolkit_list' });
  }, [send]);

  const nodes = state.systemState?.nodes ?? [];
  const selectedNode = nodes.find((n) => n.node_id === selectedNodeId);
  const availableAgents = selectedNode?.discovered_agents.filter((a) => a.available) ?? [];

  const reconTarget = state.toolkit.reconTargets.find(
    (t) => t.node_id === selectedNodeId && t.agent_short_name === selectedAgent
  );
  const sessions: SessionItem[] = reconTarget?.sessions ?? [];
  const selectedSession = sessions.find((s) => s.session_file === selectedSessionFile) ?? null;

  const canRecon = selectedNodeId.length > 0 && selectedAgent.length > 0;
  const poisoningMode = selectedTool === 'session_history_poisoning';
  const encoderMode = selectedTool === 'message_encoder';
  const canRun = poisoningMode
    ? canRecon && selectedSession !== null && selectedModelRef.length > 0
    : encoderMode && encoderInput.trim().length > 0;
  const execution = state.toolkit.execution;

  const previewKeys = useMemo(
    () =>
      (execution?.previews ?? []).map((p) => `${p.target.node_id}|${p.target.agent_short_name}|${p.target.session_file}`),
    [execution?.previews]
  );

  useEffect(() => {
    const next: Record<string, boolean> = {};
    for (const key of previewKeys) {
      next[key] = acceptMap[key] ?? true;
    }
    setAcceptMap(next);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [previewKeys.join(',')]);

  const runRecon = () => {
    if (!poisoningMode) return;
    if (!canRecon) return;
    send({
      type: 'toolkit_recon',
      tool_name: selectedTool,
      target_spec: {
        node_ids: [selectedNodeId],
        os_filter: null,
        agent_short_names: [selectedAgent],
        include_triggering_node: false,
      },
    });
  };

  const runPreview = () => {
    if (!canRun) return;
    if (poisoningMode) {
      if (!selectedSession) return;
      const target: ToolkitTargetRef = {
        node_id: selectedNodeId,
        agent_short_name: selectedAgent,
        session_id: selectedSession.session_id,
        session_file: selectedSession.session_file,
      };
      send({
        type: 'toolkit_execute',
        tool_name: selectedTool,
        target_spec: {
          node_ids: [selectedNodeId],
          os_filter: null,
          agent_short_names: [selectedAgent],
          include_triggering_node: false,
        },
        params: {
          model_ref: selectedModelRef,
          targets: [target],
        },
      });
      return;
    }

    send({
      type: 'toolkit_execute',
      tool_name: selectedTool,
      target_spec: {
        node_ids: [],
        os_filter: null,
        agent_short_names: [],
        include_triggering_node: false,
      },
      params: {
        input_text: encoderInput,
        encoding: encoderType,
      },
    });
  };

  const applyResults = () => {
    if (!execution) return;
    const decisions: ToolkitApplyDecision[] = execution.previews.map((p) => {
      const key = `${p.target.node_id}|${p.target.agent_short_name}|${p.target.session_file}`;
      return {
        target: p.target,
        accepted: !!acceptMap[key],
      };
    });
    send({
      type: 'toolkit_apply',
      execution_id: execution.execution_id,
      decisions,
    });
  };

  return (
    <div className="space-y-6 h-full overflow-auto pb-8">
      <div>
        <h1 className="text-2xl font-bold text-highlight">Toolkit</h1>
        <p className="text-muted mt-1">Run offensive toolkit workflows against selected sessions</p>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <div className="rounded border border-subtle p-4 space-y-3">
          <h2 className="text-sm font-semibold text-title">Selection</h2>

          <div>
            <label className="text-xs text-muted">Tool</label>
            <select className="w-full mt-1 bg-[var(--surface-2)] border border-subtle rounded px-2 py-1"
              value={selectedTool}
              onChange={(e) => setSelectedTool(e.target.value)}>
              {state.toolkit.tools.map((t) => (
                <option key={t.tool_name} value={t.tool_name}>{t.display_name}</option>
              ))}
            </select>
          </div>

          {poisoningMode && (
            <div>
              <label className="text-xs text-muted">Node</label>
              <select className="w-full mt-1 bg-[var(--surface-2)] border border-subtle rounded px-2 py-1"
                value={selectedNodeId}
                onChange={(e) => {
                  setSelectedNodeId(e.target.value);
                  setSelectedAgent('');
                  setSelectedSessionFile('');
                }}>
                <option value="">Select node</option>
                {nodes.map((n) => (
                  <option key={n.node_id} value={n.node_id}>{n.machine_name} ({n.node_id.slice(0, 8)})</option>
                ))}
              </select>
            </div>
          )}

          {poisoningMode && (
            <div>
              <label className="text-xs text-muted">Agent</label>
              <select className="w-full mt-1 bg-[var(--surface-2)] border border-subtle rounded px-2 py-1"
                value={selectedAgent}
                onChange={(e) => {
                  setSelectedAgent(e.target.value);
                  setSelectedSessionFile('');
                }}>
                <option value="">Select agent</option>
                {availableAgents.map((a) => (
                  <option key={a.short_name} value={a.short_name}>{a.name}</option>
                ))}
              </select>
            </div>
          )}

          {poisoningMode && (
            <button className="px-3 py-1 rounded bg-[var(--accent-info)] text-black text-sm disabled:opacity-50"
              disabled={!canRecon}
              onClick={runRecon}>
              Static Recon (Sessions)
            </button>
          )}

          {poisoningMode && (
            <div>
              <label className="text-xs text-muted">Session</label>
              <select className="w-full mt-1 bg-[var(--surface-2)] border border-subtle rounded px-2 py-1"
                value={selectedSessionFile}
                onChange={(e) => setSelectedSessionFile(e.target.value)}>
                <option value="">Select session</option>
                {sessions.map((s) => (
                  <option key={s.session_file} value={s.session_file}>
                    {s.session_id} ({s.message_count} msgs)
                  </option>
                ))}
              </select>
            </div>
          )}

          {poisoningMode && (
            <div>
              <label className="text-xs text-muted">Model Definition</label>
              <select className="w-full mt-1 bg-[var(--surface-2)] border border-subtle rounded px-2 py-1"
                value={selectedModelRef}
                onChange={(e) => setSelectedModelRef(e.target.value)}>
                <option value="">Select model</option>
                {state.toolkit.models.map((m) => (
                  <option key={m.name} value={m.name}>{m.name}</option>
                ))}
              </select>
            </div>
          )}

          {encoderMode && (
            <>
              <div>
                <label className="text-xs text-muted">Encoding</label>
                <select
                  className="w-full mt-1 bg-[var(--surface-2)] border border-subtle rounded px-2 py-1"
                  value={encoderType}
                  onChange={(e) => setEncoderType(e.target.value)}
                >
                  <option value="braille_us_type2">Braille (US Type 2)</option>
                </select>
              </div>
              <div>
                <label className="text-xs text-muted">Input Text</label>
                <textarea
                  value={encoderInput}
                  onChange={(e) => setEncoderInput(e.target.value)}
                  className="w-full h-28 mt-1 bg-[var(--surface-2)] border border-subtle rounded p-2 text-xs font-mono"
                />
              </div>
            </>
          )}

          <button className="px-3 py-1 rounded bg-[var(--accent-warning)] text-black text-sm disabled:opacity-50"
            disabled={!canRun}
            onClick={runPreview}>
            Run Preview
          </button>
        </div>

        <div className="rounded border border-subtle p-4 space-y-3">
          <h2 className="text-sm font-semibold text-title">Execution</h2>
          {!execution && <p className="text-sm text-muted">No execution yet.</p>}
          {execution && (
            <>
              <p className="text-xs text-muted">Execution ID: {execution.execution_id}</p>
              <p className="text-xs text-muted">Status: {execution.status}</p>

              {execution.previews.map((p) => {
                const key = `${p.target.node_id}|${p.target.agent_short_name}|${p.target.session_file}`;
                return (
                  <div key={key} className="border border-subtle rounded p-2 space-y-2">
                    <div className="flex items-center justify-between">
                      <span className="text-xs text-muted">
                        {p.target.agent_short_name} · {p.target.session_id}
                      </span>
                      <label className="text-xs flex items-center gap-1">
                        <input
                          type="checkbox"
                          checked={!!acceptMap[key]}
                          onChange={(e) => setAcceptMap((prev) => ({ ...prev, [key]: e.target.checked }))}
                        />
                        Accept
                      </label>
                    </div>
                    {p.error && <p className="text-xs text-[var(--accent-error)]">{p.error}</p>}
                    {p.preview_content && (
                      <textarea
                        readOnly
                        value={p.preview_content}
                        className="w-full h-40 bg-[var(--surface-2)] border border-subtle rounded p-2 text-xs font-mono"
                      />
                    )}
                  </div>
                );
              })}

              {poisoningMode && (
                <button
                  className="px-3 py-1 rounded bg-[var(--accent-success)] text-black text-sm"
                  onClick={applyResults}
                >
                  Apply Accepted
                </button>
              )}
            </>
          )}

          {state.toolkit.error && (
            <p className="text-xs text-[var(--accent-error)]">{state.toolkit.error}</p>
          )}
        </div>
      </div>
    </div>
  );
}
