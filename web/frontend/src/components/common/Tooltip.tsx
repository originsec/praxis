import type { ReactNode } from 'react';

interface TooltipProps {
  content: string;
  children: ReactNode;
  className?: string;
}

export function Tooltip({ content, children, className = '' }: TooltipProps) {
  return (
    <span className={`tooltip-wrapper ${className}`}>
      {children}
      <span className="tooltip-content">{content}</span>
    </span>
  );
}
