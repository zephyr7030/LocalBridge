use std::fs;
use std::path::{Path, PathBuf};

use crate::settings::{SettingsStore, SettingsStoreError};
use crate::state::{ActiveWorkspaceState, PermissionMode, PrivilegeFault};
use crate::workspace::{WorkspaceRegistryError, WorkspaceValidator};

use super::{
    AutostartError, AutostartManager, DesktopLifecycle, DesktopRuntimeStartError,
    STARTUP_PROFILE_FILE_NAME, ShutdownReport, StartupMode, StartupProfile, StartupProfileError,
    StartupProfileStore,
};
use crate::runtime::ProductionRuntimeConfig;

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
    Privilege(PrivilegeFault),
    AppDataIo(std::io::Error),
    InstallRootUnavailable,
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
            Self::Privilege(error) => {
                write!(f, "desktop startup privilege state failed: {error:?}")
            }
            Self::AppDataIo(error) => write!(f, "desktop startup app-data setup failed: {error}"),
            Self::InstallRootUnavailable => {
                f.write_str("desktop startup install root is unavailable")
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
    let profile_store = StartupProfileStore::new(app_data_dir.join(STARTUP_PROFILE_FILE_NAME));
    let profile = profile_store.load().map_err(DesktopStartupError::Profile)?;

    AutostartManager::for_current_executable()
        .map_err(DesktopStartupError::Autostart)?
        .set_enabled(data.settings.auto_start_services)
        .map_err(DesktopStartupError::Autostart)?;

    let permission_mode: PermissionMode = data.settings.permission_mode.into();
    restore_privilege_preference(permission_mode, lifecycle)?;

    let config = match build_background_resume_config(
        app_data_dir,
        startup_mode,
        &data,
        &profile,
        production_install_root()?,
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

fn restore_privilege_preference(
    permission_mode: PermissionMode,
    lifecycle: &DesktopLifecycle,
) -> Result<(), DesktopStartupError> {
    if permission_mode == PermissionMode::Elevated {
        lifecycle
            .privilege()
            .request_without_uac()
            .map_err(DesktopStartupError::Privilege)?;
    }
    Ok(())
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

    let control = data
        .workspace
        .to_control_state(&WorkspaceValidator)
        .map_err(DesktopStartupError::Workspace)?;
    let workspace = match control.active() {
        ActiveWorkspaceState::NoActiveWorkspace => {
            return Ok(Err(StartupSuppression::NoActiveWorkspace));
        }
        ActiveWorkspaceState::Active(workspace) => workspace.display_path().to_path_buf(),
    };
    let permission_mode: PermissionMode = data.settings.permission_mode.into();
    Ok(Ok(ProductionRuntimeConfig::new(
        install_root,
        workspace,
        app_data_dir.join("health"),
        tunnel_id,
        permission_mode,
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

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/autostart/startup.rs"
    ));
}
