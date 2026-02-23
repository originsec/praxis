import { X } from 'lucide-react';
import { useEffect, type ReactNode } from 'react';

interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  size?: 'sm' | 'md' | 'lg' | 'xl' | 'full';
  headerActions?: ReactNode;
  noPadding?: boolean;
}

const sizeClasses = {
  sm: 'max-w-md',
  md: 'max-w-lg',
  lg: 'max-w-2xl',
  xl: 'max-w-4xl',
  full: 'max-w-[95vw]',
};

export function Modal({ isOpen, onClose, title, children, size = 'md', headerActions, noPadding }: ModalProps) {
  //
  // Close on escape key.
  //
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };

    if (isOpen) {
      document.addEventListener('keydown', handleKeyDown);
      document.body.style.overflow = 'hidden';
    }

    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      document.body.style.overflow = '';
    };
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      {/*
      //
      // Backdrop.
      //
      */}
      <div
        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
        onClick={onClose}
      />

      {/*
      //
      // Modal.
      //
      */}
      <div
        className={`relative bg-panel border border-subtle shadow-2xl ${sizeClasses[size]} w-full mx-4 max-h-[90vh] flex flex-col ascii-box`}
      >
        {/*
        //
        // Header.
        //
        */}
        <div className="flex items-center justify-between px-4 py-2.5 border-b border-subtle bg-[var(--bg-tertiary)]">
          <h2 className="text-highlight font-semibold text-lg">{title}</h2>
          <div className="flex items-center gap-1">
            {headerActions}
            <button
              onClick={onClose}
              className="p-1 hover:bg-[var(--bg-secondary)] text-muted hover:text-[var(--text-primary)] transition-colors"
            >
              <X size={20} />
            </button>
          </div>
        </div>

        {/*
        //
        // Content.
        //
        */}
        <div className={`flex-1 overflow-auto ${noPadding ? '' : 'p-4'}`}>{children}</div>
      </div>
    </div>
  );
}
