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

const TASK_STATE_VERSION: u32 = 1;
const MAX_TERMINAL_COMMANDS: usize = 64;
const MAX_EVENTS: usize = 128;
const MAX_OUTPUT_REFS: usize = 4;
const MAX_STABLE_TOKEN: usize = 128;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct CommandOwner {
    pub(crate) task_id: String,
    pub(crate) session_id: String,
}

impl CommandOwner {
    pub(crate) fn new(task_id: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            task_id: bounded_token(task_id.into()),
            session_id: bounded_token(session_id.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommandTerminalStatus {
    Completed,
    Failed,
    TimedOut,
    Cancelled,
    Lost,
}

impl CommandTerminalStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::Lost => "lost",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TerminalCommandSnapshot {
    pub(crate) owner: CommandOwner,
    pub(crate) status: CommandTerminalStatus,
    pub(crate) exit_code: Option<i64>,
    pub(crate) signal: Option<String>,
    pub(crate) timed_out: bool,
    pub(crate) cancelled: bool,
    pub(crate) output_refs: Vec<String>,
    pub(crate) error_code: Option<String>,
    pub(crate) completed_at_ms: u64,
}

impl TerminalCommandSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        owner: CommandOwner,
        status: CommandTerminalStatus,
        exit_code: Option<i64>,
        signal: Option<String>,
        timed_out: bool,
        cancelled: bool,
        output_refs: Vec<String>,
        error_code: Option<String>,
    ) -> Self {
        Self {
            owner,
            status,
            exit_code,
            signal: signal.map(bounded_token),
            timed_out,
            cancelled,
            output_refs: output_refs
                .into_iter()
                .filter(|value| value.starts_with("lb-output-"))
                .take(MAX_OUTPUT_REFS)
                .map(bounded_token)
                .collect(),
            error_code: error_code.map(bounded_token),
            completed_at_ms: now_unix_ms(),
        }
    }

    fn lost(owner: CommandOwner) -> Self {
        Self::new(
            owner,
            CommandTerminalStatus::Lost,
            None,
            None,
            false,
            false,
            Vec::new(),
            Some("SessionUnavailable".to_string()),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CurrentCommandSnapshot {
    owner: CommandOwner,
    status: String,
    started_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CommandEvent {
    event: String,
    task_id: String,
    session_id: String,
    status: Option<String>,
    at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedTaskState {
    version: u32,
    current_command: Option<CurrentCommandSnapshot>,
    terminal_commands: Vec<TerminalCommandSnapshot>,
    events: Vec<CommandEvent>,
}

impl Default for PersistedTaskState {
    fn default() -> Self {
        Self {
            version: TASK_STATE_VERSION,
            current_command: None,
            terminal_commands: Vec::new(),
            events: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct CommandTaskStateInner {
    path: PathBuf,
    state: PersistedTaskState,
}

#[derive(Debug, Clone)]
pub(crate) struct CommandTaskStateStore(Arc<Mutex<CommandTaskStateInner>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandTaskStateError {
    operation: &'static str,
}

impl CommandTaskStateError {
    fn new(operation: &'static str) -> Self {
        Self { operation }
    }
}

impl fmt::Display for CommandTaskStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "command task-state {} failed", self.operation)
    }
}

impl std::error::Error for CommandTaskStateError {}

impl CommandTaskStateStore {
    pub(crate) fn for_workspace(workspace: &Path) -> Result<Self, CommandTaskStateError> {
        Self::open_at(default_task_state_path(workspace))
    }

    pub(crate) fn open_at(path: PathBuf) -> Result<Self, CommandTaskStateError> {
        let mut state = if path.exists() {
            let mut bytes = Vec::new();
            File::open(&path)
                .and_then(|mut file| file.read_to_end(&mut bytes))
                .map_err(|_| CommandTaskStateError::new("read"))?;
            let parsed: PersistedTaskState =
                serde_json::from_slice(&bytes).map_err(|_| CommandTaskStateError::new("parse"))?;
            if parsed.version != TASK_STATE_VERSION {
                return Err(CommandTaskStateError::new("version"));
            }
            parsed
        } else {
            PersistedTaskState::default()
        };

        // Runtime restart cannot leave a durable command permanently running.
        if let Some(current) = state.current_command.clone() {
            apply_finalization(&mut state, TerminalCommandSnapshot::lost(current.owner));
            trim_state(&mut state);
            persist_state(&path, &state)?;
        }

        Ok(Self(Arc::new(Mutex::new(CommandTaskStateInner {
            path,
            state,
        }))))
    }

    pub(crate) fn begin(&self, owner: CommandOwner) -> Result<(), CommandTaskStateError> {
        self.transact("begin", |state| {
            if state
                .terminal_commands
                .iter()
                .any(|terminal| terminal.owner == owner)
                || state
                    .current_command
                    .as_ref()
                    .is_some_and(|current| current.owner == owner)
            {
                return false;
            }
            state.current_command = Some(CurrentCommandSnapshot {
                owner: owner.clone(),
                status: "running".to_string(),
                started_at_ms: now_unix_ms(),
            });
            state.events.push(CommandEvent {
                event: "command_started".to_string(),
                task_id: owner.task_id,
                session_id: owner.session_id,
                status: None,
                at_ms: now_unix_ms(),
            });
            true
        })
    }

    pub(crate) fn finalize(
        &self,
        terminal: TerminalCommandSnapshot,
    ) -> Result<(), CommandTaskStateError> {
        self.transact("finalize", |state| apply_finalization(state, terminal))
    }

    pub(crate) fn latest_terminal(&self) -> Option<TerminalCommandSnapshot> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .terminal_commands
            .last()
            .cloned()
    }

    #[cfg(test)]
    pub(crate) fn latest_terminal_for_task(
        &self,
        task_id: &str,
    ) -> Option<TerminalCommandSnapshot> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .terminal_commands
            .iter()
            .rev()
            .find(|terminal| terminal.owner.task_id == task_id)
            .cloned()
    }

    pub(crate) fn terminal_for_session(&self, session_id: &str) -> Option<TerminalCommandSnapshot> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .terminal_commands
            .iter()
            .rev()
            .find(|terminal| terminal.owner.session_id == session_id)
            .cloned()
    }

    pub(crate) fn current_owner(&self) -> Option<CommandOwner> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .current_command
            .as_ref()
            .map(|current| current.owner.clone())
    }

    #[cfg(test)]
    fn state_for_test(&self) -> PersistedTaskState {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .clone()
    }

    fn transact<F>(&self, operation: &'static str, mutate: F) -> Result<(), CommandTaskStateError>
    where
        F: FnOnce(&mut PersistedTaskState) -> bool,
    {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut next = inner.state.clone();
        if !mutate(&mut next) {
            return Ok(());
        }
        trim_state(&mut next);
        persist_state(&inner.path, &next).map_err(|_| CommandTaskStateError::new(operation))?;
        inner.state = next;
        Ok(())
    }
}

fn apply_finalization(state: &mut PersistedTaskState, terminal: TerminalCommandSnapshot) -> bool {
    let owner = terminal.owner.clone();
    let already_terminal = state
        .terminal_commands
        .iter()
        .any(|existing| existing.owner == owner);
    let owns_current = state
        .current_command
        .as_ref()
        .is_some_and(|current| current.owner == owner);
    let mut changed = false;

    if !already_terminal {
        state.events.push(CommandEvent {
            event: "command_finished".to_string(),
            task_id: owner.task_id.clone(),
            session_id: owner.session_id.clone(),
            status: Some(terminal.status.as_str().to_string()),
            at_ms: terminal.completed_at_ms,
        });
        state.terminal_commands.push(terminal);
        changed = true;
    }
    if owns_current {
        state.current_command = None;
        changed = true;
    }
    changed
}

fn trim_state(state: &mut PersistedTaskState) {
    if state.terminal_commands.len() > MAX_TERMINAL_COMMANDS {
        let remove = state.terminal_commands.len() - MAX_TERMINAL_COMMANDS;
        state.terminal_commands.drain(..remove);
    }
    if state.events.len() > MAX_EVENTS {
        let remove = state.events.len() - MAX_EVENTS;
        state.events.drain(..remove);
    }
}

fn default_task_state_path(workspace: &Path) -> PathBuf {
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

fn persist_state(path: &Path, state: &PersistedTaskState) -> Result<(), CommandTaskStateError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|_| CommandTaskStateError::new("create_parent"))?;
    let mut bytes =
        serde_json::to_vec_pretty(state).map_err(|_| CommandTaskStateError::new("serialize"))?;
    bytes.push(b'\n');
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("task-state.json");
    let temp = path.with_file_name(format!(
        ".{name}.tmp-{}-{}",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| CommandTaskStateError::new("create_temp"))?;
    if file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .is_err()
    {
        let _ = fs::remove_file(&temp);
        return Err(CommandTaskStateError::new("write_temp"));
    }
    drop(file);
    if let Err(error) = atomic_replace(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(temp: &Path, target: &Path) -> Result<(), CommandTaskStateError> {
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
        Err(CommandTaskStateError::new("replace"))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(temp: &Path, target: &Path) -> Result<(), CommandTaskStateError> {
    fs::rename(temp, target).map_err(|_| CommandTaskStateError::new("replace"))
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
            "localbridge-task-state-{label}-{}-{}.json",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn owner(label: &str) -> CommandOwner {
        CommandOwner::new(format!("task-{label}"), format!("lb-session-{label}"))
    }

    fn terminal(owner: CommandOwner, status: CommandTerminalStatus) -> TerminalCommandSnapshot {
        TerminalCommandSnapshot::new(
            owner,
            status,
            (status == CommandTerminalStatus::Failed).then_some(7),
            (status == CommandTerminalStatus::Cancelled).then(|| "TERM".to_string()),
            status == CommandTerminalStatus::TimedOut,
            status == CommandTerminalStatus::Cancelled,
            vec!["lb-output-safe".to_string()],
            (status != CommandTerminalStatus::Completed).then(|| "StableError".to_string()),
        )
    }

    #[test]
    fn every_terminal_classification_atomically_finishes_and_clears_current() {
        let path = temp_path("terminal-classes");
        let store = CommandTaskStateStore::open_at(path.clone()).unwrap();
        for (index, status) in [
            CommandTerminalStatus::Completed,
            CommandTerminalStatus::Failed,
            CommandTerminalStatus::TimedOut,
            CommandTerminalStatus::Cancelled,
            CommandTerminalStatus::Lost,
        ]
        .into_iter()
        .enumerate()
        {
            let owner = owner(&format!("{index}"));
            store.begin(owner.clone()).unwrap();
            store.finalize(terminal(owner, status)).unwrap();
            assert!(store.state_for_test().current_command.is_none());
        }
        let state = store.state_for_test();
        assert_eq!(state.terminal_commands.len(), 5);
        assert_eq!(
            state
                .events
                .iter()
                .filter(|event| event.event == "command_finished")
                .count(),
            5
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn terminal_truth_survives_reload_without_any_private_runtime_session() {
        let path = temp_path("reload");
        let expected_owner = owner("reload");
        {
            let store = CommandTaskStateStore::open_at(path.clone()).unwrap();
            store.begin(expected_owner.clone()).unwrap();
            store
                .finalize(terminal(
                    expected_owner.clone(),
                    CommandTerminalStatus::TimedOut,
                ))
                .unwrap();
        }
        let reopened = CommandTaskStateStore::open_at(path.clone()).unwrap();
        let snapshot = reopened.latest_terminal().expect("durable terminal");
        assert_eq!(snapshot.owner, expected_owner);
        assert_eq!(snapshot.status, CommandTerminalStatus::TimedOut);
        assert!(snapshot.timed_out);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn delayed_old_owner_finalizer_cannot_clear_or_overwrite_new_owner() {
        let path = temp_path("owner-cas");
        let store = CommandTaskStateStore::open_at(path.clone()).unwrap();
        let owner_a = owner("a");
        let owner_b = owner("b");
        store.begin(owner_a.clone()).unwrap();
        store.begin(owner_b.clone()).unwrap();
        store
            .finalize(terminal(owner_a.clone(), CommandTerminalStatus::Completed))
            .unwrap();
        let state = store.state_for_test();
        assert_eq!(
            state.current_command.as_ref().map(|value| &value.owner),
            Some(&owner_b)
        );
        assert!(
            state
                .terminal_commands
                .iter()
                .any(|value| value.owner == owner_a)
        );
        store
            .finalize(terminal(owner_b, CommandTerminalStatus::Completed))
            .unwrap();
        assert!(store.state_for_test().current_command.is_none());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn duplicate_terminal_callback_is_idempotent_and_finished_event_is_exactly_once() {
        let path = temp_path("idempotent");
        let store = CommandTaskStateStore::open_at(path.clone()).unwrap();
        let owner = owner("same");
        store.begin(owner.clone()).unwrap();
        store
            .finalize(terminal(owner.clone(), CommandTerminalStatus::Completed))
            .unwrap();
        store
            .finalize(terminal(owner.clone(), CommandTerminalStatus::Failed))
            .unwrap();
        let state = store.state_for_test();
        assert_eq!(state.terminal_commands.len(), 1);
        assert_eq!(
            state.terminal_commands[0].status,
            CommandTerminalStatus::Completed
        );
        assert_eq!(
            state
                .events
                .iter()
                .filter(|event| event.event == "command_finished" && event.task_id == owner.task_id)
                .count(),
            1
        );
        assert!(state.current_command.is_none());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persisted_state_is_bounded_and_contains_only_public_redacted_tokens() {
        let path = temp_path("redacted");
        let store = CommandTaskStateStore::open_at(path.clone()).unwrap();
        for index in 0..(MAX_TERMINAL_COMMANDS + 12) {
            let owner = owner(&format!("redacted-{index}"));
            store.begin(owner.clone()).unwrap();
            store
                .finalize(TerminalCommandSnapshot::new(
                    owner,
                    CommandTerminalStatus::Failed,
                    Some(9),
                    Some("TERM".repeat(100)),
                    false,
                    false,
                    vec![
                        "PRIVATE_OUTPUT_SECRET".to_string(),
                        "lb-output-public-safe".repeat(20),
                    ],
                    Some("StableError".repeat(50)),
                ))
                .unwrap();
        }
        let state = store.state_for_test();
        assert_eq!(state.terminal_commands.len(), MAX_TERMINAL_COMMANDS);
        assert!(state.events.len() <= MAX_EVENTS);
        let bytes = fs::read_to_string(&path).unwrap();
        assert!(!bytes.contains("PRIVATE_OUTPUT_SECRET"));
        assert!(state.terminal_commands.iter().all(|terminal| {
            terminal.output_refs.len() <= MAX_OUTPUT_REFS
                && terminal
                    .output_refs
                    .iter()
                    .all(|value| value.starts_with("lb-output-") && value.len() <= MAX_STABLE_TOKEN)
                && terminal
                    .signal
                    .as_ref()
                    .is_none_or(|value| value.len() <= MAX_STABLE_TOKEN)
                && terminal
                    .error_code
                    .as_ref()
                    .is_none_or(|value| value.len() <= MAX_STABLE_TOKEN)
        }));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persistence_failure_does_not_publish_uncommitted_state() {
        let path = temp_path("atomic-failure");
        let store = CommandTaskStateStore::open_at(path.clone()).unwrap();
        fs::create_dir_all(&path).unwrap();
        let owner = owner("atomic-failure");
        assert!(store.begin(owner).is_err());
        let state = store.state_for_test();
        assert!(state.current_command.is_none());
        assert!(state.terminal_commands.is_empty());
        assert!(state.events.is_empty());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn stale_running_owner_is_recovered_as_lost_on_store_reopen() {
        let path = temp_path("restart-lost");
        let expected_owner = owner("restart");
        {
            let store = CommandTaskStateStore::open_at(path.clone()).unwrap();
            store.begin(expected_owner.clone()).unwrap();
        }
        let reopened = CommandTaskStateStore::open_at(path.clone()).unwrap();
        let state = reopened.state_for_test();
        assert!(state.current_command.is_none());
        assert_eq!(state.terminal_commands.len(), 1);
        assert_eq!(state.terminal_commands[0].owner, expected_owner);
        assert_eq!(
            state.terminal_commands[0].status,
            CommandTerminalStatus::Lost
        );
        assert_eq!(
            state
                .events
                .iter()
                .filter(|event| event.event == "command_finished")
                .count(),
            1
        );
        let _ = fs::remove_file(path);
    }
}
