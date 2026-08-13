use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};

use crate::app::{
    AutostartManager, DesktopLifecycle, DesktopRuntimeStartError, STARTUP_PROFILE_FILE_NAME,
    StartupProfileStore,
};
use crate::credentials::{CredentialStore, SecretString, WindowsCredentialStore};
use crate::runtime::ProductionRuntimeConfig;
use crate::settings::{AppData, SettingsStore, StoredPermissionMode};
use crate::state::{
    CurrentTaskStatus, PermissionMode, PrivilegeState, RuntimeComponent, RuntimeFault,
    RuntimeState, TaskExecutionState, TaskKind,
};
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
    runtime_key_saved: bool,
    auto_start: bool,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReconnectProjection {
    generation: u64,
}

#[tauri::command]
pub fn get_main_projection(
    app: AppHandle,
    lifecycle: State<'_, DesktopLifecycle>,
) -> Result<MainProjection, String> {
    let (_, data) = load_app_data(&app)?;
    let snapshot = lifecycle.runtime_snapshot();
    let privilege = lifecycle.privilege().refresh_broker_state();
    let metadata = WindowsCredentialStore::default()
        .runtime_api_key_metadata()
        .map_err(|_| "无法读取运行密钥状态".to_string())?;
    let active_id = data
        .workspace
        .active_workspace_id
        .as_ref()
        .map(WorkspaceId::as_str);
    let projects = data
        .workspace
        .remembered_entries()
        .iter()
        .map(|entry| ProjectProjection {
            id: entry.workspace_id.as_str().to_owned(),
            path: entry.display_path.to_string_lossy().into_owned(),
            active: active_id == Some(entry.workspace_id.as_str()),
        })
        .collect::<Vec<_>>();
    let current_project = data
        .workspace
        .active_entry()
        .map(|entry| entry.display_path.to_string_lossy().into_owned());
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
        current_task: task_projection(&snapshot.current_task),
        runtime_key_saved: metadata.has_runtime_key,
        auto_start: data.settings.auto_start_services,
        reconnect,
    })
}

#[tauri::command]
pub fn set_permission_mode(
    mode: String,
    app: AppHandle,
    lifecycle: State<'_, DesktopLifecycle>,
) -> Result<(), String> {
    let requested = parse_permission(&mode)?;
    let (store, mut data) = load_app_data(&app)?;
    let previous_stored = data.settings.permission_mode;
    let previous: PermissionMode = previous_stored.into();
    if requested == previous {
        return Ok(());
    }
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
    if previous != PermissionMode::Elevated
        && requested == PermissionMode::Elevated
        && lifecycle.privilege().request_without_uac().is_err()
    {
        data.settings.permission_mode = previous_stored;
        let _ = store.save(&data);
        if runtime_active {
            let _ = lifecycle.set_runtime_permission_mode(previous);
        }
        return Err("无法准备管理员权限".to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn set_auto_start(enabled: bool, app: AppHandle) -> Result<(), String> {
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
}

#[tauri::command]
pub fn save_runtime_key(value: String) -> Result<(), String> {
    let secret = SecretString::new(value).map_err(|_| "运行密钥格式无效".to_string())?;
    WindowsCredentialStore::default()
        .save_runtime_api_key(&secret)
        .map_err(|_| "无法安全保存运行密钥".to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_runtime_key() -> Result<(), String> {
    WindowsCredentialStore::default()
        .delete_runtime_api_key()
        .map_err(|_| "无法删除运行密钥".to_string())?;
    Ok(())
}

#[tauri::command]
pub fn enable_admin(app: AppHandle, lifecycle: State<'_, DesktopLifecycle>) -> Result<(), String> {
    let (_, data) = load_app_data(&app)?;
    if data.settings.permission_mode != StoredPermissionMode::Elevated {
        return Err("请先选择管理员模式".to_string());
    }
    let executable = std::env::current_exe().map_err(|_| "无法定位管理员组件".to_string())?;
    let broker = executable
        .parent()
        .map(|parent| parent.join("localbridge-privileged-broker.exe"))
        .ok_or_else(|| "无法定位管理员组件".to_string())?;
    lifecycle
        .privilege()
        .enable_from_explicit_user_action(&broker)
        .map_err(|_| "管理员授权未完成".to_string())?;
    Ok(())
}

#[tauri::command]
pub fn disable_admin(lifecycle: State<'_, DesktopLifecycle>) -> Result<(), String> {
    lifecycle
        .privilege()
        .request_without_uac()
        .map_err(|_| "无法关闭管理员权限".to_string())
}

#[tauri::command]
pub fn retry_connection(lifecycle: State<'_, DesktopLifecycle>) -> Result<(), String> {
    lifecycle
        .manual_retry_after_attention()
        .map_err(|_| "当前连接无法重试".to_string())?;
    Ok(())
}

#[tauri::command]
pub fn add_project(
    path: String,
    defer_activation: Option<bool>,
    app: AppHandle,
    lifecycle: State<'_, DesktopLifecycle>,
) -> Result<String, String> {
    let candidate_path = PathBuf::from(path);
    let validated = WorkspaceValidator
        .validate(&candidate_path)
        .map_err(|_| "所选项目无法验证".to_string())?;
    let (store, mut data) = load_app_data(&app)?;
    let generated = WorkspaceId::from_validated(new_workspace_id())
        .map_err(|_| "无法创建项目记录".to_string())?;
    let id = data
        .workspace
        .registry
        .upsert_validated(generated, &candidate_path, &validated, unix_seconds())
        .map_err(|_| "无法保存项目记录".to_string())?;
    let id_value = id.as_str().to_owned();
    if defer_activation.unwrap_or(false) {
        store
            .save(&data)
            .map_err(|_| "无法保存项目记录".to_string())?;
        return Ok(id_value);
    }
    activate_project(
        &app,
        &lifecycle,
        &store,
        &mut data,
        id,
        validated.resolved_path(),
    )?;
    Ok(id_value)
}

#[tauri::command]
pub fn select_project(
    id: String,
    app: AppHandle,
    lifecycle: State<'_, DesktopLifecycle>,
) -> Result<(), String> {
    let id = WorkspaceId::from_validated(id).map_err(|_| "项目不存在".to_string())?;
    let (store, mut data) = load_app_data(&app)?;
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
        &app,
        &lifecycle,
        &store,
        &mut data,
        id,
        validated.resolved_path(),
    )
}

#[tauri::command]
pub fn remove_project(
    id: String,
    app: AppHandle,
    lifecycle: State<'_, DesktopLifecycle>,
) -> Result<(), String> {
    let id = WorkspaceId::from_validated(id).map_err(|_| "项目不存在".to_string())?;
    let (store, mut data) = load_app_data(&app)?;
    let original = data.clone();
    if data.workspace.registry.get(&id).is_none() {
        return Err("项目不存在".to_string());
    }
    let was_active = data.workspace.active_workspace_id.as_ref() == Some(&id);
    let runtime_before = lifecycle.runtime_snapshot();
    if was_active && runtime_before.active {
        lifecycle
            .stop_runtime_for_control_plane()
            .map_err(|_| "无法停止当前项目服务".to_string())?;
    }
    if was_active {
        data.workspace.clear_active();
    }
    let _ = data.workspace.registry.remove(&id);
    if store.save(&data).is_err() {
        if was_active && runtime_before.active {
            if let Some(path) = runtime_before.configured_workspace.as_deref() {
                let _ = start_runtime_for_path(&app, &lifecycle, &original, path);
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
    let app_data = app_data_dir(app)?;
    let profile = StartupProfileStore::new(app_data.join(STARTUP_PROFILE_FILE_NAME))
        .load()
        .map_err(|_| "无法读取连接设置".to_string())?;
    let tunnel_id: TunnelId = profile
        .validated_tunnel_id()
        .map_err(|_| "Tunnel ID 无效".to_string())?
        .ok_or_else(|| "尚未配置 Tunnel ID".to_string())?;
    let config = ProductionRuntimeConfig::new(
        production_install_root()?,
        path,
        app_data.join("health"),
        tunnel_id,
        PermissionMode::from(data.settings.permission_mode),
    );
    lifecycle
        .start_production_runtime(config)
        .map_err(runtime_start_message)
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
        RuntimeFault::RuntimeKeyMissing => "运行密钥未配置，请返回 OpenAI 页面重新保存",
        RuntimeFault::SecretStoreFailed => "无法读取 Windows 安全凭据中的运行密钥",
        RuntimeFault::SecretInjectionUnsupported => {
            "运行密钥无法安全注入 Tunnel，请重新安装 LocalBridge"
        }
        RuntimeFault::TunnelAuthFailed => "OpenAI Tunnel 鉴权失败，请检查运行密钥与 Tunnel 权限",
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
fn task_projection(status: &CurrentTaskStatus) -> Option<TaskProjection> {
    let CurrentTaskStatus::Active(task) = status else {
        return None;
    };
    Some(TaskProjection {
        kind: task_kind_code(task.kind),
        summary: task.summary.as_deref().map(str::to_owned),
        state: task_state_code(task.state),
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
    fn runtime_start_fault_messages_are_redacted_and_actionable() {
        assert_eq!(
            runtime_fault_message(&RuntimeFault::RuntimeKeyMissing),
            "运行密钥未配置，请返回 OpenAI 页面重新保存"
        );
        assert_eq!(
            runtime_fault_message(&RuntimeFault::TunnelAuthFailed),
            "OpenAI Tunnel 鉴权失败，请检查运行密钥与 Tunnel 权限"
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
