use serde::Serialize;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows_sys::Win32::UI::Shell::{
    BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW,
    SHGetPathFromIDListW, ShellExecuteW,
};

use crate::app::{DesktopLifecycle, STARTUP_PROFILE_FILE_NAME, StartupProfileStore};
use crate::commands::ui;
use crate::credentials::{CredentialStore, SecretString, WindowsCredentialStore};
use crate::settings::SettingsStore;
use crate::state::RuntimeState;

pub const OPENAI_TUNNEL_SETTINGS_URL: &str =
    "https://platform.openai.com/settings/organization/tunnels";
pub const OPENAI_API_KEYS_URL: &str = "https://platform.openai.com/api-keys";
pub const CHATGPT_PLUGINS_SETTINGS_URL: &str = "https://chatgpt.com/plugins#settings/Plugins";
pub const CHATGPT_CUSTOM_CONNECTOR_URL: &str = "https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingReadiness {
    local_environment: bool,
    coding_service: bool,
    openai_tunnel: bool,
}

impl OnboardingReadiness {
    fn all_ready(&self) -> bool {
        self.local_environment && self.coding_service && self.openai_tunnel
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingState {
    complete: bool,
    connection_configured: bool,
    runtime_key_saved: bool,
    runtime_key_length: Option<usize>,
    tunnel_id: Option<String>,
    readiness: OnboardingReadiness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorEndpointProjection {
    endpoint: Option<String>,
}

#[tauri::command]
pub async fn get_onboarding_state(app: AppHandle) -> Result<OnboardingState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        project_state(&app, &lifecycle)
    })
    .await
    .map_err(|_| "首次设置状态后台任务异常".to_string())?
}

#[tauri::command]
pub async fn save_onboarding_connection(
    tunnel_id: String,
    runtime_key: String,
    app: AppHandle,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let app_data = app_data_dir(&app)?;
        let profile_store = StartupProfileStore::new(app_data.join(STARTUP_PROFILE_FILE_NAME));
        let mut profile = profile_store
            .load()
            .map_err(|_| "无法读取 OpenAI 连接设置".to_string())?;
        profile
            .set_tunnel_id(tunnel_id)
            .map_err(|_| "Tunnel ID 格式无效".to_string())?;
        profile_store
            .save(&profile)
            .map_err(|_| "无法保存 Tunnel ID".to_string())?;

        if runtime_key.trim().is_empty() {
            let metadata = WindowsCredentialStore::default()
                .runtime_api_key_metadata()
                .map_err(|_| "无法读取Runtime API Key状态".to_string())?;
            if !metadata.has_runtime_key {
                return Err("请输入Runtime API Key".to_string());
            }
        } else {
            let secret = SecretString::new(runtime_key)
                .map_err(|_| "Runtime API Key格式无效".to_string())?;
            WindowsCredentialStore::default()
                .save_runtime_api_key(&secret)
                .map_err(|_| "无法安全保存Runtime API Key".to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|_| "OpenAI 连接保存后台任务异常".to_string())?
}

#[tauri::command]
pub async fn open_openai_tunnel_settings() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| open_allowlisted_url(OPENAI_TUNNEL_SETTINGS_URL))
        .await
        .map_err(|_| "打开 Tunnel ID 设置后台任务异常".to_string())?
}

#[tauri::command]
pub async fn open_openai_api_keys() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| open_allowlisted_url(OPENAI_API_KEYS_URL))
        .await
        .map_err(|_| "打开 Runtime API Key 设置后台任务异常".to_string())?
}

#[tauri::command]
pub async fn open_chatgpt_plugins_settings() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| open_allowlisted_url(CHATGPT_PLUGINS_SETTINGS_URL))
        .await
        .map_err(|_| "打开 ChatGPT插件设置后台任务异常".to_string())?
}

#[tauri::command]
pub async fn open_chatgpt_custom_connector_settings() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| open_allowlisted_url(CHATGPT_CUSTOM_CONNECTOR_URL))
        .await
        .map_err(|_| "打开插件管理页后台任务异常".to_string())?
}

#[tauri::command]
pub fn get_connector_endpoint(
    lifecycle: State<'_, DesktopLifecycle>,
) -> ConnectorEndpointProjection {
    ConnectorEndpointProjection {
        endpoint: lifecycle
            .connector_endpoint()
            .map(|endpoint| endpoint.as_str().to_owned()),
    }
}

#[tauri::command]
pub async fn choose_onboarding_workspace_folder() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(pick_windows_workspace_folder)
        .await
        .map_err(|_| "无法打开文件夹选择器".to_string())?
}

#[tauri::command]
pub async fn prepare_onboarding_project(
    mode: String,
    project_id: Option<String>,
    selected_folder: Option<String>,
    app: AppHandle,
) -> Result<OnboardingState, String> {
    ui::set_permission_mode(mode, app.clone()).await?;
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(folder) = selected_folder.filter(|value| !value.trim().is_empty()) {
            let lifecycle = app.state::<DesktopLifecycle>();
            ui::add_project_blocking(folder, Some(false), &app, &lifecycle)?;
        } else if let Some(id) = project_id.filter(|value| !value.trim().is_empty()) {
            let lifecycle = app.state::<DesktopLifecycle>();
            ui::select_project_blocking(id, &app, &lifecycle)?;
        } else {
            return Err("请选择项目文件夹".to_string());
        }

        for _ in 0..120 {
            let lifecycle = app.state::<DesktopLifecycle>();
            let current = project_state(&app, &lifecycle)?;
            if current.readiness.all_ready() {
                return Ok(current);
            }
            if matches!(lifecycle.runtime_snapshot().state, RuntimeState::Faulted(_)) {
                return Err("本地服务启动失败，请检查启动检查状态".to_string());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Err("本地服务未在预期时间内就绪，请重试".to_string())
    })
    .await
    .map_err(|_| "项目准备后台任务异常".to_string())?
}

#[tauri::command]
pub async fn complete_onboarding(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = app.state::<DesktopLifecycle>();
        let current = project_state(&app, &lifecycle)?;
        if !current.readiness.all_ready() {
            return Err("本地服务尚未全部就绪".to_string());
        }
        let store = SettingsStore::new(app_data_dir(&app)?.join("settings.json"));
        let mut data = store.load().map_err(|_| "无法读取设置".to_string())?;
        data.settings.onboarding_complete = true;
        store
            .save(&data)
            .map_err(|_| "无法保存设置完成状态".to_string())
    })
    .await
    .map_err(|_| "首次设置完成后台任务异常".to_string())?
}

fn project_state(app: &AppHandle, lifecycle: &DesktopLifecycle) -> Result<OnboardingState, String> {
    let app_data = app_data_dir(app)?;
    let data = SettingsStore::new(app_data.join("settings.json"))
        .load()
        .map_err(|_| "无法读取设置".to_string())?;
    let profile = StartupProfileStore::new(app_data.join(STARTUP_PROFILE_FILE_NAME))
        .load()
        .map_err(|_| "无法读取 OpenAI 连接设置".to_string())?;
    let tunnel_id = profile
        .validated_tunnel_id()
        .map_err(|_| "Tunnel ID 格式无效".to_string())?;
    let connection_configured = tunnel_id.is_some();
    let runtime_key = WindowsCredentialStore::default()
        .read_runtime_api_key()
        .map_err(|_| "无法读取Runtime API Key状态".to_string())?;
    let runtime_key_saved = runtime_key.is_some();
    let runtime_key_length = runtime_key
        .as_ref()
        .map(|secret| secret.expose_secret().chars().count());
    Ok(OnboardingState {
        complete: data.settings.onboarding_complete,
        connection_configured,
        runtime_key_saved,
        runtime_key_length,
        tunnel_id: tunnel_id.map(|value| value.expose().to_owned()),
        readiness: readiness(lifecycle),
    })
}

fn readiness(lifecycle: &DesktopLifecycle) -> OnboardingReadiness {
    let root = production_install_root();
    let local_environment = root.as_ref().is_ok_and(|root| {
        root.join("runtime/python/python.exe").is_file()
            && root
                .join("runtime/coding-tools-mcp/coding_tools_mcp/__init__.py")
                .is_file()
    });
    let state = lifecycle.runtime_snapshot().state;
    let coding_service = matches!(
        state,
        RuntimeState::StartingTunnel | RuntimeState::WaitingTunnelReady | RuntimeState::Ready
    );
    let openai_tunnel = matches!(state, RuntimeState::Ready);
    OnboardingReadiness {
        local_environment,
        coding_service,
        openai_tunnel,
    }
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|_| "无法定位应用数据目录".to_string())
}

fn production_install_root() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "无法定位本地运行环境".to_string())
    }
    #[cfg(not(debug_assertions))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .ok_or_else(|| "无法定位本地运行环境".to_string())
    }
}

fn open_allowlisted_url(url: &str) -> Result<(), String> {
    if !matches!(
        url,
        OPENAI_TUNNEL_SETTINGS_URL
            | OPENAI_API_KEYS_URL
            | CHATGPT_PLUGINS_SETTINGS_URL
            | CHATGPT_CUSTOM_CONNECTOR_URL
    ) {
        return Err("不允许打开此地址".to_string());
    }
    let operation = wide("open");
    let target = wide(url);
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result as isize <= 32 {
        return Err("无法使用系统浏览器打开页面".to_string());
    }
    Ok(())
}

fn pick_windows_workspace_folder() -> Result<Option<String>, String> {
    let initialized = unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
    if initialized < 0 {
        return Err("无法初始化文件夹选择器".to_string());
    }

    let title = wide("选择项目文件夹");
    let mut display_name = [0u16; 260];
    let browse = BROWSEINFOW {
        hwndOwner: null_mut(),
        pidlRoot: null_mut(),
        pszDisplayName: display_name.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };
    let pidl = unsafe { SHBrowseForFolderW(&browse) };
    let result = if pidl.is_null() {
        Ok(None)
    } else {
        let mut path = [0u16; 260];
        let ok = unsafe { SHGetPathFromIDListW(pidl, path.as_mut_ptr()) } != 0;
        unsafe { CoTaskMemFree(pidl.cast()) };
        if !ok {
            Err("无法读取所选文件夹".to_string())
        } else {
            let end = path
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(path.len());
            let selected = String::from_utf16_lossy(&path[..end]);
            if selected.trim().is_empty() {
                Err("所选文件夹无效".to_string())
            } else {
                Ok(Some(selected))
            }
        }
    };
    unsafe { CoUninitialize() };
    result
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{RuntimeComponent, RuntimeFault};

    #[test]
    fn browser_allowlist_is_fixed_to_the_four_setup_destinations() {
        assert_eq!(
            OPENAI_TUNNEL_SETTINGS_URL,
            "https://platform.openai.com/settings/organization/tunnels"
        );
        assert_eq!(OPENAI_API_KEYS_URL, "https://platform.openai.com/api-keys");
        assert_eq!(
            CHATGPT_PLUGINS_SETTINGS_URL,
            "https://chatgpt.com/plugins#settings/Plugins"
        );
        assert_eq!(
            CHATGPT_CUSTOM_CONNECTOR_URL,
            "https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins"
        );
        assert!(open_allowlisted_url("https://example.invalid/").is_err());
    }

    #[test]
    fn readiness_requires_real_ready_state_for_openai_tunnel() {
        let ready = |state| {
            let coding = matches!(
                state,
                RuntimeState::StartingTunnel
                    | RuntimeState::WaitingTunnelReady
                    | RuntimeState::Ready
            );
            let tunnel = matches!(state, RuntimeState::Ready);
            (coding, tunnel)
        };
        assert_eq!(ready(RuntimeState::WaitingTunnelReady), (true, false));
        assert_eq!(ready(RuntimeState::Ready), (true, true));
        assert_eq!(
            ready(RuntimeState::Recovering {
                component: RuntimeComponent::CodingRuntime,
                attempt: 0,
            }),
            (false, false)
        );
        assert_eq!(ready(RuntimeState::Faulted(RuntimeFault::McpHealthTimeout)), (false, false));
        assert_eq!(ready(RuntimeState::Stopped), (false, false));
    }
}
