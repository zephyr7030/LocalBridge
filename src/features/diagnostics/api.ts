import { invoke } from "@tauri-apps/api/core";

export type DiagnosticLevel = "ok" | "warning" | "error";
export type BrokerDiagnosticState = "off" | "requested" | "awaiting" | "active" | "fault";
export type ReconnectAttemptState = "running" | "failed";

export interface DiagnosticCheck {
  code: string;
  label: string;
  level: DiagnosticLevel;
  detail: string;
}

export interface BrokerDiagnostics {
  state: BrokerDiagnosticState;
  generation: number | null;
}

export interface ReconnectAttempt {
  attempt: number;
  state: ReconnectAttemptState;
}

export interface ReconnectDiagnostics {
  generation: number;
  component: string;
  attentionRequired: boolean;
  attempts: ReconnectAttempt[];
}

export interface DiagnosticsSnapshot {
  schemaVersion: number;
  checks: DiagnosticCheck[];
  broker: BrokerDiagnostics;
  reconnect: ReconnectDiagnostics | null;
  runtimeKeyPresent: boolean;
  activeWorkspacePath: string | null;
  recentEvents: Array<{ level: DiagnosticLevel; message: string; timestampMs: number }>;
}

export const diagnosticsApi = {
  read: () => invoke<DiagnosticsSnapshot>("get_diagnostics"),
  openLogs: () => invoke<void>("open_logs"),
  exportReport: () => invoke<string>("export_diagnostics"),
};
