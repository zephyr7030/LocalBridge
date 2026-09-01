#[cfg(windows)]
mod windows;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ptr;

#[cfg(windows)]
pub use windows::WindowsCredentialStore;

pub const RUNTIME_API_KEY_CREDENTIAL_ID: &str = "runtime-api-key";
pub const WINDOWS_CREDENTIAL_BACKEND: &str = "windows-credential-manager";
pub const WINDOWS_CREDENTIAL_BACKEND_VERSION: u32 = 1;
pub(crate) const MAX_CREDENTIAL_BLOB_BYTES: usize = 2560;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialMetadata {
    pub credential_id: String,
    pub credential_backend: String,
    pub credential_backend_version: u32,
    pub has_runtime_key: bool,
}

impl CredentialMetadata {
    pub(crate) fn runtime_api_key(credential_id: &str, has_runtime_key: bool) -> Self {
        Self {
            credential_id: credential_id.to_owned(),
            credential_backend: WINDOWS_CREDENTIAL_BACKEND.to_owned(),
            credential_backend_version: WINDOWS_CREDENTIAL_BACKEND_VERSION,
            has_runtime_key,
        }
    }
}

pub struct SecretString {
    bytes: Vec<u8>,
}

impl SecretString {
    pub fn new(value: impl Into<String>) -> Result<Self, CredentialStoreError> {
        Self::from_backend_bytes(value.into().into_bytes())
    }

    pub fn expose_secret(&self) -> &str {
        // Construction and backend reads both validate UTF-8 before creating SecretString.
        unsafe { std::str::from_utf8_unchecked(&self.bytes) }
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn from_backend_bytes(mut bytes: Vec<u8>) -> Result<Self, CredentialStoreError> {
        let valid_utf8 = std::str::from_utf8(&bytes).is_ok();
        let non_empty = bytes.iter().any(|byte| !byte.is_ascii_whitespace());
        let has_nul = bytes.contains(&0);
        if !valid_utf8 || !non_empty || has_nul {
            wipe(&mut bytes);
            return Err(CredentialStoreError::CorruptCredential);
        }
        if bytes.len() > MAX_CREDENTIAL_BLOB_BYTES {
            wipe(&mut bytes);
            return Err(CredentialStoreError::SecretTooLarge);
        }
        Ok(Self { bytes })
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(**redacted**)")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        wipe(&mut self.bytes);
    }
}

fn wipe(bytes: &mut [u8]) {
    for byte in bytes {
        unsafe { ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

pub trait CredentialStore {
    fn save_runtime_api_key(
        &self,
        secret: &SecretString,
    ) -> Result<CredentialMetadata, CredentialStoreError>;
    fn read_runtime_api_key(&self) -> Result<Option<SecretString>, CredentialStoreError>;
    fn delete_runtime_api_key(&self) -> Result<bool, CredentialStoreError>;
    fn runtime_api_key_metadata(&self) -> Result<CredentialMetadata, CredentialStoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialStoreError {
    InvalidCredentialId,
    CorruptCredential,
    SecretTooLarge,
    WindowsApi { operation: &'static str, code: u32 },
}

impl fmt::Display for CredentialStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentialId => f.write_str("invalid credential id"),
            Self::CorruptCredential => f.write_str("credential data is invalid"),
            Self::SecretTooLarge => f.write_str("credential data exceeds secure backend limit"),
            Self::WindowsApi { operation, code } => {
                write!(
                    f,
                    "Windows credential API {operation} failed with code {code}"
                )
            }
        }
    }
}

impl std::error::Error for CredentialStoreError {}
