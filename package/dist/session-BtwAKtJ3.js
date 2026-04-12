import { t as __exportAll } from "./rolldown-runtime-CiIaOW0V.js";
import { V as QueueConnectionError, b as isRetryablePromptError, c as startPerfTimer, g as textPrompt, h as promptToDisplayText, i as measurePerf, n as getPerfMetricsSnapshot, o as resetPerfMetrics, r as incrementPerfCounter, s as setPerfGauge, t as formatPerfMetric, u as normalizeRuntimeSessionId, v as formatErrorMessage, x as normalizeOutputError } from "./perf-metrics-D0um6IR6.js";
import { A as resolveSessionRecord, C as findGitRepositoryRoot, D as listSessions, E as isoNow, F as sessionBaseDir, H as TimeoutError, I as sessionEventActivePath, L as sessionEventLockPath, O as listSessionsForAgent, P as defaultSessionEventLog, R as sessionEventSegmentPath, S as absolutePath, T as findSessionByDirectoryWalk, U as withInterrupt, V as InterruptedError, W as withTimeout, _ as recordSessionUpdate, a as applyConversation, c as setCurrentModelId, d as syncAdvertisedModelState, f as cloneSessionAcpxState, g as recordPromptSubmission, h as recordClientOperation, i as connectAndLoadSession, j as writeSessionRecord, k as normalizeName, l as setDesiredModeId, m as createSessionConversation, n as withConnectedSession, o as applyLifecycleSnapshotToRecord, p as cloneSessionConversation, r as sessionOptionsFromRecord, t as runPromptTurn, u as setDesiredModelId, v as trimConversationForRuntime, w as findSession, y as AcpClient } from "./prompt-turn-CXMtXBl-.js";
import { n as isAcpJsonRpcMessage } from "./jsonrpc-DSxh2w5R.js";
import { a as trySubmitToRunningOwner, c as isProcessAlive, d as terminateProcess, f as terminateQueueOwnerForSession, i as trySetModelOnRunningOwner, l as refreshQueueOwnerLease, m as waitMs, n as trySetConfigOptionOnRunningOwner, o as SessionQueueOwner, p as tryAcquireQueueOwnerLease, r as trySetModeOnRunningOwner, t as tryCancelOnRunningOwner, u as releaseQueueOwnerLease } from "./ipc-BM335WFg.js";
import fs, { realpathSync } from "node:fs";
import path from "node:path";
import fs$1 from "node:fs/promises";
import { spawn } from "node:child_process";
//#region src/cli/session/contracts.ts
const DEFAULT_QUEUE_OWNER_TTL_MS = 3e5;
function normalizeQueueOwnerTtlMs(ttlMs) {
	if (ttlMs == null) return DEFAULT_QUEUE_OWNER_TTL_MS;
	if (!Number.isFinite(ttlMs) || ttlMs < 0) return DEFAULT_QUEUE_OWNER_TTL_MS;
	return Math.round(ttlMs);
}
//#endregion
//#region src/cli/session/prompt-runner.ts
async function runSessionSetModeDirect(options) {
	const result = await withConnectedSession({
		sessionRecordId: options.sessionRecordId,
		loadRecord: resolveSessionRecord,
		saveRecord: writeSessionRecord,
		mcpServers: options.mcpServers,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		timeoutMs: options.timeoutMs,
		verbose: options.verbose,
		onClientAvailable: (controller) => {
			options.onClientAvailable?.(controller);
		},
		onClientClosed: options.onClientClosed,
		run: async ({ client, sessionId, record }) => {
			await withTimeout(client.setSessionMode(sessionId, options.modeId), options.timeoutMs);
			setDesiredModeId(record, options.modeId);
		}
	});
	return {
		record: result.record,
		resumed: result.resumed,
		loadError: result.loadError
	};
}
async function runSessionSetModelDirect(options) {
	const result = await withConnectedSession({
		sessionRecordId: options.sessionRecordId,
		loadRecord: resolveSessionRecord,
		saveRecord: writeSessionRecord,
		mcpServers: options.mcpServers,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		timeoutMs: options.timeoutMs,
		verbose: options.verbose,
		onClientAvailable: (controller) => {
			options.onClientAvailable?.(controller);
		},
		onClientClosed: options.onClientClosed,
		run: async ({ client, sessionId, record }) => {
			await withTimeout(client.setSessionModel(sessionId, options.modelId), options.timeoutMs);
			setDesiredModelId(record, options.modelId);
			setCurrentModelId(record, options.modelId);
		}
	});
	return {
		record: result.record,
		resumed: result.resumed,
		loadError: result.loadError
	};
}
async function runSessionSetConfigOptionDirect(options) {
	const result = await withConnectedSession({
		sessionRecordId: options.sessionRecordId,
		loadRecord: resolveSessionRecord,
		saveRecord: writeSessionRecord,
		mcpServers: options.mcpServers,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		timeoutMs: options.timeoutMs,
		verbose: options.verbose,
		onClientAvailable: (controller) => {
			options.onClientAvailable?.(controller);
		},
		onClientClosed: options.onClientClosed,
		run: async ({ client, sessionId, record }) => {
			const response = await withTimeout(client.setSessionConfigOption(sessionId, options.configId, options.value), options.timeoutMs);
			if (options.configId === "mode") setDesiredModeId(record, options.value);
			return response;
		}
	});
	return {
		record: result.record,
		response: result.value,
		resumed: result.resumed,
		loadError: result.loadError
	};
}
//#endregion
//#region src/cli/session/session-control.ts
async function cancelSessionPrompt(options) {
	const cancelled = await tryCancelOnRunningOwner(options);
	return {
		sessionId: options.sessionId,
		cancelled: cancelled === true
	};
}
async function setSessionMode(options) {
	if (await trySetModeOnRunningOwner(options.sessionId, options.modeId, options.timeoutMs, options.verbose)) {
		const record = await resolveSessionRecord(options.sessionId);
		setDesiredModeId(record, options.modeId);
		await writeSessionRecord(record);
		return {
			record,
			resumed: false
		};
	}
	return await runSessionSetModeDirect({
		sessionRecordId: options.sessionId,
		modeId: options.modeId,
		mcpServers: options.mcpServers,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		timeoutMs: options.timeoutMs,
		verbose: options.verbose
	});
}
async function setSessionModel(options) {
	if (await trySetModelOnRunningOwner(options.sessionId, options.modelId, options.timeoutMs, options.verbose)) {
		const record = await resolveSessionRecord(options.sessionId);
		setDesiredModelId(record, options.modelId);
		setCurrentModelId(record, options.modelId);
		await writeSessionRecord(record);
		return {
			record,
			resumed: false
		};
	}
	return await runSessionSetModelDirect({
		sessionRecordId: options.sessionId,
		modelId: options.modelId,
		mcpServers: options.mcpServers,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		timeoutMs: options.timeoutMs,
		verbose: options.verbose
	});
}
async function setSessionConfigOption(options) {
	const ownerResponse = await trySetConfigOptionOnRunningOwner(options.sessionId, options.configId, options.value, options.timeoutMs, options.verbose);
	if (ownerResponse) {
		const record = await resolveSessionRecord(options.sessionId);
		if (options.configId === "mode") {
			setDesiredModeId(record, options.value);
			await writeSessionRecord(record);
		}
		return {
			record,
			response: ownerResponse,
			resumed: false
		};
	}
	return await runSessionSetConfigOptionDirect({
		sessionRecordId: options.sessionId,
		configId: options.configId,
		value: options.value,
		mcpServers: options.mcpServers,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		timeoutMs: options.timeoutMs,
		verbose: options.verbose
	});
}
function firstAgentCommandToken(command) {
	const trimmed = command.trim();
	if (!trimmed) return;
	const token = trimmed.split(/\s+/, 1)[0];
	return token.length > 0 ? token : void 0;
}
async function isLikelyMatchingProcess(pid, agentCommand) {
	const expectedToken = firstAgentCommandToken(agentCommand);
	if (!expectedToken) return false;
	const procCmdline = `/proc/${pid}/cmdline`;
	try {
		const argv = (await fs$1.readFile(procCmdline, "utf8")).split("\0").map((entry) => entry.trim()).filter((entry) => entry.length > 0);
		if (argv.length === 0) return false;
		const executableBase = path.basename(argv[0]);
		const expectedBase = path.basename(expectedToken);
		return executableBase === expectedBase || argv.some((entry) => path.basename(entry) === expectedBase);
	} catch {
		return true;
	}
}
async function closeSession(sessionId) {
	const record = await resolveSessionRecord(sessionId);
	await terminateQueueOwnerForSession(record.acpxRecordId);
	if (record.pid != null && isProcessAlive(record.pid) && await isLikelyMatchingProcess(record.pid, record.agentCommand)) await terminateProcess(record.pid);
	record.pid = void 0;
	record.closed = true;
	record.closedAt = isoNow();
	await writeSessionRecord(record);
	return record;
}
//#endregion
//#region src/cli/session/session-management.ts
function persistSessionOptions(record, options) {
	const next = options && {
		model: typeof options.model === "string" ? options.model : void 0,
		allowed_tools: Array.isArray(options.allowedTools) ? [...options.allowedTools] : void 0,
		max_turns: typeof options.maxTurns === "number" ? options.maxTurns : void 0
	};
	if (Boolean(next && (typeof next.model === "string" && next.model.trim().length > 0 || Array.isArray(next.allowed_tools) && next.allowed_tools.length > 0 || typeof next.max_turns === "number")) && next) {
		record.acpx = {
			...record.acpx,
			session_options: next
		};
		return;
	}
	if (!record.acpx) return;
	delete record.acpx.session_options;
}
async function applyRequestedModelIfAdvertised$1(params) {
	const requestedModel = typeof params.requestedModel === "string" ? params.requestedModel.trim() : "";
	if (!requestedModel || !params.models) return false;
	if (params.models.currentModelId === requestedModel) return true;
	await withTimeout(params.client.setSessionModel(params.sessionId, requestedModel), params.timeoutMs);
	return true;
}
async function createSessionRecordWithClient(client, options) {
	const cwd = absolutePath(options.cwd);
	await withTimeout(client.start(), options.timeoutMs);
	let sessionId;
	let agentSessionId;
	let sessionModels;
	let requestedModelApplied = false;
	if (options.resumeSessionId) {
		if (!client.supportsLoadSession()) throw new Error(`Agent command "${options.agentCommand}" does not support session/load; cannot resume session ${options.resumeSessionId}`);
		try {
			const loadedSession = await withTimeout(client.loadSession(options.resumeSessionId, cwd), options.timeoutMs);
			sessionId = options.resumeSessionId;
			agentSessionId = normalizeRuntimeSessionId(loadedSession.agentSessionId);
			sessionModels = loadedSession.models;
			requestedModelApplied = await applyRequestedModelIfAdvertised$1({
				client,
				sessionId,
				requestedModel: options.sessionOptions?.model,
				models: sessionModels,
				timeoutMs: options.timeoutMs
			});
		} catch (error) {
			throw new Error(`Failed to resume ACP session ${options.resumeSessionId}: ${formatErrorMessage(error)}`, { cause: error });
		}
	} else {
		const createdSession = await withTimeout(client.createSession(cwd), options.timeoutMs);
		sessionId = createdSession.sessionId;
		agentSessionId = normalizeRuntimeSessionId(createdSession.agentSessionId);
		sessionModels = createdSession.models;
		requestedModelApplied = await applyRequestedModelIfAdvertised$1({
			client,
			sessionId,
			requestedModel: options.sessionOptions?.model,
			models: sessionModels,
			timeoutMs: options.timeoutMs
		});
	}
	const lifecycle = client.getAgentLifecycleSnapshot();
	const now = isoNow();
	const record = {
		schema: "acpx.session.v1",
		acpxRecordId: sessionId,
		acpSessionId: sessionId,
		agentSessionId,
		agentCommand: options.agentCommand,
		cwd,
		name: normalizeName(options.name),
		createdAt: now,
		lastUsedAt: now,
		lastSeq: 0,
		lastRequestId: void 0,
		eventLog: defaultSessionEventLog(sessionId),
		closed: false,
		closedAt: void 0,
		pid: lifecycle.pid,
		agentStartedAt: lifecycle.startedAt,
		protocolVersion: client.initializeResult?.protocolVersion,
		agentCapabilities: client.initializeResult?.agentCapabilities,
		...createSessionConversation(now),
		acpx: {}
	};
	persistSessionOptions(record, options.sessionOptions);
	syncAdvertisedModelState(record, sessionModels);
	if (requestedModelApplied) setCurrentModelId(record, options.sessionOptions?.model);
	await writeSessionRecord(record);
	return record;
}
async function createSessionWithClient(options) {
	const client = new AcpClient({
		agentCommand: options.agentCommand,
		cwd: absolutePath(options.cwd),
		mcpServers: options.mcpServers,
		permissionMode: options.permissionMode,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		verbose: options.verbose,
		sessionOptions: options.sessionOptions
	});
	try {
		return {
			record: await withInterrupt(async () => await createSessionRecordWithClient(client, options), async () => {
				await client.close();
			}),
			client
		};
	} catch (error) {
		await client.close();
		throw error;
	}
}
async function createSession(options) {
	const { record, client } = await createSessionWithClient(options);
	try {
		return record;
	} finally {
		await client.close();
	}
}
async function ensureSession(options) {
	const cwd = absolutePath(options.cwd);
	const gitRoot = findGitRepositoryRoot(cwd);
	const walkBoundary = options.walkBoundary ?? gitRoot ?? cwd;
	const existing = await findSessionByDirectoryWalk({
		agentCommand: options.agentCommand,
		cwd,
		name: options.name,
		boundary: walkBoundary
	});
	if (existing) {
		const requestedModel = options.sessionOptions?.model;
		if (requestedModel) return {
			record: (await setSessionModel({
				sessionId: existing.acpxRecordId,
				modelId: requestedModel,
				mcpServers: options.mcpServers,
				nonInteractivePermissions: options.nonInteractivePermissions,
				authCredentials: options.authCredentials,
				authPolicy: options.authPolicy,
				timeoutMs: options.timeoutMs,
				verbose: options.verbose
			})).record,
			created: false
		};
		return {
			record: existing,
			created: false
		};
	}
	return {
		record: await createSession({
			agentCommand: options.agentCommand,
			cwd,
			name: options.name,
			resumeSessionId: options.resumeSessionId,
			mcpServers: options.mcpServers,
			permissionMode: options.permissionMode,
			nonInteractivePermissions: options.nonInteractivePermissions,
			authCredentials: options.authCredentials,
			authPolicy: options.authPolicy,
			timeoutMs: options.timeoutMs,
			verbose: options.verbose,
			sessionOptions: options.sessionOptions
		}),
		created: true
	};
}
//#endregion
//#region src/perf-metrics-capture.ts
const PERF_METRICS_FILE_ENV = "ACPX_PERF_METRICS_FILE";
let installed = false;
let flushed = false;
let captureFilePath;
let captureRole = "cli";
let captureArgv = [];
let captureSequence = 0;
function shouldCapture() {
	return typeof captureFilePath === "string" && captureFilePath.trim().length > 0;
}
function buildPayload(reason) {
	return {
		timestamp: (/* @__PURE__ */ new Date()).toISOString(),
		pid: process.pid,
		ppid: process.ppid,
		role: captureRole,
		argv: captureArgv,
		cwd: process.cwd(),
		sequence: captureSequence,
		reason,
		metrics: getPerfMetricsSnapshot()
	};
}
function writePerfMetricsCapture(reason, resetAfterWrite) {
	if (!shouldCapture()) return false;
	const payload = buildPayload(reason);
	const metrics = payload.metrics;
	if (!(Object.keys(metrics.counters ?? {}).length > 0 || Object.keys(metrics.gauges ?? {}).length > 0 || Object.keys(metrics.timings ?? {}).length > 0)) return false;
	try {
		fs.mkdirSync(path.dirname(captureFilePath), { recursive: true });
		fs.appendFileSync(captureFilePath, `${JSON.stringify(payload)}\n`, "utf8");
		captureSequence += 1;
		if (resetAfterWrite) resetPerfMetrics();
		return true;
	} catch {
		return false;
	}
}
function checkpointPerfMetricsCapture() {
	flushed = false;
	writePerfMetricsCapture("checkpoint", true);
}
function flushPerfMetricsCapture(reason = "exit") {
	if (flushed || !shouldCapture()) return;
	flushed = true;
	writePerfMetricsCapture(reason, false);
}
function installPerfMetricsCapture(options = {}) {
	captureFilePath = options.filePath ?? process.env[PERF_METRICS_FILE_ENV];
	if (!shouldCapture()) return;
	resetPerfMetrics();
	captureRole = options.role ?? captureRole;
	captureArgv = options.argv ?? [];
	captureSequence = 0;
	flushed = false;
	if (installed) return;
	installed = true;
	process.once("exit", () => {
		flushPerfMetricsCapture("exit");
	});
	for (const signal of ["SIGINT", "SIGTERM"]) {
		const handler = () => {
			flushPerfMetricsCapture("signal");
			process.removeListener(signal, handler);
			process.kill(process.pid, signal);
		};
		process.once(signal, handler);
	}
}
//#endregion
//#region src/cli/queue/owner-turn-controller.ts
var QueueOwnerTurnController = class {
	options;
	state = "idle";
	pendingCancel = false;
	activeController;
	constructor(options) {
		this.options = options;
	}
	get lifecycleState() {
		return this.state;
	}
	get hasPendingCancel() {
		return this.pendingCancel;
	}
	beginTurn() {
		this.state = "starting";
		this.pendingCancel = false;
	}
	markPromptActive() {
		if (this.state === "starting" || this.state === "active") this.state = "active";
	}
	endTurn() {
		this.state = "idle";
		this.pendingCancel = false;
	}
	beginClosing() {
		this.state = "closing";
		this.pendingCancel = false;
		this.activeController = void 0;
	}
	setActiveController(controller) {
		this.activeController = controller;
	}
	clearActiveController() {
		this.activeController = void 0;
	}
	assertCanHandleControlRequest() {
		if (this.state === "closing") throw new QueueConnectionError("Queue owner is closing", {
			detailCode: "QUEUE_OWNER_SHUTTING_DOWN",
			origin: "queue",
			retryable: true
		});
	}
	async requestCancel() {
		const activeController = this.activeController;
		if (activeController?.hasActivePrompt()) {
			const cancelled = await activeController.requestCancelActivePrompt();
			if (cancelled) this.pendingCancel = false;
			return cancelled;
		}
		if (this.state === "starting" || this.state === "active") {
			this.pendingCancel = true;
			return true;
		}
		return false;
	}
	async applyPendingCancel() {
		const activeController = this.activeController;
		if (!this.pendingCancel || !activeController || !activeController.hasActivePrompt()) return false;
		const cancelled = await activeController.requestCancelActivePrompt();
		if (cancelled) this.pendingCancel = false;
		return cancelled;
	}
	async setSessionMode(modeId, timeoutMs) {
		this.assertCanHandleControlRequest();
		const activeController = this.activeController;
		if (activeController) {
			await this.options.withTimeout(async () => await activeController.setSessionMode(modeId), timeoutMs);
			return;
		}
		await this.options.setSessionModeFallback(modeId, timeoutMs);
	}
	async setSessionModel(modelId, timeoutMs) {
		this.assertCanHandleControlRequest();
		const activeController = this.activeController;
		if (activeController) {
			await this.options.withTimeout(async () => await activeController.setSessionModel(modelId), timeoutMs);
			return;
		}
		await this.options.setSessionModelFallback(modelId, timeoutMs);
	}
	async setSessionConfigOption(configId, value, timeoutMs) {
		this.assertCanHandleControlRequest();
		const activeController = this.activeController;
		if (activeController) return await this.options.withTimeout(async () => await activeController.setSessionConfigOption(configId, value), timeoutMs);
		return await this.options.setSessionConfigOptionFallback(configId, value, timeoutMs);
	}
};
//#endregion
//#region src/cli/session/queue-owner-process.ts
function sanitizeQueueOwnerExecArgv(execArgv = process.execArgv) {
	const sanitized = [];
	for (let index = 0; index < execArgv.length; index += 1) {
		const value = execArgv[index];
		if (value === "--experimental-test-coverage" || value === "--test") continue;
		if (value === "--test-name-pattern" || value === "--test-reporter" || value === "--test-reporter-destination") {
			index += 1;
			continue;
		}
		if (value.startsWith("--test-")) continue;
		if (value === "--inspect" || value === "--inspect-brk" || value === "--inspect-port" || value === "--inspect-publish-uid" || value.startsWith("--inspect=") || value.startsWith("--inspect-brk=") || value.startsWith("--inspect-port=") || value.startsWith("--inspect-publish-uid=") || value === "--debug-port" || value.startsWith("--debug-port=")) {
			if (value === "--inspect" || value === "--inspect-brk" || value === "--inspect-port" || value === "--inspect-publish-uid" || value === "--debug-port") index += 1;
			continue;
		}
		sanitized.push(value);
	}
	return sanitized;
}
function buildQueueOwnerArgOverride(entryPath, execArgv = process.execArgv) {
	const sanitized = sanitizeQueueOwnerExecArgv(execArgv);
	if (sanitized.length === 0) return null;
	return JSON.stringify([
		...sanitized,
		entryPath,
		"__queue-owner"
	]);
}
function resolveQueueOwnerSpawnArgs(argv = process.argv) {
	const override = process.env.ACPX_QUEUE_OWNER_ARGS;
	if (override) {
		const parsed = JSON.parse(override);
		if (Array.isArray(parsed) && parsed.length > 0 && parsed.every((value) => typeof value === "string" && value.length > 0)) return [...parsed];
		throw new Error("acpx self-spawn failed: invalid ACPX_QUEUE_OWNER_ARGS");
	}
	const entry = argv[1];
	if (!entry || entry.trim().length === 0) throw new Error("acpx self-spawn failed: missing CLI entry path");
	return [realpathSync(entry), "__queue-owner"];
}
function queueOwnerRuntimeOptionsFromSend(options) {
	return {
		sessionId: options.sessionId,
		mcpServers: options.mcpServers,
		permissionMode: options.permissionMode,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		suppressSdkConsoleErrors: options.suppressSdkConsoleErrors,
		verbose: options.verbose,
		ttlMs: options.ttlMs,
		maxQueueDepth: options.maxQueueDepth,
		promptRetries: options.promptRetries
	};
}
function buildQueueOwnerSpawnOptions(payload) {
	return {
		detached: true,
		stdio: "ignore",
		env: {
			...process.env,
			ACPX_QUEUE_OWNER_PAYLOAD: payload
		},
		windowsHide: true
	};
}
function spawnQueueOwnerProcess(options) {
	const payload = JSON.stringify(options);
	spawn(process.execPath, resolveQueueOwnerSpawnArgs(), buildQueueOwnerSpawnOptions(payload)).unref();
}
//#endregion
//#region src/session/events.ts
const LOCK_RETRY_MS = 15;
const EVENT_LOCK_STALE_MS = 15e3;
async function ensureSessionDir() {
	await fs$1.mkdir(sessionBaseDir(), { recursive: true });
}
async function pathExists(filePath) {
	try {
		await fs$1.access(filePath);
		return true;
	} catch {
		return false;
	}
}
async function statSize(filePath) {
	try {
		return (await fs$1.stat(filePath)).size;
	} catch {
		return 0;
	}
}
async function countExistingSegments(sessionId, maxSegments) {
	let count = 0;
	for (let segment = 1; segment <= maxSegments; segment += 1) if (await pathExists(sessionEventSegmentPath(sessionId, segment))) count += 1;
	if (await pathExists(sessionEventActivePath(sessionId))) count += 1;
	return count;
}
async function rotateSegments(sessionId, maxSegments) {
	const active = sessionEventActivePath(sessionId);
	const overflow = sessionEventSegmentPath(sessionId, maxSegments);
	await fs$1.unlink(overflow).catch((error) => {
		if (error.code !== "ENOENT") throw error;
	});
	for (let segment = maxSegments - 1; segment >= 1; segment -= 1) {
		const from = sessionEventSegmentPath(sessionId, segment);
		const to = sessionEventSegmentPath(sessionId, segment + 1);
		if (!await pathExists(from)) continue;
		await fs$1.rename(from, to);
	}
	if (await pathExists(active)) await fs$1.rename(active, sessionEventSegmentPath(sessionId, 1));
}
function parseEventLockPayload(raw) {
	try {
		const parsed = JSON.parse(raw);
		if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
		const record = parsed;
		return {
			pid: typeof record.pid === "number" ? record.pid : void 0,
			created_at: typeof record.created_at === "string" ? record.created_at : void 0
		};
	} catch {
		return {};
	}
}
async function removeStaleEventLock(lockPath) {
	try {
		const parsed = parseEventLockPayload(await fs$1.readFile(lockPath, "utf8"));
		const createdAtMs = parsed.created_at ? Date.parse(parsed.created_at) : NaN;
		const lockAgeMs = Number.isFinite(createdAtMs) ? Date.now() - createdAtMs : Number.POSITIVE_INFINITY;
		if (isProcessAlive(parsed.pid) && lockAgeMs <= EVENT_LOCK_STALE_MS) return false;
		await fs$1.unlink(lockPath);
		incrementPerfCounter("session.events.stale_lock_recovered");
		return true;
	} catch (error) {
		if (error.code === "ENOENT") return true;
		return false;
	}
}
async function acquireEventsLock(sessionId) {
	await ensureSessionDir();
	const lockPath = sessionEventLockPath(sessionId);
	const payload = JSON.stringify({
		pid: process.pid,
		created_at: (/* @__PURE__ */ new Date()).toISOString()
	}, null, 2);
	for (;;) try {
		await fs$1.writeFile(lockPath, `${payload}\n`, {
			encoding: "utf8",
			flag: "wx"
		});
		return { filePath: lockPath };
	} catch (error) {
		if (error.code !== "EEXIST") throw error;
		if (await removeStaleEventLock(lockPath)) continue;
		await new Promise((resolve) => {
			setTimeout(resolve, LOCK_RETRY_MS);
		});
	}
}
async function releaseEventsLock(lock) {
	await fs$1.unlink(lock.filePath).catch((error) => {
		if (error.code !== "ENOENT") throw error;
	});
}
var SessionEventWriter = class SessionEventWriter {
	record;
	lock;
	maxSegmentBytes;
	maxSegments;
	activePath;
	activeSizeBytes;
	segmentCount;
	closed = false;
	constructor(record, lock, options, state) {
		this.record = record;
		this.lock = lock;
		this.maxSegmentBytes = options.maxSegmentBytes;
		this.maxSegments = options.maxSegments;
		this.activePath = state.activePath;
		this.activeSizeBytes = state.activeSizeBytes;
		this.segmentCount = state.segmentCount;
	}
	static async open(record, options = {}) {
		const lock = await acquireEventsLock(record.acpxRecordId);
		const maxSegmentBytes = options.maxSegmentBytes ?? record.eventLog.max_segment_bytes ?? 67108864;
		const maxSegments = options.maxSegments ?? record.eventLog.max_segments ?? 5;
		const activePath = sessionEventActivePath(record.acpxRecordId);
		const activeSizeBytes = await statSize(activePath);
		const segmentCount = Number.isInteger(record.eventLog.segment_count) && record.eventLog.segment_count > 0 ? record.eventLog.segment_count : await countExistingSegments(record.acpxRecordId, maxSegments) || 1;
		return new SessionEventWriter(record, lock, {
			maxSegmentBytes,
			maxSegments
		}, {
			activePath,
			activeSizeBytes,
			segmentCount
		});
	}
	getRecord() {
		return this.record;
	}
	async appendMessage(message, options = {}) {
		await this.appendMessages([message], options);
	}
	async appendMessages(messages, options = {}) {
		if (this.closed) throw new Error("SessionEventWriter is closed");
		if (messages.length === 0) return;
		await ensureSessionDir();
		await measurePerf("session.events.append_batch", async () => {
			for (const message of messages) {
				if (!isAcpJsonRpcMessage(message)) throw new Error("Attempted to persist invalid ACP JSON-RPC payload");
				const line = `${JSON.stringify(message)}\n`;
				const lineBytes = Buffer.byteLength(line);
				if (this.activeSizeBytes > 0 && this.activeSizeBytes + lineBytes > this.maxSegmentBytes) {
					await rotateSegments(this.record.acpxRecordId, this.maxSegments);
					this.activePath = sessionEventActivePath(this.record.acpxRecordId);
					this.activeSizeBytes = 0;
					this.segmentCount = Math.min(this.segmentCount + 1, this.maxSegments);
					incrementPerfCounter("session.events.rotate");
				}
				await fs$1.appendFile(this.activePath, line, "utf8");
				this.activeSizeBytes += lineBytes;
				this.record.lastSeq += 1;
				if (Object.hasOwn(message, "id")) {
					const id = message.id;
					if (typeof id === "string" || typeof id === "number") this.record.lastRequestId = String(id);
				}
				const writeTs = (/* @__PURE__ */ new Date()).toISOString();
				this.record.lastUsedAt = writeTs;
				this.record.eventLog = {
					active_path: this.activePath,
					segment_count: this.segmentCount,
					max_segment_bytes: this.maxSegmentBytes,
					max_segments: this.maxSegments,
					last_write_at: writeTs,
					last_write_error: null
				};
			}
		});
		if (options.checkpoint === true) await writeSessionRecord(this.record);
	}
	async checkpoint() {
		if (this.closed) throw new Error("SessionEventWriter is closed");
		await writeSessionRecord(this.record);
	}
	async close(options = {}) {
		if (this.closed) return;
		try {
			if (options.checkpoint !== false) await writeSessionRecord(this.record);
		} finally {
			this.closed = true;
			await releaseEventsLock(this.lock);
		}
	}
};
//#endregion
//#region src/cli/session/runtime.ts
const INTERRUPT_CANCEL_WAIT_MS = 2500;
var QueueTaskOutputFormatter = class {
	requestId;
	send;
	constructor(task) {
		this.requestId = task.requestId;
		this.send = task.send;
	}
	setContext(_context) {}
	onAcpMessage(message) {
		this.send({
			type: "event",
			requestId: this.requestId,
			message
		});
	}
	onError(params) {
		this.send({
			type: "error",
			requestId: this.requestId,
			code: params.code,
			detailCode: params.detailCode,
			origin: params.origin,
			message: params.message,
			retryable: params.retryable,
			acp: params.acp
		});
	}
	flush() {}
};
const DISCARD_OUTPUT_FORMATTER = {
	setContext() {},
	onAcpMessage() {},
	onError() {},
	flush() {}
};
function toPromptResult(stopReason, sessionId, client) {
	return {
		stopReason,
		sessionId,
		permissionStats: client.getPermissionStats()
	};
}
async function applyRequestedModelIfAdvertised(params) {
	const requestedModel = typeof params.requestedModel === "string" ? params.requestedModel.trim() : "";
	if (!requestedModel || !params.models) return false;
	if (params.models.currentModelId === requestedModel) return true;
	await withTimeout(params.client.setSessionModel(params.sessionId, requestedModel), params.timeoutMs);
	return true;
}
function jsonRpcIdKey(value) {
	if (typeof value === "string") return `s:${value}`;
	if (typeof value === "number" && Number.isFinite(value)) return `n:${value}`;
}
function extractJsonRpcRequestInfo(message) {
	const candidate = message;
	if (typeof candidate.method !== "string") return;
	const idKey = jsonRpcIdKey(candidate.id);
	if (!idKey) return;
	return {
		idKey,
		method: candidate.method
	};
}
function extractJsonRpcResponseInfo(message) {
	const candidate = message;
	const idKey = jsonRpcIdKey(candidate.id);
	if (!idKey) return;
	const hasError = Object.hasOwn(candidate, "error");
	if (!hasError && !Object.hasOwn(candidate, "result")) return;
	return {
		idKey,
		hasError
	};
}
function filterRecoverableLoadFallbackOutput(messages) {
	const requestMethodById = /* @__PURE__ */ new Map();
	const failedLoadRequestIds = /* @__PURE__ */ new Set();
	for (const message of messages) {
		const request = extractJsonRpcRequestInfo(message);
		if (request) {
			requestMethodById.set(request.idKey, request.method);
			continue;
		}
		const response = extractJsonRpcResponseInfo(message);
		if (!response || !response.hasError) continue;
		if (requestMethodById.get(response.idKey) === "session/load") failedLoadRequestIds.add(response.idKey);
	}
	if (failedLoadRequestIds.size === 0) return messages;
	return messages.filter((message) => {
		const request = extractJsonRpcRequestInfo(message);
		if (request && request.method === "session/load" && failedLoadRequestIds.has(request.idKey)) return false;
		const response = extractJsonRpcResponseInfo(message);
		if (response && failedLoadRequestIds.has(response.idKey)) return false;
		return true;
	});
}
function emitPromptRetryNotice(params) {
	if (params.suppressSdkConsoleErrors) return;
	process.stderr.write(`[acpx] prompt failed (${formatErrorMessage(params.error)}), retrying in ${params.delayMs}ms (attempt ${params.attempt}/${params.maxRetries})\n`);
}
async function runQueuedTask(sessionRecordId, task, options) {
	const outputFormatter = task.waitForCompletion ? new QueueTaskOutputFormatter(task) : DISCARD_OUTPUT_FORMATTER;
	try {
		const result = await runSessionPrompt({
			sessionRecordId,
			mcpServers: options.mcpServers,
			prompt: task.prompt ?? textPrompt(task.message),
			permissionMode: task.permissionMode,
			resumePolicy: task.resumePolicy,
			nonInteractivePermissions: task.nonInteractivePermissions ?? options.nonInteractivePermissions,
			authCredentials: options.authCredentials,
			authPolicy: options.authPolicy,
			outputFormatter,
			timeoutMs: task.timeoutMs,
			suppressSdkConsoleErrors: task.suppressSdkConsoleErrors ?? options.suppressSdkConsoleErrors,
			verbose: options.verbose,
			promptRetries: options.promptRetries,
			onClientAvailable: options.onClientAvailable,
			onClientClosed: options.onClientClosed,
			onPromptActive: options.onPromptActive,
			client: options.sharedClient
		});
		if (task.waitForCompletion) task.send({
			type: "result",
			requestId: task.requestId,
			result
		});
	} catch (error) {
		const normalizedError = normalizeOutputError(error, {
			origin: "runtime",
			detailCode: "QUEUE_RUNTIME_PROMPT_FAILED"
		});
		const alreadyEmitted = error.outputAlreadyEmitted === true;
		if (task.waitForCompletion) task.send({
			type: "error",
			requestId: task.requestId,
			code: normalizedError.code,
			detailCode: normalizedError.detailCode,
			origin: normalizedError.origin,
			message: normalizedError.message,
			retryable: normalizedError.retryable,
			acp: normalizedError.acp,
			outputAlreadyEmitted: alreadyEmitted
		});
		if (error instanceof InterruptedError) throw error;
	} finally {
		task.close();
	}
}
async function runSessionPrompt(options) {
	const stopTotalTimer = startPerfTimer("runtime.prompt.total");
	const output = options.outputFormatter;
	const record = await measurePerf("session.resolve_prompt_record", async () => {
		return await resolveSessionRecord(options.sessionRecordId);
	});
	const conversation = cloneSessionConversation(record);
	let acpxState = cloneSessionAcpxState(record.acpx);
	const promptMessageId = recordPromptSubmission(conversation, options.prompt, isoNow());
	output.setContext({ sessionId: record.acpxRecordId });
	const eventWriter = await measurePerf("session.events.open", async () => {
		return await SessionEventWriter.open(record);
	});
	const pendingMessages = [];
	const pendingConnectOutputMessages = [];
	let bufferingConnectOutput = true;
	let promptTurnActive = false;
	let promptTurnHadSideEffects = false;
	let sawAcpMessage = false;
	let eventWriterClosed = false;
	const closeEventWriter = async (checkpoint) => {
		if (eventWriterClosed) return;
		eventWriterClosed = true;
		await eventWriter.close({ checkpoint });
	};
	const flushPendingMessages = async (checkpoint = false) => {
		if (pendingMessages.length === 0) return;
		const batch = pendingMessages.splice(0, pendingMessages.length);
		await measurePerf("session.events.flush_pending", async () => {
			await eventWriter.appendMessages(batch, { checkpoint });
		});
	};
	const ownClient = options.client == null;
	const client = options.client ?? new AcpClient({
		agentCommand: record.agentCommand,
		cwd: absolutePath(record.cwd),
		mcpServers: options.mcpServers,
		permissionMode: options.permissionMode,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		suppressSdkConsoleErrors: options.suppressSdkConsoleErrors,
		verbose: options.verbose,
		sessionOptions: sessionOptionsFromRecord(record)
	});
	client.updateRuntimeOptions({
		permissionMode: options.permissionMode,
		nonInteractivePermissions: options.nonInteractivePermissions,
		suppressSdkConsoleErrors: options.suppressSdkConsoleErrors,
		verbose: options.verbose
	});
	client.setEventHandlers({
		onAcpMessage: (direction, message) => {
			sawAcpMessage = true;
			pendingMessages.push(message);
			options.onAcpMessage?.(direction, message);
		},
		onAcpOutputMessage: (_direction, message) => {
			if (bufferingConnectOutput) {
				pendingConnectOutputMessages.push(message);
				return;
			}
			output.onAcpMessage(message);
		},
		onSessionUpdate: (notification) => {
			if (promptTurnActive) promptTurnHadSideEffects = true;
			acpxState = recordSessionUpdate(conversation, acpxState, notification);
			trimConversationForRuntime(conversation);
			options.onSessionUpdate?.(notification);
		},
		onClientOperation: (operation) => {
			if (promptTurnActive) promptTurnHadSideEffects = true;
			acpxState = recordClientOperation(conversation, acpxState, operation);
			trimConversationForRuntime(conversation);
			options.onClientOperation?.(operation);
		}
	});
	let activeSessionIdForControl = record.acpSessionId;
	let notifiedClientAvailable = false;
	const activeController = {
		hasActivePrompt: () => client.hasActivePrompt(),
		requestCancelActivePrompt: async () => await client.requestCancelActivePrompt(),
		setSessionMode: async (modeId) => {
			await client.setSessionMode(activeSessionIdForControl, modeId);
		},
		setSessionModel: async (modelId) => {
			await client.setSessionModel(activeSessionIdForControl, modelId);
		},
		setSessionConfigOption: async (configId, value) => {
			return await client.setSessionConfigOption(activeSessionIdForControl, configId, value);
		}
	};
	try {
		return await withInterrupt(async () => {
			const connectStartedAt = Date.now();
			const { sessionId: activeSessionId, resumed, loadError } = await measurePerf("runtime.connect_and_load", async () => {
				try {
					return await connectAndLoadSession({
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
						onConnectedRecord: (connectedRecord) => {
							connectedRecord.lastPromptAt = isoNow();
						},
						onSessionIdResolved: (sessionId) => {
							activeSessionIdForControl = sessionId;
						}
					});
				} catch (error) {
					bufferingConnectOutput = false;
					for (const message of pendingConnectOutputMessages) output.onAcpMessage(message);
					pendingConnectOutputMessages.length = 0;
					throw error;
				}
			});
			bufferingConnectOutput = false;
			const connectOutputMessages = loadError == null ? pendingConnectOutputMessages : filterRecoverableLoadFallbackOutput(pendingConnectOutputMessages);
			for (const message of connectOutputMessages) output.onAcpMessage(message);
			pendingConnectOutputMessages.length = 0;
			if (options.verbose) process.stderr.write(`[acpx] ${formatPerfMetric("prompt.connect_and_load", Date.now() - connectStartedAt)}\n`);
			output.setContext({ sessionId: record.acpxRecordId });
			await flushPendingMessages(false);
			const maxRetries = options.promptRetries ?? 0;
			let response;
			promptTurnActive = true;
			for (let attempt = 0;; attempt++) try {
				const promptStartedAt = Date.now();
				response = await measurePerf("runtime.prompt.agent_turn", async () => {
					return await runPromptTurn({
						client,
						sessionId: activeSessionId,
						prompt: options.prompt,
						timeoutMs: options.timeoutMs,
						conversation,
						promptMessageId,
						onPromptStarted: attempt === 0 && options.onPromptActive ? async () => {
							try {
								await options.onPromptActive?.();
							} catch (error) {
								if (options.verbose) process.stderr.write("[acpx] onPromptActive hook failed: " + formatErrorMessage(error) + "\n");
							}
						} : void 0
					});
				});
				if (options.verbose) process.stderr.write(`[acpx] ${formatPerfMetric("prompt.agent_turn", Date.now() - promptStartedAt)}\n`);
				break;
			} catch (error) {
				const snapshot = client.getAgentLifecycleSnapshot();
				const agentCrashed = snapshot.lastExit?.unexpectedDuringPrompt === true;
				if (attempt < maxRetries && !agentCrashed && !promptTurnHadSideEffects && isRetryablePromptError(error)) {
					const delayMs = Math.min(1e3 * 2 ** attempt, 1e4);
					emitPromptRetryNotice({
						error,
						delayMs,
						attempt: attempt + 1,
						maxRetries,
						suppressSdkConsoleErrors: options.suppressSdkConsoleErrors
					});
					await waitMs(delayMs);
					if (!promptTurnHadSideEffects) continue;
				}
				promptTurnActive = false;
				applyLifecycleSnapshotToRecord(record, snapshot);
				const lastExit = snapshot.lastExit;
				if (lastExit?.unexpectedDuringPrompt && options.verbose) process.stderr.write("[acpx] agent disconnected during prompt (" + lastExit.reason + ", exit=" + lastExit.exitCode + ", signal=" + (lastExit.signal ?? "none") + ")\n");
				const normalizedError = normalizeOutputError(error, { origin: "runtime" });
				await flushPendingMessages(false).catch(() => {});
				output.flush();
				record.lastUsedAt = isoNow();
				applyConversation(record, conversation);
				record.acpx = acpxState;
				const propagated = error instanceof Error ? error : new Error(formatErrorMessage(error));
				propagated.outputAlreadyEmitted = sawAcpMessage;
				propagated.normalizedOutputError = normalizedError;
				throw propagated;
			}
			promptTurnActive = false;
			await flushPendingMessages(false);
			output.flush();
			record.lastUsedAt = isoNow();
			record.closed = false;
			record.closedAt = void 0;
			record.protocolVersion = client.initializeResult?.protocolVersion;
			record.agentCapabilities = client.initializeResult?.agentCapabilities;
			applyConversation(record, conversation);
			record.acpx = acpxState;
			applyLifecycleSnapshotToRecord(record, client.getAgentLifecycleSnapshot());
			stopTotalTimer();
			return {
				...toPromptResult(response.stopReason, record.acpxRecordId, client),
				record,
				resumed,
				loadError
			};
		}, async () => {
			await client.cancelActivePrompt(INTERRUPT_CANCEL_WAIT_MS);
			applyLifecycleSnapshotToRecord(record, client.getAgentLifecycleSnapshot());
			record.lastUsedAt = isoNow();
			applyConversation(record, conversation);
			record.acpx = acpxState;
			await flushPendingMessages(false).catch(() => {});
			if (ownClient) await client.close();
		});
	} finally {
		if (options.verbose) process.stderr.write(`[acpx] ${formatPerfMetric("prompt.total", stopTotalTimer())}\n`);
		else stopTotalTimer();
		if (notifiedClientAvailable) options.onClientClosed?.();
		client.clearEventHandlers();
		if (ownClient) await client.close();
		applyLifecycleSnapshotToRecord(record, client.getAgentLifecycleSnapshot());
		applyConversation(record, conversation);
		record.acpx = acpxState;
		await flushPendingMessages(false).catch(() => {});
		await closeEventWriter(true).catch(() => {});
	}
}
async function runOnce(options) {
	const output = options.outputFormatter;
	let promptTurnActive = false;
	let promptTurnHadSideEffects = false;
	const client = new AcpClient({
		agentCommand: options.agentCommand,
		cwd: absolutePath(options.cwd),
		mcpServers: options.mcpServers,
		permissionMode: options.permissionMode,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		suppressSdkConsoleErrors: options.suppressSdkConsoleErrors,
		verbose: options.verbose,
		onAcpMessage: options.onAcpMessage,
		onAcpOutputMessage: (_direction, message) => output.onAcpMessage(message),
		onSessionUpdate: (notification) => {
			if (promptTurnActive) promptTurnHadSideEffects = true;
			options.onSessionUpdate?.(notification);
		},
		onClientOperation: (operation) => {
			if (promptTurnActive) promptTurnHadSideEffects = true;
			options.onClientOperation?.(operation);
		},
		sessionOptions: options.sessionOptions
	});
	try {
		return await withInterrupt(async () => {
			await measurePerf("runtime.exec.start", async () => {
				await withTimeout(client.start(), options.timeoutMs);
			});
			const createdSession = await measurePerf("runtime.exec.create_session", async () => {
				return await withTimeout(client.createSession(absolutePath(options.cwd)), options.timeoutMs);
			});
			const sessionId = createdSession.sessionId;
			await applyRequestedModelIfAdvertised({
				client,
				sessionId,
				requestedModel: options.sessionOptions?.model,
				models: createdSession.models,
				timeoutMs: options.timeoutMs
			});
			output.setContext({ sessionId });
			const maxRetries = options.promptRetries ?? 0;
			let response;
			promptTurnActive = true;
			for (let attempt = 0;; attempt++) try {
				response = await measurePerf("runtime.exec.prompt", async () => {
					return await withTimeout(client.prompt(sessionId, options.prompt), options.timeoutMs);
				});
				break;
			} catch (error) {
				if (attempt < maxRetries && !promptTurnHadSideEffects && isRetryablePromptError(error)) {
					const delayMs = Math.min(1e3 * 2 ** attempt, 1e4);
					emitPromptRetryNotice({
						error,
						delayMs,
						attempt: attempt + 1,
						maxRetries,
						suppressSdkConsoleErrors: options.suppressSdkConsoleErrors
					});
					await waitMs(delayMs);
					if (!promptTurnHadSideEffects) continue;
				}
				promptTurnActive = false;
				throw error;
			}
			promptTurnActive = false;
			output.flush();
			return toPromptResult(response.stopReason, sessionId, client);
		}, async () => {
			await client.cancelActivePrompt(INTERRUPT_CANCEL_WAIT_MS);
			await client.close();
		});
	} finally {
		await client.close();
	}
}
async function sendSessionDirect(options) {
	return await runSessionPrompt({
		sessionRecordId: options.sessionId,
		prompt: options.prompt,
		mcpServers: options.mcpServers,
		permissionMode: options.permissionMode,
		resumePolicy: options.resumePolicy,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		outputFormatter: options.outputFormatter,
		onAcpMessage: options.onAcpMessage,
		onSessionUpdate: options.onSessionUpdate,
		onClientOperation: options.onClientOperation,
		timeoutMs: options.timeoutMs,
		suppressSdkConsoleErrors: options.suppressSdkConsoleErrors,
		verbose: options.verbose,
		client: options.client
	});
}
//#endregion
//#region src/cli/session/queue-owner-runtime.ts
const QUEUE_OWNER_STARTUP_MAX_ATTEMPTS = 120;
const QUEUE_OWNER_HEARTBEAT_INTERVAL_MS = 5e3;
async function submitToRunningOwner(options, waitForCompletion) {
	return await trySubmitToRunningOwner({
		sessionId: options.sessionId,
		message: promptToDisplayText(options.prompt),
		prompt: options.prompt,
		permissionMode: options.permissionMode,
		nonInteractivePermissions: options.nonInteractivePermissions,
		outputFormatter: options.outputFormatter,
		errorEmissionPolicy: options.errorEmissionPolicy,
		timeoutMs: options.timeoutMs,
		suppressSdkConsoleErrors: options.suppressSdkConsoleErrors,
		waitForCompletion,
		verbose: options.verbose
	});
}
async function runSessionQueueOwner(options) {
	const lease = await tryAcquireQueueOwnerLease(options.sessionId);
	if (!lease) return;
	const sessionRecord = await resolveSessionRecord(options.sessionId);
	let owner;
	let heartbeatTimer;
	const sharedClient = new AcpClient({
		agentCommand: sessionRecord.agentCommand,
		cwd: absolutePath(sessionRecord.cwd),
		mcpServers: options.mcpServers,
		permissionMode: options.permissionMode,
		nonInteractivePermissions: options.nonInteractivePermissions,
		authCredentials: options.authCredentials,
		authPolicy: options.authPolicy,
		suppressSdkConsoleErrors: options.suppressSdkConsoleErrors,
		verbose: options.verbose,
		sessionOptions: sessionOptionsFromRecord(sessionRecord)
	});
	const ttlMs = normalizeQueueOwnerTtlMs(options.ttlMs);
	const maxQueueDepth = Math.max(1, Math.round(options.maxQueueDepth ?? 16));
	const taskPollTimeoutMs = ttlMs === 0 ? void 0 : ttlMs;
	const initialTaskPollTimeoutMs = taskPollTimeoutMs == null ? void 0 : Math.max(taskPollTimeoutMs, 1e3);
	const turnController = new QueueOwnerTurnController({
		withTimeout: async (run, timeoutMs) => await withTimeout(run(), timeoutMs),
		setSessionModeFallback: async (modeId, timeoutMs) => {
			await runSessionSetModeDirect({
				sessionRecordId: options.sessionId,
				modeId,
				mcpServers: options.mcpServers,
				nonInteractivePermissions: options.nonInteractivePermissions,
				authCredentials: options.authCredentials,
				authPolicy: options.authPolicy,
				timeoutMs,
				verbose: options.verbose
			});
		},
		setSessionModelFallback: async (modelId, timeoutMs) => {
			await runSessionSetModelDirect({
				sessionRecordId: options.sessionId,
				modelId,
				mcpServers: options.mcpServers,
				nonInteractivePermissions: options.nonInteractivePermissions,
				authCredentials: options.authCredentials,
				authPolicy: options.authPolicy,
				timeoutMs,
				verbose: options.verbose
			});
		},
		setSessionConfigOptionFallback: async (configId, value, timeoutMs) => {
			return (await runSessionSetConfigOptionDirect({
				sessionRecordId: options.sessionId,
				configId,
				value,
				mcpServers: options.mcpServers,
				nonInteractivePermissions: options.nonInteractivePermissions,
				authCredentials: options.authCredentials,
				authPolicy: options.authPolicy,
				timeoutMs,
				verbose: options.verbose
			})).response;
		}
	});
	const applyPendingCancel = async () => {
		return await turnController.applyPendingCancel();
	};
	const scheduleApplyPendingCancel = () => {
		applyPendingCancel().catch((error) => {
			if (options.verbose) process.stderr.write(`[acpx] failed to apply deferred cancel: ${formatErrorMessage(error)}\n`);
		});
	};
	const setActiveController = (controller) => {
		turnController.setActiveController(controller);
		scheduleApplyPendingCancel();
	};
	const clearActiveController = () => {
		turnController.clearActiveController();
	};
	const runPromptTurn = async (run) => {
		turnController.beginTurn();
		try {
			return await run();
		} finally {
			turnController.endTurn();
		}
	};
	try {
		owner = await SessionQueueOwner.start(lease, {
			cancelPrompt: async () => {
				if (!await turnController.requestCancel()) return false;
				await applyPendingCancel();
				return true;
			},
			setSessionMode: async (modeId, timeoutMs) => {
				await turnController.setSessionMode(modeId, timeoutMs);
			},
			setSessionModel: async (modelId, timeoutMs) => {
				await turnController.setSessionModel(modelId, timeoutMs);
			},
			setSessionConfigOption: async (configId, value, timeoutMs) => {
				return await turnController.setSessionConfigOption(configId, value, timeoutMs);
			}
		}, {
			maxQueueDepth,
			onQueueDepthChanged: (queueDepth) => {
				setPerfGauge("queue.owner.depth", queueDepth);
				refreshQueueOwnerLease(lease, { queueDepth }).catch(() => {});
			}
		});
		if (options.verbose) process.stderr.write(`[acpx] queue owner ready for session ${options.sessionId} (ttlMs=${ttlMs}, maxQueueDepth=${maxQueueDepth})\n`);
		await refreshQueueOwnerLease(lease, { queueDepth: owner.queueDepth() }).catch(() => {});
		heartbeatTimer = setInterval(() => {
			refreshQueueOwnerLease(lease, { queueDepth: owner?.queueDepth() ?? 0 }).catch(() => {});
		}, QUEUE_OWNER_HEARTBEAT_INTERVAL_MS);
		let isFirstTask = true;
		while (true) {
			const pollTimeoutMs = isFirstTask ? initialTaskPollTimeoutMs : taskPollTimeoutMs;
			const task = await owner.nextTask(pollTimeoutMs);
			if (!task) break;
			isFirstTask = false;
			await runPromptTurn(async () => {
				try {
					await runQueuedTask(options.sessionId, task, {
						sharedClient,
						verbose: options.verbose,
						mcpServers: options.mcpServers,
						nonInteractivePermissions: options.nonInteractivePermissions,
						authCredentials: options.authCredentials,
						authPolicy: options.authPolicy,
						suppressSdkConsoleErrors: options.suppressSdkConsoleErrors,
						promptRetries: options.promptRetries,
						onClientAvailable: setActiveController,
						onClientClosed: clearActiveController,
						onPromptActive: async () => {
							turnController.markPromptActive();
							await applyPendingCancel();
						}
					});
				} finally {
					checkpointPerfMetricsCapture();
				}
			});
		}
	} finally {
		if (heartbeatTimer) clearInterval(heartbeatTimer);
		turnController.beginClosing();
		if (owner) await owner.close();
		await sharedClient.close().catch(() => {});
		try {
			const record = await resolveSessionRecord(options.sessionId);
			applyLifecycleSnapshotToRecord(record, sharedClient.getAgentLifecycleSnapshot());
			await writeSessionRecord(record);
		} catch {}
		await releaseQueueOwnerLease(lease);
		if (options.verbose) process.stderr.write(`[acpx] queue owner stopped for session ${options.sessionId}\n`);
	}
}
async function sendSession(options) {
	const waitForCompletion = options.waitForCompletion !== false;
	const queuedToOwner = await submitToRunningOwner(options, waitForCompletion);
	if (queuedToOwner) return queuedToOwner;
	spawnQueueOwnerProcess(queueOwnerRuntimeOptionsFromSend(options));
	for (let attempt = 0; attempt < QUEUE_OWNER_STARTUP_MAX_ATTEMPTS; attempt += 1) {
		const queued = await submitToRunningOwner(options, waitForCompletion);
		if (queued) return queued;
		await waitMs(50);
	}
	throw new Error(`Session queue owner failed to start for session ${options.sessionId}`);
}
//#endregion
//#region src/session/session.ts
var session_exports = /* @__PURE__ */ __exportAll({
	DEFAULT_HISTORY_LIMIT: () => 20,
	DEFAULT_QUEUE_OWNER_TTL_MS: () => DEFAULT_QUEUE_OWNER_TTL_MS,
	InterruptedError: () => InterruptedError,
	TimeoutError: () => TimeoutError,
	cancelSessionPrompt: () => cancelSessionPrompt,
	closeSession: () => closeSession,
	createSession: () => createSession,
	createSessionWithClient: () => createSessionWithClient,
	ensureSession: () => ensureSession,
	findGitRepositoryRoot: () => findGitRepositoryRoot,
	findSession: () => findSession,
	findSessionByDirectoryWalk: () => findSessionByDirectoryWalk,
	isProcessAlive: () => isProcessAlive,
	listSessions: () => listSessions,
	listSessionsForAgent: () => listSessionsForAgent,
	normalizeQueueOwnerTtlMs: () => normalizeQueueOwnerTtlMs,
	runOnce: () => runOnce,
	runQueuedTask: () => runQueuedTask,
	runSessionQueueOwner: () => runSessionQueueOwner,
	sendSession: () => sendSession,
	sendSessionDirect: () => sendSessionDirect,
	setSessionConfigOption: () => setSessionConfigOption,
	setSessionMode: () => setSessionMode,
	setSessionModel: () => setSessionModel
});
//#endregion
export { buildQueueOwnerArgOverride as a, createSessionWithClient as c, sendSessionDirect as i, cancelSessionPrompt as l, runSessionQueueOwner as n, flushPerfMetricsCapture as o, runOnce as r, installPerfMetricsCapture as s, session_exports as t, DEFAULT_QUEUE_OWNER_TTL_MS as u };

//# sourceMappingURL=session-BtwAKtJ3.js.map