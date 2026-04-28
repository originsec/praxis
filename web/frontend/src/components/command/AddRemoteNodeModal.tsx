import { useState } from 'react';
import { X, Globe } from 'lucide-react';
import { useApp } from '../../context/AppContext';

interface AddRemoteNodeModalProps {
  onClose: () => void;
}

export function AddRemoteNodeModal({ onClose }: AddRemoteNodeModalProps) {
  const { addRemoteNode } = useApp();
  const [label, setLabel] = useState('');
  const [url, setUrl] = useState('');
  const [token, setToken] = useState('');

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!label.trim() || !url.trim()) return;
    addRemoteNode(label.trim(), url.trim(), token.trim() || null);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-[9999] flex items-center justify-center bg-black/50">
      <div className="bg-[var(--bg-primary)] border border-subtle shadow-lg w-full max-w-md">
        <div className="flex items-center justify-between px-4 py-3 border-b border-subtle">
          <div className="flex items-center gap-2">
            <Globe size={14} className="text-[var(--accent-info)]" />
            <span className="text-sm font-medium text-highlight">Add Remote Codex Node</span>
          </div>
          <button
            onClick={onClose}
            className="p-1 text-muted hover:text-[var(--text-primary)] transition-colors"
          >
            <X size={14} />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="p-4 space-y-3">
          <div>
            <label className="block text-[10px] text-muted tracking-wider mb-1">LABEL</label>
            <input
              type="text"
              value={label}
              onChange={e => setLabel(e.target.value)}
              placeholder="My Codex Instance"
              className="w-full bg-[var(--bg-secondary)] border border-subtle text-[var(--text-primary)] text-xs px-2 py-1.5 focus:outline-none focus:border-[var(--accent-info)]"
              autoFocus
            />
          </div>

          <div>
            <label className="block text-[10px] text-muted tracking-wider mb-1">URL</label>
            <input
              type="text"
              value={url}
              onChange={e => setUrl(e.target.value)}
              placeholder="ws://host:8765"
              className="w-full bg-[var(--bg-secondary)] border border-subtle text-[var(--text-primary)] text-xs px-2 py-1.5 focus:outline-none focus:border-[var(--accent-info)]"
            />
          </div>

          <div>
            <label className="block text-[10px] text-muted tracking-wider mb-1">TOKEN (OPTIONAL)</label>
            <input
              type="text"
              value={token}
              onChange={e => setToken(e.target.value)}
              placeholder="Bearer token (optional)"
              className="w-full bg-[var(--bg-secondary)] border border-subtle text-[var(--text-primary)] text-xs px-2 py-1.5 focus:outline-none focus:border-[var(--accent-info)]"
            />
          </div>

          <div className="flex justify-end gap-2 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="px-3 py-1.5 text-[10px] text-muted hover:text-[var(--text-primary)] transition-colors"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={!label.trim() || !url.trim()}
              className="px-3 py-1.5 text-[10px] bg-[var(--accent-info)] text-white hover:bg-[var(--accent-info)]/80 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              Add Node
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
