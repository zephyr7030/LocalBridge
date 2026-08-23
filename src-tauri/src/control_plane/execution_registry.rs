use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::{
    ExecutionId, ExecutionRecord, ExecutionState, ExecutionTerminal, McpSessionId, PublicSessionId,
    RuntimeCommandHandle, TaskId, TerminalOutcome,
};

const EXECUTION_STATE_VERSION: u32 = 3;
const MAX_ACTIVE_EXECUTIONS: usize = 64;
const MAX_TERMINAL_EXECUTIONS: usize = 64;
pub(crate) const EXECUTION_ORPHAN_TTL_MS: u64 = 30 * 60 * 1_000;
pub(crate) const EXECUTION_MAX_DETACHED_AGE_MS: u64 = 24 * 60 * 60 * 1_000;
const MAX_OUTPUT_REFS: usize = 4;
const MAX_STABLE_TOKEN: usize = 128;
static EXECUTION_GENERATION: AtomicU64 = AtomicU64::new(1);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedExecutionState {
    version: u32,
    executions: Vec<ExecutionRecord>,
}

impl Default for PersistedExecutionState {
    fn default() -> Self {
        Self {
            version: EXECUTION_STATE_VERSION,
            executions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyCommandOwner {
    task_id: String,
    session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyCurrentCommand {
    owner: LegacyCommandOwner,
    started_at_ms: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyTerminalStatus {
    Completed,
    Failed,
    TimedOut,
    Cancelled,
    Lost,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyTerminalCommand {
    owner: LegacyCommandOwner,
    status: LegacyTerminalStatus,
    exit_code: Option<i64>,
    signal: Option<String>,
    output_refs: Vec<String>,
    error_code: Option<String>,
    completed_at_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyTaskState {
    version: u32,
    #[serde(rename = "current_command")]
    legacy_running_command: Option<LegacyCurrentCommand>,
    terminal_commands: Vec<LegacyTerminalCommand>,
}

#[derive(Debug)]
struct ExecutionRegistryInner {
    path: PathBuf,
    state: PersistedExecutionState,
    cancellation_signals: HashMap<ExecutionId, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExecutionRegistry(Arc<Mutex<ExecutionRegistryInner>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionRegistryError {
    Storage(&'static str),
    CapacityExceeded,
    UnknownExecution(ExecutionId),
    UnknownPublicSession(PublicSessionId),
    PublicSessionCollision(PublicSessionId),
    OwnerConflict {
        execution_id: ExecutionId,
        existing: McpSessionId,
        attempted: McpSessionId,
    },
    RuntimeHandleCollision(RuntimeCommandHandle),
    AlreadyTerminal {
        execution_id: ExecutionId,
        outcome: TerminalOutcome,
    },
}

impl fmt::Display for ExecutionRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(operation) => write!(f, "execution registry {operation} failed"),
            Self::CapacityExceeded => f.write_str("execution registry capacity exceeded"),
            Self::UnknownExecution(id) => write!(f, "unknown execution {id}"),
            Self::UnknownPublicSession(id) => write!(f, "unknown public session {id}"),
            Self::PublicSessionCollision(id) => write!(f, "public session collision {id}"),
            Self::OwnerConflict {
                execution_id,
                existing,
                attempted,
            } => write!(
                f,
                "execution {execution_id} is owned by {existing}, not {attempted}"
            ),
            Self::RuntimeHandleCollision(handle) => {
                write!(f, "runtime command handle collision {}", handle.as_str())
            }
            Self::AlreadyTerminal {
                execution_id,
                outcome,
            } => write!(
                f,
                "execution {execution_id} already terminal as {}",
                outcome.as_str()
            ),
        }
    }
}

impl std::error::Error for ExecutionRegistryError {}

impl ExecutionRegistry {
    pub(crate) fn for_workspace(workspace: &Path) -> Result<Self, ExecutionRegistryError> {
        Self::open_at(default_execution_state_path(workspace))
    }

    pub(crate) fn open_at(path: PathBuf) -> Result<Self, ExecutionRegistryError> {
        let (mut state, migrated) = if path.exists() {
            let mut bytes = Vec::new();
            File::open(&path)
                .and_then(|mut file| file.read_to_end(&mut bytes))
                .map_err(|_| ExecutionRegistryError::Storage("read"))?;
            match serde_json::from_slice::<PersistedExecutionState>(&bytes) {
                Ok(mut state) if matches!(state.version, 2 | EXECUTION_STATE_VERSION) => {
                    let migrated = state.version != EXECUTION_STATE_VERSION;
                    state.version = EXECUTION_STATE_VERSION;
                    for execution in &mut state.executions {
                        if execution.last_observed_at_ms == 0 {
                            execution.last_observed_at_ms = execution.started_at_ms;
                        }
                    }
                    (state, migrated)
                }
                _ => {
                    let legacy: LegacyTaskState = serde_json::from_slice(&bytes)
                        .map_err(|_| ExecutionRegistryError::Storage("parse"))?;
                    if legacy.version != 1 {
                        return Err(ExecutionRegistryError::Storage("version"));
                    }
                    (migrate_legacy_state(legacy), true)
                }
            }
        } else {
            (PersistedExecutionState::default(), false)
        };

        let mut recovered = false;
        for execution in &mut state.executions {
            if !execution.state.is_terminal() {
                execution.state = ExecutionState::Terminal(lost_terminal());
                recovered = true;
            }
        }
        trim_state(&mut state);
        if migrated || recovered {
            persist_state(&path, &state)?;
        }

        Ok(Self(Arc::new(Mutex::new(ExecutionRegistryInner {
            path,
            state,
            cancellation_signals: HashMap::new(),
        }))))
    }

    pub(crate) fn start(
        &self,
        task_id: TaskId,
        public_session_id: PublicSessionId,
    ) -> Result<ExecutionId, ExecutionRegistryError> {
        let execution_id = next_execution_id();
        self.transact("start", |state| {
            if state
                .executions
                .iter()
                .filter(|execution| !execution.state.is_terminal())
                .count()
                >= MAX_ACTIVE_EXECUTIONS
            {
                return Err(ExecutionRegistryError::CapacityExceeded);
            }
            if state
                .executions
                .iter()
                .any(|execution| execution.public_session_id == public_session_id)
            {
                return Err(ExecutionRegistryError::PublicSessionCollision(
                    public_session_id.clone(),
                ));
            }
            let now = now_unix_ms();
            state.executions.push(ExecutionRecord {
                id: execution_id.clone(),
                task_id,
                public_session_id,
                owner_session: None,
                runtime_handle: None,
                state: ExecutionState::Running,
                started_at_ms: now,
                last_observed_at_ms: now,
                orphaned_at_ms: None,
            });
            Ok(())
        })?;
        Ok(execution_id)
    }

    pub(crate) fn bind_owner(
        &self,
        execution_id: &ExecutionId,
        owner: McpSessionId,
    ) -> Result<(), ExecutionRegistryError> {
        self.transact("bind_owner", |state| {
            let execution = state
                .executions
                .iter_mut()
                .find(|execution| &execution.id == execution_id)
                .ok_or_else(|| ExecutionRegistryError::UnknownExecution(execution_id.clone()))?;
            match &execution.owner_session {
                Some(existing) if existing == &owner => return Ok(()),
                Some(existing) => {
                    return Err(ExecutionRegistryError::OwnerConflict {
                        execution_id: execution_id.clone(),
                        existing: existing.clone(),
                        attempted: owner,
                    });
                }
                None => {}
            }
            execution.owner_session = Some(owner);
            execution.orphaned_at_ms = None;
            Ok(())
        })
    }

    pub(crate) fn adopt_owner(
        &self,
        public_session_id: &PublicSessionId,
        owner: McpSessionId,
    ) -> Result<ExecutionRecord, ExecutionRegistryError> {
        let mut adopted = None;
        self.transact("adopt_owner", |state| {
            let execution = state
                .executions
                .iter_mut()
                .find(|execution| &execution.public_session_id == public_session_id)
                .ok_or_else(|| {
                    ExecutionRegistryError::UnknownPublicSession(public_session_id.clone())
                })?;
            if let ExecutionState::Terminal(terminal) = &execution.state {
                return Err(ExecutionRegistryError::AlreadyTerminal {
                    execution_id: execution.id.clone(),
                    outcome: terminal.outcome,
                });
            }
            execution.owner_session = Some(owner);
            execution.orphaned_at_ms = None;
            execution.last_observed_at_ms = now_unix_ms();
            adopted = Some(execution.clone());
            Ok(())
        })?;
        adopted
            .ok_or_else(|| ExecutionRegistryError::UnknownPublicSession(public_session_id.clone()))
    }

    pub(crate) fn bind_runtime_handle(
        &self,
        execution_id: &ExecutionId,
        handle: RuntimeCommandHandle,
    ) -> Result<(), ExecutionRegistryError> {
        self.transact("bind_runtime_handle", |state| {
            if state.executions.iter().any(|execution| {
                &execution.id != execution_id
                    && execution.runtime_handle.as_ref() == Some(&handle)
                    && !execution.state.is_terminal()
            }) {
                return Err(ExecutionRegistryError::RuntimeHandleCollision(handle));
            }
            let execution = state
                .executions
                .iter_mut()
                .find(|execution| &execution.id == execution_id)
                .ok_or_else(|| ExecutionRegistryError::UnknownExecution(execution_id.clone()))?;
            match &execution.runtime_handle {
                Some(existing) if existing == &handle => Ok(()),
                Some(existing) => Err(ExecutionRegistryError::RuntimeHandleCollision(
                    existing.clone(),
                )),
                None => {
                    execution.runtime_handle = Some(handle);
                    Ok(())
                }
            }
        })
    }

    pub(crate) fn request_cancellation(
        &self,
        public_session_id: &PublicSessionId,
        signal: &str,
    ) -> Result<(), ExecutionRegistryError> {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let execution = inner
            .state
            .executions
            .iter()
            .find(|execution| &execution.public_session_id == public_session_id)
            .ok_or_else(|| {
                ExecutionRegistryError::UnknownPublicSession(public_session_id.clone())
            })?;
        if let ExecutionState::Terminal(terminal) = &execution.state {
            return Err(ExecutionRegistryError::AlreadyTerminal {
                execution_id: execution.id.clone(),
                outcome: terminal.outcome,
            });
        }
        let execution_id = execution.id.clone();
        inner
            .cancellation_signals
            .insert(execution_id, bounded_token(signal.to_string()));
        Ok(())
    }

    pub(crate) fn cancellation_signal(
        &self,
        public_session_id: &PublicSessionId,
    ) -> Option<String> {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let execution = inner
            .state
            .executions
            .iter()
            .find(|execution| &execution.public_session_id == public_session_id)?;
        if execution.state.is_terminal() {
            None
        } else {
            inner.cancellation_signals.get(&execution.id).cloned()
        }
    }

    pub(crate) fn clear_cancellation(&self, public_session_id: &PublicSessionId) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let execution_id = inner
            .state
            .executions
            .iter()
            .find(|execution| &execution.public_session_id == public_session_id)
            .map(|execution| execution.id.clone());
        if let Some(execution_id) = execution_id {
            inner.cancellation_signals.remove(&execution_id);
        }
    }

    pub(crate) fn finish(
        &self,
        execution_id: &ExecutionId,
        terminal: ExecutionTerminal,
    ) -> Result<(), ExecutionRegistryError> {
        let result = self.transact("finish", |state| {
            let execution = state
                .executions
                .iter_mut()
                .find(|execution| &execution.id == execution_id)
                .ok_or_else(|| ExecutionRegistryError::UnknownExecution(execution_id.clone()))?;
            match &execution.state {
                ExecutionState::Queued | ExecutionState::Running => {
                    execution.last_observed_at_ms = terminal.completed_at_ms;
                    execution.state = ExecutionState::Terminal(sanitize_terminal(terminal));
                    Ok(())
                }
                ExecutionState::Terminal(existing) if existing.outcome == terminal.outcome => {
                    Ok(())
                }
                ExecutionState::Terminal(existing) => {
                    Err(ExecutionRegistryError::AlreadyTerminal {
                        execution_id: execution_id.clone(),
                        outcome: existing.outcome,
                    })
                }
            }
        });
        if result.is_ok() {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .cancellation_signals
                .remove(execution_id);
        }
        result
    }

    pub(crate) fn execution_for_public_session(
        &self,
        public_session_id: &PublicSessionId,
    ) -> Option<ExecutionRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .executions
            .iter()
            .find(|execution| &execution.public_session_id == public_session_id)
            .cloned()
    }

    pub(crate) fn latest_running(&self) -> Option<ExecutionRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .executions
            .iter()
            .rev()
            .find(|execution| matches!(execution.state, ExecutionState::Running))
            .cloned()
    }

    pub(crate) fn running(&self) -> Vec<ExecutionRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .executions
            .iter()
            .filter(|execution| matches!(execution.state, ExecutionState::Running))
            .cloned()
            .collect()
    }

    pub(crate) fn all(&self) -> Vec<ExecutionRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .executions
            .clone()
    }

    pub(crate) fn orphan_owned_by(
        &self,
        owner: &McpSessionId,
    ) -> Result<usize, ExecutionRegistryError> {
        let now = now_unix_ms();
        let mut orphaned = 0usize;
        self.transact("orphan_owner", |state| {
            for execution in &mut state.executions {
                if !execution.state.is_terminal() && execution.owner_session.as_ref() == Some(owner)
                {
                    execution.owner_session = None;
                    execution.orphaned_at_ms.get_or_insert(now);
                    orphaned = orphaned.saturating_add(1);
                }
            }
            Ok(())
        })?;
        Ok(orphaned)
    }

    pub(crate) fn observe_all_running(&self) -> Result<(), ExecutionRegistryError> {
        let now = now_unix_ms();
        self.transact("observe", |state| {
            for execution in &mut state.executions {
                if matches!(execution.state, ExecutionState::Running) {
                    execution.last_observed_at_ms = now;
                }
            }
            Ok(())
        })
    }

    pub(crate) fn reap_stale(
        &self,
        now_ms: u64,
    ) -> Result<Vec<ExecutionId>, ExecutionRegistryError> {
        let mut lost = Vec::new();
        self.transact("reap_stale", |state| {
            for execution in &mut state.executions {
                if execution.state.is_terminal() {
                    continue;
                }
                let orphan_expired = execution.orphaned_at_ms.is_some_and(|orphaned_at| {
                    now_ms.saturating_sub(orphaned_at) >= EXECUTION_ORPHAN_TTL_MS
                });
                let max_age_expired =
                    now_ms.saturating_sub(execution.started_at_ms) >= EXECUTION_MAX_DETACHED_AGE_MS;
                if orphan_expired || max_age_expired {
                    let execution_id = execution.id.clone();
                    execution.state = ExecutionState::Terminal(ExecutionTerminal {
                        outcome: TerminalOutcome::Lost,
                        exit_code: None,
                        signal: None,
                        output_refs: Vec::new(),
                        error_code: Some(if orphan_expired {
                            "ExecutionOrphanExpired".to_string()
                        } else {
                            "ExecutionMaxAgeExceeded".to_string()
                        }),
                        completed_at_ms: now_ms,
                    });
                    execution.last_observed_at_ms = now_ms;
                    lost.push(execution_id);
                }
            }
            Ok(())
        })?;
        if !lost.is_empty() {
            let mut inner = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for execution_id in &lost {
                inner.cancellation_signals.remove(execution_id);
            }
        }
        Ok(lost)
    }

    pub(crate) fn running_owned_by(&self, owner: &McpSessionId) -> Vec<ExecutionRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .executions
            .iter()
            .filter(|execution| {
                matches!(execution.state, ExecutionState::Running)
                    && execution.owner_session.as_ref() == Some(owner)
            })
            .cloned()
            .collect()
    }

    pub(crate) fn running_unowned(&self) -> Vec<ExecutionRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .executions
            .iter()
            .filter(|execution| {
                matches!(execution.state, ExecutionState::Running)
                    && execution.owner_session.is_none()
            })
            .cloned()
            .collect()
    }

    pub(crate) fn running_for_task(&self, task_id: &TaskId) -> Vec<ExecutionRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .executions
            .iter()
            .filter(|execution| {
                matches!(execution.state, ExecutionState::Running) && &execution.task_id == task_id
            })
            .cloned()
            .collect()
    }

    pub(crate) fn latest_terminal(&self) -> Option<ExecutionRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .executions
            .iter()
            .rev()
            .find(|execution| execution.state.is_terminal())
            .cloned()
    }

    #[cfg(test)]
    pub(crate) fn latest_terminal_for_task(&self, task_id: &TaskId) -> Option<ExecutionRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .executions
            .iter()
            .rev()
            .find(|execution| &execution.task_id == task_id && execution.state.is_terminal())
            .cloned()
    }

    fn transact<F>(&self, operation: &'static str, mutate: F) -> Result<(), ExecutionRegistryError>
    where
        F: FnOnce(&mut PersistedExecutionState) -> Result<(), ExecutionRegistryError>,
    {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut next = inner.state.clone();
        mutate(&mut next)?;
        trim_state(&mut next);
        persist_state(&inner.path, &next)
            .map_err(|_| ExecutionRegistryError::Storage(operation))?;
        inner.state = next;
        Ok(())
    }
}

fn sanitize_terminal(mut terminal: ExecutionTerminal) -> ExecutionTerminal {
    terminal.signal = terminal.signal.map(bounded_token);
    terminal.output_refs = terminal
        .output_refs
        .into_iter()
        .filter(|value| value.starts_with("lb-output-"))
        .take(MAX_OUTPUT_REFS)
        .map(bounded_token)
        .collect();
    terminal.error_code = terminal.error_code.map(bounded_token);
    terminal
}

fn lost_terminal() -> ExecutionTerminal {
    ExecutionTerminal {
        outcome: TerminalOutcome::Lost,
        exit_code: None,
        signal: None,
        output_refs: Vec::new(),
        error_code: Some("SessionUnavailable".to_string()),
        completed_at_ms: now_unix_ms(),
    }
}

fn migrate_legacy_state(legacy: LegacyTaskState) -> PersistedExecutionState {
    let mut executions = Vec::new();
    for (index, terminal) in legacy.terminal_commands.into_iter().enumerate() {
        executions.push(ExecutionRecord {
            id: ExecutionId::new(format!("legacy-execution-{index}")),
            task_id: TaskId::new(terminal.owner.task_id),
            public_session_id: PublicSessionId::new(terminal.owner.session_id),
            owner_session: None,
            runtime_handle: None,
            state: ExecutionState::Terminal(sanitize_terminal(ExecutionTerminal {
                outcome: match terminal.status {
                    LegacyTerminalStatus::Completed => TerminalOutcome::Completed,
                    LegacyTerminalStatus::Failed => TerminalOutcome::Failed,
                    LegacyTerminalStatus::TimedOut => TerminalOutcome::TimedOut,
                    LegacyTerminalStatus::Cancelled => TerminalOutcome::Cancelled,
                    LegacyTerminalStatus::Lost => TerminalOutcome::Lost,
                },
                exit_code: terminal.exit_code,
                signal: terminal.signal,
                output_refs: terminal.output_refs,
                error_code: terminal.error_code,
                completed_at_ms: terminal.completed_at_ms,
            })),
            started_at_ms: terminal.completed_at_ms,
            last_observed_at_ms: terminal.completed_at_ms,
            orphaned_at_ms: None,
        });
    }
    if let Some(current) = legacy.legacy_running_command {
        if !executions
            .iter()
            .any(|execution| execution.public_session_id.as_str() == current.owner.session_id)
        {
            executions.push(ExecutionRecord {
                id: ExecutionId::new("legacy-execution-recovered"),
                task_id: TaskId::new(current.owner.task_id),
                public_session_id: PublicSessionId::new(current.owner.session_id),
                owner_session: None,
                runtime_handle: None,
                state: ExecutionState::Terminal(lost_terminal()),
                started_at_ms: current.started_at_ms,
                last_observed_at_ms: current.started_at_ms,
                orphaned_at_ms: None,
            });
        }
    }
    PersistedExecutionState {
        version: EXECUTION_STATE_VERSION,
        executions,
    }
}

fn trim_state(state: &mut PersistedExecutionState) {
    while state
        .executions
        .iter()
        .filter(|execution| execution.state.is_terminal())
        .count()
        > MAX_TERMINAL_EXECUTIONS
    {
        let Some(index) = state
            .executions
            .iter()
            .position(|execution| execution.state.is_terminal())
        else {
            break;
        };
        state.executions.remove(index);
    }
}

fn default_execution_state_path(workspace: &Path) -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    let mut normalized = workspace.to_string_lossy().replace('/', "\\");
    #[cfg(windows)]
    normalized.make_ascii_lowercase();
    let digest = Sha256::digest(normalized.as_bytes());
    use std::fmt::Write as _;
    let mut key = String::with_capacity(32);
    for byte in &digest[..16] {
        write!(&mut key, "{byte:02x}").expect("writing to String cannot fail");
    }
    base.join("LocalBridge")
        .join("task-state")
        .join(format!("workspace-{key}.json"))
}

fn persist_state(
    path: &Path,
    state: &PersistedExecutionState,
) -> Result<(), ExecutionRegistryError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|_| ExecutionRegistryError::Storage("create_parent"))?;
    let mut bytes = serde_json::to_vec_pretty(state)
        .map_err(|_| ExecutionRegistryError::Storage("serialize"))?;
    bytes.push(b'\n');
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("execution-state.json");
    let temp = path.with_file_name(format!(
        ".{name}.tmp-{}-{}",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| ExecutionRegistryError::Storage("create_temp"))?;
    if file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .is_err()
    {
        let _ = fs::remove_file(&temp);
        return Err(ExecutionRegistryError::Storage("write_temp"));
    }
    drop(file);
    if let Err(error) = atomic_replace(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(temp: &Path, target: &Path) -> Result<(), ExecutionRegistryError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
    let temp = wide(temp);
    let target = wide(target);
    let ok = unsafe {
        MoveFileExW(
            temp.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(ExecutionRegistryError::Storage("replace"))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(temp: &Path, target: &Path) -> Result<(), ExecutionRegistryError> {
    fs::rename(temp, target).map_err(|_| ExecutionRegistryError::Storage("replace"))
}

fn next_execution_id() -> ExecutionId {
    let generation = EXECUTION_GENERATION.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    ExecutionId::new(format!(
        "lb-execution-{:x}-{now:x}-{generation:x}",
        std::process::id()
    ))
}

fn bounded_token(mut value: String) -> String {
    if value.len() > MAX_STABLE_TOKEN {
        value.truncate(MAX_STABLE_TOKEN);
    }
    value
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "localbridge-execution-registry-{label}-{}-{}.json",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn terminal(outcome: TerminalOutcome) -> ExecutionTerminal {
        ExecutionTerminal {
            outcome,
            exit_code: (outcome == TerminalOutcome::Failed).then_some(7),
            signal: None,
            output_refs: Vec::new(),
            error_code: None,
            completed_at_ms: now_unix_ms(),
        }
    }

    #[test]
    fn multiple_executions_run_without_a_single_current_slot() {
        let path = temp_path("multiple");
        let registry = ExecutionRegistry::open_at(path.clone()).unwrap();
        let a = registry
            .start(TaskId::new("task-a"), PublicSessionId::new("session-a"))
            .unwrap();
        let b = registry
            .start(TaskId::new("task-b"), PublicSessionId::new("session-b"))
            .unwrap();
        assert_eq!(registry.running().len(), 2);
        registry
            .finish(&b, terminal(TerminalOutcome::Completed))
            .unwrap();
        assert_eq!(registry.running().len(), 1);
        assert_eq!(registry.running()[0].id, a);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn execution_has_exactly_one_terminal_outcome() {
        let path = temp_path("terminal-once");
        let registry = ExecutionRegistry::open_at(path.clone()).unwrap();
        let execution = registry
            .start(TaskId::new("task"), PublicSessionId::new("session"))
            .unwrap();
        registry
            .finish(&execution, terminal(TerminalOutcome::Completed))
            .unwrap();
        assert_eq!(
            registry.finish(&execution, terminal(TerminalOutcome::Failed)),
            Err(ExecutionRegistryError::AlreadyTerminal {
                execution_id: execution,
                outcome: TerminalOutcome::Completed,
            })
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn runtime_restart_converges_every_unfinished_execution_to_lost() {
        let path = temp_path("restart-lost");
        {
            let registry = ExecutionRegistry::open_at(path.clone()).unwrap();
            registry
                .start(TaskId::new("task-a"), PublicSessionId::new("session-a"))
                .unwrap();
            registry
                .start(TaskId::new("task-b"), PublicSessionId::new("session-b"))
                .unwrap();
        }
        let reopened = ExecutionRegistry::open_at(path.clone()).unwrap();
        assert!(reopened.running().is_empty());
        let state = reopened
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .clone();
        assert!(state.executions.iter().all(|execution| matches!(
            execution.state,
            ExecutionState::Terminal(ExecutionTerminal {
                outcome: TerminalOutcome::Lost,
                ..
            })
        )));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn active_execution_capacity_is_bounded_and_terminal_capacity_is_reusable() {
        let path = temp_path("active-capacity");
        let registry = ExecutionRegistry::open_at(path.clone()).unwrap();
        let mut executions = Vec::new();
        for index in 0..MAX_ACTIVE_EXECUTIONS {
            executions.push(
                registry
                    .start(
                        TaskId::new(format!("task-{index}")),
                        PublicSessionId::new(format!("session-{index}")),
                    )
                    .unwrap(),
            );
        }
        assert_eq!(
            registry.start(
                TaskId::new("overflow-task"),
                PublicSessionId::new("overflow-session"),
            ),
            Err(ExecutionRegistryError::CapacityExceeded)
        );
        registry
            .finish(&executions[0], terminal(TerminalOutcome::Completed))
            .unwrap();
        registry
            .start(
                TaskId::new("replacement-task"),
                PublicSessionId::new("replacement-session"),
            )
            .unwrap();
        let _ = fs::remove_file(path);
    }

    #[test]
    fn execution_owner_cannot_be_rebound_to_another_mcp_session() {
        let path = temp_path("owner");
        let registry = ExecutionRegistry::open_at(path.clone()).unwrap();
        let execution = registry
            .start(TaskId::new("task"), PublicSessionId::new("public"))
            .unwrap();
        registry
            .bind_owner(&execution, McpSessionId::new("mcp-a"))
            .unwrap();
        assert!(matches!(
            registry.bind_owner(&execution, McpSessionId::new("mcp-b")),
            Err(ExecutionRegistryError::OwnerConflict { .. })
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn disconnected_execution_can_be_adopted_before_orphan_ttl() {
        let path = temp_path("orphan-adopt");
        let registry = ExecutionRegistry::open_at(path.clone()).unwrap();
        let public = PublicSessionId::new("public");
        let execution = registry.start(TaskId::new("task"), public.clone()).unwrap();
        let original = McpSessionId::new("mcp-a");
        registry.bind_owner(&execution, original.clone()).unwrap();
        assert_eq!(registry.orphan_owned_by(&original).unwrap(), 1);
        let orphan = registry.execution_for_public_session(&public).unwrap();
        assert_eq!(orphan.owner_session, None);
        assert!(orphan.orphaned_at_ms.is_some());

        let adopted = registry
            .adopt_owner(&public, McpSessionId::new("mcp-b"))
            .unwrap();
        assert_eq!(adopted.owner_session, Some(McpSessionId::new("mcp-b")));
        assert_eq!(adopted.orphaned_at_ms, None);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn orphan_ttl_converges_execution_exactly_once_to_lost() {
        let path = temp_path("orphan-expiry");
        let registry = ExecutionRegistry::open_at(path.clone()).unwrap();
        let public = PublicSessionId::new("public");
        let execution = registry.start(TaskId::new("task"), public.clone()).unwrap();
        let owner = McpSessionId::new("mcp-a");
        registry.bind_owner(&execution, owner.clone()).unwrap();
        registry.orphan_owned_by(&owner).unwrap();
        let orphaned_at = registry
            .execution_for_public_session(&public)
            .unwrap()
            .orphaned_at_ms
            .unwrap();
        assert_eq!(
            registry
                .reap_stale(orphaned_at + EXECUTION_ORPHAN_TTL_MS)
                .unwrap(),
            vec![execution.clone()]
        );
        assert!(matches!(
            registry
                .execution_for_public_session(&public)
                .unwrap()
                .state,
            ExecutionState::Terminal(ExecutionTerminal {
                outcome: TerminalOutcome::Lost,
                ..
            })
        ));
        assert!(
            registry
                .reap_stale(orphaned_at + EXECUTION_ORPHAN_TTL_MS + 1)
                .unwrap()
                .is_empty()
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn runtime_command_handle_has_one_execution_owner() {
        let path = temp_path("runtime-handle-owner");
        let registry = ExecutionRegistry::open_at(path.clone()).unwrap();
        let first = registry
            .start(TaskId::new("task-a"), PublicSessionId::new("public-a"))
            .unwrap();
        let second = registry
            .start(TaskId::new("task-b"), PublicSessionId::new("public-b"))
            .unwrap();
        let handle = RuntimeCommandHandle::new("runtime-session");

        registry
            .bind_runtime_handle(&first, handle.clone())
            .unwrap();
        registry
            .bind_runtime_handle(&first, handle.clone())
            .unwrap();
        assert_eq!(
            registry.bind_runtime_handle(&second, handle.clone()),
            Err(ExecutionRegistryError::RuntimeHandleCollision(handle))
        );
        let record = registry
            .execution_for_public_session(&PublicSessionId::new("public-a"))
            .unwrap();
        assert_eq!(
            record.runtime_handle.as_ref().unwrap().as_str(),
            "runtime-session"
        );
        assert!(
            !serde_json::to_string(&record)
                .unwrap()
                .contains("runtime-session")
        );
        let _ = fs::remove_file(path);
    }
}
