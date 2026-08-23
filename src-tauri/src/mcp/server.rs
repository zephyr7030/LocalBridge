use std::collections::BTreeSet;
#[cfg(test)]
use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, TryLockError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use crate::control_plane::command_control::{
    CommandControlAction, CommandControlError, CommandControlRequest, CommandControlResult,
    CommandKillSignal, RuntimeCommandStatus, control_command_during_work,
};
use crate::control_plane::convergence::{
    ConnectionProfile, ConvergenceSnapshot, DesiredStateOwner, EffectiveState, ObservedState,
};
#[cfg(test)]
use crate::control_plane::convergence::{DesiredState, DesiredWorkspace, ServiceIntent};
use crate::control_plane::execution_registry::{ExecutionRegistry, ExecutionRegistryError};
use crate::control_plane::owner::ControlPlane;
use crate::control_plane::request_registry::{
    ActiveRequest, RequestCancellationTarget, RequestRegistry,
};
use crate::control_plane::scheduler::{Scheduler, SchedulerAdmissionError, SchedulerLane};
use crate::control_plane::session_registry::{
    MCP_SESSION_TTL_MS, SessionInsertError, SessionReaper, SessionRecord, SessionRegistry,
};
use crate::control_plane::snapshot::TaskAggregate;
use crate::control_plane::task_registry::TaskRegistry;
use crate::control_plane::workflow_checkpoint::WorkflowCheckpointStore;
use crate::diagnostics::error::{
    ErrorDiagnostic, mcp_invalid, mcp_unavailable, mcp_unknown, transport_unavailable,
};
use crate::diagnostics::{
    record_mcp_request_error, record_mcp_request_result, record_mcp_request_start,
};
use crate::domain::{
    ErrorCategory, ExecutionRecord, ExecutionState, ExecutionTerminal, LifecycleState,
    McpSessionId, OperationError, PublicSessionId, RequestKey, RpcRequestId, TaskId, TaskRecord,
    TerminalOutcome,
};
#[cfg(test)]
use crate::privilege::{
    AdministratorFilesystemAction, AdministratorFilesystemErrorCode, AdministratorFilesystemResult,
    AdministratorFilesystemSpec, PrivilegedFilesystemResult,
};
use crate::privilege::{
    ElevatedExecOutcome, ElevatedExecSpec, PrivilegedExecError, PrivilegedExecution,
    PrivilegedFilesystemSpec,
};
use crate::state::{
    Capability, CurrentTask, CurrentTaskStatus, CurrentTaskTiming, LastToolTiming, PermissionMode,
    PrivilegeState, SafeTaskSummary, TaskExecutionState, TaskKind,
};

use super::facade::{
    AGENT_API_REVISION, AgentFacade, CodingRuntimeHealth, CodingToolsRuntimeAdapter,
    FacadeCallError, FacadeDenied, FacadeError, FacadeErrorCode, FilesystemAction,
    TaskCallIdentity, parse_filesystem_request, public_command_stderr, public_error_output_schema,
    public_safe_summary, public_task_kind, public_tools_for_policy,
    run_workspace_filesystem_with_authority, stable_command_error, stable_success,
    validate_workspace_context_probe,
};
use super::http::{McpCancellationClient, McpHealthClient};
use super::runtime::{CodingToolsRuntime, CodingToolsRuntimeError};
use crate::execution::policy::CapabilityPolicy;
use crate::execution::shell::{ShellExecutionSpec, ShellExecutor, ShellSelector};
use crate::filesystem::service::FilesystemCancellation;

pub(super) const CURRENT_PROTOCOL_VERSION: &str = "2025-11-25";
const COMPATIBLE_PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_CHUNKED_WIRE_BYTES: usize = MAX_BODY_BYTES * 2 + MAX_HEADER_BYTES;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);
const ACCEPT_IDLE: Duration = Duration::from_millis(10);
const MAX_DOWNSTREAM_MCP_SESSIONS: usize = 64;
const MAX_CONNECTION_WORKERS: usize = 64;
static SESSION_GENERATION: AtomicU64 = AtomicU64::new(1);
static PRIVILEGED_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(1);
static PRIVATE_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(1);

struct ConnectionContext<'a> {
    guard: &'a Mutex<AgentFacade<CodingToolsRuntimeAdapter>>,
    public_policy: &'a RwLock<CapabilityPolicy>,
    cancellation: &'a McpCancellationClient,
    desired_state: &'a DesiredStateOwner,
    observed_workspace: &'a Path,
    observed_connection: Option<&'a ConnectionProfile>,
    current_task: &'a CurrentTaskProjection,
    executions: &'a ExecutionRegistry,
    tasks: &'a TaskRegistry,
    scheduler: &'a Scheduler,
    sessions: &'a SessionRegistry,
    requests: &'a RequestRegistry,
    privileged: Option<&'a Arc<dyn PrivilegedExecution>>,
    stopping: &'a AtomicBool,
}

struct ElevatedCallContext<'a> {
    guard: &'a Mutex<AgentFacade<CodingToolsRuntimeAdapter>>,
    privileged: Option<&'a Arc<dyn PrivilegedExecution>>,
    current_task: &'a RegisteredTaskProjection,
    requests: &'a RequestRegistry,
    stopping: &'a AtomicBool,
}

struct WorkspaceFilesystemContext<'a> {
    guard: &'a Mutex<AgentFacade<CodingToolsRuntimeAdapter>>,
    current_task: &'a RegisteredTaskProjection,
    requests: &'a RequestRegistry,
    stopping: &'a AtomicBool,
}

struct TaskControlContext<'a> {
    guard: &'a Mutex<AgentFacade<CodingToolsRuntimeAdapter>>,
    public_policy: &'a RwLock<CapabilityPolicy>,
    cancellation: &'a McpCancellationClient,
    current_task: &'a CurrentTaskProjection,
    executions: &'a ExecutionRegistry,
    tasks: &'a TaskRegistry,
    scheduler: &'a Scheduler,
    observed_workspace: &'a Path,
    requests: &'a RequestRegistry,
    privileged: Option<&'a Arc<dyn PrivilegedExecution>>,
}

struct ServeContext {
    guard: Arc<Mutex<AgentFacade<CodingToolsRuntimeAdapter>>>,
    public_policy: Arc<RwLock<CapabilityPolicy>>,
    cancellation: McpCancellationClient,
    control_plane: ControlPlane,
    observed_workspace: PathBuf,
    observed_connection: Option<ConnectionProfile>,
    current_task: CurrentTaskProjection,
    privileged: Option<Arc<dyn PrivilegedExecution>>,
    shutdown: mpsc::Receiver<()>,
}

struct SessionRequestLease {
    sessions: SessionRegistry,
    owner: McpSessionId,
    request: RequestKey,
}

impl SessionRequestLease {
    fn new(sessions: SessionRegistry, owner: McpSessionId, request: RequestKey) -> Self {
        let _ = sessions.add_request(&owner, request.clone());
        Self {
            sessions,
            owner,
            request,
        }
    }
}

impl Drop for SessionRequestLease {
    fn drop(&mut self) {
        let _ = self.sessions.remove_request(&self.owner, &self.request);
    }
}

pub type CurrentTaskWake = Arc<dyn Fn() + Send + Sync + 'static>;

struct CurrentTaskProjectionInner {
    tasks: TaskRegistry,
    wake: Option<CurrentTaskWake>,
}

/// Read-only UI adapter over the authoritative TaskRegistry.
#[derive(Clone)]
pub struct CurrentTaskProjection(Arc<CurrentTaskProjectionInner>);

struct RegisteredTaskProjection {
    task_id: TaskId,
    tasks: TaskRegistry,
    presentation: CurrentTaskProjection,
}

impl Drop for RegisteredTaskProjection {
    fn drop(&mut self) {
        let _ = self.tasks.finish(&self.task_id, TerminalOutcome::Lost);
        self.presentation.wake();
    }
}

impl RegisteredTaskProjection {
    fn new(task_id: TaskId, tasks: TaskRegistry, presentation: CurrentTaskProjection) -> Self {
        Self {
            task_id,
            tasks,
            presentation,
        }
    }

    fn project(&self, status: CurrentTaskStatus) {
        match &status {
            CurrentTaskStatus::Idle => {
                let _ = self.tasks.finish(&self.task_id, TerminalOutcome::Completed);
            }
            CurrentTaskStatus::Active(task) => match task.state {
                TaskExecutionState::Running => {
                    let _ = self.tasks.mark_running(&self.task_id);
                }
                TaskExecutionState::AwaitingAuthorization | TaskExecutionState::Blocked => {
                    let _ = self.tasks.finish(&self.task_id, TerminalOutcome::Blocked);
                }
                TaskExecutionState::Failed => {
                    let _ = self.tasks.finish(&self.task_id, TerminalOutcome::Failed);
                }
                TaskExecutionState::Cancelled => {
                    let _ = self.tasks.finish(&self.task_id, TerminalOutcome::Cancelled);
                }
                TaskExecutionState::Idle => {}
            },
        }
        self.presentation.wake();
    }

    fn finish(&self, outcome: TerminalOutcome) {
        let _ = self.tasks.finish(&self.task_id, outcome);
        self.presentation.wake();
    }
}

impl Default for CurrentTaskProjection {
    fn default() -> Self {
        Self::new(TaskRegistry::default(), None)
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
    pub(crate) fn new(tasks: TaskRegistry, wake: Option<CurrentTaskWake>) -> Self {
        Self(Arc::new(CurrentTaskProjectionInner { tasks, wake }))
    }

    pub fn snapshot(&self) -> CurrentTaskStatus {
        self.0
            .tasks
            .latest_active()
            .filter(|task| task.lifecycle == LifecycleState::Running)
            .map(|task| {
                CurrentTaskStatus::Active(CurrentTask {
                    kind: task.kind,
                    summary: task.summary,
                    state: TaskExecutionState::Running,
                })
            })
            .unwrap_or_default()
    }

    fn latest_snapshot(&self) -> CurrentTaskStatus {
        self.snapshot()
    }

    pub fn timing_snapshot(&self) -> CurrentTaskTiming {
        let active = self.0.tasks.latest_active();
        let status = active
            .as_ref()
            .filter(|task| task.lifecycle == LifecycleState::Running)
            .map(|task| {
                CurrentTaskStatus::Active(CurrentTask {
                    kind: task.kind,
                    summary: task.summary.clone(),
                    state: TaskExecutionState::Running,
                })
            })
            .unwrap_or_default();
        let now = unix_time_ms();
        CurrentTaskTiming {
            status,
            elapsed_ms: active
                .filter(|task| task.lifecycle == LifecycleState::Running)
                .map(|task| now.saturating_sub(task.created_at_ms)),
            last_tool: self.0.tasks.latest_terminal().map(|task| LastToolTiming {
                kind: task.kind,
                summary: task.summary,
                age_ms: now.saturating_sub(task.updated_at_ms),
            }),
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
            Self::UpstreamCancellationUnavailable => {
                f.write_str("policy enforcement upstream MCP cancellation client is unavailable")
            }
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
    control_plane: ControlPlane,
    current_task: CurrentTaskProjection,
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
    #[cfg(test)]
    pub fn start(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_test(coding_runtime, policy, permission_mode, None, None)
    }

    #[cfg(test)]
    pub fn start_with_wake(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
        wake: CurrentTaskWake,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_test(coding_runtime, policy, permission_mode, None, Some(wake))
    }

    #[cfg(test)]
    pub fn start_with_privilege(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
        privileged: Arc<dyn PrivilegedExecution>,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_test(
            coding_runtime,
            policy,
            permission_mode,
            Some(privileged),
            None,
        )
    }

    #[cfg(test)]
    pub fn start_with_privilege_and_wake(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
        privileged: Arc<dyn PrivilegedExecution>,
        wake: CurrentTaskWake,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_test(
            coding_runtime,
            policy,
            permission_mode,
            Some(privileged),
            Some(wake),
        )
    }

    pub fn start_with_control_plane(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        desired_state: DesiredStateOwner,
        observed_connection: Option<ConnectionProfile>,
        privileged: Option<Arc<dyn PrivilegedExecution>>,
        wake: Option<CurrentTaskWake>,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_inner(
            coding_runtime,
            policy,
            desired_state,
            observed_connection,
            privileged,
            wake,
        )
    }

    #[cfg(test)]
    fn start_test(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
        privileged: Option<Arc<dyn PrivilegedExecution>>,
        wake: Option<CurrentTaskWake>,
    ) -> Result<Self, PolicyEnforcementError> {
        let workspace = coding_runtime.workspace().to_path_buf();
        let desired_state = DesiredStateOwner::default();
        desired_state.replace(DesiredState {
            permission: permission_mode,
            workspace: Some(DesiredWorkspace::for_runtime_path(&workspace)),
            services: ServiceIntent::Enabled,
            connection: None,
        });
        Self::start_inner(
            coding_runtime,
            policy,
            desired_state,
            None,
            privileged,
            wake,
        )
    }

    fn start_inner(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        desired_state: DesiredStateOwner,
        observed_connection: Option<ConnectionProfile>,
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
        let control_plane = ControlPlane::for_workspace(desired_state, &health_workspace)
            .map_err(|_| PolicyEnforcementError::UpstreamFacadeNegotiationFailed)?;
        let guard = AgentFacade::from_coding_runtime_with_executions(
            coding_runtime,
            policy,
            control_plane.executions(),
        )
        .map_err(|_| PolicyEnforcementError::UpstreamFacadeNegotiationFailed)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| PolicyEnforcementError::BindFailed)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| PolicyEnforcementError::BindFailed)?;
        let port = listener
            .local_addr()
            .map_err(|_| PolicyEnforcementError::BindFailed)?
            .port();
        let thread_control_plane = control_plane.clone();
        let current_task = CurrentTaskProjection::new(control_plane.tasks(), wake);
        let thread_workspace = health_workspace.clone();
        let thread_connection = observed_connection.clone();
        let thread_task = current_task.clone();
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
                        control_plane: thread_control_plane,
                        observed_workspace: thread_workspace,
                        observed_connection: thread_connection,
                        current_task: thread_task,
                        privileged,
                        shutdown: shutdown_rx,
                    },
                )
            })
            .map_err(|_| PolicyEnforcementError::ThreadSpawnFailed)?;
        Ok(Self {
            port,
            control_plane,
            current_task,
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
        self.control_plane.desired().set_permission(mode);
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
        let executions = self.control_plane.executions();
        let aggregate = match self.guard.as_ref() {
            None => task_control_snapshot_with_terminal(
                &self.current_task.latest_snapshot(),
                &executions,
            ),
            Some(guard) => match guard.try_lock() {
                Ok(guard) => guard.task_aggregate_snapshot(),
                Err(TryLockError::WouldBlock) => task_control_snapshot_with_terminal(
                    &self.current_task.latest_snapshot(),
                    &executions,
                ),
                Err(TryLockError::Poisoned(error)) => error.into_inner().task_aggregate_snapshot(),
            },
        };
        merge_control_plane_activity(
            aggregate,
            &self.control_plane.tasks(),
            &executions,
            &self.control_plane.scheduler(),
        )
    }

    pub(crate) fn control_plane_activity_snapshot(&self) -> TaskAggregate {
        let running_executions = self.control_plane.executions().running();
        let running_task_ids = running_executions
            .iter()
            .map(|execution| execution.task_id.clone())
            .collect();
        TaskAggregate {
            foreground_task: self.control_plane.tasks().latest_active(),
            detached_execution: running_executions.into_iter().last(),
            last_task: self
                .control_plane
                .tasks()
                .latest_terminal_excluding(&running_task_ids),
            last_execution: self.control_plane.executions().latest_terminal(),
            scheduler: self.control_plane.scheduler().snapshot(),
        }
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
        control_plane,
        observed_workspace,
        observed_connection,
        current_task,
        privileged,
        shutdown,
    } = context;
    let desired_state = control_plane.desired();
    let requests = control_plane.requests();
    let executions = control_plane.executions();
    let tasks = control_plane.tasks();
    let scheduler = control_plane.scheduler();
    let sessions = control_plane.sessions();
    let session_reaper = control_plane.session_reaper(MCP_SESSION_TTL_MS);
    let stopping = Arc::new(AtomicBool::new(false));
    let mut workers = Vec::<JoinHandle<()>>::new();
    let mut next_session_reap = Instant::now();
    let mut next_execution_observation = Instant::now();
    loop {
        if shutdown.try_recv().is_ok() {
            stopping.store(true, Ordering::Release);
            break;
        }
        if Instant::now() >= next_session_reap {
            for expired in session_reaper.reap_expired() {
                settle_closed_session(
                    &expired,
                    &requests,
                    &scheduler,
                    &tasks,
                    &executions,
                    &cancellation,
                    privileged.as_ref(),
                );
            }
            let _ = executions.reap_stale(unix_time_ms());
            next_session_reap = Instant::now() + Duration::from_secs(30);
        }
        let observation_now = Instant::now();
        if !executions.running().is_empty()
            && next_execution_observation > observation_now + Duration::from_secs(1)
        {
            // A command can be accepted just after the idle observer selected
            // its long interval. Running registry state is the authoritative
            // wake signal; do not leave a new detached execution unobserved for
            // the full idle interval.
            next_execution_observation = observation_now;
        }
        if observation_now >= next_execution_observation {
            if let Ok(mut facade) = guard.try_lock() {
                if facade.reap_command_sessions().is_err() {
                    stopping.store(true, Ordering::Release);
                    break;
                }
            }
            let running = executions.running();
            next_execution_observation = Instant::now()
                + if running.is_empty() {
                    Duration::from_secs(30)
                } else if running
                    .iter()
                    .any(|execution| execution.owner_session.is_some())
                {
                    Duration::from_secs(1)
                } else {
                    Duration::from_secs(10)
                };
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
            Ok((mut stream, _)) => {
                if workers.len() >= MAX_CONNECTION_WORKERS {
                    let _ = write_mcp_http_error(
                        &mut stream,
                        503,
                        mcp_unavailable("connection_capacity"),
                        None,
                    );
                    continue;
                }
                let worker_guard = Arc::clone(&guard);
                let worker_policy = Arc::clone(&public_policy);
                let worker_desired = desired_state.clone();
                let worker_workspace = observed_workspace.clone();
                let worker_connection = observed_connection.clone();
                let worker_task = current_task.clone();
                let worker_executions = executions.clone();
                let worker_tasks = tasks.clone();
                let worker_scheduler = scheduler.clone();
                let worker_sessions = sessions.clone();
                let worker_requests = requests.clone();
                let worker_privileged = privileged.as_ref().map(Arc::clone);
                let worker_stopping = Arc::clone(&stopping);
                let worker_cancellation = cancellation.clone();
                let mut spawn_failure_stream = stream.try_clone().ok();
                match thread::Builder::new()
                    .name("localbridge-mcp-policy-request".into())
                    .spawn(move || {
                        let context = ConnectionContext {
                            guard: &worker_guard,
                            public_policy: &worker_policy,
                            cancellation: &worker_cancellation,
                            desired_state: &worker_desired,
                            observed_workspace: &worker_workspace,
                            observed_connection: worker_connection.as_ref(),
                            current_task: &worker_task,
                            executions: &worker_executions,
                            tasks: &worker_tasks,
                            scheduler: &worker_scheduler,
                            sessions: &worker_sessions,
                            requests: &worker_requests,
                            privileged: worker_privileged.as_ref(),
                            stopping: &worker_stopping,
                        };
                        if handle_connection(stream, context).is_err() {
                            eprintln!(
                                "[localbridge-mcp] connection closed before response completed"
                            );
                        }
                    }) {
                    Ok(worker) => workers.push(worker),
                    Err(_) => {
                        if let Some(stream) = spawn_failure_stream.as_mut() {
                            let _ = write_mcp_http_error(
                                stream,
                                503,
                                mcp_unavailable("connection_worker_unavailable"),
                                None,
                            );
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_IDLE);
            }
            Err(_) => break,
        }
    }
    for _ in 0..3 {
        let active = requests.all();
        if active.is_empty() {
            break;
        }
        for request in &active {
            if let Some(request) = requests.request_cancellation(&request.key) {
                let _ = cancel_registered_request(&request, &cancellation, privileged.as_ref());
            }
        }
        thread::sleep(Duration::from_millis(25));
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

fn settle_closed_session(
    session: &SessionRecord,
    requests: &RequestRegistry,
    scheduler: &Scheduler,
    tasks: &TaskRegistry,
    executions: &ExecutionRegistry,
    cancellation: &McpCancellationClient,
    privileged: Option<&Arc<dyn PrivilegedExecution>>,
) {
    for request in requests.owned_by(&session.id) {
        if let Some(request) = requests.request_cancellation(&request.key) {
            let _ = cancel_registered_request(&request, cancellation, privileged);
        }
    }
    for task_id in scheduler.cancel_queued_by_session(&session.id) {
        let _ = tasks.finish(&task_id, TerminalOutcome::Cancelled);
    }
    let _ = executions.orphan_owned_by(&session.id);
}

fn policy_effective_state(
    desired_state: &DesiredStateOwner,
    privileged: Option<&Arc<dyn PrivilegedExecution>>,
    observed_workspace: &Path,
    observed_connection: Option<&ConnectionProfile>,
) -> EffectiveState {
    ConvergenceSnapshot::derive(
        desired_state.snapshot(),
        ObservedState {
            broker: privileged
                .map(|gateway| gateway.state())
                .unwrap_or(PrivilegeState::Disabled),
            runtime: crate::state::RuntimeState::Ready,
            workspace: Some(observed_workspace.to_path_buf()),
            connection: observed_connection.cloned(),
        },
    )
    .effective
}

fn configure_accepted_stream(stream: &TcpStream) -> Result<(), ()> {
    // Windows accepted sockets inherit the listener's non-blocking mode. A
    // connect can reach accept before its first HTTP bytes, so leaving that
    // mode enabled turns ordinary network scheduling into a false disconnect.
    stream.set_nonblocking(false).map_err(|_| ())?;
    stream
        .set_read_timeout(Some(CONNECTION_TIMEOUT))
        .map_err(|_| ())?;
    stream
        .set_write_timeout(Some(CONNECTION_TIMEOUT))
        .map_err(|_| ())
}

fn handle_connection(mut stream: TcpStream, context: ConnectionContext<'_>) -> Result<(), ()> {
    let ConnectionContext {
        guard,
        public_policy,
        cancellation,
        desired_state,
        observed_workspace,
        observed_connection,
        current_task,
        executions,
        tasks,
        scheduler,
        sessions,
        requests,
        privileged,
        stopping,
    } = context;
    configure_accepted_stream(&stream)?;
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) if !error.respond => return Ok(()),
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
        let session_id = McpSessionId::new(session);
        if let Some(closed) = sessions.close_and_remove(&session_id) {
            settle_closed_session(
                &closed,
                requests,
                scheduler,
                tasks,
                executions,
                cancellation,
                privileged,
            );
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
        let mode = policy_effective_state(
            desired_state,
            privileged,
            observed_workspace,
            observed_connection,
        )
        .authority
        .execution;
        let current_signature = {
            let policy = public_policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            effective_tool_catalog_signature(&policy, mode)
        };
        let session_id = McpSessionId::new(session);
        let Some(stored) = sessions.get(&session_id) else {
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
        let pending = sessions
            .update(&session_id, |stored| {
                if stored.tool_catalog_signature != current_signature {
                    stored.tool_catalog_signature = current_signature;
                    stored.tools_list_changed_pending = true;
                }
                let pending = stored.tools_list_changed_pending;
                stored.tools_list_changed_pending = false;
                pending
            })
            .unwrap_or(false);
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
        for expired in SessionReaper::new(sessions.clone(), MCP_SESSION_TTL_MS).reap_expired() {
            settle_closed_session(
                &expired,
                requests,
                scheduler,
                tasks,
                executions,
                cancellation,
                privileged,
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
        let mode = policy_effective_state(
            desired_state,
            privileged,
            observed_workspace,
            observed_connection,
        )
        .authority
        .execution;
        let tool_catalog_signature = {
            let policy = public_policy
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            effective_tool_catalog_signature(&policy, mode)
        };
        let session = new_session_id();
        match sessions.insert_bounded(
            SessionRecord::new(
                session.clone(),
                protocol.to_string(),
                tool_catalog_signature,
            ),
            MAX_DOWNSTREAM_MCP_SESSIONS,
        ) {
            Ok(()) => {}
            Err(SessionInsertError::Capacity) => {
                return write_mcp_http_error(
                    &mut stream,
                    503,
                    mcp_unavailable("session_capacity"),
                    None,
                );
            }
            Err(SessionInsertError::AlreadyExists) => {
                return write_mcp_http_error(
                    &mut stream,
                    503,
                    mcp_unavailable("session_identity_collision"),
                    None,
                );
            }
        }
        return write_rpc_result(
            &mut stream,
            id,
            json!({
                "protocolVersion": protocol,
                "capabilities": {"tools": {"listChanged": true}},
                "serverInfo": {"name": "localbridge-mcp-guard", "version": format!("{}+api{}", env!("CARGO_PKG_VERSION"), AGENT_API_REVISION)}
            }),
            Some(session.as_str()),
        );
    }

    if method == "ping" && request.header("mcp-session-id").is_none() && !id.is_null() {
        return write_rpc_result(&mut stream, id, json!({}), None);
    }

    let Some(session) = request.header("mcp-session-id") else {
        return write_rpc_error(&mut stream, id, -32600, "Mcp-Session-Id is required", None);
    };
    let session_id = McpSessionId::new(session);
    let stored_session = sessions.get(&session_id);
    let Some(stored_session) = stored_session else {
        return write_mcp_http_error(&mut stream, 404, mcp_unavailable("session_not_found"), None);
    };
    let mode = policy_effective_state(
        desired_state,
        privileged,
        observed_workspace,
        observed_connection,
    )
    .authority
    .execution;
    let current_signature = {
        let policy = public_policy
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        effective_tool_catalog_signature(&policy, mode)
    };
    if stored_session.tool_catalog_signature != current_signature {
        let _ = sessions.update(&session_id, |stored| {
            stored.tool_catalog_signature = current_signature;
            stored.tools_list_changed_pending = true;
        });
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
    let _ = sessions.update(&session_id, |_| ());

    if id.is_null() {
        if method == "notifications/cancelled" {
            if let Some(request_id) = object
                .get("params")
                .and_then(Value::as_object)
                .and_then(|params| params.get("requestId"))
                .and_then(rpc_request_id_from_json)
            {
                if let Some(active) =
                    registered_request_for_transport_cancel(requests, &session_id, &request_id)
                {
                    if cancel_registered_request(&active, cancellation, privileged).is_err() {
                        return write_mcp_http_error(
                            &mut stream,
                            503,
                            mcp_unavailable("cancellation_unavailable"),
                            Some(session),
                        );
                    }
                    for _ in 0..2 {
                        thread::sleep(Duration::from_millis(25));
                        let Some(active) = registered_request_for_transport_cancel(
                            requests,
                            &session_id,
                            &request_id,
                        ) else {
                            break;
                        };
                        let _ = cancel_registered_request(&active, cancellation, privileged);
                    }
                }
                return write_empty(&mut stream, 202, Some(session));
            }
        }
        return write_empty(&mut stream, 202, Some(session));
    }

    match method {
        "ping" => write_rpc_result(&mut stream, id, json!({}), Some(session)),
        "tools/list" => {
            let mode = policy_effective_state(
                desired_state,
                privileged,
                observed_workspace,
                observed_connection,
            )
            .authority
            .execution;
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
            let effective = policy_effective_state(
                desired_state,
                privileged,
                observed_workspace,
                observed_connection,
            );
            let mode = effective.authority.execution;
            let scoped_request = request_key_from_json(session_id.clone(), &id)
                .expect("validated downstream request id");
            let _session_request_lease = SessionRequestLease::new(
                sessions.clone(),
                session_id.clone(),
                scoped_request.clone(),
            );
            let lane = scheduler_lane(name, &arguments);
            if lane == SchedulerLane::Work && !effective.work_is_authorized() {
                requests.record_error(
                    scoped_request.clone(),
                    OperationError::new(
                        "RuntimeUnavailable",
                        ErrorCategory::Unavailable,
                        "control-plane intent has not converged",
                        true,
                    ),
                );
                return write_rpc_result(
                    &mut stream,
                    id,
                    FacadeError::new(
                        FacadeErrorCode::RuntimeUnavailable,
                        "控制面目标尚未与运行状态收敛",
                        true,
                    )
                    .to_mcp_result(),
                    Some(session),
                );
            }
            let (scheduled_task, _scheduler_permit) = match lane {
                SchedulerLane::Work => {
                    let task_id = tasks.queue(
                        session_id.clone(),
                        scoped_request.clone(),
                        public_task_kind(name, &arguments),
                        public_safe_summary(name, &arguments),
                    );
                    let _ = sessions.add_task(&session_id, task_id.clone());
                    match scheduler.admit_work(session_id.clone(), task_id.clone()) {
                        Ok(permit) => {
                            let _ = tasks.mark_running(&task_id);
                            (Some(task_id), permit)
                        }
                        Err(
                            SchedulerAdmissionError::QueueCapacityExceeded
                            | SchedulerAdmissionError::ImmediateCapacityExceeded,
                        ) => {
                            let error = OperationError::new(
                                "QueueCapacityExceeded",
                                ErrorCategory::Capacity,
                                "work queue capacity was exceeded",
                                true,
                            )
                            .for_request(scoped_request.clone());
                            requests.record_error(scoped_request.clone(), error.clone());
                            let _ =
                                tasks.finish_with_error(&task_id, TerminalOutcome::Blocked, error);
                            return write_rpc_result(
                                &mut stream,
                                id,
                                FacadeError::new(
                                    FacadeErrorCode::QueueCapacityExceeded,
                                    "工作队列容量已满",
                                    true,
                                )
                                .to_mcp_result(),
                                Some(session),
                            );
                        }
                        Err(SchedulerAdmissionError::Cancelled) => {
                            let _ = tasks.finish(&task_id, TerminalOutcome::Cancelled);
                            return write_rpc_result(
                                &mut stream,
                                id,
                                FacadeError::new(
                                    FacadeErrorCode::ProcessCancelled,
                                    "排队任务已取消",
                                    false,
                                )
                                .to_mcp_result(),
                                Some(session),
                            );
                        }
                    }
                }
                immediate => match scheduler.enter_immediate(immediate) {
                    Ok(permit) => (None, permit),
                    Err(SchedulerAdmissionError::ImmediateCapacityExceeded) => {
                        requests.record_error(
                            scoped_request.clone(),
                            OperationError::new(
                                "LaneCapacityExceeded",
                                ErrorCategory::Capacity,
                                "control-plane lane capacity was exceeded",
                                true,
                            ),
                        );
                        return write_rpc_result(
                            &mut stream,
                            id,
                            FacadeError::new(
                                FacadeErrorCode::QueueCapacityExceeded,
                                "控制面请求容量已满",
                                true,
                            )
                            .to_mcp_result(),
                            Some(session),
                        );
                    }
                    Err(
                        SchedulerAdmissionError::QueueCapacityExceeded
                        | SchedulerAdmissionError::Cancelled,
                    ) => {
                        unreachable!("immediate lane does not queue")
                    }
                },
            };
            if name == "elevated_exec" {
                let task_id = scheduled_task
                    .clone()
                    .expect("elevated_exec is admitted through Work lane");
                let registered_task =
                    RegisteredTaskProjection::new(task_id, tasks.clone(), current_task.clone());
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
                        current_task: &registered_task,
                        requests,
                        stopping,
                    },
                );
                return finalize_special_handler_request(&request_key, session, result);
            }
            if name == "filesystem" {
                let task_id = scheduled_task
                    .clone()
                    .expect("filesystem is admitted through Work lane");
                let registered_task =
                    RegisteredTaskProjection::new(task_id, tasks.clone(), current_task.clone());
                return handle_workspace_filesystem(
                    &mut stream,
                    id,
                    session,
                    mode,
                    arguments,
                    WorkspaceFilesystemContext {
                        guard,
                        current_task: &registered_task,
                        requests,
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
                        executions,
                        tasks,
                        scheduler,
                        observed_workspace,
                        requests,
                        privileged,
                    },
                );
                return finalize_special_handler_request(&request_key, session, result);
            }
            let request_key = scoped_request.clone();
            let controlled_public_session = command_control_public_session(name, &arguments);
            let command_action = arguments.get("action").and_then(Value::as_str);
            if name == "command_control" && command_action == Some("adopt") {
                let decision = public_policy
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .decide_public(mode, name, &arguments);
                let result = if decision.allowed {
                    adopt_public_command_session(
                        controlled_public_session.as_ref(),
                        &session_id,
                        executions,
                    )
                } else {
                    FacadeDenied {
                        reason: decision
                            .deny_reason
                            .expect("denied command_control decision contains reason"),
                        capability: decision.descriptor.capability,
                    }
                    .to_mcp_result()
                };
                return write_rpc_result(&mut stream, id, result, Some(session));
            }
            // PublicSessionId is the stable, unguessable control capability for a detached
            // execution. MCP transport sessions are intentionally shorter lived and may be
            // recreated between exec -> poll/write/kill calls. Access remains resource scoped:
            // callers must present the exact PublicSessionId and no enumeration fallback exists.
            let private_request_id = next_private_request_id();
            if requests
                .register(
                    request_key.clone(),
                    RequestCancellationTarget::Runtime(private_request_id.clone()),
                )
                .is_err()
            {
                return write_rpc_error(
                    &mut stream,
                    id,
                    -32600,
                    "Duplicate active request id in MCP session",
                    Some(session),
                );
            }
            let registered_task = scheduled_task.clone();
            let mut guard = match lane {
                SchedulerLane::Work => guard
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                SchedulerLane::Observation | SchedulerLane::Control => match guard.try_lock() {
                    Ok(guard) => guard,
                    Err(TryLockError::Poisoned(error)) => error.into_inner(),
                    Err(TryLockError::WouldBlock) => {
                        if name == "command_control" {
                            if let Some(public_session) = controlled_public_session.as_ref() {
                                record_mcp_request_start(
                                    &request_diagnostic_key(&id),
                                    session,
                                    name,
                                );
                                let decision = public_policy
                                    .read()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .decide_public(mode, name, &arguments);
                                let result = if decision.allowed {
                                    direct_command_control_during_work(
                                        &arguments,
                                        public_session,
                                        executions,
                                        cancellation,
                                        observed_workspace,
                                        private_request_id.clone(),
                                    )
                                } else {
                                    FacadeDenied {
                                        reason: decision.deny_reason.expect(
                                            "denied command_control decision contains reason",
                                        ),
                                        capability: decision.descriptor.capability,
                                    }
                                    .to_mcp_result()
                                };
                                if let Some(error) =
                                    operation_error_from_facade_result(&Ok(result.clone()))
                                {
                                    requests.record_error(request_key.clone(), error);
                                }
                                requests.remove(&request_key);
                                return write_rpc_result(&mut stream, id, result, Some(session));
                            }
                        }
                        requests.record_error(
                            request_key.clone(),
                            OperationError::new(
                                "RuntimeUnavailable",
                                ErrorCategory::Unavailable,
                                "control lane is temporarily unavailable",
                                true,
                            ),
                        );
                        requests.remove(&request_key);
                        return write_rpc_result(
                            &mut stream,
                            id,
                            FacadeError::new(
                                FacadeErrorCode::RuntimeUnavailable,
                                "控制面正在处理工作请求",
                                true,
                            )
                            .to_mcp_result(),
                            Some(session),
                        );
                    }
                },
            };
            if stopping.load(Ordering::Acquire) {
                requests.remove(&request_key);
                if let Some(task_id) = &registered_task {
                    let _ = tasks.finish(task_id, TerminalOutcome::Lost);
                }
                return write_mcp_http_error(
                    &mut stream,
                    503,
                    mcp_unavailable("server_stopping"),
                    Some(session),
                );
            }
            record_mcp_request_start(&request_diagnostic_key(&id), session, name);
            let private_request_value = rpc_request_id_to_json(&private_request_id);
            let call_task_id = registered_task
                .clone()
                .unwrap_or_else(|| TaskId::new(format!("projection-{}", private_request_id)));
            let mut result = guard.call_tool_for_task(
                mode,
                name,
                arguments,
                TaskCallIdentity {
                    request_id: Some(&private_request_value),
                    task_id: call_task_id,
                    owner_session: Some(session_id.clone()),
                },
                |status| {
                    if let (
                        Some(task_id),
                        CurrentTaskStatus::Active(CurrentTask {
                            state: TaskExecutionState::Running,
                            ..
                        }),
                    ) = (&registered_task, &status)
                    {
                        let _ = tasks.mark_running(task_id);
                    }
                    let _ = status;
                    current_task.wake();
                },
            );
            if requests.cancellation_was_requested(&request_key) {
                result = normalize_accepted_request_cancellation(name, result);
            }
            if let Some(error) = operation_error_from_facade_result(&result) {
                requests.record_error(request_key.clone(), error.clone());
                if let Some(task_id) = &registered_task {
                    let _ = tasks.finish_with_error(task_id, task_terminal_outcome(&result), error);
                }
            }
            requests.remove(&request_key);
            if let Some(task_id) = &registered_task {
                let _ = tasks.finish(task_id, task_terminal_outcome(&result));
                current_task.wake();
            }
            if let Ok(result) = &result {
                if let Some(public_session) = public_session_from_result(name, result) {
                    let _ = guard
                        .bind_public_command_owner(public_session.as_str(), session_id.clone());
                }
            }
            match result {
                Ok(mut result) => {
                    if name == "workspace_context" {
                        enrich_workspace_context_privilege(
                            &mut result,
                            mode,
                            privileged,
                            tasks,
                            executions,
                            scheduler,
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

fn adopt_public_command_session(
    public_session_id: Option<&PublicSessionId>,
    owner: &McpSessionId,
    executions: &ExecutionRegistry,
) -> Value {
    let Some(public_session_id) = public_session_id else {
        return FacadeError::new(
            FacadeErrorCode::InvalidArgument,
            "adopt 需要 session_id",
            false,
        )
        .to_mcp_result();
    };
    match executions.adopt_owner(public_session_id, owner.clone()) {
        Ok(execution) => stable_success(
            json!({
                "status":"running",
                "session_id":execution.public_session_id,
                "task_id":execution.task_id,
                "execution_id":execution.id,
                "adopted":true,
            }),
            "Command session adopted",
        ),
        Err(ExecutionRegistryError::AlreadyTerminal { .. }) => FacadeError::new(
            FacadeErrorCode::SessionUnavailable,
            "命令已经终止，不能接管",
            false,
        )
        .to_mcp_result(),
        Err(ExecutionRegistryError::UnknownPublicSession(_)) => FacadeError::new(
            FacadeErrorCode::SessionUnavailable,
            "命令会话不存在或已回收",
            false,
        )
        .to_mcp_result(),
        Err(_) => FacadeError::new(
            FacadeErrorCode::RuntimeUnavailable,
            "命令会话接管失败",
            true,
        )
        .to_mcp_result(),
    }
}

fn enrich_workspace_context_privilege(
    result: &mut Value,
    mode: PermissionMode,
    privileged: Option<&Arc<dyn PrivilegedExecution>>,
    tasks: &TaskRegistry,
    executions: &ExecutionRegistry,
    scheduler: &Scheduler,
) {
    let state = privileged
        .map(|gateway| gateway.state())
        .unwrap_or(PrivilegeState::Disabled);
    let (observed_privilege_state, broker_state, uac_state) = match &state {
        PrivilegeState::Disabled => ("disabled", "offline", "not_requested"),
        PrivilegeState::Requested => ("requested", "offline", "not_requested"),
        PrivilegeState::AwaitingUac => ("awaiting_uac", "starting", "awaiting_user"),
        PrivilegeState::Active { .. } => ("active", "active", "authorized"),
        PrivilegeState::Faulted(_) => ("faulted", "faulted", "faulted"),
    };
    let desired_permission = match mode {
        PermissionMode::Edit => "edit",
        PermissionMode::Full => "full",
        PermissionMode::Elevated => "elevated",
    };
    let (privilege_state, effective_permission, authority_reconciliation) = match (mode, &state) {
        (PermissionMode::Edit, PrivilegeState::Disabled) => ("disabled", "edit", "converged"),
        (PermissionMode::Edit, _) => ("disabled", "edit", "disable_pending"),
        (PermissionMode::Full, PrivilegeState::Disabled) => ("disabled", "full", "converged"),
        (PermissionMode::Full, _) => ("disabled", "full", "disable_pending"),
        (PermissionMode::Elevated, PrivilegeState::Active { .. }) => {
            ("active", "elevated", "converged")
        }
        (PermissionMode::Elevated, PrivilegeState::Requested | PrivilegeState::AwaitingUac) => {
            (observed_privilege_state, "full", "awaiting_authorization")
        }
        (PermissionMode::Elevated, PrivilegeState::Disabled | PrivilegeState::Faulted(_)) => {
            (observed_privilege_state, "full", "broker_unavailable")
        }
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
    data.insert(
        "authority".into(),
        json!({
            "desired_permission":desired_permission,
            "observed_privilege":observed_privilege_state,
            "observed_broker":broker_state,
            "observed_uac":uac_state,
            "effective_permission":effective_permission,
            "reconciliation":authority_reconciliation,
        }),
    );
    let aggregate = data
        .get("current_task")
        .cloned()
        .unwrap_or_else(|| json!({"state":"idle"}));
    data.insert(
        "current_task".into(),
        merge_control_plane_activity(aggregate, tasks, executions, scheduler),
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
        executions,
        tasks,
        scheduler,
        observed_workspace,
        requests,
        privileged,
    } = context;
    {
        let policy = public_policy
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let decision = policy.decide_public(mode, "task_control", &arguments);
        if !decision.allowed {
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
    let before = current_task.latest_snapshot();
    let session_id = McpSessionId::new(session);
    let data = match action {
        "list" => task_control_list(tasks, executions, &session_id),
        "get" => match guard.try_lock() {
            _ if arguments.get("task_id").is_some() => {
                match task_control_get_by_id(&arguments, tasks, executions) {
                    Ok(data) => data,
                    Err(error) => {
                        return write_rpc_result(stream, id, error.to_mcp_result(), Some(session));
                    }
                }
            }
            Ok(guard) => guard.task_aggregate_snapshot(),
            Err(TryLockError::WouldBlock) => {
                task_control_snapshot_with_terminal(&before, executions)
            }
            Err(TryLockError::Poisoned(error)) => error.into_inner().task_aggregate_snapshot(),
        },
        "cancel" => {
            let control_request = request_key_from_json(session_id.clone(), &id)
                .expect("validated task_control request id");
            let requested = match cancel_task_id_argument(&arguments) {
                Ok(requested) => requested,
                Err(error) => {
                    return write_rpc_result(stream, id, error.to_mcp_result(), Some(session));
                }
            };
            let checkpoint_store = WorkflowCheckpointStore::for_workspace(observed_workspace).ok();
            let explicit_task_capability = requested.is_some();
            let mut candidates = requested
                .as_ref()
                .filter(|task_id| task_is_cancellable(tasks, executions, task_id))
                .cloned()
                .into_iter()
                .collect::<BTreeSet<_>>();
            if !explicit_task_capability {
                candidates = cancellable_task_ids(tasks, executions, &session_id);
                candidates.extend(
                    executions
                        .running_unowned()
                        .into_iter()
                        .map(|execution| execution.task_id),
                );
            }
            if let Some(workflow_id) = checkpoint_store
                .as_ref()
                .and_then(|store| store.active_owned_workflow::<Value>(session).ok().flatten())
            {
                let workflow_task = TaskId::new(workflow_id);
                if !candidates.contains(&workflow_task) {
                    candidates.insert(workflow_task);
                }
            }
            if let Some(workflow_id) = checkpoint_store
                .as_ref()
                .and_then(|store| store.active_unowned_workflow::<Value>().ok().flatten())
            {
                candidates.insert(TaskId::new(workflow_id));
            }
            if let Some(requested_task) = requested.as_ref() {
                if checkpoint_store.as_ref().is_some_and(|store| {
                    store.active_workflow::<Value>().ok().flatten().as_deref()
                        == Some(requested_task.as_str())
                }) {
                    candidates.insert(requested_task.clone());
                }
            }
            if !explicit_task_capability && candidates.is_empty() {
                if let Some(workflow_id) = checkpoint_store
                    .as_ref()
                    .and_then(|store| store.active_workflow::<Value>().ok().flatten())
                {
                    let error = FacadeError::new(
                        FacadeErrorCode::TaskIdRequired,
                        "存在其他 MCP Session 创建的 durable workflow；取消时必须显式指定 task_id",
                        false,
                    )
                    .with_details(json!({"task_id":workflow_id}));
                    return write_rpc_result(stream, id, error.to_mcp_result(), Some(session));
                }
                if let Some(execution) = executions.latest_running() {
                    let error = FacadeError::new(
                        FacadeErrorCode::TaskIdRequired,
                        "存在其他 MCP Session 创建的 detached execution；取消时必须显式指定 task_id",
                        false,
                    )
                    .with_details(json!({
                        "task_id":execution.task_id,
                        "execution_id":execution.id,
                        "session_id":execution.public_session_id,
                    }));
                    return write_rpc_result(stream, id, error.to_mcp_result(), Some(session));
                }
            }
            let selected = match select_cancellable_task(requested, candidates) {
                Ok(selected) => selected,
                Err(error) => {
                    return write_rpc_result(stream, id, error.to_mcp_result(), Some(session));
                }
            };
            let mut cancelled = 0u64;
            let mut queued_cancelled = false;
            let mut workflow_cancelled = false;
            if let Some(task_id) = &selected {
                workflow_cancelled = checkpoint_store.as_ref().is_some_and(|store| {
                    if explicit_task_capability {
                        store
                            .cancel_by_id::<Value>(task_id.as_str())
                            .unwrap_or(false)
                    } else {
                        store
                            .cancel_owned::<Value>(task_id.as_str(), session)
                            .unwrap_or(false)
                            || store
                                .cancel_unowned::<Value>(task_id.as_str())
                                .unwrap_or(false)
                    }
                });
                queued_cancelled = if explicit_task_capability {
                    scheduler.cancel_queued_task_by_id(task_id)
                } else {
                    scheduler.cancel_queued_task(&session_id, task_id)
                };
                if queued_cancelled {
                    let _ = tasks.finish(task_id, TerminalOutcome::Cancelled);
                }
                let active_request = tasks.get(task_id).map(|task| task.request);
                let running_executions = if explicit_task_capability {
                    executions.running_for_task(task_id)
                } else {
                    executions
                        .running_owned_by(&session_id)
                        .into_iter()
                        .chain(executions.running_unowned())
                        .filter(|execution| &execution.task_id == task_id)
                        .collect()
                };
                for execution in running_executions {
                    let cancellation_requested =
                        execution.runtime_handle.as_ref().is_some_and(|handle| {
                            if executions
                                .request_cancellation(&execution.public_session_id, "KILL")
                                .is_err()
                            {
                                return false;
                            }
                            match cancellation.kill_command_session(handle.as_str(), 0) {
                                Ok(result)
                                    if result.get("isError").and_then(Value::as_bool)
                                        == Some(true) =>
                                {
                                    true
                                }
                                Ok(result) => {
                                    if let Some(terminal) = runtime_cancellation_terminal(result) {
                                        let _ = executions.finish(&execution.id, terminal);
                                    }
                                    true
                                }
                                Err(CodingToolsRuntimeError::RequestTimeout) => true,
                                Err(_) => true,
                            }
                        });
                    if cancellation_requested {
                        let reached_terminal = executions
                            .execution_for_public_session(&execution.public_session_id)
                            .is_some_and(|execution| execution.state.is_terminal());
                        if reached_terminal
                            && WorkflowCheckpointStore::for_workspace(observed_workspace)
                                .and_then(|store| {
                                    store.settle_command_kill::<Value>(
                                        execution.public_session_id.as_str(),
                                    )
                                })
                                .is_err()
                        {
                            requests.record_error(
                                control_request.clone(),
                                OperationError::new(
                                    "WorkflowCheckpointUnavailable",
                                    ErrorCategory::Unavailable,
                                    "workflow checkpoint could not settle after cancellation",
                                    true,
                                ),
                            );
                        }
                        cancelled = cancelled.saturating_add(1);
                    }
                }
                if let Some(request_key) = active_request {
                    if let Some(active) = requests.request_cancellation(&request_key) {
                        if cancel_registered_request(&active, cancellation, privileged).is_ok() {
                            cancelled = cancelled.saturating_add(1);
                        }
                    }
                }
            }
            if queued_cancelled || cancelled > 0 {
                if let Some(task_id) = &selected {
                    wait_for_task_cancel_settlement(
                        tasks,
                        executions,
                        task_id,
                        Duration::from_millis(500),
                    );
                }
            }
            let cancellation_requested = workflow_cancelled || queued_cancelled || cancelled > 0;
            if !cancellation_requested {
                let error = FacadeError::new(
                    FacadeErrorCode::RuntimeUnavailable,
                    "The task exists, but no owned cancellation target accepted the request",
                    true,
                )
                .with_details(json!({
                    "task_id": selected.as_ref().map(TaskId::as_str),
                }));
                return write_rpc_result(stream, id, error.to_mcp_result(), Some(session));
            }
            let durable_cancelled = workflow_cancelled
                || selected.as_ref().is_some_and(|task_id| {
                    tasks.get(task_id).is_some_and(|task| {
                        task.lifecycle == LifecycleState::Terminal(TerminalOutcome::Cancelled)
                    })
                });
            let mut data = match guard.try_lock() {
                Ok(guard) => guard.task_aggregate_snapshot(),
                Err(TryLockError::WouldBlock) => {
                    task_control_snapshot_with_terminal(&current_task.latest_snapshot(), executions)
                }
                Err(TryLockError::Poisoned(error)) => error.into_inner().task_aggregate_snapshot(),
            };
            if let Some(object) = data.as_object_mut() {
                object.insert("cancelled_requests".into(), Value::from(cancelled));
                object.insert(
                    "cancellation_requested".into(),
                    Value::Bool(cancellation_requested),
                );
                object.insert(
                    "cancelled_queued_tasks".into(),
                    Value::from(u64::from(queued_cancelled)),
                );
                object.insert(
                    "durable_task_cancelled".into(),
                    Value::Bool(durable_cancelled),
                );
                object.insert("workflow_cancelled".into(), Value::Bool(workflow_cancelled));
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
    let mut data = data;
    if let Some(object) = data.as_object_mut() {
        object
            .entry("availability")
            .or_insert_with(|| Value::String("ready".into()));
    }
    let data = merge_control_plane_activity(data, tasks, executions, scheduler);
    write_rpc_result(
        stream,
        id,
        stable_success(data, "Task control completed"),
        Some(session),
    )
}

fn task_control_list(
    tasks: &TaskRegistry,
    executions: &ExecutionRegistry,
    owner: &McpSessionId,
) -> Value {
    let owned_tasks = tasks.owned_by(owner);
    let visible_executions = executions
        .all()
        .into_iter()
        .filter(|execution| execution.owner_session.as_ref() == Some(owner))
        .collect::<Vec<_>>();
    json!({
        "state": if owned_tasks.iter().any(|task| !task.lifecycle.is_terminal()) || visible_executions.iter().any(|execution| !execution.state.is_terminal()) { "active" } else { "idle" },
        "tasks": owned_tasks,
        "executions": visible_executions,
    })
}

fn task_control_get_by_id(
    arguments: &Value,
    tasks: &TaskRegistry,
    executions: &ExecutionRegistry,
) -> Result<Value, FacadeError> {
    let task_id = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(TaskId::new)
        .ok_or_else(|| FacadeError::new(FacadeErrorCode::InvalidArgument, "task_id 无效", false))?;
    let task = tasks
        .get(&task_id)
        .ok_or_else(|| FacadeError::new(FacadeErrorCode::NotFound, "任务不存在或已回收", false))?;
    // TaskId is an explicit unguessable resource capability. Enumeration is
    // session-scoped, while get/cancel by the returned stable identity remains
    // usable after the shorter-lived MCP transport session reconnects.
    let related = executions
        .all()
        .into_iter()
        .filter(|execution| execution.task_id == task_id)
        .collect::<Vec<_>>();
    Ok(json!({
        "state": if task.lifecycle.is_terminal() { "idle" } else { "active" },
        "task": task,
        "executions": related,
    }))
}

fn wait_for_task_cancel_settlement(
    tasks: &TaskRegistry,
    executions: &ExecutionRegistry,
    task_id: &TaskId,
    max_wait: Duration,
) {
    let deadline = Instant::now() + max_wait;
    while task_is_cancellable(tasks, executions, task_id) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
}

fn cancel_task_id_argument(arguments: &Value) -> Result<Option<TaskId>, FacadeError> {
    match arguments.get("task_id") {
        None => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => {
            Ok(Some(TaskId::new(value.clone())))
        }
        Some(_) => Err(FacadeError::new(
            FacadeErrorCode::InvalidArgument,
            "task_id 必须是非空字符串",
            false,
        )),
    }
}

fn cancellable_task_ids(
    tasks: &TaskRegistry,
    executions: &ExecutionRegistry,
    owner: &McpSessionId,
) -> BTreeSet<TaskId> {
    tasks
        .active_owned_by(owner)
        .into_iter()
        .map(|task| task.id)
        .chain(
            executions
                .running_owned_by(owner)
                .into_iter()
                .map(|execution| execution.task_id),
        )
        .collect()
}

fn select_cancellable_task(
    requested: Option<TaskId>,
    candidates: BTreeSet<TaskId>,
) -> Result<Option<TaskId>, FacadeError> {
    if let Some(task_id) = requested {
        return if candidates.contains(&task_id) {
            Ok(Some(task_id))
        } else {
            Err(FacadeError::new(
                FacadeErrorCode::TaskNotOwned,
                "任务不属于当前 MCP Session 或已终止",
                false,
            ))
        };
    }
    match candidates.len() {
        0 => Err(FacadeError::new(
            FacadeErrorCode::NotFound,
            "当前作用域内没有可取消任务",
            false,
        )),
        1 => Ok(candidates.into_iter().next()),
        _ => Err(FacadeError::new(
            FacadeErrorCode::TaskIdRequired,
            "当前 MCP Session 有多个可取消任务，请指定 task_id",
            false,
        )),
    }
}

fn task_is_cancellable(
    tasks: &TaskRegistry,
    executions: &ExecutionRegistry,
    task_id: &TaskId,
) -> bool {
    tasks
        .get(task_id)
        .is_some_and(|task| !task.lifecycle.is_terminal())
        || executions
            .running()
            .iter()
            .any(|execution| &execution.task_id == task_id)
}

fn runtime_cancellation_terminal(result: Value) -> Option<ExecutionTerminal> {
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let structured = result.get("structuredContent")?;
    let status = structured.get("status").and_then(Value::as_str)?;
    if !matches!(status, "killed" | "terminated" | "cancelled") {
        return None;
    }
    Some(ExecutionTerminal {
        outcome: TerminalOutcome::Cancelled,
        exit_code: structured.get("exit_code").and_then(Value::as_i64),
        signal: structured
            .get("signal")
            .and_then(Value::as_str)
            .map(str::to_string),
        output_refs: Vec::new(),
        error_code: Some("ProcessCancelled".to_string()),
        completed_at_ms: unix_time_ms(),
    })
}

fn direct_command_control_during_work(
    arguments: &Value,
    public_session_id: &PublicSessionId,
    executions: &ExecutionRegistry,
    cancellation: &McpCancellationClient,
    workspace: &Path,
    private_request_id: RpcRequestId,
) -> Value {
    let action = match arguments.get("action").and_then(Value::as_str) {
        Some("poll") => CommandControlAction::Poll,
        Some("write") => CommandControlAction::Write,
        Some("kill") => CommandControlAction::Kill,
        _ => {
            return FacadeError::new(
                FacadeErrorCode::InvalidArgument,
                "command_control action 无效",
                false,
            )
            .to_mcp_result();
        }
    };
    let Some(object) = arguments.as_object() else {
        return FacadeError::new(FacadeErrorCode::InvalidArgument, "命令控制参数无效", false)
            .to_mcp_result();
    };
    let allowed = match action {
        CommandControlAction::Poll => &["action", "session_id", "wait_ms"][..],
        CommandControlAction::Write => &["action", "session_id", "chars", "wait_ms"][..],
        CommandControlAction::Kill => &["action", "session_id", "signal", "wait_ms"][..],
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return FacadeError::new(FacadeErrorCode::InvalidArgument, "命令控制参数无效", false)
            .to_mcp_result();
    }
    let signal = if action == CommandControlAction::Kill {
        match object.get("signal").and_then(Value::as_str) {
            None | Some("TERM") => Some(CommandKillSignal::Term),
            Some("KILL") => Some(CommandKillSignal::Kill),
            Some("INT") => Some(CommandKillSignal::Interrupt),
            Some(_) => {
                return FacadeError::new(
                    FacadeErrorCode::InvalidArgument,
                    "命令终止信号无效",
                    false,
                )
                .to_mcp_result();
            }
        }
    } else {
        None
    };
    let wait_ms = arguments
        .get("wait_ms")
        .and_then(Value::as_u64)
        .unwrap_or(if action == CommandControlAction::Kill {
            5_000
        } else {
            0
        })
        .min(30_000);
    let result = match control_command_during_work(
        CommandControlRequest {
            action,
            chars: arguments
                .get("chars")
                .and_then(Value::as_str)
                .map(str::to_string),
            signal,
            wait_ms,
            request_id: private_request_id,
            public_session_id: public_session_id.clone(),
        },
        executions,
        cancellation,
    ) {
        Ok(result) => result,
        Err(error) => {
            let (code, message, retryable) = match error {
                CommandControlError::InvalidRequest => {
                    (FacadeErrorCode::InvalidArgument, "命令控制参数无效", false)
                }
                CommandControlError::SessionUnavailable => {
                    (FacadeErrorCode::SessionUnavailable, "命令会话不可用", false)
                }
                CommandControlError::RuntimeUnavailable => (
                    FacadeErrorCode::RuntimeUnavailable,
                    "命令控制通道不可用",
                    true,
                ),
                CommandControlError::RuntimeCapabilityMismatch => (
                    FacadeErrorCode::RuntimeCapabilityMismatch,
                    "命令控制响应无效",
                    false,
                ),
                CommandControlError::OperationTimedOut => (
                    FacadeErrorCode::OperationTimedOut,
                    "命令控制请求已达到 wait_ms 时间预算",
                    true,
                ),
                CommandControlError::ExecutionConflict => (
                    FacadeErrorCode::SessionUnavailable,
                    "命令终态发生冲突",
                    false,
                ),
            };
            return FacadeError::new(code, message, retryable).to_mcp_result();
        }
    };
    let checkpoint_settled = action != CommandControlAction::Kill
        || result.status == RuntimeCommandStatus::Running
        || WorkflowCheckpointStore::for_workspace(workspace)
            .and_then(|store| store.settle_command_kill::<Value>(result.public_session_id.as_str()))
            .is_ok();
    direct_command_result_to_mcp(result, action, checkpoint_settled)
}

fn direct_command_result_to_mcp(
    result: CommandControlResult,
    _action: CommandControlAction,
    checkpoint_settled: bool,
) -> Value {
    let stderr = public_command_stderr(&result.stderr);
    let output = [result.stdout.as_str(), stderr.as_str()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(if result.stdout.is_empty() || stderr.is_empty() {
            ""
        } else {
            "\n"
        });
    let mut data = Map::new();
    data.insert(
        "status".into(),
        Value::String(result.status.as_str().into()),
    );
    data.insert(
        "session_id".into(),
        Value::String(result.public_session_id.as_str().to_string()),
    );
    data.insert("task_id".into(), Value::String(result.task_id.to_string()));
    data.insert("output".into(), Value::String(output));
    data.insert("elapsed_ms".into(), Value::from(result.elapsed_ms));
    if let Some(exit_code) = result.exit_code {
        data.insert("exit_code".into(), Value::from(exit_code));
    }
    if let Some(signal) = result.signal {
        data.insert("signal".into(), Value::String(signal));
    }
    if let Some(truncated) = result.truncated {
        data.insert("truncated".into(), Value::Bool(truncated));
    }
    if !checkpoint_settled {
        return stable_command_error(
            FacadeErrorCode::RuntimeUnavailable,
            "命令已终止，但工作流恢复状态不可用",
            data,
        );
    }

    match result.status {
        RuntimeCommandStatus::Running => stable_success(Value::Object(data), "Command running"),
        RuntimeCommandStatus::Completed => stable_success(Value::Object(data), "Command completed"),
        RuntimeCommandStatus::Cancelled => stable_success(Value::Object(data), "Command cancelled"),
        RuntimeCommandStatus::TimedOut => {
            stable_command_error(FacadeErrorCode::ProcessTimedOut, "Command timed out", data)
        }
        RuntimeCommandStatus::Failed => {
            stable_command_error(FacadeErrorCode::ProcessFailed, "Command failed", data)
        }
        RuntimeCommandStatus::Lost => {
            stable_command_error(FacadeErrorCode::SessionUnavailable, "Command lost", data)
        }
    }
}

fn task_control_snapshot_with_terminal(
    status: &CurrentTaskStatus,
    _executions: &ExecutionRegistry,
) -> Value {
    let mut data = task_control_snapshot(status);
    if let Some(object) = data.as_object_mut() {
        object.insert("availability".into(), Value::String("stale".into()));
    }
    data
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn workflow_activity_value(workflow: &Value) -> Value {
    json!({
        "task_id":workflow.get("task_id").cloned().unwrap_or(Value::Null),
        "kind":"other",
        "state":workflow.get("state").cloned().unwrap_or_else(|| Value::String("waiting".into())),
        "summary":Value::Null,
        "elapsed_ms":Value::Null,
        "step":workflow.get("current_step").cloned().unwrap_or(Value::Null),
        "next_step":workflow.get("next_step").cloned().unwrap_or(Value::Null),
        "progress_current":workflow.get("progress_current").cloned().unwrap_or(Value::Null),
        "progress_total":workflow.get("progress_total").cloned().unwrap_or(Value::Null)
    })
}

fn merge_control_plane_activity(
    mut aggregate: Value,
    tasks: &TaskRegistry,
    executions: &ExecutionRegistry,
    scheduler: &Scheduler,
) -> Value {
    let active_task = tasks.latest_active();
    let running_execution = executions.latest_running();
    let current_workflow = aggregate
        .get("current_workflow")
        .filter(|value| !value.is_null())
        .cloned();
    let current_activity = active_task
        .as_ref()
        .map(registered_task_activity_value)
        .or_else(|| {
            running_execution
                .as_ref()
                .map(running_execution_activity_value)
        })
        .or_else(|| current_workflow.as_ref().map(workflow_activity_value));
    let terminal_execution = executions.latest_terminal();
    let running_task_ids = executions
        .running()
        .into_iter()
        .map(|execution| execution.task_id)
        .collect();
    let terminal_task = tasks.latest_terminal_excluding(&running_task_ids);
    let last_activity =
        latest_registry_activity(terminal_task.as_ref(), terminal_execution.as_ref());

    if let Some(object) = aggregate.as_object_mut() {
        for legacy_projection in [
            "current_workflow",
            "current_command",
            "last_command",
            "last_terminal_command",
            "last_tool",
            "task_id",
            "execution_id",
            "kind",
            "execution_state",
            "summary",
            "current_step",
            "next_step",
        ] {
            object.remove(legacy_projection);
        }
        object.insert(
            "current_activity".into(),
            current_activity.clone().unwrap_or(Value::Null),
        );
        object.insert("last_activity".into(), last_activity.unwrap_or(Value::Null));
        let scheduler = scheduler.snapshot();
        object.insert(
            "scheduler".into(),
            json!({
                "observation_active":scheduler.observation_active,
                "control_active":scheduler.control_active,
                "foreground_work_running":scheduler.work_running,
                "queue_depth":scheduler.work_queued,
                "queue_capacity":scheduler.work_capacity,
                "detached_executions_running":executions.running().len(),
                "rejected_total":scheduler.rejected_total
            }),
        );
        object.insert(
            "state".into(),
            Value::String(
                match current_activity
                    .as_ref()
                    .and_then(|value| value.get("state"))
                    .and_then(Value::as_str)
                {
                    Some("queued" | "waiting") => "waiting",
                    Some(_) => "active",
                    None => "idle",
                }
                .into(),
            ),
        );
    }
    aggregate
}

fn registered_task_activity_value(task: &TaskRecord) -> Value {
    let state = match task.lifecycle {
        LifecycleState::Queued => "queued",
        LifecycleState::Running => "running",
        LifecycleState::Terminal(_) => "terminal",
    };
    json!({
        "task_id":task.id,
        "kind":activity_kind_name(task.kind),
        "state":state,
        "summary":task.summary.as_deref(),
        "elapsed_ms":unix_time_ms().saturating_sub(task.created_at_ms),
        "step":Value::Null,
        "progress_current":Value::Null,
        "progress_total":Value::Null
    })
}

fn running_execution_activity_value(execution: &ExecutionRecord) -> Value {
    json!({
        "task_id":execution.task_id,
        "execution_id":execution.id,
        "kind":"command",
        "state":"running",
        "summary":Value::Null,
        "elapsed_ms":unix_time_ms().saturating_sub(execution.started_at_ms),
        "step":Value::Null,
        "progress_current":Value::Null,
        "progress_total":Value::Null
    })
}

fn latest_registry_activity(
    task: Option<&TaskRecord>,
    execution: Option<&ExecutionRecord>,
) -> Option<Value> {
    let task_at = task.map(|task| task.updated_at_ms).unwrap_or(0);
    let execution_at = execution
        .and_then(|execution| match &execution.state {
            ExecutionState::Terminal(terminal) => Some(terminal.completed_at_ms),
            _ => None,
        })
        .unwrap_or(0);
    if execution_at >= task_at && execution_at > 0 {
        let execution = execution?;
        let ExecutionState::Terminal(terminal) = &execution.state else {
            return None;
        };
        return Some(json!({
            "task_id":execution.task_id,
            "execution_id":execution.id,
            "session_id":execution.public_session_id,
            "kind":"command",
            "summary":Value::Null,
            "outcome":terminal.outcome.as_str(),
            "completed_at_ms":terminal.completed_at_ms,
            "exit_code":terminal.exit_code,
            "signal":terminal.signal,
            "output_refs":terminal.output_refs,
            "error_code":terminal.error_code
        }));
    }
    let task = task?;
    let LifecycleState::Terminal(outcome) = task.lifecycle else {
        return None;
    };
    Some(json!({
        "task_id":task.id,
        "kind":activity_kind_name(task.kind),
        "summary":task.summary.as_deref(),
        "outcome":outcome.as_str(),
        "completed_at_ms":task.updated_at_ms
    }))
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
    current_task: &RegisteredTaskProjection,
    kind: TaskKind,
    state: TaskExecutionState,
) {
    current_task.project(
        CurrentTaskStatus::project(kind, SafeTaskSummary::Omitted, state)
            .expect("filesystem task state is valid"),
    );
}

fn finish_filesystem_task(
    current_task: &RegisteredTaskProjection,
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
        requests,
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
    let registry_key = request_key_from_json(McpSessionId::new(session), &id)
        .expect("validated downstream request id");
    if requests
        .register(
            registry_key.clone(),
            RequestCancellationTarget::WorkspaceFilesystem(cancellation.clone()),
        )
        .is_err()
    {
        return write_rpc_error(
            stream,
            id,
            -32600,
            "Duplicate active request id in MCP session",
            Some(session),
        );
    }
    project_filesystem_task(current_task, kind, TaskExecutionState::Running);

    let result =
        run_workspace_filesystem_with_authority(workspace_authority, arguments, cancellation);
    requests.remove(&registry_key);
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

fn project_elevated_task(current_task: &RegisteredTaskProjection, state: TaskExecutionState) {
    current_task.project(
        CurrentTaskStatus::project(TaskKind::ElevatedOperation, SafeTaskSummary::Omitted, state)
            .expect("elevated task state is a valid active task state"),
    );
}

fn finish_elevated_task(
    current_task: &RegisteredTaskProjection,
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
        requests,
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
                .unwrap_or(crate::execution::policy::DenyReason::PrivilegedRouteNotAvailable),
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
    let registry_key = request_key_from_json(McpSessionId::new(session), &id)
        .expect("validated downstream request id");
    if requests
        .register(
            registry_key.clone(),
            RequestCancellationTarget::PrivilegedExecution(broker_request_id.clone()),
        )
        .is_err()
    {
        return write_rpc_error(
            stream,
            id,
            -32600,
            "Duplicate active request id in MCP session",
            Some(session),
        );
    }
    project_elevated_task(current_task, TaskExecutionState::Running);

    if let Err(error) = privileged.start_execute(broker_request_id.clone(), spec) {
        requests.remove(&registry_key);
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
    requests.remove(&registry_key);

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
    current_task.finish(match execution.outcome {
        ElevatedExecOutcome::Completed => TerminalOutcome::Completed,
        ElevatedExecOutcome::TimedOut => TerminalOutcome::TimedOut,
        ElevatedExecOutcome::Cancelled => TerminalOutcome::Cancelled,
    });
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

fn rpc_request_id_from_json(value: &Value) -> Option<RpcRequestId> {
    match value {
        Value::String(value) => Some(RpcRequestId::String(value.clone())),
        Value::Number(value) => value.as_i64().map(RpcRequestId::Number),
        _ => None,
    }
}

fn rpc_request_id_to_json(value: &RpcRequestId) -> Value {
    match value {
        RpcRequestId::Number(value) => Value::from(*value),
        RpcRequestId::String(value) => Value::String(value.clone()),
    }
}

fn request_key_from_json(session_id: McpSessionId, request_id: &Value) -> Option<RequestKey> {
    rpc_request_id_from_json(request_id).map(|request_id| RequestKey::new(session_id, request_id))
}

fn command_control_public_session(name: &str, arguments: &Value) -> Option<PublicSessionId> {
    if name != "command_control" {
        return None;
    }
    let action = arguments.get("action").and_then(Value::as_str)?;
    if !matches!(action, "adopt" | "poll" | "write" | "kill") {
        return None;
    }
    arguments
        .get("session_id")
        .and_then(Value::as_str)
        .map(PublicSessionId::new)
}

fn is_work_tool(name: &str) -> bool {
    matches!(
        name,
        "agent_workflow"
            | "filesystem"
            | "exec_command"
            | "git_workflow"
            | "document_workflow"
            | "view_image"
            | "elevated_exec"
    )
}

fn scheduler_lane(name: &str, arguments: &Value) -> SchedulerLane {
    match name {
        "workspace_context" => SchedulerLane::Observation,
        "task_control" => match arguments.get("action").and_then(Value::as_str) {
            Some("cancel") => SchedulerLane::Control,
            _ => SchedulerLane::Observation,
        },
        "command_control" => SchedulerLane::Control,
        _ if is_work_tool(name) => SchedulerLane::Work,
        _ => SchedulerLane::Observation,
    }
}

fn task_terminal_outcome(result: &Result<Value, FacadeCallError>) -> TerminalOutcome {
    let Ok(value) = result else {
        return TerminalOutcome::Blocked;
    };
    if value.get("isError").and_then(Value::as_bool) != Some(true) {
        return TerminalOutcome::Completed;
    }
    match value
        .pointer("/structuredContent/error/code")
        .and_then(Value::as_str)
    {
        Some("ProcessCancelled") => TerminalOutcome::Cancelled,
        Some("ProcessTimedOut") => TerminalOutcome::TimedOut,
        Some("SessionUnavailable") => TerminalOutcome::Lost,
        Some("RuntimeUnavailable" | "RuntimeProtocolMismatch" | "RuntimeCapabilityMismatch") => {
            TerminalOutcome::Failed
        }
        Some(
            "WorkspaceDenied"
            | "CapabilityDenied"
            | "PolicyDenied"
            | "ElevationRequired"
            | "ElevatedOperationNotReviewed"
            | "PrivilegedRouteUnavailable",
        ) => TerminalOutcome::Blocked,
        _ => TerminalOutcome::Failed,
    }
}

fn normalize_accepted_request_cancellation(
    tool_name: &str,
    result: Result<Value, FacadeCallError>,
) -> Result<Value, FacadeCallError> {
    if tool_name == "exec_command" {
        let mut data = result
            .as_ref()
            .ok()
            .and_then(|value| value.pointer("/structuredContent/data"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        data.insert("status".into(), Value::String("cancelled".into()));
        return Ok(stable_command_error(
            FacadeErrorCode::ProcessCancelled,
            "Command cancelled",
            data,
        ));
    }
    Ok(
        FacadeError::new(FacadeErrorCode::ProcessCancelled, "Task cancelled", false)
            .to_mcp_result(),
    )
}

fn operation_error_from_facade_result(
    result: &Result<Value, FacadeCallError>,
) -> Option<OperationError> {
    match result {
        Err(FacadeCallError::Denied(_)) => Some(OperationError::new(
            "PolicyDenied",
            ErrorCategory::Authorization,
            "request was denied by policy",
            false,
        )),
        Ok(value) if value.get("isError").and_then(Value::as_bool) == Some(true) => {
            let code = value
                .pointer("/structuredContent/error/code")
                .and_then(Value::as_str)
                .unwrap_or("Internal");
            let retryable = value
                .pointer("/structuredContent/error/retryable")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let category = match code {
                "QueueCapacityExceeded" => ErrorCategory::Capacity,
                "WorkspaceDenied"
                | "CapabilityDenied"
                | "PolicyDenied"
                | "ElevationRequired"
                | "ElevatedOperationNotReviewed"
                | "PrivilegedRouteUnavailable" => ErrorCategory::Authorization,
                "ProcessTimedOut" => ErrorCategory::Timeout,
                "RuntimeUnavailable" | "SessionUnavailable" => ErrorCategory::Unavailable,
                "InvalidArgument" => ErrorCategory::Validation,
                "PatchConflict" | "FileChanged" | "AmbiguousMatch" => ErrorCategory::Conflict,
                _ => ErrorCategory::Internal,
            };
            Some(OperationError::new(
                code,
                category,
                "request failed",
                retryable,
            ))
        }
        Ok(_) => None,
    }
}

fn public_session_from_result(tool_name: &str, result: &Value) -> Option<PublicSessionId> {
    let data = result.pointer("/structuredContent/data")?;
    if tool_name == "exec_command" {
        if let Some(public_session) = data
            .get("session_id")
            .and_then(Value::as_str)
            .map(PublicSessionId::new)
        {
            return Some(public_session);
        }
    }
    None
}

fn valid_downstream_request_id(request_id: &Value) -> bool {
    rpc_request_id_from_json(request_id).is_some()
}

fn registered_request_for_transport_cancel(
    requests: &RequestRegistry,
    session_id: &McpSessionId,
    request_id: &RpcRequestId,
) -> Option<ActiveRequest> {
    requests.request_cancellation(&RequestKey::new(session_id.clone(), request_id.clone()))
}

fn next_private_request_id() -> RpcRequestId {
    let generation = PRIVATE_REQUEST_GENERATION.fetch_add(1, Ordering::Relaxed);
    RpcRequestId::String(format!("lb-private-{generation:x}"))
}

fn cancel_registered_request(
    request: &ActiveRequest,
    cancellation: &McpCancellationClient,
    privileged: Option<&Arc<dyn PrivilegedExecution>>,
) -> Result<(), ()> {
    match &request.cancellation {
        RequestCancellationTarget::Runtime(request_id) => cancellation
            .cancel_request(&rpc_request_id_to_json(request_id))
            .map_err(|_| ()),
        RequestCancellationTarget::WorkspaceFilesystem(cancellation) => {
            cancellation.cancel();
            Ok(())
        }
        RequestCancellationTarget::PrivilegedExecution(broker_request_id) => privileged
            .ok_or(())?
            .cancel_execute(broker_request_id.clone())
            .map_err(|_| ()),
    }
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
    respond: bool,
}

impl HttpReadError {
    const fn new(status: u16, cause: &'static str) -> Self {
        Self {
            status,
            cause,
            respond: true,
        }
    }

    const fn disconnected_before_request() -> Self {
        Self {
            status: 400,
            cause: "connection_closed_before_request",
            respond: false,
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, HttpReadError> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(HttpReadError::new(431, "header_too_large"));
        }
        let count = match stream.read(&mut chunk) {
            Ok(count) => count,
            Err(_) if bytes.is_empty() => {
                return Err(HttpReadError::disconnected_before_request());
            }
            Err(_) => return Err(HttpReadError::new(400, "socket_read_failure")),
        };
        if count == 0 {
            if bytes.is_empty() {
                return Err(HttpReadError::disconnected_before_request());
            }
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
    let mut content_length = None;
    let mut chunked = false;
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or(HttpReadError::new(400, "malformed_request"))?;
        let name = name.trim().to_string();
        let value = value.trim().to_string();
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| HttpReadError::new(400, "malformed_request"))?;
            if content_length.replace(parsed).is_some() {
                return Err(HttpReadError::new(400, "ambiguous_body_framing"));
            }
            if parsed > MAX_BODY_BYTES {
                return Err(HttpReadError::new(413, "body_too_large"));
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if chunked || !value.eq_ignore_ascii_case("chunked") {
                return Err(HttpReadError::new(400, "unsupported_transfer_encoding"));
            }
            chunked = true;
        }
        headers.push((name, value));
    }
    if chunked && content_length.is_some() {
        return Err(HttpReadError::new(400, "ambiguous_body_framing"));
    }
    let body = if chunked {
        read_chunked_body(stream, bytes[header_end..].to_vec())?
    } else {
        read_content_length_body(
            stream,
            bytes[header_end..].to_vec(),
            content_length.unwrap_or(0),
        )?
    };
    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn read_content_length_body(
    stream: &mut TcpStream,
    mut body: Vec<u8>,
    content_length: usize,
) -> Result<Vec<u8>, HttpReadError> {
    if body.len() > content_length {
        body.truncate(content_length);
    }
    let mut chunk = [0u8; 4096];
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
    Ok(body)
}

fn read_chunked_body(stream: &mut TcpStream, mut wire: Vec<u8>) -> Result<Vec<u8>, HttpReadError> {
    let mut decoded = Vec::new();
    let mut cursor = 0usize;
    let mut scratch = [0u8; 4096];

    fn read_more(
        stream: &mut TcpStream,
        wire: &mut Vec<u8>,
        scratch: &mut [u8],
    ) -> Result<(), HttpReadError> {
        if wire.len() >= MAX_CHUNKED_WIRE_BYTES {
            return Err(HttpReadError::new(413, "body_too_large"));
        }
        let limit = scratch
            .len()
            .min(MAX_CHUNKED_WIRE_BYTES.saturating_sub(wire.len()));
        let count = stream
            .read(&mut scratch[..limit])
            .map_err(|_| HttpReadError::new(400, "socket_read_failure"))?;
        if count == 0 {
            return Err(HttpReadError::new(400, "early_eof"));
        }
        wire.extend_from_slice(&scratch[..count]);
        Ok(())
    }

    loop {
        let line_end = loop {
            if let Some(relative) = wire[cursor..]
                .windows(2)
                .position(|window| window == b"\r\n")
            {
                break cursor + relative;
            }
            if wire.len().saturating_sub(cursor) > 128 {
                return Err(HttpReadError::new(400, "malformed_chunked_body"));
            }
            read_more(stream, &mut wire, &mut scratch)?;
        };
        let size_line = std::str::from_utf8(&wire[cursor..line_end])
            .map_err(|_| HttpReadError::new(400, "malformed_chunked_body"))?;
        let size_token = size_line.split(';').next().unwrap_or_default().trim();
        if size_token.is_empty() || size_token.len() > 16 {
            return Err(HttpReadError::new(400, "malformed_chunked_body"));
        }
        let size = usize::from_str_radix(size_token, 16)
            .map_err(|_| HttpReadError::new(400, "malformed_chunked_body"))?;
        cursor = line_end + 2;
        if size == 0 {
            loop {
                let trailer_end = loop {
                    if let Some(relative) = wire[cursor..]
                        .windows(2)
                        .position(|window| window == b"\r\n")
                    {
                        break cursor + relative;
                    }
                    if wire.len().saturating_sub(cursor) > MAX_HEADER_BYTES {
                        return Err(HttpReadError::new(431, "header_too_large"));
                    }
                    read_more(stream, &mut wire, &mut scratch)?;
                };
                if trailer_end == cursor {
                    return Ok(decoded);
                }
                let trailer = std::str::from_utf8(&wire[cursor..trailer_end])
                    .map_err(|_| HttpReadError::new(400, "malformed_chunked_body"))?;
                if !trailer.contains(':') {
                    return Err(HttpReadError::new(400, "malformed_chunked_body"));
                }
                cursor = trailer_end + 2;
            }
        }
        if decoded.len().saturating_add(size) > MAX_BODY_BYTES {
            return Err(HttpReadError::new(413, "body_too_large"));
        }
        let required = cursor
            .checked_add(size)
            .and_then(|end| end.checked_add(2))
            .ok_or(HttpReadError::new(413, "body_too_large"))?;
        while wire.len() < required {
            read_more(stream, &mut wire, &mut scratch)?;
        }
        if &wire[cursor + size..required] != b"\r\n" {
            return Err(HttpReadError::new(400, "malformed_chunked_body"));
        }
        decoded.extend_from_slice(&wire[cursor..cursor + size]);
        cursor = required;
    }
}

fn new_session_id() -> McpSessionId {
    let generation = SESSION_GENERATION.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    McpSessionId::new(format!(
        "lb-{:x}-{nanos:x}-{generation:x}",
        std::process::id()
    ))
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
    use super::super::test_support::*;
    use super::*;

    #[test]
    fn task_cancel_selection_is_explicit_and_session_local() {
        let task_a = TaskId::new("task-a");
        let task_b = TaskId::new("task-b");
        let candidates = BTreeSet::from([task_a.clone(), task_b.clone()]);

        assert_eq!(
            select_cancellable_task(None, candidates.clone())
                .unwrap_err()
                .code,
            FacadeErrorCode::TaskIdRequired
        );
        assert_eq!(
            select_cancellable_task(Some(TaskId::new("foreign")), candidates.clone())
                .unwrap_err()
                .code,
            FacadeErrorCode::TaskNotOwned
        );
        assert_eq!(
            select_cancellable_task(Some(task_a.clone()), candidates).unwrap(),
            Some(task_a.clone())
        );
        assert_eq!(
            select_cancellable_task(None, BTreeSet::from([task_a.clone()])).unwrap(),
            Some(task_a)
        );
        assert_eq!(
            select_cancellable_task(None, BTreeSet::new())
                .unwrap_err()
                .code,
            FacadeErrorCode::NotFound
        );
    }

    #[test]
    fn runtime_unavailable_is_a_failed_task_not_a_lost_task() {
        let runtime_unavailable: Result<Value, FacadeCallError> = Ok(FacadeError::new(
            FacadeErrorCode::RuntimeUnavailable,
            "runtime is unavailable before execution starts",
            true,
        )
        .to_mcp_result());
        assert_eq!(
            task_terminal_outcome(&runtime_unavailable),
            TerminalOutcome::Failed
        );

        let previously_owned_session_lost: Result<Value, FacadeCallError> = Ok(FacadeError::new(
            FacadeErrorCode::SessionUnavailable,
            "an accepted session can no longer be observed",
            false,
        )
        .to_mcp_result());
        assert_eq!(
            task_terminal_outcome(&previously_owned_session_lost),
            TerminalOutcome::Lost
        );
    }

    fn http_read_error(raw: &[u8]) -> HttpReadError {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client.write_all(raw).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        read_request(&mut server).unwrap_err()
    }

    fn http_read(raw: &[u8]) -> HttpRequest {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client.write_all(raw).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        read_request(&mut server).unwrap()
    }

    #[test]
    fn schema42_mcp_http_failures_have_distinct_transport_causes() {
        assert_eq!(
            http_read_error(b"BROKEN\r\n\r\n").cause,
            "malformed_request"
        );
        let chunked = http_read(
            b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"a\r\n3\r\n\":1\r\n1\r\n}\r\n0\r\n\r\n",
        );
        assert_eq!(chunked.body, br#"{"a":1}"#);
        assert_eq!(
            http_read_error(b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n").cause,
            "unsupported_transfer_encoding"
        );
        assert_eq!(
            http_read_error(
                b"POST /mcp HTTP/1.1\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
            )
            .cause,
            "ambiguous_body_framing"
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
        let disconnected = read_request(&mut server).unwrap_err();
        assert_eq!(disconnected.cause, "connection_closed_before_request");
        assert!(!disconnected.respond);
    }

    #[test]
    fn accepted_connection_waits_for_delayed_request_bytes() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let (mut server, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "listener did not accept client");
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("listener accept failed: {error}"),
            }
        };
        let writer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            client
                .write_all(b"GET /mcp HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        configure_accepted_stream(&server).unwrap();
        let request = read_request(&mut server).expect("accepted connection waits for HTTP bytes");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/mcp");
        writer.join().unwrap();
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

    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    #[cfg(windows)]
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    #[cfg(windows)]
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SUSPEND_RESUME};

    use super::super::runtime::{
        CodingToolsPermissionMode, CodingToolsRuntimeConfig, InternalBearer,
    };

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
        let store = ExecutionRegistry::open_at(workspace.join("owned-terminal.json")).unwrap();
        let a = store
            .start(
                TaskId::new("workflow-a"),
                PublicSessionId::new("lb-session-a"),
            )
            .unwrap();
        store
            .finish(
                &a,
                ExecutionTerminal {
                    outcome: TerminalOutcome::Completed,
                    exit_code: Some(0),
                    signal: None,
                    output_refs: vec!["lb-output-a".into()],
                    error_code: None,
                    completed_at_ms: unix_time_ms(),
                },
            )
            .unwrap();
        let b = store
            .start(
                TaskId::new("direct-b"),
                PublicSessionId::new("lb-session-b"),
            )
            .unwrap();
        store
            .finish(
                &b,
                ExecutionTerminal {
                    outcome: TerminalOutcome::TimedOut,
                    exit_code: None,
                    signal: None,
                    output_refs: vec!["lb-output-b".into()],
                    error_code: Some("ProcessTimedOut".into()),
                    completed_at_ms: unix_time_ms(),
                },
            )
            .unwrap();
        let owned_terminal = store.latest_terminal_for_task(&TaskId::new("workflow-a"));
        let data = latest_registry_activity(None, owned_terminal.as_ref()).unwrap();
        assert_eq!(data["task_id"], "workflow-a");
        assert_eq!(data["session_id"], "lb-session-a");
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn task_control_get_reads_durable_terminal_without_private_session() {
        let workspace = temp_workspace();
        let path = workspace.join("durable-command-state.json");
        {
            let store = ExecutionRegistry::open_at(path.clone()).unwrap();
            let execution = store
                .start(
                    TaskId::new("task-durable"),
                    PublicSessionId::new("lb-session-durable"),
                )
                .unwrap();
            store
                .finish(
                    &execution,
                    ExecutionTerminal {
                        outcome: TerminalOutcome::TimedOut,
                        exit_code: Some(124),
                        signal: Some("TERM".to_string()),
                        output_refs: vec!["lb-output-durable".to_string()],
                        error_code: Some("ProcessTimedOut".to_string()),
                        completed_at_ms: unix_time_ms(),
                    },
                )
                .unwrap();
        }

        // Reopen from disk: there is deliberately no private runtime/session object here.
        let reopened = ExecutionRegistry::open_at(path).unwrap();
        let data = merge_control_plane_activity(
            json!({"state":"idle"}),
            &TaskRegistry::default(),
            &reopened,
            &Scheduler::default(),
        );
        let terminal = &data["last_activity"];
        assert_eq!(terminal["task_id"], "task-durable");
        assert_eq!(terminal["session_id"], "lb-session-durable");
        assert_eq!(terminal["outcome"], "timed_out");
        assert_eq!(terminal["exit_code"], 124);
        assert_eq!(terminal["output_refs"][0], "lb-output-durable");
        assert_eq!(terminal["error_code"], "ProcessTimedOut");
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn completed_foreground_task_restores_detached_execution_projection() {
        let workspace = temp_workspace();
        let executions =
            ExecutionRegistry::open_at(workspace.join("projection-executions.json")).unwrap();
        let execution_a = executions
            .start(TaskId::new("task-a"), PublicSessionId::new("session-a"))
            .unwrap();
        let tasks = TaskRegistry::default();
        let owner = McpSessionId::new("mcp-b");
        let task_b = tasks.queue(
            owner.clone(),
            RequestKey::new(owner, RpcRequestId::Number(2)),
            TaskKind::ReadFile,
            SafeTaskSummary::from_untrusted("read file"),
        );
        tasks.mark_running(&task_b).unwrap();

        let base = json!({"state":"idle","current_workflow":null});
        let scheduler = Scheduler::default();
        let foreground =
            merge_control_plane_activity(base.clone(), &tasks, &executions, &scheduler);
        assert_eq!(
            foreground["current_activity"]["task_id"],
            task_b.to_string()
        );
        assert_eq!(foreground["current_activity"]["kind"], "read");

        tasks.finish(&task_b, TerminalOutcome::Completed).unwrap();
        let restored = merge_control_plane_activity(base, &tasks, &executions, &scheduler);
        assert_eq!(
            restored["current_activity"]["execution_id"],
            execution_a.to_string()
        );
        assert_eq!(restored["current_activity"]["state"], "running");
        assert!(restored.get("current_command").is_none());
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn current_task_projection_is_a_read_only_task_registry_view() {
        let tasks = TaskRegistry::default();
        let projection = CurrentTaskProjection::new(tasks.clone(), None);
        let initial = projection.timing_snapshot();
        assert_eq!(initial.status, CurrentTaskStatus::Idle);
        assert_eq!(initial.elapsed_ms, None);
        assert_eq!(initial.last_tool, None);

        let owner = McpSessionId::new("projection-owner");
        let task_id = tasks.queue(
            owner.clone(),
            RequestKey::new(owner, RpcRequestId::Number(1)),
            TaskKind::ModifyFile,
            SafeTaskSummary::from_untrusted("write probe.txt"),
        );
        assert_eq!(projection.snapshot(), CurrentTaskStatus::Idle);
        tasks.mark_running(&task_id).unwrap();
        assert!(matches!(
            projection.snapshot(),
            CurrentTaskStatus::Active(_)
        ));
        tasks.finish(&task_id, TerminalOutcome::Completed).unwrap();
        let finished = projection.timing_snapshot();
        assert_eq!(finished.status, CurrentTaskStatus::Idle);
        assert_eq!(finished.elapsed_ms, None);
        assert_eq!(
            finished.last_tool.as_ref().map(|tool| tool.kind),
            Some(TaskKind::ModifyFile)
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

    #[test]
    fn workspace_authority_separates_desired_observed_and_effective_permission() {
        let fake = FakePrivilegedExecution::active();
        fake.set_state(PrivilegeState::Requested);
        let privileged: Arc<dyn PrivilegedExecution> = Arc::new(fake);
        let tasks = TaskRegistry::default();
        let workspace = temp_workspace();
        let executions =
            ExecutionRegistry::open_at(workspace.join("authority-executions.json")).unwrap();
        let scheduler = Scheduler::default();

        let mut full = stable_success(json!({"current_task":{"state":"idle"}}), "context");
        enrich_workspace_context_privilege(
            &mut full,
            PermissionMode::Full,
            Some(&privileged),
            &tasks,
            &executions,
            &scheduler,
        );
        let full_data = &full["structuredContent"]["data"];
        assert_eq!(full_data["privilege_state"], "disabled");
        assert_eq!(full_data["authority"]["desired_permission"], "full");
        assert_eq!(full_data["authority"]["observed_privilege"], "requested");
        assert_eq!(full_data["authority"]["effective_permission"], "full");
        assert_eq!(full_data["authority"]["reconciliation"], "disable_pending");
        assert_eq!(full_data["elevated_route_available"], false);

        let mut elevated = stable_success(json!({"current_task":{"state":"idle"}}), "context");
        enrich_workspace_context_privilege(
            &mut elevated,
            PermissionMode::Elevated,
            Some(&privileged),
            &tasks,
            &executions,
            &scheduler,
        );
        let elevated_data = &elevated["structuredContent"]["data"];
        assert_eq!(elevated_data["privilege_state"], "requested");
        assert_eq!(elevated_data["authority"]["effective_permission"], "full");
        assert_eq!(
            elevated_data["authority"]["reconciliation"],
            "awaiting_authorization"
        );
        assert_eq!(elevated_data["elevated_route_available"], false);
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn r1_transport_cancel_lookup_is_scoped_by_mcp_session() {
        let requests = RequestRegistry::default();
        let raw_id = RpcRequestId::Number(1);
        let session_a = McpSessionId::new("session-a");
        let session_b = McpSessionId::new("session-b");
        requests
            .register(
                RequestKey::new(session_a.clone(), raw_id.clone()),
                RequestCancellationTarget::Runtime(RpcRequestId::String("upstream-a".into())),
            )
            .unwrap();
        requests
            .register(
                RequestKey::new(session_b.clone(), raw_id.clone()),
                RequestCancellationTarget::Runtime(RpcRequestId::String("upstream-b".into())),
            )
            .unwrap();

        let selected_a =
            registered_request_for_transport_cancel(&requests, &session_a, &raw_id).unwrap();
        let selected_b =
            registered_request_for_transport_cancel(&requests, &session_b, &raw_id).unwrap();
        assert!(matches!(
            selected_a.cancellation,
            RequestCancellationTarget::Runtime(RpcRequestId::String(ref value)) if value == "upstream-a"
        ));
        assert!(matches!(
            selected_b.cancellation,
            RequestCancellationTarget::Runtime(RpcRequestId::String(ref value)) if value == "upstream-b"
        ));
    }

    #[test]
    fn schema42_special_handler_abort_closes_request_diagnostics() {
        crate::diagnostics::reset_request_diagnostics_for_test();
        record_mcp_request_start("special-abort", "session-special", "task_control");
        assert!(
            finalize_special_handler_request("special-abort", "session-special", Err(())).is_err()
        );
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
    fn desired_workspace_change_denies_work_until_runtime_observes_the_same_workspace() {
        let root = repo_root();
        let workspace_a = temp_workspace();
        let workspace_b = temp_workspace();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace_a,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP ready");
        let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
            .expect("workspace convergence PEP ready");
        let session = initialize(pep.port(), 650)
            .session
            .expect("downstream MCP session");

        pep.control_plane
            .desired()
            .set_workspace(Some(DesiredWorkspace::for_runtime_path(&workspace_b)));
        let denied = public_tool_call(
            pep.port(),
            &session,
            651,
            "filesystem",
            json!({"action":"read_file","path":"probe.txt"}),
        );
        assert_tool_error(&denied, "RuntimeUnavailable");
        assert_eq!(pep.control_plane.scheduler().snapshot().work_running, 0);
        assert_eq!(pep.control_plane.scheduler().snapshot().work_queued, 0);

        let _coding = pep.stop().unwrap();
        let _ = fs::remove_dir_all(workspace_a);
        let _ = fs::remove_dir_all(workspace_b);
    }

    #[test]
    fn full_cmd_rmdir_uses_current_user_authority_inside_and_outside_workspace() {
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
        assert_eq!(
            outside_response.body["result"]["isError"], false,
            "{:#?}",
            outside_response.body
        );
        assert!(
            !outside.exists(),
            "ordinary shell descendants must retain the same current-user authority"
        );

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
                    && event.connection_id == session
            })
            .expect("real tools/call request start diagnostic");
        let request_end = request_events
            .iter()
            .find(|event| {
                event.kind == crate::diagnostics::RequestDiagnosticKind::End
                    && event.connection_id == session
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
        let task_events = crate::diagnostics::request_diagnostics_for_test();
        let task_start = task_events
            .iter()
            .find(|event| {
                event.kind == crate::diagnostics::RequestDiagnosticKind::Start
                    && event.tool == "task_control"
                    && event.connection_id == session
            })
            .expect("task_control missing-action start diagnostic");
        let task_end = task_events
            .iter()
            .find(|event| {
                event.kind == crate::diagnostics::RequestDiagnosticKind::End
                    && event.connection_id == session
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

        let terminal_deadline = Instant::now() + Duration::from_secs(30);
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
        for arguments in [
            &["config", "user.email", "localbridge-test@example.invalid"][..],
            &["config", "user.name", "LocalBridge Test"][..],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(arguments)
                    .current_dir(&nested_project)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(nested_project.join("AGENTS.md"), b"before\n").unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["add", "AGENTS.md"])
                .current_dir(&nested_project)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["commit", "--quiet", "-m", "fixture"])
                .current_dir(&nested_project)
                .status()
                .unwrap()
                .success()
        );
        fs::write(nested_project.join("AGENTS.md"), b"after\n").unwrap();
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

        let fixture = PublicRuntimeFixture::start_in(workspace.clone(), PermissionMode::Full);
        let pep = fixture.runtime();
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
        let session = initialized.session.expect("schema28 downstream session");
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
                .is_some_and(|value| value.contains("resume may use the returned task_id"))
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
            json!(["adopt", "poll", "read", "write", "kill"])
        );
        assert!(
            first_turn["capabilities"].get("actions").is_none(),
            "workspace_context must not publish a second action catalog"
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
        let served_task_control = served_tools.body["result"]["tools"]
            .as_array()
            .and_then(|tools| tools.iter().find(|tool| tool["name"] == "task_control"))
            .expect("fresh serving instance exposes task_control");
        assert_eq!(
            served_task_control["inputSchema"]["properties"]["action"]["enum"],
            json!(["list", "get", "cancel"])
        );

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
        let missing_output = public_tool_call(
            pep.port(),
            &session,
            68811,
            "command_control",
            json!({"action":"read","output_ref":"lb-output-missing","stream":"stdout"}),
        );
        assert_eq!(
            missing_output.body["result"]["structuredContent"]["error"]["code"], "OutputNotFound",
            "an output handle lookup must not be classified as a command Session failure: {:#?}",
            missing_output.body
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
        assert_eq!(
            pep.current_task_projection().latest_snapshot(),
            CurrentTaskStatus::Idle,
            "foreground Task must finish after exec_command returns"
        );
        assert!(matches!(
            pep.control_plane
                .tasks()
                .latest_terminal()
                .map(|task| task.lifecycle),
            Some(LifecycleState::Terminal(TerminalOutcome::Completed))
        ));
        let detached = pep.task_aggregate_snapshot();
        assert_eq!(detached["current_activity"]["state"], "running");
        assert_eq!(detached["current_activity"]["kind"], "command");
        let lifecycle_deadline = Instant::now() + Duration::from_secs(30);
        while !pep.task_aggregate_snapshot()["current_activity"].is_null() {
            assert!(
                Instant::now() < lifecycle_deadline,
                "detached Execution did not reach a terminal outcome"
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
        let filtered_diff = public_tool_call(
            pep.port(),
            &session,
            6901,
            "git_workflow",
            json!({"action":"diff","path":"NestedProject","paths":["AGENTS.md"]}),
        );
        assert_eq!(
            filtered_diff.body["result"]["isError"], false,
            "{:#?}",
            filtered_diff.body
        );
        assert!(
            filtered_diff.body["result"]["structuredContent"]["data"]["diff"]
                .as_str()
                .is_some_and(|diff| diff.contains("AGENTS.md") && diff.contains("+after")),
            "{:#?}",
            filtered_diff.body
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
            cancel_after_failed_workflow.body["result"]["structuredContent"]["error"]["code"],
            "NotFound",
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
        let refreshed_full_session = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":6975,"method":"ping","params":{}}),
        );
        assert_eq!(refreshed_full_session.status, 200);
        assert_eq!(refreshed_full_session.body["result"], json!({}));
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
        let refreshed_edit_session = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":6977,"method":"ping","params":{}}),
        );
        assert_eq!(refreshed_edit_session.status, 200);
        assert_eq!(refreshed_edit_session.body["result"], json!({}));

        for (id, shell) in [(696u64, "windows_powershell"), (697u64, "auto")] {
            let baseline = public_tool_call(
                pep.port(),
                &session,
                id,
                "exec_command",
                json!({
                    "command":"$loc=(Get-Location).Path; $exists=Test-Path -LiteralPath '.'; $count=@(Get-ChildItem -LiteralPath '.').Count; Write-Output ('SCHEMA30_BASELINE '+$exists+' '+$count+' '+$loc)",
                    "shell":shell,
                    "yield_time_ms":0,
                    "timeout_ms":120000
                }),
            );
            let (baseline, output) =
                settle_public_command(pep.port(), &session, 10_000 + id, baseline);
            assert_eq!(
                baseline.body["result"]["isError"], false,
                "shell={shell}: {:#?}",
                baseline.body
            );
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
        let (autoload, autoload_output) =
            settle_public_command(pep.port(), &session, 10_699, autoload);
        assert_eq!(
            autoload.body["result"]["isError"], false,
            "{:#?}",
            autoload.body
        );
        assert!(
            autoload_output.contains("LB_GEN14_MODULE_AUTOLOAD_SENTINEL"),
            "ordinary PowerShell must retain the current user's native module surface: {autoload_output:?}"
        );

        let quoted = public_tool_call(
            pep.port(),
            &session,
            701,
            "exec_command",
            json!({
                "command":"Write-Output \"a|b\"; Write-Output \"a&b\"; Write-Output 'q|b'; Write-Output 'q&b'; Write-Output '中文输出✓'",
                "shell":"windows_powershell",
                "yield_time_ms":0
            }),
        );
        let (quoted, quoted_output) = settle_public_command(pep.port(), &session, 41_000, quoted);
        assert_eq!(
            quoted.body["result"]["isError"], false,
            "{:#?}",
            quoted.body
        );
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
                "yield_time_ms":0
            }),
        );
        let (powershell_error, powershell_error_output) =
            settle_public_command(pep.port(), &session, 42_000, powershell_error);
        assert_eq!(
            powershell_error.body["result"]["isError"], true,
            "{:#?}",
            powershell_error.body
        );
        let powershell_error_data = &powershell_error.body["result"]["structuredContent"]["data"];
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
        let mismatched_stream = public_tool_call(
            pep.port(),
            &session,
            70121,
            "command_control",
            json!({"action":"read","output_ref":stderr_ref,"stream":"stdout","offset":0,"limit":100}),
        );
        let mismatch_error = &mismatched_stream.body["result"]["structuredContent"]["error"];
        assert_eq!(mismatch_error["code"], "InvalidArgument");
        assert_eq!(mismatch_error["details"]["field"], "stream");
        assert_eq!(mismatch_error["details"]["expected"], "stderr");
        assert_eq!(mismatch_error["details"]["actual"], "stdout");
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
                "yield_time_ms":0
            }),
        );
        let (cmd_cd_switch, cmd_cd_output) =
            settle_public_command(pep.port(), &session, 43_000, cmd_cd_switch);
        assert_eq!(
            cmd_cd_switch.body["result"]["isError"], false,
            "{:#?}",
            cmd_cd_switch.body
        );
        assert!(
            cmd_cd_output.contains("LB_CMD_D_OK"),
            "{:#?}",
            cmd_cd_switch.body
        );
        let cmd_escape = public_tool_call(
            pep.port(),
            &session,
            7014,
            "exec_command",
            json!({
                "command":"cd /d C:\\Windows && cd",
                "shell":"cmd",
                "yield_time_ms":10000
            }),
        );
        assert_eq!(
            cmd_escape.body["result"]["isError"], false,
            "{:#?}",
            cmd_escape.body
        );
        assert!(
            cmd_escape.body["result"]["structuredContent"]["data"]["output"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains(r"c:\windows"),
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
                "yield_time_ms":0
            }),
        );
        let (auto_utf8, auto_utf8_output) =
            settle_public_command(pep.port(), &session, 44_000, auto_utf8);
        assert_eq!(
            auto_utf8.body["result"]["isError"], false,
            "{:#?}",
            auto_utf8.body
        );
        assert!(auto_utf8_output.contains("自动中文✓"));

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
        let past_eof = public_tool_call(
            pep.port(),
            &session,
            773,
            "document_workflow",
            json!({"action":"inspect","path":"range.txt","start_line":999,"end_line":1000}),
        );
        assert_eq!(
            past_eof.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
            "a document range beyond EOF must not produce start_line > end_line: {:#?}",
            past_eof.body
        );

        fixture.shutdown();
    }

    #[test]
    fn schema28_detached_command_lifecycle_is_incremental_and_durable() {
        let fixture = PublicRuntimeFixture::start(PermissionMode::Full);
        let pep = fixture.runtime();
        let (client, _) = PublicMcpClient::connect(pep.port(), 45_000);

        // Explicit stdin handshakes create causal output boundaries. This does
        // not assume a cold PowerShell process starts within a fixed delay or
        // that the OS schedules exactly one marker per transport poll.
        let mut command = client.start_detached_command(json!({
            "command":"Write-Output 'poll-1'; $null=[Console]::In.ReadLine(); Write-Output 'poll-2'; $null=[Console]::In.ReadLine(); Write-Output 'poll-3'; $line=[Console]::In.ReadLine(); Write-Output ('write:'+ $line); Start-Sleep -Seconds 30",
            "shell":"windows_powershell",
            "yield_time_ms":0,
            "timeout_ms":120000
        }));
        let public_session = command.session_id().to_string();

        for (marker, input) in [
            ("poll-1", Some("step-2\n")),
            ("poll-2", Some("step-3\n")),
            ("poll-3", Some("after-start\n")),
            ("write:after-start", None),
        ] {
            command.wait_for_output(marker, Duration::from_secs(120));
            assert_eq!(
                command.output().matches(marker).count(),
                1,
                "output was replayed: {:?}",
                command.output()
            );
            command.assert_next_poll_empty();
            if let Some(input) = input {
                command.write(input, 1_000);
            }
        }

        let killed = command.kill("TERM", 1_000);
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

        for _ in 0..3 {
            let terminal = client.call_tool(
                "command_control",
                json!({"action":"poll","session_id":public_session}),
            );
            assert_eq!(terminal.body["result"]["structuredContent"]["ok"], true);
            assert!(terminal.body["result"]["structuredContent"]["error"].is_null());
            assert_eq!(
                terminal.body["result"]["structuredContent"]["data"]["status"],
                "cancelled"
            );
            assert_eq!(
                terminal.body["result"]["structuredContent"]["data"]["output"],
                ""
            );
        }

        let durable_terminal = client.call_tool("task_control", json!({"action":"get"}));
        let durable =
            &durable_terminal.body["result"]["structuredContent"]["data"]["last_activity"];
        assert_eq!(durable["session_id"], public_session);
        assert_eq!(durable["outcome"], "cancelled");
        assert_eq!(durable["error_code"], "ProcessCancelled");
        assert!(
            !serde_json::to_string(durable).unwrap().contains("PRIVATE_"),
            "durable terminal task-state leaked a private handle: {durable:#?}"
        );

        fixture.shutdown();
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

        let absolute_redirect_target = workspace.join("absolute-redirect.txt");
        let absolute_redirect = public_tool_call(
            pep.port(),
            &session,
            816,
            "exec_command",
            json!({
                "command":format!(r#"echo ABSOLUTE_REDIRECT_OK>"{}""#, absolute_redirect_target.display()),
                "shell":"cmd",
                "workdir":workspace.to_string_lossy(),
                "yield_time_ms":10000
            }),
        );
        assert_eq!(
            absolute_redirect.body["result"]["isError"], false,
            "absolute in-workspace redirection failed: {}",
            absolute_redirect.body
        );

        let relative_redirect = public_tool_call(
            pep.port(),
            &session,
            817,
            "exec_command",
            json!({
                "command":"echo RELATIVE_REDIRECT_OK>relative-redirect.txt",
                "shell":"cmd",
                "workdir":workspace.to_string_lossy(),
                "yield_time_ms":10000
            }),
        );
        assert_eq!(
            relative_redirect.body["result"]["isError"], false,
            "relative in-workspace redirection failed: {}",
            relative_redirect.body
        );
        assert_eq!(
            fs::read_to_string(&absolute_redirect_target)
                .unwrap()
                .trim(),
            "ABSOLUTE_REDIRECT_OK"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("relative-redirect.txt"))
                .unwrap()
                .trim(),
            "RELATIVE_REDIRECT_OK"
        );

        let outside_redirect_target = outside.join("current-user-redirect.txt");
        let outside_redirect = public_tool_call(
            pep.port(),
            &session,
            818,
            "exec_command",
            json!({
                "command":format!(r#"echo CURRENT_USER_WRITE>"{}""#, outside_redirect_target.display()),
                "shell":"cmd",
                "workdir":workspace.to_string_lossy(),
                "yield_time_ms":10000
            }),
        );
        assert_eq!(
            outside_redirect.body["result"]["isError"], false,
            "ordinary shell must not pretend to enforce structured workspace authority: {}",
            outside_redirect.body
        );
        assert_eq!(
            fs::read_to_string(&outside_redirect_target).unwrap().trim(),
            "CURRENT_USER_WRITE"
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
        let session = initialize(pep.port(), 819)
            .session
            .expect("lock contention test session");
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
        let snapshot = public_tool_call(
            pep.port(),
            &session,
            820,
            "task_control",
            json!({"action":"get"}),
        );
        assert_eq!(
            snapshot.body["result"]["structuredContent"]["data"]["availability"], "stale",
            "facade lock contention must be explicit rather than fabricating a live projection: {:#?}",
            snapshot.body
        );
        assert_eq!(
            snapshot.body["result"]["structuredContent"]["data"]["current_activity"],
            Value::Null
        );
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

        pep.set_permission_mode(PermissionMode::Edit);
        let refreshed_full_call = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":3,"method":"tools/call",
                "params":{"name":"exec_command","arguments":{"command":"echo must-not-run"}}
            }),
        );
        assert_eq!(refreshed_full_call.status, 200);
        assert_tool_error(&refreshed_full_call, "PolicyDenied");
        let edit_session = session.clone();
        let denied = post(
            pep.port(),
            Some(&edit_session),
            &json!({
                "jsonrpc":"2.0","id":32,"method":"tools/call",
                "params":{"name":"exec_command","arguments":{"command":"echo must-not-run"}}
            }),
        );
        assert_tool_error(&denied, "PolicyDenied");
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
        let timing = pep.current_task_projection().timing_snapshot();
        assert_eq!(timing.status, CurrentTaskStatus::Idle);
        assert_eq!(
            timing.last_tool.as_ref().map(|tool| tool.kind),
            Some(TaskKind::ReadFile)
        );

        pep.set_permission_mode(PermissionMode::Full);
        let refreshed_full_tools = post(
            pep.port(),
            Some(&edit_session),
            &json!({"jsonrpc":"2.0","id":"cached-full","method":"tools/list","params":{}}),
        );
        assert_eq!(refreshed_full_tools.status, 200);
        let full_session = edit_session.clone();
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
        assert_eq!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Idle
        );

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
        let refreshed_policy_call = post(
            pep.port(),
            Some(&full_session),
            &json!({
                "jsonrpc":"2.0","id":"stale-policy-call","method":"tools/call",
                "params":{"name":"exec_command","arguments":{"command":"echo cached-list-must-not-run"}}
            }),
        );
        assert_tool_error(&refreshed_policy_call, "PolicyDenied");
        let narrowed_session = full_session.clone();
        let narrowed_denied = post(
            pep.port(),
            Some(&narrowed_session),
            &json!({
                "jsonrpc":"2.0","id":"narrowed-denied","method":"tools/call",
                "params":{"name":"exec_command","arguments":{"command":"echo cached-list-must-not-run"}}
            }),
        );
        assert_tool_error(&narrowed_denied, "PolicyDenied");
        assert_eq!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Idle
        );
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
    fn full_scripts_share_current_user_authority_independent_of_path_spelling() {
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
        fs::write(
            &outside,
            b"@echo CURRENT_USER_SCRIPT>outside-script.txt\r\n",
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
                json!({"command":command,"shell":shell,"yield_time_ms":0,"timeout_ms":120000}),
            );
            let (response, output) =
                settle_public_command(pep.port(), &session, 20_000 + id, response);
            assert_eq!(
                response.body["result"]["isError"], false,
                "{:#?}",
                response.body
            );
            assert!(output.contains(marker), "{:#?}", response.body);
        }

        let nul = public_tool_call(
            pep.port(),
            &session,
            705,
            "exec_command",
            json!({
                "command":"echo hidden>nul && echo hidden-error 1>nul 2>nul && echo LB_SCHEMA42_NUL_OK",
                "shell":"cmd",
                "yield_time_ms":0,
                "timeout_ms":120000
            }),
        );
        let (nul, nul_output) = settle_public_command(pep.port(), &session, 20_705, nul);
        assert_eq!(nul.body["result"]["isError"], false, "{:#?}", nul.body);
        assert!(nul_output.contains("LB_SCHEMA42_NUL_OK"));
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
        assert_eq!(
            escaped.body["result"]["isError"], false,
            "{:#?}",
            escaped.body
        );
        assert_eq!(
            fs::read_to_string(workspace.join("outside-script.txt"))
                .unwrap()
                .trim(),
            "CURRENT_USER_SCRIPT"
        );

        let mut coding = pep.stop().expect("script E2E PEP stop");
        coding.stop().expect("script E2E MCP stop");
        drop(coding);
        let _ = fs::remove_file(outside);
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn r1_same_rpc_id_in_distinct_sessions_has_isolated_cancellation() {
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
        let session_a = initialize(pep.port(), 801).session.expect("session A");
        let session_b = initialize(pep.port(), 802).session.expect("session B");
        for session in [&session_a, &session_b] {
            assert_eq!(
                post(
                    pep.port(),
                    Some(session),
                    &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
                )
                .status,
                202
            );
        }

        let port = pep.port();
        let call_session_a = session_a.clone();
        let call_a = thread::spawn(move || {
            post_with_read_timeout(
                port,
                Some(&call_session_a),
                &json!({
                    "jsonrpc":"2.0",
                    "id":1,
                    "method":"tools/call",
                    "params":{
                        "name":"exec_command",
                        "arguments":{
                            "command":"Start-Sleep -Seconds 10",
                            "shell":"windows_powershell",
                            "yield_time_ms":10000,
                            "timeout_ms":120000,
                            "max_output_bytes":4096
                        }
                    }
                }),
                Duration::from_secs(150),
            )
        });
        assert_eventually("session A never ran", Duration::from_secs(3), || {
            matches!(
                pep.current_task_projection().latest_snapshot(),
                CurrentTaskStatus::Active(ref task) if task.state == TaskExecutionState::Running
            )
        });

        let call_session_b = session_b.clone();
        let call_b = thread::spawn(move || {
            post_with_read_timeout(
                port,
                Some(&call_session_b),
                &json!({
                    "jsonrpc":"2.0",
                    "id":1,
                    "method":"tools/call",
                    "params":{
                        "name":"exec_command",
                        "arguments":{
                            "command":"Write-Output SESSION_B_SURVIVED",
                            "shell":"windows_powershell",
                            "yield_time_ms":0,
                            "timeout_ms":120000,
                            "max_output_bytes":4096
                        }
                    }
                }),
                Duration::from_secs(150),
            )
        });
        thread::sleep(Duration::from_millis(100));

        let cancelled = post(
            pep.port(),
            Some(&session_a),
            &json!({
                "jsonrpc":"2.0",
                "method":"notifications/cancelled",
                "params":{"requestId":1,"reason":"R1 isolation test"}
            }),
        );
        assert_eq!(cancelled.status, 202);
        let result_a = call_a.join().expect("session A request");
        assert_eq!(
            result_a.body["result"]["isError"], true,
            "{:#?}",
            result_a.body
        );
        assert!(
            matches!(
                result_a.body["result"]["structuredContent"]["data"]["status"].as_str(),
                Some("cancelled" | "failed")
            ),
            "{:#?}",
            result_a.body
        );
        let result_b = call_b.join().expect("session B request");
        let (result_b, result_b_output) =
            settle_public_command(pep.port(), &session_b, 30_001, result_b);
        assert_eq!(
            result_b.body["result"]["structuredContent"]["data"]["status"], "completed",
            "{:#?}",
            result_b.body
        );
        assert!(
            result_b_output.contains("SESSION_B_SURVIVED"),
            "{:#?}",
            result_b.body
        );

        let mut coding = pep.stop().expect("PEP stop after R1 isolation test");
        coding.stop().expect("MCP stop after R1 isolation test");
        assert_eq!(coding.active_processes().unwrap(), 0);
        drop(coding);
        cleanup_test_directory(&workspace);
    }

    #[test]
    fn r1_task_control_cancel_never_cancels_another_session_request() {
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
        let session_a = initialize(pep.port(), 811).session.expect("session A");
        let session_b = initialize(pep.port(), 812).session.expect("session B");
        for session in [&session_a, &session_b] {
            assert_eq!(
                post(
                    pep.port(),
                    Some(session),
                    &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
                )
                .status,
                202
            );
        }

        let port = pep.port();
        let call_session_a = session_a.clone();
        let call_a = thread::spawn(move || {
            post_with_read_timeout(
                port,
                Some(&call_session_a),
                &json!({
                    "jsonrpc":"2.0","id":7,"method":"tools/call",
                    "params":{
                        "name":"exec_command",
                        "arguments":{
                            "command":"Start-Sleep -Seconds 10",
                            "shell":"windows_powershell",
                            "yield_time_ms":10000,
                            "timeout_ms":120000,
                            "max_output_bytes":4096
                        }
                    }
                }),
                Duration::from_secs(150),
            )
        });
        let running_deadline = Instant::now() + Duration::from_secs(3);
        while !matches!(
            pep.current_task_projection().latest_snapshot(),
            CurrentTaskStatus::Active(ref task) if task.state == TaskExecutionState::Running
        ) {
            assert!(Instant::now() < running_deadline, "session A never ran");
            thread::sleep(Duration::from_millis(10));
        }

        let call_session_b = session_b.clone();
        let call_b = thread::spawn(move || {
            post_with_read_timeout(
                port,
                Some(&call_session_b),
                &json!({
                    "jsonrpc":"2.0","id":7,"method":"tools/call",
                    "params":{
                        "name":"exec_command",
                        "arguments":{
                            "command":"Write-Output SESSION_B_NOT_CANCELLED",
                            "shell":"windows_powershell",
                            "yield_time_ms":0,
                            "timeout_ms":120000,
                            "max_output_bytes":4096
                        }
                    }
                }),
                Duration::from_secs(150),
            )
        });
        assert_eventually(
            "session B did not enter Work FIFO",
            Duration::from_secs(3),
            || {
                let scheduler = pep.control_plane.scheduler().snapshot();
                scheduler.work_running == 1 && scheduler.work_queued == 1
            },
        );
        let queued = pep.control_plane.scheduler().snapshot();
        assert_eq!(queued.work_running, 1);
        assert_eq!(queued.work_queued, 1, "session B did not enter Work FIFO");

        let cancel = public_tool_call(
            pep.port(),
            &session_a,
            8,
            "task_control",
            json!({"action":"cancel"}),
        );
        assert_eq!(
            cancel.body["result"]["structuredContent"]["data"]["cancelled_requests"], 1,
            "{:#?}",
            cancel.body
        );
        let result_a = call_a.join().expect("session A request");
        assert_eq!(
            result_a.body["result"]["isError"], true,
            "{:#?}",
            result_a.body
        );
        let result_b = call_b.join().expect("session B request");
        let (result_b, result_b_output) =
            settle_public_command(pep.port(), &session_b, 30_101, result_b);
        assert_eq!(
            result_b.body["result"]["structuredContent"]["data"]["status"], "completed",
            "{:#?}",
            result_b.body
        );
        assert!(
            result_b_output.contains("SESSION_B_NOT_CANCELLED"),
            "{:#?}",
            result_b.body
        );

        let mut coding = pep.stop().expect("PEP stop after task isolation test");
        coding.stop().expect("MCP stop after task isolation test");
        assert_eq!(coding.active_processes().unwrap(), 0);
        drop(coding);
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
            post(
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
    fn command_control_kill_is_not_blocked_by_unrelated_foreground_work() {
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
        let owner = initialize(pep.port(), 340).session.expect("owner session");
        let worker = initialize(pep.port(), 341).session.expect("worker session");
        for session in [&owner, &worker] {
            assert_eq!(
                post(
                    pep.port(),
                    Some(session),
                    &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
                )
                .status,
                202
            );
        }

        let detached = public_tool_call(
            pep.port(),
            &owner,
            342,
            "exec_command",
            json!({
                "command":"Start-Sleep -Seconds 10",
                "shell":"windows_powershell",
                "yield_time_ms":0,
                "timeout_ms":20000,
                "max_output_bytes":4096
            }),
        );
        assert_eq!(
            detached.body["result"]["structuredContent"]["data"]["status"], "running",
            "{:#?}",
            detached.body
        );
        let public_session = detached.body["result"]["structuredContent"]["data"]["session_id"]
            .as_str()
            .expect("detached public session")
            .to_string();

        let port = pep.port();
        let worker_session = worker.clone();
        let foreground = thread::spawn(move || {
            post(
                port,
                Some(&worker_session),
                &json!({
                    "jsonrpc":"2.0","id":343,"method":"tools/call",
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
            )
        });
        assert_eventually(
            "foreground work never acquired its explicit scheduler slot",
            Duration::from_secs(3),
            || pep.control_plane.scheduler().snapshot().work_running == 1,
        );

        let poll_started = Instant::now();
        let polled = public_tool_call(
            pep.port(),
            &owner,
            344,
            "command_control",
            json!({"action":"poll","session_id":public_session,"wait_ms":0}),
        );
        assert!(
            poll_started.elapsed() < Duration::from_secs(2),
            "command_control poll waited behind unrelated Work"
        );
        assert_eq!(
            polled.body["result"]["structuredContent"]["data"]["status"], "running",
            "{:#?}",
            polled.body
        );

        let kill_started = Instant::now();
        let mut killed = public_tool_call(
            pep.port(),
            &owner,
            345,
            "command_control",
            json!({"action":"kill","session_id":public_session,"signal":"KILL","wait_ms":1000}),
        );
        assert!(
            kill_started.elapsed() < Duration::from_millis(2_500),
            "command_control kill waited behind unrelated Work"
        );
        if killed.body["result"]["structuredContent"]["error"]["code"] == "OperationTimedOut" {
            killed = poll_public_command_to_terminal(
                pep.port(),
                &owner,
                34501,
                &public_session,
                Duration::from_secs(30),
            );
        }
        assert_eq!(
            killed.body["result"]["structuredContent"]["data"]["status"], "cancelled",
            "{:#?}",
            killed.body
        );

        let replay = public_tool_call(
            pep.port(),
            &owner,
            346,
            "command_control",
            json!({"action":"poll","session_id":public_session,"wait_ms":0}),
        );
        assert_eq!(
            replay.body["result"]["structuredContent"]["data"]["status"], "cancelled",
            "{:#?}",
            replay.body
        );

        let cancel_worker = public_tool_call(
            pep.port(),
            &worker,
            347,
            "task_control",
            json!({"action":"cancel"}),
        );
        assert_eq!(
            cancel_worker.body["result"]["structuredContent"]["data"]["cancelled_requests"], 1,
            "{:#?}",
            cancel_worker.body
        );
        let foreground = foreground.join().expect("foreground worker response");
        assert_tool_error(&foreground, "ProcessCancelled");

        let mut coding = pep.stop().expect("PEP stop after control-lane test");
        coding.stop().expect("MCP stop after control-lane test");
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
            idle_cancel.body["result"]["structuredContent"]["error"]["code"], "NotFound",
            "{:#?}",
            idle_cancel.body
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
            pep.current_task_projection().latest_snapshot(),
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
        let cancel_data = &cancel.body["result"]["structuredContent"]["data"];
        assert_eq!(
            cancel_data["cancellation_requested"], true,
            "{:#?}",
            cancel.body
        );
        assert!(
            matches!(cancel_data["state"].as_str(), Some("active" | "idle")),
            "the cancellation ACK must publish its truthful instantaneous lifecycle: {:#?}",
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
        assert_eq!(result.body["result"]["isError"], true, "{:#?}", result.body);
        assert_eq!(
            result.body["result"]["structuredContent"]["error"]["code"], "ProcessCancelled",
            "{:#?}",
            result.body
        );
        assert_eq!(
            result.body["result"]["structuredContent"]["data"]["status"], "cancelled",
            "{:#?}",
            result.body
        );
        assert_eventually(
            "cancelled foreground task did not converge to Idle",
            Duration::from_secs(2),
            || pep.current_task_projection().latest_snapshot() == CurrentTaskStatus::Idle,
        );

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
            pep.current_task_projection().latest_snapshot(),
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
        let other_session = initialize(pep.port(), 326)
            .session
            .expect("other downstream MCP session");
        assert_eq!(
            post(
                pep.port(),
                Some(&session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );
        assert_eq!(
            post(
                pep.port(),
                Some(&other_session),
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

        let task_id = running.body["result"]["structuredContent"]["data"]["task_id"]
            .as_str()
            .expect("detached task id")
            .to_string();
        let isolated_cancel = public_tool_call(
            pep.port(),
            &other_session,
            3221,
            "task_control",
            json!({"action":"cancel"}),
        );
        assert_eq!(
            isolated_cancel.body["result"]["structuredContent"]["error"]["code"], "TaskIdRequired",
            "an active execution owned by another live MCP session must stay isolated but actionable: {:#?}",
            isolated_cancel.body
        );
        assert_eq!(
            isolated_cancel.body["result"]["structuredContent"]["error"]["details"]["task_id"],
            task_id,
            "the error must return the stable capability already exposed by current activity"
        );
        assert_eq!(
            delete(pep.port(), &session),
            204,
            "closing the transport session must not erase a detached PublicSession capability"
        );

        let cross_session_poll = public_tool_call(
            pep.port(),
            &other_session,
            327,
            "command_control",
            json!({"action":"poll","session_id":public_session,"wait_ms":0}),
        );
        assert_eq!(
            cross_session_poll.body["result"]["structuredContent"]["data"]["status"], "running",
            "{:#?}",
            cross_session_poll.body
        );

        let isolated_list = public_tool_call(
            pep.port(),
            &other_session,
            3271,
            "task_control",
            json!({"action":"list"}),
        );
        assert_eq!(
            isolated_list.body["result"]["structuredContent"]["data"]["tasks"],
            json!([]),
            "task list must not enumerate another MCP session's task capability: {:#?}",
            isolated_list.body
        );
        assert_eq!(
            isolated_list.body["result"]["structuredContent"]["data"]["executions"],
            json!([]),
            "task list must not enumerate another MCP session's detached execution: {:#?}",
            isolated_list.body
        );

        let cross_session_get = public_tool_call(
            pep.port(),
            &other_session,
            3281,
            "task_control",
            json!({"action":"get","task_id":task_id.clone()}),
        );
        assert_eq!(
            cross_session_get.body["result"]["structuredContent"]["data"]["task"]["id"], task_id,
            "an explicit TaskId capability must survive MCP transport reconnect: {:#?}",
            cross_session_get.body
        );

        let current = public_tool_call(
            pep.port(),
            &other_session,
            3282,
            "task_control",
            json!({"action":"get"}),
        );
        assert_eq!(
            current.body["result"]["structuredContent"]["data"]["current_activity"]["task_id"],
            task_id,
            "the orphaned current execution must expose its stable TaskId: {:#?}",
            current.body
        );

        let cancel_started = Instant::now();
        let cancel = public_tool_call(
            pep.port(),
            &other_session,
            323,
            "task_control",
            json!({"action":"cancel","task_id":task_id.clone()}),
        );
        assert!(
            cancel_started.elapsed() < Duration::from_secs(2),
            "detached task cancellation blocked"
        );
        assert_eq!(
            cancel.body["result"]["structuredContent"]["data"]["cancellation_requested"], true,
            "an accepted cancellation is not the same fact as a terminal execution: {:#?}",
            cancel.body
        );
        assert!(
            cancel.body["result"]["structuredContent"]["data"]["cancelled_requests"]
                .as_u64()
                .is_some_and(|count| count >= 1),
            "{:#?}",
            cancel.body
        );

        let replay = poll_public_command_to_terminal(
            pep.port(),
            &other_session,
            324,
            &public_session,
            Duration::from_secs(30),
        );
        assert_eq!(
            replay.body["result"]["structuredContent"]["ok"], true,
            "{:#?}",
            replay.body
        );
        assert!(replay.body["result"]["structuredContent"]["error"].is_null());
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
            &other_session,
            325,
            "task_control",
            json!({"action":"get"}),
        );
        assert_eq!(
            task.body["result"]["structuredContent"]["data"]["last_activity"]["outcome"],
            "cancelled",
            "{:#?}",
            task.body
        );
        assert_eq!(
            task.body["result"]["structuredContent"]["data"]["last_activity"]["error_code"],
            "ProcessCancelled"
        );
        assert_eq!(
            pep.current_task_projection().latest_snapshot(),
            CurrentTaskStatus::Idle
        );

        let second = public_tool_call(
            pep.port(),
            &other_session,
            329,
            "exec_command",
            json!({
                "command":"$line=[Console]::In.ReadLine(); Write-Output ('stdin:'+ $line); Start-Sleep -Seconds 10",
                "shell":"windows_powershell",
                "yield_time_ms":0,
                "timeout_ms":20000,
                "max_output_bytes":4096
            }),
        );
        let second_public_session =
            second.body["result"]["structuredContent"]["data"]["session_id"]
                .as_str()
                .expect("second detached public session")
                .to_string();
        let second_task_id = second.body["result"]["structuredContent"]["data"]["task_id"]
            .as_str()
            .expect("second detached task id")
            .to_string();

        let projection = public_tool_call(
            pep.port(),
            &other_session,
            330,
            "task_control",
            json!({"action":"get"}),
        );
        let projection_data = &projection.body["result"]["structuredContent"]["data"];
        for legacy in [
            "current_workflow",
            "current_command",
            "last_command",
            "last_terminal_command",
            "last_tool",
        ] {
            assert!(
                projection_data.get(legacy).is_none(),
                "legacy parallel projection {legacy} leaked: {projection_data:#?}"
            );
        }
        assert_eq!(projection_data["current_activity"]["state"], "running");
        assert_eq!(projection_data["current_activity"]["kind"], "command");
        assert_eq!(
            projection_data["scheduler"]["foreground_work_running"], 0,
            "detached execution must release the foreground scheduler slot"
        );
        assert_eq!(
            projection_data["scheduler"]["detached_executions_running"],
            1
        );
        assert_ne!(
            projection_data["last_activity"]["task_id"], second_task_id,
            "the completed launch task must not fabricate a completed command activity"
        );

        let write_started = Instant::now();
        let written = public_tool_call(
            pep.port(),
            &other_session,
            332,
            "command_control",
            json!({
                "action":"write",
                "session_id":second_public_session,
                "chars":"cross-session\n",
                "wait_ms":0
            }),
        );
        assert_ne!(
            written.body["result"]["structuredContent"]["error"]["code"], "SessionUnavailable",
            "{:#?}",
            written.body
        );
        assert!(
            write_started.elapsed() < Duration::from_millis(1_500),
            "write exceeded wait_ms plus transport headroom"
        );
        let kill_started = Instant::now();
        let mut killed = public_tool_call(
            pep.port(),
            &other_session,
            333,
            "command_control",
            json!({"action":"kill","session_id":second_public_session,"wait_ms":0}),
        );
        assert!(
            kill_started.elapsed() < Duration::from_millis(1_500),
            "kill exceeded wait_ms plus transport headroom"
        );
        if killed.body["result"]["structuredContent"]["error"]["code"] == "OperationTimedOut" {
            killed = poll_public_command_to_terminal(
                pep.port(),
                &other_session,
                33301,
                &second_public_session,
                Duration::from_secs(30),
            );
        }
        assert_eq!(
            killed.body["result"]["structuredContent"]["data"]["status"], "cancelled",
            "{:#?}",
            killed.body
        );
        let cancelled_poll = public_tool_call(
            pep.port(),
            &other_session,
            3331,
            "command_control",
            json!({"action":"poll","session_id":second_public_session,"wait_ms":0}),
        );
        assert_eq!(
            cancelled_poll.body["result"]["structuredContent"]["ok"], true,
            "kill and terminal poll must share one cancelled envelope: {:#?}",
            cancelled_poll.body
        );
        assert_eq!(
            cancelled_poll.body["result"]["structuredContent"]["data"]["status"],
            "cancelled"
        );
        assert!(cancelled_poll.body["result"]["structuredContent"]["error"].is_null());

        let prepared = public_tool_call(
            pep.port(),
            &other_session,
            334,
            "agent_workflow",
            json!({
                "action":"bugfix",
                "phase":"prepare",
                "objective":"verify durable workflow cancellation after MCP reinitialize"
            }),
        );
        let workflow_task_id = prepared.body["result"]["structuredContent"]["data"]["task_id"]
            .as_str()
            .expect("prepared durable workflow task id")
            .to_string();
        let reconnected_session = initialize(pep.port(), 3340)
            .session
            .expect("reconnected MCP session");
        let implicit_cross_session_cancel = public_tool_call(
            pep.port(),
            &reconnected_session,
            33401,
            "task_control",
            json!({"action":"cancel"}),
        );
        assert_eq!(
            implicit_cross_session_cancel.body["result"]["structuredContent"]["error"]["code"],
            "TaskIdRequired",
            "untargeted cancel must not report success for another live Session's workflow: {:#?}",
            implicit_cross_session_cancel.body
        );
        assert_eq!(
            implicit_cross_session_cancel.body["result"]["structuredContent"]["error"]["details"]["task_id"],
            workflow_task_id
        );
        let resumed_after_reconnect = public_tool_call(
            pep.port(),
            &reconnected_session,
            33402,
            "agent_workflow",
            json!({"action":"resume","task_id":workflow_task_id.clone()}),
        );
        assert_eq!(
            resumed_after_reconnect.body["result"]["structuredContent"]["data"]["state"],
            "prepared",
            "the stable TaskId must atomically transfer durable workflow ownership after reconnect: {:#?}",
            resumed_after_reconnect.body
        );
        assert_eq!(
            resumed_after_reconnect.body["result"]["structuredContent"]["data"]["task_id"],
            workflow_task_id
        );
        let edited_after_reconnect = public_tool_call(
            pep.port(),
            &reconnected_session,
            3341,
            "agent_workflow",
            json!({
                "action":"bugfix",
                "phase":"edit",
                "task_id":workflow_task_id,
                "patch":"*** Begin Patch\n*** Add File: reconnect-edit.txt\n+owned by durable task capability\n*** End Patch"
            }),
        );
        assert_eq!(
            edited_after_reconnect.body["result"]["structuredContent"]["data"]["state"], "editing",
            "a durable task capability must survive MCP transport reinitialization: {:#?}",
            edited_after_reconnect.body
        );
        assert!(workspace.join("reconnect-edit.txt").is_file());
        let visible_workflow = public_tool_call(
            pep.port(),
            &reconnected_session,
            3342,
            "task_control",
            json!({"action":"get"}),
        );
        let visible_workflow_data = &visible_workflow.body["result"]["structuredContent"]["data"];
        assert_eq!(
            visible_workflow_data["current_activity"]["task_id"], workflow_task_id,
            "a blocking durable workflow must expose its stable identity"
        );
        assert!(visible_workflow_data.get("current_step").is_none());
        assert!(visible_workflow_data.get("next_step").is_none());
        let cancel_workflow = public_tool_call(
            pep.port(),
            &reconnected_session,
            335,
            "task_control",
            json!({"action":"cancel","task_id":workflow_task_id}),
        );
        assert_eq!(
            cancel_workflow.body["result"]["structuredContent"]["data"]["workflow_cancelled"], true,
            "{:#?}",
            cancel_workflow.body
        );
        assert_eq!(
            cancel_workflow.body["result"]["structuredContent"]["data"]["durable_task_cancelled"],
            true,
            "{:#?}",
            cancel_workflow.body
        );
        let replacement = public_tool_call(
            pep.port(),
            &reconnected_session,
            336,
            "agent_workflow",
            json!({
                "action":"diagnose",
                "phase":"prepare",
                "objective":"replacement workflow after prior durable terminal"
            }),
        );
        assert_eq!(
            replacement.body["result"]["structuredContent"]["data"]["state"], "prepared",
            "{:#?}",
            replacement.body
        );
        let replacement_task_id =
            replacement.body["result"]["structuredContent"]["data"]["task_id"]
                .as_str()
                .expect("replacement durable workflow task id");
        let replacement_cancelled = public_tool_call(
            pep.port(),
            &reconnected_session,
            337,
            "task_control",
            json!({"action":"cancel","task_id":replacement_task_id}),
        );
        assert_eq!(
            replacement_cancelled.body["result"]["structuredContent"]["data"]["workflow_cancelled"],
            true,
            "{:#?}",
            replacement_cancelled.body
        );
        let after_workflow_terminal = public_tool_call(
            pep.port(),
            &reconnected_session,
            338,
            "task_control",
            json!({"action":"get"}),
        );
        assert!(
            after_workflow_terminal.body["result"]["structuredContent"]["data"]["current_activity"]
                .is_null(),
            "terminal workflow must not remain as current presentation: {:#?}",
            after_workflow_terminal.body
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
        let fixture = PublicRuntimeFixture::start(PermissionMode::Full);
        let (client, _) = PublicMcpClient::connect(fixture.runtime().port(), 326);
        let timed_out = client.call_tool(
            "exec_command",
            json!({
                "command":"Start-Sleep -Seconds 10; Write-Output SHOULD_NOT_COMPLETE",
                "shell":"windows_powershell",
                "yield_time_ms":10000,
                "timeout_ms":300,
                "max_output_bytes":4096
            }),
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

        fixture.shutdown();
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
        let session = initialized.session.expect("downstream MCP session");

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
        let reviewed_program = crate::execution::policy::reviewed_elevated_program()
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
            assert_tool_error(&denied, "ElevatedOperationNotReviewed");
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
                        && task.state == TaskExecutionState::Running
                        && fake.start_count() == 1 =>
                {
                    assert_eq!(task.summary, SafeTaskSummary::Omitted);
                    assert!(!format!("{task:?}").contains(secret));
                    break;
                }
                _ => {}
            }
            assert!(
                std::time::Instant::now() < running_deadline,
                "elevated call never projected Running (broker_starts={}, request_finished={})",
                fake.start_count(),
                call.is_finished()
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
        assert_eq!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Idle
        );

        pep.set_permission_mode(PermissionMode::Edit);
        let refreshed_full_for_edit = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":3021,"method":"tools/list","params":{}}),
        );
        assert_eq!(
            refreshed_full_for_edit.status, 200,
            "permission changes must refresh the catalog without terminating the MCP session"
        );
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
        assert_eq!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Idle
        );

        pep.set_permission_mode(PermissionMode::Elevated);
        fake.set_state(PrivilegeState::AwaitingUac);
        let refreshed_edit_session = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":3030,"method":"tools/list","params":{}}),
        );
        assert_eq!(refreshed_edit_session.status, 200);
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
        assert_tool_error(&awaiting, "PrivilegedRouteUnavailable");
        assert_eq!(fake.start_count(), 1);
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
    fn filesystem_never_escalates_outside_the_active_workspace() {
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
        assert_tool_error(&awaiting, "WorkspaceDenied");
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
        assert_tool_error(&elevated, "WorkspaceDenied");
        assert_eq!(
            fake.structured_filesystem_count(),
            0,
            "filesystem must not acquire administrator authority implicitly"
        );
        for (request_id, arguments) in [
            (
                43041,
                json!({
                    "action":"write",
                    "path":outside.join("written.txt").to_string_lossy(),
                    "content":"must-not-be-written"
                }),
            ),
            (
                43042,
                json!({"action":"delete","path":outside_file.to_string_lossy()}),
            ),
        ] {
            let mutation = public_tool_call(
                pep.port(),
                &elevated_session,
                request_id,
                "filesystem",
                arguments,
            );
            assert_tool_error(&mutation, "WorkspaceDenied");
        }
        assert!(!outside.join("written.txt").exists());
        assert!(outside_file.exists());
        assert_eq!(fake.structured_filesystem_count(), 0);

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
            0,
            "workspace-contained Elevated filesystem call incorrectly used Broker"
        );

        let control_plane = public_tool_call(
            pep.port(),
            &elevated_session,
            4306,
            "filesystem",
            json!({"action":"delete","path":"C:\\ProgramData\\LocalBridge\\settings.json"}),
        );
        assert_tool_error(&control_plane, "WorkspaceDenied");
        assert_eq!(fake.structured_filesystem_count(), 0);

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

        let reviewed_program = crate::execution::policy::reviewed_elevated_program()
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
        assert_tool_error(&shell_path_denied, "ElevatedOperationNotReviewed");
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
        assert_tool_error(&opaque_helper_denied, "ElevatedOperationNotReviewed");
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
        assert_tool_error(&control_plane_denied, "ElevatedOperationNotReviewed");
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
