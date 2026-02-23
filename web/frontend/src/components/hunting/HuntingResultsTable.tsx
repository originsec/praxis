import { useMemo } from 'react';
import { DataTable, type ColumnDef } from '../common/DataTable';

const ROWS_PER_PAGE = 100;

interface HuntingResultsTableProps {
  columns: string[];
  rows: unknown[][];
  totalCount: number;
}

export function HuntingResultsTable({ columns, rows, totalCount }: HuntingResultsTableProps) {
  const columnDefs = useMemo<ColumnDef<unknown[]>[]>(() =>
    columns.map((col, colIdx) => ({
      key: String(colIdx),
      header: col,
      sortable: true,
      sortFn: (a: unknown[], b: unknown[]) => compareCellValues(a[colIdx], b[colIdx]),
      render: (_: unknown, row: unknown[]) => (
        <CellValue value={row[colIdx]} column={col} />
      ),
    })),
  [columns]);

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
      <DataTable
        data={rows}
        columns={columnDefs}
        getRowKey={(_, index) => index}
        resizable
        stickyHeader
        textFilter
        filterFn={(row, filter) => {
          const term = filter.toLowerCase();
          return row.some(cell => {
            if (cell === null || cell === undefined) return false;
            return String(cell).toLowerCase().includes(term);
          });
        }}
        pagination={{ pageSize: ROWS_PER_PAGE }}
        expandable={{
          render: (row) => <ExpandedRowDetail row={row} columns={columns} />,
        }}
        summary={
          <SummaryText filteredCount={rows.length} totalCount={totalCount} />
        }
        emptyMessage="No results"
        className="flex flex-col flex-1 min-h-0 overflow-hidden"
      />
    </div>
  );
}

//
// Summary adaptor — the actual filtered count comes from DataTable's internal
// filtering, but since we pass the summary as a static prop we need it to work
// with the data we already have. The text filter count is reflected in the data
// length.
//

function SummaryText({ filteredCount, totalCount }: { filteredCount: number; totalCount: number }) {
  return (
    <>
      {filteredCount === totalCount
        ? `${totalCount} row${totalCount !== 1 ? 's' : ''}`
        : `${filteredCount} of ${totalCount} rows`}
    </>
  );
}

//
// Expanded detail view.
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

  if (/^\d{4}-\d{2}-\d{2}T/.test(str)) {
    try {
      return <span className="text-muted font-mono">{new Date(str).toLocaleString()}</span>;
    } catch {
      // Fall through.
    }
  }

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
