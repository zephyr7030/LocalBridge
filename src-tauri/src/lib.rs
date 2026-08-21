pub mod app;
pub mod commands;
pub mod control_plane;
pub mod credentials;
pub mod diagnostics;
pub mod domain;
pub mod execution;
pub mod filesystem;
pub mod mcp;
pub mod privilege;
pub mod runtime;
pub mod settings;
pub mod state;
pub mod tray;
pub mod tunnel;
pub mod workspace;

pub const PRODUCT_NAME: &str = "LocalBridge";

macro_rules! localbridge_invoke_handler {
    ($($extra:path),* $(,)?) => {
        tauri::generate_handler![
            commands::ui::get_main_projection,
            commands::ui::wait_main_projection_change,
            commands::ui::ui_ready,
            commands::ui::set_permission_mode,
            commands::ui::set_auto_start,
            commands::ui::set_close_window_continue_running,
            commands::ui::save_tunnel_id,
            commands::ui::save_runtime_key,
            commands::ui::delete_runtime_key,
            commands::ui::retry_connection,
            commands::ui::add_project,
            commands::ui::select_project,
            commands::ui::remove_project,
            commands::ui::restart_services,
            commands::ui::stop_services,
            commands::ui::retry_update_check,
            commands::ui::open_github_releases,
            commands::onboarding::get_onboarding_state,
            commands::onboarding::save_onboarding_connection,
            commands::onboarding::open_openai_tunnel_settings,
            commands::onboarding::open_openai_api_keys,
            commands::onboarding::open_chatgpt_plugins_settings,
            commands::onboarding::open_chatgpt_custom_connector_settings,
            commands::onboarding::get_connector_endpoint,
            commands::onboarding::choose_onboarding_workspace_folder,
            commands::onboarding::prepare_onboarding_project,
            commands::onboarding::complete_onboarding,
            commands::diagnostics::get_diagnostics,
            commands::diagnostics::open_logs,
            commands::diagnostics::export_diagnostics,
            $($extra),*
        ]
    };
}

#[cfg(debug_assertions)]
pub struct FixedWindowE2eMetricsSink(std::sync::Mutex<std::sync::mpsc::Sender<String>>);

#[cfg(debug_assertions)]
impl FixedWindowE2eMetricsSink {
    pub fn new(sender: std::sync::mpsc::Sender<String>) -> Self {
        Self(std::sync::Mutex::new(sender))
    }
}

#[cfg(debug_assertions)]
#[tauri::command]
fn fixed_window_e2e_report(
    metrics: String,
    sink: tauri::State<'_, FixedWindowE2eMetricsSink>,
) -> Result<(), String> {
    sink.0
        .lock()
        .map_err(|_| "fixed-window E2E metrics sink poisoned".to_string())?
        .send(metrics)
        .map_err(|_| "fixed-window E2E metrics receiver closed".to_string())
}

#[cfg(debug_assertions)]
pub fn build_app() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default().invoke_handler(localbridge_invoke_handler![fixed_window_e2e_report])
}

#[cfg(not(debug_assertions))]
pub fn build_app() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default().invoke_handler(localbridge_invoke_handler![])
}

pub fn run() {
    build_app()
        .run(tauri::generate_context!())
        .expect("LocalBridge 启动失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_name_is_stable() {
        assert_eq!(PRODUCT_NAME, "LocalBridge");
    }
}
