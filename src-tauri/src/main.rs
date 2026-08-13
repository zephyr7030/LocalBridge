#![cfg_attr(windows, windows_subsystem = "windows")]

use localbridge_lib::app::{
    DesktopLifecycle, SingleInstanceAcquire, SingleInstanceGuard, StartupMode,
    configure_desktop_startup,
};
use localbridge_lib::privilege::PrivilegeController;
use localbridge_lib::settings::SettingsStore;
use localbridge_lib::tray::{
    MAIN_WINDOW_LABEL, ensure_main_window, install_tray, sync_main_webview_to_client,
};
use tauri::{Manager, WindowEvent};
#[cfg(debug_assertions)]
use std::sync::mpsc::{self, Receiver};
#[cfg(debug_assertions)]
use tauri::{PhysicalSize, WebviewWindow};

fn main() {
    #[cfg(debug_assertions)]
    if let Some(view) = resize_e2e_view() {
        run_resize_e2e(view);
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
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = window.hide();
        }
        WindowEvent::Resized(client_size) => {
            let _ = sync_main_webview_to_client(window.app_handle(), *client_size);
        }
        _ => {}
    }
}

#[cfg(debug_assertions)]
#[derive(Clone, Copy)]
enum ResizeE2eView {
    Onboarding,
    Dashboard,
}

#[cfg(debug_assertions)]
impl ResizeE2eView {
    fn as_str(self) -> &'static str {
        match self {
            Self::Onboarding => "onboarding",
            Self::Dashboard => "dashboard",
        }
    }
}

#[cfg(debug_assertions)]
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResizeE2eRect {
    width: f64,
    height: f64,
}

#[cfg(debug_assertions)]
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResizeE2eMetrics {
    inner_width: f64,
    inner_height: f64,
    dpr: f64,
    document_client_width: f64,
    document_client_height: f64,
    body: Option<ResizeE2eRect>,
    root: Option<ResizeE2eRect>,
    dashboard: Option<ResizeE2eRect>,
    onboarding: Option<ResizeE2eRect>,
    onboarding_padding_top: Option<String>,
    view: String,
}

#[cfg(debug_assertions)]
const RESIZE_E2E_METRICS_SCRIPT: &str = r#"
(() => {
  const rect = (element) => element ? (() => {
    const value = element.getBoundingClientRect();
    return { width: value.width, height: value.height };
  })() : null;
  const dashboard = document.querySelector('.shell');
  const onboarding = document.querySelector('.onboarding-shell');
  const metrics = {
    innerWidth: window.innerWidth,
    innerHeight: window.innerHeight,
    dpr: window.devicePixelRatio,
    documentClientWidth: document.documentElement.clientWidth,
    documentClientHeight: document.documentElement.clientHeight,
    body: rect(document.body),
    root: rect(document.getElementById('root')),
    dashboard: rect(dashboard),
    onboarding: rect(onboarding),
    onboardingPaddingTop: onboarding ? getComputedStyle(onboarding).paddingTop : null,
    view: dashboard ? 'dashboard' : onboarding ? 'onboarding' : 'other'
  };
  void window.__TAURI_INTERNALS__.invoke('resize_e2e_report', {
    metrics: JSON.stringify(metrics)
  });
})();
"#;

#[cfg(debug_assertions)]
fn resize_e2e_view() -> Option<ResizeE2eView> {
    match std::env::var("LOCALBRIDGE_RESIZE_E2E_VIEW").ok()?.as_str() {
        "onboarding" => Some(ResizeE2eView::Onboarding),
        "dashboard" => Some(ResizeE2eView::Dashboard),
        _ => None,
    }
}

#[cfg(debug_assertions)]
fn run_resize_e2e(view: ResizeE2eView) {
    localbridge_lib::build_app()
        .setup(move |app| {
            let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
            let app_data_dir = app.path().app_data_dir()?;
            let store = SettingsStore::new(app_data_dir.join("settings.json"));
            let mut data = store
                .load()
                .map_err(|error| std::io::Error::other(format!("resize E2E settings load: {error:?}")))?;
            data.settings.onboarding_complete = matches!(view, ResizeE2eView::Dashboard);
            store
                .save(&data)
                .map_err(|error| std::io::Error::other(format!("resize E2E settings save: {error:?}")))?;
            app.manage(lifecycle);
            let (metrics_tx, metrics_rx) = mpsc::channel::<String>();
            app.manage(localbridge_lib::ResizeE2eMetricsSink::new(metrics_tx));
            let window = ensure_main_window(app.handle())?;
            let driver_window = window.clone();
            let driver_app = app.handle().clone();
            std::thread::spawn(move || {
                let result = execute_resize_e2e(&driver_window, view, &metrics_rx);
                match result {
                    Ok(summary) => {
                        println!("LB016_REAL_RESIZE_E2E=PASS view={} {summary}", view.as_str());
                        driver_app.exit(0);
                    }
                    Err(error) => {
                        eprintln!("LB016_REAL_RESIZE_E2E=FAIL view={} error={error}", view.as_str());
                        driver_app.exit(2);
                    }
                }
            });
            Ok(())
        })
        .on_window_event(handle_main_window_event)
        .run(tauri::generate_context!())
        .expect("LocalBridge resize E2E failed");
}

#[cfg(debug_assertions)]
fn execute_resize_e2e(
    window: &WebviewWindow<tauri::Wry>,
    view: ResizeE2eView,
    metrics_rx: &Receiver<String>,
) -> Result<String, String> {
    let small = resize_e2e_sample(
        window,
        view,
        metrics_rx,
        Some(PhysicalSize::new(1100, 760)),
        false,
    )?;
    let large = resize_e2e_sample(
        window,
        view,
        metrics_rx,
        Some(PhysicalSize::new(1800, 1200)),
        false,
    )?;
    let maximized = resize_e2e_sample(window, view, metrics_rx, None, true)?;

    if large.inner_width <= small.inner_width + 150.0
        || large.inner_height <= small.inner_height + 120.0
    {
        return Err("native resize did not materially change the WebView viewport".into());
    }

    match view {
        ResizeE2eView::Dashboard => {
            let small_shell = small.dashboard.as_ref().ok_or("dashboard shell missing at small size")?;
            let large_shell = large.dashboard.as_ref().ok_or("dashboard shell missing at large size")?;
            if large_shell.width <= small_shell.width + 120.0 {
                return Err("Dashboard shell did not grow with the WebView viewport".into());
            }
            if large.inner_width > 900.0 && large_shell.width <= 760.0 {
                return Err("Dashboard returned to the obsolete 760px narrow-column layout".into());
            }
        }
        ResizeE2eView::Onboarding => {
            for (label, metrics) in [("small", &small), ("large", &large), ("maximized", &maximized)] {
                let shell = metrics
                    .onboarding
                    .as_ref()
                    .ok_or_else(|| format!("onboarding shell missing at {label} size"))?;
                if (shell.width - metrics.inner_width).abs() > 1.0
                    || (shell.height - metrics.inner_height).abs() > 1.0
                {
                    return Err(format!("onboarding shell does not match viewport at {label} size"));
                }
            }
            if small.inner_height <= 560.0
                && small.onboarding_padding_top.as_deref() != Some("12px")
            {
                return Err(format!(
                    "small-height onboarding spacing did not activate: {:?}",
                    small.onboarding_padding_top
                ));
            }
            if large.inner_height > 560.0
                && large.onboarding_padding_top.as_deref() != Some("28px")
            {
                return Err(format!(
                    "large-height onboarding spacing did not restore: {:?}",
                    large.onboarding_padding_top
                ));
            }
        }
    }

    Ok(format!(
        "small={}x{} large={}x{} maximized={}x{} native_webview_sync=true maximize=true",
        small.inner_width.round(), small.inner_height.round(),
        large.inner_width.round(), large.inner_height.round(),
        maximized.inner_width.round(), maximized.inner_height.round()
    ))
}

#[cfg(debug_assertions)]
fn resize_e2e_sample(
    window: &WebviewWindow<tauri::Wry>,
    view: ResizeE2eView,
    metrics_rx: &Receiver<String>,
    size: Option<PhysicalSize<u32>>,
    maximize: bool,
) -> Result<ResizeE2eMetrics, String> {
    if maximize {
        window.maximize().map_err(|error| format!("maximize: {error}"))?;
    } else if let Some(size) = size {
        window.unmaximize().map_err(|error| format!("unmaximize: {error}"))?;
        window.set_size(size).map_err(|error| format!("set native size: {error}"))?;
    }
    std::thread::sleep(std::time::Duration::from_millis(350));
    let native = window.inner_size().map_err(|error| format!("native inner_size: {error}"))?;
    let metrics = collect_resize_e2e_metrics(window, view, metrics_rx)?;
    assert_resize_e2e_viewport_sync(native, &metrics)?;
    Ok(metrics)
}

#[cfg(debug_assertions)]
fn collect_resize_e2e_metrics(
    window: &WebviewWindow<tauri::Wry>,
    view: ResizeE2eView,
    metrics_rx: &Receiver<String>,
) -> Result<ResizeE2eMetrics, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    let mut last_payload = String::new();
    while std::time::Instant::now() < deadline {
        let _ = window.eval(RESIZE_E2E_METRICS_SCRIPT);
        if let Ok(payload) = metrics_rx.recv_timeout(std::time::Duration::from_millis(150)) {
            last_payload = payload;
            if let Ok(metrics) = serde_json::from_str::<ResizeE2eMetrics>(&last_payload) {
                if metrics.view == view.as_str() {
                    return Ok(metrics);
                }
            }
        }
    }
    Err(format!(
        "timed out waiting for live WebView metrics for {}; last payload={last_payload}",
        view.as_str()
    ))
}

#[cfg(debug_assertions)]
fn assert_resize_e2e_viewport_sync(
    native: PhysicalSize<u32>,
    metrics: &ResizeE2eMetrics,
) -> Result<(), String> {
    const TOLERANCE: f64 = 4.0;
    let webview_physical_width = metrics.inner_width * metrics.dpr;
    let webview_physical_height = metrics.inner_height * metrics.dpr;
    if (f64::from(native.width) - webview_physical_width).abs() > TOLERANCE
        || (f64::from(native.height) - webview_physical_height).abs() > TOLERANCE
    {
        return Err(format!("native client {}x{} != WebView viewport {:.1}x{:.1}", native.width, native.height, webview_physical_width, webview_physical_height));
    }
    let client_width_gap = metrics.inner_width - metrics.document_client_width;
    let client_height_gap = metrics.inner_height - metrics.document_client_height;
    if metrics.document_client_width <= 0.0
        || metrics.document_client_height <= 0.0
        || client_width_gap < -1.0
        || client_height_gap < -1.0
        || client_width_gap > 32.0
        || client_height_gap > 32.0
    {
        return Err(format!(
            "document layout viewport {}x{} is not a valid scrollbar-bounded subset of window viewport {}x{}",
            metrics.document_client_width,
            metrics.document_client_height,
            metrics.inner_width,
            metrics.inner_height
        ));
    }
    for (label, rect) in [("body", metrics.body.as_ref()), ("#root", metrics.root.as_ref())] {
        let rect = rect.ok_or_else(|| format!("{label} rect missing"))?;
        if rect.width + 1.0 < metrics.document_client_width
            || rect.height + 1.0 < metrics.document_client_height
        {
            return Err(format!(
                "{label} {}x{} does not cover document layout viewport {}x{}",
                rect.width,
                rect.height,
                metrics.document_client_width,
                metrics.document_client_height
            ));
        }
    }
    Ok(())
}
