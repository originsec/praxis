import { useState, useEffect } from 'react';
import { useSearchParams } from 'react-router-dom';
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
  Save,
} from 'lucide-react';
import { Modal } from '../components/common/Modal';
import { Tooltip } from '../components/common/Tooltip';
import type {
  InterceptRule,
  TrafficLogFilters,
  TargetDirection,
  RuleScope,
} from '../api/types';
import {
  GroupedTrafficRows,
  TrafficTableHeader,
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
    <div className="space-y-6">
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
      <div className="flex gap-4 border-b border-subtle">
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
      <div className="border border-subtle ascii-box">
        <table className="w-full text-xs">
          <thead>
            <TrafficTableHeader showNodeColumn={true} />
          </thead>
          <tbody>
            <GroupedTrafficRows
              entries={state.intercept.trafficLog}
              protocolFilter={protocolFilter}
              searchFilter={searchFilter}
              expandedRow={expandedRow}
              setExpandedRow={setExpandedRow}
              showNodeColumn={true}
              displayLimit={DISPLAY_LIMIT}
            />
            {state.intercept.trafficLog.length === 0 && (
              <tr>
                <td colSpan={7} className="px-4 py-8 text-center text-muted">
                  No traffic entries
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {/*
      //
      // Pagination.
      //
      */}
      <div className="flex items-center justify-between text-xs">
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
          <div className="bg-[var(--bg-secondary)] border border-subtle ascii-box p-6 w-[400px]">
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
      <div className="flex items-center gap-4 p-4 border border-subtle ascii-box">
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
      <div className="border border-subtle ascii-box">
        <table className="w-full text-xs">
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
                              <div className="text-xs bg-[var(--bg-primary)] p-3 border border-[var(--accent-info)]/30 whitespace-pre-wrap">
                                {match.match_info.summary}
                              </div>
                            </div>
                          )}

                          {/*
                          //
                          // Match Info.
                          //
                          */}
                          <div className="flex gap-4 text-xs">
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
                          <div className="grid grid-cols-2 gap-4">
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
      <div className="flex items-center justify-end gap-4">
        <button
          onClick={() => setShowCreateModal(true)}
          className="inline-flex items-center gap-2 px-3 py-1.5 text-sm bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors"
        >
          <Plus size={14} />
          Add Rule
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
      <div className="border border-subtle ascii-box">
        <table className="w-full text-xs">
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
          <div className="bg-[var(--bg-secondary)] border border-subtle ascii-box p-6 w-[400px]">
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
  const [name, setName] = useState(rule?.name ?? '');
  const [regexPattern, setRegexPattern] = useState(rule?.regex_pattern ?? '');
  const [targetDirection, setTargetDirection] = useState<TargetDirection>(rule?.target_direction ?? 'both');
  const [summarizationPrompt, setSummarizationPrompt] = useState(rule?.summarization_prompt ?? '');

  const ruleScope = rule?.scope;
  const [scopeType, setScopeType] = useState<'all' | 'node' | 'agent'>(
    ruleScope === 'all' ? 'all' : (ruleScope && 'node' in ruleScope) ? 'node' : (ruleScope && 'agent' in ruleScope) ? 'agent' : 'all'
  );
  const [scopeNodeId, setScopeNodeId] = useState<string>(
    ruleScope && ruleScope !== 'all' && 'node' in ruleScope ? ruleScope.node.node_id : ''
  );
  const [scopeAgentNodeId, setScopeAgentNodeId] = useState<string>(
    ruleScope && ruleScope !== 'all' && 'agent' in ruleScope ? ruleScope.agent.node_id : ''
  );
  const [scopeAgentName, setScopeAgentName] = useState<string>(
    ruleScope && ruleScope !== 'all' && 'agent' in ruleScope ? ruleScope.agent.agent_short_name : ''
  );

  const nodes = state.systemState?.nodes ?? [];

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();

    let scope: RuleScope = 'all';
    if (scopeType === 'node' && scopeNodeId) {
      scope = { node: { node_id: scopeNodeId } };
    } else if (scopeType === 'agent' && scopeAgentNodeId && scopeAgentName) {
      scope = { agent: { node_id: scopeAgentNodeId, agent_short_name: scopeAgentName } };
    }

    const promptValue = summarizationPrompt.trim() || null;

    if (rule && rule.id !== null) {
      updateInterceptRule(rule.id, {
        name,
        regex_pattern: regexPattern,
        target_direction: targetDirection,
        scope,
        summarization_prompt: promptValue,
      });
    } else {
      createInterceptRule(name, regexPattern, targetDirection, scope, promptValue);
    }

    onClose();
  };

  return (
    <Modal
      isOpen={true}
      onClose={onClose}
      title={rule ? 'Edit Rule' : 'New Rule'}
      size="lg"
    >
      <form onSubmit={handleSubmit} className="space-y-4">
        <div>
          <label className="block text-sm font-medium mb-1">Name</label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)]"
            required
          />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">
            <Tooltip content="Pattern will match against all request/response headers and content">
              <span className="border-b border-dotted border-current">Regex Pattern</span>
            </Tooltip>
          </label>
          <input
            type="text"
            value={regexPattern}
            onChange={(e) => setRegexPattern(e.target.value)}
            className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm font-mono focus:outline-none focus:border-[var(--border-active)]"
            placeholder=".*api\.example\.com.*"
            required
          />
        </div>

        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="block text-sm font-medium mb-1">Target Direction</label>
            <select
              value={targetDirection}
              onChange={(e) => setTargetDirection(e.target.value as TargetDirection)}
              className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)]"
            >
              <option value="both">Both</option>
              <option value="send">Send Only</option>
              <option value="receive">Receive Only</option>
            </select>
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">Scope</label>
            <select
              value={scopeType}
              onChange={(e) => setScopeType(e.target.value as 'all' | 'node' | 'agent')}
              className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)]"
            >
              <option value="all">All Nodes & Agents</option>
              <option value="node">Specific Node</option>
              <option value="agent">Specific Agent</option>
            </select>
          </div>
        </div>

        {scopeType === 'node' && (
          <div>
            <label className="block text-sm font-medium mb-1">Select Node</label>
            <select
              value={scopeNodeId}
              onChange={(e) => setScopeNodeId(e.target.value)}
              className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)]"
            >
              <option value="">Select Node...</option>
              {nodes.map((node) => (
                <option key={node.node_id} value={node.node_id}>
                  {node.machine_name || node.node_id.slice(0, 8)}
                </option>
              ))}
            </select>
          </div>
        )}

        {scopeType === 'agent' && (
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium mb-1">Select Node</label>
              <select
                value={scopeAgentNodeId}
                onChange={(e) => setScopeAgentNodeId(e.target.value)}
                className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)]"
              >
                <option value="">Select Node...</option>
                {nodes.map((node) => (
                  <option key={node.node_id} value={node.node_id}>
                    {node.machine_name || node.node_id.slice(0, 8)}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="block text-sm font-medium mb-1">Agent Short Name</label>
              <input
                type="text"
                value={scopeAgentName}
                onChange={(e) => setScopeAgentName(e.target.value)}
                placeholder="Agent short name..."
                className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)]"
              />
            </div>
          </div>
        )}

        <div>
          <label className="block text-sm font-medium mb-1">Summarization Prompt</label>
          <textarea
            value={summarizationPrompt}
            onChange={(e) => setSummarizationPrompt(e.target.value)}
            className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm font-mono focus:outline-none focus:border-[var(--border-active)]"
            rows={4}
            placeholder="e.g., Extract key information from this API response including user IDs, timestamps, and any error codes..."
          />
          <p className="text-xs text-muted mt-1">
            Optional. If provided, matched traffic will be summarized using the LLM. Return "NONE" to skip displaying a summary.
          </p>
        </div>

        {state.intercept.ruleError && (
          <div className="p-3 bg-[var(--accent-error)]/10 text-[var(--accent-error)] text-sm">
            {state.intercept.ruleError}
          </div>
        )}

        <div className="flex justify-end gap-3 pt-2">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 text-sm border border-subtle hover:bg-[var(--bg-tertiary)] transition-colors"
          >
            Cancel
          </button>
          <button
            type="submit"
            className="inline-flex items-center gap-2 px-4 py-2 text-sm bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors"
          >
            <Save size={16} />
            {rule ? 'Save' : 'Create'}
          </button>
        </div>
      </form>
    </Modal>
  );
}
