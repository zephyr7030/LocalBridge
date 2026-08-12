use std::ffi::OsStr;
use std::sync::Mutex;

use crate::privilege::PrivilegeController;
use crate::runtime::{RecoveryOutcome, RuntimeDriver, RuntimeOrchestrator};

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

pub trait ExitRuntime {
    fn stop_tunnel_for_exit(&mut self) -> Result<(), DesktopExitError>;
    fn finish_exit_after_tunnel(&mut self) -> Result<(), DesktopExitError>;
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
    runtime: Mutex<Option<Box<dyn ExitRuntime + Send>>>,
}

impl std::fmt::Debug for DesktopLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopLifecycle")
            .field("privilege", &self.privilege)
            .field(
                "runtime_registered",
                &self
                    .runtime
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_some(),
            )
            .finish()
    }
}

impl DesktopLifecycle {
    pub fn new(privilege: PrivilegeController) -> Self {
        Self {
            privilege,
            runtime: Mutex::new(None),
        }
    }

    pub fn privilege(&self) -> &PrivilegeController {
        &self.privilege
    }

    pub fn register_runtime<R>(&self, runtime: R)
    where
        R: ExitRuntime + Send + 'static,
    {
        *self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(runtime));
    }

    pub fn shutdown(&self) -> ShutdownReport {
        let mut runtime = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shutdown_in_security_order(runtime.as_deref_mut(), &self.privilege)
    }
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/background/background.rs"
    ));
}
