import type { OrchestratorPlan } from '../api/types';

export type OrchestratorMessageRole = 'user' | 'assistant' | 'system';

export interface OrchestratorToolExecution {
  name: string;
  display: string;
  success: boolean;
  executing?: boolean;
  input?: string;
  result?: string;
}

export interface OrchestratorMessage {
  id: string;
  role: OrchestratorMessageRole;
  content: string;
  timestamp: Date;
  toolExecutions?: OrchestratorToolExecution[];
}

export interface OrchestratorTokenUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface OrchestratorState {
  sessionActive: boolean;
  isStarting: boolean;
  provider: string | null;
  model: string | null;
  messages: OrchestratorMessage[];
  currentPlan: OrchestratorPlan | null;
  isLoading: boolean;
  streamingContent: string;
  currentToolExecutions: OrchestratorToolExecution[];
  tokenUsage: OrchestratorTokenUsage | null;
}
