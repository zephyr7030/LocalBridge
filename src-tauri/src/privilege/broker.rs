use std::collections::HashMap;
use std::fmt;
use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use crate::mcp::filesystem_service::FilesystemCancellation;

use super::protocol::is_valid_broker_pipe_name;
use super::{
    BROKER_PROTOCOL_VERSION, BrokerProtocolError, BrokerReady, BrokerRejectCode, BrokerRequest,
    BrokerRequestEnvelope, BrokerResponse, BrokerResponseEnvelope, BrokerSession,
    ElevatedExecResult, ElevatedExecSpec, NamedPipeClient, NamedPipeConnection, PrivilegeIpcError,
    ServerHello, SessionNonce, decode_frame, encode_frame, random_session_nonce,
    valid_elevated_request_id,
};
use super::{ExecutionCancel, run_elevated_exec};

struct ActiveExecution {
    cancel: ExecutionCancel,
    result: mpsc::Receiver<Result<ElevatedExecResult, super::execution::ExecutionError>>,
}

struct ActiveStructuredFilesystem {
    cancel: FilesystemCancellation,
    result: mpsc::Receiver<
        Result<super::AdministratorFilesystemResult, super::AdministratorFilesystemErrorCode>,
    >,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerProcessArgs {
    pub pipe_name: String,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerRunError {
    InvalidArguments,
    Ipc(PrivilegeIpcError),
    Protocol(BrokerProtocolError),
    HandshakeMismatch,
    UnexpectedResponse,
}

impl fmt::Display for BrokerRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments => f.write_str("invalid privileged broker arguments"),
            Self::Ipc(error) => write!(f, "privileged broker IPC failed: {error}"),
            Self::Protocol(error) => write!(f, "privileged broker protocol failed: {error:?}"),
            Self::HandshakeMismatch => f.write_str("privileged broker handshake mismatch"),
            Self::UnexpectedResponse => {
                f.write_str("privileged broker returned unexpected response")
            }
        }
    }
}

impl std::error::Error for BrokerRunError {}

impl From<PrivilegeIpcError> for BrokerRunError {
    fn from(value: PrivilegeIpcError) -> Self {
        Self::Ipc(value)
    }
}
impl From<BrokerProtocolError> for BrokerRunError {
    fn from(value: BrokerProtocolError) -> Self {
        Self::Protocol(value)
    }
}

pub fn parse_broker_args<I, S>(args: I) -> Result<BrokerProcessArgs, BrokerRunError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let mut pipe_name = None;
    let mut generation = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--pipe" if pipe_name.is_none() => pipe_name = args.next(),
            "--generation" if generation.is_none() => {
                generation = args.next().and_then(|value| value.parse::<u64>().ok());
            }
            _ => return Err(BrokerRunError::InvalidArguments),
        }
    }
    let pipe_name = pipe_name
        .filter(|value| is_valid_broker_pipe_name(value))
        .ok_or(BrokerRunError::InvalidArguments)?;
    let generation = generation
        .filter(|value| *value > 0)
        .ok_or(BrokerRunError::InvalidArguments)?;
    Ok(BrokerProcessArgs {
        pipe_name,
        generation,
    })
}

pub fn run_broker_process(args: BrokerProcessArgs) -> Result<(), BrokerRunError> {
    let mut pipe = NamedPipeClient::connect(&args.pipe_name)?;
    let hello: ServerHello = decode_frame(&pipe.read_frame()?)?;
    if hello.version != BROKER_PROTOCOL_VERSION || hello.generation != args.generation {
        return Err(BrokerRunError::HandshakeMismatch);
    }
    let ready = BrokerReady {
        version: BROKER_PROTOCOL_VERSION,
        generation: args.generation,
        session_nonce: hello.session_nonce.clone(),
    };
    pipe.write_frame(&encode_frame(&ready)?)?;
    let mut session = BrokerSession::new(args.generation, hello.session_nonce)?;
    let mut executions = HashMap::<String, ActiveExecution>::new();
    let mut structured_filesystems = HashMap::<String, ActiveStructuredFilesystem>::new();

    loop {
        let payload = pipe.read_frame()?;
        let envelope: BrokerRequestEnvelope = decode_frame(&payload)?;
        if let Err(error) = session.validate_request(&envelope) {
            let response = BrokerResponseEnvelope {
                version: BROKER_PROTOCOL_VERSION,
                generation: args.generation,
                sequence: envelope.sequence,
                response: BrokerResponse::Rejected {
                    code: reject_code(error),
                },
            };
            pipe.write_frame(&encode_frame(&response)?)?;
            continue;
        }
        let response = match envelope.request {
            BrokerRequest::Ping => BrokerResponse::Pong,
            BrokerRequest::Shutdown => {
                for execution in executions.values() {
                    execution.cancel.cancel();
                }
                for filesystem in structured_filesystems.values() {
                    filesystem.cancel.cancel();
                }
                BrokerResponse::ShutdownAck
            }
            BrokerRequest::StartExec { request_id, spec } => {
                if !valid_elevated_request_id(&request_id) || spec.validate().is_err() {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::Malformed,
                    }
                } else {
                    match executions.entry(request_id) {
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            let cancel = ExecutionCancel::default();
                            let worker_cancel = cancel.clone();
                            let (tx, rx) = mpsc::channel();
                            thread::Builder::new()
                                .name("localbridge-elevated-exec".into())
                                .spawn(move || {
                                    let _ = tx.send(run_elevated_exec(spec, worker_cancel));
                                })
                                .map_err(|_| BrokerRunError::UnexpectedResponse)?;
                            entry.insert(ActiveExecution { cancel, result: rx });
                            BrokerResponse::ExecAccepted
                        }
                        std::collections::hash_map::Entry::Occupied(_) => {
                            BrokerResponse::Rejected {
                                code: BrokerRejectCode::DuplicateRequest,
                            }
                        }
                    }
                }
            }
            BrokerRequest::PollExec { request_id } => {
                if !valid_elevated_request_id(&request_id) {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::Malformed,
                    }
                } else if let Some(execution) = executions.get(&request_id) {
                    match execution.result.try_recv() {
                        Ok(Ok(execution_result)) => {
                            executions.remove(&request_id);
                            BrokerResponse::ExecCompleted {
                                execution: execution_result,
                            }
                        }
                        Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                            executions.remove(&request_id);
                            BrokerResponse::Rejected {
                                code: BrokerRejectCode::ExecutionFailed,
                            }
                        }
                        Err(TryRecvError::Empty) => BrokerResponse::ExecPending,
                    }
                } else {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::RequestNotFound,
                    }
                }
            }
            BrokerRequest::CancelExec { request_id } => {
                if !valid_elevated_request_id(&request_id) {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::Malformed,
                    }
                } else if let Some(execution) = executions.get(&request_id) {
                    execution.cancel.cancel();
                    BrokerResponse::CancelAck
                } else {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::RequestNotFound,
                    }
                }
            }
            BrokerRequest::Filesystem { spec } => {
                if spec.validate().is_err() {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::Malformed,
                    }
                } else {
                    match super::run_privileged_filesystem(spec) {
                        Ok(filesystem) => BrokerResponse::FilesystemCompleted { filesystem },
                        Err(()) => BrokerResponse::Rejected {
                            code: BrokerRejectCode::ExecutionFailed,
                        },
                    }
                }
            }
            BrokerRequest::StructuredFilesystem { spec } => {
                if spec.validate().is_err() {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::Malformed,
                    }
                } else {
                    match super::run_administrator_filesystem(spec) {
                        Ok(filesystem) => {
                            BrokerResponse::StructuredFilesystemCompleted { filesystem }
                        }
                        Err(code) => BrokerResponse::StructuredFilesystemFailed { code },
                    }
                }
            }
            BrokerRequest::StartStructuredFilesystem { request_id, spec } => {
                if !valid_elevated_request_id(&request_id) || spec.validate().is_err() {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::Malformed,
                    }
                } else if executions.contains_key(&request_id)
                    || structured_filesystems.contains_key(&request_id)
                {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::DuplicateRequest,
                    }
                } else {
                    let cancel = FilesystemCancellation::default();
                    let worker_cancel = cancel.clone();
                    let (tx, rx) = mpsc::channel();
                    thread::Builder::new()
                        .name("localbridge-administrator-filesystem".into())
                        .spawn(move || {
                            let _ = tx.send(super::run_administrator_filesystem_with_cancellation(
                                spec,
                                worker_cancel,
                            ));
                        })
                        .map_err(|_| BrokerRunError::UnexpectedResponse)?;
                    structured_filesystems.insert(
                        request_id,
                        ActiveStructuredFilesystem { cancel, result: rx },
                    );
                    BrokerResponse::StructuredFilesystemAccepted
                }
            }
            BrokerRequest::PollStructuredFilesystem { request_id } => {
                if !valid_elevated_request_id(&request_id) {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::Malformed,
                    }
                } else if let Some(filesystem) = structured_filesystems.get(&request_id) {
                    match filesystem.result.try_recv() {
                        Ok(Ok(filesystem_result)) => {
                            structured_filesystems.remove(&request_id);
                            BrokerResponse::StructuredFilesystemCompleted {
                                filesystem: filesystem_result,
                            }
                        }
                        Ok(Err(code)) => {
                            structured_filesystems.remove(&request_id);
                            BrokerResponse::StructuredFilesystemFailed { code }
                        }
                        Err(TryRecvError::Disconnected) => {
                            structured_filesystems.remove(&request_id);
                            BrokerResponse::Rejected {
                                code: BrokerRejectCode::ExecutionFailed,
                            }
                        }
                        Err(TryRecvError::Empty) => BrokerResponse::StructuredFilesystemPending,
                    }
                } else {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::RequestNotFound,
                    }
                }
            }
            BrokerRequest::CancelStructuredFilesystem { request_id } => {
                if !valid_elevated_request_id(&request_id) {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::Malformed,
                    }
                } else if let Some(filesystem) = structured_filesystems.get(&request_id) {
                    filesystem.cancel.cancel();
                    BrokerResponse::CancelAck
                } else {
                    BrokerResponse::Rejected {
                        code: BrokerRejectCode::RequestNotFound,
                    }
                }
            }
        };
        let shutdown = matches!(response, BrokerResponse::ShutdownAck);
        pipe.write_frame(&encode_frame(&BrokerResponseEnvelope {
            version: BROKER_PROTOCOL_VERSION,
            generation: args.generation,
            sequence: envelope.sequence,
            response,
        })?)?;
        if shutdown {
            return Ok(());
        }
    }
}

fn reject_code(error: BrokerProtocolError) -> BrokerRejectCode {
    match error {
        BrokerProtocolError::ProtocolMismatch => BrokerRejectCode::ProtocolMismatch,
        BrokerProtocolError::StaleGeneration => BrokerRejectCode::StaleGeneration,
        BrokerProtocolError::SessionMismatch => BrokerRejectCode::SessionMismatch,
        BrokerProtocolError::Replay => BrokerRejectCode::Replay,
        BrokerProtocolError::EmptyFrame | BrokerProtocolError::MalformedFrame => {
            BrokerRejectCode::Malformed
        }
        BrokerProtocolError::OversizedFrame => BrokerRejectCode::Oversized,
    }
}

pub struct BrokerClientSession {
    pipe: NamedPipeConnection,
    generation: u64,
    session_nonce: SessionNonce,
    next_sequence: u64,
}

impl fmt::Debug for BrokerClientSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrokerClientSession")
            .field("generation", &self.generation)
            .field("session_nonce", &"[REDACTED]")
            .field("next_sequence", &self.next_sequence)
            .finish()
    }
}

impl BrokerClientSession {
    pub fn handshake(
        mut pipe: NamedPipeConnection,
        generation: u64,
    ) -> Result<Self, BrokerRunError> {
        if generation == 0 {
            return Err(BrokerRunError::HandshakeMismatch);
        }
        let session_nonce = random_session_nonce()?;
        pipe.write_frame(&encode_frame(&ServerHello {
            version: BROKER_PROTOCOL_VERSION,
            generation,
            session_nonce: session_nonce.clone(),
        })?)?;
        let ready: BrokerReady = decode_frame(&pipe.read_frame()?)?;
        if ready.version != BROKER_PROTOCOL_VERSION
            || ready.generation != generation
            || ready.session_nonce != session_nonce
        {
            return Err(BrokerRunError::HandshakeMismatch);
        }
        Ok(Self {
            pipe,
            generation,
            session_nonce,
            next_sequence: 1,
        })
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn ping(&mut self) -> Result<(), BrokerRunError> {
        match self.request(BrokerRequest::Ping)? {
            BrokerResponse::Pong => Ok(()),
            _ => Err(BrokerRunError::UnexpectedResponse),
        }
    }

    pub fn shutdown(&mut self) -> Result<(), BrokerRunError> {
        match self.request(BrokerRequest::Shutdown)? {
            BrokerResponse::ShutdownAck => Ok(()),
            _ => Err(BrokerRunError::UnexpectedResponse),
        }
    }

    pub fn start_exec(
        &mut self,
        request_id: String,
        spec: ElevatedExecSpec,
    ) -> Result<(), BrokerRunError> {
        match self.request(BrokerRequest::StartExec { request_id, spec })? {
            BrokerResponse::ExecAccepted => Ok(()),
            _ => Err(BrokerRunError::UnexpectedResponse),
        }
    }

    pub fn poll_exec(
        &mut self,
        request_id: String,
    ) -> Result<Option<ElevatedExecResult>, BrokerRunError> {
        match self.request(BrokerRequest::PollExec { request_id })? {
            BrokerResponse::ExecPending => Ok(None),
            BrokerResponse::ExecCompleted { execution } => Ok(Some(execution)),
            _ => Err(BrokerRunError::UnexpectedResponse),
        }
    }

    pub fn cancel_exec(&mut self, request_id: String) -> Result<(), BrokerRunError> {
        match self.request(BrokerRequest::CancelExec { request_id })? {
            BrokerResponse::CancelAck => Ok(()),
            _ => Err(BrokerRunError::UnexpectedResponse),
        }
    }

    pub fn filesystem(
        &mut self,
        spec: super::PrivilegedFilesystemSpec,
    ) -> Result<super::PrivilegedFilesystemResult, BrokerRunError> {
        match self.request(BrokerRequest::Filesystem { spec })? {
            BrokerResponse::FilesystemCompleted { filesystem } => Ok(filesystem),
            _ => Err(BrokerRunError::UnexpectedResponse),
        }
    }

    pub fn structured_filesystem(
        &mut self,
        spec: super::AdministratorFilesystemSpec,
    ) -> Result<
        Result<super::AdministratorFilesystemResult, super::AdministratorFilesystemErrorCode>,
        BrokerRunError,
    > {
        match self.request(BrokerRequest::StructuredFilesystem { spec })? {
            BrokerResponse::StructuredFilesystemCompleted { filesystem } => Ok(Ok(filesystem)),
            BrokerResponse::StructuredFilesystemFailed { code } => Ok(Err(code)),
            _ => Err(BrokerRunError::UnexpectedResponse),
        }
    }

    pub fn start_structured_filesystem(
        &mut self,
        request_id: String,
        spec: super::AdministratorFilesystemSpec,
    ) -> Result<(), BrokerRunError> {
        match self.request(BrokerRequest::StartStructuredFilesystem { request_id, spec })? {
            BrokerResponse::StructuredFilesystemAccepted => Ok(()),
            _ => Err(BrokerRunError::UnexpectedResponse),
        }
    }

    pub fn poll_structured_filesystem(
        &mut self,
        request_id: String,
    ) -> Result<
        Option<
            Result<super::AdministratorFilesystemResult, super::AdministratorFilesystemErrorCode>,
        >,
        BrokerRunError,
    > {
        match self.request(BrokerRequest::PollStructuredFilesystem { request_id })? {
            BrokerResponse::StructuredFilesystemPending => Ok(None),
            BrokerResponse::StructuredFilesystemCompleted { filesystem } => {
                Ok(Some(Ok(filesystem)))
            }
            BrokerResponse::StructuredFilesystemFailed { code } => Ok(Some(Err(code))),
            _ => Err(BrokerRunError::UnexpectedResponse),
        }
    }

    pub fn cancel_structured_filesystem(
        &mut self,
        request_id: String,
    ) -> Result<(), BrokerRunError> {
        match self.request(BrokerRequest::CancelStructuredFilesystem { request_id })? {
            BrokerResponse::CancelAck => Ok(()),
            _ => Err(BrokerRunError::UnexpectedResponse),
        }
    }

    fn request(&mut self, request: BrokerRequest) -> Result<BrokerResponse, BrokerRunError> {
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(BrokerRunError::UnexpectedResponse)?;
        self.pipe
            .write_frame(&encode_frame(&BrokerRequestEnvelope {
                version: BROKER_PROTOCOL_VERSION,
                generation: self.generation,
                session_nonce: self.session_nonce.clone(),
                sequence,
                request,
            })?)?;
        let response: BrokerResponseEnvelope = decode_frame(&self.pipe.read_frame()?)?;
        if response.version != BROKER_PROTOCOL_VERSION
            || response.generation != self.generation
            || response.sequence != sequence
        {
            return Err(BrokerRunError::UnexpectedResponse);
        }
        Ok(response.response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_cli_contains_only_pipe_and_generation_and_never_session_nonce() {
        let parsed = parse_broker_args([
            "--pipe",
            r"\\.\pipe\LocalBridge-Privileged-0123456789abcdef0123456789abcdef",
            "--generation",
            "7",
        ])
        .unwrap();
        assert_eq!(parsed.generation, 7);
        assert!(parsed.pipe_name.contains("LocalBridge-Privileged"));
        assert!(parse_broker_args(["--pipe", "bad", "--generation", "1"]).is_err());
        assert!(
            parse_broker_args([
                "--pipe",
                r"\\.\pipe\LocalBridge-Privileged-a",
                "--generation",
                "1"
            ])
            .is_err()
        );
        assert!(
            parse_broker_args([
                "--pipe",
                r"\\.\pipe\LocalBridge-Privileged-0123456789abcdef0123456789abcdef",
                "--generation",
                "1",
                "--extra",
                "x"
            ])
            .is_err()
        );
    }
}
