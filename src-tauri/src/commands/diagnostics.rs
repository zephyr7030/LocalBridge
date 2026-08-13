use tauri::{AppHandle, Manager};

use crate::app::DesktopLifecycle;
use crate::credentials::{CredentialStore, WindowsCredentialStore};
use crate::diagnostics::{
    DiagnosticsOutageInput, DiagnosticsRuntimeInput, DiagnosticsSnapshot, build_snapshot,
    export_snapshot,
};

#[tauri::command]
pub async fn get_diagnostics(app: AppHandle) -> Result<DiagnosticsSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        get_diagnostics_blocking(&lifecycle)
    })
    .await
    .map_err(|_| "诊断状态后台任务异常".to_string())?
}

fn get_diagnostics_blocking(lifecycle: &DesktopLifecycle) -> Result<DiagnosticsSnapshot, String> {
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
pub async fn open_logs(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = app
            .path()
            .app_data_dir()
            .map_err(|_| "无法定位日志目录".to_string())?
            .join("logs");
        std::fs::create_dir_all(&root).map_err(|_| "无法创建日志目录".to_string())?;
        std::process::Command::new("explorer.exe")
            .arg(&root)
            .spawn()
            .map_err(|_| "无法打开日志目录".to_string())?;
        Ok(())
    })
    .await
    .map_err(|_| "打开日志后台任务异常".to_string())?
}

#[tauri::command]
pub async fn export_diagnostics(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        let snapshot = get_diagnostics_blocking(&lifecycle)?;
        let root = app
            .path()
            .app_data_dir()
            .map_err(|_| "无法定位应用数据目录".to_string())?;
        export_snapshot(&root, &snapshot)
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|_| "无法导出诊断信息".to_string())
    })
    .await
    .map_err(|_| "诊断导出后台任务异常".to_string())?
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
