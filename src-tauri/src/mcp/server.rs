use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, TryLockError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use crate::diagnostics::error::{
    ErrorDiagnostic, mcp_invalid, mcp_unavailable, mcp_unknown, transport_unavailable,
};
use crate::diagnostics::{
    record_mcp_request_error, record_mcp_request_result, record_mcp_request_start,
};
#[cfg(test)]
use crate::privilege::PrivilegedFilesystemResult;
use crate::privilege::{
    AdministratorFilesystemAction, AdministratorFilesystemErrorCode, AdministratorFilesystemKind,
    AdministratorFilesystemResult, AdministratorFilesystemSortBy, AdministratorFilesystemSortOrder,
    AdministratorFilesystemSpec, AdministratorWorkspacePathField, ElevatedExecOutcome,
    ElevatedExecSpec, PrivilegedExecError, PrivilegedExecution, PrivilegedFilesystemSpec,
};
use crate::state::{
    Capability, CurrentTask, CurrentTaskStatus, CurrentTaskTiming, LastToolTiming, PermissionMode,
    PrivilegeState, SafeTaskSummary, TaskExecutionState, TaskKind,
};

use super::facade::{
    AGENT_API_REVISION, AgentFacade, CodingRuntimeHealth, CodingToolsRuntimeAdapter,
    FacadeCallError, FacadeDenied, FacadeError, FacadeErrorCode, FilesystemAction,
    FilesystemRequest, normalize_path_authority_error, parse_filesystem_request,
    public_error_output_schema, public_tools_for_policy, run_workspace_filesystem_with_authority,
    stable_success, validate_workspace_context_probe,
};
use super::filesystem_service::FilesystemCancellation;
use super::http::{McpCancellationClient, McpHealthClient};
use super::path_authority::PathAuthority;
use super::policy::{CapabilityPolicy, explicit_control_plane_reference};
use super::runtime::{CodingToolsRuntime, CodingToolsRuntimeError};
use super::shell::{ShellExecutionSpec, ShellExecutor, ShellSelector};
use super::task_state::{CommandTaskStateStore, TerminalCommandSnapshot};

const CURRENT_PROTOCOL_VERSION: &str = "2025-11-25";
const COMPATIBLE_PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);
const ACCEPT_IDLE: Duration = Duration::from_millis(10);
const MIN_TASK_PRESENTATION: Duration = Duration::from_millis(500);
const MAX_CONNECTION_WORKERS: usize = 32;
const MAX_DOWNSTREAM_MCP_SESSIONS: usize = 64;
static SESSION_GENERATION: AtomicU64 = AtomicU64::new(1);
static PRIVILEGED_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
struct McpSession {
    protocol: String,
    tool_catalog_signature: String,
    tools_list_changed_pending: bool,
}

struct ConnectionContext<'a> {
    guard: &'a Mutex<AgentFacade<CodingToolsRuntimeAdapter>>,
    public_policy: &'a RwLock<CapabilityPolicy>,
    cancellation: &'a McpCancellationClient,
    permission_mode: &'a RwLock<PermissionMode>,
    current_task: &'a CurrentTaskProjection,
    task_state: &'a CommandTaskStateStore,
    sessions: &'a Mutex<HashMap<String, McpSession>>,
    active_requests: &'a Mutex<Vec<Value>>,
    local_filesystem_requests: &'a Mutex<Vec<(Value, FilesystemCancellation)>>,
    privileged: Option<&'a Arc<dyn PrivilegedExecution>>,
    privileged_requests: &'a Mutex<Vec<(Value, String)>>,
    privileged_filesystem_requests: &'a Mutex<Vec<(Value, String)>>,
    stopping: &'a AtomicBool,
}

struct ElevatedCallContext<'a> {
    guard: &'a Mutex<AgentFacade<CodingToolsRuntimeAdapter>>,
    privileged: Option<&'a Arc<dyn PrivilegedExecution>>,
    current_task: &'a CurrentTaskProjection,
    active_requests: &'a Mutex<Vec<Value>>,
    privileged_requests: &'a Mutex<Vec<(Value, String)>>,
    stopping: &'a AtomicBool,
}

struct AdministratorFilesystemContext<'a> {
    guard: &'a Mutex<AgentFacade<CodingToolsRuntimeAdapter>>,
    privileged: Option<&'a Arc<dyn PrivilegedExecution>>,
    current_task: &'a CurrentTaskProjection,
    active_requests: &'a Mutex<Vec<Value>>,
    privileged_filesystem_requests: &'a Mutex<Vec<(Value, String)>>,
    stopping: &'a AtomicBool,
}

struct WorkspaceFilesystemContext<'a> {
    guard: &'a Mutex<AgentFacade<CodingToolsRuntimeAdapter>>,
    current_task: &'a CurrentTaskProjection,
    active_requests: &'a Mutex<Vec<Value>>,
    local_filesystem_requests: &'a Mutex<Vec<(Value, FilesystemCancellation)>>,
    stopping: &'a AtomicBool,
}

struct TaskControlContext<'a> {
    guard: &'a Mutex<AgentFacade<CodingToolsRuntimeAdapter>>,
    public_policy: &'a RwLock<CapabilityPolicy>,
    cancellation: &'a McpCancellationClient,
    current_task: &'a CurrentTaskProjection,
    task_state: &'a CommandTaskStateStore,
    active_requests: &'a Mutex<Vec<Value>>,
    local_filesystem_requests: &'a Mutex<Vec<(Value, FilesystemCancellation)>>,
    privileged: Option<&'a Arc<dyn PrivilegedExecution>>,
    privileged_requests: &'a Mutex<Vec<(Value, String)>>,
    privileged_filesystem_requests: &'a Mutex<Vec<(Value, String)>>,
}

struct ServeContext {
    guard: Arc<Mutex<AgentFacade<CodingToolsRuntimeAdapter>>>,
    public_policy: Arc<RwLock<CapabilityPolicy>>,
    cancellation: McpCancellationClient,
    permission_mode: Arc<RwLock<PermissionMode>>,
    current_task: CurrentTaskProjection,
    task_state: CommandTaskStateStore,
    sessions: Arc<Mutex<HashMap<String, McpSession>>>,
    privileged: Option<Arc<dyn PrivilegedExecution>>,
    shutdown: mpsc::Receiver<()>,
}

#[derive(Debug, Clone)]
struct PresentedTask {
    sequence: u64,
    task: CurrentTask,
    visible_since: Instant,
    completed_at: Option<Instant>,
}

#[derive(Debug, Clone)]
struct QueuedTask {
    sequence: u64,
    task: CurrentTask,
    completed_at: Option<Instant>,
}

#[derive(Debug, Clone)]
struct CompletedTool {
    task: CurrentTask,
    completed_at: Instant,
}

#[derive(Debug)]
struct CurrentTaskProjectionState {
    actual_status: CurrentTaskStatus,
    current: Option<PresentedTask>,
    queued: VecDeque<QueuedTask>,
    active_sequence: Option<u64>,
    next_sequence: u64,
    last_tool: Option<CompletedTool>,
}

impl Default for CurrentTaskProjectionState {
    fn default() -> Self {
        Self {
            actual_status: CurrentTaskStatus::Idle,
            current: None,
            queued: VecDeque::new(),
            active_sequence: None,
            next_sequence: 1,
            last_tool: None,
        }
    }
}

pub type CurrentTaskWake = Arc<dyn Fn() + Send + Sync + 'static>;

struct CurrentTaskProjectionInner {
    state: Mutex<CurrentTaskProjectionState>,
    wake: Option<CurrentTaskWake>,
}

#[derive(Clone)]
pub struct CurrentTaskProjection(Arc<CurrentTaskProjectionInner>);

impl Default for CurrentTaskProjection {
    fn default() -> Self {
        Self::new(None)
    }
}

impl fmt::Debug for CurrentTaskProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CurrentTaskProjection")
            .field(&self.snapshot())
            .finish()
    }
}

impl CurrentTaskProjection {
    pub fn new(wake: Option<CurrentTaskWake>) -> Self {
        Self(Arc::new(CurrentTaskProjectionInner {
            state: Mutex::new(CurrentTaskProjectionState::default()),
            wake,
        }))
    }

    pub fn snapshot(&self) -> CurrentTaskStatus {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .current
            .as_ref()
            .map(|current| CurrentTaskStatus::Active(current.task.clone()))
            .unwrap_or(CurrentTaskStatus::Idle)
    }

    fn actual_snapshot(&self) -> CurrentTaskStatus {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .actual_status
            .clone()
    }

    fn activity_observation(&self) -> (Option<Value>, Option<Value>) {
        let now = Instant::now();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = match &state.actual_status {
            CurrentTaskStatus::Idle => None,
            CurrentTaskStatus::Active(task) => {
                let visible_since = state.active_sequence.and_then(|sequence| {
                    state
                        .current
                        .as_ref()
                        .filter(|item| item.sequence == sequence)
                        .map(|item| item.visible_since)
                });
                let elapsed_ms = visible_since.map(|started| {
                    now.saturating_duration_since(started)
                        .as_millis()
                        .min(u64::MAX as u128) as u64
                });
                Some(current_task_activity_value(task, elapsed_ms))
            }
        };
        let mut latest = state
            .last_tool
            .as_ref()
            .map(|item| (item.task.clone(), item.completed_at));
        if let Some(item) = state
            .current
            .as_ref()
            .filter(|item| item.completed_at.is_some())
        {
            let completed_at = item.completed_at.expect("filtered completed task");
            if latest
                .as_ref()
                .is_none_or(|(_, previous)| completed_at > *previous)
            {
                latest = Some((item.task.clone(), completed_at));
            }
        }
        for item in state
            .queued
            .iter()
            .filter(|item| item.completed_at.is_some())
        {
            let completed_at = item.completed_at.expect("filtered completed task");
            if latest
                .as_ref()
                .is_none_or(|(_, previous)| completed_at > *previous)
            {
                latest = Some((item.task.clone(), completed_at));
            }
        }
        let last = latest.map(|(task, completed_at)| {
            let age_ms = now
                .saturating_duration_since(completed_at)
                .as_millis()
                .min(u64::MAX as u128) as u64;
            json!({
                "kind":activity_kind_name(task.kind),
                "summary":task.summary.as_deref(),
                "outcome":completed_task_outcome(task.state),
                "completed_at_ms":now_ms.saturating_sub(age_ms)
            })
        });
        (current, last)
    }

    fn project(&self, status: CurrentTaskStatus) {
        let now = Instant::now();
        let mut schedule = None;
        {
            let mut state = self
                .0
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match &status {
                CurrentTaskStatus::Active(task) => {
                    let sequence = match state.active_sequence {
                        Some(sequence)
                            if matches!(state.actual_status, CurrentTaskStatus::Active(_)) =>
                        {
                            sequence
                        }
                        _ => {
                            let sequence = state.next_sequence;
                            state.next_sequence = state.next_sequence.saturating_add(1);
                            state.active_sequence = Some(sequence);
                            if state.current.is_none() {
                                state.current = Some(PresentedTask {
                                    sequence,
                                    task: task.clone(),
                                    visible_since: now,
                                    completed_at: None,
                                });
                            } else {
                                state.queued.push_back(QueuedTask {
                                    sequence,
                                    task: task.clone(),
                                    completed_at: None,
                                });
                            }
                            sequence
                        }
                    };
                    if let Some(current) = state
                        .current
                        .as_mut()
                        .filter(|item| item.sequence == sequence)
                    {
                        current.task = task.clone();
                    } else if let Some(queued) = state
                        .queued
                        .iter_mut()
                        .find(|item| item.sequence == sequence)
                    {
                        queued.task = task.clone();
                    }
                    state.actual_status = status;
                }
                CurrentTaskStatus::Idle => {
                    if let Some(sequence) = state.active_sequence.take() {
                        if let Some(current) = state
                            .current
                            .as_mut()
                            .filter(|item| item.sequence == sequence)
                        {
                            current.completed_at = Some(now);
                            schedule = Some((sequence, current.visible_since));
                        } else if let Some(queued) = state
                            .queued
                            .iter_mut()
                            .find(|item| item.sequence == sequence)
                        {
                            queued.completed_at = Some(now);
                        }
                    }
                    state.actual_status = CurrentTaskStatus::Idle;
                }
            }
        }
        self.wake();
        if let Some((sequence, visible_since)) = schedule {
            self.schedule_retirement(sequence, visible_since);
        }
    }

    pub fn timing_snapshot(&self) -> CurrentTaskTiming {
        let now = Instant::now();
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let status = state
            .current
            .as_ref()
            .map(|current| CurrentTaskStatus::Active(current.task.clone()))
            .unwrap_or(CurrentTaskStatus::Idle);
        CurrentTaskTiming {
            status,
            elapsed_ms: state.current.as_ref().map(|current| {
                now.saturating_duration_since(current.visible_since)
                    .as_millis()
                    .min(u64::MAX as u128) as u64
            }),
            last_tool: state.last_tool.as_ref().map(|last| LastToolTiming {
                kind: last.task.kind,
                summary: last.task.summary.clone(),
                age_ms: now
                    .saturating_duration_since(last.completed_at)
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
            }),
        }
    }

    fn schedule_retirement(&self, sequence: u64, visible_since: Instant) {
        let projection = self.clone();
        let due = visible_since + MIN_TASK_PRESENTATION;
        let _ = thread::Builder::new()
            .name("localbridge-task-presentation".into())
            .spawn(move || {
                let now = Instant::now();
                if due > now {
                    thread::sleep(due - now);
                }
                projection.retire_if_completed(sequence);
            });
    }

    fn retire_if_completed(&self, sequence: u64) {
        let mut next_schedule = None;
        let changed = {
            let mut state = self
                .0
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(current) = state.current.as_ref() else {
                return;
            };
            if current.sequence != sequence
                || current.completed_at.is_none()
                || current.visible_since.elapsed() < MIN_TASK_PRESENTATION
            {
                return;
            }
            let completed = state.current.take().expect("checked current task");
            state.last_tool = Some(CompletedTool {
                task: completed.task,
                completed_at: completed.completed_at.expect("checked completion time"),
            });
            if let Some(queued) = state.queued.pop_front() {
                let visible_since = Instant::now();
                let sequence = queued.sequence;
                let completed_at = queued.completed_at;
                state.current = Some(PresentedTask {
                    sequence,
                    task: queued.task,
                    visible_since,
                    completed_at,
                });
                if completed_at.is_some() {
                    next_schedule = Some((sequence, visible_since));
                }
            }
            true
        };
        if changed {
            self.wake();
        }
        if let Some((sequence, visible_since)) = next_schedule {
            self.schedule_retirement(sequence, visible_since);
        }
    }

    fn wake(&self) {
        if let Some(wake) = self.0.wake.as_ref() {
            wake();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyEnforcementError {
    BindFailed,
    UpstreamCancellationUnavailable,
    UpstreamHealthUnavailable,
    UpstreamFacadeNegotiationFailed,
    ThreadSpawnFailed,
    ThreadTerminated,
}

impl fmt::Display for PolicyEnforcementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindFailed => f.write_str("policy enforcement loopback bind failed"),
            Self::UpstreamCancellationUnavailable => f.write_str(
                "policy enforcement upstream MCP cancellation client is unavailable",
            ),
            Self::UpstreamHealthUnavailable => {
                f.write_str("policy enforcement upstream MCP health client is unavailable")
            }
            Self::UpstreamFacadeNegotiationFailed => {
                f.write_str("policy enforcement upstream facade negotiation failed")
            }
            Self::ThreadSpawnFailed => f.write_str("policy enforcement thread could not start"),
            Self::ThreadTerminated => {
                f.write_str("policy enforcement thread terminated unexpectedly")
            }
        }
    }
}

impl std::error::Error for PolicyEnforcementError {}

pub struct PolicyEnforcementRuntime {
    port: u16,
    permission_mode: Arc<RwLock<PermissionMode>>,
    current_task: CurrentTaskProjection,
    task_state: CommandTaskStateStore,
    guard: Option<Arc<Mutex<AgentFacade<CodingToolsRuntimeAdapter>>>>,
    public_policy: Arc<RwLock<CapabilityPolicy>>,
    health_client: McpHealthClient,
    health_workspace: PathBuf,
    shutdown: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<AgentFacade<CodingToolsRuntimeAdapter>>>,
}

impl fmt::Debug for PolicyEnforcementRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PolicyEnforcementRuntime")
            .field("endpoint", &self.endpoint())
            .field("running", &self.is_running())
            .field("current_task", &self.current_task.snapshot())
            .finish()
    }
}

impl PolicyEnforcementRuntime {
    pub fn start(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_inner(coding_runtime, policy, permission_mode, None, None)
    }

    pub fn start_with_wake(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
        wake: CurrentTaskWake,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_inner(coding_runtime, policy, permission_mode, None, Some(wake))
    }

    pub fn start_with_privilege(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
        privileged: Arc<dyn PrivilegedExecution>,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_inner(
            coding_runtime,
            policy,
            permission_mode,
            Some(privileged),
            None,
        )
    }

    pub fn start_with_privilege_and_wake(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
        privileged: Arc<dyn PrivilegedExecution>,
        wake: CurrentTaskWake,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_inner(
            coding_runtime,
            policy,
            permission_mode,
            Some(privileged),
            Some(wake),
        )
    }

    fn start_inner(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
        privileged: Option<Arc<dyn PrivilegedExecution>>,
        wake: Option<CurrentTaskWake>,
    ) -> Result<Self, PolicyEnforcementError> {
        let public_policy = Arc::new(RwLock::new(policy.clone()));
        let health_workspace = coding_runtime.workspace().to_path_buf();
        let cancellation = coding_runtime
            .cancellation_client()
            .map_err(|_| PolicyEnforcementError::UpstreamCancellationUnavailable)?;
        let health_client = coding_runtime
            .health_client()
            .map_err(|_| PolicyEnforcementError::UpstreamHealthUnavailable)?;
        let guard = AgentFacade::from_coding_runtime(coding_runtime, policy)
            .map_err(|_| PolicyEnforcementError::UpstreamFacadeNegotiationFailed)?;
        let task_state = guard.command_task_state();
        let runtime_task_state = task_state.clone();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| PolicyEnforcementError::BindFailed)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| PolicyEnforcementError::BindFailed)?;
        let port = listener
            .local_addr()
            .map_err(|_| PolicyEnforcementError::BindFailed)?
            .port();
        let permission_mode = Arc::new(RwLock::new(permission_mode));
        let current_task = CurrentTaskProjection::new(wake);
        let sessions = Arc::new(Mutex::new(HashMap::<String, McpSession>::new()));
        let thread_mode = Arc::clone(&permission_mode);
        let thread_task = current_task.clone();
        let thread_sessions = Arc::clone(&sessions);
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let guard = Arc::new(Mutex::new(guard));
        let thread_guard = Arc::clone(&guard);
        let thread_policy = Arc::clone(&public_policy);
        let thread = thread::Builder::new()
            .name("localbridge-mcp-policy".into())
            .spawn(move || {
                serve(
                    listener,
                    ServeContext {
                        guard: thread_guard,
                        public_policy: thread_policy,
                        cancellation,
                        permission_mode: thread_mode,
                        current_task: thread_task,
                        task_state,
                        sessions: thread_sessions,
                        privileged,
                        shutdown: shutdown_rx,
                    },
                )
            })
            .map_err(|_| PolicyEnforcementError::ThreadSpawnFailed)?;
        Ok(Self {
            port,
            permission_mode,
            current_task,
            task_state: runtime_task_state,
            guard: Some(guard),
            public_policy,
            health_client,
            health_workspace,
            shutdown: Some(shutdown_tx),
            thread: Some(thread),
        })
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.port)
    }

    pub fn set_permission_mode(&self, mode: PermissionMode) {
        let mut current = self
            .permission_mode
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = mode;
    }

    pub fn replace_policy(&self, policy: CapabilityPolicy) -> Result<(), PolicyEnforcementError> {
        let Some(guard) = self.guard.as_ref() else {
            return Err(PolicyEnforcementError::ThreadTerminated);
        };
        let mut public_policy = self
            .public_policy
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace_policy(policy.clone());
        *public_policy = policy;
        Ok(())
    }

    pub fn current_task_projection(&self) -> CurrentTaskProjection {
        self.current_task.clone()
    }

    pub fn task_aggregate_snapshot(&self) -> Value {
        let aggregate = match self.guard.as_ref() {
            None => task_control_snapshot_with_terminal(
                &self.current_task.actual_snapshot(),
                &self.task_state,
            ),
            Some(guard) => match guard.try_lock() {
                Ok(guard) => guard.task_aggregate_snapshot(),
                Err(TryLockError::WouldBlock) => task_control_snapshot_with_terminal(
                    &self.current_task.actual_snapshot(),
                    &self.task_state,
                ),
                Err(TryLockError::Poisoned(error)) => error.into_inner().task_aggregate_snapshot(),
            },
        };
        merge_task_aggregate_activity(aggregate, &self.current_task)
    }

    pub fn is_running(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }

    pub fn upstream_root_is_running(
        &self,
    ) -> Result<Option<bool>, super::runtime::CodingToolsRuntimeError> {
        let Some(guard) = self.guard.as_ref() else {
            return Ok(Some(false));
        };
        match guard.try_lock() {
            Ok(guard) => guard.runtime_root_is_running(),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(error)) => error.into_inner().runtime_root_is_running(),
        }
    }

    pub fn coding_runtime_health(&self) -> Result<Option<CodingRuntimeHealth>, FacadeError> {
        match self
            .health_client
            .probe_default_cwd(Duration::from_millis(750))
        {
            Ok(raw) if validate_workspace_context_probe(&raw, &self.health_workspace).is_ok() => {
                Ok(Some(CodingRuntimeHealth {
                    state: super::facade::CodingRuntimeHealthState::Ready,
                    root_process_alive: true,
                    authenticated_mcp: true,
                    fault: None,
                }))
            }
            Ok(_) => Ok(Some(CodingRuntimeHealth {
                state: super::facade::CodingRuntimeHealthState::Fault,
                root_process_alive: true,
                authenticated_mcp: false,
                fault: Some(crate::state::RuntimeFault::ConfigurationInvalid),
            })),
            Err(error) => {
                let state = match error {
                    CodingToolsRuntimeError::ConnectionUnavailable
                    | CodingToolsRuntimeError::HttpStatus(_)
                    | CodingToolsRuntimeError::HealthTimeout => {
                        super::facade::CodingRuntimeHealthState::Recovering
                    }
                    _ => super::facade::CodingRuntimeHealthState::Fault,
                };
                let root_process_alive = self
                    .upstream_root_is_running()
                    .ok()
                    .flatten()
                    .unwrap_or(true);
                Ok(Some(CodingRuntimeHealth {
                    state,
                    root_process_alive,
                    authenticated_mcp: false,
                    fault: Some(error.runtime_fault()),
                }))
            }
        }
    }

    pub fn take_coding_runtime_fault(&self) -> Option<crate::state::RuntimeFault> {
        let guard = self.guard.as_ref()?;
        match guard.try_lock() {
            Ok(mut guard) => guard.take_runtime_fault(),
            Err(TryLockError::WouldBlock) => None,
            Err(TryLockError::Poisoned(error)) => error.into_inner().take_runtime_fault(),
        }
    }

    pub fn stop(mut self) -> Result<CodingToolsRuntime, PolicyEnforcementError> {
        self.signal_shutdown();
        drop(self.guard.take());
        let thread = self
            .thread
            .take()
            .ok_or(PolicyEnforcementError::ThreadTerminated)?;
        let guard = thread
            .join()
            .map_err(|_| PolicyEnforcementError::ThreadTerminated)?;
        Ok(guard.into_runtime())
    }

    fn signal_shutdown(&mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
    }
}

impl Drop for PolicyEnforcementRuntime {
    fn drop(&mut self) {
        self.signal_shutdown();
        drop(self.guard.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(listener: TcpListener, context: ServeContext) -> AgentFacade<CodingToolsRuntimeAdapter> {
    let ServeContext {
        guard,
        public_policy,
        cancellation,
        permission_mode,
        current_task,
        task_state,
        sessions,
        privileged,
        shutdown,
    } = context;
    let active_requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let local_filesystem_requests =
        Arc::new(Mutex::new(Vec::<(Value, FilesystemCancellation)>::new()));
    let privileged_requests = Arc::new(Mutex::new(Vec::<(Value, String)>::new()));
    let privileged_filesystem_requests = Arc::new(Mutex::new(Vec::<(Value, String)>::new()));
    let stopping = Arc::new(AtomicBool::new(false));
    let mut workers = Vec::<JoinHandle<()>>::new();
    let mut next_session_reap = Instant::now();
    loop {
        if shutdown.try_recv().is_ok() {
            stopping.store(true, Ordering::Release);
            break;
        }
        if Instant::now() >= next_session_reap {
            if let Ok(mut facade) = guard.try_lock() {
                if facade.reap_command_sessions().is_err() {
                    stopping.store(true, Ordering::Release);
                    break;
                }
                let running_command = facade.has_running_command_session();
                match (running_command, current_task.actual_snapshot()) {
                    (true, CurrentTaskStatus::Idle) => current_task.project(
                        CurrentTaskStatus::project(
                            TaskKind::ExecuteCommand,
                            SafeTaskSummary::Omitted,
                            TaskExecutionState::Running,
                        )
                        .expect("running command session is a valid projected task"),
                    ),
                    (
                        false,
                        CurrentTaskStatus::Active(CurrentTask {
                            kind: TaskKind::ExecuteCommand,
                            state: TaskExecutionState::Running,
                            ..
                        }),
                    ) => current_task.project(CurrentTaskStatus::Idle),
                    _ => {}
                }
            }
            next_session_reap = Instant::now() + Duration::from_millis(100);
        }
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                let worker = workers.swap_remove(index);
                let _ = worker.join();
            } else {
                index += 1;
            }
        }
        match listener.accept() {
            Ok((mut stream, _)) if workers.len() >= MAX_CONNECTION_WORKERS => {
                let _ =
                    write_mcp_http_error(&mut stream, 503, mcp_unavailable("server_busy"), None);
            }
            Ok((stream, _)) => {
                let worker_guard = Arc::clone(&guard);
                let worker_policy = Arc::clone(&public_policy);
                let worker_mode = Arc::clone(&permission_mode);
                let worker_task = current_task.clone();
                let worker_task_state = task_state.clone();
                let worker_sessions = Arc::clone(&sessions);
                let worker_active = Arc::clone(&active_requests);
                let worker_local_filesystem = Arc::clone(&local_filesystem_requests);
                let worker_privileged = privileged.as_ref().map(Arc::clone);
                let worker_privileged_requests = Arc::clone(&privileged_requests);
                let worker_privileged_filesystem_requests =
                    Arc::clone(&privileged_filesystem_requests);
                let worker_stopping = Arc::clone(&stopping);
                let worker_cancellation = cancellation.clone();
                if let Ok(worker) = thread::Builder::new()
                    .name("localbridge-mcp-policy-request".into())
                    .spawn(move || {
                        let context = ConnectionContext {
                            guard: &worker_guard,
                            public_policy: &worker_policy,
                            cancellation: &worker_cancellation,
                            permission_mode: &worker_mode,
                            current_task: &worker_task,
                            task_state: &worker_task_state,
                            sessions: &worker_sessions,
                            active_requests: &worker_active,
                            local_filesystem_requests: &worker_local_filesystem,
                            privileged: worker_privileged.as_ref(),
                            privileged_requests: &worker_privileged_requests,
                            privileged_filesystem_requests: &worker_privileged_filesystem_requests,
                            stopping: &worker_stopping,
                        };
                        let _ = handle_connection(stream, context);
                    })
                {
                    workers.push(worker);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_IDLE);
            }
            Err(_) => break,
        }
    }
    for _ in 0..3 {
        let active = active_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if active.is_empty() {
            break;
        }
        for request_id in &active {
            if cancel_local_filesystem(&local_filesystem_requests, request_id) {
                continue;
            }
            if let Some(broker_request_id) =
                privileged_request_id(&privileged_filesystem_requests, request_id)
            {
                if let Some(privileged) = privileged.as_ref() {
                    let _ = privileged.cancel_structured_filesystem(broker_request_id);
                }
                continue;
            }
            if let Some(broker_request_id) = privileged_request_id(&privileged_requests, request_id)
            {
                if let Some(privileged) = privileged.as_ref() {
                    let _ = privileged.cancel_execute(broker_request_id);
                }
            } else {
                let _ = cancellation.cancel_request(request_id);
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    if let Some(privileged) = privileged.as_ref() {
        let filesystem_requests = privileged_filesystem_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(_, broker_request_id)| broker_request_id.clone())
            .collect::<Vec<_>>();
        for broker_request_id in filesystem_requests {
            let _ = privileged.cancel_structured_filesystem(broker_request_id);
        }
        let requests = privileged_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(_, broker_request_id)| broker_request_id.clone())
            .collect::<Vec<_>>();
        for broker_request_id in requests {
            let _ = privileged.cancel_execute(broker_request_id);
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
    let guard = Arc::try_unwrap(guard)
        .unwrap_or_else(|_| panic!("policy enforcement guard still shared after worker shutdown"));
    guard
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn handle_connection(mut stream: TcpStream, context: ConnectionContext<'_>) -> Result<(), ()> {
    let ConnectionContext {
        guard,
        public_policy,
        cancellation,
        permission_mode,
        current_task,
        task_state,
        sessions,
        active_requests,
        local_filesystem_requests,
        privileged,
        privileged_requests,
        privileged_filesystem_requests,
        stopping,
    } = context;
    stream
        .set_read_timeout(Some(CONNECTION_TIMEOUT))
        .map_err(|_| ())?;
    stream
        .set_write_timeout(Some(CONNECTION_TIMEOUT))
        .map_err(|_| ())?;
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => return write_http_diagnostic_error(&mut stream, error),
    };

    if request.path != "/mcp" {
        return write_mcp_http_error(&mut stream, 404, mcp_invalid("endpoint_not_found"), None);
    }
    if request.method == "DELETE" {
        let Some(session) = request.header("mcp-session-id") else {
            return write_mcp_http_error(
                &mut stream,
                400,
                mcp_invalid("session_id_required"),
                None,
            );
        };
        if sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(session)
            .is_some()
        {
            return write_empty(&mut stream, 204, None);
        }
        return write_mcp_http_error(&mut stream, 404, mcp_unavailable("session_not_found"), None);
    }
    if request.method == "GET" {
        let Some(session) = request.header("mcp-session-id") else {
            return write_mcp_http_error(
                &mut stream,
                400,
                mcp_invalid("session_id_required"),
                None,
            );
        };
        let mode = *permission_mode
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current_signature = {
            let policy = public_policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            effective_tool_catalog_signature(&policy, mode)
        };
        let pending = {
            let mut sessions = sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(stored) = sessions.get_mut(session) else {
                return write_mcp_http_error(
                    &mut stream,
                    404,
                    mcp_unavailable("session_not_found"),
                    None,
                );
            };
            if request
                .header("mcp-protocol-version")
                .is_some_and(|version| version != stored.protocol)
            {
                return write_mcp_http_error(
                    &mut stream,
                    400,
                    mcp_invalid("protocol_version_mismatch"),
                    Some(session),
                );
            }
            if stored.tool_catalog_signature != current_signature {
                stored.tool_catalog_signature = current_signature;
                stored.tools_list_changed_pending = true;
            }
            let pending = stored.tools_list_changed_pending;
            stored.tools_list_changed_pending = false;
            pending
        };
        if pending {
            return write_sse_notification(
                &mut stream,
                &json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"}),
                session,
            );
        }
        return write_empty(&mut stream, 204, Some(session));
    }
    if request.method != "POST" {
        return write_mcp_http_error(&mut stream, 405, mcp_invalid("method_not_allowed"), None);
    }

    let payload: Value = match serde_json::from_slice(&request.body) {
        Ok(payload) => payload,
        Err(_) => return write_rpc_error(&mut stream, Value::Null, -32700, "Parse error", None),
    };
    if payload.is_array() {
        return write_rpc_error(
            &mut stream,
            Value::Null,
            -32600,
            "Batch requests are not supported",
            None,
        );
    }
    let Some(object) = payload.as_object() else {
        return write_rpc_error(&mut stream, Value::Null, -32600, "Invalid Request", None);
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return write_rpc_error(
            &mut stream,
            request_id(object),
            -32600,
            "Invalid Request",
            None,
        );
    }
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return write_rpc_error(
            &mut stream,
            request_id(object),
            -32600,
            "Invalid Request",
            None,
        );
    };
    let id = request_id(object);

    if method == "initialize" {
        if request.header("mcp-session-id").is_some() {
            return write_rpc_error(
                &mut stream,
                id,
                -32600,
                "initialize must not include Mcp-Session-Id",
                None,
            );
        }
        let protocol = object
            .get("params")
            .and_then(Value::as_object)
            .and_then(|params| params.get("protocolVersion"))
            .and_then(Value::as_str);
        let Some(protocol) = protocol.filter(|value| {
            *value == CURRENT_PROTOCOL_VERSION || *value == COMPATIBLE_PROTOCOL_VERSION
        }) else {
            return write_rpc_error(
                &mut stream,
                id,
                -32602,
                "Unsupported MCP protocol version",
                None,
            );
        };
        let mode = *permission_mode
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tool_catalog_signature = {
            let policy = public_policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            effective_tool_catalog_signature(&policy, mode)
        };
        let session = new_session_id();
        {
            let mut sessions = sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if sessions.len() >= MAX_DOWNSTREAM_MCP_SESSIONS {
                return write_mcp_http_error(
                    &mut stream,
                    503,
                    mcp_unavailable("session_capacity"),
                    None,
                );
            }
            sessions.insert(
                session.clone(),
                McpSession {
                    protocol: protocol.to_string(),
                    tool_catalog_signature,
                    tools_list_changed_pending: true,
                },
            );
        }
        return write_rpc_result(
            &mut stream,
            id,
            json!({
                "protocolVersion": protocol,
                "capabilities": {"tools": {"listChanged": true}},
                "serverInfo": {"name": "localbridge-mcp-guard", "version": format!("{}+api{}", env!("CARGO_PKG_VERSION"), AGENT_API_REVISION)}
            }),
            Some(&session),
        );
    }

    if method == "ping" && request.header("mcp-session-id").is_none() && !id.is_null() {
        return write_rpc_result(&mut stream, id, json!({}), None);
    }

    let Some(session) = request.header("mcp-session-id") else {
        return write_rpc_error(&mut stream, id, -32600, "Mcp-Session-Id is required", None);
    };
    let stored_session = sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(session)
        .cloned();
    let Some(stored_session) = stored_session else {
        return write_mcp_http_error(&mut stream, 404, mcp_unavailable("session_not_found"), None);
    };
    let mode = *permission_mode
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let current_signature = {
        let policy = public_policy
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        effective_tool_catalog_signature(&policy, mode)
    };
    if stored_session.tool_catalog_signature != current_signature {
        sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(session);
        return write_mcp_http_error(&mut stream, 404, mcp_unavailable("session_stale"), None);
    }
    if request
        .header("mcp-protocol-version")
        .is_some_and(|version| version != stored_session.protocol)
    {
        return write_rpc_error(
            &mut stream,
            id,
            -32600,
            "MCP protocol version mismatch",
            Some(session),
        );
    }

    if id.is_null() {
        if method == "notifications/cancelled" {
            if let Some(request_id) = object
                .get("params")
                .and_then(Value::as_object)
                .and_then(|params| params.get("requestId"))
                .filter(|request_id| valid_downstream_request_id(request_id))
            {
                if cancel_local_filesystem(local_filesystem_requests, request_id) {
                    return write_empty(&mut stream, 202, Some(session));
                }
                if let Some(broker_request_id) =
                    privileged_request_id(privileged_filesystem_requests, request_id)
                {
                    let Some(privileged) = privileged else {
                        return write_mcp_http_error(
                            &mut stream,
                            503,
                            mcp_unavailable("privileged_cancellation_unavailable"),
                            Some(session),
                        );
                    };
                    if privileged
                        .cancel_structured_filesystem(broker_request_id)
                        .is_err()
                    {
                        return write_mcp_http_error(
                            &mut stream,
                            503,
                            mcp_unavailable("privileged_cancellation_failed"),
                            Some(session),
                        );
                    }
                    return write_empty(&mut stream, 202, Some(session));
                }
                if let Some(broker_request_id) =
                    privileged_request_id(privileged_requests, request_id)
                {
                    let Some(privileged) = privileged else {
                        return write_mcp_http_error(
                            &mut stream,
                            503,
                            mcp_unavailable("privileged_cancellation_unavailable"),
                            Some(session),
                        );
                    };
                    if privileged.cancel_execute(broker_request_id).is_err() {
                        return write_mcp_http_error(
                            &mut stream,
                            503,
                            mcp_unavailable("privileged_cancellation_failed"),
                            Some(session),
                        );
                    }
                    return write_empty(&mut stream, 202, Some(session));
                }
                if cancellation.cancel_request(request_id).is_err() {
                    return write_mcp_http_error(
                        &mut stream,
                        503,
                        mcp_unavailable("cancellation_unavailable"),
                        Some(session),
                    );
                }
                for _ in 0..2 {
                    thread::sleep(Duration::from_millis(25));
                    if !active_request_exists(active_requests, request_id) {
                        break;
                    }
                    let _ = cancellation.cancel_request(request_id);
                }
                return write_empty(&mut stream, 202, Some(session));
            }
        }
        return write_empty(&mut stream, 202, Some(session));
    }

    match method {
        "ping" => write_rpc_result(&mut stream, id, json!({}), Some(session)),
        "tools/list" => {
            let mode = *permission_mode
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let policy = public_policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let result = effective_tool_catalog(&policy, mode);
            write_rpc_result(&mut stream, id, result, Some(session))
        }
        "tools/call" => {
            if !valid_downstream_request_id(&id) {
                return write_rpc_error(
                    &mut stream,
                    Value::Null,
                    -32600,
                    "Invalid Request",
                    Some(session),
                );
            }
            let Some(params) = object.get("params").and_then(Value::as_object) else {
                return write_rpc_error(
                    &mut stream,
                    id,
                    -32602,
                    "Invalid tools/call params",
                    Some(session),
                );
            };
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return write_rpc_error(
                    &mut stream,
                    id,
                    -32602,
                    "Invalid tools/call name",
                    Some(session),
                );
            };
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !arguments.is_object() {
                return write_rpc_error(
                    &mut stream,
                    id,
                    -32602,
                    "Invalid tools/call arguments",
                    Some(session),
                );
            }
            if stopping.load(Ordering::Acquire) {
                return write_mcp_http_error(
                    &mut stream,
                    503,
                    mcp_unavailable("server_stopping"),
                    Some(session),
                );
            }
            let mode = *permission_mode
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if name == "elevated_exec" {
                let request_key = request_diagnostic_key(&id);
                record_mcp_request_start(&request_key, session, name);
                let result = handle_elevated_exec(
                    &mut stream,
                    id,
                    session,
                    mode,
                    arguments,
                    ElevatedCallContext {
                        guard,
                        privileged,
                        current_task,
                        active_requests,
                        privileged_requests,
                        stopping,
                    },
                );
                return finalize_special_handler_request(&request_key, session, result);
            }
            if name == "filesystem" {
                if let Some(result) = handle_administrator_filesystem_if_needed(
                    &mut stream,
                    id.clone(),
                    session,
                    mode,
                    &arguments,
                    AdministratorFilesystemContext {
                        guard,
                        privileged,
                        current_task,
                        active_requests,
                        privileged_filesystem_requests,
                        stopping,
                    },
                ) {
                    return result;
                }
                return handle_workspace_filesystem(
                    &mut stream,
                    id,
                    session,
                    mode,
                    arguments,
                    WorkspaceFilesystemContext {
                        guard,
                        current_task,
                        active_requests,
                        local_filesystem_requests,
                        stopping,
                    },
                );
            }
            if name == "task_control" {
                let request_key = request_diagnostic_key(&id);
                record_mcp_request_start(&request_key, session, name);
                let result = handle_task_control(
                    &mut stream,
                    id,
                    session,
                    mode,
                    arguments,
                    TaskControlContext {
                        guard,
                        public_policy,
                        cancellation,
                        current_task,
                        task_state,
                        active_requests,
                        local_filesystem_requests,
                        privileged,
                        privileged_requests,
                        privileged_filesystem_requests,
                    },
                );
                return finalize_special_handler_request(&request_key, session, result);
            }
            active_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(id.clone());
            let mut guard = guard
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if stopping.load(Ordering::Acquire) {
                remove_active_request(active_requests, &id);
                return write_mcp_http_error(
                    &mut stream,
                    503,
                    mcp_unavailable("server_stopping"),
                    Some(session),
                );
            }
            record_mcp_request_start(&request_diagnostic_key(&id), session, name);
            let result = guard.call_tool(mode, name, arguments, Some(&id), |status| {
                current_task.project(status);
            });
            remove_active_request(active_requests, &id);
            match result {
                Ok(mut result) => {
                    if name == "workspace_context" {
                        enrich_workspace_context_privilege(
                            &mut result,
                            mode,
                            privileged,
                            current_task,
                        );
                    }
                    write_rpc_result(&mut stream, id, result, Some(session))
                }
                Err(FacadeCallError::Denied(denied)) => {
                    write_rpc_result(&mut stream, id, denied.to_mcp_result(), Some(session))
                }
            }
        }
        _ => write_rpc_error(&mut stream, id, -32601, "Method not found", Some(session)),
    }
}

fn enrich_workspace_context_privilege(
    result: &mut Value,
    mode: PermissionMode,
    privileged: Option<&Arc<dyn PrivilegedExecution>>,
    current_task: &CurrentTaskProjection,
) {
    let state = privileged
        .map(|gateway| gateway.state())
        .unwrap_or(PrivilegeState::Disabled);
    let (privilege_state, broker_state, uac_state) = match &state {
        PrivilegeState::Disabled => ("disabled", "offline", "not_requested"),
        PrivilegeState::Requested => ("requested", "offline", "not_requested"),
        PrivilegeState::AwaitingUac => ("awaiting_uac", "starting", "awaiting_user"),
        PrivilegeState::Active { .. } => ("active", "active", "authorized"),
        PrivilegeState::Faulted(_) => ("faulted", "faulted", "faulted"),
    };
    let administrator_token_available =
        mode == PermissionMode::Elevated && state.accepts_privileged_calls();
    let elevated_route_available = administrator_token_available;
    let Some(data) = result
        .pointer_mut("/structuredContent/data")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    data.insert(
        "elevated_route_available".into(),
        Value::Bool(elevated_route_available),
    );
    data.insert(
        "privilege_state".into(),
        Value::String(privilege_state.into()),
    );
    data.insert("broker_state".into(), Value::String(broker_state.into()));
    data.insert("uac_state".into(), Value::String(uac_state.into()));
    let aggregate = data
        .get("current_task")
        .cloned()
        .unwrap_or_else(|| task_control_snapshot(&current_task.actual_snapshot()));
    data.insert(
        "current_task".into(),
        merge_task_aggregate_activity(aggregate, current_task),
    );
    data.insert(
        "administrator_token_available".into(),
        Value::Bool(administrator_token_available),
    );
    data.insert("selected_route".into(), Value::String("ordinary".into()));
    if let Some(capabilities) = data.get_mut("capabilities").and_then(Value::as_object_mut) {
        let reason = if elevated_route_available {
            Value::Null
        } else if mode != PermissionMode::Elevated {
            Value::String("permission_mode_not_elevated".into())
        } else {
            Value::String("broker_not_active".into())
        };
        capabilities.insert(
            "elevated_route".into(),
            json!({"available":elevated_route_available,"reason":reason}),
        );
    }
}

fn handle_task_control(
    stream: &mut TcpStream,
    id: Value,
    session: &str,
    mode: PermissionMode,
    arguments: Value,
    context: TaskControlContext<'_>,
) -> Result<(), ()> {
    let TaskControlContext {
        guard,
        public_policy,
        cancellation,
        current_task,
        task_state,
        active_requests,
        local_filesystem_requests,
        privileged,
        privileged_requests,
        privileged_filesystem_requests,
    } = context;
    {
        let policy = public_policy
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let decision = policy.decide_public(mode, "task_control", &arguments);
        if !decision.allowed {
            current_task.project(
                CurrentTaskStatus::project(
                    TaskKind::ExecuteCommand,
                    SafeTaskSummary::Omitted,
                    TaskExecutionState::Blocked,
                )
                .expect("task_control blocked is a valid projected task"),
            );
            current_task.project(CurrentTaskStatus::Idle);
            let denied = FacadeDenied {
                reason: decision
                    .deny_reason
                    .expect("denied task_control decision contains reason"),
                capability: decision.descriptor.capability,
            };
            return write_rpc_result(stream, id, denied.to_mcp_result(), Some(session));
        }
    }

    let Some(action) = arguments.get("action").and_then(Value::as_str) else {
        return write_rpc_error(
            stream,
            id,
            -32602,
            "Invalid task_control action",
            Some(session),
        );
    };
    let before = current_task.actual_snapshot();
    let data = match action {
        "get" => match guard.try_lock() {
            Ok(guard) => guard.task_aggregate_snapshot(),
            Err(TryLockError::WouldBlock) => {
                task_control_snapshot_with_terminal(&before, task_state)
            }
            Err(TryLockError::Poisoned(error)) => error.into_inner().task_aggregate_snapshot(),
        },
        "cancel" => {
            let active = active_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let owner = task_state.current_owner();
            let cancelled = cancel_task_targets(
                &active,
                |request_id| {
                    if cancel_local_filesystem(local_filesystem_requests, request_id) {
                        Ok(())
                    } else if let Some(broker_request_id) =
                        privileged_request_id(privileged_filesystem_requests, request_id)
                    {
                        privileged
                            .ok_or(())?
                            .cancel_structured_filesystem(broker_request_id)
                            .map_err(|_| ())
                    } else if let Some(broker_request_id) =
                        privileged_request_id(privileged_requests, request_id)
                    {
                        privileged
                            .ok_or(())?
                            .cancel_execute(broker_request_id)
                            .map_err(|_| ())
                    } else {
                        cancellation.cancel_request(request_id).map_err(|_| ())
                    }
                },
                owner.as_ref().map(|owner| owner.session_id.as_str()),
                |session_id| match guard.try_lock() {
                    Ok(mut guard) => guard.cancel_public_command_session(session_id).is_ok(),
                    Err(TryLockError::WouldBlock) => false,
                    Err(TryLockError::Poisoned(error)) => error
                        .into_inner()
                        .cancel_public_command_session(session_id)
                        .is_ok(),
                },
            )?;
            let durable_cancelled = match guard.try_lock() {
                Ok(mut guard) => guard.cancel_durable_workflow().unwrap_or(false),
                Err(TryLockError::WouldBlock) => false,
                Err(TryLockError::Poisoned(error)) => error
                    .into_inner()
                    .cancel_durable_workflow()
                    .unwrap_or(false),
            };
            let cancelled = cancelled.saturating_add(u64::from(durable_cancelled));
            if cancelled > 0 {
                wait_for_task_cancel_settlement(
                    current_task,
                    task_state,
                    Duration::from_millis(1_500),
                );
            }
            let mut data = match guard.try_lock() {
                Ok(guard) => guard.task_aggregate_snapshot(),
                Err(TryLockError::WouldBlock) => {
                    task_control_snapshot_with_terminal(&current_task.actual_snapshot(), task_state)
                }
                Err(TryLockError::Poisoned(error)) => error.into_inner().task_aggregate_snapshot(),
            };
            if let Some(object) = data.as_object_mut() {
                object.insert("cancelled_requests".into(), Value::from(cancelled));
                object.insert(
                    "durable_task_cancelled".into(),
                    Value::Bool(durable_cancelled),
                );
            }
            data
        }
        _ => {
            return write_rpc_error(
                stream,
                id,
                -32602,
                "Invalid task_control action",
                Some(session),
            );
        }
    };
    let data = merge_task_aggregate_activity(data, current_task);
    write_rpc_result(
        stream,
        id,
        stable_success(data, "Task control completed"),
        Some(session),
    )
}

fn wait_for_task_cancel_settlement(
    current_task: &CurrentTaskProjection,
    task_state: &CommandTaskStateStore,
    max_wait: Duration,
) {
    let deadline = Instant::now() + max_wait;
    while (!matches!(current_task.actual_snapshot(), CurrentTaskStatus::Idle)
        || task_state.current_owner().is_some())
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(10));
    }
}

fn cancel_task_targets(
    active: &[Value],
    mut cancel_active: impl FnMut(&Value) -> Result<(), ()>,
    owner_session_id: Option<&str>,
    mut cancel_owner_session: impl FnMut(&str) -> bool,
) -> Result<u64, ()> {
    let mut cancelled = 0u64;
    for request_id in active {
        if cancel_active(request_id).is_ok() {
            cancelled = cancelled.saturating_add(1);
        }
    }
    if let Some(session_id) = owner_session_id {
        if cancel_owner_session(session_id) {
            cancelled = cancelled.saturating_add(1);
        }
    }
    Ok(cancelled)
}

fn task_control_snapshot_with_terminal(
    status: &CurrentTaskStatus,
    task_state: &CommandTaskStateStore,
) -> Value {
    let mut data = task_control_snapshot(status);
    if let Some(object) = data.as_object_mut() {
        let terminal = task_state.latest_terminal().map(terminal_command_value);
        let current_command = task_state.current_owner().map(|owner| {
            json!({
                "state":"running",
                "task_id":owner.task_id,
                "session_id":owner.session_id,
                "elapsed_ms":Value::Null
            })
        });
        object.insert("current_workflow".into(), Value::Null);
        object.insert(
            "current_command".into(),
            current_command.unwrap_or(Value::Null),
        );
        object.insert(
            "last_command".into(),
            terminal.clone().unwrap_or(Value::Null),
        );
        object.insert(
            "last_terminal_command".into(),
            terminal.unwrap_or(Value::Null),
        );
    }
    data
}

#[cfg(test)]
fn durable_task_snapshot_with_terminal(
    mut durable: Value,
    task_state: &CommandTaskStateStore,
) -> Value {
    let terminal = durable
        .get("task_id")
        .and_then(Value::as_str)
        .and_then(|task_id| task_state.latest_terminal_for_task(task_id))
        .map(terminal_command_value)
        .unwrap_or(Value::Null);
    if let Some(object) = durable.as_object_mut() {
        object.insert("last_terminal_command".into(), terminal);
    }
    durable
}

fn terminal_command_value(terminal: TerminalCommandSnapshot) -> Value {
    json!({
        "task_id": terminal.owner.task_id,
        "session_id": terminal.owner.session_id,
        "status": terminal.status.as_str(),
        "exit_code": terminal.exit_code,
        "signal": terminal.signal,
        "timed_out": terminal.timed_out,
        "cancelled": terminal.cancelled,
        "output_refs": terminal.output_refs,
        "error_code": terminal.error_code,
        "completed_at_ms": terminal.completed_at_ms
    })
}

fn current_task_activity_value(task: &CurrentTask, elapsed_ms: Option<u64>) -> Value {
    json!({
        "kind":activity_kind_name(task.kind),
        "state":task_execution_state_name(task.state),
        "summary":task.summary.as_deref(),
        "elapsed_ms":elapsed_ms,
        "step":Value::Null,
        "progress_current":Value::Null,
        "progress_total":Value::Null
    })
}

fn workflow_activity_value(workflow: &Value) -> Value {
    json!({
        "kind":"other",
        "state":workflow.get("state").cloned().unwrap_or_else(|| Value::String("waiting".into())),
        "summary":Value::Null,
        "elapsed_ms":Value::Null,
        "step":workflow.get("current_step").cloned().unwrap_or(Value::Null),
        "progress_current":workflow.get("progress_current").cloned().unwrap_or(Value::Null),
        "progress_total":workflow.get("progress_total").cloned().unwrap_or(Value::Null)
    })
}

fn command_activity_value(command: &Value) -> Value {
    json!({
        "kind":"command",
        "state":command.get("state").cloned().unwrap_or_else(|| Value::String("running".into())),
        "summary":Value::Null,
        "elapsed_ms":command.get("elapsed_ms").cloned().unwrap_or(Value::Null),
        "step":Value::Null,
        "progress_current":Value::Null,
        "progress_total":Value::Null
    })
}

fn command_last_activity_value(command: &Value) -> Option<Value> {
    let completed_at_ms = command.get("completed_at_ms")?.as_u64()?;
    Some(json!({
        "kind":"command",
        "summary":Value::Null,
        "outcome":command.get("status").cloned().unwrap_or_else(|| Value::String("lost".into())),
        "completed_at_ms":completed_at_ms
    }))
}

fn merge_task_aggregate_activity(
    mut aggregate: Value,
    current_task: &CurrentTaskProjection,
) -> Value {
    let (tool_activity, tool_last_activity) = current_task.activity_observation();
    let current_command = aggregate
        .get("current_command")
        .filter(|value| !value.is_null())
        .cloned();
    let current_workflow = aggregate
        .get("current_workflow")
        .filter(|value| !value.is_null())
        .cloned();
    let current_activity = current_command
        .as_ref()
        .map(command_activity_value)
        .or(tool_activity)
        .or_else(|| current_workflow.as_ref().map(workflow_activity_value));
    let command_last_activity = aggregate
        .get("last_command")
        .and_then(command_last_activity_value);
    let last_activity = match (command_last_activity, tool_last_activity) {
        (Some(command), Some(tool)) => {
            let command_at = command
                .get("completed_at_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let tool_at = tool
                .get("completed_at_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            Some(if command_at >= tool_at { command } else { tool })
        }
        (Some(command), None) => Some(command),
        (None, Some(tool)) => Some(tool),
        (None, None) => None,
    };
    if let Some(object) = aggregate.as_object_mut() {
        object.insert(
            "current_activity".into(),
            current_activity.clone().unwrap_or(Value::Null),
        );
        object.insert("last_activity".into(), last_activity.unwrap_or(Value::Null));
        let state = match current_activity
            .as_ref()
            .and_then(|value| value.get("state"))
            .and_then(Value::as_str)
        {
            Some("waiting") => "waiting",
            Some(_) => "active",
            None => "idle",
        };
        object.insert("state".into(), Value::String(state.into()));
    }
    aggregate
}

const fn activity_kind_name(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::ReadFile => "read",
        TaskKind::SearchCode => "search",
        TaskKind::ModifyFile => "modify",
        TaskKind::ExecuteCommand => "command",
        TaskKind::GitOperation => "git",
        TaskKind::Build => "build",
        TaskKind::Test => "test",
        TaskKind::ElevatedOperation => "admin",
        TaskKind::Other => "other",
    }
}

const fn completed_task_outcome(state: TaskExecutionState) -> &'static str {
    match state {
        TaskExecutionState::Cancelled => "cancelled",
        TaskExecutionState::Blocked | TaskExecutionState::Failed => "failed",
        TaskExecutionState::Idle
        | TaskExecutionState::Running
        | TaskExecutionState::AwaitingAuthorization => "completed",
    }
}

fn task_control_snapshot(status: &CurrentTaskStatus) -> Value {
    match status {
        CurrentTaskStatus::Idle => json!({"state":"idle"}),
        CurrentTaskStatus::Active(task) => json!({
            "state":"active",
            "execution_state": task_execution_state_name(task.state),
            "kind": task_kind_name(task.kind),
            "summary": task.summary.as_deref()
        }),
    }
}

const fn task_execution_state_name(state: TaskExecutionState) -> &'static str {
    match state {
        TaskExecutionState::Idle => "idle",
        TaskExecutionState::Running => "running",
        TaskExecutionState::AwaitingAuthorization => "awaiting_authorization",
        TaskExecutionState::Blocked => "blocked",
        TaskExecutionState::Failed => "failed",
        TaskExecutionState::Cancelled => "cancelled",
    }
}

const fn task_kind_name(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::ReadFile => "read_file",
        TaskKind::SearchCode => "search_code",
        TaskKind::ModifyFile => "modify_file",
        TaskKind::ExecuteCommand => "execute_command",
        TaskKind::GitOperation => "git_operation",
        TaskKind::Build => "build",
        TaskKind::Test => "test",
        TaskKind::ElevatedOperation => "elevated_operation",
        TaskKind::Other => "other",
    }
}

fn effective_tool_catalog(policy: &CapabilityPolicy, mode: PermissionMode) -> Value {
    let mut result = public_tools_for_policy(policy, mode);
    if policy.privileged_tool_visible(mode, "elevated_exec") {
        append_elevated_exec_tool(&mut result);
    }
    result
}

fn effective_tool_catalog_signature(policy: &CapabilityPolicy, mode: PermissionMode) -> String {
    serde_json::to_string(&json!({
        "api_revision": AGENT_API_REVISION,
        "catalog": effective_tool_catalog(policy, mode)
    }))
    .expect("LocalBridge public tool catalog signature is serializable")
}

fn elevation_required_result() -> Value {
    FacadeError::new(
        FacadeErrorCode::ElevationRequired,
        "需要有效的管理员 Broker 授权",
        true,
    )
    .to_mcp_result()
}

fn privileged_filesystem_unavailable_result() -> Value {
    FacadeError::new(
        FacadeErrorCode::PrivilegedRouteNotAvailable,
        "管理员 Broker 文件系统操作不可用",
        true,
    )
    .to_mcp_result()
}

fn filesystem_task_kind(action: FilesystemAction) -> TaskKind {
    match action {
        FilesystemAction::List
        | FilesystemAction::Stat
        | FilesystemAction::Read
        | FilesystemAction::Hash => TaskKind::ReadFile,
        FilesystemAction::Search => TaskKind::SearchCode,
        FilesystemAction::Write
        | FilesystemAction::Copy
        | FilesystemAction::Move
        | FilesystemAction::Delete => TaskKind::ModifyFile,
    }
}

fn project_filesystem_task(
    current_task: &CurrentTaskProjection,
    kind: TaskKind,
    state: TaskExecutionState,
) {
    current_task.project(
        CurrentTaskStatus::project(kind, SafeTaskSummary::Omitted, state)
            .expect("filesystem task state is valid"),
    );
}

fn finish_filesystem_task(
    current_task: &CurrentTaskProjection,
    kind: TaskKind,
    terminal: Option<TaskExecutionState>,
) {
    if let Some(state) = terminal {
        project_filesystem_task(current_task, kind, state);
    }
    current_task.project(CurrentTaskStatus::Idle);
}

fn handle_workspace_filesystem(
    stream: &mut TcpStream,
    id: Value,
    session: &str,
    mode: PermissionMode,
    arguments: Value,
    context: WorkspaceFilesystemContext<'_>,
) -> Result<(), ()> {
    let WorkspaceFilesystemContext {
        guard,
        current_task,
        active_requests,
        local_filesystem_requests,
        stopping,
    } = context;
    let request_key = request_diagnostic_key(&id);
    record_mcp_request_start(&request_key, session, "filesystem");
    let request = match parse_filesystem_request(&arguments) {
        Ok(request) => request,
        Err(error) => {
            project_filesystem_task(
                current_task,
                TaskKind::ReadFile,
                TaskExecutionState::Blocked,
            );
            current_task.project(CurrentTaskStatus::Idle);
            return finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, error.to_mcp_result(), Some(session)),
            );
        }
    };
    let kind = filesystem_task_kind(request.action);
    let workspace_authority = {
        let execution_guard = guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if stopping.load(Ordering::Acquire) {
            return write_mcp_http_error(
                stream,
                503,
                mcp_unavailable("server_stopping"),
                Some(session),
            );
        }
        if let Err(FacadeCallError::Denied(denied)) =
            execution_guard.authorize_public_request(mode, "filesystem", &arguments)
        {
            project_filesystem_task(current_task, kind, TaskExecutionState::Blocked);
            current_task.project(CurrentTaskStatus::Idle);
            return finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, denied.to_mcp_result(), Some(session)),
            );
        }
        if let Err(error) = execution_guard.validate_workspace_identity() {
            project_filesystem_task(current_task, kind, TaskExecutionState::Blocked);
            current_task.project(CurrentTaskStatus::Idle);
            return finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, error.to_mcp_result(), Some(session)),
            );
        }
        execution_guard.workspace_authority()
    };

    let cancellation = FilesystemCancellation::default();
    active_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(id.clone());
    local_filesystem_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((id.clone(), cancellation.clone()));
    project_filesystem_task(current_task, kind, TaskExecutionState::Running);

    let result =
        run_workspace_filesystem_with_authority(workspace_authority, arguments, cancellation);
    remove_local_filesystem_request(local_filesystem_requests, &id);
    remove_active_request(active_requests, &id);
    match result {
        Ok(result) => {
            finish_filesystem_task(current_task, kind, None);
            finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, result, Some(session)),
            )
        }
        Err(error) => {
            let terminal = if error.code == FacadeErrorCode::ProcessCancelled {
                TaskExecutionState::Cancelled
            } else {
                TaskExecutionState::Failed
            };
            finish_filesystem_task(current_task, kind, Some(terminal));
            finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, error.to_mcp_result(), Some(session)),
            )
        }
    }
}

fn handle_administrator_filesystem_if_needed(
    stream: &mut TcpStream,
    id: Value,
    session: &str,
    mode: PermissionMode,
    arguments: &Value,
    context: AdministratorFilesystemContext<'_>,
) -> Option<Result<(), ()>> {
    let AdministratorFilesystemContext {
        guard,
        privileged,
        current_task,
        active_requests,
        privileged_filesystem_requests,
        stopping,
    } = context;
    if mode != PermissionMode::Elevated {
        return None;
    }
    let execution_guard = guard
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let request = match parse_filesystem_request(arguments) {
        Ok(request) => request,
        Err(error) => {
            let request_key = request_diagnostic_key(&id);
            record_mcp_request_start(&request_key, session, "filesystem");
            project_filesystem_task(
                current_task,
                TaskKind::ModifyFile,
                TaskExecutionState::Blocked,
            );
            current_task.project(CurrentTaskStatus::Idle);
            return Some(finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, error.to_mcp_result(), Some(session)),
            ));
        }
    };
    let kind = filesystem_task_kind(request.action);
    if let Err(FacadeCallError::Denied(denied)) =
        execution_guard.authorize_public_request(mode, "filesystem", arguments)
    {
        let request_key = request_diagnostic_key(&id);
        record_mcp_request_start(&request_key, session, "filesystem");
        project_filesystem_task(current_task, kind, TaskExecutionState::Blocked);
        current_task.project(CurrentTaskStatus::Idle);
        return Some(finalize_special_handler_request(
            &request_key,
            session,
            write_rpc_result(stream, id, denied.to_mcp_result(), Some(session)),
        ));
    }
    if let Err(error) = execution_guard.validate_workspace_identity() {
        let request_key = request_diagnostic_key(&id);
        record_mcp_request_start(&request_key, session, "filesystem");
        project_filesystem_task(current_task, kind, TaskExecutionState::Blocked);
        current_task.project(CurrentTaskStatus::Idle);
        return Some(finalize_special_handler_request(
            &request_key,
            session,
            write_rpc_result(stream, id, error.to_mcp_result(), Some(session)),
        ));
    }
    let workspace_authority = execution_guard.workspace_authority();
    let spec = match administrator_filesystem_spec(
        execution_guard.workspace_path(),
        &workspace_authority,
        &request,
    ) {
        Ok(Some(spec)) => spec,
        Ok(None) => return None,
        Err(error) => {
            let request_key = request_diagnostic_key(&id);
            record_mcp_request_start(&request_key, session, "filesystem");
            project_filesystem_task(current_task, kind, TaskExecutionState::Blocked);
            current_task.project(CurrentTaskStatus::Idle);
            return Some(finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, error.to_mcp_result(), Some(session)),
            ));
        }
    };
    drop(execution_guard);

    let request_key = request_diagnostic_key(&id);
    record_mcp_request_start(&request_key, session, "filesystem");
    let Some(privileged) = privileged else {
        finish_filesystem_task(
            current_task,
            kind,
            Some(TaskExecutionState::AwaitingAuthorization),
        );
        return Some(finalize_special_handler_request(
            &request_key,
            session,
            write_rpc_result(stream, id, elevation_required_result(), Some(session)),
        ));
    };
    if !matches!(privileged.state(), PrivilegeState::Active { .. }) {
        finish_filesystem_task(
            current_task,
            kind,
            Some(TaskExecutionState::AwaitingAuthorization),
        );
        return Some(finalize_special_handler_request(
            &request_key,
            session,
            write_rpc_result(stream, id, elevation_required_result(), Some(session)),
        ));
    }

    let generation = PRIVILEGED_REQUEST_GENERATION.fetch_add(1, Ordering::Relaxed);
    let broker_request_id = format!("mcp-filesystem-{generation:x}");
    active_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(id.clone());
    privileged_filesystem_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((id.clone(), broker_request_id.clone()));
    project_filesystem_task(current_task, kind, TaskExecutionState::Running);

    if let Err(error) = privileged.start_structured_filesystem(broker_request_id.clone(), spec) {
        remove_active_request(active_requests, &id);
        remove_privileged_request(privileged_filesystem_requests, &id);
        return Some(match error {
            PrivilegedExecError::GateClosed(_) => {
                finish_filesystem_task(
                    current_task,
                    kind,
                    Some(TaskExecutionState::AwaitingAuthorization),
                );
                finalize_special_handler_request(
                    &request_key,
                    session,
                    write_rpc_result(stream, id, elevation_required_result(), Some(session)),
                )
            }
            PrivilegedExecError::Broker(_) => {
                finish_filesystem_task(current_task, kind, Some(TaskExecutionState::Failed));
                finalize_special_handler_request(
                    &request_key,
                    session,
                    write_rpc_result(
                        stream,
                        id,
                        privileged_filesystem_unavailable_result(),
                        Some(session),
                    ),
                )
            }
            PrivilegedExecError::Filesystem(code) => {
                let terminal = if code == AdministratorFilesystemErrorCode::Cancelled {
                    TaskExecutionState::Cancelled
                } else {
                    TaskExecutionState::Failed
                };
                finish_filesystem_task(current_task, kind, Some(terminal));
                finalize_special_handler_request(
                    &request_key,
                    session,
                    write_rpc_result(
                        stream,
                        id,
                        administrator_filesystem_error_result(code),
                        Some(session),
                    ),
                )
            }
        });
    }

    let filesystem = loop {
        if stopping.load(Ordering::Acquire) {
            let _ = privileged.cancel_structured_filesystem(broker_request_id.clone());
        }
        match privileged.poll_structured_filesystem(broker_request_id.clone()) {
            Ok(Some(result)) => break Ok(result),
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => break Err(error),
        }
    };
    remove_active_request(active_requests, &id);
    remove_privileged_request(privileged_filesystem_requests, &id);

    let filesystem = match filesystem {
        Ok(Ok(filesystem)) => filesystem,
        Ok(Err(code)) => {
            let terminal = if code == AdministratorFilesystemErrorCode::Cancelled {
                TaskExecutionState::Cancelled
            } else {
                TaskExecutionState::Failed
            };
            finish_filesystem_task(current_task, kind, Some(terminal));
            return Some(finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(
                    stream,
                    id,
                    administrator_filesystem_error_result(code),
                    Some(session),
                ),
            ));
        }
        Err(PrivilegedExecError::GateClosed(_)) => {
            finish_filesystem_task(
                current_task,
                kind,
                Some(TaskExecutionState::AwaitingAuthorization),
            );
            return Some(finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, elevation_required_result(), Some(session)),
            ));
        }
        Err(PrivilegedExecError::Broker(_)) => {
            finish_filesystem_task(current_task, kind, Some(TaskExecutionState::Failed));
            return Some(finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(
                    stream,
                    id,
                    privileged_filesystem_unavailable_result(),
                    Some(session),
                ),
            ));
        }
        Err(PrivilegedExecError::Filesystem(code)) => {
            let terminal = if code == AdministratorFilesystemErrorCode::Cancelled {
                TaskExecutionState::Cancelled
            } else {
                TaskExecutionState::Failed
            };
            finish_filesystem_task(current_task, kind, Some(terminal));
            return Some(finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(
                    stream,
                    id,
                    administrator_filesystem_error_result(code),
                    Some(session),
                ),
            ));
        }
    };
    let data = match administrator_filesystem_result_data(filesystem) {
        Ok(data) => data,
        Err(error) => {
            finish_filesystem_task(current_task, kind, Some(TaskExecutionState::Failed));
            return Some(finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, error.to_mcp_result(), Some(session)),
            ));
        }
    };
    finish_filesystem_task(current_task, kind, None);
    Some(finalize_special_handler_request(
        &request_key,
        session,
        write_rpc_result(
            stream,
            id,
            stable_success(data, "Filesystem operation completed"),
            Some(session),
        ),
    ))
}

fn administrator_filesystem_spec(
    workspace: &Path,
    authority: &PathAuthority,
    request: &FilesystemRequest,
) -> Result<Option<AdministratorFilesystemSpec>, FacadeError> {
    let mut outside = false;
    for path in request.path_inputs().into_iter().flatten() {
        if !authority
            .input_is_within_execution_root(path)
            .map_err(normalize_path_authority_error)?
        {
            outside = true;
        }
    }
    if !outside {
        return Ok(None);
    }

    let mut workspace_fields = Vec::new();
    match request.action {
        FilesystemAction::Write => {
            if validate_workspace_side_path(
                authority,
                request.path.as_deref().expect("write path parsed"),
                true,
            )? {
                workspace_fields.push(AdministratorWorkspacePathField::Path);
            }
        }
        FilesystemAction::Copy | FilesystemAction::Move => {
            if validate_workspace_side_path(
                authority,
                request.source.as_deref().expect("copy/move source parsed"),
                false,
            )? {
                workspace_fields.push(AdministratorWorkspacePathField::Source);
            }
            if validate_workspace_side_path(
                authority,
                request
                    .destination
                    .as_deref()
                    .expect("copy/move destination parsed"),
                true,
            )? {
                workspace_fields.push(AdministratorWorkspacePathField::Destination);
            }
        }
        _ => {
            if validate_workspace_side_path(
                authority,
                request.path.as_deref().expect("filesystem path parsed"),
                false,
            )? {
                workspace_fields.push(AdministratorWorkspacePathField::Path);
            }
        }
    }

    let path = request
        .path
        .as_deref()
        .map(|path| administrator_absolute_path(authority, path))
        .transpose()?;
    let source = request
        .source
        .as_deref()
        .map(|path| administrator_absolute_path(authority, path))
        .transpose()?;
    let destination = request
        .destination
        .as_deref()
        .map(|path| administrator_absolute_path(authority, path))
        .transpose()?;
    for candidate in [&path, &source, &destination].into_iter().flatten() {
        if explicit_control_plane_reference(candidate) {
            return Err(FacadeError::new(
                FacadeErrorCode::PolicyDenied,
                "LocalBridge 控制面路径禁止通过文件系统工具修改",
                false,
            ));
        }
    }

    let max_entries = u32::try_from(request.max_entries).map_err(|_| {
        FacadeError::new(FacadeErrorCode::InvalidArgument, "文件系统参数无效", false)
    })?;
    let max_results = u32::try_from(request.max_results).map_err(|_| {
        FacadeError::new(FacadeErrorCode::InvalidArgument, "文件系统参数无效", false)
    })?;
    let max_bytes = u32::try_from(request.max_bytes).map_err(|_| {
        FacadeError::new(FacadeErrorCode::InvalidArgument, "文件系统参数无效", false)
    })?;
    let workspace_identity = if workspace_fields.is_empty() {
        None
    } else {
        Some(authority.workspace_identity_token().ok_or_else(|| {
            FacadeError::new(FacadeErrorCode::Internal, "工作区对象身份不可用", false)
        })?)
    };
    let spec = AdministratorFilesystemSpec {
        action: match request.action {
            FilesystemAction::List => AdministratorFilesystemAction::List,
            FilesystemAction::Stat => AdministratorFilesystemAction::Stat,
            FilesystemAction::Read => AdministratorFilesystemAction::Read,
            FilesystemAction::Write => AdministratorFilesystemAction::Write,
            FilesystemAction::Search => AdministratorFilesystemAction::Search,
            FilesystemAction::Copy => AdministratorFilesystemAction::Copy,
            FilesystemAction::Move => AdministratorFilesystemAction::Move,
            FilesystemAction::Delete => AdministratorFilesystemAction::Delete,
            FilesystemAction::Hash => AdministratorFilesystemAction::Hash,
        },
        path,
        source,
        destination,
        workspace_root: (!workspace_fields.is_empty())
            .then(|| workspace.to_string_lossy().into_owned()),
        workspace_identity,
        workspace_fields,
        recursive: request.recursive,
        max_depth: request.max_depth,
        max_entries,
        max_results,
        offset: request.offset,
        max_bytes,
        content_base64: request.content_base64(),
        pattern: request.pattern.clone(),
        kind: request.kind.as_deref().map(|kind| match kind {
            "file" => AdministratorFilesystemKind::File,
            "directory" => AdministratorFilesystemKind::Directory,
            _ => unreachable!("filesystem parser restricts kind"),
        }),
        min_size: request.min_size,
        max_size: request.max_size,
        modified_after_ms: request.modified_after_ms,
        modified_before_ms: request.modified_before_ms,
        sort_by: match request.sort_by.as_str() {
            "path" => AdministratorFilesystemSortBy::Path,
            "size" => AdministratorFilesystemSortBy::Size,
            "modified" => AdministratorFilesystemSortBy::Modified,
            _ => unreachable!("filesystem parser restricts sort_by"),
        },
        sort_order: match request.sort_order.as_str() {
            "asc" => AdministratorFilesystemSortOrder::Asc,
            "desc" => AdministratorFilesystemSortOrder::Desc,
            _ => unreachable!("filesystem parser restricts sort_order"),
        },
        overwrite: request.overwrite,
        calculate_size: request.calculate_size,
    };
    spec.validate().map_err(|_| {
        FacadeError::new(FacadeErrorCode::InvalidArgument, "文件系统参数无效", false)
    })?;
    Ok(Some(spec))
}

fn validate_workspace_side_path(
    authority: &PathAuthority,
    path: &str,
    allow_missing_leaf: bool,
) -> Result<bool, FacadeError> {
    if !authority
        .input_is_within_execution_root(path)
        .map_err(normalize_path_authority_error)?
    {
        return Ok(false);
    }
    if allow_missing_leaf {
        authority
            .resolve_missing_leaf(path)
            .map_err(normalize_path_authority_error)?;
    } else {
        authority
            .resolve_existing(path)
            .map_err(normalize_path_authority_error)?;
    }
    Ok(true)
}

fn administrator_absolute_path(
    authority: &PathAuthority,
    path: &str,
) -> Result<String, FacadeError> {
    let absolute = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        authority
            .input_path(path)
            .map_err(normalize_path_authority_error)?
    };
    Ok(absolute.to_string_lossy().into_owned())
}

fn administrator_filesystem_result_data(
    filesystem: AdministratorFilesystemResult,
) -> Result<Value, FacadeError> {
    let mut data = serde_json::to_value(filesystem)
        .map_err(|_| FacadeError::new(FacadeErrorCode::Internal, "文件系统结果投影失败", false))?;
    let object = data.as_object_mut().ok_or_else(|| {
        FacadeError::new(FacadeErrorCode::Internal, "文件系统结果投影失败", false)
    })?;
    object.remove("result_kind");
    object.remove("action");
    Ok(data)
}

fn administrator_filesystem_error_result(code: AdministratorFilesystemErrorCode) -> Value {
    let (code, message, retryable) = match code {
        AdministratorFilesystemErrorCode::InvalidArgument
        | AdministratorFilesystemErrorCode::LimitExceeded => (
            FacadeErrorCode::InvalidArgument,
            "文件系统参数无效或超过限制",
            false,
        ),
        AdministratorFilesystemErrorCode::NotFound => {
            (FacadeErrorCode::NotFound, "文件系统对象不存在", false)
        }
        AdministratorFilesystemErrorCode::OutsideAuthority => (
            FacadeErrorCode::WorkspaceDenied,
            "文件系统路径超出授权范围",
            false,
        ),
        AdministratorFilesystemErrorCode::AlreadyExists => (
            FacadeErrorCode::FileChanged,
            "目标文件系统对象已存在",
            false,
        ),
        AdministratorFilesystemErrorCode::Cancelled => (
            FacadeErrorCode::ProcessCancelled,
            "文件系统操作已取消",
            true,
        ),
        AdministratorFilesystemErrorCode::Unsupported => (
            FacadeErrorCode::CapabilityDenied,
            "该文件系统对象类型不受支持",
            false,
        ),
        AdministratorFilesystemErrorCode::Io => {
            (FacadeErrorCode::Internal, "文件系统操作未完成", true)
        }
    };
    FacadeError::new(code, message, retryable).to_mcp_result()
}

fn append_elevated_exec_tool(result: &mut Value) {
    let Some(tools) = result.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    if tools
        .iter()
        .any(|tool| tool.get("name").and_then(Value::as_str) == Some("elevated_exec"))
    {
        return;
    }
    tools.push(json!({
        "name": "elevated_exec",
        "description": "Run a reviewed administrator operation through the active LocalBridge privileged broker.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["process", "shell", "filesystem"],
                    "description": "Privileged operation family. Omit only for the legacy direct-process form."
                },
                "program": {"type": "string", "description": "Absolute executable path for process operations."},
                "args": {"type": "array", "items": {"type": "string"}, "description": "Process arguments."},
                "shell": {"type": "string", "enum": ["auto", "powershell", "pwsh", "windows_powershell", "cmd"], "description": "Logical shell selector for shell operations."},
                "command": {"type": "string", "description": "Reviewed shell command text."},
                "workdir": {"type": ["string", "null"], "description": "Administrator-route working directory when applicable."},
                "action": {"type": "string", "enum": ["read_file", "write_file", "create_directory", "rename", "delete"], "description": "Filesystem action."},
                "path": {"type": "string", "description": "Filesystem source/target path."},
                "destination": {"type": ["string", "null"], "description": "Rename destination when applicable."},
                "content_base64": {"type": ["string", "null"], "description": "Base64 file content for write_file."},
                "recursive": {"type": "boolean", "description": "Recursive delete flag."},
                "timeout_ms": {"type": "integer", "minimum": 1, "description": "Execution timeout in milliseconds."},
                "max_output_bytes": {"type": "integer", "minimum": 1, "description": "Maximum captured process/shell output bytes."}
            },
            "additionalProperties": false
        },
        "outputSchema": elevated_exec_output_schema()
    }));
}

fn elevated_exec_output_schema() -> Value {
    json!({
        "oneOf":[
            {
                "type":"object",
                "properties":{
                    "operation":{"const":"filesystem"},
                    "result":{"type":"object","additionalProperties":true}
                },
                "required":["operation","result"],
                "additionalProperties":false
            },
            {
                "type":"object",
                "properties":{
                    "outcome":{"type":"string","enum":["completed","timed_out","cancelled"]},
                    "exit_code":{"type":["integer","null"]},
                    "error_code":{"type":["string","null"],"enum":["Timeout","Cancelled",null]},
                    "phase":{"type":["string","null"],"enum":["process",null]},
                    "cause":{"type":["string","null"]},
                    "http_status":{"type":["integer","null"],"minimum":100,"maximum":599},
                    "stdout":{"type":"string"},
                    "stderr":{"type":"string"},
                    "stdout_truncated":{"type":"boolean"},
                    "stderr_truncated":{"type":"boolean"},
                    "truncated":{"type":"boolean"},
                    "output_refs":{"type":"object","additionalProperties":{"type":"string"}}
                },
                "required":["outcome","exit_code","error_code","phase","cause","http_status","stdout","stderr","stdout_truncated","stderr_truncated","truncated","output_refs"],
                "additionalProperties":false
            },
            elevated_exec_error_output_schema()
        ]
    })
}

fn elevated_exec_error_output_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "ok":{"const":false},
            "state":{"const":"failed"},
            "summary":{"type":"string"},
            "task_id":{"type":"null"},
            "warnings":{"type":"array","items":{"type":"string"}},
            "next_step":{"type":"null"},
            "output_refs":{"type":"array","items":{"type":"string"}},
            "data":{"type":"null"},
            "error":public_error_output_schema()
        },
        "required":["ok","state","summary","task_id","warnings","next_step","output_refs","data","error"],
        "additionalProperties":false
    })
}

fn privileged_request_id(
    requests: &Mutex<Vec<(Value, String)>>,
    downstream_request_id: &Value,
) -> Option<String> {
    requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|(candidate, _)| candidate == downstream_request_id)
        .map(|(_, broker_request_id)| broker_request_id.clone())
}

fn remove_privileged_request(
    requests: &Mutex<Vec<(Value, String)>>,
    downstream_request_id: &Value,
) {
    requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|(candidate, _)| candidate != downstream_request_id);
}

fn project_elevated_task(current_task: &CurrentTaskProjection, state: TaskExecutionState) {
    current_task.project(
        CurrentTaskStatus::project(TaskKind::ElevatedOperation, SafeTaskSummary::Omitted, state)
            .expect("elevated task state is a valid active task state"),
    );
}

fn finish_elevated_task(
    current_task: &CurrentTaskProjection,
    terminal: Option<TaskExecutionState>,
) {
    if let Some(state) = terminal {
        project_elevated_task(current_task, state);
    }
    current_task.project(CurrentTaskStatus::Idle);
}

enum ElevatedExecRoute {
    Execute(ElevatedExecSpec),
    Filesystem(PrivilegedFilesystemSpec),
}

fn elevated_exec_spec(arguments: Value) -> Result<ElevatedExecRoute, ()> {
    let operation = arguments.get("operation").and_then(Value::as_str);
    match operation {
        None => {
            let spec: ElevatedExecSpec = serde_json::from_value(arguments).map_err(|_| ())?;
            spec.validate().map_err(|_| ())?;
            Ok(ElevatedExecRoute::Execute(spec))
        }
        Some("process") => {
            let mut object = arguments.as_object().cloned().ok_or(())?;
            object.remove("operation");
            let spec: ElevatedExecSpec =
                serde_json::from_value(Value::Object(object)).map_err(|_| ())?;
            spec.validate().map_err(|_| ())?;
            Ok(ElevatedExecRoute::Execute(spec))
        }
        Some("shell") => {
            let object = arguments.as_object().ok_or(())?;
            if object.len() != 6 {
                return Err(());
            }
            let shell: ShellSelector =
                serde_json::from_value(object.get("shell").cloned().ok_or(())?).map_err(|_| ())?;
            let command = object.get("command").and_then(Value::as_str).ok_or(())?;
            let workdir = object.get("workdir").and_then(Value::as_str).ok_or(())?;
            let timeout_ms = object.get("timeout_ms").and_then(Value::as_u64).ok_or(())?;
            let max_output_bytes = object
                .get("max_output_bytes")
                .and_then(Value::as_u64)
                .ok_or(())?;
            let shell_spec = ShellExecutionSpec {
                shell,
                command: command.to_string(),
                cwd: PathBuf::from(workdir),
                timeout_ms,
                max_output_bytes: usize::try_from(max_output_bytes).map_err(|_| ())?,
            };
            let direct = ShellExecutor::default()
                .broker_direct_spec(&shell_spec)
                .map_err(|_| ())?;
            let timeout_ms = u32::try_from(direct.timeout.as_millis()).map_err(|_| ())?;
            let max_output_bytes = u32::try_from(direct.max_output_bytes).map_err(|_| ())?;
            let spec = ElevatedExecSpec {
                program: direct.program.to_string_lossy().into_owned(),
                args: direct
                    .args
                    .into_iter()
                    .map(|arg| arg.into_string().map_err(|_| ()))
                    .collect::<Result<Vec<_>, _>>()?,
                workdir: Some(direct.cwd.to_string_lossy().into_owned()),
                timeout_ms,
                max_output_bytes,
            };
            spec.validate().map_err(|_| ())?;
            Ok(ElevatedExecRoute::Execute(spec))
        }
        Some("filesystem") => {
            let mut object = arguments.as_object().cloned().ok_or(())?;
            object.remove("operation");
            let spec: PrivilegedFilesystemSpec =
                serde_json::from_value(Value::Object(object)).map_err(|_| ())?;
            spec.validate().map_err(|_| ())?;
            Ok(ElevatedExecRoute::Filesystem(spec))
        }
        Some(_) => Err(()),
    }
}

fn handle_elevated_exec(
    stream: &mut TcpStream,
    id: Value,
    session: &str,
    mode: PermissionMode,
    arguments: Value,
    context: ElevatedCallContext<'_>,
) -> Result<(), ()> {
    let ElevatedCallContext {
        guard,
        privileged,
        current_task,
        active_requests,
        privileged_requests,
        stopping,
    } = context;
    let mut execution_guard = guard
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let reviewed_arguments = arguments.clone();
    let decision = execution_guard.elevated_decision(mode, &reviewed_arguments);
    if !decision.allowed || decision.descriptor.capability != Capability::ElevatedExec {
        finish_elevated_task(current_task, Some(TaskExecutionState::Blocked));
        let denied = FacadeDenied {
            reason: decision
                .deny_reason
                .unwrap_or(super::policy::DenyReason::PrivilegedRouteNotAvailable),
            capability: decision.descriptor.capability,
        };
        return write_rpc_result(stream, id, denied.to_mcp_result(), Some(session));
    }

    let Some(privileged) = privileged else {
        finish_elevated_task(
            current_task,
            Some(TaskExecutionState::AwaitingAuthorization),
        );
        return write_rpc_result(stream, id, elevation_required_result(), Some(session));
    };
    if !matches!(privileged.state(), PrivilegeState::Active { .. }) {
        finish_elevated_task(
            current_task,
            Some(TaskExecutionState::AwaitingAuthorization),
        );
        return write_rpc_result(stream, id, elevation_required_result(), Some(session));
    }
    let route = match elevated_exec_spec(arguments) {
        Ok(route) => route,
        Err(()) => {
            finish_elevated_task(current_task, Some(TaskExecutionState::Blocked));
            return write_rpc_error(
                stream,
                id,
                -32602,
                "Invalid elevated_exec arguments",
                Some(session),
            );
        }
    };

    if let ElevatedExecRoute::Filesystem(spec) = route {
        project_elevated_task(current_task, TaskExecutionState::Running);
        let filesystem = match privileged.filesystem(spec) {
            Ok(filesystem) => filesystem,
            Err(PrivilegedExecError::GateClosed(_)) => {
                finish_elevated_task(
                    current_task,
                    Some(TaskExecutionState::AwaitingAuthorization),
                );
                return write_rpc_result(stream, id, elevation_required_result(), Some(session));
            }
            Err(PrivilegedExecError::Broker(_)) => {
                finish_elevated_task(current_task, Some(TaskExecutionState::Failed));
                return write_rpc_error(
                    stream,
                    id,
                    -32603,
                    "Privileged broker filesystem operation failed",
                    Some(session),
                );
            }
            Err(PrivilegedExecError::Filesystem(_)) => {
                finish_elevated_task(current_task, Some(TaskExecutionState::Failed));
                return write_rpc_error(
                    stream,
                    id,
                    -32603,
                    "Privileged broker filesystem operation failed",
                    Some(session),
                );
            }
        };
        let response = json!({
            "content": [{"type":"text","text":"Privileged filesystem operation completed"}],
            "structuredContent": {
                "operation":"filesystem",
                "result": serde_json::to_value(filesystem).map_err(|_| ())?
            },
            "isError": false
        });
        finish_elevated_task(current_task, None);
        let result = write_rpc_result(stream, id, response, Some(session));
        drop(execution_guard);
        return result;
    }
    let ElevatedExecRoute::Execute(spec) = route else {
        unreachable!("filesystem route returned above");
    };

    let generation = PRIVILEGED_REQUEST_GENERATION.fetch_add(1, Ordering::Relaxed);
    let broker_request_id = format!("mcp-elevated-{generation:x}");
    active_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(id.clone());
    privileged_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((id.clone(), broker_request_id.clone()));
    project_elevated_task(current_task, TaskExecutionState::Running);

    if let Err(error) = privileged.start_execute(broker_request_id.clone(), spec) {
        remove_active_request(active_requests, &id);
        remove_privileged_request(privileged_requests, &id);
        return match error {
            PrivilegedExecError::GateClosed(_) => {
                finish_elevated_task(
                    current_task,
                    Some(TaskExecutionState::AwaitingAuthorization),
                );
                write_rpc_result(stream, id, elevation_required_result(), Some(session))
            }
            PrivilegedExecError::Broker(_) => {
                finish_elevated_task(current_task, Some(TaskExecutionState::Failed));
                write_rpc_error(
                    stream,
                    id,
                    -32603,
                    "Privileged broker execution failed",
                    Some(session),
                )
            }
            PrivilegedExecError::Filesystem(_) => {
                finish_elevated_task(current_task, Some(TaskExecutionState::Failed));
                write_rpc_error(
                    stream,
                    id,
                    -32603,
                    "Privileged broker execution failed",
                    Some(session),
                )
            }
        };
    }

    let execution = loop {
        if stopping.load(Ordering::Acquire) {
            let _ = privileged.cancel_execute(broker_request_id.clone());
        }
        match privileged.poll_execute(broker_request_id.clone()) {
            Ok(Some(result)) => break Ok(result),
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => break Err(error),
        }
    };
    remove_active_request(active_requests, &id);
    remove_privileged_request(privileged_requests, &id);

    let execution = match execution {
        Ok(execution) => execution,
        Err(PrivilegedExecError::GateClosed(_)) => {
            finish_elevated_task(
                current_task,
                Some(TaskExecutionState::AwaitingAuthorization),
            );
            return write_rpc_result(stream, id, elevation_required_result(), Some(session));
        }
        Err(PrivilegedExecError::Broker(_)) => {
            finish_elevated_task(current_task, Some(TaskExecutionState::Failed));
            return write_rpc_error(
                stream,
                id,
                -32603,
                "Privileged broker execution failed",
                Some(session),
            );
        }
        Err(PrivilegedExecError::Filesystem(_)) => {
            finish_elevated_task(current_task, Some(TaskExecutionState::Failed));
            return write_rpc_error(
                stream,
                id,
                -32603,
                "Privileged broker execution failed",
                Some(session),
            );
        }
    };

    let outcome = match execution.outcome {
        ElevatedExecOutcome::Completed => "completed",
        ElevatedExecOutcome::TimedOut => "timed_out",
        ElevatedExecOutcome::Cancelled => "cancelled",
    };
    let terminal = match execution.outcome {
        ElevatedExecOutcome::Completed => None,
        ElevatedExecOutcome::TimedOut => Some(TaskExecutionState::Failed),
        ElevatedExecOutcome::Cancelled => Some(TaskExecutionState::Cancelled),
    };
    let is_error = !matches!(execution.outcome, ElevatedExecOutcome::Completed);
    let diagnostic = match execution.outcome {
        ElevatedExecOutcome::Completed => None,
        ElevatedExecOutcome::TimedOut => Some(crate::diagnostics::error::from_canonical_code(
            "ProcessTimedOut",
        )),
        ElevatedExecOutcome::Cancelled => Some(crate::diagnostics::error::from_canonical_code(
            "ProcessCancelled",
        )),
    };
    const INLINE_OUTPUT_BYTES: usize = 8 * 1024;
    let (stdout, stdout_inline_truncated) = inline_output(&execution.stdout, INLINE_OUTPUT_BYTES);
    let (stderr, stderr_inline_truncated) = inline_output(&execution.stderr, INLINE_OUTPUT_BYTES);
    let mut output_refs = Map::new();
    if stdout_inline_truncated {
        output_refs.insert(
            "stdout".into(),
            Value::String(execution_guard.retain_local_output("stdout", execution.stdout.clone())),
        );
    }
    if stderr_inline_truncated {
        output_refs.insert(
            "stderr".into(),
            Value::String(execution_guard.retain_local_output("stderr", execution.stderr.clone())),
        );
    }
    let text = [stdout.as_str(), stderr.as_str()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(if stdout.is_empty() || stderr.is_empty() {
            ""
        } else {
            "\n"
        });
    let response = json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": {
            "outcome": outcome,
            "exit_code": execution.exit_code,
            "error_code": diagnostic.as_ref().map(|value| value.error_code.as_str()),
            "phase": diagnostic.as_ref().map(|value| value.phase.as_str()),
            "cause": diagnostic.as_ref().map(|value| value.cause.as_str()),
            "http_status": diagnostic.as_ref().and_then(|value| value.http_status),
            "stdout": stdout,
            "stderr": stderr,
            "stdout_truncated": execution.stdout_truncated || stdout_inline_truncated,
            "stderr_truncated": execution.stderr_truncated || stderr_inline_truncated,
            "truncated": execution.truncated || stdout_inline_truncated || stderr_inline_truncated,
            "output_refs": output_refs
        },
        "isError": is_error
    });
    finish_elevated_task(current_task, terminal);
    let result = write_rpc_result(stream, id, response, Some(session));
    drop(execution_guard);
    result
}

fn inline_output(value: &str, limit: usize) -> (String, bool) {
    if value.len() <= limit {
        return (value.to_string(), false);
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

fn request_id(object: &serde_json::Map<String, Value>) -> Value {
    object.get("id").cloned().unwrap_or(Value::Null)
}

fn valid_downstream_request_id(request_id: &Value) -> bool {
    request_id.is_string()
        || request_id
            .as_number()
            .is_some_and(|number| number.as_i64().is_some() || number.as_u64().is_some())
}

fn remove_active_request(active_requests: &Mutex<Vec<Value>>, request_id: &Value) {
    active_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|active| active != request_id);
}

fn cancel_local_filesystem(
    requests: &Mutex<Vec<(Value, FilesystemCancellation)>>,
    request_id: &Value,
) -> bool {
    let requests = requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, cancellation)) = requests.iter().find(|(active, _)| active == request_id) {
        cancellation.cancel();
        true
    } else {
        false
    }
}

fn remove_local_filesystem_request(
    requests: &Mutex<Vec<(Value, FilesystemCancellation)>>,
    request_id: &Value,
) {
    requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|(active, _)| active != request_id);
}

fn active_request_exists(active_requests: &Mutex<Vec<Value>>, request_id: &Value) -> bool {
    active_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .any(|active| active == request_id)
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HttpReadError {
    status: u16,
    cause: &'static str,
}

impl HttpReadError {
    const fn new(status: u16, cause: &'static str) -> Self {
        Self { status, cause }
    }
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, HttpReadError> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(HttpReadError::new(431, "header_too_large"));
        }
        let count = stream
            .read(&mut chunk)
            .map_err(|_| HttpReadError::new(400, "socket_read_failure"))?;
        if count == 0 {
            return Err(HttpReadError::new(400, "early_eof"));
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header_text = std::str::from_utf8(&bytes[..header_end - 4])
        .map_err(|_| HttpReadError::new(400, "malformed_request"))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or(HttpReadError::new(400, "malformed_request"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or(HttpReadError::new(400, "malformed_request"))?
        .to_string();
    let path = request_parts
        .next()
        .ok_or(HttpReadError::new(400, "malformed_request"))?
        .to_string();
    if request_parts.next() != Some("HTTP/1.1") || request_parts.next().is_some() {
        return Err(HttpReadError::new(400, "malformed_request"));
    }
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or(HttpReadError::new(400, "malformed_request"))?;
        let name = name.trim().to_string();
        let value = value.trim().to_string();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value
                .parse::<usize>()
                .map_err(|_| HttpReadError::new(400, "malformed_request"))?;
            if content_length > MAX_BODY_BYTES {
                return Err(HttpReadError::new(413, "body_too_large"));
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(HttpReadError::new(400, "unsupported_transfer_encoding"));
        }
        headers.push((name, value));
    }
    let mut body = bytes[header_end..].to_vec();
    if body.len() > content_length {
        body.truncate(content_length);
    }
    while body.len() < content_length {
        let remaining = content_length - body.len();
        let read_limit = remaining.min(chunk.len());
        let count = stream
            .read(&mut chunk[..read_limit])
            .map_err(|_| HttpReadError::new(400, "socket_read_failure"))?;
        if count == 0 {
            return Err(HttpReadError::new(400, "early_eof"));
        }
        body.extend_from_slice(&chunk[..count]);
    }
    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn new_session_id() -> String {
    let generation = SESSION_GENERATION.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("lb-{:x}-{nanos:x}-{generation:x}", std::process::id())
}

fn write_rpc_result(
    stream: &mut TcpStream,
    id: Value,
    result: Value,
    session: Option<&str>,
) -> Result<(), ()> {
    let request_key = session.map(|_| request_diagnostic_key(&id));
    let write_result = write_json(
        stream,
        200,
        &json!({"jsonrpc":"2.0","id":id,"result":result.clone()}),
        session,
    );
    match (session, request_key.as_deref()) {
        (Some(session), Some(request_key)) => {
            finalize_response_diagnostic(request_key, session, write_result, || {
                record_mcp_request_result(request_key, session, &result)
            })
        }
        _ => write_result,
    }
}

fn write_rpc_error(
    stream: &mut TcpStream,
    id: Value,
    code: i64,
    message: &str,
    session: Option<&str>,
) -> Result<(), ()> {
    let diagnostic = match code {
        -32700 => mcp_invalid("parse_error"),
        -32600 => mcp_invalid("invalid_request"),
        -32601 => mcp_invalid("method_not_found"),
        -32602 => mcp_invalid("invalid_params"),
        _ => mcp_unknown("internal_error"),
    };
    let request_key = session.map(|_| request_diagnostic_key(&id));
    let write_result = write_json(
        stream,
        200,
        &json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message,"data":diagnostic.to_value()}}),
        session,
    );
    match (session, request_key.as_deref()) {
        (Some(session), Some(request_key)) => {
            finalize_response_diagnostic(request_key, session, write_result, || {
                record_mcp_request_error(request_key, session, diagnostic)
            })
        }
        _ => write_result,
    }
}

fn finalize_response_diagnostic<F>(
    request_key: &str,
    session: &str,
    write_result: Result<(), ()>,
    delivered: F,
) -> Result<(), ()>
where
    F: FnOnce(),
{
    match write_result {
        Ok(()) => {
            delivered();
            Ok(())
        }
        Err(()) => {
            record_mcp_request_error(
                request_key,
                session,
                transport_unavailable("response_write_failure", None),
            );
            Err(())
        }
    }
}

fn request_diagnostic_key(id: &Value) -> String {
    serde_json::to_string(id).unwrap_or_else(|_| "null".to_string())
}

fn finalize_special_handler_request(
    request_key: &str,
    session: &str,
    result: Result<(), ()>,
) -> Result<(), ()> {
    if result.is_err() {
        record_mcp_request_error(request_key, session, mcp_unknown("special_handler_aborted"));
    }
    result
}

fn write_http_diagnostic_error(stream: &mut TcpStream, error: HttpReadError) -> Result<(), ()> {
    let diagnostic = transport_unavailable(error.cause, Some(error.status));
    write_mcp_http_error(stream, error.status, diagnostic, None)
}

fn write_mcp_http_error(
    stream: &mut TcpStream,
    status: u16,
    mut diagnostic: ErrorDiagnostic,
    session: Option<&str>,
) -> Result<(), ()> {
    diagnostic.http_status = Some(status);
    write_json(
        stream,
        status,
        &json!({"error":diagnostic.to_value()}),
        session,
    )
}

fn write_json(
    stream: &mut TcpStream,
    status: u16,
    value: &Value,
    session: Option<&str>,
) -> Result<(), ()> {
    let body = serde_json::to_vec(value).map_err(|_| ())?;
    write_response(stream, status, Some("application/json"), &body, session)
}

fn write_empty(stream: &mut TcpStream, status: u16, session: Option<&str>) -> Result<(), ()> {
    write_response(stream, status, None, &[], session)
}

fn write_sse_notification(
    stream: &mut TcpStream,
    notification: &Value,
    session: &str,
) -> Result<(), ()> {
    let json = serde_json::to_string(notification).map_err(|_| ())?;
    let body = format!("event: message\ndata: {json}\n\n");
    write_response(
        stream,
        200,
        Some("text/event-stream"),
        body.as_bytes(),
        Some(session),
    )
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: Option<&str>,
    body: &[u8],
    session: Option<&str>,
) -> Result<(), ()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(content_type) = content_type {
        response.push_str("Content-Type: ");
        response.push_str(content_type);
        response.push_str("\r\n");
    }
    if let Some(session) = session {
        response.push_str("Mcp-Session-Id: ");
        response.push_str(session);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes()).map_err(|_| ())?;
    stream.write_all(body).map_err(|_| ())?;
    stream.flush().map_err(|_| ())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn http_read_error(raw: &[u8]) -> HttpReadError {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client.write_all(raw).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        read_request(&mut server).unwrap_err()
    }

    #[test]
    fn schema42_mcp_http_failures_have_distinct_transport_causes() {
        assert_eq!(
            http_read_error(b"BROKEN\r\n\r\n").cause,
            "malformed_request"
        );
        assert_eq!(
            http_read_error(b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").cause,
            "unsupported_transfer_encoding"
        );
        assert_eq!(
            http_read_error(b"POST /mcp HTTP/1.1\r\nContent-Length: 5\r\n\r\nab").cause,
            "early_eof"
        );
        let oversized_body = format!(
            "POST /mcp HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY_BYTES + 1
        );
        assert_eq!(
            http_read_error(oversized_body.as_bytes()).cause,
            "body_too_large"
        );

        let mut oversized_header = b"GET /mcp HTTP/1.1\r\nX-Test: ".to_vec();
        oversized_header.extend(std::iter::repeat_n(b'a', MAX_HEADER_BYTES + 1));
        assert_eq!(http_read_error(&oversized_header).cause, "header_too_large");

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let _client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        server.set_nonblocking(true).unwrap();
        assert_eq!(
            read_request(&mut server).unwrap_err().cause,
            "socket_read_failure"
        );
    }

    #[test]
    fn schema42_http_400_response_is_unavailable_transport_not_runtime() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        write_http_diagnostic_error(
            &mut server,
            HttpReadError::new(400, "unsupported_transfer_encoding"),
        )
        .unwrap();
        drop(server);
        let response = parse_client_response(client.try_clone().unwrap());
        assert_eq!(response.status, 400);
        assert_eq!(response.body["error"]["error_code"], "Unavailable");
        assert_eq!(response.body["error"]["phase"], "transport");
        assert_eq!(
            response.body["error"]["cause"],
            "unsupported_transfer_encoding"
        );
        assert_eq!(response.body["error"]["http_status"], 400);
        let _ = client.shutdown(std::net::Shutdown::Both);
    }

    #[test]
    fn schema42_public_mcp_http_failures_are_diagnostic_while_success_stays_empty() {
        for (status, diagnostic, error_code, cause) in [
            (
                400,
                mcp_invalid("session_id_required"),
                "InvalidRequest",
                "session_id_required",
            ),
            (
                404,
                mcp_unavailable("session_not_found"),
                "Unavailable",
                "session_not_found",
            ),
            (
                503,
                mcp_unavailable("server_stopping"),
                "Unavailable",
                "server_stopping",
            ),
        ] {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            write_mcp_http_error(&mut server, status, diagnostic, None).unwrap();
            drop(server);
            let response = parse_client_response(client);
            assert_eq!(response.status, status);
            assert_eq!(response.body["error"]["error_code"], error_code);
            assert_eq!(response.body["error"]["phase"], "mcp");
            assert_eq!(response.body["error"]["cause"], cause);
            assert_eq!(response.body["error"]["http_status"], status);
        }

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        write_empty(&mut server, 204, None).unwrap();
        drop(server);
        let response = parse_client_response(client);
        assert_eq!(response.status, 204);
        assert!(response.body.is_null());
    }

    #[test]
    fn task_control_cancel_does_not_skip_owned_session_after_active_request_success() {
        let active = vec![json!("active-a"), json!("active-b")];
        let mut active_seen = Vec::new();
        let mut owner_seen = Vec::new();
        let cancelled = cancel_task_targets(
            &active,
            |request_id| {
                active_seen.push(request_id.clone());
                Ok(())
            },
            Some("lb-session-owned"),
            |session_id| {
                owner_seen.push(session_id.to_string());
                true
            },
        )
        .unwrap();
        assert_eq!(cancelled, 3);
        assert_eq!(active_seen, active);
        assert_eq!(owner_seen, vec!["lb-session-owned"]);
    }
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    #[cfg(windows)]
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    #[cfg(windows)]
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SUSPEND_RESUME};

    use super::super::runtime::{
        CodingToolsPermissionMode, CodingToolsRuntimeConfig, InternalBearer,
    };
    use super::super::task_state::{CommandOwner, CommandTerminalStatus, TerminalCommandSnapshot};

    const SYNTHETIC_BEARER: &str = "LB009_PEP_INTERNAL_BEARER_SYNTHETIC_DO_NOT_LEAK";

    #[cfg(windows)]
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtSuspendProcess(process_handle: HANDLE) -> i32;
        fn NtResumeProcess(process_handle: HANDLE) -> i32;
    }

    #[cfg(windows)]
    struct SuspendedProcess {
        handle: HANDLE,
    }

    #[cfg(windows)]
    impl SuspendedProcess {
        fn suspend(pid: u32) -> Self {
            let handle = unsafe { OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid) };
            assert!(
                !handle.is_null(),
                "OpenProcess(PROCESS_SUSPEND_RESUME) failed for {pid}"
            );
            let status = unsafe { NtSuspendProcess(handle) };
            assert_eq!(
                status, 0,
                "NtSuspendProcess failed with NTSTATUS {status:#x}"
            );
            Self { handle }
        }
    }

    #[cfg(windows)]
    impl Drop for SuspendedProcess {
        fn drop(&mut self) {
            unsafe {
                let _ = NtResumeProcess(self.handle);
                let _ = CloseHandle(self.handle);
            }
        }
    }

    #[test]
    fn durable_task_terminal_ignores_newer_unrelated_command() {
        let workspace = temp_workspace();
        let store = CommandTaskStateStore::open_at(workspace.join("owned-terminal.json")).unwrap();
        let a = CommandOwner::new("workflow-a", "lb-session-a");
        store.begin(a.clone()).unwrap();
        store
            .finalize(TerminalCommandSnapshot::new(
                a.clone(),
                CommandTerminalStatus::Completed,
                Some(0),
                None,
                false,
                false,
                vec!["lb-output-a".into()],
                None,
            ))
            .unwrap();
        let b = CommandOwner::new("direct-b", "lb-session-b");
        store.begin(b.clone()).unwrap();
        store
            .finalize(TerminalCommandSnapshot::new(
                b,
                CommandTerminalStatus::TimedOut,
                None,
                None,
                true,
                false,
                vec!["lb-output-b".into()],
                Some("ProcessTimedOut".into()),
            ))
            .unwrap();
        let data = durable_task_snapshot_with_terminal(
            json!({"task_id":"workflow-a","state":"waiting"}),
            &store,
        );
        assert_eq!(data["last_terminal_command"]["task_id"], "workflow-a");
        assert_eq!(data["last_terminal_command"]["session_id"], "lb-session-a");
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn task_control_get_reads_durable_terminal_without_private_session() {
        let workspace = temp_workspace();
        let path = workspace.join("durable-command-state.json");
        let owner = CommandOwner::new("task-durable", "lb-session-durable");
        {
            let store = CommandTaskStateStore::open_at(path.clone()).unwrap();
            store.begin(owner.clone()).unwrap();
            store
                .finalize(TerminalCommandSnapshot::new(
                    owner.clone(),
                    CommandTerminalStatus::TimedOut,
                    Some(124),
                    Some("TERM".to_string()),
                    true,
                    false,
                    vec!["lb-output-durable".to_string()],
                    Some("ProcessTimedOut".to_string()),
                ))
                .unwrap();
        }

        // Reopen from disk: there is deliberately no private runtime/session object here.
        let reopened = CommandTaskStateStore::open_at(path).unwrap();
        let data = task_control_snapshot_with_terminal(&CurrentTaskStatus::Idle, &reopened);
        let terminal = &data["last_terminal_command"];
        assert_eq!(terminal["task_id"], owner.task_id);
        assert_eq!(terminal["session_id"], owner.session_id);
        assert_eq!(terminal["status"], "timed_out");
        assert_eq!(terminal["exit_code"], 124);
        assert_eq!(terminal["timed_out"], true);
        assert_eq!(terminal["output_refs"][0], "lb-output-durable");
        assert_eq!(terminal["error_code"], "ProcessTimedOut");
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn current_task_projection_retains_fast_call_for_minimum_visibility_without_delaying_finish() {
        let projection = CurrentTaskProjection::default();
        let initial = projection.timing_snapshot();
        assert_eq!(initial.status, CurrentTaskStatus::Idle);
        assert_eq!(initial.elapsed_ms, None);
        assert_eq!(initial.last_tool, None);

        let started = Instant::now();
        projection.project(CurrentTaskStatus::start(
            TaskKind::ModifyFile,
            "write probe.txt",
        ));
        projection.project(CurrentTaskStatus::Idle);
        assert!(started.elapsed() < Duration::from_millis(100));
        let retained = projection.timing_snapshot();
        assert!(matches!(retained.status, CurrentTaskStatus::Active(_)));
        assert_eq!(retained.last_tool, None);

        thread::sleep(Duration::from_millis(540));
        let finished = projection.timing_snapshot();
        assert_eq!(finished.status, CurrentTaskStatus::Idle);
        assert_eq!(finished.elapsed_ms, None);
        assert_eq!(
            finished.last_tool.as_ref().map(|tool| tool.kind),
            Some(TaskKind::ModifyFile)
        );
    }

    #[test]
    fn schema42_task_aggregate_activity_precedence_is_command_then_tool_then_workflow() {
        let idle = CurrentTaskProjection::default();
        let workflow = json!({
            "state":"waiting",
            "current_workflow":{"state":"waiting","current_step":"edit","progress_current":null,"progress_total":null},
            "current_command":null,
            "last_command":null
        });
        let waiting = merge_task_aggregate_activity(workflow.clone(), &idle);
        assert_eq!(waiting["state"], "waiting");
        assert_eq!(waiting["current_activity"]["kind"], "other");
        assert_eq!(waiting["current_activity"]["step"], "edit");

        let tool = CurrentTaskProjection::default();
        tool.project(CurrentTaskStatus::start(
            TaskKind::SearchCode,
            "search schema42",
        ));
        let tool_active = merge_task_aggregate_activity(workflow.clone(), &tool);
        assert_eq!(tool_active["state"], "active");
        assert_eq!(tool_active["current_activity"]["kind"], "search");
        assert_eq!(
            tool_active["current_activity"]["summary"],
            "search schema42"
        );

        let command = merge_task_aggregate_activity(
            json!({
                "state":"active",
                "current_workflow":{"state":"waiting","current_step":"verify"},
                "current_command":{"state":"running","task_id":"t","session_id":"s","elapsed_ms":12},
                "last_command":null
            }),
            &tool,
        );
        assert_eq!(command["current_activity"]["kind"], "command");
        assert_eq!(command["current_activity"]["state"], "running");
        assert_eq!(command["current_activity"]["elapsed_ms"], 12);
    }

    #[test]
    fn current_task_projection_serializes_burst_fast_calls_for_full_visibility() {
        let projection = CurrentTaskProjection::default();
        projection.project(CurrentTaskStatus::start(TaskKind::ReadFile, "read a"));
        projection.project(CurrentTaskStatus::Idle);
        projection.project(CurrentTaskStatus::start(
            TaskKind::ExecuteCommand,
            "echo ok",
        ));
        projection.project(CurrentTaskStatus::Idle);

        assert!(matches!(
            projection.snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ReadFile,
                ..
            })
        ));
        thread::sleep(Duration::from_millis(540));
        assert!(matches!(
            projection.snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ExecuteCommand,
                ..
            })
        ));
        assert_eq!(
            projection
                .timing_snapshot()
                .last_tool
                .as_ref()
                .map(|tool| tool.kind),
            Some(TaskKind::ReadFile)
        );
        thread::sleep(Duration::from_millis(540));
        assert_eq!(projection.snapshot(), CurrentTaskStatus::Idle);
        assert_eq!(
            projection
                .timing_snapshot()
                .last_tool
                .as_ref()
                .map(|tool| tool.kind),
            Some(TaskKind::ExecuteCommand)
        );
    }

    #[derive(Debug)]
    struct FakePrivilegedExecution {
        state: RwLock<PrivilegeState>,
        starts: Mutex<Vec<ElevatedExecSpec>>,
        structured_filesystems: Mutex<Vec<AdministratorFilesystemSpec>>,
        structured_filesystem_results: Mutex<HashMap<String, AdministratorFilesystemResult>>,
        cancelled: AtomicBool,
        complete: AtomicBool,
    }

    impl FakePrivilegedExecution {
        fn active() -> Self {
            Self {
                state: RwLock::new(PrivilegeState::Active {
                    broker_generation: crate::state::GenerationId::new(77),
                }),
                starts: Mutex::new(Vec::new()),
                structured_filesystems: Mutex::new(Vec::new()),
                structured_filesystem_results: Mutex::new(HashMap::new()),
                cancelled: AtomicBool::new(false),
                complete: AtomicBool::new(false),
            }
        }

        fn set_state(&self, state: PrivilegeState) {
            *self
                .state
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
        }

        fn start_count(&self) -> usize {
            self.starts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len()
        }

        fn structured_filesystem_count(&self) -> usize {
            self.structured_filesystems
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len()
        }
    }

    impl PrivilegedExecution for FakePrivilegedExecution {
        fn state(&self) -> PrivilegeState {
            self.state
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        fn start_execute(
            &self,
            _request_id: String,
            spec: ElevatedExecSpec,
        ) -> Result<(), PrivilegedExecError> {
            let state = self.state();
            if !state.accepts_privileged_calls() {
                return Err(PrivilegedExecError::GateClosed(state));
            }
            self.cancelled.store(false, Ordering::Release);
            self.starts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(spec);
            Ok(())
        }

        fn poll_execute(
            &self,
            _request_id: String,
        ) -> Result<Option<crate::privilege::ElevatedExecResult>, PrivilegedExecError> {
            if self.cancelled.load(Ordering::Acquire) {
                return Ok(Some(crate::privilege::ElevatedExecResult {
                    outcome: ElevatedExecOutcome::Cancelled,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                    truncated: false,
                }));
            }
            if self.complete.load(Ordering::Acquire) {
                let large = self
                    .starts
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .last()
                    .is_some_and(|spec| spec.max_output_bytes > 8 * 1024);
                return Ok(Some(crate::privilege::ElevatedExecResult {
                    outcome: ElevatedExecOutcome::Completed,
                    exit_code: Some(0),
                    stdout: if large {
                        "O".repeat(9_000)
                    } else {
                        "LB012_FAKE_PRIVILEGED_OK".to_string()
                    },
                    stderr: if large {
                        "E".repeat(9_000)
                    } else {
                        "LB012_FAKE_PRIVILEGED_ERR".to_string()
                    },
                    stdout_truncated: false,
                    stderr_truncated: false,
                    truncated: false,
                }));
            }
            Ok(None)
        }

        fn cancel_execute(&self, _request_id: String) -> Result<(), PrivilegedExecError> {
            self.cancelled.store(true, Ordering::Release);
            Ok(())
        }

        fn filesystem(
            &self,
            spec: PrivilegedFilesystemSpec,
        ) -> Result<PrivilegedFilesystemResult, PrivilegedExecError> {
            let state = self.state();
            if !state.accepts_privileged_calls() {
                return Err(PrivilegedExecError::GateClosed(state));
            }
            Ok(PrivilegedFilesystemResult {
                action: spec.action,
                path: spec.path,
                destination: spec.destination,
                content_base64: spec.content_base64,
                bytes: 0,
            })
        }

        fn structured_filesystem(
            &self,
            spec: AdministratorFilesystemSpec,
        ) -> Result<AdministratorFilesystemResult, PrivilegedExecError> {
            let state = self.state();
            if !state.accepts_privileged_calls() {
                return Err(PrivilegedExecError::GateClosed(state));
            }
            self.structured_filesystems
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(spec.clone());
            Ok(match spec.action {
                AdministratorFilesystemAction::List | AdministratorFilesystemAction::Search => {
                    AdministratorFilesystemResult::Entries {
                        action: spec.action,
                        entries: Vec::new(),
                        scanned_entries: 0,
                        truncated: false,
                    }
                }
                AdministratorFilesystemAction::Stat => AdministratorFilesystemResult::Stat {
                    path: spec.path.unwrap(),
                    kind: "file".into(),
                    size: 0,
                    modified_ms: None,
                    calculated_size: spec.calculate_size,
                    scanned_entries: 0,
                    truncated: false,
                },
                AdministratorFilesystemAction::Read => AdministratorFilesystemResult::Read {
                    path: spec.path.unwrap(),
                    offset: spec.offset,
                    total_bytes: 15,
                    returned_bytes: 15,
                    eof: true,
                    encoding: "utf8".into(),
                    content: "LB43_FAKE_ADMIN".into(),
                },
                AdministratorFilesystemAction::Write
                | AdministratorFilesystemAction::Copy
                | AdministratorFilesystemAction::Move
                | AdministratorFilesystemAction::Delete => {
                    AdministratorFilesystemResult::Mutation {
                        action: spec.action,
                        path: spec.path.or(spec.source).unwrap(),
                        destination: spec.destination,
                        bytes: 0,
                        changed: true,
                    }
                }
                AdministratorFilesystemAction::Hash => AdministratorFilesystemResult::Hash {
                    path: spec.path.unwrap(),
                    algorithm: "sha256".into(),
                    sha256: "0".repeat(64),
                    bytes: 0,
                },
            })
        }

        fn start_structured_filesystem(
            &self,
            request_id: String,
            spec: AdministratorFilesystemSpec,
        ) -> Result<(), PrivilegedExecError> {
            let result = self.structured_filesystem(spec)?;
            self.structured_filesystem_results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(request_id, result);
            Ok(())
        }

        fn poll_structured_filesystem(
            &self,
            request_id: String,
        ) -> Result<
            Option<Result<AdministratorFilesystemResult, AdministratorFilesystemErrorCode>>,
            PrivilegedExecError,
        > {
            Ok(self
                .structured_filesystem_results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&request_id)
                .map(Ok))
        }

        fn cancel_structured_filesystem(
            &self,
            request_id: String,
        ) -> Result<(), PrivilegedExecError> {
            self.structured_filesystem_results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&request_id);
            Ok(())
        }
    }

    struct ClientResponse {
        status: u16,
        session: Option<String>,
        body: Value,
    }

    struct RawHttpResponse {
        status: u16,
        session: Option<String>,
        content_type: Option<String>,
        body: Vec<u8>,
    }

    fn assert_tool_error(response: &ClientResponse, expected_code: &str) {
        assert_eq!(response.status, 200, "{:#?}", response.body);
        assert!(response.body.get("error").is_none(), "{:#?}", response.body);
        assert_eq!(
            response.body["result"]["isError"], true,
            "{:#?}",
            response.body
        );
        assert_eq!(
            response.body["result"]["structuredContent"]["error"]["code"], expected_code,
            "{:#?}",
            response.body
        );
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri has repository parent")
            .to_path_buf()
    }

    fn temp_workspace() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "localbridge-lb009-pep-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("probe.txt"), b"LB009 PEP\n").unwrap();
        path
    }

    fn free_port() -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.local_addr().unwrap().port()
    }

    fn cleanup_test_directory(path: &Path) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match fs::remove_dir_all(path) {
                Ok(()) => return,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Other
                    ) && std::time::Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(error) => panic!("remove test workspace {}: {error}", path.display()),
            }
        }
    }

    fn policy(root: &Path) -> CapabilityPolicy {
        CapabilityPolicy::load(&root.join("runtime-policy.toml")).unwrap()
    }

    fn post(port: u16, session: Option<&str>, payload: &Value) -> ClientResponse {
        post_with_read_timeout(port, session, payload, Duration::from_secs(3))
    }

    fn post_with_read_timeout(
        port: u16,
        session: Option<&str>,
        payload: &Value,
        read_timeout: Duration,
    ) -> ClientResponse {
        let body = serde_json::to_vec(payload).unwrap();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream.set_read_timeout(Some(read_timeout)).unwrap();
        let mut request = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: {CURRENT_PROTOCOL_VERSION}\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        if let Some(session) = session {
            request.push_str("Mcp-Session-Id: ");
            request.push_str(session);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
        stream.flush().unwrap();
        parse_client_response(stream)
    }

    fn delete(port: u16, session: &str) -> u16 {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let request = format!(
            "DELETE /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nMcp-Session-Id: {session}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        parse_client_response(stream).status
    }

    fn get_sse(port: u16, session: &str) -> RawHttpResponse {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let request = format!(
            "GET /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: text/event-stream\r\nMCP-Protocol-Version: {CURRENT_PROTOCOL_VERSION}\r\nMcp-Session-Id: {session}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        parse_raw_http_response(stream)
    }

    fn parse_raw_http_response(mut stream: TcpStream) -> RawHttpResponse {
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        let split = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let headers = std::str::from_utf8(&bytes[..split]).unwrap();
        let mut lines = headers.split("\r\n");
        let status = lines
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u16>()
            .unwrap();
        let mut session = None;
        let mut content_type = None;
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("Mcp-Session-Id") {
                    session = Some(value.trim().to_string());
                } else if name.eq_ignore_ascii_case("Content-Type") {
                    content_type = Some(value.trim().to_string());
                }
            }
        }
        RawHttpResponse {
            status,
            session,
            content_type,
            body: bytes[(split + 4)..].to_vec(),
        }
    }

    fn parse_client_response(mut stream: TcpStream) -> ClientResponse {
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        let split = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let headers = std::str::from_utf8(&bytes[..split]).unwrap();
        let mut lines = headers.split("\r\n");
        let status = lines
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u16>()
            .unwrap();
        let session = lines.find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("Mcp-Session-Id")
                    .then(|| value.trim().to_string())
            })
        });
        let body_bytes = &bytes[(split + 4)..];
        let body = if body_bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(body_bytes).unwrap()
        };
        ClientResponse {
            status,
            session,
            body,
        }
    }

    fn initialize(port: u16, id: u64) -> ClientResponse {
        post(
            port,
            None,
            &json!({
                "jsonrpc":"2.0",
                "id":id,
                "method":"initialize",
                "params":{
                    "protocolVersion":CURRENT_PROTOCOL_VERSION,
                    "capabilities":{},
                    "clientInfo":{"name":"lb009-pep-test","version":"1"}
                }
            }),
        )
    }

    fn public_tool_call(
        port: u16,
        session: &str,
        id: u64,
        name: &str,
        arguments: Value,
    ) -> ClientResponse {
        post(
            port,
            Some(session),
            &json!({
                "jsonrpc":"2.0",
                "id":id,
                "method":"tools/call",
                "params":{"name":name,"arguments":arguments}
            }),
        )
    }

    #[test]
    fn schema42_special_handler_abort_closes_request_diagnostics() {
        crate::diagnostics::reset_request_diagnostics_for_test();
        record_mcp_request_start("special-abort", "session-special", "task_control");
        assert_eq!(crate::diagnostics::active_request_diagnostics_for_test(), 1);
        assert!(
            finalize_special_handler_request("special-abort", "session-special", Err(())).is_err()
        );
        assert_eq!(crate::diagnostics::active_request_diagnostics_for_test(), 0);
        let events = crate::diagnostics::request_diagnostics_for_test();
        let end = events
            .iter()
            .find(|event| event.kind == crate::diagnostics::RequestDiagnosticKind::End)
            .expect("special handler abort terminal diagnostic");
        assert_eq!(end.outcome.as_deref(), Some("failed"));
        assert_eq!(end.error_code.as_deref(), Some("Unknown"));
        assert_eq!(end.phase.as_deref(), Some("mcp"));
        assert_eq!(end.cause.as_deref(), Some("special_handler_aborted"));
    }

    #[test]
    fn schema43_response_diagnostics_finalize_after_transport_delivery() {
        use std::sync::atomic::{AtomicBool, Ordering};

        crate::diagnostics::reset_request_diagnostics_for_test();
        record_mcp_request_start("response-ok", "session-response", "filesystem");
        let delivered = AtomicBool::new(false);
        let result = stable_success(json!({"changed":true}), "done");
        assert!(
            finalize_response_diagnostic("response-ok", "session-response", Ok(()), || {
                delivered.store(true, Ordering::Release);
                record_mcp_request_result("response-ok", "session-response", &result);
            })
            .is_ok()
        );
        assert!(delivered.load(Ordering::Acquire));
        assert_eq!(crate::diagnostics::active_request_diagnostics_for_test(), 0);
        let events = crate::diagnostics::request_diagnostics_for_test();
        let success = events
            .iter()
            .find(|event| event.kind == crate::diagnostics::RequestDiagnosticKind::End)
            .expect("delivered response terminal diagnostic");
        assert_eq!(success.outcome.as_deref(), Some("success"));

        crate::diagnostics::reset_request_diagnostics_for_test();
        record_mcp_request_start("response-fail", "session-response", "filesystem");
        let delivered = AtomicBool::new(false);
        assert!(
            finalize_response_diagnostic("response-fail", "session-response", Err(()), || {
                delivered.store(true, Ordering::Release);
                record_mcp_request_result("response-fail", "session-response", &result);
            })
            .is_err()
        );
        assert!(!delivered.load(Ordering::Acquire));
        assert_eq!(crate::diagnostics::active_request_diagnostics_for_test(), 0);
        let events = crate::diagnostics::request_diagnostics_for_test();
        let failed = events
            .iter()
            .find(|event| event.kind == crate::diagnostics::RequestDiagnosticKind::End)
            .expect("failed response transport diagnostic");
        assert_eq!(failed.outcome.as_deref(), Some("failed"));
        assert_eq!(failed.phase.as_deref(), Some("transport"));
        assert_eq!(failed.cause.as_deref(), Some("response_write_failure"));
    }

    #[test]
    fn downstream_mcp_sessions_coexist_and_same_catalog_policy_replacement_preserves_them() {
        let root = repo_root();
        let workspace = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("multi-session PEP ready");

        let session_a = initialize(pep.port(), 640)
            .session
            .expect("downstream MCP session A");
        let session_b = initialize(pep.port(), 641)
            .session
            .expect("downstream MCP session B");
        assert_ne!(session_a, session_b);

        for (id, session) in [(642, &session_a), (643, &session_b)] {
            assert_eq!(
                post(
                    pep.port(),
                    Some(session),
                    &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
                )
                .status,
                202
            );
            let response = public_tool_call(
                pep.port(),
                session,
                id,
                "workspace_context",
                json!({"detail":"compact"}),
            );
            assert_eq!(response.status, 200, "session {session} must remain live");
            assert_eq!(response.body["result"]["isError"], false);
        }

        pep.replace_policy(policy(&root))
            .expect("same-catalog policy replacement");
        for (id, session) in [(644, &session_a), (645, &session_b)] {
            let response = public_tool_call(
                pep.port(),
                session,
                id,
                "workspace_context",
                json!({"detail":"compact"}),
            );
            assert_eq!(
                response.status, 200,
                "same catalog replacement must not invalidate session {session}"
            );
            assert_eq!(response.body["result"]["isError"], false);
        }

        let _coding = pep.stop().unwrap();
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn full_cmd_rmdir_workspace_cleanup_executes_through_actual_bundled_runtime() {
        let root = repo_root();
        let workspace = temp_workspace();
        let probe = workspace.join("test").join("document_workflow_probe");
        fs::create_dir_all(probe.join("nested")).unwrap();
        fs::write(probe.join("nested").join("probe.txt"), b"cleanup").unwrap();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("rmdir PEP ready");
        let session = initialize(pep.port(), 650)
            .session
            .expect("downstream MCP session");
        assert_eq!(
            post(
                pep.port(),
                Some(&session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );

        let response = public_tool_call(
            pep.port(),
            &session,
            651,
            "exec_command",
            json!({
                "shell":"cmd",
                "command":r"rmdir /s /q test\document_workflow_probe",
                "workdir":".",
                "yield_time_ms":10_000,
                "timeout_ms":30_000
            }),
        );
        assert_eq!(response.status, 200, "{:#?}", response.body);
        assert_eq!(
            response.body["result"]["isError"], false,
            "{:#?}",
            response.body
        );
        assert_eq!(
            response.body["result"]["structuredContent"]["data"]["status"], "completed",
            "{:#?}",
            response.body
        );
        assert!(
            !probe.exists(),
            "rmdir must remove the workspace probe tree"
        );

        let outside = workspace.with_extension("outside-rmdir-probe");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), b"keep").unwrap();
        let outside_response = public_tool_call(
            pep.port(),
            &session,
            652,
            "exec_command",
            json!({
                "shell":"cmd",
                "command":format!(r#"rmdir /s /q "{}""#, outside.display()),
                "workdir":".",
                "yield_time_ms":10_000,
                "timeout_ms":30_000
            }),
        );
        assert_eq!(outside_response.status, 200, "{:#?}", outside_response.body);
        assert_eq!(outside_response.body["result"]["isError"], true);
        assert_eq!(
            outside_response.body["result"]["structuredContent"]["error"]["code"],
            "WorkspaceDenied",
            "{:#?}",
            outside_response.body
        );
        assert!(outside.join("keep.txt").is_file());
        fs::remove_dir_all(&outside).unwrap();

        let _coding = pep.stop().unwrap();
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn schema27_public_facade_runtime_semantics_are_real_end_to_end() {
        let root = repo_root();
        let workspace = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("schema27 PEP ready");
        let initialized = initialize(pep.port(), 600);
        let session = initialized.session.expect("downstream MCP session");
        assert_eq!(
            post(
                pep.port(),
                Some(&session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );

        crate::diagnostics::reset_request_diagnostics_for_test();
        let context = public_tool_call(pep.port(), &session, 601, "workspace_context", json!({}));
        let request_events = crate::diagnostics::request_diagnostics_for_test();
        let request_start = request_events
            .iter()
            .find(|event| {
                event.kind == crate::diagnostics::RequestDiagnosticKind::Start
                    && event.tool == "workspace_context"
            })
            .expect("real tools/call request start diagnostic");
        let request_end = request_events
            .iter()
            .find(|event| {
                event.kind == crate::diagnostics::RequestDiagnosticKind::End
                    && event.tool == "workspace_context"
            })
            .expect("real tools/call request end diagnostic");
        assert_eq!(request_start.request_id, request_end.request_id);
        assert_eq!(request_start.connection_id, session);
        assert_eq!(request_end.connection_id, session);
        assert_eq!(request_start.attempt, 1);
        assert_eq!(request_end.attempt, 1);
        assert_eq!(request_end.outcome.as_deref(), Some("success"));
        let projected_workspace = context.body["result"]["structuredContent"]["data"]["workspace"]
            .as_str()
            .expect("workspace projection");
        assert!(!projected_workspace.is_empty());
        assert!(!projected_workspace.starts_with(r"\\?\"));
        assert_eq!(
            PathBuf::from(projected_workspace).canonicalize().unwrap(),
            workspace.canonicalize().unwrap()
        );
        assert_eq!(
            context.body["result"]["structuredContent"]["data"]["default_cwd"],
            "."
        );

        crate::diagnostics::reset_request_diagnostics_for_test();
        let invalid_task_control =
            public_tool_call(pep.port(), &session, 6999, "task_control", json!({}));
        assert_ne!(invalid_task_control.body, Value::Null);
        assert_eq!(crate::diagnostics::active_request_diagnostics_for_test(), 0);
        let task_events = crate::diagnostics::request_diagnostics_for_test();
        let task_start = task_events
            .iter()
            .find(|event| {
                event.kind == crate::diagnostics::RequestDiagnosticKind::Start
                    && event.tool == "task_control"
            })
            .expect("task_control missing-action start diagnostic");
        let task_end = task_events
            .iter()
            .find(|event| {
                event.kind == crate::diagnostics::RequestDiagnosticKind::End
                    && event.tool == "task_control"
            })
            .expect("task_control missing-action end diagnostic");
        assert_eq!(task_start.request_id, task_end.request_id);
        assert!(task_end.outcome.is_some());

        let absolute = public_tool_call(
            pep.port(),
            &session,
            602,
            "document_workflow",
            json!({"action":"inspect","path":workspace.join("probe.txt").to_string_lossy()}),
        );
        assert_eq!(
            absolute.body["result"]["isError"], false,
            "absolute in-workspace document input must match its relative form: {:#?}",
            absolute.body
        );

        let nonzero = public_tool_call(
            pep.port(),
            &session,
            603,
            "exec_command",
            json!({"command":"exit /b 7","shell":"cmd","yield_time_ms":10000}),
        );
        assert_eq!(nonzero.body["result"]["isError"], true);
        assert_eq!(
            nonzero.body["result"]["structuredContent"]["error"]["code"],
            "ProcessFailed"
        );
        assert_eq!(
            nonzero.body["result"]["structuredContent"]["data"]["exit_code"],
            7
        );

        let running = public_tool_call(
            pep.port(),
            &session,
            604,
            "exec_command",
            json!({
                "command":"Start-Sleep -Milliseconds 900; Write-Output LB_SCHEMA27_DONE",
                "shell":"windows_powershell",
                "yield_time_ms":0
            }),
        );
        assert_eq!(
            running.body["result"]["structuredContent"]["data"]["status"],
            "running"
        );
        let public_session = running.body["result"]["structuredContent"]["data"]["session_id"]
            .as_str()
            .expect("public session id")
            .to_string();
        assert!(public_session.starts_with("lb-session-"));
        assert!(
            !serde_json::to_string(&running.body)
                .unwrap()
                .contains("session:lb-session-")
        );

        let polled = public_tool_call(
            pep.port(),
            &session,
            605,
            "command_control",
            json!({"action":"poll","session_id":public_session,"wait_ms":25}),
        );
        assert!(polled.body.get("error").is_none(), "{:#?}", polled.body);

        let terminal_deadline = Instant::now() + Duration::from_secs(6);
        let mut terminal_poll_id = 606u64;
        let mut observed_output = String::new();
        let terminal = loop {
            let poll = public_tool_call(
                pep.port(),
                &session,
                terminal_poll_id,
                "command_control",
                json!({"action":"poll","session_id":public_session,"wait_ms":100}),
            );
            terminal_poll_id += 1;
            assert!(poll.body.get("error").is_none(), "{:#?}", poll.body);
            observed_output.push_str(
                poll.body["result"]["structuredContent"]["data"]["output"]
                    .as_str()
                    .unwrap_or_default(),
            );
            if poll.body["result"]["structuredContent"]["data"]["status"] != "running" {
                break poll;
            }
            assert!(
                Instant::now() < terminal_deadline,
                "schema27 command failed to converge to terminal state: {:#?}",
                poll.body
            );
            thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(
            terminal.body["result"]["structuredContent"]["data"]["status"], "completed",
            "{:#?}",
            terminal.body
        );
        assert!(
            observed_output.contains("LB_SCHEMA27_DONE"),
            "schema27 incremental output missing: {observed_output:?}"
        );

        if let Some(output_ref) =
            terminal.body["result"]["structuredContent"]["data"]["output_refs"]["stdout"].as_str()
        {
            assert!(output_ref.starts_with("lb-output-"));
            let read = public_tool_call(
                pep.port(),
                &session,
                607,
                "command_control",
                json!({"action":"read","output_ref":output_ref,"stream":"stdout","offset":0,"limit":4096}),
            );
            assert!(
                read.body["result"]["structuredContent"]["data"]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("LB_SCHEMA27_DONE")
            );
        }

        let task = public_tool_call(
            pep.port(),
            &session,
            608,
            "task_control",
            json!({"action":"get"}),
        );
        assert_eq!(task.body["result"]["structuredContent"]["ok"], true);
        for field in [
            "ok",
            "state",
            "summary",
            "task_id",
            "warnings",
            "next_step",
            "output_refs",
            "data",
            "error",
        ] {
            assert!(
                task.body["result"]["structuredContent"]
                    .get(field)
                    .is_some(),
                "task_control real MCP response lost schema41 envelope field {field}: {:#?}",
                task.body
            );
        }
        assert!(matches!(
            task.body["result"]["structuredContent"]["data"]["state"].as_str(),
            Some("idle") | Some("active")
        ));

        let create = public_tool_call(
            pep.port(),
            &session,
            609,
            "document_workflow",
            json!({"action":"create","path":"schema27.txt","content":"alpha\nbeta\n"}),
        );
        assert_eq!(
            create.body["result"]["structuredContent"]["ok"], true,
            "{:#?}",
            create.body
        );
        let inspect = public_tool_call(
            pep.port(),
            &session,
            610,
            "document_workflow",
            json!({"action":"inspect","path":"schema27.txt"}),
        );
        assert_eq!(
            inspect.body["result"]["structuredContent"]["data"]["text"],
            "alpha\nbeta\n"
        );
        let rebuild = public_tool_call(
            pep.port(),
            &session,
            611,
            "document_workflow",
            json!({"action":"rebuild","path":"schema27.txt","content":"gamma\ndelta\n"}),
        );
        assert_eq!(
            rebuild.body["result"]["structuredContent"]["ok"], true,
            "{:#?}",
            rebuild.body
        );
        let rebuilt = public_tool_call(
            pep.port(),
            &session,
            612,
            "document_workflow",
            json!({"action":"inspect","path":"schema27.txt"}),
        );
        assert_eq!(
            rebuilt.body["result"]["structuredContent"]["data"]["text"],
            "gamma\ndelta\n"
        );
        let convert = public_tool_call(
            pep.port(),
            &session,
            613,
            "document_workflow",
            json!({"action":"convert","source":"schema27.txt","path":"schema27-copy.txt"}),
        );
        assert_eq!(
            convert.body["result"]["structuredContent"]["ok"], true,
            "{:#?}",
            convert.body
        );
        let converted = public_tool_call(
            pep.port(),
            &session,
            614,
            "document_workflow",
            json!({"action":"inspect","path":"schema27-copy.txt"}),
        );
        assert_eq!(
            converted.body["result"]["structuredContent"]["data"]["text"],
            "gamma\ndelta\n"
        );

        let diagnose = public_tool_call(
            pep.port(),
            &session,
            615,
            "agent_workflow",
            json!({"action":"diagnose","objective":"schema27 context"}),
        );
        assert_eq!(
            diagnose.body["result"]["structuredContent"]["data"]["state"], "context_ready",
            "{:#?}",
            diagnose.body
        );
        let diagnose_command = public_tool_call(
            pep.port(),
            &session,
            616,
            "agent_workflow",
            json!({
                "action":"diagnose",
                "objective":"schema27 diagnose command",
                "commands":[{"command":"echo LB_SCHEMA27_DIAGNOSE","shell":"cmd","workdir":".","yield_time_ms":10000}]
            }),
        );
        assert_eq!(
            diagnose_command.body["result"]["isError"], false,
            "{:#?}",
            diagnose_command.body
        );
        assert_eq!(
            diagnose_command.body["result"]["structuredContent"]["data"]["state"],
            "completed"
        );
        assert!(
            diagnose_command.body["result"]["structuredContent"]["data"]["commands"][0]["output"]
                .as_str()
                .unwrap_or_default()
                .contains("LB_SCHEMA27_DIAGNOSE")
        );
        let executable_workflow = public_tool_call(
            pep.port(),
            &session,
            617,
            "agent_workflow",
            json!({
                "action":"bugfix",
                "objective":"schema27 executable orchestration",
                "commands":[{"command":"echo LB_SCHEMA27_AGENT","shell":"cmd","workdir":".","yield_time_ms":10000}]
            }),
        );
        assert_eq!(
            executable_workflow.body["result"]["structuredContent"]["data"]["state"], "completed",
            "{:#?}",
            executable_workflow.body
        );
        assert!(
            executable_workflow.body["result"]["structuredContent"]["data"]["commands"][0]["output"]
                .as_str()
                .unwrap_or_default()
                .contains("LB_SCHEMA27_AGENT")
        );

        let mut coding = pep.stop().expect("schema27 PEP stops");
        coding.stop().expect("schema27 Coding Tools runtime stops");
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn schema28_public_runtime_behavior_is_real_end_to_end() {
        use base64::Engine as _;

        let root = repo_root();
        let workspace = temp_workspace();
        let nested_project = workspace.join("NestedProject");
        fs::create_dir_all(nested_project.join("src")).unwrap();
        let git_init = std::process::Command::new("git")
            .arg("init")
            .arg("--quiet")
            .current_dir(&nested_project)
            .status()
            .expect("git is available for nested-project schema30 fixture");
        assert!(git_init.success());
        fs::write(
            workspace.join("range.txt"),
            b"line1\nline2\nline3\nline4\nline5\nline6\n",
        )
        .unwrap();
        image::RgbaImage::from_pixel(1024, 1024, image::Rgba([19, 37, 53, 255]))
            .save(workspace.join("large.png"))
            .unwrap();
        let module_root = workspace.join("modules");
        let module_dir = module_root.join("Invoke-LbGen14Auto");
        fs::create_dir_all(&module_dir).unwrap();
        fs::write(
            module_dir.join("Invoke-LbGen14Auto.psm1"),
            b"function Invoke-LbGen14Auto { Write-Output 'LB_GEN14_MODULE_AUTOLOAD_SENTINEL' }; Export-ModuleMember -Function Invoke-LbGen14Auto\n",
        )
        .unwrap();

        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("schema28 bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("schema28 PEP ready after live private-result semantic probe");
        let initialized = initialize(pep.port(), 700);
        assert_eq!(
            initialized.body["result"]["capabilities"]["tools"]["listChanged"], true,
            "schema39 must explicitly advertise tool-schema change capability: {:#?}",
            initialized.body
        );
        assert!(
            initialized.body["result"]["serverInfo"]["version"]
                .as_str()
                .is_some_and(|value| value.ends_with(&format!("+api{AGENT_API_REVISION}"))),
            "serverInfo version must track the current LocalBridge API revision and invalidate stale downstream metadata: {:#?}",
            initialized.body
        );
        let mut session = initialized.session.expect("schema28 downstream session");
        let first_refresh = get_sse(pep.port(), &session);
        assert_eq!(
            first_refresh.status, 200,
            "schema39 first GET did not deliver refresh"
        );
        assert_eq!(first_refresh.session.as_deref(), Some(session.as_str()));
        assert_eq!(
            first_refresh.content_type.as_deref(),
            Some("text/event-stream")
        );
        let first_refresh_body = String::from_utf8(first_refresh.body).unwrap();
        assert!(
            first_refresh_body.contains("event: message")
                && first_refresh_body.contains("notifications/tools/list_changed"),
            "schema39 refresh event missing: {first_refresh_body}"
        );
        let second_refresh = get_sse(pep.port(), &session);
        assert_eq!(
            second_refresh.status, 204,
            "schema39 refresh was not one-shot"
        );
        assert!(second_refresh.body.is_empty());
        assert_eq!(
            post(
                pep.port(),
                Some(&session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );
        let post_refresh_tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":686,"method":"tools/list","params":{}}),
        );
        assert_eq!(post_refresh_tools.status, 200);
        assert!(post_refresh_tools.body["result"]["tools"].is_array());

        let provenance =
            public_tool_call(pep.port(), &session, 687, "workspace_context", json!({}));
        assert_eq!(
            provenance.body["result"]["structuredContent"]["data"]["facade_revision"],
            AGENT_API_REVISION,
            "fresh serving instance did not identify the current LocalBridge facade revision: {:#?}",
            provenance.body
        );
        let first_turn = &provenance.body["result"]["structuredContent"]["data"];
        for field in [
            "project_name",
            "project_type",
            "project_version",
            "git_branch",
            "git_dirty",
            "git_changed_count",
            "package_manager",
            "build_system",
            "test_system",
            "runtime_availability",
            "trusted_shells",
            "current_task",
        ] {
            assert!(
                first_turn.get(field).is_some(),
                "schema39 first-turn context lost {field}: {first_turn:#?}"
            );
        }
        let served_tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":688,"method":"tools/list","params":{}}),
        );
        let served_agent = served_tools.body["result"]["tools"]
            .as_array()
            .and_then(|tools| tools.iter().find(|tool| tool["name"] == "agent_workflow"))
            .expect("fresh serving instance exposes agent_workflow");
        assert!(served_agent["inputSchema"]["properties"]["path"].is_object());
        assert!(
            served_agent["description"]
                .as_str()
                .is_some_and(|value| value.contains("resume accepts only action"))
        );
        assert_eq!(served_agent["outputSchema"]["type"], "object");
        assert_eq!(
            served_agent["outputSchema"]["properties"]["ok"]["type"],
            "boolean"
        );
        let agent_data_schema = served_agent["outputSchema"]["properties"]["data"]["anyOf"]
            .as_array()
            .and_then(|branches| branches.iter().find(|branch| branch["type"] == "object"))
            .expect("agent_workflow nullable data keeps an object domain branch");
        assert_eq!(agent_data_schema["properties"]["state"]["type"], "string");
        assert!(
            served_agent["outputSchema"]["properties"]["error"]["anyOf"]
                .as_array()
                .is_some_and(|branches| branches.iter().any(|branch| branch["type"] == "object")),
            "agent_workflow nullable error must retain a typed object branch"
        );
        let served_command_control = served_tools.body["result"]["tools"]
            .as_array()
            .and_then(|tools| tools.iter().find(|tool| tool["name"] == "command_control"))
            .expect("fresh serving instance exposes command_control");
        assert_eq!(served_command_control["inputSchema"]["type"], "object");
        assert!(served_command_control["inputSchema"].get("oneOf").is_none());
        assert_eq!(
            served_command_control["inputSchema"]["properties"]["action"]["enum"],
            json!(["poll", "read", "write", "kill"])
        );
        for property in [
            "session_id",
            "output_ref",
            "chars",
            "signal",
            "wait_ms",
            "stream",
            "offset",
            "limit",
        ] {
            assert!(
                served_command_control["inputSchema"]["properties"][property].is_object(),
                "served command_control schema lost {property}"
            );
        }

        let served_document = served_tools.body["result"]["tools"]
            .as_array()
            .and_then(|tools| {
                tools
                    .iter()
                    .find(|tool| tool["name"] == "document_workflow")
            })
            .expect("fresh serving instance exposes document_workflow");
        assert!(
            served_document["description"]
                .as_str()
                .is_some_and(|value| value.contains("rebuild requires an existing path+content"))
        );
        assert!(
            served_document["inputSchema"]["properties"]["path"]["description"]
                .as_str()
                .is_some_and(|value| value.contains("already exist"))
        );
        assert!(
            served_document["inputSchema"]["properties"]["content"]["description"]
                .as_str()
                .is_some_and(|value| value.contains("rebuild"))
        );
        let served_git = served_tools.body["result"]["tools"]
            .as_array()
            .and_then(|tools| tools.iter().find(|tool| tool["name"] == "git_workflow"))
            .expect("fresh serving instance exposes git_workflow");
        assert!(
            served_git["inputSchema"]["properties"]["path"]["description"]
                .as_str()
                .is_some_and(|value| value.contains("Required for blame"))
        );
        assert!(served_git["inputSchema"]["properties"]["include_patch"].is_object());

        let served_elevated = served_tools.body["result"]["tools"]
            .as_array()
            .and_then(|tools| tools.iter().find(|tool| tool["name"] == "elevated_exec"))
            .expect("fresh serving instance exposes elevated_exec");
        assert_eq!(served_elevated["inputSchema"]["type"], "object");
        assert!(served_elevated["inputSchema"].get("oneOf").is_none());
        for property in [
            "operation",
            "program",
            "args",
            "shell",
            "command",
            "workdir",
            "action",
            "path",
            "destination",
            "content_base64",
            "recursive",
            "timeout_ms",
            "max_output_bytes",
        ] {
            assert!(
                served_elevated["inputSchema"]["properties"][property].is_object(),
                "served elevated_exec schema lost {property}"
            );
        }
        let invalid_control = public_tool_call(
            pep.port(),
            &session,
            6881,
            "command_control",
            json!({"action":"read","output_ref":"lb-output-missing","session_id":"lb-session-cross-action"}),
        );
        assert_eq!(
            invalid_control.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
            "cross-action command_control fields were not rejected: {:#?}",
            invalid_control.body
        );
        let invalid_git = public_tool_call(
            pep.port(),
            &session,
            6882,
            "git_workflow",
            json!({"action":"status","rev":"HEAD"}),
        );
        assert_eq!(
            invalid_git.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
            "cross-action git_workflow fields were not rejected: {:#?}",
            invalid_git.body
        );
        let invalid_document = public_tool_call(
            pep.port(),
            &session,
            6883,
            "document_workflow",
            json!({"action":"rebuild","path":"range.txt","content":"x","source":"range.txt"}),
        );
        assert_eq!(
            invalid_document.body["result"]["structuredContent"]["error"]["code"],
            "InvalidArgument",
            "cross-action document_workflow fields were not rejected: {:#?}",
            invalid_document.body
        );
        let invalid_resume = public_tool_call(
            pep.port(),
            &session,
            6884,
            "agent_workflow",
            json!({"action":"resume","path":"."}),
        );
        assert_eq!(
            invalid_resume.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
            "resume accepted fields other than action: {:#?}",
            invalid_resume.body
        );
        let directory_schema = &served_agent["inputSchema"]["properties"]["directory_changes"];
        assert_eq!(directory_schema["type"], "array");
        assert_eq!(
            directory_schema["items"]["properties"]["action"]["enum"],
            json!(["create_directory", "remove_empty_directory"])
        );

        let long_command = public_tool_call(
            pep.port(),
            &session,
            689,
            "exec_command",
            json!({
                "command":"Start-Sleep -Milliseconds 1800; Write-Output LB_GEN18_LONG_DONE",
                "shell":"windows_powershell",
                "yield_time_ms":0,
                "timeout_ms":10000
            }),
        );
        assert_eq!(
            long_command.body["result"]["structuredContent"]["data"]["status"], "running",
            "generation18 lifecycle fixture did not return a public running session: {:#?}",
            long_command.body
        );
        thread::sleep(Duration::from_millis(650));
        assert!(
            matches!(
                pep.current_task_projection().actual_snapshot(),
                CurrentTaskStatus::Active(CurrentTask {
                    kind: TaskKind::ExecuteCommand,
                    state: TaskExecutionState::Running,
                    ..
                })
            ),
            "CurrentTask stopped before the public command session became terminal"
        );
        let lifecycle_deadline = Instant::now() + Duration::from_secs(6);
        while pep.current_task_projection().actual_snapshot() != CurrentTaskStatus::Idle {
            assert!(
                Instant::now() < lifecycle_deadline,
                "CurrentTask did not converge to Idle after public command terminal"
            );
            thread::sleep(Duration::from_millis(25));
        }

        let nested_status = public_tool_call(
            pep.port(),
            &session,
            690,
            "git_workflow",
            json!({"action":"status","path":"NestedProject"}),
        );
        assert_eq!(
            nested_status.body["result"]["isError"], false,
            "{:#?}",
            nested_status.body
        );
        assert_eq!(
            nested_status.body["result"]["structuredContent"]["data"]["repository_root"],
            "NestedProject"
        );
        let nested_workflow = public_tool_call(
            pep.port(),
            &session,
            691,
            "agent_workflow",
            json!({
                "action":"bugfix",
                "path":"NestedProject/src",
                "commands":[{"command":"cd","shell":"cmd","yield_time_ms":10000}]
            }),
        );
        assert_eq!(
            nested_workflow.body["result"]["isError"], false,
            "{:#?}",
            nested_workflow.body
        );
        let workflow_data = &nested_workflow.body["result"]["structuredContent"]["data"];
        assert_eq!(
            workflow_data["project"]["selected_path"],
            "NestedProject/src"
        );
        assert_eq!(workflow_data["project"]["repository_root"], "NestedProject");
        assert_eq!(
            workflow_data["git_before"]["repository_root"],
            "NestedProject"
        );
        assert_eq!(
            workflow_data["git_after"]["repository_root"],
            "NestedProject"
        );
        let nested_command_output = workflow_data["commands"][0]["output"]
            .as_str()
            .unwrap_or_default()
            .replace('/', "\\");
        assert!(
            nested_command_output
                .to_ascii_lowercase()
                .contains("nestedproject\\src"),
            "default workflow command workdir ignored selected project: {nested_command_output:?}"
        );

        let mkdir = public_tool_call(
            pep.port(),
            &session,
            692,
            "agent_workflow",
            json!({
                "action":"document",
                "directory_changes":[{"action":"create_directory","path":"schema30-dir"}]
            }),
        );
        assert_eq!(mkdir.body["result"]["isError"], false, "{:#?}", mkdir.body);
        assert!(workspace.join("schema30-dir").is_dir());
        let rmdir = public_tool_call(
            pep.port(),
            &session,
            693,
            "agent_workflow",
            json!({
                "action":"document",
                "directory_changes":[{"action":"remove_empty_directory","path":"schema30-dir"}]
            }),
        );
        assert_eq!(rmdir.body["result"]["isError"], false, "{:#?}", rmdir.body);
        assert!(!workspace.join("schema30-dir").exists());

        fs::create_dir(workspace.join("schema30-nonempty")).unwrap();
        fs::write(workspace.join("schema30-nonempty/keep.txt"), b"keep").unwrap();
        let nonempty = public_tool_call(
            pep.port(),
            &session,
            694,
            "agent_workflow",
            json!({
                "action":"document",
                "directory_changes":[{"action":"remove_empty_directory","path":"schema30-nonempty"}]
            }),
        );
        assert_eq!(
            nonempty.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
            "{:#?}",
            nonempty.body
        );
        assert!(workspace.join("schema30-nonempty/keep.txt").is_file());
        let cancel_after_failed_workflow = public_tool_call(
            pep.port(),
            &session,
            6945,
            "task_control",
            json!({"action":"cancel"}),
        );
        assert_eq!(
            cancel_after_failed_workflow.body["result"]["structuredContent"]["data"]["durable_task_cancelled"],
            false,
            "{:#?}",
            cancel_after_failed_workflow.body
        );
        assert_eq!(
            cancel_after_failed_workflow.body["result"]["structuredContent"]["data"]["state"],
            "idle",
            "{:#?}",
            cancel_after_failed_workflow.body
        );
        assert!(
            cancel_after_failed_workflow.body["result"]["structuredContent"]["data"]
                ["current_workflow"]
                .is_null(),
            "{:#?}",
            cancel_after_failed_workflow.body
        );
        let escaped_directory = public_tool_call(
            pep.port(),
            &session,
            695,
            "agent_workflow",
            json!({
                "action":"document",
                "directory_changes":[{"action":"create_directory","path":"../escape"}]
            }),
        );
        assert_eq!(
            escaped_directory.body["result"]["structuredContent"]["error"]["code"],
            "WorkspaceDenied",
            "{:#?}",
            escaped_directory.body
        );

        pep.set_permission_mode(PermissionMode::Edit);
        let stale_full_session = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":6975,"method":"ping","params":{}}),
        );
        assert_eq!(stale_full_session.status, 404);
        assert_eq!(
            stale_full_session.body["error"]["error_code"],
            "Unavailable"
        );
        assert_eq!(stale_full_session.body["error"]["phase"], "mcp");
        assert_eq!(stale_full_session.body["error"]["cause"], "session_stale");
        assert_eq!(stale_full_session.body["error"]["http_status"], 404);
        session = initialize(pep.port(), 6976)
            .session
            .expect("schema28 Edit reconnect session");
        let edit_mkdir = public_tool_call(
            pep.port(),
            &session,
            698,
            "agent_workflow",
            json!({
                "action":"document",
                "directory_changes":[{"action":"create_directory","path":"schema30-edit-dir"}]
            }),
        );
        assert_eq!(
            edit_mkdir.body["result"]["isError"], false,
            "{:#?}",
            edit_mkdir.body
        );
        assert!(workspace.join("schema30-edit-dir").is_dir());
        let edit_rmdir = public_tool_call(
            pep.port(),
            &session,
            6981,
            "agent_workflow",
            json!({
                "action":"document",
                "directory_changes":[{"action":"remove_empty_directory","path":"schema30-edit-dir"}]
            }),
        );
        assert_eq!(
            edit_rmdir.body["result"]["isError"], false,
            "{:#?}",
            edit_rmdir.body
        );
        assert!(!workspace.join("schema30-edit-dir").exists());
        pep.set_permission_mode(PermissionMode::Full);
        let stale_edit_session = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":6977,"method":"ping","params":{}}),
        );
        assert_eq!(stale_edit_session.status, 404);
        session = initialize(pep.port(), 6978)
            .session
            .expect("schema28 Full reconnect session");

        for (id, shell) in [(696u64, "windows_powershell"), (697u64, "auto")] {
            let baseline = public_tool_call(
                pep.port(),
                &session,
                id,
                "exec_command",
                json!({
                    "command":"$loc=(Get-Location).Path; $exists=Test-Path -LiteralPath '.'; $count=@(Get-ChildItem -LiteralPath '.').Count; Write-Output ('SCHEMA30_BASELINE '+$exists+' '+$count+' '+$loc)",
                    "shell":shell,
                    "yield_time_ms":10000
                }),
            );
            assert_eq!(
                baseline.body["result"]["isError"], false,
                "shell={shell}: {:#?}",
                baseline.body
            );
            let output = baseline.body["result"]["structuredContent"]["data"]["output"]
                .as_str()
                .unwrap_or_default();
            assert!(
                output.contains("SCHEMA30_BASELINE True"),
                "shell={shell}: {output:?}"
            );
        }

        let module_root_literal = module_root.to_string_lossy().replace('\'', "''");
        let autoload = public_tool_call(
            pep.port(),
            &session,
            699,
            "exec_command",
            json!({
                "command":format!("$env:PSModulePath='{module_root_literal}'; Invoke-LbGen14Auto"),
                "shell":"windows_powershell",
                "yield_time_ms":10000
            }),
        );
        assert_eq!(
            autoload.body["result"]["isError"], true,
            "{:#?}",
            autoload.body
        );
        assert_eq!(
            autoload.body["result"]["structuredContent"]["error"]["code"], "ProcessFailed",
            "{:#?}",
            autoload.body
        );
        assert!(
            !serde_json::to_string(&autoload.body)
                .unwrap()
                .contains("LB_GEN14_MODULE_AUTOLOAD_SENTINEL"),
            "module auto-loading escaped the trusted PowerShell prologue: {:#?}",
            autoload.body
        );

        let quoted = public_tool_call(
            pep.port(),
            &session,
            701,
            "exec_command",
            json!({
                "command":"Write-Output \"a|b\"; Write-Output \"a&b\"; Write-Output 'q|b'; Write-Output 'q&b'; Write-Output '中文输出✓'",
                "shell":"windows_powershell",
                "yield_time_ms":10000
            }),
        );
        assert_eq!(
            quoted.body["result"]["isError"], false,
            "{:#?}",
            quoted.body
        );
        let quoted_output = quoted.body["result"]["structuredContent"]["data"]["output"]
            .as_str()
            .unwrap_or_default();
        for literal in ["a|b", "a&b", "q|b", "q&b", "中文输出✓"] {
            assert!(
                quoted_output.contains(literal),
                "missing {literal:?}: {quoted_output:?}"
            );
        }

        let powershell_error = public_tool_call(
            pep.port(),
            &session,
            7011,
            "exec_command",
            json!({
                "command":"Write-Error \"READERR 🚀\"",
                "shell":"windows_powershell",
                "yield_time_ms":10000
            }),
        );
        assert_eq!(
            powershell_error.body["result"]["isError"], true,
            "{:#?}",
            powershell_error.body
        );
        let powershell_error_data = &powershell_error.body["result"]["structuredContent"]["data"];
        let powershell_error_output = powershell_error_data["output"].as_str().unwrap_or_default();
        assert!(
            powershell_error_output.contains("READERR 🚀"),
            "{powershell_error_output:?}"
        );
        for private in [
            "PSModuleAutoLoadingPreference",
            "Microsoft.PowerShell.Management",
            "OutputEncoding",
            "_xD83D_",
            "_xDE80_",
        ] {
            assert!(
                !powershell_error_output.contains(private),
                "{powershell_error_output:?}"
            );
        }
        let stderr_ref = powershell_error_data["output_refs"]["stderr"]
            .as_str()
            .expect("PowerShell failure exposes public retained stderr handle");
        let retained_error = public_tool_call(
            pep.port(),
            &session,
            7012,
            "command_control",
            json!({"action":"read","output_ref":stderr_ref,"stream":"stderr","offset":0,"limit":1048576}),
        );
        assert_eq!(
            retained_error.body["result"]["isError"], false,
            "{:#?}",
            retained_error.body
        );
        let retained_content =
            retained_error.body["result"]["structuredContent"]["data"]["content"]
                .as_str()
                .unwrap_or_default();
        assert!(
            retained_content.contains("READERR 🚀"),
            "{retained_content:?}"
        );
        for private in [
            "PSModuleAutoLoadingPreference",
            "Microsoft.PowerShell.Management",
            "OutputEncoding",
            "_xD83D_",
            "_xDE80_",
        ] {
            assert!(!retained_content.contains(private), "{retained_content:?}");
        }

        let cmd_cd_switch = public_tool_call(
            pep.port(),
            &session,
            7013,
            "exec_command",
            json!({
                "command":"cd /d . && echo LB_CMD_D_OK",
                "shell":"cmd",
                "yield_time_ms":10000
            }),
        );
        assert_eq!(
            cmd_cd_switch.body["result"]["isError"], false,
            "{:#?}",
            cmd_cd_switch.body
        );
        assert!(
            cmd_cd_switch.body["result"]["structuredContent"]["data"]["output"]
                .as_str()
                .unwrap_or_default()
                .contains("LB_CMD_D_OK"),
            "{:#?}",
            cmd_cd_switch.body
        );
        let cmd_escape = public_tool_call(
            pep.port(),
            &session,
            7014,
            "exec_command",
            json!({
                "command":"cd /d C:\\Windows",
                "shell":"cmd",
                "yield_time_ms":10000
            }),
        );
        assert_eq!(
            cmd_escape.body["result"]["isError"], true,
            "{:#?}",
            cmd_escape.body
        );
        assert_eq!(
            cmd_escape.body["result"]["structuredContent"]["error"]["code"], "WorkspaceDenied",
            "{:#?}",
            cmd_escape.body
        );

        let auto_utf8 = public_tool_call(
            pep.port(),
            &session,
            702,
            "exec_command",
            json!({
                "command":"Write-Output '自动中文✓'",
                "shell":"auto",
                "yield_time_ms":10000
            }),
        );
        assert_eq!(
            auto_utf8.body["result"]["isError"], false,
            "{:#?}",
            auto_utf8.body
        );
        assert!(
            auto_utf8.body["result"]["structuredContent"]["data"]["output"]
                .as_str()
                .unwrap_or_default()
                .contains("自动中文✓")
        );

        let running = public_tool_call(
            pep.port(),
            &session,
            703,
            "exec_command",
            json!({
                "command":"Start-Sleep -Milliseconds 300; Write-Output 'poll-1'; Start-Sleep -Milliseconds 600; Write-Output 'poll-2'; Start-Sleep -Milliseconds 600; Write-Output 'poll-3'; $line=[Console]::In.ReadLine(); Write-Output ('write:'+ $line); Start-Sleep -Seconds 30",
                "shell":"windows_powershell",
                "yield_time_ms":0,
                "timeout_ms":45000
            }),
        );
        assert_eq!(
            running.body["result"]["structuredContent"]["data"]["status"], "running",
            "{:#?}",
            running.body
        );
        let public_session = running.body["result"]["structuredContent"]["data"]["session_id"]
            .as_str()
            .expect("schema28 public session")
            .to_string();

        let mut nonempty_polls = Vec::new();
        let poll_deadline = Instant::now() + Duration::from_secs(6);
        let mut poll_id = 710u64;
        while nonempty_polls.len() < 3 && Instant::now() < poll_deadline {
            thread::sleep(Duration::from_millis(120));
            let poll = public_tool_call(
                pep.port(),
                &session,
                poll_id,
                "command_control",
                json!({"action":"poll","session_id":public_session,"wait_ms":0}),
            );
            poll_id += 1;
            assert!(poll.body.get("error").is_none(), "{:#?}", poll.body);
            let output = poll.body["result"]["structuredContent"]["data"]["output"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if !output.is_empty() {
                nonempty_polls.push(output);
            }
        }
        assert_eq!(
            nonempty_polls.len(),
            3,
            "incremental polls: {nonempty_polls:#?}"
        );
        for (index, marker) in ["poll-1", "poll-2", "poll-3"].into_iter().enumerate() {
            assert!(
                nonempty_polls[index].contains(marker),
                "{nonempty_polls:#?}"
            );
            for other in ["poll-1", "poll-2", "poll-3"] {
                if other != marker {
                    assert!(
                        !nonempty_polls[index].contains(other),
                        "{nonempty_polls:#?}"
                    );
                }
            }
        }
        let empty_poll = public_tool_call(
            pep.port(),
            &session,
            poll_id,
            "command_control",
            json!({"action":"poll","session_id":public_session,"wait_ms":0}),
        );
        assert_eq!(
            empty_poll.body["result"]["structuredContent"]["data"]["output"], "",
            "poll output replayed: {:#?}",
            empty_poll.body
        );

        let wait_started = Instant::now();
        let waited_poll = public_tool_call(
            pep.port(),
            &session,
            poll_id + 1,
            "command_control",
            json!({"action":"poll","session_id":public_session,"wait_ms":500}),
        );
        assert!(
            wait_started.elapsed() <= Duration::from_millis(1500),
            "poll wait_ms budget exceeded: {:?}",
            wait_started.elapsed()
        );
        assert_eq!(
            waited_poll.body["result"]["structuredContent"]["data"]["output"],
            ""
        );

        let write_started = Instant::now();
        let written = public_tool_call(
            pep.port(),
            &session,
            poll_id + 2,
            "command_control",
            json!({
                "action":"write",
                "session_id":public_session,
                "chars":"after-start\n",
                "wait_ms":500
            }),
        );
        assert!(
            write_started.elapsed() <= Duration::from_millis(1500),
            "write wait_ms budget exceeded: {:?}",
            write_started.elapsed()
        );
        assert_eq!(
            written.body["result"]["isError"], false,
            "{:#?}",
            written.body
        );
        assert!(
            written.body["result"]["structuredContent"]["data"]["output"]
                .as_str()
                .unwrap_or_default()
                .contains("write:after-start"),
            "{:#?}",
            written.body
        );

        let kill_started = Instant::now();
        let killed = public_tool_call(
            pep.port(),
            &session,
            poll_id + 3,
            "command_control",
            json!({"action":"kill","session_id":public_session,"signal":"TERM","wait_ms":1000}),
        );
        assert_eq!(
            killed.body["result"]["isError"], false,
            "healthy kill regressed: {:#?}",
            killed.body
        );
        assert!(
            kill_started.elapsed() <= Duration::from_millis(2000),
            "kill wait_ms budget exceeded: {:?}",
            kill_started.elapsed()
        );
        assert_eq!(killed.body["result"]["structuredContent"]["ok"], true);
        assert_eq!(
            killed.body["result"]["structuredContent"]["data"]["status"],
            "cancelled"
        );
        for id in [poll_id + 4, poll_id + 5] {
            let terminal = public_tool_call(
                pep.port(),
                &session,
                id,
                "command_control",
                json!({"action":"poll","session_id":public_session}),
            );
            assert_eq!(
                terminal.body["result"]["structuredContent"]["error"]["code"], "ProcessCancelled",
                "terminal regressed to unavailable: {:#?}",
                terminal.body
            );
            assert_eq!(
                terminal.body["result"]["structuredContent"]["data"]["status"],
                "cancelled"
            );
        }
        let second_terminal = public_tool_call(
            pep.port(),
            &session,
            poll_id + 5,
            "command_control",
            json!({"action":"poll","session_id":public_session}),
        );
        assert_eq!(
            second_terminal.body["result"]["structuredContent"]["data"]["output"],
            ""
        );

        let durable_terminal = public_tool_call(
            pep.port(),
            &session,
            poll_id + 6,
            "task_control",
            json!({"action":"get"}),
        );
        let durable =
            &durable_terminal.body["result"]["structuredContent"]["data"]["last_terminal_command"];
        assert_eq!(durable["session_id"], public_session);
        assert_eq!(durable["status"], "cancelled");
        assert_eq!(durable["cancelled"], true);
        assert_eq!(durable["error_code"], "ProcessCancelled");
        assert!(
            !serde_json::to_string(durable).unwrap().contains("PRIVATE_"),
            "schema29 durable terminal task-state leaked a private handle: {durable:#?}"
        );

        for (id, size) in [(760u64, 512u32), (761u64, 64u32)] {
            let viewed = public_tool_call(
                pep.port(),
                &session,
                id,
                "view_image",
                json!({
                    "path":"large.png",
                    "max_width":size,
                    "max_height":size,
                    "auto_resize":true,
                    "max_bytes":10485760
                }),
            );
            assert_eq!(
                viewed.body["result"]["isError"], false,
                "{:#?}",
                viewed.body
            );
            let data = &viewed.body["result"]["structuredContent"]["data"];
            assert_eq!(data["original_width"], 1024);
            assert_eq!(data["original_height"], 1024);
            assert_eq!(data["width"], size);
            assert_eq!(data["height"], size);
            assert_eq!(data["resized"], true);
            let encoded = viewed.body["result"]["content"][0]["data"]
                .as_str()
                .expect("public image data");
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap();
            let decoded = image::load_from_memory(&bytes).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (size, size));
        }

        let one_line = public_tool_call(
            pep.port(),
            &session,
            770,
            "document_workflow",
            json!({"action":"inspect","path":"range.txt","start_line":5,"end_line":5}),
        );
        assert_eq!(
            one_line.body["result"]["structuredContent"]["data"]["text"],
            "line5\n"
        );
        assert_eq!(
            one_line.body["result"]["structuredContent"]["data"]["start_line"],
            5
        );
        assert_eq!(
            one_line.body["result"]["structuredContent"]["data"]["end_line"],
            5
        );
        let three_lines = public_tool_call(
            pep.port(),
            &session,
            771,
            "document_workflow",
            json!({"action":"inspect","path":"range.txt","start_line":1,"end_line":3}),
        );
        assert_eq!(
            three_lines.body["result"]["structuredContent"]["data"]["text"],
            "line1\nline2\nline3\n"
        );
        let invalid_range = public_tool_call(
            pep.port(),
            &session,
            772,
            "document_workflow",
            json!({"action":"inspect","path":"range.txt","start_line":5,"end_line":3}),
        );
        assert_eq!(
            invalid_range.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
            "{:#?}",
            invalid_range.body
        );

        let mut coding = pep.stop().expect("schema28 PEP stops");
        coding.stop().expect("schema28 Coding Tools runtime stops");
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn absolute_and_relative_active_workspace_paths_are_equivalent() {
        let root = repo_root();
        let workspace = temp_workspace();
        let outside = workspace.with_extension("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(workspace.join("absolute.txt"), b"ABSOLUTE_PATH_OK\n").unwrap();
        fs::write(outside.join("outside.txt"), b"OUTSIDE\n").unwrap();
        fs::copy(
            root.join("assets/icons/localbridge.png"),
            workspace.join("absolute.png"),
        )
        .unwrap();

        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("PEP listener ready");
        let session = initialize(pep.port(), 810)
            .session
            .expect("absolute path test session");

        for (id, path) in [
            (811, "absolute.txt".to_string()),
            (
                812,
                workspace
                    .join("absolute.txt")
                    .to_string_lossy()
                    .into_owned(),
            ),
        ] {
            let response = public_tool_call(
                pep.port(),
                &session,
                id,
                "document_workflow",
                json!({"action":"inspect","path":path}),
            );
            assert_eq!(response.status, 200);
            assert_eq!(
                response.body["result"]["isError"], false,
                "in-workspace document path failed: {}",
                response.body
            );
        }

        let workdir = public_tool_call(
            pep.port(),
            &session,
            813,
            "exec_command",
            json!({
                "command":"cd",
                "shell":"cmd",
                "workdir":workspace.to_string_lossy(),
                "yield_time_ms":10000
            }),
        );
        assert_eq!(workdir.status, 200);
        assert_eq!(
            workdir.body["result"]["isError"], false,
            "absolute in-workspace workdir failed: {}",
            workdir.body
        );

        let image = public_tool_call(
            pep.port(),
            &session,
            814,
            "view_image",
            json!({
                "path":workspace.join("absolute.png").to_string_lossy(),
                "max_width":64,
                "max_height":64
            }),
        );
        assert_eq!(image.status, 200);
        assert_eq!(
            image.body["result"]["isError"], false,
            "absolute in-workspace image path failed: {}",
            image.body
        );

        let outside_denied = public_tool_call(
            pep.port(),
            &session,
            815,
            "document_workflow",
            json!({
                "action":"inspect",
                "path":outside.join("outside.txt").to_string_lossy()
            }),
        );
        assert_tool_error(&outside_denied, "WorkspaceDenied");

        let mut coding = pep.stop().expect("absolute path PEP stops");
        coding.stop().expect("absolute path MCP stops");
        cleanup_test_directory(&workspace);
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn schema40_health_probe_remains_authenticated_while_facade_lock_is_held() {
        let root = repo_root();
        let workspace = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("PEP ready");
        let facade_guard = pep.guard.as_ref().unwrap().lock().unwrap();
        let started = Instant::now();
        let health = pep
            .coding_runtime_health()
            .expect("independent authenticated health probe")
            .expect("health is available");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            health.state,
            super::super::facade::CodingRuntimeHealthState::Ready
        );
        assert!(health.authenticated_mcp);
        drop(facade_guard);
        let mut coding = pep.stop().expect("PEP stop after independent health probe");
        coding
            .stop()
            .expect("MCP stop after independent health probe");
        cleanup_test_directory(&workspace);
    }

    #[cfg(windows)]
    #[test]
    fn schema40_real_root_process_alive_but_mcp_unresponsive_is_not_ready() {
        let root = repo_root();
        let workspace = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let coding_pid = coding.process_snapshot().pid;
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("PEP ready");
        let suspended = SuspendedProcess::suspend(coding_pid);
        assert_eq!(pep.upstream_root_is_running().unwrap(), Some(true));
        let started = Instant::now();
        let health = pep
            .coding_runtime_health()
            .expect("bounded health probe")
            .expect("health state");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "health probe exceeded bound: {:?}",
            started.elapsed()
        );
        assert_ne!(
            health.state,
            super::super::facade::CodingRuntimeHealthState::Ready
        );
        assert!(!health.authenticated_mcp);
        assert!(
            health.root_process_alive,
            "suspended MCP root must still be alive"
        );
        drop(suspended);
        let ready_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let health = pep.coding_runtime_health().unwrap().unwrap();
            if health.state == super::super::facade::CodingRuntimeHealthState::Ready
                && health.authenticated_mcp
            {
                break;
            }
            assert!(
                Instant::now() < ready_deadline,
                "MCP did not recover after resume: {health:?}"
            );
            thread::sleep(Duration::from_millis(50));
        }
        let mut coding = pep.stop().expect("PEP stop after suspended MCP test");
        coding.stop().expect("MCP stop after suspended MCP test");
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn actual_bundled_mcp_is_reached_only_through_loopback_policy_server() {
        let root = repo_root();
        let workspace = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let coding_pid = coding.process_snapshot().pid;
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("PEP listener ready");
        assert!(pep.endpoint().starts_with("http://127.0.0.1:"));
        assert!(pep.is_running());
        assert!(!format!("{pep:?}").contains(SYNTHETIC_BEARER));

        let initialized = initialize(pep.port(), 1);
        assert_eq!(initialized.status, 200);
        assert_eq!(
            initialized.body["result"]["protocolVersion"],
            CURRENT_PROTOCOL_VERSION
        );
        let session = initialized.session.expect("downstream MCP session");
        let notified = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        );
        assert_eq!(notified.status, 202);

        let full_tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
        );
        let full_catalog = full_tools.body["result"]["tools"].as_array().unwrap();
        assert_eq!(full_catalog.len(), 10);
        assert!(
            full_catalog
                .iter()
                .any(|tool| tool["name"] == "exec_command")
        );
        assert!(
            full_catalog
                .iter()
                .any(|tool| tool["name"] == "elevated_exec")
        );
        let elevated_schema = full_catalog
            .iter()
            .find(|tool| tool["name"] == "elevated_exec")
            .expect("stable elevated_exec definition");
        assert_eq!(elevated_schema["inputSchema"]["type"], "object");
        for property in [
            "operation",
            "program",
            "args",
            "shell",
            "command",
            "workdir",
            "action",
            "path",
            "destination",
            "content_base64",
            "recursive",
            "timeout_ms",
            "max_output_bytes",
        ] {
            assert!(
                elevated_schema["inputSchema"]["properties"][property].is_object(),
                "elevated_exec top-level property missing: {property}"
            );
        }
        assert!(
            elevated_schema["inputSchema"].get("oneOf").is_none(),
            "elevated_exec public input schema must remain a directly projectable top-level object"
        );

        for private in [
            "read_file",
            "apply_patch",
            "git_status",
            "write_stdin",
            "server_info",
        ] {
            assert!(
                !full_catalog.iter().any(|tool| tool["name"] == private),
                "private upstream tool leaked into public registry: {private}"
            );
        }
        let raw_private = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":"raw-private","method":"tools/call",
                "params":{"name":"read_file","arguments":{"path":"probe.txt"}}
            }),
        );
        assert_tool_error(&raw_private, "CapabilityDenied");
        thread::sleep(Duration::from_millis(540));

        pep.set_permission_mode(PermissionMode::Edit);
        let stale_full_call = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":3,"method":"tools/call",
                "params":{"name":"exec_command","arguments":{"command":"echo must-not-run"}}
            }),
        );
        assert_eq!(stale_full_call.status, 404);
        let edit_session = initialize(pep.port(), 31)
            .session
            .expect("Edit reinitialize after catalog change");
        let denied = post(
            pep.port(),
            Some(&edit_session),
            &json!({
                "jsonrpc":"2.0","id":32,"method":"tools/call",
                "params":{"name":"exec_command","arguments":{"command":"echo must-not-run"}}
            }),
        );
        assert_tool_error(&denied, "PolicyDenied");
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ExecuteCommand,
                state: TaskExecutionState::Blocked,
                ..
            })
        ));
        thread::sleep(Duration::from_millis(540));
        let denied_timing = pep.current_task_projection().timing_snapshot();
        assert_eq!(denied_timing.status, CurrentTaskStatus::Idle);
        assert_eq!(
            denied_timing.last_tool.as_ref().map(|tool| tool.kind),
            Some(TaskKind::ExecuteCommand)
        );

        let edit_tools = post(
            pep.port(),
            Some(&edit_session),
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/list","params":{}}),
        );
        let edit_tool_names = edit_tools.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(edit_tool_names.len(), 8);
        assert!(edit_tool_names.contains(&"agent_workflow"));
        assert!(edit_tool_names.contains(&"elevated_exec"));
        assert!(edit_tool_names.contains(&"task_control"));
        for process_tool in ["exec_command", "command_control"] {
            assert!(!edit_tool_names.contains(&process_tool));
        }

        let read_started = Instant::now();
        let read = post(
            pep.port(),
            Some(&edit_session),
            &json!({
                "jsonrpc":"2.0","id":5,"method":"tools/call",
                "params":{"name":"document_workflow","arguments":{"action":"inspect","path":"probe.txt"}}
            }),
        );
        let read_round_trip = read_started.elapsed();
        assert!(
            read.body.get("result").is_some(),
            "allowed document_workflow inspect must forward: {}",
            read.body
        );
        assert!(
            read_round_trip < Duration::from_millis(500),
            "UI presentation retention must not delay real MCP response: {read_round_trip:?}"
        );
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ReadFile,
                ..
            })
        ));
        thread::sleep(Duration::from_millis(540));
        let timing = pep.current_task_projection().timing_snapshot();
        assert_eq!(timing.status, CurrentTaskStatus::Idle);
        assert_eq!(
            timing.last_tool.as_ref().map(|tool| tool.kind),
            Some(TaskKind::ReadFile)
        );

        pep.set_permission_mode(PermissionMode::Full);
        let stale_edit_tools = post(
            pep.port(),
            Some(&edit_session),
            &json!({"jsonrpc":"2.0","id":"cached-full","method":"tools/list","params":{}}),
        );
        assert_eq!(stale_edit_tools.status, 404);
        let full_session = initialize(pep.port(), 41)
            .session
            .expect("Full reinitialize after catalog change");
        let cached_full = post(
            pep.port(),
            Some(&full_session),
            &json!({"jsonrpc":"2.0","id":"fresh-full","method":"tools/list","params":{}}),
        );
        assert!(
            cached_full.body["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "exec_command")
        );

        let unknown_action = post(
            pep.port(),
            Some(&full_session),
            &json!({
                "jsonrpc":"2.0","id":"unknown-public-action","method":"tools/call",
                "params":{"name":"git_workflow","arguments":{"action":"future_private_action"}}
            }),
        );
        assert_tool_error(&unknown_action, "CapabilityDenied");
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                state: TaskExecutionState::Blocked,
                ..
            })
        ));
        thread::sleep(Duration::from_millis(540));

        let base_policy = fs::read_to_string(root.join("runtime-policy.toml")).unwrap();
        let narrowed_policy = base_policy
            .replace(
                "edit_tools = [\"workspace_context\", \"agent_workflow\", \"filesystem\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
                "edit_tools = [\"workspace_context\", \"filesystem\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
            )
            .replace(
                "full_tools = [\"workspace_context\", \"agent_workflow\", \"filesystem\", \"exec_command\", \"command_control\", \"task_control\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
                "full_tools = [\"workspace_context\", \"filesystem\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
            )
            .replace(
                "elevated_tools = [\"workspace_context\", \"agent_workflow\", \"filesystem\", \"exec_command\", \"command_control\", \"task_control\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
                "elevated_tools = [\"workspace_context\", \"filesystem\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
            );
        pep.replace_policy(CapabilityPolicy::from_toml(&narrowed_policy).unwrap())
            .expect("live public policy narrowing");
        let stale_policy_call = post(
            pep.port(),
            Some(&full_session),
            &json!({
                "jsonrpc":"2.0","id":"stale-policy-call","method":"tools/call",
                "params":{"name":"exec_command","arguments":{"command":"echo cached-list-must-not-run"}}
            }),
        );
        assert_eq!(stale_policy_call.status, 404);
        let narrowed_session = initialize(pep.port(), 61)
            .session
            .expect("reinitialize after policy catalog change");
        let narrowed_denied = post(
            pep.port(),
            Some(&narrowed_session),
            &json!({
                "jsonrpc":"2.0","id":"narrowed-denied","method":"tools/call",
                "params":{"name":"exec_command","arguments":{"command":"echo cached-list-must-not-run"}}
            }),
        );
        assert_tool_error(&narrowed_denied, "PolicyDenied");
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ExecuteCommand,
                state: TaskExecutionState::Blocked,
                ..
            })
        ));
        let narrowed_tools = post(
            pep.port(),
            Some(&narrowed_session),
            &json!({"jsonrpc":"2.0","id":"narrowed-tools","method":"tools/list","params":{}}),
        );
        assert!(
            !narrowed_tools.body["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "exec_command")
        );
        thread::sleep(Duration::from_millis(540));

        let reinitialized = initialize(pep.port(), 6);
        let new_session = reinitialized.session.unwrap();
        assert_ne!(new_session, narrowed_session);
        let still_live = post(
            pep.port(),
            Some(&narrowed_session),
            &json!({"jsonrpc":"2.0","id":7,"method":"ping","params":{}}),
        );
        assert_eq!(still_live.status, 200);
        assert_eq!(delete(pep.port(), &narrowed_session), 204);
        assert_eq!(delete(pep.port(), &new_session), 204);

        let pep_port = pep.port();
        let mut coding = pep.stop().expect("PEP stop returns owned MCP runtime");
        assert_eq!(coding.process_snapshot().pid, coding_pid);
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, pep_port)).is_err());
        coding.stop().expect("MCP Job stop");
        assert_eq!(coding.active_processes().unwrap(), 0);
        drop(coding);
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn schema34_full_executes_static_workspace_scripts_and_rejects_traversal_before_launch() {
        let root = repo_root();
        let workspace = temp_workspace();
        let scripts = workspace.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("probe.cmd"), b"@echo LB007_CMD_SCRIPT_OK\r\n").unwrap();
        fs::write(scripts.join("probe.bat"), b"@echo LB007_BAT_SCRIPT_OK\r\n").unwrap();
        fs::write(
            scripts.join("probe.ps1"),
            b"Write-Output 'LB007_PS1_SCRIPT_OK'\r\n",
        )
        .unwrap();

        let outside_name = format!(
            "lb007-outside-{}-{}.cmd",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let outside = workspace.parent().unwrap().join(&outside_name);
        fs::write(&outside, b"@echo SHOULD_NOT_RUN>should-not-exist.txt\r\n").unwrap();

        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready for script execution");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("PEP listener ready for script execution");
        let session = initialize(pep.port(), 700)
            .session
            .expect("script E2E MCP session");

        for (id, command, shell, marker) in [
            (701, r"scripts\probe.cmd", "cmd", "LB007_CMD_SCRIPT_OK"),
            (702, r"scripts\probe.bat", "cmd", "LB007_BAT_SCRIPT_OK"),
            (
                703,
                r".\scripts\probe.ps1",
                "windows_powershell",
                "LB007_PS1_SCRIPT_OK",
            ),
        ] {
            let response = public_tool_call(
                pep.port(),
                &session,
                id,
                "exec_command",
                json!({"command":command,"shell":shell,"yield_time_ms":10000}),
            );
            assert_eq!(
                response.body["result"]["isError"], false,
                "{:#?}",
                response.body
            );
            assert!(
                response.body["result"]["structuredContent"]["data"]["output"]
                    .as_str()
                    .unwrap_or_default()
                    .contains(marker),
                "{:#?}",
                response.body
            );
        }

        let nul = public_tool_call(
            pep.port(),
            &session,
            705,
            "exec_command",
            json!({
                "command":"echo hidden>nul && echo hidden-error 1>nul 2>nul && echo LB_SCHEMA42_NUL_OK",
                "shell":"cmd",
                "yield_time_ms":10000
            }),
        );
        assert_eq!(nul.body["result"]["isError"], false, "{:#?}", nul.body);
        assert!(
            nul.body["result"]["structuredContent"]["data"]["output"]
                .as_str()
                .unwrap_or_default()
                .contains("LB_SCHEMA42_NUL_OK")
        );
        for entry in fs::read_dir(&workspace).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            assert_ne!(name, "nul");
            assert_ne!(name, "nul.localbridge");
        }

        let oem = public_tool_call(
            pep.port(),
            &session,
            706,
            "exec_command",
            json!({
                "command":"echo LB_SCHEMA42_OEM_é中あ한Я",
                "shell":"cmd",
                "yield_time_ms":10000
            }),
        );
        assert_eq!(oem.body["result"]["isError"], false, "{:#?}", oem.body);
        let output = oem.body["result"]["structuredContent"]["data"]["output"]
            .as_str()
            .unwrap_or_default();
        assert!(output.contains("LB_SCHEMA42_OEM_"), "{output:?}");
        assert!(
            !output.contains('\u{fffd}'),
            "OEM output was decoded as lossy UTF-8: {output:?}"
        );
        assert!(
            ['é', '中', 'あ', '한', 'Я']
                .iter()
                .any(|candidate| output.contains(*candidate)),
            "no representative non-ASCII OEM character survived decoding: {output:?}"
        );

        let escaped = public_tool_call(
            pep.port(),
            &session,
            704,
            "exec_command",
            json!({
                "command":format!(r"..\{outside_name}"),
                "shell":"cmd",
                "yield_time_ms":10000
            }),
        );
        assert_tool_error(&escaped, "WorkspaceDenied");
        assert!(!workspace.join("should-not-exist.txt").exists());

        let mut coding = pep.stop().expect("script E2E PEP stop");
        coding.stop().expect("script E2E MCP stop");
        drop(coding);
        let _ = fs::remove_file(outside);
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn cancellation_reaches_actual_upstream_while_tool_call_is_running() {
        let root = repo_root();
        let workspace = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("PEP listener ready");
        let initialized = initialize(pep.port(), 20);
        let session = initialized.session.expect("downstream MCP session");
        assert_eq!(
            post(
                pep.port(),
                Some(&session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );

        let port = pep.port();
        let call_session = session.clone();
        let call_started = std::time::Instant::now();
        let call = thread::spawn(move || {
            post_with_read_timeout(
                port,
                Some(&call_session),
                &json!({
                    "jsonrpc":"2.0",
                    "id":"cancel-me",
                    "method":"tools/call",
                    "params":{
                        "name":"exec_command",
                        "arguments":{
                            "command":"Start-Sleep -Seconds 10",
                            "shell":"windows_powershell",
                            "yield_time_ms":10000,
                            "timeout_ms":20000,
                            "max_output_bytes":4096,
                            "verbosity":"summary"
                        }
                    }
                }),
                Duration::from_secs(6),
            )
        });

        let running_deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if matches!(
                pep.current_task_projection().snapshot(),
                CurrentTaskStatus::Active(ref task)
                    if task.state == crate::state::TaskExecutionState::Running
            ) {
                break;
            }
            assert!(
                std::time::Instant::now() < running_deadline,
                "tool call never projected Running"
            );
            thread::sleep(Duration::from_millis(10));
        }
        thread::sleep(Duration::from_millis(100));

        let cancel_started = std::time::Instant::now();
        let cancelled = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0",
                "method":"notifications/cancelled",
                "params":{"requestId":"cancel-me","reason":"LB-009 deterministic cancellation test"}
            }),
        );
        assert_eq!(cancelled.status, 202);
        assert!(
            cancel_started.elapsed() < Duration::from_secs(2),
            "cancellation transport was blocked"
        );

        let call_result = call.join().expect("tools/call client thread");
        assert!(
            call_started.elapsed() < Duration::from_secs(5),
            "cancelled command ran near natural 10 second duration"
        );
        assert!(
            call_result.body.get("result").is_some() || call_result.body.get("error").is_some(),
            "cancelled tools/call must terminate with a JSON-RPC response: {}",
            call_result.body
        );
        let presentation_deadline = std::time::Instant::now() + Duration::from_secs(1);
        while pep.current_task_projection().snapshot() != CurrentTaskStatus::Idle {
            assert!(
                std::time::Instant::now() < presentation_deadline,
                "cancelled tool remained visible beyond the bounded presentation window"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let mut coding = pep.stop().expect("PEP stop after cancellation");
        coding.stop().expect("MCP Job stop after cancellation");
        assert_eq!(coding.active_processes().unwrap(), 0);
        drop(coding);
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn task_control_cancel_reaches_running_call_without_waiting_for_facade_execution_lock() {
        let root = repo_root();
        let workspace = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("PEP listener ready");
        let initialized = initialize(pep.port(), 30);
        let session = initialized.session.expect("downstream MCP session");
        assert_eq!(
            post(
                pep.port(),
                Some(&session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );

        let idle_cancel = public_tool_call(
            pep.port(),
            &session,
            31,
            "task_control",
            json!({"action":"cancel"}),
        );
        assert_eq!(
            idle_cancel.body["result"]["structuredContent"]["data"]["state"], "idle",
            "{:#?}",
            idle_cancel.body
        );
        assert_eq!(
            idle_cancel.body["result"]["structuredContent"]["data"]["cancelled_requests"],
            0
        );

        let port = pep.port();
        let call_session = session.clone();
        let call_started = std::time::Instant::now();
        let call = thread::spawn(move || {
            post_with_read_timeout(
                port,
                Some(&call_session),
                &json!({
                    "jsonrpc":"2.0",
                    "id":"task-control-me",
                    "method":"tools/call",
                    "params":{
                        "name":"exec_command",
                        "arguments":{
                            "command":"Start-Sleep -Seconds 10",
                            "shell":"windows_powershell",
                            "yield_time_ms":10000,
                            "timeout_ms":20000,
                            "max_output_bytes":4096
                        }
                    }
                }),
                Duration::from_secs(6),
            )
        });

        let running_deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !matches!(
            pep.current_task_projection().actual_snapshot(),
            CurrentTaskStatus::Active(ref task) if task.state == TaskExecutionState::Running
        ) {
            assert!(
                std::time::Instant::now() < running_deadline,
                "tool call never became actually Running"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let cancel_started = std::time::Instant::now();
        let cancel = public_tool_call(
            pep.port(),
            &session,
            32,
            "task_control",
            json!({"action":"cancel"}),
        );
        assert!(
            cancel_started.elapsed() < Duration::from_secs(2),
            "task_control cancel blocked behind the facade execution mutex"
        );
        assert_eq!(
            cancel.body["result"]["structuredContent"]["data"]["state"], "idle",
            "{:#?}",
            cancel.body
        );
        assert!(
            cancel.body["result"]["structuredContent"]["data"]["cancelled_requests"]
                .as_u64()
                .is_some_and(|count| count >= 1),
            "{:#?}",
            cancel.body
        );

        let result = call.join().expect("tools/call client thread");
        assert!(
            call_started.elapsed() < Duration::from_secs(5),
            "task_control cancellation did not interrupt the long command"
        );
        assert!(result.body.get("result").is_some() || result.body.get("error").is_some());

        let mut coding = pep
            .stop()
            .expect("PEP stop after task_control cancellation");
        coding
            .stop()
            .expect("MCP Job stop after task_control cancellation");
        assert_eq!(coding.active_processes().unwrap(), 0);
        drop(coding);
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn edit_task_control_cancel_reaches_running_filesystem_hash() {
        let root = repo_root();
        let workspace = temp_workspace();
        let large = workspace.join("large-fs-cancel.bin");
        fs::File::create(&large)
            .unwrap()
            .set_len(8 * 1024 * 1024 * 1024)
            .unwrap();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Edit)
            .expect("PEP listener ready");
        let initialized = initialize(pep.port(), 330);
        let session = initialized.session.expect("downstream MCP session");
        assert_eq!(
            post(
                pep.port(),
                Some(&session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );

        let port = pep.port();
        let call_session = session.clone();
        let call_started = Instant::now();
        let call = thread::spawn(move || {
            post_with_read_timeout(
                port,
                Some(&call_session),
                &json!({
                    "jsonrpc":"2.0",
                    "id":"filesystem-cancel-me",
                    "method":"tools/call",
                    "params":{
                        "name":"filesystem",
                        "arguments":{"action":"hash","path":"large-fs-cancel.bin"}
                    }
                }),
                Duration::from_secs(6),
            )
        });

        let running_deadline = Instant::now() + Duration::from_secs(3);
        while !matches!(
            pep.current_task_projection().actual_snapshot(),
            CurrentTaskStatus::Active(ref task) if task.state == TaskExecutionState::Running
        ) {
            assert!(
                Instant::now() < running_deadline,
                "filesystem hash never became actually Running"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let cancel_started = Instant::now();
        let cancel = public_tool_call(
            pep.port(),
            &session,
            331,
            "task_control",
            json!({"action":"cancel"}),
        );
        assert!(
            cancel_started.elapsed() < Duration::from_secs(2),
            "filesystem task_control cancel blocked"
        );
        assert_eq!(
            cancel.body["result"]["structuredContent"]["data"]["state"], "idle",
            "{:#?}",
            cancel.body
        );
        assert!(
            cancel.body["result"]["structuredContent"]["data"]["cancelled_requests"]
                .as_u64()
                .is_some_and(|count| count >= 1),
            "{:#?}",
            cancel.body
        );

        let result = call.join().expect("filesystem tools/call client thread");
        assert!(
            call_started.elapsed() < Duration::from_secs(5),
            "filesystem cancellation did not interrupt the large hash"
        );
        assert_eq!(
            result.body["result"]["structuredContent"]["error"]["code"], "ProcessCancelled",
            "{:#?}",
            result.body
        );

        let mut coding = pep.stop().expect("PEP stop after filesystem cancellation");
        coding
            .stop()
            .expect("MCP Job stop after filesystem cancellation");
        assert_eq!(coding.active_processes().unwrap(), 0);
        drop(coding);
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn task_control_cancel_owns_detached_public_command_session() {
        let root = repo_root();
        let workspace = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("PEP listener ready");
        let initialized = initialize(pep.port(), 321);
        let session = initialized.session.expect("downstream MCP session");
        assert_eq!(
            post(
                pep.port(),
                Some(&session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );

        let started = Instant::now();
        let running = public_tool_call(
            pep.port(),
            &session,
            322,
            "exec_command",
            json!({
                "command":"Start-Sleep -Seconds 10; Write-Output SHOULD_NOT_COMPLETE",
                "shell":"windows_powershell",
                "yield_time_ms":0,
                "timeout_ms":20000,
                "max_output_bytes":4096
            }),
        );
        assert_eq!(
            running.body["result"]["structuredContent"]["data"]["status"], "running",
            "{:#?}",
            running.body
        );
        let public_session = running.body["result"]["structuredContent"]["data"]["session_id"]
            .as_str()
            .expect("detached public session")
            .to_string();

        let cancel_started = Instant::now();
        let cancel = public_tool_call(
            pep.port(),
            &session,
            323,
            "task_control",
            json!({"action":"cancel"}),
        );
        assert!(
            cancel_started.elapsed() < Duration::from_secs(2),
            "detached task cancellation blocked"
        );
        assert_eq!(
            cancel.body["result"]["structuredContent"]["data"]["state"], "idle",
            "{:#?}",
            cancel.body
        );
        assert!(
            cancel.body["result"]["structuredContent"]["data"]["cancelled_requests"]
                .as_u64()
                .is_some_and(|count| count >= 1),
            "{:#?}",
            cancel.body
        );

        let replay = public_tool_call(
            pep.port(),
            &session,
            324,
            "command_control",
            json!({"action":"poll","session_id":public_session,"wait_ms":100}),
        );
        assert_eq!(
            replay.body["result"]["structuredContent"]["error"]["code"], "ProcessCancelled",
            "{:#?}",
            replay.body
        );
        assert_eq!(
            replay.body["result"]["structuredContent"]["data"]["status"],
            "cancelled"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "detached command ran near its natural duration"
        );

        let task = public_tool_call(
            pep.port(),
            &session,
            325,
            "task_control",
            json!({"action":"get"}),
        );
        assert_eq!(
            task.body["result"]["structuredContent"]["data"]["last_terminal_command"]["status"],
            "cancelled",
            "{:#?}",
            task.body
        );
        assert_eq!(
            task.body["result"]["structuredContent"]["data"]["last_terminal_command"]["error_code"],
            "ProcessCancelled"
        );
        assert_eq!(
            pep.current_task_projection().actual_snapshot(),
            CurrentTaskStatus::Idle
        );

        let mut coding = pep.stop().expect("PEP stop after detached cancellation");
        coding
            .stop()
            .expect("MCP Job stop after detached cancellation");
        assert_eq!(coding.active_processes().unwrap(), 0);
        drop(coding);
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn public_timeout_converges_without_windows_ctrl_break_debug_mode() {
        let root = repo_root();
        let workspace = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("PEP listener ready");
        let initialized = initialize(pep.port(), 326);
        let session = initialized.session.expect("downstream MCP session");
        assert_eq!(
            post(
                pep.port(),
                Some(&session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );

        let started = Instant::now();
        let timed_out = public_tool_call(
            pep.port(),
            &session,
            327,
            "exec_command",
            json!({
                "command":"Start-Sleep -Seconds 10; Write-Output SHOULD_NOT_COMPLETE",
                "shell":"windows_powershell",
                "yield_time_ms":10000,
                "timeout_ms":300,
                "max_output_bytes":4096
            }),
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(1800),
            "300ms timeout converged too slowly: {elapsed:?}; body={:#?}",
            timed_out.body
        );
        assert_eq!(
            timed_out.body["result"]["isError"], true,
            "{:#?}",
            timed_out.body
        );
        assert_eq!(
            timed_out.body["result"]["structuredContent"]["error"]["code"], "ProcessTimedOut",
            "{:#?}",
            timed_out.body
        );
        assert_eq!(
            timed_out.body["result"]["structuredContent"]["data"]["status"], "timed_out",
            "{:#?}",
            timed_out.body
        );
        let rendered = serde_json::to_string(&timed_out.body).unwrap();
        assert!(
            !rendered.contains("Entering debug mode"),
            "Windows timeout leaked CTRL_BREAK PowerShell debug behavior: {rendered}"
        );

        let mut coding = pep.stop().expect("PEP stop after timeout");
        coding.stop().expect("MCP Job stop after timeout");
        assert_eq!(coding.active_processes().unwrap(), 0);
        drop(coding);
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn elevated_exec_is_broker_only_mode_gated_cancelable_and_secret_safe() {
        let root = repo_root();
        let workspace = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let fake = Arc::new(FakePrivilegedExecution::active());
        let privileged: Arc<dyn PrivilegedExecution> = fake.clone();
        let pep = PolicyEnforcementRuntime::start_with_privilege(
            coding,
            policy(&root),
            PermissionMode::Elevated,
            privileged,
        )
        .expect("PEP with privileged route ready");
        let initialized = initialize(pep.port(), 300);
        let mut session = initialized.session.expect("downstream MCP session");

        let tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":301,"method":"tools/list","params":{}}),
        );
        let elevated_count = tools.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|tool| tool["name"] == "elevated_exec")
            .count();
        assert_eq!(elevated_count, 1);
        let elevated_tool = tools.body["result"]["tools"]
            .as_array()
            .and_then(|tools| tools.iter().find(|tool| tool["name"] == "elevated_exec"))
            .expect("elevated_exec tool definition");
        assert_eq!(elevated_tool["inputSchema"]["type"], "object");
        assert!(elevated_tool["inputSchema"]["properties"]["operation"].is_object());
        assert!(elevated_tool["inputSchema"]["properties"]["shell"].is_object());
        assert!(elevated_tool["inputSchema"]["properties"]["action"].is_object());
        assert!(elevated_tool["inputSchema"]["properties"]["program"].is_object());
        assert!(elevated_tool["outputSchema"]["oneOf"].is_array());
        let elevated_error_schema = elevated_tool["outputSchema"]["oneOf"]
            .as_array()
            .and_then(|branches| {
                branches
                    .iter()
                    .find(|branch| branch["properties"]["ok"]["const"] == Value::Bool(false))
            })
            .expect("elevated_exec exposes common error envelope branch");
        for field in [
            "ok",
            "state",
            "summary",
            "task_id",
            "warnings",
            "next_step",
            "output_refs",
            "data",
            "error",
        ] {
            assert!(
                elevated_error_schema["properties"][field].is_object(),
                "elevated_exec error output schema lost {field}"
            );
        }
        for field in [
            "code",
            "error_code",
            "phase",
            "cause",
            "http_status",
            "message",
            "retryable",
            "rule_category",
            "remediation",
        ] {
            assert!(
                elevated_error_schema["properties"]["error"]["properties"][field].is_object(),
                "elevated_exec error diagnostics schema lost {field}"
            );
        }

        fake.set_state(PrivilegeState::AwaitingUac);
        let awaiting_tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":306,"method":"tools/list","params":{}}),
        );
        assert!(
            awaiting_tools.body["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "elevated_exec")
        );
        fake.set_state(PrivilegeState::Active {
            broker_generation: crate::state::GenerationId::new(77),
        });
        let active_tools_same_session = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":3063,"method":"tools/list","params":{}}),
        );
        assert_eq!(active_tools_same_session.status, 200);
        assert!(
            active_tools_same_session.body["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "elevated_exec")
        );

        let secret = "LB012_SYNTHETIC_PEP_SECRET";
        let reviewed_program = super::super::policy::reviewed_elevated_program()
            .expect("reviewed Windows diagnostic exists")
            .to_string_lossy()
            .into_owned();
        fs::write(workspace.join("whoami.exe"), b"untrusted same-name binary").unwrap();
        for (index, arguments) in [
            json!({"program":"C:/Windows/System32/cmd.exe","args":["/c","whoami"],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
            json!({"program":"C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe","args":["-Command",format!("Set-Content C:/ProgramData/LocalBridge/settings.json {secret}")],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
            json!({"program":"C:/Windows/System32/reg.exe","args":["add","HKLM\\Software\\LocalBridge"],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
            json!({"program":workspace.join("whoami.exe").to_string_lossy(),"args":["/user"],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
        ].into_iter().enumerate() {
            let denied = post(
                pep.port(),
                Some(&session),
                &json!({
                    "jsonrpc":"2.0","id":310 + index as u64,"method":"tools/call",
                    "params":{"name":"elevated_exec","arguments":arguments}
                }),
            );
            assert_tool_error(&denied, "PrivilegedRouteUnavailable");
            let structured = denied.body["result"]["structuredContent"]
                .as_object()
                .expect("elevated denial has structuredContent");
            let allowed = elevated_error_schema["properties"]
                .as_object()
                .expect("elevated error schema properties");
            assert!(
                structured.keys().all(|field| allowed.contains_key(field)),
                "elevated denial contains a top-level field rejected by outputSchema: {structured:?}"
            );
            let error = structured["error"]
                .as_object()
                .expect("elevated denial has typed error");
            let allowed_error = elevated_error_schema["properties"]["error"]["properties"]
                .as_object()
                .expect("elevated diagnostic schema properties");
            assert!(
                error.keys().all(|field| allowed_error.contains_key(field)),
                "elevated denial diagnostic contains a field rejected by outputSchema: {error:?}"
            );
            assert_eq!(error["error_code"], "Denied");
            assert_eq!(error["phase"], "policy");
            assert!(!denied.body.to_string().contains(secret));
            assert_eq!(fake.start_count(), 0, "unreviewed elevated request reached Broker");
        }

        let port = pep.port();
        let call_session = session.clone();
        let call_program = reviewed_program.clone();
        let call = thread::spawn(move || {
            post(
                port,
                Some(&call_session),
                &json!({
                    "jsonrpc":"2.0",
                    "id":"elevated-cancel",
                    "method":"tools/call",
                    "params":{
                        "name":"elevated_exec",
                        "arguments":{
                            "program":call_program,
                            "args":["/user"],
                            "workdir":null,
                            "timeout_ms":10000,
                            "max_output_bytes":4096
                        }
                    }
                }),
            )
        });
        let running_deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            match pep.current_task_projection().snapshot() {
                CurrentTaskStatus::Active(ref task)
                    if task.kind == TaskKind::ElevatedOperation
                        && task.state == TaskExecutionState::Running =>
                {
                    assert_eq!(task.summary, SafeTaskSummary::Omitted);
                    assert!(!format!("{task:?}").contains(secret));
                    break;
                }
                _ => {}
            }
            assert!(
                std::time::Instant::now() < running_deadline,
                "elevated call never projected Running"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fake.start_count(), 1);
        let cancelled = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0",
                "method":"notifications/cancelled",
                "params":{"requestId":"elevated-cancel","reason":"LB-012 test"}
            }),
        );
        assert_eq!(cancelled.status, 202);
        let cancelled_result = call.join().unwrap();
        assert_eq!(
            cancelled_result.body["result"]["structuredContent"]["outcome"],
            "cancelled"
        );
        assert_eq!(cancelled_result.body["result"]["isError"], true);
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ElevatedOperation,
                state: TaskExecutionState::Cancelled,
                ..
            })
        ));
        thread::sleep(Duration::from_millis(540));
        let cancelled_timing = pep.current_task_projection().timing_snapshot();
        assert_eq!(cancelled_timing.status, CurrentTaskStatus::Idle);
        assert_eq!(
            cancelled_timing.last_tool.as_ref().map(|tool| tool.kind),
            Some(TaskKind::ElevatedOperation)
        );

        pep.set_permission_mode(PermissionMode::Full);
        let full_tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":307,"method":"tools/list","params":{}}),
        );
        assert_eq!(full_tools.status, 200);
        assert!(
            full_tools.body["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "elevated_exec")
        );
        let full_denied = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":302,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "program":reviewed_program.clone(),"args":["/user"],"workdir":null,
                    "timeout_ms":1000,"max_output_bytes":1024
                }}
            }),
        );
        assert_tool_error(&full_denied, "PrivilegedRouteUnavailable");
        assert_eq!(fake.start_count(), 1);
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ElevatedOperation,
                state: TaskExecutionState::Blocked,
                ..
            })
        ));
        thread::sleep(Duration::from_millis(540));
        assert_eq!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Idle
        );

        pep.set_permission_mode(PermissionMode::Edit);
        let stale_full_for_edit = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":3021,"method":"tools/list","params":{}}),
        );
        assert_eq!(
            stale_full_for_edit.status, 404,
            "core Edit/Full catalog difference still invalidates stale sessions"
        );
        session = initialize(pep.port(), 3022)
            .session
            .expect("Edit reinitialize");
        let edit_tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":3023,"method":"tools/list","params":{}}),
        );
        assert!(
            edit_tools.body["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "elevated_exec")
        );
        let edit_denied = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":3024,"method":"tools/call","params":{"name":"elevated_exec","arguments":{
                "program":reviewed_program.clone(),"args":["/user"],"workdir":null,"timeout_ms":1000,"max_output_bytes":1024
            }}}),
        );
        assert_tool_error(&edit_denied, "PrivilegedRouteUnavailable");
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ElevatedOperation,
                state: TaskExecutionState::Blocked,
                ..
            })
        ));
        thread::sleep(Duration::from_millis(540));
        assert_eq!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Idle
        );

        pep.set_permission_mode(PermissionMode::Elevated);
        fake.set_state(PrivilegeState::AwaitingUac);
        let stale_edit_session = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":3030,"method":"tools/list","params":{}}),
        );
        assert_eq!(stale_edit_session.status, 404);
        session = initialize(pep.port(), 3031)
            .session
            .expect("Elevated reinitialize after Edit core catalog change");
        let elevated_awaiting_tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":3032,"method":"tools/list","params":{}}),
        );
        assert!(
            elevated_awaiting_tools.body["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "elevated_exec")
        );
        let awaiting = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":303,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "program":reviewed_program.clone(),"args":["/user"],"workdir":null,
                    "timeout_ms":1000,"max_output_bytes":1024
                }}
            }),
        );
        assert_tool_error(&awaiting, "ElevationRequired");
        assert_eq!(fake.start_count(), 1);
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ElevatedOperation,
                state: TaskExecutionState::AwaitingAuthorization,
                ..
            })
        ));
        thread::sleep(Duration::from_millis(540));
        assert_eq!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Idle
        );

        let control_plane = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":304,"method":"tools/call",
                "params":{"name":"request_permissions","arguments":{"permission":"admin"}}
            }),
        );
        assert_tool_error(&control_plane, "CapabilityDenied");
        thread::sleep(Duration::from_millis(540));
        assert_eq!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Idle
        );

        fake.set_state(PrivilegeState::Active {
            broker_generation: crate::state::GenerationId::new(78),
        });
        let active_tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":3041,"method":"tools/list","params":{}}),
        );
        assert_eq!(active_tools.status, 200);
        assert!(
            active_tools.body["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "elevated_exec")
        );
        fake.complete.store(true, Ordering::Release);
        let completed_started = Instant::now();
        let completed = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":305,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "program":reviewed_program.clone(),"args":["/user"],"workdir":null,
                    "timeout_ms":1000,"max_output_bytes":1024
                }}
            }),
        );
        let completed_round_trip = completed_started.elapsed();
        assert_eq!(
            completed.body["result"]["structuredContent"]["outcome"],
            "completed"
        );
        assert_eq!(
            completed.body["result"]["structuredContent"]["stdout"],
            "LB012_FAKE_PRIVILEGED_OK"
        );
        assert_eq!(
            completed.body["result"]["structuredContent"]["stderr"],
            "LB012_FAKE_PRIVILEGED_ERR"
        );
        assert_eq!(
            completed.body["result"]["content"][0]["text"],
            "LB012_FAKE_PRIVILEGED_OK\nLB012_FAKE_PRIVILEGED_ERR"
        );
        assert_eq!(fake.start_count(), 2);
        assert!(
            completed_round_trip < Duration::from_millis(500),
            "UI retention must not delay Broker response: {completed_round_trip:?}"
        );
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ElevatedOperation,
                ..
            })
        ));
        thread::sleep(Duration::from_millis(540));
        let completed_timing = pep.current_task_projection().timing_snapshot();
        assert_eq!(completed_timing.status, CurrentTaskStatus::Idle);
        assert_eq!(
            completed_timing.last_tool.as_ref().map(|tool| tool.kind),
            Some(TaskKind::ElevatedOperation)
        );

        fake.complete.store(false, Ordering::Release);
        let first_port = pep.port();
        let first_session = session.clone();
        let first_program = reviewed_program.clone();
        let first = thread::spawn(move || {
            post(
                first_port,
                Some(&first_session),
                &json!({
                    "jsonrpc":"2.0","id":"serialized-first","method":"tools/call",
                    "params":{"name":"elevated_exec","arguments":{
                        "program":first_program,"args":["/user"],"workdir":null,
                        "timeout_ms":5000,"max_output_bytes":1024
                    }}
                }),
            )
        });
        let first_deadline = std::time::Instant::now() + Duration::from_secs(3);
        while fake.start_count() != 3 {
            assert!(
                std::time::Instant::now() < first_deadline,
                "first serialized elevated call did not start"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(ref task)
                if task.kind == TaskKind::ElevatedOperation && task.state == TaskExecutionState::Running
        ));

        let second_port = pep.port();
        let second_session = session.clone();
        let second_program = reviewed_program.clone();
        let second = thread::spawn(move || {
            post(
                second_port,
                Some(&second_session),
                &json!({
                    "jsonrpc":"2.0","id":"serialized-second","method":"tools/call",
                    "params":{"name":"elevated_exec","arguments":{
                        "program":second_program,"args":["/groups"],"workdir":null,
                        "timeout_ms":5000,"max_output_bytes":1024
                    }}
                }),
            )
        });
        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            fake.start_count(),
            3,
            "second elevated call bypassed single execution gate"
        );
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(ref task) if task.state == TaskExecutionState::Running
        ));
        fake.complete.store(true, Ordering::Release);
        assert_eq!(
            first.join().unwrap().body["result"]["structuredContent"]["outcome"],
            "completed"
        );
        assert_eq!(
            second.join().unwrap().body["result"]["structuredContent"]["outcome"],
            "completed"
        );
        assert_eq!(fake.start_count(), 4);
        assert!(matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ElevatedOperation,
                ..
            })
        ));
        thread::sleep(Duration::from_millis(540));
        let first_serialized_retired = pep.current_task_projection().timing_snapshot();
        assert!(matches!(
            first_serialized_retired.status,
            CurrentTaskStatus::Active(CurrentTask {
                kind: TaskKind::ElevatedOperation,
                ..
            })
        ));
        assert_eq!(
            first_serialized_retired
                .last_tool
                .as_ref()
                .map(|tool| tool.kind),
            Some(TaskKind::ElevatedOperation)
        );
        thread::sleep(Duration::from_millis(540));
        let serialized_timing = pep.current_task_projection().timing_snapshot();
        assert_eq!(serialized_timing.status, CurrentTaskStatus::Idle);
        assert_eq!(
            serialized_timing.last_tool.as_ref().map(|tool| tool.kind),
            Some(TaskKind::ElevatedOperation)
        );

        let mut coding = pep.stop().expect("PEP stop after privileged routing");
        coding.stop().expect("MCP stop after privileged routing");
        drop(coding);
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn schema43_filesystem_routes_outside_workspace_only_through_active_broker() {
        let root = repo_root();
        let workspace = temp_workspace();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let outside =
            std::env::temp_dir().join(format!("lb43-fs-outside-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&outside).unwrap();
        let outside_file = outside.join("outside.txt");
        fs::write(&outside_file, b"outside").unwrap();

        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let fake = Arc::new(FakePrivilegedExecution::active());
        let privileged: Arc<dyn PrivilegedExecution> = fake.clone();
        let pep = PolicyEnforcementRuntime::start_with_privilege(
            coding,
            policy(&root),
            PermissionMode::Full,
            privileged,
        )
        .expect("PEP with schema43 filesystem route ready");

        let full_session = initialize(pep.port(), 4300).session.unwrap();
        let full_denied = public_tool_call(
            pep.port(),
            &full_session,
            4301,
            "filesystem",
            json!({"action":"read","path":outside_file.to_string_lossy()}),
        );
        assert_tool_error(&full_denied, "WorkspaceDenied");
        assert_eq!(fake.structured_filesystem_count(), 0);

        pep.set_permission_mode(PermissionMode::Elevated);
        fake.set_state(PrivilegeState::AwaitingUac);
        let elevated_session = initialize(pep.port(), 4302).session.unwrap();
        let awaiting = public_tool_call(
            pep.port(),
            &elevated_session,
            4303,
            "filesystem",
            json!({"action":"read","path":outside_file.to_string_lossy()}),
        );
        assert_tool_error(&awaiting, "ElevationRequired");
        assert_eq!(fake.structured_filesystem_count(), 0);

        fake.set_state(PrivilegeState::Active {
            broker_generation: crate::state::GenerationId::new(78),
        });
        let elevated = public_tool_call(
            pep.port(),
            &elevated_session,
            4304,
            "filesystem",
            json!({"action":"read","path":outside_file.to_string_lossy(),"max_bytes":4096}),
        );
        assert_eq!(
            elevated.body["result"]["isError"], false,
            "{:#?}",
            elevated.body
        );
        assert_eq!(
            elevated.body["result"]["structuredContent"]["data"]["content"],
            "LB43_FAKE_ADMIN"
        );
        assert!(
            elevated.body["result"]["structuredContent"]["data"]
                .get("result_kind")
                .is_none()
        );
        assert_eq!(fake.structured_filesystem_count(), 1);

        let inside = public_tool_call(
            pep.port(),
            &elevated_session,
            4305,
            "filesystem",
            json!({"action":"read","path":"probe.txt"}),
        );
        assert_eq!(
            inside.body["result"]["isError"], false,
            "{:#?}",
            inside.body
        );
        assert!(
            inside.body["result"]["structuredContent"]["data"]["content"]
                .as_str()
                .is_some_and(|content| content.contains("LB009 PEP"))
        );
        assert_eq!(
            fake.structured_filesystem_count(),
            1,
            "workspace-contained Elevated filesystem call incorrectly used Broker"
        );

        let control_plane = public_tool_call(
            pep.port(),
            &elevated_session,
            4306,
            "filesystem",
            json!({"action":"delete","path":"C:\\ProgramData\\LocalBridge\\settings.json"}),
        );
        assert_tool_error(&control_plane, "PolicyDenied");
        assert_eq!(fake.structured_filesystem_count(), 1);

        let mut coding = pep
            .stop()
            .expect("PEP stop after schema43 filesystem routing");
        coding
            .stop()
            .expect("MCP stop after schema43 filesystem routing");
        drop(coding);
        cleanup_test_directory(&workspace);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn typed_administrator_process_shell_and_filesystem_routes_are_broker_only() {
        let root = repo_root();
        let workspace = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let fake = Arc::new(FakePrivilegedExecution::active());
        fake.complete.store(true, Ordering::Release);
        let privileged: Arc<dyn PrivilegedExecution> = fake.clone();
        let pep = PolicyEnforcementRuntime::start_with_privilege(
            coding,
            policy(&root),
            PermissionMode::Elevated,
            privileged,
        )
        .expect("PEP with typed administrator route ready");
        let initialized = initialize(pep.port(), 500);
        let session = initialized.session.expect("downstream MCP session");

        let tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":501,"method":"tools/list","params":{}}),
        );
        let schema = &tools.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "elevated_exec")
            .unwrap()["inputSchema"];
        assert_eq!(schema["type"], "object");
        assert!(schema.get("oneOf").is_none());
        assert_eq!(
            schema["properties"]["operation"]["enum"],
            json!(["process", "shell", "filesystem"])
        );
        for property in [
            "program",
            "shell",
            "action",
            "path",
            "timeout_ms",
            "max_output_bytes",
        ] {
            assert!(
                schema["properties"][property].is_object(),
                "elevated_exec schema lost {property}"
            );
        }
        let output_schema = &tools.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "elevated_exec")
            .unwrap()["outputSchema"];
        let execution_schema = &output_schema["oneOf"][1]["properties"];
        for property in [
            "stdout",
            "stderr",
            "stdout_truncated",
            "stderr_truncated",
            "output_refs",
        ] {
            assert!(
                execution_schema[property].is_object(),
                "elevated_exec output schema lost {property}"
            );
        }

        let reviewed_program = super::super::policy::reviewed_elevated_program()
            .expect("trusted System32 diagnostic exists")
            .to_string_lossy()
            .into_owned();
        let process = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":502,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "operation":"process","program":reviewed_program,"args":["/user"],
                    "workdir":null,"timeout_ms":1000,"max_output_bytes":4096
                }}
            }),
        );
        assert_eq!(
            process.body["result"]["structuredContent"]["outcome"],
            "completed"
        );
        assert_eq!(
            process.body["result"]["structuredContent"]["stdout"],
            "LB012_FAKE_PRIVILEGED_OK"
        );
        assert_eq!(
            process.body["result"]["structuredContent"]["stderr"],
            "LB012_FAKE_PRIVILEGED_ERR"
        );
        assert_eq!(fake.start_count(), 1);

        let shell = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":503,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "operation":"shell","shell":"cmd","command":"whoami /user",
                    "workdir":"C:\\Windows\\Temp","timeout_ms":1000,"max_output_bytes":4096
                }}
            }),
        );
        assert_eq!(
            shell.body["result"]["structuredContent"]["outcome"],
            "completed"
        );
        assert_eq!(fake.start_count(), 2);

        let shell_path_denied = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":504,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "operation":"process","program":"C:\\Windows\\System32\\cmd.exe",
                    "args":["/c","whoami"],"workdir":null,
                    "timeout_ms":1000,"max_output_bytes":4096
                }}
            }),
        );
        assert_tool_error(&shell_path_denied, "PrivilegedRouteUnavailable");
        assert_eq!(fake.start_count(), 2);

        let opaque_helper = std::env::current_exe().expect("test helper executable");
        let opaque_helper_denied = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":5041,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "operation":"process","program":opaque_helper.to_string_lossy(),
                    "args":[],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096
                }}
            }),
        );
        assert_tool_error(&opaque_helper_denied, "PrivilegedRouteUnavailable");
        assert_eq!(
            fake.start_count(),
            2,
            "opaque administrator helper must be denied before Broker dispatch"
        );

        let filesystem = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":505,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "operation":"filesystem","action":"read_file","path":"C:\\Windows\\win.ini",
                    "destination":null,"content_base64":null,"recursive":false
                }}
            }),
        );
        assert_eq!(
            filesystem.body["result"]["structuredContent"]["operation"],
            "filesystem"
        );
        assert_eq!(
            filesystem.body["result"]["structuredContent"]["result"]["path"],
            "C:\\Windows\\win.ini"
        );
        assert_eq!(
            fake.start_count(),
            2,
            "filesystem incorrectly used process execution"
        );

        let control_plane_denied = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":506,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "operation":"filesystem","action":"delete","path":"C:\\ProgramData\\LocalBridge",
                    "destination":null,"content_base64":null,"recursive":true
                }}
            }),
        );
        assert_tool_error(&control_plane_denied, "PrivilegedRouteUnavailable");
        assert_eq!(fake.start_count(), 2);

        let retained = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":507,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "operation":"process","program":reviewed_program,"args":["/user"],
                    "workdir":null,"timeout_ms":1000,"max_output_bytes":20000
                }}
            }),
        );
        let retained_data = &retained.body["result"]["structuredContent"];
        assert_eq!(retained_data["stdout"].as_str().unwrap().len(), 8 * 1024);
        assert_eq!(retained_data["stderr"].as_str().unwrap().len(), 8 * 1024);
        assert_eq!(retained_data["stdout_truncated"], true);
        assert_eq!(retained_data["stderr_truncated"], true);
        let stdout_ref = retained_data["output_refs"]["stdout"].as_str().unwrap();
        let stderr_ref = retained_data["output_refs"]["stderr"].as_str().unwrap();
        assert!(stdout_ref.starts_with("lb-output-"));
        assert!(stderr_ref.starts_with("lb-output-"));
        let stdout_page = public_tool_call(
            pep.port(),
            &session,
            508,
            "command_control",
            json!({"action":"read","output_ref":stdout_ref,"stream":"stdout","offset":0,"limit":20000}),
        );
        assert_eq!(
            stdout_page.body["result"]["structuredContent"]["data"]["content"]
                .as_str()
                .unwrap()
                .len(),
            9_000
        );
        let stderr_page = public_tool_call(
            pep.port(),
            &session,
            509,
            "command_control",
            json!({"action":"read","output_ref":stderr_ref,"stream":"stderr","offset":0,"limit":20000}),
        );
        assert_eq!(
            stderr_page.body["result"]["structuredContent"]["data"]["content"]
                .as_str()
                .unwrap()
                .len(),
            9_000
        );
        assert_eq!(fake.start_count(), 3);

        let mut coding = pep
            .stop()
            .expect("PEP stop after typed administrator routing");
        coding
            .stop()
            .expect("MCP stop after typed administrator routing");
        drop(coding);
        cleanup_test_directory(&workspace);
    }
}
