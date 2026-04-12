//#region src/acp/jsonrpc.ts
function asRecord(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return null;
	return value;
}
function hasValidId(value) {
	return value === null || typeof value === "string" || typeof value === "number" && Number.isFinite(value);
}
function isErrorObject(value) {
	const record = asRecord(value);
	return !!record && typeof record.code === "number" && Number.isFinite(record.code) && typeof record.message === "string";
}
function hasResultOrError(value) {
	const hasResult = Object.hasOwn(value, "result");
	const hasError = Object.hasOwn(value, "error");
	if (hasResult && hasError) return false;
	if (!hasResult && !hasError) return false;
	if (hasError && !isErrorObject(value.error)) return false;
	return true;
}
function isAcpJsonRpcMessage(value) {
	const record = asRecord(value);
	if (!record || record.jsonrpc !== "2.0") return false;
	const hasMethod = typeof record.method === "string" && record.method.length > 0;
	const hasId = Object.hasOwn(record, "id");
	if (hasMethod && !hasId) return true;
	if (hasMethod && hasId) return hasValidId(record.id);
	if (!hasMethod && hasId) {
		if (!hasValidId(record.id)) return false;
		return hasResultOrError(record);
	}
	return false;
}
function isJsonRpcNotification(message) {
	return Object.hasOwn(message, "method") && typeof message.method === "string" && !Object.hasOwn(message, "id");
}
function isSessionUpdateNotification(message) {
	return isJsonRpcNotification(message) && message.method === "session/update";
}
function extractSessionUpdateNotification(message) {
	if (!isSessionUpdateNotification(message)) return;
	const params = asRecord(message.params);
	if (!params) return;
	const sessionId = typeof params.sessionId === "string" ? params.sessionId : null;
	if (!sessionId) return;
	const update = asRecord(params.update);
	if (!update || typeof update.sessionUpdate !== "string") return;
	return {
		sessionId,
		update
	};
}
function parsePromptStopReason(message) {
	if (!Object.hasOwn(message, "id") || !Object.hasOwn(message, "result")) return;
	const record = asRecord(message.result);
	if (!record) return;
	return typeof record.stopReason === "string" ? record.stopReason : void 0;
}
function parseJsonRpcErrorMessage(message) {
	if (!Object.hasOwn(message, "error")) return;
	const errorRecord = asRecord(message.error);
	if (!errorRecord || typeof errorRecord.message !== "string") return;
	return errorRecord.message;
}
//#endregion
export { parsePromptStopReason as a, parseJsonRpcErrorMessage as i, isAcpJsonRpcMessage as n, isSessionUpdateNotification as r, extractSessionUpdateNotification as t };

//# sourceMappingURL=jsonrpc-DSxh2w5R.js.map