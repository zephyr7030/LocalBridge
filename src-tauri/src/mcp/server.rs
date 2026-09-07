#[cfg(test)]
use std::collections::HashMap;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, TryLockError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde_json::{Map, Value, json};

use crate::control_plane::command_control::{
    CommandControlAction, CommandControlError, CommandControlRequest, CommandControlResult,
    CommandKillSignal, RuntimeCommandStatus, control_command_during_work,
};
#[cfg(test)]
use crate::control_plane::convergence::DesiredStateOwner;
use crate::control_plane::convergence::StructuredPathAuthority;
#[cfg(test)]
use crate::control_plane::convergence::{
    ConnectionProfile, ConvergenceSnapshot, DesiredState, DesiredWorkspace, ObservedState,
    ServiceIntent,
};
use crate::control_plane::execution_registry::{ExecutionRegistry, ExecutionRegistryError};
use crate::control_plane::owner::ControlPlane;
use crate::control_plane::request_registry::{
    ActiveRequest, ActiveRequestState, RequestCancellationTarget, RequestRegistry,
};
use crate::control_plane::scheduler::{Scheduler, SchedulerAdmissionError, SchedulerLane};
use crate::control_plane::session_registry::{
    MCP_SESSION_TTL_MS, SessionInsertError, SessionReaper, SessionRecord, SessionRegistry,
};
use crate::control_plane::snapshot::{
    AuthorityProjection, ControlPlaneSnapshotReader, ProjectionAvailability, TaskAggregate,
};
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
use crate::privilege::PrivilegedFilesystemResult;
use crate::privilege::{
    AdministratorFilesystemAction, AdministratorFilesystemErrorCode, AdministratorFilesystemKind,
    AdministratorFilesystemResult, AdministratorFilesystemSortBy, AdministratorFilesystemSortOrder,
    AdministratorFilesystemSpec, AdministratorWorkspacePathField, ElevatedExecOutcome,
    ElevatedExecSpec, PrivilegedExecError, PrivilegedExecution, PrivilegedFilesystemSpec,
};
use crate::state::{
    Capability, CurrentTask, CurrentTaskStatus, CurrentTaskTiming, LastToolTiming, PermissionMode,
    PrivilegeState, RuntimeState, SafeTaskSummary, TaskExecutionState, TaskKind,
};

use super::client_auth::ClientAuthenticator;
use super::facade::{
    AGENT_API_REVISION, AgentFacade, CodingRuntimeHealth, CodingToolsRuntimeAdapter,
    FacadeCallError, FacadeDenied, FacadeError, FacadeErrorCode, FilesystemAction,
    FilesystemRequest, TaskCallIdentity, normalize_path_authority_error, parse_filesystem_request,
    public_command_stderr, public_error_output_schema, public_safe_summary, public_task_kind,
    run_workspace_filesystem_with_authority, stable_command_error, stable_public_tool_catalog,
    stable_success, validate_workspace_context_probe,
};
use super::http::{McpCancellationClient, McpHealthClient};
use super::observation::{WorkspaceObservationSeed, render_workspace_context};
use super::runtime::{CodingToolsRuntime, CodingToolsRuntimeError};
use crate::execution::policy::CapabilityPolicy;
use crate::execution::shell::{ShellExecutionSpec, ShellExecutor, ShellSelector};
use crate::filesystem::policy::FilesystemPathPolicy;
use crate::filesystem::service::FilesystemCancellation;
use crate::workspace::path_authority::WorkspaceResolver;

pub(super) const CURRENT_PROTOCOL_VERSION: &str = "2025-11-25";
const COMPATIBLE_PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_CHUNKED_WIRE_BYTES: usize = MAX_BODY_BYTES * 2 + MAX_HEADER_BYTES;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);
const ACCEPT_IDLE: Duration = Duration::from_millis(10);
const MAX_DOWNSTREAM_MCP_SESSIONS: usize = 64;
const MAX_CONNECTION_WORKERS: usize = 64;
static PRIVILEGED_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(1);
static PRIVATE_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(1);

struct ConnectionContext<'a> {
    authenticator: &'a ClientAuthenticator,
    guard: &'a Mutex<AgentFacade<CodingToolsRuntimeAdapter>>,
    public_policy: &'a RwLock<CapabilityPolicy>,
    cancellation: &'a McpCancellationClient,
    policy_state: &'a PolicyStateSource,
    workspace_observation: &'a WorkspaceObservationSeed,
    observed_workspace: &'a Path,
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

struct AdministratorFilesystemContext<'a> {
    guard: &'a Mutex<AgentFacade<CodingToolsRuntimeAdapter>>,
    workspace: &'a Path,
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
    authenticator: ClientAuthenticator,
    guard: Arc<Mutex<AgentFacade<CodingToolsRuntimeAdapter>>>,
    public_policy: Arc<RwLock<CapabilityPolicy>>,
    cancellation: McpCancellationClient,
    control_plane: ControlPlane,
    policy_state: PolicyStateSource,
    workspace_observation: Arc<WorkspaceObservationSeed>,
    observed_workspace: PathBuf,
    current_task: CurrentTaskProjection,
    privileged: Option<Arc<dyn PrivilegedExecution>>,
    shutdown: mpsc::Receiver<()>,
}

#[derive(Clone)]
enum PolicyStateSource {
    Published(ControlPlaneSnapshotReader),
    #[cfg(test)]
    Simulated {
        desired: DesiredStateOwner,
        workspace: PathBuf,
        connection: Option<ConnectionProfile>,
        privileged: Option<Arc<dyn PrivilegedExecution>>,
    },
}

#[derive(Clone)]
struct PolicyControlState {
    revision: u64,
    authority: AuthorityProjection,
    work_authorized: bool,
    runtime: Option<RuntimeState>,
}

impl PolicyStateSource {
    fn read(&self) -> Option<PolicyControlState> {
        match self {
            Self::Published(reader) => {
                let snapshot = reader.read();
                let authority = &snapshot.authority;
                if authority.availability() != ProjectionAvailability::Ready || authority.is_stale()
                {
                    return None;
                }
                Some(PolicyControlState {
                    revision: snapshot.revision,
                    authority: authority.value()?.clone(),
                    work_authorized: snapshot.work_is_authorized(),
                    runtime: (snapshot.runtime.availability() == ProjectionAvailability::Ready
                        && !snapshot.runtime.is_stale())
                    .then(|| {
                        snapshot
                            .runtime
                            .value()
                            .map(|runtime| runtime.state.clone())
                    })
                    .flatten(),
                })
            }
            #[cfg(test)]
            Self::Simulated {
                desired,
                workspace,
                connection,
                privileged,
            } => {
                let broker = privileged
                    .as_ref()
                    .map(|gateway| gateway.state())
                    .unwrap_or(PrivilegeState::Disabled);
                let convergence = ConvergenceSnapshot::derive(
                    desired.snapshot(),
                    ObservedState {
                        broker: broker.clone(),
                        runtime: crate::state::RuntimeState::Ready,
                        workspace: Some(workspace.clone()),
                        connection: connection.clone(),
                    },
                );
                Some(PolicyControlState {
                    revision: convergence.desired_revision,
                    authority: AuthorityProjection {
                        desired: convergence.effective.authority.configured,
                        effective: convergence.effective.authority.execution,
                        broker,
                        structured_paths: convergence.effective.authority.structured_paths,
                        reconciliation: convergence.effective.authority.reconciliation,
                    },
                    work_authorized: convergence.effective.work_is_authorized(),
                    runtime: Some(RuntimeState::Ready),
                })
            }
        }
    }
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
    AuthenticationUnavailable,
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
            Self::AuthenticationUnavailable => {
                f.write_str("policy enforcement client authentication is unavailable")
            }
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
    authenticator: ClientAuthenticator,
    control_plane: ControlPlane,
    current_task: CurrentTaskProjection,
    guard: Option<Arc<Mutex<AgentFacade<CodingToolsRuntimeAdapter>>>>,
    public_policy: Arc<RwLock<CapabilityPolicy>>,
    #[cfg(test)]
    test_desired_state: Option<DesiredStateOwner>,
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
        snapshot_reader: ControlPlaneSnapshotReader,
        privileged: Option<Arc<dyn PrivilegedExecution>>,
        wake: Option<CurrentTaskWake>,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_inner(
            coding_runtime,
            policy,
            PolicyStateSource::Published(snapshot_reader),
            privileged,
            wake,
            ClientAuthenticator::generated()
                .map_err(|_| PolicyEnforcementError::AuthenticationUnavailable)?,
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
        let policy_state = PolicyStateSource::Simulated {
            desired: desired_state.clone(),
            workspace,
            connection: None,
            privileged: privileged.as_ref().map(Arc::clone),
        };
        Self::start_inner(
            coding_runtime,
            policy,
            policy_state,
            privileged,
            wake,
            ClientAuthenticator::disabled_for_isolated_unit_test(),
        )
    }

    #[cfg(test)]
    pub(crate) fn start_with_simulated_control_plane(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        desired_state: DesiredStateOwner,
        observed_connection: Option<ConnectionProfile>,
        privileged: Option<Arc<dyn PrivilegedExecution>>,
        wake: Option<CurrentTaskWake>,
    ) -> Result<Self, PolicyEnforcementError> {
        let policy_state = PolicyStateSource::Simulated {
            desired: desired_state.clone(),
            workspace: coding_runtime.workspace().to_path_buf(),
            connection: observed_connection,
            privileged: privileged.as_ref().map(Arc::clone),
        };
        Self::start_inner(
            coding_runtime,
            policy,
            policy_state,
            privileged,
            wake,
            ClientAuthenticator::disabled_for_isolated_unit_test(),
        )
    }

    fn start_inner(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        policy_state: PolicyStateSource,
        privileged: Option<Arc<dyn PrivilegedExecution>>,
        wake: Option<CurrentTaskWake>,
        authenticator: ClientAuthenticator,
    ) -> Result<Self, PolicyEnforcementError> {
        let public_policy = Arc::new(RwLock::new(policy.clone()));
        let health_workspace = coding_runtime.workspace().to_path_buf();
        let cancellation = coding_runtime
            .cancellation_client()
            .map_err(|_| PolicyEnforcementError::UpstreamCancellationUnavailable)?;
        let health_client = coding_runtime
            .health_client()
            .map_err(|_| PolicyEnforcementError::UpstreamHealthUnavailable)?;
        let control_plane = ControlPlane::for_workspace(&health_workspace)
            .map_err(|_| PolicyEnforcementError::UpstreamFacadeNegotiationFailed)?;
        #[cfg(test)]
        let test_desired_state = match &policy_state {
            PolicyStateSource::Simulated { desired, .. } => Some(desired.clone()),
            PolicyStateSource::Published(_) => None,
        };
        let guard = AgentFacade::from_coding_runtime_with_executions(
            coding_runtime,
            policy,
            control_plane.executions(),
        )
        .map_err(|_| PolicyEnforcementError::UpstreamFacadeNegotiationFailed)?;
        let workspace_observation = Arc::new(
            guard
                .workspace_observation_seed()
                .map_err(|_| PolicyEnforcementError::UpstreamFacadeNegotiationFailed)?,
        );
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
        let thread_policy_state = policy_state.clone();
        let thread_task = current_task.clone();
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let guard = Arc::new(Mutex::new(guard));
        let thread_guard = Arc::clone(&guard);
        let thread_policy = Arc::clone(&public_policy);
        let thread_authenticator = authenticator.clone();
        let thread = thread::Builder::new()
            .name("localbridge-mcp-policy".into())
            .spawn(move || {
                serve(
                    listener,
                    ServeContext {
                        authenticator: thread_authenticator,
                        guard: thread_guard,
                        public_policy: thread_policy,
                        cancellation,
                        control_plane: thread_control_plane,
                        policy_state: thread_policy_state,
                        workspace_observation,
                        observed_workspace: thread_workspace,
                        current_task: thread_task,
                        privileged,
                        shutdown: shutdown_rx,
                    },
                )
            })
            .map_err(|_| PolicyEnforcementError::ThreadSpawnFailed)?;
        Ok(Self {
            port,
            authenticator,
            control_plane,
            current_task,
            guard: Some(guard),
            public_policy,
            #[cfg(test)]
            test_desired_state,
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

    pub(crate) fn local_connector_bearer(&self) -> Option<crate::credentials::SecretString> {
        self.authenticator.bearer_copy()
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn test_client_authorization_header(&self) -> Option<String> {
        self.authenticator.test_authorization_header()
    }

    #[cfg(test)]
    pub fn set_simulated_permission_for_test(&self, mode: PermissionMode) {
        self.test_desired_state
            .as_ref()
            .expect("simulated policy state")
            .set_permission(mode);
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
        authenticator,
        guard,
        public_policy,
        cancellation,
        control_plane,
        policy_state,
        workspace_observation,
        observed_workspace,
        current_task,
        privileged,
        shutdown,
    } = context;
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
                let worker_authenticator = authenticator.clone();
                let worker_policy = Arc::clone(&public_policy);
                let worker_policy_state = policy_state.clone();
                let worker_observation = Arc::clone(&workspace_observation);
                let worker_workspace = observed_workspace.clone();
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
                // A failed invariant inside one request must not decide the fate of the
                // whole desktop application. The worker owns the connection, so the panic
                // boundary is here: the caller is answered with a transport error, the
                // Drop-based session/request/task cleanup runs during the unwind, and the
                // remaining sessions keep their runtime.
                let mut panic_stream = stream.try_clone().ok();
                match thread::Builder::new()
                    .name("localbridge-mcp-policy-request".into())
                    .spawn(move || {
                        let context = ConnectionContext {
                            authenticator: &worker_authenticator,
                            guard: &worker_guard,
                            public_policy: &worker_policy,
                            cancellation: &worker_cancellation,
                            policy_state: &worker_policy_state,
                            workspace_observation: &worker_observation,
                            observed_workspace: &worker_workspace,
                            current_task: &worker_task,
                            executions: &worker_executions,
                            tasks: &worker_tasks,
                            scheduler: &worker_scheduler,
                            sessions: &worker_sessions,
                            requests: &worker_requests,
                            privileged: worker_privileged.as_ref(),
                            stopping: &worker_stopping,
                        };
                        let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                            || handle_connection(stream, context),
                        ));
                        match handled {
                            Ok(Ok(())) => {}
                            Ok(Err(())) => {
                                eprintln!(
                                    "[localbridge-mcp] connection closed before response completed"
                                );
                            }
                            Err(_) => {
                                if let Some(stream) = panic_stream.as_mut() {
                                    let _ = write_mcp_http_error(
                                        stream,
                                        500,
                                        mcp_unknown("request_handler_panic"),
                                        None,
                                    );
                                }
                                eprintln!(
                                    "[localbridge-mcp] request handler panicked; connection failed and the runtime stays up"
                                );
                            }
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
    for task_id in scheduler.close() {
        let _ = tasks.finish(&task_id, TerminalOutcome::Cancelled);
    }
    for _ in 0..3 {
        let active = requests.all();
        if active.is_empty() {
            break;
        }
        for request in &active {
            if let Some(request) = requests.request_cancellation(&request.key) {
                let _ = cancel_registered_request(
                    &request,
                    &cancellation,
                    privileged.as_ref(),
                    &scheduler,
                    &tasks,
                );
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
            let _ = cancel_registered_request(&request, cancellation, privileged, scheduler, tasks);
        }
    }
    for task_id in scheduler.cancel_queued_by_session(&session.id) {
        let _ = tasks.finish(&task_id, TerminalOutcome::Cancelled);
    }
    let _ = executions.orphan_owned_by(&session.id);
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
        authenticator,
        guard,
        public_policy,
        cancellation,
        policy_state,
        workspace_observation,
        observed_workspace,
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

    if !authenticator.authenticate(request.header("authorization")) {
        return write_mcp_http_error(
            &mut stream,
            401,
            mcp_invalid("client_authentication_required"),
            None,
        );
    }

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
        let current_signature = stable_tool_catalog_signature();
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
        let tool_catalog_signature = stable_tool_catalog_signature();
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
    let current_signature = stable_tool_catalog_signature();
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
                    if cancel_registered_request(
                        &active,
                        cancellation,
                        privileged,
                        scheduler,
                        tasks,
                    )
                    .is_err()
                    {
                        return write_mcp_http_error(
                            &mut stream,
                            503,
                            mcp_unavailable("cancellation_unavailable"),
                            Some(session),
                        );
                    }
                    if let RequestCancellationTarget::Runtime(runtime_request_id) =
                        &active.cancellation
                    {
                        if spawn_runtime_cancellation_relay(
                            active.key.clone(),
                            runtime_request_id.clone(),
                            requests.clone(),
                            cancellation.clone(),
                        )
                        .is_err()
                        {
                            return write_mcp_http_error(
                                &mut stream,
                                503,
                                mcp_unavailable("cancellation_unavailable"),
                                Some(session),
                            );
                        }
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
            let result = stable_tool_catalog();
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
            let Some(effective) = policy_state.read() else {
                return write_mcp_http_error(
                    &mut stream,
                    503,
                    mcp_unavailable("control_plane_snapshot_unavailable"),
                    Some(session),
                );
            };
            let mode = effective.authority.effective;
            let scoped_request = request_key_from_json(session_id.clone(), &id)
                .expect("validated downstream request id");
            let _session_request_lease = SessionRequestLease::new(
                sessions.clone(),
                session_id.clone(),
                scoped_request.clone(),
            );
            let lane = scheduler_lane(name, &arguments);
            if lane == SchedulerLane::Work && !effective.work_authorized {
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
                    if requests
                        .register(
                            scoped_request.clone(),
                            RequestCancellationTarget::QueuedWork,
                        )
                        .is_err()
                    {
                        let _ = tasks.finish(&task_id, TerminalOutcome::Blocked);
                        return write_rpc_error(
                            &mut stream,
                            id,
                            -32600,
                            "Duplicate active request id in MCP session",
                            Some(session),
                        );
                    }
                    match scheduler.admit_work_for_request(
                        session_id.clone(),
                        task_id.clone(),
                        scoped_request.clone(),
                    ) {
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
                            requests.remove(&scoped_request);
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
                            requests.remove(&scoped_request);
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
                        Err(SchedulerAdmissionError::Closed) => {
                            requests.remove(&scoped_request);
                            let _ = tasks.finish(&task_id, TerminalOutcome::Cancelled);
                            return write_mcp_http_error(
                                &mut stream,
                                503,
                                mcp_unavailable("server_stopping"),
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
                    Err(SchedulerAdmissionError::Closed) => {
                        return write_mcp_http_error(
                            &mut stream,
                            503,
                            mcp_unavailable("server_stopping"),
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
                requests.remove(&scoped_request);
                return finalize_special_handler_request(&request_key, session, result);
            }
            if name == "filesystem" {
                let task_id = scheduled_task
                    .clone()
                    .expect("filesystem is admitted through Work lane");
                let registered_task =
                    RegisteredTaskProjection::new(task_id, tasks.clone(), current_task.clone());
                let result = match effective.authority.structured_paths {
                    StructuredPathAuthority::ActiveWorkspace => handle_workspace_filesystem(
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
                    ),
                    StructuredPathAuthority::AdministratorBroker => {
                        handle_administrator_filesystem(
                            &mut stream,
                            id,
                            session,
                            mode,
                            &arguments,
                            AdministratorFilesystemContext {
                                guard,
                                workspace: observed_workspace,
                                privileged,
                                current_task: &registered_task,
                                requests,
                                stopping,
                            },
                        )
                    }
                };
                requests.remove(&scoped_request);
                return result;
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
            if name == "workspace_context" {
                let request_key = request_diagnostic_key(&id);
                record_mcp_request_start(&request_key, session, name);
                let decision = public_policy
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .decide_public(mode, name, &arguments);
                let mut result = if decision.allowed {
                    match render_workspace_context(
                        workspace_observation,
                        &public_policy
                            .read()
                            .unwrap_or_else(std::sync::PoisonError::into_inner),
                        mode,
                        effective.runtime.as_ref(),
                        &arguments,
                    ) {
                        Ok(result) => result,
                        Err(error) => error.to_mcp_result(),
                    }
                } else {
                    FacadeDenied {
                        reason: decision
                            .deny_reason
                            .expect("denied workspace_context decision contains reason"),
                        capability: decision.descriptor.capability,
                    }
                    .to_mcp_result()
                };
                if decision.allowed {
                    enrich_workspace_context_privilege(
                        &mut result,
                        &effective,
                        tasks,
                        executions,
                        scheduler,
                    );
                }
                return finalize_special_handler_request(
                    &request_key,
                    session,
                    write_rpc_result(&mut stream, id, result, Some(session)),
                );
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
                        arguments.get("adoption_token").and_then(Value::as_str),
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
            let runtime_target = RequestCancellationTarget::Runtime(private_request_id.clone());
            let activated_request = if scheduled_task.is_some() {
                requests.replace_cancellation_target(&request_key, runtime_target)
            } else if requests
                .register(request_key.clone(), runtime_target)
                .is_ok()
            {
                requests.active(&request_key)
            } else {
                None
            };
            let Some(activated_request) = activated_request else {
                return write_rpc_error(
                    &mut stream,
                    id,
                    -32600,
                    "Duplicate active request id in MCP session",
                    Some(session),
                );
            };
            if activated_request.state == ActiveRequestState::CancellationRequested {
                let _ = cancel_registered_request(
                    &activated_request,
                    cancellation,
                    privileged,
                    scheduler,
                    tasks,
                );
                let _ = spawn_runtime_cancellation_relay(
                    request_key.clone(),
                    private_request_id.clone(),
                    requests.clone(),
                    cancellation.clone(),
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
                                        &session_id,
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
            match result {
                Ok(mut result) => {
                    if name == "workspace_context" {
                        enrich_workspace_context_privilege(
                            &mut result,
                            &effective,
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
    adoption_token: Option<&str>,
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
    let Some(adoption_token) = adoption_token.filter(|value| !value.is_empty()) else {
        return FacadeError::new(
            FacadeErrorCode::InvalidArgument,
            "adopt requires adoption_token",
            false,
        )
        .with_details(json!({"field":"adoption_token"}))
        .to_mcp_result();
    };
    let adoption_token = crate::domain::AdoptionToken::new(adoption_token);
    match executions.adopt_owner(public_session_id, &adoption_token, owner.clone()) {
        Ok(adopted) => stable_success(
            json!({
                "status":"running",
                "session_id":adopted.execution.public_session_id,
                "task_id":adopted.execution.task_id,
                "execution_id":adopted.execution.id,
                "adoption_token":adopted.next_adoption_token.expose(),
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
        Err(
            ExecutionRegistryError::OwnerConflict { .. } | ExecutionRegistryError::NotOrphaned(_),
        ) => FacadeError::new(
            FacadeErrorCode::TaskNotOwned,
            "command session still has an active MCP owner",
            false,
        )
        .to_mcp_result(),
        Err(ExecutionRegistryError::AdoptionDenied) => FacadeError::new(
            FacadeErrorCode::SessionUnavailable,
            "command session or adoption credential is unavailable",
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
    control: &PolicyControlState,
    tasks: &TaskRegistry,
    executions: &ExecutionRegistry,
    scheduler: &Scheduler,
) {
    let authority = &control.authority;
    let state = &authority.broker;
    let (observed_privilege_state, broker_state, uac_state) = match state {
        PrivilegeState::Disabled => ("disabled", "offline", "not_requested"),
        PrivilegeState::Requested => ("requested", "offline", "not_requested"),
        PrivilegeState::AwaitingUac => ("awaiting_uac", "starting", "awaiting_user"),
        PrivilegeState::Active { .. } => ("active", "active", "authorized"),
        PrivilegeState::Faulted(_) => ("faulted", "faulted", "faulted"),
    };
    let desired_permission = match authority.desired {
        PermissionMode::Edit => "edit",
        PermissionMode::Full => "full",
        PermissionMode::Elevated => "elevated",
    };
    let privilege_state = if authority.desired == PermissionMode::Elevated {
        observed_privilege_state
    } else {
        "disabled"
    };
    let effective_permission = match authority.effective {
        PermissionMode::Edit => "edit",
        PermissionMode::Full => "full",
        PermissionMode::Elevated => "elevated",
    };
    let authority_reconciliation = match authority.reconciliation {
        crate::control_plane::convergence::AuthorityReconciliation::Converged => "converged",
        crate::control_plane::convergence::AuthorityReconciliation::AuthorizationRequired => {
            "authorization_required"
        }
        crate::control_plane::convergence::AuthorityReconciliation::AwaitingAuthorization => {
            "awaiting_authorization"
        }
        crate::control_plane::convergence::AuthorityReconciliation::BrokerUnavailable => {
            "broker_unavailable"
        }
        crate::control_plane::convergence::AuthorityReconciliation::DisablePending => {
            "disable_pending"
        }
    };
    let administrator_token_available = matches!(
        authority.structured_paths,
        StructuredPathAuthority::AdministratorBroker
    );
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
        "permission_mode".into(),
        Value::String(effective_permission.into()),
    );
    data.insert(
        "workspace_scope".into(),
        Value::String(
            match authority.structured_paths {
                StructuredPathAuthority::ActiveWorkspace => "structured_tools_active_workspace",
                StructuredPathAuthority::AdministratorBroker => "administrator_broker_paths",
            }
            .into(),
        ),
    );
    data.insert(
        "ordinary_route_token".into(),
        Value::String("current_windows_user".into()),
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
            "revision":control.revision,
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
        } else if authority.desired != PermissionMode::Elevated {
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
                match task_control_get_by_id(
                    &arguments,
                    tasks,
                    executions,
                    &session_id,
                    observed_workspace,
                ) {
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
                .filter(|task_id| {
                    task_is_cancellable_owned(tasks, executions, task_id, &session_id)
                })
                .cloned()
                .into_iter()
                .collect::<BTreeSet<_>>();
            if !explicit_task_capability {
                candidates = cancellable_task_ids(tasks, executions, &session_id);
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
            if let Some(requested_task) = requested.as_ref() {
                if checkpoint_store.as_ref().is_some_and(|store| {
                    store
                        .active_owned_workflow::<Value>(session)
                        .ok()
                        .flatten()
                        .as_deref()
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
                    let _ = workflow_id;
                    let error = FacadeError::new(
                        FacadeErrorCode::TaskNotOwned,
                        "an active durable workflow is owned by another MCP session",
                        false,
                    );
                    return write_rpc_result(stream, id, error.to_mcp_result(), Some(session));
                }
                if let Some(execution) = executions.latest_running() {
                    let _ = execution;
                    let error = FacadeError::new(
                        FacadeErrorCode::TaskNotOwned,
                        "an active detached execution is owned by another MCP session",
                        false,
                    );
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
                    store
                        .cancel_owned::<Value>(task_id.as_str(), session)
                        .unwrap_or(false)
                });
                queued_cancelled = scheduler.cancel_queued_task(&session_id, task_id);
                if queued_cancelled {
                    let _ = tasks.finish(task_id, TerminalOutcome::Cancelled);
                }
                let active_request = tasks.get(task_id).map(|task| task.request);
                let running_executions = executions
                    .running_owned_by(&session_id)
                    .into_iter()
                    .filter(|execution| &execution.task_id == task_id)
                    .collect::<Vec<_>>();
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
                        if cancel_registered_request(
                            &active,
                            cancellation,
                            privileged,
                            scheduler,
                            tasks,
                        )
                        .is_ok()
                        {
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
    owner: &McpSessionId,
    workspace: &Path,
) -> Result<Value, FacadeError> {
    let task_id = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(TaskId::new)
        .ok_or_else(|| FacadeError::new(FacadeErrorCode::InvalidArgument, "task_id 无效", false))?;
    super::resource_access::owned_task_detail(&task_id, owner, workspace, tasks, executions)
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

fn task_is_cancellable_owned(
    tasks: &TaskRegistry,
    executions: &ExecutionRegistry,
    task_id: &TaskId,
    owner: &McpSessionId,
) -> bool {
    tasks
        .get(task_id)
        .is_some_and(|task| &task.owner_session == owner && !task.lifecycle.is_terminal())
        || executions
            .running_owned_by(owner)
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
    owner_session: &McpSessionId,
    executions: &ExecutionRegistry,
    cancellation: &McpCancellationClient,
    workspace: &Path,
    private_request_id: RpcRequestId,
) -> Value {
    if executions
        .execution_for_public_session(public_session_id)
        .is_none_or(|execution| execution.owner_session.as_ref() != Some(owner_session))
    {
        return FacadeError::new(
            FacadeErrorCode::TaskNotOwned,
            "command session is not owned by the current MCP session",
            false,
        )
        .to_mcp_result();
    }
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

fn stable_tool_catalog() -> Value {
    let mut result = stable_public_tool_catalog();
    append_elevated_exec_tool(&mut result);
    result
}

fn stable_tool_catalog_signature() -> String {
    serde_json::to_string(&json!({
        "api_revision": AGENT_API_REVISION,
        "catalog": stable_tool_catalog()
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
        FilesystemAction::Search | FilesystemAction::SearchContent => TaskKind::SearchCode,
        FilesystemAction::Write
        | FilesystemAction::Replace
        | FilesystemAction::Patch
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
    let Ok(active_request) = activate_request_target(
        requests,
        &registry_key,
        RequestCancellationTarget::WorkspaceFilesystem(cancellation.clone()),
    ) else {
        return write_rpc_error(
            stream,
            id,
            -32600,
            "Duplicate active request id in MCP session",
            Some(session),
        );
    };
    if active_request.state == ActiveRequestState::CancellationRequested {
        cancellation.cancel();
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

fn handle_administrator_filesystem(
    stream: &mut TcpStream,
    id: Value,
    session: &str,
    mode: PermissionMode,
    arguments: &Value,
    context: AdministratorFilesystemContext<'_>,
) -> Result<(), ()> {
    let AdministratorFilesystemContext {
        guard,
        workspace,
        privileged,
        current_task,
        requests,
        stopping,
    } = context;
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
            return finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, error.to_mcp_result(), Some(session)),
            );
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
        return finalize_special_handler_request(
            &request_key,
            session,
            write_rpc_result(stream, id, denied.to_mcp_result(), Some(session)),
        );
    }
    if let Err(error) = execution_guard.validate_workspace_identity() {
        let request_key = request_diagnostic_key(&id);
        record_mcp_request_start(&request_key, session, "filesystem");
        project_filesystem_task(current_task, kind, TaskExecutionState::Blocked);
        current_task.project(CurrentTaskStatus::Idle);
        return finalize_special_handler_request(
            &request_key,
            session,
            write_rpc_result(stream, id, error.to_mcp_result(), Some(session)),
        );
    }
    let workspace_authority = execution_guard.workspace_authority();
    let spec = match administrator_filesystem_spec(workspace, &workspace_authority, &request) {
        Ok(spec) => spec,
        Err(error) => {
            let request_key = request_diagnostic_key(&id);
            record_mcp_request_start(&request_key, session, "filesystem");
            project_filesystem_task(current_task, kind, TaskExecutionState::Blocked);
            current_task.project(CurrentTaskStatus::Idle);
            return finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, error.to_mcp_result(), Some(session)),
            );
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
        return finalize_special_handler_request(
            &request_key,
            session,
            write_rpc_result(stream, id, elevation_required_result(), Some(session)),
        );
    };
    if !matches!(privileged.state(), PrivilegeState::Active { .. }) {
        finish_filesystem_task(
            current_task,
            kind,
            Some(TaskExecutionState::AwaitingAuthorization),
        );
        return finalize_special_handler_request(
            &request_key,
            session,
            write_rpc_result(stream, id, elevation_required_result(), Some(session)),
        );
    }

    let generation = PRIVILEGED_REQUEST_GENERATION.fetch_add(1, Ordering::Relaxed);
    let broker_request_id = format!("mcp-filesystem-{generation:x}");
    let registry_key = request_key_from_json(McpSessionId::new(session), &id)
        .expect("validated downstream request id");
    let Ok(active_request) = activate_request_target(
        requests,
        &registry_key,
        RequestCancellationTarget::PrivilegedFilesystem(broker_request_id.clone()),
    ) else {
        return write_rpc_error(
            stream,
            id,
            -32600,
            "Duplicate active request id in MCP session",
            Some(session),
        );
    };
    let cancellation_already_requested =
        active_request.state == ActiveRequestState::CancellationRequested;
    project_filesystem_task(current_task, kind, TaskExecutionState::Running);

    if let Err(error) = privileged.start_structured_filesystem(broker_request_id.clone(), spec) {
        requests.remove(&registry_key);
        return match error {
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
        };
    }
    if cancellation_already_requested {
        let _ = privileged.cancel_structured_filesystem(broker_request_id.clone());
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
    requests.remove(&registry_key);

    let filesystem = match filesystem {
        Ok(Ok(filesystem)) => filesystem,
        Ok(Err(code)) => {
            let terminal = if code == AdministratorFilesystemErrorCode::Cancelled {
                TaskExecutionState::Cancelled
            } else {
                TaskExecutionState::Failed
            };
            finish_filesystem_task(current_task, kind, Some(terminal));
            return finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(
                    stream,
                    id,
                    administrator_filesystem_error_result(code),
                    Some(session),
                ),
            );
        }
        Err(PrivilegedExecError::GateClosed(_)) => {
            finish_filesystem_task(
                current_task,
                kind,
                Some(TaskExecutionState::AwaitingAuthorization),
            );
            return finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, elevation_required_result(), Some(session)),
            );
        }
        Err(PrivilegedExecError::Broker(_)) => {
            finish_filesystem_task(current_task, kind, Some(TaskExecutionState::Failed));
            return finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(
                    stream,
                    id,
                    privileged_filesystem_unavailable_result(),
                    Some(session),
                ),
            );
        }
        Err(PrivilegedExecError::Filesystem(code)) => {
            let terminal = if code == AdministratorFilesystemErrorCode::Cancelled {
                TaskExecutionState::Cancelled
            } else {
                TaskExecutionState::Failed
            };
            finish_filesystem_task(current_task, kind, Some(terminal));
            return finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(
                    stream,
                    id,
                    administrator_filesystem_error_result(code),
                    Some(session),
                ),
            );
        }
    };
    let data = match administrator_filesystem_result_data(filesystem) {
        Ok(data) => data,
        Err(error) => {
            finish_filesystem_task(current_task, kind, Some(TaskExecutionState::Failed));
            return finalize_special_handler_request(
                &request_key,
                session,
                write_rpc_result(stream, id, error.to_mcp_result(), Some(session)),
            );
        }
    };
    finish_filesystem_task(current_task, kind, None);
    finalize_special_handler_request(
        &request_key,
        session,
        write_rpc_result(
            stream,
            id,
            stable_success(data, "Filesystem operation completed"),
            Some(session),
        ),
    )
}

fn administrator_filesystem_spec(
    workspace: &Path,
    authority: &WorkspaceResolver,
    request: &FilesystemRequest,
) -> Result<AdministratorFilesystemSpec, FacadeError> {
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
        FilesystemAction::Patch => {}
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
        if !FilesystemPathPolicy::allows(candidate) {
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
    let workspace_bound = !workspace_fields.is_empty() || request.action == FilesystemAction::Patch;
    let workspace_identity = if !workspace_bound {
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
            FilesystemAction::Replace => AdministratorFilesystemAction::Replace,
            FilesystemAction::Patch => AdministratorFilesystemAction::Patch,
            FilesystemAction::Search => AdministratorFilesystemAction::Search,
            FilesystemAction::SearchContent => AdministratorFilesystemAction::SearchContent,
            FilesystemAction::Copy => AdministratorFilesystemAction::Copy,
            FilesystemAction::Move => AdministratorFilesystemAction::Move,
            FilesystemAction::Delete => AdministratorFilesystemAction::Delete,
            FilesystemAction::Hash => AdministratorFilesystemAction::Hash,
        },
        path,
        source,
        destination,
        workspace_root: workspace_bound.then(|| workspace.to_string_lossy().into_owned()),
        workspace_identity,
        workspace_fields,
        recursive: request.recursive,
        max_depth: request.max_depth,
        max_entries,
        max_results,
        offset: request.offset,
        max_bytes,
        content_base64: request
            .content
            .as_ref()
            .map(|content| base64::engine::general_purpose::STANDARD.encode(content)),
        expected_sha256: request.expected_sha256.clone(),
        old: request.old.clone(),
        new: request.new.clone(),
        patch: request.patch.clone(),
        expected_files: request
            .expected_files
            .as_ref()
            .map(|values| {
                values
                    .iter()
                    .filter_map(|(path, hash)| {
                        hash.as_str().map(|hash| (path.clone(), hash.to_string()))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default(),
        pattern: request.pattern.clone(),
        case_sensitive: request.case_sensitive,
        max_file_bytes: u32::try_from(request.max_file_bytes).map_err(|_| {
            FacadeError::new(
                FacadeErrorCode::InvalidArgument,
                "invalid max_file_bytes",
                false,
            )
        })?,
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
    Ok(spec)
}

fn validate_workspace_side_path(
    authority: &WorkspaceResolver,
    path: &str,
    allow_missing_leaf: bool,
) -> Result<bool, FacadeError> {
    if !authority
        .input_is_within_execution_root(path)
        .map_err(normalize_path_authority_error)?
    {
        return Ok(false);
    }
    authority
        .resolve_workspace_path(Some(path), ".", allow_missing_leaf)
        .map_err(normalize_path_authority_error)?;
    Ok(true)
}

fn administrator_absolute_path(
    authority: &WorkspaceResolver,
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
        AdministratorFilesystemErrorCode::FileChanged => (
            FacadeErrorCode::FileChanged,
            "目标文件自读取后已发生变化",
            false,
        ),
        AdministratorFilesystemErrorCode::PatchConflict => (
            FacadeErrorCode::PatchConflict,
            "编辑上下文与当前文件不匹配",
            false,
        ),
        AdministratorFilesystemErrorCode::AmbiguousMatch => {
            (FacadeErrorCode::AmbiguousMatch, "编辑匹配不唯一", false)
        }
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
    let Ok(active_request) = activate_request_target(
        requests,
        &registry_key,
        RequestCancellationTarget::PrivilegedExecution(broker_request_id.clone()),
    ) else {
        return write_rpc_error(
            stream,
            id,
            -32600,
            "Duplicate active request id in MCP session",
            Some(session),
        );
    };
    let cancellation_already_requested =
        active_request.state == ActiveRequestState::CancellationRequested;
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
    if cancellation_already_requested {
        let _ = privileged.cancel_execute(broker_request_id.clone());
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
            Value::String(execution_guard.retain_local_output(
                McpSessionId::new(session),
                "stdout",
                execution.stdout.clone(),
            )),
        );
    }
    if stderr_inline_truncated {
        output_refs.insert(
            "stderr".into(),
            Value::String(execution_guard.retain_local_output(
                McpSessionId::new(session),
                "stderr",
                execution.stderr.clone(),
            )),
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

fn activate_request_target(
    requests: &RequestRegistry,
    key: &RequestKey,
    target: RequestCancellationTarget,
) -> Result<ActiveRequest, ()> {
    if requests.active(key).is_some() {
        requests.replace_cancellation_target(key, target).ok_or(())
    } else {
        requests.register(key.clone(), target).map_err(|_| ())?;
        requests.active(key).ok_or(())
    }
}

fn spawn_runtime_cancellation_relay(
    key: RequestKey,
    runtime_request_id: RpcRequestId,
    requests: RequestRegistry,
    cancellation: McpCancellationClient,
) -> Result<(), ()> {
    thread::Builder::new()
        .name("localbridge-runtime-cancellation-relay".into())
        .spawn(move || {
            for delay_ms in [25, 50, 100, 200, 400, 800, 800, 800] {
                thread::sleep(Duration::from_millis(delay_ms));
                let Some(active) = requests.active(&key) else {
                    break;
                };
                if active.state != ActiveRequestState::CancellationRequested
                    || !matches!(
                        &active.cancellation,
                        RequestCancellationTarget::Runtime(active_runtime_request_id)
                            if active_runtime_request_id == &runtime_request_id
                    )
                {
                    break;
                }
                let _ = cancellation.cancel_request(&rpc_request_id_to_json(&runtime_request_id));
            }
        })
        .map(|_| ())
        .map_err(|_| ())
}

fn next_private_request_id() -> RpcRequestId {
    let generation = PRIVATE_REQUEST_GENERATION.fetch_add(1, Ordering::Relaxed);
    RpcRequestId::String(format!("lb-private-{generation:x}"))
}

fn cancel_registered_request(
    request: &ActiveRequest,
    cancellation: &McpCancellationClient,
    privileged: Option<&Arc<dyn PrivilegedExecution>>,
    scheduler: &Scheduler,
    tasks: &TaskRegistry,
) -> Result<(), ()> {
    match &request.cancellation {
        RequestCancellationTarget::QueuedWork => {
            if let Some(task_id) = scheduler.cancel_queued_request(&request.key) {
                let _ = tasks.finish(&task_id, TerminalOutcome::Cancelled);
            }
            Ok(())
        }
        RequestCancellationTarget::Runtime(request_id) => cancellation
            .cancel_request(&rpc_request_id_to_json(request_id))
            .map_err(|_| ()),
        RequestCancellationTarget::WorkspaceFilesystem(cancellation) => {
            cancellation.cancel();
            Ok(())
        }
        RequestCancellationTarget::PrivilegedFilesystem(broker_request_id) => privileged
            .ok_or(())?
            .cancel_structured_filesystem(broker_request_id.clone())
            .map_err(|_| ()),
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
    McpSessionId::new(crate::security::random_prefixed_id("lb-mcp-session-"))
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
#[path = "server_tests.rs"]
mod tests;
