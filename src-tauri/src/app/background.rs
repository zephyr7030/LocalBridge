use std::ffi::OsStr;
use std::sync::{Arc, Mutex, TryLockError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(windows)]
use crate::credentials::WindowsCredentialStore;
#[cfg(windows)]
use crate::mcp::InternalBearer;
use crate::privilege::PrivilegeController;
#[cfg(windows)]
use crate::privilege::{SESSION_NONCE_BYTES, random_session_nonce};
use crate::runtime::{
    AutoRecoveryRuntime, OrchestratorError, RecoveryClock, RecoveryController, RecoveryOutcome,
    RuntimeDriver, RuntimeOrchestrator, RuntimeOutage, SystemRecoveryClock, WorkspaceSwitchError,
};
#[cfg(windows)]
use crate::runtime::{ProductionRuntimeConfig, ProductionRuntimeDriver};
use crate::state::{
    CurrentTaskStatus, PermissionMode, RuntimeComponent, RuntimeFault, RuntimeState,
};
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopOutageSnapshot {
    pub generation: u64,
    pub component: RuntimeComponent,
    pub fault: RuntimeFault,
    pub user_attention_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopRuntimeSnapshot {
    pub active: bool,
    pub state: RuntimeState,
    pub current_task: CurrentTaskStatus,
    pub configured_workspace: Option<PathBuf>,
    pub outage: Option<DesktopOutageSnapshot>,
}

impl DesktopRuntimeSnapshot {
    fn inactive() -> Self {
        Self {
            active: false,
            state: RuntimeState::Stopped,
            current_task: CurrentTaskStatus::Idle,
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

    fn activate<R>(&mut self, runtime: R) -> Result<(), DesktopRuntimeStartError>
    where
        R: ExitRuntime + Send + 'static,
    {
        if self.active.is_some() {
            return Err(DesktopRuntimeStartError::AlreadyRegistered);
        }
        self.active = Some(Box::new(runtime));
        Ok(())
    }

    fn take_active(&mut self) -> Option<Box<dyn ExitRuntime + Send>> {
        self.active.take()
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
        DesktopRuntimeSnapshot {
            active: true,
            state: self.state().clone(),
            current_task: self.current_task(),
            configured_workspace: self.configured_workspace().map(Path::to_path_buf),
            outage: self.active_outage().map(|outage| DesktopOutageSnapshot {
                generation: outage.id.get(),
                component: outage.component,
                fault: outage.fault.clone(),
                user_attention_required: outage.user_attention_emitted(),
            }),
        }
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
        DesktopRuntimeSnapshot {
            active: true,
            state: runtime.state().clone(),
            current_task: runtime.current_task(),
            configured_workspace: runtime.configured_workspace().map(Path::to_path_buf),
            outage: runtime.active_outage().map(|outage| DesktopOutageSnapshot {
                generation: outage.id.get(),
                component: outage.component,
                fault: outage.fault.clone(),
                user_attention_required: outage.user_attention_emitted(),
            }),
        }
    }

    fn set_permission_mode(
        &mut self,
        mode: PermissionMode,
    ) -> Result<(), DesktopRuntimeControlError> {
        self.orchestrator_mut()
            .set_permission_mode(mode)
            .map_err(DesktopRuntimeControlError::Runtime)
    }

    fn switch_workspace(
        &mut self,
        candidate: &Path,
        rollback: Option<&Path>,
    ) -> Result<(), DesktopRuntimeControlError> {
        self.orchestrator_mut()
            .switch_workspace_to(candidate, rollback)
            .map_err(DesktopRuntimeControlError::Workspace)
    }

    fn manual_retry(&mut self) -> Result<RecoveryOutcome, DesktopRuntimeControlError> {
        self.manual_retry_current_outage()
            .ok_or(DesktopRuntimeControlError::NoActiveOutage)
    }

    fn monitor_recovery(&mut self) -> Option<RecoveryOutcome> {
        self.monitor_once()
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
    watchdog_shutdown: Mutex<Option<mpsc::Sender<()>>>,
    watchdog_thread: Mutex<Option<JoinHandle<()>>>,
}

const RUNTIME_WATCHDOG_INTERVAL: Duration = Duration::from_millis(500);

impl std::fmt::Debug for DesktopLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopLifecycle")
            .field("privilege", &self.privilege)
            .field(
                "production_runtime_active",
                &self
                    .runtime
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_active(),
            )
            .finish()
    }
}

impl DesktopLifecycle {
    pub fn new(privilege: PrivilegeController) -> Self {
        let runtime_operation = Arc::new(Mutex::new(()));
        let runtime = Arc::new(Mutex::new(ProductionRuntimeOwner::default()));
        let monitor_operation = Arc::clone(&runtime_operation);
        let monitor_runtime = Arc::clone(&runtime);
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let watchdog_thread = thread::Builder::new()
            .name("localbridge-runtime-watchdog".into())
            .spawn(move || loop {
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
                if let Some(runtime) = owner.active.as_deref_mut() {
                    let _ = runtime.monitor_recovery();
                }
            })
            .expect("runtime watchdog thread must start");
        Self {
            privilege,
            runtime_operation,
            runtime,
            watchdog_shutdown: Mutex::new(Some(shutdown_tx)),
            watchdog_thread: Mutex::new(Some(watchdog_thread)),
        }
    }

    pub fn privilege(&self) -> &PrivilegeController {
        &self.privilege
    }

    #[cfg(windows)]
    pub fn start_production_runtime(
        &self,
        config: ProductionRuntimeConfig,
    ) -> Result<(), DesktopRuntimeStartError> {
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_active()
        {
            return Err(DesktopRuntimeStartError::AlreadyRegistered);
        }

        let driver = ProductionRuntimeDriver::new_owned(
            config,
            WindowsCredentialStore::default(),
            generate_internal_bearer,
        )
        .with_privileged_execution(Arc::new(self.privilege.gateway()));
        let mut runtime = RuntimeOrchestrator::new(driver);
        runtime
            .start()
            .map_err(DesktopRuntimeStartError::Runtime)?;
        let runtime = AutoRecoveryRuntime::new(runtime, SystemRecoveryClock::default());
        self.runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .activate(runtime)
    }

    pub fn shutdown(&self) -> ShutdownReport {
        self.shutdown_with_privilege(&self.privilege)
    }

    pub fn stop_services_for_manual_action(&self) -> ShutdownReport {
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut active = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_active();
        shutdown_in_security_order(active.as_deref_mut(), &self.privilege)
    }

    pub fn stop_runtime_for_control_plane(&self) -> Result<(), DesktopRuntimeControlError> {
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut active = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_active()
            .ok_or(DesktopRuntimeControlError::NoActiveRuntime)?;
        let tunnel = active.stop_tunnel_for_exit();
        let lower = active.finish_exit_after_tunnel();
        if tunnel.is_err() || lower.is_err() {
            return Err(DesktopRuntimeControlError::Runtime(RuntimeFault::Unknown));
        }
        Ok(())
    }

    pub fn runtime_snapshot(&self) -> DesktopRuntimeSnapshot {
        self.runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .as_deref()
            .map(ExitRuntime::runtime_snapshot)
            .unwrap_or_else(DesktopRuntimeSnapshot::inactive)
    }

    pub fn set_runtime_permission_mode(
        &self,
        mode: PermissionMode,
    ) -> Result<(), DesktopRuntimeControlError> {
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut owner = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        owner
            .active
            .as_deref_mut()
            .ok_or(DesktopRuntimeControlError::NoActiveRuntime)?
            .set_permission_mode(mode)
    }

    pub fn switch_runtime_workspace(
        &self,
        candidate: &Path,
        rollback: Option<&Path>,
    ) -> Result<(), DesktopRuntimeControlError> {
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut owner = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        owner
            .active
            .as_deref_mut()
            .ok_or(DesktopRuntimeControlError::NoActiveRuntime)?
            .switch_workspace(candidate, rollback)
    }

    pub fn manual_retry_after_attention(
        &self,
    ) -> Result<RecoveryOutcome, DesktopRuntimeControlError> {
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut owner = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        owner
            .active
            .as_deref_mut()
            .ok_or(DesktopRuntimeControlError::NoActiveRuntime)?
            .manual_retry()
    }

    fn shutdown_with_privilege<P>(&self, privilege: &P) -> ShutdownReport
    where
        P: PrivilegeExit + ?Sized,
    {
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut runtime = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shutdown_in_security_order(Some(&mut *runtime), privilege)
    }

    #[cfg(test)]
    fn install_runtime_for_test<R>(&self, runtime: R) -> Result<(), DesktopRuntimeStartError>
    where
        R: ExitRuntime + Send + 'static,
    {
        let _operation = self
            .runtime_operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .activate(runtime)
    }

    #[cfg(test)]
    fn shutdown_with_privilege_for_test<P>(&self, privilege: &P) -> ShutdownReport
    where
        P: PrivilegeExit + ?Sized,
    {
        self.shutdown_with_privilege(privilege)
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
