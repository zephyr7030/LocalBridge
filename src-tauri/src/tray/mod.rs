use std::fmt;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Rect, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::app::DesktopLifecycle;

pub const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "localbridge-tray";
const MENU_OPEN_ID: &str = "open";
const MENU_EXIT_ID: &str = "exit";
const TRAY_ICON_CROP_PERCENT: u32 = 90;

#[derive(Debug)]
pub enum TraySetupError {
    Tauri(tauri::Error),
    MissingFrozenApplicationIcon,
}

impl fmt::Display for TraySetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tauri(error) => write!(f, "tray setup failed: {error}"),
            Self::MissingFrozenApplicationIcon => {
                f.write_str("frozen LocalBridge application icon is unavailable")
            }
        }
    }
}

impl std::error::Error for TraySetupError {}

impl From<tauri::Error> for TraySetupError {
    fn from(value: tauri::Error) -> Self {
        Self::Tauri(value)
    }
}

pub fn ensure_main_window<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<WebviewWindow<R>, tauri::Error> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window.show()?;
        window.set_focus()?;
        return Ok(window);
    }

    let window =
        WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
            .title("LocalBridge")
            .inner_size(900.0, 620.0)
            .min_inner_size(720.0, 500.0)
            .resizable(true)
            .build()?;
    sync_main_webview_to_client(app, window.inner_size()?)?;
    window.show()?;
    window.set_focus()?;
    Ok(window)
}

pub fn sync_main_webview_to_client<R: Runtime>(
    app: &AppHandle<R>,
    client_size: PhysicalSize<u32>,
) -> Result<(), tauri::Error> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let webview: &tauri::webview::Webview<R> = window.as_ref();
        webview.set_bounds(Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: client_size.into(),
        })?;
    }
    Ok(())
}

pub fn install_tray<R: Runtime>(app: &AppHandle<R>) -> Result<(), TraySetupError> {
    let open = MenuItem::with_id(app, MENU_OPEN_ID, "打开 LocalBridge", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, MENU_EXIT_ID, "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &exit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or(TraySetupError::MissingFrozenApplicationIcon)?;
    let icon = tray_icon_from_frozen(&icon);

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_OPEN_ID => {
                let _ = ensure_main_window(app);
            }
            MENU_EXIT_ID => {
                if let Some(lifecycle) = app.try_state::<DesktopLifecycle>() {
                    let _ = lifecycle.shutdown();
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let _ = ensure_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn tray_icon_from_frozen(icon: &Image<'_>) -> Image<'static> {
    let width = icon.width();
    let height = icon.height();
    let rgba = icon.rgba();
    if width < 4
        || height < 4
        || rgba.len() != width as usize * height as usize * 4
    {
        return icon.clone().to_owned();
    }

    let crop_width = (width * TRAY_ICON_CROP_PERCENT / 100).max(1);
    let crop_height = (height * TRAY_ICON_CROP_PERCENT / 100).max(1);
    let left = (width - crop_width) / 2;
    let top = (height - crop_height) / 2;
    let row_bytes = crop_width as usize * 4;
    let mut cropped = Vec::with_capacity(row_bytes * crop_height as usize);
    for y in 0..crop_height {
        let start = (((top + y) * width + left) * 4) as usize;
        cropped.extend_from_slice(&rgba[start..start + row_bytes]);
    }
    Image::new(&cropped, crop_width, crop_height).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_icon_zooms_frozen_source_without_replacing_asset() {
        let mut rgba = vec![0u8; 20 * 20 * 4];
        for y in 0..20usize {
            for x in 0..20usize {
                let offset = (y * 20 + x) * 4;
                rgba[offset] = x as u8;
                rgba[offset + 1] = y as u8;
                rgba[offset + 3] = 255;
            }
        }
        let source = Image::new(&rgba, 20, 20);

        let zoomed = tray_icon_from_frozen(&source);

        assert_eq!(zoomed.width(), 18);
        assert_eq!(zoomed.height(), 18);
        assert_eq!(&zoomed.rgba()[..4], &[1, 1, 0, 255]);
    }
}
