import { useState, useEffect } from 'react';
import {
  ChevronLeft, ChevronRight,
  Plus, Trash2, Pencil, Save, RefreshCw,
  Circle, CircleCheck,
} from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Modal } from '../common/Modal';
import { useApp } from '../../context/AppContext';
import {
  ScrollableTrafficTable,
  TrafficFilterBar,
  countTrafficEntries,
  type ProtocolFilter,
} from '../traffic/TrafficTable';
import type {
  TrafficLogFilters,
  InterceptRule,
  TrafficMatchWithDetails,
  TargetDirection,
  RuleScope,
} from '../../api/types';

const DISPLAY_LIMIT = 100;
const FETCH_LIMIT = 10000;

type TrafficTab = 'log' | 'rules' | 'matches';

interface TrafficModalProps {
  onClose: () => void;
  fixedNodeId?: string;
}

export function TrafficModal({ onClose, fixedNodeId }: TrafficModalProps) {
  const { state, requestTrafficLog, requestInterceptRules } = useApp();

  const [activeTab, setActiveTab] = useState<TrafficTab>('log');

  //
  // Traffic Log tab state.
  //

  const [filters, setFilters] = useState<TrafficLogFilters>({
    node_id: fixedNodeId ?? null,
    agent_short_name: null,
    start_time: null,
    end_time: null,
    url_pattern: null,
    direction: null,
    limit: FETCH_LIMIT,
    offset: 0,
  });
  const [protocolFilter, setProtocolFilter] = useState<ProtocolFilter>('all');
  const [searchFilter, setSearchFilter] = useState('');

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => {
    requestTrafficLog(filters);
    requestInterceptRules();
  }, []);

  const handleFilterChange = (newFilters: TrafficLogFilters) => {
    setFilters(newFilters);
    requestTrafficLog(newFilters);
  };

  const handleRefresh = () => {
    requestTrafficLog(filters);
  };

  const handlePrevPage = () => {
    const newOffset = Math.max(0, filters.offset - filters.limit);
    const newFilters = { ...filters, offset: newOffset };
    setFilters(newFilters);
    requestTrafficLog(newFilters);
  };

  const handleNextPage = () => {
    const newOffset = filters.offset + filters.limit;
    if (newOffset < state.intercept.trafficTotalCount) {
      const newFilters = { ...filters, offset: newOffset };
      setFilters(newFilters);
      requestTrafficLog(newFilters);
    }
  };

  const currentPage = Math.floor(filters.offset / filters.limit) + 1;
  const totalPages = Math.ceil(state.intercept.trafficTotalCount / filters.limit);
  const hasPrev = filters.offset > 0;
  const hasNext = filters.offset + filters.limit < state.intercept.trafficTotalCount;

  const tabs: { value: TrafficTab; label: string }[] = [
    { value: 'log', label: 'Traffic Log' },
    { value: 'rules', label: 'Rules' },
    { value: 'matches', label: 'Matches' },
  ];

  return (
    <Modal
      isOpen={true}
      onClose={onClose}
      title="Intercepted Traffic"
      size="full"
      noPadding
    >
      <div className="flex flex-col h-[75vh]">

        {/*
        //
        // Tab bar.
        //
        */}
        <div className="flex items-center gap-0.5 px-4 pt-3 pb-0 flex-shrink-0">
          {tabs.map(t => (
            <button
              key={t.value}
              onClick={() => setActiveTab(t.value)}
              className={`px-2 py-0.5 text-[10px] transition-colors ${
                activeTab === t.value
                  ? 'bg-[var(--accent-info)]/20 text-[var(--accent-info)] border border-[var(--accent-info)]/50'
                  : 'text-muted hover:text-[var(--text-primary)] hover:bg-[var(--highlight)]'
              }`}
            >
              {t.label}
            </button>
          ))}
        </div>

        {/*
        //
        // Traffic Log tab.
        //
        */}
        {activeTab === 'log' && (
          <div className="flex flex-col flex-1 p-4 gap-3 min-h-0">
            <TrafficFilterBar
              filters={filters}
              setFilters={handleFilterChange}
              protocolFilter={protocolFilter}
              setProtocolFilter={setProtocolFilter}
              searchFilter={searchFilter}
              setSearchFilter={setSearchFilter}
              onRefresh={handleRefresh}
            />

            <ScrollableTrafficTable
              entries={state.intercept.trafficLog}
              protocolFilter={protocolFilter}
              searchFilter={searchFilter}
              expandedRow={null}
              setExpandedRow={() => {}}
              showNodeColumn={!fixedNodeId}
              displayLimit={DISPLAY_LIMIT}
              heightMode="flex"
              emptyMessage="No intercepted traffic"
            />

            {state.intercept.trafficTotalCount > 0 && (
              <div className="flex items-center justify-between text-xs flex-shrink-0">
                <div className="text-muted">
                  Showing {Math.min(countTrafficEntries(state.intercept.trafficLog, protocolFilter, searchFilter), DISPLAY_LIMIT)} of {state.intercept.trafficTotalCount}
                </div>
                <div className="flex items-center gap-2">
                  <button
                    onClick={handlePrevPage}
                    disabled={!hasPrev}
                    className="flex items-center gap-1 px-3 py-1 text-muted hover:text-title border border-subtle transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <ChevronLeft size={12} /> PREV
                  </button>
                  <span className="text-muted px-2">{currentPage} / {totalPages || 1}</span>
                  <button
                    onClick={handleNextPage}
                    disabled={!hasNext}
                    className="flex items-center gap-1 px-3 py-1 text-muted hover:text-title border border-subtle transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    NEXT <ChevronRight size={12} />
                  </button>
                </div>
              </div>
            )}
          </div>
        )}

        {/*
        //
        // Rules tab.
        //
        */}
        {activeTab === 'rules' && (
          <RulesTab />
        )}

        {/*
        //
        // Matches tab.
        //
        */}
        {activeTab === 'matches' && (
          <MatchesTab />
        )}
      </div>
    </Modal>
  );
}

//
// Rules tab — compact modal version of InterceptPage RulesTab.
//

function RulesTab() {
  const { state, createInterceptRule, updateInterceptRule, deleteInterceptRule } = useApp();
  const rules = state.intercept.rules;
  const nodes = state.systemState?.nodes ?? [];

  const [editingRule, setEditingRule] = useState<InterceptRule | null>(null);
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [ruleToDelete, setRuleToDelete] = useState<InterceptRule | null>(null);

  const handleToggleRule = (rule: InterceptRule) => {
    if (rule.id !== null) {
      updateInterceptRule(rule.id, { enabled: !rule.enabled });
    }
  };

  const confirmDelete = () => {
    if (ruleToDelete && ruleToDelete.id !== null) {
      deleteInterceptRule(ruleToDelete.id);
    }
    setRuleToDelete(null);
  };

  return (
    <div className="flex flex-col flex-1 p-4 gap-3 min-h-0">

      <div className="flex items-center justify-between flex-shrink-0">
        <span className="text-[10px] text-muted">{rules.length} rule{rules.length !== 1 ? 's' : ''}</span>
        <button
          onClick={() => { setShowCreateForm(true); setEditingRule(null); }}
          className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] bg-[var(--accent-success)]/20 text-[var(--accent-success)] border border-dim hover:border-[var(--accent-success)] transition-colors"
        >
          <Plus size={11} />
          Add Rule
        </button>
      </div>

      {state.intercept.ruleError && (
        <div className="p-2 bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 text-[var(--accent-error)] text-[10px] flex-shrink-0">
          {state.intercept.ruleError}
        </div>
      )}

      {/*
      //
      // Rules list.
      //
      */}
      <div className="flex-1 overflow-y-auto">
        {rules.length === 0 ? (
          <div className="flex items-center justify-center h-full text-[10px] text-muted">
            No rules configured.
          </div>
        ) : (
          <div className="divide-y divide-[var(--border-dim)]">
            {rules.map(rule => (
              <div
                key={rule.id}
                className="group flex items-center gap-2 px-2.5 py-1.5 hover:bg-[var(--highlight)] transition-colors"
              >
                <button
                  onClick={() => handleToggleRule(rule)}
                  className="flex-shrink-0"
                >
                  {rule.enabled
                    ? <CircleCheck size={12} className="text-[var(--accent-success)]" />
                    : <Circle size={12} className="text-[var(--text-secondary)]" />
                  }
                </button>

                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5">
                    <span className="text-[11px] font-medium text-highlight truncate">{rule.name}</span>
                    <span className="text-[9px] font-mono text-[var(--accent-info)] truncate max-w-[200px]">{rule.regex_pattern}</span>
                  </div>
                  <div className="flex items-center gap-1.5 text-[9px] text-muted">
                    <span className="uppercase">{rule.target_direction}</span>
                    <span className="text-[var(--border-subtle)]">·</span>
                    <span>{formatScope(rule.scope)}</span>
                  </div>
                </div>

                <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0">
                  <button
                    onClick={() => { setEditingRule(rule); setShowCreateForm(false); }}
                    className="p-1 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/20 transition-colors"
                    title="Edit"
                  >
                    <Pencil size={10} />
                  </button>
                  <button
                    onClick={() => setRuleToDelete(rule)}
                    className="p-1 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors"
                    title="Delete"
                  >
                    <Trash2 size={10} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/*
      //
      // Create/Edit rule form.
      //
      */}
      {(showCreateForm || editingRule) && (
        <RuleFormModal
          rule={editingRule}
          nodes={nodes}
          onClose={() => { setShowCreateForm(false); setEditingRule(null); }}
        />
      )}

      {/*
      //
      // Delete confirmation.
      //
      */}
      {ruleToDelete && (
        <Modal
          isOpen={true}
          onClose={() => setRuleToDelete(null)}
          title="Delete Rule"
        >
          <div className="space-y-3">
            <p className="text-xs">
              Are you sure you want to delete{' '}
              <span className="font-medium text-[var(--accent-error)]">"{ruleToDelete.name}"</span>?
            </p>
            <p className="text-[10px] text-muted">This action cannot be undone.</p>
            <div className="flex justify-end gap-2 pt-1">
              <button
                onClick={() => setRuleToDelete(null)}
                className="px-3 py-1.5 text-[10px] tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={confirmDelete}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 text-[10px] tracking-wider bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors"
              >
                <Trash2 size={11} />
                Delete
              </button>
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
}

//
// Rule create/edit form modal.
//

function RuleFormModal({ rule, nodes, onClose }: {
  rule: InterceptRule | null;
  nodes: { node_id: string; machine_name: string }[];
  onClose: () => void;
}) {
  const { state, createInterceptRule, updateInterceptRule } = useApp();

  const ruleScope = rule?.scope;
  const [values, setValues] = useState({
    name: rule?.name ?? '',
    regex_pattern: rule?.regex_pattern ?? '',
    target_direction: rule?.target_direction ?? 'both',
    summarization_prompt: rule?.summarization_prompt ?? '',
    scope_type: ruleScope === 'all' ? 'all' : (ruleScope && 'node' in ruleScope) ? 'node' : (ruleScope && 'agent' in ruleScope) ? 'agent' : 'all',
    scope_node_id: ruleScope && ruleScope !== 'all' && 'node' in ruleScope ? ruleScope.node.node_id : '',
    scope_agent_node_id: ruleScope && ruleScope !== 'all' && 'agent' in ruleScope ? ruleScope.agent.node_id : '',
    scope_agent_name: ruleScope && ruleScope !== 'all' && 'agent' in ruleScope ? ruleScope.agent.agent_short_name : '',
  });

  const handleSubmit = () => {
    let scope: RuleScope = 'all';
    if (values.scope_type === 'node' && values.scope_node_id) {
      scope = { node: { node_id: values.scope_node_id } };
    } else if (values.scope_type === 'agent' && values.scope_agent_node_id && values.scope_agent_name) {
      scope = { agent: { node_id: values.scope_agent_node_id, agent_short_name: values.scope_agent_name } };
    }

    const promptValue = values.summarization_prompt.trim() || null;

    if (rule && rule.id !== null) {
      updateInterceptRule(rule.id, {
        name: values.name,
        regex_pattern: values.regex_pattern,
        target_direction: values.target_direction as TargetDirection,
        scope,
        summarization_prompt: promptValue,
      });
    } else {
      createInterceptRule(
        values.name,
        values.regex_pattern,
        values.target_direction as TargetDirection,
        scope,
        promptValue,
      );
    }

    onClose();
  };

  const inputClass = "w-full bg-[var(--bg-primary)] border border-dim px-2 py-1 text-[10px] text-highlight focus:outline-none focus:border-subtle transition-colors";

  return (
    <Modal
      isOpen={true}
      onClose={onClose}
      title={rule ? 'Edit Rule' : 'New Rule'}
    >
      <div className="space-y-2.5">
        <div>
          <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Name</label>
          <input
            type="text"
            value={values.name}
            onChange={e => setValues(v => ({ ...v, name: e.target.value }))}
            placeholder="Rule name"
            className={inputClass}
          />
        </div>

        <div>
          <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Regex Pattern</label>
          <input
            type="text"
            value={values.regex_pattern}
            onChange={e => setValues(v => ({ ...v, regex_pattern: e.target.value }))}
            placeholder=".*api\.example\.com.*"
            className={`${inputClass} font-mono`}
          />
          <p className="text-[9px] text-muted mt-0.5">Matches against request/response headers and content</p>
        </div>

        <div className="grid grid-cols-2 gap-2">
          <div>
            <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Direction</label>
            <select
              value={values.target_direction}
              onChange={e => setValues(v => ({ ...v, target_direction: e.target.value }))}
              className={inputClass}
            >
              <option value="both">Both</option>
              <option value="send">Send Only</option>
              <option value="receive">Receive Only</option>
            </select>
          </div>
          <div>
            <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Scope</label>
            <select
              value={values.scope_type}
              onChange={e => setValues(v => ({ ...v, scope_type: e.target.value }))}
              className={inputClass}
            >
              <option value="all">All Nodes & Agents</option>
              <option value="node">Specific Node</option>
              <option value="agent">Specific Agent</option>
            </select>
          </div>
        </div>

        {values.scope_type === 'node' && (
          <div>
            <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Select Node</label>
            <select
              value={values.scope_node_id}
              onChange={e => setValues(v => ({ ...v, scope_node_id: e.target.value }))}
              className={inputClass}
            >
              <option value="">Select Node...</option>
              {nodes.map(n => (
                <option key={n.node_id} value={n.node_id}>
                  {n.machine_name || n.node_id.slice(0, 8)}
                </option>
              ))}
            </select>
          </div>
        )}

        {values.scope_type === 'agent' && (
          <div className="grid grid-cols-2 gap-2">
            <div>
              <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Select Node</label>
              <select
                value={values.scope_agent_node_id}
                onChange={e => setValues(v => ({ ...v, scope_agent_node_id: e.target.value }))}
                className={inputClass}
              >
                <option value="">Select Node...</option>
                {nodes.map(n => (
                  <option key={n.node_id} value={n.node_id}>
                    {n.machine_name || n.node_id.slice(0, 8)}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Agent Short Name</label>
              <input
                type="text"
                value={values.scope_agent_name}
                onChange={e => setValues(v => ({ ...v, scope_agent_name: e.target.value }))}
                placeholder="Agent short name..."
                className={inputClass}
              />
            </div>
          </div>
        )}

        <div className="border-t border-dim pt-2.5">
          <label className="block text-[10px] tracking-wider text-[var(--text-secondary)] mb-1">Summarization Prompt</label>
          <textarea
            value={values.summarization_prompt}
            onChange={e => setValues(v => ({ ...v, summarization_prompt: e.target.value }))}
            rows={3}
            placeholder='Optional. Matched traffic will be summarized using the LLM. Return "NONE" to skip.'
            className={`${inputClass} resize-none`}
          />
        </div>

        {state.intercept.ruleError && (
          <div className="p-2 bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 text-[var(--accent-error)] text-[10px]">
            {state.intercept.ruleError}
          </div>
        )}

        <div className="flex justify-end gap-2 pt-1">
          <button
            onClick={onClose}
            className="px-2.5 py-1 text-[10px] tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleSubmit}
            disabled={!values.name || !values.regex_pattern}
            className="inline-flex items-center gap-1 px-2.5 py-1 text-[10px] tracking-wider border border-dim bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:border-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors disabled:opacity-50"
          >
            <Save size={10} />
            {rule ? 'Save' : 'Create'}
          </button>
        </div>
      </div>
    </Modal>
  );
}

//
// Matches tab — compact modal version of InterceptPage MatchesTab.
//

function MatchesTab() {
  const { state, requestTrafficMatches } = useApp();
  const [selectedRuleId, setSelectedRuleId] = useState<number | null>(null);
  const [expandedMatchId, setExpandedMatchId] = useState<number | null>(null);

  useEffect(() => {
    requestTrafficMatches(selectedRuleId, 100, 0);
  }, [selectedRuleId, requestTrafficMatches]);

  const handleRefresh = () => {
    requestTrafficMatches(selectedRuleId, 100, 0);
  };

  return (
    <div className="flex flex-col flex-1 p-4 gap-3 min-h-0">

      <div className="flex items-center gap-2 flex-shrink-0">
        <select
          className="bg-[var(--bg-primary)] border border-dim text-[10px] text-highlight px-2 py-1 focus:outline-none focus:border-subtle transition-colors"
          value={selectedRuleId ?? ''}
          onChange={e => setSelectedRuleId(e.target.value ? Number(e.target.value) : null)}
        >
          <option value="">All Rules</option>
          {state.intercept.rules.map(rule => (
            <option key={rule.id} value={rule.id ?? ''}>
              {rule.name}
            </option>
          ))}
        </select>
        <button
          onClick={handleRefresh}
          className="flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted hover:text-[var(--text-primary)] border border-dim hover:border-subtle transition-colors"
        >
          <RefreshCw size={10} />
          Refresh
        </button>
        <div className="flex-1" />
        <span className="text-[9px] text-muted">
          {state.intercept.trafficMatches.length} of {state.intercept.matchesTotalCount} matches
        </span>
      </div>

      {/*
      //
      // Matches list.
      //
      */}
      <div className="flex-1 overflow-y-auto">
        {state.intercept.trafficMatches.length === 0 ? (
          <div className="flex items-center justify-center h-full text-[10px] text-muted">
            No matches found.
          </div>
        ) : (
          <div className="divide-y divide-[var(--border-dim)]">
            {state.intercept.trafficMatches.map(match => (
              <MatchRow
                key={match.match_info.id}
                match={match}
                expanded={expandedMatchId === match.match_info.id}
                onToggle={() => setExpandedMatchId(
                  expandedMatchId === match.match_info.id ? null : match.match_info.id
                )}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function MatchRow({ match, expanded, onToggle }: {
  match: TrafficMatchWithDetails;
  expanded: boolean;
  onToggle: () => void;
}) {
  const entry = match.traffic;

  return (
    <div>
      <div
        onClick={onToggle}
        className="flex items-center gap-2 px-2.5 py-1.5 hover:bg-[var(--highlight)] transition-colors cursor-pointer"
      >
        <ChevronRight size={10} className={`text-muted flex-shrink-0 transition-transform ${expanded ? 'rotate-90' : ''}`} />
        <span className="text-[9px] font-mono text-muted flex-shrink-0 w-[130px]">
          {new Date(match.match_info.matched_at).toLocaleString()}
        </span>
        <span className="text-[10px] text-[var(--accent-success)] flex-shrink-0">
          {match.match_info.rule_name}
        </span>
        <span className="text-[9px] text-muted flex-shrink-0">
          {entry.node_id.slice(0, 8)}
        </span>
        <span className="text-[9px] text-highlight flex-shrink-0">
          {entry.agent_short_name}
        </span>
        <span className="text-[9px] font-mono text-muted flex-shrink-0">
          {entry.method ?? '-'}
        </span>
        <span className="text-[9px] font-mono text-muted truncate min-w-0">
          {entry.url}
        </span>
      </div>

      {expanded && (
        <div className="px-4 py-3 bg-[var(--bg-tertiary)] border-t border-dim space-y-3">

          {match.match_info.summary && match.match_info.summary.trim().toUpperCase() !== 'NONE' && (
            <div>
              <div className="text-[9px] text-[var(--accent-info)] mb-1 tracking-wider">AI SUMMARY</div>
              <div className="text-[10px] bg-[var(--bg-primary)] p-2.5 border border-[var(--accent-info)]/30 prose prose-invert prose-xs max-w-none prose-p:my-1 prose-headings:my-2 prose-ul:my-1 prose-ol:my-1 prose-li:my-0 prose-code:text-[var(--accent-info)] prose-code:bg-[var(--bg-tertiary)] prose-code:px-1 prose-pre:bg-[var(--bg-tertiary)] prose-pre:border prose-pre:border-subtle prose-strong:text-[var(--text-primary)]">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                  {match.match_info.summary}
                </ReactMarkdown>
              </div>
            </div>
          )}

          <div className="flex flex-wrap gap-3 text-[10px]">
            <div>
              <span className="text-muted">Timestamp:</span>{' '}
              <span className="text-highlight font-mono">{new Date(entry.timestamp).toLocaleString()}</span>
            </div>
            <div>
              <span className="text-muted">Direction:</span>{' '}
              <span className="text-highlight">{entry.direction}</span>
            </div>
            {entry.response_status && (
              <div>
                <span className="text-muted">Status:</span>{' '}
                <span className={`font-mono ${
                  entry.response_status >= 400
                    ? 'text-[var(--accent-error)]'
                    : entry.response_status >= 300
                    ? 'text-[var(--accent-warning)]'
                    : 'text-[var(--accent-success)]'
                }`}>
                  {entry.response_status}
                </span>
              </div>
            )}
          </div>

          <div>
            <div className="text-[9px] text-muted mb-1 tracking-wider">FULL URL</div>
            <pre className="text-[9px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto break-all whitespace-pre-wrap">
              {entry.method ?? 'GET'} {entry.url}
            </pre>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
            {entry.request_headers && (
              <div>
                <div className="text-[9px] text-muted mb-1 tracking-wider">REQUEST HEADERS</div>
                <pre className="text-[9px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-48">
                  {JSON.stringify(entry.request_headers, null, 2)}
                </pre>
              </div>
            )}
            {entry.request_body && (
              <div>
                <div className="text-[9px] text-muted mb-1 tracking-wider">REQUEST BODY</div>
                <pre className="text-[9px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-48 whitespace-pre-wrap">
                  {tryPrettyPrintJson(entry.request_body)}
                </pre>
              </div>
            )}
            {entry.response_headers && (
              <div>
                <div className="text-[9px] text-muted mb-1 tracking-wider">RESPONSE HEADERS</div>
                <pre className="text-[9px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-48">
                  {JSON.stringify(entry.response_headers, null, 2)}
                </pre>
              </div>
            )}
            {entry.response_body && (
              <div>
                <div className="text-[9px] text-muted mb-1 tracking-wider">RESPONSE BODY</div>
                <pre className="text-[9px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-48 whitespace-pre-wrap">
                  {tryPrettyPrintJson(entry.response_body)}
                </pre>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function formatScope(scope: RuleScope): string {
  if (scope === 'all') return 'All';
  if ('node' in scope) return `Node: ${scope.node.node_id.slice(0, 8)}`;
  if ('agent' in scope) return `Agent: ${scope.agent.agent_short_name}`;
  return 'Unknown';
}

function tryPrettyPrintJson(body: number[]): string {
  try {
    const text = new TextDecoder().decode(new Uint8Array(body));
    const parsed = JSON.parse(text);
    return JSON.stringify(parsed, null, 2);
  } catch {
    try {
      return new TextDecoder().decode(new Uint8Array(body));
    } catch {
      return `[${body.length} bytes]`;
    }
  }
}
