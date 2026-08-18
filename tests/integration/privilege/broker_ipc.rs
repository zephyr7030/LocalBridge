#![cfg(windows)]

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use localbridge_lib::mcp::{
    CapabilityPolicy, CodingToolsPermissionMode, CodingToolsRuntime, CodingToolsRuntimeConfig,
    InternalBearer, PolicyEnforcementRuntime,
};
use localbridge_lib::privilege::{
    BROKER_PROTOCOL_VERSION, BrokerClientSession, BrokerReady, BrokerRejectCode, BrokerRequest,
    BrokerRequestEnvelope, BrokerResponse, BrokerResponseEnvelope, ElevatedExecOutcome,
    ElevatedExecResult, ElevatedExecSpec, NamedPipeClient, NamedPipeServer, PrivilegeController,
    PrivilegeIpcError, PrivilegedExecution, PrivilegedFilesystemAction, PrivilegedFilesystemSpec,
    ServerHello, decode_frame, encode_frame, random_session_nonce,
};
use localbridge_lib::state::{PermissionMode, PrivilegeState};
use serde_json::{Value, json};

const BROKER_EXE: &str = env!("CARGO_BIN_EXE_localbridge-privileged-broker");

#[test]
fn actual_separate_broker_binary_handshakes_pings_and_gracefully_shuts_down() {
    let server = NamedPipeServer::create().unwrap();
    let pipe_name = server.name().to_owned();
    let generation = 11u64;
    let mut child = Command::new(BROKER_EXE)
        .args([
            "--pipe",
            &pipe_name,
            "--generation",
            &generation.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
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
        assert!(
            Instant::now() < deadline,
            "broker did not exit after typed shutdown"
        );
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
        assert!(
            Instant::now() < deadline,
            "broker execution did not complete in time"
        );
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
    connection
        .write_frame(
            &encode_frame(&ServerHello {
                version: BROKER_PROTOCOL_VERSION,
                generation,
                session_nonce: nonce.clone(),
            })
            .unwrap(),
        )
        .unwrap();
    let ready: BrokerReady = decode_frame(&connection.read_frame().unwrap()).unwrap();
    assert_eq!(ready.generation, generation);

    let ping = BrokerRequestEnvelope {
        version: BROKER_PROTOCOL_VERSION,
        generation,
        session_nonce: nonce.clone(),
        sequence: 1,
        request: BrokerRequest::Ping,
    };
    connection
        .write_frame(&encode_frame(&ping).unwrap())
        .unwrap();
    let pong: BrokerResponseEnvelope = decode_frame(&connection.read_frame().unwrap()).unwrap();
    assert!(matches!(pong.response, BrokerResponse::Pong));

    connection
        .write_frame(&encode_frame(&ping).unwrap())
        .unwrap();
    let replay: BrokerResponseEnvelope = decode_frame(&connection.read_frame().unwrap()).unwrap();
    assert!(matches!(
        replay.response,
        BrokerResponse::Rejected {
            code: BrokerRejectCode::Replay
        }
    ));

    let stale = BrokerRequestEnvelope {
        version: BROKER_PROTOCOL_VERSION,
        generation: generation - 1,
        session_nonce: nonce.clone(),
        sequence: 2,
        request: BrokerRequest::Ping,
    };
    connection
        .write_frame(&encode_frame(&stale).unwrap())
        .unwrap();
    let rejected: BrokerResponseEnvelope = decode_frame(&connection.read_frame().unwrap()).unwrap();
    assert!(matches!(
        rejected.response,
        BrokerResponse::Rejected {
            code: BrokerRejectCode::StaleGeneration
        }
    ));

    let shutdown = BrokerRequestEnvelope {
        version: BROKER_PROTOCOL_VERSION,
        generation,
        session_nonce: nonce,
        sequence: 2,
        request: BrokerRequest::Shutdown,
    };
    connection
        .write_frame(&encode_frame(&shutdown).unwrap())
        .unwrap();
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
        assert!(
            Instant::now() < deadline,
            "broker outlived disconnected LocalBridge session"
        );
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
    assert!(complete.stdout.contains("LB012_BROKER_EXEC"));
    assert!(complete.stderr.is_empty());

    session
        .start_exec(
            "split".to_string(),
            exec_spec(
                &["/d", "/c", "echo LB012_STDOUT & echo LB012_STDERR 1>&2"],
                5_000,
                4096,
            ),
        )
        .unwrap();
    let split = poll_until_complete(&mut session, "split", Duration::from_secs(5));
    assert!(split.stdout.contains("LB012_STDOUT"));
    assert!(split.stderr.contains("LB012_STDERR"));

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
    assert!(limited.stdout.len() + limited.stderr.len() <= 128);
    assert!(limited.truncated);

    session
        .start_exec(
            "large-frame".to_string(),
            exec_spec(
                &["/d", "/c", "for /L %i in (1,1,8000) do @echo 1234567890"],
                5_000,
                128 * 1024,
            ),
        )
        .unwrap();
    let large_frame = poll_until_complete(&mut session, "large-frame", Duration::from_secs(5));
    assert!(large_frame.stdout.len() > 64 * 1024);
    assert!(!large_frame.truncated);

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
    assert_eq!(redacted.stdout, "[REDACTED]");
    assert!(!redacted.stdout.contains(secret));
    assert!(!redacted.stderr.contains(secret));

    session.shutdown().unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
fn actual_broker_structured_filesystem_roundtrips_outside_workspace_without_shell() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "localbridge-lb012-broker-fs-{}-{nonce}",
        std::process::id()
    ));
    let source = root.join("source.bin");
    let renamed = root.join("renamed.bin");
    let (mut child, mut session) = authenticated_broker(42);

    session
        .filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::CreateDirectory,
            path: root.to_string_lossy().into_owned(),
            destination: None,
            content_base64: None,
            recursive: false,
        })
        .unwrap();
    session
        .filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::WriteFile,
            path: source.to_string_lossy().into_owned(),
            destination: None,
            content_base64: Some("TEIwMTI=".to_string()),
            recursive: false,
        })
        .unwrap();
    let read = session
        .filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::ReadFile,
            path: source.to_string_lossy().into_owned(),
            destination: None,
            content_base64: None,
            recursive: false,
        })
        .unwrap();
    assert_eq!(read.content_base64.as_deref(), Some("TEIwMTI="));
    assert_eq!(read.bytes, 5);
    session
        .filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::Rename,
            path: source.to_string_lossy().into_owned(),
            destination: Some(renamed.to_string_lossy().into_owned()),
            content_base64: None,
            recursive: false,
        })
        .unwrap();
    assert!(renamed.is_file());
    session
        .filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::Delete,
            path: root.to_string_lossy().into_owned(),
            destination: None,
            content_base64: None,
            recursive: true,
        })
        .unwrap();
    assert!(!root.exists());

    session.shutdown().unwrap();
    assert!(child.wait().unwrap().success());
}

#[derive(Debug)]
struct LiveMcpResponse {
    status: u16,
    session: Option<String>,
    body: Value,
}

fn live_free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.local_addr().unwrap().port()
}

fn live_post(port: u16, session: Option<&str>, payload: &Value) -> LiveMcpResponse {
    let body = serde_json::to_vec(payload).unwrap();
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let mut request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nContent-Type: application/json\r\nMCP-Protocol-Version: 2025-11-25\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(session) = session {
        request.push_str("Mcp-Session-Id: ");
        request.push_str(session);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(&body).unwrap();
    stream.flush().unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let header = std::str::from_utf8(&bytes[..split]).unwrap();
    let mut lines = header.lines();
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let response_session = lines.find_map(|line| {
        line.split_once(':').and_then(|(name, value)| {
            name.eq_ignore_ascii_case("Mcp-Session-Id")
                .then(|| value.trim().to_string())
        })
    });
    let body_bytes = &bytes[(split + 4)..];
    let body = if body_bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(body_bytes).unwrap()
    };
    LiveMcpResponse {
        status,
        session: response_session,
        body,
    }
}

fn live_initialize(port: u16, id: u64) -> LiveMcpResponse {
    live_post(
        port,
        None,
        &json!({
            "jsonrpc":"2.0",
            "id":id,
            "method":"initialize",
            "params":{
                "protocolVersion":"2025-11-25",
                "capabilities":{},
                "clientInfo":{"name":"lb012-live-uac","version":"1"}
            }
        }),
    )
}

fn live_tool_call(
    port: u16,
    session: &str,
    id: u64,
    name: &str,
    arguments: Value,
) -> LiveMcpResponse {
    live_post(
        port,
        Some(session),
        &json!({
            "jsonrpc":"2.0",
            "id":id,
            "method":"tools/call",
            "params":{"name":name,"arguments":arguments}
        }),
    )
}

fn live_tool_names(response: &LiveMcpResponse) -> Vec<String> {
    response.body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect()
}

fn live_ordinary_output(response: &LiveMcpResponse) -> &str {
    response.body["result"]["structuredContent"]["data"]["output"]
        .as_str()
        .unwrap_or_default()
}

#[test]
#[ignore = "requires explicit UAC approval"]
fn live_uac_mcp_elevated_exec_uses_administrator_token_and_revokes_catalog() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!(
        "localbridge-lb012-live-uac-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&workspace).unwrap();

    let current_exe = std::env::current_exe().unwrap();
    let sibling_broker = current_exe
        .parent()
        .unwrap()
        .join("localbridge-privileged-broker.exe");
    fs::copy(BROKER_EXE, &sibling_broker).unwrap();

    let controller = PrivilegeController::new();
    let privileged: Arc<dyn PrivilegedExecution> = Arc::new(controller.gateway());
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &repo,
            &workspace,
            live_free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new("LB012_LIVE_UAC_SYNTHETIC_BEARER").unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled coding runtime for live UAC acceptance");
    let pep = PolicyEnforcementRuntime::start_with_privilege(
        coding,
        CapabilityPolicy::load(&repo.join("runtime-policy.toml")).unwrap(),
        PermissionMode::Full,
        privileged,
    )
    .expect("PEP with real privilege gateway");

    let initialized = live_initialize(pep.port(), 9000);
    assert_eq!(initialized.status, 200);
    let mut session = initialized.session.unwrap();
    let full_tools = live_post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":9001,"method":"tools/list","params":{}}),
    );
    assert!(
        !live_tool_names(&full_tools)
            .iter()
            .any(|name| name == "elevated_exec")
    );

    let ordinary_before = live_tool_call(
        pep.port(),
        &session,
        9002,
        "exec_command",
        json!({"command":"whoami /groups","shell":"cmd","yield_time_ms":10000}),
    );
    assert_eq!(
        ordinary_before.body["result"]["isError"], false,
        "{:#?}",
        ordinary_before.body
    );
    let before = live_ordinary_output(&ordinary_before);
    assert!(
        before.contains("S-1-16-8192"),
        "ordinary route was not Medium before UAC: {before}"
    );
    assert!(
        !before.contains("S-1-16-12288"),
        "ordinary route was High before UAC: {before}"
    );

    controller
        .enable_from_explicit_user_action(&sibling_broker)
        .expect("explicit UAC broker activation");
    assert!(matches!(controller.state(), PrivilegeState::Active { .. }));
    pep.set_permission_mode(PermissionMode::Elevated);

    let stale_after_enable = live_post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":9003,"method":"ping","params":{}}),
    );
    assert_eq!(stale_after_enable.status, 404);
    session = live_initialize(pep.port(), 9004).session.unwrap();
    let elevated_tools = live_post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":9005,"method":"tools/list","params":{}}),
    );
    assert!(
        live_tool_names(&elevated_tools)
            .iter()
            .any(|name| name == "elevated_exec")
    );

    let ordinary_after = live_tool_call(
        pep.port(),
        &session,
        9006,
        "exec_command",
        json!({"command":"whoami /groups","shell":"cmd","yield_time_ms":10000}),
    );
    let after = live_ordinary_output(&ordinary_after);
    assert!(
        after.contains("S-1-16-8192"),
        "ordinary route stopped being Medium after Broker Active: {after}"
    );
    assert!(
        !after.contains("S-1-16-12288"),
        "ordinary route inherited Broker token: {after}"
    );

    let system_root = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
    let whoami = system_root.join("System32").join("whoami.exe");
    let elevated = live_tool_call(
        pep.port(),
        &session,
        9007,
        "elevated_exec",
        json!({
            "operation":"process",
            "program":whoami.to_string_lossy(),
            "args":["/groups"],
            "workdir":system_root.join("Temp").to_string_lossy(),
            "timeout_ms":10000,
            "max_output_bytes":65536
        }),
    );
    assert_eq!(
        elevated.body["result"]["isError"], false,
        "{:#?}",
        elevated.body
    );
    let elevated_output = elevated.body["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        elevated_output.contains("S-1-16-12288"),
        "Broker route did not obtain High/Administrator token: {elevated_output}"
    );

    controller
        .disable()
        .expect("disable real Broker after identity probe");
    let stale_after_disable = live_post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":9008,"method":"tools/list","params":{}}),
    );
    assert_eq!(stale_after_disable.status, 404);
    session = live_initialize(pep.port(), 9009).session.unwrap();
    let revoked_tools = live_post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":9010,"method":"tools/list","params":{}}),
    );
    assert!(
        !live_tool_names(&revoked_tools)
            .iter()
            .any(|name| name == "elevated_exec")
    );
    let revoked_call = live_tool_call(
        pep.port(),
        &session,
        9011,
        "elevated_exec",
        json!({
            "operation":"process",
            "program":whoami.to_string_lossy(),
            "args":["/groups"],
            "workdir":system_root.join("Temp").to_string_lossy(),
            "timeout_ms":10000,
            "max_output_bytes":65536
        }),
    );
    assert_eq!(
        revoked_call.body["result"]["structuredContent"]["error"]["code"],
        "ElevationRequired"
    );

    let mut coding = pep.stop().expect("stop live UAC PEP");
    coding.stop().expect("stop live UAC coding runtime");
    drop(coding);
    let _ = fs::remove_file(&sibling_broker);
    let _ = fs::remove_dir_all(&workspace);
    println!(
        "LB012_LIVE_UAC=PASS ordinary_before=S-1-16-8192 ordinary_after=S-1-16-8192 elevated=S-1-16-12288 stale_enable=404 stale_disable=404"
    );
}
