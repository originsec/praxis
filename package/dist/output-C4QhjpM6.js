import { t as __exportAll } from "./rolldown-runtime-CiIaOW0V.js";
import { a as parsePromptStopReason, i as parseJsonRpcErrorMessage, t as extractSessionUpdateNotification } from "./jsonrpc-DSxh2w5R.js";
//#region src/acp/jsonrpc-error.ts
const OUTPUT_ERROR_JSONRPC_CODES = {
	NO_SESSION: -32002,
	TIMEOUT: -32070,
	PERMISSION_DENIED: -32071,
	PERMISSION_PROMPT_UNAVAILABLE: -32072,
	RUNTIME: -32603,
	USAGE: -32602
};
function hasValidAcpError(acp) {
	return Boolean(acp && Number.isFinite(acp.code) && typeof acp.message === "string" && acp.message.trim().length > 0);
}
function buildFallbackData(params) {
	const data = {
		acpxCode: params.outputCode,
		detailCode: params.detailCode,
		origin: params.origin,
		retryable: params.retryable,
		timestamp: params.timestamp,
		sessionId: params.sessionId
	};
	for (const [key, value] of Object.entries(data)) if (value === void 0) delete data[key];
	return data;
}
function buildErrorObject(params) {
	if (hasValidAcpError(params.acp)) return {
		code: params.acp.code,
		message: params.acp.message,
		...params.acp.data !== void 0 ? { data: params.acp.data } : {}
	};
	const data = buildFallbackData(params);
	return {
		code: OUTPUT_ERROR_JSONRPC_CODES[params.outputCode] ?? -32603,
		message: params.message,
		...Object.keys(data).length > 0 ? { data } : {}
	};
}
function buildJsonRpcErrorResponse(params) {
	return {
		jsonrpc: "2.0",
		id: params.id ?? null,
		error: buildErrorObject(params)
	};
}
//#endregion
//#region src/cli/output/read-suppression.ts
const SUPPRESSED_READ_OUTPUT = "[read output suppressed]";
function inferToolKindFromTitle(title) {
	const normalized = title?.trim().toLowerCase();
	if (!normalized) return;
	const head = normalized.split(":", 1)[0]?.trim();
	if (!head) return;
	if (head.includes("read") || head.includes("cat") || head.includes("open") || head.includes("view")) return "read";
}
function isReadLikeTool(tool) {
	return tool.kind?.trim().toLowerCase() === "read" || inferToolKindFromTitle(tool.title) === "read";
}
//#endregion
//#region src/cli/output/json-formatter.ts
const DEFAULT_JSON_SESSION_ID = "unknown";
function asRecord$1(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function jsonRpcIdKey(value) {
	if (typeof value === "string") return `s:${value}`;
	if (typeof value === "number" && Number.isFinite(value)) return `n:${value}`;
}
function sanitizeReadResult(result) {
	const record = asRecord$1(result);
	if (!record || typeof record.content !== "string") return result;
	return {
		...record,
		content: SUPPRESSED_READ_OUTPUT
	};
}
function sanitizeToolContent(content) {
	if (!Array.isArray(content)) return content;
	return [{
		type: "content",
		content: {
			type: "text",
			text: SUPPRESSED_READ_OUTPUT
		}
	}];
}
function sanitizeToolMessage(message) {
	const root = asRecord$1(message);
	const params = asRecord$1(root?.params);
	const update = asRecord$1(params?.update);
	if (!root || !params || !update) return message;
	return {
		...root,
		params: {
			...params,
			update: {
				...update,
				rawOutput: Object.prototype.hasOwnProperty.call(update, "rawOutput") && update.rawOutput !== void 0 ? { content: SUPPRESSED_READ_OUTPUT } : update.rawOutput,
				content: Object.prototype.hasOwnProperty.call(update, "content") && update.content !== void 0 ? sanitizeToolContent(update.content) : update.content
			}
		}
	};
}
var JsonOutputFormatter = class {
	stdout;
	suppressReads;
	sessionId;
	requestMethodById = /* @__PURE__ */ new Map();
	toolStateById = /* @__PURE__ */ new Map();
	constructor(stdout, suppressReads, context) {
		this.stdout = stdout;
		this.suppressReads = suppressReads;
		this.sessionId = context?.sessionId?.trim() || DEFAULT_JSON_SESSION_ID;
	}
	setContext(context) {
		this.sessionId = context.sessionId?.trim() || this.sessionId || DEFAULT_JSON_SESSION_ID;
	}
	onAcpMessage(message) {
		this.stdout.write(`${JSON.stringify(this.sanitizeMessage(message))}\n`);
	}
	sanitizeMessage(message) {
		if (!this.suppressReads) return message;
		const sanitizedResponse = this.sanitizeReadResponse(message);
		if (sanitizedResponse !== message) return sanitizedResponse;
		const sanitizedToolMessage = this.sanitizeReadToolMessage(message);
		if (sanitizedToolMessage !== message) return sanitizedToolMessage;
		this.trackRequestMethod(message);
		return message;
	}
	trackRequestMethod(message) {
		const candidate = message;
		if (typeof candidate.method !== "string") return;
		const idKey = jsonRpcIdKey(candidate.id);
		if (!idKey) return;
		this.requestMethodById.set(idKey, candidate.method);
	}
	sanitizeReadResponse(message) {
		const candidate = message;
		const idKey = jsonRpcIdKey(candidate.id);
		if (!idKey || !Object.hasOwn(candidate, "result")) return message;
		const method = this.requestMethodById.get(idKey);
		this.requestMethodById.delete(idKey);
		if (method !== "fs/read_text_file") return message;
		const root = asRecord$1(message);
		if (!root) return message;
		return {
			...root,
			result: sanitizeReadResult(candidate.result)
		};
	}
	sanitizeReadToolMessage(message) {
		const root = asRecord$1(message);
		if (root?.method !== "session/update") return message;
		const params = asRecord$1(root.params);
		const update = asRecord$1(params?.update);
		if (!params || !update) return message;
		const sessionUpdate = update.sessionUpdate;
		if (sessionUpdate !== "tool_call" && sessionUpdate !== "tool_call_update") return message;
		const toolCallId = typeof update.toolCallId === "string" ? update.toolCallId : void 0;
		if (!toolCallId) return message;
		const previous = this.toolStateById.get(toolCallId) ?? {};
		const current = {
			title: typeof update.title === "string" ? update.title : previous.title,
			kind: typeof update.kind === "string" || update.kind === null ? update.kind : previous.kind
		};
		this.toolStateById.set(toolCallId, current);
		if (!isReadLikeTool(current)) return message;
		return sanitizeToolMessage(message);
	}
	onError(params) {
		this.stdout.write(`${JSON.stringify(buildJsonRpcErrorResponse({
			outputCode: params.code,
			detailCode: params.detailCode,
			origin: params.origin,
			message: params.message,
			retryable: params.retryable,
			timestamp: params.timestamp,
			sessionId: this.sessionId,
			acp: params.acp
		}))}\n`);
	}
	flush() {}
};
function createJsonOutputFormatter(stdout, suppressReads = false, context) {
	return new JsonOutputFormatter(stdout, suppressReads, context);
}
//#endregion
//#region src/cli/output/output.ts
var output_exports = /* @__PURE__ */ __exportAll({ createOutputFormatter: () => createOutputFormatter });
const MAX_THOUGHT_CHARS = 900;
const MAX_INLINE_CHARS = 220;
const MAX_OUTPUT_CHARS = 2e3;
const MAX_OUTPUT_LINES = 28;
const MAX_LOCATION_ITEMS = 5;
const OUTPUT_PRIORITY_KEYS = [
	"stdout",
	"stderr",
	"output",
	"content",
	"text",
	"message",
	"result",
	"response",
	"value"
];
function asStatus(status) {
	return status ?? "unknown";
}
function isFinalStatus(status) {
	return status === "completed" || status === "failed";
}
function toStatusLabel(status) {
	switch (status) {
		case "in_progress": return "running";
		case "pending": return "pending";
		case "completed": return "completed";
		case "failed": return "failed";
		default: return "running";
	}
}
function asRecord(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function extractJsonRpcMethod(message) {
	return Object.hasOwn(message, "method") ? message.method?.toString() : void 0;
}
function collapseWhitespace(value) {
	return value.replace(/\s+/g, " ").trim();
}
function normalizeLineEndings(value) {
	return value.replace(/\r\n?/g, "\n");
}
function truncate(value, maxChars) {
	if (value.length <= maxChars) return value;
	if (maxChars <= 3) return value.slice(0, maxChars);
	return `${value.slice(0, maxChars - 3)}...`;
}
function toInline(value, maxChars = MAX_INLINE_CHARS) {
	return truncate(collapseWhitespace(value), maxChars);
}
function indentBlock(value, prefix) {
	return value.split("\n").map((line) => `${prefix}${line}`).join("\n");
}
function dedupeStrings(values) {
	const seen = /* @__PURE__ */ new Set();
	const result = [];
	for (const value of values) {
		if (seen.has(value)) continue;
		seen.add(value);
		result.push(value);
	}
	return result;
}
function safeJson(value, spacing) {
	const seen = /* @__PURE__ */ new WeakSet();
	try {
		return JSON.stringify(value, (_key, entry) => {
			if (typeof entry === "bigint") return `${entry}n`;
			if (typeof entry === "function") return `[Function ${entry.name || "anonymous"}]`;
			if (typeof entry === "symbol") return entry.toString();
			if (entry && typeof entry === "object") {
				if (seen.has(entry)) return "[Circular]";
				seen.add(entry);
			}
			return entry;
		}, spacing);
	} catch {
		return;
	}
}
function readFirstString(source, keys) {
	for (const key of keys) {
		const value = source[key];
		if (typeof value === "string" && value.trim()) return value.trim();
	}
}
function readFirstStringArray(source, keys) {
	for (const key of keys) {
		const value = source[key];
		if (!Array.isArray(value)) continue;
		const entries = value.map((entry) => typeof entry === "string" ? entry.trim() : "").filter((entry) => entry.length > 0);
		if (entries.length > 0) return entries;
	}
}
function summarizeToolInput(rawInput) {
	if (rawInput == null) return;
	if (typeof rawInput === "string" || typeof rawInput === "number" || typeof rawInput === "boolean") return toInline(String(rawInput));
	const record = asRecord(rawInput);
	if (record) {
		const command = readFirstString(record, [
			"command",
			"cmd",
			"program"
		]);
		const args = readFirstStringArray(record, ["args", "arguments"]);
		if (command) return toInline([command, ...args ?? []].join(" "));
		const location = readFirstString(record, [
			"path",
			"file",
			"filePath",
			"filepath",
			"target",
			"uri",
			"url"
		]);
		if (location) return toInline(location);
		const query = readFirstString(record, [
			"query",
			"pattern",
			"text",
			"search"
		]);
		if (query) return toInline(query);
	}
	const json = safeJson(rawInput, 0);
	return json ? toInline(json) : void 0;
}
function formatLocations(locations) {
	if (!locations || locations.length === 0) return;
	const unique = /* @__PURE__ */ new Set();
	for (const location of locations) {
		const path = location.path?.trim();
		if (!path) continue;
		const line = typeof location.line === "number" && Number.isFinite(location.line) ? `:${Math.max(1, Math.trunc(location.line))}` : "";
		unique.add(`${path}${line}`);
	}
	const items = [...unique];
	if (items.length === 0) return;
	const visible = items.slice(0, MAX_LOCATION_ITEMS);
	const hidden = items.length - visible.length;
	if (hidden <= 0) return visible.join(", ");
	return `${visible.join(", ")}, +${hidden} more`;
}
function summarizeDiff(path, oldText, newText) {
	const oldLines = oldText ? oldText.split("\n").length : 0;
	const delta = newText.split("\n").length - oldLines;
	if (delta === 0) return `diff ${path} (line count unchanged)`;
	return `diff ${path} (${`${delta > 0 ? "+" : ""}${delta}`} lines)`;
}
function textFromContentBlock(content) {
	switch (content.type) {
		case "text": return content.text;
		case "resource_link": return content.title ?? content.name ?? content.uri;
		case "resource": {
			if ("text" in content.resource && typeof content.resource.text === "string") return content.resource.text;
			const uri = content.resource.uri;
			const mimeType = content.resource.mimeType;
			return `[resource] ${uri}${mimeType ? ` (${mimeType})` : ""}`;
		}
		case "image": return `[image] ${content.mimeType}`;
		case "audio": return `[audio] ${content.mimeType}`;
		default: return;
	}
}
function summarizeToolContent(content) {
	if (!content || content.length === 0) return;
	const fragments = [];
	for (const entry of content) {
		if (entry.type === "content") {
			const text = textFromContentBlock(entry.content);
			if (text && text.trim()) fragments.push(text.trimEnd());
			continue;
		}
		if (entry.type === "diff") {
			fragments.push(summarizeDiff(entry.path, entry.oldText, entry.newText));
			continue;
		}
		if (entry.type === "terminal") fragments.push(`[terminal] ${entry.terminalId}`);
	}
	const unique = dedupeStrings(fragments.map((fragment) => fragment.trim()).filter((fragment) => fragment.length > 0));
	if (unique.length === 0) return;
	return unique.join("\n\n");
}
function extractOutputText(value, depth = 0, seen = /* @__PURE__ */ new Set()) {
	if (value == null) return;
	if (typeof value === "string") {
		const trimmed = value.trimEnd();
		return trimmed.length > 0 ? trimmed : void 0;
	}
	if (typeof value === "number" || typeof value === "boolean") return String(value);
	if (depth >= 4) return;
	if (Array.isArray(value)) {
		const parts = value.map((entry) => extractOutputText(entry, depth + 1, seen)).filter((entry) => Boolean(entry));
		if (parts.length === 0) return;
		return dedupeStrings(parts).join("\n");
	}
	const record = asRecord(value);
	if (!record) return;
	if (seen.has(record)) return;
	seen.add(record);
	const preferred = [];
	for (const key of OUTPUT_PRIORITY_KEYS) {
		if (!(key in record)) continue;
		const extracted = extractOutputText(record[key], depth + 1, seen);
		if (extracted) preferred.push(extracted);
	}
	const uniquePreferred = dedupeStrings(preferred);
	if (uniquePreferred.length > 0) return uniquePreferred.join("\n");
	const json = safeJson(record, 2);
	if (!json || json === "{}") return;
	return json;
}
function summarizeToolOutput(rawOutput, content) {
	const fragments = dedupeStrings([extractOutputText(rawOutput), summarizeToolContent(content)].map((fragment) => fragment?.trim()).filter((fragment) => Boolean(fragment)));
	if (fragments.length === 0) return;
	return fragments.join("\n\n");
}
function renderToolOutput(state, suppressReads) {
	if (suppressReads && isReadLikeTool(state)) return SUPPRESSED_READ_OUTPUT;
	return summarizeToolOutput(state.rawOutput, state.content);
}
function limitOutputBlock(value) {
	const normalized = value.replace(/\r\n/g, "\n").trim();
	if (!normalized) return "";
	const lines = normalized.split("\n");
	const visible = lines.slice(0, MAX_OUTPUT_LINES);
	let result = visible.join("\n");
	if (lines.length > visible.length) {
		const hidden = lines.length - visible.length;
		result += `\n... (${hidden} more lines)`;
	}
	if (result.length > MAX_OUTPUT_CHARS) result = `${result.slice(0, MAX_OUTPUT_CHARS - 3)}...`;
	return result;
}
var TextOutputFormatter = class {
	stdout;
	useColor;
	suppressReads;
	toolStates = /* @__PURE__ */ new Map();
	thoughtBuffer = "";
	wroteAny = false;
	atLineStart = true;
	section = null;
	constructor(stdout, suppressReads) {
		this.stdout = stdout;
		this.useColor = Boolean(stdout.isTTY);
		this.suppressReads = suppressReads;
	}
	setContext(_context) {}
	onAcpMessage(message) {
		const notification = extractSessionUpdateNotification(message);
		if (notification) {
			this.renderSessionUpdate(notification);
			return;
		}
		const method = extractJsonRpcMethod(message);
		if (method && method !== "session/prompt" && method !== "session/cancel") {
			this.onClientOperation({
				method,
				status: "running",
				summary: method,
				timestamp: (/* @__PURE__ */ new Date()).toISOString()
			});
			return;
		}
		const stopReason = parsePromptStopReason(message);
		if (stopReason) {
			this.renderDone(stopReason);
			return;
		}
		const errorMessage = parseJsonRpcErrorMessage(message);
		if (errorMessage) this.onError({
			code: "RUNTIME",
			origin: "acp",
			message: errorMessage
		});
	}
	renderSessionUpdate(notification) {
		const update = notification.update;
		if (update.sessionUpdate !== "agent_thought_chunk") this.flushThoughtBuffer();
		switch (update.sessionUpdate) {
			case "agent_message_chunk":
				if (update.content.type === "text") this.writeAssistantChunk(update.content.text);
				return;
			case "agent_thought_chunk":
				if (update.content.type === "text") this.thoughtBuffer += update.content.text;
				return;
			case "tool_call":
				this.renderToolUpdate(update);
				return;
			case "tool_call_update":
				this.renderToolUpdate(update);
				return;
			case "plan":
				this.beginSection("plan");
				this.writeLine(this.bold("[plan]"));
				for (const entry of update.entries) this.writeLine(`  - [${entry.status}] ${entry.content}`);
				return;
			default: return;
		}
	}
	renderDone(stopReason) {
		this.flushThoughtBuffer();
		this.beginSection("done");
		this.writeLine(this.dim(`[done] ${stopReason}`));
	}
	onError(params) {
		this.flushThoughtBuffer();
		this.beginSection("done");
		this.writeLine(this.formatAnsi(`[error] ${params.code}: ${params.message}`, "31"));
	}
	onClientOperation(operation) {
		this.flushThoughtBuffer();
		this.beginSection("client");
		const normalizedStatus = operation.status === "completed" ? "completed" : operation.status === "failed" ? "failed" : "in_progress";
		const statusText = this.colorStatus(operation.status, normalizedStatus);
		this.writeLine(`${this.bold("[client]")} ${operation.summary} (${statusText})`);
		if (operation.details && operation.details.trim().length > 0) {
			this.writeLine("  details:");
			this.writeLine(indentBlock(operation.details, "    "));
		}
	}
	flush() {
		this.flushThoughtBuffer();
		if (!this.atLineStart) this.write("\n");
	}
	write(chunk) {
		if (!chunk) return;
		this.stdout.write(chunk);
		this.wroteAny = true;
		this.atLineStart = chunk.endsWith("\n");
	}
	writeLine(line) {
		this.write(`${line}\n`);
	}
	beginSection(next) {
		if (!this.atLineStart) this.write("\n");
		if (this.wroteAny) this.write("\n");
		this.section = next;
	}
	writeAssistantChunk(text) {
		if (!text) return;
		this.section = "assistant";
		this.write(text);
	}
	flushThoughtBuffer() {
		const thought = truncate(normalizeLineEndings(this.thoughtBuffer).trim(), MAX_THOUGHT_CHARS);
		this.thoughtBuffer = "";
		if (!thought) return;
		this.beginSection("thought");
		const [firstLine, ...restLines] = thought.split("\n");
		this.writeLine(this.dim(`[thinking] ${firstLine}`));
		for (const line of restLines) this.writeLine(this.dim(`           ${line}`));
	}
	renderToolUpdate(update) {
		const state = this.getOrCreateToolState(update.toolCallId);
		this.mergeToolState(state, update);
		const status = asStatus(state.status);
		if (isFinalStatus(status)) {
			const signature = this.toolSignature(state);
			if (signature !== state.finalSignature) {
				state.finalSignature = signature;
				this.renderFinalToolState(state, status);
			}
			return;
		}
		if (state.startedPrinted) return;
		state.startedPrinted = true;
		this.renderStartingToolState(state, status);
	}
	getOrCreateToolState(toolCallId) {
		const existing = this.toolStates.get(toolCallId);
		if (existing) return existing;
		const created = {
			id: toolCallId,
			startedPrinted: false
		};
		this.toolStates.set(toolCallId, created);
		return created;
	}
	mergeToolState(state, update) {
		if (typeof update.title === "string" && update.title.trim().length > 0) state.title = update.title;
		if (update.status !== void 0) state.status = update.status;
		if (update.kind !== void 0) state.kind = update.kind;
		if (update.locations !== void 0) state.locations = update.locations;
		if (update.rawInput !== void 0) state.rawInput = update.rawInput;
		if (update.rawOutput !== void 0) state.rawOutput = update.rawOutput;
		if (update.content !== void 0) state.content = update.content;
	}
	toolSignature(state) {
		const signaturePayload = {
			title: state.title,
			status: state.status,
			kind: state.kind,
			input: summarizeToolInput(state.rawInput),
			files: formatLocations(state.locations),
			output: renderToolOutput(state, this.suppressReads)
		};
		return safeJson(signaturePayload, 0) ?? JSON.stringify(signaturePayload);
	}
	renderStartingToolState(state, status) {
		this.beginSection("tool");
		const title = state.title ?? state.id;
		const label = status === "pending" ? "pending" : "running";
		const statusText = this.colorStatus(label, status);
		this.writeLine(`${this.bold("[tool]")} ${title} (${statusText})`);
		const input = summarizeToolInput(state.rawInput);
		if (input) this.writeLine(`  input: ${input}`);
		const files = formatLocations(state.locations);
		if (files) this.writeLine(`  files: ${files}`);
	}
	renderFinalToolState(state, status) {
		this.beginSection("tool");
		const title = state.title ?? state.id;
		const statusText = this.colorStatus(toStatusLabel(status), status);
		this.writeLine(`${this.bold("[tool]")} ${title} (${statusText})`);
		if (state.kind) this.writeLine(`  kind: ${state.kind}`);
		const input = summarizeToolInput(state.rawInput);
		if (input) this.writeLine(`  input: ${input}`);
		const files = formatLocations(state.locations);
		if (files) this.writeLine(`  files: ${files}`);
		const output = renderToolOutput(state, this.suppressReads);
		if (output) {
			this.writeLine("  output:");
			this.writeLine(indentBlock(limitOutputBlock(output), "    "));
		}
	}
	formatAnsi(text, code) {
		if (!this.useColor) return text;
		return `\u001b[${code}m${text}\u001b[0m`;
	}
	bold(text) {
		return this.formatAnsi(text, "1");
	}
	dim(text) {
		return this.formatAnsi(text, "2");
	}
	colorStatus(text, status) {
		if (!this.useColor) return text;
		switch (status) {
			case "completed": return this.formatAnsi(text, "32");
			case "failed": return this.formatAnsi(text, "31");
			default: return this.formatAnsi(text, "33");
		}
	}
};
var QuietOutputFormatter = class {
	stdout;
	chunks = [];
	flushed = false;
	constructor(stdout) {
		this.stdout = stdout;
	}
	setContext(_context) {}
	onAcpMessage(message) {
		const update = extractSessionUpdateNotification(message);
		if (update?.update.sessionUpdate === "agent_message_chunk" && update.update.content.type === "text") {
			this.chunks.push(update.update.content.text);
			return;
		}
		if (parsePromptStopReason(message)) this.flushBufferedOutput();
	}
	onError(_params) {}
	flush() {}
	flushBufferedOutput() {
		if (this.flushed) return;
		this.flushed = true;
		const text = this.chunks.join("");
		this.stdout.write(text.endsWith("\n") ? text : `${text}\n`);
	}
};
function createOutputFormatter(format, options = {}) {
	const stdout = options.stdout ?? process.stdout;
	const suppressReads = options.suppressReads === true;
	switch (format) {
		case "text": return new TextOutputFormatter(stdout, suppressReads);
		case "json": return createJsonOutputFormatter(stdout, suppressReads, options.jsonContext);
		case "quiet": return new QuietOutputFormatter(stdout);
		default: throw new Error("Unsupported output format");
	}
}
//#endregion
export { output_exports as n, createOutputFormatter as t };

//# sourceMappingURL=output-C4QhjpM6.js.map