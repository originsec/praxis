import { C as isAcpResourceNotFoundError, S as extractAcpError, g as textPrompt, x as normalizeOutputError } from "./perf-metrics-D0um6IR6.js";
import { B as serializeSessionRecordForDisk, G as DEFAULT_AGENT_NAME, J as resolveAgentCommand, K as listBuiltInAgents, M as parseSessionRecord, P as defaultSessionEventLog, W as withTimeout, _ as recordSessionUpdate, a as applyConversation, f as cloneSessionAcpxState, g as recordPromptSubmission, h as recordClientOperation, i as connectAndLoadSession, l as setDesiredModeId, m as createSessionConversation, n as withConnectedSession, o as applyLifecycleSnapshotToRecord, p as cloneSessionConversation, s as reconcileAgentSessionId, t as runPromptTurn, v as trimConversationForRuntime, y as AcpClient, z as assertPersistedKeyPolicy } from "./prompt-turn-CXMtXBl-.js";
import path from "node:path";
import fs from "node:fs/promises";
import { randomUUID } from "node:crypto";
//#region src/runtime/public/errors.ts
var AcpRuntimeError = class extends Error {
	code;
	cause;
	constructor(code, message, options) {
		super(message);
		this.name = "AcpRuntimeError";
		this.code = code;
		this.cause = options?.cause;
	}
};
function isAcpRuntimeError(value) {
	return value instanceof AcpRuntimeError;
}
//#endregion
//#region src/runtime/public/shared.ts
function isRecord(value) {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}
function asTrimmedString(value) {
	return typeof value === "string" ? value.trim() : "";
}
function asString(value) {
	return typeof value === "string" ? value : void 0;
}
function asOptionalString(value) {
	return asTrimmedString(value) || void 0;
}
function asOptionalBoolean(value) {
	return typeof value === "boolean" ? value : void 0;
}
function deriveAgentFromSessionKey(sessionKey, fallbackAgent) {
	const match = sessionKey.match(/^agent:([^:]+):/i);
	return (match?.[1] ? asTrimmedString(match[1]) : "") || fallbackAgent;
}
//#endregion
//#region src/runtime/public/events.ts
function safeParseJsonObject(line) {
	try {
		const parsed = JSON.parse(line);
		return isRecord(parsed) ? parsed : null;
	} catch {
		return null;
	}
}
function asOptionalFiniteNumber(value) {
	return typeof value === "number" && Number.isFinite(value) ? value : void 0;
}
function resolveStructuredPromptPayload(parsed) {
	if (asTrimmedString(parsed.method) === "session/update") {
		const params = parsed.params;
		if (isRecord(params) && isRecord(params.update)) {
			const update = params.update;
			const tag = asOptionalString(update.sessionUpdate);
			return {
				type: tag ?? "",
				payload: update,
				...tag ? { tag } : {}
			};
		}
	}
	const sessionUpdate = asOptionalString(parsed.sessionUpdate);
	if (sessionUpdate) return {
		type: sessionUpdate,
		payload: parsed,
		tag: sessionUpdate
	};
	const type = asTrimmedString(parsed.type);
	const tag = asOptionalString(parsed.tag);
	return {
		type,
		payload: parsed,
		...tag ? { tag } : {}
	};
}
function resolveStatusTextForTag(params) {
	const { tag, payload } = params;
	if (tag === "available_commands_update") {
		const commands = Array.isArray(payload.availableCommands) ? payload.availableCommands : [];
		return commands.length > 0 ? `available commands updated (${commands.length})` : "available commands updated";
	}
	if (tag === "current_mode_update") {
		const mode = asTrimmedString(payload.currentModeId) || asTrimmedString(payload.modeId) || asTrimmedString(payload.mode);
		return mode ? `mode updated: ${mode}` : "mode updated";
	}
	if (tag === "config_option_update") {
		const id = asTrimmedString(payload.id) || asTrimmedString(payload.configOptionId);
		const value = asTrimmedString(payload.currentValue) || asTrimmedString(payload.value) || asTrimmedString(payload.optionValue);
		if (id && value) return `config updated: ${id}=${value}`;
		if (id) return `config updated: ${id}`;
		return "config updated";
	}
	if (tag === "session_info_update") return asTrimmedString(payload.summary) || asTrimmedString(payload.message) || "session updated";
	if (tag === "plan") {
		const content = asTrimmedString((Array.isArray(payload.entries) ? payload.entries : []).find((entry) => isRecord(entry))?.content);
		return content ? `plan: ${content}` : null;
	}
	return null;
}
function resolveTextChunk(params) {
	const contentRaw = params.payload.content;
	if (isRecord(contentRaw)) {
		const contentType = asTrimmedString(contentRaw.type);
		if (contentType && contentType !== "text") return null;
		const text = asString(contentRaw.text);
		if (text && text.length > 0) return {
			type: "text_delta",
			text,
			stream: params.stream,
			tag: params.tag
		};
	}
	const text = asString(params.payload.text);
	if (!text || text.length === 0) return null;
	return {
		type: "text_delta",
		text,
		stream: params.stream,
		tag: params.tag
	};
}
function createTextDeltaEvent(params) {
	if (params.content == null || params.content.length === 0) return null;
	return {
		type: "text_delta",
		text: params.content,
		stream: params.stream,
		...params.tag ? { tag: params.tag } : {}
	};
}
function createToolCallEvent(params) {
	const title = asTrimmedString(params.payload.title) || "tool call";
	const status = asTrimmedString(params.payload.status);
	const toolCallId = asOptionalString(params.payload.toolCallId);
	return {
		type: "tool_call",
		text: status ? `${title} (${status})` : title,
		tag: params.tag,
		...toolCallId ? { toolCallId } : {},
		...status ? { status } : {},
		title
	};
}
function parsePromptEventLine(line) {
	const trimmed = line.trim();
	if (!trimmed) return null;
	const parsed = safeParseJsonObject(trimmed);
	if (!parsed) return {
		type: "status",
		text: trimmed
	};
	const structured = resolveStructuredPromptPayload(parsed);
	const type = structured.type;
	const payload = structured.payload;
	const tag = structured.tag;
	switch (type) {
		case "text": return createTextDeltaEvent({
			content: asString(payload.content),
			stream: "output",
			tag
		});
		case "thought": return createTextDeltaEvent({
			content: asString(payload.content),
			stream: "thought",
			tag
		});
		case "tool_call": return createToolCallEvent({
			payload,
			tag: tag ?? "tool_call"
		});
		case "tool_call_update": return createToolCallEvent({
			payload,
			tag: tag ?? "tool_call_update"
		});
		case "agent_message_chunk": return resolveTextChunk({
			payload,
			stream: "output",
			tag: "agent_message_chunk"
		});
		case "agent_thought_chunk": return resolveTextChunk({
			payload,
			stream: "thought",
			tag: "agent_thought_chunk"
		});
		case "usage_update": {
			const used = asOptionalFiniteNumber(payload.used);
			const size = asOptionalFiniteNumber(payload.size);
			return {
				type: "status",
				text: used != null && size != null ? `usage updated: ${used}/${size}` : "usage updated",
				tag: "usage_update",
				...used != null ? { used } : {},
				...size != null ? { size } : {}
			};
		}
		case "available_commands_update":
		case "current_mode_update":
		case "config_option_update":
		case "session_info_update":
		case "plan": {
			const text = resolveStatusTextForTag({
				tag: type,
				payload
			});
			if (!text) return null;
			return {
				type: "status",
				text,
				tag: type
			};
		}
		case "client_operation": {
			const text = [
				asTrimmedString(payload.method) || "operation",
				asTrimmedString(payload.status),
				asTrimmedString(payload.summary)
			].filter(Boolean).join(" ");
			if (!text) return null;
			return {
				type: "status",
				text,
				...tag ? { tag } : {}
			};
		}
		case "update": {
			const update = asTrimmedString(payload.update);
			if (!update) return null;
			return {
				type: "status",
				text: update,
				...tag ? { tag } : {}
			};
		}
		case "done": return {
			type: "done",
			stopReason: asOptionalString(payload.stopReason)
		};
		case "error": return {
			type: "error",
			message: asTrimmedString(payload.message) || "acpx runtime error",
			code: asOptionalString(payload.code),
			retryable: asOptionalBoolean(payload.retryable)
		};
		default: return null;
	}
}
//#endregion
//#region src/runtime/engine/reuse-policy.ts
function shouldReuseExistingRecord(record, params) {
	if (record.acpx?.reset_on_next_ensure === true) return false;
	if (path.resolve(record.cwd) !== path.resolve(params.cwd)) return false;
	if (record.agentCommand !== params.agentCommand) return false;
	if (params.resumeSessionId && record.acpSessionId !== params.resumeSessionId) return false;
	return true;
}
//#endregion
//#region src/runtime/engine/manager.ts
function createDeferred() {
	let resolve;
	let reject;
	return {
		promise: new Promise((res, rej) => {
			resolve = res;
			reject = rej;
		}),
		resolve,
		reject
	};
}
var AsyncEventQueue = class {
	items = [];
	waits = [];
	closed = false;
	push(item) {
		if (this.closed) return;
		const waiter = this.waits.shift();
		if (waiter) {
			waiter.resolve(item);
			return;
		}
		this.items.push(item);
	}
	close() {
		if (this.closed) return;
		this.closed = true;
		for (const waiter of this.waits.splice(0)) waiter.resolve(null);
	}
	async next() {
		if (this.items.length > 0) return this.items.shift() ?? null;
		if (this.closed) return null;
		const waiter = createDeferred();
		this.waits.push(waiter);
		return await waiter.promise;
	}
	async *iterate() {
		while (true) {
			const next = await this.next();
			if (!next) return;
			yield next;
		}
	}
};
function isoNow() {
	return (/* @__PURE__ */ new Date()).toISOString();
}
function isUnsupportedSessionCloseError(error) {
	const acp = extractAcpError(error);
	if (!acp) return false;
	if (acp.code === -32601 || acp.code === -32602) return true;
	if (acp.code !== -32603 || !acp.data || typeof acp.data !== "object") return false;
	const details = acp.data.details;
	return typeof details === "string" && details.toLowerCase().includes("invalid params");
}
function toPromptInput(text, attachments) {
	if (!attachments || attachments.length === 0) return text;
	const blocks = [];
	if (text) blocks.push({
		type: "text",
		text
	});
	for (const attachment of attachments) {
		if (!attachment.mediaType.startsWith("image/")) throw new AcpRuntimeError("ACP_TURN_FAILED", `Unsupported ACP runtime attachment media type: ${attachment.mediaType}`);
		blocks.push({
			type: "image",
			mimeType: attachment.mediaType,
			data: attachment.data
		});
	}
	return blocks.length > 0 ? blocks : textPrompt(text);
}
function createInitialRecord(params) {
	const now = isoNow();
	return {
		schema: "acpx.session.v1",
		acpxRecordId: params.recordId,
		acpSessionId: params.sessionId,
		agentSessionId: params.agentSessionId,
		agentCommand: params.agentCommand,
		cwd: params.cwd,
		name: params.sessionName,
		createdAt: now,
		lastUsedAt: now,
		lastSeq: 0,
		eventLog: defaultSessionEventLog(params.recordId),
		closed: false,
		closedAt: void 0,
		...createSessionConversation(now),
		acpx: {}
	};
}
function createRecordId(sessionKey, mode) {
	if (mode === "persistent") return sessionKey;
	return `${sessionKey}:oneshot:${randomUUID()}`;
}
function resumePolicyForSessionMode(mode) {
	return mode === "persistent" ? "same-session-only" : "allow-new";
}
function statusSummary(record) {
	return [
		`session=${record.acpxRecordId}`,
		`backendSessionId=${record.acpSessionId}`,
		record.agentSessionId ? `agentSessionId=${record.agentSessionId}` : null,
		record.pid != null ? `pid=${record.pid}` : null,
		record.closed ? "closed" : "open"
	].filter(Boolean).join(" ");
}
var AcpRuntimeManager = class {
	activeControllers = /* @__PURE__ */ new Map();
	pendingPersistentClients = /* @__PURE__ */ new Map();
	constructor(options, deps = {}) {
		this.options = options;
		this.deps = deps;
	}
	createClient(options) {
		return this.deps.clientFactory?.(options) ?? new AcpClient(options);
	}
	async ensureSession(input) {
		const cwd = path.resolve(input.cwd?.trim() || this.options.cwd);
		const agentCommand = this.options.agentRegistry.resolve(input.agent);
		const existing = await this.options.sessionStore.load(input.sessionKey);
		if (input.mode === "persistent" && existing && shouldReuseExistingRecord(existing, {
			cwd,
			agentCommand,
			resumeSessionId: input.resumeSessionId
		})) {
			existing.closed = false;
			existing.closedAt = void 0;
			await this.options.sessionStore.save(existing);
			return existing;
		}
		const client = this.createClient({
			agentCommand,
			cwd,
			mcpServers: [...this.options.mcpServers ?? []],
			permissionMode: this.options.permissionMode,
			nonInteractivePermissions: this.options.nonInteractivePermissions,
			verbose: this.options.verbose
		});
		let keepClientOpen = false;
		try {
			await client.start();
			let sessionId;
			let agentSessionId;
			if (input.resumeSessionId) {
				const loaded = await client.loadSession(input.resumeSessionId, cwd);
				sessionId = input.resumeSessionId;
				agentSessionId = loaded.agentSessionId;
			} else {
				const created = await client.createSession(cwd);
				sessionId = created.sessionId;
				agentSessionId = created.agentSessionId;
			}
			const record = createInitialRecord({
				recordId: createRecordId(input.sessionKey, input.mode),
				sessionName: input.sessionKey,
				sessionId,
				agentCommand,
				cwd,
				agentSessionId
			});
			record.protocolVersion = client.initializeResult?.protocolVersion;
			record.agentCapabilities = client.initializeResult?.agentCapabilities;
			applyLifecycleSnapshotToRecord(record, client.getAgentLifecycleSnapshot());
			await this.options.sessionStore.save(record);
			if (input.mode === "persistent") {
				const previousClient = this.pendingPersistentClients.get(record.acpxRecordId);
				this.pendingPersistentClients.set(record.acpxRecordId, client);
				keepClientOpen = true;
				await previousClient?.close().catch(() => {});
			}
			return record;
		} finally {
			if (!keepClientOpen) await client.close();
		}
	}
	async *runTurn(input) {
		const record = await this.requireRecord(input.handle.acpxRecordId ?? input.handle.sessionKey);
		const conversation = cloneSessionConversation(record);
		let acpxState = cloneSessionAcpxState(record.acpx);
		const promptInput = toPromptInput(input.text, input.attachments);
		const promptMessageId = recordPromptSubmission(conversation, promptInput, isoNow());
		trimConversationForRuntime(conversation);
		const queue = new AsyncEventQueue();
		let pendingClient = this.pendingPersistentClients.get(record.acpxRecordId);
		if (pendingClient) {
			this.pendingPersistentClients.delete(record.acpxRecordId);
			if (!pendingClient.hasReusableSession(record.acpSessionId)) {
				await pendingClient.close().catch(() => {});
				pendingClient = void 0;
			}
		}
		const client = pendingClient ?? this.createClient({
			agentCommand: record.agentCommand,
			cwd: record.cwd,
			mcpServers: [...this.options.mcpServers ?? []],
			permissionMode: this.options.permissionMode,
			nonInteractivePermissions: this.options.nonInteractivePermissions,
			verbose: this.options.verbose
		});
		let activeSessionId = record.acpSessionId;
		let sawDone = false;
		let pendingCancel = false;
		let turnActive = true;
		const sessionReady = createDeferred();
		sessionReady.promise.catch(() => {});
		const applyPendingCancel = async () => {
			if (!pendingCancel || !client.hasActivePrompt()) return false;
			const cancelled = await client.requestCancelActivePrompt();
			if (cancelled) pendingCancel = false;
			return cancelled;
		};
		const activeController = {
			hasActivePrompt: () => client.hasActivePrompt(),
			requestCancelActivePrompt: async () => {
				if (client.hasActivePrompt()) return await client.requestCancelActivePrompt();
				if (!turnActive) return false;
				pendingCancel = true;
				return true;
			},
			setSessionMode: async (modeId) => {
				if (!client.hasActivePrompt()) await sessionReady.promise;
				await client.setSessionMode(activeSessionId, modeId);
			},
			setSessionModel: async (modelId) => {
				if (!client.hasActivePrompt()) await sessionReady.promise;
				await client.setSessionModel(activeSessionId, modelId);
			},
			setSessionConfigOption: async (configId, value) => {
				if (!client.hasActivePrompt()) await sessionReady.promise;
				return await client.setSessionConfigOption(activeSessionId, configId, value);
			}
		};
		const emitParsed = (payload) => {
			const parsed = parsePromptEventLine(JSON.stringify(payload));
			if (!parsed) return;
			if (parsed.type === "done") sawDone = true;
			queue.push(parsed);
		};
		const abortHandler = () => {
			activeController.requestCancelActivePrompt();
		};
		if (input.signal) {
			if (input.signal.aborted) {
				queue.close();
				return;
			}
			input.signal.addEventListener("abort", abortHandler, { once: true });
		}
		this.activeControllers.set(record.acpxRecordId, activeController);
		(async () => {
			try {
				client.setEventHandlers({
					onSessionUpdate: (notification) => {
						acpxState = recordSessionUpdate(conversation, acpxState, notification);
						trimConversationForRuntime(conversation);
						emitParsed({
							jsonrpc: "2.0",
							method: "session/update",
							params: notification
						});
					},
					onClientOperation: (operation) => {
						acpxState = recordClientOperation(conversation, acpxState, operation);
						trimConversationForRuntime(conversation);
						emitParsed({
							type: "client_operation",
							...operation
						});
					}
				});
				const { sessionId, resumed, loadError } = pendingClient ? {
					sessionId: record.acpSessionId,
					resumed: false,
					loadError: void 0
				} : await connectAndLoadSession({
					client,
					record,
					resumePolicy: resumePolicyForSessionMode(input.sessionMode),
					timeoutMs: this.options.timeoutMs,
					activeController,
					onClientAvailable: (controller) => {
						this.activeControllers.set(record.acpxRecordId, controller);
					},
					onConnectedRecord: (connectedRecord) => {
						connectedRecord.lastPromptAt = isoNow();
					},
					onSessionIdResolved: (sessionIdValue) => {
						activeSessionId = sessionIdValue;
					}
				});
				sessionReady.resolve();
				record.lastRequestId = input.requestId;
				record.lastPromptAt = isoNow();
				record.closed = false;
				record.closedAt = void 0;
				record.lastUsedAt = isoNow();
				if (resumed || loadError) emitParsed({
					type: "status",
					text: loadError ? `load fallback: ${loadError}` : "session resumed"
				});
				if (pendingCancel || input.signal?.aborted) {
					pendingCancel = false;
					if (!sawDone) queue.push({
						type: "done",
						stopReason: "cancelled"
					});
					return;
				}
				await applyPendingCancel();
				const response = await runPromptTurn({
					client,
					sessionId,
					prompt: promptInput,
					timeoutMs: input.timeoutMs ?? this.options.timeoutMs,
					conversation,
					promptMessageId
				});
				record.acpSessionId = activeSessionId;
				reconcileAgentSessionId(record, record.agentSessionId);
				record.protocolVersion = client.initializeResult?.protocolVersion;
				record.agentCapabilities = client.initializeResult?.agentCapabilities;
				record.acpx = acpxState;
				applyConversation(record, conversation);
				applyLifecycleSnapshotToRecord(record, client.getAgentLifecycleSnapshot());
				await this.options.sessionStore.save(record);
				if (!sawDone) queue.push({
					type: "done",
					stopReason: response.stopReason
				});
			} catch (error) {
				sessionReady.reject(error);
				const normalized = normalizeOutputError(error, { origin: "runtime" });
				queue.push({
					type: "error",
					message: normalized.message,
					code: normalized.code,
					retryable: normalized.retryable
				});
			} finally {
				turnActive = false;
				if (input.signal) input.signal.removeEventListener("abort", abortHandler);
				this.activeControllers.delete(record.acpxRecordId);
				client.clearEventHandlers();
				applyLifecycleSnapshotToRecord(record, client.getAgentLifecycleSnapshot());
				record.acpx = acpxState;
				applyConversation(record, conversation);
				record.lastUsedAt = isoNow();
				await this.options.sessionStore.save(record).catch(() => {});
				await client.close().catch(() => {});
				queue.close();
			}
		})();
		yield* queue.iterate();
	}
	async getStatus(handle) {
		const record = await this.requireRecord(handle.acpxRecordId ?? handle.sessionKey);
		return {
			summary: statusSummary(record),
			acpxRecordId: record.acpxRecordId,
			backendSessionId: record.acpSessionId,
			agentSessionId: record.agentSessionId,
			details: {
				cwd: record.cwd,
				lastUsedAt: record.lastUsedAt,
				closed: record.closed === true
			}
		};
	}
	async setMode(handle, mode, sessionMode = "persistent") {
		const record = await this.requireRecord(handle.acpxRecordId ?? handle.sessionKey);
		const controller = this.activeControllers.get(record.acpxRecordId);
		let targetRecord = record;
		if (controller) await controller.setSessionMode(mode);
		else targetRecord = (await withConnectedSession({
			sessionRecordId: record.acpxRecordId,
			loadRecord: async (sessionRecordId) => await this.requireRecord(sessionRecordId),
			saveRecord: async (connectedRecord) => await this.options.sessionStore.save(connectedRecord),
			createClient: (options) => this.createClient(options),
			mcpServers: [...this.options.mcpServers ?? []],
			permissionMode: this.options.permissionMode,
			nonInteractivePermissions: this.options.nonInteractivePermissions,
			verbose: this.options.verbose,
			timeoutMs: this.options.timeoutMs,
			resumePolicy: resumePolicyForSessionMode(sessionMode),
			run: async ({ client, sessionId }) => {
				await client.setSessionMode(sessionId, mode);
			}
		})).record;
		setDesiredModeId(targetRecord, mode);
		await this.options.sessionStore.save(targetRecord);
	}
	async setConfigOption(handle, key, value, sessionMode = "persistent") {
		const record = await this.requireRecord(handle.acpxRecordId ?? handle.sessionKey);
		const controller = this.activeControllers.get(record.acpxRecordId);
		let targetRecord = record;
		if (controller) await controller.setSessionConfigOption(key, value);
		else targetRecord = (await withConnectedSession({
			sessionRecordId: record.acpxRecordId,
			loadRecord: async (sessionRecordId) => await this.requireRecord(sessionRecordId),
			saveRecord: async (connectedRecord) => await this.options.sessionStore.save(connectedRecord),
			createClient: (options) => this.createClient(options),
			mcpServers: [...this.options.mcpServers ?? []],
			permissionMode: this.options.permissionMode,
			nonInteractivePermissions: this.options.nonInteractivePermissions,
			verbose: this.options.verbose,
			timeoutMs: this.options.timeoutMs,
			resumePolicy: resumePolicyForSessionMode(sessionMode),
			run: async ({ client, sessionId, record: connectedRecord }) => {
				await client.setSessionConfigOption(sessionId, key, value);
				if (key === "mode") setDesiredModeId(connectedRecord, value);
			}
		})).record;
		if (key === "mode") setDesiredModeId(targetRecord, value);
		await this.options.sessionStore.save(targetRecord);
	}
	async cancel(handle) {
		await this.activeControllers.get(handle.acpxRecordId ?? handle.sessionKey)?.requestCancelActivePrompt();
	}
	async close(handle, options = {}) {
		const record = await this.requireRecord(handle.acpxRecordId ?? handle.sessionKey);
		await this.cancel(handle);
		if (options.discardPersistentState) {
			await this.closeBackendSession(record);
			record.acpx = {
				...record.acpx,
				reset_on_next_ensure: true
			};
		}
		record.closed = true;
		record.closedAt = isoNow();
		await this.options.sessionStore.save(record);
	}
	async closeBackendSession(record) {
		const pendingClient = this.pendingPersistentClients.get(record.acpxRecordId);
		if (pendingClient) this.pendingPersistentClients.delete(record.acpxRecordId);
		const reusablePendingClient = pendingClient?.hasReusableSession(record.acpSessionId) === true ? pendingClient : void 0;
		if (pendingClient && !reusablePendingClient) await pendingClient.close().catch(() => {});
		const client = reusablePendingClient ?? this.createClient({
			agentCommand: record.agentCommand,
			cwd: record.cwd,
			mcpServers: [...this.options.mcpServers ?? []],
			permissionMode: this.options.permissionMode,
			nonInteractivePermissions: this.options.nonInteractivePermissions,
			verbose: this.options.verbose
		});
		try {
			if (!reusablePendingClient) await withTimeout(client.start(), this.options.timeoutMs);
			if (!client.supportsCloseSession()) throw new AcpRuntimeError("ACP_BACKEND_UNSUPPORTED_CONTROL", `Agent does not support session/close for ${record.acpxRecordId}.`);
			await withTimeout(client.closeSession(record.acpSessionId), this.options.timeoutMs);
		} catch (error) {
			if (isUnsupportedSessionCloseError(error)) throw new AcpRuntimeError("ACP_BACKEND_UNSUPPORTED_CONTROL", `Agent does not support session/close for ${record.acpxRecordId}.`, { cause: error });
			if (isAcpResourceNotFoundError(error)) return;
			throw error;
		} finally {
			await client.close().catch(() => {});
		}
	}
	async requireRecord(sessionId) {
		const record = await this.options.sessionStore.load(sessionId);
		if (!record) throw new Error(`ACP session not found: ${sessionId}`);
		return record;
	}
};
//#endregion
//#region src/runtime/public/file-session-store.ts
function safeSessionId(sessionId) {
	return encodeURIComponent(sessionId);
}
var FileSessionStore = class {
	constructor(stateDir) {
		this.stateDir = stateDir;
	}
	get sessionDir() {
		return path.join(this.stateDir, "sessions");
	}
	filePath(sessionId) {
		return path.join(this.sessionDir, `${safeSessionId(sessionId)}.json`);
	}
	async ensureDir() {
		await fs.mkdir(this.sessionDir, { recursive: true });
	}
	async load(sessionId) {
		await this.ensureDir();
		try {
			const payload = await fs.readFile(this.filePath(sessionId), "utf8");
			return parseSessionRecord(JSON.parse(payload)) ?? void 0;
		} catch (error) {
			if (error.code === "ENOENT") return;
			throw error;
		}
	}
	async save(record) {
		await this.ensureDir();
		const persisted = serializeSessionRecordForDisk(record);
		assertPersistedKeyPolicy(persisted);
		const file = this.filePath(record.acpxRecordId);
		const tempFile = `${file}.${process.pid}.${Date.now()}.tmp`;
		const payload = JSON.stringify(persisted, null, 2);
		await fs.writeFile(tempFile, `${payload}\n`, "utf8");
		await fs.rename(tempFile, file);
	}
};
function createFileSessionStore(options) {
	return new FileSessionStore(path.resolve(options.stateDir));
}
//#endregion
//#region src/runtime/public/handle-state.ts
const ACPX_RUNTIME_HANDLE_PREFIX = "acpx:v2:";
function encodeAcpxRuntimeHandleState(state) {
	return `${ACPX_RUNTIME_HANDLE_PREFIX}${Buffer.from(JSON.stringify(state), "utf8").toString("base64url")}`;
}
function decodeAcpxRuntimeHandleState(runtimeSessionName) {
	const trimmed = runtimeSessionName.trim();
	if (!trimmed.startsWith(ACPX_RUNTIME_HANDLE_PREFIX)) return null;
	try {
		const raw = Buffer.from(trimmed.slice(8), "base64url").toString("utf8");
		const parsed = JSON.parse(raw);
		const name = asOptionalString(parsed.name);
		const agent = asOptionalString(parsed.agent);
		const cwd = asOptionalString(parsed.cwd);
		const mode = asOptionalString(parsed.mode);
		if (!name || !agent || !cwd || mode !== "persistent" && mode !== "oneshot") return null;
		return {
			name,
			agent,
			cwd,
			mode,
			acpxRecordId: asOptionalString(parsed.acpxRecordId),
			backendSessionId: asOptionalString(parsed.backendSessionId),
			agentSessionId: asOptionalString(parsed.agentSessionId)
		};
	} catch {
		return null;
	}
}
function writeHandleState(handle, state) {
	handle.runtimeSessionName = encodeAcpxRuntimeHandleState(state);
	handle.cwd = state.cwd;
	handle.acpxRecordId = state.acpxRecordId;
	handle.backendSessionId = state.backendSessionId;
	handle.agentSessionId = state.agentSessionId;
}
//#endregion
//#region src/runtime/public/probe.ts
async function probeRuntime(options, deps = {}) {
	const agentName = options.probeAgent?.trim() || "codex";
	const agentCommand = options.agentRegistry.resolve(agentName);
	const client = deps.clientFactory?.({
		agentCommand,
		cwd: options.cwd,
		mcpServers: [...options.mcpServers ?? []],
		permissionMode: options.permissionMode,
		nonInteractivePermissions: options.nonInteractivePermissions,
		verbose: options.verbose
	}) ?? new AcpClient({
		agentCommand,
		cwd: options.cwd,
		mcpServers: [...options.mcpServers ?? []],
		permissionMode: options.permissionMode,
		nonInteractivePermissions: options.nonInteractivePermissions,
		verbose: options.verbose
	});
	try {
		await client.start();
		return {
			ok: true,
			message: "embedded ACP runtime ready",
			details: [
				`agent=${agentName}`,
				`command=${agentCommand}`,
				`cwd=${options.cwd}`,
				...client.initializeResult?.protocolVersion ? [`protocolVersion=${client.initializeResult.protocolVersion}`] : []
			]
		};
	} catch (error) {
		return {
			ok: false,
			message: "embedded ACP runtime probe failed",
			details: [
				`agent=${agentName}`,
				`command=${agentCommand}`,
				`cwd=${options.cwd}`,
				error instanceof Error ? error.message : String(error)
			]
		};
	} finally {
		await client.close().catch(() => {});
	}
}
//#endregion
//#region src/runtime.ts
const ACPX_BACKEND_ID = "acpx";
const ACPX_CAPABILITIES = { controls: [
	"session/set_mode",
	"session/set_config_option",
	"session/status"
] };
function createAgentRegistry(params) {
	return {
		resolve(agentName) {
			return resolveAgentCommand(agentName, params?.overrides);
		},
		list() {
			return listBuiltInAgents(params?.overrides);
		}
	};
}
var AcpxRuntime = class {
	healthy = false;
	manager = null;
	managerPromise = null;
	constructor(options, testOptions) {
		this.options = options;
		this.testOptions = testOptions;
	}
	isHealthy() {
		return this.healthy;
	}
	async probeAvailability() {
		this.healthy = (await this.runProbe()).ok;
	}
	async doctor() {
		const report = await this.runProbe();
		this.healthy = report.ok;
		return {
			ok: report.ok,
			code: report.ok ? void 0 : "ACP_BACKEND_UNAVAILABLE",
			message: report.message,
			details: report.details
		};
	}
	async ensureSession(input) {
		const sessionName = input.sessionKey.trim();
		if (!sessionName) throw new AcpRuntimeError("ACP_SESSION_INIT_FAILED", "ACP session key is required.");
		const agent = input.agent.trim();
		if (!agent) throw new AcpRuntimeError("ACP_SESSION_INIT_FAILED", "ACP agent id is required.");
		const record = await (await this.getManager()).ensureSession({
			sessionKey: sessionName,
			agent,
			mode: input.mode,
			cwd: input.cwd ?? this.options.cwd,
			resumeSessionId: input.resumeSessionId
		});
		const handle = {
			sessionKey: input.sessionKey,
			backend: ACPX_BACKEND_ID,
			runtimeSessionName: "",
			cwd: record.cwd,
			acpxRecordId: record.acpxRecordId,
			backendSessionId: record.acpSessionId,
			agentSessionId: record.agentSessionId
		};
		writeHandleState(handle, {
			name: sessionName,
			agent,
			cwd: record.cwd,
			mode: input.mode,
			acpxRecordId: record.acpxRecordId,
			backendSessionId: record.acpSessionId,
			agentSessionId: record.agentSessionId
		});
		return handle;
	}
	async *runTurn(input) {
		const state = this.resolveHandleState(input.handle);
		yield* (await this.getManager()).runTurn({
			handle: {
				...input.handle,
				acpxRecordId: state.acpxRecordId ?? input.handle.acpxRecordId ?? input.handle.sessionKey
			},
			text: input.text,
			attachments: input.attachments,
			mode: input.mode,
			sessionMode: state.mode,
			requestId: input.requestId,
			timeoutMs: input.timeoutMs,
			signal: input.signal
		});
	}
	getCapabilities() {
		return ACPX_CAPABILITIES;
	}
	async getStatus(input) {
		const state = this.resolveHandleState(input.handle);
		return await (await this.getManager()).getStatus({
			...input.handle,
			acpxRecordId: state.acpxRecordId ?? input.handle.acpxRecordId ?? input.handle.sessionKey
		});
	}
	async setMode(input) {
		const state = this.resolveHandleState(input.handle);
		await (await this.getManager()).setMode({
			...input.handle,
			acpxRecordId: state.acpxRecordId ?? input.handle.acpxRecordId ?? input.handle.sessionKey
		}, input.mode, state.mode);
	}
	async setConfigOption(input) {
		const state = this.resolveHandleState(input.handle);
		await (await this.getManager()).setConfigOption({
			...input.handle,
			acpxRecordId: state.acpxRecordId ?? input.handle.acpxRecordId ?? input.handle.sessionKey
		}, input.key, input.value, state.mode);
	}
	async cancel(input) {
		const state = this.resolveHandleState(input.handle);
		await (await this.getManager()).cancel({
			...input.handle,
			acpxRecordId: state.acpxRecordId ?? input.handle.acpxRecordId ?? input.handle.sessionKey
		});
	}
	async close(input) {
		const state = this.resolveHandleState(input.handle);
		await (await this.getManager()).close({
			...input.handle,
			acpxRecordId: state.acpxRecordId ?? input.handle.acpxRecordId ?? input.handle.sessionKey
		}, { discardPersistentState: input.discardPersistentState });
	}
	async getManager() {
		if (this.manager) return this.manager;
		if (!this.managerPromise) this.managerPromise = Promise.resolve(this.testOptions?.managerFactory?.(this.options) ?? new AcpRuntimeManager(this.options)).then((manager) => {
			this.manager = manager;
			return manager;
		});
		return await this.managerPromise;
	}
	async runProbe() {
		return await (this.testOptions?.probeRunner?.(this.options) ?? probeRuntime(this.options));
	}
	resolveHandleState(handle) {
		const decoded = decodeAcpxRuntimeHandleState(handle.runtimeSessionName);
		if (decoded) return {
			...decoded,
			acpxRecordId: decoded.acpxRecordId ?? handle.acpxRecordId,
			backendSessionId: decoded.backendSessionId ?? handle.backendSessionId,
			agentSessionId: decoded.agentSessionId ?? handle.agentSessionId
		};
		const runtimeSessionName = handle.runtimeSessionName.trim();
		if (!runtimeSessionName) throw new AcpRuntimeError("ACP_SESSION_INIT_FAILED", "Invalid embedded ACP runtime handle: runtimeSessionName is missing.");
		return {
			name: runtimeSessionName,
			agent: deriveAgentFromSessionKey(handle.sessionKey, DEFAULT_AGENT_NAME),
			cwd: handle.cwd ?? this.options.cwd,
			mode: "persistent",
			acpxRecordId: handle.acpxRecordId,
			backendSessionId: handle.backendSessionId,
			agentSessionId: handle.agentSessionId
		};
	}
};
function createAcpRuntime(options) {
	return new AcpxRuntime(options);
}
function createRuntimeStore(options) {
	return createFileSessionStore(options);
}
//#endregion
export { ACPX_BACKEND_ID, AcpRuntimeError, AcpxRuntime, DEFAULT_AGENT_NAME, createAcpRuntime, createAgentRegistry, createFileSessionStore, createRuntimeStore, decodeAcpxRuntimeHandleState, encodeAcpxRuntimeHandleState, isAcpRuntimeError };

//# sourceMappingURL=runtime.js.map