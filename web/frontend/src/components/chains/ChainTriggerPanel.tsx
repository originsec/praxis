import { useState, useEffect, useCallback } from 'react';
import { Zap, Clock, Wifi, MonitorSmartphone, Plus, Trash2, ChevronDown, ChevronRight, Save } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { TargetSpecEditor } from '../common/TargetSpecEditor';
import type { ChainTriggerInfo, TriggerConfig, TargetSpec, ScheduleSpec } from '../../api/types';

interface ChainTriggerPanelProps {
  chainId: string;
}

//
// Summarize a trigger config for display.
//

function triggerConfigSummary(config: TriggerConfig, interceptRules?: { id: number | null; name: string }[]): string {
  switch (config.type) {
    case 'Scheduled': {
      const sched = config.schedule;
      const schedText = sched.type === 'DailyAt'
        ? `Daily at ${String(sched.hour).padStart(2, '0')}:${String(sched.minute).padStart(2, '0')}`
        : `Every ${sched.minutes}m`;
      return `${schedText}${config.recurring ? '' : ' (once)'}`;
    }
    case 'InterceptMatch': {
      const ruleName = interceptRules?.find(r => r.id === config.rule_id)?.name;
      return ruleName ? `Intercept: ${ruleName}` : `Intercept rule #${config.rule_id}`;
    }
    case 'NewNode':
      return 'New node connected';
  }
}

function triggerTypeIcon(config: TriggerConfig) {
  switch (config.type) {
    case 'Scheduled':
      return <Clock size={12} className="text-[var(--accent-warning)]" />;
    case 'InterceptMatch':
      return <Wifi size={12} className="text-[var(--accent-info)]" />;
    case 'NewNode':
      return <MonitorSmartphone size={12} className="text-[var(--accent-success)]" />;
  }
}

const defaultTargetSpec: TargetSpec = {
  node_ids: [],
  os_filter: null,
  agent_short_names: [],
  include_triggering_node: false,
};

export function ChainTriggerPanel({ chainId }: ChainTriggerPanelProps) {
  const {
    state,
    requestChainTriggers,
    createChainTrigger,
    updateChainTrigger,
    deleteChainTrigger,
  } = useApp();

  const nodes = state.systemState?.nodes ?? [];

  const triggers = state.chains.triggers.filter(t => t.chain_id === chainId);
  const [collapsed, setCollapsed] = useState(true);
  const [showAddForm, setShowAddForm] = useState(false);

  //
  // Add form state.
  //

  const [triggerType, setTriggerType] = useState<'Scheduled' | 'InterceptMatch' | 'NewNode'>('Scheduled');
  const [scheduleType, setScheduleType] = useState<'DailyAt' | 'Interval'>('Interval');
  const [dailyHour, setDailyHour] = useState(0);
  const [dailyMinute, setDailyMinute] = useState(0);
  const [intervalMinutes, setIntervalMinutes] = useState(60);
  const [recurring, setRecurring] = useState(true);
  const [ruleId, setRuleId] = useState(0);
  const [targetSpec, setTargetSpec] = useState<TargetSpec>({ ...defaultTargetSpec });

  const interceptRules = state.intercept.rules;

  useEffect(() => {
    if (!collapsed) {
      requestChainTriggers(chainId);
    }
  }, [collapsed, chainId, requestChainTriggers]);

  const resetForm = useCallback(() => {
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

  return (
    <div className="border-t border-subtle bg-[var(--bg-tertiary)]">
      <button
        type="button"
        onClick={() => setCollapsed(!collapsed)}
        className="flex items-center gap-2 w-full px-4 py-2 text-xs tracking-wider text-[var(--text-secondary)] hover:text-highlight transition-colors"
      >
        {collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
        <Zap size={12} className="text-[var(--accent-warning)]" />
        Triggers
        {triggers.length > 0 && (
          <span className="text-[var(--accent-warning)]">({triggers.length})</span>
        )}
      </button>

      {!collapsed && (
        <div className="px-4 pb-3 space-y-2">
          {/*
          //
          // Existing triggers list.
          //
          */}
          {triggers.length === 0 && !showAddForm && (
            <p className="text-xs text-muted py-1">No triggers configured.</p>
          )}

          {triggers.map(trigger => (
            <div
              key={trigger.id}
              className="flex items-center gap-2 px-2.5 py-1.5 bg-[var(--bg-primary)] border border-dim text-xs"
            >
              {triggerTypeIcon(trigger.trigger_config)}
              <span className="flex-1 text-highlight">{triggerConfigSummary(trigger.trigger_config, interceptRules)}</span>

              {trigger.next_fire_at && (
                <span className="text-muted" title="Next fire">
                  Next: {new Date(trigger.next_fire_at).toLocaleString()}
                </span>
              )}

              <button
                type="button"
                onClick={() => handleToggleEnabled(trigger)}
                className={`px-2 py-0.5 text-[10px] tracking-wider border transition-colors ${
                  trigger.enabled
                    ? 'border-[var(--accent-success)]/40 text-[var(--accent-success)] bg-[var(--accent-success)]/10'
                    : 'border-dim text-muted'
                }`}
              >
                {trigger.enabled ? 'ON' : 'OFF'}
              </button>

              <button
                type="button"
                onClick={() => handleDelete(trigger.id)}
                className="p-1 hover:bg-[var(--accent-error)]/10 text-muted hover:text-[var(--accent-error)] transition-colors"
                title="Delete trigger"
              >
                <Trash2 size={12} />
              </button>
            </div>
          ))}

          {/*
          //
          // Add trigger form.
          //
          */}
          {showAddForm ? (
            <div className="bg-[var(--bg-secondary)] border border-subtle p-3 space-y-3">
              {/*
              //
              // Type selector.
              //
              */}
              <div>
                <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Trigger Type</label>
                <select
                  value={triggerType}
                  onChange={(e) => setTriggerType(e.target.value as typeof triggerType)}
                  className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle transition-colors"
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
                    <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Schedule</label>
                    <select
                      value={scheduleType}
                      onChange={(e) => setScheduleType(e.target.value as typeof scheduleType)}
                      className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle transition-colors"
                    >
                      <option value="Interval">Interval</option>
                      <option value="DailyAt">Daily At</option>
                    </select>
                  </div>

                  {scheduleType === 'DailyAt' ? (
                    <div className="grid grid-cols-2 gap-2">
                      <div>
                        <label className="block text-xs text-muted mb-1">Hour (0-23)</label>
                        <input
                          type="number"
                          min={0}
                          max={23}
                          value={dailyHour}
                          onChange={(e) => setDailyHour(parseInt(e.target.value) || 0)}
                          className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle transition-colors"
                        />
                      </div>
                      <div>
                        <label className="block text-xs text-muted mb-1">Minute (0-59)</label>
                        <input
                          type="number"
                          min={0}
                          max={59}
                          value={dailyMinute}
                          onChange={(e) => setDailyMinute(parseInt(e.target.value) || 0)}
                          className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle transition-colors"
                        />
                      </div>
                    </div>
                  ) : (
                    <div>
                      <label className="block text-xs text-muted mb-1">Interval (minutes)</label>
                      <input
                        type="number"
                        min={1}
                        value={intervalMinutes}
                        onChange={(e) => setIntervalMinutes(parseInt(e.target.value) || 1)}
                        className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle transition-colors"
                      />
                    </div>
                  )}

                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={recurring}
                      onChange={(e) => setRecurring(e.target.checked)}
                      className="accent-[var(--accent-info)]"
                    />
                    <span className="text-xs text-[var(--text-secondary)]">Recurring</span>
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
                  <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Intercept Rule</label>
                  {interceptRules.length === 0 ? (
                    <p className="text-xs text-muted italic py-1">No intercept rules configured. Create rules in the Intercept page first.</p>
                  ) : (
                    <select
                      value={ruleId}
                      onChange={(e) => setRuleId(parseInt(e.target.value) || 0)}
                      className="w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle transition-colors"
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
              <div className="border-t border-dim pt-3">
                <div className="text-[10px] tracking-widest text-[var(--text-secondary)] mb-2" style={{ letterSpacing: '0.08em' }}>
                  TARGET SPEC
                </div>
                <TargetSpecEditor
                  value={targetSpec}
                  onChange={setTargetSpec}
                  nodes={nodes}
                  showTriggeringNodeOption={triggerType === 'NewNode' || triggerType === 'InterceptMatch'}
                />
              </div>

              {/*
              //
              // Save/Cancel.
              //
              */}
              <div className="flex justify-end gap-2 pt-1">
                <button
                  type="button"
                  onClick={resetForm}
                  className="px-3 py-1.5 text-xs tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  onClick={handleCreate}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs tracking-wider border border-dim bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:border-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30 transition-colors"
                >
                  <Save size={12} />
                  Save
                </button>
              </div>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setShowAddForm(true)}
              className="flex items-center gap-1.5 px-2.5 py-1.5 text-xs text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors w-full"
            >
              <Plus size={12} />
              Add Trigger
            </button>
          )}
        </div>
      )}
    </div>
  );
}

export { triggerConfigSummary, triggerTypeIcon };
