#![cfg(windows)]

use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use localbridge_lib::privilege::{
    BROKER_PROTOCOL_VERSION, BrokerClientSession, BrokerReady, BrokerRejectCode, BrokerRequest,
    BrokerRequestEnvelope, BrokerResponse, BrokerResponseEnvelope, ElevatedExecOutcome,
    ElevatedExecResult, ElevatedExecSpec, NamedPipeClient, NamedPipeServer, PrivilegeIpcError,
    ServerHello, decode_frame, encode_frame, random_session_nonce,
};

const BROKER_EXE: &str = env!("CARGO_BIN_EXE_localbridge-privileged-broker");

#[test]
fn actual_separate_broker_binary_handshakes_pings_and_gracefully_shuts_down() {
    let server = NamedPipeServer::create().unwrap();
    let pipe_name = server.name().to_owned();
    let generation = 11u64;
    let mut child = Command::new(BROKER_EXE)
        .args(["--pipe", &pipe_name, "--generation", &generation.to_string()])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().unwrap();
    let pid = child.id();
    let connection = server.accept_expected_client(pid).unwrap();
    let mut session = BrokerClientSession::handshake(connection, generation).unwrap();
    assert_eq!(session.generation(), generation);
    let debug = format!("{session:?}");
    assert!(debug.contains("[REDACTED]"));
    session.ping().unwrap();
    session.shutdown().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "broker did not exit after typed shutdown");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn same_user_wrong_pid_cannot_pass_server_peer_authentication() {
    let server = NamedPipeServer::create().unwrap();
    let pipe_name = server.name().to_owned();
    let client = thread::spawn(move || NamedPipeClient::connect(&pipe_name).map(drop));
    let error = server.accept_expected_client(u32::MAX).unwrap_err();
    assert!(matches!(error, PrivilegeIpcError::UnauthorizedPeer { .. }));
    let _ = client.join().unwrap();
}

fn spawn_broker(server: &NamedPipeServer, generation: u64) -> std::process::Child {
    Command::new(BROKER_EXE)
        .args([
            "--pipe",
            server.name(),
            "--generation",
            &generation.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn authenticated_broker(generation: u64) -> (std::process::Child, BrokerClientSession) {
    let server = NamedPipeServer::create().unwrap();
    let child = spawn_broker(&server, generation);
    let connection = server.accept_expected_client(child.id()).unwrap();
    let session = BrokerClientSession::handshake(connection, generation).unwrap();
    (child, session)
}

fn exec_spec(args: &[&str], timeout_ms: u32, max_output_bytes: u32) -> ElevatedExecSpec {
    ElevatedExecSpec {
        program: r"C:\Windows\System32\cmd.exe".to_string(),
        args: args.iter().map(|value| value.to_string()).collect(),
        workdir: Some(r"C:\Windows\Temp".to_string()),
        timeout_ms,
        max_output_bytes,
    }
}

fn poll_until_complete(
    session: &mut BrokerClientSession,
    request_id: &str,
    timeout: Duration,
) -> ElevatedExecResult {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(result) = session.poll_exec(request_id.to_string()).unwrap() {
            return result;
        }
        assert!(Instant::now() < deadline, "broker execution did not complete in time");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn actual_broker_rejects_replay_and_stale_generation_before_dispatch() {
    let server = NamedPipeServer::create().unwrap();
    let generation = 21u64;
    let mut child = spawn_broker(&server, generation);
    let mut connection = server.accept_expected_client(child.id()).unwrap();
    let nonce = random_session_nonce().unwrap();
    connection.write_frame(&encode_frame(&ServerHello {
        version: BROKER_PROTOCOL_VERSION,
        generation,
        session_nonce: nonce.clone(),
    }).unwrap()).unwrap();
    let ready: BrokerReady = decode_frame(&connection.read_frame().unwrap()).unwrap();
    assert_eq!(ready.generation, generation);

    let ping = BrokerRequestEnvelope {
        version: BROKER_PROTOCOL_VERSION,
        generation,
        session_nonce: nonce.clone(),
        sequence: 1,
        request: BrokerRequest::Ping,
    };
    connection.write_frame(&encode_frame(&ping).unwrap()).unwrap();
    let pong: BrokerResponseEnvelope = decode_frame(&connection.read_frame().unwrap()).unwrap();
    assert!(matches!(pong.response, BrokerResponse::Pong));

    connection.write_frame(&encode_frame(&ping).unwrap()).unwrap();
    let replay: BrokerResponseEnvelope = decode_frame(&connection.read_frame().unwrap()).unwrap();
    assert!(matches!(replay.response, BrokerResponse::Rejected { code: BrokerRejectCode::Replay }));

    let stale = BrokerRequestEnvelope {
        version: BROKER_PROTOCOL_VERSION,
        generation: generation - 1,
        session_nonce: nonce.clone(),
        sequence: 2,
        request: BrokerRequest::Ping,
    };
    connection.write_frame(&encode_frame(&stale).unwrap()).unwrap();
    let rejected: BrokerResponseEnvelope = decode_frame(&connection.read_frame().unwrap()).unwrap();
    assert!(matches!(rejected.response, BrokerResponse::Rejected { code: BrokerRejectCode::StaleGeneration }));

    let shutdown = BrokerRequestEnvelope {
        version: BROKER_PROTOCOL_VERSION,
        generation,
        session_nonce: nonce,
        sequence: 2,
        request: BrokerRequest::Shutdown,
    };
    connection.write_frame(&encode_frame(&shutdown).unwrap()).unwrap();
    let ack: BrokerResponseEnvelope = decode_frame(&connection.read_frame().unwrap()).unwrap();
    assert!(matches!(ack.response, BrokerResponse::ShutdownAck));
    assert!(child.wait().unwrap().success());
}

#[test]
fn broker_does_not_outlive_localbridge_pipe_session() {
    let server = NamedPipeServer::create().unwrap();
    let generation = 31u64;
    let mut child = spawn_broker(&server, generation);
    let connection = server.accept_expected_client(child.id()).unwrap();
    let session = BrokerClientSession::handshake(connection, generation).unwrap();
    drop(session);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "broker outlived disconnected LocalBridge session");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn actual_broker_structured_execution_supports_completion_timeout_cancel_limit_and_redaction() {
    let (mut child, mut session) = authenticated_broker(41);

    session
        .start_exec(
            "complete".to_string(),
            exec_spec(&["/d", "/c", "echo LB012_BROKER_EXEC"], 5_000, 4096),
        )
        .unwrap();
    let complete = poll_until_complete(&mut session, "complete", Duration::from_secs(5));
    assert_eq!(complete.outcome, ElevatedExecOutcome::Completed);
    assert!(complete.output.contains("LB012_BROKER_EXEC"));

    session
        .start_exec(
            "timeout".to_string(),
            exec_spec(&["/d", "/c", "ping -n 6 127.0.0.1 >nul"], 100, 4096),
        )
        .unwrap();
    let timed = poll_until_complete(&mut session, "timeout", Duration::from_secs(5));
    assert_eq!(timed.outcome, ElevatedExecOutcome::TimedOut);

    session
        .start_exec(
            "cancel".to_string(),
            exec_spec(&["/d", "/c", "ping -n 6 127.0.0.1 >nul"], 10_000, 4096),
        )
        .unwrap();
    thread::sleep(Duration::from_millis(80));
    session.cancel_exec("cancel".to_string()).unwrap();
    let cancelled = poll_until_complete(&mut session, "cancel", Duration::from_secs(5));
    assert_eq!(cancelled.outcome, ElevatedExecOutcome::Cancelled);

    session
        .start_exec(
            "limit".to_string(),
            exec_spec(
                &["/d", "/c", "for /L %i in (1,1,1000) do @echo 1234567890"],
                5_000,
                128,
            ),
        )
        .unwrap();
    let limited = poll_until_complete(&mut session, "limit", Duration::from_secs(5));
    assert!(limited.output.len() <= 128);
    assert!(limited.truncated);

    let secret = "LB012_SYNTHETIC_BROKER_SECRET";
    session
        .start_exec(
            "redact".to_string(),
            exec_spec(
                &["/d", "/c", &format!("echo {secret}"), "--api-key", secret],
                5_000,
                4096,
            ),
        )
        .unwrap();
    let redacted = poll_until_complete(&mut session, "redact", Duration::from_secs(5));
    assert_eq!(redacted.output, "[REDACTED]");
    assert!(!redacted.output.contains(secret));

    session.shutdown().unwrap();
    assert!(child.wait().unwrap().success());
}
