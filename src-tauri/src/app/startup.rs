use std::fs;
use std::path::{Path, PathBuf};

use crate::control_plane::convergence::{
    ConnectionProfile, DesiredState, DesiredWorkspace, ServiceIntent,
};
use crate::control_plane::snapshot::SettingsProjection;
#[cfg(windows)]
use crate::credentials::{CredentialStore, WindowsCredentialStore};
use crate::domain::{ErrorCategory, OperationError};
use crate::settings::{SettingsStore, SettingsStoreError};
use crate::state::PermissionMode;
use crate::workspace::{WorkspaceRegistryError, WorkspaceValidator};

use super::{
    AutostartError, AutostartManager, DesktopLifecycle, DesktopRuntimeStartError,
    STARTUP_PROFILE_FILE_NAME, ShutdownReport, StartupMode, StartupProfile, StartupProfileError,
    StartupProfileStore,
};
use crate::mcp::ProductionRuntimeConfig;
use crate::privilege::{UacLaunchError, current_process_is_elevated};
use crate::state::PrivilegeFault;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupSuppression {
    Foreground,
    OnboardingIncomplete,
    AutoStartDisabled,
    ManualStopLatched,
    TunnelIdMissing,
    NoActiveWorkspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopStartupOutcome {
    ServicesStarted,
    ServicesAwaitingUiReady,
    ServicesSuppressed(StartupSuppression),
}

#[derive(Debug)]
pub enum DesktopStartupError {
    Settings(SettingsStoreError),
    Profile(StartupProfileError),
    Autostart(AutostartError),
    Workspace(WorkspaceRegistryError),
    Runtime(DesktopRuntimeStartError),
    AppDataIo(std::io::Error),
    InstallRootUnavailable,
    AuthorityProbe(UacLaunchError),
    Authority(PrivilegeFault),
}

impl std::fmt::Display for DesktopStartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Settings(error) => write!(f, "desktop startup settings failed: {error:?}"),
            Self::Profile(error) => write!(f, "desktop startup profile failed: {error}"),
            Self::Autostart(error) => write!(f, "desktop startup autostart failed: {error}"),
            Self::Workspace(error) => {
                write!(f, "desktop startup workspace validation failed: {error:?}")
            }
            Self::Runtime(error) => write!(f, "desktop startup runtime failed: {error}"),
            Self::AppDataIo(error) => write!(f, "desktop startup app-data setup failed: {error}"),
            Self::InstallRootUnavailable => {
                f.write_str("desktop startup install root is unavailable")
            }
            Self::AuthorityProbe(error) => {
                write!(f, "desktop startup authority probe failed: {error}")
            }
            Self::Authority(error) => {
                write!(
                    f,
                    "desktop startup administrator activation failed: {error:?}"
                )
            }
        }
    }
}

impl std::error::Error for DesktopStartupError {}

pub fn configure_desktop_startup(
    app_data_dir: &Path,
    startup_mode: StartupMode,
    lifecycle: &DesktopLifecycle,
) -> Result<DesktopStartupOutcome, DesktopStartupError> {
    fs::create_dir_all(app_data_dir).map_err(DesktopStartupError::AppDataIo)?;
    let data = SettingsStore::new(app_data_dir.join("settings.json"))
        .load()
        .map_err(DesktopStartupError::Settings)?;
    lifecycle.set_close_window_continue_running(data.settings.close_window_continue_running);
    #[cfg(windows)]
    let (runtime_key_saved, runtime_key_length, settings_error) =
        match WindowsCredentialStore::default().read_runtime_api_key() {
            Ok(secret) => (
                secret.is_some(),
                secret
                    .as_ref()
                    .map(|secret| secret.expose_secret().chars().count()),
                None,
            ),
            Err(_) => (
                false,
                None,
                Some(OperationError::new(
                    "Settings.CredentialMetadataUnavailable",
                    ErrorCategory::Unavailable,
                    "Runtime credential metadata is unavailable",
                    true,
                )),
            ),
        };
    #[cfg(not(windows))]
    let (runtime_key_saved, runtime_key_length, settings_error) = (false, None, None);
    lifecycle.publish_settings_snapshot(
        SettingsProjection::from_app_data(&data, runtime_key_saved, runtime_key_length),
        settings_error,
    );
    let profile_store = StartupProfileStore::new(app_data_dir.join(STARTUP_PROFILE_FILE_NAME));
    let profile = profile_store.load().map_err(DesktopStartupError::Profile)?;

    let desired_workspace = data
        .workspace
        .resolve_active(&WorkspaceValidator)
        .map_err(DesktopStartupError::Workspace)?
        .map(|workspace| {
            DesiredWorkspace::new(workspace.workspace_id, workspace.validated.execution_path())
        });
    let desired_connection = profile
        .validated_tunnel_id()
        .map_err(DesktopStartupError::Profile)?
        .map(|tunnel_id| ConnectionProfile::new(tunnel_id, 0));
    let process_elevated =
        current_process_is_elevated().map_err(DesktopStartupError::AuthorityProbe)?;
    let stored_permission: PermissionMode = data.settings.permission_mode.into();
    let permission_mode = startup_permission(stored_permission, process_elevated);
    lifecycle.replace_desired_state(DesiredState {
        permission: permission_mode,
        workspace: desired_workspace,
        services: ServiceIntent::Disabled,
        connection: desired_connection,
    });
    let install_root = production_install_root()?;
    if process_elevated {
        let broker = current_broker_executable()?;
        let activated = lifecycle
            .reconcile_permission_from_elevated_startup(&broker)
            .map_err(DesktopStartupError::Authority)?;
        if !activated {
            return Err(DesktopStartupError::Authority(PrivilegeFault::Unknown));
        }
    }
    lifecycle.publish_local_environment_observation(
        install_root.join("runtime/python/python.exe").is_file()
            && install_root
                .join("runtime/coding-tools-mcp/coding_tools_mcp/__init__.py")
                .is_file(),
    );

    AutostartManager::for_current_executable()
        .map_err(DesktopStartupError::Autostart)?
        .set_enabled(data.settings.auto_start_services)
        .map_err(DesktopStartupError::Autostart)?;

    let config = match build_background_resume_config(
        app_data_dir,
        startup_mode,
        &data,
        &profile,
        install_root,
    )? {
        Ok(config) => config,
        Err(suppression) => return Ok(DesktopStartupOutcome::ServicesSuppressed(suppression)),
    };
    if startup_mode == StartupMode::Foreground {
        if lifecycle.stage_foreground_start(config) {
            return Ok(DesktopStartupOutcome::ServicesAwaitingUiReady);
        }
        return Ok(DesktopStartupOutcome::ServicesStarted);
    }
    lifecycle
        .backend_handle()
        .spawn_start_production_runtime(config)
        .map_err(DesktopStartupError::AppDataIo)?;
    Ok(DesktopStartupOutcome::ServicesStarted)
}

pub fn manual_stop_services(
    app_data_dir: &Path,
    lifecycle: &DesktopLifecycle,
) -> Result<ShutdownReport, DesktopStartupError> {
    let store = StartupProfileStore::new(app_data_dir.join(STARTUP_PROFILE_FILE_NAME));
    let mut profile = store.load().map_err(DesktopStartupError::Profile)?;
    profile.record_manual_stop();
    store.save(&profile).map_err(DesktopStartupError::Profile)?;
    Ok(lifecycle.stop_services_for_manual_action())
}

fn build_background_resume_config(
    app_data_dir: &Path,
    startup_mode: StartupMode,
    data: &crate::settings::AppData,
    profile: &StartupProfile,
    install_root: PathBuf,
) -> Result<Result<ProductionRuntimeConfig, StartupSuppression>, DesktopStartupError> {
    if !data.settings.onboarding_complete {
        return Ok(Err(StartupSuppression::OnboardingIncomplete));
    }
    if startup_mode == StartupMode::Background && !data.settings.auto_start_services {
        return Ok(Err(StartupSuppression::AutoStartDisabled));
    }
    if startup_mode == StartupMode::Background && profile.manual_stop_latched() {
        return Ok(Err(StartupSuppression::ManualStopLatched));
    }
    let Some(tunnel_id) = profile
        .validated_tunnel_id()
        .map_err(DesktopStartupError::Profile)?
    else {
        return Ok(Err(StartupSuppression::TunnelIdMissing));
    };

    let resolved = data
        .workspace
        .resolve_active(&WorkspaceValidator)
        .map_err(DesktopStartupError::Workspace)?;
    let Some(workspace) = resolved else {
        return Ok(Err(StartupSuppression::NoActiveWorkspace));
    };
    Ok(Ok(ProductionRuntimeConfig::new(
        install_root,
        workspace.validated.execution_path(),
        app_data_dir.join("health"),
        tunnel_id,
    )))
}

fn production_install_root() -> Result<PathBuf, DesktopStartupError> {
    #[cfg(debug_assertions)]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .ok_or(DesktopStartupError::InstallRootUnavailable)
    }
    #[cfg(not(debug_assertions))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .ok_or(DesktopStartupError::InstallRootUnavailable)
    }
}

fn current_broker_executable() -> Result<PathBuf, DesktopStartupError> {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()
                .map(|parent| parent.join("localbridge-privileged-broker.exe"))
        })
        .ok_or(DesktopStartupError::InstallRootUnavailable)
}

fn startup_permission(stored: PermissionMode, process_elevated: bool) -> PermissionMode {
    if process_elevated {
        PermissionMode::Elevated
    } else {
        stored
    }
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/autostart/startup.rs"
    ));
}
