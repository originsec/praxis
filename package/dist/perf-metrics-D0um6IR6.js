//#region src/errors.ts
var AcpxOperationalError = class extends Error {
	outputCode;
	detailCode;
	origin;
	retryable;
	acp;
	outputAlreadyEmitted;
	constructor(message, options) {
		super(message, options);
		this.name = new.target.name;
		this.outputCode = options?.outputCode;
		this.detailCode = options?.detailCode;
		this.origin = options?.origin;
		this.retryable = options?.retryable;
		this.acp = options?.acp;
		this.outputAlreadyEmitted = options?.outputAlreadyEmitted;
	}
};
var SessionNotFoundError = class extends AcpxOperationalError {
	sessionId;
	constructor(sessionId) {
		super(`Session not found: ${sessionId}`);
		this.sessionId = sessionId;
	}
};
var SessionResolutionError = class extends AcpxOperationalError {};
var AgentSpawnError = class extends AcpxOperationalError {
	agentCommand;
	constructor(agentCommand, cause) {
		super(`Failed to spawn agent command: ${agentCommand}`, { cause: cause instanceof Error ? cause : void 0 });
		this.agentCommand = agentCommand;
	}
};
var AgentStartupError = class extends AcpxOperationalError {
	agentCommand;
	exitCode;
	signal;
	stderrSummary;
	constructor(params) {
		const exitSummary = `exit=${params.exitCode ?? "null"}, signal=${params.signal ?? "null"}`;
		const stderrSuffix = typeof params.stderrSummary === "string" && params.stderrSummary.trim().length > 0 ? `: ${params.stderrSummary.trim()}` : "";
		super(`ACP agent exited before initialize completed (${exitSummary})${stderrSuffix}`, {
			cause: params.cause instanceof Error ? params.cause : void 0,
			outputCode: "RUNTIME",
			detailCode: "AGENT_STARTUP_FAILED",
			origin: "acp"
		});
		this.agentCommand = params.agentCommand;
		this.exitCode = params.exitCode;
		this.signal = params.signal;
		this.stderrSummary = params.stderrSummary?.trim() || void 0;
	}
};
var AgentDisconnectedError = class extends AcpxOperationalError {
	reason;
	exitCode;
	signal;
	constructor(reason, exitCode, signal, options) {
		super(`ACP agent disconnected during request (${reason}, exit=${exitCode ?? "null"}, signal=${signal ?? "null"})`, {
			outputCode: "RUNTIME",
			detailCode: "AGENT_DISCONNECTED",
			origin: "acp",
			...options
		});
		this.reason = reason;
		this.exitCode = exitCode;
		this.signal = signal;
	}
};
var SessionResumeRequiredError = class extends AcpxOperationalError {
	constructor(message, options) {
		super(message, {
			outputCode: "RUNTIME",
			detailCode: "SESSION_RESUME_REQUIRED",
			origin: "acp",
			retryable: true,
			...options
		});
	}
};
var GeminiAcpStartupTimeoutError = class extends AcpxOperationalError {
	constructor(message, options) {
		super(message, {
			outputCode: "TIMEOUT",
			detailCode: "GEMINI_ACP_STARTUP_TIMEOUT",
			origin: "acp",
			...options
		});
	}
};
var SessionModeReplayError = class extends AcpxOperationalError {
	constructor(message, options) {
		super(message, {
			outputCode: "RUNTIME",
			detailCode: "SESSION_MODE_REPLAY_FAILED",
			origin: "acp",
			...options
		});
	}
};
var SessionModelReplayError = class extends AcpxOperationalError {
	constructor(message, options) {
		super(message, {
			outputCode: "RUNTIME",
			detailCode: "SESSION_MODEL_REPLAY_FAILED",
			origin: "acp",
			...options
		});
	}
};
var ClaudeAcpSessionCreateTimeoutError = class extends AcpxOperationalError {
	constructor(message, options) {
		super(message, {
			outputCode: "TIMEOUT",
			detailCode: "CLAUDE_ACP_SESSION_CREATE_TIMEOUT",
			origin: "acp",
			...options
		});
	}
};
var CopilotAcpUnsupportedError = class extends AcpxOperationalError {
	constructor(message, options) {
		super(message, {
			outputCode: "RUNTIME",
			detailCode: "COPILOT_ACP_UNSUPPORTED",
			origin: "acp",
			...options
		});
	}
};
var AuthPolicyError = class extends AcpxOperationalError {
	constructor(message, options) {
		super(message, {
			outputCode: "RUNTIME",
			detailCode: "AUTH_REQUIRED",
			origin: "acp",
			...options
		});
	}
};
var QueueConnectionError = class extends AcpxOperationalError {};
var QueueProtocolError = class extends AcpxOperationalError {};
var PermissionDeniedError = class extends AcpxOperationalError {};
var PermissionPromptUnavailableError = class extends AcpxOperationalError {
	constructor() {
		super("Permission prompt unavailable in non-interactive mode");
	}
};
//#endregion
//#region src/types.ts
const EXIT_CODES = {
	SUCCESS: 0,
	ERROR: 1,
	USAGE: 2,
	TIMEOUT: 3,
	NO_SESSION: 4,
	PERMISSION_DENIED: 5,
	INTERRUPTED: 130
};
const OUTPUT_FORMATS = [
	"text",
	"json",
	"quiet"
];
const PERMISSION_MODES = [
	"approve-all",
	"approve-reads",
	"deny-all"
];
const AUTH_POLICIES = ["skip", "fail"];
const NON_INTERACTIVE_PERMISSION_POLICIES = ["deny", "fail"];
const OUTPUT_ERROR_CODES = [
	"NO_SESSION",
	"TIMEOUT",
	"PERMISSION_DENIED",
	"PERMISSION_PROMPT_UNAVAILABLE",
	"RUNTIME",
	"USAGE"
];
const OUTPUT_ERROR_ORIGINS = [
	"cli",
	"runtime",
	"queue",
	"acp"
];
const SESSION_RECORD_SCHEMA = "acpx.session.v1";
//#endregion
//#region src/acp/error-shapes.ts
const RESOURCE_NOT_FOUND_ACP_CODES = new Set([-32001, -32002]);
function asRecord$2(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function toAcpErrorPayload(value) {
	const record = asRecord$2(value);
	if (!record) return;
	if (typeof record.code !== "number" || !Number.isFinite(record.code)) return;
	if (typeof record.message !== "string" || record.message.length === 0) return;
	return {
		code: record.code,
		message: record.message,
		data: record.data
	};
}
function extractAcpErrorInternal(value, depth) {
	if (depth > 5) return;
	const direct = toAcpErrorPayload(value);
	if (direct) return direct;
	const record = asRecord$2(value);
	if (!record) return;
	if ("error" in record) {
		const nested = extractAcpErrorInternal(record.error, depth + 1);
		if (nested) return nested;
	}
	if ("acp" in record) {
		const nested = extractAcpErrorInternal(record.acp, depth + 1);
		if (nested) return nested;
	}
	if ("cause" in record) {
		const nested = extractAcpErrorInternal(record.cause, depth + 1);
		if (nested) return nested;
	}
}
function formatUnknownErrorMessage(error) {
	if (error instanceof Error) return error.message;
	if (error && typeof error === "object") {
		const maybeMessage = error.message;
		if (typeof maybeMessage === "string" && maybeMessage.length > 0) return maybeMessage;
		try {
			return JSON.stringify(error);
		} catch {}
	}
	return String(error);
}
const SESSION_NOT_FOUND_PATTERN = /session\s+["'\w-]+\s+not found/i;
function isSessionNotFoundText(value) {
	if (typeof value !== "string") return false;
	const normalized = value.toLowerCase();
	return normalized.includes("resource_not_found") || normalized.includes("resource not found") || normalized.includes("session not found") || normalized.includes("unknown session") || normalized.includes("invalid session identifier") || SESSION_NOT_FOUND_PATTERN.test(value);
}
function hasSessionNotFoundHint(value, depth = 0) {
	if (depth > 4) return false;
	if (isSessionNotFoundText(value)) return true;
	if (Array.isArray(value)) return value.some((entry) => hasSessionNotFoundHint(entry, depth + 1));
	const record = asRecord$2(value);
	if (!record) return false;
	return Object.values(record).some((entry) => hasSessionNotFoundHint(entry, depth + 1));
}
function extractAcpError(error) {
	return extractAcpErrorInternal(error, 0);
}
function isAcpResourceNotFoundError(error) {
	const acp = extractAcpError(error);
	if (acp && RESOURCE_NOT_FOUND_ACP_CODES.has(acp.code)) return true;
	if (acp) {
		if (isSessionNotFoundText(acp.message)) return true;
		if (hasSessionNotFoundHint(acp.data)) return true;
	}
	return isSessionNotFoundText(formatUnknownErrorMessage(error));
}
//#endregion
//#region src/acp/error-normalization.ts
const AUTH_REQUIRED_ACP_CODES = new Set([-32e3]);
const QUERY_CLOSED_BEFORE_RESPONSE_DETAIL = "query closed before response received";
function asRecord$1(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function isAuthRequiredMessage(value) {
	if (!value) return false;
	const normalized = value.toLowerCase();
	return normalized.includes("auth required") || normalized.includes("authentication required") || normalized.includes("authorization required") || normalized.includes("credential required") || normalized.includes("credentials required") || normalized.includes("token required") || normalized.includes("login required");
}
function isAcpAuthRequiredPayload(acp) {
	if (!acp) return false;
	if (!AUTH_REQUIRED_ACP_CODES.has(acp.code)) return false;
	if (isAuthRequiredMessage(acp.message)) return true;
	const data = asRecord$1(acp.data);
	if (!data) return false;
	if (data.authRequired === true) return true;
	const methodId = data.methodId;
	if (typeof methodId === "string" && methodId.trim().length > 0) return true;
	const methods = data.methods;
	if (Array.isArray(methods) && methods.length > 0) return true;
	return false;
}
function isOutputErrorCode(value) {
	return typeof value === "string" && OUTPUT_ERROR_CODES.includes(value);
}
function isOutputErrorOrigin(value) {
	return typeof value === "string" && OUTPUT_ERROR_ORIGINS.includes(value);
}
function readOutputErrorMeta(error) {
	const record = asRecord$1(error);
	if (!record) return {};
	return {
		outputCode: isOutputErrorCode(record.outputCode) ? record.outputCode : void 0,
		detailCode: typeof record.detailCode === "string" && record.detailCode.trim().length > 0 ? record.detailCode : void 0,
		origin: isOutputErrorOrigin(record.origin) ? record.origin : void 0,
		retryable: typeof record.retryable === "boolean" ? record.retryable : void 0,
		acp: extractAcpError(record.acp)
	};
}
function isTimeoutLike(error) {
	return error instanceof Error && error.name === "TimeoutError";
}
function isNoSessionLike(error) {
	return error instanceof Error && error.name === "NoSessionError";
}
function isUsageLike(error) {
	if (!(error instanceof Error)) return false;
	return error.name === "CommanderError" || error.name === "InvalidArgumentError" || asRecord$1(error)?.code === "commander.invalidArgument";
}
function formatErrorMessage(error) {
	return formatUnknownErrorMessage(error);
}
function isAcpQueryClosedBeforeResponseError(error) {
	const acp = extractAcpError(error);
	if (!acp || acp.code !== -32603) return false;
	const details = asRecord$1(acp.data)?.details;
	if (typeof details !== "string") return false;
	return details.toLowerCase().includes(QUERY_CLOSED_BEFORE_RESPONSE_DETAIL);
}
function mapErrorCode(error) {
	if (error instanceof PermissionPromptUnavailableError) return "PERMISSION_PROMPT_UNAVAILABLE";
	if (error instanceof PermissionDeniedError) return "PERMISSION_DENIED";
	if (isTimeoutLike(error)) return "TIMEOUT";
	if (isNoSessionLike(error) || isAcpResourceNotFoundError(error)) return "NO_SESSION";
	if (isUsageLike(error)) return "USAGE";
}
function normalizeOutputError(error, options = {}) {
	const meta = readOutputErrorMeta(error);
	let code = mapErrorCode(error) ?? options.defaultCode ?? "RUNTIME";
	if (meta.outputCode) code = meta.outputCode;
	if (code === "RUNTIME" && isAcpResourceNotFoundError(error)) code = "NO_SESSION";
	const acp = options.acp ?? meta.acp ?? extractAcpError(error);
	const detailCode = meta.detailCode ?? options.detailCode ?? (error instanceof AuthPolicyError || isAcpAuthRequiredPayload(acp) ? "AUTH_REQUIRED" : void 0);
	return {
		code,
		message: formatErrorMessage(error),
		detailCode,
		origin: meta.origin ?? options.origin,
		retryable: meta.retryable ?? options.retryable,
		acp
	};
}
/**
* Returns true when an error from `client.prompt()` looks transient and
* can reasonably be retried (e.g. model-API 400/500, network hiccups that
* surface as ACP internal errors).
*
* Errors that are definitively non-recoverable (auth, missing session,
* invalid params, timeout, permission) return false.
*/
function isRetryablePromptError(error) {
	if (error instanceof PermissionDeniedError || error instanceof PermissionPromptUnavailableError) return false;
	if (isTimeoutLike(error) || isNoSessionLike(error) || isUsageLike(error)) return false;
	const acp = extractAcpError(error);
	if (!acp) return false;
	if (acp.code === -32001 || acp.code === -32002) return false;
	if (isAcpAuthRequiredPayload(acp)) return false;
	if (acp.code === -32601 || acp.code === -32602) return false;
	return acp.code === -32603 || acp.code === -32700;
}
function exitCodeForOutputErrorCode(code) {
	switch (code) {
		case "USAGE": return EXIT_CODES.USAGE;
		case "TIMEOUT": return EXIT_CODES.TIMEOUT;
		case "NO_SESSION": return EXIT_CODES.NO_SESSION;
		case "PERMISSION_DENIED":
		case "PERMISSION_PROMPT_UNAVAILABLE": return EXIT_CODES.PERMISSION_DENIED;
		default: return EXIT_CODES.ERROR;
	}
}
//#endregion
//#region src/prompt-content.ts
var PromptInputValidationError = class extends Error {
	constructor(message) {
		super(message);
		this.name = "PromptInputValidationError";
	}
};
function asRecord(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function isNonEmptyString(value) {
	return typeof value === "string" && value.trim().length > 0;
}
function isBase64Data(value) {
	if (value.length === 0 || value.length % 4 !== 0) return false;
	return /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value);
}
function isImageMimeType(value) {
	return /^image\/[A-Za-z0-9.+-]+$/i.test(value);
}
function isTextBlock(value) {
	const record = asRecord(value);
	return record?.type === "text" && typeof record.text === "string";
}
function isImageBlock(value) {
	const record = asRecord(value);
	return record?.type === "image" && isNonEmptyString(record.mimeType) && isImageMimeType(record.mimeType) && typeof record.data === "string" && isBase64Data(record.data);
}
function isResourceLinkBlock(value) {
	const record = asRecord(value);
	return record?.type === "resource_link" && isNonEmptyString(record.uri) && (record.title === void 0 || typeof record.title === "string") && (record.name === void 0 || typeof record.name === "string");
}
function isResourcePayload(value) {
	const record = asRecord(value);
	if (!record || !isNonEmptyString(record.uri)) return false;
	return record.text === void 0 || typeof record.text === "string";
}
function isResourceBlock(value) {
	const record = asRecord(value);
	return record?.type === "resource" && isResourcePayload(record.resource);
}
function isContentBlock(value) {
	return isTextBlock(value) || isImageBlock(value) || isResourceLinkBlock(value) || isResourceBlock(value);
}
function getContentBlockValidationError(value, index) {
	const record = asRecord(value);
	if (!record || typeof record.type !== "string") return `prompt[${index}] must be an ACP content block object`;
	switch (record.type) {
		case "text": return typeof record.text === "string" ? void 0 : `prompt[${index}] text block must include a string text field`;
		case "image":
			if (!isNonEmptyString(record.mimeType)) return `prompt[${index}] image block must include a non-empty mimeType`;
			if (!isImageMimeType(record.mimeType)) return `prompt[${index}] image block mimeType must start with image/`;
			if (typeof record.data !== "string" || record.data.length === 0) return `prompt[${index}] image block must include non-empty base64 data`;
			if (!isBase64Data(record.data)) return `prompt[${index}] image block data must be valid base64`;
			return;
		case "resource_link":
			if (!isNonEmptyString(record.uri)) return `prompt[${index}] resource_link block must include a non-empty uri`;
			if (record.title !== void 0 && typeof record.title !== "string") return `prompt[${index}] resource_link block title must be a string when present`;
			if (record.name !== void 0 && typeof record.name !== "string") return `prompt[${index}] resource_link block name must be a string when present`;
			return;
		case "resource":
			if (!asRecord(record.resource)) return `prompt[${index}] resource block must include a resource object`;
			if (!isResourcePayload(record.resource)) return `prompt[${index}] resource block resource must include a non-empty uri and optional text`;
			return;
		default: return `prompt[${index}] has unsupported content block type ${JSON.stringify(record.type)}`;
	}
}
function isPromptInput(value) {
	return Array.isArray(value) && value.every((entry) => isContentBlock(entry));
}
function textPrompt(text) {
	return [{
		type: "text",
		text
	}];
}
function parseStructuredPrompt(source) {
	if (!source.startsWith("[")) return;
	try {
		const parsed = JSON.parse(source);
		if (isPromptInput(parsed)) return parsed;
		if (Array.isArray(parsed)) throw new PromptInputValidationError(parsed.map((entry, index) => getContentBlockValidationError(entry, index)).find((message) => message !== void 0) ?? "Structured prompt JSON must be an array of valid ACP content blocks");
		return;
	} catch (error) {
		if (error instanceof PromptInputValidationError) throw error;
		return;
	}
}
function parsePromptSource(source) {
	const trimmed = source.trim();
	const structured = parseStructuredPrompt(trimmed);
	if (structured) return structured;
	if (!trimmed) return [];
	return textPrompt(trimmed);
}
function mergePromptSourceWithText(source, suffixText) {
	const prompt = parsePromptSource(source);
	const appended = suffixText.trim();
	if (!appended) return prompt;
	if (prompt.length === 0) return textPrompt(appended);
	return [...prompt, ...textPrompt(appended)];
}
function promptToDisplayText(prompt) {
	return prompt.map((block) => {
		switch (block.type) {
			case "text": return block.text;
			case "resource_link": return block.title ?? block.name ?? block.uri;
			case "resource": return "text" in block.resource && typeof block.resource.text === "string" ? block.resource.text : block.resource.uri;
			case "image": return `[image] ${block.mimeType}`;
			default: return "";
		}
	}).filter((entry) => entry.trim().length > 0).join("\n\n").trim();
}
//#endregion
//#region src/acp/agent-session-id.ts
const AGENT_SESSION_ID_META_KEYS = ["agentSessionId", "sessionId"];
function normalizeAgentSessionId(value) {
	if (typeof value !== "string") return;
	const trimmed = value.trim();
	return trimmed.length > 0 ? trimmed : void 0;
}
function asMetaRecord(meta) {
	if (!meta || typeof meta !== "object" || Array.isArray(meta)) return;
	return meta;
}
function extractAgentSessionId(meta) {
	const record = asMetaRecord(meta);
	if (!record) return;
	for (const key of AGENT_SESSION_ID_META_KEYS) {
		const normalized = normalizeAgentSessionId(record[key]);
		if (normalized) return normalized;
	}
}
//#endregion
//#region src/session/runtime-session-id.ts
function normalizeRuntimeSessionId(value) {
	return normalizeAgentSessionId(value);
}
function extractRuntimeSessionId(meta) {
	return extractAgentSessionId(meta);
}
//#endregion
//#region src/perf-metrics.ts
const counters = /* @__PURE__ */ new Map();
const gauges = /* @__PURE__ */ new Map();
const timings = /* @__PURE__ */ new Map();
function hrNow() {
	return process.hrtime.bigint();
}
function durationMs(start) {
	return Number(process.hrtime.bigint() - start) / 1e6;
}
function roundMetric(value) {
	return Number(value.toFixed(3));
}
function incrementPerfCounter(name, delta = 1) {
	counters.set(name, (counters.get(name) ?? 0) + delta);
}
function setPerfGauge(name, value) {
	gauges.set(name, value);
}
function recordPerfDuration(name, durationMsValue) {
	const next = timings.get(name) ?? {
		count: 0,
		totalMs: 0,
		maxMs: 0
	};
	next.count += 1;
	next.totalMs += durationMsValue;
	next.maxMs = Math.max(next.maxMs, durationMsValue);
	timings.set(name, next);
}
async function measurePerf(name, run) {
	const startedAt = hrNow();
	try {
		return await run();
	} finally {
		recordPerfDuration(name, durationMs(startedAt));
	}
}
function startPerfTimer(name) {
	const startedAt = hrNow();
	return () => {
		const elapsedMs = durationMs(startedAt);
		recordPerfDuration(name, elapsedMs);
		return elapsedMs;
	};
}
function getPerfMetricsSnapshot() {
	return {
		counters: Object.fromEntries(counters.entries()),
		gauges: Object.fromEntries(gauges.entries()),
		timings: Object.fromEntries([...timings.entries()].map(([name, bucket]) => [name, {
			count: bucket.count,
			totalMs: roundMetric(bucket.totalMs),
			maxMs: roundMetric(bucket.maxMs)
		}]))
	};
}
function resetPerfMetrics() {
	counters.clear();
	gauges.clear();
	timings.clear();
}
function formatPerfMetric(name, durationMsValue) {
	return `${name}=${roundMetric(durationMsValue)}ms`;
}
//#endregion
export { PERMISSION_MODES as A, PermissionPromptUnavailableError as B, isAcpResourceNotFoundError as C, OUTPUT_ERROR_CODES as D, NON_INTERACTIVE_PERMISSION_POLICIES as E, AuthPolicyError as F, SessionNotFoundError as G, QueueProtocolError as H, ClaudeAcpSessionCreateTimeoutError as I, SessionResolutionError as K, CopilotAcpUnsupportedError as L, AgentDisconnectedError as M, AgentSpawnError as N, OUTPUT_ERROR_ORIGINS as O, AgentStartupError as P, GeminiAcpStartupTimeoutError as R, extractAcpError as S, EXIT_CODES as T, SessionModeReplayError as U, QueueConnectionError as V, SessionModelReplayError as W, exitCodeForOutputErrorCode as _, recordPerfDuration as a, isRetryablePromptError as b, startPerfTimer as c, PromptInputValidationError as d, isPromptInput as f, textPrompt as g, promptToDisplayText as h, measurePerf as i, SESSION_RECORD_SCHEMA as j, OUTPUT_FORMATS as k, extractRuntimeSessionId as l, parsePromptSource as m, getPerfMetricsSnapshot as n, resetPerfMetrics as o, mergePromptSourceWithText as p, SessionResumeRequiredError as q, incrementPerfCounter as r, setPerfGauge as s, formatPerfMetric as t, normalizeRuntimeSessionId as u, formatErrorMessage as v, AUTH_POLICIES as w, normalizeOutputError as x, isAcpQueryClosedBeforeResponseError as y, PermissionDeniedError as z };

//# sourceMappingURL=perf-metrics-D0um6IR6.js.map