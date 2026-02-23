import type { ExecutionTab } from '../../context/executionTypes';
import type {
  ChainDefinitionFull,
  ChainDefinitionInput,
  OperationDefinitionInfo,
  ChainExecutionUpdate,
  ChainExecutionEvent,
  NodeState,
  ToolkitToolInfo,
  PayloadInfo,
  BrowserMessage,
} from '../../api/types';
import { ChainBuilder } from '../chains/ChainBuilder';
import { ExecutionViewer } from './ExecutionViewer';

interface ModelDefinition {
  name: string;
  provider: string;
  model: string;
  apiKey: string;
}

interface ExecutionTabContentProps {
  tab: ExecutionTab;
  chain: ChainDefinitionFull | null;
  execution: ChainExecutionUpdate | null;
  executionEvents: ChainExecutionEvent[];
  operationDefs: OperationDefinitionInfo[];
  modelDefs: ModelDefinition[];
  nodes: NodeState[];
  toolkitTools: ToolkitToolInfo[];
  payloads: PayloadInfo[];
  send: (msg: BrowserMessage) => void;
  saveStatus?: string | null;
  saveError?: string | null;
  onSaveChain: (definition: ChainDefinitionInput) => void;
  onDuplicateChain?: (definition: ChainDefinitionInput) => void;
  onRunChain: () => void;
  onCancelExecution: () => void;
  onCreateOp: () => void;
  onClearExecution: () => void;
  externalChainUpdate: ChainDefinitionInput | null;
  onLocalChainChange?: (chain: ChainDefinitionInput | null) => void;
}

export function ExecutionTabContent({
  tab,
  chain,
  execution,
  executionEvents,
  operationDefs,
  modelDefs,
  nodes,
  toolkitTools,
  payloads,
  send,
  saveStatus,
  saveError,
  onSaveChain,
  onDuplicateChain,
  onRunChain,
  onCancelExecution,
  onCreateOp,
  onClearExecution,
  externalChainUpdate,
  onLocalChainChange,
}: ExecutionTabContentProps) {
  const isRunning = execution && (execution.status === 'Running' || execution.status === 'Queued');

  //
  // Show execution viewer if there's an active or completed execution.
  //
  if (execution) {
    return (
      <ExecutionViewer
        execution={execution}
        chain={chain}
        executionEvents={executionEvents}
        operationDefs={operationDefs}
        payloads={payloads}
        onCancel={isRunning ? onCancelExecution : undefined}
        onBack={!isRunning ? onClearExecution : undefined}
      />
    );
  }

  //
  // Use externalChainUpdate if provided, otherwise use the tab's localChain
  // as the initial chain to load (for orchestrator-created tabs).
  //
  const effectiveExternalUpdate = externalChainUpdate ?? (
    !chain && tab.localChain ? tab.localChain : null
  );

  return (
    <div className="h-full flex flex-col">
      <div className="flex-1 min-h-0">
        <ChainBuilder
          chain={chain}
          onSave={onSaveChain}
          onDuplicate={onDuplicateChain}
          onCancel={() => {}}
          operationDefs={operationDefs}
          modelDefs={modelDefs}
          nodes={nodes}
          toolkitTools={toolkitTools}
          payloads={payloads}
          send={send}
          saveStatus={saveStatus}
          saveError={saveError}
          externalChainUpdate={effectiveExternalUpdate}
          onLocalChainChange={onLocalChainChange}
        />
      </div>
    </div>
  );
}
