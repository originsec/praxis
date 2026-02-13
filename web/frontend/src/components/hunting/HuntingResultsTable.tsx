import { useState, useMemo, useEffect, useCallback, useRef } from 'react';
import { ChevronLeft, ChevronRight, ChevronUp, ChevronDown, Search } from 'lucide-react';

const ROWS_PER_PAGE = 100;
const EXPAND_COL_WIDTH = 40;
const DEFAULT_COL_WIDTH = 150;
const MIN_COL_WIDTH = 60;

type SortDir = 'asc' | 'desc';

interface SortState {
  colIdx: number;
  dir: SortDir;
}

interface HuntingResultsTableProps {
  columns: string[];
  rows: unknown[][];
  totalCount: number;
}

export function HuntingResultsTable({ columns, rows, totalCount }: HuntingResultsTableProps) {
  const [filter, setFilter] = useState('');
  const [page, setPage] = useState(0);
  const [expandedRow, setExpandedRow] = useState<number | null>(null);
  const [colWidths, setColWidths] = useState<number[]>([]);
  const [sort, setSort] = useState<SortState | null>(null);
  const headerRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  //
  // Reset state when columns change (new query). Distribute initial widths
  // to fill available space.
  //

  useEffect(() => {
    setPage(0);
    setExpandedRow(null);
    setSort(null);

    //
    // Compute initial column widths. If the container is wider than the
    // default total, distribute space evenly so columns fill the width.
    //

    const containerWidth = containerRef.current?.clientWidth ?? 0;
    const available = containerWidth - EXPAND_COL_WIDTH;
    const perCol = columns.length > 0
      ? Math.max(DEFAULT_COL_WIDTH, Math.floor(available / columns.length))
      : DEFAULT_COL_WIDTH;
    setColWidths(columns.map(() => perCol));
  }, [columns]);

  useEffect(() => {
    setExpandedRow(null);
  }, [rows]);

  const handleResize = useCallback((colIdx: number, delta: number) => {
    setColWidths((prev) => {
      const next = [...prev];
      next[colIdx] = Math.max(MIN_COL_WIDTH, (next[colIdx] ?? DEFAULT_COL_WIDTH) + delta);
      return next;
    });
  }, []);

  //
  // Double-click on a column separator: auto-expand by measuring the widest
  // visible cell content for that column.
  //

  const handleAutoExpand = useCallback((colIdx: number) => {
    const container = containerRef.current;
    if (!container) return;

    //
    // Measure cell widths in the body table (second table in container).
    //

    const bodyTable = container.querySelectorAll('table')[1];
    if (!bodyTable) return;

    const cells = bodyTable.querySelectorAll(`td:nth-child(${colIdx + 2})`); // +2: 1-indexed + expand col
    let maxWidth = MIN_COL_WIDTH;
    cells.forEach((cell) => {
      maxWidth = Math.max(maxWidth, cell.scrollWidth + 16); // +16 for padding
    });

    //
    // Also measure the header text.
    //

    const headerTable = container.querySelector('table');
    if (headerTable) {
      const th = headerTable.querySelectorAll('th')[colIdx + 1]; // +1 for expand col
      if (th) {
        maxWidth = Math.max(maxWidth, th.scrollWidth + 16);
      }
    }

    setColWidths((prev) => {
      const next = [...prev];
      next[colIdx] = Math.max(next[colIdx], maxWidth);
      return next;
    });
  }, []);

  const handleSort = useCallback((colIdx: number) => {
    setSort((prev) => {
      if (prev && prev.colIdx === colIdx) {
        if (prev.dir === 'asc') return { colIdx, dir: 'desc' };
        return null; // third click clears sort
      }
      return { colIdx, dir: 'asc' };
    });
    setPage(0);
    setExpandedRow(null);
  }, []);

  //
  // Client-side text filter.
  //

  const filteredRows = useMemo(() => {
    if (!filter.trim()) return rows;
    const term = filter.toLowerCase();
    return rows.filter((row) =>
      row.some((cell) => {
        if (cell === null || cell === undefined) return false;
        return String(cell).toLowerCase().includes(term);
      })
    );
  }, [rows, filter]);

  //
  // Client-side sort.
  //

  const sortedRows = useMemo(() => {
    if (!sort) return filteredRows;
    const { colIdx, dir } = sort;
    const sorted = [...filteredRows].sort((a, b) => {
      const av = a[colIdx];
      const bv = b[colIdx];
      const cmp = compareCellValues(av, bv);
      return dir === 'asc' ? cmp : -cmp;
    });
    return sorted;
  }, [filteredRows, sort]);

  const totalPages = Math.max(1, Math.ceil(sortedRows.length / ROWS_PER_PAGE));
  const safePage = Math.min(page, totalPages - 1);
  const pageRows = sortedRows.slice(safePage * ROWS_PER_PAGE, (safePage + 1) * ROWS_PER_PAGE);
  const colSpan = columns.length + 1;
  const totalColWidth = EXPAND_COL_WIDTH + colWidths.reduce((s, w) => s + w, 0);
  const tableWidth = Math.max(totalColWidth, containerRef.current?.clientWidth ?? 0);

  if (columns.length === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center py-16 text-muted">
        <div className="text-sm">Run a query to see results</div>
        <div className="text-xs mt-2 opacity-60">Write KQL above and press Ctrl+Enter</div>
      </div>
    );
  }

  return (
    <div className="flex flex-col flex-1 min-h-0 overflow-hidden">
      {/*
      //
      // Filter bar.
      //
      */}
      <div className="flex items-center gap-4 p-4 border-b border-subtle">
        <div className="flex items-center gap-2">
          <Search size={14} className="text-muted" />
          <input
            type="text"
            placeholder="Filter results..."
            value={filter}
            onChange={(e) => { setFilter(e.target.value); setPage(0); setExpandedRow(null); }}
            className="bg-transparent border-b border-subtle text-xs text-title px-2 py-1 w-48 focus:border-[var(--accent-success)] outline-none"
          />
        </div>
        <span className="text-xs text-muted">
          {sortedRows.length === totalCount
            ? `${totalCount} row${totalCount !== 1 ? 's' : ''}`
            : `${sortedRows.length} of ${totalCount} rows`}
        </span>

        <div className="flex-1" />

        <div className="flex items-center gap-2 text-xs text-muted">
          <button
            onClick={() => { setPage(Math.max(0, safePage - 1)); setExpandedRow(null); }}
            disabled={safePage === 0}
            className="px-2 py-1 border border-subtle hover:text-title hover:border-[var(--border-hover)] disabled:opacity-30 disabled:hover:text-muted disabled:hover:border-subtle transition-colors"
          >
            <ChevronLeft size={12} />
          </button>
          <span className="font-mono">
            {safePage + 1} / {totalPages}
          </span>
          <button
            onClick={() => { setPage(Math.min(totalPages - 1, safePage + 1)); setExpandedRow(null); }}
            disabled={safePage >= totalPages - 1}
            className="px-2 py-1 border border-subtle hover:text-title hover:border-[var(--border-hover)] disabled:opacity-30 disabled:hover:text-muted disabled:hover:border-subtle transition-colors"
          >
            <ChevronRight size={12} />
          </button>
        </div>
      </div>

      {/*
      //
      // Table.
      //
      */}
      <div ref={containerRef} className="flex-1 min-h-0 flex flex-col overflow-hidden">
        <div ref={headerRef} className="overflow-x-hidden flex-shrink-0">
          <table className="text-xs" style={{ width: tableWidth, tableLayout: 'fixed' }}>
            <ColGroup colWidths={colWidths} />
            <thead>
              <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
                <th className="text-left px-4 py-2 text-muted tracking-wider" style={{ width: EXPAND_COL_WIDTH }}></th>
                {columns.map((col, idx) => (
                  <ResizableTh
                    key={col}
                    colIdx={idx}
                    onResize={handleResize}
                    onAutoExpand={handleAutoExpand}
                    onSort={handleSort}
                    sortDir={sort?.colIdx === idx ? sort.dir : null}
                  >
                    {col.toUpperCase()}
                  </ResizableTh>
                ))}
              </tr>
            </thead>
          </table>
        </div>
        <div
          className="flex-1 overflow-auto"
          onScroll={(e) => {
            if (headerRef.current) {
              headerRef.current.scrollLeft = e.currentTarget.scrollLeft;
            }
          }}
        >
          <table className="text-xs" style={{ width: tableWidth, tableLayout: 'fixed' }}>
            <ColGroup colWidths={colWidths} />
            <tbody>
              {pageRows.length === 0 ? (
                <tr>
                  <td colSpan={colSpan} className="px-4 py-8 text-center text-muted">
                    No results
                  </td>
                </tr>
              ) : (
                pageRows.map((row, rowIdx) => {
                  const globalIdx = safePage * ROWS_PER_PAGE + rowIdx;
                  const isExpanded = expandedRow === globalIdx;

                  return (
                    <HuntingRow
                      key={globalIdx}
                      row={row}
                      columns={columns}
                      expanded={isExpanded}
                      onToggle={() => setExpandedRow(isExpanded ? null : globalIdx)}
                      colSpan={colSpan}
                    />
                  );
                })
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

//
// Shared colgroup.
//

function ColGroup({ colWidths }: { colWidths: number[] }) {
  return (
    <colgroup>
      <col style={{ width: EXPAND_COL_WIDTH }} />
      {colWidths.map((w, i) => (
        <col key={i} style={{ width: w }} />
      ))}
    </colgroup>
  );
}

//
// Header cell with resize handle + sort-on-click.
//

function ResizableTh({
  colIdx,
  onResize,
  onAutoExpand,
  onSort,
  sortDir,
  children,
}: {
  colIdx: number;
  onResize: (colIdx: number, delta: number) => void;
  onAutoExpand: (colIdx: number) => void;
  onSort: (colIdx: number) => void;
  sortDir: SortDir | null;
  children: React.ReactNode;
}) {
  const draggingRef = useRef(false);
  const startXRef = useRef(0);
  const movedRef = useRef(false);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    draggingRef.current = true;
    movedRef.current = false;
    startXRef.current = e.clientX;

    const onMouseMove = (ev: MouseEvent) => {
      if (!draggingRef.current) return;
      const delta = ev.clientX - startXRef.current;
      if (delta !== 0) {
        movedRef.current = true;
        startXRef.current = ev.clientX;
        onResize(colIdx, delta);
      }
    };

    const onMouseUp = () => {
      draggingRef.current = false;
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', onMouseUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };

    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }, [colIdx, onResize]);

  const handleDoubleClick = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    onAutoExpand(colIdx);
  }, [colIdx, onAutoExpand]);

  const handleHeaderClick = useCallback(() => {
    //
    // Don't sort if we just finished a resize drag.
    //
    if (movedRef.current) return;
    onSort(colIdx);
  }, [colIdx, onSort]);

  return (
    <th
      className="relative text-left px-4 py-2 text-muted tracking-wider font-normal whitespace-nowrap overflow-hidden cursor-pointer select-none hover:text-title transition-colors"
      onClick={handleHeaderClick}
    >
      <span className="flex items-center gap-1">
        {children}
        {sortDir === 'asc' && <ChevronUp size={10} className="text-[var(--accent-info)] flex-shrink-0" />}
        {sortDir === 'desc' && <ChevronDown size={10} className="text-[var(--accent-info)] flex-shrink-0" />}
      </span>
      <div
        onMouseDown={handleMouseDown}
        onDoubleClick={handleDoubleClick}
        onClick={(e) => e.stopPropagation()}
        className="absolute top-0 right-0 w-[5px] h-full cursor-col-resize group"
      >
        <div className="absolute top-1/4 right-[2px] w-px h-1/2 bg-[var(--border-subtle)] group-hover:bg-[var(--text-muted)] transition-colors" />
      </div>
    </th>
  );
}

//
// Data row with expand/collapse.
//

function HuntingRow({
  row,
  columns,
  expanded,
  onToggle,
  colSpan,
}: {
  row: unknown[];
  columns: string[];
  expanded: boolean;
  onToggle: () => void;
  colSpan: number;
}) {
  return (
    <>
      <tr
        className="border-b border-dim hover:bg-[var(--highlight)] cursor-pointer"
        onClick={onToggle}
      >
        <td className="px-4 py-2 text-muted" style={{ width: EXPAND_COL_WIDTH }}>
          <ChevronRight
            size={12}
            className={`transition-transform ${expanded ? 'rotate-90' : ''}`}
          />
        </td>
        {row.map((cell, colIdx) => (
          <td
            key={colIdx}
            className="px-4 py-2 truncate overflow-hidden"
            title={cell !== null && cell !== undefined ? String(cell) : ''}
          >
            <CellValue value={cell} column={columns[colIdx]} />
          </td>
        ))}
      </tr>
      {expanded && (
        <tr className="bg-[var(--bg-tertiary)]">
          <td colSpan={colSpan} className="px-4 py-4">
            <ExpandedRowDetail row={row} columns={columns} />
          </td>
        </tr>
      )}
    </>
  );
}

//
// Expanded detail.
//

function ExpandedRowDetail({ row, columns }: { row: unknown[]; columns: string[] }) {
  return (
    <div className="space-y-3">
      {columns.map((col, idx) => {
        const value = row[idx];
        const isLong = isLongValue(value);

        return (
          <div key={col}>
            <div className="text-[10px] text-muted tracking-wider mb-1">{col.toUpperCase()}</div>
            {isLong ? (
              <pre className="text-[10px] font-mono bg-[var(--bg-primary)] p-2 border border-subtle overflow-auto max-h-64 whitespace-pre-wrap break-all">
                {formatFullValue(value)}
              </pre>
            ) : (
              <div className="text-xs text-title font-mono">
                <CellValue value={value} column={col} />
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

//
// Cell renderer — column-aware styling to match intercept table.
//

function CellValue({ value, column }: { value: unknown; column?: string }) {
  if (value === null || value === undefined) {
    return <span className="text-muted opacity-40">null</span>;
  }
  if (typeof value === 'boolean') {
    return (
      <span className={value ? 'text-[var(--accent-success)]' : 'text-muted'}>
        {String(value)}
      </span>
    );
  }

  //
  // Numeric values — apply status code coloring for response_status.
  //

  if (typeof value === 'number') {
    const col = column?.toLowerCase() ?? '';
    if (col === 'response_status' || col === 'status') {
      const color =
        value >= 400 ? 'text-[var(--accent-alert)]'
        : value >= 300 ? 'text-[var(--accent-warning)]'
        : 'text-[var(--accent-success)]';
      return <span className={`font-mono ${color}`}>{value}</span>;
    }
    return <span className="font-mono text-title">{value}</span>;
  }

  if (typeof value === 'object') {
    return <span className="text-muted font-mono">{JSON.stringify(value)}</span>;
  }

  const str = String(value);

  //
  // ISO timestamp formatting.
  //

  if (/^\d{4}-\d{2}-\d{2}T/.test(str)) {
    try {
      return <span className="text-muted font-mono">{new Date(str).toLocaleString()}</span>;
    } catch {
      // Fall through.
    }
  }

  //
  // Column-aware styling to match intercept table.
  //

  const col = column?.toLowerCase() ?? '';
  if (col === 'method') {
    return <span className="text-title font-mono font-medium">{str}</span>;
  }
  if (col === 'url' || col === 'host') {
    return <span className="text-title font-mono">{str}</span>;
  }
  if (col === 'node_id') {
    return <span className="text-title font-mono">{str}</span>;
  }
  if (col === 'agent_short_name' || col === 'agent_name') {
    return <span className="text-highlight">{str}</span>;
  }

  return <span className="text-title">{str}</span>;
}

//
// Sort comparator for cell values.
//

function compareCellValues(a: unknown, b: unknown): number {
  //
  // Nulls sort last.
  //

  if (a === null || a === undefined) return b === null || b === undefined ? 0 : 1;
  if (b === null || b === undefined) return -1;

  //
  // Number comparison.
  //

  if (typeof a === 'number' && typeof b === 'number') return a - b;

  //
  // Boolean comparison.
  //

  if (typeof a === 'boolean' && typeof b === 'boolean') return a === b ? 0 : a ? -1 : 1;

  //
  // String comparison (try numeric first for string-encoded numbers).
  //

  const sa = String(a);
  const sb = String(b);

  const na = Number(sa);
  const nb = Number(sb);
  if (!isNaN(na) && !isNaN(nb) && sa !== '' && sb !== '') return na - nb;

  return sa.localeCompare(sb);
}

function isLongValue(value: unknown): boolean {
  if (value === null || value === undefined) return false;
  if (typeof value === 'object') return true;
  const str = String(value);
  return str.length > 80 || str.includes('\n');
}

function formatFullValue(value: unknown): string {
  if (value === null || value === undefined) return 'null';
  if (typeof value === 'object') {
    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return String(value);
    }
  }
  return String(value);
}
