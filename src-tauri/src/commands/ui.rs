use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::app::{
    AutostartManager, DesktopLifecycle, DesktopRuntimeStartError, STARTUP_PROFILE_FILE_NAME,
    StartupProfileStore, manual_stop_services,
};
use crate::credentials::{CredentialStore, SecretString, WindowsCredentialStore};
use crate::runtime::ProductionRuntimeConfig;
use crate::settings::{AppData, SettingsStore, StoredPermissionMode};
use crate::state::{
    LastToolTiming, PermissionMode, PrivilegeState, RuntimeComponent, RuntimeFault, RuntimeState, TaskKind,
};
#[cfg(test)]
use crate::state::{CurrentTaskStatus, TaskExecutionState};
use crate::tunnel::TunnelId;
use crate::workspace::{WorkspaceId, WorkspaceValidator};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MainProjection {
    permission: &'static str,
    privilege: &'static str,
    local_environment_service: &'static str,
    tunnel_service: &'static str,
    coding_service: &'static str,
    current_project: Option<String>,
    projects: Vec<ProjectProjection>,
    current_task: Option<TaskProjection>,
    current_workflow: Option<CurrentWorkflowProjection>,
    current_command: Option<CurrentCommandProjection>,
    last_command: Option<LastCommandProjection>,
    last_tool: Option<LastToolProjection>,
    current_activity: Option<CurrentActivityProjection>,
    last_activity: Option<LastActivityProjection>,
    projection_revision: u64,
    tunnel_id: Option<String>,
    runtime_key_saved: bool,
    auto_start: bool,
    close_window_continue_running: bool,
    reconnect: Option<ReconnectProjection>,
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
struct CurrentWorkflowProjection { state: &'static str }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CurrentCommandProjection { state: &'static str }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct LastCommandProjection { status: &'static str, age_ms: u64 }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct LastToolProjection {
    kind: &'static str,
    summary: Option<String>,
    age_ms: u64,
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

const ADMIN_CONSENT_DURATION: Duration = Duration::from_millis(9000);

#[derive(Debug)]
struct PendingAdminConsent {
    challenge_id: String,
    not_before: Instant,
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

fn begin_admin_consent_challenge(challenge_id: &str) -> Result<(), String> {
    if !valid_admin_consent_challenge_id(challenge_id) {
        return Err("管理员确认标识无效".to_string());
    }
    admin_consent_gate()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .begin_at(challenge_id, Instant::now());
    Ok(())
}

fn cancel_admin_consent_challenge(challenge_id: &str) -> Result<(), String> {
    if !valid_admin_consent_challenge_id(challenge_id) {
        return Err("管理员确认标识无效".to_string());
    }
    admin_consent_gate()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .cancel(challenge_id);
    Ok(())
}

fn confirm_admin_consent_challenge(challenge_id: &str) -> Result<(), String> {
    if !valid_admin_consent_challenge_id(challenge_id) {
        return Err("管理员确认标识无效".to_string());
    }
    let confirmed = admin_consent_gate()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .confirm_at(challenge_id, Instant::now());
    if confirmed {
        Ok(())
    } else {
        Err("管理员确认无效或尚未完成".to_string())
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
pub async fn get_main_projection(app: AppHandle) -> Result<MainProjection, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        get_main_projection_blocking(app.clone(), &lifecycle)
    })
    .await
    .map_err(|_| "主控状态后台任务异常".to_string())?
}

#[tauri::command]
pub async fn wait_main_projection_change(
    since_revision: u64,
    app: AppHandle,
) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        Ok(lifecycle.wait_projection_change_after(since_revision))
    })
    .await
    .map_err(|_| "状态唤醒后台任务异常".to_string())?
}

#[tauri::command]
pub async fn ui_ready(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        lifecycle
            .start_staged_foreground_after_ui_ready()
            .map(|_| ())
            .map_err(|_| "无法启动后台服务".to_string())
    })
    .await
    .map_err(|_| "界面就绪后台任务异常".to_string())?
}

fn get_main_projection_blocking(
    app: AppHandle,
    lifecycle: &DesktopLifecycle,
) -> Result<MainProjection, String> {
    let (_, data) = load_app_data(&app)?;
    let (snapshot, projection_revision) = lifecycle.runtime_snapshot_with_revision();
    let task_aggregate = lifecycle.task_aggregate_snapshot();
    let privilege = lifecycle.privilege().refresh_broker_state();
    let metadata = WindowsCredentialStore::default()
        .runtime_api_key_metadata()
        .map_err(|_| "无法读取 Runtime API Key 状态".to_string())?;
    let profile = StartupProfileStore::new(app_data_dir(&app)?.join(STARTUP_PROFILE_FILE_NAME))
        .load()
        .map_err(|_| "无法读取连接设置".to_string())?;
    let tunnel_id = profile
        .validated_tunnel_id()
        .map_err(|_| "Tunnel ID 格式无效".to_string())?
        .map(|value| value.expose().to_owned());
    let active_id = data
        .workspace
        .active_workspace_id
        .as_ref()
        .map(WorkspaceId::as_str);
    let projects = data
        .workspace
        .remembered_entries()
        .iter()
        .map(|entry| {
            let path = WorkspaceValidator
                .validate(&entry.display_path)
                .ok()
                .filter(|validated| {
                    entry.validated_identity.as_str() == validated.identity().as_str()
                })
                .map(|validated| validated.execution_path().to_string_lossy().into_owned())
                .unwrap_or_else(|| "项目已无法访问".to_string());
            ProjectProjection {
                id: entry.workspace_id.as_str().to_owned(),
                path,
                active: active_id == Some(entry.workspace_id.as_str()),
            }
        })
        .collect::<Vec<_>>();
    let current_project = projects
        .iter()
        .find(|project| project.active)
        .map(|project| project.path.clone());
    let (tunnel_service, coding_service) = service_codes(&snapshot.state);
    let reconnect = snapshot.outage.and_then(|outage| {
        outage
            .user_attention_required
            .then_some(ReconnectProjection {
                generation: outage.generation,
            })
    });
    Ok(MainProjection {
        permission: stored_permission_code(data.settings.permission_mode),
        privilege: privilege_code(&privilege),
        local_environment_service: local_environment_service_code(&snapshot.state),
        tunnel_service,
        coding_service,
        current_project,
        projects,
        current_task: legacy_task_projection_from_aggregate(&task_aggregate, snapshot.current_task_elapsed_ms),
        current_workflow: current_workflow_projection(&task_aggregate),
        current_command: current_command_projection(&task_aggregate),
        last_command: last_command_projection(&task_aggregate),
        last_tool: snapshot.last_tool.as_ref().map(last_tool_projection),
        current_activity: current_activity_projection(&task_aggregate),
        last_activity: last_activity_projection(&task_aggregate),
        projection_revision,
        tunnel_id,
        runtime_key_saved: metadata.has_runtime_key,
        auto_start: data.settings.auto_start_services,
        close_window_continue_running: data.settings.close_window_continue_running,
        reconnect,
    })
}

#[tauri::command]
pub async fn set_permission_mode(mode: String, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(challenge_id) = mode.strip_prefix("admin-consent-begin:") {
            return begin_admin_consent_challenge(challenge_id);
        }
        if let Some(challenge_id) = mode.strip_prefix("admin-consent-cancel:") {
            return cancel_admin_consent_challenge(challenge_id);
        }
        if let Some(challenge_id) = mode.strip_prefix("admin-consent-confirm:") {
            return confirm_admin_consent_challenge(challenge_id);
        }
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
            return Err("管理员确认尚未完成".to_string());
        }
        let (store, mut data) = load_app_data(&app)?;
        let previous_stored = data.settings.permission_mode;
        let previous: PermissionMode = previous_stored.into();
        if previous == PermissionMode::Elevated && requested != PermissionMode::Elevated {
            lifecycle
                .privilege()
                .disable()
                .map_err(|_| "无法关闭管理员权限".to_string())?;
        }
        let runtime_active = lifecycle.runtime_snapshot().active;
        if runtime_active {
            lifecycle
                .set_runtime_permission_mode(requested)
                .map_err(|_| "无法更新当前权限模式".to_string())?;
        }
        data.settings.permission_mode = requested.into();
        if store.save(&data).is_err() {
            if runtime_active {
                let _ = lifecycle.set_runtime_permission_mode(previous);
            }
            return Err("无法保存权限设置".to_string());
        }
        if requested == PermissionMode::Elevated
            && !privilege_active
            && request_explicit_admin(&lifecycle).is_err()
        {
            data.settings.permission_mode = previous_stored;
            let _ = store.save(&data);
            if runtime_active {
                let _ = lifecycle.set_runtime_permission_mode(previous);
            }
            if previous != PermissionMode::Elevated {
                let _ = lifecycle.privilege().disable();
            }
            return Err("无法准备管理员权限".to_string());
        }
        Ok(())
    })
    .await
    .map_err(|_| "权限设置后台任务异常".to_string())?
}

fn request_explicit_admin(lifecycle: &DesktopLifecycle) -> Result<(), ()> {
    let executable = std::env::current_exe().map_err(|_| ())?;
    let broker = executable
        .parent()
        .map(|parent| parent.join("localbridge-privileged-broker.exe"))
        .ok_or(())?;
    lifecycle
        .privilege()
        .enable_from_explicit_user_action(&broker)
        .map(|_| ())
        .map_err(|_| ())
}

#[tauri::command]
pub async fn set_auto_start(enabled: bool, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
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
            return Err("无法保存开机启动设置".to_string());
        }
        Ok(())
    })
    .await
    .map_err(|_| "开机启动后台任务异常".to_string())?
}

#[tauri::command]
pub async fn set_close_window_continue_running(
    enabled: bool,
    app: AppHandle,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        let (store, mut data) = load_app_data(&app)?;
        data.settings.close_window_continue_running = enabled;
        store
            .save(&data)
            .map_err(|_| "无法保存常规设置".to_string())?;
        lifecycle.set_close_window_continue_running(enabled);
        Ok(())
    })
    .await
    .map_err(|_| "常规设置后台任务异常".to_string())?
}

#[tauri::command]
pub async fn save_runtime_key(value: String, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
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
        reconnect_after_connection_change(&app, &lifecycle)?;
        Ok(())
    })
    .await
    .map_err(|_| "Runtime API Key 保存后台任务异常".to_string())?
}

#[tauri::command]
pub async fn save_tunnel_id(value: String, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
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
        reconnect_after_connection_change(&app, &lifecycle)?;
        Ok(())
    })
    .await
    .map_err(|_| "Tunnel ID 保存后台任务异常".to_string())?
}

#[tauri::command]
pub async fn delete_runtime_key(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let deleted = WindowsCredentialStore::default()
            .delete_runtime_api_key()
            .map_err(|_| "无法删除Runtime API Key".to_string())?;
        if deleted {
            let lifecycle = app.state::<DesktopLifecycle>();
            reconnect_after_connection_change(&app, &lifecycle)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| "Runtime API Key 删除后台任务异常".to_string())?
}

#[tauri::command]
pub async fn retry_connection(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        lifecycle
            .manual_retry_after_attention()
            .map_err(|_| "当前连接无法重试".to_string())?;
        Ok(())
    })
    .await
    .map_err(|_| "连接重试后台任务异常".to_string())?
}

#[tauri::command]
pub async fn add_project(
    path: String,
    defer_activation: Option<bool>,
    app: AppHandle,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        add_project_blocking(path, defer_activation, &app, &lifecycle)
    })
    .await
    .map_err(|_| "项目添加后台任务异常".to_string())?
}

pub(crate) fn add_project_blocking(
    path: String,
    defer_activation: Option<bool>,
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
) -> Result<String, String> {
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
    Ok(id_value)
}

#[tauri::command]
pub async fn select_project(id: String, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        select_project_blocking(id, &app, &lifecycle)
    })
    .await
    .map_err(|_| "项目切换后台任务异常".to_string())?
}

pub(crate) fn select_project_blocking(
    id: String,
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
) -> Result<(), String> {
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
        return Err("项目身份已变化，请重新添加".to_string());
    }
    activate_project(
        app,
        lifecycle,
        &store,
        &mut data,
        id,
        validated.execution_path(),
    )
}

#[tauri::command]
pub async fn remove_project(id: String, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        remove_project_blocking(id, &app, &lifecycle)
    })
    .await
    .map_err(|_| "项目移除后台任务异常".to_string())?
}

fn remove_project_blocking(
    id: String,
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
) -> Result<(), String> {
    let id = WorkspaceId::from_validated(id).map_err(|_| "项目不存在".to_string())?;
    let (store, mut data) = load_app_data(app)?;
    let original = data.clone();
    if data.workspace.registry.get(&id).is_none() {
        return Err("项目不存在".to_string());
    }
    let was_active = data.workspace.active_workspace_id.as_ref() == Some(&id);
    let runtime_before = lifecycle.runtime_snapshot();
    if was_active {
        lifecycle
            .stop_runtime_for_control_plane()
            .map_err(|_| "无法停止当前项目服务".to_string())?;
    }
    if was_active {
        data.workspace.clear_active();
    }
    let _ = data.workspace.registry.remove(&id);
    if store.save(&data).is_err() {
        if was_active && !matches!(runtime_before.state, RuntimeState::Stopped) {
            if let Some(path) = runtime_before.configured_workspace.as_deref() {
                let _ = start_runtime_for_path(app, lifecycle, &original, path);
            }
        }
        return Err("无法保存项目变更".to_string());
    }
    Ok(())
}

fn activate_project(
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
    store: &SettingsStore,
    data: &mut AppData,
    id: WorkspaceId,
    candidate: &Path,
) -> Result<(), String> {
    clear_manual_stop_for_explicit_action(app)?;
    let before = lifecycle.runtime_snapshot();
    let already_current = before
        .configured_workspace
        .as_deref()
        .is_some_and(|path| path == candidate);
    if !already_current {
        if before.active {
            lifecycle
                .switch_runtime_workspace(candidate, before.configured_workspace.as_deref())
                .map_err(|_| "无法切换到所选项目".to_string())?;
        } else {
            start_runtime_for_path(app, lifecycle, data, candidate)?;
        }
    }
    data.workspace
        .set_active_reference(id)
        .map_err(|_| "无法设置当前项目".to_string())?;
    if store.save(data).is_err() {
        if !already_current {
            if let Some(previous) = before.configured_workspace.as_deref() {
                let _ = lifecycle.switch_runtime_workspace(previous, Some(candidate));
            } else if lifecycle.runtime_snapshot().active {
                let _ = lifecycle.stop_runtime_for_control_plane();
            }
        }
        return Err("无法保存当前项目".to_string());
    }
    Ok(())
}

fn start_runtime_for_path(
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
    data: &AppData,
    path: &Path,
) -> Result<(), String> {
    let config = production_runtime_config_for_path(app, data, path)?;
    lifecycle
        .start_production_runtime(config)
        .map_err(runtime_start_message)
}

fn production_runtime_config_for_path(
    app: &AppHandle,
    data: &AppData,
    path: &Path,
) -> Result<ProductionRuntimeConfig, String> {
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
        PermissionMode::from(data.settings.permission_mode),
    ))
}

fn production_runtime_config_for_active_workspace(
    app: &AppHandle,
    data: &AppData,
) -> Result<ProductionRuntimeConfig, String> {
    let entry = data
        .workspace
        .active_entry()
        .ok_or_else(|| "尚未选择项目".to_string())?;
    let validated = WorkspaceValidator
        .validate(&entry.display_path)
        .map_err(|_| "当前项目已无法访问".to_string())?;
    if entry.validated_identity.as_str() != validated.identity().as_str() {
        return Err("项目身份已变化，请重新添加".to_string());
    }
    production_runtime_config_for_path(app, data, validated.execution_path())
}

fn connection_change_requires_restart(state: &RuntimeState) -> bool {
    !matches!(state, RuntimeState::Stopped)
}

fn reconnect_after_connection_change(
    app: &AppHandle,
    lifecycle: &DesktopLifecycle,
) -> Result<(), String> {
    let snapshot = lifecycle.runtime_snapshot();
    if !connection_change_requires_restart(&snapshot.state) {
        return Ok(());
    }
    let (_, data) = load_app_data(app)?;
    let config = production_runtime_config_for_active_workspace(app, &data)?;
    lifecycle
        .backend_handle()
        .restart_production_runtime(config)
        .map_err(|_| "连接设置已保存，但服务重连失败".to_string())
}

#[tauri::command]
pub async fn restart_services(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        clear_manual_stop_for_explicit_action(&app)?;
        let (_, data) = load_app_data(&app)?;
        let config = production_runtime_config_for_active_workspace(&app, &data)?;
        let lifecycle = app.state::<DesktopLifecycle>();
        lifecycle
            .backend_handle()
            .restart_production_runtime(config)
            .map_err(|_| "无法重启服务".to_string())
    })
    .await
    .map_err(|_| "服务重启后台任务异常".to_string())?
}

#[tauri::command]
pub async fn stop_services(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let app_data = app_data_dir(&app)?;
        let lifecycle = app.state::<DesktopLifecycle>();
        let report =
            manual_stop_services(&app_data, &lifecycle).map_err(|_| "无法关闭服务".to_string())?;
        if report.tunnel_stop_failed
            || report.privilege_stop_failed
            || report.lower_runtime_stop_failed
        {
            return Err("服务关闭不完整，请查看诊断".to_string());
        }
        Ok(())
    })
    .await
    .map_err(|_| "服务关闭后台任务异常".to_string())?
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

fn clear_manual_stop_for_explicit_action(app: &AppHandle) -> Result<(), String> {
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

fn load_app_data(app: &AppHandle) -> Result<(SettingsStore, AppData), String> {
    let store = SettingsStore::new(app_data_dir(app)?.join("settings.json"));
    let data = store.load().map_err(|_| "无法读取设置".to_string())?;
    Ok((store, data))
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|_| "无法定位应用数据目录".to_string())
}

fn production_install_root() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "无法定位本地运行环境".to_string())
    }
    #[cfg(not(debug_assertions))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .ok_or_else(|| "无法定位本地运行环境".to_string())
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
fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn parse_permission(value: &str) -> Result<PermissionMode, String> {
    match value {
        "edit" => Ok(PermissionMode::Edit),
        "full" => Ok(PermissionMode::Full),
        "admin" => Ok(PermissionMode::Elevated),
        _ => Err("权限模式无效".to_string()),
    }
}
fn stored_permission_code(value: StoredPermissionMode) -> &'static str {
    match value {
        StoredPermissionMode::Edit => "edit",
        StoredPermissionMode::Full => "full",
        StoredPermissionMode::Elevated => "admin",
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
fn last_tool_projection(last: &LastToolTiming) -> LastToolProjection {
    LastToolProjection {
        kind: task_kind_code(last.kind),
        summary: last.summary.as_deref().map(str::to_owned),
        age_ms: last.age_ms,
    }
}
fn current_workflow_projection(aggregate: &Value) -> Option<CurrentWorkflowProjection> {
    let state=aggregate.get("current_workflow")?.get("state")?.as_str()?;
    Some(CurrentWorkflowProjection{state:match state {"running"=>"running","waiting"=>"waiting",_=>return None}})
}
fn current_command_projection(aggregate: &Value) -> Option<CurrentCommandProjection> {
    let state=aggregate.get("current_command")?.get("state")?.as_str()?;
    Some(CurrentCommandProjection{state:match state {"running"=>"running","waiting_input"=>"waiting_input","cancelling"=>"cancelling",_=>return None}})
}
fn last_command_projection(aggregate: &Value) -> Option<LastCommandProjection> {
    let terminal=aggregate.get("last_command")?; let status=terminal.get("status")?.as_str()?;
    let status=match status {"completed"=>"completed","failed"=>"failed","cancelled"=>"cancelled","timed_out"=>"timed_out","lost"=>"lost",_=>return None};
    let completed=terminal.get("completed_at_ms").and_then(Value::as_u64).unwrap_or(0);
    let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u64::MAX as u128) as u64;
    Some(LastCommandProjection{status,age_ms:now.saturating_sub(completed)})
}
fn activity_kind_code(value: &Value) -> Option<&'static str> {
    match value.as_str()? {
        "read" => Some("read"), "search" => Some("search"), "modify" => Some("modify"),
        "command" => Some("command"), "git" => Some("git"), "build" => Some("build"),
        "test" => Some("test"), "admin" => Some("admin"), "other" => Some("other"), _ => None,
    }
}
fn current_activity_projection(aggregate: &Value) -> Option<CurrentActivityProjection> {
    let activity = aggregate.get("current_activity")?.as_object()?;
    let kind = activity_kind_code(activity.get("kind")?)?;
    let state = match activity.get("state")?.as_str()? {
        "running" => "running", "waiting" => "waiting", "waiting_input" => "waiting_input",
        "cancelling" => "cancelling", _ => return None,
    };
    Some(CurrentActivityProjection {
        kind, state,
        summary: activity.get("summary").and_then(Value::as_str).map(str::to_owned),
        elapsed_ms: activity.get("elapsed_ms").and_then(Value::as_u64),
        step: activity.get("step").and_then(Value::as_str).map(str::to_owned),
        progress_current: activity.get("progress_current").and_then(Value::as_u64),
        progress_total: activity.get("progress_total").and_then(Value::as_u64),
    })
}
fn last_activity_projection(aggregate: &Value) -> Option<LastActivityProjection> {
    let activity = aggregate.get("last_activity")?.as_object()?;
    let kind = activity_kind_code(activity.get("kind")?)?;
    let outcome = match activity.get("outcome")?.as_str()? {
        "completed" => "completed", "failed" => "failed", "cancelled" => "cancelled",
        "timed_out" => "timed_out", "lost" => "lost", _ => return None,
    };
    Some(LastActivityProjection {
        kind,
        summary: activity.get("summary").and_then(Value::as_str).map(str::to_owned),
        outcome,
        completed_at_ms: activity.get("completed_at_ms")?.as_u64()?,
    })
}
fn legacy_task_projection_from_aggregate(aggregate:&Value, elapsed_ms:Option<u64>)->Option<TaskProjection>{
    if let Some(command)=current_command_projection(aggregate){ return Some(TaskProjection{kind:"command",summary:None,state:match command.state{"running"=>"running","waiting_input"=>"waiting","cancelling"=>"running",_=>"running"},elapsed_ms}); }
    current_workflow_projection(aggregate).map(|workflow|TaskProjection{kind:"other",summary:None,state:if workflow.state=="waiting"{"waiting"}else{"running"},elapsed_ms})
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
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/unit/ui/backend_projection.rs"
    ));

    #[test]
    fn admin_consent_gate_rejects_early_and_consumes_exact_boundary() {
        let start = Instant::now();
        let mut gate = AdminConsentGate::default();
        gate.begin_at("challenge-a-0001", start);
        assert!(!gate.confirm_at("challenge-a-0001", start + Duration::from_millis(8_999)));
        assert!(gate.confirm_at("challenge-a-0001", start + ADMIN_CONSENT_DURATION));
        assert!(gate.consume_confirmed());
        assert!(!gate.consume_confirmed());
    }

    #[test]
    fn admin_consent_gate_fresh_begin_resets_full_deadline() {
        let start = Instant::now();
        let mut gate = AdminConsentGate::default();
        gate.begin_at("challenge-a-0001", start);
        gate.begin_at("challenge-b-0002", start + Duration::from_millis(8_500));
        assert!(!gate.confirm_at("challenge-b-0002", start + Duration::from_millis(9_000)));
        assert!(gate.confirm_at("challenge-b-0002", start + Duration::from_millis(17_500)));
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

    #[test]
    fn connection_changes_restart_runtime_when_starting_or_connected() {
        assert!(!connection_change_requires_restart(&RuntimeState::Stopped));
        assert!(connection_change_requires_restart(
            &RuntimeState::StartingMcp
        ));
        assert!(connection_change_requires_restart(
            &RuntimeState::WaitingMcpReady
        ));
        assert!(connection_change_requires_restart(&RuntimeState::Ready));
    }
}
