use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

use super::error::{UiError, UiResult};
use crate::app::DesktopLifecycle;
use crate::control_plane::snapshot::ProjectionAvailability;
use crate::diagnostics::{
    BrokerDiagnosticState, DiagnosticCheck, DiagnosticEvent, DiagnosticFault,
    DiagnosticsOutageInput, DiagnosticsRuntimeInput, DiagnosticsSnapshot, build_snapshot,
    diagnostics_log_revision, export_snapshot, materialize_log_directory,
    wait_diagnostics_log_change_after,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsViewProjection {
    projection_revision: u64,
    log_revision: u64,
    checks: Vec<DiagnosticCheck>,
    privilege: BrokerDiagnosticState,
    active_workspace_path: Option<String>,
    recent_events: Vec<DiagnosticEvent>,
    active_faults: Vec<DiagnosticFault>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsRevisionProjection {
    projection_revision: u64,
    log_revision: u64,
}

#[tauri::command]
pub async fn get_diagnostics(app: AppHandle) -> UiResult<DiagnosticsViewProjection> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<DiagnosticsViewProjection> {
        let lifecycle = app.state::<DesktopLifecycle>();
        let (projection_revision, snapshot) = get_diagnostics_snapshot_blocking(&lifecycle)?;
        Ok(project_diagnostics_view(projection_revision, snapshot))
    })
    .await
    .map_err(|_| UiError::internal("Ui.DiagnosticsReadJoinFailed", "诊断状态后台任务异常"))?
    .map_err(UiError::from_string)
}

fn get_diagnostics_snapshot_blocking(
    lifecycle: &DesktopLifecycle,
) -> UiResult<(u64, DiagnosticsSnapshot)> {
    let install_root = production_install_root()?;
    let control_plane = lifecycle.control_plane_snapshot();
    let runtime_section = &control_plane.runtime;
    let runtime = runtime_section.value();
    let workspace = &control_plane.workspace;
    let diagnostics_runtime = DiagnosticsRuntimeInput {
        available: runtime_section.availability() == ProjectionAvailability::Ready,
        stale: runtime_section.is_stale(),
        active: runtime.map(|runtime| runtime.active),
        state: runtime.map(|runtime| runtime.state.clone()),
        active_workspace: (workspace.availability() == ProjectionAvailability::Ready
            && !workspace.is_stale())
        .then(|| {
            workspace
                .value()
                .and_then(|value| value.observed_path.as_ref())
        })
        .flatten()
        .map(std::path::PathBuf::from),
        outage: runtime
            .and_then(|runtime| runtime.outage.as_ref())
            .map(|outage| DiagnosticsOutageInput {
                generation: outage.generation,
                request_id: outage.operation_id.clone(),
                component: outage.component,
                fault: outage.fault.clone(),
                user_attention_required: outage.user_attention_required,
            }),
    };
    let authority = &control_plane.authority;
    let privilege = (authority.availability() == ProjectionAvailability::Ready
        && !authority.is_stale())
    .then(|| authority.value().map(|value| &value.broker))
    .flatten();
    let settings = &control_plane.settings;
    let runtime_key_present = (settings.availability() == ProjectionAvailability::Ready
        && !settings.is_stale())
    .then(|| settings.value().map(|value| value.runtime_key_saved))
    .flatten();
    let mut snapshot = build_snapshot(
        &install_root,
        &diagnostics_runtime,
        privilege,
        runtime_key_present,
    );
    snapshot.active_faults = control_plane
        .active_faults
        .iter()
        .map(|fault| DiagnosticFault {
            code: fault.error.code.clone(),
            category: fault.error.category,
            message: fault.error.message.clone(),
            retryable: fault.error.retryable,
        })
        .collect();
    Ok((control_plane.revision, snapshot))
}

fn project_diagnostics_view(
    projection_revision: u64,
    snapshot: DiagnosticsSnapshot,
) -> DiagnosticsViewProjection {
    DiagnosticsViewProjection {
        projection_revision,
        log_revision: snapshot.revision,
        checks: snapshot
            .checks
            .into_iter()
            .filter(|check| check.code != "runtime_key")
            .collect(),
        privilege: snapshot.broker.state,
        active_workspace_path: snapshot.active_workspace_path,
        recent_events: snapshot.recent_events,
        active_faults: snapshot.active_faults,
    }
}

#[tauri::command]
pub async fn wait_diagnostics_change(
    since_projection_revision: u64,
    since_log_revision: u64,
    app: AppHandle,
) -> UiResult<DiagnosticsRevisionProjection> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<DiagnosticsRevisionProjection> {
        let lifecycle = app.state::<DesktopLifecycle>();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let projection_revision = lifecycle.control_plane_snapshot().revision;
            let log_revision = diagnostics_log_revision();
            if projection_revision > since_projection_revision
                || log_revision > since_log_revision
                || Instant::now() >= deadline
            {
                return Ok(DiagnosticsRevisionProjection {
                    projection_revision,
                    log_revision,
                });
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let wait = remaining.min(Duration::from_millis(250));
            let _ = wait_diagnostics_log_change_after(since_log_revision, wait);
        }
    })
    .await
    .map_err(|_| UiError::internal("Ui.DiagnosticsWaitJoinFailed", "诊断状态唤醒后台任务异常"))?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn open_logs(app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let lifecycle = app.state::<DesktopLifecycle>();
        let (_, snapshot) = get_diagnostics_snapshot_blocking(&lifecycle)?;
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
        let (_, snapshot) = get_diagnostics_snapshot_blocking(&lifecycle)?;
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
