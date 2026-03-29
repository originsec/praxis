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
