use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use crate::state::{GenerationId, PrivilegeFault, PrivilegeState};

use super::{
    AdministratorFilesystemErrorCode, AdministratorFilesystemResult, AdministratorFilesystemSpec,
    BrokerClientSession, BrokerRunError, ElevatedBrokerProcess, ElevatedExecResult,
    ElevatedExecSpec, NamedPipeServer, PrivilegeIpcError, PrivilegedFilesystemResult,
    PrivilegedFilesystemSpec, UacLaunchError, launch_broker_with_explicit_uac,
};

const BROKER_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const EXEC_POLL_INTERVAL: Duration = Duration::from_millis(25);

struct ActiveBroker {
    session: BrokerClientSession,
    process: ElevatedBrokerProcess,
}

struct PrivilegeShared {
    state: RwLock<PrivilegeState>,
    gate_open: AtomicBool,
    next_generation: AtomicU64,
    active: Mutex<Option<ActiveBroker>>,
}

impl Default for PrivilegeShared {
    fn default() -> Self {
        Self {
            state: RwLock::new(PrivilegeState::Disabled),
            gate_open: AtomicBool::new(false),
            next_generation: AtomicU64::new(1),
            active: Mutex::new(None),
        }
    }
}

impl PrivilegeShared {
    fn cached_state(&self) -> PrivilegeState {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn set_state(&self, state: PrivilegeState) {
        *self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
    }

    fn apply_broker_liveness(&self, running: bool) {
        if running || !self.gate_open.swap(false, Ordering::AcqRel) {
            return;
        }
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        self.set_state(PrivilegeState::Faulted(PrivilegeFault::BrokerExited));
    }

    fn refresh_broker_liveness(&self) -> PrivilegeState {
        if !self.gate_open.load(Ordering::Acquire) {
            return self.cached_state();
        }
        let running = {
            let active = self
                .active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            active
                .as_ref()
                .and_then(|active| active.process.is_running().ok())
                .unwrap_or(false)
        };
        self.apply_broker_liveness(running);
        self.cached_state()
    }
}

#[derive(Clone)]
pub struct PrivilegeController {
    shared: Arc<PrivilegeShared>,
}

impl fmt::Debug for PrivilegeController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivilegeController")
            .field("state", &self.state())
            .field(
                "call_gate_open",
                &self.shared.gate_open.load(Ordering::Acquire),
            )
            .finish()
    }
}

impl Default for PrivilegeController {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivilegeController {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(PrivilegeShared::default()),
        }
    }

    pub fn state(&self) -> PrivilegeState {
        self.shared.refresh_broker_liveness()
    }

    pub fn gateway(&self) -> PrivilegedExecutionGateway {
        PrivilegedExecutionGateway {
            shared: Arc::clone(&self.shared),
        }
    }

    pub fn request_without_uac(&self) -> Result<(), PrivilegeFault> {
        self.disable()?;
        self.set_state(PrivilegeState::Requested);
        Ok(())
    }

    pub fn enable_from_explicit_user_action(
        &self,
        broker_executable: &Path,
    ) -> Result<GenerationId, PrivilegeFault> {
        self.disable()?;
        let generation_value = self
            .shared
            .next_generation
            .fetch_add(1, Ordering::AcqRel)
            .max(1);
        let generation = GenerationId::new(generation_value);
        self.set_state(PrivilegeState::Requested);
        let server = NamedPipeServer::create().map_err(|error| self.fail(map_ipc_fault(error)))?;
        self.set_state(PrivilegeState::AwaitingUac);
        let process =
            launch_broker_with_explicit_uac(broker_executable, server.name(), generation_value)
                .map_err(|error| self.fail(map_uac_fault(error)))?;
        let connection = server
            .accept_elevated_client(&process)
            .map_err(|error| self.fail(map_ipc_fault(error)))?;
        let mut session = BrokerClientSession::handshake(connection, generation_value)
            .map_err(|error| self.fail(map_broker_fault(error)))?;
        session
            .ping()
            .map_err(|error| self.fail(map_broker_fault(error)))?;
        {
            let mut active = self
                .shared
                .active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *active = Some(ActiveBroker { session, process });
        }
        self.set_state(PrivilegeState::Active {
            broker_generation: generation,
        });
        self.shared.gate_open.store(true, Ordering::Release);
        Ok(generation)
    }

    pub fn disable(&self) -> Result<(), PrivilegeFault> {
        self.shared.gate_open.store(false, Ordering::Release);
        self.set_state(PrivilegeState::Disabled);
        let active = self
            .shared
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let Some(mut active) = active else {
            return Ok(());
        };
        let shutdown_result = active.session.shutdown();
        drop(active.session);
        let exited = active
            .process
            .wait_for_exit(BROKER_EXIT_TIMEOUT)
            .map_err(map_uac_fault)?;
        if !exited {
            active.process.terminate().map_err(map_uac_fault)?;
            if !active
                .process
                .wait_for_exit(BROKER_EXIT_TIMEOUT)
                .map_err(map_uac_fault)?
            {
                return Err(PrivilegeFault::BrokerExited);
            }
        }
        shutdown_result.map_err(map_broker_fault)
    }

    pub fn refresh_broker_state(&self) -> PrivilegeState {
        self.state()
    }

    fn fail(&self, fault: PrivilegeFault) -> PrivilegeFault {
        self.shared.gate_open.store(false, Ordering::Release);
        self.shared
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        self.set_state(PrivilegeState::Faulted(fault.clone()));
        fault
    }

    fn set_state(&self, state: PrivilegeState) {
        self.shared.set_state(state);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegedExecError {
    GateClosed(PrivilegeState),
    Broker(PrivilegeFault),
    Filesystem(AdministratorFilesystemErrorCode),
}

pub trait PrivilegedExecution: Send + Sync {
    fn state(&self) -> PrivilegeState;
    fn start_execute(
        &self,
        request_id: String,
        spec: ElevatedExecSpec,
    ) -> Result<(), PrivilegedExecError>;
    fn poll_execute(
        &self,
        request_id: String,
    ) -> Result<Option<ElevatedExecResult>, PrivilegedExecError>;
    fn cancel_execute(&self, request_id: String) -> Result<(), PrivilegedExecError>;
    fn filesystem(
        &self,
        spec: PrivilegedFilesystemSpec,
    ) -> Result<PrivilegedFilesystemResult, PrivilegedExecError>;
    fn structured_filesystem(
        &self,
        _spec: AdministratorFilesystemSpec,
    ) -> Result<AdministratorFilesystemResult, PrivilegedExecError> {
        Err(PrivilegedExecError::GateClosed(self.state()))
    }
    fn start_structured_filesystem(
        &self,
        _request_id: String,
        _spec: AdministratorFilesystemSpec,
    ) -> Result<(), PrivilegedExecError> {
        Err(PrivilegedExecError::GateClosed(self.state()))
    }
    fn poll_structured_filesystem(
        &self,
        _request_id: String,
    ) -> Result<
        Option<Result<AdministratorFilesystemResult, AdministratorFilesystemErrorCode>>,
        PrivilegedExecError,
    > {
        Err(PrivilegedExecError::GateClosed(self.state()))
    }
    fn cancel_structured_filesystem(&self, _request_id: String) -> Result<(), PrivilegedExecError> {
        Err(PrivilegedExecError::GateClosed(self.state()))
    }
}

#[derive(Clone)]
pub struct PrivilegedExecutionGateway {
    shared: Arc<PrivilegeShared>,
}

impl fmt::Debug for PrivilegedExecutionGateway {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivilegedExecutionGateway")
            .field("state", &self.state())
            .field("gate_open", &self.shared.gate_open.load(Ordering::Acquire))
            .finish()
    }
}

impl PrivilegedExecutionGateway {
    pub fn state(&self) -> PrivilegeState {
        self.shared.refresh_broker_liveness()
    }

    pub fn execute(
        &self,
        request_id: String,
        spec: ElevatedExecSpec,
    ) -> Result<ElevatedExecResult, PrivilegedExecError> {
        self.start_execute(request_id.clone(), spec)?;
        loop {
            if let Some(result) = self.poll_execute(request_id.clone())? {
                return Ok(result);
            }
            thread::sleep(EXEC_POLL_INTERVAL);
        }
    }

    pub fn cancel(&self, request_id: String) -> Result<(), PrivilegedExecError> {
        self.cancel_execute(request_id)
    }

    fn require_gate(&self) -> Result<(), PrivilegedExecError> {
        let state = self.state();
        if self.shared.gate_open.load(Ordering::Acquire) && state.accepts_privileged_calls() {
            Ok(())
        } else {
            Err(PrivilegedExecError::GateClosed(state))
        }
    }

    fn with_session<T>(
        &self,
        operation: impl FnOnce(&mut BrokerClientSession) -> Result<T, BrokerRunError>,
    ) -> Result<T, PrivilegedExecError> {
        let mut active = self
            .shared
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.shared.gate_open.load(Ordering::Acquire) {
            return Err(PrivilegedExecError::GateClosed(self.shared.cached_state()));
        }
        let Some(broker) = active.as_mut() else {
            let state = if self.shared.gate_open.swap(false, Ordering::AcqRel) {
                let state = PrivilegeState::Faulted(PrivilegeFault::BrokerExited);
                self.shared.set_state(state.clone());
                state
            } else {
                self.shared.cached_state()
            };
            return Err(PrivilegedExecError::GateClosed(state));
        };
        operation(&mut broker.session).map_err(|error| {
            let fault = map_broker_fault(error);
            self.shared.gate_open.store(false, Ordering::Release);
            self.shared
                .set_state(PrivilegeState::Faulted(fault.clone()));
            PrivilegedExecError::Broker(fault)
        })
    }
}

impl PrivilegedExecution for PrivilegedExecutionGateway {
    fn state(&self) -> PrivilegeState {
        PrivilegedExecutionGateway::state(self)
    }

    fn start_execute(
        &self,
        request_id: String,
        spec: ElevatedExecSpec,
    ) -> Result<(), PrivilegedExecError> {
        self.require_gate()?;
        self.with_session(|session| session.start_exec(request_id, spec))
    }

    fn poll_execute(
        &self,
        request_id: String,
    ) -> Result<Option<ElevatedExecResult>, PrivilegedExecError> {
        self.require_gate()?;
        self.with_session(|session| session.poll_exec(request_id))
    }

    fn cancel_execute(&self, request_id: String) -> Result<(), PrivilegedExecError> {
        self.require_gate()?;
        self.with_session(|session| session.cancel_exec(request_id))
    }

    fn filesystem(
        &self,
        spec: PrivilegedFilesystemSpec,
    ) -> Result<PrivilegedFilesystemResult, PrivilegedExecError> {
        self.require_gate()?;
        self.with_session(|session| session.filesystem(spec))
    }

    fn structured_filesystem(
        &self,
        spec: AdministratorFilesystemSpec,
    ) -> Result<AdministratorFilesystemResult, PrivilegedExecError> {
        self.require_gate()?;
        self.with_session(|session| session.structured_filesystem(spec))?
            .map_err(PrivilegedExecError::Filesystem)
    }

    fn start_structured_filesystem(
        &self,
        request_id: String,
        spec: AdministratorFilesystemSpec,
    ) -> Result<(), PrivilegedExecError> {
        self.require_gate()?;
        self.with_session(|session| session.start_structured_filesystem(request_id, spec))
    }

    fn poll_structured_filesystem(
        &self,
        request_id: String,
    ) -> Result<
        Option<Result<AdministratorFilesystemResult, AdministratorFilesystemErrorCode>>,
        PrivilegedExecError,
    > {
        self.require_gate()?;
        self.with_session(|session| session.poll_structured_filesystem(request_id))
    }

    fn cancel_structured_filesystem(&self, request_id: String) -> Result<(), PrivilegedExecError> {
        self.require_gate()?;
        self.with_session(|session| session.cancel_structured_filesystem(request_id))
    }
}

fn map_uac_fault(error: UacLaunchError) -> PrivilegeFault {
    match error {
        UacLaunchError::UacDenied => PrivilegeFault::UacDenied,
        UacLaunchError::InvalidBrokerExecutable
        | UacLaunchError::InvalidLaunchContext
        | UacLaunchError::LaunchFailed(_) => PrivilegeFault::BrokerLaunchFailed,
    }
}

fn map_ipc_fault(error: PrivilegeIpcError) -> PrivilegeFault {
    match error {
        PrivilegeIpcError::UnauthorizedPeer { .. } => PrivilegeFault::UnauthorizedPeer,
        _ => PrivilegeFault::IpcUnavailable,
    }
}

fn map_broker_fault(error: BrokerRunError) -> PrivilegeFault {
    match error {
        BrokerRunError::HandshakeMismatch => PrivilegeFault::HandshakeFailed,
        BrokerRunError::Protocol(BrokerProtocolError::ProtocolMismatch) => {
            PrivilegeFault::ProtocolMismatch
        }
        BrokerRunError::Ipc(PrivilegeIpcError::UnauthorizedPeer { .. }) => {
            PrivilegeFault::UnauthorizedPeer
        }
        BrokerRunError::Ipc(_) => PrivilegeFault::IpcUnavailable,
        BrokerRunError::Protocol(_)
        | BrokerRunError::InvalidArguments
        | BrokerRunError::UnexpectedResponse => PrivilegeFault::ProtocolMismatch,
    }
}

use super::BrokerProtocolError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_accepts_calls_only_when_active_and_closes_before_disabled_state_is_observed() {
        let controller = PrivilegeController::new();
        assert!(matches!(
            controller.gateway().execute(
                "x".into(),
                ElevatedExecSpec {
                    program: r"C:\Windows\System32\cmd.exe".into(),
                    args: vec![],
                    workdir: None,
                    timeout_ms: 1,
                    max_output_bytes: 1
                }
            ),
            Err(PrivilegedExecError::GateClosed(PrivilegeState::Disabled))
        ));
        controller.shared.gate_open.store(true, Ordering::Release);
        controller.set_state(PrivilegeState::Active {
            broker_generation: GenerationId::new(1),
        });
        controller.shared.gate_open.store(false, Ordering::Release);
        controller.set_state(PrivilegeState::Disabled);
        assert!(!controller.shared.gate_open.load(Ordering::Acquire));
        assert_eq!(controller.state(), PrivilegeState::Disabled);
    }

    #[test]
    fn broker_crash_immediately_closes_gate_and_leaves_active_state() {
        let controller = PrivilegeController::new();
        controller.shared.gate_open.store(true, Ordering::Release);
        controller.set_state(PrivilegeState::Active {
            broker_generation: GenerationId::new(9),
        });
        controller.shared.apply_broker_liveness(false);
        assert!(!controller.shared.gate_open.load(Ordering::Acquire));
        assert_eq!(
            controller.state(),
            PrivilegeState::Faulted(PrivilegeFault::BrokerExited)
        );
    }

    #[test]
    fn gateway_state_refreshes_stale_active_without_ui_or_diagnostics_poll() {
        let controller = PrivilegeController::new();
        controller.shared.gate_open.store(true, Ordering::Release);
        controller.set_state(PrivilegeState::Active {
            broker_generation: GenerationId::new(10),
        });

        assert_eq!(
            controller.gateway().state(),
            PrivilegeState::Faulted(PrivilegeFault::BrokerExited)
        );
        assert!(!controller.shared.gate_open.load(Ordering::Acquire));
        assert_eq!(
            controller.state(),
            PrivilegeState::Faulted(PrivilegeFault::BrokerExited)
        );
    }

    #[test]
    fn background_request_sets_requested_without_opening_privileged_gate() {
        let controller = PrivilegeController::new();
        controller.request_without_uac().unwrap();
        assert_eq!(controller.state(), PrivilegeState::Requested);
        assert!(matches!(
            controller.gateway().execute(
                "background-request".into(),
                ElevatedExecSpec {
                    program: r"C:\Windows\System32\cmd.exe".into(),
                    args: vec![],
                    workdir: None,
                    timeout_ms: 1,
                    max_output_bytes: 1,
                }
            ),
            Err(PrivilegedExecError::GateClosed(PrivilegeState::Requested))
        ));
    }

    #[test]
    fn uac_and_ipc_errors_map_to_typed_privilege_faults() {
        assert_eq!(
            map_uac_fault(UacLaunchError::UacDenied),
            PrivilegeFault::UacDenied
        );
        assert_eq!(
            map_uac_fault(UacLaunchError::LaunchFailed(5)),
            PrivilegeFault::BrokerLaunchFailed
        );
        assert_eq!(
            map_ipc_fault(PrivilegeIpcError::UnauthorizedPeer {
                expected_pid: 1,
                actual_pid: 2
            }),
            PrivilegeFault::UnauthorizedPeer
        );
    }
}
