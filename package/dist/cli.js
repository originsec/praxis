#!/usr/bin/env node
import { T as EXIT_CODES, _ as exitCodeForOutputErrorCode, d as PromptInputValidationError, g as textPrompt, k as OUTPUT_FORMATS, m as parsePromptSource, p as mergePromptSourceWithText, x as normalizeOutputError } from "./perf-metrics-D0um6IR6.js";
import { C as findGitRepositoryRoot, G as DEFAULT_AGENT_NAME, K as listBuiltInAgents, T as findSessionByDirectoryWalk, V as InterruptedError, q as normalizeAgentName, w as findSession } from "./prompt-turn-CXMtXBl-.js";
import { a as buildQueueOwnerArgOverride, n as runSessionQueueOwner, o as flushPerfMetricsCapture, s as installPerfMetricsCapture } from "./session-BtwAKtJ3.js";
import { s as probeQueueOwnerHealth } from "./ipc-BM335WFg.js";
import { c as parseMaxTurns, d as parseTtlSeconds, f as resolveAgentInvocation, g as resolveSessionNameFromFlags, h as resolvePermissionMode, i as addSessionOption, l as parseNonEmptyValue, m as resolveOutputPolicy, n as addPromptInputOption, o as parseAllowedTools, p as resolveGlobalFlags, r as addSessionNameOption, s as parseHistoryLimit, t as addGlobalFlags, u as parseSessionName } from "./flags-ceSqz2T6.js";
import { i as emitJsonResult, n as formatPromptSessionBannerLine, t as agentSessionIdPayload } from "./render-Br-kVPK_.js";
import { t as createOutputFormatter } from "./output-C4QhjpM6.js";
import { readFileSync, realpathSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";
import { Command, CommanderError, InvalidArgumentError } from "commander";
import fs$1 from "node:fs/promises";
import os from "node:os";
//#region src/cli-public.ts
function configurePublicCli(options) {
	const builtInAgents = options.listBuiltInAgents(options.config.agents);
	for (const agentName of builtInAgents) options.registerAgentCommand(options.program, agentName, options.config);
	options.registerDefaultCommands(options.program, options.config);
	const scan = options.detectAgentToken(options.argv);
	if (!scan.hasAgentOverride && scan.token && !options.topLevelVerbs.has(scan.token) && !builtInAgents.includes(scan.token)) options.registerAgentCommand(options.program, scan.token, options.config);
	options.program.argument("[prompt...]", "Prompt text").action(async function(promptParts) {
		if (promptParts.length === 0 && process.stdin.isTTY) {
			if (options.requestedJsonStrict) throw new InvalidArgumentError("Prompt is required (pass as argument, --file, or pipe via stdin)");
			this.outputHelp();
			return;
		}
		await options.handlePromptAction(this, promptParts);
	});
	options.program.addHelpText("after", `
Examples:
  acpx pi "review recent changes"
  acpx openclaw exec "summarize active session state"
  acpx codex sessions new
  acpx codex "fix the tests"
  acpx codex prompt "fix the tests"
  acpx codex --no-wait "queue follow-up task"
  acpx codex exec "what does this repo do"
  acpx codex cancel
  acpx codex set-mode plan
  acpx codex set thought_level high
  acpx codex -s backend "fix the API"
  acpx codex sessions
  acpx codex sessions new --name backend
  acpx codex sessions ensure --name backend
  acpx codex sessions close backend
  acpx codex status
  acpx config show
  acpx config init
  acpx --ttl 30 codex "investigate flaky tests"
  acpx claude "refactor auth"
  acpx --agent ./my-custom-server "do something"`);
}
//#endregion
//#region src/acp/codex-compat.ts
function isCodexInvocation(agentName, agentCommand) {
	if (agentName === "codex") return true;
	return /\bcodex-acp\b/u.test(agentCommand);
}
//#endregion
//#region src/cli/command-handlers.ts
var NoSessionError = class extends Error {
	constructor(message) {
		super(message);
		this.name = "NoSessionError";
	}
};
let sessionModulePromise;
let outputModulePromise;
let outputRenderModulePromise;
function loadSessionModule() {
	sessionModulePromise ??= import("./session-BtwAKtJ3.js").then((n) => n.t);
	return sessionModulePromise;
}
function loadOutputModule() {
	outputModulePromise ??= import("./output-C4QhjpM6.js").then((n) => n.n);
	return outputModulePromise;
}
function loadOutputRenderModule() {
	outputRenderModulePromise ??= import("./render-Br-kVPK_.js").then((n) => n.r);
	return outputRenderModulePromise;
}
async function readPromptInputFromStdin() {
	let data = "";
	for await (const chunk of process.stdin) data += String(chunk);
	return data;
}
async function readPrompt(promptParts, filePath, cwd) {
	try {
		if (filePath) {
			const prompt = mergePromptSourceWithText(filePath === "-" ? await readPromptInputFromStdin() : await fs$1.readFile(path.resolve(cwd, filePath), "utf8"), promptParts.join(" "));
			if (prompt.length === 0) throw new InvalidArgumentError("Prompt from --file is empty");
			return prompt;
		}
		const joined = promptParts.join(" ").trim();
		if (joined.length > 0) return textPrompt(joined);
		if (process.stdin.isTTY) throw new InvalidArgumentError("Prompt is required (pass as argument, --file, or pipe via stdin)");
		const prompt = parsePromptSource(await readPromptInputFromStdin());
		if (prompt.length === 0) throw new InvalidArgumentError("Prompt from stdin is empty");
		return prompt;
	} catch (error) {
		if (error instanceof PromptInputValidationError) throw new InvalidArgumentError(error.message);
		throw error;
	}
}
function applyPermissionExitCode(result) {
	const stats = result.permissionStats;
	const deniedOrCancelled = stats.denied + stats.cancelled;
	if (stats.requested > 0 && stats.approved === 0 && deniedOrCancelled > 0) process.exitCode = EXIT_CODES.PERMISSION_DENIED;
}
function resolveCompatibleConfigId(agent, configId) {
	if (isCodexInvocation(agent.agentName, agent.agentCommand) && configId === "thought_level") return "reasoning_effort";
	return configId;
}
function resolveRequestedOutputPolicy(globalFlags) {
	return {
		...resolveOutputPolicy(globalFlags.format, globalFlags.jsonStrict === true),
		suppressReads: globalFlags.suppressReads === true
	};
}
async function findRoutedSessionOrThrow(agentCommand, agentName, cwd, sessionName) {
	const walkBoundary = findGitRepositoryRoot(cwd) ?? cwd;
	const record = await findSessionByDirectoryWalk({
		agentCommand,
		cwd,
		name: sessionName,
		boundary: walkBoundary
	});
	if (record) return record;
	throw new NoSessionError(`⚠ No acpx session found (searched up to ${walkBoundary}).\nCreate one: ${sessionName ? `acpx ${agentName} sessions new --name ${sessionName}` : `acpx ${agentName} sessions new`}`);
}
async function handlePrompt(explicitAgentName, promptParts, flags, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const outputPolicy = resolveRequestedOutputPolicy(globalFlags);
	const permissionMode = resolvePermissionMode(globalFlags, config.defaultPermissions);
	const prompt = await readPrompt(promptParts, flags.file, globalFlags.cwd);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	const [{ createOutputFormatter }, { printPromptSessionBanner, printQueuedPromptByFormat }, { sendSession }] = await Promise.all([
		loadOutputModule(),
		loadOutputRenderModule(),
		loadSessionModule()
	]);
	const record = await findRoutedSessionOrThrow(agent.agentCommand, agent.agentName, agent.cwd, flags.session);
	const outputFormatter = createOutputFormatter(outputPolicy.format, {
		jsonContext: { sessionId: record.acpxRecordId },
		suppressReads: outputPolicy.suppressReads
	});
	await printPromptSessionBanner(record, agent.cwd, outputPolicy.format, outputPolicy.jsonStrict);
	const result = await sendSession({
		sessionId: record.acpxRecordId,
		prompt,
		mcpServers: config.mcpServers,
		permissionMode,
		nonInteractivePermissions: globalFlags.nonInteractivePermissions,
		authCredentials: config.auth,
		authPolicy: globalFlags.authPolicy,
		outputFormatter,
		errorEmissionPolicy: { queueErrorAlreadyEmitted: outputPolicy.queueErrorAlreadyEmitted },
		suppressSdkConsoleErrors: outputPolicy.suppressSdkConsoleErrors,
		timeoutMs: globalFlags.timeout,
		ttlMs: globalFlags.ttl,
		maxQueueDepth: config.queueMaxDepth,
		promptRetries: globalFlags.promptRetries,
		verbose: globalFlags.verbose,
		waitForCompletion: flags.wait !== false
	});
	if ("queued" in result) {
		printQueuedPromptByFormat(result, outputPolicy.format);
		return;
	}
	applyPermissionExitCode(result);
	if (globalFlags.verbose && result.loadError) process.stderr.write(`[acpx] loadSession failed, started fresh session: ${result.loadError}\n`);
}
async function handleExec(explicitAgentName, promptParts, flags, command, config) {
	if (config.disableExec) {
		if (resolveRequestedOutputPolicy(resolveGlobalFlags(command, config)).format === "json") process.stdout.write(`${JSON.stringify({
			jsonrpc: "2.0",
			error: {
				code: -32603,
				message: "exec subcommand is disabled by configuration (disableExec: true)",
				data: { acpxCode: "EXEC_DISABLED" }
			}
		})}\n`);
		else process.stderr.write("Error: exec subcommand is disabled by configuration (disableExec: true)\n");
		process.exitCode = 1;
		return;
	}
	const globalFlags = resolveGlobalFlags(command, config);
	const outputPolicy = resolveRequestedOutputPolicy(globalFlags);
	const permissionMode = resolvePermissionMode(globalFlags, config.defaultPermissions);
	const prompt = await readPrompt(promptParts, flags.file, globalFlags.cwd);
	const [{ createOutputFormatter }, { runOnce }] = await Promise.all([loadOutputModule(), loadSessionModule()]);
	const outputFormatter = createOutputFormatter(outputPolicy.format, { suppressReads: outputPolicy.suppressReads });
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	applyPermissionExitCode(await runOnce({
		agentCommand: agent.agentCommand,
		cwd: agent.cwd,
		prompt,
		mcpServers: config.mcpServers,
		permissionMode,
		nonInteractivePermissions: globalFlags.nonInteractivePermissions,
		authCredentials: config.auth,
		authPolicy: globalFlags.authPolicy,
		outputFormatter,
		suppressSdkConsoleErrors: outputPolicy.suppressSdkConsoleErrors,
		timeoutMs: globalFlags.timeout,
		verbose: globalFlags.verbose,
		promptRetries: globalFlags.promptRetries,
		sessionOptions: {
			model: globalFlags.model,
			allowedTools: globalFlags.allowedTools,
			maxTurns: globalFlags.maxTurns
		}
	}));
}
function printCancelResultByFormat(result, format) {
	if (emitJsonResult(format, {
		action: "cancel_result",
		acpxRecordId: result.sessionId || "unknown",
		cancelled: result.cancelled
	})) return;
	process.stdout.write(result.cancelled ? "cancel requested\n" : "nothing to cancel\n");
}
function printSetModeResultByFormat(modeId, result, format) {
	if (emitJsonResult(format, {
		action: "mode_set",
		modeId,
		resumed: result.resumed,
		acpxRecordId: result.record.acpxRecordId,
		acpxSessionId: result.record.acpSessionId,
		agentSessionId: result.record.agentSessionId
	})) return;
	process.stdout.write(format === "quiet" ? `${modeId}\n` : `mode set: ${modeId}\n`);
}
function printSetModelResultByFormat(modelId, result, format) {
	if (emitJsonResult(format, {
		action: "model_set",
		modelId,
		resumed: result.resumed,
		acpxRecordId: result.record.acpxRecordId,
		acpxSessionId: result.record.acpSessionId,
		agentSessionId: result.record.agentSessionId
	})) return;
	process.stdout.write(format === "quiet" ? `${modelId}\n` : `model set: ${modelId}\n`);
}
function printSetConfigOptionResultByFormat(configId, value, result, format) {
	if (emitJsonResult(format, {
		action: "config_set",
		configId,
		value,
		resumed: result.resumed,
		configOptions: result.response.configOptions,
		acpxRecordId: result.record.acpxRecordId,
		acpxSessionId: result.record.acpSessionId,
		agentSessionId: result.record.agentSessionId
	})) return;
	process.stdout.write(format === "quiet" ? `${value}\n` : `config set: ${configId}=${value} (${result.response.configOptions.length} options)\n`);
}
async function handleCancel(explicitAgentName, flags, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	const { cancelSessionPrompt } = await loadSessionModule();
	const walkBoundary = findGitRepositoryRoot(agent.cwd) ?? agent.cwd;
	const record = await findSessionByDirectoryWalk({
		agentCommand: agent.agentCommand,
		cwd: agent.cwd,
		name: resolveSessionNameFromFlags(flags, command),
		boundary: walkBoundary
	});
	if (!record) {
		printCancelResultByFormat({
			sessionId: "",
			cancelled: false
		}, globalFlags.format);
		return;
	}
	printCancelResultByFormat(await cancelSessionPrompt({
		sessionId: record.acpxRecordId,
		verbose: globalFlags.verbose
	}), globalFlags.format);
}
async function handleSetMode(explicitAgentName, modeId, flags, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	const { setSessionMode } = await loadSessionModule();
	const result = await setSessionMode({
		sessionId: (await findRoutedSessionOrThrow(agent.agentCommand, agent.agentName, agent.cwd, resolveSessionNameFromFlags(flags, command))).acpxRecordId,
		modeId,
		mcpServers: config.mcpServers,
		nonInteractivePermissions: globalFlags.nonInteractivePermissions,
		authCredentials: config.auth,
		authPolicy: globalFlags.authPolicy,
		timeoutMs: globalFlags.timeout,
		verbose: globalFlags.verbose
	});
	if (globalFlags.verbose && result.loadError) process.stderr.write(`[acpx] loadSession failed, started fresh session: ${result.loadError}\n`);
	printSetModeResultByFormat(modeId, result, globalFlags.format);
}
async function handleSetModel(explicitAgentName, modelId, flags, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	const { setSessionModel } = await loadSessionModule();
	const result = await setSessionModel({
		sessionId: (await findRoutedSessionOrThrow(agent.agentCommand, agent.agentName, agent.cwd, resolveSessionNameFromFlags(flags, command))).acpxRecordId,
		modelId,
		mcpServers: config.mcpServers,
		nonInteractivePermissions: globalFlags.nonInteractivePermissions,
		authCredentials: config.auth,
		authPolicy: globalFlags.authPolicy,
		timeoutMs: globalFlags.timeout,
		verbose: globalFlags.verbose
	});
	if (globalFlags.verbose && result.loadError) process.stderr.write(`[acpx] loadSession failed, started fresh session: ${result.loadError}\n`);
	printSetModelResultByFormat(modelId, result, globalFlags.format);
}
async function handleSetConfigOption(explicitAgentName, configId, value, flags, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	if (configId === "model") {
		await handleSetModel(explicitAgentName, value, flags, command, config);
		return;
	}
	const resolvedConfigId = resolveCompatibleConfigId(agent, configId);
	const { setSessionConfigOption } = await loadSessionModule();
	const result = await setSessionConfigOption({
		sessionId: (await findRoutedSessionOrThrow(agent.agentCommand, agent.agentName, agent.cwd, resolveSessionNameFromFlags(flags, command))).acpxRecordId,
		configId: resolvedConfigId,
		value,
		mcpServers: config.mcpServers,
		nonInteractivePermissions: globalFlags.nonInteractivePermissions,
		authCredentials: config.auth,
		authPolicy: globalFlags.authPolicy,
		timeoutMs: globalFlags.timeout,
		verbose: globalFlags.verbose
	});
	if (globalFlags.verbose && result.loadError) process.stderr.write(`[acpx] loadSession failed, started fresh session: ${result.loadError}\n`);
	printSetConfigOptionResultByFormat(configId, value, result, globalFlags.format);
}
async function handleSessionsList(explicitAgentName, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	const [{ listSessionsForAgent }, { printSessionsByFormat }] = await Promise.all([loadSessionModule(), loadOutputRenderModule()]);
	printSessionsByFormat(await listSessionsForAgent(agent.agentCommand), globalFlags.format);
}
async function handleSessionsClose(explicitAgentName, sessionName, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	const [{ closeSession }, { printClosedSessionByFormat }] = await Promise.all([loadSessionModule(), loadOutputRenderModule()]);
	const record = await findSession({
		agentCommand: agent.agentCommand,
		cwd: agent.cwd,
		name: sessionName
	});
	if (!record) throw new Error(sessionName ? `No named session "${sessionName}" for cwd ${agent.cwd} and agent ${agent.agentName}` : `No cwd session for ${agent.cwd} and agent ${agent.agentName}`);
	printClosedSessionByFormat(await closeSession(record.acpxRecordId), globalFlags.format);
}
async function handleSessionsNew(explicitAgentName, flags, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const permissionMode = resolvePermissionMode(globalFlags, config.defaultPermissions);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	const [{ createSession, closeSession }, { printCreatedSessionBanner, printNewSessionByFormat }] = await Promise.all([loadSessionModule(), loadOutputRenderModule()]);
	const replaced = await findSession({
		agentCommand: agent.agentCommand,
		cwd: agent.cwd,
		name: flags.name
	});
	if (replaced) {
		await closeSession(replaced.acpxRecordId);
		if (globalFlags.verbose) process.stderr.write(`[acpx] soft-closed prior session: ${replaced.acpxRecordId}\n`);
	}
	const created = await createSession({
		agentCommand: agent.agentCommand,
		cwd: agent.cwd,
		name: flags.name,
		resumeSessionId: flags.resumeSession,
		mcpServers: config.mcpServers,
		permissionMode,
		nonInteractivePermissions: globalFlags.nonInteractivePermissions,
		authCredentials: config.auth,
		authPolicy: globalFlags.authPolicy,
		timeoutMs: globalFlags.timeout,
		verbose: globalFlags.verbose,
		sessionOptions: {
			model: globalFlags.model,
			allowedTools: globalFlags.allowedTools,
			maxTurns: globalFlags.maxTurns
		}
	});
	printCreatedSessionBanner(created, agent.agentName, globalFlags.format, globalFlags.jsonStrict);
	if (globalFlags.verbose) {
		const scope = flags.name ? `named session "${flags.name}"` : "cwd session";
		process.stderr.write(`[acpx] created ${scope}: ${created.acpxRecordId}\n`);
	}
	printNewSessionByFormat(created, replaced, globalFlags.format);
}
async function handleSessionsEnsure(explicitAgentName, flags, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const permissionMode = resolvePermissionMode(globalFlags, config.defaultPermissions);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	const [{ ensureSession }, { printCreatedSessionBanner, printEnsuredSessionByFormat }] = await Promise.all([loadSessionModule(), loadOutputRenderModule()]);
	const result = await ensureSession({
		agentCommand: agent.agentCommand,
		cwd: agent.cwd,
		name: flags.name,
		resumeSessionId: flags.resumeSession,
		mcpServers: config.mcpServers,
		permissionMode,
		nonInteractivePermissions: globalFlags.nonInteractivePermissions,
		authCredentials: config.auth,
		authPolicy: globalFlags.authPolicy,
		timeoutMs: globalFlags.timeout,
		verbose: globalFlags.verbose,
		sessionOptions: {
			model: globalFlags.model,
			allowedTools: globalFlags.allowedTools,
			maxTurns: globalFlags.maxTurns
		}
	});
	if (result.created) printCreatedSessionBanner(result.record, agent.agentName, globalFlags.format, globalFlags.jsonStrict);
	printEnsuredSessionByFormat(result.record, result.created, globalFlags.format);
}
function userContentToText(content) {
	if ("Text" in content) return content.Text;
	if ("Mention" in content) return content.Mention.content;
	if ("Image" in content) return content.Image.source || "[image]";
	return "";
}
function agentContentToText(content) {
	if ("Text" in content) return content.Text;
	if ("Thinking" in content) return content.Thinking.text;
	if ("RedactedThinking" in content) return "[redacted_thinking]";
	if ("ToolUse" in content) return `[tool:${content.ToolUse.name}]`;
	return "";
}
function conversationHistoryEntries(record) {
	const entries = [];
	for (const message of record.messages) {
		if (message === "Resume") continue;
		if ("User" in message) {
			const text = message.User.content.map((entry) => userContentToText(entry)).join(" ").trim();
			if (!text) continue;
			entries.push({
				role: "user",
				timestamp: record.updated_at,
				textPreview: text
			});
			continue;
		}
		if ("Agent" in message) {
			const text = message.Agent.content.map((entry) => agentContentToText(entry)).join(" ").trim();
			if (!text) continue;
			entries.push({
				role: "assistant",
				timestamp: record.updated_at,
				textPreview: text
			});
		}
	}
	return entries;
}
function printSessionDetailsByFormat(record, format) {
	if (format === "json") {
		process.stdout.write(`${JSON.stringify(record)}\n`);
		return;
	}
	if (format === "quiet") {
		process.stdout.write(`${record.acpxRecordId}\n`);
		return;
	}
	process.stdout.write(`id: ${record.acpxRecordId}\n`);
	process.stdout.write(`sessionId: ${record.acpSessionId}\n`);
	process.stdout.write(`agentSessionId: ${record.agentSessionId ?? "-"}\n`);
	process.stdout.write(`agent: ${record.agentCommand}\n`);
	process.stdout.write(`cwd: ${record.cwd}\n`);
	process.stdout.write(`name: ${record.name ?? "-"}\n`);
	process.stdout.write(`created: ${record.createdAt}\n`);
	process.stdout.write(`lastActivity: ${record.lastUsedAt}\n`);
	process.stdout.write(`lastPrompt: ${record.lastPromptAt ?? "-"}\n`);
	process.stdout.write(`closed: ${record.closed ? "yes" : "no"}\n`);
	process.stdout.write(`closedAt: ${record.closedAt ?? "-"}\n`);
	process.stdout.write(`pid: ${record.pid ?? "-"}\n`);
	process.stdout.write(`agentStartedAt: ${record.agentStartedAt ?? "-"}\n`);
	process.stdout.write(`lastExitCode: ${record.lastAgentExitCode ?? "-"}\n`);
	process.stdout.write(`lastExitSignal: ${record.lastAgentExitSignal ?? "-"}\n`);
	process.stdout.write(`lastExitAt: ${record.lastAgentExitAt ?? "-"}\n`);
	process.stdout.write(`disconnectReason: ${record.lastAgentDisconnectReason ?? "-"}\n`);
	process.stdout.write(`historyEntries: ${conversationHistoryEntries(record).length}\n`);
}
function printSessionHistoryByFormat(record, limit, format) {
	const history = conversationHistoryEntries(record);
	const visible = limit === 0 ? history : history.slice(Math.max(0, history.length - limit));
	if (format === "json") {
		process.stdout.write(`${JSON.stringify({
			id: record.acpxRecordId,
			sessionId: record.acpSessionId,
			limit,
			count: visible.length,
			entries: visible
		})}\n`);
		return;
	}
	if (format === "quiet") {
		for (const entry of visible) process.stdout.write(`${entry.textPreview}\n`);
		return;
	}
	process.stdout.write(`session: ${record.acpxRecordId} (${visible.length}/${history.length} shown)\n`);
	if (visible.length === 0) {
		process.stdout.write("No history\n");
		return;
	}
	for (const entry of visible) process.stdout.write(`${entry.timestamp}\t${entry.role}\t${entry.textPreview}\n`);
}
async function handleSessionsShow(explicitAgentName, sessionName, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	const record = await findSession({
		agentCommand: agent.agentCommand,
		cwd: agent.cwd,
		name: sessionName,
		includeClosed: true
	});
	if (!record) throw new Error(sessionName ? `No named session "${sessionName}" for cwd ${agent.cwd} and agent ${agent.agentName}` : `No cwd session for ${agent.cwd} and agent ${agent.agentName}`);
	printSessionDetailsByFormat(record, globalFlags.format);
}
async function handleSessionsHistory(explicitAgentName, sessionName, flags, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	const record = await findSession({
		agentCommand: agent.agentCommand,
		cwd: agent.cwd,
		name: sessionName,
		includeClosed: true
	});
	if (!record) throw new Error(sessionName ? `No named session "${sessionName}" for cwd ${agent.cwd} and agent ${agent.agentName}` : `No cwd session for ${agent.cwd} and agent ${agent.agentName}`);
	printSessionHistoryByFormat(record, flags.limit, globalFlags.format);
}
//#endregion
//#region src/mcp-servers.ts
function asRecord$1(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function parseNonEmptyString(value, path) {
	if (typeof value !== "string" || value.trim().length === 0) throw new Error(`Invalid ${path}: expected non-empty string`);
	return value.trim();
}
function parseHeaders(value, path) {
	if (value == null) return [];
	if (!Array.isArray(value)) throw new Error(`Invalid ${path}: expected array`);
	const headers = [];
	for (const [index, rawHeader] of value.entries()) {
		const headerRecord = asRecord$1(rawHeader);
		if (!headerRecord) throw new Error(`Invalid ${path}[${index}]: expected object`);
		const name = parseNonEmptyString(headerRecord.name, `${path}[${index}].name`);
		const headerValue = parseNonEmptyString(headerRecord.value, `${path}[${index}].value`);
		headers.push({
			name,
			value: headerValue
		});
	}
	return headers;
}
function parseArgs(value, path) {
	if (value == null) return [];
	if (!Array.isArray(value)) throw new Error(`Invalid ${path}: expected array`);
	const args = [];
	for (const [index, rawArg] of value.entries()) {
		if (typeof rawArg !== "string") throw new Error(`Invalid ${path}[${index}]: expected string`);
		args.push(rawArg);
	}
	return args;
}
function parseEnv(value, path) {
	if (value == null) return [];
	if (!Array.isArray(value)) throw new Error(`Invalid ${path}: expected array`);
	const env = [];
	for (const [index, rawEntry] of value.entries()) {
		const entry = asRecord$1(rawEntry);
		if (!entry) throw new Error(`Invalid ${path}[${index}]: expected object`);
		const name = parseNonEmptyString(entry.name, `${path}[${index}].name`);
		const envValue = parseNonEmptyString(entry.value, `${path}[${index}].value`);
		env.push({
			name,
			value: envValue
		});
	}
	return env;
}
function parseMeta(value, path) {
	if (value === void 0) return;
	if (value === null) return null;
	if (!asRecord$1(value)) throw new Error(`Invalid ${path}: expected object or null`);
	return value;
}
function parseServer(rawServer, path) {
	const serverRecord = asRecord$1(rawServer);
	if (!serverRecord) throw new Error(`Invalid ${path}: expected object`);
	const name = parseNonEmptyString(serverRecord.name, `${path}.name`);
	const _meta = parseMeta(serverRecord._meta, `${path}._meta`);
	const rawType = serverRecord.type;
	let typeValue;
	if (rawType === void 0) typeValue = "stdio";
	else {
		const parsedType = parseNonEmptyString(rawType, `${path}.type`);
		if (parsedType !== "http" && parsedType !== "sse" && parsedType !== "stdio") throw new Error(`Invalid ${path}.type: expected http, sse, or stdio`);
		typeValue = parsedType;
	}
	if (typeValue === "http" || typeValue === "sse") {
		const url = parseNonEmptyString(serverRecord.url, `${path}.url`);
		const headers = parseHeaders(serverRecord.headers, `${path}.headers`);
		return {
			type: typeValue,
			name,
			url,
			headers,
			_meta
		};
	}
	if (typeValue === "stdio") return {
		name,
		command: parseNonEmptyString(serverRecord.command, `${path}.command`),
		args: parseArgs(serverRecord.args, `${path}.args`),
		env: parseEnv(serverRecord.env, `${path}.env`),
		_meta
	};
	throw new Error(`Invalid ${path}.type: expected http, sse, or stdio`);
}
function parseMcpServers(value, sourcePath, fieldName = "mcpServers") {
	const fieldPath = `${fieldName} in ${sourcePath}`;
	if (!Array.isArray(value)) throw new Error(`Invalid ${fieldPath}: expected array`);
	const parsed = [];
	for (const [index, rawServer] of value.entries()) parsed.push(parseServer(rawServer, `${fieldName}[${index}] in ${sourcePath}`));
	return parsed;
}
function parseOptionalMcpServers(value, sourcePath, fieldName = "mcpServers") {
	if (value === void 0) return;
	return parseMcpServers(value, sourcePath, fieldName);
}
//#endregion
//#region src/cli/config.ts
const DEFAULT_TIMEOUT_MS = void 0;
const DEFAULT_TTL_MS = 3e5;
const DEFAULT_PERMISSION_MODE = "approve-reads";
const DEFAULT_NON_INTERACTIVE_PERMISSION_POLICY = "deny";
const DEFAULT_AUTH_POLICY = "skip";
const DEFAULT_OUTPUT_FORMAT = "text";
const DEFAULT_QUEUE_MAX_DEPTH = 16;
const DEFAULT_DISABLE_EXEC = false;
const VALID_PERMISSION_MODES = new Set([
	"approve-all",
	"approve-reads",
	"deny-all"
]);
const VALID_NON_INTERACTIVE_PERMISSION_POLICIES = new Set(["deny", "fail"]);
const VALID_AUTH_POLICIES = new Set(["skip", "fail"]);
const VALID_OUTPUT_FORMATS = new Set([
	"text",
	"json",
	"quiet"
]);
function defaultGlobalConfigPath() {
	return path.join(os.homedir(), ".acpx", "config.json");
}
function projectConfigPath(cwd) {
	return path.join(path.resolve(cwd), ".acpxrc.json");
}
function isObject(value) {
	return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
function parseTtlMs(value, sourcePath) {
	if (value == null) return;
	if (typeof value !== "number" || !Number.isFinite(value) || value < 0) throw new Error(`Invalid config ttl in ${sourcePath}: expected non-negative seconds`);
	return Math.round(value * 1e3);
}
function parseTimeoutMs(value, sourcePath) {
	if (value == null) return;
	if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) throw new Error(`Invalid config timeout in ${sourcePath}: expected positive seconds or null`);
	return Math.round(value * 1e3);
}
function parseQueueMaxDepth(value, sourcePath) {
	if (value == null) return;
	if (!Number.isInteger(value) || value <= 0) throw new Error(`Invalid config queueMaxDepth in ${sourcePath}: expected positive integer`);
	return value;
}
function parsePermissionMode(value, sourcePath) {
	if (value == null) return;
	if (typeof value !== "string" || !VALID_PERMISSION_MODES.has(value)) throw new Error(`Invalid config defaultPermissions in ${sourcePath}: expected approve-all, approve-reads, or deny-all`);
	return value;
}
function parseNonInteractivePermissionPolicy(value, sourcePath) {
	if (value == null) return;
	if (typeof value !== "string" || !VALID_NON_INTERACTIVE_PERMISSION_POLICIES.has(value)) throw new Error(`Invalid config nonInteractivePermissions in ${sourcePath}: expected deny or fail`);
	return value;
}
function parseAuthPolicy(value, sourcePath) {
	if (value == null) return;
	if (typeof value !== "string" || !VALID_AUTH_POLICIES.has(value)) throw new Error(`Invalid config authPolicy in ${sourcePath}: expected skip or fail`);
	return value;
}
function parseOutputFormat(value, sourcePath) {
	if (value == null) return;
	if (typeof value !== "string" || !VALID_OUTPUT_FORMATS.has(value)) throw new Error(`Invalid config format in ${sourcePath}: expected text, json, or quiet`);
	return value;
}
function parseDefaultAgent(value, sourcePath) {
	if (value == null) return;
	if (typeof value !== "string" || value.trim().length === 0) throw new Error(`Invalid config defaultAgent in ${sourcePath}: expected non-empty string`);
	return normalizeAgentName(value);
}
function parseAgents(value, sourcePath) {
	if (value == null) return;
	if (!isObject(value)) throw new Error(`Invalid config agents in ${sourcePath}: expected object`);
	const parsed = {};
	for (const [name, raw] of Object.entries(value)) {
		if (!isObject(raw)) throw new Error(`Invalid config agents.${name} in ${sourcePath}: expected object with command`);
		const command = raw.command;
		if (typeof command !== "string" || command.trim().length === 0) throw new Error(`Invalid config agents.${name}.command in ${sourcePath}: expected non-empty string`);
		parsed[normalizeAgentName(name)] = command.trim();
	}
	return parsed;
}
function parseAuth(value, sourcePath) {
	if (value == null) return;
	if (!isObject(value)) throw new Error(`Invalid config auth in ${sourcePath}: expected object`);
	const parsed = {};
	for (const [methodId, rawCredential] of Object.entries(value)) {
		if (typeof rawCredential !== "string" || rawCredential.trim().length === 0) throw new Error(`Invalid config auth.${methodId} in ${sourcePath}: expected non-empty string`);
		parsed[methodId] = rawCredential;
	}
	return parsed;
}
function parseDisableExec(value, sourcePath) {
	if (value == null) return;
	if (typeof value !== "boolean") throw new Error(`Invalid config disableExec in ${sourcePath}: expected boolean`);
	return value;
}
async function readConfigFile(filePath) {
	try {
		const payload = await fs$1.readFile(filePath, "utf8");
		let parsed;
		try {
			parsed = JSON.parse(payload);
		} catch (error) {
			const reason = error instanceof Error ? error.message : String(error);
			throw new Error(`Invalid JSON in ${filePath}: ${reason}`, { cause: error });
		}
		if (!isObject(parsed)) throw new Error(`Invalid config in ${filePath}: expected top-level JSON object`);
		return {
			config: parsed,
			exists: true
		};
	} catch (error) {
		if (error.code === "ENOENT") return { exists: false };
		throw error;
	}
}
function mergeAgents(globalAgents, projectAgents) {
	return {
		...globalAgents,
		...projectAgents
	};
}
function mergeAuth(globalAuth, projectAuth) {
	return {
		...globalAuth,
		...projectAuth
	};
}
async function loadResolvedConfig(cwd) {
	const globalPath = defaultGlobalConfigPath();
	const projectPath = projectConfigPath(cwd);
	const [globalResult, projectResult] = await Promise.all([readConfigFile(globalPath), readConfigFile(projectPath)]);
	const globalConfig = globalResult.config;
	const projectConfig = projectResult.config;
	const defaultAgent = parseDefaultAgent(projectConfig?.defaultAgent, projectPath) ?? parseDefaultAgent(globalConfig?.defaultAgent, globalPath) ?? "codex";
	const defaultPermissions = parsePermissionMode(projectConfig?.defaultPermissions, projectPath) ?? parsePermissionMode(globalConfig?.defaultPermissions, globalPath) ?? DEFAULT_PERMISSION_MODE;
	const nonInteractivePermissions = parseNonInteractivePermissionPolicy(projectConfig?.nonInteractivePermissions, projectPath) ?? parseNonInteractivePermissionPolicy(globalConfig?.nonInteractivePermissions, globalPath) ?? DEFAULT_NON_INTERACTIVE_PERMISSION_POLICY;
	const authPolicy = parseAuthPolicy(projectConfig?.authPolicy, projectPath) ?? parseAuthPolicy(globalConfig?.authPolicy, globalPath) ?? DEFAULT_AUTH_POLICY;
	const ttlMs = parseTtlMs(projectConfig?.ttl, projectPath) ?? parseTtlMs(globalConfig?.ttl, globalPath) ?? DEFAULT_TTL_MS;
	const timeoutConfiguredInProject = projectConfig != null && Object.prototype.hasOwnProperty.call(projectConfig, "timeout");
	const timeoutConfiguredInGlobal = globalConfig != null && Object.prototype.hasOwnProperty.call(globalConfig, "timeout");
	let timeoutMs = DEFAULT_TIMEOUT_MS;
	if (timeoutConfiguredInProject) timeoutMs = parseTimeoutMs(projectConfig?.timeout, projectPath);
	else if (timeoutConfiguredInGlobal) timeoutMs = parseTimeoutMs(globalConfig?.timeout, globalPath);
	const format = parseOutputFormat(projectConfig?.format, projectPath) ?? parseOutputFormat(globalConfig?.format, globalPath) ?? DEFAULT_OUTPUT_FORMAT;
	const queueMaxDepth = parseQueueMaxDepth(projectConfig?.queueMaxDepth, projectPath) ?? parseQueueMaxDepth(globalConfig?.queueMaxDepth, globalPath) ?? DEFAULT_QUEUE_MAX_DEPTH;
	const agents = mergeAgents(parseAgents(globalConfig?.agents, globalPath), parseAgents(projectConfig?.agents, projectPath));
	const auth = mergeAuth(parseAuth(globalConfig?.auth, globalPath), parseAuth(projectConfig?.auth, projectPath));
	const mcpServersConfiguredInProject = projectConfig != null && Object.prototype.hasOwnProperty.call(projectConfig, "mcpServers");
	const mcpServersConfiguredInGlobal = globalConfig != null && Object.prototype.hasOwnProperty.call(globalConfig, "mcpServers");
	let mcpServers = [];
	if (mcpServersConfiguredInProject) mcpServers = parseMcpServers(projectConfig?.mcpServers, projectPath);
	else if (mcpServersConfiguredInGlobal) mcpServers = parseMcpServers(globalConfig?.mcpServers, globalPath);
	const disableExec = parseDisableExec(projectConfig?.disableExec, projectPath) ?? parseDisableExec(globalConfig?.disableExec, globalPath) ?? DEFAULT_DISABLE_EXEC;
	return {
		defaultAgent,
		defaultPermissions,
		nonInteractivePermissions,
		authPolicy,
		ttlMs,
		timeoutMs,
		queueMaxDepth,
		format,
		agents,
		auth,
		disableExec,
		mcpServers,
		globalPath,
		projectPath,
		hasGlobalConfig: globalResult.exists,
		hasProjectConfig: projectResult.exists
	};
}
function toConfigDisplay(config) {
	const agents = {};
	for (const [name, command] of Object.entries(config.agents)) agents[name] = { command };
	return {
		defaultAgent: config.defaultAgent,
		defaultPermissions: config.defaultPermissions,
		nonInteractivePermissions: config.nonInteractivePermissions,
		authPolicy: config.authPolicy,
		ttl: Math.round(config.ttlMs / 1e3),
		timeout: config.timeoutMs == null ? null : config.timeoutMs / 1e3,
		queueMaxDepth: config.queueMaxDepth,
		format: config.format,
		agents,
		authMethods: Object.keys(config.auth).toSorted(),
		disableExec: config.disableExec
	};
}
async function initGlobalConfigFile() {
	const configPath = defaultGlobalConfigPath();
	await fs$1.mkdir(path.dirname(configPath), { recursive: true });
	try {
		await fs$1.access(configPath);
		return {
			path: configPath,
			created: false
		};
	} catch {}
	const payload = {
		defaultAgent: DEFAULT_AGENT_NAME,
		defaultPermissions: "approve-all",
		nonInteractivePermissions: "deny",
		authPolicy: "skip",
		ttl: 300,
		timeout: null,
		queueMaxDepth: DEFAULT_QUEUE_MAX_DEPTH,
		format: "text",
		agents: {},
		auth: {}
	};
	await fs$1.writeFile(configPath, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
	return {
		path: configPath,
		created: true
	};
}
//#endregion
//#region src/cli/config-command.ts
async function handleConfigShow(command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const payload = {
		...toConfigDisplay(config),
		paths: {
			global: config.globalPath,
			project: config.projectPath
		},
		loaded: {
			global: config.hasGlobalConfig,
			project: config.hasProjectConfig
		}
	};
	if (globalFlags.format === "json") {
		process.stdout.write(`${JSON.stringify(payload)}\n`);
		return;
	}
	process.stdout.write(`${JSON.stringify(payload, null, 2)}\n`);
}
async function handleConfigInit(command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const result = await initGlobalConfigFile();
	if (globalFlags.format === "json") {
		process.stdout.write(`${JSON.stringify({
			path: result.path,
			created: result.created
		})}\n`);
		return;
	}
	if (globalFlags.format === "quiet") {
		process.stdout.write(`${result.path}\n`);
		return;
	}
	if (result.created) {
		process.stdout.write(`Created ${result.path}\n`);
		return;
	}
	process.stdout.write(`Config already exists: ${result.path}\n`);
}
function registerConfigCommand(program, config) {
	const configCommand = program.command("config").description("Inspect and initialize acpx configuration");
	configCommand.command("show").description("Show resolved config").action(async function() {
		await handleConfigShow(this, config);
	});
	configCommand.command("init").description("Create global config template").action(async function() {
		await handleConfigInit(this, config);
	});
	configCommand.action(async function() {
		await handleConfigShow(this, config);
	});
}
//#endregion
//#region src/cli/status-command.ts
function formatUptime(startedAt) {
	if (!startedAt) return;
	const startedMs = Date.parse(startedAt);
	if (!Number.isFinite(startedMs)) return;
	const elapsedMs = Math.max(0, Date.now() - startedMs);
	const seconds = Math.floor(elapsedMs / 1e3);
	const hours = Math.floor(seconds / 3600);
	const minutes = Math.floor(seconds % 3600 / 60);
	const remSeconds = seconds % 60;
	return `${hours.toString().padStart(2, "0")}:${minutes.toString().padStart(2, "0")}:${remSeconds.toString().padStart(2, "0")}`;
}
async function handleStatus(explicitAgentName, flags, command, config) {
	const globalFlags = resolveGlobalFlags(command, config);
	const agent = resolveAgentInvocation(explicitAgentName, globalFlags, config);
	const record = await findSession({
		agentCommand: agent.agentCommand,
		cwd: agent.cwd,
		name: resolveSessionNameFromFlags(flags, command)
	});
	if (!record) {
		if (emitJsonResult(globalFlags.format, {
			action: "status_snapshot",
			status: "no-session",
			summary: "no active session"
		})) return;
		if (globalFlags.format === "quiet") {
			process.stdout.write("no-session\n");
			return;
		}
		process.stdout.write("session: -\n");
		process.stdout.write(`agent: ${agent.agentCommand}\n`);
		process.stdout.write("pid: -\n");
		process.stdout.write("status: no-session\n");
		process.stdout.write("model: -\n");
		process.stdout.write("mode: -\n");
		process.stdout.write("uptime: -\n");
		process.stdout.write("lastPromptTime: -\n");
		return;
	}
	const health = await probeQueueOwnerHealth(record.acpxRecordId);
	const running = health.healthy;
	const payload = {
		sessionId: record.acpxRecordId,
		agentCommand: record.agentCommand,
		pid: health.pid ?? record.pid ?? null,
		status: running ? "running" : "dead",
		model: record.acpx?.current_model_id ?? null,
		mode: record.acpx?.current_mode_id ?? null,
		availableModels: record.acpx?.available_models ?? null,
		uptime: running ? formatUptime(record.agentStartedAt) ?? null : null,
		lastPromptTime: record.lastPromptAt ?? null,
		exitCode: running ? null : record.lastAgentExitCode ?? null,
		signal: running ? null : record.lastAgentExitSignal ?? null,
		...agentSessionIdPayload(record.agentSessionId)
	};
	if (emitJsonResult(globalFlags.format, {
		action: "status_snapshot",
		status: running ? "alive" : "dead",
		pid: payload.pid ?? void 0,
		summary: running ? "queue owner healthy" : "queue owner unavailable",
		model: payload.model ?? void 0,
		mode: payload.mode ?? void 0,
		availableModels: payload.availableModels ?? void 0,
		uptime: payload.uptime ?? void 0,
		lastPromptTime: payload.lastPromptTime ?? void 0,
		exitCode: payload.exitCode ?? void 0,
		signal: payload.signal ?? void 0,
		acpxRecordId: record.acpxRecordId,
		acpxSessionId: record.acpSessionId,
		agentSessionId: record.agentSessionId
	})) return;
	if (globalFlags.format === "quiet") {
		process.stdout.write(`${payload.status}\n`);
		return;
	}
	process.stdout.write(`session: ${payload.sessionId}\n`);
	if ("agentSessionId" in payload) process.stdout.write(`agentSessionId: ${payload.agentSessionId}\n`);
	process.stdout.write(`agent: ${payload.agentCommand}\n`);
	process.stdout.write(`pid: ${payload.pid ?? "-"}\n`);
	process.stdout.write(`status: ${payload.status}\n`);
	process.stdout.write(`model: ${payload.model ?? "-"}\n`);
	process.stdout.write(`mode: ${payload.mode ?? "-"}\n`);
	process.stdout.write(`uptime: ${payload.uptime ?? "-"}\n`);
	process.stdout.write(`lastPromptTime: ${payload.lastPromptTime ?? "-"}\n`);
	if (payload.status === "dead") {
		process.stdout.write(`exitCode: ${payload.exitCode ?? "-"}\n`);
		process.stdout.write(`signal: ${payload.signal ?? "-"}\n`);
	}
}
function registerStatusCommand(parent, explicitAgentName, config, description) {
	const statusCommand = parent.command("status").description(description);
	addSessionNameOption(statusCommand);
	statusCommand.action(async function(flags) {
		await handleStatus(explicitAgentName, flags, this, config);
	});
}
//#endregion
//#region src/cli/command-registration.ts
function registerSessionsCommand(parent, explicitAgentName, config) {
	const sessionsCommand = parent.command("sessions").description("List, ensure, create, or close sessions for this agent");
	sessionsCommand.action(async function() {
		await handleSessionsList(explicitAgentName, this, config);
	});
	sessionsCommand.command("list").description("List sessions").action(async function() {
		await handleSessionsList(explicitAgentName, this, config);
	});
	sessionsCommand.command("new").description("Create a fresh session for current cwd").option("--name <name>", "Session name", parseSessionName).option("--resume-session <id>", "Resume existing ACP session id", (value) => parseNonEmptyValue("Resume session id", value)).action(async function(flags) {
		await handleSessionsNew(explicitAgentName, flags, this, config);
	});
	sessionsCommand.command("ensure").description("Ensure a session exists for current cwd or ancestor").option("--name <name>", "Session name", parseSessionName).option("--resume-session <id>", "Resume existing ACP session id", (value) => parseNonEmptyValue("Resume session id", value)).action(async function(flags) {
		await handleSessionsEnsure(explicitAgentName, flags, this, config);
	});
	sessionsCommand.command("close").description("Close session for current cwd").argument("[name]", "Session name", parseSessionName).action(async function(name) {
		await handleSessionsClose(explicitAgentName, name, this, config);
	});
	sessionsCommand.command("show").description("Show session metadata for current cwd").argument("[name]", "Session name", parseSessionName).action(async function(name) {
		await handleSessionsShow(explicitAgentName, name, this, config);
	});
	sessionsCommand.command("history").description("Show recent session history entries").argument("[name]", "Session name", parseSessionName).option("--limit <count>", `Maximum number of entries to show (default: 20)`, parseHistoryLimit, 20).action(async function(name, flags) {
		await handleSessionsHistory(explicitAgentName, name, flags, this, config);
	});
	sessionsCommand.command("read").description("Read full session history").argument("[name]", "Session name", parseSessionName).option("--tail <count>", "Show only the last N entries instead of all history", parseHistoryLimit).action(async function(name, flags) {
		await handleSessionsHistory(explicitAgentName, name, { limit: flags.tail ?? 0 }, this, config);
	});
}
function registerSharedAgentSubcommands(parent, explicitAgentName, config, descriptions) {
	const promptCommand = parent.command("prompt").description(descriptions.prompt).argument("[prompt...]", "Prompt text").showHelpAfterError();
	addSessionOption(promptCommand);
	addPromptInputOption(promptCommand);
	promptCommand.action(async function(promptParts, flags) {
		await handlePrompt(explicitAgentName, promptParts, flags, this, config);
	});
	const execCommand = parent.command("exec").description(descriptions.exec).argument("[prompt...]", "Prompt text").showHelpAfterError();
	addPromptInputOption(execCommand);
	execCommand.action(async function(promptParts, flags) {
		await handleExec(explicitAgentName, promptParts, flags, this, config);
	});
	const cancelCommand = parent.command("cancel").description(descriptions.cancel);
	addSessionNameOption(cancelCommand);
	cancelCommand.action(async function(flags) {
		await handleCancel(explicitAgentName, flags, this, config);
	});
	const setModeCommand = parent.command("set-mode").description(descriptions.setMode).argument("<mode>", "Mode id", (value) => parseNonEmptyValue("Mode", value));
	addSessionNameOption(setModeCommand);
	setModeCommand.action(async function(modeId, flags) {
		await handleSetMode(explicitAgentName, modeId, flags, this, config);
	});
	const setConfigCommand = parent.command("set").description(descriptions.setConfig).argument("<key>", "Config option id", (value) => parseNonEmptyValue("Config option key", value)).argument("<value>", "Config option value", (value) => parseNonEmptyValue("Config option value", value));
	addSessionNameOption(setConfigCommand);
	setConfigCommand.action(async function(key, value, flags) {
		await handleSetConfigOption(explicitAgentName, key, value, flags, this, config);
	});
	registerStatusCommand(parent, explicitAgentName, config, descriptions.status);
}
function registerAgentCommand(program, agentName, config) {
	const agentCommand = program.command(agentName).description(`Use ${agentName} agent`).argument("[prompt...]", "Prompt text").enablePositionalOptions().passThroughOptions().showHelpAfterError();
	addSessionOption(agentCommand);
	addPromptInputOption(agentCommand);
	agentCommand.action(async function(promptParts, flags) {
		await handlePrompt(agentName, promptParts, flags, this, config);
	});
	registerSharedAgentSubcommands(agentCommand, agentName, config, {
		prompt: "Prompt using persistent session",
		exec: "One-shot prompt without saved session",
		cancel: "Cooperatively cancel current in-flight prompt",
		setMode: "Set session mode",
		setConfig: "Set session config option",
		status: "Show local status of current session agent process"
	});
	registerSessionsCommand(agentCommand, agentName, config);
}
function registerFlowCommand(program, config) {
	program.command("flow").description("Run multi-step ACP workflows from flow files").command("run").description("Run a flow file").argument("<file>", "Flow module path").option("--input-json <json>", "Flow input as JSON").option("--input-file <path>", "Read flow input JSON from file").option("--default-agent <name>", "Default agent profile for ACP nodes without profile", (value) => parseNonEmptyValue("Default agent", value)).action(async function(file, flags) {
		const { handleFlowRun } = await import("./cli-ChWsO-bb.js");
		await handleFlowRun(file, flags, this, config);
	});
}
function registerDefaultCommands(program, config) {
	registerSharedAgentSubcommands(program, void 0, config, {
		prompt: `Prompt using ${config.defaultAgent} by default`,
		exec: `One-shot prompt using ${config.defaultAgent} by default`,
		cancel: `Cancel active prompt for ${config.defaultAgent} by default`,
		setMode: `Set session mode for ${config.defaultAgent} by default`,
		setConfig: `Set session config option for ${config.defaultAgent} by default`,
		status: `Show local status for ${config.defaultAgent} by default`
	});
	registerSessionsCommand(program, void 0, config);
	registerConfigCommand(program, config);
	registerFlowCommand(program, config);
}
//#endregion
//#region src/cli/queue/owner-env.ts
function asRecord(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return;
	return value;
}
function parseQueueOwnerPayload(raw) {
	const record = asRecord(JSON.parse(raw));
	if (!record) throw new Error("queue owner payload must be an object");
	if (typeof record.sessionId !== "string" || record.sessionId.trim().length === 0) throw new Error("queue owner payload missing sessionId");
	if (record.permissionMode !== "approve-all" && record.permissionMode !== "approve-reads" && record.permissionMode !== "deny-all") throw new Error("queue owner payload has invalid permissionMode");
	const options = {
		sessionId: record.sessionId,
		permissionMode: record.permissionMode
	};
	const parsedMcpServers = parseOptionalMcpServers(record.mcpServers, "queue owner payload");
	if (parsedMcpServers) options.mcpServers = parsedMcpServers;
	if (typeof record.nonInteractivePermissions === "string") options.nonInteractivePermissions = record.nonInteractivePermissions === "deny" || record.nonInteractivePermissions === "fail" ? record.nonInteractivePermissions : void 0;
	if (record.authCredentials && typeof record.authCredentials === "object") {
		const entries = Object.entries(record.authCredentials).filter(([, value]) => typeof value === "string");
		options.authCredentials = Object.fromEntries(entries);
	}
	if (record.authPolicy === "skip" || record.authPolicy === "fail") options.authPolicy = record.authPolicy;
	if (typeof record.suppressSdkConsoleErrors === "boolean") options.suppressSdkConsoleErrors = record.suppressSdkConsoleErrors;
	if (typeof record.verbose === "boolean") options.verbose = record.verbose;
	if (typeof record.ttlMs === "number" && Number.isFinite(record.ttlMs)) options.ttlMs = record.ttlMs;
	if (typeof record.maxQueueDepth === "number" && Number.isFinite(record.maxQueueDepth)) options.maxQueueDepth = Math.max(1, Math.round(record.maxQueueDepth));
	if (typeof record.promptRetries === "number" && Number.isFinite(record.promptRetries)) options.promptRetries = Math.max(0, Math.round(record.promptRetries));
	return options;
}
async function runQueueOwnerFromEnv(env) {
	const payload = env.ACPX_QUEUE_OWNER_PAYLOAD;
	if (!payload) throw new Error("missing ACPX_QUEUE_OWNER_PAYLOAD");
	await runSessionQueueOwner(parseQueueOwnerPayload(payload));
}
//#endregion
//#region src/version.ts
const UNKNOWN_VERSION = "0.0.0-unknown";
const MODULE_DIR = path.dirname(fileURLToPath(import.meta.url));
let cachedVersion = null;
function parseVersion(value) {
	if (typeof value !== "string") return null;
	const trimmed = value.trim();
	return trimmed.length > 0 ? trimmed : null;
}
function readPackageVersion(packageJsonPath) {
	try {
		return parseVersion(JSON.parse(readFileSync(packageJsonPath, "utf8")).version);
	} catch {
		return null;
	}
}
function resolveVersionFromAncestors(startDir) {
	let current = startDir;
	while (true) {
		const packageVersion = readPackageVersion(path.join(current, "package.json"));
		if (packageVersion) return packageVersion;
		const parent = path.dirname(current);
		if (parent === current) return null;
		current = parent;
	}
}
function resolveAcpxVersion(params) {
	const env = params?.env ?? process.env;
	const envPackageName = parseVersion(env.npm_package_name);
	const envVersion = parseVersion(env.npm_package_version);
	if (envPackageName === "acpx" && envVersion) return envVersion;
	if (params?.packageJsonPath) return readPackageVersion(params.packageJsonPath) ?? UNKNOWN_VERSION;
	return resolveVersionFromAncestors(MODULE_DIR) ?? UNKNOWN_VERSION;
}
function getAcpxVersion() {
	if (cachedVersion) return cachedVersion;
	cachedVersion = resolveAcpxVersion();
	return cachedVersion;
}
//#endregion
//#region src/cli-core.ts
const TOP_LEVEL_VERBS = new Set([
	"prompt",
	"exec",
	"cancel",
	"flow",
	"set-mode",
	"set",
	"sessions",
	"status",
	"config",
	"help"
]);
let skillflagModulePromise;
function loadSkillflagModule() {
	skillflagModulePromise ??= import("skillflag");
	return skillflagModulePromise;
}
function shouldMaybeHandleSkillflag(argv) {
	return argv.some((token) => token === "--skill" || token.startsWith("--skill="));
}
function detectAgentToken(argv) {
	let hasAgentOverride = false;
	for (let index = 0; index < argv.length; index += 1) {
		const token = argv[index];
		if (token === "--") break;
		if (!token.startsWith("-") || token === "-") return {
			token,
			hasAgentOverride
		};
		if (token === "--agent") {
			hasAgentOverride = true;
			index += 1;
			continue;
		}
		if (token.startsWith("--agent=")) {
			hasAgentOverride = true;
			continue;
		}
		if (token === "--cwd" || token === "--auth-policy" || token === "--non-interactive-permissions" || token === "--format" || token === "--model" || token === "--allowed-tools" || token === "--max-turns" || token === "--timeout" || token === "--ttl" || token === "--file") {
			index += 1;
			continue;
		}
		if (token.startsWith("--cwd=") || token.startsWith("--auth-policy=") || token.startsWith("--non-interactive-permissions=") || token.startsWith("--format=") || token.startsWith("--model=") || token.startsWith("--allowed-tools=") || token.startsWith("--max-turns=") || token.startsWith("--json-strict=") || token.startsWith("--timeout=") || token.startsWith("--ttl=") || token.startsWith("--file=")) continue;
		if (token === "--approve-all" || token === "--approve-reads" || token === "--deny-all" || token === "--json-strict" || token === "--verbose" || token === "--suppress-reads") continue;
		return { hasAgentOverride };
	}
	return { hasAgentOverride };
}
function detectInitialCwd(argv) {
	for (let index = 0; index < argv.length; index += 1) {
		const token = argv[index];
		if (token === "--cwd") {
			const next = argv[index + 1];
			if (next && next !== "--") return path.resolve(next);
			break;
		}
		if (token.startsWith("--cwd=")) {
			const value = token.slice(6).trim();
			if (value.length > 0) return path.resolve(value);
			break;
		}
		if (token === "--") break;
	}
	return process.cwd();
}
function detectRequestedOutputFormat(argv, fallback) {
	let detectedFormat = fallback;
	for (let index = 0; index < argv.length; index += 1) {
		const token = argv[index];
		if (token === "--") break;
		if (token === "--json-strict" || token.startsWith("--json-strict=")) return "json";
		if (token === "--format") {
			const raw = argv[index + 1];
			if (raw && OUTPUT_FORMATS.includes(raw)) detectedFormat = raw;
			continue;
		}
		if (token.startsWith("--format=")) {
			const raw = token.slice(9).trim();
			if (OUTPUT_FORMATS.includes(raw)) detectedFormat = raw;
		}
	}
	return detectedFormat;
}
function detectJsonStrict(argv) {
	for (let index = 0; index < argv.length; index += 1) {
		const token = argv[index];
		if (token === "--") break;
		if (token === "--json-strict" || token.startsWith("--json-strict=")) return true;
	}
	return false;
}
async function emitJsonErrorEvent(error) {
	const formatter = createOutputFormatter("json", {
		jsonContext: { sessionId: "unknown" },
		suppressReads: false
	});
	formatter.onError(error);
	formatter.flush();
}
function isOutputAlreadyEmitted(error) {
	if (!error || typeof error !== "object") return false;
	return error.outputAlreadyEmitted === true;
}
async function emitRequestedError(error, normalized, outputPolicy) {
	if (isOutputAlreadyEmitted(error)) return;
	if (outputPolicy.format === "json") {
		await emitJsonErrorEvent(normalized);
		return;
	}
	if (!outputPolicy.suppressNonJsonStderr) process.stderr.write(`${normalized.message}\n`);
}
async function runWithOutputPolicy(_outputPolicy, run) {
	return await run();
}
async function main(argv = process.argv) {
	installPerfMetricsCapture({
		argv: argv.slice(2),
		role: argv[2] === "__queue-owner" ? "queue_owner" : "cli"
	});
	if (argv.includes("--version") || argv.includes("-V")) {
		process.stdout.write(`${getAcpxVersion()}\n`);
		return;
	}
	if (argv[2] === "__queue-owner") try {
		await runQueueOwnerFromEnv(process.env);
		return;
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		process.stderr.write(`[acpx] queue owner failed: ${message}\n`);
		process.exit(EXIT_CODES.ERROR);
	}
	if (shouldMaybeHandleSkillflag(argv)) {
		const { findSkillsRoot, maybeHandleSkillflag } = await loadSkillflagModule();
		await maybeHandleSkillflag(argv, {
			skillsRoot: findSkillsRoot(import.meta.url),
			includeBundledSkill: false
		});
	}
	const rawArgs = argv.slice(2);
	const config = await loadResolvedConfig(detectInitialCwd(rawArgs));
	const requestedJsonStrict = detectJsonStrict(rawArgs);
	const requestedOutputPolicy = {
		...resolveOutputPolicy(detectRequestedOutputFormat(rawArgs, config.format), requestedJsonStrict),
		suppressReads: rawArgs.some((token) => token === "--suppress-reads")
	};
	const program = new Command();
	program.name("acpx").description("Headless CLI client for the Agent Client Protocol").version(getAcpxVersion()).enablePositionalOptions().showHelpAfterError();
	if (requestedJsonStrict) program.configureOutput({
		writeOut: () => {},
		writeErr: () => {}
	});
	addGlobalFlags(program);
	configurePublicCli({
		program,
		argv: rawArgs,
		config,
		requestedJsonStrict,
		topLevelVerbs: TOP_LEVEL_VERBS,
		listBuiltInAgents,
		detectAgentToken,
		registerAgentCommand,
		registerDefaultCommands,
		handlePromptAction: async (command, promptParts) => {
			await handlePrompt(void 0, promptParts, {}, command, config);
		}
	});
	program.exitOverride((error) => {
		throw error;
	});
	try {
		await runWithOutputPolicy(requestedOutputPolicy, async () => {
			try {
				await program.parseAsync(argv);
			} catch (error) {
				if (error instanceof CommanderError) {
					if (error.code === "commander.helpDisplayed" || error.code === "commander.version") process.exit(EXIT_CODES.SUCCESS);
					const normalized = normalizeOutputError(error, {
						defaultCode: "USAGE",
						origin: "cli"
					});
					await emitRequestedError(error, normalized, requestedOutputPolicy);
					process.exit(exitCodeForOutputErrorCode(normalized.code));
				}
				if (error instanceof InterruptedError) process.exit(EXIT_CODES.INTERRUPTED);
				const normalized = normalizeOutputError(error, { origin: "cli" });
				await emitRequestedError(error, normalized, requestedOutputPolicy);
				process.exit(exitCodeForOutputErrorCode(normalized.code));
			}
		});
	} finally {
		flushPerfMetricsCapture();
	}
}
//#endregion
//#region src/cli.ts
const queueOwnerArgOverride = buildQueueOwnerArgOverride(fileURLToPath(import.meta.url));
if (queueOwnerArgOverride) process.env.ACPX_QUEUE_OWNER_ARGS ??= queueOwnerArgOverride;
function isCliEntrypoint(argv) {
	const entry = argv[1];
	if (!entry) return false;
	try {
		const resolved = pathToFileURL(realpathSync(entry)).href;
		return import.meta.url === resolved;
	} catch {
		return false;
	}
}
if (isCliEntrypoint(process.argv)) main(process.argv);
//#endregion
export { formatPromptSessionBannerLine, parseAllowedTools, parseMaxTurns, parseTtlSeconds };

//# sourceMappingURL=cli.js.map