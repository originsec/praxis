import { Download } from 'lucide-react';
import { Modal } from './Modal';
import { ChainExecutionViewer } from '../chains/ChainExecutionViewer';
import { exportChainExecution, downloadTextFile } from '../../utils/export';
import type { ChainExecutionUpdate, ChainDefinitionFull } from '../../api/types';

interface ChainExecutionModalProps {
  execution: ChainExecutionUpdate | null;
  chain: ChainDefinitionFull | null;
  isLoading?: boolean;
  onClose: () => void;
  onEditChain?: (chainId: string) => void;
}

export function ChainExecutionModal({ execution, chain, isLoading, onClose, onEditChain }: ChainExecutionModalProps) {
  const handleExport = () => {
    if (!execution) return;
    const content = exportChainExecution(execution);
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    downloadTextFile(content, `chain-${execution.chain_name}-${timestamp}.md`);
  };

  return (
    <Modal
      isOpen={execution !== null}
      title={`Chain Execution: ${execution?.chain_name ?? ''}`}
      onClose={onClose}
      size="xl"
      noPadding
      headerActions={execution && (
        <button
          onClick={handleExport}
          className="p-1 hover:bg-[var(--bg-tertiary)] text-muted hover:text-[var(--text-primary)] transition-colors"
          title="Export as Markdown"
        >
          <Download size={20} />
        </button>
      )}
    >
      {execution && (
        <div className="h-[70vh] overflow-auto">
          <ChainExecutionViewer
            execution={execution}
            chain={chain}
            isLoading={isLoading}
            onEditChain={onEditChain}
          />
        </div>
      )}
    </Modal>
  );
}
