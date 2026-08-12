mod broker;
mod protocol;
#[cfg(windows)]
mod windows;

pub use broker::{
    BrokerClientSession, BrokerProcessArgs, BrokerRunError, parse_broker_args, run_broker_process,
};
pub use protocol::{
    BROKER_PROTOCOL_VERSION, BrokerProtocolError, BrokerReady, BrokerRejectCode, BrokerRequest,
    BrokerRequestEnvelope, BrokerResponse, BrokerResponseEnvelope, BrokerSession,
    MAX_BROKER_FRAME_BYTES, SESSION_NONCE_BYTES, ServerHello, SessionNonce, decode_frame,
    encode_frame,
};
#[cfg(windows)]
pub use windows::{
    ElevatedBrokerProcess, NamedPipeClient, NamedPipeConnection, NamedPipeServer, PrivilegeIpcError,
    UacLaunchError, launch_broker_with_explicit_uac, random_session_nonce,
};
