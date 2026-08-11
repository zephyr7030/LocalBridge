use std::ffi::c_void;
use std::mem::zeroed;
use std::ptr::null_mut;

use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};

use super::{
    CredentialMetadata, CredentialStore, CredentialStoreError, MAX_CREDENTIAL_BLOB_BYTES,
    RUNTIME_API_KEY_CREDENTIAL_ID, SecretString, wipe,
};

const ERROR_NOT_FOUND_CODE: u32 = 1168;
const TARGET_PREFIX: &str = "LocalBridge/RuntimeApiKey/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsCredentialStore {
    credential_id: String,
    target_name: String,
}

impl Default for WindowsCredentialStore {
    fn default() -> Self {
        Self::for_credential_id(RUNTIME_API_KEY_CREDENTIAL_ID)
            .expect("built-in Runtime API Key credential id is valid")
    }
}

impl WindowsCredentialStore {
    pub fn for_credential_id(
        credential_id: impl Into<String>,
    ) -> Result<Self, CredentialStoreError> {
        let credential_id = credential_id.into();
        if credential_id.is_empty()
            || !credential_id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            return Err(CredentialStoreError::InvalidCredentialId);
        }
        let target_name = format!("{TARGET_PREFIX}{credential_id}");
        Ok(Self {
            credential_id,
            target_name,
        })
    }

    fn metadata(&self, has_runtime_key: bool) -> CredentialMetadata {
        CredentialMetadata::runtime_api_key(&self.credential_id, has_runtime_key)
    }
}

impl CredentialStore for WindowsCredentialStore {
    fn save_runtime_api_key(
        &self,
        secret: &SecretString,
    ) -> Result<CredentialMetadata, CredentialStoreError> {
        if secret.as_bytes().len() > MAX_CREDENTIAL_BLOB_BYTES {
            return Err(CredentialStoreError::SecretTooLarge);
        }
        let mut target = wide_null(&self.target_name);
        let mut credential: CREDENTIALW = unsafe { zeroed() };
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = target.as_mut_ptr();
        credential.CredentialBlobSize = secret.as_bytes().len() as u32;
        credential.CredentialBlob = secret.as_bytes().as_ptr() as *mut u8;
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;

        let ok = unsafe { CredWriteW(&credential, 0) };
        if ok == 0 {
            return Err(last_error("CredWriteW"));
        }
        Ok(self.metadata(true))
    }

    fn read_runtime_api_key(&self) -> Result<Option<SecretString>, CredentialStoreError> {
        let target = wide_null(&self.target_name);
        let mut raw: *mut CREDENTIALW = null_mut();
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) };
        if ok == 0 {
            return classify_read_failure(last_error_code());
        }
        if raw.is_null() {
            return Err(CredentialStoreError::CorruptCredential);
        }
        let _buffer = CredentialBuffer(raw);
        let credential = unsafe { &*raw };
        let size = credential.CredentialBlobSize as usize;
        if size == 0 || size > MAX_CREDENTIAL_BLOB_BYTES || credential.CredentialBlob.is_null() {
            return Err(CredentialStoreError::CorruptCredential);
        }
        let bytes = unsafe { std::slice::from_raw_parts(credential.CredentialBlob, size) }.to_vec();
        SecretString::from_backend_bytes(bytes).map(Some)
    }

    fn delete_runtime_api_key(&self) -> Result<bool, CredentialStoreError> {
        let target = wide_null(&self.target_name);
        let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok != 0 {
            return Ok(true);
        }
        let code = last_error_code();
        if code == ERROR_NOT_FOUND_CODE {
            return Ok(false);
        }
        Err(CredentialStoreError::WindowsApi {
            operation: "CredDeleteW",
            code,
        })
    }

    fn runtime_api_key_metadata(&self) -> Result<CredentialMetadata, CredentialStoreError> {
        let present = self.read_runtime_api_key()?.is_some();
        Ok(self.metadata(present))
    }
}

struct CredentialBuffer(*mut CREDENTIALW);

impl Drop for CredentialBuffer {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                let credential = &mut *self.0;
                let size = credential.CredentialBlobSize as usize;
                if size > 0 && !credential.CredentialBlob.is_null() {
                    let bytes = std::slice::from_raw_parts_mut(credential.CredentialBlob, size);
                    wipe(bytes);
                }
            }
            CredFree(self.0 as *const c_void);
        }
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error(operation: &'static str) -> CredentialStoreError {
    CredentialStoreError::WindowsApi {
        operation,
        code: last_error_code(),
    }
}

fn last_error_code() -> u32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32
}

fn classify_read_failure(code: u32) -> Result<Option<SecretString>, CredentialStoreError> {
    if code == ERROR_NOT_FOUND_CODE {
        return Ok(None);
    }
    Err(CredentialStoreError::WindowsApi {
        operation: "CredReadW",
        code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERROR_ACCESS_DENIED_CODE: u32 = 5;
    const ERROR_INVALID_DATA_CODE: u32 = 13;

    #[test]
    fn inaccessible_or_wrong_user_credential_fails_closed() {
        for code in [ERROR_ACCESS_DENIED_CODE, ERROR_INVALID_DATA_CODE] {
            let error = classify_read_failure(code).unwrap_err();
            assert_eq!(
                error,
                CredentialStoreError::WindowsApi {
                    operation: "CredReadW",
                    code,
                }
            );
        }
    }

    #[test]
    fn only_not_found_is_treated_as_absent() {
        assert!(
            classify_read_failure(ERROR_NOT_FOUND_CODE)
                .unwrap()
                .is_none()
        );
    }
}
