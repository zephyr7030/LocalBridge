import { invoke } from "@tauri-apps/api/core";

export type AccessCode = "edit" | "full" | "admin";
export type PrivilegeCode = "off" | "requested" | "awaiting" | "active" | "fault";
export type ServiceCode = "off" | "starting" | "online" | "recovering" | "fault";
export type TaskKindCode = "read" | "search" | "modify" | "command" | "git" | "build" | "test" | "admin" | "other";
export type TaskStateCode = "idle" | "running" | "waiting" | "blocked" | "failed" | "cancelled";
export interface ProjectProjection { id: string; path: string; active: boolean }
export interface TaskProjection { kind: TaskKindCode; summary: string | null; state: TaskStateCode }
export interface ReconnectProjection { generation: number }
export interface MainProjection { permission: AccessCode; privilege: PrivilegeCode; localEnvironmentService: ServiceCode; tunnelService: ServiceCode; codingService: ServiceCode; currentProject: string | null; projects: ProjectProjection[]; currentTask: TaskProjection | null; tunnelId: string | null; runtimeKeySaved: boolean; autoStart: boolean; closeWindowContinueRunning: boolean; reconnect: ReconnectProjection | null; }
export const bridge = {
  read: () => invoke<MainProjection>("get_main_projection"),
  setAccess: (mode: AccessCode) => invoke<void>("set_permission_mode", { mode }),
  setAutoStart: (enabled: boolean) => invoke<void>("set_auto_start", { enabled }),
  setCloseWindowContinueRunning: (enabled: boolean) => invoke<void>("set_close_window_continue_running", { enabled }),
  saveTunnelId: (value: string) => invoke<void>("save_tunnel_id", { value }),
  saveKey: (value: string) => invoke<void>("save_runtime_key", { value }),
  retry: () => invoke<void>("retry_connection"),
  chooseProjectFolder: () => invoke<string | null>("choose_onboarding_workspace_folder"),
  addProject: (path: string) => invoke<void>("add_project", { path }),
  selectProject: (id: string) => invoke<void>("select_project", { id }),
  removeProject: (id: string) => invoke<void>("remove_project", { id }),
};
