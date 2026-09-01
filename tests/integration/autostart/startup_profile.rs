use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const VALID_TUNNEL: &str = "tunnel_01401401401401401401401401401401";

fn temp_store(label: &str) -> (PathBuf, StartupProfileStore) {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("localbridge-lb014-profile-{label}-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let store = StartupProfileStore::new(root.join(STARTUP_PROFILE_FILE_NAME));
    (root, store)
}

#[test]
fn manual_stop_and_tunnel_id_persist_without_secret_fields() {
    let (root, store) = temp_store("persist");
    let mut profile = StartupProfile::default();
    profile.set_tunnel_id(VALID_TUNNEL).unwrap();
    profile.record_manual_stop();
    store.save(&profile).unwrap();

    let loaded = store.load().unwrap();
    assert!(loaded.manual_stop_latched());
    assert!(loaded.validated_tunnel_id().unwrap().is_some());
    let text = fs::read_to_string(store.path()).unwrap();
    assert!(text.contains("manual_stop_latched"));
    assert!(text.contains("tunnel_id"));
    for forbidden in ["api_key", "bearer", "authorization", "broker_nonce", "password"] {
        assert!(!text.to_ascii_lowercase().contains(forbidden));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_tunnel_and_future_schema_fail_closed_without_overwrite() {
    let (root, store) = temp_store("fail-closed");
    let mut profile = StartupProfile::default();
    assert!(profile.set_tunnel_id("invalid-tunnel").is_err());
    store.save(&profile).unwrap();
    let original = fs::read(store.path()).unwrap();

    let future = format!("{{\"schema_version\":{},\"tunnel_id\":null,\"manual_stop_latched\":false}}\n", STARTUP_PROFILE_SCHEMA_VERSION + 1);
    fs::write(store.path(), future.as_bytes()).unwrap();
    assert!(matches!(
        store.load(),
        Err(StartupProfileError::UnsupportedSchema { .. })
    ));
    assert_eq!(fs::read(store.path()).unwrap(), future.as_bytes());
    fs::write(store.path(), original).unwrap();
    fs::remove_dir_all(root).unwrap();
}
