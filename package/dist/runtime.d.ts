import { a as PermissionMode, i as NonInteractivePermissionPolicy, r as McpServer, s as SessionRecord } from "./types-yxf-gcOE.js";
import { t as AcpClient } from "./client-D-4_aZf2.js";
import fs from "node:fs";

//#region src/runtime/public/contract.d.ts
type AcpRuntimePromptMode = "prompt" | "steer";
type AcpRuntimeSessionMode = "persistent" | "oneshot";
type AcpSessionUpdateTag = "agent_message_chunk" | "agent_thought_chunk" | "tool_call" | "tool_call_update" | "usage_update" | "available_commands_update" | "current_mode_update" | "config_option_update" | "session_info_update" | "plan" | (string & {});
type AcpRuntimeControl = "session/set_mode" | "session/set_config_option" | "session/status";
type AcpRuntimeHandle = {
  sessionKey: string;
  backend: string;
  runtimeSessionName: string;
  cwd?: string;
  acpxRecordId?: string;
  backendSessionId?: string;
  agentSessionId?: string;
};
type AcpRuntimeEnsureInput = {
  sessionKey: string;
  agent: string;
  mode: AcpRuntimeSessionMode;
  resumeSessionId?: string;
  cwd?: string;
};
type AcpRuntimeTurnAttachment = {
  mediaType: string;
  data: string;
};
type AcpRuntimeTurnInput = {
  handle: AcpRuntimeHandle;
  text: string;
  attachments?: AcpRuntimeTurnAttachment[];
  mode: AcpRuntimePromptMode;
  requestId: string;
  timeoutMs?: number;
  signal?: AbortSignal;
};
type AcpRuntimeCapabilities = {
  controls: AcpRuntimeControl[];
  configOptionKeys?: string[];
};
type AcpRuntimeStatus = {
  summary?: string;
  acpxRecordId?: string;
  backendSessionId?: string;
  agentSessionId?: string;
  details?: Record<string, unknown>;
};
type AcpRuntimeDoctorReport = {
  ok: boolean;
  code?: string;
  message: string;
  installCommand?: string;
  details?: string[];
};
type AcpRuntimeEvent = {
  type: "text_delta";
  text: string;
  stream?: "output" | "thought";
  tag?: AcpSessionUpdateTag;
} | {
  type: "status";
  text: string;
  tag?: AcpSessionUpdateTag;
  used?: number;
  size?: number;
} | {
  type: "tool_call";
  text: string;
  tag?: AcpSessionUpdateTag;
  toolCallId?: string;
  status?: string;
  title?: string;
} | {
  type: "done";
  stopReason?: string;
} | {
  type: "error";
  message: string;
  code?: string;
  retryable?: boolean;
};
interface AcpRuntime {
  ensureSession(input: AcpRuntimeEnsureInput): Promise<AcpRuntimeHandle>;
  runTurn(input: AcpRuntimeTurnInput): AsyncIterable<AcpRuntimeEvent>;
  getCapabilities?(input: {
    handle?: AcpRuntimeHandle;
  }): Promise<AcpRuntimeCapabilities> | AcpRuntimeCapabilities;
  getStatus?(input: {
    handle: AcpRuntimeHandle;
    signal?: AbortSignal;
  }): Promise<AcpRuntimeStatus>;
  setMode?(input: {
    handle: AcpRuntimeHandle;
    mode: string;
  }): Promise<void>;
  setConfigOption?(input: {
    handle: AcpRuntimeHandle;
    key: string;
    value: string;
  }): Promise<void>;
  doctor?(): Promise<AcpRuntimeDoctorReport>;
  cancel(input: {
    handle: AcpRuntimeHandle;
    reason?: string;
  }): Promise<void>;
  close(input: {
    handle: AcpRuntimeHandle;
    reason: string;
    discardPersistentState?: boolean;
  }): Promise<void>;
}
type AcpSessionRecord = SessionRecord;
interface AcpSessionStore {
  load(sessionId: string): Promise<AcpSessionRecord | undefined>;
  save(record: AcpSessionRecord): Promise<void>;
}
interface AcpAgentRegistry {
  resolve(agentName: string): string;
  list(): string[];
}
type AcpRuntimeOptions = {
  cwd: string;
  sessionStore: AcpSessionStore;
  agentRegistry: AcpAgentRegistry;
  mcpServers?: McpServer[];
  permissionMode: PermissionMode;
  nonInteractivePermissions?: NonInteractivePermissionPolicy;
  timeoutMs?: number;
  probeAgent?: string;
  verbose?: boolean;
};
type AcpFileSessionStoreOptions = {
  stateDir: string;
};
//#endregion
//#region src/agent-registry.d.ts
declare const DEFAULT_AGENT_NAME = "codex";
//#endregion
//#region src/runtime/engine/manager.d.ts
type AcpRuntimeManagerDeps = {
  clientFactory?: (options: ConstructorParameters<typeof AcpClient>[0]) => AcpClient;
};
declare class AcpRuntimeManager {
  private readonly options;
  private readonly deps;
  private readonly activeControllers;
  private readonly pendingPersistentClients;
  constructor(options: AcpRuntimeOptions, deps?: AcpRuntimeManagerDeps);
  private createClient;
  ensureSession(input: {
    sessionKey: string;
    agent: string;
    mode: "persistent" | "oneshot";
    cwd?: string;
    resumeSessionId?: string;
  }): Promise<SessionRecord>;
  runTurn(input: {
    handle: AcpRuntimeHandle;
    text: string;
    attachments?: AcpRuntimeTurnAttachment[];
    mode: AcpRuntimePromptMode;
    sessionMode: "persistent" | "oneshot";
    requestId: string;
    timeoutMs?: number;
    signal?: AbortSignal;
  }): AsyncIterable<AcpRuntimeEvent>;
  getStatus(handle: AcpRuntimeHandle): Promise<AcpRuntimeStatus>;
  setMode(handle: AcpRuntimeHandle, mode: string, sessionMode?: "persistent" | "oneshot"): Promise<void>;
  setConfigOption(handle: AcpRuntimeHandle, key: string, value: string, sessionMode?: "persistent" | "oneshot"): Promise<void>;
  cancel(handle: AcpRuntimeHandle): Promise<void>;
  close(handle: AcpRuntimeHandle, options?: {
    discardPersistentState?: boolean;
  }): Promise<void>;
  private closeBackendSession;
  private requireRecord;
}
//#endregion
//#region src/runtime/public/file-session-store.d.ts
declare function createFileSessionStore(options: AcpFileSessionStoreOptions): AcpSessionStore;
//#endregion
//#region src/runtime/public/errors.d.ts
declare const ACP_ERROR_CODES: readonly ["ACP_BACKEND_MISSING", "ACP_BACKEND_UNAVAILABLE", "ACP_BACKEND_UNSUPPORTED_CONTROL", "ACP_DISPATCH_DISABLED", "ACP_INVALID_RUNTIME_OPTION", "ACP_SESSION_INIT_FAILED", "ACP_TURN_FAILED"];
type AcpRuntimeErrorCode = (typeof ACP_ERROR_CODES)[number];
declare class AcpRuntimeError extends Error {
  readonly code: AcpRuntimeErrorCode;
  readonly cause?: unknown;
  constructor(code: AcpRuntimeErrorCode, message: string, options?: {
    cause?: unknown;
  });
}
declare function isAcpRuntimeError(value: unknown): value is AcpRuntimeError;
//#endregion
//#region src/runtime/public/shared.d.ts
type AcpxHandleState = {
  name: string;
  agent: string;
  cwd: string;
  mode: "persistent" | "oneshot";
  acpxRecordId?: string;
  backendSessionId?: string;
  agentSessionId?: string;
};
//#endregion
//#region src/runtime/public/handle-state.d.ts
declare function encodeAcpxRuntimeHandleState(state: AcpxHandleState): string;
declare function decodeAcpxRuntimeHandleState(runtimeSessionName: string): AcpxHandleState | null;
//#endregion
//#region src/runtime.d.ts
declare const ACPX_BACKEND_ID = "acpx";
type AcpxRuntimeLike = AcpRuntime & {
  probeAvailability(): Promise<void>;
  isHealthy(): boolean;
  doctor(): Promise<AcpRuntimeDoctorReport>;
};
declare function createAgentRegistry(params?: {
  overrides?: Record<string, string>;
}): AcpAgentRegistry;
declare class AcpxRuntime implements AcpxRuntimeLike {
  private readonly options;
  private readonly testOptions?;
  private healthy;
  private manager;
  private managerPromise;
  constructor(options: AcpRuntimeOptions, testOptions?: {
    managerFactory?: (options: AcpRuntimeOptions) => AcpRuntimeManager;
    probeRunner?: (options: AcpRuntimeOptions) => Promise<{
      ok: boolean;
      message: string;
      details?: string[];
    }>;
  } | undefined);
  isHealthy(): boolean;
  probeAvailability(): Promise<void>;
  doctor(): Promise<AcpRuntimeDoctorReport>;
  ensureSession(input: AcpRuntimeEnsureInput): Promise<AcpRuntimeHandle>;
  runTurn(input: AcpRuntimeTurnInput): AsyncIterable<AcpRuntimeEvent>;
  getCapabilities(): AcpRuntimeCapabilities;
  getStatus(input: {
    handle: AcpRuntimeHandle;
    signal?: AbortSignal;
  }): Promise<AcpRuntimeStatus>;
  setMode(input: {
    handle: AcpRuntimeHandle;
    mode: string;
  }): Promise<void>;
  setConfigOption(input: {
    handle: AcpRuntimeHandle;
    key: string;
    value: string;
  }): Promise<void>;
  cancel(input: {
    handle: AcpRuntimeHandle;
    reason?: string;
  }): Promise<void>;
  close(input: {
    handle: AcpRuntimeHandle;
    reason: string;
    discardPersistentState?: boolean;
  }): Promise<void>;
  private getManager;
  private runProbe;
  private resolveHandleState;
}
declare function createAcpRuntime(options: AcpRuntimeOptions): AcpxRuntime;
declare function createRuntimeStore(options: {
  stateDir: string;
}): AcpSessionStore;
//#endregion
export { ACPX_BACKEND_ID, type AcpAgentRegistry, type AcpFileSessionStoreOptions, type AcpRuntime, type AcpRuntimeCapabilities, type AcpRuntimeDoctorReport, type AcpRuntimeEnsureInput, AcpRuntimeError, type AcpRuntimeErrorCode, type AcpRuntimeEvent, type AcpRuntimeHandle, type AcpRuntimeOptions, type AcpRuntimePromptMode, type AcpRuntimeSessionMode, type AcpRuntimeStatus, type AcpRuntimeTurnAttachment, type AcpRuntimeTurnInput, type AcpSessionRecord, type AcpSessionStore, type AcpSessionUpdateTag, AcpxRuntime, DEFAULT_AGENT_NAME, createAcpRuntime, createAgentRegistry, createFileSessionStore, createRuntimeStore, decodeAcpxRuntimeHandleState, encodeAcpxRuntimeHandleState, isAcpRuntimeError };
//# sourceMappingURL=runtime.d.ts.map