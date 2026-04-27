import { useState } from 'react';
import { Modal } from '../common/Modal';
import { useApp } from '../../context/AppContext';

interface Props {
  isOpen: boolean;
  onClose: () => void;
}

const inputCls =
  'w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle transition-colors';

export function AddRemoteNodeModal({ isOpen, onClose }: Props) {
  const { addRemoteNode } = useApp();
  const [label, setLabel] = useState('');
  const [url, setUrl] = useState('');
  const [token, setToken] = useState('');
  const [error, setError] = useState('');

  const handleSubmit = () => {
    const trimLabel = label.trim();
    const trimUrl = url.trim();

    if (!trimLabel) {
      setError('Label is required.');
      return;
    }
    if (!trimUrl) {
      setError('URL is required.');
      return;
    }
    if (!trimUrl.startsWith('ws://') && !trimUrl.startsWith('wss://')) {
      setError('URL must start with ws:// or wss://');
      return;
    }

    addRemoteNode(trimLabel, trimUrl, token.trim() || null);
    setLabel('');
    setUrl('');
    setToken('');
    setError('');
    onClose();
  };

  const handleClose = () => {
    setError('');
    onClose();
  };

  return (
    <Modal isOpen={isOpen} onClose={handleClose} title="Add Remote Codex Node" size="sm">
      <div className="p-4 space-y-3 flex flex-col h-full">
        <p className="text-[10px] text-muted">
          Connect to a running{' '}
          <span className="font-mono text-[var(--text-secondary)]">codex app-server</span>{' '}
          instance. The node will appear in the list and support ACP sessions.
        </p>

        <div className="space-y-2.5 flex-1">
          <div>
            <label className="block text-[10px] text-muted mb-1 uppercase tracking-wider">Label</label>
            <input
              type="text"
              value={label}
              onChange={e => setLabel(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && handleSubmit()}
              placeholder="My Codex Instance"
              className={inputCls}
              autoFocus
            />
          </div>

          <div>
            <label className="block text-[10px] text-muted mb-1 uppercase tracking-wider">URL</label>
            <input
              type="text"
              value={url}
              onChange={e => setUrl(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && handleSubmit()}
              placeholder="ws://host:8765"
              className={inputCls}
            />
          </div>

          <div>
            <label className="block text-[10px] text-muted mb-1 uppercase tracking-wider">
              Token{' '}
              <span className="normal-case text-muted/70">(optional)</span>
            </label>
            <input
              type="password"
              value={token}
              onChange={e => setToken(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && handleSubmit()}
              placeholder="Bearer token"
              className={inputCls}
            />
          </div>

          {error && (
            <p className="text-[10px] text-[var(--accent-error)]">{error}</p>
          )}
        </div>

        <div className="flex justify-end gap-2 pt-1 border-t border-subtle">
          <button
            onClick={handleClose}
            className="px-3 py-1.5 text-[10px] text-muted hover:text-[var(--text-primary)] transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleSubmit}
            disabled={!label.trim() || !url.trim()}
            className="px-3 py-1.5 text-[10px] bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Add Node
          </button>
        </div>
      </div>
    </Modal>
  );
}
