use localbridge_lib::settings::{MigrationError, SettingsStore, CURRENT_SETTINGS_SCHEMA_VERSION};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_file(name: &str, contents: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "localbridge-lb003-migration-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    (dir, path)
}

#[test]
fn v1_migrates_sequentially_to_current_registry_and_active_reference() {
    let (dir, path) = temp_file("settings.json", include_str!("v1-single-workspace.json"));
    let data = SettingsStore::new(&path).load().unwrap();
    assert_eq!(data.schema_version, CURRENT_SETTINGS_SCHEMA_VERSION);
    assert_eq!(data.workspace.registry.entries().len(), 1);
    assert_eq!(data.workspace.active_entry().unwrap().workspace_id.as_str(), "legacy-one");
    assert!(SettingsStore::new(&path).backup_path().exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn v2_migrates_to_current_without_skipping_version_contract() {
    let (dir, path) = temp_file("settings.json", include_str!("v2-single-workspace.json"));
    let data = SettingsStore::new(&path).load().unwrap();
    assert_eq!(data.schema_version, CURRENT_SETTINGS_SCHEMA_VERSION);
    assert_eq!(data.workspace.active_entry().unwrap().validated_identity.as_str(), "validated:v2");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn v3_migrates_close_window_policy_to_safe_continue_running_default() {
    let (dir, path) = temp_file("settings.json", include_str!("v3-close-policy.json"));
    let data = SettingsStore::new(&path).load().unwrap();
    assert_eq!(data.schema_version, CURRENT_SETTINGS_SCHEMA_VERSION);
    assert!(data.settings.close_window_continue_running);
    assert!(SettingsStore::new(&path).backup_path().exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unvalidated_historical_workspace_is_preserved_as_pending_but_not_authorized() {
    let (dir, path) = temp_file("settings.json", include_str!("v2-unvalidated-workspace.json"));
    let data = SettingsStore::new(&path).load().unwrap();
    assert!(data.workspace.registry.entries().is_empty());
    assert!(data.workspace.active_workspace_id.is_none());
    assert!(data.workspace.pending_workspace_confirmation.is_some());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn future_schema_fails_safely_and_preserves_original_bytes() {
    let fixture = include_str!("future-v99.json");
    let (dir, path) = temp_file("settings.json", fixture);
    let before = fs::read(&path).unwrap();
    let error = SettingsStore::new(&path).load().unwrap_err();
    assert!(matches!(
        error,
        localbridge_lib::settings::SettingsStoreError::Migration(
            MigrationError::ConfigurationVersionUnsupported { found: 99, .. }
        )
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_migration_preserves_original_data_and_does_not_reset() {
    let fixture = include_str!("invalid-v2.json");
    let (dir, path) = temp_file("settings.json", fixture);
    let before = fs::read(&path).unwrap();
    assert!(SettingsStore::new(&path).load().is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!SettingsStore::new(&path).backup_path().exists());
    fs::remove_dir_all(dir).unwrap();
}
