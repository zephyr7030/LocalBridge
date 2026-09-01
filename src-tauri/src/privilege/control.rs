use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::state::{GenerationId, PrivilegeFault, PrivilegeState};

use super::{
    AdministratorFilesystemErrorCode, AdministratorFilesystemResult, AdministratorFilesystemSpec,
    BrokerClientSession, BrokerRunError, ElevatedBrokerProcess, ElevatedExecResult,
    ElevatedExecSpec, NamedPipeServer, PrivilegeIpcError, PrivilegedFilesystemResult,
    PrivilegedFilesystemSpec, UacLaunchError, current_process_is_elevated,
    launch_broker_with_explicit_uac,
};

const BROKER_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const EXEC_POLL_INTERVAL: Duration = Duration::from_millis(25);

struct ActiveBroker {
    session: BrokerClientSession,
    process: ElevatedBrokerProcess,
}

enum BrokerLifecycle {
    Disabled,
    Requested,
    AwaitingUac,
    Active {
        generation: GenerationId,
        broker: ActiveBroker,
    },
    Faulted(PrivilegeFault),
}

impl BrokerLifecycle {
    fn state(&self) -> PrivilegeState {
        match self {
            Self::Disabled => PrivilegeState::Disabled,
            Self::Requested => PrivilegeState::Requested,
            Self::AwaitingUac => PrivilegeState::AwaitingUac,
            Self::Active { generation, .. } => PrivilegeState::Active {
                broker_generation: *generation,
            },
            Self::Faulted(fault) => PrivilegeState::Faulted(fault.clone()),
        }
    }
}

struct PrivilegeShared {
    lifecycle: Mutex<BrokerLifecycle>,
    next_generation: AtomicU64,
}

impl Default for PrivilegeShared {
    fn default() -> Self {
        Self {
            lifecycle: Mutex::new(BrokerLifecycle::Disabled),
            next_generation: AtomicU64::new(1),
        }
    }
}

impl PrivilegeShared {
    fn refresh_broker_liveness(&self) -> PrivilegeState {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let broker_exited = match &*lifecycle {
            BrokerLifecycle::Active { broker, .. } => !broker.process.is_running().unwrap_or(false),
            _ => false,
        };
        if broker_exited {
            *lifecycle = BrokerLifecycle::Faulted(PrivilegeFault::BrokerExited);
        }
        lifecycle.state()
    }

    fn replace(&self, next: BrokerLifecycle) -> BrokerLifecycle {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::replace(&mut *lifecycle, next)
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

    pub fn enable_from_explicit_user_action(
        &self,
        broker_executable: &Path,
    ) -> Result<GenerationId, PrivilegeFault> {
        self.enable_broker(broker_executable)
    }

    pub fn enable_from_elevated_startup(
        &self,
        broker_executable: &Path,
    ) -> Result<Option<GenerationId>, PrivilegeFault> {
        let elevated = current_process_is_elevated()
            .map_err(map_uac_fault)
            .map_err(|fault| self.fail(fault))?;
        if !elevated {
            return Ok(None);
        }
        // The process token has already passed Windows authorization. Reusing the
        // normal broker launch keeps execution behind the same gateway without a
        // second application-level consent owner.
        self.enable_broker(broker_executable).map(Some)
    }

    fn enable_broker(&self, broker_executable: &Path) -> Result<GenerationId, PrivilegeFault> {
        self.disable()?;
        let generation_value = self
            .shared
            .next_generation
            .fetch_add(1, Ordering::AcqRel)
            .max(1);
        let generation = GenerationId::new(generation_value);
        self.shared.replace(BrokerLifecycle::Requested);
        let server = NamedPipeServer::create().map_err(|error| self.fail(map_ipc_fault(error)))?;
        self.shared.replace(BrokerLifecycle::AwaitingUac);
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
        self.shared.replace(BrokerLifecycle::Active {
            generation,
            broker: ActiveBroker { session, process },
        });
        Ok(generation)
    }

    pub fn disable(&self) -> Result<(), PrivilegeFault> {
        let previous = self.shared.replace(BrokerLifecycle::Disabled);
        let BrokerLifecycle::Active {
            broker: mut active, ..
        } = previous
        else {
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
        self.shared.replace(BrokerLifecycle::Faulted(fault.clone()));
        fault
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

    fn with_session<T>(
        &self,
        operation: impl FnOnce(&mut BrokerClientSession) -> Result<T, BrokerRunError>,
    ) -> Result<T, PrivilegedExecError> {
        let mut lifecycle = self
            .shared
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = lifecycle.state();
        let running = match &*lifecycle {
            BrokerLifecycle::Active { broker, .. } => broker.process.is_running().unwrap_or(false),
            _ => return Err(PrivilegedExecError::GateClosed(state)),
        };
        if !running {
            let state = PrivilegeState::Faulted(PrivilegeFault::BrokerExited);
            *lifecycle = BrokerLifecycle::Faulted(PrivilegeFault::BrokerExited);
            return Err(PrivilegedExecError::GateClosed(state));
        }
        let result = match &mut *lifecycle {
            BrokerLifecycle::Active { broker, .. } => operation(&mut broker.session),
            _ => unreachable!("active broker lifecycle changed while locked"),
        };
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                let fault = map_broker_fault(error);
                *lifecycle = BrokerLifecycle::Faulted(fault.clone());
                Err(PrivilegedExecError::Broker(fault))
            }
        }
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
        self.with_session(|session| session.start_exec(request_id, spec))
    }

    fn poll_execute(
        &self,
        request_id: String,
    ) -> Result<Option<ElevatedExecResult>, PrivilegedExecError> {
        self.with_session(|session| session.poll_exec(request_id))
    }

    fn cancel_execute(&self, request_id: String) -> Result<(), PrivilegedExecError> {
        self.with_session(|session| session.cancel_exec(request_id))
    }

    fn filesystem(
        &self,
        spec: PrivilegedFilesystemSpec,
    ) -> Result<PrivilegedFilesystemResult, PrivilegedExecError> {
        self.with_session(|session| session.filesystem(spec))
    }

    fn structured_filesystem(
        &self,
        spec: AdministratorFilesystemSpec,
    ) -> Result<AdministratorFilesystemResult, PrivilegedExecError> {
        self.with_session(|session| session.structured_filesystem(spec))?
            .map_err(PrivilegedExecError::Filesystem)
    }

    fn start_structured_filesystem(
        &self,
        request_id: String,
        spec: AdministratorFilesystemSpec,
    ) -> Result<(), PrivilegedExecError> {
        self.with_session(|session| session.start_structured_filesystem(request_id, spec))
    }

    fn poll_structured_filesystem(
        &self,
        request_id: String,
    ) -> Result<
        Option<Result<AdministratorFilesystemResult, AdministratorFilesystemErrorCode>>,
        PrivilegedExecError,
    > {
        self.with_session(|session| session.poll_structured_filesystem(request_id))
    }

    fn cancel_structured_filesystem(&self, request_id: String) -> Result<(), PrivilegedExecError> {
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
    fn gateway_rejects_calls_without_an_active_broker_lifecycle() {
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
        controller.shared.replace(BrokerLifecycle::Requested);
        assert_eq!(
            controller
                .gateway()
                .with_session(|_| Ok::<_, BrokerRunError>(())),
            Err(PrivilegedExecError::GateClosed(PrivilegeState::Requested))
        );
    }

    #[test]
    fn fault_transition_replaces_the_previous_lifecycle_atomically() {
        let controller = PrivilegeController::new();
        controller.fail(PrivilegeFault::BrokerExited);
        assert_eq!(
            controller.gateway().state(),
            PrivilegeState::Faulted(PrivilegeFault::BrokerExited)
        );
    }

    #[test]
    fn requested_lifecycle_never_opens_privileged_gate() {
        let controller = PrivilegeController::new();
        controller.shared.replace(BrokerLifecycle::Requested);
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
