import { invoke } from "@tauri-apps/api/core";

export type DiagnosticLevel = "ok" | "warning" | "error";
export type BrokerDiagnosticState = "off" | "requested" | "awaiting" | "active" | "fault";

export interface DiagnosticCheck {
  code: string;
  label: string;
  level: DiagnosticLevel;
  detail: string;
}

export interface DiagnosticsViewProjection {
  checks: DiagnosticCheck[];
  privilege: BrokerDiagnosticState;
  activeWorkspacePath: string | null;
  recentEvents: Array<{ level: DiagnosticLevel; message: string; timestampMs: number }>;
}

export const diagnosticsApi = {
  read: () => invoke<DiagnosticsViewProjection>("get_diagnostics"),
  openLogs: () => invoke<void>("open_logs"),
  exportReport: () => invoke<string>("export_diagnostics"),
};
