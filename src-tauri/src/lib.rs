pub mod app;
pub mod commands;
pub mod credentials;
pub mod diagnostics;
pub mod mcp;
pub mod privilege;
pub mod runtime;
pub mod settings;
pub mod state;
pub mod tray;
pub mod tunnel;
pub mod workspace;

pub const PRODUCT_NAME: &str = "LocalBridge";

pub fn build_app() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default().invoke_handler(tauri::generate_handler![
        commands::ui::get_main_projection,
        commands::ui::set_permission_mode,
        commands::ui::set_auto_start,
        commands::ui::save_runtime_key,
        commands::ui::delete_runtime_key,
        commands::ui::enable_admin,
        commands::ui::disable_admin,
        commands::ui::retry_connection,
        commands::ui::add_project,
        commands::ui::select_project,
        commands::ui::remove_project,
    ])
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
