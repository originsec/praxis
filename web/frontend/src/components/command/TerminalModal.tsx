import { useState } from 'react';
import { Play, Square, Terminal as TerminalIcon } from 'lucide-react';
import { FloatingPanel } from './FloatingPanel';
import { Terminal } from '../terminal/Terminal';
import { useApp } from '../../context/AppContext';
import type { NodeState } from '../../api/types';

interface TerminalModalProps {
  nodeId: string;
  node: NodeState;
  onClose: () => void;
}

export function TerminalModal({ nodeId, node, onClose }: TerminalModalProps) {
  const { sendCommand } = useApp();
  const [isCreating, setIsCreating] = useState(false);
  const terminalId = node.active_terminal_id ?? null;

  const handleCreate = async () => {
    if (terminalId) return;
    setIsCreating(true);
    try {
      await sendCommand(nodeId, { Terminal: 'Create' });
    } finally {
      setIsCreating(false);
    }
  };

  const handleClose = async () => {
    await sendCommand(nodeId, { Terminal: 'Close' });
    onClose();
  };

  return (
    <FloatingPanel
      title={`Terminal · ${node.machine_name || nodeId.slice(0, 8)}`}
      onClose={onClose}
      defaultWidth={640}
      defaultHeight={440}
      headerActions={
        terminalId ? (
          <button
            onClick={handleClose}
            className="p-1 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/10 transition-colors"
            title="Close terminal"
          >
            <Square size={11} />
          </button>
        ) : undefined
      }
    >
      {terminalId ? (
        <Terminal nodeId={nodeId} terminalId={terminalId} />
      ) : (
        <div className="flex items-center justify-center flex-1">
          <div className="text-center p-4">
            <TerminalIcon size={28} className="mx-auto mb-2 text-muted opacity-50" />
            <p className="text-muted text-[11px] mb-3">No terminal session</p>
            <button
              onClick={handleCreate}
              disabled={isCreating}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-[11px] bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors disabled:opacity-50"
            >
              <Play size={12} />
              {isCreating ? 'Creating...' : 'Start Terminal'}
            </button>
          </div>
        </div>
      )}
    </FloatingPanel>
  );
}
