import { Loader2 } from 'lucide-react';

interface LoadingSpinnerProps {
  size?: number;
  className?: string;
  label?: string;
}

export function LoadingSpinner({ size = 24, className = '', label }: LoadingSpinnerProps) {
  return (
    <div className={`flex items-center gap-2 ${className}`}>
      <Loader2 size={size} className="animate-spin text-[var(--accent-info)]" />
      {label && <span className="text-muted text-sm">{label}</span>}
    </div>
  );
}

export function FullPageSpinner({ label = 'Loading...' }: { label?: string }) {
  return (
    <div className="flex items-center justify-center h-64">
      <LoadingSpinner size={32} label={label} />
    </div>
  );
}
