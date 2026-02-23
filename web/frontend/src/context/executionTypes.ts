import type { OrchestratorPlan } from '../api/types';
import type { OrchestratorMessage, OrchestratorToolExecution, OrchestratorTokenUsage } from './orchestratorTypes';

export type ChainOrchMode = 'build' | 'execute';

export interface ChainOrchestratorState {
  sessionActive: boolean;
  isStarting: boolean;
  provider: string | null;
  model: string | null;
  mode: ChainOrchMode;
  messages: OrchestratorMessage[];
  currentPlan: OrchestratorPlan | null;
  isLoading: boolean;
  streamingContent: string;
  currentToolExecutions: OrchestratorToolExecution[];
  tokenUsage: OrchestratorTokenUsage | null;
  currentPromptId: string | null;
}

export interface ExecutionTab {
  id: string;
  name: string;
  chainId: string | null;
  localChain: import('../api/types').ChainDefinitionInput | null;
  executionId: string | null;
  executionEvents: ChainExecutionEvent[];
  isDirty: boolean;
}

export interface ChainExecutionEvent {
  execution_id: string;
  timestamp: string;
  kind: ChainExecutionEventKind;
}

export type ChainExecutionEventKind =
  | { type: 'ElementStarted'; element_id: string; element_type: string; element_label: string }
  | { type: 'ElementCompleted'; element_id: string; output_preview: string }
  | { type: 'ElementFailed'; element_id: string; error: string }
  | { type: 'PromptSent'; element_id: string; prompt_preview: string }
  | { type: 'ResponseReceived'; element_id: string; response_preview: string }
  | { type: 'ToolCallStarted'; element_id: string; tool_name: string; input_preview: string }
  | { type: 'ToolCallCompleted'; element_id: string; tool_name: string; success: boolean; result_preview: string }
  | { type: 'AgentIteration'; element_id: string; iteration: number; total: number }
  | { type: 'LlmCallStarted'; element_id: string; model: string }
  | { type: 'LlmCallCompleted'; element_id: string; tokens_used: number }
  | { type: 'SessionCreated'; element_id: string; session_id: string }
  | { type: 'SessionClosed'; session_id: string }
  | { type: 'OutputChunk'; element_id: string; chunk: string };
