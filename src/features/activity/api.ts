import { invoke } from "@tauri-apps/api/core";

import { isRecord, isStringOrNull } from "../../bridge";

export type ActivitySource = "tool" | "administrator";

export interface ActivityEntry {
  source: ActivitySource;
  timestampMs: number;
  action: string;
  operation: string | null;
  target: string | null;
  outcome: string | null;
  errorCode: string | null;
  durationMs: number | null;
  exitCode: number | null;
  risk: string[];
}

export interface ActivityProjection {
  logRevision: number;
  entries: ActivityEntry[];
}

const sources: ActivitySource[] = ["tool", "administrator"];
const isNumberOrNull = (value: unknown): value is number | null =>
  value === null || typeof value === "number";

const isEntry = (value: unknown): value is ActivityEntry =>
  isRecord(value)
  && typeof value.source === "string" && sources.includes(value.source as ActivitySource)
  && typeof value.timestampMs === "number"
  && typeof value.action === "string"
  && isStringOrNull(value.operation)
  && isStringOrNull(value.target)
  && isStringOrNull(value.outcome)
  && isStringOrNull(value.errorCode)
  && isNumberOrNull(value.durationMs)
  && isNumberOrNull(value.exitCode)
  && Array.isArray(value.risk) && value.risk.every((item) => typeof item === "string");

function parseActivity(value: unknown): ActivityProjection {
  if (!isRecord(value)
    || typeof value.logRevision !== "number"
    || !Array.isArray(value.entries)
    || !value.entries.every(isEntry)) {
    throw new Error("后端活动记录合同不兼容");
  }
  return value as unknown as ActivityProjection;
}

export const activityApi = {
  read: async () => parseActivity(await invoke<unknown>("get_activity")),
  waitForChange: (sinceRevision: number) => invoke<number>("wait_activity_change", { sinceRevision }),
};
