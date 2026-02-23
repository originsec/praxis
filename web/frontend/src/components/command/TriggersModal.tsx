import { useState, useEffect, useCallback } from 'react';
import { Plus, Trash2, Save, Clock } from 'lucide-react';
import { Modal } from '../common/Modal';
import { TargetSpecEditor } from '../common/TargetSpecEditor';
import { triggerConfigSummary, triggerTypeIcon } from '../chains/ChainTriggerPanel';
import { useApp } from '../../context/AppContext';
import type { ChainTriggerInfo, TriggerConfig, TargetSpec, ScheduleSpec } from '../../api/types';

interface TriggersModalProps {
  onClose: () => void;
}

const defaultTargetSpec: TargetSpec = {
  node_ids: [],
  os_filter: null,
  agent_short_names: [],
  include_triggering_node: false,
};

export function TriggersModal({ onClose }: TriggersModalProps) {
  const {
    state,
    requestChainTriggers,
    requestChainDefList,
    requestInterceptRules,
    createChainTrigger,
    updateChainTrigger,
    deleteChainTrigger,
  } = useApp();

  const triggers = state.chains.triggers;
  const chains = state.chains.chains;
  const interceptRules = state.intercept.rules;
  const nodes = state.systemState?.nodes ?? [];

  const [showAddForm, setShowAddForm] = useState(false);

  //
  // Add form state.
  //

  const [chainId, setChainId] = useState('');
  const [triggerType, setTriggerType] = useState<'Scheduled' | 'InterceptMatch' | 'NewNode'>('Scheduled');
  const [scheduleType, setScheduleType] = useState<'DailyAt' | 'Interval'>('Interval');
  const [dailyHour, setDailyHour] = useState(0);
  const [dailyMinute, setDailyMinute] = useState(0);
  const [intervalMinutes, setIntervalMinutes] = useState(60);
  const [recurring, setRecurring] = useState(true);
  const [ruleId, setRuleId] = useState(0);
  const [targetSpec, setTargetSpec] = useState<TargetSpec>({ ...defaultTargetSpec });

  useEffect(() => {
    requestChainTriggers();
    requestChainDefList();
    requestInterceptRules();
  }, [requestChainTriggers, requestChainDefList, requestInterceptRules]);

  const resetForm = useCallback(() => {
    setChainId('');
    setTriggerType('Scheduled');
    setScheduleType('Interval');
    setDailyHour(0);
    setDailyMinute(0);
    setIntervalMinutes(60);
    setRecurring(true);
    setRuleId(0);
    setTargetSpec({ ...defaultTargetSpec });
    setShowAddForm(false);
  }, []);

  const handleCreate = useCallback(() => {
    if (!chainId) return;

    let config: TriggerConfig;
    switch (triggerType) {
      case 'Scheduled': {
        let schedule: ScheduleSpec;
        if (scheduleType === 'DailyAt') {
          schedule = { type: 'DailyAt', hour: dailyHour, minute: dailyMinute };
        } else {
          schedule = { type: 'Interval', minutes: intervalMinutes };
        }
        config = { type: 'Scheduled', schedule, recurring };
        break;
      }
      case 'InterceptMatch':
        config = { type: 'InterceptMatch', rule_id: ruleId };
        break;
      case 'NewNode':
        config = { type: 'NewNode' };
        break;
    }

    createChainTrigger(chainId, config, targetSpec);
    resetForm();
  }, [chainId, triggerType, scheduleType, dailyHour, dailyMinute, intervalMinutes, recurring, ruleId, targetSpec, createChainTrigger, resetForm]);

  const handleToggleEnabled = useCallback((trigger: ChainTriggerInfo) => {
    updateChainTrigger(trigger.id, { enabled: !trigger.enabled });
  }, [updateChainTrigger]);

  const handleDelete = useCallback((triggerId: string) => {
    deleteChainTrigger(triggerId);
  }, [deleteChainTrigger]);

  const resolveChainName = (cId: string) => {
    return chains.find(c => c.id === cId)?.name ?? cId.slice(0, 8);
  };

  return (
    <Modal
      isOpen={true}
      onClose={onClose}
      title="Triggers"
      size="lg"
      noPadding
    >
      <div className="flex flex-col" style={{ height: '60vh' }}>

        {/*
        //
        // Top bar: trigger count + new trigger button.
        //
        */}
        <div className="flex items-center justify-between px-2.5 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)]">
          <span className="text-[10px] text-muted">
            {triggers.length} trigger{triggers.length !== 1 ? 's' : ''}
          </span>
          <button
            onClick={() => setShowAddForm(!showAddForm)}
            className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] border border-dim hover:border-[var(--accent-warning)] transition-colors"
          >
            <Plus size={11} />
            New Trigger
          </button>
        </div>

        {/*
        //
        // Scrollable content: add form + trigger list.
        //
        */}
        <div className="flex-1 overflow-y-auto">

          {/*
          //
          // Inline add form.
          //
          */}
          {showAddForm && (
            <div className="border-b border-subtle bg-[var(--bg-secondary)] p-3 space-y-2.5">
              <div>
                <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Chain</label>
                <select
                  value={chainId}
                  onChange={e => setChainId(e.target.value)}
                  className="w-full bg-[var(--bg-primary)] border border-dim px-2 py-1 text-[10px] text-highlight focus:outline-none focus:border-subtle transition-colors"
                >
                  <option value="">Select a chain...</option>
                  {chains.map(c => (
                    <option key={c.id} value={c.id}>{c.name}</option>
                  ))}
                </select>
              </div>

              <div>
                <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Trigger Type</label>
                <select
                  value={triggerType}
                  onChange={e => setTriggerType(e.target.value as typeof triggerType)}
                  className="w-full bg-[var(--bg-primary)] border border-dim px-2 py-1 text-[10px] text-highlight focus:outline-none focus:border-subtle transition-colors"
                >
                  <option value="Scheduled">Scheduled</option>
                  <option value="InterceptMatch">Intercept Match</option>
                  <option value="NewNode">New Node</option>
                </select>
              </div>

              {/*
              //
              // Scheduled config.
              //
              */}
              {triggerType === 'Scheduled' && (
                <div className="space-y-2">
                  <div>
                    <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Schedule</label>
                    <select
                      value={scheduleType}
                      onChange={e => setScheduleType(e.target.value as typeof scheduleType)}
                      className="w-full bg-[var(--bg-primary)] border border-dim px-2 py-1 text-[10px] text-highlight focus:outline-none focus:border-subtle transition-colors"
                    >
                      <option value="Interval">Interval</option>
                      <option value="DailyAt">Daily At</option>
                    </select>
                  </div>

                  {scheduleType === 'DailyAt' ? (
                    <div className="grid grid-cols-2 gap-2">
                      <div>
                        <label className="block text-[9px] text-muted mb-0.5">Hour (0-23)</label>
                        <input
                          type="number" min={0} max={23}
                          value={dailyHour}
                          onChange={e => setDailyHour(parseInt(e.target.value) || 0)}
                          className="w-full bg-[var(--bg-primary)] border border-dim px-2 py-1 text-[10px] text-highlight focus:outline-none focus:border-subtle transition-colors"
                        />
                      </div>
                      <div>
                        <label className="block text-[9px] text-muted mb-0.5">Minute (0-59)</label>
                        <input
                          type="number" min={0} max={59}
                          value={dailyMinute}
                          onChange={e => setDailyMinute(parseInt(e.target.value) || 0)}
                          className="w-full bg-[var(--bg-primary)] border border-dim px-2 py-1 text-[10px] text-highlight focus:outline-none focus:border-subtle transition-colors"
                        />
                      </div>
                    </div>
                  ) : (
                    <div>
                      <label className="block text-[9px] text-muted mb-0.5">Interval (minutes)</label>
                      <input
                        type="number" min={1}
                        value={intervalMinutes}
                        onChange={e => setIntervalMinutes(parseInt(e.target.value) || 1)}
                        className="w-full bg-[var(--bg-primary)] border border-dim px-2 py-1 text-[10px] text-highlight focus:outline-none focus:border-subtle transition-colors"
                      />
                    </div>
                  )}

                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={recurring}
                      onChange={e => setRecurring(e.target.checked)}
                      className="accent-[var(--accent-info)]"
                    />
                    <span className="text-[10px] text-[var(--text-secondary)]">Recurring</span>
                  </label>
                </div>
              )}

              {/*
              //
              // InterceptMatch config.
              //
              */}
              {triggerType === 'InterceptMatch' && (
                <div>
                  <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Intercept Rule</label>
                  {interceptRules.length === 0 ? (
                    <p className="text-[10px] text-muted italic py-1">No intercept rules configured.</p>
                  ) : (
                    <select
                      value={ruleId}
                      onChange={e => setRuleId(parseInt(e.target.value) || 0)}
                      className="w-full bg-[var(--bg-primary)] border border-dim px-2 py-1 text-[10px] text-highlight focus:outline-none focus:border-subtle transition-colors"
                    >
                      <option value={0}>Select a rule...</option>
                      {interceptRules.map(rule => (
                        <option key={rule.id} value={rule.id ?? 0}>
                          {rule.name} ({rule.regex_pattern})
                        </option>
                      ))}
                    </select>
                  )}
                </div>
              )}

              {/*
              //
              // Target spec.
              //
              */}
              <div className="border-t border-dim pt-2.5">
                <div className="text-[9px] tracking-widest text-[var(--text-secondary)] mb-1.5" style={{ letterSpacing: '0.08em' }}>
                  TARGET SPEC
                </div>
                <TargetSpecEditor
                  value={targetSpec}
                  onChange={setTargetSpec}
                  nodes={nodes}
                  showTriggeringNodeOption={triggerType === 'NewNode' || triggerType === 'InterceptMatch'}
                />
              </div>

              <div className="flex justify-end gap-2 pt-1">
                <button
                  onClick={resetForm}
                  className="px-2.5 py-1 text-[10px] tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={handleCreate}
                  disabled={!chainId}
                  className="inline-flex items-center gap-1 px-2.5 py-1 text-[10px] tracking-wider border border-dim bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:border-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30 transition-colors disabled:opacity-50"
                >
                  <Save size={10} />
                  Save
                </button>
              </div>
            </div>
          )}

          {/*
          //
          // Trigger list.
          //
          */}
          {triggers.length === 0 && !showAddForm ? (
            <div className="flex flex-col items-center justify-center h-full text-muted">
              <Clock size={18} className="mb-1.5 opacity-40" />
              <p className="text-[10px]">No triggers configured.</p>
            </div>
          ) : (
            <div className="divide-y divide-[var(--border-dim)]">
              {triggers.map(trigger => (
                <div
                  key={trigger.id}
                  className="group flex items-center gap-2 px-2.5 py-1.5 hover:bg-[var(--highlight)] transition-colors"
                >
                  {triggerTypeIcon(trigger.trigger_config)}

                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-1.5">
                      <span className="text-[11px] font-medium text-highlight truncate">
                        {resolveChainName(trigger.chain_id)}
                      </span>
                    </div>
                    <div className="flex items-center gap-1.5 text-[9px] text-muted">
                      <span>{triggerConfigSummary(trigger.trigger_config, interceptRules)}</span>
                      {trigger.next_fire_at && (
                        <>
                          <span className="text-[var(--border-subtle)]">·</span>
                          <span>Next: {new Date(trigger.next_fire_at).toLocaleString()}</span>
                        </>
                      )}
                    </div>
                  </div>

                  <button
                    onClick={() => handleToggleEnabled(trigger)}
                    className={`px-1.5 py-0.5 text-[9px] tracking-wider border transition-colors flex-shrink-0 ${
                      trigger.enabled
                        ? 'border-[var(--accent-success)]/40 text-[var(--accent-success)] bg-[var(--accent-success)]/10'
                        : 'border-dim text-muted'
                    }`}
                  >
                    {trigger.enabled ? 'ON' : 'OFF'}
                  </button>

                  <button
                    onClick={() => handleDelete(trigger.id)}
                    className="p-1 text-muted/30 hover:text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors opacity-0 group-hover:opacity-100 flex-shrink-0"
                    title="Delete trigger"
                  >
                    <Trash2 size={10} />
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </Modal>
  );
}
