import { D as OUTPUT_ERROR_CODES, H as QueueProtocolError, O as OUTPUT_ERROR_ORIGINS, V as QueueConnectionError, a as recordPerfDuration, f as isPromptInput, g as textPrompt, i as measurePerf, r as incrementPerfCounter, x as normalizeOutputError } from "./perf-metrics-D0um6IR6.js";
import { n as isAcpJsonRpcMessage } from "./jsonrpc-DSxh2w5R.js";
import path from "node:path";
import fs from "node:fs/promises";
import os from "node:os";
import { createHash, randomUUID } from "node:crypto";
import net from "node:net";
//#region src/cli/queue/paths.ts
function shortHash(value, length) {
	return createHash("sha256").update(value).digest("hex").slice(0, length);
}
function queueKeyForSession(sessionId) {
	return shortHash(sessionId, 24);
}
function queueBaseDir(homeDir = os.homedir()) {
	return path.join(homeDir, ".acpx", "queues");
}
function queueSocketBaseDir(homeDir = os.homedir()) {
	if (process.platform === "win32") return;
	return path.join("/tmp", `acpx-${shortHash(homeDir, 10)}`);
}
function queueLockFilePath(sessionId, homeDir = os.homedir()) {
	return path.join(queueBaseDir(homeDir), `${queueKeyForSession(sessionId)}.lock`);
}
function queueSocketPath(sessionId, homeDir = os.homedir()) {
	const key = queueKeyForSession(sessionId);
	if (process.platform === "win32") return `\\\\.\\pipe\\acpx-${key}`;
	return path.join(queueSocketBaseDir(homeDir) ?? "/tmp", `${key}.sock`);
}
//#endregion
//#region src/cli/queue/lease-store.ts
const PROCESS_EXIT_GRACE_MS = 1500;
const PROCESS_POLL_MS = 50;
const QUEUE_OWNER_STALE_HEARTBEAT_MS = 15e3;
function parseQueueOwnerRecord(raw) {
	if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
	const record = raw;
	if (!Number.isInteger(record.pid) || record.pid <= 0 || typeof record.sessionId !== "string" || typeof record.socketPath !== "string" || typeof record.createdAt !== "string" || typeof record.heartbeatAt !== "string" || !Number.isInteger(record.ownerGeneration) || record.ownerGeneration <= 0 || !Number.isInteger(record.queueDepth) || record.queueDepth < 0) return null;
	return {
		pid: record.pid,
		sessionId: record.sessionId,
		socketPath: record.socketPath,
		createdAt: record.createdAt,
		heartbeatAt: record.heartbeatAt,
		ownerGeneration: record.ownerGeneration,
		queueDepth: record.queueDepth
	};
}
function createOwnerGeneration() {
	return Date.now() * 1e3 + Math.floor(Math.random() * 1e3);
}
function nowIso() {
	return (/* @__PURE__ */ new Date()).toISOString();
}
function isQueueOwnerHeartbeatStale(owner) {
	const heartbeatMs = Date.parse(owner.heartbeatAt);
	if (!Number.isFinite(heartbeatMs)) return true;
	return Date.now() - heartbeatMs > QUEUE_OWNER_STALE_HEARTBEAT_MS;
}
async function ensureQueueDir() {
	await fs.mkdir(queueBaseDir(), { recursive: true });
	const socketDir = queueSocketBaseDir();
	if (socketDir) await fs.mkdir(socketDir, { recursive: true });
}
async function removeSocketFile(socketPath) {
	if (process.platform === "win32") return;
	try {
		await fs.unlink(socketPath);
	} catch (error) {
		if (error.code !== "ENOENT") throw error;
	}
}
async function waitForProcessExit(pid, timeoutMs) {
	const deadline = Date.now() + Math.max(0, timeoutMs);
	while (Date.now() <= deadline) {
		if (!isProcessAlive(pid)) return true;
		await waitMs(PROCESS_POLL_MS);
	}
	return !isProcessAlive(pid);
}
async function cleanupStaleQueueOwner(sessionId, owner) {
	const lockPath = queueLockFilePath(sessionId);
	await removeSocketFile(owner?.socketPath ?? queueSocketPath(sessionId)).catch(() => {});
	await fs.unlink(lockPath).catch((error) => {
		if (error.code !== "ENOENT") throw error;
	});
}
async function readQueueOwnerRecord(sessionId) {
	const lockPath = queueLockFilePath(sessionId);
	try {
		const payload = await fs.readFile(lockPath, "utf8");
		return parseQueueOwnerRecord(JSON.parse(payload)) ?? void 0;
	} catch {
		return;
	}
}
function isProcessAlive(pid) {
	if (!pid || !Number.isInteger(pid) || pid <= 0 || pid === process.pid) return false;
	try {
		process.kill(pid, 0);
		return true;
	} catch {
		return false;
	}
}
async function terminateProcess(pid) {
	if (!isProcessAlive(pid)) return false;
	try {
		process.kill(pid, "SIGTERM");
	} catch {
		return false;
	}
	if (await waitForProcessExit(pid, PROCESS_EXIT_GRACE_MS)) return true;
	try {
		process.kill(pid, "SIGKILL");
	} catch {
		return false;
	}
	await waitForProcessExit(pid, PROCESS_EXIT_GRACE_MS);
	return true;
}
async function ensureOwnerIsUsable(sessionId, owner) {
	const alive = isProcessAlive(owner.pid);
	const stale = isQueueOwnerHeartbeatStale(owner);
	if (alive && !stale) return true;
	if (alive) await terminateProcess(owner.pid).catch(() => {});
	await cleanupStaleQueueOwner(sessionId, owner);
	return false;
}
async function readQueueOwnerStatus(sessionId) {
	const owner = await readQueueOwnerRecord(sessionId);
	if (!owner) return;
	const alive = await ensureOwnerIsUsable(sessionId, owner);
	if (!alive) return;
	return {
		pid: owner.pid,
		socketPath: owner.socketPath,
		heartbeatAt: owner.heartbeatAt,
		ownerGeneration: owner.ownerGeneration,
		queueDepth: owner.queueDepth,
		alive,
		stale: isQueueOwnerHeartbeatStale(owner)
	};
}
async function tryAcquireQueueOwnerLease(sessionId, nowIsoFactory = nowIso) {
	await ensureQueueDir();
	const lockPath = queueLockFilePath(sessionId);
	const socketPath = queueSocketPath(sessionId);
	const createdAt = nowIsoFactory();
	const ownerGeneration = createOwnerGeneration();
	const payload = JSON.stringify({
		pid: process.pid,
		sessionId,
		socketPath,
		createdAt,
		heartbeatAt: createdAt,
		ownerGeneration,
		queueDepth: 0
	}, null, 2);
	try {
		await fs.writeFile(lockPath, `${payload}\n`, {
			encoding: "utf8",
			flag: "wx"
		});
		await removeSocketFile(socketPath).catch(() => {});
		return {
			sessionId,
			lockPath,
			socketPath,
			createdAt,
			ownerGeneration
		};
	} catch (error) {
		if (error.code !== "EEXIST") throw error;
		const owner = await readQueueOwnerRecord(sessionId);
		if (!owner) {
			await cleanupStaleQueueOwner(sessionId, owner);
			return;
		}
		if (!isProcessAlive(owner.pid) || isQueueOwnerHeartbeatStale(owner)) {
			if (isProcessAlive(owner.pid)) await terminateProcess(owner.pid).catch(() => {});
			await cleanupStaleQueueOwner(sessionId, owner);
		}
		return;
	}
}
async function refreshQueueOwnerLease(lease, options, nowIsoFactory = nowIso) {
	const payload = JSON.stringify({
		pid: process.pid,
		sessionId: lease.sessionId,
		socketPath: lease.socketPath,
		createdAt: lease.createdAt,
		heartbeatAt: nowIsoFactory(),
		ownerGeneration: lease.ownerGeneration,
		queueDepth: Math.max(0, Math.round(options.queueDepth))
	}, null, 2);
	await fs.writeFile(lease.lockPath, `${payload}\n`, { encoding: "utf8" });
}
async function releaseQueueOwnerLease(lease) {
	await removeSocketFile(lease.socketPath).catch(() => {});
	await fs.unlink(lease.lockPath).catch((error) => {
		if (error.code !== "ENOENT") throw error;
	});
}
async function terminateQueueOwnerForSession(sessionId) {
	const owner = await readQueueOwnerRecord(sessionId);
	if (!owner) return;
	if (isProcessAlive(owner.pid)) await terminateProcess(owner.pid);
	await cleanupStaleQueueOwner(sessionId, owner);
}
async function waitMs(ms) {
	await new Promise((resolve) => {
		setTimeout(resolve, ms);
	});
}
//#endregion
//#region src/cli/queue/ipc-transport.ts
const QUEUE_CONNECT_ATTEMPTS = 40;
const SOCKET_CONNECTION_TIMEOUT_MS = 5e3;
function shouldRetryQueueConnect(error) {
	const code = error.code;
	return code === "ENOENT" || code === "ECONNREFUSED";
}
async function connectToSocket(socketPath, timeoutMs = SOCKET_CONNECTION_TIMEOUT_MS) {
	return await new Promise((resolve, reject) => {
		const socket = net.createConnection(socketPath);
		let settled = false;
		const timeout = setTimeout(() => {
			if (settled) return;
			settled = true;
			socket.destroy();
			reject(/* @__PURE__ */ new Error(`Connection to ${socketPath} timed out after ${timeoutMs}ms`));
		}, timeoutMs);
		const onConnect = () => {
			if (settled) return;
			settled = true;
			clearTimeout(timeout);
			socket.off("error", onError);
			resolve(socket);
		};
		const onError = (error) => {
			if (settled) return;
			settled = true;
			clearTimeout(timeout);
			socket.off("connect", onConnect);
			reject(error);
		};
		socket.once("connect", onConnect);
		socket.once("error", onError);
	});
}
async function connectToQueueOwner(owner, maxAttempts = QUEUE_CONNECT_ATTEMPTS) {
	let lastError;
	const attempts = Math.max(1, Math.trunc(maxAttempts));
	for (let attempt = 0; attempt < attempts; attempt += 1) try {
		return await measurePerf("queue.connect", async () => await connectToSocket(owner.socketPath));
	} catch (error) {
		lastError = error;
		if (!shouldRetryQueueConnect(error)) throw error;
		await waitMs(50);
	}
	if (lastError && !shouldRetryQueueConnect(lastError)) throw lastError;
}
//#endregion
//#region src/cli/queue/ipc-health.ts
async function probeQueueOwnerHealth(sessionId) {
	const ownerRecord = await readQueueOwnerRecord(sessionId);
	if (!ownerRecord) return {
		sessionId,
		hasLease: false,
		healthy: false,
		socketReachable: false,
		pidAlive: false
	};
	const owner = await readQueueOwnerStatus(sessionId);
	if (!owner) return {
		sessionId,
		hasLease: false,
		healthy: false,
		socketReachable: false,
		pidAlive: false
	};
	const pidAlive = owner.alive;
	let socketReachable = false;
	try {
		const socket = await connectToQueueOwner(ownerRecord, 2);
		if (socket) {
			socketReachable = true;
			if (!socket.destroyed) socket.end();
		}
	} catch {
		socketReachable = false;
	}
	return {
		sessionId,
		hasLease: true,
		healthy: socketReachable,
		socketReachable,
		pidAlive,
		pid: owner.pid,
		socketPath: owner.socketPath,
		ownerGeneration: owner.ownerGeneration,
		queueDepth: owner.queueDepth
	};
}
//#endregion
//#region src/cli/queue/messages.ts
function asRecord(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function isPermissionMode(value) {
	return value === "approve-all" || value === "approve-reads" || value === "deny-all";
}
function isSessionResumePolicy(value) {
	return value === "allow-new" || value === "same-session-only";
}
function isNonInteractivePermissionPolicy(value) {
	return value === "deny" || value === "fail";
}
function isOutputErrorCode(value) {
	return typeof value === "string" && OUTPUT_ERROR_CODES.includes(value);
}
function isOutputErrorOrigin(value) {
	return typeof value === "string" && OUTPUT_ERROR_ORIGINS.includes(value);
}
function parseAcpError(value) {
	const record = asRecord(value);
	if (!record) return;
	if (typeof record.code !== "number" || !Number.isFinite(record.code)) return;
	if (typeof record.message !== "string" || record.message.length === 0) return;
	return {
		code: record.code,
		message: record.message,
		data: record.data
	};
}
function parseOwnerGeneration(value) {
	if (value == null) return;
	if (typeof value !== "number" || !Number.isInteger(value) || value <= 0) return null;
	return value;
}
function parseQueueRequest(raw) {
	const request = asRecord(raw);
	if (!request) return null;
	if (typeof request.type !== "string" || typeof request.requestId !== "string") return null;
	const ownerGeneration = parseOwnerGeneration(request.ownerGeneration);
	if (ownerGeneration === null) return null;
	const timeoutRaw = request.timeoutMs;
	const timeoutMs = typeof timeoutRaw === "number" && Number.isFinite(timeoutRaw) && timeoutRaw > 0 ? Math.round(timeoutRaw) : void 0;
	if (request.type === "submit_prompt") {
		const resumePolicy = request.resumePolicy == null ? void 0 : isSessionResumePolicy(request.resumePolicy) ? request.resumePolicy : null;
		const nonInteractivePermissions = request.nonInteractivePermissions == null ? void 0 : isNonInteractivePermissionPolicy(request.nonInteractivePermissions) ? request.nonInteractivePermissions : null;
		const suppressSdkConsoleErrors = request.suppressSdkConsoleErrors == null ? void 0 : typeof request.suppressSdkConsoleErrors === "boolean" ? request.suppressSdkConsoleErrors : null;
		const prompt = request.prompt == null ? void 0 : isPromptInput(request.prompt) ? request.prompt : null;
		if (typeof request.message !== "string" || !isPermissionMode(request.permissionMode) || resumePolicy === null || prompt === null || nonInteractivePermissions === null || suppressSdkConsoleErrors === null || typeof request.waitForCompletion !== "boolean") return null;
		return {
			type: "submit_prompt",
			requestId: request.requestId,
			ownerGeneration,
			message: request.message,
			prompt: prompt ?? textPrompt(request.message),
			permissionMode: request.permissionMode,
			...resumePolicy !== void 0 ? { resumePolicy } : {},
			nonInteractivePermissions,
			timeoutMs,
			...suppressSdkConsoleErrors !== void 0 ? { suppressSdkConsoleErrors } : {},
			waitForCompletion: request.waitForCompletion
		};
	}
	if (request.type === "cancel_prompt") return {
		type: "cancel_prompt",
		requestId: request.requestId,
		ownerGeneration
	};
	if (request.type === "set_mode") {
		if (typeof request.modeId !== "string" || request.modeId.trim().length === 0) return null;
		return {
			type: "set_mode",
			requestId: request.requestId,
			ownerGeneration,
			modeId: request.modeId,
			timeoutMs
		};
	}
	if (request.type === "set_model") {
		if (typeof request.modelId !== "string" || request.modelId.trim().length === 0) return null;
		return {
			type: "set_model",
			requestId: request.requestId,
			ownerGeneration,
			modelId: request.modelId,
			timeoutMs
		};
	}
	if (request.type === "set_config_option") {
		if (typeof request.configId !== "string" || request.configId.trim().length === 0 || typeof request.value !== "string" || request.value.trim().length === 0) return null;
		return {
			type: "set_config_option",
			requestId: request.requestId,
			ownerGeneration,
			configId: request.configId,
			value: request.value,
			timeoutMs
		};
	}
	return null;
}
function parseSessionSendResult(raw) {
	const result = asRecord(raw);
	if (!result) return null;
	if (typeof result.stopReason !== "string" || typeof result.sessionId !== "string" || typeof result.resumed !== "boolean") return null;
	const permissionStats = asRecord(result.permissionStats);
	const record = asRecord(result.record);
	if (!permissionStats || !record) return null;
	if (!(typeof permissionStats.requested === "number" && typeof permissionStats.approved === "number" && typeof permissionStats.denied === "number" && typeof permissionStats.cancelled === "number")) return null;
	if (!(typeof record.acpxRecordId === "string" && typeof record.acpSessionId === "string" && typeof record.agentCommand === "string" && typeof record.cwd === "string" && typeof record.createdAt === "string" && typeof record.lastUsedAt === "string" && Array.isArray(record.messages) && typeof record.updated_at === "string" && typeof record.lastSeq === "number" && Number.isInteger(record.lastSeq) && !!record.eventLog && typeof record.eventLog === "object")) return null;
	return result;
}
function parseQueueOwnerMessage(raw) {
	const message = asRecord(raw);
	if (!message || typeof message.type !== "string") return null;
	if (typeof message.requestId !== "string") return null;
	const ownerGeneration = parseOwnerGeneration(message.ownerGeneration);
	if (ownerGeneration === null) return null;
	if (message.type === "accepted") return {
		type: "accepted",
		requestId: message.requestId,
		ownerGeneration
	};
	if (message.type === "event") {
		if (!isAcpJsonRpcMessage(message.message)) return null;
		return {
			type: "event",
			requestId: message.requestId,
			ownerGeneration,
			message: message.message
		};
	}
	if (message.type === "result") {
		const parsedResult = parseSessionSendResult(message.result);
		if (!parsedResult) return null;
		return {
			type: "result",
			requestId: message.requestId,
			ownerGeneration,
			result: parsedResult
		};
	}
	if (message.type === "cancel_result") {
		if (typeof message.cancelled !== "boolean") return null;
		return {
			type: "cancel_result",
			requestId: message.requestId,
			ownerGeneration,
			cancelled: message.cancelled
		};
	}
	if (message.type === "set_mode_result") {
		if (typeof message.modeId !== "string") return null;
		return {
			type: "set_mode_result",
			requestId: message.requestId,
			ownerGeneration,
			modeId: message.modeId
		};
	}
	if (message.type === "set_model_result") {
		if (typeof message.modelId !== "string") return null;
		return {
			type: "set_model_result",
			requestId: message.requestId,
			ownerGeneration,
			modelId: message.modelId
		};
	}
	if (message.type === "set_config_option_result") {
		const response = asRecord(message.response);
		if (!response || !Array.isArray(response.configOptions)) return null;
		return {
			type: "set_config_option_result",
			requestId: message.requestId,
			ownerGeneration,
			response
		};
	}
	if (message.type === "error") {
		if (typeof message.message !== "string" || !isOutputErrorCode(message.code) || !isOutputErrorOrigin(message.origin)) return null;
		const detailCode = typeof message.detailCode === "string" && message.detailCode.trim().length > 0 ? message.detailCode : void 0;
		const retryable = typeof message.retryable === "boolean" ? message.retryable : void 0;
		const acp = parseAcpError(message.acp);
		const outputAlreadyEmitted = typeof message.outputAlreadyEmitted === "boolean" ? message.outputAlreadyEmitted : void 0;
		return {
			type: "error",
			requestId: message.requestId,
			ownerGeneration,
			code: message.code,
			detailCode,
			origin: message.origin,
			message: message.message,
			retryable,
			acp,
			...outputAlreadyEmitted === void 0 ? {} : { outputAlreadyEmitted }
		};
	}
	return null;
}
//#endregion
//#region src/cli/queue/ipc-server.ts
function makeQueueOwnerError(requestId, message, detailCode, options = {}) {
	return {
		type: "error",
		requestId,
		ownerGeneration: void 0,
		code: "RUNTIME",
		detailCode,
		origin: "queue",
		retryable: options.retryable,
		message
	};
}
function makeQueueOwnerErrorFromUnknown(requestId, error, detailCode, options = {}) {
	const normalized = normalizeOutputError(error, {
		defaultCode: "RUNTIME",
		origin: "queue",
		detailCode,
		retryable: options.retryable
	});
	return {
		type: "error",
		requestId,
		code: normalized.code,
		detailCode: normalized.detailCode,
		origin: normalized.origin,
		message: normalized.message,
		retryable: normalized.retryable,
		acp: normalized.acp
	};
}
function writeQueueMessage(socket, message) {
	if (socket.destroyed || !socket.writable) return;
	socket.write(`${JSON.stringify(message)}\n`);
}
var SessionQueueOwner = class SessionQueueOwner {
	server;
	controlHandlers;
	ownerGeneration;
	maxQueueDepth;
	onQueueDepthChanged;
	pending = [];
	waiters = [];
	closed = false;
	constructor(server, controlHandlers, lease, options) {
		this.server = server;
		this.controlHandlers = controlHandlers;
		this.ownerGeneration = lease.ownerGeneration;
		this.maxQueueDepth = Math.max(1, Math.round(options.maxQueueDepth));
		this.onQueueDepthChanged = options.onQueueDepthChanged;
	}
	static async start(lease, controlHandlers, options = { maxQueueDepth: 16 }) {
		const ownerRef = { current: void 0 };
		const server = net.createServer((socket) => {
			ownerRef.current?.handleConnection(socket);
		});
		ownerRef.current = new SessionQueueOwner(server, controlHandlers, lease, options);
		await new Promise((resolve, reject) => {
			const onListening = () => {
				server.off("error", onError);
				resolve();
			};
			const onError = (error) => {
				server.off("listening", onListening);
				reject(error);
			};
			server.once("listening", onListening);
			server.once("error", onError);
			server.listen(lease.socketPath);
		});
		return ownerRef.current;
	}
	async close() {
		if (this.closed) return;
		this.closed = true;
		for (const waiter of this.waiters.splice(0)) waiter(void 0);
		for (const task of this.pending.splice(0)) {
			if (task.waitForCompletion) task.send(makeQueueOwnerError(task.requestId, "Queue owner shutting down before prompt execution", "QUEUE_OWNER_SHUTTING_DOWN", { retryable: true }));
			task.close();
		}
		this.emitQueueDepth();
		await new Promise((resolve) => {
			this.server.close(() => resolve());
		});
	}
	async nextTask(timeoutMs) {
		if (this.pending.length > 0) {
			const task = this.pending.shift();
			this.emitQueueDepth();
			if (task) recordPerfDuration("queue.owner.wait_ms", Date.now() - task.enqueuedAt);
			return task;
		}
		if (this.closed) return;
		return await new Promise((resolve) => {
			const timer = timeoutMs != null && setTimeout(() => {
				const index = this.waiters.indexOf(waiter);
				if (index >= 0) this.waiters.splice(index, 1);
				resolve(void 0);
			}, Math.max(0, timeoutMs));
			const waiter = (task) => {
				if (timer) clearTimeout(timer);
				resolve(task);
			};
			this.waiters.push(waiter);
		});
	}
	queueDepth() {
		return this.pending.length;
	}
	emitQueueDepth() {
		this.onQueueDepthChanged?.(this.pending.length);
	}
	enqueue(task) {
		if (this.closed) {
			if (task.waitForCompletion) task.send(makeQueueOwnerError(task.requestId, "Queue owner is shutting down", "QUEUE_OWNER_SHUTTING_DOWN", { retryable: true }));
			task.close();
			return;
		}
		const waiter = this.waiters.shift();
		if (waiter) {
			waiter(task);
			return;
		}
		if (this.pending.length >= this.maxQueueDepth) {
			if (task.waitForCompletion) task.send({
				...makeQueueOwnerError(task.requestId, `Queue owner is overloaded (${this.pending.length}/${this.maxQueueDepth} queued)`, "QUEUE_OWNER_OVERLOADED", { retryable: true }),
				ownerGeneration: this.ownerGeneration
			});
			task.close();
			return;
		}
		this.pending.push(task);
		this.emitQueueDepth();
	}
	handleControlRequest(options) {
		writeQueueMessage(options.socket, {
			type: "accepted",
			requestId: options.requestId,
			ownerGeneration: this.ownerGeneration
		});
		options.run().then((message) => {
			writeQueueMessage(options.socket, {
				...message,
				ownerGeneration: this.ownerGeneration
			});
		}).catch((error) => {
			writeQueueMessage(options.socket, {
				...makeQueueOwnerErrorFromUnknown(options.requestId, error, "QUEUE_CONTROL_REQUEST_FAILED"),
				ownerGeneration: this.ownerGeneration
			});
		}).finally(() => {
			if (!options.socket.destroyed) options.socket.end();
		});
	}
	handleConnection(socket) {
		socket.setEncoding("utf8");
		if (this.closed) {
			writeQueueMessage(socket, makeQueueOwnerError("unknown", "Queue owner is closed", "QUEUE_OWNER_CLOSED", { retryable: true }));
			socket.end();
			return;
		}
		let buffer = "";
		let handled = false;
		const fail = (requestId, message, detailCode) => {
			writeQueueMessage(socket, {
				...makeQueueOwnerError(requestId, message, detailCode, { retryable: false }),
				ownerGeneration: this.ownerGeneration
			});
			socket.end();
		};
		const processLine = (line) => {
			if (handled) return;
			handled = true;
			let parsed;
			try {
				parsed = JSON.parse(line);
			} catch {
				fail("unknown", "Invalid queue request payload", "QUEUE_REQUEST_PAYLOAD_INVALID_JSON");
				return;
			}
			const request = parseQueueRequest(parsed);
			if (!request) {
				fail("unknown", "Invalid queue request", "QUEUE_REQUEST_INVALID");
				return;
			}
			if (request.ownerGeneration !== void 0 && this.ownerGeneration !== void 0 && request.ownerGeneration !== this.ownerGeneration) {
				fail(request.requestId, "Queue request targeted a stale queue owner generation", "QUEUE_OWNER_GENERATION_MISMATCH");
				return;
			}
			if (request.type === "cancel_prompt") {
				this.handleControlRequest({
					socket,
					requestId: request.requestId,
					run: async () => ({
						type: "cancel_result",
						requestId: request.requestId,
						cancelled: await this.controlHandlers.cancelPrompt()
					})
				});
				return;
			}
			if (request.type === "set_mode") {
				this.handleControlRequest({
					socket,
					requestId: request.requestId,
					run: async () => {
						await this.controlHandlers.setSessionMode(request.modeId, request.timeoutMs);
						return {
							type: "set_mode_result",
							requestId: request.requestId,
							modeId: request.modeId
						};
					}
				});
				return;
			}
			if (request.type === "set_model") {
				this.handleControlRequest({
					socket,
					requestId: request.requestId,
					run: async () => {
						await this.controlHandlers.setSessionModel(request.modelId, request.timeoutMs);
						return {
							type: "set_model_result",
							requestId: request.requestId,
							modelId: request.modelId
						};
					}
				});
				return;
			}
			if (request.type === "set_config_option") {
				this.handleControlRequest({
					socket,
					requestId: request.requestId,
					run: async () => ({
						type: "set_config_option_result",
						requestId: request.requestId,
						response: await this.controlHandlers.setSessionConfigOption(request.configId, request.value, request.timeoutMs)
					})
				});
				return;
			}
			const task = {
				requestId: request.requestId,
				message: request.message,
				prompt: request.prompt ?? textPrompt(request.message),
				permissionMode: request.permissionMode,
				resumePolicy: request.resumePolicy,
				nonInteractivePermissions: request.nonInteractivePermissions,
				timeoutMs: request.timeoutMs,
				suppressSdkConsoleErrors: request.suppressSdkConsoleErrors,
				waitForCompletion: request.waitForCompletion,
				enqueuedAt: Date.now(),
				send: (message) => {
					writeQueueMessage(socket, {
						...message,
						ownerGeneration: this.ownerGeneration
					});
				},
				close: () => {
					if (!socket.destroyed) socket.end();
				}
			};
			writeQueueMessage(socket, {
				type: "accepted",
				requestId: request.requestId,
				ownerGeneration: this.ownerGeneration
			});
			if (!request.waitForCompletion) task.close();
			this.enqueue(task);
		};
		socket.on("data", (chunk) => {
			buffer += chunk;
			let index = buffer.indexOf("\n");
			while (index >= 0) {
				const line = buffer.slice(0, index).trim();
				buffer = buffer.slice(index + 1);
				if (line.length > 0) processLine(line);
				index = buffer.indexOf("\n");
			}
		});
		socket.on("error", () => {});
	}
};
//#endregion
//#region src/cli/queue/ipc.ts
const MAX_MESSAGE_BUFFER_SIZE = 10 * 1024 * 1024;
const STALE_OWNER_PROTOCOL_DETAIL_CODES = new Set(["QUEUE_PROTOCOL_MALFORMED_MESSAGE", "QUEUE_PROTOCOL_UNEXPECTED_RESPONSE"]);
async function maybeRecoverStaleOwnerAfterProtocolMismatch(params) {
	if (!(params.error instanceof QueueProtocolError)) return false;
	const detailCode = params.error.detailCode;
	if (!detailCode || !STALE_OWNER_PROTOCOL_DETAIL_CODES.has(detailCode)) return false;
	await terminateQueueOwnerForSession(params.sessionId).catch(() => {});
	incrementPerfCounter("queue.owner.stale_recovered");
	if (params.verbose) process.stderr.write(`[acpx] dropped stale queue owner metadata after protocol mismatch for session ${params.sessionId} (${detailCode})\n`);
	return true;
}
function assertOwnerGeneration(owner, message) {
	if (owner.ownerGeneration !== void 0 && message.ownerGeneration !== void 0 && message.ownerGeneration !== owner.ownerGeneration) throw new QueueProtocolError("Queue owner returned mismatched generation", {
		detailCode: "QUEUE_OWNER_GENERATION_MISMATCH",
		origin: "queue",
		retryable: true
	});
	return message;
}
function makeMalformedQueueMessageError() {
	return new QueueProtocolError("Queue owner sent malformed message", {
		detailCode: "QUEUE_PROTOCOL_MALFORMED_MESSAGE",
		origin: "queue",
		retryable: true
	});
}
function parseQueueOwnerResponseLine(owner, requestId, line) {
	let parsed;
	try {
		parsed = JSON.parse(line);
	} catch {
		throw new QueueProtocolError("Queue owner sent invalid JSON payload", {
			detailCode: "QUEUE_PROTOCOL_INVALID_JSON",
			origin: "queue",
			retryable: true
		});
	}
	const parsedMessage = parseQueueOwnerMessage(parsed);
	if (!parsedMessage) throw makeMalformedQueueMessageError();
	const message = assertOwnerGeneration(owner, parsedMessage);
	if (message.requestId !== requestId) throw makeMalformedQueueMessageError();
	return message;
}
async function runQueueOwnerRequest(options) {
	const socket = await connectToQueueOwner(options.owner);
	if (!socket) return;
	socket.setEncoding("utf8");
	return await new Promise((resolve, reject) => {
		let settled = false;
		let buffer = "";
		const state = { acknowledged: false };
		const finishResolve = (result) => {
			if (settled) return;
			settled = true;
			socket.removeAllListeners();
			if (!socket.destroyed) socket.end();
			resolve(result);
		};
		const finishReject = (error) => {
			if (settled) return;
			settled = true;
			socket.removeAllListeners();
			if (!socket.destroyed) socket.destroy();
			reject(error);
		};
		const controls = {
			state,
			resolve: finishResolve,
			reject: finishReject
		};
		const processLine = (line) => {
			let message;
			try {
				message = parseQueueOwnerResponseLine(options.owner, options.request.requestId, line);
			} catch (error) {
				finishReject(error);
				return;
			}
			if (message.type === "accepted") {
				state.acknowledged = true;
				options.onAccepted?.(controls);
				return;
			}
			options.onMessage(message, controls);
		};
		socket.on("data", (chunk) => {
			buffer += chunk;
			if (buffer.length > 10485760) {
				socket.destroy();
				finishReject(/* @__PURE__ */ new Error(`Message buffer exceeded ${MAX_MESSAGE_BUFFER_SIZE} bytes`));
				return;
			}
			let index = buffer.indexOf("\n");
			while (index >= 0) {
				const line = buffer.slice(0, index).trim();
				buffer = buffer.slice(index + 1);
				if (line.length > 0) processLine(line);
				index = buffer.indexOf("\n");
			}
		});
		socket.once("error", (error) => {
			finishReject(error);
		});
		socket.once("close", () => {
			if (settled) return;
			options.onClose(controls);
		});
		socket.write(`${JSON.stringify(options.request)}\n`);
	});
}
async function submitToQueueOwner(owner, options) {
	const requestId = randomUUID();
	const request = {
		type: "submit_prompt",
		requestId,
		ownerGeneration: owner.ownerGeneration,
		message: options.message,
		prompt: options.prompt,
		permissionMode: options.permissionMode,
		resumePolicy: options.resumePolicy,
		nonInteractivePermissions: options.nonInteractivePermissions,
		timeoutMs: options.timeoutMs,
		suppressSdkConsoleErrors: options.suppressSdkConsoleErrors,
		waitForCompletion: options.waitForCompletion
	};
	options.outputFormatter.setContext({ sessionId: options.sessionId });
	return await runQueueOwnerRequest({
		owner,
		request,
		onAccepted: ({ resolve }) => {
			options.outputFormatter.setContext({ sessionId: options.sessionId });
			if (!options.waitForCompletion) resolve({
				queued: true,
				sessionId: options.sessionId,
				requestId
			});
		},
		onMessage: (message, { state, resolve, reject }) => {
			if (message.type === "error") {
				options.outputFormatter.setContext({ sessionId: options.sessionId });
				const queueErrorAlreadyEmitted = options.errorEmissionPolicy?.queueErrorAlreadyEmitted ?? true;
				if (!(message.outputAlreadyEmitted === true) || !queueErrorAlreadyEmitted) {
					options.outputFormatter.onError({
						code: message.code ?? "RUNTIME",
						detailCode: message.detailCode,
						origin: message.origin ?? "queue",
						message: message.message,
						retryable: message.retryable,
						acp: message.acp
					});
					options.outputFormatter.flush();
				}
				reject(new QueueConnectionError(message.message, {
					outputCode: message.code,
					detailCode: message.detailCode,
					origin: message.origin ?? "queue",
					retryable: message.retryable,
					acp: message.acp,
					...queueErrorAlreadyEmitted ? { outputAlreadyEmitted: true } : {}
				}));
				return;
			}
			if (!state.acknowledged) {
				reject(new QueueConnectionError("Queue owner did not acknowledge request", {
					detailCode: "QUEUE_ACK_MISSING",
					origin: "queue",
					retryable: true
				}));
				return;
			}
			if (message.type === "event") {
				options.outputFormatter.onAcpMessage(message.message);
				return;
			}
			if (message.type === "result") {
				options.outputFormatter.flush();
				resolve(message.result);
				return;
			}
			reject(new QueueProtocolError("Queue owner returned unexpected response", {
				detailCode: "QUEUE_PROTOCOL_UNEXPECTED_RESPONSE",
				origin: "queue",
				retryable: true
			}));
		},
		onClose: ({ state, resolve, reject }) => {
			if (!state.acknowledged) {
				reject(new QueueConnectionError("Queue owner disconnected before acknowledging request", {
					detailCode: "QUEUE_DISCONNECTED_BEFORE_ACK",
					origin: "queue",
					retryable: true
				}));
				return;
			}
			if (!options.waitForCompletion) {
				resolve({
					queued: true,
					sessionId: options.sessionId,
					requestId
				});
				return;
			}
			reject(new QueueConnectionError("Queue owner disconnected before prompt completion", {
				detailCode: "QUEUE_DISCONNECTED_BEFORE_COMPLETION",
				origin: "queue",
				retryable: true
			}));
		}
	});
}
async function submitControlToQueueOwner(owner, request, isExpectedResponse) {
	return await runQueueOwnerRequest({
		owner,
		request,
		onMessage: (message, { state, resolve, reject }) => {
			if (message.type === "error") {
				reject(new QueueConnectionError(message.message, {
					outputCode: message.code,
					detailCode: message.detailCode,
					origin: message.origin ?? "queue",
					retryable: message.retryable,
					acp: message.acp
				}));
				return;
			}
			if (!state.acknowledged) {
				reject(new QueueConnectionError("Queue owner did not acknowledge request", {
					detailCode: "QUEUE_ACK_MISSING",
					origin: "queue",
					retryable: true
				}));
				return;
			}
			if (!isExpectedResponse(message)) {
				reject(new QueueProtocolError("Queue owner returned unexpected response", {
					detailCode: "QUEUE_PROTOCOL_UNEXPECTED_RESPONSE",
					origin: "queue",
					retryable: true
				}));
				return;
			}
			resolve(message);
		},
		onClose: ({ state, reject }) => {
			if (!state.acknowledged) {
				reject(new QueueConnectionError("Queue owner disconnected before acknowledging request", {
					detailCode: "QUEUE_DISCONNECTED_BEFORE_ACK",
					origin: "queue",
					retryable: true
				}));
				return;
			}
			reject(new QueueConnectionError("Queue owner disconnected before responding", {
				detailCode: "QUEUE_DISCONNECTED_BEFORE_COMPLETION",
				origin: "queue",
				retryable: true
			}));
		}
	});
}
async function submitCancelToQueueOwner(owner) {
	const request = {
		type: "cancel_prompt",
		requestId: randomUUID(),
		ownerGeneration: owner.ownerGeneration
	};
	const response = await submitControlToQueueOwner(owner, request, (message) => message.type === "cancel_result");
	if (!response) return;
	if (response.requestId !== request.requestId) throw new QueueProtocolError("Queue owner returned mismatched cancel response", {
		detailCode: "QUEUE_PROTOCOL_MALFORMED_MESSAGE",
		origin: "queue",
		retryable: true
	});
	return response.cancelled;
}
async function submitSetModeToQueueOwner(owner, modeId, timeoutMs) {
	const request = {
		type: "set_mode",
		requestId: randomUUID(),
		ownerGeneration: owner.ownerGeneration,
		modeId,
		timeoutMs
	};
	const response = await submitControlToQueueOwner(owner, request, (message) => message.type === "set_mode_result");
	if (!response) return;
	if (response.requestId !== request.requestId) throw new QueueProtocolError("Queue owner returned mismatched set_mode response", {
		detailCode: "QUEUE_PROTOCOL_MALFORMED_MESSAGE",
		origin: "queue",
		retryable: true
	});
	return true;
}
async function submitSetModelToQueueOwner(owner, modelId, timeoutMs) {
	const request = {
		type: "set_model",
		requestId: randomUUID(),
		ownerGeneration: owner.ownerGeneration,
		modelId,
		timeoutMs
	};
	const response = await submitControlToQueueOwner(owner, request, (message) => message.type === "set_model_result");
	if (!response) return;
	if (response.requestId !== request.requestId) throw new QueueProtocolError("Queue owner returned mismatched set_model response", {
		detailCode: "QUEUE_PROTOCOL_MALFORMED_MESSAGE",
		origin: "queue",
		retryable: true
	});
	return true;
}
async function submitSetConfigOptionToQueueOwner(owner, configId, value, timeoutMs) {
	const request = {
		type: "set_config_option",
		requestId: randomUUID(),
		ownerGeneration: owner.ownerGeneration,
		configId,
		value,
		timeoutMs
	};
	const response = await submitControlToQueueOwner(owner, request, (message) => message.type === "set_config_option_result");
	if (!response) return;
	if (response.requestId !== request.requestId) throw new QueueProtocolError("Queue owner returned mismatched set_config_option response", {
		detailCode: "QUEUE_PROTOCOL_MALFORMED_MESSAGE",
		origin: "queue",
		retryable: true
	});
	return response.response;
}
async function trySubmitToRunningOwner(options) {
	const owner = await readQueueOwnerRecord(options.sessionId);
	if (!owner) return;
	let submitted;
	try {
		submitted = await submitToQueueOwner(owner, options);
	} catch (error) {
		if (await maybeRecoverStaleOwnerAfterProtocolMismatch({
			sessionId: options.sessionId,
			owner,
			error,
			verbose: options.verbose
		})) return;
		throw error;
	}
	if (submitted) {
		if (options.verbose) process.stderr.write(`[acpx] queued prompt on active owner pid ${owner.pid} for session ${options.sessionId}\n`);
		return submitted;
	}
	if (!(await probeQueueOwnerHealth(options.sessionId)).hasLease) return;
	throw new QueueConnectionError("Session queue owner is running but not accepting queue requests", {
		detailCode: "QUEUE_NOT_ACCEPTING_REQUESTS",
		origin: "queue",
		retryable: true
	});
}
async function tryCancelOnRunningOwner(options) {
	const owner = await readQueueOwnerRecord(options.sessionId);
	if (!owner) return;
	const cancelled = await submitCancelToQueueOwner(owner);
	if (cancelled !== void 0) {
		if (options.verbose) process.stderr.write(`[acpx] requested cancel on active owner pid ${owner.pid} for session ${options.sessionId}\n`);
		return cancelled;
	}
	if (!(await probeQueueOwnerHealth(options.sessionId)).hasLease) return;
	throw new QueueConnectionError("Session queue owner is running but not accepting cancel requests", {
		detailCode: "QUEUE_NOT_ACCEPTING_REQUESTS",
		origin: "queue",
		retryable: true
	});
}
async function trySetModeOnRunningOwner(sessionId, modeId, timeoutMs, verbose) {
	const owner = await readQueueOwnerRecord(sessionId);
	if (!owner) return;
	if (await submitSetModeToQueueOwner(owner, modeId, timeoutMs)) {
		if (verbose) process.stderr.write(`[acpx] requested session/set_mode on owner pid ${owner.pid} for session ${sessionId}\n`);
		return true;
	}
	if (!(await probeQueueOwnerHealth(sessionId)).hasLease) return;
	throw new QueueConnectionError("Session queue owner is running but not accepting set_mode requests", {
		detailCode: "QUEUE_NOT_ACCEPTING_REQUESTS",
		origin: "queue",
		retryable: true
	});
}
async function trySetModelOnRunningOwner(sessionId, modelId, timeoutMs, verbose) {
	const owner = await readQueueOwnerRecord(sessionId);
	if (!owner) return;
	if (await submitSetModelToQueueOwner(owner, modelId, timeoutMs)) {
		if (verbose) process.stderr.write(`[acpx] requested session/set_model on owner pid ${owner.pid} for session ${sessionId}\n`);
		return true;
	}
	if (!(await probeQueueOwnerHealth(sessionId)).hasLease) return;
	throw new QueueConnectionError("Session queue owner is running but not accepting set_model requests", {
		detailCode: "QUEUE_NOT_ACCEPTING_REQUESTS",
		origin: "queue",
		retryable: true
	});
}
async function trySetConfigOptionOnRunningOwner(sessionId, configId, value, timeoutMs, verbose) {
	const owner = await readQueueOwnerRecord(sessionId);
	if (!owner) return;
	const response = await submitSetConfigOptionToQueueOwner(owner, configId, value, timeoutMs);
	if (response) {
		if (verbose) process.stderr.write(`[acpx] requested session/set_config_option on owner pid ${owner.pid} for session ${sessionId}\n`);
		return response;
	}
	if (!(await probeQueueOwnerHealth(sessionId)).hasLease) return;
	throw new QueueConnectionError("Session queue owner is running but not accepting set_config_option requests", {
		detailCode: "QUEUE_NOT_ACCEPTING_REQUESTS",
		origin: "queue",
		retryable: true
	});
}
//#endregion
export { trySubmitToRunningOwner as a, isProcessAlive as c, terminateProcess as d, terminateQueueOwnerForSession as f, trySetModelOnRunningOwner as i, refreshQueueOwnerLease as l, waitMs as m, trySetConfigOptionOnRunningOwner as n, SessionQueueOwner as o, tryAcquireQueueOwnerLease as p, trySetModeOnRunningOwner as r, probeQueueOwnerHealth as s, tryCancelOnRunningOwner as t, releaseQueueOwnerLease as u };

//# sourceMappingURL=ipc-BM335WFg.js.map