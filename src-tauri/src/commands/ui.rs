use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

use super::error::{UiError, UiResult};
use crate::app::{
    AutostartManager, DesktopLifecycle, DesktopRuntimeReconcileError, DesktopRuntimeStartError,
    STARTUP_PROFILE_FILE_NAME, StartupProfileStore, manual_stop_services,
};
use crate::control_plane::convergence::{
    AuthorityReconciliation, ConnectionProfile, DesiredWorkspace, ServiceIntent,
    StructuredPathAuthority,
};
use crate::control_plane::snapshot::{
    ControlPlaneSnapshot, EffectiveAvailability, ProjectionAvailability, ProjectionSection,
    TaskAggregate,
};
use crate::control_plane::update::UpdateStartError;
use crate::credentials::{CredentialStore, SecretString, WindowsCredentialStore};
use crate::domain::{
    ErrorCategory, ExecutionState, LifecycleState, OperationError, TerminalOutcome,
    UpdateCheckTrigger, UpdateLifecycle,
};
use crate::mcp::ProductionRuntimeConfig;
use crate::settings::{AppData, SettingsStore};
#[cfg(test)]
use crate::state::{CurrentTaskStatus, TaskExecutionState};
use crate::state::{
    PermissionMode, PrivilegeFault, PrivilegeState, RuntimeComponent, RuntimeFault, RuntimeState,
    TaskKind,
};
use crate::tunnel::TunnelId;
use crate::workspace::{WorkspaceId, WorkspaceValidator};
use windows_sys::Win32::UI::Shell::ShellExecuteW;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MainProjection {
    authority_status: &'static str,
    runtime_status: &'static str,
    settings_status: &'static str,
    workspace_status: &'static str,
    connection_status: &'static str,
    activity_status: &'static str,
    update_status: &'static str,
    permission: Option<&'static str>,
    effective_permission: Option<&'static str>,
    permission_reconciliation: Option<&'static str>,
    path_authority: Option<&'static str>,
    privilege: Option<&'static str>,
    local_environment_service: Option<&'static str>,
    tunnel_service: Option<&'static str>,
    coding_service: Option<&'static str>,
    onboarding_ready: Option<bool>,
    workspace: Option<UiWorkspaceProjection>,
    projects: Option<Vec<ProjectProjection>>,
    current_task: Option<TaskProjection>,
    current_activity: Option<CurrentActivityProjection>,
    last_activity: Option<LastActivityProjection>,
    projection_revision: u64,
    connection: Option<UiConnectionProjection>,
    runtime_key_saved: Option<bool>,
    auto_start: Option<bool>,
    close_window_continue_running: Option<bool>,
    reconnect: Option<ReconnectProjection>,
    update: Option<UpdateProjection>,
    active_faults: Vec<UiFaultProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiWorkspaceProjection {
    desired_path: Option<String>,
    observed_path: Option<String>,
    effective: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiConnectionProjection {
    desired_tunnel_id: Option<String>,
    observed_tunnel_id: Option<String>,
    effective: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjection {
    state: &'static str,
    current_version: String,
    latest_version: Option<String>,
    release_url: Option<String>,
    operation_id: Option<String>,
    attempt: Option<u8>,
    retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenReleaseProjection {
    release_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct UiFaultProjection {
    code: String,
    category: &'static str,
    message: String,
    retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectProjection {
    id: String,
    path: String,
    active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskProjection {
    kind: &'static str,
    summary: Option<String>,
    state: &'static str,
    elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CurrentActivityProjection {
    kind: &'static str,
    state: &'static str,
    summary: Option<String>,
    elapsed_ms: Option<u64>,
    step: Option<String>,
    progress_current: Option<u64>,
    progress_total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct LastActivityProjection {
    kind: &'static str,
    summary: Option<String>,
    outcome: &'static str,
    completed_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReconnectProjection {
    generation: u64,
}

const ADMIN_CONSENT_DURATION: Duration = Duration::from_millis(3000);

#[derive(Debug)]
struct PendingAdminConsent {
    challenge_id: String,
    not_before: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminConsentChallenge {
    challenge_id: String,
    not_before_unix_ms: u64,
}

#[derive(Debug, Default)]
struct AdminConsentGate {
    pending: Option<PendingAdminConsent>,
    confirmed: bool,
}

impl AdminConsentGate {
    fn begin_at(&mut self, challenge_id: &str, now: Instant) {
        self.pending = Some(PendingAdminConsent {
            challenge_id: challenge_id.to_string(),
            not_before: now + ADMIN_CONSENT_DURATION,
        });
        self.confirmed = false;
    }

    fn cancel(&mut self, challenge_id: &str) -> bool {
        let matches = self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.challenge_id == challenge_id);
        if matches {
            self.pending = None;
            self.confirmed = false;
        }
        matches
    }

    fn confirm_at(&mut self, challenge_id: &str, now: Instant) -> bool {
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        if pending.challenge_id != challenge_id || now < pending.not_before {
            return false;
        }
        self.pending = None;
        self.confirmed = true;
        true
    }

    fn consume_confirmed(&mut self) -> bool {
        std::mem::take(&mut self.confirmed)
    }

    fn reset(&mut self) {
        self.pending = None;
        self.confirmed = false;
    }
}

fn valid_admin_consent_challenge_id(challenge_id: &str) -> bool {
    (16..=64).contains(&challenge_id.len())
        && challenge_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn admin_consent_gate() -> &'static Mutex<AdminConsentGate> {
    static GATE: OnceLock<Mutex<AdminConsentGate>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(AdminConsentGate::default()))
}

fn begin_admin_consent_challenge(challenge_id: &str) -> UiResult<AdminConsentChallenge> {
    if !valid_admin_consent_challenge_id(challenge_id) {
        return Err(UiError::from("管理员确认标识无效"));
    }
    admin_consent_gate()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .begin_at(challenge_id, Instant::now());
    Ok(AdminConsentChallenge {
        challenge_id: challenge_id.to_string(),
        not_before_unix_ms: unix_millis().saturating_add(ADMIN_CONSENT_DURATION.as_millis() as u64),
    })
}

fn cancel_admin_consent_challenge(challenge_id: &str) -> UiResult<()> {
    if !valid_admin_consent_challenge_id(challenge_id) {
        return Err(UiError::from("管理员确认标识无效"));
    }
    admin_consent_gate()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .cancel(challenge_id);
    Ok(())
}

fn confirm_admin_consent_challenge(challenge_id: &str) -> UiResult<()> {
    if !valid_admin_consent_challenge_id(challenge_id) {
        return Err(UiError::from("管理员确认标识无效"));
    }
    let confirmed = admin_consent_gate()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .confirm_at(challenge_id, Instant::now());
    if confirmed {
        Ok(())
    } else {
        Err(UiError::from("管理员确认无效或尚未完成"))
    }
}

fn consume_confirmed_admin_consent() -> bool {
    admin_consent_gate()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .consume_confirmed()
}

fn reset_admin_consent() {
    admin_consent_gate()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .reset();
}
#[tauri::command]
pub async fn get_main_projection(app: AppHandle) -> UiResult<MainProjection> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<MainProjection> {
        let lifecycle = app.state::<DesktopLifecycle>();
        get_main_projection_blocking(&lifecycle)
    })
    .await
    .map_err(|_| UiError::internal("Ui.ProjectionJoinFailed", "主控状态后台任务异常"))?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn wait_main_projection_change(since_revision: u64, app: AppHandle) -> UiResult<u64> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<u64> {
        let lifecycle = app.state::<DesktopLifecycle>();
        Ok(lifecycle.wait_projection_change_after(since_revision))
    })
    .await
    .map_err(|_| UiError::internal("Ui.ProjectionWaitJoinFailed", "状态唤醒后台任务异常"))?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn ui_ready(app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let lifecycle = app.state::<DesktopLifecycle>();
        Ok(lifecycle
            .start_staged_foreground_after_ui_ready()
            .map(|_| ())
            .map_err(|_| "无法启动后台服务".to_string())?)
    })
    .await
    .map_err(|_| UiError::internal("Ui.ReadyJoinFailed", "界面就绪后台任务异常"))?
    .map_err(UiError::from_string)
}

fn get_main_projection_blocking(lifecycle: &DesktopLifecycle) -> UiResult<MainProjection> {
    let control_plane = lifecycle.control_plane_snapshot();
    let runtime = ready_section_value(&control_plane.runtime);
    let authority = ready_section_value(&control_plane.authority);
    let settings = ready_section_value(&control_plane.settings);
    let workspace = ready_section_value(&control_plane.workspace);
    let task_aggregate = ready_section_value(&control_plane.activity);
    let projects = settings.map(|settings| {
        settings
            .projects
            .iter()
            .map(|project| ProjectProjection {
                id: project.id.clone(),
                path: project
                    .accessible_path
                    .clone()
                    .unwrap_or_else(|| "项目已无法访问".to_string()),
                active: project.active,
            })
            .collect::<Vec<_>>()
    });
    let workspace_projection = workspace.map(|workspace| UiWorkspaceProjection {
        desired_path: workspace.desired_path.clone(),
        observed_path: workspace.observed_path.clone(),
        effective: effective_availability_code(workspace.effective),
    });
    let connection_projection =
        ready_section_value(&control_plane.connection).map(|connection| UiConnectionProjection {
            desired_tunnel_id: connection.desired_tunnel_id.clone(),
            observed_tunnel_id: connection.observed_tunnel_id.clone(),
            effective: effective_availability_code(connection.effective),
        });
    let runtime_state = runtime.map(|runtime| &runtime.state);
    let (tunnel_service, coding_service) = runtime_state
        .map(service_codes)
        .map_or((None, None), |(tunnel, coding)| {
            (Some(tunnel), Some(coding))
        });
    let reconnect = runtime
        .and_then(|runtime| runtime.outage.as_ref())
        .and_then(|outage| {
            outage
                .user_attention_required
                .then_some(ReconnectProjection {
                    generation: outage.generation,
                })
        });
    Ok(MainProjection {
        authority_status: projection_section_code(&control_plane.authority),
        runtime_status: projection_section_code(&control_plane.runtime),
        settings_status: projection_section_code(&control_plane.settings),
        workspace_status: projection_section_code(&control_plane.workspace),
        connection_status: projection_section_code(&control_plane.connection),
        activity_status: projection_section_code(&control_plane.activity),
        update_status: projection_section_code(&control_plane.update),
        permission: authority.map(|authority| permission_code(authority.desired)),
        effective_permission: authority.map(|authority| permission_code(authority.effective)),
        permission_reconciliation: authority
            .map(|authority| authority_reconciliation_code(authority.reconciliation)),
        path_authority: authority.map(|authority| match authority.structured_paths {
            StructuredPathAuthority::ActiveWorkspace => "workspace",
            StructuredPathAuthority::AdministratorBroker => "administrator",
        }),
        privilege: authority.map(|authority| privilege_code(&authority.broker)),
        local_environment_service: runtime_state.map(local_environment_service_code),
        tunnel_service,
        coding_service,
        onboarding_ready: runtime.map(|_| control_plane.onboarding_readiness().all_ready()),
        workspace: workspace_projection,
        projects,
        current_task: task_aggregate.and_then(|aggregate| {
            task_projection_from_aggregate(
                aggregate,
                runtime.and_then(|runtime| runtime.current_task_elapsed_ms),
            )
        }),
        current_activity: task_aggregate.and_then(current_activity_projection),
        last_activity: task_aggregate.and_then(last_activity_projection),
        projection_revision: control_plane.revision,
        connection: connection_projection,
        runtime_key_saved: settings.map(|settings| settings.runtime_key_saved),
        auto_start: settings.map(|settings| settings.auto_start),
        close_window_continue_running: settings
            .map(|settings| settings.close_window_continue_running),
        reconnect,
        update: ready_section_value(&control_plane.update)
            .map(|update| update_projection(Some(update))),
        active_faults: ui_faults(&control_plane),
    })
}

fn effective_availability_code(availability: EffectiveAvailability) -> &'static str {
    match availability {
        EffectiveAvailability::Available => "available",
        EffectiveAvailability::Disabled => "disabled",
        EffectiveAvailability::Reconciling => "reconciling",
        EffectiveAvailability::Unavailable => "unavailable",
    }
}

fn projection_section_code<T>(section: &ProjectionSection<T>) -> &'static str {
    match section.availability() {
        ProjectionAvailability::Ready if !section.is_stale() && section.value().is_some() => {
            "ready"
        }
        ProjectionAvailability::Fault => "fault",
        ProjectionAvailability::TemporarilyUnavailable if section.is_stale() => "stale",
        ProjectionAvailability::Ready | ProjectionAvailability::TemporarilyUnavailable => {
            "unavailable"
        }
    }
}

fn ready_section_value<T>(section: &ProjectionSection<T>) -> Option<&T> {
    section.ready_value()
}

#[tauri::command]
pub async fn retry_update_check(app: AppHandle) -> UiResult<UpdateProjection> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        lifecycle
            .start_update_check(UpdateCheckTrigger::Manual)
            .map_err(update_start_error)?;
        let state = lifecycle.update_lifecycle();
        Ok(update_projection(Some(&state)))
    })
    .await
    .map_err(|_| UiError::internal("Update.JoinFailed", "更新检查后台任务异常"))?
}

#[tauri::command]
pub async fn open_github_releases(app: AppHandle) -> UiResult<OpenReleaseProjection> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        let repository = lifecycle.update_repository().ok_or_else(|| {
            UiError::from(OperationError::new(
                "Update.SourceUnavailable",
                ErrorCategory::Unavailable,
                "当前构建未包含 GitHub 发布源",
                false,
            ))
        })?;
        let projection = release_projection(&repository, &lifecycle.update_lifecycle())?;
        open_system_url(&projection.release_url)?;
        Ok(projection)
    })
    .await
    .map_err(|_| UiError::internal("Update.OpenJoinFailed", "打开发布页面后台任务异常"))?
}

#[tauri::command]
pub async fn set_permission_mode(mode: String, app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let lifecycle = app.state::<DesktopLifecycle>();
        let requested = parse_permission(&mode)?;
        if requested != PermissionMode::Elevated {
            reset_admin_consent();
        }
        let privilege_active =
            matches!(lifecycle.privilege().state(), PrivilegeState::Active { .. });
        if requested == PermissionMode::Elevated
            && !privilege_active
            && !consume_confirmed_admin_consent()
        {
            return Err(UiError::from("管理员确认尚未完成"));
        }
        let (store, mut data) = load_app_data(&app)?;
        data.settings.permission_mode = requested.into();
        // DesiredStateOwner is the single live permission owner. The settings
        // file is only its restart seed. A downgrade closes the privileged
        // execution surface before persistence, and neither result can skip
        // observing the other.
        let (downgrade, persistence) = apply_permission_transition(
            requested,
            || lifecycle.set_desired_permission(requested),
            || lifecycle.reconcile_permission_downgrade(),
            || store.save(&data),
        );
        if persistence.is_err() {
            lifecycle.publish_settings_fault(OperationError::new(
                "Settings.PermissionPersistenceFailed",
                ErrorCategory::Unavailable,
                "权限期望已生效，但无法持久化为下次启动设置",
                true,
            ));
        }
        downgrade.map_err(|fault| {
            UiError::from(OperationError::new(
                format!("Authority.{fault:?}"),
                ErrorCategory::Authorization,
                "管理员执行面未能完全关闭",
                true,
            ))
        })?;
        persistence.map_err(|_| UiError::from("无法保存权限设置"))?;
        refresh_settings_snapshot(&app, &lifecycle)?;
        reconcile_explicit_permission(&lifecycle)?;
        Ok(())
    })
    .await
    .map_err(|_| UiError::internal("Ui.PermissionJoinFailed", "权限设置后台任务异常"))?
    .map_err(UiError::from_string)
}

fn apply_permission_transition<E>(
    requested: PermissionMode,
    set_desired: impl FnOnce(),
    secure_downgrade: impl FnOnce() -> Result<(), PrivilegeFault>,
    persist: impl FnOnce() -> Result<(), E>,
) -> (Result<(), PrivilegeFault>, Result<(), E>) {
    set_desired();
    let downgrade = if requested == PermissionMode::Elevated {
        Ok(())
    } else {
        secure_downgrade()
    };
    let persistence = persist();
    (downgrade, persistence)
}

#[tauri::command]
pub async fn begin_admin_consent(challenge_id: String) -> UiResult<AdminConsentChallenge> {
    tauri::async_runtime::spawn_blocking(move || begin_admin_consent_challenge(&challenge_id))
        .await
        .map_err(|_| {
            UiError::internal("Ui.AdminConsentBeginJoinFailed", "管理员确认后台任务异常")
        })?
}

#[tauri::command]
pub async fn cancel_admin_consent(challenge_id: String) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || cancel_admin_consent_challenge(&challenge_id))
        .await
        .map_err(|_| {
            UiError::internal("Ui.AdminConsentCancelJoinFailed", "管理员确认后台任务异常")
        })?
}

#[tauri::command]
pub async fn confirm_admin_consent(challenge_id: String) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || confirm_admin_consent_challenge(&challenge_id))
        .await
        .map_err(|_| {
            UiError::internal("Ui.AdminConsentConfirmJoinFailed", "管理员确认后台任务异常")
        })?
}

fn reconcile_explicit_permission(lifecycle: &DesktopLifecycle) -> UiResult<()> {
    let executable = std::env::current_exe().map_err(|_| {
        OperationError::new(
            "Authority.ExecutableUnavailable",
            ErrorCategory::Unavailable,
            "Unable to locate the LocalBridge executable",
            true,
        )
    })?;
    let broker = executable
        .parent()
        .map(|parent| parent.join("localbridge-privileged-broker.exe"))
        .ok_or_else(|| {
            OperationError::new(
                "Authority.BrokerUnavailable",
                ErrorCategory::Unavailable,
                "Unable to locate the administrator service",
                true,
            )
        })?;
    lifecycle
        .reconcile_permission_from_explicit_action(&broker)
        .map_err(|fault| {
            OperationError::new(
                format!("Authority.{fault:?}"),
                ErrorCategory::Authorization,
                "Administrator authorization did not complete",
                true,
            )
            .into()
        })
}

#[tauri::command]
pub async fn set_auto_start(enabled: bool, app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let (store, mut data) = load_app_data(&app)?;
        let previous = data.settings.auto_start_services;
        if previous == enabled {
            return Ok(());
        }
        let manager = AutostartManager::for_current_executable()
            .map_err(|_| "无法读取开机启动设置".to_string())?;
        manager
            .set_enabled(enabled)
            .map_err(|_| "无法更新开机启动设置".to_string())?;
        data.settings.auto_start_services = enabled;
        if store.save(&data).is_err() {
            let _ = manager.set_enabled(previous);
            return Err(UiError::from("无法保存开机启动设置"));
        }
        refresh_settings_snapshot(&app, &app.state::<DesktopLifecycle>())?;
        Ok(())
    })
    .await
    .map_err(|_| UiError::internal("Ui.AutostartJoinFailed", "开机启动后台任务异常"))?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn set_close_window_continue_running(enabled: bool, app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let lifecycle = app.state::<DesktopLifecycle>();
        let (store, mut data) = load_app_data(&app)?;
        data.settings.close_window_continue_running = enabled;
        store
            .save(&data)
            .map_err(|_| "无法保存常规设置".to_string())?;
        lifecycle.set_close_window_continue_running(enabled);
        refresh_settings_snapshot(&app, &lifecycle)?;
        Ok(())
    })
    .await
    .map_err(|_| UiError::internal("Ui.SettingsJoinFailed", "常规设置后台任务异常"))?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn save_runtime_key(value: String, app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let secret =
            SecretString::new(value).map_err(|_| "Runtime API Key 格式无效".to_string())?;
        let credentials = WindowsCredentialStore::default();
        let unchanged = credentials
            .read_runtime_api_key()
            .map_err(|_| "无法读取 Runtime API Key 状态".to_string())?
            .as_ref()
            .is_some_and(|current| current.expose_secret() == secret.expose_secret());
        if unchanged {
            return Ok(());
        }
        credentials
            .save_runtime_api_key(&secret)
            .map_err(|_| "无法安全保存 Runtime API Key".to_string())?;
        let lifecycle = app.state::<DesktopLifecycle>();
        lifecycle.mark_connection_credentials_changed();
        refresh_settings_snapshot(&app, &lifecycle)?;
        reconnect_after_connection_change(&app, &lifecycle)?;
        Ok(())
    })
    .await
    .map_err(|_| {
        UiError::internal(
            "Ui.RuntimeKeySaveJoinFailed",
            "Runtime API Key 保存后台任务异常",
        )
    })?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn save_tunnel_id(value: String, app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let requested =
            TunnelId::new(value.trim().to_owned()).map_err(|_| "Tunnel ID 格式无效".to_string())?;
        let app_data = app_data_dir(&app)?;
        let store = StartupProfileStore::new(app_data.join(STARTUP_PROFILE_FILE_NAME));
        let mut profile = store.load().map_err(|_| "无法读取连接设置".to_string())?;
        if profile
            .validated_tunnel_id()
            .map_err(|_| "已有 Tunnel ID 无效".to_string())?
            .as_ref()
            .is_some_and(|old| old.expose() == requested.expose())
        {
            return Ok(());
        }
        profile
            .set_tunnel_id(requested.expose().to_owned())
            .map_err(|_| "Tunnel ID 格式无效".to_string())?;
        store
            .save(&profile)
            .map_err(|_| "无法保存 Tunnel ID".to_string())?;
        let lifecycle = app.state::<DesktopLifecycle>();
        let epoch = lifecycle
            .desired_state()
            .snapshot()
            .state
            .connection
            .as_ref()
            .map(|profile| profile.credential_epoch)
            .unwrap_or(0);
        lifecycle.set_desired_connection(Some(ConnectionProfile::new(requested, epoch)));
        reconnect_after_connection_change(&app, &lifecycle)?;
        Ok(())
    })
    .await
    .map_err(|_| UiError::internal("Ui.TunnelSaveJoinFailed", "Tunnel ID 保存后台任务异常"))?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn delete_runtime_key(app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let deleted = WindowsCredentialStore::default()
            .delete_runtime_api_key()
            .map_err(|_| "无法删除Runtime API Key".to_string())?;
        if deleted {
            let lifecycle = app.state::<DesktopLifecycle>();
            lifecycle.mark_connection_credentials_changed();
            refresh_settings_snapshot(&app, &lifecycle)?;
            reconnect_after_connection_change(&app, &lifecycle)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| {
        UiError::internal(
            "Ui.RuntimeKeyDeleteJoinFailed",
            "Runtime API Key 删除后台任务异常",
        )
    })?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn retry_connection(app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let lifecycle = app.state::<DesktopLifecycle>();
        lifecycle
            .manual_retry_after_attention()
            .map_err(|_| "当前连接无法重试".to_string())?;
        Ok(())
    })
    .await
    .map_err(|_| UiError::internal("Ui.ConnectionRetryJoinFailed", "连接重试后台任务异常"))?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn add_project(
    path: String,
    defer_activation: Option<bool>,
    app: AppHandle,
) -> UiResult<String> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<String> {
        let lifecycle = app.state::<DesktopLifecycle>();
        add_project_blocking(path, defer_activation, &app, &lifecycle)
    })
    .await
    .map_err(|_| UiError::internal("Ui.ProjectAddJoinFailed", "项目添加后台任务异常"))?
    .map_err(UiError::from_string)
}

pub(crate) fn add_project_blocking(
    path: String,
    defer_activation: Option<bool>,
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
) -> UiResult<String> {
    let candidate_path = PathBuf::from(path);
    let validated = WorkspaceValidator
        .validate(&candidate_path)
        .map_err(|_| "所选项目无法验证".to_string())?;
    let (store, mut data) = load_app_data(app)?;
    let generated = WorkspaceId::from_validated(new_workspace_id())
        .map_err(|_| "无法创建项目记录".to_string())?;
    let id = data
        .workspace
        .registry
        .upsert_validated(
            generated,
            validated.execution_path(),
            &validated,
            unix_seconds(),
        )
        .map_err(|_| "无法保存项目记录".to_string())?;
    let id_value = id.as_str().to_owned();
    if defer_activation.unwrap_or(false) {
        store
            .save(&data)
            .map_err(|_| "无法保存项目记录".to_string())?;
        refresh_settings_snapshot(app, lifecycle)?;
        return Ok(id_value);
    }
    activate_project(
        app,
        lifecycle,
        &store,
        &mut data,
        id,
        validated.execution_path(),
    )?;
    refresh_settings_snapshot(app, lifecycle)?;
    Ok(id_value)
}

#[tauri::command]
pub async fn select_project(id: String, app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let lifecycle = app.state::<DesktopLifecycle>();
        select_project_blocking(id, &app, &lifecycle)
    })
    .await
    .map_err(|_| UiError::internal("Ui.ProjectSelectJoinFailed", "项目切换后台任务异常"))?
    .map_err(UiError::from_string)
}

pub(crate) fn select_project_blocking(
    id: String,
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
) -> UiResult<()> {
    let id = WorkspaceId::from_validated(id).map_err(|_| "项目不存在".to_string())?;
    let (store, mut data) = load_app_data(app)?;
    let entry = data
        .workspace
        .registry
        .get(&id)
        .cloned()
        .ok_or_else(|| "项目不存在".to_string())?;
    let validated = WorkspaceValidator
        .validate(&entry.display_path)
        .map_err(|_| "项目已无法访问".to_string())?;
    if entry.validated_identity.as_str() != validated.identity().as_str() {
        return Err(UiError::from("项目身份已变化，请重新添加"));
    }
    activate_project(
        app,
        lifecycle,
        &store,
        &mut data,
        id,
        validated.execution_path(),
    )?;
    refresh_settings_snapshot(app, lifecycle)
}

#[tauri::command]
pub async fn remove_project(id: String, app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let lifecycle = app.state::<DesktopLifecycle>();
        remove_project_blocking(id, &app, &lifecycle)
    })
    .await
    .map_err(|_| UiError::internal("Ui.ProjectRemoveJoinFailed", "项目移除后台任务异常"))?
    .map_err(UiError::from_string)
}

fn remove_project_blocking(
    id: String,
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
) -> UiResult<()> {
    let id = WorkspaceId::from_validated(id).map_err(|_| "项目不存在".to_string())?;
    let (store, mut data) = load_app_data(app)?;
    if data.workspace.registry.get(&id).is_none() {
        return Err(UiError::from("项目不存在"));
    }
    let was_active = data.workspace.active_workspace_id.as_ref() == Some(&id);
    if was_active {
        data.workspace.clear_active();
    }
    let _ = data.workspace.registry.remove(&id);
    store
        .save(&data)
        .map_err(|_| "无法保存项目变更".to_string())?;
    if was_active {
        lifecycle.set_desired_workspace(None);
        lifecycle.set_desired_services(ServiceIntent::Disabled);
        lifecycle
            .stop_runtime_for_control_plane()
            .map_err(|_| "项目移除目标已保存，但旧服务尚未完全停止".to_string())?;
    }
    refresh_settings_snapshot(app, lifecycle)?;
    Ok(())
}

fn activate_project(
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
    store: &SettingsStore,
    data: &mut AppData,
    id: WorkspaceId,
    candidate: &Path,
) -> UiResult<()> {
    clear_manual_stop_for_explicit_action(app)?;
    data.workspace
        .set_active_reference(id.clone())
        .map_err(|_| "无法设置当前项目".to_string())?;
    store
        .save(data)
        .map_err(|_| "无法保存当前项目".to_string())?;
    lifecycle.set_desired_workspace(Some(DesiredWorkspace::new(id, candidate)));
    lifecycle.set_desired_services(ServiceIntent::Enabled);
    lifecycle
        .reconcile_runtime_from_desired_state(|| production_runtime_config_for_path(app, candidate))
        .map_err(|error| runtime_reconciliation_message(error, "项目目标已保存"))
}

fn production_runtime_config_for_path(
    app: &AppHandle,
    path: &Path,
) -> UiResult<ProductionRuntimeConfig> {
    let app_data = app_data_dir(app)?;
    let profile = StartupProfileStore::new(app_data.join(STARTUP_PROFILE_FILE_NAME))
        .load()
        .map_err(|_| "无法读取连接设置".to_string())?;
    let tunnel_id: TunnelId = profile
        .validated_tunnel_id()
        .map_err(|_| "Tunnel ID 无效".to_string())?
        .ok_or_else(|| "尚未配置 Tunnel ID".to_string())?;
    Ok(ProductionRuntimeConfig::new(
        production_install_root()?,
        path,
        app_data.join("health"),
        tunnel_id,
    ))
}

fn production_runtime_config_for_active_workspace(
    app: &AppHandle,
    data: &AppData,
) -> UiResult<ProductionRuntimeConfig> {
    let entry = data
        .workspace
        .active_entry()
        .ok_or_else(|| "尚未选择项目".to_string())?;
    let validated = WorkspaceValidator
        .validate(&entry.display_path)
        .map_err(|_| "当前项目已无法访问".to_string())?;
    if entry.validated_identity.as_str() != validated.identity().as_str() {
        return Err(UiError::from("项目身份已变化，请重新添加"));
    }
    production_runtime_config_for_path(app, validated.execution_path())
}

fn reconnect_after_connection_change(
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
) -> UiResult<()> {
    lifecycle
        .reconcile_runtime_from_desired_state(|| {
            let (_, data) = load_app_data(app)?;
            production_runtime_config_for_active_workspace(app, &data)
        })
        .map_err(|error| runtime_reconciliation_message(error, "连接设置已保存"))
}

fn runtime_reconciliation_message(
    error: DesktopRuntimeReconcileError<UiError>,
    context: &str,
) -> UiError {
    match error {
        DesktopRuntimeReconcileError::Configuration(error) => error,
        DesktopRuntimeReconcileError::Start(error) => OperationError::new(
            "Runtime.ReconcileStartFailed",
            ErrorCategory::Unavailable,
            format!("{context}，{}", runtime_start_message(error)),
            true,
        )
        .into(),
        DesktopRuntimeReconcileError::Control(_) => OperationError::new(
            "Runtime.ReconcileControlFailed",
            ErrorCategory::Unavailable,
            format!("{context}，运行服务仍在收敛"),
            true,
        )
        .into(),
        DesktopRuntimeReconcileError::WaitingForObservation => OperationError::new(
            "Runtime.Reconciling",
            ErrorCategory::Conflict,
            format!("{context}，运行服务正在收敛"),
            true,
        )
        .into(),
    }
}

#[tauri::command]
pub async fn restart_services(app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        clear_manual_stop_for_explicit_action(&app)?;
        let lifecycle = app.state::<DesktopLifecycle>();
        lifecycle.set_desired_services(ServiceIntent::Enabled);
        let (_, data) = load_app_data(&app)?;
        let config = production_runtime_config_for_active_workspace(&app, &data)?;
        Ok(lifecycle
            .backend_handle()
            .restart_production_runtime(config)
            .map_err(|_| "无法重启服务".to_string())?)
    })
    .await
    .map_err(|_| UiError::internal("Ui.ServicesRestartJoinFailed", "服务重启后台任务异常"))?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn stop_services(app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let app_data = app_data_dir(&app)?;
        let lifecycle = app.state::<DesktopLifecycle>();
        let report =
            manual_stop_services(&app_data, &lifecycle).map_err(|_| "无法关闭服务".to_string())?;
        if report.tunnel_stop_failed
            || report.privilege_stop_failed
            || report.lower_runtime_stop_failed
        {
            return Err(UiError::from("服务关闭不完整，请查看诊断"));
        }
        Ok(())
    })
    .await
    .map_err(|_| UiError::internal("Ui.ServicesStopJoinFailed", "服务关闭后台任务异常"))?
    .map_err(UiError::from_string)
}

fn runtime_start_message(error: DesktopRuntimeStartError) -> String {
    match error {
        DesktopRuntimeStartError::AlreadyRegistered => {
            "本地编码服务已在运行，请重试项目激活".to_string()
        }
        DesktopRuntimeStartError::Runtime(error) => runtime_fault_message(&error.fault).to_string(),
    }
}

fn runtime_fault_message(fault: &RuntimeFault) -> &'static str {
    match fault {
        RuntimeFault::WorkspaceMissing | RuntimeFault::WorkspaceInvalid => {
            "项目目录不可用，请返回项目与权限页面重新选择"
        }
        RuntimeFault::RuntimeMissing | RuntimeFault::RuntimeChecksumMismatch => {
            "本地运行环境缺失或损坏，请重新安装 LocalBridge"
        }
        RuntimeFault::ProcessOwnershipFailed => {
            "本地服务进程无法安全启动，请重启 LocalBridge 后重试"
        }
        RuntimeFault::McpSpawnFailed | RuntimeFault::McpHealthTimeout | RuntimeFault::McpExited => {
            "编码服务启动失败，请重试"
        }
        RuntimeFault::PolicyBindFailed
        | RuntimeFault::PolicyInvalid
        | RuntimeFault::PolicyCapabilityUnknown => "本地安全策略服务启动失败，请重试",
        RuntimeFault::TunnelIdMissing => "尚未配置 Tunnel ID，请返回 OpenAI 页面重新保存",
        RuntimeFault::RuntimeKeyMissing => "Runtime API Key未配置，请返回 OpenAI 页面重新保存",
        RuntimeFault::SecretStoreFailed => "无法读取 Windows 安全凭据中的Runtime API Key",
        RuntimeFault::SecretInjectionUnsupported => {
            "Runtime API Key无法安全注入 Tunnel，请重新安装 LocalBridge"
        }
        RuntimeFault::TunnelAuthFailed => {
            "OpenAI Tunnel 鉴权失败，请检查Runtime API Key与 Tunnel 权限"
        }
        RuntimeFault::TunnelSpawnFailed => "OpenAI Tunnel 进程启动失败，请重试",
        RuntimeFault::TunnelHealthTimeout | RuntimeFault::TunnelExited => {
            "OpenAI Tunnel 暂时无法连接，请检查网络后重试"
        }
        RuntimeFault::PortUnavailable => "本地服务端口暂时不可用，请关闭冲突程序后重试",
        RuntimeFault::ConfigurationInvalid => "OpenAI Tunnel 配置无效，请检查 Tunnel ID 与连接设置",
        RuntimeFault::UserStopped => "本地服务已停止，请重试",
        RuntimeFault::Unknown => "本地服务启动失败，请重试",
    }
}

fn clear_manual_stop_for_explicit_action(app: &AppHandle) -> UiResult<()> {
    let app_data = app_data_dir(app)?;
    let store = StartupProfileStore::new(app_data.join(STARTUP_PROFILE_FILE_NAME));
    let mut profile = store.load().map_err(|_| "无法读取连接设置".to_string())?;
    if profile.manual_stop_latched() {
        profile.clear_manual_stop();
        store
            .save(&profile)
            .map_err(|_| "无法保存连接设置".to_string())?;
    }
    Ok(())
}

fn load_app_data(app: &AppHandle) -> UiResult<(SettingsStore, AppData)> {
    let store = SettingsStore::new(app_data_dir(app)?.join("settings.json"));
    let data = store.load().map_err(|_| "无法读取设置".to_string())?;
    Ok((store, data))
}

pub(crate) fn refresh_settings_snapshot(
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
) -> UiResult<()> {
    let (_, data) = load_app_data(app)?;
    let (runtime_key_saved, runtime_key_length, error) =
        match WindowsCredentialStore::default().read_runtime_api_key() {
            Ok(secret) => (
                secret.is_some(),
                secret
                    .as_ref()
                    .map(|secret| secret.expose_secret().chars().count()),
                None,
            ),
            Err(_) => (
                false,
                None,
                Some(crate::domain::OperationError::new(
                    "Settings.CredentialMetadataUnavailable",
                    crate::domain::ErrorCategory::Unavailable,
                    "Runtime credential metadata is unavailable",
                    true,
                )),
            ),
        };
    lifecycle.publish_settings_snapshot(
        crate::control_plane::snapshot::SettingsProjection::from_app_data(
            &data,
            runtime_key_saved,
            runtime_key_length,
        ),
        error,
    );
    Ok(())
}

fn app_data_dir(app: &AppHandle) -> UiResult<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|_| "无法定位应用数据目录".to_string())?)
}

fn production_install_root() -> UiResult<PathBuf> {
    #[cfg(debug_assertions)]
    {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "无法定位本地运行环境".to_string())?)
    }
    #[cfg(not(debug_assertions))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .ok_or_else(|| "无法定位本地运行环境".to_string())
            .map_err(UiError::from_string)
    }
}

fn new_workspace_id() -> String {
    format!("ui-{}-{}", std::process::id(), unix_nanos())
}
fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn parse_permission(value: &str) -> UiResult<PermissionMode> {
    match value {
        "edit" => Ok(PermissionMode::Edit),
        "full" => Ok(PermissionMode::Full),
        "admin" => Ok(PermissionMode::Elevated),
        _ => Err(UiError::from("权限模式无效")),
    }
}
fn permission_code(value: PermissionMode) -> &'static str {
    match value {
        PermissionMode::Edit => "edit",
        PermissionMode::Full => "full",
        PermissionMode::Elevated => "admin",
    }
}
fn privilege_code(value: &PrivilegeState) -> &'static str {
    match value {
        PrivilegeState::Disabled => "off",
        PrivilegeState::Requested => "requested",
        PrivilegeState::AwaitingUac => "awaiting",
        PrivilegeState::Active { .. } => "active",
        PrivilegeState::Faulted(_) => "fault",
    }
}

fn authority_reconciliation_code(value: AuthorityReconciliation) -> &'static str {
    match value {
        AuthorityReconciliation::Converged => "converged",
        AuthorityReconciliation::AuthorizationRequired => "authorization_required",
        AuthorityReconciliation::AwaitingAuthorization => "awaiting_authorization",
        AuthorityReconciliation::BrokerUnavailable => "broker_unavailable",
        AuthorityReconciliation::DisablePending => "disable_pending",
    }
}
fn service_codes(state: &RuntimeState) -> (&'static str, &'static str) {
    match state {
        RuntimeState::Stopped => ("off", "off"),
        RuntimeState::StartingMcp
        | RuntimeState::WaitingMcpReady
        | RuntimeState::StartingPolicyEnforcement
        | RuntimeState::WaitingPolicyReady => ("off", "starting"),
        RuntimeState::StartingTunnel | RuntimeState::WaitingTunnelReady => ("starting", "online"),
        RuntimeState::Ready => ("online", "online"),
        RuntimeState::Recovering { component, .. } => match component {
            RuntimeComponent::Tunnel => ("recovering", "online"),
            RuntimeComponent::PolicyEnforcement | RuntimeComponent::CodingRuntime => {
                ("recovering", "recovering")
            }
        },
        RuntimeState::SwitchingWorkspace { .. } => ("recovering", "recovering"),
        RuntimeState::Faulted(_) => ("fault", "fault"),
    }
}
fn local_environment_service_code(state: &RuntimeState) -> &'static str {
    match state {
        RuntimeState::Stopped => "off",
        RuntimeState::StartingMcp | RuntimeState::WaitingMcpReady => "starting",
        RuntimeState::StartingPolicyEnforcement
        | RuntimeState::WaitingPolicyReady
        | RuntimeState::StartingTunnel
        | RuntimeState::WaitingTunnelReady
        | RuntimeState::Ready => "online",
        RuntimeState::Recovering { component, .. } => match component {
            RuntimeComponent::CodingRuntime => "recovering",
            RuntimeComponent::PolicyEnforcement | RuntimeComponent::Tunnel => "online",
        },
        RuntimeState::SwitchingWorkspace { .. } => "recovering",
        RuntimeState::Faulted(_) => "fault",
    }
}
fn current_activity_projection(aggregate: &TaskAggregate) -> Option<CurrentActivityProjection> {
    if let Some(task) = aggregate.foreground_task.as_ref() {
        return Some(CurrentActivityProjection {
            kind: task_kind_code(task.kind),
            state: match task.lifecycle {
                LifecycleState::Queued => "waiting",
                LifecycleState::Running => "running",
                LifecycleState::Terminal(_) => return None,
            },
            summary: task.summary.as_deref().map(str::to_owned),
            elapsed_ms: Some(now_unix_ms().saturating_sub(task.created_at_ms)),
            step: None,
            progress_current: None,
            progress_total: None,
        });
    }
    aggregate.detached_execution.as_ref().and_then(|execution| {
        matches!(execution.state, ExecutionState::Running).then_some(CurrentActivityProjection {
            kind: "command",
            state: "running",
            summary: None,
            elapsed_ms: Some(now_unix_ms().saturating_sub(execution.started_at_ms)),
            step: None,
            progress_current: None,
            progress_total: None,
        })
    })
}

fn last_activity_projection(aggregate: &TaskAggregate) -> Option<LastActivityProjection> {
    let task = aggregate.last_task.as_ref().and_then(|task| {
        let LifecycleState::Terminal(outcome) = task.lifecycle else {
            return None;
        };
        Some(LastActivityProjection {
            kind: task_kind_code(task.kind),
            summary: task.summary.as_deref().map(str::to_owned),
            outcome: terminal_outcome_code(outcome),
            completed_at_ms: task.updated_at_ms,
        })
    });
    let execution = aggregate.last_execution.as_ref().and_then(|execution| {
        let ExecutionState::Terminal(terminal) = &execution.state else {
            return None;
        };
        Some(LastActivityProjection {
            kind: "command",
            summary: aggregate
                .last_task
                .as_ref()
                .filter(|task| task.id == execution.task_id)
                .and_then(|task| task.summary.as_deref())
                .map(str::to_owned),
            outcome: terminal_outcome_code(terminal.outcome),
            completed_at_ms: terminal.completed_at_ms,
        })
    });
    match (task, execution) {
        (Some(task), Some(execution)) if execution.completed_at_ms >= task.completed_at_ms => {
            Some(execution)
        }
        (Some(task), _) => Some(task),
        (None, execution) => execution,
    }
}

fn task_projection_from_aggregate(
    aggregate: &TaskAggregate,
    elapsed_ms: Option<u64>,
) -> Option<TaskProjection> {
    if let Some(task) = aggregate.foreground_task.as_ref() {
        return Some(TaskProjection {
            kind: task_kind_code(task.kind),
            summary: task.summary.as_deref().map(str::to_owned),
            state: match task.lifecycle {
                LifecycleState::Queued => "waiting",
                LifecycleState::Running => "running",
                LifecycleState::Terminal(TerminalOutcome::Blocked) => "blocked",
                LifecycleState::Terminal(TerminalOutcome::Cancelled) => "cancelled",
                LifecycleState::Terminal(_) => "failed",
            },
            elapsed_ms,
        });
    }
    aggregate.detached_execution.as_ref().and_then(|execution| {
        matches!(execution.state, ExecutionState::Running).then_some(TaskProjection {
            kind: "command",
            summary: None,
            state: "running",
            elapsed_ms: Some(now_unix_ms().saturating_sub(execution.started_at_ms)),
        })
    })
}

fn terminal_outcome_code(outcome: TerminalOutcome) -> &'static str {
    match outcome {
        TerminalOutcome::Completed => "completed",
        TerminalOutcome::Failed => "failed",
        TerminalOutcome::Blocked => "blocked",
        TerminalOutcome::Cancelled => "cancelled",
        TerminalOutcome::TimedOut => "timed_out",
        TerminalOutcome::Lost => "lost",
    }
}

fn ui_faults(snapshot: &ControlPlaneSnapshot) -> Vec<UiFaultProjection> {
    snapshot
        .active_faults
        .iter()
        .map(|fault| UiFaultProjection {
            code: fault.error.code.clone(),
            category: match fault.error.category {
                crate::domain::ErrorCategory::Validation => "validation",
                crate::domain::ErrorCategory::Authorization => "authorization",
                crate::domain::ErrorCategory::Capacity => "capacity",
                crate::domain::ErrorCategory::Conflict => "conflict",
                crate::domain::ErrorCategory::Timeout => "timeout",
                crate::domain::ErrorCategory::Unavailable => "unavailable",
                crate::domain::ErrorCategory::Internal => "internal",
            },
            message: fault.error.message.clone(),
            retryable: fault.error.retryable,
        })
        .collect()
}

fn update_projection(state: Option<&UpdateLifecycle>) -> UpdateProjection {
    let Some(state) = state else {
        return UpdateProjection {
            state: "source_unavailable",
            current_version: env!("CARGO_PKG_VERSION").to_owned(),
            latest_version: None,
            release_url: None,
            operation_id: None,
            attempt: None,
            retryable: false,
        };
    };
    match state {
        UpdateLifecycle::SourceUnavailable {
            current_version, ..
        } => UpdateProjection {
            state: "source_unavailable",
            current_version: current_version.to_string(),
            latest_version: None,
            release_url: None,
            operation_id: None,
            attempt: None,
            retryable: false,
        },
        UpdateLifecycle::Idle {
            current_version,
            releases_url,
        } => UpdateProjection {
            state: "idle",
            current_version: current_version.to_string(),
            latest_version: None,
            release_url: Some(releases_url.clone()),
            operation_id: None,
            attempt: None,
            retryable: true,
        },
        UpdateLifecycle::Checking {
            current_version,
            releases_url,
            operation_id,
            attempt,
            ..
        } => UpdateProjection {
            state: "checking",
            current_version: current_version.to_string(),
            latest_version: None,
            release_url: Some(releases_url.clone()),
            operation_id: Some(operation_id.clone()),
            attempt: Some(*attempt),
            retryable: false,
        },
        UpdateLifecycle::Current {
            current_version,
            releases_url,
            operation_id,
            ..
        } => UpdateProjection {
            state: "current",
            current_version: current_version.to_string(),
            latest_version: None,
            release_url: Some(releases_url.clone()),
            operation_id: Some(operation_id.clone()),
            attempt: None,
            retryable: true,
        },
        UpdateLifecycle::Available {
            current_version,
            latest_version,
            release_url,
            operation_id,
            ..
        } => UpdateProjection {
            state: "available",
            current_version: current_version.to_string(),
            latest_version: Some(latest_version.to_string()),
            release_url: Some(release_url.clone()),
            operation_id: Some(operation_id.clone()),
            attempt: None,
            retryable: true,
        },
        UpdateLifecycle::Failed {
            current_version,
            releases_url,
            operation_id,
            attempts,
            error,
            ..
        } => UpdateProjection {
            state: "failed",
            current_version: current_version.to_string(),
            latest_version: None,
            release_url: Some(releases_url.clone()),
            operation_id: Some(operation_id.clone()),
            attempt: Some(*attempts),
            retryable: error.retryable,
        },
    }
}

fn release_projection(
    repository: &crate::domain::GitHubRepository,
    lifecycle: &UpdateLifecycle,
) -> UiResult<OpenReleaseProjection> {
    let release_url = lifecycle
        .release_url()
        .map(str::to_owned)
        .unwrap_or_else(|| repository.releases_url());
    if !repository.owns_release_url(&release_url) {
        return Err(UiError::from(OperationError::new(
            "Update.ReleaseLinkDenied",
            ErrorCategory::Authorization,
            "发布页面不属于当前构建的 GitHub 仓库",
            false,
        )));
    }
    Ok(OpenReleaseProjection { release_url })
}

fn update_start_error(error: UpdateStartError) -> UiError {
    let operation = match error {
        UpdateStartError::SourceUnavailable => OperationError::new(
            "Update.SourceUnavailable",
            ErrorCategory::Unavailable,
            "当前构建未包含 GitHub 发布源",
            false,
        ),
        UpdateStartError::AlreadyChecking => OperationError::new(
            "Update.AlreadyChecking",
            ErrorCategory::Conflict,
            "更新检查正在进行",
            false,
        ),
        UpdateStartError::ThreadSpawnFailed => OperationError::new(
            "Update.StartFailed",
            ErrorCategory::Unavailable,
            "无法启动更新检查",
            true,
        ),
    };
    UiError::from(operation)
}

fn open_system_url(url: &str) -> UiResult<()> {
    let operation = wide("open");
    let target = wide(url);
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result as isize <= 32 {
        return Err(UiError::from(OperationError::new(
            "Update.OpenFailed",
            ErrorCategory::Unavailable,
            "无法使用系统浏览器打开发布页面",
            true,
        )));
    }
    Ok(())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
fn task_projection(status: &CurrentTaskStatus, elapsed_ms: Option<u64>) -> Option<TaskProjection> {
    let CurrentTaskStatus::Active(task) = status else {
        return None;
    };
    Some(TaskProjection {
        kind: task_kind_code(task.kind),
        summary: task.summary.as_deref().map(str::to_owned),
        state: task_state_code(task.state),
        elapsed_ms,
    })
}
fn task_kind_code(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::ReadFile => "read",
        TaskKind::SearchCode => "search",
        TaskKind::ModifyFile => "modify",
        TaskKind::ExecuteCommand => "command",
        TaskKind::GitOperation => "git",
        TaskKind::Build => "build",
        TaskKind::Test => "test",
        TaskKind::ElevatedOperation => "admin",
        TaskKind::Other => "other",
    }
}
#[cfg(test)]
fn task_state_code(state: TaskExecutionState) -> &'static str {
    match state {
        TaskExecutionState::Idle => "idle",
        TaskExecutionState::Running => "running",
        TaskExecutionState::AwaitingAuthorization => "waiting",
        TaskExecutionState::Blocked => "blocked",
        TaskExecutionState::Failed => "failed",
        TaskExecutionState::Cancelled => "cancelled",
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/unit/ui/backend_projection.rs"
    ));

    #[test]
    fn admin_consent_gate_rejects_early_and_consumes_exact_boundary() {
        let start = Instant::now();
        let mut gate = AdminConsentGate::default();
        gate.begin_at("challenge-a-0001", start);
        assert!(!gate.confirm_at(
            "challenge-a-0001",
            start + ADMIN_CONSENT_DURATION - Duration::from_millis(1)
        ));
        assert!(gate.confirm_at("challenge-a-0001", start + ADMIN_CONSENT_DURATION));
        assert!(gate.consume_confirmed());
        assert!(!gate.consume_confirmed());
    }

    #[test]
    fn admin_consent_gate_fresh_begin_resets_full_deadline() {
        let start = Instant::now();
        let mut gate = AdminConsentGate::default();
        gate.begin_at("challenge-a-0001", start);
        let restarted_at = start + Duration::from_millis(500);
        gate.begin_at("challenge-b-0002", restarted_at);
        assert!(!gate.confirm_at(
            "challenge-b-0002",
            restarted_at + ADMIN_CONSENT_DURATION - Duration::from_millis(1)
        ));
        assert!(gate.confirm_at("challenge-b-0002", restarted_at + ADMIN_CONSENT_DURATION));
        assert!(gate.consume_confirmed());
    }

    #[test]
    fn admin_consent_gate_stale_identity_cannot_confirm_or_cancel_newer_challenge() {
        let start = Instant::now();
        let mut gate = AdminConsentGate::default();
        gate.begin_at("challenge-a-0001", start);
        gate.begin_at("challenge-b-0002", start + Duration::from_millis(500));
        let ready = start + Duration::from_millis(500) + ADMIN_CONSENT_DURATION;
        assert!(!gate.confirm_at("challenge-a-0001", ready));
        assert!(!gate.cancel("challenge-a-0001"));
        assert!(!gate.consume_confirmed());
        assert!(gate.confirm_at("challenge-b-0002", ready));
        assert!(gate.consume_confirmed());
        assert!(!gate.confirm_at("challenge-b-0002", ready));
    }

    #[test]
    fn admin_consent_gate_matching_cancel_invalidates_challenge() {
        let start = Instant::now();
        let mut gate = AdminConsentGate::default();
        gate.begin_at("challenge-a-0001", start);
        assert!(gate.cancel("challenge-a-0001"));
        assert!(!gate.confirm_at("challenge-a-0001", start + ADMIN_CONSENT_DURATION));
        assert!(!gate.consume_confirmed());
    }

    #[test]
    fn permission_downgrade_closes_privileged_surface_before_failed_persistence() {
        let events = RefCell::new(Vec::new());
        let (downgrade, persistence) = apply_permission_transition(
            PermissionMode::Full,
            || events.borrow_mut().push("desired"),
            || {
                events.borrow_mut().push("broker_disabled");
                Ok(())
            },
            || {
                events.borrow_mut().push("persistence_attempted");
                Err("disk_full")
            },
        );
        assert_eq!(downgrade, Ok(()));
        assert_eq!(persistence, Err("disk_full"));
        assert_eq!(
            events.into_inner(),
            ["desired", "broker_disabled", "persistence_attempted"]
        );
    }
    #[test]
    fn runtime_start_fault_messages_are_redacted_and_actionable() {
        assert_eq!(
            runtime_fault_message(&RuntimeFault::RuntimeKeyMissing),
            "Runtime API Key未配置，请返回 OpenAI 页面重新保存"
        );
        assert_eq!(
            runtime_fault_message(&RuntimeFault::TunnelAuthFailed),
            "OpenAI Tunnel 鉴权失败，请检查Runtime API Key与 Tunnel 权限"
        );
        assert_eq!(
            runtime_fault_message(&RuntimeFault::ConfigurationInvalid),
            "OpenAI Tunnel 配置无效，请检查 Tunnel ID 与连接设置"
        );
        for fault in [
            RuntimeFault::RuntimeChecksumMismatch,
            RuntimeFault::McpSpawnFailed,
            RuntimeFault::TunnelHealthTimeout,
            RuntimeFault::PolicyInvalid,
        ] {
            let message = runtime_fault_message(&fault);
            assert!(!message.contains("RuntimeFault"));
            assert!(!message.contains("OrchestratorError"));
            assert!(!message.contains("synthetic-secret"));
        }
    }
}
