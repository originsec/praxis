import { B as PermissionPromptUnavailableError, C as isAcpResourceNotFoundError, F as AuthPolicyError, G as SessionNotFoundError, I as ClaudeAcpSessionCreateTimeoutError, K as SessionResolutionError, L as CopilotAcpUnsupportedError, M as AgentDisconnectedError, N as AgentSpawnError, P as AgentStartupError, R as GeminiAcpStartupTimeoutError, S as extractAcpError, U as SessionModeReplayError, W as SessionModelReplayError, g as textPrompt, i as measurePerf, j as SESSION_RECORD_SCHEMA, l as extractRuntimeSessionId, q as SessionResumeRequiredError, r as incrementPerfCounter, u as normalizeRuntimeSessionId, v as formatErrorMessage, y as isAcpQueryClosedBeforeResponseError, z as PermissionDeniedError } from "./perf-metrics-D0um6IR6.js";
import { r as isSessionUpdateNotification } from "./jsonrpc-DSxh2w5R.js";
import fs, { statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs$1 from "node:fs/promises";
import os from "node:os";
import { spawn } from "node:child_process";
import { Readable, Writable } from "node:stream";
import { ClientSideConnection, PROTOCOL_VERSION } from "@agentclientprotocol/sdk";
import readline from "node:readline/promises";
import { randomUUID } from "node:crypto";
//#region src/agent-registry.ts
const ACP_ADAPTER_PACKAGE_RANGES = {
	pi: "^0.0.22",
	codex: "^0.11.1",
	claude: "^0.25.0"
};
const AGENT_REGISTRY = {
	pi: `npx pi-acp@${ACP_ADAPTER_PACKAGE_RANGES.pi}`,
	openclaw: "openclaw acp",
	codex: `npx @zed-industries/codex-acp@${ACP_ADAPTER_PACKAGE_RANGES.codex}`,
	claude: `npx -y @agentclientprotocol/claude-agent-acp@${ACP_ADAPTER_PACKAGE_RANGES.claude}`,
	gemini: "gemini --acp",
	cursor: "cursor-agent acp",
	copilot: "copilot --acp --stdio",
	droid: "droid exec --output-format acp",
	iflow: "iflow --experimental-acp",
	kilocode: "npx -y @kilocode/cli acp",
	kimi: "kimi acp",
	kiro: "kiro-cli-chat acp",
	opencode: "npx -y opencode-ai acp",
	qoder: "qodercli --acp",
	qwen: "qwen --acp",
	trae: "traecli acp serve"
};
const BUILT_IN_AGENT_PACKAGES = {
	codex: {
		packageName: "@zed-industries/codex-acp",
		packageRange: ACP_ADAPTER_PACKAGE_RANGES.codex,
		preferredBinName: "codex-acp",
		fallbackCommand: AGENT_REGISTRY.codex,
		legacyFallbackCommands: []
	},
	claude: {
		packageName: "@agentclientprotocol/claude-agent-acp",
		packageRange: ACP_ADAPTER_PACKAGE_RANGES.claude,
		preferredBinName: "claude-agent-acp",
		fallbackCommand: AGENT_REGISTRY.claude,
		legacyFallbackCommands: [`npm exec @agentclientprotocol/claude-agent-acp@${ACP_ADAPTER_PACKAGE_RANGES.claude}`]
	}
};
const AGENT_ALIASES = {
	"factory-droid": "droid",
	factorydroid: "droid"
};
const DEFAULT_AGENT_NAME = "codex";
function normalizeAgentName$1(value) {
	return value.trim().toLowerCase();
}
function mergeAgentRegistry(overrides) {
	if (!overrides) return { ...AGENT_REGISTRY };
	const merged = { ...AGENT_REGISTRY };
	for (const [name, command] of Object.entries(overrides)) {
		const normalized = normalizeAgentName$1(name);
		if (!normalized || !command.trim()) continue;
		merged[normalized] = command.trim();
	}
	return merged;
}
function resolveAgentCommand(agentName, overrides) {
	const normalized = normalizeAgentName$1(agentName);
	const registry = mergeAgentRegistry(overrides);
	return registry[normalized] ?? registry[AGENT_ALIASES[normalized] ?? normalized] ?? agentName;
}
function findBuiltInAgentPackage(agentCommand) {
	const normalized = agentCommand.trim();
	return Object.values(BUILT_IN_AGENT_PACKAGES).find((spec) => spec.fallbackCommand === normalized || spec.legacyFallbackCommands?.includes(normalized));
}
function defaultResolvePackageRoot(packageName) {
	const segments = packageName.split("/");
	let cursor = path.dirname(fileURLToPath(import.meta.url));
	while (true) {
		const candidateRoot = path.join(cursor, "node_modules", ...segments);
		const manifestPath = path.join(candidateRoot, "package.json");
		if (fs.existsSync(manifestPath)) try {
			if (JSON.parse(fs.readFileSync(manifestPath, "utf8")).name === packageName) return candidateRoot;
		} catch {}
		const parent = path.dirname(cursor);
		if (parent === cursor) throw new Error(`Built-in agent package not found: ${packageName}`);
		cursor = parent;
	}
}
function resolvePackageBin(spec, manifest) {
	if (typeof manifest.bin === "string") return manifest.bin;
	if (!manifest.bin || typeof manifest.bin !== "object") return;
	return manifest.bin[spec.preferredBinName] ?? (Object.keys(manifest.bin).length === 1 ? Object.values(manifest.bin)[0] : void 0);
}
function defaultResolveNpmCliPath(execPath) {
	const candidate = path.resolve(path.dirname(execPath), "..", "lib", "node_modules", "npm", "bin", "npm-cli.js");
	if (!fs.existsSync(candidate)) throw new Error(`npm CLI not found for execPath: ${execPath}`);
	return candidate;
}
function resolveInstalledBuiltInAgentLaunch(agentCommand, options = {}) {
	const spec = findBuiltInAgentPackage(agentCommand);
	if (!spec) return;
	const readFileSync = options.readFileSync ?? fs.readFileSync;
	const existsSync = options.existsSync ?? fs.existsSync;
	const resolvePackageRoot = options.resolvePackageRoot ?? defaultResolvePackageRoot;
	try {
		const packageRoot = resolvePackageRoot(spec.packageName);
		const manifest = JSON.parse(readFileSync(path.join(packageRoot, "package.json"), "utf8"));
		if (manifest.name !== spec.packageName) return;
		const relativeBinPath = resolvePackageBin(spec, manifest);
		if (!relativeBinPath) return;
		const binPath = path.resolve(packageRoot, relativeBinPath);
		if (!existsSync(binPath)) return;
		return {
			source: "installed",
			command: process.execPath,
			args: [binPath],
			packageName: spec.packageName,
			packageRange: spec.packageRange,
			packageVersion: manifest.version,
			binPath
		};
	} catch {
		return;
	}
}
function resolvePackageExecBuiltInAgentLaunch(agentCommand, options = {}) {
	const spec = findBuiltInAgentPackage(agentCommand);
	if (!spec) return;
	const existsSync = options.existsSync ?? fs.existsSync;
	const execPath = options.execPath ?? process.execPath;
	const resolveNpmCliPath = options.resolveNpmCliPath ?? defaultResolveNpmCliPath;
	try {
		const npmCliPath = resolveNpmCliPath(execPath);
		if (!existsSync(npmCliPath)) return;
		return {
			source: "package-exec",
			command: execPath,
			args: [
				npmCliPath,
				"exec",
				"--yes",
				`--package=${spec.packageName}@${spec.packageRange}`,
				"--",
				spec.preferredBinName
			],
			packageName: spec.packageName,
			packageRange: spec.packageRange,
			npmCliPath
		};
	} catch {
		return;
	}
}
function resolveBuiltInAgentLaunch(agentCommand, options = {}) {
	return resolveInstalledBuiltInAgentLaunch(agentCommand, options) ?? resolvePackageExecBuiltInAgentLaunch(agentCommand, options);
}
function listBuiltInAgents(overrides) {
	return Object.keys(mergeAgentRegistry(overrides));
}
//#endregion
//#region src/async-control.ts
var TimeoutError = class extends Error {
	constructor(timeoutMs) {
		super(`Timed out after ${timeoutMs}ms`);
		this.name = "TimeoutError";
	}
};
var InterruptedError = class extends Error {
	constructor() {
		super("Interrupted");
		this.name = "InterruptedError";
	}
};
async function withTimeout(promise, timeoutMs) {
	if (timeoutMs == null || timeoutMs <= 0) return await promise;
	let timer;
	const timeoutPromise = new Promise((_resolve, reject) => {
		timer = setTimeout(() => {
			reject(new TimeoutError(timeoutMs));
		}, timeoutMs);
	});
	try {
		return await Promise.race([promise, timeoutPromise]);
	} finally {
		if (timer) clearTimeout(timer);
	}
}
async function withInterrupt(run, onInterrupt) {
	return await new Promise((resolve, reject) => {
		let settled = false;
		const finish = (cb) => {
			if (settled) return;
			settled = true;
			process.off("SIGINT", onSigint);
			process.off("SIGTERM", onSigterm);
			process.off("SIGHUP", onSighup);
			cb();
		};
		const rejectInterrupted = () => {
			onInterrupt().finally(() => {
				finish(() => reject(new InterruptedError()));
			});
		};
		const onSigint = () => {
			rejectInterrupted();
		};
		const onSigterm = () => {
			rejectInterrupted();
		};
		const onSighup = () => {
			rejectInterrupted();
		};
		process.once("SIGINT", onSigint);
		process.once("SIGTERM", onSigterm);
		process.once("SIGHUP", onSighup);
		run().then((result) => finish(() => resolve(result)), (error) => finish(() => reject(error)));
	});
}
//#endregion
//#region src/session/persistence/serialize.ts
function serializeSessionRecordForDisk(record) {
	const canonical = {
		...record,
		schema: SESSION_RECORD_SCHEMA
	};
	return {
		schema: canonical.schema,
		acpx_record_id: canonical.acpxRecordId,
		acp_session_id: canonical.acpSessionId,
		agent_session_id: normalizeRuntimeSessionId(canonical.agentSessionId),
		agent_command: canonical.agentCommand,
		cwd: canonical.cwd,
		name: canonical.name,
		created_at: canonical.createdAt,
		last_used_at: canonical.lastUsedAt,
		last_seq: canonical.lastSeq,
		last_request_id: canonical.lastRequestId,
		event_log: canonical.eventLog,
		closed: canonical.closed,
		closed_at: canonical.closedAt,
		pid: canonical.pid,
		agent_started_at: canonical.agentStartedAt,
		last_prompt_at: canonical.lastPromptAt,
		last_agent_exit_code: canonical.lastAgentExitCode,
		last_agent_exit_signal: canonical.lastAgentExitSignal,
		last_agent_exit_at: canonical.lastAgentExitAt,
		last_agent_disconnect_reason: canonical.lastAgentDisconnectReason,
		protocol_version: canonical.protocolVersion,
		agent_capabilities: canonical.agentCapabilities,
		title: canonical.title,
		messages: canonical.messages,
		updated_at: canonical.updated_at,
		cumulative_token_usage: canonical.cumulative_token_usage,
		request_token_usage: canonical.request_token_usage,
		acpx: canonical.acpx
	};
}
//#endregion
//#region src/persisted-key-policy.ts
const SNAKE_CASE_KEY = /^[a-z][a-z0-9_]*$/;
const ZED_TAG_KEYS = new Set([
	"User",
	"Agent",
	"Resume",
	"Text",
	"Mention",
	"Image",
	"Thinking",
	"RedactedThinking",
	"ToolUse"
]);
const MAP_OBJECT_PATHS = new Set(["request_token_usage", "messages.Agent.tool_results"]);
const OPAQUE_VALUE_PATHS = new Set([
	"agent_capabilities",
	"messages.Agent.content.ToolUse.input",
	"acpx.config_options"
]);
function isRecord(value) {
	return !!value && typeof value === "object" && !Array.isArray(value);
}
function joinPath(path) {
	return path.join(".");
}
function isAllowedKey(path, key) {
	if (ZED_TAG_KEYS.has(key)) return true;
	return false;
}
function shouldSkipKeyRule(path) {
	return MAP_OBJECT_PATHS.has(joinPath(path));
}
function shouldSkipDescend(path) {
	return OPAQUE_VALUE_PATHS.has(joinPath(path)) || isToolResultOutputPath(path);
}
function isToolResultOutputPath(path) {
	if (path.length < 5 || path[path.length - 1] !== "output") return false;
	const toolResultsIndex = path.lastIndexOf("tool_results");
	if (toolResultsIndex === -1 || toolResultsIndex + 2 !== path.length - 1) return false;
	return path.slice(0, toolResultsIndex + 1).join(".") === "messages.Agent.tool_results";
}
function collectViolations(value, path, violations) {
	if (Array.isArray(value)) {
		for (const entry of value) collectViolations(entry, path, violations);
		return;
	}
	if (!isRecord(value)) return;
	const skipKeyRule = shouldSkipKeyRule(path);
	for (const [key, child] of Object.entries(value)) {
		if (!skipKeyRule && !SNAKE_CASE_KEY.test(key) && !isAllowedKey(path, key)) violations.push(`${joinPath(path)}.${key}`.replace(/^\./, ""));
		const childPath = [...path, key];
		if (shouldSkipDescend(childPath)) continue;
		collectViolations(child, childPath, violations);
	}
}
function findPersistedKeyPolicyViolations(value) {
	const violations = [];
	collectViolations(value, [], violations);
	return violations;
}
function assertPersistedKeyPolicy(value) {
	const violations = findPersistedKeyPolicyViolations(value);
	if (violations.length === 0) return;
	throw new Error(`Persisted key policy violation (expected snake_case keys): ${violations.join(", ")}`);
}
//#endregion
//#region src/session/event-log.ts
const DEFAULT_EVENT_SEGMENT_MAX_BYTES = 64 * 1024 * 1024;
function sessionBaseDir$1() {
	return path.join(os.homedir(), ".acpx", "sessions");
}
function safeSessionId(sessionId) {
	return encodeURIComponent(sessionId);
}
function sessionEventActivePath(sessionId) {
	return path.join(sessionBaseDir$1(), `${safeSessionId(sessionId)}.stream.ndjson`);
}
function sessionEventSegmentPath(sessionId, segment) {
	return path.join(sessionBaseDir$1(), `${safeSessionId(sessionId)}.stream.${segment}.ndjson`);
}
function sessionEventLockPath(sessionId) {
	return path.join(sessionBaseDir$1(), `${safeSessionId(sessionId)}.stream.lock`);
}
function defaultSessionEventLog(sessionId) {
	return {
		active_path: sessionEventActivePath(sessionId),
		segment_count: 5,
		max_segment_bytes: DEFAULT_EVENT_SEGMENT_MAX_BYTES,
		max_segments: 5,
		last_write_at: void 0,
		last_write_error: null
	};
}
//#endregion
//#region src/session/persistence/parse.ts
function asRecord$3(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function hasOwn$1(source, key) {
	return Object.prototype.hasOwnProperty.call(source, key);
}
function isStringArray(value) {
	return Array.isArray(value) && value.every((entry) => typeof entry === "string");
}
function parseTokenUsage(raw) {
	if (raw === void 0 || raw === null) return;
	const record = asRecord$3(raw);
	if (!record) return null;
	const usage = {};
	for (const field of [
		"input_tokens",
		"output_tokens",
		"cache_creation_input_tokens",
		"cache_read_input_tokens"
	]) {
		const value = record[field];
		if (value === void 0) continue;
		if (typeof value !== "number" || !Number.isFinite(value) || value < 0) return null;
		usage[field] = value;
	}
	return usage;
}
function parseRequestTokenUsage(raw) {
	if (raw === void 0 || raw === null) return;
	const record = asRecord$3(raw);
	if (!record) return null;
	const usage = {};
	for (const [key, value] of Object.entries(record)) {
		const parsed = parseTokenUsage(value);
		if (parsed == null) return null;
		usage[key] = parsed;
	}
	return usage;
}
function isSessionMessageImage(raw) {
	const record = asRecord$3(raw);
	if (!record || typeof record.source !== "string") return false;
	if (record.size === void 0 || record.size === null) return true;
	const size = asRecord$3(record.size);
	return !!size && typeof size.width === "number" && Number.isFinite(size.width) && typeof size.height === "number" && Number.isFinite(size.height);
}
function isUserContent(raw) {
	const record = asRecord$3(raw);
	if (!record) return false;
	if (typeof record.Text === "string") return true;
	if (record.Mention !== void 0) {
		const mention = asRecord$3(record.Mention);
		return !!mention && typeof mention.uri === "string" && typeof mention.content === "string";
	}
	if (record.Image !== void 0) return isSessionMessageImage(record.Image);
	return false;
}
function isToolUse(raw) {
	const record = asRecord$3(raw);
	return !!record && typeof record.id === "string" && typeof record.name === "string" && typeof record.raw_input === "string" && hasOwn$1(record, "input") && typeof record.is_input_complete === "boolean" && (record.thought_signature === void 0 || record.thought_signature === null || typeof record.thought_signature === "string");
}
function isToolResultContent(raw) {
	const record = asRecord$3(raw);
	if (!record) return false;
	if (typeof record.Text === "string") return true;
	if (record.Image !== void 0) return isSessionMessageImage(record.Image);
	return false;
}
function isToolResult(raw) {
	const record = asRecord$3(raw);
	return !!record && typeof record.tool_use_id === "string" && typeof record.tool_name === "string" && typeof record.is_error === "boolean" && isToolResultContent(record.content);
}
function isAgentContent(raw) {
	const record = asRecord$3(raw);
	if (!record) return false;
	if (typeof record.Text === "string") return true;
	if (record.Thinking !== void 0) {
		const thinking = asRecord$3(record.Thinking);
		return !!thinking && typeof thinking.text === "string" && (thinking.signature === void 0 || thinking.signature === null || typeof thinking.signature === "string");
	}
	if (typeof record.RedactedThinking === "string") return true;
	if (record.ToolUse !== void 0) return isToolUse(record.ToolUse);
	return false;
}
function isUserMessage$1(raw) {
	const record = asRecord$3(raw);
	if (!record || record.User === void 0) return false;
	const user = asRecord$3(record.User);
	return !!user && typeof user.id === "string" && Array.isArray(user.content) && user.content.every((entry) => isUserContent(entry));
}
function isAgentMessage$1(raw) {
	const record = asRecord$3(raw);
	if (!record || record.Agent === void 0) return false;
	const agent = asRecord$3(record.Agent);
	if (!agent || !Array.isArray(agent.content) || !agent.content.every(isAgentContent)) return false;
	const toolResults = asRecord$3(agent.tool_results);
	if (!toolResults) return false;
	return Object.values(toolResults).every(isToolResult);
}
function isConversationMessage(raw) {
	return raw === "Resume" || isUserMessage$1(raw) || isAgentMessage$1(raw);
}
function parseConversationRecord(record) {
	if (!Array.isArray(record.messages) || !record.messages.every(isConversationMessage) || typeof record.updated_at !== "string") return;
	if (record.title !== void 0 && record.title !== null && typeof record.title !== "string") return;
	const cumulativeTokenUsage = parseTokenUsage(record.cumulative_token_usage);
	const requestTokenUsage = parseRequestTokenUsage(record.request_token_usage);
	if (cumulativeTokenUsage === null || requestTokenUsage === null) return;
	return {
		title: record.title === void 0 || record.title === null || typeof record.title === "string" ? record.title : null,
		messages: record.messages,
		updated_at: record.updated_at,
		cumulative_token_usage: cumulativeTokenUsage ?? {},
		request_token_usage: requestTokenUsage ?? {}
	};
}
function parseAcpxState(raw) {
	const record = asRecord$3(raw);
	if (!record) return;
	const state = {};
	if (record.reset_on_next_ensure === true) state.reset_on_next_ensure = true;
	if (typeof record.current_mode_id === "string") state.current_mode_id = record.current_mode_id;
	if (typeof record.desired_mode_id === "string") state.desired_mode_id = record.desired_mode_id;
	if (typeof record.current_model_id === "string") state.current_model_id = record.current_model_id;
	if (isStringArray(record.available_models)) state.available_models = [...record.available_models];
	if (isStringArray(record.available_commands)) state.available_commands = [...record.available_commands];
	if (Array.isArray(record.config_options)) state.config_options = record.config_options;
	const sessionOptions = asRecord$3(record.session_options);
	if (sessionOptions) {
		const parsedSessionOptions = {};
		if (typeof sessionOptions.model === "string") parsedSessionOptions.model = sessionOptions.model;
		if (isStringArray(sessionOptions.allowed_tools)) parsedSessionOptions.allowed_tools = [...sessionOptions.allowed_tools];
		if (typeof sessionOptions.max_turns === "number" && Number.isInteger(sessionOptions.max_turns) && sessionOptions.max_turns > 0) parsedSessionOptions.max_turns = sessionOptions.max_turns;
		if (Object.keys(parsedSessionOptions).length > 0) state.session_options = parsedSessionOptions;
	}
	return state;
}
function parseEventLog(raw, sessionId) {
	const record = asRecord$3(raw);
	if (!record) return defaultSessionEventLog(sessionId);
	if (typeof record.active_path !== "string" || typeof record.segment_count !== "number" || !Number.isInteger(record.segment_count) || record.segment_count < 1 || typeof record.max_segment_bytes !== "number" || !Number.isInteger(record.max_segment_bytes) || record.max_segment_bytes < 1 || typeof record.max_segments !== "number" || !Number.isInteger(record.max_segments) || record.max_segments < 1) return defaultSessionEventLog(sessionId);
	return {
		active_path: record.active_path,
		segment_count: record.segment_count,
		max_segment_bytes: record.max_segment_bytes,
		max_segments: record.max_segments,
		last_write_at: typeof record.last_write_at === "string" ? record.last_write_at : void 0,
		last_write_error: record.last_write_error == null || typeof record.last_write_error === "string" ? record.last_write_error : null
	};
}
function normalizeOptionalName(value) {
	if (value == null) return;
	if (typeof value !== "string") return null;
	const trimmed = value.trim();
	return trimmed.length > 0 ? trimmed : void 0;
}
function normalizeOptionalPid(value) {
	if (value == null) return;
	if (!Number.isInteger(value) || value <= 0) return null;
	return value;
}
function normalizeOptionalBoolean(value, fallback = false) {
	if (value == null) return fallback;
	return typeof value === "boolean" ? value : null;
}
function normalizeOptionalString(value) {
	if (value == null) return;
	return typeof value === "string" ? value : null;
}
function normalizeOptionalExitCode(value) {
	if (value === void 0) return;
	if (value === null) return null;
	if (Number.isInteger(value)) return value;
	return Symbol("invalid");
}
function normalizeOptionalSignal(value) {
	if (value === void 0) return;
	if (value === null) return null;
	if (typeof value === "string") return value;
	return Symbol("invalid");
}
function parseSessionRecord(raw) {
	const record = asRecord$3(raw);
	if (!record) return null;
	if (record.schema !== "acpx.session.v1") return null;
	const name = normalizeOptionalName(record.name);
	const pid = normalizeOptionalPid(record.pid);
	const closed = normalizeOptionalBoolean(record.closed, false);
	const closedAt = normalizeOptionalString(record.closed_at);
	const agentStartedAt = normalizeOptionalString(record.agent_started_at);
	const lastPromptAt = normalizeOptionalString(record.last_prompt_at);
	const lastAgentExitCode = normalizeOptionalExitCode(record.last_agent_exit_code);
	const lastAgentExitSignal = normalizeOptionalSignal(record.last_agent_exit_signal);
	const lastAgentExitAt = normalizeOptionalString(record.last_agent_exit_at);
	const lastAgentDisconnectReason = normalizeOptionalString(record.last_agent_disconnect_reason);
	if (typeof record.acpx_record_id !== "string" || typeof record.acp_session_id !== "string" || typeof record.agent_command !== "string" || typeof record.cwd !== "string" || typeof record.created_at !== "string" || typeof record.last_used_at !== "string" || typeof record.last_seq !== "number" || !Number.isInteger(record.last_seq) || record.last_seq < 0 || name === null || pid === null || closed === null || closedAt === null || agentStartedAt === null || lastPromptAt === null || typeof lastAgentExitCode === "symbol" || typeof lastAgentExitSignal === "symbol" || lastAgentExitAt === null || lastAgentDisconnectReason === null) return null;
	const conversation = parseConversationRecord(record);
	if (!conversation) return null;
	const eventLog = parseEventLog(record.event_log, record.acpx_record_id);
	const lastRequestId = normalizeOptionalString(record.last_request_id);
	if (lastRequestId === null) return null;
	return {
		schema: SESSION_RECORD_SCHEMA,
		acpxRecordId: record.acpx_record_id,
		acpSessionId: record.acp_session_id,
		agentSessionId: normalizeRuntimeSessionId(record.agent_session_id),
		agentCommand: record.agent_command,
		cwd: record.cwd,
		name,
		createdAt: record.created_at,
		lastUsedAt: record.last_used_at,
		lastSeq: record.last_seq,
		lastRequestId,
		eventLog,
		closed,
		closedAt,
		pid,
		agentStartedAt,
		lastPromptAt,
		lastAgentExitCode,
		lastAgentExitSignal,
		lastAgentExitAt,
		lastAgentDisconnectReason,
		protocolVersion: typeof record.protocol_version === "number" ? record.protocol_version : void 0,
		agentCapabilities: asRecord$3(record.agent_capabilities),
		title: conversation.title,
		messages: conversation.messages,
		updated_at: conversation.updated_at,
		cumulative_token_usage: conversation.cumulative_token_usage,
		request_token_usage: conversation.request_token_usage,
		acpx: parseAcpxState(record.acpx)
	};
}
//#endregion
//#region src/session/persistence/index.ts
const SESSION_INDEX_SCHEMA = "acpx.session-index.v1";
function asRecord$2(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function parseIndexEntry(raw) {
	const record = asRecord$2(raw);
	if (!record) return;
	if (typeof record.file !== "string" || typeof record.acpxRecordId !== "string" || typeof record.acpSessionId !== "string" || typeof record.agentCommand !== "string" || typeof record.cwd !== "string" || typeof record.lastUsedAt !== "string" || typeof record.closed !== "boolean") return;
	if (record.name !== void 0 && typeof record.name !== "string") return;
	return {
		file: record.file,
		acpxRecordId: record.acpxRecordId,
		acpSessionId: record.acpSessionId,
		agentCommand: record.agentCommand,
		cwd: record.cwd,
		name: record.name,
		closed: record.closed,
		lastUsedAt: record.lastUsedAt
	};
}
function sessionIndexPath(sessionDir) {
	return path.join(sessionDir, "index.json");
}
function toSessionIndexEntry(record, fileName) {
	return {
		file: fileName,
		acpxRecordId: record.acpxRecordId,
		acpSessionId: record.acpSessionId,
		agentCommand: record.agentCommand,
		cwd: record.cwd,
		name: record.name,
		closed: record.closed === true,
		lastUsedAt: record.lastUsedAt
	};
}
async function readSessionIndex(sessionDir) {
	const filePath = sessionIndexPath(sessionDir);
	try {
		const payload = await fs$1.readFile(filePath, "utf8");
		const record = asRecord$2(JSON.parse(payload));
		if (!record || record.schema !== SESSION_INDEX_SCHEMA || !Array.isArray(record.files)) return;
		const files = record.files.filter((entry) => typeof entry === "string");
		if (files.length !== record.files.length || !Array.isArray(record.entries)) return;
		const entries = record.entries.map((entry) => parseIndexEntry(entry)).filter((entry) => Boolean(entry));
		if (entries.length !== record.entries.length) return;
		return {
			schema: SESSION_INDEX_SCHEMA,
			files,
			entries
		};
	} catch {
		return;
	}
}
async function writeSessionIndex(sessionDir, index) {
	const filePath = sessionIndexPath(sessionDir);
	const tempFile = `${filePath}.${process.pid}.${Date.now()}.tmp`;
	const payload = JSON.stringify({
		schema: SESSION_INDEX_SCHEMA,
		files: [...index.files].toSorted(),
		entries: [...index.entries].toSorted((a, b) => b.lastUsedAt.localeCompare(a.lastUsedAt))
	}, null, 2);
	await fs$1.writeFile(tempFile, `${payload}\n`, "utf8");
	await fs$1.rename(tempFile, filePath);
}
async function rebuildSessionIndex(sessionDir) {
	const files = (await fs$1.readdir(sessionDir, { withFileTypes: true })).filter((entry) => entry.isFile() && entry.name.endsWith(".json") && entry.name !== "index.json").map((entry) => entry.name).toSorted();
	const indexEntries = [];
	for (const file of files) try {
		const payload = await fs$1.readFile(path.join(sessionDir, file), "utf8");
		const parsed = parseSessionRecord(JSON.parse(payload));
		if (!parsed) continue;
		indexEntries.push(toSessionIndexEntry(parsed, file));
	} catch {}
	const index = {
		schema: SESSION_INDEX_SCHEMA,
		files,
		entries: indexEntries
	};
	await writeSessionIndex(sessionDir, index);
	return index;
}
async function loadOrRebuildSessionIndex(sessionDir) {
	const files = (await fs$1.readdir(sessionDir, { withFileTypes: true })).filter((entry) => entry.isFile() && entry.name.endsWith(".json") && entry.name !== "index.json").map((entry) => entry.name).toSorted();
	const existing = await readSessionIndex(sessionDir);
	if (existing && existing.files.length === files.length && existing.files.every((file, index) => file === files[index])) return existing;
	return await rebuildSessionIndex(sessionDir);
}
//#endregion
//#region src/session/persistence/repository.ts
const DEFAULT_HISTORY_LIMIT = 20;
function sessionFilePath(acpxRecordId) {
	const safeId = encodeURIComponent(acpxRecordId);
	return path.join(sessionBaseDir(), `${safeId}.json`);
}
function sessionBaseDir() {
	return path.join(os.homedir(), ".acpx", "sessions");
}
async function ensureSessionDir() {
	await fs$1.mkdir(sessionBaseDir(), { recursive: true });
}
async function loadRecordFromIndexEntry(entry) {
	try {
		const payload = await fs$1.readFile(path.join(sessionBaseDir(), entry.file), "utf8");
		return parseSessionRecord(JSON.parse(payload)) ?? void 0;
	} catch {
		return;
	}
}
async function loadSessionIndexEntries() {
	await ensureSessionDir();
	return (await measurePerf("session.index_load", async () => {
		return await loadOrRebuildSessionIndex(sessionBaseDir());
	})).entries;
}
function matchesSessionEntry(session, normalizedCwd, normalizedName, includeClosed = false) {
	if (session.cwd !== normalizedCwd) return false;
	if (!includeClosed && session.closed) return false;
	if (normalizedName == null) return session.name == null;
	return session.name === normalizedName;
}
async function writeSessionRecord(record) {
	await measurePerf("session.write_record", async () => {
		await ensureSessionDir();
		const persisted = serializeSessionRecordForDisk(record);
		assertPersistedKeyPolicy(persisted);
		const file = sessionFilePath(record.acpxRecordId);
		const tempFile = `${file}.${process.pid}.${Date.now()}.tmp`;
		const payload = JSON.stringify(persisted, null, 2);
		await fs$1.writeFile(tempFile, `${payload}\n`, "utf8");
		await fs$1.rename(tempFile, file);
		const sessionDir = sessionBaseDir();
		const index = await loadOrRebuildSessionIndex(sessionDir);
		const fileName = path.basename(file);
		const entries = index.entries.filter((entry) => entry.file !== fileName);
		entries.push(toSessionIndexEntry(record, fileName));
		await writeSessionIndex(sessionDir, {
			files: [...new Set([...index.files.filter((entry) => entry !== fileName), fileName])],
			entries
		});
	});
}
async function resolveSessionRecord(sessionId) {
	await ensureSessionDir();
	const directPath = sessionFilePath(sessionId);
	try {
		const directPayload = await measurePerf("session.resolve_direct", async () => {
			return await fs$1.readFile(directPath, "utf8");
		});
		const directRecord = parseSessionRecord(JSON.parse(directPayload));
		if (directRecord) return directRecord;
	} catch {}
	const entries = await loadSessionIndexEntries();
	const exactEntries = entries.filter((entry) => entry.acpxRecordId === sessionId || entry.acpSessionId === sessionId);
	const exactRecords = (await Promise.all(exactEntries.map((entry) => loadRecordFromIndexEntry(entry)))).filter((entry) => Boolean(entry));
	if (exactRecords.length === 1) return exactRecords[0];
	if (exactRecords.length > 1) throw new SessionResolutionError(`Multiple sessions match id: ${sessionId}`);
	const suffixEntries = entries.filter((entry) => entry.acpxRecordId.endsWith(sessionId) || entry.acpSessionId.endsWith(sessionId));
	const suffixRecords = (await Promise.all(suffixEntries.map((entry) => loadRecordFromIndexEntry(entry)))).filter((entry) => Boolean(entry));
	if (suffixRecords.length === 1) return suffixRecords[0];
	if (suffixRecords.length > 1) throw new SessionResolutionError(`Session id is ambiguous: ${sessionId}`);
	incrementPerfCounter("session.resolve_miss");
	throw new SessionNotFoundError(sessionId);
}
function hasGitDirectory(dir) {
	const gitPath = path.join(dir, ".git");
	try {
		return statSync(gitPath).isDirectory();
	} catch {
		return false;
	}
}
function isWithinBoundary(boundary, target) {
	const relative = path.relative(boundary, target);
	return relative.length === 0 || !relative.startsWith("..") && !path.isAbsolute(relative);
}
function absolutePath(value) {
	return path.resolve(value);
}
function findGitRepositoryRoot(startDir) {
	let current = absolutePath(startDir);
	const root = path.parse(current).root;
	for (;;) {
		if (hasGitDirectory(current)) return current;
		if (current === root) return;
		const parent = path.dirname(current);
		if (parent === current) return;
		current = parent;
	}
}
function normalizeName(value) {
	if (value == null) return;
	const trimmed = value.trim();
	return trimmed.length > 0 ? trimmed : void 0;
}
function isoNow$2() {
	return (/* @__PURE__ */ new Date()).toISOString();
}
async function listSessions() {
	await ensureSessionDir();
	const entries = await loadSessionIndexEntries();
	const records = [];
	for (const entry of entries) {
		const parsed = await loadRecordFromIndexEntry(entry);
		if (parsed) records.push(parsed);
	}
	records.sort((a, b) => b.lastUsedAt.localeCompare(a.lastUsedAt));
	return records;
}
async function listSessionsForAgent(agentCommand) {
	const entries = (await loadSessionIndexEntries()).filter((session) => session.agentCommand === agentCommand);
	return (await Promise.all(entries.map((entry) => loadRecordFromIndexEntry(entry)))).filter((entry) => Boolean(entry)).toSorted((a, b) => b.lastUsedAt.localeCompare(a.lastUsedAt));
}
async function findSession(options) {
	const normalizedCwd = absolutePath(options.cwd);
	const normalizedName = normalizeName(options.name);
	const match = (await loadSessionIndexEntries()).find((session) => session.agentCommand === options.agentCommand && matchesSessionEntry(session, normalizedCwd, normalizedName, options.includeClosed));
	if (!match) return;
	return await loadRecordFromIndexEntry(match);
}
async function findSessionByDirectoryWalk(options) {
	const normalizedName = normalizeName(options.name);
	const normalizedStart = absolutePath(options.cwd);
	const normalizedBoundary = absolutePath(options.boundary ?? normalizedStart);
	const walkBoundary = isWithinBoundary(normalizedBoundary, normalizedStart) ? normalizedBoundary : normalizedStart;
	const sessions = (await loadSessionIndexEntries()).filter((session) => session.agentCommand === options.agentCommand);
	let current = normalizedStart;
	const walkRoot = path.parse(current).root;
	for (;;) {
		const match = sessions.find((session) => matchesSessionEntry(session, current, normalizedName));
		if (match) return await loadRecordFromIndexEntry(match);
		if (current === walkBoundary || current === walkRoot) return;
		const parent = path.dirname(current);
		if (parent === current) return;
		current = parent;
		if (!isWithinBoundary(walkBoundary, current)) return;
	}
}
//#endregion
//#region src/permission-prompt.ts
async function promptForPermission(options) {
	if (!process.stdin.isTTY || !process.stderr.isTTY) return false;
	if (options.header) process.stderr.write(`\n${options.header}\n`);
	if (options.details && options.details.trim().length > 0) process.stderr.write(`${options.details}\n`);
	const rl = readline.createInterface({
		input: process.stdin,
		output: process.stderr
	});
	try {
		const normalized = (await rl.question(options.prompt)).trim().toLowerCase();
		return normalized === "y" || normalized === "yes";
	} finally {
		rl.close();
	}
}
//#endregion
//#region src/filesystem.ts
const WRITE_PREVIEW_MAX_LINES = 16;
const WRITE_PREVIEW_MAX_CHARS = 1200;
function nowIso$1() {
	return (/* @__PURE__ */ new Date()).toISOString();
}
function isWithinRoot(rootDir, targetPath) {
	const relative = path.relative(rootDir, targetPath);
	return relative.length === 0 || !relative.startsWith("..") && !path.isAbsolute(relative);
}
function toWritePreview(content) {
	const lines = content.replace(/\r\n/g, "\n").split("\n");
	const visibleLines = lines.slice(0, WRITE_PREVIEW_MAX_LINES);
	let preview = visibleLines.join("\n");
	if (lines.length > visibleLines.length) preview += `\n... (${lines.length - visibleLines.length} more lines)`;
	if (preview.length > WRITE_PREVIEW_MAX_CHARS) preview = `${preview.slice(0, WRITE_PREVIEW_MAX_CHARS - 3)}...`;
	return preview;
}
async function defaultConfirmWrite(filePath, preview) {
	return await promptForPermission({
		header: `[permission] Allow write to ${filePath}?`,
		details: preview,
		prompt: "Allow write? (y/N) "
	});
}
function canPromptForPermission$2() {
	return Boolean(process.stdin.isTTY && process.stderr.isTTY);
}
var FileSystemHandlers = class {
	rootDir;
	permissionMode;
	nonInteractivePermissions;
	onOperation;
	usesDefaultConfirmWrite;
	confirmWrite;
	constructor(options) {
		this.rootDir = path.resolve(options.cwd);
		this.permissionMode = options.permissionMode;
		this.nonInteractivePermissions = options.nonInteractivePermissions ?? "deny";
		this.onOperation = options.onOperation;
		this.usesDefaultConfirmWrite = options.confirmWrite == null;
		this.confirmWrite = options.confirmWrite ?? defaultConfirmWrite;
	}
	updatePermissionPolicy(permissionMode, nonInteractivePermissions) {
		this.permissionMode = permissionMode;
		this.nonInteractivePermissions = nonInteractivePermissions ?? "deny";
	}
	async readTextFile(params) {
		const filePath = this.resolvePathWithinRoot(params.path);
		const summary = `read_text_file: ${filePath}`;
		this.emitOperation({
			method: "fs/read_text_file",
			status: "running",
			summary,
			details: this.readWindowDetails(params.line, params.limit),
			timestamp: nowIso$1()
		});
		try {
			if (this.permissionMode === "deny-all") throw new PermissionDeniedError("Permission denied for fs/read_text_file (--deny-all)");
			const content = await fs$1.readFile(filePath, "utf8");
			const sliced = this.sliceContent(content, params.line, params.limit);
			this.emitOperation({
				method: "fs/read_text_file",
				status: "completed",
				summary,
				details: this.readWindowDetails(params.line, params.limit),
				timestamp: nowIso$1()
			});
			return { content: sliced };
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			this.emitOperation({
				method: "fs/read_text_file",
				status: "failed",
				summary,
				details: message,
				timestamp: nowIso$1()
			});
			throw error;
		}
	}
	async writeTextFile(params) {
		const filePath = this.resolvePathWithinRoot(params.path);
		const preview = toWritePreview(params.content);
		const summary = `write_text_file: ${filePath}`;
		this.emitOperation({
			method: "fs/write_text_file",
			status: "running",
			summary,
			details: preview,
			timestamp: nowIso$1()
		});
		try {
			if (!await this.isWriteApproved(filePath, preview)) throw new PermissionDeniedError("Permission denied for fs/write_text_file");
			await fs$1.mkdir(path.dirname(filePath), { recursive: true });
			await fs$1.writeFile(filePath, params.content, "utf8");
			this.emitOperation({
				method: "fs/write_text_file",
				status: "completed",
				summary,
				details: preview,
				timestamp: nowIso$1()
			});
			return {};
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			this.emitOperation({
				method: "fs/write_text_file",
				status: "failed",
				summary,
				details: message,
				timestamp: nowIso$1()
			});
			throw error;
		}
	}
	async isWriteApproved(filePath, preview) {
		if (this.permissionMode === "approve-all") return true;
		if (this.permissionMode === "deny-all") return false;
		if (this.usesDefaultConfirmWrite && this.nonInteractivePermissions === "fail" && !canPromptForPermission$2()) throw new PermissionPromptUnavailableError();
		return await this.confirmWrite(filePath, preview);
	}
	resolvePathWithinRoot(rawPath) {
		if (!path.isAbsolute(rawPath)) throw new Error(`Path must be absolute: ${rawPath}`);
		const resolved = path.resolve(rawPath);
		if (!isWithinRoot(this.rootDir, resolved)) throw new Error(`Path is outside allowed cwd subtree: ${resolved}`);
		return resolved;
	}
	sliceContent(content, line, limit) {
		if (line == null && limit == null) return content;
		const lines = content.split("\n");
		const startIndex = Math.max(0, (line == null ? 1 : Math.max(1, Math.trunc(line))) - 1);
		const maxLines = limit == null ? void 0 : Math.max(0, Math.trunc(limit));
		if (maxLines === 0) return "";
		const endIndex = maxLines == null ? lines.length : Math.min(lines.length, startIndex + maxLines);
		return lines.slice(startIndex, endIndex).join("\n");
	}
	readWindowDetails(line, limit) {
		if (line == null && limit == null) return;
		return `line=${line == null ? 1 : Math.max(1, Math.trunc(line))}, limit=${limit == null ? "all" : Math.max(0, Math.trunc(limit))}`;
	}
	emitOperation(operation) {
		this.onOperation?.(operation);
	}
};
//#endregion
//#region src/permissions.ts
const PERMISSION_MODE_RANK = {
	"deny-all": 0,
	"approve-reads": 1,
	"approve-all": 2
};
function selected(optionId) {
	return { outcome: {
		outcome: "selected",
		optionId
	} };
}
function cancelled() {
	return { outcome: { outcome: "cancelled" } };
}
function pickOption(options, kinds) {
	for (const kind of kinds) {
		const match = options.find((option) => option.kind === kind);
		if (match) return match;
	}
}
function inferToolKind(params) {
	if (params.toolCall.kind) return params.toolCall.kind;
	const title = params.toolCall.title?.trim().toLowerCase();
	if (!title) return;
	const head = title.split(":", 1)[0]?.trim();
	if (!head) return;
	if (head.includes("read") || head.includes("cat")) return "read";
	if (head.includes("search") || head.includes("find") || head.includes("grep")) return "search";
	if (head.includes("write") || head.includes("edit") || head.includes("patch")) return "edit";
	if (head.includes("delete") || head.includes("remove")) return "delete";
	if (head.includes("move") || head.includes("rename")) return "move";
	if (head.includes("run") || head.includes("execute") || head.includes("bash")) return "execute";
	if (head.includes("fetch") || head.includes("http") || head.includes("url")) return "fetch";
	if (head.includes("think")) return "think";
	return "other";
}
function isAutoApprovedReadKind(kind) {
	return kind === "read" || kind === "search";
}
async function promptForToolPermission(params) {
	return await promptForPermission({ prompt: `\n[permission] Allow ${params.toolCall.title ?? "tool"} [${inferToolKind(params) ?? "other"}]? (y/N) ` });
}
function canPromptForPermission$1() {
	return Boolean(process.stdin.isTTY && process.stderr.isTTY);
}
function permissionModeSatisfies(actual, required) {
	return PERMISSION_MODE_RANK[actual] >= PERMISSION_MODE_RANK[required];
}
async function resolvePermissionRequest(params, mode, nonInteractivePolicy = "deny") {
	const options = params.options ?? [];
	if (options.length === 0) return cancelled();
	const allowOption = pickOption(options, ["allow_once", "allow_always"]);
	const rejectOption = pickOption(options, ["reject_once", "reject_always"]);
	if (mode === "approve-all") {
		if (allowOption) return selected(allowOption.optionId);
		return selected(options[0].optionId);
	}
	if (mode === "deny-all") {
		if (rejectOption) return selected(rejectOption.optionId);
		return cancelled();
	}
	if (isAutoApprovedReadKind(inferToolKind(params)) && allowOption) return selected(allowOption.optionId);
	if (!canPromptForPermission$1()) {
		if (nonInteractivePolicy === "fail") throw new PermissionPromptUnavailableError();
		if (rejectOption) return selected(rejectOption.optionId);
		return cancelled();
	}
	const approved = await promptForToolPermission(params);
	if (approved && allowOption) return selected(allowOption.optionId);
	if (!approved && rejectOption) return selected(rejectOption.optionId);
	return cancelled();
}
function classifyPermissionDecision(params, response) {
	if (response.outcome.outcome !== "selected") return "cancelled";
	const selectedOptionId = response.outcome.optionId;
	const selectedOption = params.options.find((option) => option.optionId === selectedOptionId);
	if (!selectedOption) return "cancelled";
	if (selectedOption.kind === "allow_once" || selectedOption.kind === "allow_always") return "approved";
	return "denied";
}
//#endregion
//#region src/spawn-command-options.ts
function readWindowsEnvValue(env, key) {
	const matchedKey = Object.keys(env).find((entry) => entry.toUpperCase() === key);
	return matchedKey ? env[matchedKey] : void 0;
}
function resolveWindowsCommand(command, env = process.env) {
	const extensions = (readWindowsEnvValue(env, "PATHEXT") ?? ".COM;.EXE;.BAT;.CMD").split(";").map((value) => value.trim().toLowerCase()).filter((value) => value.length > 0);
	const candidates = path.extname(command).length > 0 ? [command] : extensions.map((extension) => `${command}${extension}`);
	if (command.includes("/") || command.includes("\\") || path.isAbsolute(command)) return candidates.find((candidate) => fs.existsSync(candidate));
	const pathValue = readWindowsEnvValue(env, "PATH");
	if (!pathValue) return;
	for (const directory of pathValue.split(";")) {
		const trimmedDirectory = directory.trim();
		if (trimmedDirectory.length === 0) continue;
		for (const candidate of candidates) {
			const resolved = path.join(trimmedDirectory, candidate);
			if (fs.existsSync(resolved)) return resolved;
		}
	}
}
function shouldUseWindowsBatchShell(command, platform = process.platform, env = process.env) {
	if (platform !== "win32") return false;
	const resolvedCommand = resolveWindowsCommand(command, env) ?? command;
	const ext = path.extname(resolvedCommand).toLowerCase();
	return ext === ".cmd" || ext === ".bat";
}
function buildSpawnCommandOptions(command, options, platform = process.platform, env = process.env) {
	if (!shouldUseWindowsBatchShell(command, platform, env)) return options;
	return {
		...options,
		shell: true
	};
}
//#endregion
//#region src/acp/client-process.ts
function isoNow$1() {
	return (/* @__PURE__ */ new Date()).toISOString();
}
function waitForSpawn$1(child) {
	return new Promise((resolve, reject) => {
		const onSpawn = () => {
			child.off("error", onError);
			resolve();
		};
		const onError = (error) => {
			child.off("spawn", onSpawn);
			reject(error);
		};
		child.once("spawn", onSpawn);
		child.once("error", onError);
	});
}
function isChildProcessRunning(child) {
	return child.exitCode == null && child.signalCode == null;
}
function requireAgentStdio(child) {
	if (!child.stdin || !child.stdout || !child.stderr) throw new Error("ACP agent must be spawned with piped stdin/stdout/stderr");
	return child;
}
function waitForChildExit(child, timeoutMs) {
	if (!isChildProcessRunning(child)) return Promise.resolve(true);
	return new Promise((resolve) => {
		let settled = false;
		const timer = setTimeout(() => {
			finish(false);
		}, Math.max(0, timeoutMs));
		const finish = (value) => {
			if (settled) return;
			settled = true;
			child.off("close", onExitLike);
			child.off("exit", onExitLike);
			clearTimeout(timer);
			resolve(value);
		};
		const onExitLike = () => {
			finish(true);
		};
		child.once("close", onExitLike);
		child.once("exit", onExitLike);
	});
}
function splitCommandLine(value) {
	const parts = [];
	let current = "";
	let quote = null;
	let escaping = false;
	for (const ch of value) {
		if (escaping) {
			current += ch;
			escaping = false;
			continue;
		}
		if (ch === "\\" && quote !== "'") {
			escaping = true;
			continue;
		}
		if (quote) {
			if (ch === quote) quote = null;
			else current += ch;
			continue;
		}
		if (ch === "'" || ch === "\"") {
			quote = ch;
			continue;
		}
		if (/\s/.test(ch)) {
			if (current.length > 0) {
				parts.push(current);
				current = "";
			}
			continue;
		}
		current += ch;
	}
	if (escaping) current += "\\";
	if (quote) throw new Error("Invalid --agent command: unterminated quote");
	if (current.length > 0) parts.push(current);
	if (parts.length === 0) throw new Error("Invalid --agent command: empty command");
	return {
		command: parts[0],
		args: parts.slice(1)
	};
}
function asAbsoluteCwd(cwd) {
	return path.resolve(cwd);
}
function basenameToken(value) {
	return path.basename(value).toLowerCase().replace(/\.(cmd|exe|bat)$/u, "");
}
//#endregion
//#region src/acp/agent-command.ts
const DEFAULT_AGENT_CLOSE_AFTER_STDIN_END_MS = 100;
const QODER_AGENT_CLOSE_AFTER_STDIN_END_MS = 750;
const GEMINI_ACP_STARTUP_TIMEOUT_MS = 15e3;
const CLAUDE_ACP_SESSION_CREATE_TIMEOUT_MS = 6e4;
const GEMINI_VERSION_TIMEOUT_MS = 2e3;
const GEMINI_ACP_FLAG_VERSION = [
	0,
	33,
	0
];
const COPILOT_HELP_TIMEOUT_MS = 2e3;
const QODER_BENIGN_STDOUT_LINES = new Set(["Received interrupt signal. Cleaning up resources...", "Cleanup completed. Exiting..."]);
function resolveAgentCloseAfterStdinEndMs(agentCommand) {
	const { command } = splitCommandLine(agentCommand);
	return basenameToken(command) === "qodercli" ? QODER_AGENT_CLOSE_AFTER_STDIN_END_MS : DEFAULT_AGENT_CLOSE_AFTER_STDIN_END_MS;
}
function shouldIgnoreNonJsonAgentOutputLine(agentCommand, trimmedLine) {
	const { command } = splitCommandLine(agentCommand);
	return basenameToken(command) === "qodercli" && QODER_BENIGN_STDOUT_LINES.has(trimmedLine);
}
function isGeminiAcpCommand(command, args) {
	return basenameToken(command) === "gemini" && (args.includes("--acp") || args.includes("--experimental-acp"));
}
function isClaudeAcpCommand(command, args) {
	if (basenameToken(command) === "claude-agent-acp") return true;
	return args.some((arg) => arg.includes("claude-agent-acp"));
}
function isCopilotAcpCommand(command, args) {
	return basenameToken(command) === "copilot" && args.includes("--acp");
}
function isQoderAcpCommand(command, args) {
	return basenameToken(command) === "qodercli" && args.includes("--acp");
}
function hasCommandFlag(args, flagName) {
	return args.some((arg) => arg === flagName || arg.startsWith(`${flagName}=`));
}
function normalizeQoderAllowedToolName(tool) {
	switch (tool.trim().toLowerCase()) {
		case "bash":
		case "glob":
		case "grep":
		case "ls":
		case "read":
		case "write": return tool.trim().toUpperCase();
		default: return tool.trim();
	}
}
function buildQoderAcpCommandArgs(initialArgs, options) {
	const args = [...initialArgs];
	const sessionOptions = options.sessionOptions;
	if (typeof sessionOptions?.maxTurns === "number" && !hasCommandFlag(args, "--max-turns")) args.push(`--max-turns=${sessionOptions.maxTurns}`);
	if (Array.isArray(sessionOptions?.allowedTools) && !hasCommandFlag(args, "--allowed-tools") && !hasCommandFlag(args, "--disallowed-tools")) {
		const encodedTools = sessionOptions.allowedTools.map(normalizeQoderAllowedToolName).join(",");
		args.push(`--allowed-tools=${encodedTools}`);
	}
	return args;
}
function resolveGeminiAcpStartupTimeoutMs() {
	const raw = process.env.ACPX_GEMINI_ACP_STARTUP_TIMEOUT_MS;
	if (typeof raw === "string" && raw.trim().length > 0) {
		const parsed = Number(raw);
		if (Number.isFinite(parsed) && parsed > 0) return Math.round(parsed);
	}
	return GEMINI_ACP_STARTUP_TIMEOUT_MS;
}
function resolveClaudeAcpSessionCreateTimeoutMs() {
	const raw = process.env.ACPX_CLAUDE_ACP_SESSION_CREATE_TIMEOUT_MS;
	if (typeof raw === "string" && raw.trim().length > 0) {
		const parsed = Number(raw);
		if (Number.isFinite(parsed) && parsed > 0) return Math.round(parsed);
	}
	return CLAUDE_ACP_SESSION_CREATE_TIMEOUT_MS;
}
function parseGeminiVersion(value) {
	if (typeof value !== "string") return;
	const normalized = value.trim();
	const match = normalized.match(/(\d+)\.(\d+)\.(\d+)/);
	if (!match) return;
	return {
		raw: normalized,
		parts: [
			Number(match[1]),
			Number(match[2]),
			Number(match[3])
		]
	};
}
function compareVersionParts(left, right) {
	for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
		const leftPart = left[index] ?? 0;
		const rightPart = right[index] ?? 0;
		if (leftPart !== rightPart) return leftPart - rightPart;
	}
	return 0;
}
async function detectGeminiVersion(command) {
	return await new Promise((resolve) => {
		const child = spawn(command, ["--version"], buildSpawnCommandOptions(command, {
			stdio: [
				"ignore",
				"pipe",
				"pipe"
			],
			windowsHide: true
		}));
		let stdout = "";
		let stderr = "";
		let settled = false;
		const finish = (value) => {
			if (settled) return;
			settled = true;
			clearTimeout(timer);
			child.removeAllListeners();
			child.stdout?.removeAllListeners();
			child.stderr?.removeAllListeners();
			resolve(value);
		};
		const timer = setTimeout(() => {
			child.kill("SIGKILL");
			finish(void 0);
		}, GEMINI_VERSION_TIMEOUT_MS);
		child.stdout?.setEncoding("utf8");
		child.stderr?.setEncoding("utf8");
		child.stdout?.on("data", (chunk) => {
			stdout += chunk;
		});
		child.stderr?.on("data", (chunk) => {
			stderr += chunk;
		});
		child.once("error", () => {
			finish(void 0);
		});
		child.once("close", () => {
			finish(parseGeminiVersion(`${stdout}\n${stderr}`.split(/\r?\n/).map((line) => line.trim()).find((line) => /\d+\.\d+\.\d+/.test(line))));
		});
	});
}
async function resolveGeminiCommandArgs(command, args) {
	if (basenameToken(command) !== "gemini" || !args.includes("--acp")) return [...args];
	const version = await detectGeminiVersion(command);
	if (version && compareVersionParts(version.parts, GEMINI_ACP_FLAG_VERSION) < 0) return args.map((arg) => arg === "--acp" ? "--experimental-acp" : arg);
	return [...args];
}
async function readCommandOutput(command, args, timeoutMs) {
	return await new Promise((resolve) => {
		const child = spawn(command, [...args], buildSpawnCommandOptions(command, {
			stdio: [
				"ignore",
				"pipe",
				"pipe"
			],
			windowsHide: true
		}));
		let stdout = "";
		let stderr = "";
		let settled = false;
		const finish = (value) => {
			if (settled) return;
			settled = true;
			clearTimeout(timer);
			child.removeAllListeners();
			child.stdout?.removeAllListeners();
			child.stderr?.removeAllListeners();
			resolve(value);
		};
		const timer = setTimeout(() => {
			child.kill("SIGKILL");
			finish(void 0);
		}, timeoutMs);
		child.stdout?.setEncoding("utf8");
		child.stderr?.setEncoding("utf8");
		child.stdout?.on("data", (chunk) => {
			stdout += chunk;
		});
		child.stderr?.on("data", (chunk) => {
			stderr += chunk;
		});
		child.once("error", () => {
			finish(void 0);
		});
		child.once("close", () => {
			finish(`${stdout}\n${stderr}`);
		});
	});
}
async function buildGeminiAcpStartupTimeoutMessage(command) {
	const parts = ["Gemini CLI ACP startup timed out before initialize completed.", "This usually means the local Gemini CLI is waiting on interactive OAuth or has incompatible ACP subprocess behavior."];
	const version = await detectGeminiVersion(command);
	if (version) parts.push(`Detected Gemini CLI version: ${version.raw}.`);
	if (!process.env.GEMINI_API_KEY && !process.env.GOOGLE_API_KEY) parts.push("No GEMINI_API_KEY or GOOGLE_API_KEY was set for non-interactive auth.");
	parts.push("Try upgrading Gemini CLI and using API-key-based auth for non-interactive ACP runs.");
	return parts.join(" ");
}
function buildClaudeAcpSessionCreateTimeoutMessage() {
	return [
		"Claude ACP session creation timed out before session/new completed.",
		"This matches the known persistent-session stall seen with some Claude Code and @agentclientprotocol/claude-agent-acp combinations.",
		"In harnessed or non-interactive runs, prefer --approve-all with nonInteractivePermissions=deny, upgrade Claude Code and the Claude ACP adapter, or use acpx claude exec as a one-shot fallback."
	].join(" ");
}
async function buildCopilotAcpUnsupportedMessage(command) {
	const parts = ["GitHub Copilot CLI ACP stdio mode is not available in the installed copilot binary.", "acpx copilot expects a Copilot CLI release that supports --acp --stdio."];
	const helpOutput = await readCommandOutput(command, ["--help"], COPILOT_HELP_TIMEOUT_MS);
	if (typeof helpOutput === "string" && !helpOutput.includes("--acp")) parts.push("Detected copilot --help output without --acp support.");
	parts.push("Upgrade GitHub Copilot CLI to a release with ACP stdio support, or use --agent with another ACP-compatible adapter in the meantime.");
	return parts.join(" ");
}
async function ensureCopilotAcpSupport(command) {
	const helpOutput = await readCommandOutput(command, ["--help"], COPILOT_HELP_TIMEOUT_MS);
	if (typeof helpOutput === "string" && !helpOutput.includes("--acp")) throw new CopilotAcpUnsupportedError(await buildCopilotAcpUnsupportedMessage(command), { retryable: false });
}
function buildClaudeCodeOptionsMeta(options) {
	if (!options) return;
	const claudeCodeOptions = {};
	if (typeof options.model === "string" && options.model.trim().length > 0) claudeCodeOptions.model = options.model;
	if (Array.isArray(options.allowedTools)) claudeCodeOptions.allowedTools = [...options.allowedTools];
	if (typeof options.maxTurns === "number") claudeCodeOptions.maxTurns = options.maxTurns;
	if (Object.keys(claudeCodeOptions).length === 0) return;
	return { claudeCode: { options: claudeCodeOptions } };
}
//#endregion
//#region src/acp/auth-env.ts
function toEnvToken(value) {
	return value.trim().replace(/[^a-zA-Z0-9]+/g, "_").replace(/^_+|_+$/g, "").toUpperCase();
}
function buildAuthEnvKeys(methodId) {
	const token = toEnvToken(methodId);
	const keys = new Set([methodId]);
	if (token) {
		keys.add(token);
		keys.add(`ACPX_AUTH_${token}`);
	}
	return [...keys];
}
const authEnvKeysCache = /* @__PURE__ */ new Map();
function authEnvKeys(methodId) {
	const cached = authEnvKeysCache.get(methodId);
	if (cached) return cached;
	const keys = buildAuthEnvKeys(methodId);
	authEnvKeysCache.set(methodId, keys);
	return keys;
}
function readEnvCredential(methodId) {
	for (const key of authEnvKeys(methodId)) {
		const value = process.env[key];
		if (typeof value === "string" && value.trim().length > 0) return value;
	}
}
function buildAgentEnvironment(authCredentials) {
	const env = { ...process.env };
	if (!authCredentials) return env;
	for (const [methodId, credential] of Object.entries(authCredentials)) {
		if (typeof credential !== "string" || credential.trim().length === 0) continue;
		if (!methodId.includes("=") && !methodId.includes("\0") && env[methodId] == null) env[methodId] = credential;
		const normalized = toEnvToken(methodId);
		if (normalized) {
			const prefixed = `ACPX_AUTH_${normalized}`;
			if (env[prefixed] == null) env[prefixed] = credential;
			if (env[normalized] == null) env[normalized] = credential;
		}
	}
	return env;
}
function resolveConfiguredAuthCredential(methodId, authCredentials) {
	const configCredentials = authCredentials ?? {};
	return configCredentials[methodId] ?? configCredentials[toEnvToken(methodId)];
}
function buildAgentSpawnOptions(cwd, authCredentials) {
	return {
		cwd,
		env: buildAgentEnvironment(authCredentials),
		stdio: [
			"pipe",
			"pipe",
			"pipe"
		],
		windowsHide: true
	};
}
//#endregion
//#region src/acp/session-control-errors.ts
const SESSION_CONTROL_UNSUPPORTED_ACP_CODES = new Set([-32601, -32602]);
function asRecord$1(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function isLikelySessionControlUnsupportedError(acp) {
	if (SESSION_CONTROL_UNSUPPORTED_ACP_CODES.has(acp.code)) return true;
	if (acp.code !== -32603) return false;
	const details = asRecord$1(acp.data)?.details;
	return typeof details === "string" && details.toLowerCase().includes("invalid params");
}
function formatSessionControlAcpSummary(acp) {
	const details = asRecord$1(acp.data)?.details;
	if (typeof details === "string" && details.trim().length > 0) return `${details.trim()} (ACP ${acp.code}, adapter reported "${acp.message}")`;
	return `${acp.message} (ACP ${acp.code})`;
}
function maybeWrapSessionControlError(method, error, context) {
	const acp = extractAcpError(error);
	if (!acp || !isLikelySessionControlUnsupportedError(acp)) return error;
	const acpSummary = formatSessionControlAcpSummary(acp);
	const message = `Agent rejected ${method}${context ? ` ${context}` : ""}: ${acpSummary}. The adapter may not implement ${method}, or the requested value is not supported.`;
	const wrapped = new Error(message, { cause: error instanceof Error ? error : void 0 });
	wrapped.acp = acp;
	return wrapped;
}
//#endregion
//#region src/acp/terminal-manager.ts
const DEFAULT_TERMINAL_OUTPUT_LIMIT_BYTES = 64 * 1024;
const DEFAULT_KILL_GRACE_MS = 1500;
function nowIso() {
	return (/* @__PURE__ */ new Date()).toISOString();
}
function toCommandLine(command, args) {
	const renderedArgs = (args ?? []).map((arg) => JSON.stringify(arg)).join(" ");
	return renderedArgs.length > 0 ? `${command} ${renderedArgs}` : command;
}
function toEnvObject(env) {
	if (!env || env.length === 0) return;
	const merged = { ...process.env };
	for (const entry of env) merged[entry.name] = entry.value;
	return merged;
}
function buildTerminalSpawnOptions(command, cwd, env, platform = process.platform) {
	const resolvedEnv = toEnvObject(env);
	return buildSpawnCommandOptions(command, {
		cwd,
		env: resolvedEnv,
		stdio: [
			"ignore",
			"pipe",
			"pipe"
		],
		windowsHide: true
	}, platform, resolvedEnv ?? process.env);
}
function trimToUtf8Boundary(buffer, limit) {
	if (limit <= 0) return Buffer.alloc(0);
	if (buffer.length <= limit) return buffer;
	let start = buffer.length - limit;
	while (start < buffer.length && (buffer[start] & 192) === 128) start += 1;
	if (start >= buffer.length) start = buffer.length - limit;
	return buffer.subarray(start);
}
function waitForSpawn(process) {
	return new Promise((resolve, reject) => {
		const onSpawn = () => {
			process.off("error", onError);
			resolve();
		};
		const onError = (error) => {
			process.off("spawn", onSpawn);
			reject(error);
		};
		process.once("spawn", onSpawn);
		process.once("error", onError);
	});
}
async function defaultConfirmExecute(commandLine) {
	return await promptForPermission({ prompt: `\n[permission] Allow terminal command "${commandLine}"? (y/N) ` });
}
function canPromptForPermission() {
	return Boolean(process.stdin.isTTY && process.stderr.isTTY);
}
function waitMs(ms) {
	return new Promise((resolve) => {
		setTimeout(resolve, Math.max(0, ms));
	});
}
var TerminalManager = class {
	cwd;
	permissionMode;
	nonInteractivePermissions;
	onOperation;
	usesDefaultConfirmExecute;
	confirmExecute;
	killGraceMs;
	terminals = /* @__PURE__ */ new Map();
	constructor(options) {
		this.cwd = options.cwd;
		this.permissionMode = options.permissionMode;
		this.nonInteractivePermissions = options.nonInteractivePermissions ?? "deny";
		this.onOperation = options.onOperation;
		this.usesDefaultConfirmExecute = options.confirmExecute == null;
		this.confirmExecute = options.confirmExecute ?? defaultConfirmExecute;
		this.killGraceMs = Math.max(0, Math.round(options.killGraceMs ?? DEFAULT_KILL_GRACE_MS));
	}
	updatePermissionPolicy(permissionMode, nonInteractivePermissions) {
		this.permissionMode = permissionMode;
		this.nonInteractivePermissions = nonInteractivePermissions ?? "deny";
	}
	async createTerminal(params) {
		const commandLine = toCommandLine(params.command, params.args);
		const summary = `terminal/create: ${commandLine}`;
		this.emitOperation({
			method: "terminal/create",
			status: "running",
			summary,
			timestamp: nowIso()
		});
		try {
			if (!await this.isExecuteApproved(commandLine)) throw new PermissionDeniedError("Permission denied for terminal/create");
			const outputByteLimit = Math.max(0, Math.round(params.outputByteLimit ?? DEFAULT_TERMINAL_OUTPUT_LIMIT_BYTES));
			const proc = spawn(params.command, params.args ?? [], buildTerminalSpawnOptions(params.command, params.cwd ?? this.cwd, params.env));
			await waitForSpawn(proc);
			let resolveExit = () => {};
			const exitPromise = new Promise((resolve) => {
				resolveExit = resolve;
			});
			const terminal = {
				process: proc,
				output: Buffer.alloc(0),
				truncated: false,
				outputByteLimit,
				exitCode: void 0,
				signal: void 0,
				exitPromise,
				resolveExit
			};
			const appendOutput = (chunk) => {
				const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
				if (bytes.length === 0) return;
				terminal.output = Buffer.concat([terminal.output, bytes]);
				if (terminal.output.length > terminal.outputByteLimit) {
					terminal.output = trimToUtf8Boundary(terminal.output, terminal.outputByteLimit);
					terminal.truncated = true;
				}
			};
			proc.stdout.on("data", appendOutput);
			proc.stderr.on("data", appendOutput);
			proc.once("exit", (exitCode, signal) => {
				terminal.exitCode = exitCode;
				terminal.signal = signal;
				terminal.resolveExit({
					exitCode: exitCode ?? null,
					signal: signal ?? null
				});
			});
			const terminalId = randomUUID();
			this.terminals.set(terminalId, terminal);
			this.emitOperation({
				method: "terminal/create",
				status: "completed",
				summary,
				details: `terminalId=${terminalId}`,
				timestamp: nowIso()
			});
			return { terminalId };
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			this.emitOperation({
				method: "terminal/create",
				status: "failed",
				summary,
				details: message,
				timestamp: nowIso()
			});
			throw error;
		}
	}
	async terminalOutput(params) {
		const terminal = this.getTerminal(params.terminalId);
		if (!terminal) throw new Error(`Unknown terminal: ${params.terminalId}`);
		const hasExitStatus = terminal.exitCode !== void 0 || terminal.signal !== void 0;
		this.emitOperation({
			method: "terminal/output",
			status: "completed",
			summary: `terminal/output: ${params.terminalId}`,
			timestamp: nowIso()
		});
		return {
			output: terminal.output.toString("utf8"),
			truncated: terminal.truncated,
			exitStatus: hasExitStatus ? {
				exitCode: terminal.exitCode ?? null,
				signal: terminal.signal ?? null
			} : void 0
		};
	}
	async waitForTerminalExit(params) {
		const terminal = this.getTerminal(params.terminalId);
		if (!terminal) throw new Error(`Unknown terminal: ${params.terminalId}`);
		const response = await terminal.exitPromise;
		this.emitOperation({
			method: "terminal/wait_for_exit",
			status: "completed",
			summary: `terminal/wait_for_exit: ${params.terminalId}`,
			details: `exitCode=${response.exitCode ?? "null"}, signal=${response.signal ?? "null"}`,
			timestamp: nowIso()
		});
		return response;
	}
	async killTerminal(params) {
		const terminal = this.getTerminal(params.terminalId);
		if (!terminal) throw new Error(`Unknown terminal: ${params.terminalId}`);
		const summary = `terminal/kill: ${params.terminalId}`;
		this.emitOperation({
			method: "terminal/kill",
			status: "running",
			summary,
			timestamp: nowIso()
		});
		try {
			await this.killProcess(terminal);
			this.emitOperation({
				method: "terminal/kill",
				status: "completed",
				summary,
				timestamp: nowIso()
			});
			return {};
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			this.emitOperation({
				method: "terminal/kill",
				status: "failed",
				summary,
				details: message,
				timestamp: nowIso()
			});
			throw error;
		}
	}
	async releaseTerminal(params) {
		const summary = `terminal/release: ${params.terminalId}`;
		this.emitOperation({
			method: "terminal/release",
			status: "running",
			summary,
			timestamp: nowIso()
		});
		const terminal = this.getTerminal(params.terminalId);
		if (!terminal) {
			this.emitOperation({
				method: "terminal/release",
				status: "completed",
				summary,
				details: "already released",
				timestamp: nowIso()
			});
			return {};
		}
		try {
			await this.killProcess(terminal);
			await terminal.exitPromise.catch(() => {});
			terminal.output = Buffer.alloc(0);
			this.terminals.delete(params.terminalId);
			this.emitOperation({
				method: "terminal/release",
				status: "completed",
				summary,
				timestamp: nowIso()
			});
			return {};
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			this.emitOperation({
				method: "terminal/release",
				status: "failed",
				summary,
				details: message,
				timestamp: nowIso()
			});
			throw error;
		}
	}
	async shutdown() {
		for (const terminalId of Array.from(this.terminals.keys())) await this.releaseTerminal({
			terminalId,
			sessionId: "shutdown"
		});
	}
	getTerminal(terminalId) {
		return this.terminals.get(terminalId);
	}
	emitOperation(operation) {
		this.onOperation?.(operation);
	}
	async isExecuteApproved(commandLine) {
		if (this.permissionMode === "approve-all") return true;
		if (this.permissionMode === "deny-all") return false;
		if (this.usesDefaultConfirmExecute && this.nonInteractivePermissions === "fail" && !canPromptForPermission()) throw new PermissionPromptUnavailableError();
		return await this.confirmExecute(commandLine);
	}
	isRunning(terminal) {
		return terminal.exitCode === void 0 && terminal.signal === void 0;
	}
	async killProcess(terminal) {
		if (!this.isRunning(terminal)) return;
		try {
			terminal.process.kill("SIGTERM");
		} catch {
			return;
		}
		if (await Promise.race([terminal.exitPromise.then(() => true), waitMs(this.killGraceMs).then(() => false)]) || !this.isRunning(terminal)) return;
		try {
			terminal.process.kill("SIGKILL");
		} catch {
			return;
		}
		await Promise.race([terminal.exitPromise.then(() => void 0), waitMs(this.killGraceMs)]);
	}
};
//#endregion
//#region src/acp/client.ts
const REPLAY_IDLE_MS = 80;
const REPLAY_DRAIN_TIMEOUT_MS = 5e3;
const DRAIN_POLL_INTERVAL_MS = 20;
const AGENT_CLOSE_TERM_GRACE_MS = 1500;
const AGENT_CLOSE_KILL_GRACE_MS = 1e3;
const STARTUP_STDERR_MAX_CHARS = 8192;
function shouldSuppressSdkConsoleError(args) {
	if (args.length === 0) return false;
	return typeof args[0] === "string" && args[0] === "Error handling request";
}
function installSdkConsoleErrorSuppression() {
	const originalConsoleError = console.error;
	console.error = (...args) => {
		if (shouldSuppressSdkConsoleError(args)) return;
		originalConsoleError(...args);
	};
	return () => {
		console.error = originalConsoleError;
	};
}
function createNdJsonMessageStream(agentCommand, output, input) {
	const textEncoder = new TextEncoder();
	const textDecoder = new TextDecoder();
	return {
		readable: new ReadableStream({ async start(controller) {
			let content = "";
			const reader = input.getReader();
			try {
				while (true) {
					const { value, done } = await reader.read();
					if (done) break;
					if (!value) continue;
					content += textDecoder.decode(value, { stream: true });
					const lines = content.split("\n");
					content = lines.pop() || "";
					for (const line of lines) {
						const trimmedLine = line.trim();
						if (!trimmedLine || shouldIgnoreNonJsonAgentOutputLine(agentCommand, trimmedLine)) continue;
						try {
							const message = JSON.parse(trimmedLine);
							controller.enqueue(message);
						} catch (err) {
							console.error("Failed to parse JSON message:", trimmedLine, err);
						}
					}
				}
			} finally {
				reader.releaseLock();
				controller.close();
			}
		} }),
		writable: new WritableStream({ async write(message) {
			const content = JSON.stringify(message) + "\n";
			const writer = output.getWriter();
			try {
				await writer.write(textEncoder.encode(content));
			} finally {
				writer.releaseLock();
			}
		} })
	};
}
var AcpClient = class {
	options;
	connection;
	agent;
	initResult;
	loadedSessionId;
	eventHandlers;
	permissionStats = {
		requested: 0,
		approved: 0,
		denied: 0,
		cancelled: 0
	};
	filesystem;
	terminalManager;
	sessionUpdateChain = Promise.resolve();
	observedSessionUpdates = 0;
	processedSessionUpdates = 0;
	suppressSessionUpdates = false;
	suppressReplaySessionUpdateMessages = false;
	activePrompt;
	cancellingSessionIds = /* @__PURE__ */ new Set();
	closing = false;
	agentStartedAt;
	lastAgentExit;
	lastKnownPid;
	promptPermissionFailures = /* @__PURE__ */ new Map();
	pendingConnectionRequests = /* @__PURE__ */ new Set();
	constructor(options) {
		this.options = {
			...options,
			cwd: asAbsoluteCwd(options.cwd),
			authPolicy: options.authPolicy ?? "skip"
		};
		this.eventHandlers = {
			onAcpMessage: this.options.onAcpMessage,
			onAcpOutputMessage: this.options.onAcpOutputMessage,
			onSessionUpdate: this.options.onSessionUpdate,
			onClientOperation: this.options.onClientOperation
		};
		this.filesystem = new FileSystemHandlers({
			cwd: this.options.cwd,
			permissionMode: this.options.permissionMode,
			nonInteractivePermissions: this.options.nonInteractivePermissions,
			onOperation: (operation) => {
				this.eventHandlers.onClientOperation?.(operation);
			}
		});
		this.terminalManager = new TerminalManager({
			cwd: this.options.cwd,
			permissionMode: this.options.permissionMode,
			nonInteractivePermissions: this.options.nonInteractivePermissions,
			onOperation: (operation) => {
				this.eventHandlers.onClientOperation?.(operation);
			}
		});
	}
	get initializeResult() {
		return this.initResult;
	}
	getAgentPid() {
		return this.agent?.pid ?? this.lastKnownPid;
	}
	getPermissionStats() {
		return { ...this.permissionStats };
	}
	getAgentLifecycleSnapshot() {
		const pid = this.agent?.pid ?? this.lastKnownPid;
		const running = Boolean(this.agent) && this.agent?.exitCode == null && this.agent?.signalCode == null && !this.agent?.killed;
		return {
			pid,
			startedAt: this.agentStartedAt,
			running,
			lastExit: this.lastAgentExit ? { ...this.lastAgentExit } : void 0
		};
	}
	supportsLoadSession() {
		return Boolean(this.initResult?.agentCapabilities?.loadSession);
	}
	supportsCloseSession() {
		return Boolean(this.initResult?.agentCapabilities?.sessionCapabilities?.close);
	}
	setEventHandlers(handlers) {
		this.eventHandlers = { ...handlers };
	}
	clearEventHandlers() {
		this.eventHandlers = {};
	}
	updateRuntimeOptions(options) {
		if (options.permissionMode) this.options.permissionMode = options.permissionMode;
		if (options.nonInteractivePermissions !== void 0) this.options.nonInteractivePermissions = options.nonInteractivePermissions;
		if (options.permissionMode || options.nonInteractivePermissions !== void 0) {
			this.filesystem.updatePermissionPolicy(this.options.permissionMode, this.options.nonInteractivePermissions);
			this.terminalManager.updatePermissionPolicy(this.options.permissionMode, this.options.nonInteractivePermissions);
		}
		if (options.suppressSdkConsoleErrors !== void 0) this.options.suppressSdkConsoleErrors = options.suppressSdkConsoleErrors;
		if (options.verbose !== void 0) this.options.verbose = options.verbose;
	}
	hasReusableSession(sessionId) {
		return this.connection != null && this.agent != null && isChildProcessRunning(this.agent) && this.loadedSessionId === sessionId;
	}
	hasActivePrompt(sessionId) {
		if (!this.activePrompt) return false;
		if (sessionId == null) return true;
		return this.activePrompt.sessionId === sessionId;
	}
	async start() {
		if (this.connection && this.agent && isChildProcessRunning(this.agent)) return;
		if (this.connection || this.agent) await this.close();
		const configuredCommand = splitCommandLine(this.options.agentCommand);
		const resolvedBuiltInLaunch = resolveBuiltInAgentLaunch(this.options.agentCommand);
		const spawnCommand = resolvedBuiltInLaunch?.command ?? configuredCommand.command;
		let args = resolvedBuiltInLaunch?.args ?? configuredCommand.args;
		args = await resolveGeminiCommandArgs(spawnCommand, args);
		if (isQoderAcpCommand(spawnCommand, args)) args = buildQoderAcpCommandArgs(args, this.options);
		if (resolvedBuiltInLaunch?.source === "installed") this.log(`spawning installed built-in agent ${resolvedBuiltInLaunch.packageName}${resolvedBuiltInLaunch.packageVersion ? `@${resolvedBuiltInLaunch.packageVersion}` : ""} via ${spawnCommand} ${args.join(" ")}`);
		else if (resolvedBuiltInLaunch?.source === "package-exec") this.log(`spawning built-in agent ${resolvedBuiltInLaunch.packageName}@${resolvedBuiltInLaunch.packageRange} via current Node package exec bridge ${spawnCommand} ${args.join(" ")}`);
		else this.log(`spawning agent: ${spawnCommand} ${args.join(" ")}`);
		const geminiAcp = isGeminiAcpCommand(spawnCommand, args);
		if (isCopilotAcpCommand(spawnCommand, args)) await ensureCopilotAcpSupport(spawnCommand);
		const spawnedChild = spawn(spawnCommand, args, buildSpawnCommandOptions(spawnCommand, buildAgentSpawnOptions(this.options.cwd, this.options.authCredentials)));
		try {
			await waitForSpawn$1(spawnedChild);
		} catch (error) {
			throw new AgentSpawnError(this.options.agentCommand, error);
		}
		const child = requireAgentStdio(spawnedChild);
		this.closing = false;
		this.agentStartedAt = isoNow$1();
		this.lastAgentExit = void 0;
		this.lastKnownPid = child.pid ?? void 0;
		this.attachAgentLifecycleObservers(child);
		const startupStderr = [];
		child.stderr.on("data", (chunk) => {
			this.captureStartupStderr(startupStderr, chunk);
			if (!this.options.verbose) return;
			process.stderr.write(chunk);
		});
		const input = Writable.toWeb(child.stdin);
		const output = Readable.toWeb(child.stdout);
		const connection = new ClientSideConnection(() => ({
			sessionUpdate: async (params) => {
				await this.handleSessionUpdate(params);
			},
			requestPermission: async (params) => {
				return this.handlePermissionRequest(params);
			},
			readTextFile: async (params) => {
				return this.handleReadTextFile(params);
			},
			writeTextFile: async (params) => {
				return this.handleWriteTextFile(params);
			},
			createTerminal: async (params) => {
				return this.handleCreateTerminal(params);
			},
			terminalOutput: async (params) => {
				return this.handleTerminalOutput(params);
			},
			waitForTerminalExit: async (params) => {
				return this.handleWaitForTerminalExit(params);
			},
			killTerminal: async (params) => {
				return this.handleKillTerminal(params);
			},
			releaseTerminal: async (params) => {
				return this.handleReleaseTerminal(params);
			}
		}), this.createTappedStream(createNdJsonMessageStream(this.options.agentCommand, input, output)));
		connection.signal.addEventListener("abort", () => {
			this.recordAgentExit("connection_close", child.exitCode ?? null, child.signalCode ?? null);
		}, { once: true });
		const startupFailure = this.createStartupFailureWatcher(child, startupStderr);
		try {
			const initResult = await Promise.race([(async () => {
				const initializePromise = connection.initialize({
					protocolVersion: PROTOCOL_VERSION,
					clientCapabilities: {
						fs: {
							readTextFile: true,
							writeTextFile: true
						},
						terminal: true
					},
					clientInfo: {
						name: "acpx",
						version: "0.1.0"
					}
				});
				const initialized = geminiAcp ? await withTimeout(initializePromise, resolveGeminiAcpStartupTimeoutMs()) : await initializePromise;
				await this.authenticateIfRequired(connection, initialized.authMethods ?? []);
				return initialized;
			})(), startupFailure.promise]);
			startupFailure.dispose();
			this.connection = connection;
			this.agent = child;
			this.initResult = initResult;
			this.log(`initialized protocol version ${initResult.protocolVersion}`);
		} catch (error) {
			startupFailure.dispose();
			try {
				child.kill();
			} catch {}
			if (geminiAcp && error instanceof TimeoutError) throw new GeminiAcpStartupTimeoutError(await buildGeminiAcpStartupTimeoutMessage(spawnCommand), {
				cause: error,
				retryable: true
			});
			throw error;
		}
	}
	createTappedStream(base) {
		const onAcpMessage = () => this.eventHandlers.onAcpMessage;
		const onAcpOutputMessage = () => this.eventHandlers.onAcpOutputMessage;
		const shouldSuppressInboundReplaySessionUpdate = (message) => {
			return this.suppressReplaySessionUpdateMessages && isSessionUpdateNotification(message);
		};
		return {
			readable: new ReadableStream({ async start(controller) {
				const reader = base.readable.getReader();
				try {
					while (true) {
						const { value, done } = await reader.read();
						if (done) break;
						if (!value) continue;
						if (!shouldSuppressInboundReplaySessionUpdate(value)) {
							onAcpOutputMessage()?.("inbound", value);
							onAcpMessage()?.("inbound", value);
						}
						controller.enqueue(value);
					}
				} finally {
					reader.releaseLock();
					controller.close();
				}
			} }),
			writable: new WritableStream({ async write(message) {
				onAcpOutputMessage()?.("outbound", message);
				onAcpMessage()?.("outbound", message);
				const writer = base.writable.getWriter();
				try {
					await writer.write(message);
				} finally {
					writer.releaseLock();
				}
			} })
		};
	}
	async createSession(cwd = this.options.cwd) {
		const connection = this.getConnection();
		const { command, args } = splitCommandLine(this.options.agentCommand);
		const claudeAcp = isClaudeAcpCommand(command, args);
		let result;
		try {
			const createPromise = this.runConnectionRequest(() => connection.newSession({
				cwd: asAbsoluteCwd(cwd),
				mcpServers: this.options.mcpServers ?? [],
				_meta: buildClaudeCodeOptionsMeta(this.options.sessionOptions)
			}));
			result = claudeAcp ? await withTimeout(createPromise, resolveClaudeAcpSessionCreateTimeoutMs()) : await createPromise;
		} catch (error) {
			if (claudeAcp && error instanceof TimeoutError) throw new ClaudeAcpSessionCreateTimeoutError(buildClaudeAcpSessionCreateTimeoutMessage(), {
				cause: error,
				retryable: true
			});
			throw error;
		}
		this.loadedSessionId = result.sessionId;
		return {
			sessionId: result.sessionId,
			agentSessionId: extractRuntimeSessionId(result._meta),
			models: result.models ?? void 0
		};
	}
	async loadSession(sessionId, cwd = this.options.cwd) {
		this.getConnection();
		return await this.loadSessionWithOptions(sessionId, cwd, {});
	}
	async loadSessionWithOptions(sessionId, cwd = this.options.cwd, options = {}) {
		const connection = this.getConnection();
		const previousSuppression = this.suppressSessionUpdates;
		const previousReplaySuppression = this.suppressReplaySessionUpdateMessages;
		this.suppressSessionUpdates = previousSuppression || Boolean(options.suppressReplayUpdates);
		this.suppressReplaySessionUpdateMessages = previousReplaySuppression || Boolean(options.suppressReplayUpdates);
		let response;
		try {
			response = await this.runConnectionRequest(() => connection.loadSession({
				sessionId,
				cwd: asAbsoluteCwd(cwd),
				mcpServers: this.options.mcpServers ?? []
			}));
			await this.waitForSessionUpdateDrain(options.replayIdleMs ?? REPLAY_IDLE_MS, options.replayDrainTimeoutMs ?? REPLAY_DRAIN_TIMEOUT_MS);
		} finally {
			this.suppressSessionUpdates = previousSuppression;
			this.suppressReplaySessionUpdateMessages = previousReplaySuppression;
		}
		this.loadedSessionId = sessionId;
		return {
			agentSessionId: extractRuntimeSessionId(response?._meta),
			models: response?.models ?? void 0
		};
	}
	async prompt(sessionId, prompt) {
		const connection = this.getConnection();
		const restoreConsoleError = this.options.suppressSdkConsoleErrors ? installSdkConsoleErrorSuppression() : void 0;
		let promptPromise;
		try {
			promptPromise = this.runConnectionRequest(() => connection.prompt({
				sessionId,
				prompt: typeof prompt === "string" ? textPrompt(prompt) : prompt
			}));
		} catch (error) {
			restoreConsoleError?.();
			throw error;
		}
		this.activePrompt = {
			sessionId,
			promise: promptPromise
		};
		try {
			const response = await promptPromise;
			const permissionFailure = this.consumePromptPermissionFailure(sessionId);
			if (permissionFailure) throw permissionFailure;
			return response;
		} catch (error) {
			const permissionFailure = this.consumePromptPermissionFailure(sessionId);
			if (permissionFailure) throw permissionFailure;
			throw error;
		} finally {
			restoreConsoleError?.();
			if (this.activePrompt?.promise === promptPromise) this.activePrompt = void 0;
			this.cancellingSessionIds.delete(sessionId);
			this.promptPermissionFailures.delete(sessionId);
		}
	}
	async setSessionMode(sessionId, modeId) {
		const connection = this.getConnection();
		try {
			await this.runConnectionRequest(() => connection.setSessionMode({
				sessionId,
				modeId
			}));
		} catch (error) {
			throw maybeWrapSessionControlError("session/set_mode", error, `for mode "${modeId}"`);
		}
	}
	async setSessionConfigOption(sessionId, configId, value) {
		const connection = this.getConnection();
		try {
			return await this.runConnectionRequest(() => connection.setSessionConfigOption({
				sessionId,
				configId,
				value
			}));
		} catch (error) {
			throw maybeWrapSessionControlError("session/set_config_option", error, `for "${configId}"="${value}"`);
		}
	}
	async setSessionModel(sessionId, modelId) {
		const connection = this.getConnection();
		try {
			await this.runConnectionRequest(() => connection.unstable_setSessionModel({
				sessionId,
				modelId
			}));
		} catch (error) {
			const wrapped = maybeWrapSessionControlError("session/set_model", error, `for model "${modelId}"`);
			if (wrapped !== error) throw wrapped;
			const acp = extractAcpError(error);
			const summary = acp ? formatSessionControlAcpSummary(acp) : error instanceof Error ? error.message : String(error);
			if (error instanceof Error) throw new Error(`Failed session/set_model for model "${modelId}": ${summary}`, { cause: error });
			throw new Error(`Failed session/set_model for model "${modelId}": ${summary}`, { cause: error });
		}
	}
	async cancel(sessionId) {
		const connection = this.getConnection();
		this.cancellingSessionIds.add(sessionId);
		await this.runConnectionRequest(() => connection.cancel({ sessionId }));
	}
	async closeSession(sessionId) {
		const connection = this.getConnection();
		await this.runConnectionRequest(() => connection.unstable_closeSession({ sessionId }));
		if (this.loadedSessionId === sessionId) this.loadedSessionId = void 0;
	}
	async requestCancelActivePrompt() {
		const active = this.activePrompt;
		if (!active) return false;
		await this.cancel(active.sessionId);
		return true;
	}
	async cancelActivePrompt(waitMs = 2500) {
		const active = this.activePrompt;
		if (!active) return;
		try {
			await this.cancel(active.sessionId);
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			this.log(`failed to send session/cancel: ${message}`);
		}
		if (waitMs <= 0) return;
		let timer;
		const timeoutPromise = new Promise((resolve) => {
			timer = setTimeout(resolve, waitMs);
		});
		try {
			return await Promise.race([active.promise.then((response) => response, () => void 0), timeoutPromise]);
		} finally {
			if (timer) clearTimeout(timer);
		}
	}
	async close() {
		this.closing = true;
		await this.terminalManager.shutdown();
		const agent = this.agent;
		if (agent) await this.terminateAgentProcess(agent);
		if (this.pendingConnectionRequests.size > 0) this.rejectPendingConnectionRequests(this.lastAgentExit ? new AgentDisconnectedError(this.lastAgentExit.reason, this.lastAgentExit.exitCode, this.lastAgentExit.signal, { outputAlreadyEmitted: Boolean(this.activePrompt) }) : new AgentDisconnectedError("connection_close", null, null, { outputAlreadyEmitted: Boolean(this.activePrompt) }));
		this.sessionUpdateChain = Promise.resolve();
		this.observedSessionUpdates = 0;
		this.processedSessionUpdates = 0;
		this.suppressSessionUpdates = false;
		this.suppressReplaySessionUpdateMessages = false;
		this.activePrompt = void 0;
		this.cancellingSessionIds.clear();
		this.promptPermissionFailures.clear();
		this.loadedSessionId = void 0;
		this.initResult = void 0;
		this.connection = void 0;
		this.agent = void 0;
	}
	async terminateAgentProcess(child) {
		const stdinCloseGraceMs = resolveAgentCloseAfterStdinEndMs(this.options.agentCommand);
		if (!child.stdin.destroyed) try {
			child.stdin.end();
		} catch {}
		let exited = await waitForChildExit(child, stdinCloseGraceMs);
		if (!exited && isChildProcessRunning(child)) {
			try {
				child.kill("SIGTERM");
			} catch {}
			exited = await waitForChildExit(child, AGENT_CLOSE_TERM_GRACE_MS);
		}
		if (!exited && isChildProcessRunning(child)) {
			this.log(`agent did not exit after ${AGENT_CLOSE_TERM_GRACE_MS}ms; forcing SIGKILL`);
			try {
				child.kill("SIGKILL");
			} catch {}
			exited = await waitForChildExit(child, AGENT_CLOSE_KILL_GRACE_MS);
		}
		this.detachAgentHandles(child, !exited);
	}
	detachAgentHandles(agent, unref) {
		const stdin = agent.stdin;
		const stdout = agent.stdout;
		const stderr = agent.stderr;
		stdin?.destroy();
		stdout?.destroy();
		stderr?.destroy();
		if (unref) try {
			agent.unref();
		} catch {}
	}
	getConnection() {
		if (!this.connection) throw new Error("ACP client not started");
		return this.connection;
	}
	log(message) {
		if (!this.options.verbose) return;
		process.stderr.write(`[acpx] ${message}\n`);
	}
	captureStartupStderr(target, chunk) {
		const text = typeof chunk === "string" ? chunk : chunk.toString("utf8");
		if (text.length === 0) return;
		target.push(text);
		if (target.join("").length - STARTUP_STDERR_MAX_CHARS <= 0) return;
		const joined = target.join("");
		target.splice(0, target.length, joined.slice(-STARTUP_STDERR_MAX_CHARS));
	}
	summarizeStartupStderr(target) {
		const joined = target.join("").trim();
		if (!joined) return;
		return joined.replace(/\s+/gu, " ").trim().slice(0, STARTUP_STDERR_MAX_CHARS);
	}
	createStartupFailureWatcher(child, startupStderr) {
		let settled = false;
		let rejectPromise;
		const cleanup = () => {
			child.off("error", onError);
			child.off("exit", onExit);
			child.off("close", onClose);
		};
		const finish = (error) => {
			if (settled) return;
			settled = true;
			cleanup();
			if (error) rejectPromise(error);
		};
		const createError = (params) => new AgentStartupError({
			agentCommand: this.options.agentCommand,
			exitCode: params?.exitCode ?? child.exitCode ?? null,
			signal: params?.signal ?? child.signalCode ?? null,
			stderrSummary: this.summarizeStartupStderr(startupStderr),
			cause: params?.cause
		});
		const onError = (error) => {
			finish(createError({ cause: error }));
		};
		const onExit = (exitCode, signal) => {
			finish(createError({
				exitCode,
				signal
			}));
		};
		const onClose = (exitCode, signal) => {
			finish(createError({
				exitCode,
				signal
			}));
		};
		return {
			promise: new Promise((_resolve, reject) => {
				rejectPromise = reject;
				child.once("error", onError);
				child.once("exit", onExit);
				child.once("close", onClose);
			}),
			dispose: () => finish()
		};
	}
	selectAuthMethod(methods) {
		for (const method of methods) {
			const envCredential = readEnvCredential(method.id);
			if (envCredential) return {
				methodId: method.id,
				credential: envCredential,
				source: "env"
			};
			const configCredential = resolveConfiguredAuthCredential(method.id, this.options.authCredentials);
			if (typeof configCredential === "string" && configCredential.trim().length > 0) return {
				methodId: method.id,
				credential: configCredential,
				source: "config"
			};
		}
	}
	async authenticateIfRequired(connection, methods) {
		if (methods.length === 0) return;
		const selected = this.selectAuthMethod(methods);
		if (!selected) {
			if (this.options.authPolicy === "fail") throw new AuthPolicyError(`agent advertised auth methods [${methods.map((m) => m.id).join(", ")}] but no matching credentials found`);
			this.log(`agent advertised auth methods [${methods.map((m) => m.id).join(", ")}] but no matching credentials found — skipping (agent may handle auth internally)`);
			return;
		}
		await connection.authenticate({ methodId: selected.methodId });
		this.log(`authenticated with method ${selected.methodId} (${selected.source})`);
	}
	async handlePermissionRequest(params) {
		if (this.cancellingSessionIds.has(params.sessionId)) return { outcome: { outcome: "cancelled" } };
		let response;
		try {
			response = await resolvePermissionRequest(params, this.options.permissionMode, this.options.nonInteractivePermissions ?? "deny");
		} catch (error) {
			if (error instanceof PermissionPromptUnavailableError) {
				this.notePromptPermissionFailure(params.sessionId, error);
				this.recordPermissionDecision("cancelled");
				return { outcome: { outcome: "cancelled" } };
			}
			throw error;
		}
		const decision = classifyPermissionDecision(params, response);
		this.recordPermissionDecision(decision);
		return response;
	}
	attachAgentLifecycleObservers(child) {
		child.once("exit", (exitCode, signal) => {
			this.recordAgentExit("process_exit", exitCode, signal);
		});
		child.once("close", (exitCode, signal) => {
			this.recordAgentExit("process_close", exitCode, signal);
		});
		child.stdout.once("close", () => {
			this.recordAgentExit("pipe_close", child.exitCode ?? null, child.signalCode ?? null);
		});
	}
	recordAgentExit(reason, exitCode, signal) {
		if (this.lastAgentExit) return;
		this.lastAgentExit = {
			exitCode,
			signal,
			exitedAt: isoNow$1(),
			reason,
			unexpectedDuringPrompt: !this.closing && Boolean(this.activePrompt)
		};
		this.rejectPendingConnectionRequests(new AgentDisconnectedError(reason, exitCode, signal, { outputAlreadyEmitted: Boolean(this.activePrompt) }));
	}
	notePromptPermissionFailure(sessionId, error) {
		if (!this.promptPermissionFailures.has(sessionId)) this.promptPermissionFailures.set(sessionId, error);
	}
	consumePromptPermissionFailure(sessionId) {
		const error = this.promptPermissionFailures.get(sessionId);
		if (error) this.promptPermissionFailures.delete(sessionId);
		return error;
	}
	async runConnectionRequest(run) {
		return await new Promise((resolve, reject) => {
			const pending = {
				settled: false,
				reject
			};
			const finish = (cb) => {
				if (pending.settled) return;
				pending.settled = true;
				this.pendingConnectionRequests.delete(pending);
				cb();
			};
			this.pendingConnectionRequests.add(pending);
			Promise.resolve().then(run).then((value) => finish(() => resolve(value)), (error) => finish(() => reject(error)));
		});
	}
	rejectPendingConnectionRequests(error) {
		for (const pending of this.pendingConnectionRequests) {
			if (pending.settled) {
				this.pendingConnectionRequests.delete(pending);
				continue;
			}
			pending.settled = true;
			this.pendingConnectionRequests.delete(pending);
			pending.reject(error);
		}
	}
	async handleReadTextFile(params) {
		try {
			return await this.filesystem.readTextFile(params);
		} catch (error) {
			this.recordPermissionError(params.sessionId, error);
			throw error;
		}
	}
	async handleWriteTextFile(params) {
		try {
			return await this.filesystem.writeTextFile(params);
		} catch (error) {
			this.recordPermissionError(params.sessionId, error);
			throw error;
		}
	}
	async handleCreateTerminal(params) {
		try {
			return await this.terminalManager.createTerminal(params);
		} catch (error) {
			this.recordPermissionError(params.sessionId, error);
			throw error;
		}
	}
	async handleTerminalOutput(params) {
		return await this.terminalManager.terminalOutput(params);
	}
	async handleWaitForTerminalExit(params) {
		return await this.terminalManager.waitForTerminalExit(params);
	}
	async handleKillTerminal(params) {
		return await this.terminalManager.killTerminal(params);
	}
	async handleReleaseTerminal(params) {
		return await this.terminalManager.releaseTerminal(params);
	}
	recordPermissionDecision(decision) {
		this.permissionStats.requested += 1;
		if (decision === "approved") {
			this.permissionStats.approved += 1;
			return;
		}
		if (decision === "denied") {
			this.permissionStats.denied += 1;
			return;
		}
		this.permissionStats.cancelled += 1;
	}
	recordPermissionError(sessionId, error) {
		if (error instanceof PermissionPromptUnavailableError) {
			this.notePromptPermissionFailure(sessionId, error);
			this.recordPermissionDecision("cancelled");
			return;
		}
		if (error instanceof PermissionDeniedError) this.recordPermissionDecision("denied");
	}
	async handleSessionUpdate(notification) {
		const sequence = ++this.observedSessionUpdates;
		this.sessionUpdateChain = this.sessionUpdateChain.then(async () => {
			try {
				if (!this.suppressSessionUpdates) this.eventHandlers.onSessionUpdate?.(notification);
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error);
				this.log(`session update handler failed: ${message}`);
			} finally {
				this.processedSessionUpdates = sequence;
			}
		});
		await this.sessionUpdateChain;
	}
	async waitForSessionUpdateDrain(idleMs, timeoutMs) {
		const normalizedIdleMs = Math.max(0, idleMs);
		const normalizedTimeoutMs = Math.max(normalizedIdleMs, timeoutMs);
		const deadline = Date.now() + normalizedTimeoutMs;
		let lastObserved = this.observedSessionUpdates;
		let idleSince = Date.now();
		while (Date.now() <= deadline) {
			const observed = this.observedSessionUpdates;
			if (observed !== lastObserved) {
				lastObserved = observed;
				idleSince = Date.now();
			}
			if (this.processedSessionUpdates === this.observedSessionUpdates && Date.now() - idleSince >= normalizedIdleMs) {
				await this.sessionUpdateChain;
				if (this.processedSessionUpdates === this.observedSessionUpdates) return;
			}
			await new Promise((resolve) => {
				setTimeout(resolve, DRAIN_POLL_INTERVAL_MS);
			});
		}
		throw new Error(`Timed out waiting for session replay drain after ${normalizedTimeoutMs}ms`);
	}
	async waitForSessionUpdatesIdle(options) {
		await this.waitForSessionUpdateDrain(options?.idleMs ?? 0, options?.timeoutMs ?? 0);
	}
};
//#endregion
//#region src/session/conversation-model.ts
const MAX_RUNTIME_MESSAGES = 200;
const MAX_RUNTIME_AGENT_TEXT_CHARS = 8e3;
const MAX_RUNTIME_THINKING_CHARS = 4e3;
const MAX_RUNTIME_TOOL_IO_CHARS = 4e3;
const MAX_RUNTIME_REQUEST_TOKEN_USAGE = 100;
function isoNow() {
	return (/* @__PURE__ */ new Date()).toISOString();
}
function deepClone(value) {
	try {
		return structuredClone(value);
	} catch {
		return value;
	}
}
function hasOwn(source, key) {
	return Object.prototype.hasOwnProperty.call(source, key);
}
function normalizeAgentName(value) {
	if (typeof value !== "string") return;
	const trimmed = value.trim();
	return trimmed.length > 0 ? trimmed : void 0;
}
function extractText(content) {
	if (content.type === "text") return content.text;
	if (content.type === "resource_link") return content.title ?? content.name ?? content.uri;
	if (content.type === "resource") {
		if ("text" in content.resource && typeof content.resource.text === "string") return content.resource.text;
		return content.resource.uri;
	}
}
function contentToUserContent(content) {
	if (content.type === "text") return { Text: content.text };
	if (content.type === "resource_link") {
		const value = content.title ?? content.name ?? content.uri;
		return { Mention: {
			uri: content.uri,
			content: value
		} };
	}
	if (content.type === "resource") {
		if ("text" in content.resource && typeof content.resource.text === "string") return { Text: content.resource.text };
		return { Mention: {
			uri: content.resource.uri,
			content: content.resource.uri
		} };
	}
	if (content.type === "image") return { Image: {
		source: content.data,
		size: null
	} };
}
function nextUserMessageId() {
	return randomUUID();
}
function isUserMessage(message) {
	return typeof message === "object" && message !== null && hasOwn(message, "User");
}
function isAgentMessage(message) {
	return typeof message === "object" && message !== null && hasOwn(message, "Agent");
}
function isAgentTextContent(content) {
	return hasOwn(content, "Text");
}
function isAgentThinkingContent(content) {
	return hasOwn(content, "Thinking");
}
function isAgentToolUseContent(content) {
	return hasOwn(content, "ToolUse");
}
function updateConversationTimestamp(conversation, timestamp) {
	conversation.updated_at = timestamp;
}
function ensureAgentMessage(conversation) {
	const last = conversation.messages.at(-1);
	if (last && isAgentMessage(last)) return last.Agent;
	const created = {
		content: [],
		tool_results: {}
	};
	conversation.messages.push({ Agent: created });
	return created;
}
function appendAgentText(agent, text) {
	if (!text.trim()) return;
	const last = agent.content.at(-1);
	if (last && isAgentTextContent(last)) {
		last.Text = trimRuntimeText(`${last.Text}${text}`, MAX_RUNTIME_AGENT_TEXT_CHARS);
		return;
	}
	const next = { Text: text };
	agent.content.push(next);
}
function appendAgentThinking(agent, text) {
	if (!text.trim()) return;
	const last = agent.content.at(-1);
	if (last && isAgentThinkingContent(last)) {
		last.Thinking.text = trimRuntimeText(`${last.Thinking.text}${text}`, MAX_RUNTIME_THINKING_CHARS);
		return;
	}
	const next = { Thinking: {
		text,
		signature: null
	} };
	agent.content.push(next);
}
function trimRuntimeText(value, maxChars) {
	if (value.length <= maxChars) return value;
	return `${value.slice(0, Math.max(0, maxChars - 3))}...`;
}
function statusIndicatesComplete(status) {
	if (typeof status !== "string") return false;
	const normalized = status.toLowerCase();
	return normalized.includes("complete") || normalized.includes("done") || normalized.includes("success") || normalized.includes("failed") || normalized.includes("error") || normalized.includes("cancel");
}
function statusIndicatesError(status) {
	if (typeof status !== "string") return false;
	const normalized = status.toLowerCase();
	return normalized.includes("fail") || normalized.includes("error");
}
function toToolResultContent(value) {
	if (typeof value === "string") return { Text: trimRuntimeText(value, MAX_RUNTIME_TOOL_IO_CHARS) };
	if (value != null) try {
		return { Text: trimRuntimeText(JSON.stringify(value), MAX_RUNTIME_TOOL_IO_CHARS) };
	} catch {
		return { Text: "[Unserializable value]" };
	}
	return { Text: "" };
}
function toRawInput(value) {
	if (typeof value === "string") return trimRuntimeText(value, MAX_RUNTIME_TOOL_IO_CHARS);
	try {
		return trimRuntimeText(JSON.stringify(value ?? {}), MAX_RUNTIME_TOOL_IO_CHARS);
	} catch {
		return value == null ? "" : "[Unserializable input]";
	}
}
function ensureToolUseContent(agent, toolCallId) {
	for (const content of agent.content) if (isAgentToolUseContent(content) && content.ToolUse.id === toolCallId) return content.ToolUse;
	const created = {
		id: toolCallId,
		name: "tool_call",
		raw_input: "{}",
		input: {},
		is_input_complete: false,
		thought_signature: null
	};
	agent.content.push({ ToolUse: created });
	return created;
}
function upsertToolResult(agent, toolCallId, patch) {
	const existing = agent.tool_results[toolCallId];
	const next = {
		tool_use_id: toolCallId,
		tool_name: patch.tool_name ?? existing?.tool_name ?? "tool_call",
		is_error: patch.is_error ?? existing?.is_error ?? false,
		content: patch.content ?? existing?.content ?? { Text: "" },
		output: patch.output ?? existing?.output
	};
	agent.tool_results[toolCallId] = next;
}
function applyToolCallUpdate(agent, update) {
	const tool = ensureToolUseContent(agent, update.toolCallId);
	if (hasOwn(update, "title")) tool.name = normalizeAgentName(update.title) ?? tool.name ?? "tool_call";
	if (hasOwn(update, "kind")) {
		const kindName = normalizeAgentName(update.kind);
		if (!tool.name || tool.name === "tool_call") tool.name = kindName ?? tool.name;
	}
	if (hasOwn(update, "rawInput")) {
		const rawInput = deepClone(update.rawInput);
		tool.input = rawInput ?? {};
		tool.raw_input = toRawInput(rawInput);
	}
	if (hasOwn(update, "status")) tool.is_input_complete = statusIndicatesComplete(update.status);
	if (hasOwn(update, "rawOutput") || hasOwn(update, "status") || hasOwn(update, "title") || hasOwn(update, "kind")) {
		const status = update.status;
		const output = hasOwn(update, "rawOutput") ? deepClone(update.rawOutput) : void 0;
		upsertToolResult(agent, update.toolCallId, {
			tool_name: tool.name,
			is_error: statusIndicatesError(status),
			content: output === void 0 ? void 0 : toToolResultContent(output),
			output
		});
	}
}
function asRecord(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function numberField(source, keys) {
	for (const key of keys) {
		const value = source[key];
		if (typeof value === "number" && Number.isFinite(value) && value >= 0) return value;
	}
}
function usageToTokenUsage(update) {
	const updateRecord = asRecord(update);
	const usageMeta = asRecord(updateRecord?._meta)?.usage;
	const source = asRecord(usageMeta) ?? updateRecord;
	if (!source) return;
	const normalized = {
		input_tokens: numberField(source, ["input_tokens", "inputTokens"]),
		output_tokens: numberField(source, ["output_tokens", "outputTokens"]),
		cache_creation_input_tokens: numberField(source, [
			"cache_creation_input_tokens",
			"cacheCreationInputTokens",
			"cachedWriteTokens"
		]),
		cache_read_input_tokens: numberField(source, [
			"cache_read_input_tokens",
			"cacheReadInputTokens",
			"cachedReadTokens"
		])
	};
	if (normalized.input_tokens === void 0 && normalized.output_tokens === void 0 && normalized.cache_creation_input_tokens === void 0 && normalized.cache_read_input_tokens === void 0) return;
	return normalized;
}
function ensureAcpxState$1(state) {
	return state ?? {};
}
function lastUserMessageId(conversation) {
	for (let index = conversation.messages.length - 1; index >= 0; index -= 1) {
		const message = conversation.messages[index];
		if (message && isUserMessage(message)) return message.User.id;
	}
}
function createSessionConversation(timestamp = isoNow()) {
	return {
		title: null,
		messages: [],
		updated_at: timestamp,
		cumulative_token_usage: {},
		request_token_usage: {}
	};
}
function cloneSessionConversation(conversation) {
	if (!conversation) return createSessionConversation();
	return {
		title: conversation.title,
		messages: deepClone(conversation.messages ?? []),
		updated_at: conversation.updated_at,
		cumulative_token_usage: deepClone(conversation.cumulative_token_usage ?? {}),
		request_token_usage: deepClone(conversation.request_token_usage ?? {})
	};
}
function cloneSessionAcpxState(state) {
	if (!state) return;
	return {
		current_mode_id: state.current_mode_id,
		desired_mode_id: state.desired_mode_id,
		current_model_id: state.current_model_id,
		available_models: state.available_models ? [...state.available_models] : void 0,
		available_commands: state.available_commands ? [...state.available_commands] : void 0,
		config_options: state.config_options ? deepClone(state.config_options) : void 0,
		session_options: state.session_options ? {
			model: state.session_options.model,
			allowed_tools: state.session_options.allowed_tools ? [...state.session_options.allowed_tools] : void 0,
			max_turns: state.session_options.max_turns
		} : void 0
	};
}
function recordPromptSubmission(conversation, prompt, timestamp = isoNow()) {
	const userContent = (typeof prompt === "string" ? textPrompt(prompt) : prompt).map((content) => contentToUserContent(content)).filter((content) => content !== void 0);
	if (userContent.length === 0) return;
	const promptMessageId = nextUserMessageId();
	conversation.messages.push({ User: {
		id: promptMessageId,
		content: userContent.map((content) => {
			if ("Text" in content) return { Text: trimRuntimeText(content.Text, MAX_RUNTIME_AGENT_TEXT_CHARS) };
			return content;
		})
	} });
	updateConversationTimestamp(conversation, timestamp);
	trimConversationForRuntime(conversation);
	return promptMessageId;
}
function agentMessageHasObservedReply(message) {
	return message.content.length > 0 || Object.keys(message.tool_results).length > 0;
}
function hasAgentReplyAfterPrompt(conversation, promptMessageId) {
	let sawPrompt = false;
	for (const message of conversation.messages) {
		if (!sawPrompt) {
			if (isUserMessage(message) && message.User.id === promptMessageId) sawPrompt = true;
			continue;
		}
		if (isAgentMessage(message) && agentMessageHasObservedReply(message.Agent)) return true;
	}
	return false;
}
function recordSessionUpdate(conversation, state, notification, timestamp = isoNow()) {
	const acpx = ensureAcpxState$1(state);
	const update = notification.update;
	switch (update.sessionUpdate) {
		case "user_message_chunk": {
			const userContent = contentToUserContent(update.content);
			if (userContent) conversation.messages.push({ User: {
				id: nextUserMessageId(),
				content: [userContent]
			} });
			break;
		}
		case "agent_message_chunk": {
			const text = extractText(update.content);
			if (text) appendAgentText(ensureAgentMessage(conversation), text);
			break;
		}
		case "agent_thought_chunk": {
			const text = extractText(update.content);
			if (text) appendAgentThinking(ensureAgentMessage(conversation), text);
			break;
		}
		case "tool_call":
		case "tool_call_update":
			applyToolCallUpdate(ensureAgentMessage(conversation), update);
			break;
		case "usage_update": {
			const usage = usageToTokenUsage(update);
			if (usage) {
				conversation.cumulative_token_usage = usage;
				const userId = lastUserMessageId(conversation);
				if (userId) conversation.request_token_usage[userId] = usage;
			}
			break;
		}
		case "session_info_update":
			if (hasOwn(update, "title")) conversation.title = update.title ?? null;
			if (hasOwn(update, "updatedAt")) conversation.updated_at = update.updatedAt ?? conversation.updated_at;
			break;
		case "available_commands_update":
			acpx.available_commands = update.availableCommands.map((entry) => entry.name).filter((entry) => typeof entry === "string" && entry.trim().length > 0);
			break;
		case "current_mode_update":
			acpx.current_mode_id = update.currentModeId;
			break;
		case "config_option_update":
			acpx.config_options = deepClone(update.configOptions);
			break;
		default: break;
	}
	updateConversationTimestamp(conversation, timestamp);
	trimConversationForRuntime(conversation);
	return acpx;
}
function recordClientOperation(conversation, state, operation, timestamp = isoNow()) {
	const acpx = ensureAcpxState$1(state);
	updateConversationTimestamp(conversation, timestamp);
	trimConversationForRuntime(conversation);
	return acpx;
}
function trimConversationForRuntime(conversation) {
	if (conversation.messages.length > MAX_RUNTIME_MESSAGES) conversation.messages = conversation.messages.slice(-MAX_RUNTIME_MESSAGES);
	for (const message of conversation.messages) {
		if (!isAgentMessage(message)) {
			if (isUserMessage(message)) message.User.content = message.User.content.map((content) => {
				if ("Text" in content) return { Text: trimRuntimeText(content.Text, MAX_RUNTIME_AGENT_TEXT_CHARS) };
				return content;
			});
			continue;
		}
		for (const content of message.Agent.content) if ("Text" in content) content.Text = trimRuntimeText(content.Text, MAX_RUNTIME_AGENT_TEXT_CHARS);
		else if ("Thinking" in content) content.Thinking.text = trimRuntimeText(content.Thinking.text, MAX_RUNTIME_THINKING_CHARS);
		else if ("ToolUse" in content) content.ToolUse.raw_input = trimRuntimeText(content.ToolUse.raw_input, MAX_RUNTIME_TOOL_IO_CHARS);
		for (const result of Object.values(message.Agent.tool_results)) {
			if ("Text" in result.content) result.content.Text = trimRuntimeText(result.content.Text, MAX_RUNTIME_TOOL_IO_CHARS);
			if (typeof result.output === "string") result.output = trimRuntimeText(result.output, MAX_RUNTIME_TOOL_IO_CHARS);
		}
	}
	const requestUsageEntries = Object.entries(conversation.request_token_usage);
	if (requestUsageEntries.length > MAX_RUNTIME_REQUEST_TOKEN_USAGE) conversation.request_token_usage = Object.fromEntries(requestUsageEntries.slice(-MAX_RUNTIME_REQUEST_TOKEN_USAGE));
}
//#endregion
//#region src/session/mode-preference.ts
function ensureAcpxState(state) {
	return state ?? {};
}
function normalizeModeId(modeId) {
	if (typeof modeId !== "string") return;
	const trimmed = modeId.trim();
	return trimmed.length > 0 ? trimmed : void 0;
}
function normalizeModelId(modelId) {
	if (typeof modelId !== "string") return;
	const trimmed = modelId.trim();
	return trimmed.length > 0 ? trimmed : void 0;
}
function getDesiredModeId(state) {
	return normalizeModeId(state?.desired_mode_id);
}
function setDesiredModeId(record, modeId) {
	const acpx = ensureAcpxState(record.acpx);
	const normalized = normalizeModeId(modeId);
	if (normalized) acpx.desired_mode_id = normalized;
	else delete acpx.desired_mode_id;
	record.acpx = acpx;
}
function getDesiredModelId(state) {
	return normalizeModelId(state?.session_options?.model);
}
function setDesiredModelId(record, modelId) {
	const acpx = ensureAcpxState(record.acpx);
	const normalized = normalizeModelId(modelId);
	const sessionOptions = { ...acpx.session_options };
	if (normalized) sessionOptions.model = normalized;
	else delete sessionOptions.model;
	if (typeof sessionOptions.model === "string" || Array.isArray(sessionOptions.allowed_tools) || typeof sessionOptions.max_turns === "number") acpx.session_options = sessionOptions;
	else delete acpx.session_options;
	record.acpx = acpx;
}
function setCurrentModelId(record, modelId) {
	const acpx = ensureAcpxState(record.acpx);
	const normalized = normalizeModelId(modelId);
	if (normalized) acpx.current_model_id = normalized;
	else delete acpx.current_model_id;
	record.acpx = acpx;
}
function syncAdvertisedModelState(record, models) {
	if (!models) return;
	const acpx = ensureAcpxState(record.acpx);
	acpx.current_model_id = models.currentModelId;
	acpx.available_models = models.availableModels.map((model) => model.modelId);
	record.acpx = acpx;
}
//#endregion
//#region src/runtime/engine/lifecycle.ts
function applyLifecycleSnapshotToRecord(record, snapshot) {
	if (!snapshot) return;
	record.pid = snapshot.pid;
	record.agentStartedAt = snapshot.startedAt;
	if (snapshot.lastExit) {
		record.lastAgentExitCode = snapshot.lastExit.exitCode;
		record.lastAgentExitSignal = snapshot.lastExit.signal;
		record.lastAgentExitAt = snapshot.lastExit.exitedAt;
		record.lastAgentDisconnectReason = snapshot.lastExit.reason;
		return;
	}
	record.lastAgentExitCode = void 0;
	record.lastAgentExitSignal = void 0;
	record.lastAgentExitAt = void 0;
	record.lastAgentDisconnectReason = void 0;
}
function reconcileAgentSessionId(record, agentSessionId) {
	const normalized = normalizeRuntimeSessionId(agentSessionId);
	if (!normalized) return;
	record.agentSessionId = normalized;
}
function sessionHasAgentMessages(recordOrConversation) {
	return recordOrConversation.messages.some((message) => typeof message === "object" && message !== null && "Agent" in message);
}
function applyConversation(record, conversation) {
	record.title = conversation.title;
	record.updated_at = conversation.updated_at;
	record.messages = conversation.messages;
	record.cumulative_token_usage = conversation.cumulative_token_usage;
	record.request_token_usage = conversation.request_token_usage;
}
//#endregion
//#region src/runtime/engine/reconnect.ts
function isProcessAlive(pid) {
	if (!pid || !Number.isInteger(pid) || pid <= 0 || pid === process.pid) return false;
	try {
		process.kill(pid, 0);
		return true;
	} catch {
		return false;
	}
}
const SESSION_LOAD_UNSUPPORTED_CODES = new Set([-32601, -32602]);
function shouldFallbackToNewSession(error, record) {
	if (error instanceof TimeoutError || error instanceof InterruptedError) return false;
	if (isAcpResourceNotFoundError(error)) return true;
	const acp = extractAcpError(error);
	if (acp && SESSION_LOAD_UNSUPPORTED_CODES.has(acp.code)) return true;
	if (!sessionHasAgentMessages(record)) {
		if (isAcpQueryClosedBeforeResponseError(error)) return true;
		if (acp?.code === -32603) return true;
	}
	return false;
}
function requiresSameSession(resumePolicy) {
	return resumePolicy === "same-session-only";
}
function makeSessionResumeRequiredError(params) {
	return new SessionResumeRequiredError(`Persistent ACP session ${params.record.acpSessionId} could not be resumed: ${params.reason}`, { cause: params.cause instanceof Error ? params.cause : void 0 });
}
async function replayDesiredMode(params) {
	if (!params.desiredModeId) return;
	try {
		await withTimeout(params.client.setSessionMode(params.sessionId, params.desiredModeId), params.timeoutMs);
		if (params.verbose) process.stderr.write(`[acpx] replayed desired mode ${params.desiredModeId} on fresh ACP session ${params.sessionId} (previous ${params.previousSessionId})\n`);
	} catch (error) {
		throw new SessionModeReplayError(`Failed to replay saved session mode ${params.desiredModeId} on fresh ACP session ${params.sessionId}: ${formatErrorMessage(error)}`, {
			cause: error instanceof Error ? error : void 0,
			retryable: true
		});
	}
}
async function replayDesiredModel(params) {
	if (!params.desiredModelId || !params.models) return;
	if (params.models.currentModelId === params.desiredModelId) return;
	try {
		await withTimeout(params.client.setSessionModel(params.sessionId, params.desiredModelId), params.timeoutMs);
		if (params.verbose) process.stderr.write(`[acpx] replayed desired model ${params.desiredModelId} on fresh ACP session ${params.sessionId} (previous ${params.previousSessionId})\n`);
	} catch (error) {
		throw new SessionModelReplayError(`Failed to replay saved session model ${params.desiredModelId} on fresh ACP session ${params.sessionId}: ${formatErrorMessage(error)}`, {
			cause: error instanceof Error ? error : void 0,
			retryable: true
		});
	}
}
function restoreOriginalSessionState(params) {
	params.record.acpSessionId = params.sessionId;
	params.record.agentSessionId = params.agentSessionId;
}
async function connectAndLoadSession(options) {
	const record = options.record;
	const client = options.client;
	const sameSessionOnly = requiresSameSession(options.resumePolicy);
	const originalSessionId = record.acpSessionId;
	const originalAgentSessionId = record.agentSessionId;
	const desiredModeId = getDesiredModeId(record.acpx);
	const desiredModelId = getDesiredModelId(record.acpx);
	const storedProcessAlive = isProcessAlive(record.pid);
	const shouldReconnect = Boolean(record.pid) && !storedProcessAlive;
	if (options.verbose) {
		if (storedProcessAlive) process.stderr.write(`[acpx] saved session pid ${record.pid} is running; reconnecting with loadSession\n`);
		else if (shouldReconnect) process.stderr.write(`[acpx] saved session pid ${record.pid} is dead; respawning agent and attempting session/load\n`);
	}
	const reusingLoadedSession = client.hasReusableSession(record.acpSessionId);
	if (reusingLoadedSession) incrementPerfCounter("runtime.connect_and_load.reused_session");
	else await withTimeout(client.start(), options.timeoutMs);
	options.onClientAvailable?.(options.activeController);
	applyLifecycleSnapshotToRecord(record, client.getAgentLifecycleSnapshot());
	record.closed = false;
	record.closedAt = void 0;
	options.onConnectedRecord?.(record);
	let resumed = false;
	let loadError;
	let sessionId = record.acpSessionId;
	let createdFreshSession = false;
	let pendingAgentSessionId = record.agentSessionId;
	let sessionModels;
	if (reusingLoadedSession) resumed = true;
	else if (client.supportsLoadSession()) try {
		const loadResult = await withTimeout(client.loadSessionWithOptions(record.acpSessionId, record.cwd, { suppressReplayUpdates: true }), options.timeoutMs);
		reconcileAgentSessionId(record, loadResult.agentSessionId);
		sessionModels = loadResult.models;
		resumed = true;
	} catch (error) {
		loadError = formatErrorMessage(error);
		if (sameSessionOnly) throw makeSessionResumeRequiredError({
			record,
			reason: loadError,
			cause: error
		});
		if (!shouldFallbackToNewSession(error, record)) throw error;
		const createdSession = await withTimeout(client.createSession(record.cwd), options.timeoutMs);
		sessionId = createdSession.sessionId;
		createdFreshSession = true;
		pendingAgentSessionId = createdSession.agentSessionId;
		sessionModels = createdSession.models;
	}
	else {
		if (sameSessionOnly) throw makeSessionResumeRequiredError({
			record,
			reason: "agent does not support session/load"
		});
		const createdSession = await withTimeout(client.createSession(record.cwd), options.timeoutMs);
		sessionId = createdSession.sessionId;
		createdFreshSession = true;
		pendingAgentSessionId = createdSession.agentSessionId;
		sessionModels = createdSession.models;
	}
	if (createdFreshSession) {
		try {
			await replayDesiredMode({
				client,
				sessionId,
				desiredModeId,
				previousSessionId: originalSessionId,
				timeoutMs: options.timeoutMs,
				verbose: options.verbose
			});
			await replayDesiredModel({
				client,
				sessionId,
				desiredModelId,
				previousSessionId: originalSessionId,
				models: sessionModels,
				timeoutMs: options.timeoutMs,
				verbose: options.verbose
			});
		} catch (error) {
			restoreOriginalSessionState({
				record,
				sessionId: originalSessionId,
				agentSessionId: originalAgentSessionId
			});
			if (options.verbose) process.stderr.write(`[acpx] ${formatErrorMessage(error)}\n`);
			throw error;
		}
		record.acpSessionId = sessionId;
		reconcileAgentSessionId(record, pendingAgentSessionId);
	}
	syncAdvertisedModelState(record, sessionModels);
	if (createdFreshSession && desiredModelId && sessionModels) setCurrentModelId(record, desiredModelId);
	options.onSessionIdResolved?.(sessionId);
	return {
		sessionId,
		agentSessionId: record.agentSessionId,
		resumed,
		loadError
	};
}
//#endregion
//#region src/runtime/engine/session-options.ts
function sessionOptionsFromRecord(record) {
	const stored = record.acpx?.session_options;
	if (!stored) return;
	const sessionOptions = {};
	if (typeof stored.model === "string" && stored.model.trim().length > 0) sessionOptions.model = stored.model;
	if (Array.isArray(stored.allowed_tools)) sessionOptions.allowedTools = [...stored.allowed_tools];
	if (typeof stored.max_turns === "number") sessionOptions.maxTurns = stored.max_turns;
	return Object.keys(sessionOptions).length > 0 ? sessionOptions : void 0;
}
//#endregion
//#region src/runtime/engine/connected-session.ts
function createActiveSessionController(params) {
	const getActiveSessionId = () => params.getActiveSessionId();
	return {
		hasActivePrompt: () => params.client.hasActivePrompt(),
		requestCancelActivePrompt: async () => await params.client.requestCancelActivePrompt(),
		setSessionMode: async (modeId) => {
			await params.client.setSessionMode(getActiveSessionId(), modeId);
		},
		setSessionModel: async (modelId) => {
			await params.client.setSessionModel(getActiveSessionId(), modelId);
		},
		setSessionConfigOption: async (configId, value) => {
			return await params.client.setSessionConfigOption(getActiveSessionId(), configId, value);
		}
	};
}
async function withConnectedSession(options) {
	const record = await options.loadRecord(options.sessionRecordId);
	const client = options.createClient?.({
		agentCommand: record.agentCommand,
		cwd: absolutePath(record.cwd),
		mcpServers: options.mcpServers,
		permissionMode: options.permissionMode ?? "approve-reads",
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		verbose: options.verbose,
		sessionOptions: sessionOptionsFromRecord(record)
	}) ?? new AcpClient({
		agentCommand: record.agentCommand,
		cwd: absolutePath(record.cwd),
		mcpServers: options.mcpServers,
		permissionMode: options.permissionMode ?? "approve-reads",
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		verbose: options.verbose,
		sessionOptions: sessionOptionsFromRecord(record)
	});
	let activeSessionIdForControl = record.acpSessionId;
	let notifiedClientAvailable = false;
	const activeController = createActiveSessionController({
		client,
		getActiveSessionId: () => activeSessionIdForControl
	});
	try {
		return await withInterrupt(async () => {
			const { sessionId, resumed, loadError } = await connectAndLoadSession({
				client,
				record,
				resumePolicy: options.resumePolicy,
				timeoutMs: options.timeoutMs,
				verbose: options.verbose,
				activeController,
				onClientAvailable: (controller) => {
					options.onClientAvailable?.(controller);
					notifiedClientAvailable = true;
				},
				onConnectedRecord: options.onConnectedRecord,
				onSessionIdResolved: (sessionIdValue) => {
					activeSessionIdForControl = sessionIdValue;
				}
			});
			const value = await options.run({
				record,
				client,
				activeController,
				sessionId,
				resumed,
				loadError
			});
			record.lastUsedAt = isoNow$2();
			record.closed = false;
			record.closedAt = void 0;
			record.protocolVersion = client.initializeResult?.protocolVersion;
			record.agentCapabilities = client.initializeResult?.agentCapabilities;
			applyLifecycleSnapshotToRecord(record, client.getAgentLifecycleSnapshot());
			await options.saveRecord(record);
			return {
				value,
				record,
				resumed,
				loadError
			};
		}, async () => {
			if (options.onInterrupt) await options.onInterrupt({
				client,
				record
			});
			else await client.cancelActivePrompt(2500);
			applyLifecycleSnapshotToRecord(record, client.getAgentLifecycleSnapshot());
			record.lastUsedAt = isoNow$2();
			await options.saveRecord(record).catch(() => {});
			await client.close();
		});
	} finally {
		if (notifiedClientAvailable) options.onClientClosed?.();
		await client.close();
		applyLifecycleSnapshotToRecord(record, client.getAgentLifecycleSnapshot());
		await options.saveRecord(record).catch(() => {});
	}
}
//#endregion
//#region src/runtime/engine/prompt-turn.ts
const SESSION_REPLY_IDLE_MS = 1e3;
const SESSION_REPLY_DRAIN_TIMEOUT_MS = 5e3;
async function runPromptTurn(params) {
	try {
		const promptPromise = params.client.prompt(params.sessionId, params.prompt);
		await params.onPromptStarted?.();
		return {
			stopReason: (await withTimeout(promptPromise, params.timeoutMs)).stopReason,
			source: "rpc"
		};
	} catch (error) {
		if (!(error instanceof TimeoutError) || !params.promptMessageId) throw error;
		await params.client.waitForSessionUpdatesIdle?.({
			idleMs: SESSION_REPLY_IDLE_MS,
			timeoutMs: SESSION_REPLY_DRAIN_TIMEOUT_MS
		}).catch(() => {});
		if (hasAgentReplyAfterPrompt(params.conversation, params.promptMessageId)) return {
			stopReason: "end_turn",
			source: "session"
		};
		throw error;
	}
}
//#endregion
export { resolveSessionRecord as A, serializeSessionRecordForDisk as B, findGitRepositoryRoot as C, listSessions as D, isoNow$2 as E, sessionBaseDir$1 as F, DEFAULT_AGENT_NAME as G, TimeoutError as H, sessionEventActivePath as I, resolveAgentCommand as J, listBuiltInAgents as K, sessionEventLockPath as L, parseSessionRecord as M, DEFAULT_EVENT_SEGMENT_MAX_BYTES as N, listSessionsForAgent as O, defaultSessionEventLog as P, sessionEventSegmentPath as R, absolutePath as S, findSessionByDirectoryWalk as T, withInterrupt as U, InterruptedError as V, withTimeout as W, recordSessionUpdate as _, applyConversation as a, permissionModeSatisfies as b, setCurrentModelId as c, syncAdvertisedModelState as d, cloneSessionAcpxState as f, recordPromptSubmission as g, recordClientOperation as h, connectAndLoadSession as i, writeSessionRecord as j, normalizeName as k, setDesiredModeId as l, createSessionConversation as m, withConnectedSession as n, applyLifecycleSnapshotToRecord as o, cloneSessionConversation as p, normalizeAgentName$1 as q, sessionOptionsFromRecord as r, reconcileAgentSessionId as s, runPromptTurn as t, setDesiredModelId as u, trimConversationForRuntime as v, findSession as w, DEFAULT_HISTORY_LIMIT as x, AcpClient as y, assertPersistedKeyPolicy as z };

//# sourceMappingURL=prompt-turn-CXMtXBl-.js.map