#![cfg_attr(windows, windows_subsystem = "windows")]

use localbridge_lib::app::{
    DesktopLifecycle, SingleInstanceAcquire, SingleInstanceGuard, StartupMode,
    configure_desktop_startup,
};
use localbridge_lib::domain::UpdateCheckTrigger;
use localbridge_lib::privilege::PrivilegeController;
use localbridge_lib::tray::{
    MAIN_WINDOW_LABEL, ensure_main_window, install_tray, sync_main_webview_to_client,
};
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
use tauri::{Manager, WindowEvent};

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
            let _ = lifecycle.start_update_check(UpdateCheckTrigger::Startup);
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
            let Some(lifecycle) = window.app_handle().try_state::<DesktopLifecycle>() else {
                let _ = window.hide();
                return;
            };
            if lifecycle.close_window_continue_running() {
                let _ = window.hide();
                return;
            }
            let backend = lifecycle.backend_handle();
            let app = window.app_handle().clone();
            if backend
                .spawn_shutdown_then(move |_| app.exit(0))
                .is_err()
            {
                window.app_handle().exit(1);
            }
        }
        WindowEvent::ScaleFactorChanged { .. } => {
            if let Ok(client_size) = window.inner_size() {
                let _ = sync_main_webview_to_client(window.app_handle(), client_size);
            }
        }
        _ => {}
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
    dashboard_overlay_count_before_settings: usize,
    dashboard_card_background_before_settings: Option<String>,
    dashboard_card_border_width_before_settings: Option<String>,
    dashboard_card_box_shadow_before_settings: Option<String>,
    settings_replace_lefts: Vec<f64>,
    settings_sheet_overflowing: bool,
    settings_sheet_border_radius: Option<String>,
    settings_sheet_clip_path: Option<String>,
    settings_sheet_scrollbar_gutter: Option<String>,
    settings_sheet_overflow_y: Option<String>,
    settings_scrollbar_width: Option<String>,
    settings_scrollbar_track_display: Option<String>,
    settings_scrollbar_track_margin_top: Option<String>,
    settings_scrollbar_track_margin_bottom: Option<String>,
    settings_scrollbar_thumb_display: Option<String>,
    settings_scrollbar_button_display: Option<String>,
    settings_scrollbar_button_width: Option<String>,
    settings_scrollbar_button_height: Option<String>,
    settings_scrollbar_button_appearance: Option<String>,
    settings_sheet_scroll_range: f64,
    settings_sheet_scroll_top: f64,
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
  const dashboardCard = dashboard ? document.querySelector('.card') : null;
  const dashboardCardStyle = dashboardCard ? getComputedStyle(dashboardCard) : null;
  if (dashboard && window.__LOCALBRIDGE_E2E_INITIAL_DASHBOARD_SURFACE__ === undefined) {
    window.__LOCALBRIDGE_E2E_INITIAL_DASHBOARD_SURFACE__ = {
      overlayCount: document.querySelectorAll('.sheet-backdrop,.dialog-backdrop').length,
      background: dashboardCardStyle?.backgroundColor || null,
      borderWidth: dashboardCardStyle?.borderTopWidth || null,
      boxShadow: dashboardCardStyle?.boxShadow || null,
    };
  }
  const initialDashboardSurface = window.__LOCALBRIDGE_E2E_INITIAL_DASHBOARD_SURFACE__ || {
    overlayCount: 0,
    background: null,
    borderWidth: null,
    boxShadow: null,
  };
  if (dashboard && !document.querySelector('.sheet')) {
    const settings = Array.from(document.querySelectorAll('.top-actions button'))
      .find((element) => (element.textContent || '').trim() === '设置');
    settings?.click();
  }
  const sheet = document.querySelector('.sheet');
  if (dashboard && sheet) sheet.style.maxHeight = '180px';
  const sheetStyle = sheet ? getComputedStyle(sheet) : null;
  const scrollbarStyle = sheet ? getComputedStyle(sheet, '::-webkit-scrollbar') : null;
  const scrollbarTrackStyle = sheet ? getComputedStyle(sheet, '::-webkit-scrollbar-track') : null;
  const scrollbarThumbStyle = sheet ? getComputedStyle(sheet, '::-webkit-scrollbar-thumb') : null;
  const scrollbarButtonStyle = sheet ? getComputedStyle(sheet, '::-webkit-scrollbar-button') : null;
  const settingsSheetScrollRange = sheet ? Math.max(0, sheet.scrollHeight - sheet.clientHeight) : 0;
  if (sheet && settingsSheetScrollRange > 0) sheet.scrollTop = Math.min(24, settingsSheetScrollRange);
  const settingsReplaceLefts = Array.from(document.querySelectorAll('.settings-replace'))
    .map((element) => element.getBoundingClientRect().left);
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
    dashboardOverlayCountBeforeSettings: initialDashboardSurface.overlayCount,
    dashboardCardBackgroundBeforeSettings: initialDashboardSurface.background,
    dashboardCardBorderWidthBeforeSettings: initialDashboardSurface.borderWidth,
    dashboardCardBoxShadowBeforeSettings: initialDashboardSurface.boxShadow,
    settingsReplaceLefts,
    settingsSheetOverflowing: !!sheet && sheet.scrollHeight > sheet.clientHeight,
    settingsSheetBorderRadius: sheetStyle?.borderRadius || null,
    settingsSheetClipPath: sheetStyle?.clipPath || null,
    settingsSheetScrollbarGutter: sheetStyle?.scrollbarGutter || null,
    settingsSheetOverflowY: sheetStyle?.overflowY || null,
    settingsScrollbarWidth: scrollbarStyle?.width || null,
    settingsScrollbarTrackDisplay: scrollbarTrackStyle?.display || null,
    settingsScrollbarTrackMarginTop: scrollbarTrackStyle?.marginTop || null,
    settingsScrollbarTrackMarginBottom: scrollbarTrackStyle?.marginBottom || null,
    settingsScrollbarThumbDisplay: scrollbarThumbStyle?.display || null,
    settingsScrollbarButtonDisplay: scrollbarButtonStyle?.display || null,
    settingsScrollbarButtonWidth: scrollbarButtonStyle?.width || null,
    settingsScrollbarButtonHeight: scrollbarButtonStyle?.height || null,
    settingsScrollbarButtonAppearance: scrollbarButtonStyle?.webkitAppearance || scrollbarButtonStyle?.appearance || null,
    settingsSheetScrollRange,
    settingsSheetScrollTop: sheet?.scrollTop || 0,
    controls: Array.from(document.querySelectorAll('.window-control')).map((element) => element.getAttribute('aria-label') || ''),
    view: dashboard ? 'dashboard' : onboarding ? 'onboarding' : 'other'
  };
  void window.__TAURI_INTERNALS__.invoke('fixed_window_e2e_report', { metrics: JSON.stringify(metrics) });
})();
"#;

#[cfg(debug_assertions)]
fn fixed_window_e2e_view() -> Option<FixedWindowE2eView> {
    match std::env::var("LOCALBRIDGE_FIXED_WINDOW_E2E_VIEW")
        .ok()?
        .as_str()
    {
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
            let mut data = store.load().map_err(|error| {
                std::io::Error::other(format!("fixed-window E2E settings load: {error:?}"))
            })?;
            data.settings.onboarding_complete = matches!(view, FixedWindowE2eView::Dashboard);
            store.save(&data).map_err(|error| {
                std::io::Error::other(format!("fixed-window E2E settings save: {error:?}"))
            })?;
            app.manage(lifecycle);
            let (metrics_tx, metrics_rx) = mpsc::channel::<String>();
            app.manage(FixedWindowE2eMetricsSink::new(metrics_tx));
            let window = ensure_main_window(app.handle())?;
            let driver_window = window.clone();
            let driver_app = app.handle().clone();
            std::thread::spawn(move || {
                match execute_fixed_window_e2e(&driver_window, view, &metrics_rx) {
                    Ok(summary) => {
                        println!(
                            "LB016_FIXED_WINDOW_E2E=PASS view={} {summary}",
                            view.as_str()
                        );
                        driver_app.exit(0);
                    }
                    Err(error) => {
                        eprintln!(
                            "LB016_FIXED_WINDOW_E2E=FAIL view={} error={error}",
                            view.as_str()
                        );
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
    let physical = window
        .inner_size()
        .map_err(|error| format!("inner_size: {error}"))?;
    let scale = window
        .scale_factor()
        .map_err(|error| format!("scale_factor: {error}"))?;
    if !scale.is_finite() || scale <= 0.0 {
        return Err(format!("invalid native scale factor: {scale}"));
    }
    let logical_width = f64::from(physical.width) / scale;
    let logical_height = f64::from(physical.height) / scale;
    if (logical_width - 780.0).abs() > 2.0 || (logical_height - 620.0).abs() > 2.0 {
        return Err(format!(
            "native client is {logical_width:.1}x{logical_height:.1} logical ({}x{} physical at {scale}x), expected 780x620 logical",
            physical.width,
            physical.height
        ));
    }
    if window
        .is_resizable()
        .map_err(|error| format!("is_resizable: {error}"))?
    {
        return Err("native window remains resizable".into());
    }
    if window
        .is_maximizable()
        .map_err(|error| format!("is_maximizable: {error}"))?
    {
        return Err("native window remains maximizable".into());
    }
    if window
        .is_decorated()
        .map_err(|error| format!("is_decorated: {error}"))?
    {
        return Err("native window decorations are still enabled".into());
    }

    let outer_position = window
        .outer_position()
        .map_err(|error| format!("outer_position: {error}"))?;
    let outer_size = window
        .outer_size()
        .map_err(|error| format!("outer_size: {error}"))?;
    let monitor = window
        .current_monitor()
        .map_err(|error| format!("current_monitor: {error}"))?
        .ok_or("current monitor unavailable")?;
    let work_area = monitor.work_area();
    let expected_x = work_area.position.x
        + (i32::try_from(work_area.size.width).map_err(|_| "work area width overflow")?
            - i32::try_from(outer_size.width).map_err(|_| "outer width overflow")?) / 2;
    let expected_y = work_area.position.y
        + (i32::try_from(work_area.size.height).map_err(|_| "work area height overflow")?
            - i32::try_from(outer_size.height).map_err(|_| "outer height overflow")?) / 2;
    if (outer_position.x - expected_x).abs() > 3 || (outer_position.y - expected_y).abs() > 3 {
        return Err(format!(
            "first-created main window is not centered in monitor work area: actual=({}, {}) expected=({}, {})",
            outer_position.x, outer_position.y, expected_x, expected_y
        ));
    }

    let metrics = collect_fixed_window_e2e_metrics(window, view, metrics_rx)?;
    assert_fixed_window_e2e_metrics(&metrics, view)?;

    window
        .eval("document.querySelector('[aria-label=\"最小化\"]')?.click();")
        .map_err(|error| format!("click minimize: {error}"))?;
    wait_for_fixed_window_state(Duration::from_secs(4), || {
        window.is_minimized().ok() == Some(true)
    })
    .ok_or("custom minimize control did not minimize the native window")?;
    window
        .unminimize()
        .map_err(|error| format!("unminimize: {error}"))?;

    window
        .eval("document.querySelector('[aria-label=\"关闭\"]')?.click();")
        .map_err(|error| format!("click close: {error}"))?;
    wait_for_fixed_window_state(Duration::from_secs(4), || {
        window.is_visible().ok() == Some(false)
    })
    .ok_or("custom close control did not reach CloseRequested close-to-hide behavior")?;

    let dashboard_geometry = if matches!(view, FixedWindowE2eView::Dashboard) {
        format!(
            " settings_replace_delta={:.2}px rounded_scroll=true scrollbar_arrows=false scroll_surface=true",
            (metrics.settings_replace_lefts[0] - metrics.settings_replace_lefts[1]).abs()
        )
    } else {
        String::new()
    };
    Ok(format!(
        "logical={}x{} physical={}x{} webview={}x{} native_scale={} dpr={} decorations=false resizable=false maximizable=false chrome=edge-to-edge controls=drag,minimize,close minimize_click=true close_hide=true{}",
        logical_width.round(),
        logical_height.round(),
        physical.width,
        physical.height,
        metrics.inner_width.round(),
        metrics.inner_height.round(),
        scale,
        metrics.dpr,
        dashboard_geometry
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
                let dashboard_ready = !matches!(view, FixedWindowE2eView::Dashboard)
                    || (metrics.settings_replace_lefts.len() == 2
                        && metrics.settings_sheet_clip_path.is_some());
                if metrics.view == view.as_str() && dashboard_ready {
                    return Ok(metrics);
                }
            }
        }
    }
    Err(format!(
        "timed out waiting for live WebView metrics; last={last_payload}"
    ))
}

#[cfg(debug_assertions)]
fn assert_fixed_window_e2e_metrics(
    metrics: &FixedWindowE2eMetrics,
    view: FixedWindowE2eView,
) -> Result<(), String> {
    let chrome = metrics.chrome.as_ref().ok_or("window chrome missing")?;
    if metrics.chrome_count != 1 {
        return Err(format!(
            "expected exactly one window chrome, got {}",
            metrics.chrome_count
        ));
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
        return Err(format!(
            "custom titlebar does not follow the single chrome inner edge: {titlebar:?}"
        ));
    }
    let content_right_inset = metrics.inner_width - (content.left + content.width);
    let titlebar_bottom = titlebar.top + titlebar.height;
    if (content.left - titlebar.left).abs() > 0.5
        || (content_right_inset - titlebar_right_inset).abs() > 0.5
        || content.top + 0.5 < titlebar_bottom
        || content.top - titlebar_bottom > 2.0
    {
        return Err(format!(
            "window content is not directly below the custom titlebar: titlebar={titlebar:?} content={content:?}"
        ));
    }
    if metrics.controls != ["最小化".to_string(), "关闭".to_string()] {
        return Err(format!(
            "unexpected custom window controls: {:?}",
            metrics.controls
        ));
    }
    match view {
        FixedWindowE2eView::Onboarding => {
            let child = metrics
                .onboarding
                .as_ref()
                .ok_or("onboarding shell missing")?;
            if child.width > content.width + 1.0 || child.height > content.height + 1.0 {
                return Err("onboarding exceeds fixed chrome content area".into());
            }
        }
        FixedWindowE2eView::Dashboard => {
            let child = metrics
                .dashboard
                .as_ref()
                .ok_or("dashboard shell missing")?;
            if child.width > content.width + 1.0 || child.height < 1.0 {
                return Err("dashboard does not fit fixed chrome content area".into());
            }
            if metrics.dashboard_overlay_count_before_settings != 0 {
                return Err(format!(
                    "dashboard surface metrics were captured with an overlay present: {}",
                    metrics.dashboard_overlay_count_before_settings
                ));
            }
            let card_background = metrics
                .dashboard_card_background_before_settings
                .as_deref()
                .ok_or("dashboard card computed background missing")?;
            if card_background != "rgba(0, 0, 0, 0)" && card_background != "transparent" {
                return Err(format!("dashboard card still has an independent background: {card_background}"));
            }
            let card_border = metrics
                .dashboard_card_border_width_before_settings
                .as_deref()
                .ok_or("dashboard card computed border missing")?;
            if card_border != "0px" {
                return Err(format!("dashboard card still has an independent border: {card_border}"));
            }
            let card_shadow = metrics
                .dashboard_card_box_shadow_before_settings
                .as_deref()
                .ok_or("dashboard card computed shadow missing")?;
            if card_shadow != "none" && !card_shadow.is_empty() {
                return Err(format!("dashboard card still has an independent shadow: {card_shadow}"));
            }
            let replace_delta =
                (metrics.settings_replace_lefts[0] - metrics.settings_replace_lefts[1]).abs();
            if replace_delta > 1.0 {
                return Err(format!(
                    "Settings replacement buttons are not in one action column: delta={replace_delta:.2}px"
                ));
            }
            if !metrics.settings_sheet_overflowing {
                return Err("forced Settings sheet did not produce real overflow".into());
            }
            let radius = metrics
                .settings_sheet_border_radius
                .as_deref()
                .ok_or("Settings sheet computed border radius missing")?;
            if !radius.contains("20px") {
                return Err(format!("Settings sheet rounded corners drifted: {radius}"));
            }
            let clip = metrics
                .settings_sheet_clip_path
                .as_deref()
                .ok_or("Settings sheet computed clip path missing")?;
            if clip == "none" || !clip.contains("20px") {
                return Err(format!("Settings scrollbar is not clipped by rounded shell: {clip}"));
            }
            let gutter = metrics
                .settings_sheet_scrollbar_gutter
                .as_deref()
                .ok_or("Settings sheet scrollbar gutter missing")?;
            if !gutter.contains("stable") {
                return Err(format!("Settings scrollbar gutter is not stable/inset: {gutter}"));
            }
            let overflow = metrics
                .settings_sheet_overflow_y
                .as_deref()
                .ok_or("Settings sheet overflowY missing")?;
            if overflow != "auto" && overflow != "scroll" {
                return Err(format!("Settings sheet is not a real scroll surface: {overflow}"));
            }
            let scrollbar_width = metrics.settings_scrollbar_width.as_deref().unwrap_or("");
            if scrollbar_width.is_empty() || scrollbar_width == "0px" || scrollbar_width == "auto" {
                return Err(format!(
                    "Settings custom scrollbar width is not active: {scrollbar_width}"
                ));
            }
            let track_display = metrics
                .settings_scrollbar_track_display
                .as_deref()
                .unwrap_or("");
            let track_margin_top = metrics
                .settings_scrollbar_track_margin_top
                .as_deref()
                .unwrap_or("");
            let track_margin_bottom = metrics
                .settings_scrollbar_track_margin_bottom
                .as_deref()
                .unwrap_or("");
            let thumb_display = metrics
                .settings_scrollbar_thumb_display
                .as_deref()
                .unwrap_or("");
            if track_display == "none" || thumb_display == "none" {
                return Err(format!(
                    "Settings custom scrollbar lost track/thumb: track={track_display} thumb={thumb_display}"
                ));
            }
            let parse_px = |value: &str| {
                value
                    .strip_suffix("px")
                    .and_then(|number| number.parse::<f64>().ok())
                    .unwrap_or(-1.0)
            };
            if parse_px(track_margin_top) < 14.0 || parse_px(track_margin_bottom) < 14.0 {
                return Err(format!(
                    "Settings scrollbar track does not stay clear of rounded corners: top={track_margin_top} bottom={track_margin_bottom}"
                ));
            }
            let button_display = metrics
                .settings_scrollbar_button_display
                .as_deref()
                .unwrap_or("");
            let button_width = metrics
                .settings_scrollbar_button_width
                .as_deref()
                .unwrap_or("");
            let button_height = metrics
                .settings_scrollbar_button_height
                .as_deref()
                .unwrap_or("");
            let button_appearance = metrics
                .settings_scrollbar_button_appearance
                .as_deref()
                .unwrap_or("");
            let button_hidden = button_display == "none"
                && button_width == "0px"
                && button_height == "0px"
                && (button_appearance == "none" || button_appearance.is_empty());
            if !button_hidden {
                return Err(format!(
                    "Settings scrollbar button/arrow contract is not fully suppressed: display={button_display} width={button_width} height={button_height} appearance={button_appearance}"
                ));
            }
            if metrics.settings_sheet_scroll_range <= 0.0 {
                return Err("Settings sheet lost a real scroll range".into());
            }
            if metrics.settings_sheet_scroll_top <= 0.0 {
                return Err("Settings sheet no longer scrolls after arrow removal".into());
            }
        }
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn wait_for_fixed_window_state(
    timeout: Duration,
    mut predicate: impl FnMut() -> bool,
) -> Option<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return Some(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}
