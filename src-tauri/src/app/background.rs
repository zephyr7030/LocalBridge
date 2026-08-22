use std::ffi::OsStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::control_plane::convergence::{
    ConnectionProfile, ConvergenceSnapshot, DesiredState, DesiredStateOwner, DesiredWorkspace,
    EffectiveConnection, EffectiveWorkspaceAuthority, ObservedState, ReconcilePlan, Reconciler,
    ServiceIntent,
};
use crate::control_plane::snapshot::{
    AuthorityProjection, ConnectionProjection, ControlPlaneSnapshot, ControlPlaneSnapshotOwner,
    EffectiveAvailability, LastToolProjection, OutageProjection, ProjectionSection,
    RuntimeProjection, SettingsProjection, SnapshotDraft, TaskAggregate, WorkspaceProjection,
};
use crate::control_plane::update::{UpdateStartError, UpdateStateOwner};
#[cfg(windows)]
use crate::credentials::WindowsCredentialStore;
use crate::diagnostics::{
    DiagnosticsOutageInput, record_recovery_attempt_event, record_runtime_user_events,
};
use crate::domain::{
    ErrorCategory, FaultSource, GitHubRepository, OperationError, PersistentFault,
    UpdateCheckTrigger, UpdateLifecycle,
};
#[cfg(windows)]
use crate::mcp::{
    CurrentTaskWake, InternalBearer, ProductionRuntimeConfig, ProductionRuntimeDriver,
};
use crate::privilege::PrivilegeController;
#[cfg(windows)]
use crate::privilege::{SESSION_NONCE_BYTES, random_session_nonce};
use crate::runtime::{
    AutoRecoveryRuntime, OrchestratorError, RecoveryAttemptEvent, RecoveryCancellation,
    RecoveryClock, RecoveryController, RecoveryOutcome, RuntimeDriver, RuntimeOrchestrator,
    RuntimeOutage, SystemRecoveryClock, WorkspaceSwitchError,
};
#[cfg(windows)]
use crate::state::{
    CurrentTaskStatus, LastToolTiming, PermissionMode, PrivilegeState, RuntimeComponent,
    RuntimeFault, RuntimeState,
};
use crate::tunnel::ConnectorEndpoint;
use std::path::{Path, PathBuf};

use super::update::UpdateChecker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupMode {
    Foreground,
    Background,
}

impl StartupMode {
    pub fn from_args<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        if args
            .into_iter()
            .any(|arg| arg.as_ref() == OsStr::new("--background"))
        {
            Self::Background
        } else {
            Self::Foreground
        }
    }

    pub const fn creates_main_window_at_startup(self) -> bool {
        matches!(self, Self::Foreground)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundRecoveryAction {
    None,
    ShowFinalErrorWindow,
}

pub const fn attention_action(user_attention_required: bool) -> BackgroundRecoveryAction {
    if user_attention_required {
        BackgroundRecoveryAction::ShowFinalErrorWindow
    } else {
        BackgroundRecoveryAction::None
    }
}

pub const fn recovery_action(outcome: &RecoveryOutcome) -> BackgroundRecoveryAction {
    match outcome {
        RecoveryOutcome::Exhausted {
            user_attention_required,
            ..
        }
        | RecoveryOutcome::NonRecoverable {
            user_attention_required,
            ..
        } => attention_action(*user_attention_required),
        RecoveryOutcome::Recovered { .. } => BackgroundRecoveryAction::None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopExitError {
    Runtime,
    Privilege,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopRuntimeStartError {
    AlreadyRegistered,
    Runtime(OrchestratorError),
}

impl std::fmt::Display for DesktopRuntimeStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRegistered => f.write_str("desktop runtime is already registered"),
            Self::Runtime(error) => write!(f, "desktop runtime start failed: {error}"),
        }
    }
}

impl std::error::Error for DesktopRuntimeStartError {}

pub trait ExitRuntime {
    fn stop_tunnel_for_exit(&mut self) -> Result<(), DesktopExitError>;
    fn finish_exit_after_tunnel(&mut self) -> Result<(), DesktopExitError>;

    fn runtime_snapshot(&self) -> DesktopRuntimeSnapshot {
        DesktopRuntimeSnapshot::inactive()
    }

    fn task_aggregate_snapshot(&self) -> TaskAggregate {
        TaskAggregate::idle()
    }

    fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        None
    }

    fn switch_workspace(&mut self, _candidate: &Path) -> Result<(), DesktopRuntimeControlError> {
        Err(DesktopRuntimeControlError::NoActiveRuntime)
    }

    fn manual_retry(&mut self) -> Result<RecoveryOutcome, DesktopRuntimeControlError> {
        Err(DesktopRuntimeControlError::NoActiveOutage)
    }

    fn monitor_recovery(&mut self) -> Option<RecoveryOutcome> {
        None
    }

    fn monitor_recovery_with_observer(
        &mut self,
        _observer: &mut dyn FnMut(RecoveryAttemptEvent),
    ) -> Option<RecoveryOutcome> {
        self.monitor_recovery()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopOutageSnapshot {
    pub generation: u64,
    pub request_id: String,
    pub component: RuntimeComponent,
    pub fault: RuntimeFault,
    pub user_attention_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopRuntimeSnapshot {
    pub active: bool,
    pub state: RuntimeState,
    pub current_task: CurrentTaskStatus,
    pub current_task_elapsed_ms: Option<u64>,
    pub last_tool: Option<LastToolTiming>,
    pub configured_workspace: Option<PathBuf>,
    pub connection_profile: Option<ConnectionProfile>,
    pub outage: Option<DesktopOutageSnapshot>,
}

impl DesktopRuntimeSnapshot {
    fn inactive() -> Self {
        Self {
            active: false,
            state: RuntimeState::Stopped,
            current_task: CurrentTaskStatus::Idle,
            current_task_elapsed_ms: None,
            last_tool: None,
            configured_workspace: None,
            connection_profile: None,
            outage: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopRuntimeControlError {
    NoActiveRuntime,
    NoActiveOutage,
    Runtime(RuntimeFault),
    Workspace(WorkspaceSwitchError),
}

struct ProductionRuntimeOwner {
    active: Option<Box<dyn ExitRuntime + Send>>,
    observed_inactive: DesktopRuntimeSnapshot,
}

impl Default for ProductionRuntimeOwner {
    fn default() -> Self {
        Self {
            active: None,
            observed_inactive: DesktopRuntimeSnapshot::inactive(),
        }
    }
}

impl ProductionRuntimeOwner {
    fn is_active(&self) -> bool {
        self.active.is_some()
    }

    fn activate_boxed(
        &mut self,
        runtime: Box<dyn ExitRuntime + Send>,
    ) -> Result<(), DesktopRuntimeStartError> {
        if self.active.is_some() {
            return Err(DesktopRuntimeStartError::AlreadyRegistered);
        }
        self.active = Some(runtime);
        Ok(())
    }

    fn take_active(&mut self) -> Option<Box<dyn ExitRuntime + Send>> {
        self.active.take()
    }

    fn observe_inactive(&mut self, snapshot: DesktopRuntimeSnapshot) {
        debug_assert!(!snapshot.active);
        self.observed_inactive = snapshot;
    }

    fn snapshot(&self) -> DesktopRuntimeSnapshot {
        self.active
            .as_deref()
            .map(ExitRuntime::runtime_snapshot)
            .unwrap_or_else(|| self.observed_inactive.clone())
    }

    fn task_aggregate_snapshot(&self) -> TaskAggregate {
        self.active
            .as_deref()
            .map(ExitRuntime::task_aggregate_snapshot)
            .unwrap_or_else(TaskAggregate::idle)
    }
}

impl ExitRuntime for ProductionRuntimeOwner {
    fn stop_tunnel_for_exit(&mut self) -> Result<(), DesktopExitError> {
        match self.active.as_deref_mut() {
            Some(runtime) => runtime.stop_tunnel_for_exit(),
            None => Ok(()),
        }
    }

    fn finish_exit_after_tunnel(&mut self) -> Result<(), DesktopExitError> {
        match self.active.as_deref_mut() {
            Some(runtime) => runtime.finish_exit_after_tunnel(),
            None => Ok(()),
        }
    }

    fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        self.active
            .as_deref()
            .and_then(ExitRuntime::connector_endpoint)
    }
}

impl<D> ExitRuntime for RuntimeOrchestrator<D>
where
    D: RuntimeDriver,
    RuntimeOrchestrator<D>: Send,
{
    fn stop_tunnel_for_exit(&mut self) -> Result<(), DesktopExitError> {
        RuntimeOrchestrator::stop_tunnel_for_exit(self).map_err(|_| DesktopExitError::Runtime)
    }

    fn finish_exit_after_tunnel(&mut self) -> Result<(), DesktopExitError> {
        RuntimeOrchestrator::finish_exit_after_tunnel(self).map_err(|_| DesktopExitError::Runtime)
    }

    fn runtime_snapshot(&self) -> DesktopRuntimeSnapshot {
        let timing = self.current_task_timing();
        DesktopRuntimeSnapshot {
            active: true,
            state: self.state().clone(),
            current_task: timing.status,
            current_task_elapsed_ms: timing.elapsed_ms,
            last_tool: timing.last_tool,
            configured_workspace: self.configured_workspace().map(Path::to_path_buf),
            connection_profile: self.configured_connection_profile(),
            outage: self.active_outage().map(|outage| DesktopOutageSnapshot {
                generation: outage.id.get(),
                request_id: outage.request_id.clone(),
                component: outage.component,
                fault: outage.fault.clone(),
                user_attention_required: outage.user_attention_emitted(),
            }),
        }
    }

    fn task_aggregate_snapshot(&self) -> TaskAggregate {
        self.task_aggregate()
    }

    fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        RuntimeOrchestrator::connector_endpoint(self)
    }

    fn switch_workspace(&mut self, candidate: &Path) -> Result<(), DesktopRuntimeControlError> {
        RuntimeOrchestrator::switch_workspace_to(self, candidate)
            .map_err(DesktopRuntimeControlError::Workspace)
    }

    fn manual_retry(&mut self) -> Result<RecoveryOutcome, DesktopRuntimeControlError> {
        let outage = self
            .active_outage()
            .cloned()
            .ok_or(DesktopRuntimeControlError::NoActiveOutage)?;
        let mut controller = RecoveryController::new(SystemRecoveryClock::default());
        Ok(controller.manual_retry(
            self,
            RuntimeOutage::classify(outage.component, outage.fault),
        ))
    }
}

impl<D, C> ExitRuntime for AutoRecoveryRuntime<D, C>
where
    D: RuntimeDriver,
    C: RecoveryClock + Send,
    AutoRecoveryRuntime<D, C>: Send,
{
    fn stop_tunnel_for_exit(&mut self) -> Result<(), DesktopExitError> {
        self.orchestrator_mut()
            .stop_tunnel_for_exit()
            .map_err(|_| DesktopExitError::Runtime)
    }

    fn finish_exit_after_tunnel(&mut self) -> Result<(), DesktopExitError> {
        self.orchestrator_mut()
            .finish_exit_after_tunnel()
            .map_err(|_| DesktopExitError::Runtime)
    }

    fn runtime_snapshot(&self) -> DesktopRuntimeSnapshot {
        let runtime = self.runtime();
        let timing = runtime.current_task_timing();
        DesktopRuntimeSnapshot {
            active: true,
            state: runtime.state().clone(),
            current_task: timing.status,
            current_task_elapsed_ms: timing.elapsed_ms,
            last_tool: timing.last_tool,
            configured_workspace: runtime.configured_workspace().map(Path::to_path_buf),
            connection_profile: runtime.configured_connection_profile(),
            outage: runtime.active_outage().map(|outage| DesktopOutageSnapshot {
                generation: outage.id.get(),
                request_id: outage.request_id.clone(),
                component: outage.component,
                fault: outage.fault.clone(),
                user_attention_required: outage.user_attention_emitted(),
            }),
        }
    }

    fn task_aggregate_snapshot(&self) -> TaskAggregate {
        self.runtime().task_aggregate()
    }

    fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        self.runtime().connector_endpoint()
    }

    fn switch_workspace(&mut self, candidate: &Path) -> Result<(), DesktopRuntimeControlError> {
        self.switch_workspace_after_control_cancellation(candidate)
            .map_err(DesktopRuntimeControlError::Workspace)
    }

    fn manual_retry(&mut self) -> Result<RecoveryOutcome, DesktopRuntimeControlError> {
        self.manual_retry_current_outage()
            .ok_or(DesktopRuntimeControlError::NoActiveOutage)
    }

    fn monitor_recovery(&mut self) -> Option<RecoveryOutcome> {
        self.monitor_once()
    }

    fn monitor_recovery_with_observer(
        &mut self,
        observer: &mut dyn FnMut(RecoveryAttemptEvent),
    ) -> Option<RecoveryOutcome> {
        self.monitor_once_with_observer(observer)
    }
}

pub trait PrivilegeExit {
    fn close_gate_and_stop_broker(&self) -> Result<(), DesktopExitError>;
}

impl PrivilegeExit for PrivilegeController {
    fn close_gate_and_stop_broker(&self) -> Result<(), DesktopExitError> {
        self.disable().map_err(|_| DesktopExitError::Privilege)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShutdownReport {
    pub tunnel_stop_failed: bool,
    pub privilege_stop_failed: bool,
    pub lower_runtime_stop_failed: bool,
}

pub fn shutdown_in_security_order<R, P>(
    mut runtime: Option<&mut R>,
    privilege: &P,
) -> ShutdownReport
where
    R: ExitRuntime + ?Sized,
    P: PrivilegeExit + ?Sized,
{
    let mut report = ShutdownReport::default();
    if let Some(runtime) = runtime.as_mut() {
        report.tunnel_stop_failed = (**runtime).stop_tunnel_for_exit().is_err();
    }
    report.privilege_stop_failed = privilege.close_gate_and_stop_broker().is_err();
    if let Some(runtime) = runtime.as_mut() {
        report.lower_runtime_stop_failed = (**runtime).finish_exit_after_tunnel().is_err();
    }
    report
}

pub struct DesktopLifecycle {
    privilege: PrivilegeController,
    desired: DesiredStateOwner,
    runtime_operation: Arc<Mutex<()>>,
    runtime: Arc<Mutex<ProductionRuntimeOwner>>,
    recovery_cancellation: RecoveryCancellation,
    runtime_control_generation: Arc<AtomicU64>,
    snapshot_owner: ControlPlaneSnapshotOwner,
    update_checker: UpdateChecker,
    #[cfg(windows)]
    foreground_start_pending: Arc<Mutex<Option<ProductionRuntimeConfig>>>,
    close_window_continue_running: Arc<AtomicBool>,
    watchdog_shutdown: Mutex<Option<mpsc::Sender<()>>>,
    watchdog_thread: Mutex<Option<JoinHandle<()>>>,
}

const RUNTIME_WATCHDOG_INTERVAL: Duration = Duration::from_millis(500);

fn next_projection_display_wait(activity_active: bool, last_tool_age_ms: Option<u64>) -> Duration {
    if activity_active {
        return Duration::from_millis(500);
    }
    let Some(age) = last_tool_age_ms else {
        return Duration::from_secs(30);
    };
    let wait_ms = if age < 60_000 {
        1_000 - age % 1_000
    } else if age < 3_600_000 {
        60_000 - age % 60_000
    } else if age < 86_400_000 {
        86_400_000 - age
    } else {
        86_400_000 - age % 86_400_000
    };
    Duration::from_millis(wait_ms.max(1))
}

fn publish_control_plane_observation(
    owner: &ControlPlaneSnapshotOwner,
    desired: &DesiredStateOwner,
    privilege: &PrivilegeController,
    runtime: DesktopRuntimeSnapshot,
    activity: Option<TaskAggregate>,
) -> ControlPlaneSnapshot {
    let previous = owner.read();
    let broker = privilege.state();
    let convergence = ConvergenceSnapshot::derive(
        desired.snapshot(),
        ObservedState {
            broker: broker.clone(),
            runtime: runtime.state.clone(),
            workspace: runtime.configured_workspace.clone(),
            connection: runtime.connection_profile.clone(),
        },
    );
    let runtime_value = RuntimeProjection {
        active: runtime.active,
        state: runtime.state.clone(),
        local_environment_available: previous
            .runtime
            .value()
            .and_then(|runtime| runtime.local_environment_available),
        current_task_elapsed_ms: runtime.current_task_elapsed_ms,
        last_tool: runtime.last_tool.as_ref().map(|tool| LastToolProjection {
            kind: tool.kind,
            summary: tool.summary.as_deref().map(str::to_owned),
            age_ms: tool.age_ms,
        }),
        outage: runtime.outage.as_ref().map(|outage| OutageProjection {
            generation: outage.generation,
            operation_id: outage.request_id.clone(),
            user_attention_required: outage.user_attention_required,
        }),
    };
    let runtime_section = if activity.is_some() {
        ProjectionSection::ready(runtime_value)
    } else {
        ProjectionSection::stale(previous.runtime.into_value())
    };
    let authority = ProjectionSection::ready(AuthorityProjection {
        desired: convergence.effective.authority.configured,
        effective: convergence.effective.authority.execution,
        broker: broker.clone(),
        elevated_active: convergence.effective.authority.elevated_active,
    });
    let workspace = ProjectionSection::ready(WorkspaceProjection {
        desired_id: convergence
            .desired
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.id.as_ref())
            .map(|id| id.as_str().to_owned()),
        desired_path: convergence
            .desired
            .workspace
            .as_ref()
            .map(|workspace| workspace.execution_path.to_string_lossy().into_owned()),
        observed_path: convergence
            .observed
            .workspace
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        effective: match convergence.effective.workspace {
            EffectiveWorkspaceAuthority::Available(_) => EffectiveAvailability::Available,
            EffectiveWorkspaceAuthority::Unavailable(_) => EffectiveAvailability::Unavailable,
        },
    });
    let connection = ProjectionSection::ready(ConnectionProjection {
        desired_tunnel_id: convergence
            .desired
            .connection
            .as_ref()
            .map(|profile| profile.tunnel_id.expose().to_owned()),
        desired_credential_epoch: convergence
            .desired
            .connection
            .as_ref()
            .map(|profile| profile.credential_epoch),
        observed_tunnel_id: convergence
            .observed
            .connection
            .as_ref()
            .map(|profile| profile.tunnel_id.expose().to_owned()),
        observed_credential_epoch: convergence
            .observed
            .connection
            .as_ref()
            .map(|profile| profile.credential_epoch),
        effective: match convergence.effective.connection {
            EffectiveConnection::Available(_) => EffectiveAvailability::Available,
            EffectiveConnection::Unavailable(_) => EffectiveAvailability::Unavailable,
        },
    });
    let mut active_faults = previous
        .active_faults
        .into_iter()
        .filter(|fault| fault.source == FaultSource::Settings)
        .collect::<Vec<_>>();
    if let RuntimeState::Faulted(fault) = &runtime.state {
        active_faults.push(persistent_fault(
            previous_fault(&owner.read(), "runtime"),
            "runtime",
            FaultSource::Runtime,
            OperationError::new(
                format!("Runtime.{fault:?}"),
                ErrorCategory::Unavailable,
                "Runtime is unavailable",
                true,
            ),
        ));
    }
    if let PrivilegeState::Faulted(fault) = &broker {
        active_faults.push(persistent_fault(
            previous_fault(&owner.read(), "authority"),
            "authority",
            FaultSource::Authority,
            OperationError::new(
                format!("Authority.{fault:?}"),
                ErrorCategory::Authorization,
                "Privilege broker is unavailable",
                true,
            ),
        ));
    }
    let (scheduler, activity) = match activity {
        Some(activity) => (
            ProjectionSection::ready(activity.scheduler.clone()),
            ProjectionSection::ready(activity),
        ),
        None => (
            ProjectionSection::stale(previous.scheduler.into_value()),
            ProjectionSection::stale(previous.activity.into_value()),
        ),
    };
    owner.publish(SnapshotDraft {
        runtime: runtime_section,
        authority,
        scheduler,
        workspace,
        connection,
        settings: previous.settings,
        activity,
        update: previous.update,
        active_faults,
    })
}

fn previous_fault(snapshot: &ControlPlaneSnapshot, id: &str) -> Option<PersistentFault> {
    snapshot
        .active_faults
        .iter()
        .find(|fault| fault.id == id)
        .cloned()
}

fn persistent_fault(
    previous: Option<PersistentFault>,
    id: &str,
    source: FaultSource,
    error: OperationError,
) -> PersistentFault {
    let now = now_unix_ms();
    PersistentFault {
        id: id.to_owned(),
        source,
        error,
        first_seen_at_ms: previous.map_or(now, |fault| fault.first_seen_at_ms),
        last_seen_at_ms: now,
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

impl std::fmt::Debug for DesktopLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopLifecycle")
            .field("privilege", &self.privilege)
            .field("production_runtime_active", &self.runtime_snapshot().active)
            .finish()
    }
}

impl DesktopLifecycle {
    pub fn new(privilege: PrivilegeController) -> Self {
        let desired = DesiredStateOwner::default();
        let runtime_operation = Arc::new(Mutex::new(()));
        let runtime = Arc::new(Mutex::new(ProductionRuntimeOwner::default()));
        let recovery_cancellation = RecoveryCancellation::default();
        let runtime_control_generation = Arc::new(AtomicU64::new(0));
        let snapshot_owner = ControlPlaneSnapshotOwner::default();
        let update_owner = UpdateStateOwner::default();
        let update_snapshot_owner = snapshot_owner.clone();
        update_owner
            .bind_publisher(Arc::new(move |state| {
                update_snapshot_owner.publish_update(ProjectionSection::ready(state));
            }))
            .expect("update snapshot publisher must have one binding");
        let update_checker = UpdateChecker::production(update_owner);
        #[cfg(windows)]
        let foreground_start_pending = Arc::new(Mutex::new(None));
        let close_window_continue_running = Arc::new(AtomicBool::new(true));
        let monitor_operation = Arc::clone(&runtime_operation);
        let monitor_runtime = Arc::clone(&runtime);
        let monitor_control_plane = snapshot_owner.clone();
        let monitor_desired = desired.clone();
        let monitor_privilege = privilege.clone();
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let watchdog_thread = thread::Builder::new()
            .name("localbridge-runtime-watchdog".into())
            .spawn(move || {
                loop {
                    match shutdown_rx.recv_timeout(RUNTIME_WATCHDOG_INTERVAL) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    let _operation = match monitor_operation.try_lock() {
                        Ok(operation) => operation,
                        Err(TryLockError::WouldBlock) => continue,
                        Err(TryLockError::Poisoned(error)) => error.into_inner(),
                    };
                    let mut owner = monitor_runtime
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let Some(runtime) = owner.active.as_deref_mut() else {
                        continue;
                    };
                    let mut recovery_observer = |event| record_recovery_attempt_event(&event);
                    let _ = runtime.monitor_recovery_with_observer(&mut recovery_observer);
                    let snapshot = owner.snapshot();
                    let activity = owner.task_aggregate_snapshot();
                    drop(owner);
                    record_desktop_runtime_events(&snapshot, &monitor_privilege);
                    publish_control_plane_observation(
                        &monitor_control_plane,
                        &monitor_desired,
                        &monitor_privilege,
                        snapshot,
                        Some(activity),
                    );
                }
            })
            .expect("runtime watchdog thread must start");
        let lifecycle = Self {
            privilege,
            desired,
            runtime_operation,
            runtime,
            recovery_cancellation,
            runtime_control_generation,
            snapshot_owner,
            update_checker,
            #[cfg(windows)]
            foreground_start_pending,
            close_window_continue_running,
            watchdog_shutdown: Mutex::new(Some(shutdown_tx)),
            watchdog_thread: Mutex::new(Some(watchdog_thread)),
        };
        lifecycle.publish_current_observation();
        lifecycle
    }

    pub fn privilege(&self) -> &PrivilegeController {
        &self.privilege
    }

    pub fn update_lifecycle(&self) -> UpdateLifecycle {
        self.update_checker.owner().snapshot()
    }

    pub fn update_repository(&self) -> Option<GitHubRepository> {
        self.update_checker.owner().repository()
    }

    pub fn start_update_check(&self, trigger: UpdateCheckTrigger) -> Result<(), UpdateStartError> {
        self.update_checker.start(trigger)
    }

    pub fn desired_state(&self) -> DesiredStateOwner {
        self.desired.clone()
    }

    pub fn replace_desired_state(&self, state: DesiredState) {
        let before = self.desired.snapshot().revision;
        let after = self.desired.replace(state);
        if after != before {
            self.publish_current_observation();
        }
    }

    pub fn set_desired_permission(&self, permission: PermissionMode) {
        let before = self.desired.snapshot().revision;
        let after = self.desired.set_permission(permission);
        if after != before {
            self.publish_current_observation();
        }
    }

    pub fn set_desired_workspace(&self, workspace: Option<DesiredWorkspace>) {
        let before = self.desired.snapshot().revision;
        let after = self.desired.set_workspace(workspace);
        if after != before {
            self.publish_current_observation();
        }
    }

    pub fn set_desired_services(&self, services: ServiceIntent) {
        let before = self.desired.snapshot().revision;
        let after = self.desired.set_services(services);
        if after != before {
            self.publish_current_observation();
        }
    }

    pub fn set_desired_connection(&self, connection: Option<ConnectionProfile>) {
        let before = self.desired.snapshot().revision;
        let after = self.desired.set_connection(connection);
        if after != before {
            self.publish_current_observation();
        }
    }

    pub fn mark_connection_credentials_changed(&self) {
        let before = self.desired.snapshot().revision;
        let after = self.desired.mark_credentials_changed();
        if after != before {
            self.publish_current_observation();
        }
    }

    pub fn convergence_snapshot(&self) -> ConvergenceSnapshot {
        let runtime = self.runtime_snapshot();
        ConvergenceSnapshot::derive(
            self.desired.snapshot(),
            ObservedState {
                broker: self.privilege.state(),
                runtime: runtime.state,
                workspace: runtime.configured_workspace,
                connection: runtime.connection_profile,
            },
        )
    }

    pub fn reconciliation_plan(&self) -> ReconcilePlan {
        Reconciler::plan(&self.convergence_snapshot())
    }

    pub fn publish_settings_snapshot(
        &self,
        settings: SettingsProjection,
        error: Option<OperationError>,
    ) -> ControlPlaneSnapshot {
        let previous = self.snapshot_owner.read();
        let mut active_faults = previous
            .active_faults
            .iter()
            .filter(|fault| fault.source != FaultSource::Settings)
            .cloned()
            .collect::<Vec<_>>();
        let settings = match error {
            Some(error) => {
                active_faults.push(persistent_fault(
                    previous_fault(&previous, "settings"),
                    "settings",
                    FaultSource::Settings,
                    error,
                ));
                ProjectionSection::faulted(Some(settings))
            }
            None => ProjectionSection::ready(settings),
        };
        self.snapshot_owner.publish(SnapshotDraft {
            runtime: previous.runtime,
            authority: previous.authority,
            scheduler: previous.scheduler,
            workspace: previous.workspace,
            connection: previous.connection,
            settings,
            activity: previous.activity,
            update: previous.update,
            active_faults,
        })
    }

    pub fn publish_settings_fault(&self, error: OperationError) -> ControlPlaneSnapshot {
        let settings = self
            .snapshot_owner
            .read()
            .settings
            .value()
            .cloned()
            .unwrap_or_default();
        self.publish_settings_snapshot(settings, Some(error))
    }

    pub fn publish_local_environment_observation(&self, available: bool) -> ControlPlaneSnapshot {
        let previous = self.snapshot_owner.read();
        let runtime = previous.runtime.map(|mut runtime| {
            runtime.local_environment_available = Some(available);
            runtime
        });
        self.snapshot_owner.publish(SnapshotDraft {
            runtime,
            authority: previous.authority,
            scheduler: previous.scheduler,
            workspace: previous.workspace,
            connection: previous.connection,
            settings: previous.settings,
            activity: previous.activity,
            update: previous.update,
            active_faults: previous.active_faults,
        })
    }

    pub fn control_plane_snapshot(&self) -> ControlPlaneSnapshot {
        self.snapshot_owner.read()
    }

    pub(crate) fn publish_current_observation(&self) -> ControlPlaneSnapshot {
        let owner = match self.runtime.try_lock() {
            Ok(owner) => owner,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return self.snapshot_owner.mark_observation_stale(),
        };
        let runtime = owner.snapshot();
        let activity = owner.task_aggregate_snapshot();
        drop(owner);
        publish_control_plane_observation(
            &self.snapshot_owner,
            &self.desired,
            &self.privilege,
            runtime,
            Some(activity),
        )
    }

    pub fn set_close_window_continue_running(&self, enabled: bool) {
        self.close_window_continue_running
            .store(enabled, Ordering::Release);
    }

    pub fn close_window_continue_running(&self) -> bool {
        self.close_window_continue_running.load(Ordering::Acquire)
    }

    pub fn backend_handle(&self) -> DesktopBackendHandle {
        DesktopBackendHandle {
            privilege: self.privilege.clone(),
            desired: self.desired.clone(),
            runtime_operation: Arc::clone(&self.runtime_operation),
            runtime: Arc::clone(&self.runtime),
            recovery_cancellation: self.recovery_cancellation.clone(),
            runtime_control_generation: Arc::clone(&self.runtime_control_generation),
            snapshot_owner: self.snapshot_owner.clone(),
            #[cfg(windows)]
            foreground_start_pending: Arc::clone(&self.foreground_start_pending),
        }
    }

    #[cfg(windows)]
    pub fn stage_foreground_start(&self, config: ProductionRuntimeConfig) -> bool {
        if self.runtime_snapshot().active {
            return false;
        }
        let mut pending = self
            .foreground_start_pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if pending.is_some() {
            return false;
        }
        *pending = Some(config);
        self.set_desired_services(ServiceIntent::Enabled);
        true
    }

    #[cfg(windows)]
    pub fn foreground_start_is_pending(&self) -> bool {
        self.foreground_start_pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    #[cfg(windows)]
    pub fn start_staged_foreground_after_ui_ready(&self) -> Result<bool, std::io::Error> {
        let config = self
            .foreground_start_pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let Some(config) = config else {
            return Ok(false);
        };
        if self.runtime_snapshot().active {
            return Ok(false);
        }
        self.backend_handle()
            .spawn_start_production_runtime(config)
            .map(|_| true)
    }

    #[cfg(windows)]
    fn clear_staged_foreground_start(&self) {
        self.foreground_start_pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }

    #[cfg(windows)]
    pub fn start_production_runtime(
        &self,
        config: ProductionRuntimeConfig,
    ) -> Result<(), DesktopRuntimeStartError> {
        self.backend_handle().start_production_runtime(config)
    }

    pub fn shutdown(&self) -> ShutdownReport {
        self.shutdown_with_privilege(&self.privilege)
    }

    pub fn stop_services_for_manual_action(&self) -> ShutdownReport {
        self.set_desired_services(ServiceIntent::Disabled);
        #[cfg(windows)]
        self.clear_staged_foreground_start();
        self.recovery_cancellation.cancel();
        self.runtime_control_generation
            .fetch_add(1, Ordering::AcqRel);
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut active = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_active();
        let report = shutdown_in_security_order(active.as_deref_mut(), &self.privilege);
        self.publish_runtime_observation(DesktopRuntimeSnapshot::inactive());
        report
    }

    pub fn stop_runtime_for_control_plane(&self) -> Result<(), DesktopRuntimeControlError> {
        self.set_desired_services(ServiceIntent::Disabled);
        #[cfg(windows)]
        self.clear_staged_foreground_start();
        self.recovery_cancellation.cancel();
        self.runtime_control_generation
            .fetch_add(1, Ordering::AcqRel);
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut active = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_active();
        let tunnel = active
            .as_deref_mut()
            .map(ExitRuntime::stop_tunnel_for_exit)
            .unwrap_or(Ok(()));
        let lower = active
            .as_deref_mut()
            .map(ExitRuntime::finish_exit_after_tunnel)
            .unwrap_or(Ok(()));
        self.publish_runtime_observation(DesktopRuntimeSnapshot::inactive());
        if tunnel.is_err() || lower.is_err() {
            return Err(DesktopRuntimeControlError::Runtime(RuntimeFault::Unknown));
        }
        Ok(())
    }

    pub fn task_aggregate_snapshot(&self) -> Option<TaskAggregate> {
        match self.runtime.try_lock() {
            Ok(owner) => Some(owner.task_aggregate_snapshot()),
            Err(TryLockError::Poisoned(error)) => {
                Some(error.into_inner().task_aggregate_snapshot())
            }
            Err(TryLockError::WouldBlock) => None,
        }
    }

    pub fn runtime_snapshot(&self) -> DesktopRuntimeSnapshot {
        self.runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    pub fn wait_projection_change_after(&self, since: u64) -> u64 {
        let snapshot = self.snapshot_owner.read();
        let activity_active = snapshot.activity.value().is_some_and(|activity| {
            activity.foreground_task.is_some() || activity.detached_execution.is_some()
        });
        let last_tool_age_ms = snapshot
            .runtime
            .value()
            .and_then(|runtime| runtime.last_tool.as_ref())
            .map(|last| last.age_ms);
        let timeout = next_projection_display_wait(activity_active, last_tool_age_ms);
        self.snapshot_owner.wait_after(since, timeout)
    }

    pub fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        self.runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .connector_endpoint()
    }

    pub fn switch_runtime_workspace(
        &self,
        candidate: &Path,
    ) -> Result<(), DesktopRuntimeControlError> {
        self.recovery_cancellation.cancel();
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut owner = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = owner
            .active
            .as_deref_mut()
            .ok_or(DesktopRuntimeControlError::NoActiveRuntime)?
            .switch_workspace(candidate);
        let snapshot = owner.snapshot();
        drop(owner);
        self.publish_runtime_observation(snapshot);
        result
    }

    pub fn manual_retry_after_attention(
        &self,
    ) -> Result<RecoveryOutcome, DesktopRuntimeControlError> {
        self.recovery_cancellation.cancel();
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut owner = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = owner
            .active
            .as_deref_mut()
            .ok_or(DesktopRuntimeControlError::NoActiveRuntime)?
            .manual_retry();
        let snapshot = owner.snapshot();
        drop(owner);
        self.publish_runtime_observation(snapshot);
        result
    }

    fn shutdown_with_privilege<P>(&self, privilege: &P) -> ShutdownReport
    where
        P: PrivilegeExit + ?Sized,
    {
        #[cfg(windows)]
        self.clear_staged_foreground_start();
        self.recovery_cancellation.cancel();
        self.runtime_control_generation
            .fetch_add(1, Ordering::AcqRel);
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut active = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_active();
        let report = shutdown_in_security_order(active.as_deref_mut(), privilege);
        self.publish_runtime_observation(DesktopRuntimeSnapshot::inactive());
        report
    }

    #[cfg(test)]
    fn install_runtime_for_test<R>(&self, runtime: R) -> Result<(), DesktopRuntimeStartError>
    where
        R: ExitRuntime + Send + 'static,
    {
        self.recovery_cancellation.cancel();
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut owner = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        owner.activate_boxed(Box::new(runtime))?;
        let snapshot = owner.snapshot();
        drop(owner);
        self.publish_runtime_observation(snapshot);
        Ok(())
    }

    #[cfg(test)]
    fn shutdown_with_privilege_for_test<P>(&self, privilege: &P) -> ShutdownReport
    where
        P: PrivilegeExit + ?Sized,
    {
        self.shutdown_with_privilege(privilege)
    }

    fn publish_runtime_observation(&self, snapshot: DesktopRuntimeSnapshot) {
        record_desktop_runtime_events(&snapshot, &self.privilege);
        if !snapshot.active {
            self.runtime
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .observe_inactive(snapshot);
        }
        self.publish_current_observation();
    }
}

fn record_desktop_runtime_events(
    snapshot: &DesktopRuntimeSnapshot,
    privilege: &PrivilegeController,
) {
    let outage = snapshot
        .outage
        .as_ref()
        .map(|outage| DiagnosticsOutageInput {
            generation: outage.generation,
            request_id: outage.request_id.clone(),
            component: outage.component,
            fault: outage.fault.clone(),
            user_attention_required: outage.user_attention_required,
        });
    record_runtime_user_events(
        &snapshot.state,
        outage.as_ref(),
        &privilege.refresh_broker_state(),
    );
}

#[derive(Clone)]
pub struct DesktopBackendHandle {
    privilege: PrivilegeController,
    desired: DesiredStateOwner,
    runtime_operation: Arc<Mutex<()>>,
    runtime: Arc<Mutex<ProductionRuntimeOwner>>,
    recovery_cancellation: RecoveryCancellation,
    runtime_control_generation: Arc<AtomicU64>,
    snapshot_owner: ControlPlaneSnapshotOwner,
    #[cfg(windows)]
    foreground_start_pending: Arc<Mutex<Option<ProductionRuntimeConfig>>>,
}

impl DesktopBackendHandle {
    #[cfg(windows)]
    pub fn start_production_runtime(
        &self,
        config: ProductionRuntimeConfig,
    ) -> Result<(), DesktopRuntimeStartError> {
        self.desired.set_services(ServiceIntent::Enabled);
        self.clear_staged_foreground_start();
        self.recovery_cancellation.cancel();
        let generation = self
            .runtime_control_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        let configured_workspace = config.workspace.clone();
        self.publish_starting_if_current(generation, configured_workspace.clone());
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = self.start_production_runtime_locked(config, generation);
        if let Err(DesktopRuntimeStartError::Runtime(error)) = &result {
            self.publish_fault_if_current(generation, configured_workspace, error.fault.clone());
        }
        result
    }

    #[cfg(windows)]
    fn start_production_runtime_locked(
        &self,
        config: ProductionRuntimeConfig,
        generation: u64,
    ) -> Result<(), DesktopRuntimeStartError> {
        if !self.is_current_generation(generation) {
            return Ok(());
        }
        if self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_active()
        {
            return Err(DesktopRuntimeStartError::AlreadyRegistered);
        }
        let snapshot_owner = self.snapshot_owner.clone();
        let wake: CurrentTaskWake = Arc::new(move || {
            snapshot_owner.mark_activity_stale();
        });
        let driver = ProductionRuntimeDriver::new_owned(
            config,
            WindowsCredentialStore::default(),
            generate_internal_bearer,
        )
        .with_privileged_execution(Arc::new(self.privilege.gateway()))
        .with_task_projection_wake(wake)
        .with_control_plane_state(
            self.desired.clone(),
            self.desired.snapshot().state.connection,
        );
        let mut runtime = RuntimeOrchestrator::new(driver);
        runtime.start().map_err(DesktopRuntimeStartError::Runtime)?;
        let runtime = AutoRecoveryRuntime::new_with_cancellation(
            runtime,
            SystemRecoveryClock::default(),
            self.recovery_cancellation.clone(),
        );
        self.activate_runtime_if_current_locked(Box::new(runtime), generation)
    }

    fn activate_runtime_if_current_locked(
        &self,
        mut runtime: Box<dyn ExitRuntime + Send>,
        generation: u64,
    ) -> Result<(), DesktopRuntimeStartError> {
        if !self.is_current_generation(generation) {
            let _ = runtime.stop_tunnel_for_exit();
            let _ = runtime.finish_exit_after_tunnel();
            return Ok(());
        }
        let mut owner = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        owner.activate_boxed(runtime)?;
        if !self.is_current_generation(generation) {
            let mut stale = owner.take_active();
            drop(owner);
            if let Some(runtime) = stale.as_deref_mut() {
                let _ = runtime.stop_tunnel_for_exit();
                let _ = runtime.finish_exit_after_tunnel();
            }
            return Ok(());
        }
        let snapshot = owner.snapshot();
        drop(owner);
        if self.is_current_generation(generation) {
            self.publish_observation(snapshot, None);
        }
        Ok(())
    }

    #[cfg(windows)]
    pub fn restart_production_runtime(
        &self,
        config: ProductionRuntimeConfig,
    ) -> Result<(), DesktopRuntimeControlError> {
        self.desired.set_services(ServiceIntent::Enabled);
        self.clear_staged_foreground_start();
        self.recovery_cancellation.cancel();
        let generation = self
            .runtime_control_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        let configured_workspace = config.workspace.clone();
        self.publish_starting_if_current(generation, configured_workspace.clone());
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.is_current_generation(generation) {
            return Ok(());
        }
        let mut active = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_active();
        if let Some(runtime) = active.as_deref_mut() {
            let tunnel = runtime.stop_tunnel_for_exit();
            let lower = runtime.finish_exit_after_tunnel();
            if tunnel.is_err() || lower.is_err() {
                self.publish_fault_if_current(
                    generation,
                    configured_workspace,
                    RuntimeFault::Unknown,
                );
                return Err(DesktopRuntimeControlError::Runtime(RuntimeFault::Unknown));
            }
        }
        if !self.is_current_generation(generation) {
            return Ok(());
        }
        match self.start_production_runtime_locked(config, generation) {
            Ok(()) => Ok(()),
            Err(DesktopRuntimeStartError::Runtime(error)) => {
                self.publish_fault_if_current(
                    generation,
                    configured_workspace,
                    error.fault.clone(),
                );
                Err(DesktopRuntimeControlError::Runtime(error.fault))
            }
            Err(DesktopRuntimeStartError::AlreadyRegistered) => {
                Err(DesktopRuntimeControlError::Runtime(RuntimeFault::Unknown))
            }
        }
    }

    #[cfg(windows)]
    pub fn spawn_start_production_runtime(
        &self,
        config: ProductionRuntimeConfig,
    ) -> std::io::Result<JoinHandle<()>> {
        self.desired.set_services(ServiceIntent::Enabled);
        self.clear_staged_foreground_start();
        self.recovery_cancellation.cancel();
        let backend = self.clone();
        let generation = self
            .runtime_control_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        let configured_workspace = config.workspace.clone();
        self.publish_starting_if_current(generation, configured_workspace.clone());
        let spawn = thread::Builder::new()
            .name("localbridge-desktop-start".into())
            .spawn(move || {
                let _operation = backend
                    .runtime_operation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                match backend.start_production_runtime_locked(config, generation) {
                    Ok(()) => {}
                    Err(DesktopRuntimeStartError::Runtime(error)) => {
                        backend.publish_fault_if_current(
                            generation,
                            configured_workspace,
                            error.fault,
                        );
                    }
                    Err(DesktopRuntimeStartError::AlreadyRegistered) => {
                        if backend.is_current_generation(generation) {
                            let snapshot = backend
                                .runtime
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .snapshot();
                            backend.publish_observation(snapshot, None);
                        }
                    }
                }
            });
        if spawn.is_err() && self.is_current_generation(generation) {
            let snapshot = DesktopRuntimeSnapshot::inactive();
            self.observe_inactive_and_publish(snapshot, Some(TaskAggregate::idle()));
        }
        spawn
    }

    fn is_current_generation(&self, generation: u64) -> bool {
        self.runtime_control_generation.load(Ordering::Acquire) == generation
    }

    #[cfg(windows)]
    fn clear_staged_foreground_start(&self) {
        self.foreground_start_pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }

    fn publish_starting_if_current(&self, generation: u64, workspace: PathBuf) {
        if !self.is_current_generation(generation) {
            return;
        }
        let snapshot = DesktopRuntimeSnapshot {
            active: false,
            state: RuntimeState::StartingMcp,
            current_task: CurrentTaskStatus::Idle,
            current_task_elapsed_ms: None,
            last_tool: None,
            configured_workspace: Some(workspace),
            connection_profile: self.desired.snapshot().state.connection,
            outage: None,
        };
        self.observe_inactive_and_publish(snapshot, Some(TaskAggregate::idle()));
    }

    fn publish_fault_if_current(&self, generation: u64, workspace: PathBuf, fault: RuntimeFault) {
        if !self.is_current_generation(generation) {
            return;
        }
        let snapshot = DesktopRuntimeSnapshot {
            active: false,
            state: RuntimeState::Faulted(fault),
            current_task: CurrentTaskStatus::Idle,
            current_task_elapsed_ms: None,
            last_tool: None,
            configured_workspace: Some(workspace),
            connection_profile: self.desired.snapshot().state.connection,
            outage: None,
        };
        self.observe_inactive_and_publish(snapshot, Some(TaskAggregate::idle()));
    }

    pub fn shutdown(&self) -> ShutdownReport {
        self.desired.set_services(ServiceIntent::Disabled);
        #[cfg(windows)]
        self.clear_staged_foreground_start();
        self.recovery_cancellation.cancel();
        self.runtime_control_generation
            .fetch_add(1, Ordering::AcqRel);
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut active = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_active();
        let report = shutdown_in_security_order(active.as_deref_mut(), &self.privilege);
        let snapshot = DesktopRuntimeSnapshot::inactive();
        self.observe_inactive_and_publish(snapshot, Some(TaskAggregate::idle()));
        report
    }

    fn observe_inactive_and_publish(
        &self,
        runtime: DesktopRuntimeSnapshot,
        activity: Option<TaskAggregate>,
    ) -> ControlPlaneSnapshot {
        self.runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observe_inactive(runtime.clone());
        self.publish_observation(runtime, activity)
    }

    fn publish_observation(
        &self,
        runtime: DesktopRuntimeSnapshot,
        activity: Option<TaskAggregate>,
    ) -> ControlPlaneSnapshot {
        publish_control_plane_observation(
            &self.snapshot_owner,
            &self.desired,
            &self.privilege,
            runtime,
            activity,
        )
    }

    pub fn spawn_shutdown_then(
        &self,
        after_shutdown: impl FnOnce(ShutdownReport) + Send + 'static,
    ) -> std::io::Result<JoinHandle<()>> {
        let backend = self.clone();
        thread::Builder::new()
            .name("localbridge-desktop-shutdown".into())
            .spawn(move || after_shutdown(backend.shutdown()))
    }
}

impl Drop for DesktopLifecycle {
    fn drop(&mut self) {
        if let Some(shutdown) = self
            .watchdog_shutdown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self
            .watchdog_thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = thread.join();
        }
    }
}

#[cfg(windows)]
fn generate_internal_bearer() -> Result<InternalBearer, RuntimeFault> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let nonce = random_session_nonce().map_err(|_| RuntimeFault::ConfigurationInvalid)?;
    let mut encoded = [0u8; SESSION_NONCE_BYTES * 2];
    for (index, byte) in nonce.as_bytes().iter().copied().enumerate() {
        encoded[index * 2] = HEX[(byte >> 4) as usize];
        encoded[index * 2 + 1] = HEX[(byte & 0x0f) as usize];
    }
    let result = std::str::from_utf8(&encoded)
        .map_err(|_| RuntimeFault::ConfigurationInvalid)
        .and_then(|value| {
            InternalBearer::new(value).map_err(|_| RuntimeFault::ConfigurationInvalid)
        });
    for byte in &mut encoded {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    result
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/background/background.rs"
    ));
}
