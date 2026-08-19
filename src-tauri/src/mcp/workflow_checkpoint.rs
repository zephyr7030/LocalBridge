use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const CHECKPOINT_VERSION: u32 = 2;
const LEGACY_CHECKPOINT_VERSION: u32 = 1;
const MAX_CHECKPOINT_PLAINTEXT_BYTES: usize = 262_144;
const MAX_CHECKPOINT_CIPHERTEXT_BYTES: usize = 524_288;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WorkflowCheckpoint {
    pub version: u32,
    pub workflow_id: String,
    pub arguments: Value,
    #[serde(default)]
    pub redacted_stdin_command_indices: Vec<usize>,
    #[serde(default)]
    pub coding_profile: Option<String>,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub current_step: Option<String>,
    #[serde(default)]
    pub next_step: Option<String>,
    #[serde(default)]
    pub files_read: Vec<Value>,
    #[serde(default)]
    pub modified_files: Vec<String>,
    #[serde(default)]
    pub commands: Vec<Value>,
    #[serde(default)]
    pub test_results: Vec<Value>,
    #[serde(default)]
    pub build_results: Vec<Value>,
    #[serde(default)]
    pub failure: Option<Value>,
    #[serde(default)]
    pub output_refs: Vec<String>,
    #[serde(default)]
    pub git_before: Option<Value>,
    #[serde(default)]
    pub git_after: Option<Value>,
    #[serde(default)]
    pub verification_plan: Vec<Value>,
    #[serde(default)]
    pub completed: bool,
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
        let (arguments, redacted_stdin_command_indices) = sanitize_checkpoint_arguments(arguments);
        Self {
            version: CHECKPOINT_VERSION,
            workflow_id,
            arguments,
            redacted_stdin_command_indices,
            coding_profile: None,
            objective: None,
            current_step: None,
            next_step: None,
            files_read: Vec::new(),
            modified_files: Vec::new(),
            commands: Vec::new(),
            test_results: Vec::new(),
            build_results: Vec::new(),
            failure: None,
            output_refs: Vec::new(),
            git_before: None,
            git_after: None,
            verification_plan: Vec::new(),
            completed: false,
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

    pub(crate) fn new_coding(
        workflow_id: String,
        arguments: Value,
        objective: String,
    ) -> Self {
        let mut checkpoint = Self::new(workflow_id, arguments);
        checkpoint.coding_profile = Some("coding-agent-v1".into());
        checkpoint.objective = Some(objective);
        checkpoint.current_step = Some("prepare".into());
        checkpoint.next_step = Some("edit".into());
        checkpoint
    }

    pub(crate) fn is_coding_task(&self) -> bool {
        self.coding_profile.as_deref() == Some("coding-agent-v1")
    }
}

fn sanitize_checkpoint_arguments(mut arguments: Value) -> (Value, Vec<usize>) {
    let mut redacted = Vec::new();
    if let Some(commands) = arguments.get_mut("commands").and_then(Value::as_array_mut) {
        for (index, command) in commands.iter_mut().enumerate() {
            if let Some(object) = command.as_object_mut() {
                if object.remove("stdin").is_some() { redacted.push(index); }
            }
        }
    }
    (arguments, redacted)
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
        let mut checkpoint: WorkflowCheckpoint =
            serde_json::from_slice(&plain).map_err(|_| "checkpoint decode failed")?;
        if checkpoint.workflow_id.is_empty()
            || !matches!(checkpoint.version, LEGACY_CHECKPOINT_VERSION | CHECKPOINT_VERSION)
        {
            return Err("checkpoint version or identity invalid".into());
        }
        if checkpoint.version == LEGACY_CHECKPOINT_VERSION {
            checkpoint.version = CHECKPOINT_VERSION;
            checkpoint.objective = checkpoint
                .arguments
                .get("objective")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        let (arguments, newly_redacted) = sanitize_checkpoint_arguments(checkpoint.arguments);
        checkpoint.arguments = arguments;
        if !newly_redacted.is_empty() {
            checkpoint.redacted_stdin_command_indices.extend(newly_redacted);
            checkpoint.redacted_stdin_command_indices.sort_unstable();
            checkpoint.redacted_stdin_command_indices.dedup();
            self.save(&checkpoint)?;
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
                "commands":[{"command":"echo SECRET_COMMAND_SENTINEL","shell":"cmd","stdin":"SECRET_STDIN_SENTINEL"}]
            }),
        );
        checkpoint.directory_index = 1;
        checkpoint.patch_applied = true;
        checkpoint.command_index = 1;
        assert_eq!(checkpoint.redacted_stdin_command_indices, vec![0]);
        assert!(checkpoint.arguments.pointer("/commands/0/stdin").is_none());
        assert!(!serde_json::to_string(&checkpoint).unwrap().contains("SECRET_STDIN_SENTINEL"));
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
        assert_eq!(loaded.version, CHECKPOINT_VERSION);
        reopened.clear().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn coding_checkpoint_persists_required_task_state_without_plaintext() {
        let path = temp_path("coding-v2");
        let store = WorkflowCheckpointStore::open_at(path.clone());
        let mut checkpoint = WorkflowCheckpoint::new_coding(
            "lb-workflow-coding".into(),
            json!({"action":"bugfix","objective":"repair durable task"}),
            "repair durable task".into(),
        );
        checkpoint.files_read.push(json!({"path":"src/a.rs","start_line":1,"end_line":4,"content_sha256":"a".repeat(64)}));
        checkpoint.modified_files.push("src/a.rs".into());
        checkpoint.commands.push(json!({"command":"cargo test","source":"verification_plan"}));
        checkpoint.test_results.push(json!({"command":"cargo test","status":"passed"}));
        checkpoint.git_before = Some(json!({"clean":true}));
        checkpoint.current_step = Some("verify".into());
        checkpoint.next_step = Some("persist".into());
        store.save(&checkpoint).unwrap();

        let raw = fs::read(store.path_for_test()).unwrap();
        assert!(!raw.windows(b"repair durable task".len()).any(|window| window == b"repair durable task"));
        let loaded = store.load().unwrap().unwrap();
        assert!(loaded.is_coding_task());
        assert_eq!(loaded.objective.as_deref(), Some("repair durable task"));
        assert_eq!(loaded.current_step.as_deref(), Some("verify"));
        assert_eq!(loaded.next_step.as_deref(), Some("persist"));
        assert_eq!(loaded.modified_files, vec!["src/a.rs"]);
        assert_eq!(loaded.commands.len(), 1);
        assert_eq!(loaded.test_results.len(), 1);
        store.clear().unwrap();
    }
}
