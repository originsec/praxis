import { useState } from 'react';
import { Modal } from '../common/Modal';
import { useApp } from '../../context/AppContext';

interface AddRemoteNodeModalProps {
  isOpen: boolean;
  onClose: () => void;
}

//
// Static kinds list — mirrors `common::REMOTE_NODE_KINDS` on the
// service side. Keeping it inline here avoids a build-time codegen
// step; updating either side requires the other to follow.
//

const REMOTE_NODE_KINDS: { id: string; displayName: string }[] = [
  { id: 'codex', displayName: 'Codex' },
];

export function AddRemoteNodeModal({ isOpen, onClose }: AddRemoteNodeModalProps) {
  const { addRemoteNode } = useApp();
  const [kindIdx, setKindIdx] = useState(0);
  const [url, setUrl] = useState('');
  const [token, setToken] = useState('');

  const canSubmit = url.trim().length > 0;

  const handleSubmit = () => {
    if (!canSubmit) return;
    const kind = REMOTE_NODE_KINDS[kindIdx]?.id ?? 'codex';
    addRemoteNode(kind, url.trim(), token.trim() ? token.trim() : null);
    setUrl('');
    setToken('');
    setKindIdx(0);
    onClose();
  };

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="Add Remote Node" size="sm">
      <div className="space-y-3">
        <p className="text-xs text-muted">
          Connect to a remote agent server. The node's name is taken from
          the upstream agent once it identifies itself.
        </p>

        <div>
          <label className="block text-[10px] tracking-wider text-muted mb-1">
            TYPE
          </label>
          <div className="grid grid-cols-1 gap-1">
            {REMOTE_NODE_KINDS.map((kind, idx) => {
              const selected = idx === kindIdx;
              return (
                <button
                  key={kind.id}
                  type="button"
                  onClick={() => setKindIdx(idx)}
                  className={`flex items-center justify-between px-2 py-1.5 text-left text-xs border transition-colors ${
                    selected
                      ? 'bg-[var(--accent-info)]/10 border-[var(--accent-info)]/30 text-[var(--accent-info)]'
                      : 'bg-[var(--bg-secondary)] border-subtle text-muted hover:text-[var(--text-primary)]'
                  }`}
                >
                  <span>{kind.displayName}</span>
                  <span className="font-mono text-[10px] opacity-60">{kind.id}</span>
                </button>
              );
            })}
          </div>
        </div>

        <div>
          <label className="block text-[10px] tracking-wider text-muted mb-1">
            WEBSOCKET URL
          </label>
          <input
            type="text"
            value={url}
            onChange={e => setUrl(e.target.value)}
            placeholder="ws://host:port"
            className="w-full bg-[var(--bg-secondary)] border border-subtle px-2 py-1 text-sm focus:outline-none focus:border-[var(--accent-info)]"
            autoFocus
          />
        </div>

        <div>
          <label className="block text-[10px] tracking-wider text-muted mb-1">
            BEARER TOKEN (OPTIONAL)
          </label>
          <input
            type="password"
            value={token}
            onChange={e => setToken(e.target.value)}
            placeholder="Leave empty if not required"
            className="w-full bg-[var(--bg-secondary)] border border-subtle px-2 py-1 text-sm focus:outline-none focus:border-[var(--accent-info)]"
          />
        </div>

        <div className="flex justify-end gap-2 pt-2">
          <button
            onClick={onClose}
            className="px-3 py-1 text-xs bg-[var(--bg-secondary)] text-muted hover:text-[var(--text-primary)] transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleSubmit}
            disabled={!canSubmit}
            className="px-3 py-1 text-xs bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Add Node
          </button>
        </div>
      </div>
    </Modal>
  );
}
