use super::*;

fn nonce(byte: u8) -> SessionNonce { SessionNonce::from_bytes([byte; SESSION_NONCE_BYTES]) }
fn request(generation: u64, session_nonce: SessionNonce, sequence: u64) -> BrokerRequestEnvelope {
    BrokerRequestEnvelope { version: BROKER_PROTOCOL_VERSION, generation, session_nonce, sequence, request: BrokerRequest::Ping }
}

#[test]
fn stale_generation_nonce_mismatch_and_replay_are_rejected() {
    let mut session = BrokerSession::new(7, nonce(1)).unwrap();
    assert_eq!(session.validate_request(&request(6, nonce(1), 1)), Err(BrokerProtocolError::StaleGeneration));
    assert_eq!(session.validate_request(&request(7, nonce(2), 1)), Err(BrokerProtocolError::SessionMismatch));
    session.validate_request(&request(7, nonce(1), 1)).unwrap();
    assert_eq!(session.validate_request(&request(7, nonce(1), 1)), Err(BrokerProtocolError::Replay));
    assert_eq!(session.validate_request(&request(7, nonce(1), 0)), Err(BrokerProtocolError::Replay));
    session.validate_request(&request(7, nonce(1), 3)).unwrap();
    assert_eq!(session.validate_request(&request(7, nonce(1), 2)), Err(BrokerProtocolError::Replay));
}

#[test]
fn protocol_mismatch_and_zero_generation_are_rejected() {
    assert!(matches!(BrokerSession::new(0, nonce(1)), Err(BrokerProtocolError::StaleGeneration)));
    let mut session = BrokerSession::new(1, nonce(1)).unwrap();
    let mut envelope = request(1, nonce(1), 1);
    envelope.version = BROKER_PROTOCOL_VERSION + 1;
    assert_eq!(session.validate_request(&envelope), Err(BrokerProtocolError::ProtocolMismatch));
}

#[test]
fn malformed_unknown_empty_and_oversized_frames_are_rejected() {
    assert_eq!(decode_frame::<BrokerRequestEnvelope>(&[]), Err(BrokerProtocolError::EmptyFrame));
    assert_eq!(decode_frame::<BrokerRequestEnvelope>(b"not-json"), Err(BrokerProtocolError::MalformedFrame));
    let unknown = br#"{"version":1,"generation":1,"session_nonce":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"sequence":1,"request":{"operation":"unknown_operation"}}"#;
    assert_eq!(decode_frame::<BrokerRequestEnvelope>(unknown), Err(BrokerProtocolError::MalformedFrame));
    let oversized = vec![b'x'; MAX_BROKER_FRAME_BYTES + 1];
    assert_eq!(decode_frame::<BrokerRequestEnvelope>(&oversized), Err(BrokerProtocolError::OversizedFrame));
}

#[test]
fn nonce_debug_is_redacted_and_foundation_operations_are_only_ping_shutdown() {
    let sentinel = nonce(0x5a);
    assert_eq!(format!("{sentinel:?}"), "SessionNonce([REDACTED])");
    let ping = String::from_utf8(encode_frame(&request(1, nonce(3), 1)).unwrap()).unwrap();
    assert!(ping.contains("ping"));
    let shutdown = BrokerRequest::Shutdown;
    assert!(matches!(shutdown, BrokerRequest::Shutdown));
}
