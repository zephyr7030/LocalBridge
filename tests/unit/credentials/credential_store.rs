#![cfg(windows)]

use std::ffi::c_void;
use std::mem::zeroed;
use std::sync::atomic::{AtomicU64, Ordering};

use localbridge_lib::credentials::{
    CredentialStore, CredentialStoreError, SecretString, WINDOWS_CREDENTIAL_BACKEND,
    WINDOWS_CREDENTIAL_BACKEND_VERSION, WindowsCredentialStore,
};
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW, CredWriteW,
};

static SEQ: AtomicU64 = AtomicU64::new(1);
const TARGET_PREFIX: &str = "LocalBridge/RuntimeApiKey/";

fn unique_id(label: &str) -> String {
    format!(
        "lb005-{label}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

fn synthetic_secret(label: &str) -> String {
    format!("lb005-synthetic-{label}-{}", std::process::id())
}

struct Cleanup(WindowsCredentialStore);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = self.0.delete_runtime_api_key();
    }
}

#[test]
fn save_get_remove_round_trip_uses_secure_backend() {
    let store = WindowsCredentialStore::for_credential_id(unique_id("roundtrip")).unwrap();
    let _cleanup = Cleanup(store.clone());
    let expected = synthetic_secret("roundtrip");
    let secret = SecretString::new(expected.clone()).unwrap();

    let metadata = store.save_runtime_api_key(&secret).unwrap();
    assert!(metadata.has_runtime_key);
    assert_eq!(metadata.credential_backend, WINDOWS_CREDENTIAL_BACKEND);
    assert_eq!(
        metadata.credential_backend_version,
        WINDOWS_CREDENTIAL_BACKEND_VERSION
    );

    let loaded = store.read_runtime_api_key().unwrap().unwrap();
    assert_eq!(loaded.expose_secret(), expected);
    assert!(store.delete_runtime_api_key().unwrap());
    assert!(store.read_runtime_api_key().unwrap().is_none());
    assert!(!store.runtime_api_key_metadata().unwrap().has_runtime_key);
}

#[test]
fn secure_backend_survives_store_restart() {
    let id = unique_id("restart");
    let expected = synthetic_secret("restart");
    let first = WindowsCredentialStore::for_credential_id(id.clone()).unwrap();
    let _cleanup = Cleanup(first.clone());
    first
        .save_runtime_api_key(&SecretString::new(expected.clone()).unwrap())
        .unwrap();
    drop(first);

    let reopened = WindowsCredentialStore::for_credential_id(id).unwrap();
    assert_eq!(
        reopened
            .read_runtime_api_key()
            .unwrap()
            .unwrap()
            .expose_secret(),
        expected
    );
}

#[test]
fn missing_credential_is_absent_without_fallback() {
    let store = WindowsCredentialStore::for_credential_id(unique_id("missing")).unwrap();
    assert!(!store.delete_runtime_api_key().unwrap());
    assert!(store.read_runtime_api_key().unwrap().is_none());
    assert!(!store.runtime_api_key_metadata().unwrap().has_runtime_key);
}

#[test]
fn corrupt_credential_fails_closed() {
    let id = unique_id("corrupt");
    let store = WindowsCredentialStore::for_credential_id(id.clone()).unwrap();
    let _cleanup = Cleanup(store.clone());
    write_raw_empty_credential(&id);
    assert_eq!(
        store.read_runtime_api_key().unwrap_err(),
        CredentialStoreError::CorruptCredential
    );
}

#[test]
fn secret_debug_and_errors_are_fully_redacted() {
    let sentinel = synthetic_secret("redaction");
    let secret = SecretString::new(sentinel.clone()).unwrap();
    let debug = format!("{secret:?}");
    assert!(!debug.contains(&sentinel));
    assert!(debug.contains("redacted"));

    let error = CredentialStoreError::WindowsApi {
        operation: "synthetic",
        code: 5,
    };
    assert!(!format!("{error:?} {error}").contains(&sentinel));
}

#[test]
fn metadata_serialization_contains_reference_only() {
    let id = unique_id("metadata");
    let store = WindowsCredentialStore::for_credential_id(id.clone()).unwrap();
    let metadata = store.runtime_api_key_metadata().unwrap();
    let json = serde_json::to_string(&metadata).unwrap();
    assert!(json.contains(&id));
    assert!(json.contains(WINDOWS_CREDENTIAL_BACKEND));
    assert!(json.contains("has_runtime_key"));
    for forbidden in ["secret", "credential_blob", "authorization", "bearer"] {
        assert!(!json.to_ascii_lowercase().contains(forbidden));
    }
}

#[test]
fn invalid_credential_ids_and_secret_values_fail_closed() {
    assert_eq!(
        WindowsCredentialStore::for_credential_id("../plaintext").unwrap_err(),
        CredentialStoreError::InvalidCredentialId
    );
    assert_eq!(
        SecretString::new("   ").unwrap_err(),
        CredentialStoreError::CorruptCredential
    );
    assert_eq!(
        SecretString::new("contains\0nul").unwrap_err(),
        CredentialStoreError::CorruptCredential
    );
}

fn write_raw_empty_credential(id: &str) {
    let target_name = format!("{TARGET_PREFIX}{id}");
    let mut target: Vec<u16> = target_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut credential: CREDENTIALW = unsafe { zeroed() };
    credential.Type = CRED_TYPE_GENERIC;
    credential.TargetName = target.as_mut_ptr();
    credential.CredentialBlobSize = 0;
    credential.CredentialBlob = std::ptr::null_mut();
    credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
    assert_ne!(unsafe { CredWriteW(&credential, 0) }, 0);

    let mut read_back: *mut CREDENTIALW = std::ptr::null_mut();
    assert_ne!(
        unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut read_back) },
        0
    );
    assert!(!read_back.is_null());
    unsafe { CredFree(read_back as *const c_void) };
}
