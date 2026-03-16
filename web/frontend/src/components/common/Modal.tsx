import { X } from 'lucide-react';
import { useEffect, useState, useCallback, useRef, type ReactNode } from 'react';

interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  size?: 'sm' | 'md' | 'lg' | 'xl' | 'full';
  headerActions?: ReactNode;
  noPadding?: boolean;
  resizable?: boolean;
  storageKey?: string;
  defaultWidth?: number;
  defaultHeight?: number;
}

const sizeClasses = {
  sm: 'max-w-md',
  md: 'max-w-lg',
  lg: 'max-w-2xl',
  xl: 'max-w-4xl',
  full: 'max-w-[95vw]',
};

function getStoredSize(key: string): { width: number; height: number } | null {
  try {
    const raw = localStorage.getItem(`modal-size-${key}`);
    if (raw) return JSON.parse(raw);
  } catch { /* ignore */ }
  return null;
}

function saveSize(key: string, w: number, h: number) {
  try {
    localStorage.setItem(`modal-size-${key}`, JSON.stringify({ width: Math.round(w), height: Math.round(h) }));
  } catch { /* ignore */ }
}

export function Modal({ isOpen, onClose, title, children, size = 'md', headerActions, noPadding, resizable, storageKey, defaultWidth, defaultHeight }: ModalProps) {

  //
  // Resizable state — initialized from localStorage or defaults.
  //

  const [modalSize, setModalSize] = useState<{ width: number; height: number } | null>(() => {
    if (!resizable || !storageKey) return null;
    const stored = getStoredSize(storageKey);
    if (stored) return stored;
    if (defaultWidth && defaultHeight) return { width: defaultWidth, height: defaultHeight };
    return null;
  });

  const modalRef = useRef<HTMLDivElement>(null);

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

  //
  // Corner resize handler — both width and height scale by 2x because the
  // modal is centered, so a 1px cursor movement shifts the edge by 0.5px.
  //

  const handleResizeStart = useCallback((e: React.MouseEvent) => {
    if (!resizable || !storageKey) return;
    e.preventDefault();
    e.stopPropagation();

    const startX = e.clientX;
    const startY = e.clientY;
    const startW = modalRef.current?.offsetWidth ?? defaultWidth ?? 600;
    const startH = modalRef.current?.offsetHeight ?? defaultHeight ?? 400;

    const onMouseMove = (ev: MouseEvent) => {
      const newW = Math.max(300, Math.min(window.innerWidth * 0.95, startW + (ev.clientX - startX) * 2));
      const newH = Math.max(200, Math.min(window.innerHeight * 0.95, startH + (ev.clientY - startY) * 2));
      setModalSize({ width: newW, height: newH });
    };

    const onMouseUp = (ev: MouseEvent) => {
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', onMouseUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      const finalW = Math.max(300, Math.min(window.innerWidth * 0.95, startW + (ev.clientX - startX) * 2));
      const finalH = Math.max(200, Math.min(window.innerHeight * 0.95, startH + (ev.clientY - startY) * 2));
      saveSize(storageKey, finalW, finalH);
    };

    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
    document.body.style.cursor = 'nwse-resize';
    document.body.style.userSelect = 'none';
  }, [resizable, storageKey, defaultWidth, defaultHeight]);

  if (!isOpen) return null;

  const hasCustomSize = resizable && modalSize;
  const sizeStyle = hasCustomSize ? {
    width: modalSize.width,
    height: modalSize.height,
    maxWidth: '95vw',
    maxHeight: '95vh',
  } : undefined;

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
        ref={modalRef}
        className={`relative bg-panel border border-subtle shadow-2xl ${hasCustomSize ? '' : `${sizeClasses[size]} w-full`} mx-4 ${!hasCustomSize ? (size === 'full' ? 'h-[90vh]' : 'max-h-[90vh]') : ''} flex flex-col ascii-box`}
        style={sizeStyle}
      >
        {/*
        //
        // Header.
        //
        */}
        <div className="flex items-center justify-between px-4 py-2.5 border-b border-subtle bg-[var(--bg-tertiary)]">
          <h2 className="text-highlight font-semibold text-xs">{title}</h2>
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

        {/*
        //
        // Resize handle (bottom-right corner).
        //
        */}
        {resizable && (
          <div
            onMouseDown={handleResizeStart}
            className="absolute bottom-0 right-0 w-4 h-4 cursor-nwse-resize z-10 group"
            style={{ touchAction: 'none' }}
          >
            <svg width="16" height="16" viewBox="0 0 16 16" className="text-[var(--border-subtle)] group-hover:text-[var(--text-muted)] transition-colors">
              <line x1="14" y1="4" x2="4" y2="14" stroke="currentColor" strokeWidth="1" />
              <line x1="14" y1="8" x2="8" y2="14" stroke="currentColor" strokeWidth="1" />
              <line x1="14" y1="12" x2="12" y2="14" stroke="currentColor" strokeWidth="1" />
            </svg>
          </div>
        )}
      </div>
    </div>
  );
}
