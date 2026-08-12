#![cfg_attr(windows, windows_subsystem = "windows")]

use localbridge_lib::app::{DesktopLifecycle, StartupMode};
use localbridge_lib::privilege::PrivilegeController;
use localbridge_lib::tray::{MAIN_WINDOW_LABEL, ensure_main_window, install_tray};
use tauri::{Manager, WindowEvent};

fn main() {
    let startup_mode = StartupMode::from_args(std::env::args_os());
    localbridge_lib::build_app()
        .setup(move |app| {
            app.manage(DesktopLifecycle::new(PrivilegeController::new()));
            install_tray(app.handle())?;
            if startup_mode.creates_main_window_at_startup() {
                ensure_main_window(app.handle())?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == MAIN_WINDOW_LABEL {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("LocalBridge 启动失败");
}
