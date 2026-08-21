use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::error::{UiError, UiResult};
use crate::app::DesktopLifecycle;
use crate::credentials::{CredentialStore, WindowsCredentialStore};
use crate::diagnostics::{
    BrokerDiagnosticState, DiagnosticCheck, DiagnosticEvent, DiagnosticsOutageInput,
    DiagnosticsRuntimeInput, DiagnosticsSnapshot, build_snapshot, export_snapshot,
    materialize_log_directory,
};
use crate::settings::SettingsStore;
use crate::workspace::WorkspaceValidator;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsViewProjection {
    checks: Vec<DiagnosticCheck>,
    privilege: BrokerDiagnosticState,
    active_workspace_path: Option<String>,
    recent_events: Vec<DiagnosticEvent>,
}

#[tauri::command]
pub async fn get_diagnostics(app: AppHandle) -> UiResult<DiagnosticsViewProjection> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<DiagnosticsViewProjection> {
        let lifecycle = app.state::<DesktopLifecycle>();
        let snapshot = get_diagnostics_snapshot_blocking(&lifecycle)?;
        let active_workspace_path = active_workspace_path(&app)?;
        Ok(project_diagnostics_view(snapshot, active_workspace_path))
    })
    .await
    .map_err(|_| UiError::internal("Ui.DiagnosticsReadJoinFailed", "诊断状态后台任务异常"))?
    .map_err(UiError::from_string)
}

fn get_diagnostics_snapshot_blocking(
    lifecycle: &DesktopLifecycle,
) -> UiResult<DiagnosticsSnapshot> {
    let metadata = WindowsCredentialStore::default()
        .runtime_api_key_metadata()
        .map_err(|_| "无法读取Runtime API Key状态".to_string())?;
    let install_root = production_install_root()?;
    let runtime = lifecycle.runtime_snapshot();
    let diagnostics_runtime = DiagnosticsRuntimeInput {
        active: runtime.active,
        state: runtime.state,
        active_workspace: runtime.configured_workspace,
        outage: runtime.outage.map(|outage| DiagnosticsOutageInput {
            generation: outage.generation,
            request_id: outage.request_id,
            component: outage.component,
            fault: outage.fault,
            user_attention_required: outage.user_attention_required,
        }),
    };
    let broker = lifecycle.privilege().refresh_broker_state();
    lifecycle.publish_current_observation();
    Ok(build_snapshot(
        &install_root,
        &diagnostics_runtime,
        &broker,
        metadata.has_runtime_key,
    ))
}

fn project_diagnostics_view(
    snapshot: DiagnosticsSnapshot,
    active_workspace_path: Option<String>,
) -> DiagnosticsViewProjection {
    DiagnosticsViewProjection {
        checks: snapshot
            .checks
            .into_iter()
            .filter(|check| check.code != "runtime_key")
            .collect(),
        privilege: snapshot.broker.state,
        active_workspace_path,
        recent_events: snapshot.recent_events,
    }
}

fn active_workspace_path(app: &AppHandle) -> UiResult<Option<String>> {
    let settings = SettingsStore::new(
        app.path()
            .app_data_dir()
            .map_err(|_| "无法定位应用数据目录".to_string())?
            .join("settings.json"),
    )
    .load()
    .map_err(|_| "无法读取设置".to_string())?;
    let Some(entry) = settings.workspace.active_entry() else {
        return Ok(None);
    };
    let validated = WorkspaceValidator
        .validate(&entry.display_path)
        .map_err(|_| "当前项目已无法访问".to_string())?;
    if entry.validated_identity.as_str() != validated.identity().as_str() {
        return Err(UiError::from("项目身份已变化，请重新添加"));
    }
    Ok(Some(
        validated.execution_path().to_string_lossy().into_owned(),
    ))
}

#[tauri::command]
pub async fn open_logs(app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let lifecycle = app.state::<DesktopLifecycle>();
        let snapshot = get_diagnostics_snapshot_blocking(&lifecycle)?;
        let root = app
            .path()
            .app_data_dir()
            .map_err(|_| "无法定位日志目录".to_string())?;
        let directory = materialize_log_directory(&root, &snapshot)
            .map_err(|_| "无法生成诊断日志".to_string())?;
        std::process::Command::new("explorer.exe")
            .arg(&directory)
            .spawn()
            .map_err(|_| "无法打开日志目录".to_string())?;
        Ok(())
    })
    .await
    .map_err(|_| UiError::internal("Ui.OpenLogsJoinFailed", "打开日志后台任务异常"))?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn export_diagnostics(app: AppHandle) -> UiResult<String> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<String> {
        let lifecycle = app.state::<DesktopLifecycle>();
        let snapshot = get_diagnostics_snapshot_blocking(&lifecycle)?;
        let root = app
            .path()
            .app_data_dir()
            .map_err(|_| "无法定位应用数据目录".to_string())?;
        Ok(export_snapshot(&root, &snapshot)
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|_| "无法导出诊断信息".to_string())?)
    })
    .await
    .map_err(|_| UiError::internal("Ui.DiagnosticsExportJoinFailed", "诊断导出后台任务异常"))?
    .map_err(UiError::from_string)
}

fn production_install_root() -> UiResult<std::path::PathBuf> {
    #[cfg(debug_assertions)]
    {
        Ok(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(std::path::Path::to_path_buf)
            .ok_or_else(|| "无法定位本地运行环境".to_string())?)
    }
    #[cfg(not(debug_assertions))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
            .ok_or_else(|| "无法定位本地运行环境".to_string())
            .map_err(UiError::from_string)
    }
}
