import { s as SessionRecord } from "./types-yxf-gcOE.js";
import { Command } from "commander";

//#region src/cli/flags.d.ts
declare function parseTtlSeconds(value: string): number;
declare function parseAllowedTools(value: string): string[];
declare function parseMaxTurns(value: string): number;
//#endregion
//#region src/cli/output/render.d.ts
type SessionConnectionStatus = "connected" | "needs reconnect";
declare function formatPromptSessionBannerLine(record: SessionRecord, currentCwd: string, connectionStatus?: SessionConnectionStatus): string;
//#endregion
export { formatPromptSessionBannerLine, parseAllowedTools, parseMaxTurns, parseTtlSeconds };
//# sourceMappingURL=cli.d.ts.map