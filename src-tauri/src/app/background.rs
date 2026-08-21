use serde_json::{Value, json};
use std::ffi::OsStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock, TryLockError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(windows)]
use crate::credentials::WindowsCredentialStore;
use crate::diagnostics::{
    DiagnosticsOutageInput, record_recovery_attempt_event, record_runtime_user_events,
};
#[cfg(windows)]
use crate::mcp::{CurrentTaskWake, InternalBearer};
use crate::privilege::PrivilegeController;
#[cfg(windows)]
use crate::privilege::{SESSION_NONCE_BYTES, random_session_nonce};
use crate::runtime::{
    AutoRecoveryRuntime, OrchestratorError, RecoveryCancellation, RecoveryClock,
    RecoveryAttemptEvent, RecoveryController, RecoveryOutcome, RuntimeDriver, RuntimeOrchestrator,
    RuntimeOutage, SystemRecoveryClock, WorkspaceSwitchError,
};
#[cfg(windows)]
use crate::runtime::{ProductionRuntimeConfig, ProductionRuntimeDriver};
use crate::state::{
    CurrentTaskStatus, LastToolTiming, PermissionMode, RuntimeComponent, RuntimeFault, RuntimeState,
};
use crate::tunnel::ConnectorEndpoint;
use std::path::{Path, PathBuf};

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

    fn task_aggregate_snapshot(&self) -> Value {
        json!({"state":"idle","current_workflow":null,"current_command":null,"last_command":null})
    }

    fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        None
    }

    fn set_permission_mode(
        &mut self,
        _mode: PermissionMode,
    ) -> Result<(), DesktopRuntimeControlError> {
        Err(DesktopRuntimeControlError::NoActiveRuntime)
    }

    fn switch_workspace(
        &mut self,
        _candidate: &Path,
        _rollback: Option<&Path>,
    ) -> Result<(), DesktopRuntimeControlError> {
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

#[derive(Default)]
struct ProductionRuntimeOwner {
    active: Option<Box<dyn ExitRuntime + Send>>,
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

    fn snapshot(&self) -> DesktopRuntimeSnapshot {
        self.active
            .as_deref()
            .map(ExitRuntime::runtime_snapshot)
            .unwrap_or_else(DesktopRuntimeSnapshot::inactive)
    }

    fn task_aggregate_snapshot(&self) -> Value {
        self.active
            .as_deref()
            .map(ExitRuntime::task_aggregate_snapshot)
            .unwrap_or_else(|| json!({"state":"idle","current_workflow":null,"current_command":null,"last_command":null}))
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
            outage: self.active_outage().map(|outage| DesktopOutageSnapshot {
                generation: outage.id.get(),
                request_id: outage.request_id.clone(),
                component: outage.component,
                fault: outage.fault.clone(),
                user_attention_required: outage.user_attention_emitted(),
            }),
        }
    }

    fn task_aggregate_snapshot(&self) -> Value { self.task_aggregate() }

    fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        RuntimeOrchestrator::connector_endpoint(self)
    }

    fn set_permission_mode(
        &mut self,
        mode: PermissionMode,
    ) -> Result<(), DesktopRuntimeControlError> {
        RuntimeOrchestrator::set_permission_mode(self, mode)
            .map_err(DesktopRuntimeControlError::Runtime)
    }

    fn switch_workspace(
        &mut self,
        candidate: &Path,
        rollback: Option<&Path>,
    ) -> Result<(), DesktopRuntimeControlError> {
        RuntimeOrchestrator::switch_workspace_to(self, candidate, rollback)
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
            outage: runtime.active_outage().map(|outage| DesktopOutageSnapshot {
                generation: outage.id.get(),
                request_id: outage.request_id.clone(),
                component: outage.component,
                fault: outage.fault.clone(),
                user_attention_required: outage.user_attention_emitted(),
            }),
        }
    }

    fn task_aggregate_snapshot(&self) -> Value { self.runtime().task_aggregate() }

    fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        self.runtime().connector_endpoint()
    }

    fn set_permission_mode(
        &mut self,
        mode: PermissionMode,
    ) -> Result<(), DesktopRuntimeControlError> {
        self.set_permission_mode_after_control_cancellation(mode)
            .map_err(DesktopRuntimeControlError::Runtime)
    }

    fn switch_workspace(
        &mut self,
        candidate: &Path,
        rollback: Option<&Path>,
    ) -> Result<(), DesktopRuntimeControlError> {
        self.switch_workspace_after_control_cancellation(candidate, rollback)
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
    runtime_operation: Arc<Mutex<()>>,
    runtime: Arc<Mutex<ProductionRuntimeOwner>>,
    recovery_cancellation: RecoveryCancellation,
    runtime_snapshot_cache: Arc<RwLock<DesktopRuntimeSnapshot>>,
    runtime_control_generation: Arc<AtomicU64>,
    projection_wake: ProjectionWake,
    #[cfg(windows)]
    foreground_start_pending: Arc<Mutex<Option<ProductionRuntimeConfig>>>,
    close_window_continue_running: Arc<AtomicBool>,
    watchdog_shutdown: Mutex<Option<mpsc::Sender<()>>>,
    watchdog_thread: Mutex<Option<JoinHandle<()>>>,
}

const RUNTIME_WATCHDOG_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Default)]
struct ProjectionWake(Arc<(Mutex<u64>, Condvar)>);

impl ProjectionWake {
    fn revision(&self) -> u64 {
        *self
            .0
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn capture_revision<T>(&self, mut capture: impl FnMut() -> T) -> (T, u64) {
        loop {
            let before = self.revision();
            let value = capture();
            let after = self.revision();
            if before == after {
                return (value, after);
            }
        }
    }

    fn notify(&self) {
        let (revision, wake) = &*self.0;
        let mut revision = revision
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *revision = revision.saturating_add(1);
        wake.notify_all();
    }

    fn wait_after(&self, since: u64, timeout: Duration) -> u64 {
        let (revision, wake) = &*self.0;
        let revision = revision
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *revision != since {
            return *revision;
        }
        let (revision, _) = wake
            .wait_timeout_while(revision, timeout, |value| *value == since)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *revision
    }
}

fn projection_change_requires_wake(
    previous: &DesktopRuntimeSnapshot,
    next: &DesktopRuntimeSnapshot,
) -> bool {
    previous.active != next.active
        || previous.state != next.state
        || previous.current_task != next.current_task
        || previous.configured_workspace != next.configured_workspace
        || previous.outage != next.outage
        || previous
            .last_tool
            .as_ref()
            .map(|tool| (&tool.kind, &tool.summary))
            != next
                .last_tool
                .as_ref()
                .map(|tool| (&tool.kind, &tool.summary))
}

fn next_projection_display_wait(snapshot: &DesktopRuntimeSnapshot) -> Duration {
    if matches!(snapshot.current_task, CurrentTaskStatus::Active(_)) {
        return Duration::from_millis(500);
    }
    let Some(last) = snapshot.last_tool.as_ref() else {
        return Duration::from_secs(30);
    };
    let age = last.age_ms;
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
        let runtime_operation = Arc::new(Mutex::new(()));
        let runtime = Arc::new(Mutex::new(ProductionRuntimeOwner::default()));
        let recovery_cancellation = RecoveryCancellation::default();
        let runtime_snapshot_cache = Arc::new(RwLock::new(DesktopRuntimeSnapshot::inactive()));
        let runtime_control_generation = Arc::new(AtomicU64::new(0));
        let projection_wake = ProjectionWake::default();
        #[cfg(windows)]
        let foreground_start_pending = Arc::new(Mutex::new(None));
        let close_window_continue_running = Arc::new(AtomicBool::new(true));
        let monitor_operation = Arc::clone(&runtime_operation);
        let monitor_runtime = Arc::clone(&runtime);
        let monitor_snapshot = Arc::clone(&runtime_snapshot_cache);
        let monitor_wake = projection_wake.clone();
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
                    drop(owner);
                    record_desktop_runtime_events(&snapshot, &monitor_privilege);
                    let mut cached = monitor_snapshot
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let notify = projection_change_requires_wake(&cached, &snapshot);
                    *cached = snapshot;
                    drop(cached);
                    if notify {
                        monitor_wake.notify();
                    }
                }
            })
            .expect("runtime watchdog thread must start");
        Self {
            privilege,
            runtime_operation,
            runtime,
            recovery_cancellation,
            runtime_snapshot_cache,
            runtime_control_generation,
            projection_wake,
            #[cfg(windows)]
            foreground_start_pending,
            close_window_continue_running,
            watchdog_shutdown: Mutex::new(Some(shutdown_tx)),
            watchdog_thread: Mutex::new(Some(watchdog_thread)),
        }
    }

    pub fn privilege(&self) -> &PrivilegeController {
        &self.privilege
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
            runtime_operation: Arc::clone(&self.runtime_operation),
            runtime: Arc::clone(&self.runtime),
            recovery_cancellation: self.recovery_cancellation.clone(),
            runtime_snapshot_cache: Arc::clone(&self.runtime_snapshot_cache),
            runtime_control_generation: Arc::clone(&self.runtime_control_generation),
            projection_wake: self.projection_wake.clone(),
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
        self.write_snapshot_cache(DesktopRuntimeSnapshot::inactive());
        report
    }

    pub fn stop_runtime_for_control_plane(&self) -> Result<(), DesktopRuntimeControlError> {
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
        self.write_snapshot_cache(DesktopRuntimeSnapshot::inactive());
        if tunnel.is_err() || lower.is_err() {
            return Err(DesktopRuntimeControlError::Runtime(RuntimeFault::Unknown));
        }
        Ok(())
    }

    pub fn task_aggregate_snapshot(&self) -> Value {
        match self.runtime.try_lock() {
            Ok(owner) => owner.task_aggregate_snapshot(),
            Err(TryLockError::Poisoned(error)) => error.into_inner().task_aggregate_snapshot(),
            Err(TryLockError::WouldBlock) => json!({"state":"active","current_workflow":{"state":"running"},"current_command":null,"last_command":null}),
        }
    }

    pub fn runtime_snapshot(&self) -> DesktopRuntimeSnapshot {
        match self.runtime.try_lock() {
            Ok(owner) => {
                if !owner.is_active() {
                    drop(owner);
                    return self
                        .runtime_snapshot_cache
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                }
                let snapshot = owner.snapshot();
                *self
                    .runtime_snapshot_cache
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot.clone();
                return snapshot;
            }
            Err(TryLockError::Poisoned(error)) => {
                let owner = error.into_inner();
                if !owner.is_active() {
                    drop(owner);
                    return self
                        .runtime_snapshot_cache
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                }
                let snapshot = owner.snapshot();
                *self
                    .runtime_snapshot_cache
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot.clone();
                return snapshot;
            }
            Err(TryLockError::WouldBlock) => {}
        }
        self.runtime_snapshot_cache
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn projection_revision(&self) -> u64 {
        self.projection_wake.revision()
    }

    pub fn runtime_snapshot_with_revision(&self) -> (DesktopRuntimeSnapshot, u64) {
        self.projection_wake
            .capture_revision(|| self.runtime_snapshot())
    }

    pub fn wait_projection_change_after(&self, since: u64) -> u64 {
        let timeout = next_projection_display_wait(&self.runtime_snapshot());
        self.projection_wake.wait_after(since, timeout)
    }

    pub fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        self.runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .connector_endpoint()
    }

    pub fn set_runtime_permission_mode(
        &self,
        mode: PermissionMode,
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
            .set_permission_mode(mode);
        let snapshot = owner.snapshot();
        drop(owner);
        self.write_snapshot_cache(snapshot);
        result
    }

    pub fn switch_runtime_workspace(
        &self,
        candidate: &Path,
        rollback: Option<&Path>,
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
            .switch_workspace(candidate, rollback);
        let snapshot = owner.snapshot();
        drop(owner);
        self.write_snapshot_cache(snapshot);
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
        self.write_snapshot_cache(snapshot);
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
        let mut runtime = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let report = shutdown_in_security_order(Some(&mut *runtime), privilege);
        self.write_snapshot_cache(DesktopRuntimeSnapshot::inactive());
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
        self.write_snapshot_cache(owner.snapshot());
        Ok(())
    }

    #[cfg(test)]
    fn shutdown_with_privilege_for_test<P>(&self, privilege: &P) -> ShutdownReport
    where
        P: PrivilegeExit + ?Sized,
    {
        self.shutdown_with_privilege(privilege)
    }

    fn write_snapshot_cache(&self, snapshot: DesktopRuntimeSnapshot) {
        record_desktop_runtime_events(&snapshot, &self.privilege);
        let mut cached = self
            .runtime_snapshot_cache
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let notify = projection_change_requires_wake(&cached, &snapshot);
        *cached = snapshot;
        drop(cached);
        if notify {
            self.projection_wake.notify();
        }
    }
}

fn record_desktop_runtime_events(snapshot: &DesktopRuntimeSnapshot, privilege: &PrivilegeController) {
    let outage = snapshot.outage.as_ref().map(|outage| DiagnosticsOutageInput {
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
    runtime_operation: Arc<Mutex<()>>,
    runtime: Arc<Mutex<ProductionRuntimeOwner>>,
    recovery_cancellation: RecoveryCancellation,
    runtime_snapshot_cache: Arc<RwLock<DesktopRuntimeSnapshot>>,
    runtime_control_generation: Arc<AtomicU64>,
    projection_wake: ProjectionWake,
    #[cfg(windows)]
    foreground_start_pending: Arc<Mutex<Option<ProductionRuntimeConfig>>>,
}

impl DesktopBackendHandle {
    #[cfg(windows)]
    pub fn start_production_runtime(
        &self,
        config: ProductionRuntimeConfig,
    ) -> Result<(), DesktopRuntimeStartError> {
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
        let projection_wake = self.projection_wake.clone();
        let wake: CurrentTaskWake = Arc::new(move || projection_wake.notify());
        let driver = ProductionRuntimeDriver::new_owned(
            config,
            WindowsCredentialStore::default(),
            generate_internal_bearer,
        )
        .with_privileged_execution(Arc::new(self.privilege.gateway()))
        .with_task_projection_wake(wake);
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
            *self
                .runtime_snapshot_cache
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot;
            self.projection_wake.notify();
        }
        Ok(())
    }

    #[cfg(windows)]
    pub fn restart_production_runtime(
        &self,
        config: ProductionRuntimeConfig,
    ) -> Result<(), DesktopRuntimeControlError> {
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
                            *backend
                                .runtime_snapshot_cache
                                .write()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot;
                            backend.projection_wake.notify();
                        }
                    }
                }
            });
        if spawn.is_err() && self.is_current_generation(generation) {
            *self
                .runtime_snapshot_cache
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                DesktopRuntimeSnapshot::inactive();
            self.projection_wake.notify();
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
        *self
            .runtime_snapshot_cache
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = DesktopRuntimeSnapshot {
            active: false,
            state: RuntimeState::StartingMcp,
            current_task: CurrentTaskStatus::Idle,
            current_task_elapsed_ms: None,
            last_tool: None,
            configured_workspace: Some(workspace),
            outage: None,
        };
        self.projection_wake.notify();
    }

    fn publish_fault_if_current(&self, generation: u64, workspace: PathBuf, fault: RuntimeFault) {
        if !self.is_current_generation(generation) {
            return;
        }
        *self
            .runtime_snapshot_cache
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = DesktopRuntimeSnapshot {
            active: false,
            state: RuntimeState::Faulted(fault),
            current_task: CurrentTaskStatus::Idle,
            current_task_elapsed_ms: None,
            last_tool: None,
            configured_workspace: Some(workspace),
            outage: None,
        };
        self.projection_wake.notify();
    }

    pub fn shutdown(&self) -> ShutdownReport {
        #[cfg(windows)]
        self.clear_staged_foreground_start();
        self.recovery_cancellation.cancel();
        self.runtime_control_generation
            .fetch_add(1, Ordering::AcqRel);
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut runtime = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let report = shutdown_in_security_order(Some(&mut *runtime), &self.privilege);
        *self
            .runtime_snapshot_cache
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            DesktopRuntimeSnapshot::inactive();
        self.projection_wake.notify();
        report
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
