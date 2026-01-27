import type { SkynetPlan } from '../api/types';

export type SkynetMessageRole = 'user' | 'assistant' | 'system';

export interface SkynetToolExecution {
  name: string;
  display: string;
  success: boolean;
  executing?: boolean;
  input?: string;
  result?: string;
}

export interface SkynetMessage {
  id: string;
  role: SkynetMessageRole;
  content: string;
  timestamp: Date;
  toolExecutions?: SkynetToolExecution[];
}

export interface SkynetTokenUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface SkynetState {
  sessionActive: boolean;
  isStarting: boolean;
  messages: SkynetMessage[];
  currentPlan: SkynetPlan | null;
  isLoading: boolean;
  streamingContent: string;
  currentToolExecutions: SkynetToolExecution[];
  tokenUsage: SkynetTokenUsage | null;
}
