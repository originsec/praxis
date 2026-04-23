import { useRef, useCallback, useState, useEffect } from 'react';
import { Highlight, Prism } from 'prism-react-renderer';
import type { PrismTheme } from 'prism-react-renderer';

const kqlTheme: PrismTheme = {
  plain: {
    color: 'var(--text-primary)',
    backgroundColor: 'var(--bg-primary)',
  },
  styles: [
    { types: ['comment'], style: { color: 'var(--text-muted)' } },
    { types: ['string', 'char'], style: { color: 'var(--accent-warning)' } },
    { types: ['number'], style: { color: 'var(--accent-info)' } },
    { types: ['keyword'], style: { color: 'var(--accent-purple)' } },
    { types: ['function'], style: { color: 'var(--text-highlight)' } },
    { types: ['operator'], style: { color: 'var(--text-secondary)' } },
    { types: ['punctuation'], style: { color: 'var(--text-secondary)' } },
    { types: ['boolean'], style: { color: 'var(--accent-info)' } },
  ],
};

//
// Register KQL language with Prism.
//

(Prism as unknown as { languages: Record<string, unknown> }).languages.kql = {
  'comment': /\/\/.*/,
  'string': {
    pattern: /(["'])(?:(?!\1)[^\\\r\n]|\\[\s\S])*\1/,
    greedy: true,
  },
  'number': /\b\d+(?:\.\d+)?(?:e[+-]?\d+)?\b/i,
  'keyword': /\b(?:where|project|sort|order|take|limit|extend|summarize|count|distinct|by|asc|desc|and|or|not|contains|startswith|endswith|has|ago|now|true|false|null|top|project_away|union|join|let|print|datatable|in|between|matches|regex)\b/i,
  'function': /(?!\d)\w+(?=\s*\()/,
  'operator': /[|=!<>+\-*/%]+/,
  'punctuation': /[[\](){},;.]/,
};

//
// Autocomplete data.
//

interface TableSchema {
  name: string;
  columns: string[];
}

const TABLE_SCHEMAS: TableSchema[] = [
  { name: 'AgentLogs', columns: ['timestamp', 'node_id', 'agent_short_name', 'agent_name', 'version'] },
  { name: 'EventLogs', columns: ['timestamp', 'source', 'source_id', 'level', 'target', 'message'] },
  { name: 'NodeLogs', columns: ['timestamp', 'node_id', 'machine_name', 'os_details', 'intercept_active'] },
  { name: 'ReconLogs', columns: ['timestamp', 'node_id', 'agent_short_name', 'is_semantic', 'mcp_server_count', 'skill_count', 'internal_tool_count', 'config_count', 'session_count', 'project_path_count'] },
  { name: 'ReconMetadataLogs', columns: ['timestamp', 'node_id', 'agent_short_name', 'entry_type', 'value'] },
  { name: 'ReconSessionLogs', columns: ['timestamp', 'node_id', 'agent_short_name', 'session_id', 'context_path', 'last_modified', 'message_count'] },
  { name: 'ReconToolLogs', columns: ['timestamp', 'node_id', 'agent_short_name', 'tool_type', 'server_name', 'tool_name', 'tool_description', 'transport'] },
  { name: 'SemanticOperationChainLogs', columns: ['timestamp', 'execution_id', 'chain_id', 'chain_name', 'node_id', 'agent_short_name', 'status', 'elements', 'outputs', 'started_at', 'ended_at'] },
  { name: 'SemanticOperationLogs', columns: ['timestamp', 'operation_id', 'node_id', 'agent_short_name', 'status', 'operation_spec', 'start_time', 'end_time', 'summary', 'result', 'chain_execution_id'] },
  { name: 'ToolkitActionsLog', columns: ['timestamp', 'id', 'execution_id', 'tool_name', 'action', 'status', 'node_id', 'agent_short_name', 'session_id', 'details_json'] },
  { name: 'TrafficLogs', columns: ['timestamp', 'traffic_id', 'node_id', 'agent_short_name', 'intercept_method', 'direction', 'method', 'url', 'host', 'request_headers', 'request_body', 'response_status', 'response_headers', 'response_body'] },
  { name: 'TrafficMatchLogs', columns: ['timestamp', 'traffic_id', 'node_id', 'agent_short_name', 'rule_id', 'rule_name', 'summary', 'method', 'url', 'host', 'direction', 'response_status'] },
];

const KQL_OPERATORS = [
  'where', 'project', 'project-away', 'sort', 'order', 'take', 'limit',
  'extend', 'summarize', 'count', 'distinct', 'top', 'join',
];

const KQL_FUNCTIONS = [
  'strlen', 'tolower', 'toupper',
  'isnotempty', 'isnull', 'isempty', 'isnotnull', 'now', 'count', 'sum',
  'avg', 'min', 'max', 'dcount', 'tostring', 'toint', 'tolong',
];

const KQL_INFIX_OPS = [
  'contains', '!contains', 'startswith', '!startswith',
  'endswith', '!endswith', 'has', '!has',
];

const KQL_KEYWORDS = ['and', 'or', 'not', 'by', 'on', '$left', '$right', 'asc', 'desc', 'true', 'false', 'null'];

interface Suggestion {
  label: string;
  kind: 'table' | 'column' | 'operator' | 'function' | 'keyword';
}

//
// Determine which table the query references (first line before any pipe).
//

function detectTable(text: string): TableSchema | null {
  const firstLine = text.split('\n')[0].trim();
  const tableName = firstLine.split(/\s|\|/)[0];
  return TABLE_SCHEMAS.find(t => t.name.toLowerCase() === tableName.toLowerCase()) ?? null;
}

//
// Determine the autocomplete context from text before cursor.
//

function getCompletionContext(textBeforeCursor: string): Suggestion[] {
  const trimmed = textBeforeCursor.trimEnd();
  const table = detectTable(textBeforeCursor);

  //
  // Get the current word being typed (partial match).
  //

  const wordMatch = textBeforeCursor.match(/(\w[\w-]*)$/);
  const partial = wordMatch ? wordMatch[1].toLowerCase() : '';

  //
  // After a pipe — only suggest if user has started typing an operator name.
  //

  if (/\|\s+\w[\w-]*$/.test(textBeforeCursor) && partial) {
    const items: Suggestion[] = KQL_OPERATORS.map(op => ({ label: op, kind: 'operator' }));
    return filterSuggestions(items, partial);
  }

  //
  // After `join (` — suggest table names for the right-hand side.
  //

  if (/join\s*\(\s*$/.test(trimmed) || /join\s*\(\s*\w[\w]*$/.test(textBeforeCursor)) {
    const items: Suggestion[] = TABLE_SCHEMAS.map(t => ({ label: t.name, kind: 'table' }));
    return filterSuggestions(items, partial);
  }

  //
  // After where/extend/summarize ... by — suggest columns + functions.
  //

  const lastPipeSegment = getLastPipeSegment(textBeforeCursor);
  const segOp = lastPipeSegment.trim().split(/\s+/)[0]?.toLowerCase();

  if (['where', 'extend', 'summarize'].includes(segOp ?? '')) {
    const wordCount = lastPipeSegment.trim().split(/\s+/).length;
    if (wordCount > 1 || partial !== segOp) {

      //
      // Suppress suggestions right after an infix operator or comparison
      // operator — the user needs to type a value, not pick from a list.
      // Also suppress when partial is empty and the previous token isn't a
      // keyword that starts a new expression (e.g. after a column name the
      // user needs to type an operator, not pick another column).
      //

      const tokensBeforeCursor = lastPipeSegment.trimEnd().split(/\s+/);
      const prevToken = tokensBeforeCursor[tokensBeforeCursor.length - (partial ? 2 : 1)]?.toLowerCase();
      if (prevToken && (KQL_INFIX_OPS.includes(prevToken) || ['==', '!=', '<', '>', '<=', '>='].includes(prevToken))) {
        return [];
      }

      const expressionStarters = ['where', 'extend', 'summarize', 'and', 'or', 'not', 'by', ',', '('];
      if (!partial && (!prevToken || !expressionStarters.includes(prevToken))) {
        return [];
      }

      const items: Suggestion[] = [];
      if (table) {
        items.push(...table.columns.map(c => ({ label: c, kind: 'column' as const })));
      }
      items.push(...KQL_INFIX_OPS.map(op => ({ label: op, kind: 'keyword' as const })));
      items.push(...KQL_FUNCTIONS.map(f => ({ label: f, kind: 'function' as const })));
      items.push(...KQL_KEYWORDS.map(k => ({ label: k, kind: 'keyword' as const })));
      return filterSuggestions(items, partial);
    }
  }

  //
  // After project/project-away/sort/order/distinct/top — suggest columns.
  //

  if (['project', 'project-away', 'sort', 'order', 'distinct', 'top', 'join'].includes(segOp ?? '')) {
    const wordCount = lastPipeSegment.trim().split(/\s+/).length;
    if ((wordCount > 1 || partial !== segOp) && table) {
      const items: Suggestion[] = table.columns.map(c => ({ label: c, kind: 'column' }));
      return filterSuggestions(items, partial);
    }
  }

  //
  // Start of query — only suggest table names if no table has been detected
  // yet (i.e. the user hasn't typed a valid table name on the first line).
  //

  if (!table && !textBeforeCursor.includes('|')) {
    const items: Suggestion[] = TABLE_SCHEMAS.map(t => ({ label: t.name, kind: 'table' }));
    return filterSuggestions(items, partial);
  }

  //
  // Table exists but no pipe yet and no partial word — nothing to suggest.
  // User needs to type `|` next.
  //

  if (table && !textBeforeCursor.includes('|') && !partial) {
    return [];
  }

  //
  // Generic fallback: columns + functions + keywords.
  //

  if (partial.length > 0) {
    const items: Suggestion[] = [];
    if (table) {
      items.push(...table.columns.map(c => ({ label: c, kind: 'column' as const })));
    }
    items.push(...KQL_FUNCTIONS.map(f => ({ label: f, kind: 'function' as const })));
    items.push(...KQL_KEYWORDS.map(k => ({ label: k, kind: 'keyword' as const })));
    items.push(...KQL_OPERATORS.map(o => ({ label: o, kind: 'operator' as const })));
    return filterSuggestions(items, partial);
  }

  return [];
}

function getLastPipeSegment(text: string): string {
  const pipes = text.split('|');
  return pipes[pipes.length - 1] ?? '';
}

function filterSuggestions(items: Suggestion[], partial: string): Suggestion[] {
  if (!partial) return items;
  return items.filter(s => s.label.toLowerCase().startsWith(partial) && s.label.toLowerCase() !== partial);
}

//
// Kind icons/colors for the suggestion dropdown.
//

function kindLabel(kind: Suggestion['kind']): { text: string; className: string } {
  switch (kind) {
    case 'table': return { text: 'TBL', className: 'text-[var(--accent-info)]' };
    case 'column': return { text: 'COL', className: 'text-[var(--accent-success)]' };
    case 'operator': return { text: 'OP', className: 'text-[var(--accent-purple)]' };
    case 'function': return { text: 'FN', className: 'text-[var(--accent-warning)]' };
    case 'keyword': return { text: 'KW', className: 'text-[var(--accent-purple)]' };
  }
}

//
// Editor component.
//

interface KqlCodeEditorProps {
  value: string;
  onChange?: (value: string) => void;
  onCtrlEnter?: () => void;
  readOnly?: boolean;
}

export function KqlCodeEditor({ value, onChange, onCtrlEnter, readOnly = false }: KqlCodeEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const preRef = useRef<HTMLPreElement>(null);
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [selectedIdx, setSelectedIdx] = useState(0);
  const [dropdownPos, setDropdownPos] = useState({ top: 0, left: 0 });
  const [showDropdown, setShowDropdown] = useState(false);
  const focusedRef = useRef(false);

  const handleScroll = useCallback(() => {
    if (textareaRef.current && preRef.current) {
      preRef.current.scrollTop = textareaRef.current.scrollTop;
      preRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
    setShowDropdown(false);
  }, []);

  const handleFocus = useCallback(() => {
    focusedRef.current = true;
  }, []);

  const handleBlur = useCallback(() => {
    focusedRef.current = false;
    //
    // Small delay so clicking a suggestion in the dropdown fires before we
    // hide it.
    //
    setTimeout(() => {
      if (!focusedRef.current) setShowDropdown(false);
    }, 150);
  }, []);

  //
  // Apply a suggestion: replace the partial word before cursor with the full
  // suggestion.
  //

  const applySuggestion = useCallback((suggestion: Suggestion) => {
    const textarea = textareaRef.current;
    if (!textarea || !onChange) return;

    const cursorPos = textarea.selectionStart;
    const textBefore = value.slice(0, cursorPos);
    const textAfter = value.slice(cursorPos);

    //
    // Find the partial word to replace.
    //

    const wordMatch = textBefore.match(/(\w[\w-]*)$/);
    const replaceStart = wordMatch ? cursorPos - wordMatch[1].length : cursorPos;

    let insert = suggestion.label;
    if (suggestion.kind === 'function') insert += '(';

    const newValue = value.slice(0, replaceStart) + insert + textAfter;
    onChange(newValue);
    setShowDropdown(false);

    //
    // Set cursor position after the inserted text.
    //

    const newCursorPos = replaceStart + insert.length;
    requestAnimationFrame(() => {
      textarea.selectionStart = newCursorPos;
      textarea.selectionEnd = newCursorPos;
      textarea.focus();
    });
  }, [value, onChange]);

  //
  // Compute cursor pixel position for dropdown placement.
  //

  const updateDropdownPosition = useCallback(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;

    const cursorPos = textarea.selectionStart;
    const textBefore = value.slice(0, cursorPos);
    const lines = textBefore.split('\n');
    const lineIdx = lines.length - 1;
    const colIdx = lines[lineIdx].length;

    const lineHeight = 16.5; // 11px * 1.5
    const charWidth = 6.6; // approximate for 11px monospace

    const top = (lineIdx + 1) * lineHeight + 12 - textarea.scrollTop;
    const left = colIdx * charWidth + 48 - textarea.scrollLeft;

    setDropdownPos({ top, left });
  }, [value]);

  const handleChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    onChange?.(e.target.value);
  }, [onChange]);

  //
  // Update suggestions on value or cursor change.
  //

  useEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea || readOnly || !focusedRef.current) {
      setShowDropdown(false);
      return;
    }

    const update = () => {
      if (!focusedRef.current) {
        setShowDropdown(false);
        return;
      }

      const cursorPos = textarea.selectionStart;
      const textBefore = value.slice(0, cursorPos);

      //
      // Don't show if cursor is inside a string.
      //

      const quotes = (textBefore.match(/"/g) || []).length;
      if (quotes % 2 !== 0) {
        setShowDropdown(false);
        return;
      }

      const items = getCompletionContext(textBefore);
      setSuggestions(items);
      setSelectedIdx(0);

      if (items.length > 0) {
        updateDropdownPosition();
        setShowDropdown(true);
      } else {
        setShowDropdown(false);
      }
    };

    //
    // Delay slightly so selectionStart is updated after onChange.
    //

    const timer = setTimeout(update, 10);
    return () => clearTimeout(timer);
  }, [value, readOnly, updateDropdownPosition]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      setShowDropdown(false);
      onCtrlEnter?.();
      return;
    }

    if (!showDropdown || suggestions.length === 0) return;

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelectedIdx((prev) => (prev + 1) % suggestions.length);
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelectedIdx((prev) => (prev - 1 + suggestions.length) % suggestions.length);
      return;
    }
    if (e.key === 'Tab' || e.key === 'Enter') {
      e.preventDefault();
      applySuggestion(suggestions[selectedIdx]);
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      setShowDropdown(false);
      return;
    }
  }, [showDropdown, suggestions, selectedIdx, applySuggestion, onCtrlEnter]);

  return (
    <div className="relative flex-1 overflow-hidden" style={{ minHeight: 0 }}>
      <Highlight
        prism={Prism}
        theme={kqlTheme}
        code={value}
        language="kql"
      >
        {({ tokens, getLineProps, getTokenProps }) => (
          <pre
            ref={preRef}
            className="absolute inset-0 m-0 overflow-hidden pointer-events-none"
            style={{
              padding: '12px',
              paddingLeft: '48px',
              fontFamily: '"JetBrains Mono", "Fira Code", "SF Mono", "Cascadia Code", Menlo, Monaco, "Courier New", monospace',
              fontSize: '11px',
              lineHeight: '1.5',
              background: 'var(--bg-primary)',
              whiteSpace: 'pre',
              minWidth: 'fit-content',
            }}
          >
            {tokens.map((line, i) => {
              const lineProps = getLineProps({ line, key: i });
              return (
                <div key={i} {...lineProps} style={{ ...lineProps.style, display: 'flex' }}>
                  <span
                    style={{
                      width: '36px',
                      marginLeft: '-36px',
                      display: 'inline-block',
                      textAlign: 'right',
                      paddingRight: '12px',
                      color: 'var(--text-muted)',
                      opacity: 0.4,
                      userSelect: 'none',
                      flexShrink: 0,
                    }}
                  >
                    {i + 1}
                  </span>
                  <span>
                    {line.map((token, key) => {
                      const tokenProps = getTokenProps({ token, key });
                      return <span key={key} {...tokenProps} />;
                    })}
                  </span>
                </div>
              );
            })}
          </pre>
        )}
      </Highlight>

      <textarea
        ref={textareaRef}
        value={value}
        onChange={handleChange}
        onScroll={handleScroll}
        onKeyDown={handleKeyDown}
        onFocus={handleFocus}
        onBlur={handleBlur}
        readOnly={readOnly}
        spellCheck={false}
        className="absolute inset-0 w-full h-full resize-none focus:outline-none"
        style={{
          padding: '12px',
          paddingLeft: '48px',
          fontFamily: '"JetBrains Mono", "Fira Code", "SF Mono", "Cascadia Code", Menlo, Monaco, "Courier New", monospace',
          fontSize: '11px',
          lineHeight: '1.5',
          background: 'transparent',
          color: 'transparent',
          caretColor: 'var(--text-highlight)',
          whiteSpace: 'pre',
          overflowWrap: 'normal',
          tabSize: 2,
        }}
      />

      {/*
      //
      // Autocomplete dropdown.
      //
      */}
      {showDropdown && suggestions.length > 0 && (
        <div
          className="absolute z-50 border border-subtle bg-[var(--bg-secondary)] shadow-lg overflow-hidden"
          style={{
            top: dropdownPos.top,
            left: dropdownPos.left,
            maxHeight: '160px',
            minWidth: '200px',
            maxWidth: '320px',
          }}
        >
          <div className="overflow-y-auto" style={{ maxHeight: '160px' }}>
            {suggestions.slice(0, 20).map((s, idx) => {
              const kl = kindLabel(s.kind);
              return (
                <div
                  key={`${s.kind}-${s.label}`}
                  className={`flex items-center gap-2 px-3 py-1 cursor-pointer text-xs ${
                    idx === selectedIdx
                      ? 'bg-[var(--highlight)] text-title'
                      : 'text-title hover:bg-[var(--highlight)]'
                  }`}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    applySuggestion(s);
                  }}
                  onMouseEnter={() => setSelectedIdx(idx)}
                >
                  <span className={`text-[9px] font-mono w-6 flex-shrink-0 ${kl.className}`}>
                    {kl.text}
                  </span>
                  <span className="font-mono truncate">{s.label}</span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
