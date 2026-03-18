import { X } from 'lucide-react';
import { useEffect, useState, useCallback, useRef, type ReactNode } from 'react';
import { nextZIndex, currentZIndex } from '../../utils/zIndex';

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

const sizeDefaults: Record<string, { width: number; height: number }> = {
  sm: { width: 420, height: 300 },
  md: { width: 520, height: 400 },
  lg: { width: 672, height: 500 },
  xl: { width: 896, height: 600 },
  full: { width: Math.round(window.innerWidth * 0.9), height: Math.round(window.innerHeight * 0.9) },
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

function getStoredPos(key: string): { top: number; left: number } | null {
  try {
    const raw = localStorage.getItem(`modal-pos-${key}`);
    if (raw) return JSON.parse(raw);
  } catch { /* ignore */ }
  return null;
}

function savePos(key: string, top: number, left: number) {
  try {
    localStorage.setItem(`modal-pos-${key}`, JSON.stringify({ top: Math.round(top), left: Math.round(left) }));
  } catch { /* ignore */ }
}

export function Modal({ isOpen, onClose, title, children, size = 'md', headerActions, noPadding, resizable, storageKey, defaultWidth, defaultHeight }: ModalProps) {

  const effectiveWidth = defaultWidth ?? sizeDefaults[size].width;
  const effectiveHeight = defaultHeight ?? sizeDefaults[size].height;

  //
  // Size state — initialized from localStorage or defaults.
  //

  const [modalSize, setModalSize] = useState<{ width: number; height: number }>(() => {
    if (resizable && storageKey) {
      const stored = getStoredSize(storageKey);
      if (stored) return stored;
    }
    return { width: effectiveWidth, height: effectiveHeight };
  });

  //
  // Position state — starts centered, persisted per storageKey.
  //

  const [pos, setPos] = useState(() => {
    if (storageKey) {
      const stored = getStoredPos(storageKey);
      if (stored) return stored;
    }
    return {
      top: Math.max(8, (window.innerHeight - effectiveHeight) / 2),
      left: Math.max(8, (window.innerWidth - effectiveWidth) / 2),
    };
  });

  //
  // Z-index management — bring to front on interaction.
  //

  const [zIndex, setZIndex] = useState(() => nextZIndex());

  const bringToFront = useCallback(() => {
    setZIndex(prev => {
      if (prev < currentZIndex()) return nextZIndex();
      return prev;
    });
  }, []);

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
    }

    return () => {
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [isOpen, onClose]);

  //
  // Drag-to-move via the header bar.
  //

  const [isDragging, setIsDragging] = useState(false);
  const dragStart = useRef({ x: 0, y: 0, top: 0, left: 0 });

  const handleDragStart = useCallback((e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('button')) return;
    e.preventDefault();
    bringToFront();
    setIsDragging(true);
    dragStart.current = { x: e.clientX, y: e.clientY, top: pos.top, left: pos.left };
    document.body.style.userSelect = 'none';
  }, [pos, bringToFront]);

  useEffect(() => {
    if (!isDragging) return;
    const handleMouseMove = (e: MouseEvent) => {
      setPos({
        top: Math.max(0, dragStart.current.top + (e.clientY - dragStart.current.y)),
        left: Math.max(0, dragStart.current.left + (e.clientX - dragStart.current.x)),
      });
    };
    const handleMouseUp = (e: MouseEvent) => {
      setIsDragging(false);
      document.body.style.userSelect = '';
      const finalPos = {
        top: Math.max(0, dragStart.current.top + (e.clientY - dragStart.current.y)),
        left: Math.max(0, dragStart.current.left + (e.clientX - dragStart.current.x)),
      };
      setPos(finalPos);
      if (storageKey) savePos(storageKey, finalPos.top, finalPos.left);
    };
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isDragging, storageKey]);

  //
  // Resize handler — direct drag (not 2x centered).
  //

  const handleResizeStart = useCallback((e: React.MouseEvent) => {
    if (!resizable || !storageKey) return;
    e.preventDefault();
    e.stopPropagation();

    const startX = e.clientX;
    const startY = e.clientY;
    const startW = modalRef.current?.offsetWidth ?? modalSize.width;
    const startH = modalRef.current?.offsetHeight ?? modalSize.height;

    const onMouseMove = (ev: MouseEvent) => {
      const newW = Math.max(300, Math.min(window.innerWidth * 0.95, startW + (ev.clientX - startX)));
      const newH = Math.max(200, Math.min(window.innerHeight * 0.95, startH + (ev.clientY - startY)));
      setModalSize({ width: newW, height: newH });
    };

    const onMouseUp = (ev: MouseEvent) => {
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', onMouseUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      const finalW = Math.max(300, Math.min(window.innerWidth * 0.95, startW + (ev.clientX - startX)));
      const finalH = Math.max(200, Math.min(window.innerHeight * 0.95, startH + (ev.clientY - startY)));
      saveSize(storageKey, finalW, finalH);
    };

    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
    document.body.style.cursor = 'nwse-resize';
    document.body.style.userSelect = 'none';
  }, [resizable, storageKey, modalSize]);

  if (!isOpen) return null;

  return (
    <div
      ref={modalRef}
      onPointerDownCapture={bringToFront}
      className="fixed flex flex-col bg-panel border border-subtle shadow-2xl ascii-box"
      style={{
        width: modalSize.width,
        ...(resizable ? { height: modalSize.height } : {}),
        maxWidth: '95vw',
        maxHeight: '95vh',
        top: pos.top,
        left: pos.left,
        zIndex,
      }}
    >
      {/*
      //
      // Header — drag handle.
      //
      */}
      <div
        onMouseDown={handleDragStart}
        className={`flex items-center justify-between px-3 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)] flex-shrink-0 select-none ${isDragging ? 'cursor-grabbing' : 'cursor-grab'}`}
      >
        <span className="text-[11px] font-medium text-highlight truncate">{title}</span>
        <div className="flex items-center gap-0.5">
          {headerActions}
          <button
            onClick={onClose}
            className="p-1 text-muted hover:text-[var(--text-primary)] transition-colors"
          >
            <X size={11} />
          </button>
        </div>
      </div>

      {/*
      //
      // Content.
      //
      */}
      <div className={`flex-1 overflow-auto min-h-0 ${noPadding ? '' : 'p-4'}`}>{children}</div>

      {/*
      //
      // Resize handle (bottom-right corner).
      //
      */}
      {resizable && (
        <div
          onMouseDown={handleResizeStart}
          className="absolute bottom-0 right-0 w-3 h-3 cursor-nwse-resize z-10"
          style={{ borderRight: '2px solid var(--text-muted)', borderBottom: '2px solid var(--text-muted)', opacity: 0.3 }}
        />
      )}
    </div>
  );
}
