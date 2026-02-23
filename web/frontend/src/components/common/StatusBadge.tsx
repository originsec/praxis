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
  online: 'bg-[var(--accent-success)]/10',
  warning: 'bg-[var(--accent-warning)]/10',
  offline: 'bg-[var(--accent-error)]/10',
  info: 'bg-[var(--accent-info)]/10',
};

export function StatusBadge({ status, label, showDot = true }: StatusBadgeProps) {
  return (
    <span
      className={`inline-flex items-center gap-1 text-[9px] ${statusColors[status]}`}
    >
      {showDot && <Circle size={5} fill="currentColor" />}
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
