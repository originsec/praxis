import { t as __exportAll } from "./rolldown-runtime-CiIaOW0V.js";
import { u as normalizeRuntimeSessionId } from "./perf-metrics-D0um6IR6.js";
import { s as probeQueueOwnerHealth } from "./ipc-BM335WFg.js";
import path from "node:path";
//#region src/cli/output/json-output.ts
function emitJsonResult(format, payload) {
	if (format !== "json") return false;
	process.stdout.write(`${JSON.stringify(payload)}\n`);
	return true;
}
//#endregion
//#region src/cli/output/render.ts
var render_exports = /* @__PURE__ */ __exportAll({
	agentSessionIdPayload: () => agentSessionIdPayload,
	formatPromptSessionBannerLine: () => formatPromptSessionBannerLine,
	printClosedSessionByFormat: () => printClosedSessionByFormat,
	printCreatedSessionBanner: () => printCreatedSessionBanner,
	printEnsuredSessionByFormat: () => printEnsuredSessionByFormat,
	printNewSessionByFormat: () => printNewSessionByFormat,
	printPromptSessionBanner: () => printPromptSessionBanner,
	printQueuedPromptByFormat: () => printQueuedPromptByFormat,
	printSessionsByFormat: () => printSessionsByFormat
});
function formatSessionLabel(record) {
	return record.name ?? "cwd";
}
function formatRoutedFrom(sessionCwd, currentCwd) {
	const relative = path.relative(sessionCwd, currentCwd);
	if (!relative || relative === ".") return;
	return relative.startsWith(".") ? relative : `.${path.sep}${relative}`;
}
async function resolveSessionConnectionStatus(record) {
	return (await probeQueueOwnerHealth(record.acpxRecordId)).healthy ? "connected" : "needs reconnect";
}
function printSessionsByFormat(sessions, format) {
	if (format === "json") {
		process.stdout.write(`${JSON.stringify(sessions)}\n`);
		return;
	}
	if (format === "quiet") {
		for (const session of sessions) {
			const closedMarker = session.closed ? " [closed]" : "";
			process.stdout.write(`${session.acpxRecordId}${closedMarker}\n`);
		}
		return;
	}
	if (sessions.length === 0) {
		process.stdout.write("No sessions\n");
		return;
	}
	for (const session of sessions) {
		const closedMarker = session.closed ? " [closed]" : "";
		process.stdout.write(`${session.acpxRecordId}${closedMarker}\t${session.name ?? "-"}\t${session.cwd}\t${session.lastUsedAt}\n`);
	}
}
function printClosedSessionByFormat(record, format) {
	if (emitJsonResult(format, {
		action: "session_closed",
		acpxRecordId: record.acpxRecordId,
		acpxSessionId: record.acpSessionId,
		agentSessionId: record.agentSessionId
	})) return;
	if (format === "quiet") return;
	process.stdout.write(`${record.acpxRecordId}\n`);
}
function printNewSessionByFormat(record, replaced, format) {
	if (emitJsonResult(format, {
		action: "session_ensured",
		created: true,
		acpxRecordId: record.acpxRecordId,
		acpxSessionId: record.acpSessionId,
		agentSessionId: record.agentSessionId,
		name: record.name,
		replacedSessionId: replaced?.acpxRecordId
	})) return;
	if (format === "quiet") {
		process.stdout.write(`${record.acpxRecordId}\n`);
		return;
	}
	if (replaced) {
		process.stdout.write(`${record.acpxRecordId}\t(replaced ${replaced.acpxRecordId})\n`);
		return;
	}
	process.stdout.write(`${record.acpxRecordId}\n`);
}
function printEnsuredSessionByFormat(record, created, format) {
	if (emitJsonResult(format, {
		action: "session_ensured",
		created,
		acpxRecordId: record.acpxRecordId,
		acpxSessionId: record.acpSessionId,
		agentSessionId: record.agentSessionId,
		name: record.name
	})) return;
	if (format === "quiet") {
		process.stdout.write(`${record.acpxRecordId}\n`);
		return;
	}
	const action = created ? "created" : "existing";
	process.stdout.write(`${record.acpxRecordId}\t(${action})\n`);
}
function printQueuedPromptByFormat(result, format) {
	if (emitJsonResult(format, {
		action: "prompt_queued",
		acpxRecordId: result.sessionId,
		requestId: result.requestId
	})) return;
	if (format === "quiet") return;
	process.stdout.write(`[queued] ${result.requestId}\n`);
}
function formatPromptSessionBannerLine(record, currentCwd, connectionStatus = "needs reconnect") {
	const label = formatSessionLabel(record);
	const normalizedSessionCwd = path.resolve(record.cwd);
	const normalizedCurrentCwd = path.resolve(currentCwd);
	const routedFrom = normalizedSessionCwd === normalizedCurrentCwd ? void 0 : formatRoutedFrom(normalizedSessionCwd, normalizedCurrentCwd);
	const status = connectionStatus;
	if (routedFrom) return `[acpx] session ${label} (${record.acpxRecordId}) · ${normalizedSessionCwd} (routed from ${routedFrom}) · agent ${status}`;
	return `[acpx] session ${label} (${record.acpxRecordId}) · ${normalizedSessionCwd} · agent ${status}`;
}
async function printPromptSessionBanner(record, currentCwd, format, jsonStrict = false) {
	if (format === "quiet" || jsonStrict && format === "json") return;
	const status = await resolveSessionConnectionStatus(record);
	process.stderr.write(`${formatPromptSessionBannerLine(record, currentCwd, status)}\n`);
}
function printCreatedSessionBanner(record, agentName, format, jsonStrict = false) {
	if (format === "quiet" || jsonStrict && format === "json") return;
	const label = formatSessionLabel(record);
	process.stderr.write(`[acpx] created session ${label} (${record.acpxRecordId})\n`);
	process.stderr.write(`[acpx] agent: ${agentName}\n`);
	process.stderr.write(`[acpx] cwd: ${record.cwd}\n`);
}
function agentSessionIdPayload(agentSessionId) {
	const normalized = normalizeRuntimeSessionId(agentSessionId);
	if (!normalized) return {};
	return { agentSessionId: normalized };
}
//#endregion
export { emitJsonResult as i, formatPromptSessionBannerLine as n, render_exports as r, agentSessionIdPayload as t };

//# sourceMappingURL=render-Br-kVPK_.js.map