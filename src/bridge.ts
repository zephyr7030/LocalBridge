import { invoke } from "@tauri-apps/api/core";

export type AccessCode = "edit" | "full" | "admin";
export type ProjectionStatusCode = "ready" | "stale" | "unavailable" | "fault";
export type PermissionReconciliationCode = "converged" | "authorization_required" | "awaiting_authorization" | "broker_unavailable" | "disable_pending" | "unavailable";
export type PathAuthorityCode = "workspace" | "administrator";
export type EffectiveAvailabilityCode = "available" | "disabled" | "reconciling" | "unavailable";
export type PrivilegeCode = "off" | "requested" | "awaiting" | "active" | "fault";
export type ServiceCode = "off" | "starting" | "online" | "recovering" | "fault";
export type TaskKindCode = "read" | "search" | "modify" | "command" | "git" | "build" | "test" | "admin" | "other";
export type TaskStateCode = "idle" | "running" | "waiting" | "blocked" | "failed" | "cancelled";
export interface ProjectProjection { id: string; path: string; active: boolean }
export interface WorkspaceProjection { desiredPath: string | null; observedPath: string | null; effective: EffectiveAvailabilityCode }
export interface ConnectionProjection { desiredTunnelId: string | null; observedTunnelId: string | null; effective: EffectiveAvailabilityCode }
export interface TaskProjection { kind: TaskKindCode; summary: string | null; state: TaskStateCode; elapsedMs: number | null }
export type WorkflowStateCode = "running" | "waiting";
export type CommandActivityStateCode = "running" | "waiting_input" | "cancelling";
export type CommandTerminalStateCode = "completed" | "failed" | "cancelled" | "timed_out" | "lost";
export interface LastCommandProjection { status: CommandTerminalStateCode; ageMs: number }
export interface LastToolProjection { kind: TaskKindCode; summary: string | null; ageMs: number }
export type ActivityStateCode = "running" | "waiting" | "waiting_input" | "cancelling";
export type ActivityOutcomeCode = "completed" | "failed" | "blocked" | "cancelled" | "timed_out" | "lost";
export interface CurrentActivityProjection { kind: TaskKindCode; state: ActivityStateCode; summary: string | null; elapsedMs: number | null; step: string | null; progressCurrent: number | null; progressTotal: number | null }
export interface LastActivityProjection { kind: TaskKindCode; summary: string | null; outcome: ActivityOutcomeCode; completedAtMs: number }
export interface ReconnectProjection { generation: number }
export type UiErrorCategory = "validation" | "authorization" | "capacity" | "conflict" | "timeout" | "unavailable" | "internal";
export interface UiError { code: string; category: UiErrorCategory; message: string; retryable: boolean; operationId: string | null; sessionId: string | null; requestId: number | string | null; taskId: string | null }
export interface UiFaultProjection { code: string; category: UiErrorCategory; message: string; retryable: boolean }
export type UpdateStateCode = "source_unavailable" | "idle" | "checking" | "current" | "available" | "failed";
export interface UpdateProjection { state: UpdateStateCode; currentVersion: string; latestVersion: string | null; releaseUrl: string | null; operationId: string | null; attempt: number | null; retryable: boolean }
export interface OpenReleaseProjection { releaseUrl: string }
export interface AdminConsentChallenge { challengeId: string; notBeforeUnixMs: number }
export interface MainProjection { authorityStatus: ProjectionStatusCode; runtimeStatus: ProjectionStatusCode; settingsStatus: ProjectionStatusCode; workspaceStatus: ProjectionStatusCode; connectionStatus: ProjectionStatusCode; activityStatus: ProjectionStatusCode; updateStatus: ProjectionStatusCode; permission: AccessCode | null; effectivePermission: AccessCode | null; permissionReconciliation: PermissionReconciliationCode | null; pathAuthority: PathAuthorityCode | null; privilege: PrivilegeCode | null; localEnvironmentService: ServiceCode | null; tunnelService: ServiceCode | null; codingService: ServiceCode | null; onboardingReady: boolean | null; workspace: WorkspaceProjection | null; projects: ProjectProjection[] | null; currentTask: TaskProjection | null; currentActivity: CurrentActivityProjection | null; lastActivity: LastActivityProjection | null; projectionRevision: number; connection: ConnectionProjection | null; runtimeKeySaved: boolean | null; autoStart: boolean | null; closeWindowContinueRunning: boolean | null; reconnect: ReconnectProjection | null; update: UpdateProjection | null; activeFaults: UiFaultProjection[]; }
const projectionStatuses: ProjectionStatusCode[] = ["ready", "stale", "unavailable", "fault"];
const accessCodes: AccessCode[] = ["edit", "full", "admin"];
const reconciliationCodes: PermissionReconciliationCode[] = ["converged", "authorization_required", "awaiting_authorization", "broker_unavailable", "disable_pending", "unavailable"];
const pathAuthorities: PathAuthorityCode[] = ["workspace", "administrator"];
const effectiveAvailabilities: EffectiveAvailabilityCode[] = ["available", "disabled", "reconciling", "unavailable"];
const privilegeCodes: PrivilegeCode[] = ["off", "requested", "awaiting", "active", "fault"];
const serviceCodes: ServiceCode[] = ["off", "starting", "online", "recovering", "fault"];
const taskKinds: TaskKindCode[] = ["read", "search", "modify", "command", "git", "build", "test", "admin", "other"];
const taskStates: TaskStateCode[] = ["idle", "running", "waiting", "blocked", "failed", "cancelled"];
const activityStates: ActivityStateCode[] = ["running", "waiting", "waiting_input", "cancelling"];
const activityOutcomes: ActivityOutcomeCode[] = ["completed", "failed", "blocked", "cancelled", "timed_out", "lost"];
const updateStates: UpdateStateCode[] = ["source_unavailable", "idle", "checking", "current", "available", "failed"];
const errorCategories: UiErrorCategory[] = ["validation", "authorization", "capacity", "conflict", "timeout", "unavailable", "internal"];
export const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
export const isStringOrNull = (value: unknown): value is string | null => value === null || typeof value === "string";
export const isNumberOrNull = (value: unknown): value is number | null => value === null || typeof value === "number";
export const isBooleanOrNull = (value: unknown): value is boolean | null => value === null || typeof value === "boolean";
export const isEnum = <T extends string>(value: unknown, values: readonly T[]): value is T => typeof value === "string" && values.includes(value as T);
export const isEnumOrNull = <T extends string>(value: unknown, values: readonly T[]): value is T | null => value === null || isEnum(value, values);
export const isUiErrorCategory = (value: unknown): value is UiErrorCategory => isEnum(value, errorCategories);
const isProject = (value: unknown): value is ProjectProjection => isRecord(value) && typeof value.id === "string" && typeof value.path === "string" && typeof value.active === "boolean";
const isWorkspace = (value: unknown): value is WorkspaceProjection => isRecord(value) && isStringOrNull(value.desiredPath) && isStringOrNull(value.observedPath) && isEnum(value.effective, effectiveAvailabilities);
const isConnection = (value: unknown): value is ConnectionProjection => isRecord(value) && isStringOrNull(value.desiredTunnelId) && isStringOrNull(value.observedTunnelId) && isEnum(value.effective, effectiveAvailabilities);
const isTask = (value: unknown): value is TaskProjection => isRecord(value) && isEnum(value.kind, taskKinds) && isStringOrNull(value.summary) && isEnum(value.state, taskStates) && isNumberOrNull(value.elapsedMs);
const isCurrentActivity = (value: unknown): value is CurrentActivityProjection => isRecord(value) && isEnum(value.kind, taskKinds) && isEnum(value.state, activityStates) && isStringOrNull(value.summary) && isNumberOrNull(value.elapsedMs) && isStringOrNull(value.step) && isNumberOrNull(value.progressCurrent) && isNumberOrNull(value.progressTotal);
const isLastActivity = (value: unknown): value is LastActivityProjection => isRecord(value) && isEnum(value.kind, taskKinds) && isStringOrNull(value.summary) && isEnum(value.outcome, activityOutcomes) && typeof value.completedAtMs === "number";
const isUpdate = (value: unknown): value is UpdateProjection => isRecord(value) && isEnum(value.state, updateStates) && typeof value.currentVersion === "string" && isStringOrNull(value.latestVersion) && isStringOrNull(value.releaseUrl) && isStringOrNull(value.operationId) && isNumberOrNull(value.attempt) && typeof value.retryable === "boolean";
const isFault = (value: unknown): value is UiFaultProjection => isRecord(value) && typeof value.code === "string" && isUiErrorCategory(value.category) && typeof value.message === "string" && typeof value.retryable === "boolean";
const isUiError = (value: unknown): value is UiError => isRecord(value)
  && typeof value.code === "string"
  && isUiErrorCategory(value.category)
  && typeof value.message === "string"
  && typeof value.retryable === "boolean"
  && isStringOrNull(value.operationId)
  && isStringOrNull(value.sessionId)
  && (value.requestId === null || typeof value.requestId === "string" || typeof value.requestId === "number")
  && isStringOrNull(value.taskId);
export function parseMainProjection(value: unknown): MainProjection {
  if (!isRecord(value)
    || !isEnum(value.authorityStatus, projectionStatuses)
    || !isEnum(value.runtimeStatus, projectionStatuses)
    || !isEnum(value.settingsStatus, projectionStatuses)
    || !isEnum(value.workspaceStatus, projectionStatuses)
    || !isEnum(value.connectionStatus, projectionStatuses)
    || !isEnum(value.activityStatus, projectionStatuses)
    || !isEnum(value.updateStatus, projectionStatuses)
    || !isEnumOrNull(value.permission, accessCodes)
    || !isEnumOrNull(value.effectivePermission, accessCodes)
    || !isEnumOrNull(value.permissionReconciliation, reconciliationCodes)
    || !isEnumOrNull(value.pathAuthority, pathAuthorities)
    || !isEnumOrNull(value.privilege, privilegeCodes)
    || !isEnumOrNull(value.localEnvironmentService, serviceCodes)
    || !isEnumOrNull(value.tunnelService, serviceCodes)
    || !isEnumOrNull(value.codingService, serviceCodes)
    || !isBooleanOrNull(value.onboardingReady)
    || !(value.workspace === null || isWorkspace(value.workspace))
    || !(value.projects === null || (Array.isArray(value.projects) && value.projects.every(isProject)))
    || !(value.currentTask === null || isTask(value.currentTask))
    || !(value.currentActivity === null || isCurrentActivity(value.currentActivity))
    || !(value.lastActivity === null || isLastActivity(value.lastActivity))
    || typeof value.projectionRevision !== "number"
    || !(value.connection === null || isConnection(value.connection))
    || !isBooleanOrNull(value.runtimeKeySaved)
    || !isBooleanOrNull(value.autoStart)
    || !isBooleanOrNull(value.closeWindowContinueRunning)
    || !(value.reconnect === null || (isRecord(value.reconnect) && typeof value.reconnect.generation === "number"))
    || !(value.update === null || isUpdate(value.update))
    || !Array.isArray(value.activeFaults) || !value.activeFaults.every(isFault)) {
    throw new Error("后端主状态合同不兼容");
  }
  return value as unknown as MainProjection;
}
export function parseUpdateProjection(value: unknown): UpdateProjection {
  if (!isUpdate(value)) throw new Error("后端更新状态合同不兼容");
  return value;
}
export function parseOpenReleaseProjection(value: unknown): OpenReleaseProjection {
  if (!isRecord(value) || typeof value.releaseUrl !== "string") throw new Error("后端发布地址合同不兼容");
  return value as unknown as OpenReleaseProjection;
}
export function parseAdminConsentChallenge(value: unknown): AdminConsentChallenge {
  if (!isRecord(value) || typeof value.challengeId !== "string" || typeof value.notBeforeUnixMs !== "number") throw new Error("后端管理员确认合同不兼容");
  return value as unknown as AdminConsentChallenge;
}
export function uiErrorMessage(value: unknown, fallback: string): string {
  return parseUiError(value, fallback).message;
}
export function parseUiError(value: unknown, fallback: string): UiError {
  if (isUiError(value)) return value;
  return {
    code: value instanceof Error ? "Ui.FrontendFailure" : "Ui.Unavailable",
    category: "unavailable",
    message: value instanceof Error && value.message.trim() ? value.message : fallback,
    retryable: true,
    operationId: null,
    sessionId: null,
    requestId: null,
    taskId: null,
  };
}
export const bridge = {
  read: async () => parseMainProjection(await invoke<unknown>("get_main_projection")),
  waitForProjectionChange: (sinceRevision: number) => invoke<number>("wait_main_projection_change", { sinceRevision }),
  uiReady: () => invoke<void>("ui_ready"),
  setAccess: (mode: AccessCode) => invoke<void>("set_permission_mode", { mode }),
  beginAdminConsent: async (challengeId: string) => parseAdminConsentChallenge(await invoke<unknown>("begin_admin_consent", { challengeId })),
  cancelAdminConsent: (challengeId: string) => invoke<void>("cancel_admin_consent", { challengeId }),
  confirmAdminConsent: (challengeId: string) => invoke<void>("confirm_admin_consent", { challengeId }),
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
  retryUpdateCheck: async () => parseUpdateProjection(await invoke<unknown>("retry_update_check")),
  openGitHubReleases: async () => parseOpenReleaseProjection(await invoke<unknown>("open_github_releases")),
};
