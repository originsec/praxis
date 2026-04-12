import { A as PERMISSION_MODES, g as textPrompt, h as promptToDisplayText, j as SESSION_RECORD_SCHEMA } from "./perf-metrics-D0um6IR6.js";
import { A as resolveSessionRecord, H as TimeoutError, P as defaultSessionEventLog, U as withInterrupt, V as InterruptedError, W as withTimeout, _ as recordSessionUpdate, f as cloneSessionAcpxState, g as recordPromptSubmission, h as recordClientOperation, m as createSessionConversation } from "./prompt-turn-CXMtXBl-.js";
import { c as createSessionWithClient, i as sendSessionDirect, l as cancelSessionPrompt, r as runOnce } from "./session-BtwAKtJ3.js";
import { t as createOutputFormatter } from "./output-C4QhjpM6.js";
import path from "node:path";
import fs from "node:fs/promises";
import os from "node:os";
import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { ZodError, z } from "zod";
//#region src/flows/authoring.ts
const FLOW_DEFINITION_BRAND = Symbol.for("acpx.flow.definition");
function markDefinedFlow(definition) {
	if (isDefinedFlow(definition)) return definition;
	Object.defineProperty(definition, FLOW_DEFINITION_BRAND, {
		value: true,
		enumerable: false,
		configurable: false,
		writable: false
	});
	return definition;
}
function isDefinedFlow(value) {
	return value != null && typeof value === "object" && value[FLOW_DEFINITION_BRAND] === true;
}
//#endregion
//#region src/flows/schema.ts
const FLOW_NODE_TYPES = [
	"acp",
	"compute",
	"action",
	"checkpoint"
];
const finiteNonNegativeNumberSchema = z.number().finite().nonnegative();
const nonEmptyTrimmedStringSchema = z.string().refine((value) => value.trim().length > 0, { message: "must not be empty" });
function extensibleObject(shape) {
	return z.object(shape).passthrough();
}
function functionSchema(label) {
	return z.custom((value) => typeof value === "function", { message: `${label} must be a function` });
}
const flowNodeCommonShape = {
	timeoutMs: finiteNonNegativeNumberSchema.optional(),
	heartbeatMs: finiteNonNegativeNumberSchema.optional(),
	statusDetail: z.string().optional()
};
const flowPermissionRequirementsSchema = extensibleObject({
	requiredMode: z.enum(PERMISSION_MODES),
	requireExplicitGrant: z.boolean().optional(),
	reason: nonEmptyTrimmedStringSchema.optional()
});
const flowRunDefinitionSchema = extensibleObject({ title: z.union([z.string(), functionSchema("run.title")]).optional() });
const acpSessionSchema = extensibleObject({
	handle: z.string().optional(),
	isolated: z.boolean().optional()
});
const acpNodeSchema = extensibleObject({
	...flowNodeCommonShape,
	nodeType: z.literal("acp"),
	profile: z.string().optional(),
	cwd: z.union([z.string(), functionSchema("cwd")]).optional(),
	session: acpSessionSchema.optional(),
	prompt: functionSchema("prompt"),
	parse: functionSchema("parse").optional()
});
const computeNodeSchema = extensibleObject({
	...flowNodeCommonShape,
	nodeType: z.literal("compute"),
	run: functionSchema("run")
});
const functionActionNodeSchema = extensibleObject({
	...flowNodeCommonShape,
	nodeType: z.literal("action"),
	run: functionSchema("run")
});
const shellActionNodeSchema = extensibleObject({
	...flowNodeCommonShape,
	nodeType: z.literal("action"),
	exec: functionSchema("exec"),
	parse: functionSchema("parse").optional()
}).refine((node) => !hasOwn(node, "run"), { message: "shell action nodes must not define run" });
const checkpointNodeSchema = extensibleObject({
	...flowNodeCommonShape,
	nodeType: z.literal("checkpoint"),
	summary: z.string().optional(),
	run: functionSchema("run").optional()
});
const directFlowEdgeSchema = extensibleObject({
	from: z.string(),
	to: z.string()
});
const switchFlowEdgeSchema = extensibleObject({
	from: z.string(),
	switch: extensibleObject({
		on: z.string(),
		cases: z.record(z.string(), z.string())
	})
});
const flowDefinitionSchema = extensibleObject({
	name: nonEmptyTrimmedStringSchema,
	run: flowRunDefinitionSchema.optional(),
	permissions: flowPermissionRequirementsSchema.optional(),
	startAt: z.string(),
	nodes: z.record(z.string(), z.unknown()),
	edges: z.array(z.unknown())
});
const flowNodeTypeSchema = z.object({ nodeType: z.enum(FLOW_NODE_TYPES) });
function assertValidFlowDefinitionShape(flow) {
	const parsed = parseWithSchema("flow definition", flowDefinitionSchema, flow);
	for (const [nodeId, node] of Object.entries(parsed.nodes)) assertValidFlowNodeDefinitionShape(node, `flow node "${nodeId}"`);
	parsed.edges.forEach((edge, index) => {
		assertValidFlowEdgeShape(edge, `flow definition: edges.${index}`);
	});
}
function assertValidAcpNodeDefinition(node) {
	parseWithSchema("acp node definition", acpNodeSchema, node);
}
function assertValidComputeNodeDefinition(node) {
	parseWithSchema("compute node definition", computeNodeSchema, node);
}
function assertValidActionNodeDefinition(node) {
	assertValidActionNodeDefinitionShape(node, "action node definition");
}
function assertValidShellActionNodeDefinition(node) {
	parseWithSchema("shell action node definition", shellActionNodeSchema, node);
}
function assertValidCheckpointNodeDefinition(node) {
	parseWithSchema("checkpoint node definition", checkpointNodeSchema, node);
}
function assertValidFlowNodeDefinitionShape(node, label) {
	const { nodeType } = parseWithSchema(label, flowNodeTypeSchema, node);
	switch (nodeType) {
		case "acp":
			parseWithSchema(label, acpNodeSchema, node);
			return;
		case "compute":
			parseWithSchema(label, computeNodeSchema, node);
			return;
		case "action":
			assertValidActionNodeDefinitionShape(node, label);
			return;
		case "checkpoint":
			parseWithSchema(label, checkpointNodeSchema, node);
			return;
	}
}
function assertValidActionNodeDefinitionShape(node, label) {
	const hasRun = hasOwn(node, "run");
	const hasExec = hasOwn(node, "exec");
	if (hasRun === hasExec) throw new Error(`Invalid ${label}: action nodes must define exactly one of run or exec`);
	if (hasExec) {
		parseWithSchema(label, shellActionNodeSchema, node);
		return;
	}
	parseWithSchema(label, functionActionNodeSchema, node);
}
function assertValidFlowEdgeShape(edge, label) {
	const hasTo = hasOwn(edge, "to");
	if (hasTo === hasOwn(edge, "switch")) throw new Error(`Invalid ${label}: edge must define exactly one of to or switch`);
	if (hasTo) {
		parseWithSchema(label, directFlowEdgeSchema, edge);
		return;
	}
	parseWithSchema(label, switchFlowEdgeSchema, edge);
}
function parseWithSchema(label, schema, value) {
	try {
		return schema.parse(value);
	} catch (error) {
		if (error instanceof ZodError) throw new Error(formatValidationError(label, error), { cause: error });
		throw error;
	}
}
function formatValidationError(label, error) {
	return `Invalid ${label}: ${Array.from(new Set(error.issues.flatMap((issue) => formatIssue(issue)))).join("; ")}`;
}
function formatIssue(issue, parentPath = []) {
	const path = [...parentPath, ...issue.path.map(String)];
	if (issue.code === "invalid_union") return issue.errors.flatMap((branch) => branch.flatMap((nestedIssue) => formatIssue(nestedIssue, path)));
	const renderedPath = path.join(".");
	return [renderedPath ? `${renderedPath}: ${issue.message}` : issue.message];
}
function hasOwn(value, key) {
	return value != null && typeof value === "object" && Object.prototype.hasOwnProperty.call(value, key);
}
//#endregion
//#region src/flows/definition.ts
function defineFlow(definition) {
	assertValidFlowDefinitionShape(definition);
	return markDefinedFlow(definition);
}
function acp(definition) {
	const node = {
		nodeType: "acp",
		...definition
	};
	assertValidAcpNodeDefinition(node);
	return node;
}
function compute(definition) {
	const node = {
		nodeType: "compute",
		...definition
	};
	assertValidComputeNodeDefinition(node);
	return node;
}
function action(definition) {
	const node = {
		nodeType: "action",
		...definition
	};
	assertValidActionNodeDefinition(node);
	return node;
}
function shell(definition) {
	const node = {
		nodeType: "action",
		...definition
	};
	assertValidShellActionNodeDefinition(node);
	return node;
}
function checkpoint(definition = {}) {
	const node = {
		nodeType: "checkpoint",
		...definition
	};
	assertValidCheckpointNodeDefinition(node);
	return node;
}
//#endregion
//#region src/flows/executors/shell.ts
function formatShellActionSummary(spec) {
	return `shell: ${renderShellCommand(spec.command, spec.args ?? [])}`;
}
function renderShellCommand(command, args) {
	const renderedArgs = args.map((arg) => JSON.stringify(arg)).join(" ");
	return renderedArgs.length > 0 ? `${command} ${renderedArgs}` : command;
}
async function runShellAction(spec) {
	const cwd = spec.cwd ?? process.cwd();
	const args = spec.args ?? [];
	const startMs = Date.now();
	const child = spawn(spec.command, args, {
		cwd,
		env: {
			...process.env,
			...spec.env
		},
		shell: spec.shell,
		stdio: [
			"pipe",
			"pipe",
			"pipe"
		],
		windowsHide: true
	});
	let stdout = "";
	let stderr = "";
	let timedOut = false;
	let timeout;
	const finish = new Promise((resolve, reject) => {
		child.stdout.setEncoding("utf8");
		child.stderr.setEncoding("utf8");
		child.stdout.on("data", (chunk) => {
			stdout += chunk;
		});
		child.stderr.on("data", (chunk) => {
			stderr += chunk;
		});
		child.once("error", reject);
		child.once("exit", (exitCode, signal) => {
			const result = {
				command: spec.command,
				args,
				cwd,
				stdout,
				stderr,
				combinedOutput: `${stdout}${stderr}`,
				exitCode,
				signal,
				durationMs: Date.now() - startMs
			};
			if (timedOut) {
				reject(new TimeoutError(spec.timeoutMs ?? 0));
				return;
			}
			if (((exitCode ?? 0) !== 0 || signal != null) && spec.allowNonZeroExit !== true) {
				reject(/* @__PURE__ */ new Error(`Shell action failed (${renderShellCommand(spec.command, args)}): ${signal ? `signal ${signal}` : `exit ${String(exitCode)}`}${stderr.length > 0 ? `\n${stderr.trim()}` : ""}`));
				return;
			}
			resolve(result);
		});
	});
	if (spec.stdin != null) child.stdin.write(spec.stdin);
	child.stdin.end();
	if (spec.timeoutMs != null && spec.timeoutMs > 0) timeout = setTimeout(() => {
		timedOut = true;
		child.kill("SIGTERM");
		setTimeout(() => {
			child.kill("SIGKILL");
		}, 1e3).unref();
	}, spec.timeoutMs);
	try {
		return await finish;
	} finally {
		if (timeout) clearTimeout(timeout);
	}
}
//#endregion
//#region src/flows/graph.ts
function validateFlowDefinition(flow) {
	assertValidFlowDefinitionShape(flow);
	if (!flow.nodes[flow.startAt]) throw new Error(`Flow start node is missing: ${flow.startAt}`);
	const outgoingEdges = /* @__PURE__ */ new Set();
	for (const edge of flow.edges) {
		if (!flow.nodes[edge.from]) throw new Error(`Flow edge references unknown from-node: ${edge.from}`);
		if (outgoingEdges.has(edge.from)) throw new Error(`Flow node must not declare multiple outgoing edges: ${edge.from}`);
		outgoingEdges.add(edge.from);
		if ("to" in edge) {
			if (!flow.nodes[edge.to]) throw new Error(`Flow edge references unknown to-node: ${edge.to}`);
			continue;
		}
		for (const target of Object.values(edge.switch.cases)) if (!flow.nodes[target]) throw new Error(`Flow switch references unknown to-node: ${target}`);
	}
}
function resolveNext(edges, from, output, result) {
	const edge = edges.find((candidate) => candidate.from === from);
	if (!edge) return null;
	if ("to" in edge) return edge.to;
	const value = getBySwitchPath(output, result, edge.switch.on);
	if (typeof value !== "string" && typeof value !== "number" && typeof value !== "boolean") throw new Error(`Flow switch value must be scalar for ${edge.switch.on}`);
	const next = edge.switch.cases[String(value)];
	if (!next) throw new Error(`No flow switch case for ${edge.switch.on}=${JSON.stringify(value)}`);
	return next;
}
function resolveNextForOutcome(edges, from, result) {
	const edge = edges.find((candidate) => candidate.from === from);
	if (!edge || "to" in edge) return null;
	if (!edge.switch.on.startsWith("$result.")) return null;
	const value = getBySwitchPath(void 0, result, edge.switch.on);
	if (typeof value !== "string" && typeof value !== "number" && typeof value !== "boolean") throw new Error(`Flow switch value must be scalar for ${edge.switch.on}`);
	const next = edge.switch.cases[String(value)];
	if (!next) throw new Error(`No flow switch case for ${edge.switch.on}=${JSON.stringify(value)}`);
	return next;
}
function getBySwitchPath(output, result, jsonPath) {
	if (jsonPath.startsWith("$result.")) return getByPath(result, `$.${jsonPath.slice(8)}`);
	if (jsonPath.startsWith("$output.")) return getByPath(output, `$.${jsonPath.slice(8)}`);
	return getByPath(output, jsonPath);
}
function getByPath(value, jsonPath) {
	if (!jsonPath.startsWith("$.")) throw new Error(`Unsupported JSON path: ${jsonPath}`);
	return jsonPath.slice(2).split(".").reduce((current, key) => {
		if (current == null || typeof current !== "object") return;
		return current[key];
	}, value);
}
//#endregion
//#region src/flows/runtime-support.ts
function isoNow$1() {
	return (/* @__PURE__ */ new Date()).toISOString();
}
function persistRunFailure(store, runDir, state, error) {
	if (state.finishedAt !== void 0 && (state.status === "failed" || state.status === "timed_out")) return Promise.resolve();
	state.status = error instanceof TimeoutError ? "timed_out" : "failed";
	state.updatedAt = isoNow$1();
	state.finishedAt = state.updatedAt;
	state.error = error instanceof Error ? error.message : String(error);
	state.statusDetail = state.currentNode ? `Failed in ${state.currentNode}: ${state.error}` : state.error;
	return store.writeSnapshot(runDir, state, {
		scope: "run",
		type: "run_failed",
		payload: {
			status: state.status,
			error: state.error
		}
	});
}
function makeFlowNodeContext(state, input, services) {
	return {
		input,
		outputs: state.outputs,
		results: state.results,
		state,
		services
	};
}
function markNodeStarted(state, nodeId, attemptId, nodeType, startedAt, detail) {
	state.status = "running";
	state.waitingOn = void 0;
	state.currentNode = nodeId;
	state.currentAttemptId = attemptId;
	state.currentNodeType = nodeType;
	state.currentNodeStartedAt = startedAt;
	state.lastHeartbeatAt = startedAt;
	state.statusDetail = detail ?? `Running ${nodeType} node ${nodeId}`;
}
function clearActiveNode(state, detail) {
	state.currentNode = void 0;
	state.currentAttemptId = void 0;
	state.currentNodeType = void 0;
	state.currentNodeStartedAt = void 0;
	state.lastHeartbeatAt = void 0;
	state.statusDetail = detail;
}
function updateStatusDetail(state, detail) {
	if (!detail) return;
	state.statusDetail = detail;
}
async function finalizeStepTrace(store, runDir, state, nodeId, attemptId, output, baseTrace) {
	const trace = baseTrace ? structuredClone(baseTrace) : {};
	if (output !== void 0) {
		const inlineOutput = toInlineOutput(output);
		if (inlineOutput !== void 0) trace.outputInline = inlineOutput;
		else trace.outputArtifact = await store.writeArtifact(runDir, state, output, {
			mediaType: outputArtifactMediaType(output),
			extension: outputArtifactExtension(output),
			nodeId,
			attemptId
		});
	}
	return Object.keys(trace).length > 0 ? trace : null;
}
function normalizePromptInput(prompt) {
	return typeof prompt === "string" ? textPrompt(prompt) : prompt;
}
async function resolveNodeCwd(defaultCwd, cwd, context) {
	if (typeof cwd === "function") {
		const resolved = await cwd(context) ?? defaultCwd;
		return path.resolve(defaultCwd, resolved);
	}
	return path.resolve(defaultCwd, cwd ?? defaultCwd);
}
function resolveShellActionCwd(defaultCwd, cwd) {
	return path.resolve(defaultCwd, cwd ?? defaultCwd);
}
function summarizePrompt(promptText, explicitDetail) {
	if (explicitDetail) return explicitDetail;
	const line = promptText.split("\n").map((candidate) => candidate.trim()).find((candidate) => candidate.length > 0);
	if (!line) return "Running ACP prompt";
	return `ACP: ${line.length > 120 ? `${line.slice(0, 117)}...` : line}`;
}
function createQuietCaptureOutput() {
	const chunks = [];
	return {
		formatter: createOutputFormatter("quiet", { stdout: { write(chunk) {
			chunks.push(chunk);
		} } }),
		read: () => chunks.join("").trim()
	};
}
async function resolveFlowRunTitle(flow, input, flowPath) {
	const titleDefinition = flow.run?.title;
	if (titleDefinition === void 0) return;
	return normalizeFlowRunTitle(typeof titleDefinition === "function" ? await Promise.resolve(titleDefinition({
		input,
		flowName: flow.name,
		flowPath
	})) : titleDefinition);
}
function normalizeFlowRunTitle(value) {
	const trimmed = value?.trim();
	return trimmed ? trimmed : void 0;
}
function createRunId(flowName) {
	return `${isoNow$1().replaceAll(":", "").replaceAll(".", "")}-${flowName.replace(/[^a-z0-9]+/gi, "-").replace(/^-+|-+$/g, "").toLowerCase()}-${randomUUID().slice(0, 8)}`;
}
function createSessionBindingKey(agentCommand, cwd, handle) {
	return `${agentCommand}::${cwd}::${handle}`;
}
function createSessionName(flowName, handle, cwd, runId) {
	return `${flowName}-${handle}-${stableShortHash(cwd)}-${runId.slice(-8)}`;
}
function createSessionBundleId(handle, key) {
	return `${handle.replace(/[^a-z0-9]+/gi, "-").replace(/^-+|-+$/g, "").toLowerCase() || "session"}-${stableShortHash(key)}`;
}
function createIsolatedSessionBinding(flowName, runId, attemptId, profile, agent) {
	const key = `isolated::${attemptId}`;
	const handle = "isolated";
	return {
		key,
		handle,
		bundleId: createSessionBundleId(`${handle}-${attemptId}`, `${key}::${agent.cwd}`),
		name: `${flowName}-${attemptId}-${runId.slice(-8)}`,
		profile,
		agentName: agent.agentName,
		agentCommand: agent.agentCommand,
		cwd: agent.cwd,
		acpxRecordId: key,
		acpSessionId: key
	};
}
function createSyntheticSessionRecord(options) {
	return {
		schema: SESSION_RECORD_SCHEMA,
		acpxRecordId: options.binding.acpxRecordId,
		acpSessionId: options.binding.acpSessionId,
		agentSessionId: options.binding.agentSessionId,
		agentCommand: options.binding.agentCommand,
		cwd: options.binding.cwd,
		name: options.binding.name,
		createdAt: options.createdAt,
		lastUsedAt: options.updatedAt,
		lastSeq: options.lastSeq,
		lastRequestId: void 0,
		eventLog: defaultSessionEventLog(options.binding.acpxRecordId),
		closed: true,
		closedAt: options.updatedAt,
		title: options.conversation.title,
		messages: options.conversation.messages,
		updated_at: options.conversation.updated_at,
		cumulative_token_usage: options.conversation.cumulative_token_usage,
		request_token_usage: options.conversation.request_token_usage,
		acpx: options.acpxState
	};
}
function createNodeResult(options) {
	return {
		attemptId: options.attemptId,
		nodeId: options.nodeId,
		nodeType: options.nodeType,
		outcome: options.outcome,
		startedAt: options.startedAt,
		finishedAt: options.finishedAt,
		durationMs: new Date(options.finishedAt).getTime() - new Date(options.startedAt).getTime(),
		output: options.output,
		error: options.error
	};
}
function outcomeForError(error) {
	if (error instanceof TimeoutError) return "timed_out";
	if (error instanceof InterruptedError) return "cancelled";
	return "failed";
}
function stableShortHash(value) {
	return createHash("sha1").update(value).digest("hex").slice(0, 8);
}
function nextAttemptId(attemptCounts, nodeId) {
	const next = (attemptCounts.get(nodeId) ?? 0) + 1;
	attemptCounts.set(nodeId, next);
	return `${nodeId}#${next}`;
}
function createNodeOutcomePayload(result, trace) {
	return {
		nodeType: result.nodeType,
		outcome: result.outcome,
		durationMs: result.durationMs,
		error: result.error ?? null,
		...trace
	};
}
function attachStepTrace(error, trace) {
	const attached = error instanceof Error ? error : new Error(typeof error === "string" ? error : String(error));
	attached.flowStepTrace = trace;
	return attached;
}
function extractAttachedStepTrace(error) {
	if (!(error instanceof Error)) return;
	return error.flowStepTrace;
}
function toInlineOutput(value) {
	if (value == null || typeof value === "number" || typeof value === "boolean") return value;
	if (typeof value === "string") return value.length <= 200 && !value.includes("\n") ? value : void 0;
	try {
		const serialized = JSON.stringify(value);
		if (serialized.length <= 200 && !serialized.includes("\n")) return value;
	} catch {
		return;
	}
}
function outputArtifactMediaType(value) {
	return typeof value === "string" ? "text/plain" : "application/json";
}
function outputArtifactExtension(value) {
	return typeof value === "string" ? "txt" : "json";
}
function findConversationDeltaStart(before, after) {
	const maxOverlap = Math.min(before.length, after.length);
	for (let overlap = maxOverlap; overlap >= 0; overlap -= 1) {
		let matches = true;
		for (let index = 0; index < overlap; index += 1) {
			const beforeMessage = before[before.length - overlap + index];
			const afterMessage = after[index];
			if (!deepEqualJson(beforeMessage, afterMessage)) {
				matches = false;
				break;
			}
		}
		if (matches) return overlap;
	}
	return 0;
}
function deepEqualJson(left, right) {
	return JSON.stringify(left) === JSON.stringify(right);
}
//#endregion
//#region src/flows/store.ts
const FLOW_BUNDLE_SCHEMA = "acpx.flow-run-bundle.v1";
const FLOW_TRACE_SCHEMA = "acpx.flow-trace-event.v1";
const FLOW_SNAPSHOT_SCHEMA = "acpx.flow-definition-snapshot.v1";
const MANIFEST_PATH = "manifest.json";
const FLOW_SNAPSHOT_PATH = "flow.json";
const TRACE_PATH = "trace.ndjson";
const PROJECTIONS_DIR = "projections";
const RUN_PROJECTION_PATH = "projections/run.json";
const LIVE_PROJECTION_PATH = "projections/live.json";
const STEPS_PROJECTION_PATH = "projections/steps.json";
const SESSIONS_DIR = "sessions";
const ARTIFACTS_DIR = "artifacts";
function flowRunsBaseDir(homeDir = os.homedir()) {
	return path.join(homeDir, ".acpx", "flows", "runs");
}
var FlowRunStore = class {
	outputRoot;
	traceSeqByRun = /* @__PURE__ */ new Map();
	sessionSeqByBundle = /* @__PURE__ */ new Map();
	manifestByRun = /* @__PURE__ */ new Map();
	appendChainByPath = /* @__PURE__ */ new Map();
	constructor(outputRoot = flowRunsBaseDir()) {
		this.outputRoot = outputRoot;
	}
	async createRunDir(runId) {
		const runDir = path.join(this.outputRoot, runId);
		await fs.mkdir(path.join(runDir, PROJECTIONS_DIR), { recursive: true });
		await fs.mkdir(path.join(runDir, SESSIONS_DIR), { recursive: true });
		await fs.mkdir(path.join(runDir, ARTIFACTS_DIR), { recursive: true });
		this.traceSeqByRun.set(runDir, 0);
		return runDir;
	}
	async initializeRunBundle(runDir, options) {
		const snapshot = createFlowDefinitionSnapshot(options.flow);
		const manifest = {
			schema: FLOW_BUNDLE_SCHEMA,
			runId: options.state.runId,
			flowName: options.state.flowName,
			runTitle: options.state.runTitle,
			flowPath: options.state.flowPath,
			startedAt: options.state.startedAt,
			finishedAt: options.state.finishedAt,
			status: options.state.status,
			traceSchema: FLOW_TRACE_SCHEMA,
			paths: {
				flow: FLOW_SNAPSHOT_PATH,
				trace: TRACE_PATH,
				runProjection: RUN_PROJECTION_PATH,
				liveProjection: LIVE_PROJECTION_PATH,
				stepsProjection: STEPS_PROJECTION_PATH,
				sessionsDir: SESSIONS_DIR,
				artifactsDir: ARTIFACTS_DIR
			},
			sessions: []
		};
		this.manifestByRun.set(runDir, manifest);
		await writeJsonAtomic(this.resolveRunPath(runDir, FLOW_SNAPSHOT_PATH), snapshot);
		await writeJsonAtomic(this.resolveRunPath(runDir, MANIFEST_PATH), manifest);
		await writeJsonAtomic(this.resolveRunPath(runDir, RUN_PROJECTION_PATH), options.state);
		await writeJsonAtomic(this.resolveRunPath(runDir, LIVE_PROJECTION_PATH), createLiveState(options.state));
		await writeJsonAtomic(this.resolveRunPath(runDir, STEPS_PROJECTION_PATH), options.state.steps);
		await ensureFile(this.resolveRunPath(runDir, TRACE_PATH));
		await this.appendTrace(runDir, options.state, {
			scope: "run",
			type: "run_started",
			payload: {
				flowName: options.state.flowName,
				...options.state.runTitle ? { runTitle: options.state.runTitle } : {},
				...options.state.flowPath ? { flowPath: options.state.flowPath } : {},
				...options.inputArtifact ? { inputArtifact: options.inputArtifact } : {}
			}
		});
	}
	async writeSnapshot(runDir, state, event) {
		state.updatedAt = isoNow();
		await writeJsonAtomic(this.resolveRunPath(runDir, RUN_PROJECTION_PATH), state);
		await writeJsonAtomic(this.resolveRunPath(runDir, LIVE_PROJECTION_PATH), createLiveState(state));
		await writeJsonAtomic(this.resolveRunPath(runDir, STEPS_PROJECTION_PATH), state.steps);
		await this.writeManifest(runDir, state);
		await this.appendTrace(runDir, state, event);
	}
	async writeLive(runDir, state, event) {
		state.updatedAt = isoNow();
		await writeJsonAtomic(this.resolveRunPath(runDir, LIVE_PROJECTION_PATH), createLiveState(state));
		await this.writeManifest(runDir, state);
		await this.appendTrace(runDir, state, event);
	}
	async appendTrace(runDir, state, event) {
		const traceEvent = {
			seq: this.nextTraceSeq(runDir),
			at: isoNow(),
			runId: state.runId,
			...event
		};
		await this.appendJsonLine(this.resolveRunPath(runDir, TRACE_PATH), traceEvent);
		return traceEvent;
	}
	async writeArtifact(runDir, state, content, options) {
		const buffer = toArtifactBuffer(content, options.mediaType);
		const sha256 = createHash("sha256").update(buffer).digest("hex");
		const relativePath = path.posix.join(ARTIFACTS_DIR, `sha256-${sha256}${normalizeArtifactExtension(options.extension)}`);
		const filePath = this.resolveRunPath(runDir, relativePath);
		await fs.mkdir(path.dirname(filePath), { recursive: true });
		try {
			await fs.access(filePath);
		} catch {
			await fs.writeFile(filePath, buffer);
		}
		const artifact = {
			path: relativePath,
			mediaType: options.mediaType,
			bytes: buffer.byteLength,
			sha256
		};
		if (options.emitTrace !== false) await this.appendTrace(runDir, state, {
			scope: "artifact",
			type: "artifact_written",
			nodeId: options.nodeId,
			attemptId: options.attemptId,
			sessionId: options.sessionId,
			artifact,
			payload: { artifact }
		});
		return artifact;
	}
	async ensureSessionBundle(runDir, state, binding, record) {
		const sessionDir = this.resolveRunPath(runDir, sessionDirPath(binding.bundleId));
		await fs.mkdir(sessionDir, { recursive: true });
		await writeJsonAtomic(path.join(sessionDir, "binding.json"), binding);
		await ensureFile(path.join(sessionDir, "events.ndjson"));
		if (record) await this.writeSessionRecord(runDir, state, binding, record);
		const manifest = this.getManifest(runDir, state);
		const isNew = !manifest.sessions.find((entry) => entry.id === binding.bundleId);
		if (isNew) {
			const entry = {
				id: binding.bundleId,
				handle: binding.handle,
				bindingPath: path.posix.join(sessionDirPath(binding.bundleId), "binding.json"),
				recordPath: path.posix.join(sessionDirPath(binding.bundleId), "record.json"),
				eventsPath: path.posix.join(sessionDirPath(binding.bundleId), "events.ndjson")
			};
			manifest.sessions.push(entry);
			await writeJsonAtomic(this.resolveRunPath(runDir, MANIFEST_PATH), manifest);
		}
		if (isNew) await this.appendTrace(runDir, state, {
			scope: "session",
			type: "session_bound",
			sessionId: binding.bundleId,
			payload: {
				sessionId: binding.bundleId,
				handle: binding.handle,
				bindingArtifact: {
					path: path.posix.join(sessionDirPath(binding.bundleId), "binding.json"),
					mediaType: "application/json",
					sha256: await fileSha256(path.join(sessionDir, "binding.json"))
				}
			}
		});
	}
	async writeSessionRecord(runDir, _state, binding, record) {
		const bundledRecord = createBundledSessionRecord(binding, record, this.sessionSeqByBundle.get(`${runDir}::${binding.bundleId}`) ?? 0);
		await writeJsonAtomic(this.resolveRunPath(runDir, path.posix.join(sessionDirPath(binding.bundleId), "record.json")), bundledRecord);
	}
	async appendSessionEvent(runDir, binding, direction, message) {
		const sessionKey = `${runDir}::${binding.bundleId}`;
		const seq = (this.sessionSeqByBundle.get(sessionKey) ?? 0) + 1;
		this.sessionSeqByBundle.set(sessionKey, seq);
		await this.appendJsonLine(this.resolveRunPath(runDir, path.posix.join(sessionDirPath(binding.bundleId), "events.ndjson")), {
			seq,
			at: isoNow(),
			direction,
			message
		});
		return seq;
	}
	getManifest(runDir, state) {
		const existing = this.manifestByRun.get(runDir);
		if (existing) {
			existing.startedAt = state.startedAt;
			existing.finishedAt = state.finishedAt;
			existing.status = state.status;
			existing.flowPath = state.flowPath;
			existing.flowName = state.flowName;
			existing.runTitle = state.runTitle;
			return existing;
		}
		const created = {
			schema: FLOW_BUNDLE_SCHEMA,
			runId: state.runId,
			flowName: state.flowName,
			runTitle: state.runTitle,
			flowPath: state.flowPath,
			startedAt: state.startedAt,
			finishedAt: state.finishedAt,
			status: state.status,
			traceSchema: FLOW_TRACE_SCHEMA,
			paths: {
				flow: FLOW_SNAPSHOT_PATH,
				trace: TRACE_PATH,
				runProjection: RUN_PROJECTION_PATH,
				liveProjection: LIVE_PROJECTION_PATH,
				stepsProjection: STEPS_PROJECTION_PATH,
				sessionsDir: SESSIONS_DIR,
				artifactsDir: ARTIFACTS_DIR
			},
			sessions: []
		};
		this.manifestByRun.set(runDir, created);
		return created;
	}
	async writeManifest(runDir, state) {
		const manifest = this.getManifest(runDir, state);
		await writeJsonAtomic(this.resolveRunPath(runDir, MANIFEST_PATH), manifest);
	}
	nextTraceSeq(runDir) {
		const next = (this.traceSeqByRun.get(runDir) ?? 0) + 1;
		this.traceSeqByRun.set(runDir, next);
		return next;
	}
	resolveRunPath(runDir, relativePath) {
		return path.join(runDir, ...relativePath.split("/"));
	}
	async appendJsonLine(filePath, value) {
		const tracked = (this.appendChainByPath.get(filePath) ?? Promise.resolve()).then(async () => {
			await fs.mkdir(path.dirname(filePath), { recursive: true });
			await fs.appendFile(filePath, `${JSON.stringify(value)}\n`, "utf8");
		}).finally(() => {
			if (this.appendChainByPath.get(filePath) === tracked) this.appendChainByPath.delete(filePath);
		});
		this.appendChainByPath.set(filePath, tracked);
		await tracked;
	}
};
function createLiveState(state) {
	return {
		runId: state.runId,
		flowName: state.flowName,
		runTitle: state.runTitle,
		flowPath: state.flowPath,
		startedAt: state.startedAt,
		finishedAt: state.finishedAt,
		updatedAt: state.updatedAt,
		status: state.status,
		currentNode: state.currentNode,
		currentAttemptId: state.currentAttemptId,
		currentNodeType: state.currentNodeType,
		currentNodeStartedAt: state.currentNodeStartedAt,
		lastHeartbeatAt: state.lastHeartbeatAt,
		statusDetail: state.statusDetail,
		waitingOn: state.waitingOn,
		error: state.error,
		sessionBindings: state.sessionBindings
	};
}
function createFlowDefinitionSnapshot(flow) {
	return {
		schema: FLOW_SNAPSHOT_SCHEMA,
		name: flow.name,
		...flow.run?.title !== void 0 ? { run: { hasTitle: true } } : {},
		...flow.permissions ? { permissions: structuredClone(flow.permissions) } : {},
		startAt: flow.startAt,
		nodes: Object.fromEntries(Object.entries(flow.nodes).map(([nodeId, node]) => [nodeId, snapshotNode(node)])),
		edges: structuredClone(flow.edges)
	};
}
function snapshotNode(node) {
	const common = {
		nodeType: node.nodeType,
		...node.timeoutMs !== void 0 ? { timeoutMs: node.timeoutMs } : {},
		...node.heartbeatMs !== void 0 ? { heartbeatMs: node.heartbeatMs } : {},
		...node.statusDetail ? { statusDetail: node.statusDetail } : {}
	};
	switch (node.nodeType) {
		case "acp": return {
			...common,
			...node.profile ? { profile: node.profile } : {},
			session: {
				...node.session?.handle ? { handle: node.session.handle } : {},
				...node.session?.isolated ? { isolated: true } : {}
			},
			cwd: snapshotCwd(node.cwd),
			hasPrompt: true,
			hasParse: typeof node.parse === "function"
		};
		case "compute": return {
			...common,
			hasRun: true
		};
		case "action": {
			const actionExecution = "exec" in node ? "shell" : "function";
			return {
				...common,
				actionExecution,
				hasRun: "run" in node,
				hasExec: "exec" in node,
				hasParse: "parse" in node && typeof node.parse === "function"
			};
		}
		case "checkpoint": return {
			...common,
			...node.summary ? { summary: node.summary } : {},
			hasRun: typeof node.run === "function"
		};
	}
}
function snapshotCwd(cwd) {
	if (typeof cwd === "function") return { mode: "dynamic" };
	if (typeof cwd === "string") return {
		mode: "static",
		value: cwd
	};
	return { mode: "default" };
}
function createBundledSessionRecord(binding, record, bundleLastSeq) {
	return {
		...structuredClone(record),
		lastSeq: bundleLastSeq,
		eventLog: {
			...structuredClone(record.eventLog),
			active_path: path.posix.join(sessionDirPath(binding.bundleId), "events.ndjson"),
			segment_count: 1,
			max_segments: 1
		}
	};
}
async function writeJsonAtomic(filePath, value) {
	const tempPath = `${filePath}.${process.pid}.${Date.now()}.${randomUUID()}.tmp`;
	const payload = JSON.stringify(value, null, 2);
	await fs.mkdir(path.dirname(filePath), { recursive: true });
	await fs.writeFile(tempPath, `${payload}\n`, "utf8");
	await fs.rename(tempPath, filePath);
}
async function ensureFile(filePath) {
	await fs.mkdir(path.dirname(filePath), { recursive: true });
	await fs.appendFile(filePath, "", "utf8");
}
async function fileSha256(filePath) {
	const payload = await fs.readFile(filePath);
	return createHash("sha256").update(payload).digest("hex");
}
function toArtifactBuffer(content, mediaType) {
	if (typeof content === "string") return Buffer.from(content, "utf8");
	if (Buffer.isBuffer(content)) return content;
	if (content instanceof Uint8Array) return Buffer.from(content);
	if (mediaType === "application/json") return Buffer.from(`${JSON.stringify(content, null, 2)}\n`, "utf8");
	return Buffer.from(String(content), "utf8");
}
function normalizeArtifactExtension(extension) {
	if (!extension) return "";
	return extension.startsWith(".") ? extension : `.${extension}`;
}
function sessionDirPath(bundleId) {
	return path.posix.join(SESSIONS_DIR, bundleId);
}
function isoNow() {
	return (/* @__PURE__ */ new Date()).toISOString();
}
//#endregion
//#region src/flows/runtime.ts
const DEFAULT_FLOW_HEARTBEAT_MS = 5e3;
const DEFAULT_FLOW_STEP_TIMEOUT_MS = 15 * 6e4;
var FlowRunner = class {
	resolveAgent;
	defaultCwd;
	permissionMode;
	mcpServers;
	nonInteractivePermissions;
	authCredentials;
	authPolicy;
	timeoutMs;
	defaultNodeTimeoutMs;
	verbose;
	suppressSdkConsoleErrors;
	sessionOptions;
	services;
	store;
	pendingPersistentSessionClients = /* @__PURE__ */ new Map();
	constructor(options) {
		this.resolveAgent = options.resolveAgent;
		this.defaultCwd = options.resolveAgent(void 0).cwd;
		this.permissionMode = options.permissionMode;
		this.mcpServers = options.mcpServers;
		this.nonInteractivePermissions = options.nonInteractivePermissions;
		this.authCredentials = options.authCredentials;
		this.authPolicy = options.authPolicy;
		this.timeoutMs = options.timeoutMs;
		this.defaultNodeTimeoutMs = options.defaultNodeTimeoutMs ?? options.timeoutMs ?? DEFAULT_FLOW_STEP_TIMEOUT_MS;
		this.verbose = options.verbose;
		this.suppressSdkConsoleErrors = options.suppressSdkConsoleErrors;
		this.sessionOptions = options.sessionOptions;
		this.services = options.services ?? {};
		this.store = new FlowRunStore(options.outputRoot);
	}
	async run(flow, input, options = {}) {
		validateFlowDefinition(flow);
		const runId = createRunId(flow.name);
		const runTitle = await resolveFlowRunTitle(flow, input, options.flowPath);
		const runDir = await this.store.createRunDir(runId);
		const state = {
			runId,
			flowName: flow.name,
			runTitle,
			flowPath: options.flowPath,
			startedAt: isoNow$1(),
			updatedAt: isoNow$1(),
			status: "running",
			input,
			outputs: {},
			results: {},
			steps: [],
			sessionBindings: {}
		};
		const inputArtifact = await this.store.writeArtifact(runDir, state, input, {
			mediaType: "application/json",
			extension: "json",
			emitTrace: false
		});
		await this.store.initializeRunBundle(runDir, {
			flow,
			state,
			inputArtifact
		});
		let current = flow.startAt;
		const attemptCounts = /* @__PURE__ */ new Map();
		try {
			return await withInterrupt(async () => {
				try {
					while (current) {
						const node = flow.nodes[current];
						if (!node) throw new Error(`Unknown flow node: ${current}`);
						const attemptId = nextAttemptId(attemptCounts, current);
						const startedAt = isoNow$1();
						const context = makeFlowNodeContext(state, input, this.services);
						let output;
						let promptText = null;
						let rawText = null;
						let sessionInfo = null;
						let agentInfo = null;
						let trace = null;
						markNodeStarted(state, current, attemptId, node.nodeType, startedAt, node.statusDetail);
						await this.store.writeSnapshot(runDir, state, {
							scope: "node",
							type: "node_started",
							nodeId: current,
							attemptId,
							payload: {
								nodeType: node.nodeType,
								...node.timeoutMs !== void 0 ? { timeoutMs: node.timeoutMs ?? this.defaultNodeTimeoutMs } : { timeoutMs: this.defaultNodeTimeoutMs },
								...state.statusDetail ? { statusDetail: state.statusDetail } : {}
							}
						});
						let nodeResult;
						let executionError;
						try {
							({output, promptText, rawText, sessionInfo, agentInfo, trace} = await this.executeNode(runDir, state, flow, current, node, context));
							trace = await finalizeStepTrace(this.store, runDir, state, current, attemptId, output, trace);
							nodeResult = createNodeResult({
								attemptId,
								nodeId: current,
								nodeType: node.nodeType,
								outcome: "ok",
								startedAt,
								finishedAt: isoNow$1(),
								output
							});
						} catch (error) {
							executionError = error;
							trace = extractAttachedStepTrace(error) ?? trace;
							trace = await finalizeStepTrace(this.store, runDir, state, current, attemptId, void 0, trace);
							nodeResult = createNodeResult({
								attemptId,
								nodeId: current,
								nodeType: node.nodeType,
								outcome: outcomeForError(error),
								startedAt,
								finishedAt: isoNow$1(),
								error: error instanceof Error ? error.message : String(error)
							});
						}
						state.results[current] = nodeResult;
						if (nodeResult.outcome === "ok" && node.nodeType === "checkpoint") {
							state.outputs[current] = output;
							state.waitingOn = current;
							state.updatedAt = isoNow$1();
							state.status = "waiting";
							clearActiveNode(state, output?.summary ?? current);
							state.steps.push({
								attemptId,
								nodeId: current,
								nodeType: node.nodeType,
								outcome: nodeResult.outcome,
								startedAt,
								finishedAt: nodeResult.finishedAt,
								promptText,
								rawText,
								output,
								session: null,
								agent: null,
								...trace ? { trace } : {}
							});
							await this.store.writeSnapshot(runDir, state, {
								scope: "node",
								type: "node_outcome",
								nodeId: current,
								attemptId,
								payload: createNodeOutcomePayload(nodeResult, trace)
							});
							return {
								runDir,
								state
							};
						}
						if (nodeResult.outcome === "ok") state.outputs[current] = output;
						state.updatedAt = isoNow$1();
						clearActiveNode(state);
						state.steps.push({
							attemptId,
							nodeId: current,
							nodeType: node.nodeType,
							outcome: nodeResult.outcome,
							startedAt,
							finishedAt: nodeResult.finishedAt,
							promptText,
							rawText,
							output,
							error: nodeResult.error,
							session: sessionInfo,
							agent: agentInfo,
							...trace ? { trace } : {}
						});
						await this.store.writeSnapshot(runDir, state, {
							scope: "node",
							type: "node_outcome",
							nodeId: current,
							attemptId,
							payload: createNodeOutcomePayload(nodeResult, trace)
						});
						if (nodeResult.outcome === "ok") {
							current = resolveNext(flow.edges, current, output, nodeResult);
							continue;
						}
						const next = resolveNextForOutcome(flow.edges, current, nodeResult);
						if (next) {
							current = next;
							continue;
						}
						throw executionError;
					}
					state.status = "completed";
					state.finishedAt = isoNow$1();
					state.updatedAt = state.finishedAt;
					clearActiveNode(state);
					await this.store.writeSnapshot(runDir, state, {
						scope: "run",
						type: "run_completed",
						payload: { status: state.status }
					});
					return {
						runDir,
						state
					};
				} catch (error) {
					await persistRunFailure(this.store, runDir, state, error);
					throw error;
				}
			}, async () => {
				await persistRunFailure(this.store, runDir, state, new InterruptedError());
			});
		} finally {
			await this.closePendingPersistentSessionClients();
		}
	}
	async executeNode(runDir, state, flow, nodeId, node, context) {
		switch (node.nodeType) {
			case "compute": return await this.executeComputeNode(runDir, state, node, context);
			case "action": return await this.executeActionNode(runDir, state, node, context);
			case "checkpoint": return await this.executeCheckpointNode(runDir, state, nodeId, node, context);
			case "acp": return await this.executeAcpNode(runDir, state, flow, node, context);
			default: throw new Error(`Unsupported flow node: ${String(node)}`);
		}
	}
	async executeComputeNode(runDir, state, node, context) {
		const nodeTimeoutMs = node.timeoutMs ?? this.defaultNodeTimeoutMs;
		return {
			output: await this.runWithHeartbeat(runDir, state, state.currentNode ?? "", node, nodeTimeoutMs, async () => await Promise.resolve(node.run(context))),
			promptText: null,
			rawText: null,
			sessionInfo: null,
			agentInfo: null,
			trace: null
		};
	}
	async executeActionNode(runDir, state, node, context) {
		const nodeTimeoutMs = node.timeoutMs ?? this.defaultNodeTimeoutMs;
		if ("run" in node) return {
			output: await this.runWithHeartbeat(runDir, state, state.currentNode ?? "", node, nodeTimeoutMs, async () => await Promise.resolve(node.run(context))),
			promptText: null,
			rawText: null,
			sessionInfo: null,
			agentInfo: null,
			trace: { action: { actionType: "function" } }
		};
		const { output, rawText, trace } = await this.runWithHeartbeat(runDir, state, state.currentNode ?? "", node, nodeTimeoutMs, async () => {
			const execution = await Promise.resolve(node.exec(context));
			const effectiveExecution = {
				...execution,
				cwd: resolveShellActionCwd(this.defaultCwd, execution.cwd),
				timeoutMs: execution.timeoutMs ?? nodeTimeoutMs
			};
			updateStatusDetail(state, formatShellActionSummary(effectiveExecution));
			await this.store.writeLive(runDir, state, {
				scope: "node",
				type: "node_heartbeat",
				nodeId: state.currentNode,
				attemptId: state.currentAttemptId,
				payload: { statusDetail: state.statusDetail }
			});
			await this.store.appendTrace(runDir, state, {
				scope: "action",
				type: "action_prepared",
				nodeId: state.currentNode,
				attemptId: state.currentAttemptId,
				payload: { action: {
					actionType: "shell",
					command: effectiveExecution.command,
					args: effectiveExecution.args ?? [],
					cwd: effectiveExecution.cwd
				} }
			});
			const result = await runShellAction(effectiveExecution);
			const stdoutArtifact = await this.store.writeArtifact(runDir, state, result.stdout, {
				mediaType: "text/plain",
				extension: "txt",
				nodeId: state.currentNode,
				attemptId: state.currentAttemptId
			});
			const stderrArtifact = await this.store.writeArtifact(runDir, state, result.stderr, {
				mediaType: "text/plain",
				extension: "txt",
				nodeId: state.currentNode,
				attemptId: state.currentAttemptId
			});
			await this.store.appendTrace(runDir, state, {
				scope: "action",
				type: "action_completed",
				nodeId: state.currentNode,
				attemptId: state.currentAttemptId,
				payload: {
					action: {
						actionType: "shell",
						command: result.command,
						args: result.args,
						cwd: result.cwd,
						exitCode: result.exitCode,
						signal: result.signal,
						durationMs: result.durationMs
					},
					stdoutArtifact,
					stderrArtifact
				}
			});
			const trace = {
				action: {
					actionType: "shell",
					command: result.command,
					args: result.args,
					cwd: result.cwd,
					exitCode: result.exitCode,
					signal: result.signal,
					durationMs: result.durationMs
				},
				stdoutArtifact,
				stderrArtifact
			};
			let parsedOutput;
			try {
				parsedOutput = node.parse ? await node.parse(result, context) : result;
			} catch (error) {
				throw attachStepTrace(error, trace);
			}
			return {
				output: parsedOutput,
				rawText: result.combinedOutput,
				trace
			};
		});
		return {
			output,
			promptText: null,
			rawText,
			sessionInfo: null,
			agentInfo: null,
			trace
		};
	}
	async executeCheckpointNode(runDir, state, nodeId, node, context) {
		const nodeTimeoutMs = node.timeoutMs ?? this.defaultNodeTimeoutMs;
		return {
			output: typeof node.run === "function" ? await this.runWithHeartbeat(runDir, state, state.currentNode ?? "", node, nodeTimeoutMs, async () => await Promise.resolve(node.run?.(context))) : {
				checkpoint: nodeId,
				summary: node.summary ?? nodeId
			},
			promptText: null,
			rawText: null,
			sessionInfo: null,
			agentInfo: null,
			trace: null
		};
	}
	async executeAcpNode(runDir, state, flow, node, context) {
		const nodeTimeoutMs = node.timeoutMs ?? this.defaultNodeTimeoutMs;
		let boundSession = null;
		return await this.runWithHeartbeat(runDir, state, state.currentNode ?? "", node, nodeTimeoutMs, async () => {
			const resolvedAgent = this.resolveAgent(node.profile);
			const agentInfo = {
				...resolvedAgent,
				cwd: await resolveNodeCwd(resolvedAgent.cwd, node.cwd, context)
			};
			const prompt = normalizePromptInput(await Promise.resolve(node.prompt(context)));
			const promptText = promptToDisplayText(prompt);
			updateStatusDetail(state, summarizePrompt(promptText, node.statusDetail));
			await this.store.writeLive(runDir, state, {
				scope: "node",
				type: "node_heartbeat",
				nodeId: state.currentNode,
				attemptId: state.currentAttemptId,
				payload: { statusDetail: state.statusDetail }
			});
			const promptArtifact = await this.store.writeArtifact(runDir, state, promptText, {
				mediaType: "text/plain",
				extension: "txt",
				nodeId: state.currentNode,
				attemptId: state.currentAttemptId
			});
			if (node.session?.isolated) {
				const isolatedBinding = createIsolatedSessionBinding(flow.name, state.runId, state.currentAttemptId ?? randomUUID(), node.profile, agentInfo);
				const initialIsolatedRecord = createSyntheticSessionRecord({
					binding: isolatedBinding,
					createdAt: state.currentNodeStartedAt ?? isoNow$1(),
					updatedAt: state.currentNodeStartedAt ?? isoNow$1(),
					conversation: createSessionConversation(state.currentNodeStartedAt ?? isoNow$1()),
					acpxState: void 0,
					lastSeq: 0
				});
				await this.store.ensureSessionBundle(runDir, state, isolatedBinding, initialIsolatedRecord);
				await this.store.appendTrace(runDir, state, {
					scope: "acp",
					type: "acp_prompt_prepared",
					nodeId: state.currentNode,
					attemptId: state.currentAttemptId,
					sessionId: isolatedBinding.bundleId,
					payload: {
						sessionId: isolatedBinding.bundleId,
						promptArtifact
					}
				});
				const isolatedPrompt = await this.runIsolatedPrompt(runDir, state, isolatedBinding, agentInfo, prompt, nodeTimeoutMs);
				const rawResponseArtifact = await this.store.writeArtifact(runDir, state, isolatedPrompt.rawText, {
					mediaType: "text/plain",
					extension: "txt",
					nodeId: state.currentNode,
					attemptId: state.currentAttemptId,
					sessionId: isolatedBinding.bundleId
				});
				await this.store.appendTrace(runDir, state, {
					scope: "acp",
					type: "acp_response_parsed",
					nodeId: state.currentNode,
					attemptId: state.currentAttemptId,
					sessionId: isolatedBinding.bundleId,
					payload: {
						sessionId: isolatedBinding.bundleId,
						conversation: isolatedPrompt.conversation,
						rawResponseArtifact
					}
				});
				const trace = {
					sessionId: isolatedBinding.bundleId,
					promptArtifact,
					rawResponseArtifact,
					conversation: isolatedPrompt.conversation
				};
				let parsedOutput;
				try {
					parsedOutput = node.parse ? await node.parse(isolatedPrompt.rawText, context) : isolatedPrompt.rawText;
				} catch (error) {
					throw attachStepTrace(error, trace);
				}
				return {
					output: parsedOutput,
					promptText,
					rawText: isolatedPrompt.rawText,
					sessionInfo: isolatedBinding,
					agentInfo,
					trace
				};
			}
			boundSession = await this.ensureSessionBinding(runDir, state, flow, node, agentInfo, nodeTimeoutMs);
			await this.store.appendTrace(runDir, state, {
				scope: "acp",
				type: "acp_prompt_prepared",
				nodeId: state.currentNode,
				attemptId: state.currentAttemptId,
				sessionId: boundSession.bundleId,
				payload: {
					sessionId: boundSession.bundleId,
					promptArtifact
				}
			});
			const persistentPrompt = await this.runPersistentPrompt(runDir, state, boundSession, prompt, nodeTimeoutMs);
			const rawResponseArtifact = await this.store.writeArtifact(runDir, state, persistentPrompt.rawText, {
				mediaType: "text/plain",
				extension: "txt",
				nodeId: state.currentNode,
				attemptId: state.currentAttemptId,
				sessionId: persistentPrompt.sessionInfo.bundleId
			});
			await this.store.appendTrace(runDir, state, {
				scope: "acp",
				type: "acp_response_parsed",
				nodeId: state.currentNode,
				attemptId: state.currentAttemptId,
				sessionId: persistentPrompt.sessionInfo.bundleId,
				payload: {
					sessionId: persistentPrompt.sessionInfo.bundleId,
					conversation: persistentPrompt.conversation,
					rawResponseArtifact
				}
			});
			const trace = {
				sessionId: persistentPrompt.sessionInfo.bundleId,
				promptArtifact,
				rawResponseArtifact,
				conversation: persistentPrompt.conversation
			};
			let parsedOutput;
			try {
				parsedOutput = node.parse ? await node.parse(persistentPrompt.rawText, context) : persistentPrompt.rawText;
			} catch (error) {
				throw attachStepTrace(error, trace);
			}
			return {
				output: parsedOutput,
				promptText,
				rawText: persistentPrompt.rawText,
				sessionInfo: persistentPrompt.sessionInfo,
				agentInfo,
				trace
			};
		}, async () => {
			if (!boundSession) return;
			await cancelSessionPrompt({ sessionId: boundSession.acpxRecordId });
		});
	}
	async runWithHeartbeat(runDir, state, nodeId, node, timeoutMs, run, onTimeout) {
		const heartbeatMs = Math.max(0, Math.round(node.heartbeatMs ?? DEFAULT_FLOW_HEARTBEAT_MS));
		let timer;
		let active = true;
		const heartbeat = async () => {
			if (!active) return;
			state.lastHeartbeatAt = isoNow$1();
			state.updatedAt = state.lastHeartbeatAt;
			await this.store.writeLive(runDir, state, {
				scope: "node",
				type: "node_heartbeat",
				nodeId,
				attemptId: state.currentAttemptId,
				payload: { statusDetail: state.statusDetail }
			});
		};
		if (heartbeatMs > 0) timer = setInterval(() => {
			heartbeat();
		}, heartbeatMs);
		try {
			return await withTimeout(run(), timeoutMs);
		} catch (error) {
			if (error instanceof TimeoutError && onTimeout) await onTimeout().catch(() => {});
			throw error;
		} finally {
			active = false;
			if (timer) clearInterval(timer);
		}
	}
	async ensureSessionBinding(runDir, state, flow, node, agent, timeoutMs) {
		const handle = node.session?.handle ?? "main";
		const key = createSessionBindingKey(agent.agentCommand, agent.cwd, handle);
		const existing = state.sessionBindings[key];
		if (existing) {
			await this.store.ensureSessionBundle(runDir, state, existing);
			return existing;
		}
		const name = createSessionName(flow.name, handle, agent.cwd, state.runId);
		const created = await createSessionWithClient({
			agentCommand: agent.agentCommand,
			cwd: agent.cwd,
			name,
			mcpServers: this.mcpServers,
			permissionMode: this.permissionMode,
			nonInteractivePermissions: this.nonInteractivePermissions,
			authCredentials: this.authCredentials,
			authPolicy: this.authPolicy,
			timeoutMs,
			verbose: this.verbose,
			sessionOptions: this.sessionOptions
		});
		const binding = {
			key,
			handle,
			bundleId: createSessionBundleId(handle, key),
			name,
			profile: node.profile,
			agentName: agent.agentName,
			agentCommand: agent.agentCommand,
			cwd: agent.cwd,
			acpxRecordId: created.record.acpxRecordId,
			acpSessionId: created.record.acpSessionId,
			agentSessionId: created.record.agentSessionId
		};
		state.sessionBindings[key] = binding;
		this.pendingPersistentSessionClients.set(binding.key, created.client);
		await this.store.ensureSessionBundle(runDir, state, binding, created.record);
		return binding;
	}
	async refreshSessionBinding(binding) {
		const record = await resolveSessionRecord(binding.acpxRecordId);
		return {
			...binding,
			acpSessionId: record.acpSessionId,
			agentSessionId: record.agentSessionId
		};
	}
	async runPersistentPrompt(runDir, state, binding, prompt, timeoutMs) {
		const capture = createQuietCaptureOutput();
		const beforeRecord = await resolveSessionRecord(binding.acpxRecordId);
		let eventStartSeq;
		let eventEndSeq;
		const pendingEventWrites = [];
		const initialClient = this.pendingPersistentSessionClients.get(binding.key);
		if (initialClient) this.pendingPersistentSessionClients.delete(binding.key);
		try {
			await sendSessionDirect({
				sessionId: binding.acpxRecordId,
				prompt,
				resumePolicy: "same-session-only",
				mcpServers: this.mcpServers,
				permissionMode: this.permissionMode,
				nonInteractivePermissions: this.nonInteractivePermissions,
				authCredentials: this.authCredentials,
				authPolicy: this.authPolicy,
				outputFormatter: capture.formatter,
				onAcpMessage: (direction, message) => {
					const pending = this.store.appendSessionEvent(runDir, binding, direction, message).then((seq) => {
						eventStartSeq = eventStartSeq === void 0 ? seq : Math.min(eventStartSeq, seq);
						eventEndSeq = eventEndSeq === void 0 ? seq : Math.max(eventEndSeq, seq);
					});
					pendingEventWrites.push(pending);
				},
				suppressSdkConsoleErrors: this.suppressSdkConsoleErrors,
				timeoutMs,
				verbose: this.verbose,
				client: initialClient
			});
			await Promise.all(pendingEventWrites);
			const sessionInfo = await this.refreshSessionBinding(binding);
			state.sessionBindings[sessionInfo.key] = sessionInfo;
			await this.store.ensureSessionBundle(runDir, state, sessionInfo);
			const afterRecord = await resolveSessionRecord(sessionInfo.acpxRecordId);
			await this.store.writeSessionRecord(runDir, state, sessionInfo, afterRecord);
			const messageStartResolved = findConversationDeltaStart(beforeRecord.messages, afterRecord.messages);
			return {
				rawText: capture.read(),
				sessionInfo,
				conversation: {
					sessionId: sessionInfo.bundleId,
					messageStart: messageStartResolved,
					messageEnd: Math.max(messageStartResolved, afterRecord.messages.length - 1),
					eventStartSeq: eventStartSeq ?? (() => {
						throw new Error(`Missing ACP event capture for session ${sessionInfo.bundleId}`);
					})(),
					eventEndSeq: eventEndSeq ?? (() => {
						throw new Error(`Missing ACP event capture for session ${sessionInfo.bundleId}`);
					})()
				}
			};
		} finally {
			if (initialClient) await initialClient.close().catch(() => {});
		}
	}
	async closePendingPersistentSessionClients() {
		const pendingClients = [...this.pendingPersistentSessionClients.values()];
		this.pendingPersistentSessionClients.clear();
		await Promise.all(pendingClients.map(async (client) => {
			await client.close().catch(() => {});
		}));
	}
	async runIsolatedPrompt(runDir, state, binding, agent, prompt, timeoutMs) {
		const capture = createQuietCaptureOutput();
		const conversation = createSessionConversation(state.currentNodeStartedAt ?? isoNow$1());
		let acpxState;
		recordPromptSubmission(conversation, prompt, state.currentNodeStartedAt ?? isoNow$1());
		let eventStartSeq;
		let eventEndSeq;
		const pendingEventWrites = [];
		const result = await runOnce({
			agentCommand: agent.agentCommand,
			cwd: agent.cwd,
			prompt,
			mcpServers: this.mcpServers,
			permissionMode: this.permissionMode,
			nonInteractivePermissions: this.nonInteractivePermissions,
			authCredentials: this.authCredentials,
			authPolicy: this.authPolicy,
			outputFormatter: capture.formatter,
			onAcpMessage: (direction, message) => {
				const pending = this.store.appendSessionEvent(runDir, binding, direction, message).then((seq) => {
					eventStartSeq = eventStartSeq === void 0 ? seq : Math.min(eventStartSeq, seq);
					eventEndSeq = eventEndSeq === void 0 ? seq : Math.max(eventEndSeq, seq);
				});
				pendingEventWrites.push(pending);
			},
			onSessionUpdate: (notification) => {
				acpxState = recordSessionUpdate(conversation, acpxState, notification);
			},
			onClientOperation: (operation) => {
				acpxState = recordClientOperation(conversation, acpxState, operation);
			},
			suppressSdkConsoleErrors: this.suppressSdkConsoleErrors,
			timeoutMs,
			verbose: this.verbose,
			sessionOptions: this.sessionOptions
		});
		await Promise.all(pendingEventWrites);
		const sessionInfo = {
			...binding,
			acpxRecordId: result.sessionId,
			acpSessionId: result.sessionId
		};
		await this.store.ensureSessionBundle(runDir, state, sessionInfo);
		const syntheticRecord = createSyntheticSessionRecord({
			binding: sessionInfo,
			createdAt: state.currentNodeStartedAt ?? isoNow$1(),
			updatedAt: conversation.updated_at,
			conversation,
			acpxState: cloneSessionAcpxState(acpxState),
			lastSeq: eventEndSeq ?? 0
		});
		await this.store.writeSessionRecord(runDir, state, sessionInfo, syntheticRecord);
		return {
			rawText: capture.read(),
			sessionInfo,
			conversation: {
				sessionId: sessionInfo.bundleId,
				messageStart: 0,
				messageEnd: Math.max(0, conversation.messages.length - 1),
				eventStartSeq: eventStartSeq ?? (() => {
					throw new Error(`Missing ACP event capture for session ${sessionInfo.bundleId}`);
				})(),
				eventEndSeq: eventEndSeq ?? (() => {
					throw new Error(`Missing ACP event capture for session ${sessionInfo.bundleId}`);
				})()
			}
		};
	}
};
//#endregion
//#region src/flows/json.ts
function parseJsonObject(text, options = {}) {
	const trimmed = String(text ?? "").trim();
	if (!trimmed) throw new Error("Expected JSON output, got empty text");
	const mode = options.mode ?? "compat";
	const direct = tryParse(trimmed);
	if (direct.ok) return direct.value;
	if (mode === "fenced" || mode === "compat") {
		const fencedMatch = trimmed.match(/```(?:json)?\s*([\s\S]*?)```/i);
		if (fencedMatch) {
			const fenced = tryParse(fencedMatch[1].trim());
			if (fenced.ok) return fenced.value;
		}
	}
	if (mode === "compat") for (const candidate of extractBalancedJsonCandidates(trimmed)) {
		const parsed = tryParse(candidate);
		if (parsed.ok) return parsed.value;
	}
	throw new Error(`Could not parse JSON from assistant output:\n${trimmed}`);
}
function parseStrictJsonObject(text) {
	return parseJsonObject(text, { mode: "strict" });
}
function extractJsonObject(text) {
	return parseJsonObject(text, { mode: "compat" });
}
function tryParse(text) {
	try {
		return {
			ok: true,
			value: JSON.parse(text)
		};
	} catch {
		return { ok: false };
	}
}
function extractBalancedJsonCandidates(text) {
	const candidates = [];
	for (let index = 0; index < text.length; index += 1) {
		if (text[index] !== "{" && text[index] !== "[") continue;
		const result = scanBalanced(text, index);
		if (result) candidates.push(result);
	}
	return candidates;
}
function scanBalanced(text, startIndex) {
	const stack = [];
	let inString = false;
	let escaped = false;
	for (let index = startIndex; index < text.length; index += 1) {
		const char = text[index];
		if (inString) {
			if (escaped) escaped = false;
			else if (char === "\\") escaped = true;
			else if (char === "\"") inString = false;
			continue;
		}
		if (char === "\"") {
			inString = true;
			continue;
		}
		if (char === "{" || char === "[") {
			stack.push(char);
			continue;
		}
		if (char !== "}" && char !== "]") continue;
		const last = stack.at(-1);
		if (last === "{" && char !== "}" || last === "[" && char !== "]") return null;
		stack.pop();
		if (stack.length === 0) return text.slice(startIndex, index + 1);
	}
	return null;
}
//#endregion
export { flowRunsBaseDir as a, action as c, defineFlow as d, shell as f, FlowRunner as i, checkpoint as l, parseJsonObject as n, validateFlowDefinition as o, isDefinedFlow as p, parseStrictJsonObject as r, acp as s, extractJsonObject as t, compute as u };

//# sourceMappingURL=flows-_KmnuUXd.js.map