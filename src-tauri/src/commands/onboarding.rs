use serde::Serialize;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
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

use super::error::{UiError, UiResult};
use crate::app::{DesktopLifecycle, STARTUP_PROFILE_FILE_NAME, StartupProfileStore};
use crate::commands::ui;
use crate::control_plane::convergence::ConnectionProfile;
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
pub async fn get_onboarding_state(app: AppHandle) -> UiResult<OnboardingState> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<OnboardingState> {
        let lifecycle = app.state::<DesktopLifecycle>();
        project_state(&lifecycle)
    })
    .await
    .map_err(|_| UiError::internal("Ui.OnboardingReadJoinFailed", "首次设置状态后台任务异常"))?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn save_onboarding_connection(
    tunnel_id: String,
    runtime_key: String,
    app: AppHandle,
) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
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

        let credentials_changed = if runtime_key.trim().is_empty() {
            let metadata = WindowsCredentialStore::default()
                .runtime_api_key_metadata()
                .map_err(|_| "无法读取Runtime API Key状态".to_string())?;
            if !metadata.has_runtime_key {
                return Err(UiError::from("请输入Runtime API Key"));
            }
            false
        } else {
            let secret = SecretString::new(runtime_key)
                .map_err(|_| "Runtime API Key格式无效".to_string())?;
            WindowsCredentialStore::default()
                .save_runtime_api_key(&secret)
                .map_err(|_| "无法安全保存Runtime API Key".to_string())?;
            true
        };
        let tunnel_id = profile
            .validated_tunnel_id()
            .map_err(|_| "Tunnel ID 格式无效".to_string())?
            .expect("saved profile has a tunnel id");
        let lifecycle = app.state::<DesktopLifecycle>();
        let current = lifecycle.desired_state().snapshot().state.connection;
        let mut epoch = current
            .as_ref()
            .map(|profile| profile.credential_epoch)
            .unwrap_or(0);
        if credentials_changed {
            epoch = epoch.saturating_add(1);
        }
        lifecycle.set_desired_connection(Some(ConnectionProfile::new(tunnel_id, epoch)));
        ui::refresh_settings_snapshot(&app, &lifecycle)?;
        Ok(())
    })
    .await
    .map_err(|_| {
        UiError::internal(
            "Ui.OnboardingConnectionJoinFailed",
            "OpenAI 连接保存后台任务异常",
        )
    })?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn open_openai_tunnel_settings() -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(|| open_allowlisted_url(OPENAI_TUNNEL_SETTINGS_URL))
        .await
        .map_err(|_| {
            UiError::internal(
                "Ui.OpenTunnelSettingsJoinFailed",
                "打开 Tunnel ID 设置后台任务异常",
            )
        })?
        .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn open_openai_api_keys() -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(|| open_allowlisted_url(OPENAI_API_KEYS_URL))
        .await
        .map_err(|_| {
            UiError::internal(
                "Ui.OpenApiKeysJoinFailed",
                "打开 Runtime API Key 设置后台任务异常",
            )
        })?
        .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn open_chatgpt_plugins_settings() -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(|| open_allowlisted_url(CHATGPT_PLUGINS_SETTINGS_URL))
        .await
        .map_err(|_| {
            UiError::internal(
                "Ui.OpenPluginsJoinFailed",
                "打开 ChatGPT插件设置后台任务异常",
            )
        })?
        .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn open_chatgpt_custom_connector_settings() -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(|| open_allowlisted_url(CHATGPT_CUSTOM_CONNECTOR_URL))
        .await
        .map_err(|_| UiError::internal("Ui.OpenConnectorJoinFailed", "打开插件管理页后台任务异常"))?
        .map_err(UiError::from_string)
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
pub async fn choose_onboarding_workspace_folder() -> UiResult<Option<String>> {
    tauri::async_runtime::spawn_blocking(pick_windows_workspace_folder)
        .await
        .map_err(|_| UiError::internal("Ui.FolderPickerJoinFailed", "无法打开文件夹选择器"))?
        .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn prepare_onboarding_project(
    mode: String,
    project_id: Option<String>,
    selected_folder: Option<String>,
    app: AppHandle,
) -> UiResult<OnboardingState> {
    ui::set_permission_mode(mode, app.clone()).await?;
    tauri::async_runtime::spawn_blocking(move || -> UiResult<OnboardingState> {
        if let Some(folder) = selected_folder.filter(|value| !value.trim().is_empty()) {
            let lifecycle = app.state::<DesktopLifecycle>();
            ui::add_project_blocking(folder, Some(false), &app, &lifecycle)?;
        } else if let Some(id) = project_id.filter(|value| !value.trim().is_empty()) {
            let lifecycle = app.state::<DesktopLifecycle>();
            ui::select_project_blocking(id, &app, &lifecycle)?;
        } else {
            return Err(UiError::from("请选择项目文件夹"));
        }

        for _ in 0..120 {
            let lifecycle = app.state::<DesktopLifecycle>();
            let current = project_state(&lifecycle)?;
            if current.readiness.all_ready() {
                return Ok(current);
            }
            if matches!(lifecycle.runtime_snapshot().state, RuntimeState::Faulted(_)) {
                return Err(UiError::from("本地服务启动失败，请检查启动检查状态"));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Err(UiError::from("本地服务未在预期时间内就绪，请重试"))
    })
    .await
    .map_err(|_| UiError::internal("Ui.OnboardingProjectJoinFailed", "项目准备后台任务异常"))?
    .map_err(UiError::from_string)
}

#[tauri::command]
pub async fn complete_onboarding(app: AppHandle) -> UiResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> UiResult<()> {
        let lifecycle = app.state::<DesktopLifecycle>();
        let current = project_state(&lifecycle)?;
        if !current.readiness.all_ready() {
            return Err(UiError::from("本地服务尚未全部就绪"));
        }
        let store = SettingsStore::new(app_data_dir(&app)?.join("settings.json"));
        let mut data = store.load().map_err(|_| "无法读取设置".to_string())?;
        data.settings.onboarding_complete = true;
        store
            .save(&data)
            .map_err(|_| "无法保存设置完成状态".to_string())?;
        ui::refresh_settings_snapshot(&app, &lifecycle)?;
        Ok(())
    })
    .await
    .map_err(|_| {
        UiError::internal(
            "Ui.OnboardingCompleteJoinFailed",
            "首次设置完成后台任务异常",
        )
    })?
    .map_err(UiError::from_string)
}

fn project_state(lifecycle: &DesktopLifecycle) -> UiResult<OnboardingState> {
    let snapshot = lifecycle.control_plane_snapshot();
    let settings = snapshot.settings.value.as_ref();
    let connection = snapshot.connection.value.as_ref();
    let runtime = snapshot.runtime.value.as_ref();
    Ok(OnboardingState {
        complete: settings.is_some_and(|settings| settings.onboarding_complete),
        connection_configured: connection
            .is_some_and(|connection| connection.desired_tunnel_id.is_some()),
        runtime_key_saved: settings.is_some_and(|settings| settings.runtime_key_saved),
        runtime_key_length: settings.and_then(|settings| settings.runtime_key_length),
        tunnel_id: connection.and_then(|connection| connection.desired_tunnel_id.clone()),
        readiness: readiness(runtime),
    })
}

fn readiness(
    runtime: Option<&crate::control_plane::snapshot::RuntimeProjection>,
) -> OnboardingReadiness {
    let local_environment = runtime
        .and_then(|runtime| runtime.local_environment_available)
        .unwrap_or(false);
    let state = runtime
        .map(|runtime| &runtime.state)
        .unwrap_or(&RuntimeState::Stopped);
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

fn app_data_dir(app: &AppHandle) -> UiResult<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|_| "无法定位应用数据目录".to_string())?)
}

fn open_allowlisted_url(url: &str) -> UiResult<()> {
    if !matches!(
        url,
        OPENAI_TUNNEL_SETTINGS_URL
            | OPENAI_API_KEYS_URL
            | CHATGPT_PLUGINS_SETTINGS_URL
            | CHATGPT_CUSTOM_CONNECTOR_URL
    ) {
        return Err(UiError::from("不允许打开此地址"));
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
        return Err(UiError::from("无法使用系统浏览器打开页面"));
    }
    Ok(())
}

fn pick_windows_workspace_folder() -> UiResult<Option<String>> {
    let initialized = unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
    if initialized < 0 {
        return Err(UiError::from("无法初始化文件夹选择器"));
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
    Ok(result?)
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
        assert_eq!(
            ready(RuntimeState::Faulted(RuntimeFault::McpHealthTimeout)),
            (false, false)
        );
        assert_eq!(ready(RuntimeState::Stopped), (false, false));
    }
}
