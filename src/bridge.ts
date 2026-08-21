import { invoke } from "@tauri-apps/api/core";

export type AccessCode = "edit" | "full" | "admin";
export type PrivilegeCode = "off" | "requested" | "awaiting" | "active" | "fault";
export type ServiceCode = "off" | "starting" | "online" | "recovering" | "fault";
export type TaskKindCode = "read" | "search" | "modify" | "command" | "git" | "build" | "test" | "admin" | "other";
export type TaskStateCode = "idle" | "running" | "waiting" | "blocked" | "failed" | "cancelled";
export interface ProjectProjection { id: string; path: string; active: boolean }
export interface TaskProjection { kind: TaskKindCode; summary: string | null; state: TaskStateCode; elapsedMs: number | null }
export type WorkflowStateCode = "running" | "waiting";
export type CommandActivityStateCode = "running" | "waiting_input" | "cancelling";
export type CommandTerminalStateCode = "completed" | "failed" | "cancelled" | "timed_out" | "lost";
export interface CurrentWorkflowProjection { state: WorkflowStateCode }
export interface CurrentCommandProjection { state: CommandActivityStateCode }
export interface LastCommandProjection { status: CommandTerminalStateCode; ageMs: number }
export interface LastToolProjection { kind: TaskKindCode; summary: string | null; ageMs: number }
export type ActivityStateCode = "running" | "waiting" | "waiting_input" | "cancelling";
export type ActivityOutcomeCode = "completed" | "failed" | "cancelled" | "timed_out" | "lost";
export interface CurrentActivityProjection { kind: TaskKindCode; state: ActivityStateCode; summary: string | null; elapsedMs: number | null; step: string | null; progressCurrent: number | null; progressTotal: number | null }
export interface LastActivityProjection { kind: TaskKindCode; summary: string | null; outcome: ActivityOutcomeCode; completedAtMs: number }
export interface ReconnectProjection { generation: number }
export type UiErrorCategory = "validation" | "authorization" | "capacity" | "conflict" | "timeout" | "unavailable" | "internal";
export interface UiError { code: string; category: UiErrorCategory; message: string; retryable: boolean; operationId: string | null; sessionId: string | null; requestId: number | string | null; taskId: string | null }
export interface UiFaultProjection { code: string; category: UiErrorCategory; message: string; retryable: boolean }
export type UpdateStateCode = "source_unavailable" | "idle" | "checking" | "current" | "available" | "failed";
export interface UpdateProjection { state: UpdateStateCode; currentVersion: string; latestVersion: string | null; releaseUrl: string | null; operationId: string | null; attempt: number | null; retryable: boolean }
export interface MainProjection { permission: AccessCode; privilege: PrivilegeCode; localEnvironmentService: ServiceCode; tunnelService: ServiceCode; codingService: ServiceCode; currentProject: string | null; projects: ProjectProjection[]; currentTask: TaskProjection | null; currentWorkflow: CurrentWorkflowProjection | null; currentCommand: CurrentCommandProjection | null; lastCommand: LastCommandProjection | null; lastTool: LastToolProjection | null; currentActivity: CurrentActivityProjection | null; lastActivity: LastActivityProjection | null; projectionRevision: number; tunnelId: string | null; runtimeKeySaved: boolean; autoStart: boolean; closeWindowContinueRunning: boolean; reconnect: ReconnectProjection | null; update: UpdateProjection; activeFaults: UiFaultProjection[]; }
export function uiErrorMessage(value: unknown, fallback: string): string {
  if (typeof value === "object" && value !== null && "message" in value && typeof value.message === "string" && value.message.trim()) return value.message;
  if (value instanceof Error && value.message.trim()) return value.message;
  return fallback;
}
export const bridge = {
  read: () => invoke<MainProjection>("get_main_projection"),
  waitForProjectionChange: (sinceRevision: number) => invoke<number>("wait_main_projection_change", { sinceRevision }),
  uiReady: () => invoke<void>("ui_ready"),
  setAccess: (mode: AccessCode) => invoke<void>("set_permission_mode", { mode }),
  setAutoStart: (enabled: boolean) => invoke<void>("set_auto_start", { enabled }),
  setCloseWindowContinueRunning: (enabled: boolean) => invoke<void>("set_close_window_continue_running", { enabled }),
  saveTunnelId: (value: string) => invoke<void>("save_tunnel_id", { value }),
  saveKey: (value: string) => invoke<void>("save_runtime_key", { value }),
  clearKey: () => invoke<void>("delete_runtime_key"),
  retry: () => invoke<void>("retry_connection"),
  chooseProjectFolder: () => invoke<string | null>("choose_onboarding_workspace_folder"),
  addProject: (path: string) => invoke<void>("add_project", { path }),
  selectProject: (id: string) => invoke<void>("select_project", { id }),
  removeProject: (id: string) => invoke<void>("remove_project", { id }),
  restartServices: () => invoke<void>("restart_services"),
  stopServices: () => invoke<void>("stop_services"),
  retryUpdateCheck: () => invoke<void>("retry_update_check"),
  openGitHubReleases: () => invoke<void>("open_github_releases"),
};
