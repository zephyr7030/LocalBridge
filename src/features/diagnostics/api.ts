import { invoke } from "@tauri-apps/api/core";

import { isEnum, isRecord, isStringOrNull, isUiErrorCategory, type UiErrorCategory } from "../../bridge";

export type DiagnosticLevel = "ok" | "warning" | "error" | "unknown";
export type BrokerDiagnosticState = "off" | "requested" | "awaiting" | "active" | "fault" | "unavailable";

export interface DiagnosticCheck {
  code: string;
  label: string;
  level: DiagnosticLevel;
  detail: string;
}

export interface DiagnosticsViewProjection {
  projectionRevision: number;
  logRevision: number;
  checks: DiagnosticCheck[];
  privilege: BrokerDiagnosticState;
  activeWorkspacePath: string | null;
  recentEvents: Array<{ level: DiagnosticLevel; message: string; timestampMs: number }>;
  activeFaults: Array<{ code: string; category: UiErrorCategory; message: string; retryable: boolean }>;
}

export interface DiagnosticsRevisionProjection {
  projectionRevision: number;
  logRevision: number;
}

const diagnosticLevels: DiagnosticLevel[] = ["ok", "warning", "error", "unknown"];
const brokerStates: BrokerDiagnosticState[] = ["off", "requested", "awaiting", "active", "fault", "unavailable"];
const isCheck = (value: unknown): value is DiagnosticCheck => isRecord(value)
  && typeof value.code === "string" && typeof value.label === "string"
  && isEnum(value.level, diagnosticLevels) && typeof value.detail === "string";
const isEvent = (value: unknown) => isRecord(value) && isEnum(value.level, diagnosticLevels)
  && typeof value.message === "string" && typeof value.timestampMs === "number";
const isFault = (value: unknown) => isRecord(value) && typeof value.code === "string"
  && isUiErrorCategory(value.category) && typeof value.message === "string" && typeof value.retryable === "boolean";

export function parseDiagnosticsProjection(value: unknown): DiagnosticsViewProjection {
  if (!isRecord(value)
    || typeof value.projectionRevision !== "number"
    || typeof value.logRevision !== "number"
    || !Array.isArray(value.checks) || !value.checks.every(isCheck)
    || !isEnum(value.privilege, brokerStates)
    || !isStringOrNull(value.activeWorkspacePath)
    || !Array.isArray(value.recentEvents) || !value.recentEvents.every(isEvent)
    || !Array.isArray(value.activeFaults) || !value.activeFaults.every(isFault)) {
    throw new Error("后端诊断状态合同不兼容");
  }
  return value as unknown as DiagnosticsViewProjection;
}

function parseDiagnosticsRevision(value: unknown): DiagnosticsRevisionProjection {
  if (!isRecord(value) || typeof value.projectionRevision !== "number" || typeof value.logRevision !== "number") {
    throw new Error("后端诊断 revision 合同不兼容");
  }
  return value as unknown as DiagnosticsRevisionProjection;
}

export const diagnosticsApi = {
  read: async () => parseDiagnosticsProjection(await invoke<unknown>("get_diagnostics")),
  waitForChange: async (sinceProjectionRevision: number, sinceLogRevision: number) => parseDiagnosticsRevision(await invoke<unknown>("wait_diagnostics_change", { sinceProjectionRevision, sinceLogRevision })),
  openLogs: () => invoke<void>("open_logs"),
  exportReport: () => invoke<string>("export_diagnostics"),
};
