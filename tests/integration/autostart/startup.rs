use super::*;
use crate::privilege::PrivilegeController;
use crate::settings::{AppData, StoredPermissionMode};
use crate::state::{PrivilegeState, RuntimeState};
use crate::workspace::{WorkspaceId, WorkspaceValidator};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const VALID_TUNNEL: &str = "tunnel_01401401401401401401401401401401";

struct TempDir(PathBuf);
impl TempDir {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "localbridge-lb014-startup-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn resumable_data(workspace: &Path) -> AppData {
    let mut data = AppData::default();
    data.settings.permission_mode = StoredPermissionMode::Full;
    data.settings.auto_start_services = true;
    data.settings.onboarding_complete = true;
    let validated = WorkspaceValidator.validate(workspace).unwrap();
    let id = data
        .workspace
        .registry
        .upsert_validated(
            WorkspaceId::from_validated("lb014-active").unwrap(),
            workspace,
            &validated,
            1,
        )
        .unwrap();
    data.workspace.set_active_reference(id).unwrap();
    data
}

#[test]
fn background_resume_requires_auto_start_no_manual_stop_and_fresh_active_workspace() {
    let app_data = TempDir::new("app-data");
    let workspace = TempDir::new("workspace");
    let mut profile = StartupProfile::default();
    profile.set_tunnel_id(VALID_TUNNEL).unwrap();
    let data = resumable_data(&workspace.0);

    let config = build_background_resume_config(
        &app_data.0,
        StartupMode::Background,
        &data,
        &profile,
        PathBuf::from(r"C:\LocalBridge"),
    )
    .unwrap()
    .unwrap();
    let freshly_validated = WorkspaceValidator.validate(&workspace.0).unwrap();
    assert_eq!(config.workspace, freshly_validated.execution_path());

    profile.record_manual_stop();
    assert_eq!(
        build_background_resume_config(
            &app_data.0,
            StartupMode::Background,
            &data,
            &profile,
            PathBuf::from(r"C:\LocalBridge")
        )
        .unwrap()
        .unwrap_err(),
        StartupSuppression::ManualStopLatched
    );
}

#[test]
fn background_resume_is_suppressed_when_windows_login_autostart_is_disabled() {
    let app_data = TempDir::new("background-disabled-data");
    let workspace = TempDir::new("background-disabled-workspace");
    let mut profile = StartupProfile::default();
    profile.set_tunnel_id(VALID_TUNNEL).unwrap();
    let mut data = resumable_data(&workspace.0);
    data.settings.auto_start_services = false;

    assert_eq!(
        build_background_resume_config(
            &app_data.0,
            StartupMode::Background,
            &data,
            &profile,
            PathBuf::from(r"C:\LocalBridge"),
        )
        .unwrap()
        .unwrap_err(),
        StartupSuppression::AutoStartDisabled
    );
}

#[test]
fn manual_foreground_launch_ignores_login_autostart_and_manual_stop_latch() {
    let app_data = TempDir::new("foreground-data");
    let workspace = TempDir::new("foreground-workspace");
    let mut profile = StartupProfile::default();
    profile.set_tunnel_id(VALID_TUNNEL).unwrap();
    profile.record_manual_stop();
    let mut data = resumable_data(&workspace.0);
    data.settings.auto_start_services = false;

    let config = build_background_resume_config(
        &app_data.0,
        StartupMode::Foreground,
        &data,
        &profile,
        PathBuf::from(r"C:\LocalBridge"),
    )
    .unwrap()
    .unwrap();
    let freshly_validated = WorkspaceValidator.validate(&workspace.0).unwrap();
    assert_eq!(config.workspace, freshly_validated.execution_path());
    assert_eq!(config.permission_mode, PermissionMode::Full);
}

#[test]
fn no_active_workspace_never_falls_back_to_remembered_project() {
    let app_data = TempDir::new("no-active-data");
    let remembered = TempDir::new("remembered");
    let mut data = resumable_data(&remembered.0);
    data.workspace.clear_active();
    let mut profile = StartupProfile::default();
    profile.set_tunnel_id(VALID_TUNNEL).unwrap();
    assert_eq!(
        build_background_resume_config(
            &app_data.0,
            StartupMode::Background,
            &data,
            &profile,
            PathBuf::from(r"C:\LocalBridge")
        )
        .unwrap()
        .unwrap_err(),
        StartupSuppression::NoActiveWorkspace
    );
    assert_eq!(data.workspace.remembered_entries().len(), 1);
}

#[test]
fn elevated_preference_restores_requested_without_uac_for_background_and_foreground() {
    let background = DesktopLifecycle::new(PrivilegeController::new());
    restore_privilege_preference(PermissionMode::Elevated, &background).unwrap();
    assert_eq!(background.privilege().state(), PrivilegeState::Requested);

    let foreground = DesktopLifecycle::new(PrivilegeController::new());
    restore_privilege_preference(PermissionMode::Elevated, &foreground).unwrap();
    assert_eq!(foreground.privilege().state(), PrivilegeState::Requested);
}

#[test]
fn manual_stop_services_persists_latch_before_shutdown() {
    let app_data = TempDir::new("manual-stop");
    let store = StartupProfileStore::new(app_data.0.join(STARTUP_PROFILE_FILE_NAME));
    let mut profile = StartupProfile::default();
    profile.set_tunnel_id(VALID_TUNNEL).unwrap();
    store.save(&profile).unwrap();
    let lifecycle = DesktopLifecycle::new(PrivilegeController::new());

    let report = manual_stop_services(&app_data.0, &lifecycle).unwrap();

    assert_eq!(report, ShutdownReport::default());
    assert!(store.load().unwrap().manual_stop_latched());
    assert_eq!(lifecycle.runtime_snapshot().state, RuntimeState::Stopped);
}
