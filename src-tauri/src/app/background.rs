use std::ffi::OsStr;
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use crate::credentials::WindowsCredentialStore;
#[cfg(windows)]
use crate::mcp::InternalBearer;
use crate::privilege::PrivilegeController;
#[cfg(windows)]
use crate::privilege::{SESSION_NONCE_BYTES, random_session_nonce};
use crate::runtime::{OrchestratorError, RecoveryOutcome, RuntimeDriver, RuntimeOrchestrator};
#[cfg(windows)]
use crate::runtime::{ProductionRuntimeConfig, ProductionRuntimeDriver};
use crate::state::RuntimeFault;

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
    runtime_operation: Mutex<()>,
    runtime: Mutex<ProductionRuntimeOwner>,
}

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
        Self {
            privilege,
            runtime_operation: Mutex::new(()),
            runtime: Mutex::new(ProductionRuntimeOwner::default()),
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
        self.runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .activate(runtime)
    }

    pub fn shutdown(&self) -> ShutdownReport {
        self.shutdown_with_privilege(&self.privilege)
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
