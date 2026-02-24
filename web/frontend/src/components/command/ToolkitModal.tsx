import { useEffect, useState } from 'react';
import { Code2, Eye, ShieldAlert } from 'lucide-react';
import { Modal } from '../common/Modal';
import { useApp } from '../../context/AppContext';
import type { ToolkitToolInfo } from '../../api/types';

//
// Lazy-import the tool-specific modals from the ToolkitPage. They are
// self-contained and already use Modal internally.
//

import { SessionHistoryPoisoningModal, MessageEncoderModal } from '../../pages/ToolkitPage';

function toolIcon(toolName: string) {
  if (toolName === 'session_history_poisoning') return ShieldAlert;
  if (toolName === 'message_encoder') return Code2;
  return Eye;
}

interface ToolkitModalProps {
  onClose: () => void;
}

export function ToolkitModal({ onClose }: ToolkitModalProps) {
  const { state, send } = useApp();
  const [activeTool, setActiveTool] = useState<string | null>(null);

  useEffect(() => {
    send({ type: 'toolkit_list' });
  }, [send]);

  const tools = state.toolkit.tools;

  const descriptionFor = (toolName: string) =>
    tools.find(t => t.tool_name === toolName)?.description ?? '';

  return (
    <>
      <Modal isOpen={true} onClose={onClose} title="Toolkit" size="lg">
        <div className="space-y-4">
          <p className="text-[10px] text-muted">Specialized offensive tools</p>

          {tools.length === 0 ? (
            <div className="p-8 text-center text-muted text-xs">
              No tools available
            </div>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2">
              {tools.map((tool: ToolkitToolInfo) => {
                const Icon = toolIcon(tool.tool_name);
                return (
                  <button
                    key={tool.tool_name}
                    onClick={() => setActiveTool(tool.tool_name)}
                    className="text-left border border-subtle bg-[var(--bg-secondary)] hover:bg-[var(--bg-tertiary)] transition-colors p-3 ascii-box"
                  >
                    <div className="flex items-center gap-2 mb-1.5">
                      <Icon size={15} className="text-[var(--accent-info)]" />
                      <h2 className="text-xs font-semibold text-highlight">{tool.display_name}</h2>
                    </div>
                    <p className="text-[10px] text-muted leading-relaxed">{tool.description}</p>
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </Modal>

      {activeTool === 'session_history_poisoning' && (
        <SessionHistoryPoisoningModal
          isOpen
          onClose={() => setActiveTool(null)}
          description={descriptionFor('session_history_poisoning')}
        />
      )}

      {activeTool === 'message_encoder' && (
        <MessageEncoderModal
          isOpen
          onClose={() => setActiveTool(null)}
          description={descriptionFor('message_encoder')}
        />
      )}
    </>
  );
}
