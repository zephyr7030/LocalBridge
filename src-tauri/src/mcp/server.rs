use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, TryLockError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

#[cfg(test)]
use crate::privilege::PrivilegedFilesystemResult;
use crate::privilege::{
    ElevatedExecOutcome, ElevatedExecSpec, PrivilegedExecError, PrivilegedExecution,
    PrivilegedFilesystemSpec,
};
use crate::state::{
    Capability, CurrentTask, CurrentTaskStatus, CurrentTaskTiming, LastToolTiming, PermissionMode,
    PrivilegeState, SafeTaskSummary, TaskExecutionState, TaskKind,
};

use super::facade::{
    AgentFacade, CodingToolsRuntimeAdapter, FacadeCallError, FacadeDenied, FacadeError,
    FacadeErrorCode, V1_CORE_TOOL_NAMES,
};
use super::http::McpCancellationClient;
use super::policy::CapabilityPolicy;
use super::runtime::CodingToolsRuntime;
use super::shell::{ShellExecutionSpec, ShellExecutor, ShellSelector};
use super::task_state::CommandTaskStateStore;

const CURRENT_PROTOCOL_VERSION: &str = "2025-11-25";
const COMPATIBLE_PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);
const ACCEPT_IDLE: Duration = Duration::from_millis(10);
const MIN_TASK_PRESENTATION: Duration = Duration::from_millis(500);
const MAX_CONNECTION_WORKERS: usize = 32;
static SESSION_GENERATION: AtomicU64 = AtomicU64::new(1);
static PRIVILEGED_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
struct McpSession {
    protocol: String,
    tool_catalog_signature: String,
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
    privileged: Option<&'a Arc<dyn PrivilegedExecution>>,
    privileged_requests: &'a Mutex<Vec<(Value, String)>>,
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

struct TaskControlContext<'a> {
    public_policy: &'a RwLock<CapabilityPolicy>,
    cancellation: &'a McpCancellationClient,
    current_task: &'a CurrentTaskProjection,
    task_state: &'a CommandTaskStateStore,
    active_requests: &'a Mutex<Vec<Value>>,
    privileged: Option<&'a Arc<dyn PrivilegedExecution>>,
    privileged_requests: &'a Mutex<Vec<(Value, String)>>,
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
    UpstreamSessionUnavailable,
    ThreadSpawnFailed,
    ThreadTerminated,
}

impl fmt::Display for PolicyEnforcementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindFailed => f.write_str("policy enforcement loopback bind failed"),
            Self::UpstreamSessionUnavailable => {
                f.write_str("policy enforcement upstream MCP session is unavailable")
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
    guard: Option<Arc<Mutex<AgentFacade<CodingToolsRuntimeAdapter>>>>,
    public_policy: Arc<RwLock<CapabilityPolicy>>,
    sessions: Arc<Mutex<HashMap<String, McpSession>>>,
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
        let guard = AgentFacade::from_coding_runtime(coding_runtime, policy)
            .map_err(|_| PolicyEnforcementError::UpstreamSessionUnavailable)?;
        let task_state = guard.command_task_state();
        let cancellation = guard
            .cancellation_client()
            .map_err(|_| PolicyEnforcementError::UpstreamSessionUnavailable)?;
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
            guard: Some(guard),
            public_policy,
            sessions,
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
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        Ok(())
    }

    pub fn current_task_projection(&self) -> CurrentTaskProjection {
        self.current_task.clone()
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
    let privileged_requests = Arc::new(Mutex::new(Vec::<(Value, String)>::new()));
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
                let _ = write_empty(&mut stream, 503, None);
            }
            Ok((stream, _)) => {
                let worker_guard = Arc::clone(&guard);
                let worker_policy = Arc::clone(&public_policy);
                let worker_mode = Arc::clone(&permission_mode);
                let worker_task = current_task.clone();
                let worker_task_state = task_state.clone();
                let worker_sessions = Arc::clone(&sessions);
                let worker_active = Arc::clone(&active_requests);
                let worker_privileged = privileged.as_ref().map(Arc::clone);
                let worker_privileged_requests = Arc::clone(&privileged_requests);
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
                            privileged: worker_privileged.as_ref(),
                            privileged_requests: &worker_privileged_requests,
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
        privileged,
        privileged_requests,
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
        Err(status) => return write_empty(&mut stream, status, None),
    };

    if request.path != "/mcp" {
        return write_empty(&mut stream, 404, None);
    }
    if request.method == "DELETE" {
        let Some(session) = request.header("mcp-session-id") else {
            return write_empty(&mut stream, 400, None);
        };
        if sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(session)
            .is_some()
        {
            return write_empty(&mut stream, 204, None);
        }
        return write_empty(&mut stream, 404, None);
    }
    if request.method != "POST" {
        return write_empty(&mut stream, 405, None);
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
            sessions.clear();
            sessions.insert(
                session.clone(),
                McpSession {
                    protocol: protocol.to_string(),
                    tool_catalog_signature,
                },
            );
        }
        return write_rpc_result(
            &mut stream,
            id,
            json!({
                "protocolVersion": protocol,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "localbridge-mcp-guard", "version": env!("CARGO_PKG_VERSION")}
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
        return write_empty(&mut stream, 404, None);
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
        return write_empty(&mut stream, 404, None);
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
                if let Some(broker_request_id) =
                    privileged_request_id(privileged_requests, request_id)
                {
                    let Some(privileged) = privileged else {
                        return write_empty(&mut stream, 503, Some(session));
                    };
                    if privileged.cancel_execute(broker_request_id).is_err() {
                        return write_empty(&mut stream, 503, Some(session));
                    }
                    return write_empty(&mut stream, 202, Some(session));
                }
                if cancellation.cancel_request(request_id).is_err() {
                    return write_empty(&mut stream, 503, Some(session));
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
            let guard = guard
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let result = effective_tool_catalog(&guard, mode);
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
                return write_empty(&mut stream, 503, Some(session));
            }
            let mode = *permission_mode
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if name == "elevated_exec" {
                return handle_elevated_exec(
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
            }
            if name == "task_control" {
                return handle_task_control(
                    &mut stream,
                    id,
                    session,
                    mode,
                    arguments,
                    TaskControlContext {
                        public_policy,
                        cancellation,
                        current_task,
                        task_state,
                        active_requests,
                        privileged,
                        privileged_requests,
                    },
                );
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
                return write_empty(&mut stream, 503, Some(session));
            }
            let result = guard.call_tool(mode, name, arguments, Some(&id), |status| {
                current_task.project(status);
            });
            remove_active_request(active_requests, &id);
            match result {
                Ok(mut result) => {
                    if name == "workspace_context" {
                        enrich_workspace_context_privilege(&mut result, mode, privileged);
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
    data.insert("privilege_state".into(), Value::String(privilege_state.into()));
    data.insert("broker_state".into(), Value::String(broker_state.into()));
    data.insert("uac_state".into(), Value::String(uac_state.into()));
    data.insert(
        "administrator_token_available".into(),
        Value::Bool(administrator_token_available),
    );
    data.insert("selected_route".into(), Value::String("ordinary".into()));
    if let Some(capabilities) = data
        .get_mut("capabilities")
        .and_then(Value::as_object_mut)
    {
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
        public_policy,
        cancellation,
        current_task,
        task_state,
        active_requests,
        privileged,
        privileged_requests,
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

    let action = arguments.get("action").and_then(Value::as_str).ok_or(())?;
    let before = current_task.actual_snapshot();
    let data = match action {
        "get" => task_control_snapshot_with_terminal(&before, task_state),
        "cancel" => {
            let active = active_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let mut cancelled = 0u64;
            for request_id in active {
                let result = if let Some(broker_request_id) =
                    privileged_request_id(privileged_requests, &request_id)
                {
                    privileged
                        .ok_or(())?
                        .cancel_execute(broker_request_id)
                        .map_err(|_| ())
                } else {
                    cancellation.cancel_request(&request_id).map_err(|_| ())
                };
                if result.is_ok() {
                    cancelled = cancelled.saturating_add(1);
                }
            }
            if cancelled > 0 {
                json!({"state":"cancel_requested","cancelled_requests":cancelled})
            } else {
                let after = current_task.actual_snapshot();
                let mut data = task_control_snapshot_with_terminal(&after, task_state);
                if let Some(object) = data.as_object_mut() {
                    object.insert("cancelled_requests".into(), Value::from(0));
                }
                data
            }
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
    write_rpc_result(
        stream,
        id,
        json!({
            "content":[{"type":"text","text":"Task control completed"}],
            "structuredContent":{"ok":true,"data":data},
            "isError":false
        }),
        Some(session),
    )
}

fn task_control_snapshot_with_terminal(
    status: &CurrentTaskStatus,
    task_state: &CommandTaskStateStore,
) -> Value {
    let mut data = task_control_snapshot(status);
    if let Some(object) = data.as_object_mut() {
        let terminal = task_state.latest_terminal().map(|terminal| {
            json!({
                "task_id": terminal.owner.task_id,
                "session_id": terminal.owner.session_id,
                "status": terminal.status.as_str(),
                "exit_code": terminal.exit_code,
                "signal": terminal.signal,
                "timed_out": terminal.timed_out,
                "cancelled": terminal.cancelled,
                "output_refs": terminal.output_refs,
                "error_code": terminal.error_code
            })
        });
        object.insert(
            "last_terminal_command".into(),
            terminal.unwrap_or(Value::Null),
        );
    }
    data
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

fn effective_tool_catalog(
    guard: &AgentFacade<CodingToolsRuntimeAdapter>,
    mode: PermissionMode,
) -> Value {
    let mut result = guard.public_tools(mode);
    if guard.privileged_tool_visible(mode, "elevated_exec") {
        append_elevated_exec_tool(&mut result);
    }
    result
}

fn effective_tool_catalog_signature(
    policy: &CapabilityPolicy,
    mode: PermissionMode,
) -> String {
    let mut names = V1_CORE_TOOL_NAMES
        .iter()
        .copied()
        .filter(|name| policy.public_tool_allowed_for_list(mode, name))
        .collect::<Vec<_>>();
    if policy.privileged_tool_visible(mode, "elevated_exec") {
        names.push("elevated_exec");
    }
    serde_json::to_string(&names)
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
            "additionalProperties": false,
            "oneOf": [
                {
                    "not": {"required": ["operation"]},
                    "required": ["program", "args", "timeout_ms", "max_output_bytes"],
                    "description": "Legacy direct process form."
                },
                {
                    "properties": {"operation": {"const": "process"}},
                    "required": ["operation", "program", "args", "timeout_ms", "max_output_bytes"]
                },
                {
                    "properties": {"operation": {"const": "shell"}},
                    "required": ["operation", "shell", "command", "workdir", "timeout_ms", "max_output_bytes"]
                },
                {
                    "properties": {"operation": {"const": "filesystem"}},
                    "required": ["operation", "action", "path", "destination", "content_base64", "recursive"]
                }
            ]
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
                    "truncated":{"type":"boolean"}
                },
                "required":["outcome","exit_code","truncated"],
                "additionalProperties":false
            },
            {
                "type":"object",
                "properties":{
                    "ok":{"const":false},
                    "error":{
                        "type":"object",
                        "properties":{
                            "code":{"type":"string"},
                            "message":{"type":"string"},
                            "retryable":{"type":"boolean"}
                        },
                        "required":["code","message","retryable"],
                        "additionalProperties":false
                    }
                },
                "required":["ok","error"],
                "additionalProperties":false
            }
        ]
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
    let execution_guard = guard
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
    let response = json!({
        "content": [{"type": "text", "text": execution.output}],
        "structuredContent": {
            "outcome": outcome,
            "exit_code": execution.exit_code,
            "truncated": execution.truncated
        },
        "isError": is_error
    });
    finish_elevated_task(current_task, terminal);
    let result = write_rpc_result(stream, id, response, Some(session));
    drop(execution_guard);
    result
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

fn active_request_exists(active_requests: &Mutex<Vec<Value>>, request_id: &Value) -> bool {
    active_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .any(|active| active == request_id)
}

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

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, u16> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(431);
        }
        let count = stream.read(&mut chunk).map_err(|_| 400u16)?;
        if count == 0 {
            return Err(400);
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header_text = std::str::from_utf8(&bytes[..header_end - 4]).map_err(|_| 400u16)?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().ok_or(400u16)?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().ok_or(400u16)?.to_string();
    let path = request_parts.next().ok_or(400u16)?.to_string();
    if request_parts.next() != Some("HTTP/1.1") || request_parts.next().is_some() {
        return Err(400);
    }
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(400u16)?;
        let name = name.trim().to_string();
        let value = value.trim().to_string();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse::<usize>().map_err(|_| 400u16)?;
            if content_length > MAX_BODY_BYTES {
                return Err(413);
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(400);
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
        let count = stream.read(&mut chunk[..read_limit]).map_err(|_| 400u16)?;
        if count == 0 {
            return Err(400);
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
    write_json(
        stream,
        200,
        &json!({"jsonrpc":"2.0","id":id,"result":result}),
        session,
    )
}

fn write_rpc_error(
    stream: &mut TcpStream,
    id: Value,
    code: i64,
    message: &str,
    session: Option<&str>,
) -> Result<(), ()> {
    write_json(
        stream,
        200,
        &json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}),
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
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::runtime::{
        CodingToolsPermissionMode, CodingToolsRuntimeConfig, InternalBearer,
    };
    use super::super::task_state::{CommandOwner, CommandTerminalStatus, TerminalCommandSnapshot};

    const SYNTHETIC_BEARER: &str = "LB009_PEP_INTERNAL_BEARER_SYNTHETIC_DO_NOT_LEAK";

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
                    output: String::new(),
                    truncated: false,
                }));
            }
            if self.complete.load(Ordering::Acquire) {
                return Ok(Some(crate::privilege::ElevatedExecResult {
                    outcome: ElevatedExecOutcome::Completed,
                    exit_code: Some(0),
                    output: "LB012_FAKE_PRIVILEGED_OK".to_string(),
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
    }

    struct ClientResponse {
        status: u16,
        session: Option<String>,
        body: Value,
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

        let context = public_tool_call(pep.port(), &session, 601, "workspace_context", json!({}));
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
        let mut session = initialized.session.expect("schema28 downstream session");
        assert_eq!(
            post(
                pep.port(),
                Some(&session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );

        let provenance =
            public_tool_call(pep.port(), &session, 687, "workspace_context", json!({}));
        assert_eq!(
            provenance.body["result"]["structuredContent"]["data"]["facade_revision"], 36,
            "fresh serving instance did not identify the revision34 facade: {:#?}",
            provenance.body
        );
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
        assert_eq!(served_agent["outputSchema"]["type"], "object");
        assert_eq!(served_agent["outputSchema"]["properties"]["ok"]["type"], "boolean");
        assert_eq!(
            served_agent["outputSchema"]["properties"]["data"]["properties"]["state"]["type"],
            "string"
        );
        assert!(served_agent["outputSchema"]["properties"]["error"].is_object());
        let served_command_control = served_tools.body["result"]["tools"]
            .as_array()
            .and_then(|tools| tools.iter().find(|tool| tool["name"] == "command_control"))
            .expect("fresh serving instance exposes command_control");
        let command_control_variants = served_command_control["inputSchema"]["oneOf"]
            .as_array()
            .expect("command_control action-discriminated schema");
        let read_variant = command_control_variants
            .iter()
            .find(|variant| variant["properties"]["action"]["const"] == "read")
            .expect("command_control read schema");
        assert!(
            read_variant["required"]
                .as_array()
                .unwrap()
                .contains(&json!("output_ref"))
        );
        assert!(read_variant["properties"].get("session_id").is_none());
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

        let written = public_tool_call(
            pep.port(),
            &session,
            poll_id + 1,
            "command_control",
            json!({
                "action":"write",
                "session_id":public_session,
                "chars":"after-start\n",
                "wait_ms":1000
            }),
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

        let killed = public_tool_call(
            pep.port(),
            &session,
            poll_id + 2,
            "command_control",
            json!({"action":"kill","session_id":public_session,"signal":"TERM","wait_ms":1000}),
        );
        assert_eq!(
            killed.body["result"]["isError"], false,
            "healthy kill regressed: {:#?}",
            killed.body
        );
        assert_eq!(killed.body["result"]["structuredContent"]["ok"], true);
        assert_eq!(
            killed.body["result"]["structuredContent"]["data"]["status"],
            "cancelled"
        );
        for id in [poll_id + 3, poll_id + 4] {
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
            serde_json::to_string(durable)
                .unwrap()
                .find("PRIVATE_")
                .is_none(),
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
            (812, workspace.join("absolute.txt").to_string_lossy().into_owned()),
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
        assert_eq!(full_catalog.len(), 9);
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
        assert_eq!(edit_tool_names.len(), 6);
        assert!(edit_tool_names.contains(&"agent_workflow"));
        assert!(edit_tool_names.contains(&"elevated_exec"));
        for process_tool in ["exec_command", "command_control", "task_control"] {
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
                "edit_tools = [\"workspace_context\", \"agent_workflow\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
                "edit_tools = [\"workspace_context\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
            )
            .replace(
                "full_tools = [\"workspace_context\", \"agent_workflow\", \"exec_command\", \"command_control\", \"task_control\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
                "full_tools = [\"workspace_context\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
            )
            .replace(
                "elevated_tools = [\"workspace_context\", \"agent_workflow\", \"exec_command\", \"command_control\", \"task_control\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
                "elevated_tools = [\"workspace_context\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
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
        let stale = post(
            pep.port(),
            Some(&narrowed_session),
            &json!({"jsonrpc":"2.0","id":7,"method":"ping","params":{}}),
        );
        assert_eq!(stale.status, 404);
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
            cancel.body["result"]["structuredContent"]["data"]["state"], "cancel_requested",
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
        assert!(active_tools_same_session.body["result"]["tools"]
            .as_array().unwrap().iter().any(|tool| tool["name"] == "elevated_exec"));

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
        assert_eq!(stale_full_for_edit.status, 404, "core Edit/Full catalog difference still invalidates stale sessions");
        session = initialize(pep.port(), 3022).session.expect("Edit reinitialize");
        let edit_tools = post(
            pep.port(), Some(&session),
            &json!({"jsonrpc":"2.0","id":3023,"method":"tools/list","params":{}}),
        );
        assert!(edit_tools.body["result"]["tools"].as_array().unwrap().iter().any(|tool| tool["name"] == "elevated_exec"));
        let edit_denied = post(
            pep.port(), Some(&session),
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
        assert_eq!(pep.current_task_projection().snapshot(), CurrentTaskStatus::Idle);

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
            pep.port(), Some(&session),
            &json!({"jsonrpc":"2.0","id":3032,"method":"tools/list","params":{}}),
        );
        assert!(elevated_awaiting_tools.body["result"]["tools"].as_array().unwrap().iter().any(|tool| tool["name"] == "elevated_exec"));
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
        assert!(active_tools.body["result"]["tools"].as_array().unwrap().iter().any(|tool| tool["name"] == "elevated_exec"));
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
            completed.body["result"]["content"][0]["text"],
            "LB012_FAKE_PRIVILEGED_OK"
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
        assert_eq!(schema["oneOf"].as_array().unwrap().len(), 4);

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
