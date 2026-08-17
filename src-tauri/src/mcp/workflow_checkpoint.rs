use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const CHECKPOINT_VERSION: u32 = 1;
const MAX_CHECKPOINT_PLAINTEXT_BYTES: usize = 262_144;
const MAX_CHECKPOINT_CIPHERTEXT_BYTES: usize = 524_288;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WorkflowCheckpoint {
    pub version: u32,
    pub workflow_id: String,
    pub arguments: Value,
    pub directory_index: usize,
    pub directory_results: Vec<Value>,
    pub directory_inflight: bool,
    pub patch_applied: bool,
    pub patch_inflight: bool,
    pub command_index: usize,
    pub command_inflight: bool,
    pub current_session_id: Option<String>,
    pub command_results: Vec<Value>,
}

impl WorkflowCheckpoint {
    pub(crate) fn new(workflow_id: String, arguments: Value) -> Self {
        Self {
            version: CHECKPOINT_VERSION,
            workflow_id,
            arguments,
            directory_index: 0,
            directory_results: Vec::new(),
            directory_inflight: false,
            patch_applied: false,
            patch_inflight: false,
            command_index: 0,
            command_inflight: false,
            current_session_id: None,
            command_results: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WorkflowCheckpointStore {
    path: PathBuf,
}

impl WorkflowCheckpointStore {
    pub(crate) fn for_workspace(workspace: &Path) -> Result<Self, String> {
        let root = std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| "LOCALAPPDATA unavailable".to_string())?;
        let mut hasher = Sha256::new();
        hasher.update(workspace.to_string_lossy().replace('/', "\\").to_ascii_lowercase());
        let identity = format!("{:x}", hasher.finalize());
        Ok(Self {
            path: root
                .join("LocalBridge")
                .join("task-state")
                .join(format!("workflow-{}.bin", &identity[..24])),
        })
    }

    pub(crate) fn save(&self, checkpoint: &WorkflowCheckpoint) -> Result<(), String> {
        let plain = serde_json::to_vec(checkpoint).map_err(|_| "checkpoint serialize failed")?;
        if plain.len() > MAX_CHECKPOINT_PLAINTEXT_BYTES {
            return Err("workflow checkpoint exceeds bounded size".into());
        }
        let protected = protect_user_data(&plain)?;
        let Some(parent) = self.path.parent() else {
            return Err("checkpoint parent unavailable".into());
        };
        fs::create_dir_all(parent).map_err(|_| "checkpoint directory create failed")?;
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, protected).map_err(|_| "checkpoint temporary write failed")?;
        atomic_replace(&tmp, &self.path)?;
        Ok(())
    }

    pub(crate) fn load(&self) -> Result<Option<WorkflowCheckpoint>, String> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("checkpoint read failed".into()),
        };
        if bytes.is_empty() || bytes.len() > MAX_CHECKPOINT_CIPHERTEXT_BYTES {
            return Err("checkpoint ciphertext invalid".into());
        }
        let plain = unprotect_user_data(&bytes)?;
        if plain.len() > MAX_CHECKPOINT_PLAINTEXT_BYTES {
            return Err("checkpoint plaintext exceeds bounded size".into());
        }
        let checkpoint: WorkflowCheckpoint =
            serde_json::from_slice(&plain).map_err(|_| "checkpoint decode failed")?;
        if checkpoint.version != CHECKPOINT_VERSION || checkpoint.workflow_id.is_empty() {
            return Err("checkpoint version or identity invalid".into());
        }
        Ok(Some(checkpoint))
    }

    pub(crate) fn clear(&self) -> Result<(), String> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("checkpoint delete failed".into()),
        }
    }

    #[cfg(test)]
    pub(crate) fn path_for_test(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    fn open_at(path: PathBuf) -> Self {
        Self { path }
    }
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(format!("checkpoint commit failed: {}", unsafe { GetLastError() }));
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(_source: &Path, _destination: &Path) -> Result<(), String> {
    Err("workflow checkpoint persistence requires Windows atomic replace".into())
}

#[cfg(windows)]
fn protect_user_data(input: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };

    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
    let ok = unsafe {
        CryptProtectData(
            &input_blob,
            null(),
            null(),
            null_mut(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(format!("CryptProtectData failed: {}", unsafe { GetLastError() }));
    }
    let protected = unsafe {
        std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
    };
    unsafe { local_free(output.pbData.cast()) };
    Ok(protected)
}

#[cfg(windows)]
fn unprotect_user_data(input: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
    let ok = unsafe {
        CryptUnprotectData(
            &input_blob,
            null_mut(),
            null(),
            null_mut(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(format!("CryptUnprotectData failed: {}", unsafe { GetLastError() }));
    }
    let plain = unsafe {
        std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
    };
    unsafe { local_free(output.pbData.cast()) };
    Ok(plain)
}

#[cfg(windows)]
unsafe fn local_free(memory: *mut c_void) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    if !memory.is_null() {
        let _ = unsafe { LocalFree(memory) };
    }
}

#[cfg(not(windows))]
fn protect_user_data(_input: &[u8]) -> Result<Vec<u8>, String> {
    Err("workflow checkpoint persistence requires Windows DPAPI".into())
}

#[cfg(not(windows))]
fn unprotect_user_data(_input: &[u8]) -> Result<Vec<u8>, String> {
    Err("workflow checkpoint persistence requires Windows DPAPI".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_path(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "localbridge-workflow-checkpoint-{label}-{}-{nonce}.bin",
            std::process::id()
        ))
    }

    #[test]
    fn checkpoint_is_durable_user_scoped_and_not_plaintext() {
        let path = temp_path("dpapi");
        let store = WorkflowCheckpointStore::open_at(path.clone());
        let mut checkpoint = WorkflowCheckpoint::new(
            "lb-workflow-test".into(),
            json!({
                "action":"bugfix",
                "patch":"SECRET_PATCH_SENTINEL",
                "commands":[{"command":"echo SECRET_COMMAND_SENTINEL","shell":"cmd"}]
            }),
        );
        checkpoint.directory_index = 1;
        checkpoint.patch_applied = true;
        checkpoint.command_index = 1;
        store.save(&checkpoint).unwrap();

        let raw = fs::read(store.path_for_test()).unwrap();
        assert!(!raw.windows(b"SECRET_PATCH_SENTINEL".len()).any(|window| window == b"SECRET_PATCH_SENTINEL"));
        assert!(!raw.windows(b"SECRET_COMMAND_SENTINEL".len()).any(|window| window == b"SECRET_COMMAND_SENTINEL"));

        let reopened = WorkflowCheckpointStore::open_at(path.clone());
        let loaded = reopened.load().unwrap().expect("durable checkpoint");
        assert_eq!(loaded.workflow_id, checkpoint.workflow_id);
        assert_eq!(loaded.arguments, checkpoint.arguments);
        assert_eq!(loaded.directory_index, 1);
        assert!(loaded.patch_applied);
        assert_eq!(loaded.command_index, 1);
        reopened.clear().unwrap();
        assert!(!path.exists());
    }
}
