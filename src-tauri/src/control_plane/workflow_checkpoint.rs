use std::ffi::c_void;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::UNIX_EPOCH;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CHECKPOINT_VERSION: u32 = 3;
const LEGACY_CHECKPOINT_VERSION: u32 = 1;
const MAX_CHECKPOINT_PLAINTEXT_BYTES: usize = 262_144;
const MAX_CHECKPOINT_CIPHERTEXT_BYTES: usize = 524_288;
const WORKFLOW_STALE_AFTER_MS: u64 = 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkflowCheckpointError {
    LocationUnavailable,
    EncodeFailed,
    SizeExceeded,
    ProtectionFailed,
    ParentUnavailable,
    CreateDirectoryFailed,
    WriteFailed,
    CommitFailed,
    ReadFailed,
    CiphertextInvalid,
    DecodeFailed,
    InvalidIdentity,
    DeleteFailed,
    #[cfg(not(windows))]
    PlatformUnavailable,
}

impl fmt::Display for WorkflowCheckpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::LocationUnavailable => "workflow_checkpoint_location_unavailable",
            Self::EncodeFailed => "workflow_checkpoint_encode_failed",
            Self::SizeExceeded => "workflow_checkpoint_size_exceeded",
            Self::ProtectionFailed => "workflow_checkpoint_protection_failed",
            Self::ParentUnavailable => "workflow_checkpoint_parent_unavailable",
            Self::CreateDirectoryFailed => "workflow_checkpoint_directory_create_failed",
            Self::WriteFailed => "workflow_checkpoint_write_failed",
            Self::CommitFailed => "workflow_checkpoint_commit_failed",
            Self::ReadFailed => "workflow_checkpoint_read_failed",
            Self::CiphertextInvalid => "workflow_checkpoint_ciphertext_invalid",
            Self::DecodeFailed => "workflow_checkpoint_decode_failed",
            Self::InvalidIdentity => "workflow_checkpoint_identity_invalid",
            Self::DeleteFailed => "workflow_checkpoint_delete_failed",
            #[cfg(not(windows))]
            Self::PlatformUnavailable => "workflow_checkpoint_platform_unavailable",
        };
        formatter.write_str(code)
    }
}

impl std::error::Error for WorkflowCheckpointError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkflowFailure {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<usize>,
}

impl WorkflowFailure {
    pub(crate) fn new(code: impl Into<String>, status: Option<impl Into<String>>) -> Self {
        Self {
            code: code.into(),
            status: status.map(Into::into),
            step: None,
        }
    }

    pub(crate) fn at_step(code: impl Into<String>, step: usize) -> Self {
        Self {
            code: code.into(),
            status: None,
            step: Some(step),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "Payload: Serialize",
    deserialize = "Payload: Deserialize<'de>"
))]
pub(crate) struct WorkflowCheckpoint<Payload> {
    pub version: u32,
    pub workflow_id: String,
    #[serde(default)]
    pub owner_session_id: Option<String>,
    #[serde(default)]
    pub adoption_token_hash: Option<crate::domain::AdoptionTokenHash>,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default = "default_workflow_stale_after_ms")]
    pub stale_after_ms: u64,
    /// Transport-specific workflow arguments are intentionally opaque at this persistence boundary.
    pub arguments: Payload,
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
    pub files_read: Vec<Payload>,
    #[serde(default)]
    pub modified_files: Vec<String>,
    #[serde(default)]
    pub commands: Vec<Payload>,
    #[serde(default)]
    pub test_results: Vec<Payload>,
    #[serde(default)]
    pub build_results: Vec<Payload>,
    #[serde(default)]
    pub failure: Option<WorkflowFailure>,
    #[serde(default)]
    pub output_refs: Vec<String>,
    #[serde(default)]
    pub git_before: Option<Payload>,
    #[serde(default)]
    pub git_after: Option<Payload>,
    #[serde(default)]
    pub verification_plan: Vec<Payload>,
    #[serde(default)]
    pub completed: bool,
    pub directory_index: usize,
    pub directory_results: Vec<Payload>,
    pub directory_inflight: bool,
    pub patch_applied: bool,
    pub patch_inflight: bool,
    pub command_index: usize,
    pub command_inflight: bool,
    pub current_session_id: Option<String>,
    pub command_results: Vec<Payload>,
}

impl<Payload> WorkflowCheckpoint<Payload> {
    pub(crate) fn new(workflow_id: String, arguments: Payload) -> Self {
        let now = now_unix_ms();
        Self {
            version: CHECKPOINT_VERSION,
            workflow_id,
            owner_session_id: None,
            adoption_token_hash: None,
            created_at_ms: now,
            updated_at_ms: now,
            stale_after_ms: WORKFLOW_STALE_AFTER_MS,
            arguments,
            redacted_stdin_command_indices: Vec::new(),
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

    pub(crate) fn new_coding(workflow_id: String, arguments: Payload, objective: String) -> Self {
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

    pub(crate) fn settle_command_kill(&mut self, public_session_id: &str) -> bool {
        if self.completed || self.current_session_id.as_deref() != Some(public_session_id) {
            return false;
        }
        self.current_session_id = None;
        self.command_inflight = false;
        if self.next_step.is_none() {
            self.next_step = Some(
                if self.is_coding_task() {
                    "verify"
                } else {
                    "resume"
                }
                .into(),
            );
        }
        true
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WorkflowCheckpointStore {
    path: PathBuf,
}

impl WorkflowCheckpointStore {
    pub(crate) fn for_workspace(workspace: &Path) -> Result<Self, WorkflowCheckpointError> {
        let root = std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or(WorkflowCheckpointError::LocationUnavailable)?;
        let mut hasher = Sha256::new();
        hasher.update(
            workspace
                .to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase(),
        );
        let identity = format!("{:x}", hasher.finalize());
        Ok(Self {
            path: root
                .join("LocalBridge")
                .join("task-state")
                .join(format!("workflow-{}.bin", &identity[..24])),
        })
    }

    pub(crate) fn save<Payload>(
        &self,
        checkpoint: &WorkflowCheckpoint<Payload>,
    ) -> Result<(), WorkflowCheckpointError>
    where
        Payload: Clone + Serialize,
    {
        let mut checkpoint = checkpoint.clone();
        let now = now_unix_ms();
        if checkpoint.created_at_ms == 0 {
            checkpoint.created_at_ms = now;
        }
        checkpoint.updated_at_ms = now;
        if checkpoint.stale_after_ms == 0 {
            checkpoint.stale_after_ms = WORKFLOW_STALE_AFTER_MS;
        }
        let plain =
            serde_json::to_vec(&checkpoint).map_err(|_| WorkflowCheckpointError::EncodeFailed)?;
        if plain.len() > MAX_CHECKPOINT_PLAINTEXT_BYTES {
            return Err(WorkflowCheckpointError::SizeExceeded);
        }
        let protected = protect_user_data(&plain)?;
        let Some(parent) = self.path.parent() else {
            return Err(WorkflowCheckpointError::ParentUnavailable);
        };
        fs::create_dir_all(parent).map_err(|_| WorkflowCheckpointError::CreateDirectoryFailed)?;
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, protected).map_err(|_| WorkflowCheckpointError::WriteFailed)?;
        atomic_replace(&tmp, &self.path)?;
        Ok(())
    }

    pub(crate) fn load<Payload>(
        &self,
    ) -> Result<Option<WorkflowCheckpoint<Payload>>, WorkflowCheckpointError>
    where
        Payload: Clone + Serialize + DeserializeOwned,
    {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(WorkflowCheckpointError::ReadFailed),
        };
        if bytes.is_empty() || bytes.len() > MAX_CHECKPOINT_CIPHERTEXT_BYTES {
            return Err(WorkflowCheckpointError::CiphertextInvalid);
        }
        let plain = unprotect_user_data(&bytes)?;
        if plain.len() > MAX_CHECKPOINT_PLAINTEXT_BYTES {
            return Err(WorkflowCheckpointError::SizeExceeded);
        }
        let file_updated_at_ms = fs::metadata(&self.path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64);
        let mut checkpoint: WorkflowCheckpoint<Payload> =
            serde_json::from_slice(&plain).map_err(|_| WorkflowCheckpointError::DecodeFailed)?;
        if checkpoint.workflow_id.is_empty()
            || !matches!(
                checkpoint.version,
                LEGACY_CHECKPOINT_VERSION | 2 | CHECKPOINT_VERSION
            )
        {
            return Err(WorkflowCheckpointError::InvalidIdentity);
        }
        let mut migrated = checkpoint.version != CHECKPOINT_VERSION;
        // Legacy payload interpretation belongs to the MCP persistence adapter.
        // This owner migrates only typed lifecycle metadata and its timestamps.
        checkpoint.version = CHECKPOINT_VERSION;
        let now = now_unix_ms();
        if checkpoint.created_at_ms == 0 {
            checkpoint.created_at_ms = file_updated_at_ms.unwrap_or(now);
            migrated = true;
        }
        if checkpoint.updated_at_ms == 0 {
            checkpoint.updated_at_ms = file_updated_at_ms.unwrap_or(checkpoint.created_at_ms);
            migrated = true;
        }
        if checkpoint.stale_after_ms == 0 {
            checkpoint.stale_after_ms = WORKFLOW_STALE_AFTER_MS;
            migrated = true;
        }
        let stale = !checkpoint.completed
            && now.saturating_sub(checkpoint.updated_at_ms) >= checkpoint.stale_after_ms;
        if stale {
            checkpoint.completed = true;
            checkpoint.current_step = Some("stale".into());
            checkpoint.next_step = None;
            checkpoint.directory_inflight = false;
            checkpoint.patch_inflight = false;
            checkpoint.command_inflight = false;
            checkpoint.current_session_id = None;
            checkpoint.failure = Some(WorkflowFailure::new("workflow_stale", Some("lost")));
        }
        if stale || migrated {
            self.save(&checkpoint)?;
        }
        Ok(Some(checkpoint))
    }

    pub(crate) fn clear(&self) -> Result<(), WorkflowCheckpointError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(WorkflowCheckpointError::DeleteFailed),
        }
    }

    pub(crate) fn settle_command_kill<Payload>(
        &self,
        public_session_id: &str,
    ) -> Result<bool, WorkflowCheckpointError>
    where
        Payload: Clone + Serialize + DeserializeOwned,
    {
        let Some(mut checkpoint) = self.load::<Payload>()? else {
            return Ok(false);
        };
        if !checkpoint.settle_command_kill(public_session_id) {
            return Ok(false);
        }
        self.save(&checkpoint)?;
        Ok(true)
    }

    pub(crate) fn active_owned_workflow<Payload>(
        &self,
        owner_session_id: &str,
    ) -> Result<Option<String>, WorkflowCheckpointError>
    where
        Payload: Clone + Serialize + DeserializeOwned,
    {
        Ok(self.load::<Payload>()?.and_then(|checkpoint| {
            (!checkpoint.completed
                && checkpoint.owner_session_id.as_deref() == Some(owner_session_id))
            .then_some(checkpoint.workflow_id)
        }))
    }

    pub(crate) fn active_workflow<Payload>(&self) -> Result<Option<String>, WorkflowCheckpointError>
    where
        Payload: Clone + Serialize + DeserializeOwned,
    {
        Ok(self
            .load::<Payload>()?
            .and_then(|checkpoint| (!checkpoint.completed).then_some(checkpoint.workflow_id)))
    }

    pub(crate) fn cancel_owned<Payload>(
        &self,
        workflow_id: &str,
        owner_session_id: &str,
    ) -> Result<bool, WorkflowCheckpointError>
    where
        Payload: Clone + Serialize + DeserializeOwned,
    {
        let Some(mut checkpoint) = self.load::<Payload>()? else {
            return Ok(false);
        };
        if checkpoint.completed
            || checkpoint.workflow_id != workflow_id
            || checkpoint.owner_session_id.as_deref() != Some(owner_session_id)
        {
            return Ok(false);
        }
        checkpoint.completed = true;
        checkpoint.current_step = Some("cancelled".into());
        checkpoint.next_step = None;
        checkpoint.directory_inflight = false;
        checkpoint.patch_inflight = false;
        checkpoint.command_inflight = false;
        checkpoint.current_session_id = None;
        checkpoint.failure = Some(WorkflowFailure::new("cancelled", Some("cancelled")));
        self.save(&checkpoint)?;
        Ok(true)
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

const fn default_workflow_stale_after_ms() -> u64 {
    WORKFLOW_STALE_AFTER_MS
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), WorkflowCheckpointError> {
    use std::os::windows::ffi::OsStrExt;
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
        return Err(WorkflowCheckpointError::CommitFailed);
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(_source: &Path, _destination: &Path) -> Result<(), WorkflowCheckpointError> {
    Err(WorkflowCheckpointError::PlatformUnavailable)
}

#[cfg(windows)]
fn protect_user_data(input: &[u8]) -> Result<Vec<u8>, WorkflowCheckpointError> {
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };

    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
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
        return Err(WorkflowCheckpointError::ProtectionFailed);
    }
    let protected =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe { local_free(output.pbData.cast()) };
    Ok(protected)
}

#[cfg(windows)]
fn unprotect_user_data(input: &[u8]) -> Result<Vec<u8>, WorkflowCheckpointError> {
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
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
        return Err(WorkflowCheckpointError::ProtectionFailed);
    }
    let plain =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
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
fn protect_user_data(_input: &[u8]) -> Result<Vec<u8>, WorkflowCheckpointError> {
    Err(WorkflowCheckpointError::PlatformUnavailable)
}

#[cfg(not(windows))]
fn unprotect_user_data(_input: &[u8]) -> Result<Vec<u8>, WorkflowCheckpointError> {
    Err(WorkflowCheckpointError::PlatformUnavailable)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use serde_json::json;
    use serde_json::value::Value as JsonPayload;

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
        let mut arguments = json!({
            "action":"bugfix",
            "patch":"SECRET_PATCH_SENTINEL",
            "commands":[{"command":"echo SECRET_COMMAND_SENTINEL","shell":"cmd","stdin":"SECRET_STDIN_SENTINEL"}]
        });
        arguments["commands"][0]
            .as_object_mut()
            .unwrap()
            .remove("stdin");
        let mut checkpoint = WorkflowCheckpoint::new("lb-workflow-test".into(), arguments);
        checkpoint.redacted_stdin_command_indices.push(0);
        checkpoint.directory_index = 1;
        checkpoint.patch_applied = true;
        checkpoint.command_index = 1;
        assert_eq!(checkpoint.redacted_stdin_command_indices, vec![0]);
        assert!(
            serde_json::to_value(&checkpoint.arguments)
                .unwrap()
                .pointer("/commands/0/stdin")
                .is_none()
        );
        assert!(
            !serde_json::to_string(&checkpoint)
                .unwrap()
                .contains("SECRET_STDIN_SENTINEL")
        );
        store.save(&checkpoint).unwrap();

        let raw = fs::read(store.path_for_test()).unwrap();
        assert!(
            !raw.windows(b"SECRET_PATCH_SENTINEL".len())
                .any(|window| window == b"SECRET_PATCH_SENTINEL")
        );
        assert!(
            !raw.windows(b"SECRET_COMMAND_SENTINEL".len())
                .any(|window| window == b"SECRET_COMMAND_SENTINEL")
        );

        let reopened = WorkflowCheckpointStore::open_at(path.clone());
        let loaded: WorkflowCheckpoint<JsonPayload> =
            reopened.load().unwrap().expect("durable checkpoint");
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
    fn timestamp_less_v2_checkpoint_uses_file_age_and_terminalizes_once() {
        let path = temp_path("v2-file-age");
        let store = WorkflowCheckpointStore::open_at(path.clone());
        let mut checkpoint = WorkflowCheckpoint::new_coding(
            "lb-workflow-v2-stale".into(),
            json!({"action":"bugfix"}),
            "stale migration".into(),
        );
        checkpoint.version = 2;
        checkpoint.created_at_ms = 0;
        checkpoint.updated_at_ms = 0;
        checkpoint.stale_after_ms = 0;
        checkpoint.current_step = Some("edit".into());
        checkpoint.next_step = Some("verify".into());
        let plain = serde_json::to_vec(&checkpoint).unwrap();
        fs::write(&path, protect_user_data(&plain).unwrap()).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(25 * 60 * 60);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();

        let migrated: WorkflowCheckpoint<JsonPayload> = store.load().unwrap().unwrap();
        assert_eq!(migrated.version, CHECKPOINT_VERSION);
        assert!(migrated.completed);
        assert_eq!(migrated.current_step.as_deref(), Some("stale"));
        assert!(migrated.next_step.is_none());
        assert_eq!(
            migrated
                .failure
                .as_ref()
                .map(|failure| failure.code.as_str()),
            Some("workflow_stale")
        );

        let reloaded: WorkflowCheckpoint<JsonPayload> = store.load().unwrap().unwrap();
        let stable: WorkflowCheckpoint<JsonPayload> = store.load().unwrap().unwrap();
        assert!(reloaded.updated_at_ms >= migrated.updated_at_ms);
        assert_eq!(stable.updated_at_ms, reloaded.updated_at_ms);
        assert!(reloaded.completed);
        store.clear().unwrap();
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
        checkpoint.files_read.push(
            json!({"path":"src/a.rs","start_line":1,"end_line":4,"content_sha256":"a".repeat(64)}),
        );
        checkpoint.modified_files.push("src/a.rs".into());
        checkpoint
            .commands
            .push(json!({"command":"cargo test","source":"verification_plan"}));
        checkpoint
            .test_results
            .push(json!({"command":"cargo test","status":"passed"}));
        checkpoint.git_before = Some(json!({"clean":true}));
        checkpoint.current_step = Some("verify".into());
        checkpoint.next_step = Some("persist".into());
        store.save(&checkpoint).unwrap();

        let raw = fs::read(store.path_for_test()).unwrap();
        assert!(
            !raw.windows(b"repair durable task".len())
                .any(|window| window == b"repair durable task")
        );
        let loaded: WorkflowCheckpoint<JsonPayload> = store.load().unwrap().unwrap();
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
