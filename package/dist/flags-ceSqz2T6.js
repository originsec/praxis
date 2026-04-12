import { E as NON_INTERACTIVE_PERMISSION_POLICIES, k as OUTPUT_FORMATS, w as AUTH_POLICIES } from "./perf-metrics-D0um6IR6.js";
import { J as resolveAgentCommand } from "./prompt-turn-CXMtXBl-.js";
import "./session-BtwAKtJ3.js";
import path from "node:path";
import { InvalidArgumentError } from "commander";
//#region src/cli/flags.ts
function hasExplicitPermissionModeFlag(flags) {
	return flags.approveAll === true || flags.approveReads === true || flags.denyAll === true;
}
function parseOutputFormat(value) {
	if (!OUTPUT_FORMATS.includes(value)) throw new InvalidArgumentError(`Invalid format "${value}". Expected one of: ${OUTPUT_FORMATS.join(", ")}`);
	return value;
}
function parseAuthPolicy(value) {
	if (!AUTH_POLICIES.includes(value)) throw new InvalidArgumentError(`Invalid auth policy "${value}". Expected one of: ${AUTH_POLICIES.join(", ")}`);
	return value;
}
function parseNonInteractivePermissionPolicy(value) {
	if (!NON_INTERACTIVE_PERMISSION_POLICIES.includes(value)) throw new InvalidArgumentError(`Invalid non-interactive permission policy "${value}". Expected one of: ${NON_INTERACTIVE_PERMISSION_POLICIES.join(", ")}`);
	return value;
}
function parseTimeoutSeconds(value) {
	const parsed = Number(value);
	if (!Number.isFinite(parsed) || parsed <= 0) throw new InvalidArgumentError("Timeout must be a positive number of seconds");
	return Math.round(parsed * 1e3);
}
function parseTtlSeconds(value) {
	const parsed = Number(value);
	if (!Number.isFinite(parsed) || parsed < 0) throw new InvalidArgumentError("TTL must be a non-negative number of seconds");
	return Math.round(parsed * 1e3);
}
function parseSessionName(value) {
	const trimmed = value.trim();
	if (trimmed.length === 0) throw new InvalidArgumentError("Session name must not be empty");
	return trimmed;
}
function parseNonEmptyValue(label, value) {
	const trimmed = value.trim();
	if (trimmed.length === 0) throw new InvalidArgumentError(`${label} must not be empty`);
	return trimmed;
}
function parseHistoryLimit(value) {
	const parsed = Number(value);
	if (!Number.isInteger(parsed) || parsed <= 0) throw new InvalidArgumentError("Limit must be a positive integer");
	return parsed;
}
function parseAllowedTools(value) {
	const trimmed = value.trim();
	if (trimmed.length === 0) return [];
	const items = trimmed.split(",").map((item) => item.trim());
	if (items.some((item) => item.length === 0)) throw new InvalidArgumentError("Allowed tools must be a comma-separated list without empty entries");
	return items;
}
function parseMaxTurns(value) {
	const parsed = Number(value);
	if (!Number.isInteger(parsed) || parsed <= 0) throw new InvalidArgumentError("Max turns must be a positive integer");
	return parsed;
}
function parsePromptRetries(value) {
	const parsed = Number(value);
	if (!Number.isInteger(parsed) || parsed < 0) throw new InvalidArgumentError("Prompt retries must be a non-negative integer");
	return parsed;
}
function resolvePermissionMode(flags, defaultMode) {
	if ([
		flags.approveAll,
		flags.approveReads,
		flags.denyAll
	].filter(Boolean).length > 1) throw new InvalidArgumentError("Use only one permission mode: --approve-all, --approve-reads, or --deny-all");
	if (flags.approveAll) return "approve-all";
	if (flags.approveReads) return "approve-reads";
	if (flags.denyAll) return "deny-all";
	return defaultMode;
}
function addGlobalFlags(command) {
	return command.option("--agent <command>", "Raw ACP agent command (escape hatch)").option("--cwd <dir>", "Working directory", process.cwd()).option("--auth-policy <policy>", "Authentication policy: skip or fail when auth is required", parseAuthPolicy).option("--approve-all", "Auto-approve all permission requests").option("--approve-reads", "Auto-approve read/search requests and prompt for writes").option("--deny-all", "Deny all permission requests").option("--non-interactive-permissions <policy>", "When prompting is unavailable: deny or fail", parseNonInteractivePermissionPolicy).option("--format <fmt>", "Output format: text, json, quiet", parseOutputFormat).option("--suppress-reads", "Suppress raw read-file contents in output").option("--model <id>", "Agent model id").option("--allowed-tools <list>", "Allowed tool names as a comma-separated list (use \"\" for no tools)", parseAllowedTools).option("--max-turns <count>", "Maximum turns for the session", parseMaxTurns).option("--prompt-retries <count>", "Retry failed prompt turns on transient errors (default: 0)", parsePromptRetries).option("--json-strict", "Strict JSON mode: requires --format json and suppresses non-JSON stderr output").option("--timeout <seconds>", "Maximum time to wait for agent response", parseTimeoutSeconds).option("--ttl <seconds>", "Queue owner idle TTL before shutdown (0 = keep alive forever) (default: 300)", parseTtlSeconds).option("--verbose", "Enable verbose debug logs");
}
function addSessionOption(command) {
	return command.option("-s, --session <name>", "Use named session instead of cwd default", parseSessionName).option("--no-wait", "Queue prompt and return immediately when another prompt is already running");
}
function addSessionNameOption(command) {
	return command.option("-s, --session <name>", "Use named session instead of cwd default", parseSessionName);
}
function resolveSessionNameFromFlags(flags, command) {
	if (flags.session) return flags.session;
	const allOpts = command.optsWithGlobals?.();
	if (allOpts && typeof allOpts.session === "string") return parseSessionName(allOpts.session);
	const parentOpts = command.parent?.opts?.();
	if (parentOpts && typeof parentOpts.session === "string") return parseSessionName(parentOpts.session);
}
function addPromptInputOption(command) {
	return command.option("-f, --file <path>", "Read prompt text from file path (use - for stdin)");
}
function resolveGlobalFlags(command, config) {
	const opts = command.optsWithGlobals();
	const format = opts.format ?? config.format ?? "text";
	const jsonStrict = opts.jsonStrict === true;
	const verbose = opts.verbose === true;
	if (jsonStrict && format !== "json") throw new InvalidArgumentError("--json-strict requires --format json");
	if (jsonStrict && verbose) throw new InvalidArgumentError("--json-strict cannot be combined with --verbose");
	return {
		agent: opts.agent,
		cwd: opts.cwd ?? process.cwd(),
		authPolicy: opts.authPolicy ?? config.authPolicy,
		nonInteractivePermissions: opts.nonInteractivePermissions ?? config.nonInteractivePermissions,
		jsonStrict,
		suppressReads: opts.suppressReads === true,
		timeout: opts.timeout ?? config.timeoutMs,
		ttl: opts.ttl ?? config.ttlMs ?? 3e5,
		verbose,
		format,
		model: typeof opts.model === "string" ? parseNonEmptyValue("Model", opts.model) : void 0,
		allowedTools: Array.isArray(opts.allowedTools) ? opts.allowedTools : void 0,
		maxTurns: typeof opts.maxTurns === "number" ? opts.maxTurns : void 0,
		promptRetries: typeof opts.promptRetries === "number" ? opts.promptRetries : void 0,
		approveAll: opts.approveAll ? true : void 0,
		approveReads: opts.approveReads ? true : void 0,
		denyAll: opts.denyAll ? true : void 0
	};
}
function resolveOutputPolicy(format, jsonStrict) {
	return {
		format,
		jsonStrict,
		suppressReads: false,
		suppressNonJsonStderr: jsonStrict,
		queueErrorAlreadyEmitted: format !== "quiet",
		suppressSdkConsoleErrors: jsonStrict
	};
}
function resolveAgentInvocation(explicitAgentName, globalFlags, config) {
	const override = globalFlags.agent?.trim();
	if (override && explicitAgentName) throw new InvalidArgumentError("Do not combine positional agent with --agent override");
	const agentName = explicitAgentName ?? config.defaultAgent ?? "codex";
	return {
		agentName,
		agentCommand: override && override.length > 0 ? override : resolveAgentCommand(agentName, config.agents),
		cwd: path.resolve(globalFlags.cwd)
	};
}
//#endregion
export { hasExplicitPermissionModeFlag as a, parseMaxTurns as c, parseTtlSeconds as d, resolveAgentInvocation as f, resolveSessionNameFromFlags as g, resolvePermissionMode as h, addSessionOption as i, parseNonEmptyValue as l, resolveOutputPolicy as m, addPromptInputOption as n, parseAllowedTools as o, resolveGlobalFlags as p, addSessionNameOption as r, parseHistoryLimit as s, addGlobalFlags as t, parseSessionName as u };

//# sourceMappingURL=flags-ceSqz2T6.js.map