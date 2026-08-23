use localbridge_lib::settings::{
    AppData, CURRENT_SETTINGS_SCHEMA_VERSION, SettingsStore, StoredSettings,
};
use localbridge_lib::state::{PermissionMode, Settings};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "localbridge-lb003-settings-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn settings_domain_round_trip_preserves_only_preferences() {
    for mode in [
        PermissionMode::Edit,
        PermissionMode::Full,
        PermissionMode::Elevated,
    ] {
        let domain = Settings {
            permission_mode: mode,
            auto_start_services: true,
            onboarding_complete: true,
        };
        let stored = StoredSettings::from_domain(&domain);
        assert_eq!(stored.to_domain(), domain);
    }
}

#[test]
fn atomic_write_replaces_target_and_preserves_previous_rollback_point() {
    let dir = temp_dir();
    let path = dir.join("settings.json");
    let store = SettingsStore::new(&path);
    let first = AppData::default();
    store.save(&first).unwrap();
    let first_bytes = fs::read(&path).unwrap();

    let mut second = first.clone();
    second.settings.auto_start_services = true;
    store.save(&second).unwrap();
    let loaded = store.load().unwrap();
    assert!(loaded.settings.auto_start_services);
    assert_eq!(fs::read(store.backup_path()).unwrap(), first_bytes);
    assert_eq!(loaded.schema_version, CURRENT_SETTINGS_SCHEMA_VERSION);
    assert!(!fs::read_dir(&dir).unwrap().any(|item| {
        item.unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp-")
    }));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn serialized_settings_cannot_gain_unknown_secret_fields() {
    let mut value = serde_json::to_value(AppData::default()).unwrap();
    value["api_key"] = serde_json::Value::String("forbidden".into());
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(serde_json::from_slice::<AppData>(&bytes).is_err());

    let clean = serde_json::to_string(&AppData::default())
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in [
        "api_key",
        "access_token",
        "client_secret",
        "password",
        "credential_secret",
    ] {
        assert!(!clean.contains(forbidden));
    }
}
