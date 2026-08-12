use serde::Serialize;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use tauri::{AppHandle, Manager, State};
use windows_sys::Win32::UI::Shell::ShellExecuteW;

use crate::app::{DesktopLifecycle, STARTUP_PROFILE_FILE_NAME, StartupProfileStore};
use crate::credentials::{CredentialStore, SecretString, WindowsCredentialStore};
use crate::settings::SettingsStore;
use crate::state::RuntimeState;

pub const CHATGPT_MCP_SETTINGS_URL: &str = "https://chatgpt.com/";

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
    readiness: OnboardingReadiness,
}

#[tauri::command]
pub fn get_onboarding_state(app: AppHandle, lifecycle: State<'_, DesktopLifecycle>) -> Result<OnboardingState, String> {
    project_state(&app, &lifecycle)
}

#[tauri::command]
pub fn save_onboarding_connection(tunnel_id: String, runtime_key: String, app: AppHandle) -> Result<(), String> {
    let app_data = app_data_dir(&app)?;
    let profile_store = StartupProfileStore::new(app_data.join(STARTUP_PROFILE_FILE_NAME));
    let mut profile = profile_store.load().map_err(|_| "无法读取 OpenAI 连接设置".to_string())?;
    profile.set_tunnel_id(tunnel_id).map_err(|_| "Tunnel ID 格式无效".to_string())?;
    profile_store.save(&profile).map_err(|_| "无法保存 Tunnel ID".to_string())?;

    if runtime_key.trim().is_empty() {
        let metadata = WindowsCredentialStore::default().runtime_api_key_metadata().map_err(|_| "无法读取运行密钥状态".to_string())?;
        if !metadata.has_runtime_key {
            return Err("请输入运行密钥".to_string());
        }
    } else {
        let secret = SecretString::new(runtime_key).map_err(|_| "运行密钥格式无效".to_string())?;
        WindowsCredentialStore::default().save_runtime_api_key(&secret).map_err(|_| "无法安全保存运行密钥".to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn open_chatgpt_mcp_page() -> Result<(), String> {
    open_allowlisted_url(CHATGPT_MCP_SETTINGS_URL)
}

#[tauri::command]
pub fn complete_onboarding(app: AppHandle, lifecycle: State<'_, DesktopLifecycle>) -> Result<(), String> {
    let current = project_state(&app, &lifecycle)?;
    if !current.readiness.all_ready() {
        return Err("本地服务尚未全部就绪".to_string());
    }
    let store = SettingsStore::new(app_data_dir(&app)?.join("settings.json"));
    let mut data = store.load().map_err(|_| "无法读取设置".to_string())?;
    data.settings.onboarding_complete = true;
    store.save(&data).map_err(|_| "无法保存设置完成状态".to_string())
}

fn project_state(app: &AppHandle, lifecycle: &DesktopLifecycle) -> Result<OnboardingState, String> {
    let app_data = app_data_dir(app)?;
    let data = SettingsStore::new(app_data.join("settings.json")).load().map_err(|_| "无法读取设置".to_string())?;
    let profile = StartupProfileStore::new(app_data.join(STARTUP_PROFILE_FILE_NAME)).load().map_err(|_| "无法读取 OpenAI 连接设置".to_string())?;
    let connection_configured = profile.validated_tunnel_id().map_err(|_| "Tunnel ID 格式无效".to_string())?.is_some();
    let runtime_key_saved = WindowsCredentialStore::default().runtime_api_key_metadata().map_err(|_| "无法读取运行密钥状态".to_string())?.has_runtime_key;
    Ok(OnboardingState {
        complete: data.settings.onboarding_complete,
        connection_configured,
        runtime_key_saved,
        readiness: readiness(lifecycle),
    })
}

fn readiness(lifecycle: &DesktopLifecycle) -> OnboardingReadiness {
    let root = production_install_root();
    let local_environment = root.as_ref().is_ok_and(|root| {
        root.join("runtime/python/python.exe").is_file()
            && root.join("runtime/coding-tools-mcp/coding_tools_mcp/__init__.py").is_file()
    });
    let state = lifecycle.runtime_snapshot().state;
    let coding_service = matches!(state, RuntimeState::StartingTunnel | RuntimeState::WaitingTunnelReady | RuntimeState::Ready);
    let openai_tunnel = matches!(state, RuntimeState::Ready);
    OnboardingReadiness { local_environment, coding_service, openai_tunnel }
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|_| "无法定位应用数据目录".to_string())
}

fn production_install_root() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).ok_or_else(|| "无法定位本地运行环境".to_string())
    }
    #[cfg(not(debug_assertions))]
    {
        std::env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf)).ok_or_else(|| "无法定位本地运行环境".to_string())
    }
}

fn open_allowlisted_url(url: &str) -> Result<(), String> {
    if url != CHATGPT_MCP_SETTINGS_URL || !url.starts_with("https://chatgpt.com/") {
        return Err("不允许打开此地址".to_string());
    }
    let operation = wide("open");
    let target = wide(url);
    let result = unsafe { ShellExecuteW(null_mut(), operation.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
    if result as isize <= 32 {
        return Err("无法使用系统浏览器打开 ChatGPT".to_string());
    }
    Ok(())
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_allowlist_is_single_fixed_chatgpt_https_url() {
        assert!(CHATGPT_MCP_SETTINGS_URL.starts_with("https://chatgpt.com/"));
        assert_ne!(CHATGPT_MCP_SETTINGS_URL, "http://chatgpt.com/");
    }

    #[test]
    fn readiness_requires_real_ready_state_for_openai_tunnel() {
        let ready = |state| {
            let coding = matches!(state, RuntimeState::StartingTunnel | RuntimeState::WaitingTunnelReady | RuntimeState::Ready);
            let tunnel = matches!(state, RuntimeState::Ready);
            (coding, tunnel)
        };
        assert_eq!(ready(RuntimeState::WaitingTunnelReady), (true, false));
        assert_eq!(ready(RuntimeState::Ready), (true, true));
        assert_eq!(ready(RuntimeState::Stopped), (false, false));
    }
}
