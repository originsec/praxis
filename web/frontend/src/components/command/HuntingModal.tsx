import { useState, useEffect, useRef } from 'react';
import { Crosshair, Play, Loader2 } from 'lucide-react';
import { Modal } from '../common/Modal';
import { useApp } from '../../context/AppContext';

interface HuntingModalProps {
  onClose: () => void;
}

export function HuntingModal({ onClose }: HuntingModalProps) {
  const { state, huntingQuery, huntingSetQuery } = useApp();
  const { hunting } = state;
  const [localQuery, setLocalQuery] = useState(hunting.query);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleRun = () => {
    if (!localQuery.trim() || hunting.isRunning) return;
    huntingSetQuery(localQuery.trim());
    huntingQuery(localQuery.trim());
  };

  return (
    <Modal
      isOpen={true}
      onClose={onClose}
      title="Hunting"
      size="full"
      noPadding
    >
      <div className="flex flex-col h-[75vh] p-4 gap-3">
        {/*
        //
        // Query input.
        //
        */}
        <div className="flex gap-2">
          <textarea
            ref={inputRef}
            value={localQuery}
            onChange={e => setLocalQuery(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) handleRun();
            }}
            placeholder="Enter KQL query..."
            className="flex-1 bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm font-mono text-[var(--text-primary)] placeholder-[var(--text-secondary)] focus:outline-none focus:border-[var(--border-active)] resize-none"
            rows={3}
          />
          <button
            onClick={handleRun}
            disabled={!localQuery.trim() || hunting.isRunning}
            className="px-4 bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors disabled:opacity-50 self-end"
          >
            {hunting.isRunning ? <Loader2 size={16} className="animate-spin" /> : <Play size={16} />}
          </button>
        </div>

        {/*
        //
        // Error.
        //
        */}
        {hunting.error && (
          <div className="px-3 py-2 bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 text-xs text-[var(--accent-error)]">
            {hunting.error}
          </div>
        )}

        {/*
        //
        // Results table.
        //
        */}
        <div className="flex-1 overflow-auto border border-subtle">
          {hunting.columns.length === 0 ? (
            <div className="flex items-center justify-center h-full text-muted text-sm">
              <div className="text-center">
                <Crosshair size={32} className="mx-auto mb-3 opacity-50" />
                <p>Run a KQL query to see results</p>
                <p className="text-xs mt-1">Ctrl+Enter to execute</p>
              </div>
            </div>
          ) : (
            <table className="w-full text-xs">
              <thead className="sticky top-0 bg-[var(--bg-tertiary)]">
                <tr>
                  {hunting.columns.map((col, idx) => (
                    <th key={idx} className="px-3 py-2 text-left text-muted font-medium border-b border-subtle whitespace-nowrap">
                      {col}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {hunting.rows.map((row, ridx) => (
                  <tr key={ridx} className="hover:bg-[var(--highlight)] transition-colors">
                    {row.map((cell, cidx) => (
                      <td key={cidx} className="px-3 py-1.5 border-b border-subtle text-highlight whitespace-nowrap max-w-[300px] truncate font-mono">
                        {cell === null ? <span className="text-muted">null</span> : String(cell)}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        {hunting.totalCount > 0 && (
          <div className="text-xs text-muted flex-shrink-0">
            {hunting.rows.length} of {hunting.totalCount} results
          </div>
        )}
      </div>
    </Modal>
  );
}
