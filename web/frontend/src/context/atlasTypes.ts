import type { AtlasPlan } from '../api/types';

export type AtlasMessageRole = 'user' | 'assistant' | 'system';

export interface AtlasToolExecution {
  name: string;
  display: string;
  success: boolean;
  executing?: boolean;
  input?: string;
  result?: string;
}

export interface AtlasMessage {
  id: string;
  role: AtlasMessageRole;
  content: string;
  timestamp: Date;
  toolExecutions?: AtlasToolExecution[];
}

export interface AtlasTokenUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface AtlasState {
  sessionActive: boolean;
  isStarting: boolean;
  messages: AtlasMessage[];
  currentPlan: AtlasPlan | null;
  isLoading: boolean;
  streamingContent: string;
  currentToolExecutions: AtlasToolExecution[];
  tokenUsage: AtlasTokenUsage | null;
}
