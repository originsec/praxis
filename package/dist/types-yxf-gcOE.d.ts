import { AgentCapabilities, AnyMessage, ContentBlock, McpServer, McpServer as McpServer$1, SessionConfigOption, SessionNotification, SetSessionConfigOptionResponse } from "@agentclientprotocol/sdk";

//#region src/prompt-content.d.ts
type PromptInput = ContentBlock[];
//#endregion
//#region src/types.d.ts
declare const PERMISSION_MODES: readonly ["approve-all", "approve-reads", "deny-all"];
type PermissionMode = (typeof PERMISSION_MODES)[number];
declare const AUTH_POLICIES: readonly ["skip", "fail"];
type AuthPolicy = (typeof AUTH_POLICIES)[number];
declare const NON_INTERACTIVE_PERMISSION_POLICIES: readonly ["deny", "fail"];
type NonInteractivePermissionPolicy = (typeof NON_INTERACTIVE_PERMISSION_POLICIES)[number];
type AcpJsonRpcMessage = AnyMessage;
type AcpMessageDirection = "outbound" | "inbound";
type PermissionStats = {
  requested: number;
  approved: number;
  denied: number;
  cancelled: number;
};
type ClientOperationMethod = "fs/read_text_file" | "fs/write_text_file" | "terminal/create" | "terminal/output" | "terminal/wait_for_exit" | "terminal/kill" | "terminal/release";
type ClientOperationStatus = "running" | "completed" | "failed";
type ClientOperation = {
  method: ClientOperationMethod;
  status: ClientOperationStatus;
  summary: string;
  details?: string;
  timestamp: string;
};
type SessionEventLog = {
  active_path: string;
  segment_count: number;
  max_segment_bytes: number;
  max_segments: number;
  last_write_at?: string;
  last_write_error?: string | null;
};
type AcpClientOptions = {
  agentCommand: string;
  cwd: string;
  mcpServers?: McpServer[];
  permissionMode: PermissionMode;
  nonInteractivePermissions?: NonInteractivePermissionPolicy;
  authCredentials?: Record<string, string>;
  authPolicy?: AuthPolicy;
  suppressSdkConsoleErrors?: boolean;
  verbose?: boolean;
  sessionOptions?: {
    model?: string;
    allowedTools?: string[];
    maxTurns?: number;
  };
  onAcpMessage?: (direction: AcpMessageDirection, message: AcpJsonRpcMessage) => void;
  onAcpOutputMessage?: (direction: AcpMessageDirection, message: AcpJsonRpcMessage) => void;
  onSessionUpdate?: (notification: SessionNotification) => void;
  onClientOperation?: (operation: ClientOperation) => void;
};
declare const SESSION_RECORD_SCHEMA: "acpx.session.v1";
type SessionMessageImage = {
  source: string;
  size?: {
    width: number;
    height: number;
  } | null;
};
type SessionUserContent = {
  Text: string;
} | {
  Mention: {
    uri: string;
    content: string;
  };
} | {
  Image: SessionMessageImage;
};
type SessionToolUse = {
  id: string;
  name: string;
  raw_input: string;
  input: unknown;
  is_input_complete: boolean;
  thought_signature?: string | null;
};
type SessionToolResultContent = {
  Text: string;
} | {
  Image: SessionMessageImage;
};
type SessionToolResult = {
  tool_use_id: string;
  tool_name: string;
  is_error: boolean;
  content: SessionToolResultContent;
  output?: unknown;
};
type SessionAgentContent = {
  Text: string;
} | {
  Thinking: {
    text: string;
    signature?: string | null;
  };
} | {
  RedactedThinking: string;
} | {
  ToolUse: SessionToolUse;
};
type SessionUserMessage = {
  id: string;
  content: SessionUserContent[];
};
type SessionAgentMessage = {
  content: SessionAgentContent[];
  tool_results: Record<string, SessionToolResult>;
  reasoning_details?: unknown;
};
type SessionMessage = {
  User: SessionUserMessage;
} | {
  Agent: SessionAgentMessage;
} | "Resume";
type SessionTokenUsage = {
  input_tokens?: number;
  output_tokens?: number;
  cache_creation_input_tokens?: number;
  cache_read_input_tokens?: number;
};
type SessionAcpxState = {
  reset_on_next_ensure?: boolean;
  current_mode_id?: string;
  desired_mode_id?: string;
  current_model_id?: string;
  available_models?: string[];
  available_commands?: string[];
  config_options?: SessionConfigOption[];
  session_options?: {
    model?: string;
    allowed_tools?: string[];
    max_turns?: number;
  };
};
type SessionRecord = {
  schema: typeof SESSION_RECORD_SCHEMA;
  acpxRecordId: string;
  acpSessionId: string;
  agentSessionId?: string;
  agentCommand: string;
  cwd: string;
  name?: string;
  createdAt: string;
  lastUsedAt: string;
  lastSeq: number;
  lastRequestId?: string;
  eventLog: SessionEventLog;
  closed?: boolean;
  closedAt?: string;
  pid?: number;
  agentStartedAt?: string;
  lastPromptAt?: string;
  lastAgentExitCode?: number | null;
  lastAgentExitSignal?: NodeJS.Signals | null;
  lastAgentExitAt?: string;
  lastAgentDisconnectReason?: string;
  protocolVersion?: number;
  agentCapabilities?: AgentCapabilities;
  title?: string | null;
  messages: SessionMessage[];
  updated_at: string;
  cumulative_token_usage: SessionTokenUsage;
  request_token_usage: Record<string, SessionTokenUsage>;
  acpx?: SessionAcpxState;
};
//#endregion
export { PermissionMode as a, PromptInput as c, NonInteractivePermissionPolicy as i, AuthPolicy as n, PermissionStats as o, McpServer$1 as r, SessionRecord as s, AcpClientOptions as t };
//# sourceMappingURL=types-yxf-gcOE.d.ts.map