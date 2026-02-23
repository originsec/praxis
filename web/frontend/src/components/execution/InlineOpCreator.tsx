import { useState } from 'react';
import { X, Save, Zap } from 'lucide-react';
import { useApp } from '../../context/AppContext';

interface InlineOpCreatorProps {
  onClose: () => void;
  onCreated?: (fullName: string) => void;
}

export function InlineOpCreator({ onClose, onCreated }: InlineOpCreatorProps) {
  const { send, state } = useApp();
  const [name, setName] = useState('');
  const [category, setCategory] = useState('custom');
  const [description, setDescription] = useState('');
  const [operationPrompt, setOperationPrompt] = useState('');
  const [mode, setMode] = useState<'single_prompt' | 'agent'>('single_prompt');
  const [agentIterations, setAgentIterations] = useState(5);
  const [timeout, setTimeout] = useState(120);
  const [yoloMode, setYoloMode] = useState(false);
  const [modelRef, setModelRef] = useState('');
  const [agentInfo, setAgentInfo] = useState('');
  const [saving, setSaving] = useState(false);

  const modelDefs = (() => {
    try {
      const raw = state.config.llm_model_definitions;
      if (!raw) return [];
      return JSON.parse(raw) as Array<{ name: string; provider: string; model: string }>;
    } catch {
      return [];
    }
  })();

  const canSave = name.trim() && operationPrompt.trim();

  const handleSave = () => {
    if (!canSave) return;
    setSaving(true);

    //
    // Build YAML content matching the service's expected format.
    //
    const yamlLines = [
      `name: "${name.trim()}"`,
      `category: "${category.trim() || 'custom'}"`,
      `description: "${description.trim()}"`,
      `agent_info: "${agentInfo.trim()}"`,
      `mode: "${mode}"`,
      `agent_iterations: ${agentIterations}`,
      `timeout: ${timeout}`,
      `yolo_mode: ${yoloMode}`,
    ];
    if (modelRef) {
      yamlLines.push(`model_ref: "${modelRef}"`);
    }
    yamlLines.push(`operation_prompt: |`);
    operationPrompt.split('\n').forEach(line => {
      yamlLines.push(`  ${line}`);
    });

    const content = yamlLines.join('\n');
    send({ type: 'op_def_add', content });

    const fullName = `${category.trim() || 'custom'}/${name.trim()}`;
    onCreated?.(fullName);
    onClose();
  };

  return (
    <div className="absolute inset-y-0 right-0 w-[420px] bg-[var(--bg-primary)] border-l border-subtle z-50 flex flex-col shadow-xl">
      <div className="flex items-center justify-between px-4 py-3 border-b border-subtle bg-[var(--bg-secondary)]">
        <div className="flex items-center gap-2">
          <Zap size={14} className="text-[var(--accent-warning)]" />
          <span className="text-sm font-medium text-title">Create Operation</span>
        </div>
        <button onClick={onClose} className="text-muted hover:text-title transition-colors">
          <X size={14} />
        </button>
      </div>

      <div className="flex-1 overflow-auto p-4 space-y-4">
        <div>
          <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1">Name *</label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="my_operation"
            className="w-full bg-[var(--bg-secondary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle"
          />
        </div>

        <div>
          <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1">Category</label>
          <input
            type="text"
            value={category}
            onChange={(e) => setCategory(e.target.value)}
            placeholder="custom"
            className="w-full bg-[var(--bg-secondary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle"
          />
        </div>

        <div>
          <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1">Description</label>
          <input
            type="text"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="What this operation does"
            className="w-full bg-[var(--bg-secondary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle"
          />
        </div>

        <div>
          <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1">Mode</label>
          <div className="flex gap-2">
            <button
              onClick={() => setMode('single_prompt')}
              className={`flex-1 px-3 py-2 text-xs border transition-colors ${
                mode === 'single_prompt'
                  ? 'bg-[var(--accent-info)]/20 text-[var(--accent-info)] border-[var(--accent-info)]'
                  : 'bg-[var(--bg-secondary)] border-dim hover:border-subtle'
              }`}
            >
              Single Prompt
            </button>
            <button
              onClick={() => setMode('agent')}
              className={`flex-1 px-3 py-2 text-xs border transition-colors ${
                mode === 'agent'
                  ? 'bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] border-[var(--accent-purple)]'
                  : 'bg-[var(--bg-secondary)] border-dim hover:border-subtle'
              }`}
            >
              Agent
            </button>
          </div>
        </div>

        {mode === 'agent' && (
          <div className="flex gap-3">
            <div className="flex-1">
              <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1">Iterations</label>
              <input
                type="number"
                value={agentIterations}
                onChange={(e) => setAgentIterations(parseInt(e.target.value) || 5)}
                min={1}
                className="w-full bg-[var(--bg-secondary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle"
              />
            </div>
            <div className="flex-1">
              <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1">Timeout (s)</label>
              <input
                type="number"
                value={timeout}
                onChange={(e) => setTimeout(parseInt(e.target.value) || 120)}
                min={1}
                className="w-full bg-[var(--bg-secondary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle"
              />
            </div>
          </div>
        )}

        <div>
          <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1">Model</label>
          <select
            value={modelRef}
            onChange={(e) => setModelRef(e.target.value)}
            className="w-full bg-[var(--bg-secondary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle"
          >
            <option value="">Use default model</option>
            {modelDefs.map((m) => (
              <option key={m.name} value={m.name}>{m.name}</option>
            ))}
          </select>
        </div>

        <div>
          <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1">Agent Info</label>
          <input
            type="text"
            value={agentInfo}
            onChange={(e) => setAgentInfo(e.target.value)}
            placeholder="Target agent description (optional)"
            className="w-full bg-[var(--bg-secondary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle"
          />
        </div>

        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={yoloMode}
            onChange={(e) => setYoloMode(e.target.checked)}
            id="yolo-mode"
            className="accent-[var(--accent-warning)]"
          />
          <label htmlFor="yolo-mode" className="text-xs text-[var(--text-secondary)]">
            YOLO mode (autonomous tool execution)
          </label>
        </div>

        <div>
          <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1">Operation Prompt *</label>
          <textarea
            value={operationPrompt}
            onChange={(e) => setOperationPrompt(e.target.value)}
            placeholder="Instructions for the LLM..."
            className="w-full bg-[var(--bg-secondary)] border border-dim px-3 py-2 text-sm text-highlight font-mono min-h-[150px] resize-none focus:outline-none focus:border-subtle"
          />
        </div>
      </div>

      <div className="px-4 py-3 border-t border-subtle flex justify-end gap-2">
        <button
          onClick={onClose}
          className="px-4 py-2 text-xs tracking-wider text-muted border border-dim hover:border-subtle transition-colors"
        >
          Cancel
        </button>
        <button
          onClick={handleSave}
          disabled={!canSave || saving}
          className="inline-flex items-center gap-2 px-4 py-2 text-xs tracking-wider border border-dim bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:border-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Save size={14} />
          Create
        </button>
      </div>
    </div>
  );
}
