use tauri::{AppHandle, Manager, State};

use crate::app::DesktopLifecycle;
use crate::credentials::{CredentialStore, WindowsCredentialStore};
use crate::diagnostics::{
    DiagnosticsOutageInput, DiagnosticsRuntimeInput, DiagnosticsSnapshot, build_snapshot,
    export_snapshot,
};

#[tauri::command]
pub fn get_diagnostics(_app: AppHandle, lifecycle: State<'_, DesktopLifecycle>) -> Result<DiagnosticsSnapshot, String> {
    let metadata = WindowsCredentialStore::default()
        .runtime_api_key_metadata()
        .map_err(|_| "无法读取运行密钥状态".to_string())?;
    let install_root = production_install_root()?;
    let runtime = lifecycle.runtime_snapshot();
    let diagnostics_runtime = DiagnosticsRuntimeInput {
        active: runtime.active,
        state: runtime.state,
        active_workspace: runtime.configured_workspace.is_some(),
        outage: runtime.outage.map(|outage| DiagnosticsOutageInput {
            generation: outage.generation,
            component: outage.component,
            fault: outage.fault,
            user_attention_required: outage.user_attention_required,
        }),
    };
    Ok(build_snapshot(
        &install_root,
        &diagnostics_runtime,
        &lifecycle.privilege().refresh_broker_state(),
        metadata.has_runtime_key,
    ))
}

#[tauri::command]
pub fn diagnostics_retry_connection(lifecycle: State<'_, DesktopLifecycle>) -> Result<(), String> {
    lifecycle
        .manual_retry_after_attention()
        .map_err(|_| "当前连接无法重试".to_string())?;
    Ok(())
}

#[tauri::command]
pub fn export_diagnostics(app: AppHandle, lifecycle: State<'_, DesktopLifecycle>) -> Result<String, String> {
    let snapshot = get_diagnostics(app.clone(), lifecycle)?;
    let root = app.path().app_data_dir().map_err(|_| "无法定位应用数据目录".to_string())?;
    export_snapshot(&root, &snapshot)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|_| "无法导出诊断信息".to_string())
}

fn production_install_root() -> Result<std::path::PathBuf, String> {
    #[cfg(debug_assertions)]
    {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(std::path::Path::to_path_buf)
            .ok_or_else(|| "无法定位本地运行环境".to_string())
    }
    #[cfg(not(debug_assertions))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
            .ok_or_else(|| "无法定位本地运行环境".to_string())
    }
}
