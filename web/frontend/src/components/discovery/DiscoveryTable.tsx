import { Cpu, Lock, RefreshCw } from 'lucide-react';
import { DataTable, type ColumnDef } from '../common/DataTable';
import type { DiscoveredLlmEndpoint } from '../../api/types';

interface DiscoveryTableProps {
  endpoints: DiscoveredLlmEndpoint[];
  showNodeColumn: boolean;
  isLoading?: boolean;
}

export function DiscoveryTable({
  endpoints,
  showNodeColumn,
  isLoading,
}: DiscoveryTableProps) {
  if (endpoints.length === 0 && !isLoading) {
    return (
      <div className="p-12 text-center">
        <Cpu size={48} className="mx-auto mb-4 text-muted opacity-50" />
        <h2 className="text-title font-semibold text-lg mb-2">No Endpoints Discovered</h2>
        <p className="text-muted">
          Enable agent discovery and the proxy will probe connections for OpenAI-compatible endpoints.
        </p>
      </div>
    );
  }

  const columns: ColumnDef<DiscoveredLlmEndpoint>[] = [
    ...(showNodeColumn ? [{
      key: 'node_id',
      header: 'Node',
      render: (_: unknown, row: DiscoveredLlmEndpoint) => (
        <span className="font-mono text-muted">{row.node_id.slice(0, 8)}</span>
      ),
    }] : []),
    {
      key: 'endpoint',
      header: 'Endpoint',
      render: (_: unknown, row: DiscoveredLlmEndpoint) => {
        const displayHost = row.domain || row.ip_address;
        return (
          <div className="flex items-center gap-2">
            {row.is_https && <Lock size={12} className="text-[var(--accent-success)]" />}
            <span className="font-mono text-highlight">{displayHost}</span>
          </div>
        );
      },
    },
    {
      key: 'port',
      header: 'Port',
      render: (_: unknown, row: DiscoveredLlmEndpoint) => (
        <span className="font-mono text-title">{row.port}</span>
      ),
    },
    {
      key: 'models',
      header: 'Models',
      render: (_: unknown, row: DiscoveredLlmEndpoint) =>
        row.models.length > 0
          ? <span className="text-title" title={row.models.join(', ')}>{row.models.length}</span>
          : <span className="text-muted">-</span>,
    },
    {
      key: 'api_key',
      header: 'Key',
      render: (_: unknown, row: DiscoveredLlmEndpoint) =>
        row.api_key
          ? <span className="text-[var(--accent-success)]">Yes</span>
          : <span className="text-muted">No</span>,
    },
    {
      key: 'discovered_at',
      header: 'Discovered',
      render: (_: unknown, row: DiscoveredLlmEndpoint) => (
        <span className="text-muted font-mono">{new Date(row.discovered_at).toLocaleString()}</span>
      ),
    },
  ];

  return (
    <DataTable
      data={endpoints}
      columns={columns}
      getRowKey={row => row.id}
      emptyMessage={
        isLoading ? (
          <span>
            <RefreshCw size={16} className="inline mr-2 animate-spin" />
            Loading discovered endpoints...
          </span>
        ) : 'No endpoints discovered'
      }
    />
  );
}
