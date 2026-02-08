import { Cpu, Lock, RefreshCw } from 'lucide-react';
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

  return (
    <table className="w-full text-xs">
      <thead>
        <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
          {showNodeColumn && (
            <th className="text-left px-4 py-2 text-muted tracking-wider">NODE</th>
          )}
          <th className="text-left px-4 py-2 text-muted tracking-wider">ENDPOINT</th>
          <th className="text-left px-4 py-2 text-muted tracking-wider">PORT</th>
          <th className="text-left px-4 py-2 text-muted tracking-wider">MODELS</th>
          <th className="text-left px-4 py-2 text-muted tracking-wider">KEY</th>
          <th className="text-left px-4 py-2 text-muted tracking-wider">DISCOVERED</th>
        </tr>
      </thead>
      <tbody>
        {isLoading && endpoints.length === 0 && (
          <tr>
            <td colSpan={showNodeColumn ? 6 : 5} className="px-4 py-8 text-center text-muted">
              <RefreshCw size={16} className="inline mr-2 animate-spin" />
              Loading discovered endpoints...
            </td>
          </tr>
        )}
        {endpoints.map((endpoint) => {
          const displayHost = endpoint.domain || endpoint.ip_address;

          return (
            <tr
              key={endpoint.id}
              className="border-b border-dim hover:bg-[var(--highlight)]"
            >
              {showNodeColumn && (
                <td className="px-4 py-2 font-mono text-muted">
                  {endpoint.node_id.slice(0, 8)}
                </td>
              )}
              <td className="px-4 py-2">
                <div className="flex items-center gap-2">
                  {endpoint.is_https && (
                    <Lock size={12} className="text-[var(--accent-success)]" />
                  )}
                  <span className="font-mono text-highlight">{displayHost}</span>
                </div>
              </td>
              <td className="px-4 py-2 font-mono text-title">
                {endpoint.port}
              </td>
              <td className="px-4 py-2 text-title">
                {endpoint.models.length > 0 ? (
                  <span title={endpoint.models.join(', ')}>{endpoint.models.length}</span>
                ) : (
                  <span className="text-muted">-</span>
                )}
              </td>
              <td className="px-4 py-2">
                {endpoint.api_key ? (
                  <span className="text-[var(--accent-success)]">Yes</span>
                ) : (
                  <span className="text-muted">No</span>
                )}
              </td>
              <td className="px-4 py-2 text-muted font-mono">
                {new Date(endpoint.discovered_at).toLocaleString()}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
