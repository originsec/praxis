import type { NexusPlan } from '../api/types';

export type NexusMessageRole = 'user' | 'assistant' | 'system';

export interface NexusToolExecution {
  name: string;
  display: string;
  success: boolean;
  executing?: boolean;
  input?: string;
  result?: string;
}

export interface NexusMessage {
  id: string;
  role: NexusMessageRole;
  content: string;
  timestamp: Date;
  toolExecutions?: NexusToolExecution[];
}

export interface NexusTokenUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface NexusState {
  sessionActive: boolean;
  isStarting: boolean;
  messages: NexusMessage[];
  currentPlan: NexusPlan | null;
  isLoading: boolean;
  streamingContent: string;
  currentToolExecutions: NexusToolExecution[];
  tokenUsage: NexusTokenUsage | null;
}
