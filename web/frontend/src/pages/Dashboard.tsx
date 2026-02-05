import { useMemo, useEffect } from 'react';
import { Link } from 'react-router-dom';
import { Server, Bot, Zap, Activity, AlertCircle, GitBranch } from 'lucide-react';
import { useApp } from '../context/AppContext';
import { StatusBadge, getNodeStatus, getOperationStatusColor } from '../components/common/StatusBadge';

interface StatCardProps {
  icon: React.ReactNode;
  label: string;
  value: string | number;
  subtext?: string;
  color?: string;
}

function StatCard({ icon, label, value, subtext, color = 'text-[var(--accent-info)]' }: StatCardProps) {
  return (
    <div className="bg-card p-4 border border-subtle ascii-box">
      <div className="flex items-start justify-between">
        <div>
          <p className="text-muted text-xs tracking-wider uppercase">{label}</p>
          <p className="text-2xl font-bold mt-1 text-highlight">{value}</p>
          {subtext && <p className="text-muted text-xs mt-1">{subtext}</p>}
        </div>
        <div className={`p-2 bg-[var(--bg-tertiary)] ${color}`}>{icon}</div>
      </div>
    </div>
  );
}

export function Dashboard() {
  const { state, requestOperations, requestChainExecutions } = useApp();
  const nodes = state.systemState?.nodes ?? [];
  const operations = state.operations;
  const chainExecutions = state.chains.executions;
  const isConnected = state.connected;

  //
  // Fetch operations and chain executions when connected.
  //
  useEffect(() => {
    if (isConnected) {
      requestOperations();
      requestChainExecutions();
    }
  }, [isConnected, requestOperations, requestChainExecutions]);

  const onlineNodes = nodes.filter((n) => getNodeStatus(n.last_update) === 'online').length;
  const activeSessions = nodes.filter((n) => n.selected_agent?.session_id).length;
  const runningOps = operations.filter((op) => op.status === 'Running').length;
  const runningChains = chainExecutions.filter((exec) => exec.status === 'Running' || exec.status === 'Queued').length;
  const totalAgents = nodes.reduce((acc, n) => acc + n.discovered_agents.length, 0);

  //
  // Combined recent items (operations + chain executions) sorted by time.
  //
  const recentItems = useMemo(() => {
    const opItems = operations.map((op) => ({
      type: 'operation' as const,
      id: op.operation_id,
      name: op.spec.name,
      agent: op.agent_short_name,
      status: op.status,
      time: new Date(op.start_time).getTime(),
    }));
    const chainItems = chainExecutions.map((exec) => ({
      type: 'chain' as const,
      id: exec.execution_id,
      name: exec.chain_name,
      agent: exec.agent_short_name,
      status: exec.status,
      time: new Date(exec.started_at).getTime(),
    }));
    return [...opItems, ...chainItems]
      .sort((a, b) => b.time - a.time)
      .slice(0, 5);
  }, [operations, chainExecutions]);

  return (
    <div className="space-y-6">
      {/*
      //
      // Page header.
      //
      */}
      <div>
        <h1 className="text-2xl font-bold text-highlight">Dashboard</h1>
        <p className="text-muted mt-1">System overview</p>
      </div>

      {/*
      //
      // Stats grid.
      //
      */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <StatCard
          icon={<Server size={20} />}
          label="Connected Nodes"
          value={onlineNodes}
          subtext={`${nodes.length} total`}
          color="text-[var(--accent-success)]"
        />
        <StatCard
          icon={<Bot size={20} />}
          label="Active Sessions"
          value={activeSessions}
          subtext={`${totalAgents} agents discovered`}
          color="text-[var(--accent-success)]"
        />
        <StatCard
          icon={<Zap size={20} />}
          label="Running Operations"
          value={runningOps + runningChains}
          subtext={`${operations.length + chainExecutions.length} total`}
          color="text-[var(--accent-success)]"
        />
        <StatCard
          icon={<Activity size={20} />}
          label="System Status"
          value={
            !isConnected
              ? 'Offline'
              : state.systemState
                ? 'Online'
                : 'Connecting...'
          }
          subtext={
            !isConnected
              ? 'WebSocket disconnected'
              : !state.systemState
                ? 'Waiting for service'
                : undefined
          }
          color={
            !isConnected
              ? 'text-[var(--accent-error)]'
              : state.systemState
                ? 'text-[var(--accent-success)]'
                : 'text-[var(--accent-warning)]'
          }
        />
      </div>

      {/*
      //
      // Two column layout.
      //
      */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {/*
        //
        // Nodes.
        //
        */}
        <div className="bg-card border border-subtle ascii-box">
          <div className="px-4 py-3 border-b border-subtle flex items-center justify-between bg-[var(--bg-tertiary)]">
            <h2 className="text-xs font-medium tracking-wider text-muted">NODES</h2>
            <Link to="/nodes" className="text-xs text-[var(--accent-info)] hover:underline">
              VIEW ALL
            </Link>
          </div>
          <div className="p-2">
            {nodes.length === 0 ? (
              <div className="py-8 text-center text-muted">
                <Server size={32} className="mx-auto mb-2 opacity-50" />
                <p className="text-xs">No nodes connected</p>
              </div>
            ) : (
              <div className="space-y-1">
                {nodes.slice(0, 5).map((node) => (
                  <Link
                    key={node.node_id}
                    to={`/nodes/${node.node_id}`}
                    className="flex items-center justify-between p-2 hover:bg-[var(--highlight)] transition-colors"
                  >
                    <div className="flex items-center gap-3">
                      <Server size={14} className="text-muted" />
                      <div>
                        <p className="text-xs font-medium text-highlight">
                          {node.machine_name || node.node_id.slice(0, 8)}
                        </p>
                        <p className="text-xs text-muted">{node.os_details}</p>
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-muted">
                        {node.discovered_agents.length} agents
                      </span>
                      <StatusBadge status={getNodeStatus(node.last_update)} />
                    </div>
                  </Link>
                ))}
              </div>
            )}
          </div>
        </div>

        {/*
        //
        // Recent Operations.
        //
        */}
        <div className="bg-card border border-subtle ascii-box">
          <div className="px-4 py-3 border-b border-subtle flex items-center justify-between bg-[var(--bg-tertiary)]">
            <h2 className="text-xs font-medium tracking-wider text-muted">RECENT OPERATIONS</h2>
            <Link to="/operations" className="text-xs text-[var(--accent-info)] hover:underline">
              VIEW ALL
            </Link>
          </div>
          <div className="p-2">
            {recentItems.length === 0 ? (
              <div className="py-8 text-center text-muted">
                <Zap size={32} className="mx-auto mb-2 opacity-50" />
                <p className="text-xs">No operations yet</p>
              </div>
            ) : (
              <div className="space-y-1">
                {recentItems.map((item) => (
                  <Link
                    key={item.id}
                    to="/operations"
                    className="flex items-center justify-between p-2 hover:bg-[var(--highlight)] transition-colors"
                  >
                    <div className="flex items-center gap-3">
                      {item.type === 'chain' ? (
                        <GitBranch size={14} className="text-muted" />
                      ) : (
                        <Zap size={14} className="text-muted" />
                      )}
                      <div>
                        <p className="text-xs font-medium text-highlight">{item.name}</p>
                        <p className="text-xs text-muted">{item.agent}</p>
                      </div>
                    </div>
                    <StatusBadge
                      status={getOperationStatusColor(item.status)}
                      label={item.status}
                    />
                  </Link>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>

      {/*
      //
      // Connection warning.
      //
      */}
      {!isConnected && (
        <div className="bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 p-4 flex items-center gap-3 ascii-box">
          <AlertCircle className="text-[var(--accent-error)]" size={20} />
          <div>
            <p className="text-xs font-medium text-[var(--accent-error)]">CONNECTION LOST</p>
            <p className="text-xs text-muted">
              Attempting to reconnect to the Praxis web server...
            </p>
          </div>
        </div>
      )}
      {isConnected && !state.systemState && (
        <div className="bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/30 p-4 flex items-center gap-3 ascii-box">
          <AlertCircle className="text-[var(--accent-warning)]" size={20} />
          <div>
            <p className="text-xs font-medium text-[var(--accent-warning)]">SERVICE UNAVAILABLE</p>
            <p className="text-xs text-muted">
              Connected to web server but the Praxis service is not responding...
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
