import { useState, useRef, useEffect } from 'react';
import { X, Plus, Loader2, FolderOpen } from 'lucide-react';
import type { ExecutionTab } from '../../context/executionTypes';

interface TabBarProps {
  tabs: ExecutionTab[];
  activeTabId: string;
  onSelectTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onRenameTab: (tabId: string, name: string) => void;
  onAddTab: () => void;
  onOpenChain?: () => void;
}

export function TabBar({ tabs, activeTabId, onSelectTab, onCloseTab, onRenameTab, onAddTab, onOpenChain }: TabBarProps) {
  const [editingTabId, setEditingTabId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editingTabId && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editingTabId]);

  const handleDoubleClick = (tab: ExecutionTab) => {
    setEditingTabId(tab.id);
    setEditValue(tab.name);
  };

  const commitRename = () => {
    if (editingTabId && editValue.trim()) {
      onRenameTab(editingTabId, editValue.trim());
    }
    setEditingTabId(null);
  };

  return (
    <div className="flex items-center border-b border-subtle bg-[var(--bg-secondary)] overflow-x-auto scrollbar-thin">
      {tabs.map((tab) => {
        const isActive = tab.id === activeTabId;
        const isRunning = !!tab.executionId;

        return (
          <div
            key={tab.id}
            onClick={() => onSelectTab(tab.id)}
            onDoubleClick={() => handleDoubleClick(tab)}
            className={`group flex items-center gap-1.5 px-3 py-2 text-xs cursor-pointer border-r border-subtle select-none min-w-0 max-w-[200px] ${
              isActive
                ? 'bg-[var(--bg-primary)] text-title border-b-2 border-b-[var(--accent-success)]'
                : 'text-muted hover:text-title hover:bg-[var(--bg-tertiary)]'
            }`}
          >
            {isRunning && (
              <Loader2 size={10} className="animate-spin text-[var(--accent-warning)] flex-shrink-0" />
            )}
            {tab.isDirty && !isRunning && (
              <span className="w-1.5 h-1.5 rounded-full bg-[var(--accent-warning)] flex-shrink-0" />
            )}

            {editingTabId === tab.id ? (
              <input
                ref={inputRef}
                value={editValue}
                onChange={(e) => setEditValue(e.target.value)}
                onBlur={commitRename}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') commitRename();
                  if (e.key === 'Escape') setEditingTabId(null);
                }}
                className="bg-transparent border-b border-[var(--accent-success)] text-xs text-title outline-none w-full min-w-[40px]"
                onClick={(e) => e.stopPropagation()}
              />
            ) : (
              <span className="truncate">{tab.name}</span>
            )}

            <button
              onClick={(e) => {
                e.stopPropagation();
                onCloseTab(tab.id);
              }}
              className="opacity-0 group-hover:opacity-100 hover:text-[var(--accent-error)] transition-opacity flex-shrink-0 ml-auto"
            >
              <X size={10} />
            </button>
          </div>
        );
      })}

      <button
        onClick={onAddTab}
        className="flex items-center px-2 py-2 text-muted hover:text-title hover:bg-[var(--bg-tertiary)] transition-colors"
        title="New tab"
      >
        <Plus size={12} />
      </button>

      {onOpenChain && (
        <button
          onClick={onOpenChain}
          className="flex items-center gap-1 px-2 py-2 text-muted hover:text-title hover:bg-[var(--bg-tertiary)] transition-colors"
          title="Open chain from library"
        >
          <FolderOpen size={12} />
        </button>
      )}
    </div>
  );
}
