use std::fmt;

use serde::{Deserialize, Serialize};

pub const BROKER_PROTOCOL_VERSION: u16 = 1;
pub const MAX_BROKER_FRAME_BYTES: usize = 64 * 1024;
pub const SESSION_NONCE_BYTES: usize = 32;
const BROKER_PIPE_PREFIX: &str = r"\\.\pipe\LocalBridge-Privileged-";

pub(crate) fn is_valid_broker_pipe_name(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix(BROKER_PIPE_PREFIX) else { return false; };
    suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionNonce([u8; SESSION_NONCE_BYTES]);

impl SessionNonce {
    pub const fn from_bytes(bytes: [u8; SESSION_NONCE_BYTES]) -> Self { Self(bytes) }
    pub const fn as_bytes(&self) -> &[u8; SESSION_NONCE_BYTES] { &self.0 }
}

impl fmt::Debug for SessionNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("SessionNonce([REDACTED])") }
}

impl Drop for SessionNonce {
    fn drop(&mut self) {
        for byte in &mut self.0 { unsafe { std::ptr::write_volatile(byte, 0) }; }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerHello { pub version: u16, pub generation: u64, pub session_nonce: SessionNonce }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerReady { pub version: u16, pub generation: u64, pub session_nonce: SessionNonce }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrokerRequest { Ping, Shutdown }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerRequestEnvelope {
    pub version: u16,
    pub generation: u64,
    pub session_nonce: SessionNonce,
    pub sequence: u64,
    pub request: BrokerRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerRejectCode { ProtocolMismatch, StaleGeneration, SessionMismatch, Replay, Malformed, Oversized }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrokerResponse { Pong, ShutdownAck, Rejected { code: BrokerRejectCode } }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerResponseEnvelope { pub version: u16, pub generation: u64, pub sequence: u64, pub response: BrokerResponse }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerProtocolError { EmptyFrame, OversizedFrame, MalformedFrame, ProtocolMismatch, StaleGeneration, SessionMismatch, Replay }

#[derive(Debug)]
pub struct BrokerSession { generation: u64, session_nonce: SessionNonce, last_sequence: u64 }

impl BrokerSession {
    pub fn new(generation: u64, session_nonce: SessionNonce) -> Result<Self, BrokerProtocolError> {
        if generation == 0 { return Err(BrokerProtocolError::StaleGeneration); }
        Ok(Self { generation, session_nonce, last_sequence: 0 })
    }
    pub const fn generation(&self) -> u64 { self.generation }
    pub const fn last_sequence(&self) -> u64 { self.last_sequence }
    pub fn validate_request(&mut self, request: &BrokerRequestEnvelope) -> Result<(), BrokerProtocolError> {
        if request.version != BROKER_PROTOCOL_VERSION { return Err(BrokerProtocolError::ProtocolMismatch); }
        if request.generation != self.generation { return Err(BrokerProtocolError::StaleGeneration); }
        if request.session_nonce != self.session_nonce { return Err(BrokerProtocolError::SessionMismatch); }
        if request.sequence == 0 || request.sequence <= self.last_sequence { return Err(BrokerProtocolError::Replay); }
        self.last_sequence = request.sequence;
        Ok(())
    }
}

pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, BrokerProtocolError> {
    let payload = serde_json::to_vec(value).map_err(|_| BrokerProtocolError::MalformedFrame)?;
    if payload.is_empty() { return Err(BrokerProtocolError::EmptyFrame); }
    if payload.len() > MAX_BROKER_FRAME_BYTES { return Err(BrokerProtocolError::OversizedFrame); }
    Ok(payload)
}

pub fn decode_frame<T>(payload: &[u8]) -> Result<T, BrokerProtocolError>
where T: for<'de> Deserialize<'de> {
    if payload.is_empty() { return Err(BrokerProtocolError::EmptyFrame); }
    if payload.len() > MAX_BROKER_FRAME_BYTES { return Err(BrokerProtocolError::OversizedFrame); }
    serde_json::from_slice(payload).map_err(|_| BrokerProtocolError::MalformedFrame)
}

#[cfg(test)]
mod tests {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/unit/privilege/protocol.rs"));
}
