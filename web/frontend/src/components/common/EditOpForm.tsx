import { Circle, CircleCheck, Save, Download, Loader2 } from 'lucide-react';
import type { OperationDefinitionInfo } from '../../api/types';

interface EditOpFormProps {
  editDef: OperationDefinitionInfo;
  isNewOp: boolean;
  isSaving: boolean;
  error: string | null;
  onUpdate: (field: keyof OperationDefinitionInfo, value: string | number | boolean | string[]) => void;
  onSave: () => void;
  onExport?: () => void;
  onCancel: () => void;
}

export function EditOpForm({ editDef, isNewOp, isSaving, error, onUpdate, onSave, onExport, onCancel }: EditOpFormProps) {
  return (
    <div className="space-y-0">

      {/*
      //
      // Basic information.
      //
      */}
      <div className="space-y-2 p-3 bg-[var(--bg-secondary)]">
        <div className="grid grid-cols-2 gap-2">
          <div>
            <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">
              Name {isNewOp && <span className="text-[var(--accent-error)]/70">*</span>}
            </label>
            <input
              type="text"
              value={editDef.name}
              onChange={e => onUpdate('name', e.target.value)}
              disabled={isSaving}
              className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors"
              placeholder="Display name for operation"
            />
          </div>
          <div>
            <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">
              Short Name {isNewOp && <span className="text-[var(--accent-error)]/70">*</span>}
            </label>
            <input
              type="text"
              value={editDef.short_name}
              onChange={e => onUpdate('short_name', e.target.value)}
              disabled={!isNewOp || isSaving}
              className={`w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle ${
                !isNewOp ? 'opacity-50 cursor-not-allowed' : ''
              } disabled:opacity-50 transition-colors`}
              placeholder="unique_identifier"
            />
            {!isNewOp && <p className="text-[9px] text-muted mt-1">Cannot be changed</p>}
          </div>
        </div>

        <div className="grid grid-cols-2 gap-2">
          <div>
            <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">
              Category {isNewOp && <span className="text-[var(--accent-error)]/70">*</span>}
            </label>
            <input
              type="text"
              value={editDef.category}
              onChange={e => onUpdate('category', e.target.value)}
              disabled={!isNewOp || isSaving}
              className={`w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle ${
                !isNewOp ? 'opacity-50 cursor-not-allowed' : ''
              } disabled:opacity-50 transition-colors`}
              placeholder="recon, exfiltration, etc."
            />
            {!isNewOp && <p className="text-[9px] text-muted mt-1">Cannot be changed</p>}
          </div>
          <div>
            <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Mode</label>
            <select
              value={editDef.mode}
              onChange={e => onUpdate('mode', e.target.value)}
              disabled={isSaving}
              className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors"
            >
              <option value="one-shot">one-shot</option>
              <option value="agent">agent</option>
            </select>
          </div>
        </div>

        <div className={`grid ${editDef.mode === 'agent' ? 'grid-cols-2' : 'grid-cols-1'} gap-2`}>
          <div>
            <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Timeout (seconds)</label>
            <input
              type="number"
              value={editDef.timeout}
              onChange={e => onUpdate('timeout', parseInt(e.target.value) || 60)}
              disabled={isSaving}
              className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors"
            />
          </div>
          {editDef.mode === 'agent' && (
            <div>
              <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Agent Iterations</label>
              <input
                type="number"
                value={editDef.agent_iterations}
                onChange={e => onUpdate('agent_iterations', parseInt(e.target.value) || 5)}
                disabled={isSaving}
                className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors"
              />
            </div>
          )}
        </div>

        <div>
          <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Description</label>
          <input
            type="text"
            value={editDef.description}
            onChange={e => onUpdate('description', e.target.value)}
            disabled={isSaving}
            className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors"
            placeholder="Brief description of what this operation does"
          />
        </div>
      </div>

      <div className="border-t border-dim" />

      {/*
      //
      // Prompt configuration.
      //
      */}
      <div className="space-y-2 p-3 bg-[var(--bg-secondary)]">
        <div>
          <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Agent Info</label>
          <p className="text-[9px] mb-1.5 leading-relaxed" style={{ color: 'var(--text-muted)' }}>
            Optional. Technical context for AI agents to understand when and how to use this operation.
          </p>
          <textarea
            value={editDef.agent_info}
            onChange={e => onUpdate('agent_info', e.target.value)}
            disabled={isSaving}
            rows={3}
            className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs font-mono text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors resize-none"
            placeholder="e.g., Searches for emails through communication channels..."
          />
        </div>

        <div>
          <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">
            Operation Prompt <span className="text-[var(--accent-error)]/70">*</span>
          </label>
          <textarea
            value={editDef.operation_prompt}
            onChange={e => onUpdate('operation_prompt', e.target.value)}
            disabled={isSaving}
            rows={6}
            className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs font-mono text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors resize-none"
            placeholder="The actual instructions given to the agent when executing this operation"
          />
        </div>
      </div>

      <div className="border-t border-dim" />

      {/*
      //
      // Toggles and actions.
      //
      */}
      <div className="p-3 bg-[var(--bg-secondary)]">
        <div className="flex items-center gap-4 mb-3">
          <button
            onClick={() => onUpdate('yolo_mode', !editDef.yolo_mode)}
            disabled={isSaving}
            className="flex items-center gap-1.5 disabled:opacity-50 hover:opacity-80 transition-opacity"
            type="button"
          >
            {editDef.yolo_mode
              ? <CircleCheck size={12} className="text-[var(--accent-error)]" />
              : <Circle size={12} className="text-[var(--text-secondary)]" />
            }
            <span className={`text-[10px] tracking-wider ${editDef.yolo_mode ? 'text-[var(--accent-error)]' : 'text-[var(--text-secondary)]'}`}>
              YOLO Mode
            </span>
          </button>

          <button
            onClick={() => onUpdate('disabled', !editDef.disabled)}
            disabled={isSaving}
            className="flex items-center gap-1.5 disabled:opacity-50 hover:opacity-80 transition-opacity"
            type="button"
          >
            {editDef.disabled
              ? <CircleCheck size={12} className="text-[var(--accent-error)]" />
              : <Circle size={12} className="text-[var(--text-secondary)]" />
            }
            <span className={`text-[10px] tracking-wider ${editDef.disabled ? 'text-[var(--accent-error)]' : 'text-[var(--text-secondary)]'}`}>
              Disabled
            </span>
          </button>
        </div>

        {error && (
          <div className="mb-3 p-2 bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 text-[var(--accent-error)] text-[10px]">
            {error}
          </div>
        )}

        <div className="flex justify-end gap-2">
          {onExport && (
            <button
              onClick={onExport}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-[10px] tracking-wider text-muted border border-dim hover:border-[var(--accent-purple)] hover:text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/10 transition-colors"
            >
              <Download size={11} />
              Export
            </button>
          )}
          <button
            onClick={onCancel}
            disabled={isSaving}
            className="px-3 py-1.5 text-[10px] tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            onClick={onSave}
            disabled={isSaving || (isNewOp && (!editDef.short_name || !editDef.category))}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 text-[10px] tracking-wider bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors disabled:opacity-50"
          >
            {isSaving && <Loader2 size={11} className="animate-spin" />}
            <Save size={11} />
            {isSaving ? 'Saving...' : isNewOp ? 'Create' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  );
}
