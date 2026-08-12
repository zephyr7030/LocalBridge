use std::fmt;

use super::{
    BROKER_PROTOCOL_VERSION, BrokerProtocolError, BrokerReady, BrokerRejectCode, BrokerRequest,
    BrokerRequestEnvelope, BrokerResponse, BrokerResponseEnvelope, BrokerSession,
    NamedPipeClient, NamedPipeConnection, PrivilegeIpcError, ServerHello, SessionNonce, decode_frame,
    encode_frame, random_session_nonce,
};
use super::protocol::is_valid_broker_pipe_name;

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
            Self::UnexpectedResponse => f.write_str("privileged broker returned unexpected response"),
        }
    }
}

impl std::error::Error for BrokerRunError {}

impl From<PrivilegeIpcError> for BrokerRunError {
    fn from(value: PrivilegeIpcError) -> Self { Self::Ipc(value) }
}
impl From<BrokerProtocolError> for BrokerRunError {
    fn from(value: BrokerProtocolError) -> Self { Self::Protocol(value) }
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
    let pipe_name = pipe_name.filter(|value| is_valid_broker_pipe_name(value))
        .ok_or(BrokerRunError::InvalidArguments)?;
    let generation = generation.filter(|value| *value > 0).ok_or(BrokerRunError::InvalidArguments)?;
    Ok(BrokerProcessArgs { pipe_name, generation })
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

    loop {
        let payload = pipe.read_frame()?;
        let envelope: BrokerRequestEnvelope = decode_frame(&payload)?;
        if let Err(error) = session.validate_request(&envelope) {
            let response = BrokerResponseEnvelope {
                version: BROKER_PROTOCOL_VERSION,
                generation: args.generation,
                sequence: envelope.sequence,
                response: BrokerResponse::Rejected { code: reject_code(error) },
            };
            pipe.write_frame(&encode_frame(&response)?)?;
            continue;
        }
        let response = match envelope.request {
            BrokerRequest::Ping => BrokerResponse::Pong,
            BrokerRequest::Shutdown => BrokerResponse::ShutdownAck,
        };
        let shutdown = matches!(response, BrokerResponse::ShutdownAck);
        pipe.write_frame(&encode_frame(&BrokerResponseEnvelope {
            version: BROKER_PROTOCOL_VERSION,
            generation: args.generation,
            sequence: envelope.sequence,
            response,
        })?)?;
        if shutdown { return Ok(()); }
    }
}

fn reject_code(error: BrokerProtocolError) -> BrokerRejectCode {
    match error {
        BrokerProtocolError::ProtocolMismatch => BrokerRejectCode::ProtocolMismatch,
        BrokerProtocolError::StaleGeneration => BrokerRejectCode::StaleGeneration,
        BrokerProtocolError::SessionMismatch => BrokerRejectCode::SessionMismatch,
        BrokerProtocolError::Replay => BrokerRejectCode::Replay,
        BrokerProtocolError::EmptyFrame | BrokerProtocolError::MalformedFrame => BrokerRejectCode::Malformed,
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
    pub fn handshake(mut pipe: NamedPipeConnection, generation: u64) -> Result<Self, BrokerRunError> {
        if generation == 0 { return Err(BrokerRunError::HandshakeMismatch); }
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
        Ok(Self { pipe, generation, session_nonce, next_sequence: 1 })
    }

    pub const fn generation(&self) -> u64 { self.generation }

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

    fn request(&mut self, request: BrokerRequest) -> Result<BrokerResponse, BrokerRunError> {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.checked_add(1).ok_or(BrokerRunError::UnexpectedResponse)?;
        self.pipe.write_frame(&encode_frame(&BrokerRequestEnvelope {
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
            "--pipe", r"\\.\pipe\LocalBridge-Privileged-0123456789abcdef0123456789abcdef",
            "--generation", "7",
        ]).unwrap();
        assert_eq!(parsed.generation, 7);
        assert!(parsed.pipe_name.contains("LocalBridge-Privileged"));
        assert!(parse_broker_args(["--pipe", "bad", "--generation", "1"]).is_err());
        assert!(parse_broker_args(["--pipe", r"\\.\pipe\LocalBridge-Privileged-a", "--generation", "1"]).is_err());
        assert!(parse_broker_args(["--pipe", r"\\.\pipe\LocalBridge-Privileged-0123456789abcdef0123456789abcdef", "--generation", "1", "--extra", "x"]).is_err());
    }
}
