mod broker;
mod control;
#[cfg(windows)]
mod filesystem;
#[cfg(windows)]
mod execution;
mod protocol;
#[cfg(windows)]
mod windows;

pub use broker::{
    BrokerClientSession, BrokerProcessArgs, BrokerRunError, parse_broker_args, run_broker_process,
};
pub use control::{
    PrivilegeController, PrivilegedExecError, PrivilegedExecution, PrivilegedExecutionGateway,
};
#[cfg(windows)]
pub(crate) use execution::{ExecutionCancel, run_elevated_exec};
#[cfg(windows)]
pub(crate) use filesystem::run_privileged_filesystem;
pub use protocol::{
    BROKER_PROTOCOL_VERSION, BrokerProtocolError, BrokerReady, BrokerRejectCode, BrokerRequest,
    BrokerRequestEnvelope, BrokerResponse, BrokerResponseEnvelope, BrokerSession,
    ElevatedExecOutcome, ElevatedExecResult, ElevatedExecSpec, MAX_BROKER_FRAME_BYTES,
    MAX_ELEVATED_ARGS, MAX_ELEVATED_OUTPUT_BYTES, MAX_ELEVATED_REQUEST_ID_BYTES,
    MAX_ELEVATED_STRING_BYTES, MAX_ELEVATED_TIMEOUT_MS, MAX_PRIVILEGED_FILE_BYTES,
    PrivilegedFilesystemAction, PrivilegedFilesystemResult, PrivilegedFilesystemSpec,
    SESSION_NONCE_BYTES, ServerHello, SessionNonce, decode_frame, encode_frame,
    valid_elevated_request_id,
};
#[cfg(windows)]
pub use windows::{
    ElevatedBrokerProcess, NamedPipeClient, NamedPipeConnection, NamedPipeServer,
    PrivilegeIpcError, UacLaunchError, launch_broker_with_explicit_uac, random_session_nonce,
};
