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
export interface MainProjection { permission: AccessCode; privilege: PrivilegeCode; localEnvironmentService: ServiceCode; tunnelService: ServiceCode; codingService: ServiceCode; currentProject: string | null; projects: ProjectProjection[]; currentTask: TaskProjection | null; currentWorkflow: CurrentWorkflowProjection | null; currentCommand: CurrentCommandProjection | null; lastCommand: LastCommandProjection | null; lastTool: LastToolProjection | null; currentActivity: CurrentActivityProjection | null; lastActivity: LastActivityProjection | null; projectionRevision: number; tunnelId: string | null; runtimeKeySaved: boolean; autoStart: boolean; closeWindowContinueRunning: boolean; reconnect: ReconnectProjection | null; }
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
};
