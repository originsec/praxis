import { Download } from 'lucide-react';
import { FloatingPanel } from './FloatingPanel';
import { ChainExecutionViewer } from '../chains/ChainExecutionViewer';
import { exportChainExecution, downloadTextFile } from '../../utils/export';
import type { ChainExecutionUpdate } from '../../api/types';

interface Props {
  execution: ChainExecutionUpdate;
  onClose: () => void;
}

export function ChainExecutionFloating({ execution, onClose }: Props) {
  const handleExport = () => {
    const content = exportChainExecution(execution);
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    downloadTextFile(content, `chain-${execution.chain_name}-${timestamp}.md`);
  };

  return (
    <FloatingPanel
      title={`Chain: ${execution.chain_name}`}
      onClose={onClose}
      defaultWidth={600}
      defaultHeight={500}
      headerActions={
        <button
          onClick={handleExport}
          className="p-1 text-muted hover:text-[var(--text-primary)] transition-colors"
          title="Export"
        >
          <Download size={11} />
        </button>
      }
    >
      <div className="flex-1 overflow-auto">
        <ChainExecutionViewer
          execution={execution}
          chain={null}
        />
      </div>
    </FloatingPanel>
  );
}
