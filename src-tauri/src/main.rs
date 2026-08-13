#![cfg_attr(windows, windows_subsystem = "windows")]

use localbridge_lib::app::{
    DesktopLifecycle, SingleInstanceAcquire, SingleInstanceGuard, StartupMode,
    configure_desktop_startup,
};
use localbridge_lib::privilege::PrivilegeController;
use localbridge_lib::tray::{MAIN_WINDOW_LABEL, ensure_main_window, install_tray};
use tauri::{Manager, WindowEvent};

fn main() {
    let startup_mode = StartupMode::from_args(std::env::args_os());
    let single_instance = match SingleInstanceGuard::acquire()
        .expect("LocalBridge single-instance initialization failed")
    {
        SingleInstanceAcquire::Primary(primary) => primary,
        SingleInstanceAcquire::Secondary => return,
    };
    localbridge_lib::build_app()
        .setup(move |app| {
            let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
            let app_data_dir = app.path().app_data_dir()?;
            let _startup = configure_desktop_startup(&app_data_dir, startup_mode, &lifecycle)?;
            app.manage(lifecycle);
            install_tray(app.handle())?;
            let wake_app = app.handle().clone();
            single_instance.start_wake_listener(move || {
                let _ = ensure_main_window(&wake_app);
            })?;
            app.manage(single_instance);
            if startup_mode.creates_main_window_at_startup() {
                ensure_main_window(app.handle())?;
            }
            Ok(())
        })
        .on_window_event(handle_main_window_event)
        .run(tauri::generate_context!())
        .expect("LocalBridge 启动失败");
}

fn handle_main_window_event(window: &tauri::Window<tauri::Wry>, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW_LABEL {
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let _ = window.hide();
    }
}
