import { a as PermissionMode, c as PromptInput, i as NonInteractivePermissionPolicy, o as PermissionStats, t as AcpClientOptions } from "./types-yxf-gcOE.js";
import { InitializeResponse, PromptResponse, SessionModelState, SetSessionConfigOptionResponse } from "@agentclientprotocol/sdk";

//#region src/acp/client.d.ts
type LoadSessionOptions = {
  suppressReplayUpdates?: boolean;
  replayIdleMs?: number;
  replayDrainTimeoutMs?: number;
};
type SessionCreateResult = {
  sessionId: string;
  agentSessionId?: string;
  models?: SessionModelState;
};
type SessionLoadResult = {
  agentSessionId?: string;
  models?: SessionModelState;
};
type AgentDisconnectReason = "process_exit" | "process_close" | "pipe_close" | "connection_close";
type AgentExitInfo = {
  exitCode: number | null;
  signal: NodeJS.Signals | null;
  exitedAt: string;
  reason: AgentDisconnectReason;
  unexpectedDuringPrompt: boolean;
};
type AgentLifecycleSnapshot = {
  pid?: number;
  startedAt?: string;
  running: boolean;
  lastExit?: AgentExitInfo;
};
declare class AcpClient {
  private options;
  private connection?;
  private agent?;
  private initResult?;
  private loadedSessionId?;
  private eventHandlers;
  private readonly permissionStats;
  private readonly filesystem;
  private readonly terminalManager;
  private sessionUpdateChain;
  private observedSessionUpdates;
  private processedSessionUpdates;
  private suppressSessionUpdates;
  private suppressReplaySessionUpdateMessages;
  private activePrompt?;
  private readonly cancellingSessionIds;
  private closing;
  private agentStartedAt?;
  private lastAgentExit?;
  private lastKnownPid?;
  private readonly promptPermissionFailures;
  private readonly pendingConnectionRequests;
  constructor(options: AcpClientOptions);
  get initializeResult(): InitializeResponse | undefined;
  getAgentPid(): number | undefined;
  getPermissionStats(): PermissionStats;
  getAgentLifecycleSnapshot(): AgentLifecycleSnapshot;
  supportsLoadSession(): boolean;
  supportsCloseSession(): boolean;
  setEventHandlers(handlers: Pick<AcpClientOptions, "onAcpMessage" | "onAcpOutputMessage" | "onSessionUpdate" | "onClientOperation">): void;
  clearEventHandlers(): void;
  updateRuntimeOptions(options: {
    permissionMode?: PermissionMode;
    nonInteractivePermissions?: NonInteractivePermissionPolicy;
    suppressSdkConsoleErrors?: boolean;
    verbose?: boolean;
  }): void;
  hasReusableSession(sessionId: string): boolean;
  hasActivePrompt(sessionId?: string): boolean;
  start(): Promise<void>;
  private createTappedStream;
  createSession(cwd?: string): Promise<SessionCreateResult>;
  loadSession(sessionId: string, cwd?: string): Promise<SessionLoadResult>;
  loadSessionWithOptions(sessionId: string, cwd?: string, options?: LoadSessionOptions): Promise<SessionLoadResult>;
  prompt(sessionId: string, prompt: PromptInput | string): Promise<PromptResponse>;
  setSessionMode(sessionId: string, modeId: string): Promise<void>;
  setSessionConfigOption(sessionId: string, configId: string, value: string): Promise<SetSessionConfigOptionResponse>;
  setSessionModel(sessionId: string, modelId: string): Promise<void>;
  cancel(sessionId: string): Promise<void>;
  closeSession(sessionId: string): Promise<void>;
  requestCancelActivePrompt(): Promise<boolean>;
  cancelActivePrompt(waitMs?: number): Promise<PromptResponse | undefined>;
  close(): Promise<void>;
  private terminateAgentProcess;
  private detachAgentHandles;
  private getConnection;
  private log;
  private captureStartupStderr;
  private summarizeStartupStderr;
  private createStartupFailureWatcher;
  private selectAuthMethod;
  private authenticateIfRequired;
  private handlePermissionRequest;
  private attachAgentLifecycleObservers;
  private recordAgentExit;
  private notePromptPermissionFailure;
  private consumePromptPermissionFailure;
  private runConnectionRequest;
  private rejectPendingConnectionRequests;
  private handleReadTextFile;
  private handleWriteTextFile;
  private handleCreateTerminal;
  private handleTerminalOutput;
  private handleWaitForTerminalExit;
  private handleKillTerminal;
  private handleReleaseTerminal;
  private recordPermissionDecision;
  private recordPermissionError;
  private handleSessionUpdate;
  private waitForSessionUpdateDrain;
  waitForSessionUpdatesIdle(options?: {
    idleMs?: number;
    timeoutMs?: number;
  }): Promise<void>;
}
//#endregion
export { AcpClient as t };
//# sourceMappingURL=client-D-4_aZf2.d.ts.map