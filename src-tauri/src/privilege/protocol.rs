use std::fmt;

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const BROKER_PROTOCOL_VERSION: u16 = 2;
pub const MAX_BROKER_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const SESSION_NONCE_BYTES: usize = 32;
pub const MAX_ELEVATED_ARGS: usize = 128;
pub const MAX_ELEVATED_STRING_BYTES: usize = 32 * 1024;
pub const MAX_ELEVATED_TIMEOUT_MS: u32 = 120_000;
pub const MAX_ELEVATED_OUTPUT_BYTES: u32 = 1024 * 1024;
pub const MAX_ELEVATED_REQUEST_ID_BYTES: usize = 128;
pub const MAX_PRIVILEGED_FILE_BYTES: usize = 24 * 1024;
pub const MAX_ADMINISTRATOR_FILESYSTEM_CONTENT_BYTES: usize = 1024 * 1024;
const BROKER_PIPE_PREFIX: &str = r"\\.\pipe\LocalBridge-Privileged-";

fn is_windows_verbatim_path(value: &str) -> bool {
    value.starts_with(r"\\?\")
}

pub(crate) fn is_valid_broker_pipe_name(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix(BROKER_PIPE_PREFIX) else {
        return false;
    };
    suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionNonce([u8; SESSION_NONCE_BYTES]);

impl SessionNonce {
    pub const fn from_bytes(bytes: [u8; SESSION_NONCE_BYTES]) -> Self {
        Self(bytes)
    }
    pub const fn as_bytes(&self) -> &[u8; SESSION_NONCE_BYTES] {
        &self.0
    }
}

impl fmt::Debug for SessionNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionNonce([REDACTED])")
    }
}

impl Drop for SessionNonce {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerHello {
    pub version: u16,
    pub generation: u64,
    pub session_nonce: SessionNonce,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerReady {
    pub version: u16,
    pub generation: u64,
    pub session_nonce: SessionNonce,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrokerRequest {
    Ping,
    Shutdown,
    StartExec {
        request_id: String,
        spec: ElevatedExecSpec,
    },
    PollExec {
        request_id: String,
    },
    CancelExec {
        request_id: String,
    },
    Filesystem {
        spec: PrivilegedFilesystemSpec,
    },
    StructuredFilesystem {
        spec: AdministratorFilesystemSpec,
    },
    StartStructuredFilesystem {
        request_id: String,
        spec: AdministratorFilesystemSpec,
    },
    PollStructuredFilesystem {
        request_id: String,
    },
    CancelStructuredFilesystem {
        request_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerRequestEnvelope {
    pub version: u16,
    pub generation: u64,
    pub session_nonce: SessionNonce,
    pub sequence: u64,
    pub request: BrokerRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerRejectCode {
    ProtocolMismatch,
    StaleGeneration,
    SessionMismatch,
    Replay,
    Malformed,
    Oversized,
    DuplicateRequest,
    RequestNotFound,
    ExecutionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrokerResponse {
    Pong,
    ShutdownAck,
    ExecAccepted,
    ExecPending,
    ExecCompleted {
        execution: ElevatedExecResult,
    },
    FilesystemCompleted {
        filesystem: PrivilegedFilesystemResult,
    },
    StructuredFilesystemCompleted {
        filesystem: AdministratorFilesystemResult,
    },
    StructuredFilesystemFailed {
        code: AdministratorFilesystemErrorCode,
    },
    StructuredFilesystemAccepted,
    StructuredFilesystemPending,
    CancelAck,
    Rejected {
        code: BrokerRejectCode,
    },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElevatedExecSpec {
    pub program: String,
    pub args: Vec<String>,
    pub workdir: Option<String>,
    pub timeout_ms: u32,
    pub max_output_bytes: u32,
}

impl fmt::Debug for ElevatedExecSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElevatedExecSpec")
            .field("program", &self.program)
            .field("arg_count", &self.args.len())
            .field("args", &"[REDACTED]")
            .field("workdir", &self.workdir)
            .field("timeout_ms", &self.timeout_ms)
            .field("max_output_bytes", &self.max_output_bytes)
            .finish()
    }
}

impl ElevatedExecSpec {
    pub fn validate(&self) -> Result<(), BrokerProtocolError> {
        if self.program.is_empty()
            || self.program.len() > MAX_ELEVATED_STRING_BYTES
            || !Path::new(&self.program).is_absolute()
            || is_windows_verbatim_path(&self.program)
            || self.args.len() > MAX_ELEVATED_ARGS
            || self
                .args
                .iter()
                .any(|arg| arg.len() > MAX_ELEVATED_STRING_BYTES || arg.as_bytes().contains(&0))
            || self.program.as_bytes().contains(&0)
            || self.workdir.as_ref().is_some_and(|value| {
                value.is_empty()
                    || value.len() > MAX_ELEVATED_STRING_BYTES
                    || value.as_bytes().contains(&0)
                    || !Path::new(value).is_absolute()
                    || is_windows_verbatim_path(value)
            })
            || self.timeout_ms == 0
            || self.timeout_ms > MAX_ELEVATED_TIMEOUT_MS
            || self.max_output_bytes == 0
            || self.max_output_bytes > MAX_ELEVATED_OUTPUT_BYTES
        {
            return Err(BrokerProtocolError::MalformedFrame);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElevatedExecOutcome {
    Completed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElevatedExecResult {
    pub outcome: ElevatedExecOutcome,
    pub exit_code: Option<u32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegedFilesystemAction {
    ReadFile,
    WriteFile,
    CreateDirectory,
    Rename,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivilegedFilesystemSpec {
    pub action: PrivilegedFilesystemAction,
    pub path: String,
    pub destination: Option<String>,
    pub content_base64: Option<String>,
    #[serde(default)]
    pub recursive: bool,
}

impl PrivilegedFilesystemSpec {
    pub fn validate(&self) -> Result<(), BrokerProtocolError> {
        if !valid_privileged_absolute_path(&self.path)
            || self
                .destination
                .as_deref()
                .is_some_and(|value| !valid_privileged_absolute_path(value))
            || self.content_base64.as_ref().is_some_and(|value| {
                value.len() > MAX_PRIVILEGED_FILE_BYTES.div_ceil(3) * 4
                    || value.as_bytes().contains(&0)
            })
        {
            return Err(BrokerProtocolError::MalformedFrame);
        }
        let valid_shape = match self.action {
            PrivilegedFilesystemAction::ReadFile | PrivilegedFilesystemAction::CreateDirectory => {
                self.destination.is_none() && self.content_base64.is_none() && !self.recursive
            }
            PrivilegedFilesystemAction::WriteFile => {
                self.destination.is_none() && self.content_base64.is_some() && !self.recursive
            }
            PrivilegedFilesystemAction::Rename => {
                self.destination.is_some() && self.content_base64.is_none() && !self.recursive
            }
            PrivilegedFilesystemAction::Delete => {
                self.destination.is_none() && self.content_base64.is_none()
            }
        };
        valid_shape
            .then_some(())
            .ok_or(BrokerProtocolError::MalformedFrame)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedFilesystemResult {
    pub action: PrivilegedFilesystemAction,
    pub path: String,
    pub destination: Option<String>,
    pub content_base64: Option<String>,
    pub bytes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdministratorFilesystemAction {
    List,
    Stat,
    Read,
    Write,
    Search,
    Copy,
    Move,
    Delete,
    Hash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdministratorFilesystemKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdministratorFilesystemSortBy {
    Path,
    Size,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdministratorFilesystemSortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdministratorFilesystemErrorCode {
    InvalidArgument,
    NotFound,
    OutsideAuthority,
    AlreadyExists,
    LimitExceeded,
    Cancelled,
    Unsupported,
    Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdministratorWorkspacePathField {
    Path,
    Source,
    Destination,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdministratorFilesystemSpec {
    pub action: AdministratorFilesystemAction,
    pub path: Option<String>,
    pub source: Option<String>,
    pub destination: Option<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub workspace_identity: Option<String>,
    #[serde(default)]
    pub workspace_fields: Vec<AdministratorWorkspacePathField>,
    pub recursive: bool,
    pub max_depth: u32,
    pub max_entries: u32,
    pub max_results: u32,
    pub offset: u64,
    pub max_bytes: u32,
    pub content_base64: Option<String>,
    pub pattern: Option<String>,
    pub kind: Option<AdministratorFilesystemKind>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub modified_after_ms: Option<u64>,
    pub modified_before_ms: Option<u64>,
    pub sort_by: AdministratorFilesystemSortBy,
    pub sort_order: AdministratorFilesystemSortOrder,
    pub overwrite: bool,
    pub calculate_size: bool,
}

impl AdministratorFilesystemSpec {
    pub fn validate(&self) -> Result<(), BrokerProtocolError> {
        let paths_valid = self
            .path
            .as_deref()
            .is_none_or(valid_privileged_absolute_path)
            && self
                .source
                .as_deref()
                .is_none_or(valid_privileged_absolute_path)
            && self
                .destination
                .as_deref()
                .is_none_or(valid_privileged_absolute_path);
        let bounds_valid = self.max_depth > 0
            && self.max_depth <= 64
            && self.max_entries > 0
            && self.max_entries <= 100_000
            && self.max_results > 0
            && self.max_results <= 10_000
            && self.max_bytes > 0
            && self.max_bytes <= 1024 * 1024
            && self
                .min_size
                .zip(self.max_size)
                .is_none_or(|(min, max)| min <= max)
            && self
                .modified_after_ms
                .zip(self.modified_before_ms)
                .is_none_or(|(min, max)| min <= max);
        let content_valid = self.content_base64.as_ref().is_none_or(|value| {
            value.len() <= MAX_ADMINISTRATOR_FILESYSTEM_CONTENT_BYTES.div_ceil(3) * 4 + 4
                && !value.as_bytes().contains(&0)
        });
        let pattern_valid = self.pattern.as_ref().is_none_or(|value| {
            !value.is_empty()
                && value.len() <= MAX_ELEVATED_STRING_BYTES
                && !value.as_bytes().contains(&0)
                && !value.contains(['\n', '\r'])
        });
        let workspace_binding_valid = match (
            self.workspace_root.as_deref(),
            self.workspace_identity.as_deref(),
        ) {
            (None, None) => self.workspace_fields.is_empty(),
            (Some(root), Some(identity)) => {
                valid_privileged_absolute_path(root)
                    && !identity.is_empty()
                    && identity.len() <= 128
                    && !identity.as_bytes().contains(&0)
                    && !self.workspace_fields.is_empty()
                    && self
                        .workspace_fields
                        .iter()
                        .enumerate()
                        .all(|(index, field)| {
                            !self.workspace_fields[..index].contains(field)
                                && match field {
                                    AdministratorWorkspacePathField::Path => self.path.is_some(),
                                    AdministratorWorkspacePathField::Source => {
                                        self.source.is_some()
                                    }
                                    AdministratorWorkspacePathField::Destination => {
                                        self.destination.is_some()
                                    }
                                }
                        })
            }
            _ => false,
        };
        if !paths_valid
            || !bounds_valid
            || !content_valid
            || !pattern_valid
            || !workspace_binding_valid
        {
            return Err(BrokerProtocolError::MalformedFrame);
        }
        let shape_valid = match self.action {
            AdministratorFilesystemAction::List
            | AdministratorFilesystemAction::Stat
            | AdministratorFilesystemAction::Read
            | AdministratorFilesystemAction::Delete
            | AdministratorFilesystemAction::Hash => {
                self.path.is_some()
                    && self.source.is_none()
                    && self.destination.is_none()
                    && self.content_base64.is_none()
                    && self.pattern.is_none()
                    && self.kind.is_none()
                    && self.min_size.is_none()
                    && self.max_size.is_none()
                    && self.modified_after_ms.is_none()
                    && self.modified_before_ms.is_none()
            }
            AdministratorFilesystemAction::Write => {
                self.path.is_some()
                    && self.source.is_none()
                    && self.destination.is_none()
                    && self.content_base64.is_some()
                    && self.pattern.is_none()
                    && self.kind.is_none()
                    && self.min_size.is_none()
                    && self.max_size.is_none()
                    && self.modified_after_ms.is_none()
                    && self.modified_before_ms.is_none()
            }
            AdministratorFilesystemAction::Search => {
                self.path.is_some()
                    && self.source.is_none()
                    && self.destination.is_none()
                    && self.content_base64.is_none()
                    && self.pattern.is_some()
            }
            AdministratorFilesystemAction::Copy | AdministratorFilesystemAction::Move => {
                self.path.is_none()
                    && self.source.is_some()
                    && self.destination.is_some()
                    && self.content_base64.is_none()
                    && self.pattern.is_none()
                    && self.kind.is_none()
                    && self.min_size.is_none()
                    && self.max_size.is_none()
                    && self.modified_after_ms.is_none()
                    && self.modified_before_ms.is_none()
            }
        };
        shape_valid
            .then_some(())
            .ok_or(BrokerProtocolError::MalformedFrame)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdministratorFilesystemEntry {
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub modified_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdministratorFilesystemResult {
    Entries {
        action: AdministratorFilesystemAction,
        entries: Vec<AdministratorFilesystemEntry>,
        scanned_entries: u32,
        truncated: bool,
    },
    Stat {
        path: String,
        kind: String,
        size: u64,
        modified_ms: Option<u64>,
        calculated_size: bool,
        scanned_entries: u32,
        truncated: bool,
    },
    Read {
        path: String,
        offset: u64,
        total_bytes: u64,
        returned_bytes: u32,
        eof: bool,
        encoding: String,
        content: String,
    },
    Mutation {
        action: AdministratorFilesystemAction,
        path: String,
        destination: Option<String>,
        bytes: u64,
        changed: bool,
    },
    Hash {
        path: String,
        algorithm: String,
        sha256: String,
        bytes: u64,
    },
}

fn valid_privileged_absolute_path(value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_ELEVATED_STRING_BYTES
        || value.as_bytes().contains(&0)
        || value.contains(['\n', '\r'])
        || is_windows_verbatim_path(value)
    {
        return false;
    }
    let path = Path::new(value);
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
}

pub fn valid_elevated_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ELEVATED_REQUEST_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerResponseEnvelope {
    pub version: u16,
    pub generation: u64,
    pub sequence: u64,
    pub response: BrokerResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerProtocolError {
    EmptyFrame,
    OversizedFrame,
    MalformedFrame,
    ProtocolMismatch,
    StaleGeneration,
    SessionMismatch,
    Replay,
}

#[derive(Debug)]
pub struct BrokerSession {
    generation: u64,
    session_nonce: SessionNonce,
    last_sequence: u64,
}

impl BrokerSession {
    pub fn new(generation: u64, session_nonce: SessionNonce) -> Result<Self, BrokerProtocolError> {
        if generation == 0 {
            return Err(BrokerProtocolError::StaleGeneration);
        }
        Ok(Self {
            generation,
            session_nonce,
            last_sequence: 0,
        })
    }
    pub const fn generation(&self) -> u64 {
        self.generation
    }
    pub const fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
    pub fn validate_request(
        &mut self,
        request: &BrokerRequestEnvelope,
    ) -> Result<(), BrokerProtocolError> {
        if request.version != BROKER_PROTOCOL_VERSION {
            return Err(BrokerProtocolError::ProtocolMismatch);
        }
        if request.generation != self.generation {
            return Err(BrokerProtocolError::StaleGeneration);
        }
        if request.session_nonce != self.session_nonce {
            return Err(BrokerProtocolError::SessionMismatch);
        }
        if request.sequence == 0 || request.sequence <= self.last_sequence {
            return Err(BrokerProtocolError::Replay);
        }
        self.last_sequence = request.sequence;
        Ok(())
    }
}

pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, BrokerProtocolError> {
    let payload = serde_json::to_vec(value).map_err(|_| BrokerProtocolError::MalformedFrame)?;
    if payload.is_empty() {
        return Err(BrokerProtocolError::EmptyFrame);
    }
    if payload.len() > MAX_BROKER_FRAME_BYTES {
        return Err(BrokerProtocolError::OversizedFrame);
    }
    Ok(payload)
}

pub fn decode_frame<T>(payload: &[u8]) -> Result<T, BrokerProtocolError>
where
    T: for<'de> Deserialize<'de>,
{
    if payload.is_empty() {
        return Err(BrokerProtocolError::EmptyFrame);
    }
    if payload.len() > MAX_BROKER_FRAME_BYTES {
        return Err(BrokerProtocolError::OversizedFrame);
    }
    serde_json::from_slice(payload).map_err(|_| BrokerProtocolError::MalformedFrame)
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/unit/privilege/protocol.rs"
    ));
}
