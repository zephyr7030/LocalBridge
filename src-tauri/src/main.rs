#![cfg_attr(windows, windows_subsystem = "windows")]

use localbridge_lib::app::{
    DesktopLifecycle, SingleInstanceAcquire, SingleInstanceGuard, StartupMode,
    configure_desktop_startup,
};
use localbridge_lib::privilege::PrivilegeController;
use localbridge_lib::tray::{MAIN_WINDOW_LABEL, ensure_main_window, install_tray};
use tauri::{Manager, WindowEvent};
#[cfg(debug_assertions)]
use localbridge_lib::{FixedWindowE2eMetricsSink, settings::SettingsStore};
#[cfg(debug_assertions)]
use serde::Deserialize;
#[cfg(debug_assertions)]
use std::sync::mpsc::{self, Receiver};
#[cfg(debug_assertions)]
use std::time::{Duration, Instant};
#[cfg(debug_assertions)]
use tauri::WebviewWindow;

fn main() {
    #[cfg(debug_assertions)]
    if let Some(view) = fixed_window_e2e_view() {
        run_fixed_window_e2e(view);
        return;
    }

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

#[cfg(debug_assertions)]
#[derive(Clone, Copy)]
enum FixedWindowE2eView {
    Onboarding,
    Dashboard,
}

#[cfg(debug_assertions)]
impl FixedWindowE2eView {
    fn as_str(self) -> &'static str {
        match self {
            Self::Onboarding => "onboarding",
            Self::Dashboard => "dashboard",
        }
    }
}

#[cfg(debug_assertions)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixedWindowE2eRect {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

#[cfg(debug_assertions)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixedWindowE2eMetrics {
    inner_width: f64,
    inner_height: f64,
    dpr: f64,
    chrome_count: usize,
    chrome: Option<FixedWindowE2eRect>,
    titlebar: Option<FixedWindowE2eRect>,
    content: Option<FixedWindowE2eRect>,
    onboarding: Option<FixedWindowE2eRect>,
    dashboard: Option<FixedWindowE2eRect>,
    controls: Vec<String>,
    view: String,
}

#[cfg(debug_assertions)]
const FIXED_WINDOW_E2E_METRICS_SCRIPT: &str = r#"
(() => {
  const rect = (element) => element ? (() => {
    const value = element.getBoundingClientRect();
    return { left: value.left, top: value.top, width: value.width, height: value.height };
  })() : null;
  const chromes = document.querySelectorAll('.window-chrome');
  const dashboard = document.querySelector('.shell');
  const onboarding = document.querySelector('.onboarding-shell');
  const metrics = {
    innerWidth: window.innerWidth,
    innerHeight: window.innerHeight,
    dpr: window.devicePixelRatio,
    chromeCount: chromes.length,
    chrome: rect(chromes[0]),
    titlebar: rect(document.querySelector('.window-titlebar')),
    content: rect(document.querySelector('.window-content')),
    onboarding: rect(onboarding),
    dashboard: rect(dashboard),
    controls: Array.from(document.querySelectorAll('.window-control')).map((element) => element.getAttribute('aria-label') || ''),
    view: dashboard ? 'dashboard' : onboarding ? 'onboarding' : 'other'
  };
  void window.__TAURI_INTERNALS__.invoke('fixed_window_e2e_report', { metrics: JSON.stringify(metrics) });
})();
"#;

#[cfg(debug_assertions)]
fn fixed_window_e2e_view() -> Option<FixedWindowE2eView> {
    match std::env::var("LOCALBRIDGE_FIXED_WINDOW_E2E_VIEW").ok()?.as_str() {
        "onboarding" => Some(FixedWindowE2eView::Onboarding),
        "dashboard" => Some(FixedWindowE2eView::Dashboard),
        _ => None,
    }
}

#[cfg(debug_assertions)]
fn run_fixed_window_e2e(view: FixedWindowE2eView) {
    localbridge_lib::build_app()
        .setup(move |app| {
            let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
            let app_data = app.path().app_data_dir()?;
            let store = SettingsStore::new(app_data.join("settings.json"));
            let mut data = store
                .load()
                .map_err(|error| std::io::Error::other(format!("fixed-window E2E settings load: {error:?}")))?;
            data.settings.onboarding_complete = matches!(view, FixedWindowE2eView::Dashboard);
            store
                .save(&data)
                .map_err(|error| std::io::Error::other(format!("fixed-window E2E settings save: {error:?}")))?;
            app.manage(lifecycle);
            let (metrics_tx, metrics_rx) = mpsc::channel::<String>();
            app.manage(FixedWindowE2eMetricsSink::new(metrics_tx));
            let window = ensure_main_window(app.handle())?;
            let driver_window = window.clone();
            let driver_app = app.handle().clone();
            std::thread::spawn(move || {
                match execute_fixed_window_e2e(&driver_window, view, &metrics_rx) {
                    Ok(summary) => {
                        println!("LB016_FIXED_WINDOW_E2E=PASS view={} {summary}", view.as_str());
                        driver_app.exit(0);
                    }
                    Err(error) => {
                        eprintln!("LB016_FIXED_WINDOW_E2E=FAIL view={} error={error}", view.as_str());
                        driver_app.exit(2);
                    }
                }
            });
            Ok(())
        })
        .on_window_event(handle_main_window_event)
        .run(tauri::generate_context!())
        .expect("LocalBridge fixed-window E2E failed");
}

#[cfg(debug_assertions)]
fn execute_fixed_window_e2e(
    window: &WebviewWindow<tauri::Wry>,
    view: FixedWindowE2eView,
    metrics_rx: &Receiver<String>,
) -> Result<String, String> {
    std::thread::sleep(Duration::from_millis(450));
    let physical = window.inner_size().map_err(|error| format!("inner_size: {error}"))?;
    let scale = window.scale_factor().map_err(|error| format!("scale_factor: {error}"))?;
    let logical_width = f64::from(physical.width) / scale;
    let logical_height = f64::from(physical.height) / scale;
    if (logical_width - 900.0).abs() > 2.0 || (logical_height - 620.0).abs() > 2.0 {
        return Err(format!("native client is {logical_width:.1}x{logical_height:.1}, expected 900x620"));
    }
    if window.is_resizable().map_err(|error| format!("is_resizable: {error}"))? {
        return Err("native window remains resizable".into());
    }
    if window.is_maximizable().map_err(|error| format!("is_maximizable: {error}"))? {
        return Err("native window remains maximizable".into());
    }
    if window.is_decorated().map_err(|error| format!("is_decorated: {error}"))? {
        return Err("native window decorations are still enabled".into());
    }

    let metrics = collect_fixed_window_e2e_metrics(window, view, metrics_rx)?;
    assert_fixed_window_e2e_metrics(&metrics, view)?;

    window
        .eval("document.querySelector('[aria-label=\"最小化\"]')?.click();")
        .map_err(|error| format!("click minimize: {error}"))?;
    wait_for_fixed_window_state(Duration::from_secs(4), || window.is_minimized().ok() == Some(true))
        .ok_or("custom minimize control did not minimize the native window")?;
    window.unminimize().map_err(|error| format!("unminimize: {error}"))?;

    window
        .eval("document.querySelector('[aria-label=\"关闭\"]')?.click();")
        .map_err(|error| format!("click close: {error}"))?;
    wait_for_fixed_window_state(Duration::from_secs(4), || window.is_visible().ok() == Some(false))
        .ok_or("custom close control did not reach CloseRequested close-to-hide behavior")?;

    Ok(format!(
        "logical={}x{} webview={}x{} dpr={} decorations=false resizable=false maximizable=false chrome=edge-to-edge controls=drag,minimize,close minimize_click=true close_hide=true",
        logical_width.round(),
        logical_height.round(),
        metrics.inner_width.round(),
        metrics.inner_height.round(),
        metrics.dpr
    ))
}

#[cfg(debug_assertions)]
fn collect_fixed_window_e2e_metrics(
    window: &WebviewWindow<tauri::Wry>,
    view: FixedWindowE2eView,
    metrics_rx: &Receiver<String>,
) -> Result<FixedWindowE2eMetrics, String> {
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut last_payload = String::new();
    while Instant::now() < deadline {
        window
            .eval(FIXED_WINDOW_E2E_METRICS_SCRIPT)
            .map_err(|error| format!("eval metrics: {error}"))?;
        if let Ok(payload) = metrics_rx.recv_timeout(Duration::from_millis(150)) {
            last_payload = payload;
            if let Ok(metrics) = serde_json::from_str::<FixedWindowE2eMetrics>(&last_payload) {
                if metrics.view == view.as_str() {
                    return Ok(metrics);
                }
            }
        }
    }
    Err(format!("timed out waiting for live WebView metrics; last={last_payload}"))
}

#[cfg(debug_assertions)]
fn assert_fixed_window_e2e_metrics(
    metrics: &FixedWindowE2eMetrics,
    view: FixedWindowE2eView,
) -> Result<(), String> {
    let chrome = metrics.chrome.as_ref().ok_or("window chrome missing")?;
    if metrics.chrome_count != 1 {
        return Err(format!("expected exactly one window chrome, got {}", metrics.chrome_count));
    }
    if chrome.left.abs() > 0.5
        || chrome.top.abs() > 0.5
        || (chrome.width - metrics.inner_width).abs() > 1.0
        || (chrome.height - metrics.inner_height).abs() > 1.0
    {
        return Err(format!("custom chrome is not edge-to-edge: {chrome:?}"));
    }
    let titlebar = metrics.titlebar.as_ref().ok_or("custom titlebar missing")?;
    let content = metrics.content.as_ref().ok_or("window content missing")?;
    let titlebar_right_inset = metrics.inner_width - (titlebar.left + titlebar.width);
    if titlebar.left < -0.5
        || titlebar.left > 2.0
        || titlebar.top < -0.5
        || titlebar.top > 2.0
        || !(-0.5..=2.0).contains(&titlebar_right_inset)
    {
        return Err(format!("custom titlebar does not follow the single chrome inner edge: {titlebar:?}"));
    }
    let content_right_inset = metrics.inner_width - (content.left + content.width);
    let titlebar_bottom = titlebar.top + titlebar.height;
    if (content.left - titlebar.left).abs() > 0.5
        || (content_right_inset - titlebar_right_inset).abs() > 0.5
        || content.top + 0.5 < titlebar_bottom
        || content.top - titlebar_bottom > 2.0
    {
        return Err(format!("window content is not directly below the custom titlebar: titlebar={titlebar:?} content={content:?}"));
    }
    if metrics.controls != ["最小化".to_string(), "关闭".to_string()] {
        return Err(format!("unexpected custom window controls: {:?}", metrics.controls));
    }
    match view {
        FixedWindowE2eView::Onboarding => {
            let child = metrics.onboarding.as_ref().ok_or("onboarding shell missing")?;
            if child.width > content.width + 1.0 || child.height > content.height + 1.0 {
                return Err("onboarding exceeds fixed chrome content area".into());
            }
        }
        FixedWindowE2eView::Dashboard => {
            let child = metrics.dashboard.as_ref().ok_or("dashboard shell missing")?;
            if child.width > content.width + 1.0 || child.height < 1.0 {
                return Err("dashboard does not fit fixed chrome content area".into());
            }
        }
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn wait_for_fixed_window_state(timeout: Duration, mut predicate: impl FnMut() -> bool) -> Option<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return Some(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}
