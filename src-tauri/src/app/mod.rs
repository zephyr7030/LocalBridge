#[cfg(windows)]
mod autostart;
mod background;
#[cfg(windows)]
mod single_instance;
#[cfg(windows)]
mod startup;
mod startup_profile;
mod update;

#[cfg(windows)]
pub use autostart::{
    AutostartError, AutostartManager, CURRENT_USER_RUN_KEY, LOCALBRIDGE_RUN_VALUE,
};
pub use background::{
    BackgroundRecoveryAction, DesktopBackendHandle, DesktopExitError, DesktopLifecycle,
    DesktopRuntimeReconcileError, DesktopRuntimeStartError, ExitRuntime, PrivilegeExit,
    ShutdownReport, StartupMode, attention_action, recovery_action, shutdown_in_security_order,
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
pub use update::{GitHubReleaseSource, ReleaseSource, UpdateChecker, UpdateFetchError};
