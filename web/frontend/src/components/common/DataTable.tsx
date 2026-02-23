import {
  useState, useMemo, useCallback, useRef, useEffect,
  type ReactNode,
} from 'react';
import { ChevronLeft, ChevronRight, ChevronUp, ChevronDown, ChevronRight as Expand, Search } from 'lucide-react';

export type SortDir = 'asc' | 'desc';

export interface SortState {
  key: string;
  dir: SortDir;
}

export interface ColumnDef<T> {
  key: string;
  header: string;
  width?: number;
  minWidth?: number;
  render?: (value: unknown, row: T, index: number) => ReactNode;
  sortable?: boolean;
  sortFn?: (a: T, b: T) => number;
  hidden?: boolean;
  headerClassName?: string;
  cellClassName?: string;
  pinned?: 'right';
}

export interface RowAction<T> {
  icon: ReactNode;
  label: string;
  onClick: (row: T, index: number) => void;
  visible?: (row: T) => boolean;
  disabled?: (row: T) => boolean;
  hoverColor?: string;
}

export interface PaginationConfig {
  pageSize: number;
  controlled?: {
    page: number;
    totalCount: number;
    onPageChange: (page: number) => void;
  };
}

export interface DataTableProps<T> {
  data: T[];
  columns: ColumnDef<T>[];
  getRowKey: (row: T, index: number) => string | number;

  expandable?: {
    render: (row: T, index: number) => ReactNode;
    singleExpand?: boolean;
  };

  actions?: RowAction<T>[] | ((row: T) => RowAction<T>[]);

  sort?: SortState | null;
  onSortChange?: (sort: SortState | null) => void;

  filterBar?: ReactNode;
  textFilter?: boolean;
  textFilterValue?: string;
  onTextFilterChange?: (v: string) => void;
  filterFn?: (row: T, filter: string) => boolean;

  pagination?: PaginationConfig;

  resizable?: boolean;
  stickyHeader?: boolean;
  pinnedActions?: boolean;

  onRowClick?: (row: T, index: number) => void;
  rowClassName?: string | ((row: T, index: number) => string);

  emptyMessage?: string | ReactNode;
  summary?: ReactNode;
  className?: string;
  compact?: boolean;
}

const DEFAULT_COL_WIDTH = 150;
const MIN_COL_WIDTH = 60;

export function DataTable<T>({
  data,
  columns: allColumns,
  getRowKey,
  expandable,
  actions,
  sort: controlledSort,
  onSortChange,
  filterBar,
  textFilter,
  textFilterValue,
  onTextFilterChange,
  filterFn,
  pagination,
  resizable,
  stickyHeader,
  pinnedActions,
  onRowClick,
  rowClassName,
  emptyMessage,
  summary,
  className,
  compact,
}: DataTableProps<T>) {
  const columns = useMemo(() => allColumns.filter(c => !c.hidden), [allColumns]);
  const isControlledSort = controlledSort !== undefined;
  const [internalSort, setInternalSort] = useState<SortState | null>(null);
  const sort = isControlledSort ? controlledSort : internalSort;

  const [internalFilter, setInternalFilter] = useState('');
  const filterValue = textFilterValue ?? internalFilter;

  const [internalPage, setInternalPage] = useState(0);
  const [expanded, setExpanded] = useState<Set<string | number>>(new Set());

  const [colWidths, setColWidths] = useState<number[]>([]);
  const headerRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  //
  // Reset column widths when the set of columns changes (new keys), not on
  // every render. Using a stable string key prevents resize-triggered
  // re-renders from resetting widths.
  //

  const columnKeys = columns.map(c => c.key).join(',');

  useEffect(() => {
    const containerWidth = containerRef.current?.clientWidth ?? 0;
    const expandCol = expandable ? 40 : 0;
    const actionsCol = actions ? 80 : 0;
    const available = containerWidth - expandCol - actionsCol;
    const perCol = columns.length > 0
      ? Math.max(DEFAULT_COL_WIDTH, Math.floor(available / columns.length))
      : DEFAULT_COL_WIDTH;
    setColWidths(columns.map(c => c.width ?? perCol));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [columnKeys]);

  //
  // Sorting.
  //

  const handleSort = useCallback((key: string) => {
    const next = (prev: SortState | null): SortState | null => {
      if (prev && prev.key === key) {
        if (prev.dir === 'asc') return { key, dir: 'desc' };
        return null;
      }
      return { key, dir: 'asc' };
    };

    if (isControlledSort) {
      onSortChange?.(next(controlledSort ?? null));
    } else {
      setInternalSort(prev => next(prev));
    }
    setInternalPage(0);
    setExpanded(new Set());
  }, [isControlledSort, controlledSort, onSortChange]);

  //
  // Filtering.
  //

  const handleFilterChange = useCallback((v: string) => {
    if (onTextFilterChange) {
      onTextFilterChange(v);
    } else {
      setInternalFilter(v);
    }
    setInternalPage(0);
    setExpanded(new Set());
  }, [onTextFilterChange]);

  const filteredData = useMemo(() => {
    if (!filterValue.trim()) return data;
    if (filterFn) {
      return data.filter(row => filterFn(row, filterValue));
    }
    const term = filterValue.toLowerCase();
    return data.filter(row =>
      columns.some(col => {
        const val = (row as Record<string, unknown>)[col.key];
        if (val === null || val === undefined) return false;
        return String(val).toLowerCase().includes(term);
      })
    );
  }, [data, filterValue, filterFn, columns]);

  //
  // Sorting.
  //

  const sortedData = useMemo(() => {
    if (!sort) return filteredData;
    const col = columns.find(c => c.key === sort.key);
    if (!col) return filteredData;

    const sorted = [...filteredData].sort((a, b) => {
      if (col.sortFn) {
        const cmp = col.sortFn(a, b);
        return sort.dir === 'asc' ? cmp : -cmp;
      }
      const av = (a as Record<string, unknown>)[col.key];
      const bv = (b as Record<string, unknown>)[col.key];
      const cmp = defaultCompare(av, bv);
      return sort.dir === 'asc' ? cmp : -cmp;
    });
    return sorted;
  }, [filteredData, sort, columns]);

  //
  // Pagination.
  //

  const isControlledPagination = pagination?.controlled !== undefined;
  const pageSize = pagination?.pageSize ?? sortedData.length;
  const currentPage = isControlledPagination ? pagination!.controlled!.page : internalPage;
  const totalCount = isControlledPagination ? pagination!.controlled!.totalCount : sortedData.length;
  const totalPages = Math.max(1, Math.ceil(totalCount / pageSize));
  const safePage = Math.min(currentPage, totalPages - 1);

  const pageData = useMemo(() => {
    if (!pagination) return sortedData;
    if (isControlledPagination) return sortedData;
    return sortedData.slice(safePage * pageSize, (safePage + 1) * pageSize);
  }, [sortedData, pagination, isControlledPagination, safePage, pageSize]);

  const handlePageChange = useCallback((page: number) => {
    if (isControlledPagination) {
      pagination!.controlled!.onPageChange(page);
    } else {
      setInternalPage(page);
    }
    setExpanded(new Set());
  }, [isControlledPagination, pagination]);

  //
  // Expand/collapse.
  //

  const toggleExpand = useCallback((key: string | number) => {
    setExpanded(prev => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        if (expandable?.singleExpand !== false) {
          next.clear();
        }
        next.add(key);
      }
      return next;
    });
  }, [expandable]);

  //
  // Resize handling.
  //

  const handleResize = useCallback((colIdx: number, delta: number) => {
    setColWidths(prev => {
      const next = [...prev];
      const minW = columns[colIdx]?.minWidth ?? MIN_COL_WIDTH;
      next[colIdx] = Math.max(minW, (next[colIdx] ?? DEFAULT_COL_WIDTH) + delta);
      return next;
    });
  }, [columns]);

  const handleAutoExpand = useCallback((colIdx: number) => {
    const container = containerRef.current;
    if (!container) return;

    const tables = container.querySelectorAll('table');
    const bodyTable = tables[tables.length > 1 ? 1 : 0];
    if (!bodyTable) return;

    const expandOffset = expandable ? 2 : 1;
    const cells = bodyTable.querySelectorAll(`td:nth-child(${colIdx + expandOffset})`);
    let maxWidth = columns[colIdx]?.minWidth ?? MIN_COL_WIDTH;
    cells.forEach(cell => {
      maxWidth = Math.max(maxWidth, cell.scrollWidth + 16);
    });

    const headerTable = container.querySelector('table');
    if (headerTable) {
      const th = headerTable.querySelectorAll('th')[colIdx + (expandable ? 1 : 0)];
      if (th) maxWidth = Math.max(maxWidth, th.scrollWidth + 16);
    }

    setColWidths(prev => {
      const next = [...prev];
      next[colIdx] = Math.max(next[colIdx], maxWidth);
      return next;
    });
  }, [columns, expandable]);

  //
  // Layout calculations.
  //

  const expandColWidth = expandable ? 40 : 0;
  const actionsColWidth = actions ? 80 : 0;
  const totalColWidth = expandColWidth + colWidths.reduce((s, w) => s + w, 0) + actionsColWidth;

  const hasFilters = filterBar || textFilter;
  const hasPagination = pagination !== undefined;
  const showToolbar = hasFilters || hasPagination || summary;

  const colCount = columns.length + (expandable ? 1 : 0) + (actions ? 1 : 0);
  const py = compact ? 'py-1.5' : 'py-2';

  const pinnedThCls = 'sticky right-0 z-[1] bg-[var(--bg-tertiary)] ';

  //
  // Render the table using either sticky header (split tables) or simple
  // single table layout.
  //

  const renderColGroup = () => (
    <colgroup>
      {expandable && <col style={{ width: expandColWidth }} />}
      {columns.map((col, i) => (
        <col key={col.key} style={resizable ? { width: colWidths[i] ?? DEFAULT_COL_WIDTH } : undefined} />
      ))}
      {actions && <col style={{ width: actionsColWidth }} />}
    </colgroup>
  );

  const renderHeaderCells = () => (
    <>
      {expandable && <th className={`text-left px-4 ${py} text-muted tracking-wider`} style={{ width: expandColWidth }} />}
      {columns.map((col, idx) => {
        const isSortable = col.sortable !== false;
        const sortDir = sort?.key === col.key ? sort.dir : null;
        const pinCls = col.pinned === 'right' ? pinnedThCls : '';

        if (resizable) {
          return (
            <ResizableHeaderCell
              key={col.key}
              colIdx={idx}
              onResize={handleResize}
              onAutoExpand={handleAutoExpand}
              onSort={isSortable ? () => handleSort(col.key) : undefined}
              sortDir={sortDir}
              className={`${col.headerClassName ?? ''} ${pinCls}`}
              py={py}
            >
              {col.header.toUpperCase()}
            </ResizableHeaderCell>
          );
        }

        return (
          <HeaderCell
            key={col.key}
            sortable={isSortable}
            sortDir={sortDir}
            onClick={isSortable ? () => handleSort(col.key) : undefined}
            className={`${col.headerClassName ?? ''} ${pinCls}`}
            py={py}
          >
            {col.header.toUpperCase()}
          </HeaderCell>
        );
      })}
      {actions && <th className={`px-4 ${py} ${pinnedActions ? pinnedThCls : ''}`} />}
    </>
  );

  const renderRow = (row: T, index: number) => {
    const key = getRowKey(row, index);
    const isExpanded = expanded.has(key);
    const rowCls = typeof rowClassName === 'function' ? rowClassName(row, index) : rowClassName ?? '';
    const clickable = onRowClick || expandable;

    return (
      <DataRow
        key={key}
        row={row}
        index={index}
        columns={columns}
        expandable={expandable}
        isExpanded={isExpanded}
        onToggleExpand={() => toggleExpand(key)}
        actions={actions}
        pinnedActions={pinnedActions}
        onRowClick={onRowClick}
        className={rowCls}
        clickable={!!clickable}
        colSpan={colCount}
        py={py}
      />
    );
  };

  const tableStyle = resizable
    ? { width: Math.max(totalColWidth, containerRef.current?.clientWidth ?? 0), tableLayout: 'fixed' as const }
    : undefined;
  const tableClass = resizable ? 'text-xs' : 'w-full text-xs';

  return (
    <div className={className ?? ''}>
      {showToolbar && (
        <div className="flex items-center gap-4 p-4 border-b border-subtle flex-wrap">
          {textFilter && (
            <div className="flex items-center gap-2">
              <Search size={14} className="text-muted" />
              <input
                type="text"
                placeholder="Filter results..."
                value={filterValue}
                onChange={e => handleFilterChange(e.target.value)}
                className="bg-transparent border-b border-subtle text-xs text-title px-2 py-1 w-48 focus:border-[var(--accent-success)] outline-none"
              />
            </div>
          )}
          {filterBar}
          {summary && <span className="text-xs text-muted">{summary}</span>}
          <div className="flex-1" />
          {hasPagination && (
            <PaginationControls
              page={safePage}
              totalPages={totalPages}
              onPageChange={handlePageChange}
            />
          )}
        </div>
      )}

      <div ref={containerRef} className={stickyHeader ? 'flex-1 min-h-0 flex flex-col overflow-hidden' : ''}>
        {stickyHeader ? (
          <>
            <div ref={headerRef} className="overflow-x-hidden flex-shrink-0">
              <table className={tableClass} style={tableStyle}>
                {resizable && renderColGroup()}
                <thead>
                  <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
                    {renderHeaderCells()}
                  </tr>
                </thead>
              </table>
            </div>
            <div
              className="flex-1 overflow-auto"
              onScroll={e => {
                if (headerRef.current) {
                  headerRef.current.scrollLeft = e.currentTarget.scrollLeft;
                }
              }}
            >
              <table className={tableClass} style={tableStyle}>
                {resizable && renderColGroup()}
                <tbody>
                  {pageData.length === 0 ? (
                    <tr>
                      <td colSpan={colCount} className="px-4 py-8 text-center text-muted">
                        {emptyMessage ?? 'No results'}
                      </td>
                    </tr>
                  ) : (
                    pageData.map(renderRow)
                  )}
                </tbody>
              </table>
            </div>
          </>
        ) : (
          <table className={tableClass} style={tableStyle}>
            {resizable && renderColGroup()}
            <thead>
              <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
                {renderHeaderCells()}
              </tr>
            </thead>
            <tbody>
              {pageData.length === 0 ? (
                <tr>
                  <td colSpan={colCount} className="px-4 py-8 text-center text-muted">
                    {emptyMessage ?? 'No results'}
                  </td>
                </tr>
              ) : (
                pageData.map(renderRow)
              )}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

//
// Resizable header cell with drag handle and double-click auto-expand.
//

function ResizableHeaderCell({
  colIdx,
  onResize,
  onAutoExpand,
  onSort,
  sortDir,
  children,
  className,
  py,
}: {
  colIdx: number;
  onResize: (colIdx: number, delta: number) => void;
  onAutoExpand: (colIdx: number) => void;
  onSort?: () => void;
  sortDir: SortDir | null;
  children: ReactNode;
  className?: string;
  py: string;
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
    if (movedRef.current) return;
    onSort?.();
  }, [onSort]);

  return (
    <th
      className={`relative text-left px-4 ${py} text-muted tracking-wider font-normal whitespace-nowrap overflow-hidden select-none ${
        onSort ? 'cursor-pointer hover:text-title transition-colors' : ''
      } ${className ?? ''}`}
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
        onClick={e => e.stopPropagation()}
        className="absolute top-0 right-0 w-[5px] h-full cursor-col-resize group"
      >
        <div className="absolute top-1/4 right-[2px] w-px h-1/2 bg-[var(--border-subtle)] group-hover:bg-[var(--text-muted)] transition-colors" />
      </div>
    </th>
  );
}

//
// Simple header cell with sort indicator.
//

function HeaderCell({
  sortable,
  sortDir,
  onClick,
  children,
  className,
  py,
}: {
  sortable: boolean;
  sortDir: SortDir | null;
  onClick?: () => void;
  children: ReactNode;
  className?: string;
  py: string;
}) {
  return (
    <th
      className={`text-left px-4 ${py} text-muted tracking-wider font-normal ${
        sortable ? 'cursor-pointer hover:text-title transition-colors select-none' : ''
      } ${className ?? ''}`}
      onClick={onClick}
    >
      <span className="flex items-center gap-1">
        {children}
        {sortDir === 'asc' && <ChevronUp size={10} className="text-[var(--accent-info)] flex-shrink-0" />}
        {sortDir === 'desc' && <ChevronDown size={10} className="text-[var(--accent-info)] flex-shrink-0" />}
      </span>
    </th>
  );
}

//
// Data row with expand chevron, cells, and actions.
//

function DataRow<T>({
  row,
  index,
  columns,
  expandable,
  isExpanded,
  onToggleExpand,
  actions,
  pinnedActions,
  onRowClick,
  className,
  clickable,
  colSpan,
  py,
}: {
  row: T;
  index: number;
  columns: ColumnDef<T>[];
  expandable?: DataTableProps<T>['expandable'];
  isExpanded: boolean;
  onToggleExpand: () => void;
  actions?: DataTableProps<T>['actions'];
  pinnedActions?: boolean;
  onRowClick?: (row: T, index: number) => void;
  className: string;
  clickable: boolean;
  colSpan: number;
  py: string;
}) {
  const pinnedTdCls = 'sticky right-0 z-[1] bg-[var(--bg-secondary)] group-hover:bg-[var(--highlight)] ';

  const handleClick = () => {
    if (onRowClick) {
      onRowClick(row, index);
    } else if (expandable) {
      onToggleExpand();
    }
  };

  const rowActions = actions
    ? typeof actions === 'function' ? actions(row) : actions
    : [];

  return (
    <>
      <tr
        className={`group border-b border-dim hover:bg-[var(--highlight)] transition-colors ${
          clickable ? 'cursor-pointer' : ''
        } ${className}`}
        onClick={clickable ? handleClick : undefined}
      >
        {expandable && (
          <td
            className={`px-4 ${py} text-muted`}
            style={{ width: 40 }}
            onClick={e => {
              if (onRowClick) {
                e.stopPropagation();
                onToggleExpand();
              }
            }}
          >
            <Expand
              size={12}
              className={`transition-transform ${isExpanded ? 'rotate-90' : ''}`}
            />
          </td>
        )}
        {columns.map(col => {
          const val = (row as Record<string, unknown>)[col.key];
          const pinCls = col.pinned === 'right' ? pinnedTdCls : '';
          return (
            <td
              key={col.key}
              className={`px-4 ${py} truncate overflow-hidden ${col.cellClassName ?? ''} ${pinCls}`}
              title={val !== null && val !== undefined ? String(val) : ''}
            >
              {col.render ? col.render(val, row, index) : (val !== null && val !== undefined ? String(val) : '')}
            </td>
          );
        })}
        {actions && (
          <td className={`px-4 ${py} ${pinnedActions ? pinnedTdCls : ''}`}>
            <ActionsCell row={row} index={index} actions={rowActions} />
          </td>
        )}
      </tr>
      {expandable && isExpanded && (
        <tr className="bg-[var(--bg-tertiary)]">
          <td colSpan={colSpan} className="px-4 py-4">
            {expandable.render(row, index)}
          </td>
        </tr>
      )}
    </>
  );
}

//
// Action buttons cell.
//

function ActionsCell<T>({
  row,
  index,
  actions,
}: {
  row: T;
  index: number;
  actions: RowAction<T>[];
}) {
  return (
    <div className="flex items-center gap-1 justify-end" onClick={e => e.stopPropagation()}>
      {actions.map((action, i) => {
        if (action.visible && !action.visible(row)) return null;
        const isDisabled = action.disabled?.(row) ?? false;
        const hoverCls = action.hoverColor
          ? ''
          : 'hover:text-title';

        return (
          <button
            key={i}
            onClick={() => !isDisabled && action.onClick(row, index)}
            disabled={isDisabled}
            className={`p-2 text-muted transition-colors ${hoverCls} ${
              isDisabled ? 'opacity-30 cursor-not-allowed' : ''
            }`}
            style={!isDisabled && action.hoverColor ? {} : undefined}
            onMouseEnter={e => {
              if (!isDisabled && action.hoverColor) {
                (e.currentTarget as HTMLElement).style.color = action.hoverColor;
              }
            }}
            onMouseLeave={e => {
              if (action.hoverColor) {
                (e.currentTarget as HTMLElement).style.color = '';
              }
            }}
            title={action.label}
          >
            {action.icon}
          </button>
        );
      })}
    </div>
  );
}

//
// Pagination controls.
//

function PaginationControls({
  page,
  totalPages,
  onPageChange,
}: {
  page: number;
  totalPages: number;
  onPageChange: (page: number) => void;
}) {
  return (
    <div className="flex items-center gap-2 text-xs text-muted">
      <button
        onClick={() => onPageChange(Math.max(0, page - 1))}
        disabled={page === 0}
        className="px-2 py-1 border border-subtle hover:text-title hover:border-[var(--border-hover)] disabled:opacity-30 disabled:hover:text-muted disabled:hover:border-subtle transition-colors"
      >
        <ChevronLeft size={12} />
      </button>
      <span className="font-mono">
        {page + 1} / {totalPages}
      </span>
      <button
        onClick={() => onPageChange(Math.min(totalPages - 1, page + 1))}
        disabled={page >= totalPages - 1}
        className="px-2 py-1 border border-subtle hover:text-title hover:border-[var(--border-hover)] disabled:opacity-30 disabled:hover:text-muted disabled:hover:border-subtle transition-colors"
      >
        <ChevronRight size={12} />
      </button>
    </div>
  );
}

//
// Default comparator for cell values.
//

function defaultCompare(a: unknown, b: unknown): number {
  if (a === null || a === undefined) return b === null || b === undefined ? 0 : 1;
  if (b === null || b === undefined) return -1;
  if (typeof a === 'number' && typeof b === 'number') return a - b;
  if (typeof a === 'boolean' && typeof b === 'boolean') return a === b ? 0 : a ? -1 : 1;

  const sa = String(a);
  const sb = String(b);
  const na = Number(sa);
  const nb = Number(sb);
  if (!isNaN(na) && !isNaN(nb) && sa !== '' && sb !== '') return na - nb;

  return sa.localeCompare(sb);
}
