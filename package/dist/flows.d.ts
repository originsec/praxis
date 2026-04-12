import { a as PermissionMode, c as PromptInput, i as NonInteractivePermissionPolicy, n as AuthPolicy, r as McpServer$1 } from "./types-yxf-gcOE.js";
import { SetSessionConfigOptionResponse } from "@agentclientprotocol/sdk";

//#region src/runtime/engine/session-options.d.ts
type SessionAgentOptions = {
  model?: string;
  allowedTools?: string[];
  maxTurns?: number;
};
//#endregion
//#region src/flows/types.d.ts
type MaybePromise<T> = T | Promise<T>;
type FlowRunDefinition<TInput = unknown> = {
  title?: string | ((context: {
    input: TInput;
    flowName: string;
    flowPath?: string;
  }) => MaybePromise<string | undefined>);
};
type FlowNodeContext<TInput = unknown> = {
  input: TInput;
  outputs: Record<string, unknown>;
  results: Record<string, FlowNodeResult>;
  state: FlowRunState;
  services: Record<string, unknown>;
};
type FlowNodeCommon = {
  timeoutMs?: number;
  heartbeatMs?: number;
  statusDetail?: string;
};
type FlowEdge = {
  from: string;
  to: string;
} | {
  from: string;
  switch: {
    on: string;
    cases: Record<string, string>;
  };
};
type AcpNodeDefinition = FlowNodeCommon & {
  nodeType: "acp";
  profile?: string;
  cwd?: string | ((context: FlowNodeContext) => MaybePromise<string | undefined>);
  session?: {
    handle?: string;
    isolated?: boolean;
  };
  prompt: (context: FlowNodeContext) => MaybePromise<PromptInput | string>;
  parse?: (text: string, context: FlowNodeContext) => MaybePromise<unknown>;
};
type ComputeNodeDefinition = FlowNodeCommon & {
  nodeType: "compute";
  run: (context: FlowNodeContext) => MaybePromise<unknown>;
};
type FunctionActionNodeDefinition = FlowNodeCommon & {
  nodeType: "action";
  run: (context: FlowNodeContext) => MaybePromise<unknown>;
};
type ShellActionExecution = {
  command: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  stdin?: string;
  shell?: boolean | string;
  allowNonZeroExit?: boolean;
  timeoutMs?: number;
};
type ShellActionResult = {
  command: string;
  args: string[];
  cwd: string;
  stdout: string;
  stderr: string;
  combinedOutput: string;
  exitCode: number | null;
  signal: NodeJS.Signals | null;
  durationMs: number;
};
type ShellActionNodeDefinition = FlowNodeCommon & {
  nodeType: "action";
  exec: (context: FlowNodeContext) => MaybePromise<ShellActionExecution>;
  parse?: (result: ShellActionResult, context: FlowNodeContext) => MaybePromise<unknown>;
};
type ActionNodeDefinition = FunctionActionNodeDefinition | ShellActionNodeDefinition;
type CheckpointNodeDefinition = FlowNodeCommon & {
  nodeType: "checkpoint";
  summary?: string;
  run?: (context: FlowNodeContext) => MaybePromise<unknown>;
};
type FlowNodeDefinition = AcpNodeDefinition | ComputeNodeDefinition | ActionNodeDefinition | CheckpointNodeDefinition;
type FlowPermissionRequirements = {
  requiredMode: PermissionMode;
  requireExplicitGrant?: boolean;
  reason?: string;
};
type FlowDefinition = {
  name: string;
  run?: FlowRunDefinition;
  permissions?: FlowPermissionRequirements;
  startAt: string;
  nodes: Record<string, FlowNodeDefinition>;
  edges: FlowEdge[];
};
type FlowNodeOutcome = "ok" | "timed_out" | "failed" | "cancelled";
type FlowNodeResult = {
  attemptId: string;
  nodeId: string;
  nodeType: FlowNodeDefinition["nodeType"];
  outcome: FlowNodeOutcome;
  startedAt: string;
  finishedAt: string;
  durationMs: number;
  output?: unknown;
  error?: string;
};
type FlowArtifactRef = {
  path: string;
  mediaType: string;
  bytes: number;
  sha256: string;
};
type FlowConversationTrace = {
  sessionId: string;
  messageStart: number;
  messageEnd: number;
  eventStartSeq: number;
  eventEndSeq: number;
};
type FlowActionReceipt = {
  actionType: "shell" | "function";
  command?: string;
  args?: string[];
  cwd?: string;
  exitCode?: number | null;
  signal?: NodeJS.Signals | null;
  durationMs?: number;
};
type FlowStepTrace = {
  sessionId?: string;
  promptArtifact?: FlowArtifactRef;
  rawResponseArtifact?: FlowArtifactRef;
  outputArtifact?: FlowArtifactRef;
  outputInline?: unknown;
  stdoutArtifact?: FlowArtifactRef;
  stderrArtifact?: FlowArtifactRef;
  conversation?: FlowConversationTrace;
  action?: FlowActionReceipt;
};
type FlowStepRecord = {
  attemptId: string;
  nodeId: string;
  nodeType: FlowNodeDefinition["nodeType"];
  outcome: FlowNodeOutcome;
  startedAt: string;
  finishedAt: string;
  promptText: string | null;
  rawText: string | null;
  output: unknown;
  error?: string;
  session: FlowSessionBinding | null;
  agent: {
    agentName: string;
    agentCommand: string;
    cwd: string;
  } | null;
  trace?: FlowStepTrace;
};
type FlowSessionBinding = {
  key: string;
  handle: string;
  bundleId: string;
  name: string;
  profile?: string;
  agentName: string;
  agentCommand: string;
  cwd: string;
  acpxRecordId: string;
  acpSessionId: string;
  agentSessionId?: string;
};
type FlowRunState = {
  runId: string;
  flowName: string;
  runTitle?: string;
  flowPath?: string;
  startedAt: string;
  finishedAt?: string;
  updatedAt: string;
  status: "running" | "waiting" | "completed" | "failed" | "timed_out";
  input: unknown;
  outputs: Record<string, unknown>;
  results: Record<string, FlowNodeResult>;
  steps: FlowStepRecord[];
  sessionBindings: Record<string, FlowSessionBinding>;
  currentNode?: string;
  currentAttemptId?: string;
  currentNodeType?: FlowNodeDefinition["nodeType"];
  currentNodeStartedAt?: string;
  lastHeartbeatAt?: string;
  statusDetail?: string;
  waitingOn?: string;
  error?: string;
};
type FlowRunResult = {
  runDir: string;
  state: FlowRunState;
};
type ResolvedFlowAgent = {
  agentName: string;
  agentCommand: string;
  cwd: string;
};
type FlowRunnerOptions = {
  resolveAgent: (profile?: string) => ResolvedFlowAgent;
  permissionMode: PermissionMode;
  mcpServers?: McpServer$1[];
  nonInteractivePermissions?: NonInteractivePermissionPolicy;
  authCredentials?: Record<string, string>;
  authPolicy?: AuthPolicy;
  timeoutMs?: number;
  defaultNodeTimeoutMs?: number;
  ttlMs?: number;
  verbose?: boolean;
  suppressSdkConsoleErrors?: boolean;
  sessionOptions?: SessionAgentOptions;
  services?: Record<string, unknown>;
  outputRoot?: string;
};
//#endregion
//#region src/flows/definition.d.ts
declare function defineFlow<TFlow extends FlowDefinition>(definition: TFlow): TFlow;
declare function acp(definition: Omit<AcpNodeDefinition, "nodeType">): AcpNodeDefinition;
declare function compute(definition: Omit<ComputeNodeDefinition, "nodeType">): ComputeNodeDefinition;
declare function action(definition: Omit<FunctionActionNodeDefinition, "nodeType">): FunctionActionNodeDefinition;
declare function action(definition: Omit<ShellActionNodeDefinition, "nodeType">): ShellActionNodeDefinition;
declare function shell(definition: Omit<ShellActionNodeDefinition, "nodeType">): ShellActionNodeDefinition;
declare function checkpoint(definition?: Omit<CheckpointNodeDefinition, "nodeType">): CheckpointNodeDefinition;
//#endregion
//#region src/flows/runtime.d.ts
declare class FlowRunner {
  private readonly resolveAgent;
  private readonly defaultCwd;
  private readonly permissionMode;
  private readonly mcpServers?;
  private readonly nonInteractivePermissions?;
  private readonly authCredentials?;
  private readonly authPolicy?;
  private readonly timeoutMs?;
  private readonly defaultNodeTimeoutMs;
  private readonly verbose?;
  private readonly suppressSdkConsoleErrors?;
  private readonly sessionOptions?;
  private readonly services;
  private readonly store;
  private readonly pendingPersistentSessionClients;
  constructor(options: FlowRunnerOptions);
  run(flow: FlowDefinition, input: unknown, options?: {
    flowPath?: string;
  }): Promise<FlowRunResult>;
  private executeNode;
  private executeComputeNode;
  private executeActionNode;
  private executeCheckpointNode;
  private executeAcpNode;
  private runWithHeartbeat;
  private ensureSessionBinding;
  private refreshSessionBinding;
  private runPersistentPrompt;
  private closePendingPersistentSessionClients;
  private runIsolatedPrompt;
}
//#endregion
//#region src/flows/store.d.ts
declare function flowRunsBaseDir(homeDir?: string): string;
//#endregion
//#region src/flows/json.d.ts
type JsonObjectParseMode = "strict" | "fenced" | "compat";
declare function parseJsonObject(text: string, options?: {
  mode?: JsonObjectParseMode;
}): unknown;
declare function parseStrictJsonObject(text: string): unknown;
declare function extractJsonObject(text: string): unknown;
//#endregion
export { type AcpNodeDefinition, type ActionNodeDefinition, type CheckpointNodeDefinition, type ComputeNodeDefinition, type FlowDefinition, type FlowEdge, type FlowNodeCommon, type FlowNodeContext, type FlowNodeDefinition, type FlowPermissionRequirements, type FlowRunDefinition, type FlowRunResult, type FlowRunState, FlowRunner, type FlowRunnerOptions, type FlowSessionBinding, type FlowStepRecord, type FunctionActionNodeDefinition, type JsonObjectParseMode, type ResolvedFlowAgent, type ShellActionExecution, type ShellActionNodeDefinition, type ShellActionResult, acp, action, checkpoint, compute, defineFlow, extractJsonObject, flowRunsBaseDir, parseJsonObject, parseStrictJsonObject, shell };
//# sourceMappingURL=flows.d.ts.map