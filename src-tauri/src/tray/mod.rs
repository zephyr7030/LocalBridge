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
const TRAY_LOGICAL_ICON_SIZE: f64 = 16.0;
const FROZEN_TRAY_ICON_ICO: &[u8] = include_bytes!("../../../assets/icons/localbridge-tray.ico");
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug)]
pub enum TraySetupError {
    Tauri(tauri::Error),
    InvalidFrozenIcon(&'static str),
}

impl fmt::Display for TraySetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tauri(error) => write!(f, "tray setup failed: {error}"),
            Self::InvalidFrozenIcon(reason) => {
                write!(f, "invalid frozen LocalBridge tray icon: {reason}")
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
        sync_main_webview_to_client(app, window.inner_size()?)?;
        window.show()?;
        window.set_focus()?;
        return Ok(window);
    }

    let window =
        WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
            .title("LocalBridge")
            .visible(false)
            .inner_size(780.0, 620.0)
            .min_inner_size(780.0, 620.0)
            .max_inner_size(780.0, 620.0)
            .resizable(false)
            .maximizable(false)
            .decorations(false)
            .build()?;
    sync_main_webview_to_client(app, window.inner_size()?)?;
    window.center()?;
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
    let scale_factor = app
        .primary_monitor()?
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1.0);
    let icon = tray_icon_from_frozen_ico(FROZEN_TRAY_ICON_ICO, scale_factor)?;

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
                    let backend = lifecycle.backend_handle();
                    let exit_app = app.clone();
                    if backend
                        .spawn_shutdown_then(move |_| exit_app.exit(0))
                        .is_ok()
                    {
                        return;
                    }
                }
                app.exit(1);
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

#[derive(Clone, Copy, Debug)]
struct FrozenIcoFrame<'a> {
    size: u32,
    bytes: &'a [u8],
}

fn tray_icon_from_frozen_ico(
    ico: &'static [u8],
    scale_factor: f64,
) -> Result<Image<'static>, TraySetupError> {
    let frame = select_frozen_ico_frame(ico, scale_factor)?;
    let image = Image::from_bytes(frame.bytes)?;
    if image.width() != frame.size || image.height() != frame.size {
        return Err(TraySetupError::InvalidFrozenIcon(
            "decoded frame dimensions do not match ICO directory",
        ));
    }
    Ok(image.to_owned())
}

fn select_frozen_ico_frame(
    ico: &[u8],
    scale_factor: f64,
) -> Result<FrozenIcoFrame<'_>, TraySetupError> {
    if ico.len() < 6
        || u16::from_le_bytes([ico[0], ico[1]]) != 0
        || u16::from_le_bytes([ico[2], ico[3]]) != 1
    {
        return Err(TraySetupError::InvalidFrozenIcon("invalid ICO header"));
    }
    let count = usize::from(u16::from_le_bytes([ico[4], ico[5]]));
    let directory_end = 6usize
        .checked_add(
            count
                .checked_mul(16)
                .ok_or(TraySetupError::InvalidFrozenIcon("ICO directory overflow"))?,
        )
        .ok_or(TraySetupError::InvalidFrozenIcon("ICO directory overflow"))?;
    if count == 0 || directory_end > ico.len() {
        return Err(TraySetupError::InvalidFrozenIcon("truncated ICO directory"));
    }

    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let target = (TRAY_LOGICAL_ICON_SIZE * scale).round().max(1.0) as u32;
    let mut best_above: Option<FrozenIcoFrame<'_>> = None;
    let mut largest_below: Option<FrozenIcoFrame<'_>> = None;

    for index in 0..count {
        let entry = 6 + index * 16;
        let width = if ico[entry] == 0 {
            256
        } else {
            u32::from(ico[entry])
        };
        let height = if ico[entry + 1] == 0 {
            256
        } else {
            u32::from(ico[entry + 1])
        };
        if width != height {
            continue;
        }
        let bytes_len = u32::from_le_bytes(
            ico[entry + 8..entry + 12]
                .try_into()
                .expect("fixed ICO directory slice"),
        ) as usize;
        let bytes_offset = u32::from_le_bytes(
            ico[entry + 12..entry + 16]
                .try_into()
                .expect("fixed ICO directory slice"),
        ) as usize;
        let bytes_end = match bytes_offset.checked_add(bytes_len) {
            Some(end) if end <= ico.len() => end,
            _ => continue,
        };
        let bytes = &ico[bytes_offset..bytes_end];
        if !bytes.starts_with(PNG_SIGNATURE) {
            continue;
        }
        let frame = FrozenIcoFrame { size: width, bytes };
        if width >= target {
            if best_above.is_none_or(|current| width < current.size) {
                best_above = Some(frame);
            }
        } else if largest_below.is_none_or(|current| width > current.size) {
            largest_below = Some(frame);
        }
    }

    best_above
        .or(largest_below)
        .ok_or(TraySetupError::InvalidFrozenIcon(
            "no usable PNG frame in frozen ICO",
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_tray_icon_selects_native_dpi_frames() {
        assert_eq!(
            select_frozen_ico_frame(FROZEN_TRAY_ICON_ICO, 1.0)
                .unwrap()
                .size,
            16
        );
        assert_eq!(
            select_frozen_ico_frame(FROZEN_TRAY_ICON_ICO, 1.25)
                .unwrap()
                .size,
            20
        );
        assert_eq!(
            select_frozen_ico_frame(FROZEN_TRAY_ICON_ICO, 1.5)
                .unwrap()
                .size,
            24
        );
        assert_eq!(
            select_frozen_ico_frame(FROZEN_TRAY_ICON_ICO, 2.0)
                .unwrap()
                .size,
            32
        );
    }

    #[test]
    fn frozen_tray_icon_decodes_selected_frame_without_resampling() {
        let image = tray_icon_from_frozen_ico(FROZEN_TRAY_ICON_ICO, 1.5).unwrap();
        assert_eq!((image.width(), image.height()), (24, 24));
    }
}
