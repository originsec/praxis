import { useState, useRef, useEffect, useCallback, type ReactNode } from 'react';
import { X } from 'lucide-react';
import { nextZIndex, currentZIndex } from '../../utils/zIndex';

interface FloatingPanelProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  defaultWidth?: number;
  defaultHeight?: number;
  headerActions?: ReactNode;
}

export function FloatingPanel({
  title,
  onClose,
  children,
  defaultWidth = 480,
  defaultHeight = 420,
  headerActions,
}: FloatingPanelProps) {
  const [size, setSize] = useState({ width: defaultWidth, height: defaultHeight });
  const [zIndex, setZIndex] = useState(() => nextZIndex());

  const bringToFront = useCallback(() => {
    setZIndex(prev => {
      if (prev < currentZIndex()) return nextZIndex();
      return prev;
    });
  }, []);

  //
  // Position — starts centered, then tracks drag offsets.
  //
  const [pos, setPos] = useState(() => ({
    top: Math.max(8, (window.innerHeight - defaultHeight) / 2),
    left: Math.max(8, (window.innerWidth - defaultWidth) / 2),
  }));

  //
  // Drag-to-move via the header bar.
  //
  const [isDragging, setIsDragging] = useState(false);
  const dragStart = useRef({ x: 0, y: 0, top: 0, left: 0 });

  const handleDragStart = useCallback((e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('button')) return;
    e.preventDefault();
    setIsDragging(true);
    dragStart.current = { x: e.clientX, y: e.clientY, top: pos.top, left: pos.left };
    document.body.style.userSelect = 'none';
  }, [pos]);

  useEffect(() => {
    if (!isDragging) return;
    const handleMouseMove = (e: MouseEvent) => {
      setPos({
        top: Math.max(0, dragStart.current.top + (e.clientY - dragStart.current.y)),
        left: Math.max(0, dragStart.current.left + (e.clientX - dragStart.current.x)),
      });
    };
    const handleMouseUp = () => {
      setIsDragging(false);
      document.body.style.userSelect = '';
    };
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isDragging]);

  //
  // Drag-to-resize from bottom-right corner.
  //
  const [isResizing, setIsResizing] = useState(false);
  const resizeStart = useRef({ x: 0, y: 0, w: 0, h: 0 });

  const handleResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsResizing(true);
    resizeStart.current = { x: e.clientX, y: e.clientY, w: size.width, h: size.height };
    document.body.style.cursor = 'nwse-resize';
    document.body.style.userSelect = 'none';
  }, [size]);

  useEffect(() => {
    if (!isResizing) return;
    const handleMouseMove = (e: MouseEvent) => {
      setSize({
        width: Math.max(320, Math.min(1200, resizeStart.current.w + (e.clientX - resizeStart.current.x))),
        height: Math.max(200, Math.min(900, resizeStart.current.h + (e.clientY - resizeStart.current.y))),
      });
    };
    const handleMouseUp = () => {
      setIsResizing(false);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isResizing]);

  return (
    <div
      onPointerDownCapture={bringToFront}
      className="fixed flex flex-col bg-panel border border-subtle shadow-2xl ascii-box"
      style={{
        width: size.width,
        height: size.height,
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
        className={`flex items-center justify-between px-3 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)] flex-shrink-0 ${isDragging ? 'cursor-grabbing' : 'cursor-grab'}`}
      >
        <span className="text-[11px] font-medium text-highlight truncate select-none">{title}</span>
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
      <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
        {children}
      </div>

      {/*
      //
      // Resize handle — bottom-right corner.
      //
      */}
      <div
        onMouseDown={handleResizeStart}
        className="absolute bottom-0 right-0 w-3 h-3 cursor-nwse-resize z-10"
        style={{ borderRight: '2px solid var(--text-muted)', borderBottom: '2px solid var(--text-muted)', opacity: 0.3 }}
      />
    </div>
  );
}
