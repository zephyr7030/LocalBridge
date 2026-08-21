use crate::state::{
    CurrentTaskStatus, PermissionMode, PrivilegeState, RuntimeState, Settings,
    WorkspaceControlState,
};

#[cfg(windows)]
mod autostart;
mod background;
#[cfg(windows)]
mod single_instance;
#[cfg(windows)]
mod startup;
mod startup_profile;

#[cfg(windows)]
pub use autostart::{
    AutostartError, AutostartManager, CURRENT_USER_RUN_KEY, LOCALBRIDGE_RUN_VALUE,
};
pub use background::{
    BackgroundRecoveryAction, DesktopBackendHandle, DesktopExitError, DesktopLifecycle,
    DesktopRuntimeStartError, ExitRuntime, PrivilegeExit, ShutdownReport, StartupMode,
    attention_action, recovery_action, shutdown_in_security_order,
};
#[cfg(windows)]
pub use single_instance::{SingleInstanceAcquire, SingleInstanceError, SingleInstanceGuard};
#[cfg(windows)]
pub use startup::{
    DesktopStartupError, DesktopStartupOutcome, StartupSuppression, configure_desktop_startup,
    manual_stop_services,
};
pub use startup_profile::{
    STARTUP_PROFILE_FILE_NAME, STARTUP_PROFILE_SCHEMA_VERSION, StartupProfile, StartupProfileError,
    StartupProfileStore,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub settings: Settings,
    pub privilege_state: PrivilegeState,
    pub runtime_state: RuntimeState,
    pub workspace: WorkspaceControlState,
    pub current_task: CurrentTaskStatus,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            privilege_state: PrivilegeState::Disabled,
            runtime_state: RuntimeState::Stopped,
            workspace: WorkspaceControlState::default(),
            current_task: CurrentTaskStatus::Idle,
        }
    }
}

impl AppState {
    pub const fn permission_mode(&self) -> PermissionMode {
        self.settings.permission_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ActiveWorkspaceState;

    #[test]
    fn default_app_state_is_non_privileged_stopped_and_has_no_active_workspace() {
        let state = AppState::default();
        assert_eq!(state.permission_mode(), PermissionMode::Edit);
        assert_eq!(state.privilege_state, PrivilegeState::Disabled);
        assert_eq!(state.runtime_state, RuntimeState::Stopped);
        assert_eq!(
            state.workspace.active(),
            &ActiveWorkspaceState::NoActiveWorkspace
        );
        assert_eq!(state.current_task, CurrentTaskStatus::Idle);
    }
}
