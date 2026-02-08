import { useState, useEffect, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { useApp } from '../context/AppContext';
import {
  Radio,
  FileText,
  List,
  RefreshCw,
  Trash2,
  Plus,
  Edit,
  ToggleLeft,
  ToggleRight,
  ChevronLeft,
  ChevronRight,
  ChevronDown,
} from 'lucide-react';
import { ConfigModal, type ConfigItem } from '../components/common/ConfigModal';
import type {
  InterceptRule,
  TrafficLogFilters,
  TargetDirection,
  RuleScope,
} from '../api/types';
import {
  ScrollableTrafficTable,
  TrafficFilterBar,
  countTrafficEntries,
  tryPrettyPrintJson,
  type ProtocolFilter,
} from '../components/traffic/TrafficTable';

type Tab = 'traffic' | 'matches' | 'rules';

export function InterceptPage() {
  const { state, requestInterceptRules } = useApp();
  const [searchParams, setSearchParams] = useSearchParams();

  //
  // Tab from URL or default to 'traffic'.
  //
  const tabParam = searchParams.get('tab');
  const activeTab: Tab = (tabParam === 'matches' || tabParam === 'rules') ? tabParam : 'traffic';
  const setActiveTab = (tab: Tab) => {
    setSearchParams({ tab }, { replace: true });
  };

  //
  // Load rules on mount.
  //
  useEffect(() => {
    requestInterceptRules();
  }, [requestInterceptRules]);

  return (
    <div className="space-y-4 md:space-y-6">
      {/*
      //
      // Header.
      //
      */}
      <div>
        <h1 className="text-2xl font-bold text-highlight">Traffic Interception</h1>
        <p className="text-muted mt-1">
          {state.intercept.trafficTotalCount} entries | {state.intercept.rules.length} rules
        </p>
      </div>

      {/*
      //
      // Tab Navigation.
      //
      */}
      <div className="flex gap-4 border-b border-subtle overflow-x-auto">
        <TabButton
          active={activeTab === 'traffic'}
          onClick={() => setActiveTab('traffic')}
          icon={<Radio size={14} />}
          label="Traffic Log"
        />
        <TabButton
          active={activeTab === 'matches'}
          onClick={() => setActiveTab('matches')}
          icon={<FileText size={14} />}
          label="Matches"
        />
        <TabButton
          active={activeTab === 'rules'}
          onClick={() => setActiveTab('rules')}
          icon={<List size={14} />}
          label="Rules"
        />
      </div>

      {/*
      //
      // Tab Content.
      //
      */}
      {activeTab === 'traffic' && <TrafficLogTab />}
      {activeTab === 'matches' && <MatchesTab />}
      {activeTab === 'rules' && <RulesTab />}
    </div>
  );
}

function TabButton({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-2 pb-3 px-1 text-sm font-medium transition-colors border-b-2 ${
        active
          ? 'text-title border-[var(--accent-info)]'
          : 'text-muted hover:text-[var(--text-primary)] border-transparent'
      }`}
    >
      {icon}
      {label}
    </button>
  );
}

//
// Display limit for logical entries (HTTP + WS groups).
//
const DISPLAY_LIMIT = 100;
//
// Fetch limit for raw entries (higher to ensure we get enough after grouping).
//
const FETCH_LIMIT = 10000;

function TrafficLogTab() {
  const { state, requestTrafficLog, clearTraffic } = useApp();
  const [filters, setFilters] = useState<TrafficLogFilters>({
    node_id: null,
    agent_short_name: null,
    start_time: null,
    end_time: null,
    url_pattern: null,
    direction: null,
    limit: FETCH_LIMIT,
    offset: 0,
  });
  const [expandedRow, setExpandedRow] = useState<number | null>(null);
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [protocolFilter, setProtocolFilter] = useState<ProtocolFilter>('all');
  const [searchFilter, setSearchFilter] = useState('');

  //
  // Refresh on mount and when requestTrafficLog becomes available.
  //
  useEffect(() => {
    requestTrafficLog(filters);
  }, [requestTrafficLog]);

  const handleRefresh = () => {
    requestTrafficLog(filters);
  };

  const handleClear = () => {
    setShowClearConfirm(true);
  };

  const confirmClear = () => {
    clearTraffic();
    setShowClearConfirm(false);
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

  const nodes = state.systemState?.nodes ?? [];
  const currentPage = Math.floor(filters.offset / filters.limit) + 1;
  const totalPages = Math.ceil(state.intercept.trafficTotalCount / filters.limit);
  const hasPrev = filters.offset > 0;
  const hasNext = filters.offset + filters.limit < state.intercept.trafficTotalCount;

  //
  // Handle filter changes with auto-refresh.
  //
  const handleFilterChange = (newFilters: TrafficLogFilters) => {
    setFilters(newFilters);
    requestTrafficLog(newFilters);
  };

  return (
    <div className="space-y-4">
      {/*
      //
      // Filters.
      //
      */}
      <TrafficFilterBar
        filters={filters}
        setFilters={handleFilterChange}
        protocolFilter={protocolFilter}
        setProtocolFilter={setProtocolFilter}
        searchFilter={searchFilter}
        setSearchFilter={setSearchFilter}
        onRefresh={handleRefresh}
        onClear={handleClear}
        nodes={nodes}
        showNodeSelector={true}
        showAgentSelector={true}
      />

      {/*
      //
      // Traffic Table.
      //
      */}
      <ScrollableTrafficTable
        entries={state.intercept.trafficLog}
        protocolFilter={protocolFilter}
        searchFilter={searchFilter}
        expandedRow={expandedRow}
        setExpandedRow={setExpandedRow}
        showNodeColumn={true}
        displayLimit={DISPLAY_LIMIT}
        heightMode="fixed"
        maxHeight="70vh"
      />

      {/*
      //
      // Pagination.
      //
      */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 text-xs">
        <div className="text-muted">
          Showing {Math.min(countTrafficEntries(state.intercept.trafficLog, protocolFilter, searchFilter), DISPLAY_LIMIT)} entries (of {state.intercept.trafficTotalCount} total)
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={handlePrevPage}
            disabled={!hasPrev}
            className="flex items-center gap-1 px-3 py-1 text-muted hover:text-title border border-subtle hover:border-[var(--border-hover)] transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <ChevronLeft size={12} />
            PREV
          </button>
          <span className="text-muted px-2">
            {currentPage} / {totalPages || 1}
          </span>
          <button
            onClick={handleNextPage}
            disabled={!hasNext}
            className="flex items-center gap-1 px-3 py-1 text-muted hover:text-title border border-subtle hover:border-[var(--border-hover)] transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            NEXT
            <ChevronRight size={12} />
          </button>
        </div>
      </div>

      {/*
      //
      // Clear Confirmation Modal.
      //
      */}
      {showClearConfirm && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-[var(--bg-secondary)] border border-subtle ascii-box p-4 md:p-6 w-[92vw] max-w-[400px]">
            <h2 className="text-sm font-bold tracking-wider text-title mb-4">CLEAR TRAFFIC LOG</h2>
            <p className="text-xs text-muted mb-6">
              Are you sure you want to clear all traffic entries? This action cannot be undone.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setShowClearConfirm(false)}
                className="px-4 py-2 text-xs text-muted border border-subtle hover:border-[var(--border-hover)] transition-colors"
              >
                CANCEL
              </button>
              <button
                onClick={confirmClear}
                className="px-4 py-2 text-xs text-[var(--accent-error)] border border-[var(--accent-error)] hover:bg-[var(--accent-error)] hover:text-[var(--bg-primary)] transition-colors"
              >
                CLEAR ALL
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

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
    <div className="space-y-4">
      {/*
      //
      // Filters.
      //
      */}
      <div className="flex flex-col sm:flex-row sm:items-center gap-3 sm:gap-4 p-4 border border-subtle ascii-box">
        <select
          className="bg-[var(--bg-tertiary)] border border-subtle text-xs text-title px-2 py-1 outline-none"
          value={selectedRuleId ?? ''}
          onChange={(e) => setSelectedRuleId(e.target.value ? Number(e.target.value) : null)}
        >
          <option value="">All Rules</option>
          {state.intercept.rules.map((rule) => (
            <option key={rule.id} value={rule.id ?? ''}>
              {rule.name}
            </option>
          ))}
        </select>
        <div className="flex-1" />
        <button
          onClick={handleRefresh}
          className="flex items-center gap-2 px-3 py-1 text-xs text-muted hover:text-title border border-subtle hover:border-[var(--border-hover)] transition-colors"
        >
          <RefreshCw size={12} />
          REFRESH
        </button>
      </div>

      {/*
      //
      // Matches Table.
      //
      */}
      <div className="border border-subtle ascii-box overflow-x-auto">
        <table className="w-full min-w-[920px] text-xs">
          <thead>
            <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
              <th className="text-left px-4 py-2 text-muted tracking-wider w-8"></th>
              <th className="text-left px-4 py-2 text-muted tracking-wider">MATCHED AT</th>
              <th className="text-left px-4 py-2 text-muted tracking-wider">RULE</th>
              <th className="text-left px-4 py-2 text-muted tracking-wider">NODE</th>
              <th className="text-left px-4 py-2 text-muted tracking-wider">AGENT</th>
              <th className="text-left px-4 py-2 text-muted tracking-wider">METHOD</th>
              <th className="text-left px-4 py-2 text-muted tracking-wider">URL</th>
            </tr>
          </thead>
          <tbody>
            {state.intercept.trafficMatches.map((match) => {
              const isExpanded = expandedMatchId === match.match_info.id;
              const entry = match.traffic;
              return (
                <>
                  <tr
                    key={match.match_info.id}
                    className="border-b border-dim hover:bg-[var(--highlight)] cursor-pointer"
                    onClick={() => setExpandedMatchId(isExpanded ? null : match.match_info.id)}
                  >
                    <td className="px-4 py-2">
                      {isExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                    </td>
                    <td className="px-4 py-2 text-muted font-mono">
                      {new Date(match.match_info.matched_at).toLocaleString()}
                    </td>
                    <td className="px-4 py-2 text-[var(--accent-success)]">{match.match_info.rule_name}</td>
                    <td className="px-4 py-2 text-title">{entry.node_id.slice(0, 8)}</td>
                    <td className="px-4 py-2 text-highlight">{entry.agent_short_name}</td>
                    <td className="px-4 py-2 text-title font-mono">{entry.method ?? '-'}</td>
                    <td className="px-4 py-2 text-title font-mono truncate max-w-md">{entry.url}</td>
                  </tr>
                  {isExpanded && (
                    <tr key={`${match.match_info.id}-details`} className="bg-[var(--bg-tertiary)]">
                      <td colSpan={7} className="px-4 py-4">
                        <div className="space-y-4">
                          {/*
                          //
                          // LLM Summary.
                          //
                          */}
                          {match.match_info.summary && match.match_info.summary.trim().toUpperCase() !== 'NONE' && (
                            <div>
                              <div className="text-[var(--accent-info)] mb-2 tracking-wider">AI SUMMARY</div>
                              <div className="text-xs bg-[var(--bg-primary)] p-3 border border-[var(--accent-info)]/30 prose prose-invert prose-xs max-w-none prose-p:my-1 prose-headings:my-2 prose-ul:my-1 prose-ol:my-1 prose-li:my-0 prose-code:text-[var(--accent-info)] prose-code:bg-[var(--bg-tertiary)] prose-code:px-1 prose-code:rounded prose-pre:bg-[var(--bg-tertiary)] prose-pre:border prose-pre:border-subtle prose-strong:text-[var(--text-primary)] prose-table:text-xs prose-th:px-2 prose-th:py-1 prose-td:px-2 prose-td:py-1 prose-th:border prose-td:border prose-th:border-subtle prose-td:border-subtle">
                                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                                  {match.match_info.summary}
                                </ReactMarkdown>
                              </div>
                            </div>
                          )}

                          {/*
                          //
                          // Match Info.
                          //
                          */}
                          <div className="flex flex-col sm:flex-row gap-2 sm:gap-4 text-xs">
                            <div>
                              <span className="text-muted">Traffic Timestamp:</span>{' '}
                              <span className="text-title font-mono">
                                {new Date(entry.timestamp).toLocaleString()}
                              </span>
                            </div>
                            <div>
                              <span className="text-muted">Direction:</span>{' '}
                              <span className="text-title">{entry.direction}</span>
                            </div>
                            {entry.response_status && (
                              <div>
                                <span className="text-muted">Status:</span>{' '}
                                <span className={`font-mono ${
                                  entry.response_status >= 400
                                    ? 'text-[var(--accent-alert)]'
                                    : entry.response_status >= 300
                                    ? 'text-[var(--accent-warning)]'
                                    : 'text-[var(--accent-success)]'
                                }`}>
                                  {entry.response_status}
                                </span>
                              </div>
                            )}
                          </div>

                          {/*
                          //
                          // Full URL.
                          //
                          */}
                          <div>
                            <div className="text-muted mb-2 tracking-wider">FULL URL</div>
                            <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto break-all whitespace-pre-wrap">
                              {entry.method ?? 'GET'} {entry.url}
                            </pre>
                          </div>

                          {/*
                          //
                          // HTTP request/response content.
                          //
                          */}
                          <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                            {entry.request_headers && (
                              <div>
                                <div className="text-muted mb-2 tracking-wider">REQUEST HEADERS</div>
                                <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64">
                                  {JSON.stringify(entry.request_headers, null, 2)}
                                </pre>
                              </div>
                            )}
                            {entry.request_body && (
                              <div>
                                <div className="text-muted mb-2 tracking-wider">REQUEST BODY</div>
                                <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64 whitespace-pre-wrap">
                                  {tryPrettyPrintJson(entry.request_body)}
                                </pre>
                              </div>
                            )}
                            {entry.response_headers && (
                              <div>
                                <div className="text-muted mb-2 tracking-wider">RESPONSE HEADERS</div>
                                <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64">
                                  {JSON.stringify(entry.response_headers, null, 2)}
                                </pre>
                              </div>
                            )}
                            {entry.response_body && (
                              <div>
                                <div className="text-muted mb-2 tracking-wider">RESPONSE BODY</div>
                                <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64 whitespace-pre-wrap">
                                  {tryPrettyPrintJson(entry.response_body)}
                                </pre>
                              </div>
                            )}
                          </div>
                        </div>
                      </td>
                    </tr>
                  )}
                </>
              );
            })}
            {state.intercept.trafficMatches.length === 0 && (
              <tr>
                <td colSpan={7} className="px-4 py-8 text-center text-muted">
                  No matches found
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {/*
      //
      // Pagination info.
      //
      */}
      <div className="text-xs text-muted text-right">
        Showing {state.intercept.trafficMatches.length} of {state.intercept.matchesTotalCount} matches
      </div>
    </div>
  );
}

function RulesTab() {
  const { state, updateInterceptRule, deleteInterceptRule } = useApp();
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [editingRule, setEditingRule] = useState<InterceptRule | null>(null);
  const [ruleToDelete, setRuleToDelete] = useState<InterceptRule | null>(null);

  const handleToggleRule = (rule: InterceptRule) => {
    if (rule.id !== null) {
      updateInterceptRule(rule.id, { enabled: !rule.enabled });
    }
  };

  const handleDeleteRule = (rule: InterceptRule) => {
    setRuleToDelete(rule);
  };

  const confirmDelete = () => {
    if (ruleToDelete && ruleToDelete.id !== null) {
      deleteInterceptRule(ruleToDelete.id);
    }
    setRuleToDelete(null);
  };

  return (
    <div className="space-y-4">
      {/*
      //
      // Actions.
      //
      */}
      <div className="flex items-center justify-end gap-2 sm:gap-4">
        <button
          onClick={() => setShowCreateModal(true)}
          className="inline-flex items-center gap-2 px-3 py-1.5 text-xs tracking-wider bg-[var(--accent-success)]/20 text-[var(--accent-success)] border border-dim hover:border-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors"
        >
          <Plus size={14} />
          Add
        </button>
      </div>

      {/*
      //
      // Error display.
      //
      */}
      {state.intercept.ruleError && (
        <div className="p-3 border border-[var(--accent-alert)] text-[var(--accent-alert)] text-xs">
          {state.intercept.ruleError}
        </div>
      )}

      {/*
      //
      // Rules Table.
      //
      */}
      <div className="border border-subtle ascii-box overflow-x-auto">
        <table className="w-full min-w-[860px] text-xs">
          <thead>
            <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
              <th className="text-left px-4 py-2 text-muted tracking-wider">STATUS</th>
              <th className="text-left px-4 py-2 text-muted tracking-wider">NAME</th>
              <th className="text-left px-4 py-2 text-muted tracking-wider">PATTERN</th>
              <th className="text-left px-4 py-2 text-muted tracking-wider">DIRECTION</th>
              <th className="text-left px-4 py-2 text-muted tracking-wider">SCOPE</th>
              <th className="text-left px-4 py-2 text-muted tracking-wider">ACTIONS</th>
            </tr>
          </thead>
          <tbody>
            {state.intercept.rules.map((rule) => (
              <tr key={rule.id} className="border-b border-dim hover:bg-[var(--highlight)]">
                <td className="px-4 py-2">
                  <button
                    onClick={() => handleToggleRule(rule)}
                    className="flex items-center gap-1"
                  >
                    {rule.enabled ? (
                      <ToggleRight size={16} className="text-[var(--accent-success)]" />
                    ) : (
                      <ToggleLeft size={16} className="text-muted" />
                    )}
                  </button>
                </td>
                <td className="px-4 py-2 text-title">{rule.name}</td>
                <td className="px-4 py-2 text-highlight font-mono">{rule.regex_pattern}</td>
                <td className="px-4 py-2 text-muted uppercase">{rule.target_direction}</td>
                <td className="px-4 py-2 text-muted">{formatScope(rule.scope)}</td>
                <td className="px-4 py-2">
                  <div className="flex items-center gap-2">
                    <button
                      onClick={() => setEditingRule(rule)}
                      className="text-muted hover:text-title"
                    >
                      <Edit size={12} />
                    </button>
                    <button
                      onClick={() => handleDeleteRule(rule)}
                      className="text-muted hover:text-[var(--accent-alert)]"
                    >
                      <Trash2 size={12} />
                    </button>
                  </div>
                </td>
              </tr>
            ))}
            {state.intercept.rules.length === 0 && (
              <tr>
                <td colSpan={6} className="px-4 py-8 text-center text-muted">
                  No rules configured
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {/*
      //
      // Create/Edit Modal.
      //
      */}
      {(showCreateModal || editingRule) && (
        <RuleModal
          rule={editingRule}
          onClose={() => {
            setShowCreateModal(false);
            setEditingRule(null);
          }}
        />
      )}

      {/*
      //
      // Delete Confirmation Modal.
      //
      */}
      {ruleToDelete && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-[var(--bg-secondary)] border border-subtle ascii-box p-4 md:p-6 w-[92vw] max-w-[400px]">
            <h2 className="text-sm font-bold tracking-wider text-title mb-4">DELETE RULE</h2>
            <p className="text-xs text-muted mb-2">
              Are you sure you want to delete this rule?
            </p>
            <p className="text-xs text-highlight font-mono mb-6 break-all">
              {ruleToDelete.name}
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setRuleToDelete(null)}
                className="px-4 py-2 text-xs text-muted border border-subtle hover:border-[var(--border-hover)] transition-colors"
              >
                CANCEL
              </button>
              <button
                onClick={confirmDelete}
                className="px-4 py-2 text-xs text-[var(--accent-error)] border border-[var(--accent-error)] hover:bg-[var(--accent-error)] hover:text-[var(--bg-primary)] transition-colors"
              >
                DELETE
              </button>
            </div>
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

function RuleModal({ rule, onClose }: { rule: InterceptRule | null; onClose: () => void }) {
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

  const nodes = state.systemState?.nodes ?? [];

  const handleChange = (name: string, value: any) => {
    setValues(prev => ({ ...prev, [name]: value }));
  };

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
        promptValue
      );
    }

    onClose();
  };

  const nodeOptions = useMemo(() => [
    { value: '', label: 'Select Node...' },
    ...nodes.map(node => ({
      value: node.node_id,
      label: node.machine_name || node.node_id.slice(0, 8)
    }))
  ], [nodes]);

  const config: ConfigItem[] = [
    {
      type: 'section',
      fields: [
        {
          name: 'name',
          label: 'Name',
          type: 'text',
          placeholder: 'Rule name',
          required: true,
          span: 'full',
        },
        {
          name: 'regex_pattern',
          label: 'Regex Pattern',
          type: 'text',
          placeholder: '.*api\\.example\\.com.*',
          required: true,
          span: 'full',
          help: 'Pattern will match against all request/response headers and content',
        },
        {
          name: 'target_direction',
          label: 'Target Direction',
          type: 'select',
          options: [
            { value: 'both', label: 'Both' },
            { value: 'send', label: 'Send Only' },
            { value: 'receive', label: 'Receive Only' },
          ],
          span: 'half',
        },
        {
          name: 'scope_type',
          label: 'Scope',
          type: 'select',
          options: [
            { value: 'all', label: 'All Nodes & Agents' },
            { value: 'node', label: 'Specific Node' },
            { value: 'agent', label: 'Specific Agent' },
          ],
          span: 'half',
        },
      ],
    },
  ];

  if (values.scope_type === 'node') {
    config.push({
      type: 'section',
      fields: [
        {
          name: 'scope_node_id',
          label: 'Select Node',
          type: 'select',
          options: nodeOptions,
          span: 'full',
        },
      ],
    });
  }

  if (values.scope_type === 'agent') {
    config.push({
      type: 'section',
      fields: [
        {
          name: 'scope_agent_node_id',
          label: 'Select Node',
          type: 'select',
          options: nodeOptions,
          span: 'half',
        },
        {
          name: 'scope_agent_name',
          label: 'Agent Short Name',
          type: 'text',
          placeholder: 'Agent short name...',
          span: 'half',
        },
      ],
    });
  }

  config.push(
    { type: 'divider' },
    {
      type: 'section',
      fields: [
        {
          name: 'summarization_prompt',
          label: 'Summarization Prompt',
          type: 'textarea',
          rows: 4,
          placeholder: 'e.g., Extract key information from this API response including user IDs, timestamps, and any error codes...',
          span: 'full',
          help: 'Optional. If provided, matched traffic will be summarized using the LLM. Return "NONE" to skip displaying a summary.',
        },
      ],
    }
  );

  return (
    <ConfigModal
      isOpen={true}
      onClose={onClose}
      title={rule ? 'Edit Rule' : 'New Rule'}
      size="lg"
      config={config}
      values={values}
      onChange={handleChange}
      onSubmit={handleSubmit}
      submitLabel={rule ? 'Save' : 'Create'}
      submitVariant="info"
      error={state.intercept.ruleError}
    />
  );
}
