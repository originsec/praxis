import { Circle } from 'lucide-react';

interface StatusBadgeProps {
  status: 'online' | 'warning' | 'offline' | 'info';
  label?: string;
  showDot?: boolean;
}

const statusColors = {
  online: 'status-online',
  warning: 'status-warning',
  offline: 'status-offline',
  info: 'status-info',
};

const statusBgColors = {
  online: 'bg-green-500/10',
  warning: 'bg-yellow-500/10',
  offline: 'bg-red-500/10',
  info: 'bg-blue-500/10',
};

export function StatusBadge({ status, label, showDot = true }: StatusBadgeProps) {
  return (
    <span
      className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-medium ${statusColors[status]} ${statusBgColors[status]}`}
    >
      {showDot && <Circle size={8} fill="currentColor" />}
      {label}
    </span>
  );
}

//
// Helper to determine node status based on last update time.
//
export function getNodeStatus(lastUpdate: string): 'online' | 'warning' | 'offline' {
  const lastUpdateTime = new Date(lastUpdate).getTime();
  const now = Date.now();
  const diffSeconds = (now - lastUpdateTime) / 1000;

  if (diffSeconds < 60) return 'online';
  if (diffSeconds < 120) return 'warning';
  return 'offline';
}

//
// Helper for operation status.
//
export function getOperationStatusColor(
  status: string
): 'online' | 'warning' | 'offline' | 'info' {
  switch (status) {
    case 'Running':
      return 'info';
    case 'Completed':
      return 'online';
    case 'Failed':
      return 'offline';
    case 'Cancelled':
      return 'warning';
    case 'Queued':
      return 'warning';
    default:
      return 'info';
  }
}
