use super::super::test_support::*;
use super::*;
use crate::mcp::V1_CORE_TOOL_NAMES;

#[test]
fn task_cancel_selection_is_explicit_and_session_local() {
    let task_a = TaskId::new("task-a");
    let task_b = TaskId::new("task-b");
    let candidates = BTreeSet::from([task_a.clone(), task_b.clone()]);

    assert_eq!(
        select_cancellable_task(None, candidates.clone())
            .unwrap_err()
            .code,
        FacadeErrorCode::TaskIdRequired
    );
    assert_eq!(
        select_cancellable_task(Some(TaskId::new("foreign")), candidates.clone())
            .unwrap_err()
            .code,
        FacadeErrorCode::TaskNotOwned
    );
    assert_eq!(
        select_cancellable_task(Some(task_a.clone()), candidates).unwrap(),
        Some(task_a.clone())
    );
    assert_eq!(
        select_cancellable_task(None, BTreeSet::from([task_a.clone()])).unwrap(),
        Some(task_a)
    );
    assert_eq!(
        select_cancellable_task(None, BTreeSet::new())
            .unwrap_err()
            .code,
        FacadeErrorCode::NotFound
    );
}

#[test]
fn runtime_unavailable_is_a_failed_task_not_a_lost_task() {
    let runtime_unavailable: Result<Value, FacadeCallError> = Ok(FacadeError::new(
        FacadeErrorCode::RuntimeUnavailable,
        "runtime is unavailable before execution starts",
        true,
    )
    .to_mcp_result());
    assert_eq!(
        task_terminal_outcome(&runtime_unavailable),
        TerminalOutcome::Failed
    );

    let previously_owned_session_lost: Result<Value, FacadeCallError> = Ok(FacadeError::new(
        FacadeErrorCode::SessionUnavailable,
        "an accepted session can no longer be observed",
        false,
    )
    .to_mcp_result());
    assert_eq!(
        task_terminal_outcome(&previously_owned_session_lost),
        TerminalOutcome::Lost
    );
}

#[test]
fn public_tool_schemas_remain_complete_across_long_multi_tool_sessions() {
    fn listed_names(response: &ClientResponse) -> Vec<&str> {
        response.body["result"]["tools"]
            .as_array()
            .expect("tools/list array")
            .iter()
            .map(|tool| tool["name"].as_str().expect("public tool name"))
            .collect()
    }

    let fixture = PublicRuntimeFixture::start(PermissionMode::Full);
    let pep = fixture.runtime();
    let initialized = initialize(pep.port(), 40_000);
    let session = initialized.session.expect("downstream MCP session");
    assert_eq!(
        post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .status,
        202
    );
    assert_eq!(get_sse(pep.port(), &session).status, 200);

    let expected = V1_CORE_TOOL_NAMES
        .iter()
        .copied()
        .chain(std::iter::once("elevated_exec"))
        .collect::<Vec<_>>();
    let mut next_id = 40_001u64;
    for turn in 0..32 {
        if turn == 8 {
            pep.set_simulated_permission_for_test(PermissionMode::Edit);
        } else if turn == 20 {
            pep.set_simulated_permission_for_test(PermissionMode::Full);
        }
        let (name, arguments) = match turn % 4 {
            0 => ("workspace_context", json!({"detail":"compact"})),
            1 => ("filesystem", json!({"action":"stat","path":"probe.txt"})),
            2 => (
                "document_workflow",
                json!({"action":"inspect","path":"probe.txt","max_lines":8}),
            ),
            _ => ("git_workflow", json!({"action":"status","path":"."})),
        };
        let response = public_tool_call(pep.port(), &session, next_id, name, arguments);
        next_id += 1;
        assert_eq!(response.status, 200, "turn={turn} tool={name}");

        if turn % 4 == 3 {
            let listed = post(
                pep.port(),
                Some(&session),
                &json!({"jsonrpc":"2.0","id":next_id,"method":"tools/list","params":{}}),
            );
            next_id += 1;
            assert_eq!(listed_names(&listed), expected, "turn={turn}");
        }
    }

    pep.set_simulated_permission_for_test(PermissionMode::Edit);
    let context = public_tool_call(
        pep.port(),
        &session,
        next_id,
        "workspace_context",
        json!({"detail":"compact"}),
    );
    let capabilities = &context.body["result"]["structuredContent"]["data"]["capabilities"];
    assert_eq!(capabilities["tool_schema_projection"], "stable");
    assert_eq!(
        capabilities["public_tools"].as_array().unwrap().len(),
        expected.len()
    );
    assert!(
        !capabilities["policy_allowed_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "exec_command")
    );
    let denied = public_tool_call(
        pep.port(),
        &session,
        next_id + 1,
        "exec_command",
        json!({"command":"echo must-not-run","shell":"cmd"}),
    );
    assert_tool_error(&denied, "PolicyDenied");
    let listed = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":next_id + 2,"method":"tools/list","params":{}}),
    );
    assert_eq!(listed_names(&listed), expected);
    assert_eq!(
        get_sse(pep.port(), &session).status,
        204,
        "permission changes must not announce a schema deletion"
    );

    fixture.shutdown();
}

fn http_read_error(raw: &[u8]) -> HttpReadError {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    client.write_all(raw).unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    read_request(&mut server).unwrap_err()
}

fn http_read(raw: &[u8]) -> HttpRequest {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    client.write_all(raw).unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    read_request(&mut server).unwrap()
}

#[test]
fn schema42_mcp_http_failures_have_distinct_transport_causes() {
    assert_eq!(
        http_read_error(b"BROKEN\r\n\r\n").cause,
        "malformed_request"
    );
    let chunked = http_read(
        b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"a\r\n3\r\n\":1\r\n1\r\n}\r\n0\r\n\r\n",
    );
    assert_eq!(chunked.body, br#"{"a":1}"#);
    assert_eq!(
        http_read_error(b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n").cause,
        "unsupported_transfer_encoding"
    );
    assert_eq!(
        http_read_error(
            b"POST /mcp HTTP/1.1\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
        )
        .cause,
        "ambiguous_body_framing"
    );
    assert_eq!(
        http_read_error(b"POST /mcp HTTP/1.1\r\nContent-Length: 5\r\n\r\nab").cause,
        "early_eof"
    );
    let oversized_body = format!(
        "POST /mcp HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        MAX_BODY_BYTES + 1
    );
    assert_eq!(
        http_read_error(oversized_body.as_bytes()).cause,
        "body_too_large"
    );

    let mut oversized_header = b"GET /mcp HTTP/1.1\r\nX-Test: ".to_vec();
    oversized_header.extend(std::iter::repeat_n(b'a', MAX_HEADER_BYTES + 1));
    assert_eq!(http_read_error(&oversized_header).cause, "header_too_large");

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let _client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    server.set_nonblocking(true).unwrap();
    let disconnected = read_request(&mut server).unwrap_err();
    assert_eq!(disconnected.cause, "connection_closed_before_request");
    assert!(!disconnected.respond);
}

#[test]
fn accepted_connection_waits_for_delayed_request_bytes() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    let (mut server, _) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "listener did not accept client");
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("listener accept failed: {error}"),
        }
    };
    let writer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        client
            .write_all(b"GET /mcp HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    configure_accepted_stream(&server).unwrap();
    let request = read_request(&mut server).expect("accepted connection waits for HTTP bytes");
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/mcp");
    writer.join().unwrap();
}

#[test]
fn schema42_http_400_response_is_unavailable_transport_not_runtime() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    write_http_diagnostic_error(
        &mut server,
        HttpReadError::new(400, "unsupported_transfer_encoding"),
    )
    .unwrap();
    drop(server);
    let response = parse_client_response(client.try_clone().unwrap());
    assert_eq!(response.status, 400);
    assert_eq!(response.body["error"]["error_code"], "Unavailable");
    assert_eq!(response.body["error"]["phase"], "transport");
    assert_eq!(
        response.body["error"]["cause"],
        "unsupported_transfer_encoding"
    );
    assert_eq!(response.body["error"]["http_status"], 400);
    let _ = client.shutdown(std::net::Shutdown::Both);
}

#[test]
fn schema42_public_mcp_http_failures_are_diagnostic_while_success_stays_empty() {
    for (status, diagnostic, error_code, cause) in [
        (
            400,
            mcp_invalid("session_id_required"),
            "InvalidRequest",
            "session_id_required",
        ),
        (
            404,
            mcp_unavailable("session_not_found"),
            "Unavailable",
            "session_not_found",
        ),
        (
            503,
            mcp_unavailable("server_stopping"),
            "Unavailable",
            "server_stopping",
        ),
    ] {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        write_mcp_http_error(&mut server, status, diagnostic, None).unwrap();
        drop(server);
        let response = parse_client_response(client);
        assert_eq!(response.status, status);
        assert_eq!(response.body["error"]["error_code"], error_code);
        assert_eq!(response.body["error"]["phase"], "mcp");
        assert_eq!(response.body["error"]["cause"], cause);
        assert_eq!(response.body["error"]["http_status"], status);
    }

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    write_empty(&mut server, 204, None).unwrap();
    drop(server);
    let response = parse_client_response(client);
    assert_eq!(response.status, 204);
    assert!(response.body.is_null());
}

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SUSPEND_RESUME};

use super::super::runtime::{CodingToolsPermissionMode, CodingToolsRuntimeConfig, InternalBearer};

const SYNTHETIC_BEARER: &str = "LB009_PEP_INTERNAL_BEARER_SYNTHETIC_DO_NOT_LEAK";

#[cfg(windows)]
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSuspendProcess(process_handle: HANDLE) -> i32;
    fn NtResumeProcess(process_handle: HANDLE) -> i32;
}

#[cfg(windows)]
struct SuspendedProcess {
    handle: HANDLE,
}

#[cfg(windows)]
impl SuspendedProcess {
    fn suspend(pid: u32) -> Self {
        let handle = unsafe { OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid) };
        assert!(
            !handle.is_null(),
            "OpenProcess(PROCESS_SUSPEND_RESUME) failed for {pid}"
        );
        let status = unsafe { NtSuspendProcess(handle) };
        assert_eq!(
            status, 0,
            "NtSuspendProcess failed with NTSTATUS {status:#x}"
        );
        Self { handle }
    }
}

#[cfg(windows)]
impl Drop for SuspendedProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = NtResumeProcess(self.handle);
            let _ = CloseHandle(self.handle);
        }
    }
}

#[test]
fn durable_task_terminal_ignores_newer_unrelated_command() {
    let workspace = temp_workspace();
    let store = ExecutionRegistry::open_at(workspace.join("owned-terminal.json")).unwrap();
    let a = store
        .start(
            TaskId::new("workflow-a"),
            PublicSessionId::new("lb-session-a"),
        )
        .unwrap();
    store
        .finish(
            &a,
            ExecutionTerminal {
                outcome: TerminalOutcome::Completed,
                exit_code: Some(0),
                signal: None,
                output_refs: vec!["lb-output-a".into()],
                error_code: None,
                completed_at_ms: unix_time_ms(),
            },
        )
        .unwrap();
    let b = store
        .start(
            TaskId::new("direct-b"),
            PublicSessionId::new("lb-session-b"),
        )
        .unwrap();
    store
        .finish(
            &b,
            ExecutionTerminal {
                outcome: TerminalOutcome::TimedOut,
                exit_code: None,
                signal: None,
                output_refs: vec!["lb-output-b".into()],
                error_code: Some("ProcessTimedOut".into()),
                completed_at_ms: unix_time_ms(),
            },
        )
        .unwrap();
    let owned_terminal = store.latest_terminal_for_task(&TaskId::new("workflow-a"));
    let data = latest_registry_activity(None, owned_terminal.as_ref()).unwrap();
    assert_eq!(data["task_id"], "workflow-a");
    assert_eq!(data["session_id"], "lb-session-a");
    cleanup_test_directory(&workspace);
}

#[test]
fn task_control_get_reads_durable_terminal_without_private_session() {
    let workspace = temp_workspace();
    let path = workspace.join("durable-command-state.json");
    {
        let store = ExecutionRegistry::open_at(path.clone()).unwrap();
        let execution = store
            .start(
                TaskId::new("task-durable"),
                PublicSessionId::new("lb-session-durable"),
            )
            .unwrap();
        store
            .finish(
                &execution,
                ExecutionTerminal {
                    outcome: TerminalOutcome::TimedOut,
                    exit_code: Some(124),
                    signal: Some("TERM".to_string()),
                    output_refs: vec!["lb-output-durable".to_string()],
                    error_code: Some("ProcessTimedOut".to_string()),
                    completed_at_ms: unix_time_ms(),
                },
            )
            .unwrap();
    }

    // Reopen from disk: there is deliberately no private runtime/session object here.
    let reopened = ExecutionRegistry::open_at(path).unwrap();
    let data = merge_control_plane_activity(
        json!({"state":"idle"}),
        &TaskRegistry::default(),
        &reopened,
        &Scheduler::default(),
    );
    let terminal = &data["last_activity"];
    assert_eq!(terminal["task_id"], "task-durable");
    assert_eq!(terminal["session_id"], "lb-session-durable");
    assert_eq!(terminal["outcome"], "timed_out");
    assert_eq!(terminal["exit_code"], 124);
    assert_eq!(terminal["output_refs"][0], "lb-output-durable");
    assert_eq!(terminal["error_code"], "ProcessTimedOut");
    cleanup_test_directory(&workspace);
}

#[test]
fn completed_foreground_task_restores_detached_execution_projection() {
    let workspace = temp_workspace();
    let executions =
        ExecutionRegistry::open_at(workspace.join("projection-executions.json")).unwrap();
    let execution_a = executions
        .start(TaskId::new("task-a"), PublicSessionId::new("session-a"))
        .unwrap();
    let tasks = TaskRegistry::default();
    let owner = McpSessionId::new("mcp-b");
    let task_b = tasks.queue(
        owner.clone(),
        RequestKey::new(owner, RpcRequestId::Number(2)),
        TaskKind::ReadFile,
        SafeTaskSummary::from_untrusted("read file"),
    );
    tasks.mark_running(&task_b).unwrap();

    let base = json!({"state":"idle","current_workflow":null});
    let scheduler = Scheduler::default();
    let foreground = merge_control_plane_activity(base.clone(), &tasks, &executions, &scheduler);
    assert_eq!(
        foreground["current_activity"]["task_id"],
        task_b.to_string()
    );
    assert_eq!(foreground["current_activity"]["kind"], "read");

    tasks.finish(&task_b, TerminalOutcome::Completed).unwrap();
    let restored = merge_control_plane_activity(base, &tasks, &executions, &scheduler);
    assert_eq!(
        restored["current_activity"]["execution_id"],
        execution_a.to_string()
    );
    assert_eq!(restored["current_activity"]["state"], "running");
    assert!(restored.get("current_command").is_none());
    cleanup_test_directory(&workspace);
}

#[test]
fn current_task_projection_is_a_read_only_task_registry_view() {
    let tasks = TaskRegistry::default();
    let projection = CurrentTaskProjection::new(tasks.clone(), None);
    let initial = projection.timing_snapshot();
    assert_eq!(initial.status, CurrentTaskStatus::Idle);
    assert_eq!(initial.elapsed_ms, None);
    assert_eq!(initial.last_tool, None);

    let owner = McpSessionId::new("projection-owner");
    let task_id = tasks.queue(
        owner.clone(),
        RequestKey::new(owner, RpcRequestId::Number(1)),
        TaskKind::ModifyFile,
        SafeTaskSummary::from_untrusted("write probe.txt"),
    );
    assert_eq!(projection.snapshot(), CurrentTaskStatus::Idle);
    tasks.mark_running(&task_id).unwrap();
    assert!(matches!(
        projection.snapshot(),
        CurrentTaskStatus::Active(_)
    ));
    tasks.finish(&task_id, TerminalOutcome::Completed).unwrap();
    let finished = projection.timing_snapshot();
    assert_eq!(finished.status, CurrentTaskStatus::Idle);
    assert_eq!(finished.elapsed_ms, None);
    assert_eq!(
        finished.last_tool.as_ref().map(|tool| tool.kind),
        Some(TaskKind::ModifyFile)
    );
}

#[derive(Debug)]
struct FakePrivilegedExecution {
    state: RwLock<PrivilegeState>,
    starts: Mutex<Vec<ElevatedExecSpec>>,
    structured_filesystems: Mutex<Vec<AdministratorFilesystemSpec>>,
    structured_filesystem_results: Mutex<HashMap<String, AdministratorFilesystemResult>>,
    cancelled: AtomicBool,
    complete: AtomicBool,
}

impl FakePrivilegedExecution {
    fn active() -> Self {
        Self {
            state: RwLock::new(PrivilegeState::Active {
                broker_generation: crate::state::GenerationId::new(77),
            }),
            starts: Mutex::new(Vec::new()),
            structured_filesystems: Mutex::new(Vec::new()),
            structured_filesystem_results: Mutex::new(HashMap::new()),
            cancelled: AtomicBool::new(false),
            complete: AtomicBool::new(false),
        }
    }

    fn set_state(&self, state: PrivilegeState) {
        *self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
    }

    fn start_count(&self) -> usize {
        self.starts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    fn structured_filesystem_count(&self) -> usize {
        self.structured_filesystems
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

impl PrivilegedExecution for FakePrivilegedExecution {
    fn state(&self) -> PrivilegeState {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn start_execute(
        &self,
        _request_id: String,
        spec: ElevatedExecSpec,
    ) -> Result<(), PrivilegedExecError> {
        let state = self.state();
        if !state.accepts_privileged_calls() {
            return Err(PrivilegedExecError::GateClosed(state));
        }
        self.cancelled.store(false, Ordering::Release);
        self.starts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(spec);
        Ok(())
    }

    fn poll_execute(
        &self,
        _request_id: String,
    ) -> Result<Option<crate::privilege::ElevatedExecResult>, PrivilegedExecError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Ok(Some(crate::privilege::ElevatedExecResult {
                outcome: ElevatedExecOutcome::Cancelled,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                truncated: false,
            }));
        }
        if self.complete.load(Ordering::Acquire) {
            let large = self
                .starts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .last()
                .is_some_and(|spec| spec.max_output_bytes > 8 * 1024);
            return Ok(Some(crate::privilege::ElevatedExecResult {
                outcome: ElevatedExecOutcome::Completed,
                exit_code: Some(0),
                stdout: if large {
                    "O".repeat(9_000)
                } else {
                    "LB012_FAKE_PRIVILEGED_OK".to_string()
                },
                stderr: if large {
                    "E".repeat(9_000)
                } else {
                    "LB012_FAKE_PRIVILEGED_ERR".to_string()
                },
                stdout_truncated: false,
                stderr_truncated: false,
                truncated: false,
            }));
        }
        Ok(None)
    }

    fn cancel_execute(&self, _request_id: String) -> Result<(), PrivilegedExecError> {
        self.cancelled.store(true, Ordering::Release);
        Ok(())
    }

    fn filesystem(
        &self,
        spec: PrivilegedFilesystemSpec,
    ) -> Result<PrivilegedFilesystemResult, PrivilegedExecError> {
        let state = self.state();
        if !state.accepts_privileged_calls() {
            return Err(PrivilegedExecError::GateClosed(state));
        }
        Ok(PrivilegedFilesystemResult {
            action: spec.action,
            path: spec.path,
            destination: spec.destination,
            content_base64: spec.content_base64,
            bytes: 0,
        })
    }

    fn structured_filesystem(
        &self,
        spec: AdministratorFilesystemSpec,
    ) -> Result<AdministratorFilesystemResult, PrivilegedExecError> {
        let state = self.state();
        if !state.accepts_privileged_calls() {
            return Err(PrivilegedExecError::GateClosed(state));
        }
        self.structured_filesystems
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(spec.clone());
        Ok(match spec.action {
            AdministratorFilesystemAction::List | AdministratorFilesystemAction::Search => {
                AdministratorFilesystemResult::Entries {
                    action: spec.action,
                    entries: Vec::new(),
                    scanned_entries: 0,
                    truncated: false,
                }
            }
            AdministratorFilesystemAction::SearchContent => {
                AdministratorFilesystemResult::ContentMatches {
                    action: spec.action,
                    matches: Vec::new(),
                    scanned_entries: 0,
                    scanned_files: 0,
                    skipped_binary_files: 0,
                    skipped_oversized_files: 0,
                    truncated: false,
                }
            }
            AdministratorFilesystemAction::Stat => AdministratorFilesystemResult::Stat {
                path: spec.path.unwrap(),
                kind: "file".into(),
                size: 0,
                modified_ms: None,
                calculated_size: spec.calculate_size,
                scanned_entries: 0,
                truncated: false,
            },
            AdministratorFilesystemAction::Read => AdministratorFilesystemResult::Read {
                path: spec.path.unwrap(),
                offset: spec.offset,
                total_bytes: 15,
                returned_bytes: 15,
                eof: true,
                encoding: "utf8".into(),
                content: "LB43_FAKE_ADMIN".into(),
            },
            AdministratorFilesystemAction::Write
            | AdministratorFilesystemAction::Copy
            | AdministratorFilesystemAction::Move
            | AdministratorFilesystemAction::Delete => AdministratorFilesystemResult::Mutation {
                action: spec.action,
                path: spec.path.or(spec.source).unwrap(),
                destination: spec.destination,
                bytes: 0,
                changed: true,
            },
            AdministratorFilesystemAction::Replace | AdministratorFilesystemAction::Patch => {
                AdministratorFilesystemResult::Edit {
                    action: spec.action,
                    path: spec.path,
                    affected_files: Vec::new(),
                    sha256: (spec.action == AdministratorFilesystemAction::Replace)
                        .then(|| "0".repeat(64)),
                    changed: true,
                }
            }
            AdministratorFilesystemAction::Hash => AdministratorFilesystemResult::Hash {
                path: spec.path.unwrap(),
                algorithm: "sha256".into(),
                sha256: "0".repeat(64),
                bytes: 0,
            },
        })
    }

    fn start_structured_filesystem(
        &self,
        request_id: String,
        spec: AdministratorFilesystemSpec,
    ) -> Result<(), PrivilegedExecError> {
        let result = self.structured_filesystem(spec)?;
        self.structured_filesystem_results
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(request_id, result);
        Ok(())
    }

    fn poll_structured_filesystem(
        &self,
        request_id: String,
    ) -> Result<
        Option<Result<AdministratorFilesystemResult, AdministratorFilesystemErrorCode>>,
        PrivilegedExecError,
    > {
        Ok(self
            .structured_filesystem_results
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&request_id)
            .map(Ok))
    }

    fn cancel_structured_filesystem(&self, request_id: String) -> Result<(), PrivilegedExecError> {
        self.structured_filesystem_results
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&request_id);
        Ok(())
    }
}

#[test]
fn workspace_authority_separates_desired_observed_and_effective_permission() {
    let fake = FakePrivilegedExecution::active();
    fake.set_state(PrivilegeState::Requested);
    let privileged: Arc<dyn PrivilegedExecution> = Arc::new(fake);
    let tasks = TaskRegistry::default();
    let workspace = temp_workspace();
    let executions =
        ExecutionRegistry::open_at(workspace.join("authority-executions.json")).unwrap();
    let scheduler = Scheduler::default();
    let control = |mode| {
        let projected =
            crate::control_plane::convergence::derive_authority(mode, &privileged.state());
        PolicyControlState {
            revision: 7,
            authority: AuthorityProjection {
                desired: projected.configured,
                effective: projected.execution,
                broker: privileged.state(),
                structured_paths: projected.structured_paths,
                reconciliation: projected.reconciliation,
            },
            work_authorized: true,
            runtime: Some(RuntimeState::Ready),
        }
    };

    let mut full = stable_success(json!({"current_task":{"state":"idle"}}), "context");
    let full_control = control(PermissionMode::Full);
    enrich_workspace_context_privilege(&mut full, &full_control, &tasks, &executions, &scheduler);
    let full_data = &full["structuredContent"]["data"];
    assert_eq!(full_data["privilege_state"], "disabled");
    assert_eq!(full_data["authority"]["desired_permission"], "full");
    assert_eq!(full_data["authority"]["observed_privilege"], "requested");
    assert_eq!(full_data["authority"]["effective_permission"], "full");
    assert_eq!(full_data["authority"]["reconciliation"], "disable_pending");
    assert_eq!(full_data["elevated_route_available"], false);

    let mut elevated = stable_success(json!({"current_task":{"state":"idle"}}), "context");
    let elevated_control = control(PermissionMode::Elevated);
    enrich_workspace_context_privilege(
        &mut elevated,
        &elevated_control,
        &tasks,
        &executions,
        &scheduler,
    );
    let elevated_data = &elevated["structuredContent"]["data"];
    assert_eq!(elevated_data["privilege_state"], "requested");
    assert_eq!(elevated_data["authority"]["effective_permission"], "full");
    assert_eq!(
        elevated_data["authority"]["reconciliation"],
        "awaiting_authorization"
    );
    assert_eq!(elevated_data["elevated_route_available"], false);
    cleanup_test_directory(&workspace);
}

#[test]
fn r1_transport_cancel_lookup_is_scoped_by_mcp_session() {
    let requests = RequestRegistry::default();
    let raw_id = RpcRequestId::Number(1);
    let session_a = McpSessionId::new("session-a");
    let session_b = McpSessionId::new("session-b");
    requests
        .register(
            RequestKey::new(session_a.clone(), raw_id.clone()),
            RequestCancellationTarget::Runtime(RpcRequestId::String("upstream-a".into())),
        )
        .unwrap();
    requests
        .register(
            RequestKey::new(session_b.clone(), raw_id.clone()),
            RequestCancellationTarget::Runtime(RpcRequestId::String("upstream-b".into())),
        )
        .unwrap();

    let selected_a =
        registered_request_for_transport_cancel(&requests, &session_a, &raw_id).unwrap();
    let selected_b =
        registered_request_for_transport_cancel(&requests, &session_b, &raw_id).unwrap();
    assert!(matches!(
        selected_a.cancellation,
        RequestCancellationTarget::Runtime(RpcRequestId::String(ref value)) if value == "upstream-a"
    ));
    assert!(matches!(
        selected_b.cancellation,
        RequestCancellationTarget::Runtime(RpcRequestId::String(ref value)) if value == "upstream-b"
    ));
}

#[test]
fn schema42_special_handler_abort_closes_request_diagnostics() {
    crate::diagnostics::reset_request_diagnostics_for_test();
    record_mcp_request_start("special-abort", "session-special", "task_control");
    assert!(finalize_special_handler_request("special-abort", "session-special", Err(())).is_err());
    let events = crate::diagnostics::request_diagnostics_for_test();
    let end = events
        .iter()
        .find(|event| event.kind == crate::diagnostics::RequestDiagnosticKind::End)
        .expect("special handler abort terminal diagnostic");
    assert_eq!(end.outcome.as_deref(), Some("failed"));
    assert_eq!(end.error_code.as_deref(), Some("Unknown"));
    assert_eq!(end.phase.as_deref(), Some("mcp"));
    assert_eq!(end.cause.as_deref(), Some("special_handler_aborted"));
}

#[test]
fn schema43_response_diagnostics_finalize_after_transport_delivery() {
    use std::sync::atomic::{AtomicBool, Ordering};

    crate::diagnostics::reset_request_diagnostics_for_test();
    record_mcp_request_start("response-ok", "session-response", "filesystem");
    let delivered = AtomicBool::new(false);
    let result = stable_success(json!({"changed":true}), "done");
    assert!(
        finalize_response_diagnostic("response-ok", "session-response", Ok(()), || {
            delivered.store(true, Ordering::Release);
            record_mcp_request_result("response-ok", "session-response", &result);
        })
        .is_ok()
    );
    assert!(delivered.load(Ordering::Acquire));
    let events = crate::diagnostics::request_diagnostics_for_test();
    let success = events
        .iter()
        .find(|event| event.kind == crate::diagnostics::RequestDiagnosticKind::End)
        .expect("delivered response terminal diagnostic");
    assert_eq!(success.outcome.as_deref(), Some("success"));

    crate::diagnostics::reset_request_diagnostics_for_test();
    record_mcp_request_start("response-fail", "session-response", "filesystem");
    let delivered = AtomicBool::new(false);
    assert!(
        finalize_response_diagnostic("response-fail", "session-response", Err(()), || {
            delivered.store(true, Ordering::Release);
            record_mcp_request_result("response-fail", "session-response", &result);
        })
        .is_err()
    );
    assert!(!delivered.load(Ordering::Acquire));
    let events = crate::diagnostics::request_diagnostics_for_test();
    let failed = events
        .iter()
        .find(|event| event.kind == crate::diagnostics::RequestDiagnosticKind::End)
        .expect("failed response transport diagnostic");
    assert_eq!(failed.outcome.as_deref(), Some("failed"));
    assert_eq!(failed.phase.as_deref(), Some("transport"));
    assert_eq!(failed.cause.as_deref(), Some("response_write_failure"));
}

#[test]
fn downstream_mcp_sessions_coexist_and_same_catalog_policy_replacement_preserves_them() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("multi-session PEP ready");

    let session_a = initialize(pep.port(), 640)
        .session
        .expect("downstream MCP session A");
    let session_b = initialize(pep.port(), 641)
        .session
        .expect("downstream MCP session B");
    assert_ne!(session_a, session_b);

    for (id, session) in [(642, &session_a), (643, &session_b)] {
        assert_eq!(
            post(
                pep.port(),
                Some(session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );
        let response = public_tool_call(
            pep.port(),
            session,
            id,
            "workspace_context",
            json!({"detail":"compact"}),
        );
        assert_eq!(response.status, 200, "session {session} must remain live");
        assert_eq!(response.body["result"]["isError"], false);
    }

    pep.replace_policy(policy(&root))
        .expect("same-catalog policy replacement");
    for (id, session) in [(644, &session_a), (645, &session_b)] {
        let response = public_tool_call(
            pep.port(),
            session,
            id,
            "workspace_context",
            json!({"detail":"compact"}),
        );
        assert_eq!(
            response.status, 200,
            "same catalog replacement must not invalidate session {session}"
        );
        assert_eq!(response.body["result"]["isError"], false);
    }

    let _coding = pep.stop().unwrap();
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn desired_workspace_change_denies_work_until_runtime_observes_the_same_workspace() {
    let root = repo_root();
    let workspace_a = temp_workspace();
    let workspace_b = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace_a,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("workspace convergence PEP ready");
    let session = initialize(pep.port(), 650)
        .session
        .expect("downstream MCP session");

    pep.test_desired_state
        .as_ref()
        .expect("simulated desired state")
        .set_workspace(Some(DesiredWorkspace::for_runtime_path(&workspace_b)));
    let denied = public_tool_call(
        pep.port(),
        &session,
        651,
        "filesystem",
        json!({"action":"read_file","path":"probe.txt"}),
    );
    assert_tool_error(&denied, "RuntimeUnavailable");
    assert_eq!(pep.control_plane.scheduler().snapshot().work_running, 0);
    assert_eq!(pep.control_plane.scheduler().snapshot().work_queued, 0);

    let _coding = pep.stop().unwrap();
    let _ = fs::remove_dir_all(workspace_a);
    let _ = fs::remove_dir_all(workspace_b);
}

#[test]
fn full_cmd_rmdir_uses_current_user_authority_inside_and_outside_workspace() {
    let root = repo_root();
    let workspace = temp_workspace();
    let probe = workspace.join("test").join("document_workflow_probe");
    fs::create_dir_all(probe.join("nested")).unwrap();
    fs::write(probe.join("nested").join("probe.txt"), b"cleanup").unwrap();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("rmdir PEP ready");
    let session = initialize(pep.port(), 650)
        .session
        .expect("downstream MCP session");
    assert_eq!(
        post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .status,
        202
    );

    let response = public_tool_call(
        pep.port(),
        &session,
        651,
        "exec_command",
        json!({
            "shell":"cmd",
            "command":r"rmdir /s /q test\document_workflow_probe",
            "workdir":".",
            "yield_time_ms":10_000,
            "timeout_ms":30_000
        }),
    );
    assert_eq!(response.status, 200, "{:#?}", response.body);
    assert_eq!(
        response.body["result"]["isError"], false,
        "{:#?}",
        response.body
    );
    assert_eq!(
        response.body["result"]["structuredContent"]["data"]["status"], "completed",
        "{:#?}",
        response.body
    );
    assert!(
        !probe.exists(),
        "rmdir must remove the workspace probe tree"
    );

    let outside = workspace.with_extension("outside-rmdir-probe");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), b"keep").unwrap();
    let outside_response = public_tool_call(
        pep.port(),
        &session,
        652,
        "exec_command",
        json!({
            "shell":"cmd",
            "command":format!(r#"rmdir /s /q "{}""#, outside.display()),
            "workdir":".",
            "yield_time_ms":10_000,
            "timeout_ms":30_000
        }),
    );
    assert_eq!(outside_response.status, 200, "{:#?}", outside_response.body);
    assert_eq!(
        outside_response.body["result"]["isError"], false,
        "{:#?}",
        outside_response.body
    );
    assert!(
        !outside.exists(),
        "ordinary shell descendants must retain the same current-user authority"
    );

    let _coding = pep.stop().unwrap();
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn schema27_public_facade_runtime_semantics_are_real_end_to_end() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("schema27 PEP ready");
    let initialized = initialize(pep.port(), 600);
    let session = initialized.session.expect("downstream MCP session");
    assert_eq!(
        post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .status,
        202
    );

    crate::diagnostics::reset_request_diagnostics_for_test();
    let context = public_tool_call(pep.port(), &session, 601, "workspace_context", json!({}));
    let request_events = crate::diagnostics::request_diagnostics_for_test();
    let request_start = request_events
        .iter()
        .find(|event| {
            event.kind == crate::diagnostics::RequestDiagnosticKind::Start
                && event.tool == "workspace_context"
                && event.connection_id == session
        })
        .expect("real tools/call request start diagnostic");
    let request_end = request_events
        .iter()
        .find(|event| {
            event.kind == crate::diagnostics::RequestDiagnosticKind::End
                && event.connection_id == session
        })
        .expect("real tools/call request end diagnostic");
    assert_eq!(request_start.request_id, request_end.request_id);
    assert_eq!(request_start.connection_id, session);
    assert_eq!(request_end.connection_id, session);
    assert_eq!(request_start.attempt, 1);
    assert_eq!(request_end.attempt, 1);
    assert_eq!(request_end.outcome.as_deref(), Some("success"));
    let projected_workspace = context.body["result"]["structuredContent"]["data"]["workspace"]
        .as_str()
        .expect("workspace projection");
    assert!(!projected_workspace.is_empty());
    assert!(!projected_workspace.starts_with(r"\\?\"));
    assert_eq!(
        PathBuf::from(projected_workspace).canonicalize().unwrap(),
        workspace.canonicalize().unwrap()
    );
    assert_eq!(
        context.body["result"]["structuredContent"]["data"]["default_cwd"],
        "."
    );

    crate::diagnostics::reset_request_diagnostics_for_test();
    let invalid_task_control =
        public_tool_call(pep.port(), &session, 6999, "task_control", json!({}));
    assert_ne!(invalid_task_control.body, Value::Null);
    let task_events = crate::diagnostics::request_diagnostics_for_test();
    let task_start = task_events
        .iter()
        .find(|event| {
            event.kind == crate::diagnostics::RequestDiagnosticKind::Start
                && event.tool == "task_control"
                && event.connection_id == session
        })
        .expect("task_control missing-action start diagnostic");
    let task_end = task_events
        .iter()
        .find(|event| {
            event.kind == crate::diagnostics::RequestDiagnosticKind::End
                && event.connection_id == session
        })
        .expect("task_control missing-action end diagnostic");
    assert_eq!(task_start.request_id, task_end.request_id);
    assert!(task_end.outcome.is_some());

    let absolute = public_tool_call(
        pep.port(),
        &session,
        602,
        "document_workflow",
        json!({"action":"inspect","path":workspace.join("probe.txt").to_string_lossy()}),
    );
    assert_eq!(
        absolute.body["result"]["isError"], false,
        "absolute in-workspace document input must match its relative form: {:#?}",
        absolute.body
    );

    let nonzero = public_tool_call(
        pep.port(),
        &session,
        603,
        "exec_command",
        json!({"command":"exit /b 7","shell":"cmd","yield_time_ms":10000}),
    );
    assert_eq!(nonzero.body["result"]["isError"], true);
    assert_eq!(
        nonzero.body["result"]["structuredContent"]["error"]["code"],
        "ProcessFailed"
    );
    assert_eq!(
        nonzero.body["result"]["structuredContent"]["data"]["exit_code"],
        7
    );

    let running = public_tool_call(
        pep.port(),
        &session,
        604,
        "exec_command",
        json!({
            "command":"Start-Sleep -Milliseconds 900; Write-Output LB_SCHEMA27_DONE",
            "shell":"windows_powershell",
            "yield_time_ms":0
        }),
    );
    assert_eq!(
        running.body["result"]["structuredContent"]["data"]["status"],
        "running"
    );
    let public_session = running.body["result"]["structuredContent"]["data"]["session_id"]
        .as_str()
        .expect("public session id")
        .to_string();
    assert!(public_session.starts_with("lb-session-"));
    assert!(
        !serde_json::to_string(&running.body)
            .unwrap()
            .contains("session:lb-session-")
    );

    let polled = public_tool_call(
        pep.port(),
        &session,
        605,
        "command_control",
        json!({"action":"poll","session_id":public_session,"wait_ms":25}),
    );
    assert!(polled.body.get("error").is_none(), "{:#?}", polled.body);

    let terminal_deadline = Instant::now() + Duration::from_secs(30);
    let mut terminal_poll_id = 606u64;
    let mut observed_output = String::new();
    let terminal = loop {
        let poll = public_tool_call(
            pep.port(),
            &session,
            terminal_poll_id,
            "command_control",
            json!({"action":"poll","session_id":public_session,"wait_ms":100}),
        );
        terminal_poll_id += 1;
        assert!(poll.body.get("error").is_none(), "{:#?}", poll.body);
        observed_output.push_str(
            poll.body["result"]["structuredContent"]["data"]["output"]
                .as_str()
                .unwrap_or_default(),
        );
        if poll.body["result"]["structuredContent"]["data"]["status"] != "running" {
            break poll;
        }
        assert!(
            Instant::now() < terminal_deadline,
            "schema27 command failed to converge to terminal state: {:#?}",
            poll.body
        );
        thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        terminal.body["result"]["structuredContent"]["data"]["status"], "completed",
        "{:#?}",
        terminal.body
    );
    assert!(
        observed_output.contains("LB_SCHEMA27_DONE"),
        "schema27 incremental output missing: {observed_output:?}"
    );

    if let Some(output_ref) =
        terminal.body["result"]["structuredContent"]["data"]["output_refs"]["stdout"].as_str()
    {
        assert!(output_ref.starts_with("lb-output-"));
        let read = public_tool_call(
            pep.port(),
            &session,
            607,
            "command_control",
            json!({"action":"read","output_ref":output_ref,"stream":"stdout","offset":0,"limit":4096}),
        );
        assert!(
            read.body["result"]["structuredContent"]["data"]["content"]
                .as_str()
                .unwrap_or_default()
                .contains("LB_SCHEMA27_DONE")
        );
    }

    let task = public_tool_call(
        pep.port(),
        &session,
        608,
        "task_control",
        json!({"action":"get"}),
    );
    assert_eq!(task.body["result"]["structuredContent"]["ok"], true);
    for field in [
        "ok",
        "state",
        "summary",
        "task_id",
        "warnings",
        "next_step",
        "output_refs",
        "data",
        "error",
    ] {
        assert!(
            task.body["result"]["structuredContent"]
                .get(field)
                .is_some(),
            "task_control real MCP response lost schema41 envelope field {field}: {:#?}",
            task.body
        );
    }
    assert!(matches!(
        task.body["result"]["structuredContent"]["data"]["state"].as_str(),
        Some("idle") | Some("active")
    ));

    let create = public_tool_call(
        pep.port(),
        &session,
        609,
        "document_workflow",
        json!({"action":"create","path":"schema27.txt","content":"alpha\nbeta\n"}),
    );
    assert_eq!(
        create.body["result"]["structuredContent"]["ok"], true,
        "{:#?}",
        create.body
    );
    let inspect = public_tool_call(
        pep.port(),
        &session,
        610,
        "document_workflow",
        json!({"action":"inspect","path":"schema27.txt"}),
    );
    assert_eq!(
        inspect.body["result"]["structuredContent"]["data"]["text"],
        "alpha\nbeta\n"
    );
    let document_sha256 = inspect.body["result"]["structuredContent"]["data"]["sha256"]
        .as_str()
        .expect("inspect returns the mutation identity")
        .to_string();
    let rebuild = public_tool_call(
        pep.port(),
        &session,
        611,
        "document_workflow",
        json!({"action":"rebuild","path":"schema27.txt","content":"gamma\ndelta\n","expected_sha256":document_sha256}),
    );
    assert_eq!(
        rebuild.body["result"]["structuredContent"]["ok"], true,
        "{:#?}",
        rebuild.body
    );
    let rebuilt = public_tool_call(
        pep.port(),
        &session,
        612,
        "document_workflow",
        json!({"action":"inspect","path":"schema27.txt"}),
    );
    assert_eq!(
        rebuilt.body["result"]["structuredContent"]["data"]["text"],
        "gamma\ndelta\n"
    );
    let convert = public_tool_call(
        pep.port(),
        &session,
        613,
        "document_workflow",
        json!({"action":"convert","source":"schema27.txt","path":"schema27-copy.txt"}),
    );
    assert_eq!(
        convert.body["result"]["structuredContent"]["ok"], true,
        "{:#?}",
        convert.body
    );
    let converted = public_tool_call(
        pep.port(),
        &session,
        614,
        "document_workflow",
        json!({"action":"inspect","path":"schema27-copy.txt"}),
    );
    assert_eq!(
        converted.body["result"]["structuredContent"]["data"]["text"],
        "gamma\ndelta\n"
    );

    let diagnose = public_tool_call(
        pep.port(),
        &session,
        615,
        "agent_workflow",
        json!({"action":"diagnose","objective":"schema27 context"}),
    );
    assert_eq!(
        diagnose.body["result"]["structuredContent"]["data"]["state"], "context_ready",
        "{:#?}",
        diagnose.body
    );
    let diagnose_command = public_tool_call(
        pep.port(),
        &session,
        616,
        "agent_workflow",
        json!({
            "action":"diagnose",
            "objective":"schema27 diagnose command",
            "commands":[{"command":"echo LB_SCHEMA27_DIAGNOSE","shell":"cmd","workdir":".","yield_time_ms":10000}]
        }),
    );
    assert_eq!(
        diagnose_command.body["result"]["isError"], false,
        "{:#?}",
        diagnose_command.body
    );
    assert_eq!(
        diagnose_command.body["result"]["structuredContent"]["data"]["state"],
        "completed"
    );
    assert!(
        diagnose_command.body["result"]["structuredContent"]["data"]["commands"][0]["output"]
            .as_str()
            .unwrap_or_default()
            .contains("LB_SCHEMA27_DIAGNOSE")
    );
    let executable_workflow = public_tool_call(
        pep.port(),
        &session,
        617,
        "agent_workflow",
        json!({
            "action":"bugfix",
            "objective":"schema27 executable orchestration",
            "commands":[{"command":"echo LB_SCHEMA27_AGENT","shell":"cmd","workdir":".","yield_time_ms":10000}]
        }),
    );
    assert_eq!(
        executable_workflow.body["result"]["structuredContent"]["data"]["state"], "completed",
        "{:#?}",
        executable_workflow.body
    );
    assert!(
        executable_workflow.body["result"]["structuredContent"]["data"]["commands"][0]["output"]
            .as_str()
            .unwrap_or_default()
            .contains("LB_SCHEMA27_AGENT")
    );

    let mut coding = pep.stop().expect("schema27 PEP stops");
    coding.stop().expect("schema27 Coding Tools runtime stops");
    cleanup_test_directory(&workspace);
}

#[test]
fn schema28_public_runtime_behavior_is_real_end_to_end() {
    use base64::Engine as _;

    let workspace = temp_workspace();
    let nested_project = workspace.join("NestedProject");
    fs::create_dir_all(nested_project.join("src")).unwrap();
    let git_init = std::process::Command::new("git")
        .arg("init")
        .arg("--quiet")
        .current_dir(&nested_project)
        .status()
        .expect("git is available for nested-project schema30 fixture");
    assert!(git_init.success());
    for arguments in [
        &["config", "user.email", "localbridge-test@example.invalid"][..],
        &["config", "user.name", "LocalBridge Test"][..],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(arguments)
                .current_dir(&nested_project)
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(nested_project.join("AGENTS.md"), b"before\n").unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["add", "AGENTS.md"])
            .current_dir(&nested_project)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args(["commit", "--quiet", "-m", "fixture"])
            .current_dir(&nested_project)
            .status()
            .unwrap()
            .success()
    );
    fs::write(nested_project.join("AGENTS.md"), b"after\n").unwrap();
    fs::write(
        workspace.join("range.txt"),
        b"line1\nline2\nline3\nline4\nline5\nline6\n",
    )
    .unwrap();
    image::RgbaImage::from_pixel(1024, 1024, image::Rgba([19, 37, 53, 255]))
        .save(workspace.join("large.png"))
        .unwrap();
    let module_root = workspace.join("modules");
    let module_dir = module_root.join("Invoke-LbGen14Auto");
    fs::create_dir_all(&module_dir).unwrap();
    fs::write(
        module_dir.join("Invoke-LbGen14Auto.psm1"),
        b"function Invoke-LbGen14Auto { Write-Output 'LB_GEN14_MODULE_AUTOLOAD_SENTINEL' }; Export-ModuleMember -Function Invoke-LbGen14Auto\n",
    )
    .unwrap();

    let fixture = PublicRuntimeFixture::start_in(workspace.clone(), PermissionMode::Full);
    let pep = fixture.runtime();
    let initialized = initialize(pep.port(), 700);
    assert_eq!(
        initialized.body["result"]["capabilities"]["tools"]["listChanged"], true,
        "schema39 must explicitly advertise tool-schema change capability: {:#?}",
        initialized.body
    );
    assert!(
        initialized.body["result"]["serverInfo"]["version"]
            .as_str()
            .is_some_and(|value| value.ends_with(&format!("+api{AGENT_API_REVISION}"))),
        "serverInfo version must track the current LocalBridge API revision and invalidate stale downstream metadata: {:#?}",
        initialized.body
    );
    let session = initialized.session.expect("schema28 downstream session");
    let first_refresh = get_sse(pep.port(), &session);
    assert_eq!(
        first_refresh.status, 200,
        "schema39 first GET did not deliver refresh"
    );
    assert_eq!(first_refresh.session.as_deref(), Some(session.as_str()));
    assert_eq!(
        first_refresh.content_type.as_deref(),
        Some("text/event-stream")
    );
    let first_refresh_body = String::from_utf8(first_refresh.body).unwrap();
    assert!(
        first_refresh_body.contains("event: message")
            && first_refresh_body.contains("notifications/tools/list_changed"),
        "schema39 refresh event missing: {first_refresh_body}"
    );
    let second_refresh = get_sse(pep.port(), &session);
    assert_eq!(
        second_refresh.status, 204,
        "schema39 refresh was not one-shot"
    );
    assert!(second_refresh.body.is_empty());
    assert_eq!(
        post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .status,
        202
    );
    let post_refresh_tools = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":686,"method":"tools/list","params":{}}),
    );
    assert_eq!(post_refresh_tools.status, 200);
    assert!(post_refresh_tools.body["result"]["tools"].is_array());

    let provenance = public_tool_call(pep.port(), &session, 687, "workspace_context", json!({}));
    assert_eq!(
        provenance.body["result"]["structuredContent"]["data"]["facade_revision"],
        AGENT_API_REVISION,
        "fresh serving instance did not identify the current LocalBridge facade revision: {:#?}",
        provenance.body
    );
    let first_turn = &provenance.body["result"]["structuredContent"]["data"];
    for field in [
        "project_name",
        "project_type",
        "project_version",
        "git_branch",
        "git_dirty",
        "git_changed_count",
        "package_manager",
        "build_system",
        "test_system",
        "runtime_availability",
        "trusted_shells",
        "current_task",
    ] {
        assert!(
            first_turn.get(field).is_some(),
            "schema39 first-turn context lost {field}: {first_turn:#?}"
        );
    }
    let served_tools = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":688,"method":"tools/list","params":{}}),
    );
    let served_agent = served_tools.body["result"]["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|tool| tool["name"] == "agent_workflow"))
        .expect("fresh serving instance exposes agent_workflow");
    assert!(served_agent["inputSchema"]["properties"]["path"].is_object());
    assert_eq!(
        served_agent["inputSchema"]["properties"]["adoption_token"]["type"],
        "string"
    );
    assert_eq!(
        served_agent["inputSchema"]["properties"]["adoption_token"]["minLength"],
        1
    );
    assert_eq!(served_agent["outputSchema"]["type"], "object");
    assert_eq!(
        served_agent["outputSchema"]["properties"]["ok"]["type"],
        "boolean"
    );
    let agent_data_schema = served_agent["outputSchema"]["properties"]["data"]["anyOf"]
        .as_array()
        .and_then(|branches| branches.iter().find(|branch| branch["type"] == "object"))
        .expect("agent_workflow nullable data keeps an object domain branch");
    assert_eq!(agent_data_schema["properties"]["state"]["type"], "string");
    assert!(
        served_agent["outputSchema"]["properties"]["error"]["anyOf"]
            .as_array()
            .is_some_and(|branches| branches.iter().any(|branch| branch["type"] == "object")),
        "agent_workflow nullable error must retain a typed object branch"
    );
    let served_command_control = served_tools.body["result"]["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|tool| tool["name"] == "command_control"))
        .expect("fresh serving instance exposes command_control");
    assert_eq!(served_command_control["inputSchema"]["type"], "object");
    assert!(served_command_control["inputSchema"].get("oneOf").is_none());
    assert_eq!(
        served_command_control["inputSchema"]["properties"]["action"]["enum"],
        json!(["adopt", "poll", "read", "write", "kill"])
    );
    assert!(
        first_turn["capabilities"].get("actions").is_none(),
        "workspace_context must not publish a second action catalog"
    );
    for property in [
        "session_id",
        "output_ref",
        "chars",
        "signal",
        "wait_ms",
        "stream",
        "offset",
        "limit",
    ] {
        assert!(
            served_command_control["inputSchema"]["properties"][property].is_object(),
            "served command_control schema lost {property}"
        );
    }
    let served_task_control = served_tools.body["result"]["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|tool| tool["name"] == "task_control"))
        .expect("fresh serving instance exposes task_control");
    assert_eq!(
        served_task_control["inputSchema"]["properties"]["action"]["enum"],
        json!(["list", "get", "cancel"])
    );

    let served_document = served_tools.body["result"]["tools"]
        .as_array()
        .and_then(|tools| {
            tools
                .iter()
                .find(|tool| tool["name"] == "document_workflow")
        })
        .expect("fresh serving instance exposes document_workflow");
    assert_eq!(
        served_document["inputSchema"]["properties"]["action"]["enum"],
        json!(["inspect", "search", "create", "edit", "convert", "rebuild"])
    );
    assert_eq!(
        served_document["inputSchema"]["properties"]["expected_sha256"]["minLength"],
        64
    );
    assert_eq!(
        served_document["inputSchema"]["properties"]["edits"]["items"]["properties"]["operation"]["enum"],
        json!(["replace", "insert_before", "insert_after", "delete"])
    );
    let served_git = served_tools.body["result"]["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|tool| tool["name"] == "git_workflow"))
        .expect("fresh serving instance exposes git_workflow");
    assert!(
        served_git["inputSchema"]["properties"]["path"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("Required for blame"))
    );
    assert!(served_git["inputSchema"]["properties"]["include_patch"].is_object());

    let served_elevated = served_tools.body["result"]["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|tool| tool["name"] == "elevated_exec"))
        .expect("fresh serving instance exposes elevated_exec");
    assert_eq!(served_elevated["inputSchema"]["type"], "object");
    assert!(served_elevated["inputSchema"].get("oneOf").is_none());
    for property in [
        "operation",
        "program",
        "args",
        "shell",
        "command",
        "workdir",
        "action",
        "path",
        "destination",
        "content_base64",
        "recursive",
        "timeout_ms",
        "max_output_bytes",
    ] {
        assert!(
            served_elevated["inputSchema"]["properties"][property].is_object(),
            "served elevated_exec schema lost {property}"
        );
    }
    let invalid_control = public_tool_call(
        pep.port(),
        &session,
        6881,
        "command_control",
        json!({"action":"read","output_ref":"lb-output-missing","session_id":"lb-session-cross-action"}),
    );
    assert_eq!(
        invalid_control.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
        "cross-action command_control fields were not rejected: {:#?}",
        invalid_control.body
    );
    let missing_output = public_tool_call(
        pep.port(),
        &session,
        68811,
        "command_control",
        json!({"action":"read","output_ref":"lb-output-missing","stream":"stdout"}),
    );
    assert_eq!(
        missing_output.body["result"]["structuredContent"]["error"]["code"], "OutputNotFound",
        "an output handle lookup must not be classified as a command Session failure: {:#?}",
        missing_output.body
    );
    let invalid_git = public_tool_call(
        pep.port(),
        &session,
        6882,
        "git_workflow",
        json!({"action":"status","rev":"HEAD"}),
    );
    assert_eq!(
        invalid_git.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
        "cross-action git_workflow fields were not rejected: {:#?}",
        invalid_git.body
    );
    let invalid_document = public_tool_call(
        pep.port(),
        &session,
        6883,
        "document_workflow",
        json!({"action":"rebuild","path":"range.txt","content":"x","source":"range.txt"}),
    );
    assert_eq!(
        invalid_document.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
        "cross-action document_workflow fields were not rejected: {:#?}",
        invalid_document.body
    );
    let invalid_resume = public_tool_call(
        pep.port(),
        &session,
        6884,
        "agent_workflow",
        json!({"action":"resume","path":"."}),
    );
    assert_eq!(
        invalid_resume.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
        "resume accepted fields other than action: {:#?}",
        invalid_resume.body
    );
    let directory_schema = &served_agent["inputSchema"]["properties"]["directory_changes"];
    assert_eq!(directory_schema["type"], "array");
    assert_eq!(
        directory_schema["items"]["properties"]["action"]["enum"],
        json!(["create_directory", "remove_empty_directory"])
    );

    let long_command = public_tool_call(
        pep.port(),
        &session,
        689,
        "exec_command",
        json!({
            "command":"Start-Sleep -Milliseconds 1800; Write-Output LB_GEN18_LONG_DONE",
            "shell":"windows_powershell",
            "yield_time_ms":0,
            "timeout_ms":10000
        }),
    );
    assert_eq!(
        long_command.body["result"]["structuredContent"]["data"]["status"], "running",
        "generation18 lifecycle fixture did not return a public running session: {:#?}",
        long_command.body
    );
    thread::sleep(Duration::from_millis(650));
    assert_eq!(
        pep.current_task_projection().latest_snapshot(),
        CurrentTaskStatus::Idle,
        "foreground Task must finish after exec_command returns"
    );
    assert!(matches!(
        pep.control_plane
            .tasks()
            .latest_terminal()
            .map(|task| task.lifecycle),
        Some(LifecycleState::Terminal(TerminalOutcome::Completed))
    ));
    let detached = pep.task_aggregate_snapshot();
    assert_eq!(detached["current_activity"]["state"], "running");
    assert_eq!(detached["current_activity"]["kind"], "command");
    let lifecycle_deadline = Instant::now() + Duration::from_secs(30);
    while !pep.task_aggregate_snapshot()["current_activity"].is_null() {
        assert!(
            Instant::now() < lifecycle_deadline,
            "detached Execution did not reach a terminal outcome"
        );
        thread::sleep(Duration::from_millis(25));
    }

    let nested_status = public_tool_call(
        pep.port(),
        &session,
        690,
        "git_workflow",
        json!({"action":"status","path":"NestedProject"}),
    );
    assert_eq!(
        nested_status.body["result"]["isError"], false,
        "{:#?}",
        nested_status.body
    );
    let filtered_diff = public_tool_call(
        pep.port(),
        &session,
        6901,
        "git_workflow",
        json!({"action":"diff","path":"NestedProject","paths":["AGENTS.md"]}),
    );
    assert_eq!(
        filtered_diff.body["result"]["isError"], false,
        "{:#?}",
        filtered_diff.body
    );
    assert!(
        filtered_diff.body["result"]["structuredContent"]["data"]["diff"]
            .as_str()
            .is_some_and(|diff| diff.contains("AGENTS.md") && diff.contains("+after")),
        "{:#?}",
        filtered_diff.body
    );
    assert_eq!(
        nested_status.body["result"]["structuredContent"]["data"]["repository_root"],
        "NestedProject"
    );
    let nested_workflow = public_tool_call(
        pep.port(),
        &session,
        691,
        "agent_workflow",
        json!({
            "action":"bugfix",
            "path":"NestedProject/src",
            "commands":[{"command":"cd","shell":"cmd","yield_time_ms":10000}]
        }),
    );
    assert_eq!(
        nested_workflow.body["result"]["isError"], false,
        "{:#?}",
        nested_workflow.body
    );
    let workflow_data = &nested_workflow.body["result"]["structuredContent"]["data"];
    assert_eq!(
        workflow_data["project"]["selected_path"],
        "NestedProject/src"
    );
    assert_eq!(workflow_data["project"]["repository_root"], "NestedProject");
    assert_eq!(
        workflow_data["git_before"]["repository_root"],
        "NestedProject"
    );
    assert_eq!(
        workflow_data["git_after"]["repository_root"],
        "NestedProject"
    );
    let nested_command_output = workflow_data["commands"][0]["output"]
        .as_str()
        .unwrap_or_default()
        .replace('/', "\\");
    assert!(
        nested_command_output
            .to_ascii_lowercase()
            .contains("nestedproject\\src"),
        "default workflow command workdir ignored selected project: {nested_command_output:?}"
    );

    let mkdir = public_tool_call(
        pep.port(),
        &session,
        692,
        "agent_workflow",
        json!({
            "action":"document",
            "directory_changes":[{"action":"create_directory","path":"schema30-dir"}]
        }),
    );
    assert_eq!(mkdir.body["result"]["isError"], false, "{:#?}", mkdir.body);
    assert!(workspace.join("schema30-dir").is_dir());
    let rmdir = public_tool_call(
        pep.port(),
        &session,
        693,
        "agent_workflow",
        json!({
            "action":"document",
            "directory_changes":[{"action":"remove_empty_directory","path":"schema30-dir"}]
        }),
    );
    assert_eq!(rmdir.body["result"]["isError"], false, "{:#?}", rmdir.body);
    assert!(!workspace.join("schema30-dir").exists());

    fs::create_dir(workspace.join("schema30-nonempty")).unwrap();
    fs::write(workspace.join("schema30-nonempty/keep.txt"), b"keep").unwrap();
    let nonempty = public_tool_call(
        pep.port(),
        &session,
        694,
        "agent_workflow",
        json!({
            "action":"document",
            "directory_changes":[{"action":"remove_empty_directory","path":"schema30-nonempty"}]
        }),
    );
    assert_eq!(
        nonempty.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
        "{:#?}",
        nonempty.body
    );
    assert!(workspace.join("schema30-nonempty/keep.txt").is_file());
    let cancel_after_failed_workflow = public_tool_call(
        pep.port(),
        &session,
        6945,
        "task_control",
        json!({"action":"cancel"}),
    );
    assert_eq!(
        cancel_after_failed_workflow.body["result"]["structuredContent"]["error"]["code"],
        "NotFound",
        "{:#?}",
        cancel_after_failed_workflow.body
    );
    let escaped_directory = public_tool_call(
        pep.port(),
        &session,
        695,
        "agent_workflow",
        json!({
            "action":"document",
            "directory_changes":[{"action":"create_directory","path":"../escape"}]
        }),
    );
    assert_eq!(
        escaped_directory.body["result"]["structuredContent"]["error"]["code"], "WorkspaceDenied",
        "{:#?}",
        escaped_directory.body
    );

    pep.set_simulated_permission_for_test(PermissionMode::Edit);
    let refreshed_full_session = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":6975,"method":"ping","params":{}}),
    );
    assert_eq!(refreshed_full_session.status, 200);
    assert_eq!(refreshed_full_session.body["result"], json!({}));
    let edit_mkdir = public_tool_call(
        pep.port(),
        &session,
        698,
        "agent_workflow",
        json!({
            "action":"document",
            "directory_changes":[{"action":"create_directory","path":"schema30-edit-dir"}]
        }),
    );
    assert_eq!(
        edit_mkdir.body["result"]["isError"], false,
        "{:#?}",
        edit_mkdir.body
    );
    assert!(workspace.join("schema30-edit-dir").is_dir());
    let edit_rmdir = public_tool_call(
        pep.port(),
        &session,
        6981,
        "agent_workflow",
        json!({
            "action":"document",
            "directory_changes":[{"action":"remove_empty_directory","path":"schema30-edit-dir"}]
        }),
    );
    assert_eq!(
        edit_rmdir.body["result"]["isError"], false,
        "{:#?}",
        edit_rmdir.body
    );
    assert!(!workspace.join("schema30-edit-dir").exists());
    pep.set_simulated_permission_for_test(PermissionMode::Full);
    let refreshed_edit_session = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":6977,"method":"ping","params":{}}),
    );
    assert_eq!(refreshed_edit_session.status, 200);
    assert_eq!(refreshed_edit_session.body["result"], json!({}));

    for (id, shell) in [(696u64, "windows_powershell"), (697u64, "auto")] {
        let baseline = public_tool_call(
            pep.port(),
            &session,
            id,
            "exec_command",
            json!({
                "command":"$loc=(Get-Location).Path; $exists=Test-Path -LiteralPath '.'; $count=@(Get-ChildItem -LiteralPath '.').Count; Write-Output ('SCHEMA30_BASELINE '+$exists+' '+$count+' '+$loc)",
                "shell":shell,
                "yield_time_ms":0,
                "timeout_ms":120000
            }),
        );
        let (baseline, output) = settle_public_command(pep.port(), &session, 10_000 + id, baseline);
        assert_eq!(
            baseline.body["result"]["isError"], false,
            "shell={shell}: {:#?}",
            baseline.body
        );
        assert!(
            output.contains("SCHEMA30_BASELINE True"),
            "shell={shell}: {output:?}"
        );
    }

    let module_root_literal = module_root.to_string_lossy().replace('\'', "''");
    let autoload = public_tool_call(
        pep.port(),
        &session,
        699,
        "exec_command",
        json!({
            "command":format!("$env:PSModulePath='{module_root_literal}'; Invoke-LbGen14Auto"),
            "shell":"windows_powershell",
            "yield_time_ms":10000
        }),
    );
    let (autoload, autoload_output) = settle_public_command(pep.port(), &session, 10_699, autoload);
    assert_eq!(
        autoload.body["result"]["isError"], false,
        "{:#?}",
        autoload.body
    );
    assert!(
        autoload_output.contains("LB_GEN14_MODULE_AUTOLOAD_SENTINEL"),
        "ordinary PowerShell must retain the current user's native module surface: {autoload_output:?}"
    );

    let quoted = public_tool_call(
        pep.port(),
        &session,
        701,
        "exec_command",
        json!({
            "command":"Write-Output \"a|b\"; Write-Output \"a&b\"; Write-Output 'q|b'; Write-Output 'q&b'; Write-Output '中文输出✓'",
            "shell":"windows_powershell",
            "yield_time_ms":0
        }),
    );
    let (quoted, quoted_output) = settle_public_command(pep.port(), &session, 41_000, quoted);
    assert_eq!(
        quoted.body["result"]["isError"], false,
        "{:#?}",
        quoted.body
    );
    for literal in ["a|b", "a&b", "q|b", "q&b", "中文输出✓"] {
        assert!(
            quoted_output.contains(literal),
            "missing {literal:?}: {quoted_output:?}"
        );
    }

    let powershell_error = public_tool_call(
        pep.port(),
        &session,
        7011,
        "exec_command",
        json!({
            "command":"Write-Error \"READERR 🚀\"",
            "shell":"windows_powershell",
            "yield_time_ms":0
        }),
    );
    let (powershell_error, powershell_error_output) =
        settle_public_command(pep.port(), &session, 42_000, powershell_error);
    assert_eq!(
        powershell_error.body["result"]["isError"], true,
        "{:#?}",
        powershell_error.body
    );
    let powershell_error_data = &powershell_error.body["result"]["structuredContent"]["data"];
    assert!(
        powershell_error_output.contains("READERR 🚀"),
        "{powershell_error_output:?}"
    );
    for private in [
        "PSModuleAutoLoadingPreference",
        "Microsoft.PowerShell.Management",
        "OutputEncoding",
        "_xD83D_",
        "_xDE80_",
    ] {
        assert!(
            !powershell_error_output.contains(private),
            "{powershell_error_output:?}"
        );
    }
    let stderr_ref = powershell_error_data["output_refs"]["stderr"]
        .as_str()
        .unwrap_or_else(|| {
            panic!("PowerShell failure must retain stderr: {powershell_error_data:#?}")
        });
    let retained_error = public_tool_call(
        pep.port(),
        &session,
        7012,
        "command_control",
        json!({"action":"read","output_ref":stderr_ref,"stream":"stderr","offset":0,"limit":1048576}),
    );
    assert_eq!(
        retained_error.body["result"]["isError"], false,
        "{:#?}",
        retained_error.body
    );
    let mismatched_stream = public_tool_call(
        pep.port(),
        &session,
        70121,
        "command_control",
        json!({"action":"read","output_ref":stderr_ref,"stream":"stdout","offset":0,"limit":100}),
    );
    let mismatch_error = &mismatched_stream.body["result"]["structuredContent"]["error"];
    assert_eq!(mismatch_error["code"], "InvalidArgument");
    assert_eq!(mismatch_error["details"]["field"], "stream");
    assert_eq!(mismatch_error["details"]["expected"], "stderr");
    assert_eq!(mismatch_error["details"]["actual"], "stdout");
    let retained_content = retained_error.body["result"]["structuredContent"]["data"]["content"]
        .as_str()
        .unwrap_or_default();
    assert!(
        retained_content.contains("READERR 🚀"),
        "{retained_content:?}"
    );
    for private in [
        "PSModuleAutoLoadingPreference",
        "Microsoft.PowerShell.Management",
        "OutputEncoding",
        "_xD83D_",
        "_xDE80_",
    ] {
        assert!(!retained_content.contains(private), "{retained_content:?}");
    }

    let cmd_cd_switch = public_tool_call(
        pep.port(),
        &session,
        7013,
        "exec_command",
        json!({
            "command":"cd /d . && echo LB_CMD_D_OK",
            "shell":"cmd",
            "yield_time_ms":0
        }),
    );
    let (cmd_cd_switch, cmd_cd_output) =
        settle_public_command(pep.port(), &session, 43_000, cmd_cd_switch);
    assert_eq!(
        cmd_cd_switch.body["result"]["isError"], false,
        "{:#?}",
        cmd_cd_switch.body
    );
    assert!(
        cmd_cd_output.contains("LB_CMD_D_OK"),
        "{:#?}",
        cmd_cd_switch.body
    );
    let cmd_escape = public_tool_call(
        pep.port(),
        &session,
        7014,
        "exec_command",
        json!({
            "command":"cd /d C:\\Windows && cd",
            "shell":"cmd",
            "yield_time_ms":10000
        }),
    );
    assert_eq!(
        cmd_escape.body["result"]["isError"], false,
        "{:#?}",
        cmd_escape.body
    );
    assert!(
        cmd_escape.body["result"]["structuredContent"]["data"]["output"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains(r"c:\windows"),
        "{:#?}",
        cmd_escape.body
    );

    let auto_utf8 = public_tool_call(
        pep.port(),
        &session,
        702,
        "exec_command",
        json!({
            "command":"Write-Output '自动中文✓'",
            "shell":"auto",
            "yield_time_ms":0
        }),
    );
    let (auto_utf8, auto_utf8_output) =
        settle_public_command(pep.port(), &session, 44_000, auto_utf8);
    assert_eq!(
        auto_utf8.body["result"]["isError"], false,
        "{:#?}",
        auto_utf8.body
    );
    assert!(auto_utf8_output.contains("自动中文✓"));

    for (id, size) in [(760u64, 512u32), (761u64, 64u32)] {
        let viewed = public_tool_call(
            pep.port(),
            &session,
            id,
            "view_image",
            json!({
                "path":"large.png",
                "max_width":size,
                "max_height":size,
                "auto_resize":true,
                "max_bytes":10485760
            }),
        );
        assert_eq!(
            viewed.body["result"]["isError"], false,
            "{:#?}",
            viewed.body
        );
        let data = &viewed.body["result"]["structuredContent"]["data"];
        assert_eq!(data["original_width"], 1024);
        assert_eq!(data["original_height"], 1024);
        assert_eq!(data["width"], size);
        assert_eq!(data["height"], size);
        assert_eq!(data["resized"], true);
        let encoded = viewed.body["result"]["content"][0]["data"]
            .as_str()
            .expect("public image data");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (size, size));
    }

    let one_line = public_tool_call(
        pep.port(),
        &session,
        770,
        "document_workflow",
        json!({"action":"inspect","path":"range.txt","start_block":5,"max_blocks":1}),
    );
    assert_eq!(
        one_line.body["result"]["structuredContent"]["data"]["text"],
        "line5"
    );
    assert_eq!(
        one_line.body["result"]["structuredContent"]["data"]["start_block"],
        5
    );
    assert_eq!(
        one_line.body["result"]["structuredContent"]["data"]["end_block"],
        5
    );
    let three_lines = public_tool_call(
        pep.port(),
        &session,
        771,
        "document_workflow",
        json!({"action":"inspect","path":"range.txt","start_block":1,"max_blocks":3}),
    );
    assert_eq!(
        three_lines.body["result"]["structuredContent"]["data"]["text"],
        "line1\nline2\nline3"
    );
    let invalid_range = public_tool_call(
        pep.port(),
        &session,
        772,
        "document_workflow",
        json!({"action":"inspect","path":"range.txt","start_block":0,"max_blocks":3}),
    );
    assert_eq!(
        invalid_range.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
        "{:#?}",
        invalid_range.body
    );
    let past_eof = public_tool_call(
        pep.port(),
        &session,
        773,
        "document_workflow",
        json!({"action":"inspect","path":"range.txt","start_block":999,"max_blocks":2}),
    );
    assert_eq!(
        past_eof.body["result"]["structuredContent"]["error"]["code"], "InvalidArgument",
        "a document block range beyond EOF must fail explicitly: {:#?}",
        past_eof.body
    );

    fixture.shutdown();
}

#[test]
fn schema28_detached_command_lifecycle_is_incremental_and_durable() {
    let fixture = PublicRuntimeFixture::start(PermissionMode::Full);
    let pep = fixture.runtime();
    let (client, _) = PublicMcpClient::connect(pep.port(), 45_000);

    // Explicit stdin handshakes create causal output boundaries. This does
    // not assume a cold PowerShell process starts within a fixed delay or
    // that the OS schedules exactly one marker per transport poll.
    let mut command = client.start_detached_command(json!({
        "command":"Write-Output 'poll-1'; $null=[Console]::In.ReadLine(); Write-Output 'poll-2'; $null=[Console]::In.ReadLine(); Write-Output 'poll-3'; $line=[Console]::In.ReadLine(); Write-Output ('write:'+ $line); Start-Sleep -Seconds 30",
        "shell":"windows_powershell",
        "yield_time_ms":0,
        "timeout_ms":120000
    }));
    let public_session = command.session_id().to_string();

    for (marker, input) in [
        ("poll-1", Some("step-2\n")),
        ("poll-2", Some("step-3\n")),
        ("poll-3", Some("after-start\n")),
        ("write:after-start", None),
    ] {
        command.wait_for_output(marker, Duration::from_secs(120));
        assert_eq!(
            command.output().matches(marker).count(),
            1,
            "output was replayed: {:?}",
            command.output()
        );
        command.assert_next_poll_empty();
        if let Some(input) = input {
            command.write(input, 1_000);
        }
    }

    let killed = command.kill("TERM", 1_000);
    assert_eq!(
        killed.body["result"]["isError"], false,
        "healthy kill regressed: {:#?}",
        killed.body
    );
    assert_eq!(killed.body["result"]["structuredContent"]["ok"], true);
    assert_eq!(
        killed.body["result"]["structuredContent"]["data"]["status"],
        "cancelled"
    );

    for _ in 0..3 {
        let terminal = client.call_tool(
            "command_control",
            json!({"action":"poll","session_id":public_session}),
        );
        assert_eq!(terminal.body["result"]["structuredContent"]["ok"], true);
        assert!(terminal.body["result"]["structuredContent"]["error"].is_null());
        assert_eq!(
            terminal.body["result"]["structuredContent"]["data"]["status"],
            "cancelled"
        );
        assert_eq!(
            terminal.body["result"]["structuredContent"]["data"]["output"],
            ""
        );
    }

    let durable_terminal = client.call_tool("task_control", json!({"action":"get"}));
    let durable = &durable_terminal.body["result"]["structuredContent"]["data"]["last_activity"];
    assert_eq!(durable["session_id"], public_session);
    assert_eq!(durable["outcome"], "cancelled");
    assert_eq!(durable["error_code"], "ProcessCancelled");
    assert!(
        !serde_json::to_string(durable).unwrap().contains("PRIVATE_"),
        "durable terminal task-state leaked a private handle: {durable:#?}"
    );

    fixture.shutdown();
}

#[test]
fn absolute_and_relative_active_workspace_paths_are_equivalent() {
    let root = repo_root();
    let workspace = temp_workspace();
    let outside = workspace.with_extension("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(workspace.join("absolute.txt"), b"ABSOLUTE_PATH_OK\n").unwrap();
    fs::write(outside.join("outside.txt"), b"OUTSIDE\n").unwrap();
    fs::copy(
        root.join("assets/icons/localbridge.png"),
        workspace.join("absolute.png"),
    )
    .unwrap();

    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("PEP listener ready");
    let session = initialize(pep.port(), 810)
        .session
        .expect("absolute path test session");

    for (id, path) in [
        (811, "absolute.txt".to_string()),
        (
            812,
            workspace
                .join("absolute.txt")
                .to_string_lossy()
                .into_owned(),
        ),
    ] {
        let response = public_tool_call(
            pep.port(),
            &session,
            id,
            "document_workflow",
            json!({"action":"inspect","path":path}),
        );
        assert_eq!(response.status, 200);
        assert_eq!(
            response.body["result"]["isError"], false,
            "in-workspace document path failed: {}",
            response.body
        );
    }

    let workdir = public_tool_call(
        pep.port(),
        &session,
        813,
        "exec_command",
        json!({
            "command":"cd",
            "shell":"cmd",
            "workdir":workspace.to_string_lossy(),
            "yield_time_ms":10000
        }),
    );
    assert_eq!(workdir.status, 200);
    assert_eq!(
        workdir.body["result"]["isError"], false,
        "absolute in-workspace workdir failed: {}",
        workdir.body
    );

    let absolute_redirect_target = workspace.join("absolute-redirect.txt");
    let absolute_redirect = public_tool_call(
        pep.port(),
        &session,
        816,
        "exec_command",
        json!({
            "command":format!(r#"echo ABSOLUTE_REDIRECT_OK>"{}""#, absolute_redirect_target.display()),
            "shell":"cmd",
            "workdir":workspace.to_string_lossy(),
            "yield_time_ms":10000
        }),
    );
    assert_eq!(
        absolute_redirect.body["result"]["isError"], false,
        "absolute in-workspace redirection failed: {}",
        absolute_redirect.body
    );

    let relative_redirect = public_tool_call(
        pep.port(),
        &session,
        817,
        "exec_command",
        json!({
            "command":"echo RELATIVE_REDIRECT_OK>relative-redirect.txt",
            "shell":"cmd",
            "workdir":workspace.to_string_lossy(),
            "yield_time_ms":10000
        }),
    );
    assert_eq!(
        relative_redirect.body["result"]["isError"], false,
        "relative in-workspace redirection failed: {}",
        relative_redirect.body
    );
    assert_eq!(
        fs::read_to_string(&absolute_redirect_target)
            .unwrap()
            .trim(),
        "ABSOLUTE_REDIRECT_OK"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("relative-redirect.txt"))
            .unwrap()
            .trim(),
        "RELATIVE_REDIRECT_OK"
    );

    let outside_redirect_target = outside.join("current-user-redirect.txt");
    let outside_redirect = public_tool_call(
        pep.port(),
        &session,
        818,
        "exec_command",
        json!({
            "command":format!(r#"echo CURRENT_USER_WRITE>"{}""#, outside_redirect_target.display()),
            "shell":"cmd",
            "workdir":workspace.to_string_lossy(),
            "yield_time_ms":10000
        }),
    );
    assert_eq!(
        outside_redirect.body["result"]["isError"], false,
        "ordinary shell must not pretend to enforce structured workspace authority: {}",
        outside_redirect.body
    );
    assert_eq!(
        fs::read_to_string(&outside_redirect_target).unwrap().trim(),
        "CURRENT_USER_WRITE"
    );

    let image = public_tool_call(
        pep.port(),
        &session,
        814,
        "view_image",
        json!({
            "path":workspace.join("absolute.png").to_string_lossy(),
            "max_width":64,
            "max_height":64
        }),
    );
    assert_eq!(image.status, 200);
    assert_eq!(
        image.body["result"]["isError"], false,
        "absolute in-workspace image path failed: {}",
        image.body
    );

    let outside_denied = public_tool_call(
        pep.port(),
        &session,
        815,
        "document_workflow",
        json!({
            "action":"inspect",
            "path":outside.join("outside.txt").to_string_lossy()
        }),
    );
    assert_tool_error(&outside_denied, "WorkspaceDenied");

    let mut coding = pep.stop().expect("absolute path PEP stops");
    coding.stop().expect("absolute path MCP stops");
    cleanup_test_directory(&workspace);
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn schema40_health_probe_remains_authenticated_while_facade_lock_is_held() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("PEP ready");
    let session = initialize(pep.port(), 819)
        .session
        .expect("lock contention test session");
    let facade_guard = pep.guard.as_ref().unwrap().lock().unwrap();
    let started = Instant::now();
    let health = pep
        .coding_runtime_health()
        .expect("independent authenticated health probe")
        .expect("health is available");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        health.state,
        super::super::facade::CodingRuntimeHealthState::Ready
    );
    assert!(health.authenticated_mcp);
    let snapshot = public_tool_call(
        pep.port(),
        &session,
        820,
        "task_control",
        json!({"action":"get"}),
    );
    assert_eq!(
        snapshot.body["result"]["structuredContent"]["data"]["availability"], "stale",
        "facade lock contention must be explicit rather than fabricating a live projection: {:#?}",
        snapshot.body
    );
    assert_eq!(
        snapshot.body["result"]["structuredContent"]["data"]["current_activity"],
        Value::Null
    );
    drop(facade_guard);
    let mut coding = pep.stop().expect("PEP stop after independent health probe");
    coding
        .stop()
        .expect("MCP stop after independent health probe");
    cleanup_test_directory(&workspace);
}

#[cfg(windows)]
#[test]
fn schema40_real_root_process_alive_but_mcp_unresponsive_is_not_ready() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let coding_pid = coding.process_snapshot().pid;
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("PEP ready");
    let suspended = SuspendedProcess::suspend(coding_pid);
    assert_eq!(pep.upstream_root_is_running().unwrap(), Some(true));
    let started = Instant::now();
    let health = pep
        .coding_runtime_health()
        .expect("bounded health probe")
        .expect("health state");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "health probe exceeded bound: {:?}",
        started.elapsed()
    );
    assert_ne!(
        health.state,
        super::super::facade::CodingRuntimeHealthState::Ready
    );
    assert!(!health.authenticated_mcp);
    assert!(
        health.root_process_alive,
        "suspended MCP root must still be alive"
    );
    drop(suspended);
    let ready_deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let health = pep.coding_runtime_health().unwrap().unwrap();
        if health.state == super::super::facade::CodingRuntimeHealthState::Ready
            && health.authenticated_mcp
        {
            break;
        }
        assert!(
            Instant::now() < ready_deadline,
            "MCP did not recover after resume: {health:?}"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let mut coding = pep.stop().expect("PEP stop after suspended MCP test");
    coding.stop().expect("MCP stop after suspended MCP test");
    cleanup_test_directory(&workspace);
}

#[test]
fn actual_bundled_mcp_is_reached_only_through_loopback_policy_server() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let coding_pid = coding.process_snapshot().pid;
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("PEP listener ready");
    assert!(pep.endpoint().starts_with("http://127.0.0.1:"));
    assert!(pep.is_running());
    assert!(!format!("{pep:?}").contains(SYNTHETIC_BEARER));

    let initialized = initialize(pep.port(), 1);
    assert_eq!(initialized.status, 200);
    assert_eq!(
        initialized.body["result"]["protocolVersion"],
        CURRENT_PROTOCOL_VERSION
    );
    let session = initialized.session.expect("downstream MCP session");
    let notified = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    );
    assert_eq!(notified.status, 202);

    let full_tools = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    );
    let full_catalog = full_tools.body["result"]["tools"].as_array().unwrap();
    assert_eq!(full_catalog.len(), 10);
    assert!(
        full_catalog
            .iter()
            .any(|tool| tool["name"] == "exec_command")
    );
    assert!(
        full_catalog
            .iter()
            .any(|tool| tool["name"] == "elevated_exec")
    );
    let elevated_schema = full_catalog
        .iter()
        .find(|tool| tool["name"] == "elevated_exec")
        .expect("stable elevated_exec definition");
    assert_eq!(elevated_schema["inputSchema"]["type"], "object");
    for property in [
        "operation",
        "program",
        "args",
        "shell",
        "command",
        "workdir",
        "action",
        "path",
        "destination",
        "content_base64",
        "recursive",
        "timeout_ms",
        "max_output_bytes",
    ] {
        assert!(
            elevated_schema["inputSchema"]["properties"][property].is_object(),
            "elevated_exec top-level property missing: {property}"
        );
    }
    assert!(
        elevated_schema["inputSchema"].get("oneOf").is_none(),
        "elevated_exec public input schema must remain a directly projectable top-level object"
    );

    for private in [
        "read_file",
        "apply_patch",
        "git_status",
        "write_stdin",
        "server_info",
    ] {
        assert!(
            !full_catalog.iter().any(|tool| tool["name"] == private),
            "private upstream tool leaked into public registry: {private}"
        );
    }
    let raw_private = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":"raw-private","method":"tools/call",
            "params":{"name":"read_file","arguments":{"path":"probe.txt"}}
        }),
    );
    assert_tool_error(&raw_private, "CapabilityDenied");

    pep.set_simulated_permission_for_test(PermissionMode::Edit);
    let refreshed_full_call = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"exec_command","arguments":{"command":"echo must-not-run"}}
        }),
    );
    assert_eq!(refreshed_full_call.status, 200);
    assert_tool_error(&refreshed_full_call, "PolicyDenied");
    let edit_session = session.clone();
    let denied = post(
        pep.port(),
        Some(&edit_session),
        &json!({
            "jsonrpc":"2.0","id":32,"method":"tools/call",
            "params":{"name":"exec_command","arguments":{"command":"echo must-not-run"}}
        }),
    );
    assert_tool_error(&denied, "PolicyDenied");
    let denied_timing = pep.current_task_projection().timing_snapshot();
    assert_eq!(denied_timing.status, CurrentTaskStatus::Idle);
    assert_eq!(
        denied_timing.last_tool.as_ref().map(|tool| tool.kind),
        Some(TaskKind::ExecuteCommand)
    );

    let edit_tools = post(
        pep.port(),
        Some(&edit_session),
        &json!({"jsonrpc":"2.0","id":4,"method":"tools/list","params":{}}),
    );
    let edit_tool_names = edit_tools.body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(edit_tool_names.len(), 10);
    assert!(edit_tool_names.contains(&"agent_workflow"));
    assert!(edit_tool_names.contains(&"elevated_exec"));
    assert!(edit_tool_names.contains(&"task_control"));
    for process_tool in ["exec_command", "command_control"] {
        assert!(edit_tool_names.contains(&process_tool));
    }

    let read_started = Instant::now();
    let read = post(
        pep.port(),
        Some(&edit_session),
        &json!({
            "jsonrpc":"2.0","id":5,"method":"tools/call",
            "params":{"name":"document_workflow","arguments":{"action":"inspect","path":"probe.txt"}}
        }),
    );
    let read_round_trip = read_started.elapsed();
    assert!(
        read.body.get("result").is_some(),
        "allowed document_workflow inspect must forward: {}",
        read.body
    );
    assert!(
        read_round_trip < Duration::from_millis(500),
        "UI presentation retention must not delay real MCP response: {read_round_trip:?}"
    );
    let timing = pep.current_task_projection().timing_snapshot();
    assert_eq!(timing.status, CurrentTaskStatus::Idle);
    assert_eq!(
        timing.last_tool.as_ref().map(|tool| tool.kind),
        Some(TaskKind::ReadFile)
    );

    pep.set_simulated_permission_for_test(PermissionMode::Full);
    let refreshed_full_tools = post(
        pep.port(),
        Some(&edit_session),
        &json!({"jsonrpc":"2.0","id":"cached-full","method":"tools/list","params":{}}),
    );
    assert_eq!(refreshed_full_tools.status, 200);
    let full_session = edit_session.clone();
    let cached_full = post(
        pep.port(),
        Some(&full_session),
        &json!({"jsonrpc":"2.0","id":"fresh-full","method":"tools/list","params":{}}),
    );
    assert!(
        cached_full.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "exec_command")
    );

    let unknown_action = post(
        pep.port(),
        Some(&full_session),
        &json!({
            "jsonrpc":"2.0","id":"unknown-public-action","method":"tools/call",
            "params":{"name":"git_workflow","arguments":{"action":"future_private_action"}}
        }),
    );
    assert_tool_error(&unknown_action, "CapabilityDenied");
    assert_eq!(
        pep.current_task_projection().snapshot(),
        CurrentTaskStatus::Idle
    );

    let base_policy = fs::read_to_string(root.join("runtime-policy.toml")).unwrap();
    let narrowed_policy = base_policy
        .replace(
            "edit_tools = [\"workspace_context\", \"agent_workflow\", \"filesystem\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
            "edit_tools = [\"workspace_context\", \"filesystem\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
        )
        .replace(
            "full_tools = [\"workspace_context\", \"agent_workflow\", \"filesystem\", \"exec_command\", \"command_control\", \"task_control\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
            "full_tools = [\"workspace_context\", \"filesystem\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
        )
        .replace(
            "elevated_tools = [\"workspace_context\", \"agent_workflow\", \"filesystem\", \"exec_command\", \"command_control\", \"task_control\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
            "elevated_tools = [\"workspace_context\", \"filesystem\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
        );
    pep.replace_policy(CapabilityPolicy::from_toml(&narrowed_policy).unwrap())
        .expect("live public policy narrowing");
    let refreshed_policy_call = post(
        pep.port(),
        Some(&full_session),
        &json!({
            "jsonrpc":"2.0","id":"stale-policy-call","method":"tools/call",
            "params":{"name":"exec_command","arguments":{"command":"echo cached-list-must-not-run"}}
        }),
    );
    assert_tool_error(&refreshed_policy_call, "PolicyDenied");
    let narrowed_session = full_session.clone();
    let narrowed_denied = post(
        pep.port(),
        Some(&narrowed_session),
        &json!({
            "jsonrpc":"2.0","id":"narrowed-denied","method":"tools/call",
            "params":{"name":"exec_command","arguments":{"command":"echo cached-list-must-not-run"}}
        }),
    );
    assert_tool_error(&narrowed_denied, "PolicyDenied");
    assert_eq!(
        pep.current_task_projection().snapshot(),
        CurrentTaskStatus::Idle
    );
    let narrowed_tools = post(
        pep.port(),
        Some(&narrowed_session),
        &json!({"jsonrpc":"2.0","id":"narrowed-tools","method":"tools/list","params":{}}),
    );
    assert!(
        narrowed_tools.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "exec_command"),
        "policy changes must not remove a public schema"
    );
    let reinitialized = initialize(pep.port(), 6);
    let new_session = reinitialized.session.unwrap();
    assert_ne!(new_session, narrowed_session);
    let still_live = post(
        pep.port(),
        Some(&narrowed_session),
        &json!({"jsonrpc":"2.0","id":7,"method":"ping","params":{}}),
    );
    assert_eq!(still_live.status, 200);
    assert_eq!(delete(pep.port(), &narrowed_session), 204);
    assert_eq!(delete(pep.port(), &new_session), 204);

    let pep_port = pep.port();
    let mut coding = pep.stop().expect("PEP stop returns owned MCP runtime");
    assert_eq!(coding.process_snapshot().pid, coding_pid);
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, pep_port)).is_err());
    coding.stop().expect("MCP Job stop");
    assert_eq!(coding.active_processes().unwrap(), 0);
    drop(coding);
    cleanup_test_directory(&workspace);
}

#[test]
fn full_scripts_share_current_user_authority_independent_of_path_spelling() {
    let root = repo_root();
    let workspace = temp_workspace();
    let scripts = workspace.join("scripts");
    fs::create_dir_all(&scripts).unwrap();
    fs::write(scripts.join("probe.cmd"), b"@echo LB007_CMD_SCRIPT_OK\r\n").unwrap();
    fs::write(scripts.join("probe.bat"), b"@echo LB007_BAT_SCRIPT_OK\r\n").unwrap();
    fs::write(
        scripts.join("probe.ps1"),
        b"Write-Output 'LB007_PS1_SCRIPT_OK'\r\n",
    )
    .unwrap();

    let outside_name = format!(
        "lb007-outside-{}-{}.cmd",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let outside = workspace.parent().unwrap().join(&outside_name);
    fs::write(
        &outside,
        b"@echo CURRENT_USER_SCRIPT>outside-script.txt\r\n",
    )
    .unwrap();

    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready for script execution");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("PEP listener ready for script execution");
    let session = initialize(pep.port(), 700)
        .session
        .expect("script E2E MCP session");

    for (id, command, shell, marker) in [
        (701, r"scripts\probe.cmd", "cmd", "LB007_CMD_SCRIPT_OK"),
        (702, r"scripts\probe.bat", "cmd", "LB007_BAT_SCRIPT_OK"),
        (
            703,
            r".\scripts\probe.ps1",
            "windows_powershell",
            "LB007_PS1_SCRIPT_OK",
        ),
    ] {
        let response = public_tool_call(
            pep.port(),
            &session,
            id,
            "exec_command",
            json!({"command":command,"shell":shell,"yield_time_ms":0,"timeout_ms":120000}),
        );
        let (response, output) = settle_public_command(pep.port(), &session, 20_000 + id, response);
        assert_eq!(
            response.body["result"]["isError"], false,
            "{:#?}",
            response.body
        );
        assert!(output.contains(marker), "{:#?}", response.body);
    }

    let nul = public_tool_call(
        pep.port(),
        &session,
        705,
        "exec_command",
        json!({
            "command":"echo hidden>nul && echo hidden-error 1>nul 2>nul && echo LB_SCHEMA42_NUL_OK",
            "shell":"cmd",
            "yield_time_ms":0,
            "timeout_ms":120000
        }),
    );
    let (nul, nul_output) = settle_public_command(pep.port(), &session, 20_705, nul);
    assert_eq!(nul.body["result"]["isError"], false, "{:#?}", nul.body);
    assert!(nul_output.contains("LB_SCHEMA42_NUL_OK"));
    for entry in fs::read_dir(&workspace).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        assert_ne!(name, "nul");
        assert_ne!(name, "nul.localbridge");
    }

    let oem = public_tool_call(
        pep.port(),
        &session,
        706,
        "exec_command",
        json!({
            "command":"echo LB_SCHEMA42_OEM_é中あ한Я",
            "shell":"cmd",
            "yield_time_ms":10000
        }),
    );
    assert_eq!(oem.body["result"]["isError"], false, "{:#?}", oem.body);
    let output = oem.body["result"]["structuredContent"]["data"]["output"]
        .as_str()
        .unwrap_or_default();
    assert!(output.contains("LB_SCHEMA42_OEM_"), "{output:?}");
    assert!(
        !output.contains('\u{fffd}'),
        "OEM output was decoded as lossy UTF-8: {output:?}"
    );
    assert!(
        ['é', '中', 'あ', '한', 'Я']
            .iter()
            .any(|candidate| output.contains(*candidate)),
        "no representative non-ASCII OEM character survived decoding: {output:?}"
    );

    let escaped = public_tool_call(
        pep.port(),
        &session,
        704,
        "exec_command",
        json!({
            "command":format!(r"..\{outside_name}"),
            "shell":"cmd",
            "yield_time_ms":10000
        }),
    );
    assert_eq!(
        escaped.body["result"]["isError"], false,
        "{:#?}",
        escaped.body
    );
    assert_eq!(
        fs::read_to_string(workspace.join("outside-script.txt"))
            .unwrap()
            .trim(),
        "CURRENT_USER_SCRIPT"
    );

    let mut coding = pep.stop().expect("script E2E PEP stop");
    coding.stop().expect("script E2E MCP stop");
    drop(coding);
    let _ = fs::remove_file(outside);
    cleanup_test_directory(&workspace);
}

#[test]
fn r1_same_rpc_id_in_distinct_sessions_has_isolated_cancellation() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("PEP listener ready");
    let session_a = initialize(pep.port(), 801).session.expect("session A");
    let session_b = initialize(pep.port(), 802).session.expect("session B");
    for session in [&session_a, &session_b] {
        assert_eq!(
            post(
                pep.port(),
                Some(session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );
    }

    let port = pep.port();
    let call_session_a = session_a.clone();
    let call_a = thread::spawn(move || {
        post_with_read_timeout(
            port,
            Some(&call_session_a),
            &json!({
                "jsonrpc":"2.0",
                "id":1,
                "method":"tools/call",
                "params":{
                    "name":"exec_command",
                    "arguments":{
                        "command":"Start-Sleep -Seconds 10",
                        "shell":"windows_powershell",
                        "yield_time_ms":10000,
                        "timeout_ms":120000,
                        "max_output_bytes":4096
                    }
                }
            }),
            Duration::from_secs(150),
        )
    });
    assert_eventually("session A never ran", Duration::from_secs(3), || {
        matches!(
            pep.current_task_projection().latest_snapshot(),
            CurrentTaskStatus::Active(ref task) if task.state == TaskExecutionState::Running
        )
    });

    let call_session_b = session_b.clone();
    let call_b = thread::spawn(move || {
        post_with_read_timeout(
            port,
            Some(&call_session_b),
            &json!({
                "jsonrpc":"2.0",
                "id":1,
                "method":"tools/call",
                "params":{
                    "name":"exec_command",
                    "arguments":{
                        "command":"Write-Output SESSION_B_SURVIVED",
                        "shell":"windows_powershell",
                        "yield_time_ms":0,
                        "timeout_ms":120000,
                        "max_output_bytes":4096
                    }
                }
            }),
            Duration::from_secs(150),
        )
    });
    thread::sleep(Duration::from_millis(100));

    let cancelled = post(
        pep.port(),
        Some(&session_a),
        &json!({
            "jsonrpc":"2.0",
            "method":"notifications/cancelled",
            "params":{"requestId":1,"reason":"R1 isolation test"}
        }),
    );
    assert_eq!(cancelled.status, 202);
    let result_a = call_a.join().expect("session A request");
    assert_eq!(
        result_a.body["result"]["isError"], true,
        "{:#?}",
        result_a.body
    );
    assert!(
        matches!(
            result_a.body["result"]["structuredContent"]["data"]["status"].as_str(),
            Some("cancelled" | "failed")
        ),
        "{:#?}",
        result_a.body
    );
    let result_b = call_b.join().expect("session B request");
    let (result_b, result_b_output) =
        settle_public_command(pep.port(), &session_b, 30_001, result_b);
    assert_eq!(
        result_b.body["result"]["structuredContent"]["data"]["status"], "completed",
        "{:#?}",
        result_b.body
    );
    assert!(
        result_b_output.contains("SESSION_B_SURVIVED"),
        "{:#?}",
        result_b.body
    );

    let mut coding = pep.stop().expect("PEP stop after R1 isolation test");
    coding.stop().expect("MCP stop after R1 isolation test");
    assert_eq!(coding.active_processes().unwrap(), 0);
    drop(coding);
    cleanup_test_directory(&workspace);
}

#[test]
fn r1_task_control_cancel_never_cancels_another_session_request() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("PEP listener ready");
    let session_a = initialize(pep.port(), 811).session.expect("session A");
    let session_b = initialize(pep.port(), 812).session.expect("session B");
    for session in [&session_a, &session_b] {
        assert_eq!(
            post(
                pep.port(),
                Some(session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );
    }

    let port = pep.port();
    let call_session_a = session_a.clone();
    let call_a = thread::spawn(move || {
        post_with_read_timeout(
            port,
            Some(&call_session_a),
            &json!({
                "jsonrpc":"2.0","id":7,"method":"tools/call",
                "params":{
                    "name":"exec_command",
                    "arguments":{
                        "command":"Start-Sleep -Seconds 10",
                        "shell":"windows_powershell",
                        "yield_time_ms":10000,
                        "timeout_ms":120000,
                        "max_output_bytes":4096
                    }
                }
            }),
            Duration::from_secs(150),
        )
    });
    let running_deadline = Instant::now() + Duration::from_secs(3);
    while !matches!(
        pep.current_task_projection().latest_snapshot(),
        CurrentTaskStatus::Active(ref task) if task.state == TaskExecutionState::Running
    ) {
        assert!(Instant::now() < running_deadline, "session A never ran");
        thread::sleep(Duration::from_millis(10));
    }

    let call_session_b = session_b.clone();
    let call_b = thread::spawn(move || {
        post_with_read_timeout(
            port,
            Some(&call_session_b),
            &json!({
                "jsonrpc":"2.0","id":7,"method":"tools/call",
                "params":{
                    "name":"exec_command",
                    "arguments":{
                        "command":"Write-Output SESSION_B_NOT_CANCELLED",
                        "shell":"windows_powershell",
                        "yield_time_ms":0,
                        "timeout_ms":120000,
                        "max_output_bytes":4096
                    }
                }
            }),
            Duration::from_secs(150),
        )
    });
    assert_eventually(
        "session B did not enter Work FIFO",
        Duration::from_secs(3),
        || {
            let scheduler = pep.control_plane.scheduler().snapshot();
            scheduler.work_running == 1 && scheduler.work_queued == 1
        },
    );
    let queued = pep.control_plane.scheduler().snapshot();
    assert_eq!(queued.work_running, 1);
    assert_eq!(queued.work_queued, 1, "session B did not enter Work FIFO");

    let cancel = public_tool_call(
        pep.port(),
        &session_a,
        8,
        "task_control",
        json!({"action":"cancel"}),
    );
    assert_eq!(
        cancel.body["result"]["structuredContent"]["data"]["cancelled_requests"], 1,
        "{:#?}",
        cancel.body
    );
    let result_a = call_a.join().expect("session A request");
    assert_eq!(
        result_a.body["result"]["isError"], true,
        "{:#?}",
        result_a.body
    );
    let result_b = call_b.join().expect("session B request");
    let (result_b, result_b_output) =
        settle_public_command(pep.port(), &session_b, 30_101, result_b);
    assert_eq!(
        result_b.body["result"]["structuredContent"]["data"]["status"], "completed",
        "{:#?}",
        result_b.body
    );
    assert!(
        result_b_output.contains("SESSION_B_NOT_CANCELLED"),
        "{:#?}",
        result_b.body
    );

    let mut coding = pep.stop().expect("PEP stop after task isolation test");
    coding.stop().expect("MCP stop after task isolation test");
    assert_eq!(coding.active_processes().unwrap(), 0);
    drop(coding);
    cleanup_test_directory(&workspace);
}

#[test]
fn cancellation_reaches_actual_upstream_while_tool_call_is_running() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("PEP listener ready");
    let initialized = initialize(pep.port(), 20);
    let session = initialized.session.expect("downstream MCP session");
    assert_eq!(
        post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .status,
        202
    );

    let port = pep.port();
    let call_session = session.clone();
    let call = thread::spawn(move || {
        post(
            port,
            Some(&call_session),
            &json!({
                "jsonrpc":"2.0",
                "id":"cancel-me",
                "method":"tools/call",
                "params":{
                    "name":"exec_command",
                    "arguments":{
                        "command":"Start-Sleep -Seconds 10",
                        "shell":"windows_powershell",
                        "yield_time_ms":10000,
                        "timeout_ms":20000,
                        "max_output_bytes":4096,
                        "verbosity":"summary"
                    }
                }
            }),
        )
    });

    let running_deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if matches!(
            pep.current_task_projection().snapshot(),
            CurrentTaskStatus::Active(ref task)
                if task.state == crate::state::TaskExecutionState::Running
        ) {
            break;
        }
        assert!(
            std::time::Instant::now() < running_deadline,
            "tool call never projected Running"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let cancel_started = std::time::Instant::now();
    let cancelled = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0",
            "method":"notifications/cancelled",
            "params":{"requestId":"cancel-me","reason":"LB-009 deterministic cancellation test"}
        }),
    );
    assert_eq!(cancelled.status, 202);
    assert!(
        cancel_started.elapsed() < Duration::from_secs(2),
        "cancellation transport was blocked"
    );

    let call_result = call.join().expect("tools/call client thread");
    let cancellation_settle_elapsed = cancel_started.elapsed();
    assert!(
        cancellation_settle_elapsed < Duration::from_secs(5),
        "upstream cancellation did not settle within 5 seconds; elapsed_ms={}",
        cancellation_settle_elapsed.as_millis()
    );
    assert!(
        call_result.body.get("result").is_some() || call_result.body.get("error").is_some(),
        "cancelled tools/call must terminate with a JSON-RPC response: {}",
        call_result.body
    );
    let presentation_deadline = std::time::Instant::now() + Duration::from_secs(1);
    while pep.current_task_projection().snapshot() != CurrentTaskStatus::Idle {
        assert!(
            std::time::Instant::now() < presentation_deadline,
            "cancelled tool remained visible beyond the bounded presentation window"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let mut coding = pep.stop().expect("PEP stop after cancellation");
    coding.stop().expect("MCP Job stop after cancellation");
    assert_eq!(coding.active_processes().unwrap(), 0);
    drop(coding);
    cleanup_test_directory(&workspace);
}

#[test]
fn command_control_kill_is_not_blocked_by_unrelated_foreground_work() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("PEP listener ready");
    let owner = initialize(pep.port(), 340).session.expect("owner session");
    let worker = initialize(pep.port(), 341).session.expect("worker session");
    for session in [&owner, &worker] {
        assert_eq!(
            post(
                pep.port(),
                Some(session),
                &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            )
            .status,
            202
        );
    }

    let detached = public_tool_call(
        pep.port(),
        &owner,
        342,
        "exec_command",
        json!({
            "command":"Start-Sleep -Seconds 10",
            "shell":"windows_powershell",
            "yield_time_ms":0,
            "timeout_ms":20000,
            "max_output_bytes":4096
        }),
    );
    assert_eq!(
        detached.body["result"]["structuredContent"]["data"]["status"], "running",
        "{:#?}",
        detached.body
    );
    let public_session = detached.body["result"]["structuredContent"]["data"]["session_id"]
        .as_str()
        .expect("detached public session")
        .to_string();

    let port = pep.port();
    let worker_session = worker.clone();
    let foreground = thread::spawn(move || {
        post(
            port,
            Some(&worker_session),
            &json!({
                "jsonrpc":"2.0","id":343,"method":"tools/call",
                "params":{
                    "name":"exec_command",
                    "arguments":{
                        "command":"Start-Sleep -Seconds 10",
                        "shell":"windows_powershell",
                        "yield_time_ms":10000,
                        "timeout_ms":20000,
                        "max_output_bytes":4096
                    }
                }
            }),
        )
    });
    assert_eventually(
        "foreground work never acquired its explicit scheduler slot",
        Duration::from_secs(3),
        || pep.control_plane.scheduler().snapshot().work_running == 1,
    );

    let poll_started = Instant::now();
    let polled = public_tool_call(
        pep.port(),
        &owner,
        344,
        "command_control",
        json!({"action":"poll","session_id":public_session,"wait_ms":0}),
    );
    assert!(
        poll_started.elapsed() < Duration::from_secs(2),
        "command_control poll waited behind unrelated Work"
    );
    assert_eq!(
        polled.body["result"]["structuredContent"]["data"]["status"], "running",
        "{:#?}",
        polled.body
    );

    let kill_started = Instant::now();
    let mut killed = public_tool_call(
        pep.port(),
        &owner,
        345,
        "command_control",
        json!({"action":"kill","session_id":public_session,"signal":"KILL","wait_ms":1000}),
    );
    assert!(
        kill_started.elapsed() < Duration::from_millis(2_500),
        "command_control kill waited behind unrelated Work"
    );
    if killed.body["result"]["structuredContent"]["error"]["code"] == "OperationTimedOut" {
        killed = poll_public_command_to_terminal(
            pep.port(),
            &owner,
            34501,
            &public_session,
            Duration::from_secs(30),
        );
    }
    assert_eq!(
        killed.body["result"]["structuredContent"]["data"]["status"], "cancelled",
        "{:#?}",
        killed.body
    );

    let replay = public_tool_call(
        pep.port(),
        &owner,
        346,
        "command_control",
        json!({"action":"poll","session_id":public_session,"wait_ms":0}),
    );
    assert_eq!(
        replay.body["result"]["structuredContent"]["data"]["status"], "cancelled",
        "{:#?}",
        replay.body
    );

    let cancel_worker = public_tool_call(
        pep.port(),
        &worker,
        347,
        "task_control",
        json!({"action":"cancel"}),
    );
    assert_eq!(
        cancel_worker.body["result"]["structuredContent"]["data"]["cancelled_requests"], 1,
        "{:#?}",
        cancel_worker.body
    );
    let foreground = foreground.join().expect("foreground worker response");
    assert_tool_error(&foreground, "ProcessCancelled");

    let mut coding = pep.stop().expect("PEP stop after control-lane test");
    coding.stop().expect("MCP stop after control-lane test");
    assert_eq!(coding.active_processes().unwrap(), 0);
    drop(coding);
    cleanup_test_directory(&workspace);
}

#[test]
fn task_control_cancel_reaches_running_call_without_waiting_for_facade_execution_lock() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("PEP listener ready");
    let initialized = initialize(pep.port(), 30);
    let session = initialized.session.expect("downstream MCP session");
    assert_eq!(
        post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .status,
        202
    );

    let idle_cancel = public_tool_call(
        pep.port(),
        &session,
        31,
        "task_control",
        json!({"action":"cancel"}),
    );
    assert_eq!(
        idle_cancel.body["result"]["structuredContent"]["error"]["code"], "NotFound",
        "{:#?}",
        idle_cancel.body
    );

    let port = pep.port();
    let call_session = session.clone();
    let call_started = std::time::Instant::now();
    let call = thread::spawn(move || {
        post_with_read_timeout(
            port,
            Some(&call_session),
            &json!({
                "jsonrpc":"2.0",
                "id":"task-control-me",
                "method":"tools/call",
                "params":{
                    "name":"exec_command",
                    "arguments":{
                        "command":"Start-Sleep -Seconds 10",
                        "shell":"windows_powershell",
                        "yield_time_ms":10000,
                        "timeout_ms":20000,
                        "max_output_bytes":4096
                    }
                }
            }),
            Duration::from_secs(6),
        )
    });

    let running_deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !matches!(
        pep.current_task_projection().latest_snapshot(),
        CurrentTaskStatus::Active(ref task) if task.state == TaskExecutionState::Running
    ) {
        assert!(
            std::time::Instant::now() < running_deadline,
            "tool call never became actually Running"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let cancel_started = std::time::Instant::now();
    let cancel = public_tool_call(
        pep.port(),
        &session,
        32,
        "task_control",
        json!({"action":"cancel"}),
    );
    assert!(
        cancel_started.elapsed() < Duration::from_secs(2),
        "task_control cancel blocked behind the facade execution mutex"
    );
    let cancel_data = &cancel.body["result"]["structuredContent"]["data"];
    assert_eq!(
        cancel_data["cancellation_requested"], true,
        "{:#?}",
        cancel.body
    );
    assert!(
        matches!(cancel_data["state"].as_str(), Some("active" | "idle")),
        "the cancellation ACK must publish its truthful instantaneous lifecycle: {:#?}",
        cancel.body
    );
    assert!(
        cancel.body["result"]["structuredContent"]["data"]["cancelled_requests"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "{:#?}",
        cancel.body
    );

    let result = call.join().expect("tools/call client thread");
    assert!(
        call_started.elapsed() < Duration::from_secs(5),
        "task_control cancellation did not interrupt the long command"
    );
    assert_eq!(result.body["result"]["isError"], true, "{:#?}", result.body);
    assert_eq!(
        result.body["result"]["structuredContent"]["error"]["code"], "ProcessCancelled",
        "{:#?}",
        result.body
    );
    assert_eq!(
        result.body["result"]["structuredContent"]["data"]["status"], "cancelled",
        "{:#?}",
        result.body
    );
    assert_eventually(
        "cancelled foreground task did not converge to Idle",
        Duration::from_secs(2),
        || pep.current_task_projection().latest_snapshot() == CurrentTaskStatus::Idle,
    );

    let mut coding = pep
        .stop()
        .expect("PEP stop after task_control cancellation");
    coding
        .stop()
        .expect("MCP Job stop after task_control cancellation");
    assert_eq!(coding.active_processes().unwrap(), 0);
    drop(coding);
    cleanup_test_directory(&workspace);
}

#[test]
fn edit_task_control_cancel_reaches_running_filesystem_hash() {
    let root = repo_root();
    let workspace = temp_workspace();
    let large = workspace.join("large-fs-cancel.bin");
    fs::File::create(&large)
        .unwrap()
        .set_len(8 * 1024 * 1024 * 1024)
        .unwrap();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Edit)
        .expect("PEP listener ready");
    let initialized = initialize(pep.port(), 330);
    let session = initialized.session.expect("downstream MCP session");
    assert_eq!(
        post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .status,
        202
    );

    let port = pep.port();
    let call_session = session.clone();
    let call_started = Instant::now();
    let call = thread::spawn(move || {
        post_with_read_timeout(
            port,
            Some(&call_session),
            &json!({
                "jsonrpc":"2.0",
                "id":"filesystem-cancel-me",
                "method":"tools/call",
                "params":{
                    "name":"filesystem",
                    "arguments":{"action":"hash","path":"large-fs-cancel.bin"}
                }
            }),
            Duration::from_secs(6),
        )
    });

    let running_deadline = Instant::now() + Duration::from_secs(3);
    while !matches!(
        pep.current_task_projection().latest_snapshot(),
        CurrentTaskStatus::Active(ref task) if task.state == TaskExecutionState::Running
    ) {
        assert!(
            Instant::now() < running_deadline,
            "filesystem hash never became actually Running"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let cancel_started = Instant::now();
    let cancel = public_tool_call(
        pep.port(),
        &session,
        331,
        "task_control",
        json!({"action":"cancel"}),
    );
    assert!(
        cancel_started.elapsed() < Duration::from_secs(2),
        "filesystem task_control cancel blocked"
    );
    assert_eq!(
        cancel.body["result"]["structuredContent"]["data"]["state"], "idle",
        "{:#?}",
        cancel.body
    );
    assert!(
        cancel.body["result"]["structuredContent"]["data"]["cancelled_requests"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "{:#?}",
        cancel.body
    );

    let result = call.join().expect("filesystem tools/call client thread");
    assert!(
        call_started.elapsed() < Duration::from_secs(5),
        "filesystem cancellation did not interrupt the large hash"
    );
    assert_eq!(
        result.body["result"]["structuredContent"]["error"]["code"], "ProcessCancelled",
        "{:#?}",
        result.body
    );

    let mut coding = pep.stop().expect("PEP stop after filesystem cancellation");
    coding
        .stop()
        .expect("MCP Job stop after filesystem cancellation");
    assert_eq!(coding.active_processes().unwrap(), 0);
    drop(coding);
    cleanup_test_directory(&workspace);
}

#[test]
fn task_control_cancel_owns_detached_public_command_session() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let pep = PolicyEnforcementRuntime::start(coding, policy(&root), PermissionMode::Full)
        .expect("PEP listener ready");
    let initialized = initialize(pep.port(), 321);
    let session = initialized.session.expect("downstream MCP session");
    let other_session = initialize(pep.port(), 326)
        .session
        .expect("other downstream MCP session");
    assert_eq!(
        post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .status,
        202
    );
    assert_eq!(
        post(
            pep.port(),
            Some(&other_session),
            &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .status,
        202
    );

    let started = Instant::now();
    let running = public_tool_call(
        pep.port(),
        &session,
        322,
        "exec_command",
        json!({
            "command":"Start-Sleep -Seconds 10; Write-Output SHOULD_NOT_COMPLETE",
            "shell":"windows_powershell",
            "yield_time_ms":0,
            "timeout_ms":20000,
            "max_output_bytes":4096
        }),
    );
    assert_eq!(
        running.body["result"]["structuredContent"]["data"]["status"], "running",
        "{:#?}",
        running.body
    );
    let public_session = running.body["result"]["structuredContent"]["data"]["session_id"]
        .as_str()
        .expect("detached public session")
        .to_string();

    let task_id = running.body["result"]["structuredContent"]["data"]["task_id"]
        .as_str()
        .expect("detached task id")
        .to_string();
    let adoption_token = running.body["result"]["structuredContent"]["data"]["adoption_token"]
        .as_str()
        .expect("detached adoption credential")
        .to_string();
    let isolated_cancel = public_tool_call(
        pep.port(),
        &other_session,
        3221,
        "task_control",
        json!({"action":"cancel"}),
    );
    assert_eq!(
        isolated_cancel.body["result"]["structuredContent"]["error"]["code"], "TaskNotOwned",
        "another live MCP session cannot cancel an execution by discovering its ID: {:#?}",
        isolated_cancel.body
    );
    assert_eq!(
        delete(pep.port(), &session),
        204,
        "closing the transport session must not erase a detached PublicSession capability"
    );

    let cross_session_poll = public_tool_call(
        pep.port(),
        &other_session,
        327,
        "command_control",
        json!({"action":"poll","session_id":public_session,"wait_ms":0}),
    );
    assert_eq!(
        cross_session_poll.body["result"]["structuredContent"]["error"]["code"], "TaskNotOwned",
        "{:#?}",
        cross_session_poll.body
    );

    let isolated_list = public_tool_call(
        pep.port(),
        &other_session,
        3271,
        "task_control",
        json!({"action":"list"}),
    );
    assert_eq!(
        isolated_list.body["result"]["structuredContent"]["data"]["tasks"],
        json!([]),
        "task list must not enumerate another MCP session's task capability: {:#?}",
        isolated_list.body
    );
    assert_eq!(
        isolated_list.body["result"]["structuredContent"]["data"]["executions"],
        json!([]),
        "task list must not enumerate another MCP session's detached execution: {:#?}",
        isolated_list.body
    );

    let cross_session_get = public_tool_call(
        pep.port(),
        &other_session,
        3281,
        "task_control",
        json!({"action":"get","task_id":task_id.clone()}),
    );
    assert_eq!(
        cross_session_get.body["result"]["structuredContent"]["error"]["code"], "TaskNotOwned",
        "a TaskId is an identifier, not an authorization credential: {:#?}",
        cross_session_get.body
    );

    let adopted = public_tool_call(
        pep.port(),
        &other_session,
        3280,
        "command_control",
        json!({"action":"adopt","session_id":public_session,"adoption_token":adoption_token}),
    );
    assert_eq!(
        adopted.body["result"]["structuredContent"]["data"]["adopted"],
        true
    );

    let current = public_tool_call(
        pep.port(),
        &other_session,
        3282,
        "task_control",
        json!({"action":"get"}),
    );
    assert_eq!(
        current.body["result"]["structuredContent"]["data"]["current_activity"]["task_id"], task_id,
        "the orphaned current execution must expose its stable TaskId: {:#?}",
        current.body
    );

    let cancel_started = Instant::now();
    let cancel = public_tool_call(
        pep.port(),
        &other_session,
        323,
        "task_control",
        json!({"action":"cancel","task_id":task_id.clone()}),
    );
    assert!(
        cancel_started.elapsed() < Duration::from_secs(2),
        "detached task cancellation blocked"
    );
    assert_eq!(
        cancel.body["result"]["structuredContent"]["data"]["cancellation_requested"], true,
        "an accepted cancellation is not the same fact as a terminal execution: {:#?}",
        cancel.body
    );
    assert!(
        cancel.body["result"]["structuredContent"]["data"]["cancelled_requests"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "{:#?}",
        cancel.body
    );

    let replay = poll_public_command_to_terminal(
        pep.port(),
        &other_session,
        324,
        &public_session,
        Duration::from_secs(30),
    );
    assert_eq!(
        replay.body["result"]["structuredContent"]["ok"], true,
        "{:#?}",
        replay.body
    );
    assert!(replay.body["result"]["structuredContent"]["error"].is_null());
    assert_eq!(
        replay.body["result"]["structuredContent"]["data"]["status"],
        "cancelled"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "detached command ran near its natural duration"
    );

    let task = public_tool_call(
        pep.port(),
        &other_session,
        325,
        "task_control",
        json!({"action":"get"}),
    );
    assert_eq!(
        task.body["result"]["structuredContent"]["data"]["last_activity"]["outcome"], "cancelled",
        "{:#?}",
        task.body
    );
    assert_eq!(
        task.body["result"]["structuredContent"]["data"]["last_activity"]["error_code"],
        "ProcessCancelled"
    );
    assert_eq!(
        pep.current_task_projection().latest_snapshot(),
        CurrentTaskStatus::Idle
    );

    let second = public_tool_call(
        pep.port(),
        &other_session,
        329,
        "exec_command",
        json!({
            "command":"$line=[Console]::In.ReadLine(); Write-Output ('stdin:'+ $line); Start-Sleep -Seconds 10",
            "shell":"windows_powershell",
            "yield_time_ms":0,
            "timeout_ms":20000,
            "max_output_bytes":4096
        }),
    );
    let second_public_session = second.body["result"]["structuredContent"]["data"]["session_id"]
        .as_str()
        .expect("second detached public session")
        .to_string();
    let second_task_id = second.body["result"]["structuredContent"]["data"]["task_id"]
        .as_str()
        .expect("second detached task id")
        .to_string();

    let projection = public_tool_call(
        pep.port(),
        &other_session,
        330,
        "task_control",
        json!({"action":"get"}),
    );
    let projection_data = &projection.body["result"]["structuredContent"]["data"];
    for legacy in [
        "current_workflow",
        "current_command",
        "last_command",
        "last_terminal_command",
        "last_tool",
    ] {
        assert!(
            projection_data.get(legacy).is_none(),
            "legacy parallel projection {legacy} leaked: {projection_data:#?}"
        );
    }
    assert_eq!(projection_data["current_activity"]["state"], "running");
    assert_eq!(projection_data["current_activity"]["kind"], "command");
    assert_eq!(
        projection_data["scheduler"]["foreground_work_running"], 0,
        "detached execution must release the foreground scheduler slot"
    );
    assert_eq!(
        projection_data["scheduler"]["detached_executions_running"],
        1
    );
    assert_ne!(
        projection_data["last_activity"]["task_id"], second_task_id,
        "the completed launch task must not fabricate a completed command activity"
    );

    let write_started = Instant::now();
    let written = public_tool_call(
        pep.port(),
        &other_session,
        332,
        "command_control",
        json!({
            "action":"write",
            "session_id":second_public_session,
            "chars":"cross-session\n",
            "wait_ms":0
        }),
    );
    assert_ne!(
        written.body["result"]["structuredContent"]["error"]["code"], "SessionUnavailable",
        "{:#?}",
        written.body
    );
    assert!(
        write_started.elapsed() < Duration::from_millis(1_500),
        "write exceeded wait_ms plus transport headroom"
    );
    let kill_started = Instant::now();
    let mut killed = public_tool_call(
        pep.port(),
        &other_session,
        333,
        "command_control",
        json!({"action":"kill","session_id":second_public_session,"wait_ms":0}),
    );
    assert!(
        kill_started.elapsed() < Duration::from_millis(1_500),
        "kill exceeded wait_ms plus transport headroom"
    );
    if killed.body["result"]["structuredContent"]["error"]["code"] == "OperationTimedOut" {
        killed = poll_public_command_to_terminal(
            pep.port(),
            &other_session,
            33301,
            &second_public_session,
            Duration::from_secs(30),
        );
    }
    assert_eq!(
        killed.body["result"]["structuredContent"]["data"]["status"], "cancelled",
        "{:#?}",
        killed.body
    );
    let cancelled_poll = public_tool_call(
        pep.port(),
        &other_session,
        3331,
        "command_control",
        json!({"action":"poll","session_id":second_public_session,"wait_ms":0}),
    );
    assert_eq!(
        cancelled_poll.body["result"]["structuredContent"]["ok"], true,
        "kill and terminal poll must share one cancelled envelope: {:#?}",
        cancelled_poll.body
    );
    assert_eq!(
        cancelled_poll.body["result"]["structuredContent"]["data"]["status"],
        "cancelled"
    );
    assert!(cancelled_poll.body["result"]["structuredContent"]["error"].is_null());

    let prepared = public_tool_call(
        pep.port(),
        &other_session,
        334,
        "agent_workflow",
        json!({
            "action":"bugfix",
            "phase":"prepare",
            "objective":"verify durable workflow cancellation after MCP reinitialize"
        }),
    );
    let workflow_task_id = prepared.body["result"]["structuredContent"]["data"]["task_id"]
        .as_str()
        .expect("prepared durable workflow task id")
        .to_string();
    let workflow_adoption_token =
        prepared.body["result"]["structuredContent"]["data"]["adoption_token"]
            .as_str()
            .expect("prepared workflow adoption credential")
            .to_string();
    let reconnected_session = initialize(pep.port(), 3340)
        .session
        .expect("reconnected MCP session");
    let implicit_cross_session_cancel = public_tool_call(
        pep.port(),
        &reconnected_session,
        33401,
        "task_control",
        json!({"action":"cancel"}),
    );
    assert_eq!(
        implicit_cross_session_cancel.body["result"]["structuredContent"]["error"]["code"],
        "TaskNotOwned",
        "untargeted cancel must not report success for another live Session's workflow: {:#?}",
        implicit_cross_session_cancel.body
    );
    let resumed_after_reconnect = public_tool_call(
        pep.port(),
        &reconnected_session,
        33402,
        "agent_workflow",
        json!({"action":"resume","task_id":workflow_task_id.clone(),"adoption_token":workflow_adoption_token}),
    );
    assert_eq!(
        resumed_after_reconnect.body["result"]["structuredContent"]["data"]["state"], "prepared",
        "the stable TaskId must atomically transfer durable workflow ownership after reconnect: {:#?}",
        resumed_after_reconnect.body
    );
    assert_eq!(
        resumed_after_reconnect.body["result"]["structuredContent"]["data"]["task_id"],
        workflow_task_id
    );
    let edited_after_reconnect = public_tool_call(
        pep.port(),
        &reconnected_session,
        3341,
        "agent_workflow",
        json!({
            "action":"bugfix",
            "phase":"edit",
            "task_id":workflow_task_id,
            "patch":"*** Begin Patch\n*** Add File: reconnect-edit.txt\n+owned by durable task capability\n*** End Patch"
        }),
    );
    assert_eq!(
        edited_after_reconnect.body["result"]["structuredContent"]["data"]["state"], "editing",
        "a durable task capability must survive MCP transport reinitialization: {:#?}",
        edited_after_reconnect.body
    );
    assert!(workspace.join("reconnect-edit.txt").is_file());
    let visible_workflow = public_tool_call(
        pep.port(),
        &reconnected_session,
        3342,
        "task_control",
        json!({"action":"get"}),
    );
    let visible_workflow_data = &visible_workflow.body["result"]["structuredContent"]["data"];
    assert_eq!(
        visible_workflow_data["current_activity"]["task_id"], workflow_task_id,
        "a blocking durable workflow must expose its stable identity"
    );
    assert!(visible_workflow_data.get("current_step").is_none());
    assert!(visible_workflow_data.get("next_step").is_none());
    let cancel_workflow = public_tool_call(
        pep.port(),
        &reconnected_session,
        335,
        "task_control",
        json!({"action":"cancel","task_id":workflow_task_id}),
    );
    assert_eq!(
        cancel_workflow.body["result"]["structuredContent"]["data"]["workflow_cancelled"], true,
        "{:#?}",
        cancel_workflow.body
    );
    assert_eq!(
        cancel_workflow.body["result"]["structuredContent"]["data"]["durable_task_cancelled"], true,
        "{:#?}",
        cancel_workflow.body
    );
    let replacement = public_tool_call(
        pep.port(),
        &reconnected_session,
        336,
        "agent_workflow",
        json!({
            "action":"diagnose",
            "phase":"prepare",
            "objective":"replacement workflow after prior durable terminal"
        }),
    );
    assert_eq!(
        replacement.body["result"]["structuredContent"]["data"]["state"], "prepared",
        "{:#?}",
        replacement.body
    );
    let replacement_task_id = replacement.body["result"]["structuredContent"]["data"]["task_id"]
        .as_str()
        .expect("replacement durable workflow task id");
    let replacement_cancelled = public_tool_call(
        pep.port(),
        &reconnected_session,
        337,
        "task_control",
        json!({"action":"cancel","task_id":replacement_task_id}),
    );
    assert_eq!(
        replacement_cancelled.body["result"]["structuredContent"]["data"]["workflow_cancelled"],
        true,
        "{:#?}",
        replacement_cancelled.body
    );
    let after_workflow_terminal = public_tool_call(
        pep.port(),
        &reconnected_session,
        338,
        "task_control",
        json!({"action":"get"}),
    );
    assert!(
        after_workflow_terminal.body["result"]["structuredContent"]["data"]["current_activity"]
            .is_null(),
        "terminal workflow must not remain as current presentation: {:#?}",
        after_workflow_terminal.body
    );

    let mut coding = pep.stop().expect("PEP stop after detached cancellation");
    coding
        .stop()
        .expect("MCP Job stop after detached cancellation");
    assert_eq!(coding.active_processes().unwrap(), 0);
    drop(coding);
    cleanup_test_directory(&workspace);
}

#[test]
fn public_timeout_converges_without_windows_ctrl_break_debug_mode() {
    let fixture = PublicRuntimeFixture::start(PermissionMode::Full);
    let (client, _) = PublicMcpClient::connect(fixture.runtime().port(), 326);
    let timed_out = client.call_tool(
        "exec_command",
        json!({
            "command":"Start-Sleep -Seconds 10; Write-Output SHOULD_NOT_COMPLETE",
            "shell":"windows_powershell",
            "yield_time_ms":10000,
            "timeout_ms":300,
            "max_output_bytes":4096
        }),
    );
    assert_eq!(
        timed_out.body["result"]["isError"], true,
        "{:#?}",
        timed_out.body
    );
    assert_eq!(
        timed_out.body["result"]["structuredContent"]["error"]["code"], "ProcessTimedOut",
        "{:#?}",
        timed_out.body
    );
    assert_eq!(
        timed_out.body["result"]["structuredContent"]["data"]["status"], "timed_out",
        "{:#?}",
        timed_out.body
    );
    let rendered = serde_json::to_string(&timed_out.body).unwrap();
    assert!(
        !rendered.contains("Entering debug mode"),
        "Windows timeout leaked CTRL_BREAK PowerShell debug behavior: {rendered}"
    );

    fixture.shutdown();
}

#[test]
fn elevated_exec_is_broker_only_mode_gated_cancelable_and_secret_safe() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let fake = Arc::new(FakePrivilegedExecution::active());
    let privileged: Arc<dyn PrivilegedExecution> = fake.clone();
    let pep = PolicyEnforcementRuntime::start_with_privilege(
        coding,
        policy(&root),
        PermissionMode::Elevated,
        privileged,
    )
    .expect("PEP with privileged route ready");
    let initialized = initialize(pep.port(), 300);
    let session = initialized.session.expect("downstream MCP session");

    let tools = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":301,"method":"tools/list","params":{}}),
    );
    let elevated_count = tools.body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|tool| tool["name"] == "elevated_exec")
        .count();
    assert_eq!(elevated_count, 1);
    let elevated_tool = tools.body["result"]["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|tool| tool["name"] == "elevated_exec"))
        .expect("elevated_exec tool definition");
    assert_eq!(elevated_tool["inputSchema"]["type"], "object");
    assert!(elevated_tool["inputSchema"]["properties"]["operation"].is_object());
    assert!(elevated_tool["inputSchema"]["properties"]["shell"].is_object());
    assert!(elevated_tool["inputSchema"]["properties"]["action"].is_object());
    assert!(elevated_tool["inputSchema"]["properties"]["program"].is_object());
    assert!(elevated_tool["outputSchema"]["oneOf"].is_array());
    let elevated_error_schema = elevated_tool["outputSchema"]["oneOf"]
        .as_array()
        .and_then(|branches| {
            branches
                .iter()
                .find(|branch| branch["properties"]["ok"]["const"] == Value::Bool(false))
        })
        .expect("elevated_exec exposes common error envelope branch");
    for field in [
        "ok",
        "state",
        "summary",
        "task_id",
        "warnings",
        "next_step",
        "output_refs",
        "data",
        "error",
    ] {
        assert!(
            elevated_error_schema["properties"][field].is_object(),
            "elevated_exec error output schema lost {field}"
        );
    }
    for field in [
        "code",
        "error_code",
        "phase",
        "cause",
        "http_status",
        "message",
        "retryable",
        "rule_category",
        "remediation",
    ] {
        assert!(
            elevated_error_schema["properties"]["error"]["properties"][field].is_object(),
            "elevated_exec error diagnostics schema lost {field}"
        );
    }

    fake.set_state(PrivilegeState::AwaitingUac);
    let awaiting_tools = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":306,"method":"tools/list","params":{}}),
    );
    assert!(
        awaiting_tools.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "elevated_exec")
    );
    fake.set_state(PrivilegeState::Active {
        broker_generation: crate::state::GenerationId::new(77),
    });
    let active_tools_same_session = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":3063,"method":"tools/list","params":{}}),
    );
    assert_eq!(active_tools_same_session.status, 200);
    assert!(
        active_tools_same_session.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "elevated_exec")
    );

    let secret = "LB012_SYNTHETIC_PEP_SECRET";
    let reviewed_program = crate::execution::policy::reviewed_elevated_program()
        .expect("reviewed Windows diagnostic exists")
        .to_string_lossy()
        .into_owned();
    fs::write(workspace.join("whoami.exe"), b"untrusted same-name binary").unwrap();
    for (index, arguments) in [
        json!({"program":"C:/Windows/System32/cmd.exe","args":["/c","whoami"],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
        json!({"program":"C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe","args":["-Command",format!("Set-Content C:/ProgramData/LocalBridge/settings.json {secret}")],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
        json!({"program":"C:/Windows/System32/reg.exe","args":["add","HKLM\\Software\\LocalBridge"],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
        json!({"program":workspace.join("whoami.exe").to_string_lossy(),"args":["/user"],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
    ].into_iter().enumerate() {
        let denied = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":310 + index as u64,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":arguments}
            }),
        );
        assert_tool_error(&denied, "ElevatedOperationNotReviewed");
        let structured = denied.body["result"]["structuredContent"]
            .as_object()
            .expect("elevated denial has structuredContent");
        let allowed = elevated_error_schema["properties"]
            .as_object()
            .expect("elevated error schema properties");
        assert!(
            structured.keys().all(|field| allowed.contains_key(field)),
            "elevated denial contains a top-level field rejected by outputSchema: {structured:?}"
        );
        let error = structured["error"]
            .as_object()
            .expect("elevated denial has typed error");
        let allowed_error = elevated_error_schema["properties"]["error"]["properties"]
            .as_object()
            .expect("elevated diagnostic schema properties");
        assert!(
            error.keys().all(|field| allowed_error.contains_key(field)),
            "elevated denial diagnostic contains a field rejected by outputSchema: {error:?}"
        );
        assert_eq!(error["error_code"], "Denied");
        assert_eq!(error["phase"], "policy");
        assert!(!denied.body.to_string().contains(secret));
        assert_eq!(fake.start_count(), 0, "unreviewed elevated request reached Broker");
    }

    let port = pep.port();
    let call_session = session.clone();
    let call_program = reviewed_program.clone();
    let call = thread::spawn(move || {
        post(
            port,
            Some(&call_session),
            &json!({
                "jsonrpc":"2.0",
                "id":"elevated-cancel",
                "method":"tools/call",
                "params":{
                    "name":"elevated_exec",
                    "arguments":{
                        "program":call_program,
                        "args":["/user"],
                        "workdir":null,
                        "timeout_ms":10000,
                        "max_output_bytes":4096
                    }
                }
            }),
        )
    });
    let running_deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        match pep.current_task_projection().snapshot() {
            CurrentTaskStatus::Active(ref task)
                if task.kind == TaskKind::ElevatedOperation
                    && task.state == TaskExecutionState::Running
                    && fake.start_count() == 1 =>
            {
                assert_eq!(task.summary, SafeTaskSummary::Omitted);
                assert!(!format!("{task:?}").contains(secret));
                break;
            }
            _ => {}
        }
        assert!(
            std::time::Instant::now() < running_deadline,
            "elevated call never projected Running (broker_starts={}, request_finished={})",
            fake.start_count(),
            call.is_finished()
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fake.start_count(), 1);
    let cancelled = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0",
            "method":"notifications/cancelled",
            "params":{"requestId":"elevated-cancel","reason":"LB-012 test"}
        }),
    );
    assert_eq!(cancelled.status, 202);
    let cancelled_result = call.join().unwrap();
    assert_eq!(
        cancelled_result.body["result"]["structuredContent"]["outcome"],
        "cancelled"
    );
    assert_eq!(cancelled_result.body["result"]["isError"], true);
    let cancelled_timing = pep.current_task_projection().timing_snapshot();
    assert_eq!(cancelled_timing.status, CurrentTaskStatus::Idle);
    assert_eq!(
        cancelled_timing.last_tool.as_ref().map(|tool| tool.kind),
        Some(TaskKind::ElevatedOperation)
    );

    pep.set_simulated_permission_for_test(PermissionMode::Full);
    let full_tools = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":307,"method":"tools/list","params":{}}),
    );
    assert_eq!(full_tools.status, 200);
    assert!(
        full_tools.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "elevated_exec")
    );
    let full_denied = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":302,"method":"tools/call",
            "params":{"name":"elevated_exec","arguments":{
                "program":reviewed_program.clone(),"args":["/user"],"workdir":null,
                "timeout_ms":1000,"max_output_bytes":1024
            }}
        }),
    );
    assert_tool_error(&full_denied, "PrivilegedRouteUnavailable");
    assert_eq!(fake.start_count(), 1);
    assert_eq!(
        pep.current_task_projection().snapshot(),
        CurrentTaskStatus::Idle
    );

    pep.set_simulated_permission_for_test(PermissionMode::Edit);
    let refreshed_full_for_edit = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":3021,"method":"tools/list","params":{}}),
    );
    assert_eq!(
        refreshed_full_for_edit.status, 200,
        "permission changes must refresh the catalog without terminating the MCP session"
    );
    let edit_tools = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":3023,"method":"tools/list","params":{}}),
    );
    assert!(
        edit_tools.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "elevated_exec")
    );
    let edit_denied = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":3024,"method":"tools/call","params":{"name":"elevated_exec","arguments":{
            "program":reviewed_program.clone(),"args":["/user"],"workdir":null,"timeout_ms":1000,"max_output_bytes":1024
        }}}),
    );
    assert_tool_error(&edit_denied, "PrivilegedRouteUnavailable");
    assert_eq!(
        pep.current_task_projection().snapshot(),
        CurrentTaskStatus::Idle
    );

    pep.set_simulated_permission_for_test(PermissionMode::Elevated);
    fake.set_state(PrivilegeState::AwaitingUac);
    let refreshed_edit_session = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":3030,"method":"tools/list","params":{}}),
    );
    assert_eq!(refreshed_edit_session.status, 200);
    let elevated_awaiting_tools = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":3032,"method":"tools/list","params":{}}),
    );
    assert!(
        elevated_awaiting_tools.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "elevated_exec")
    );
    let awaiting = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":303,"method":"tools/call",
            "params":{"name":"elevated_exec","arguments":{
                "program":reviewed_program.clone(),"args":["/user"],"workdir":null,
                "timeout_ms":1000,"max_output_bytes":1024
            }}
        }),
    );
    assert_tool_error(&awaiting, "PrivilegedRouteUnavailable");
    assert_eq!(fake.start_count(), 1);
    assert_eq!(
        pep.current_task_projection().snapshot(),
        CurrentTaskStatus::Idle
    );

    let control_plane = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":304,"method":"tools/call",
            "params":{"name":"request_permissions","arguments":{"permission":"admin"}}
        }),
    );
    assert_tool_error(&control_plane, "CapabilityDenied");
    assert_eq!(
        pep.current_task_projection().snapshot(),
        CurrentTaskStatus::Idle
    );

    fake.set_state(PrivilegeState::Active {
        broker_generation: crate::state::GenerationId::new(78),
    });
    let active_tools = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":3041,"method":"tools/list","params":{}}),
    );
    assert_eq!(active_tools.status, 200);
    assert!(
        active_tools.body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "elevated_exec")
    );
    fake.complete.store(true, Ordering::Release);
    let completed_started = Instant::now();
    let completed = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":305,"method":"tools/call",
            "params":{"name":"elevated_exec","arguments":{
                "program":reviewed_program.clone(),"args":["/user"],"workdir":null,
                "timeout_ms":1000,"max_output_bytes":1024
            }}
        }),
    );
    let completed_round_trip = completed_started.elapsed();
    assert_eq!(
        completed.body["result"]["structuredContent"]["outcome"],
        "completed"
    );
    assert_eq!(
        completed.body["result"]["structuredContent"]["stdout"],
        "LB012_FAKE_PRIVILEGED_OK"
    );
    assert_eq!(
        completed.body["result"]["structuredContent"]["stderr"],
        "LB012_FAKE_PRIVILEGED_ERR"
    );
    assert_eq!(
        completed.body["result"]["content"][0]["text"],
        "LB012_FAKE_PRIVILEGED_OK\nLB012_FAKE_PRIVILEGED_ERR"
    );
    assert_eq!(fake.start_count(), 2);
    assert!(
        completed_round_trip < Duration::from_millis(500),
        "UI retention must not delay Broker response: {completed_round_trip:?}"
    );
    let completed_timing = pep.current_task_projection().timing_snapshot();
    assert_eq!(completed_timing.status, CurrentTaskStatus::Idle);
    assert_eq!(
        completed_timing.last_tool.as_ref().map(|tool| tool.kind),
        Some(TaskKind::ElevatedOperation)
    );

    fake.complete.store(false, Ordering::Release);
    let first_port = pep.port();
    let first_session = session.clone();
    let first_program = reviewed_program.clone();
    let first = thread::spawn(move || {
        post(
            first_port,
            Some(&first_session),
            &json!({
                "jsonrpc":"2.0","id":"serialized-first","method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "program":first_program,"args":["/user"],"workdir":null,
                    "timeout_ms":5000,"max_output_bytes":1024
                }}
            }),
        )
    });
    let first_deadline = std::time::Instant::now() + Duration::from_secs(3);
    while fake.start_count() != 3 {
        assert!(
            std::time::Instant::now() < first_deadline,
            "first serialized elevated call did not start"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(
        pep.current_task_projection().snapshot(),
        CurrentTaskStatus::Active(ref task)
            if task.kind == TaskKind::ElevatedOperation && task.state == TaskExecutionState::Running
    ));

    let second_port = pep.port();
    let second_session = session.clone();
    let second_program = reviewed_program.clone();
    let second = thread::spawn(move || {
        post(
            second_port,
            Some(&second_session),
            &json!({
                "jsonrpc":"2.0","id":"serialized-second","method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "program":second_program,"args":["/groups"],"workdir":null,
                    "timeout_ms":5000,"max_output_bytes":1024
                }}
            }),
        )
    });
    thread::sleep(Duration::from_millis(150));
    assert_eq!(
        fake.start_count(),
        3,
        "second elevated call bypassed single execution gate"
    );
    assert!(matches!(
        pep.current_task_projection().snapshot(),
        CurrentTaskStatus::Active(ref task) if task.state == TaskExecutionState::Running
    ));
    fake.complete.store(true, Ordering::Release);
    assert_eq!(
        first.join().unwrap().body["result"]["structuredContent"]["outcome"],
        "completed"
    );
    assert_eq!(
        second.join().unwrap().body["result"]["structuredContent"]["outcome"],
        "completed"
    );
    assert_eq!(fake.start_count(), 4);
    let serialized_timing = pep.current_task_projection().timing_snapshot();
    assert_eq!(serialized_timing.status, CurrentTaskStatus::Idle);
    assert_eq!(
        serialized_timing.last_tool.as_ref().map(|tool| tool.kind),
        Some(TaskKind::ElevatedOperation)
    );

    let mut coding = pep.stop().expect("PEP stop after privileged routing");
    coding.stop().expect("MCP stop after privileged routing");
    drop(coding);
    cleanup_test_directory(&workspace);
}

#[test]
fn filesystem_escalates_only_after_elevated_authority_converges() {
    let root = repo_root();
    let workspace = temp_workspace();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let outside =
        std::env::temp_dir().join(format!("lb43-fs-outside-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&outside).unwrap();
    let outside_file = outside.join("outside.txt");
    fs::write(&outside_file, b"outside").unwrap();

    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let fake = Arc::new(FakePrivilegedExecution::active());
    let privileged: Arc<dyn PrivilegedExecution> = fake.clone();
    let pep = PolicyEnforcementRuntime::start_with_privilege(
        coding,
        policy(&root),
        PermissionMode::Full,
        privileged,
    )
    .expect("PEP with schema43 filesystem route ready");

    let full_session = initialize(pep.port(), 4300).session.unwrap();
    let full_denied = public_tool_call(
        pep.port(),
        &full_session,
        4301,
        "filesystem",
        json!({"action":"read","path":outside_file.to_string_lossy()}),
    );
    assert_tool_error(&full_denied, "WorkspaceDenied");
    assert_eq!(fake.structured_filesystem_count(), 0);

    pep.set_simulated_permission_for_test(PermissionMode::Elevated);
    fake.set_state(PrivilegeState::AwaitingUac);
    let elevated_session = initialize(pep.port(), 4302).session.unwrap();
    let awaiting = public_tool_call(
        pep.port(),
        &elevated_session,
        4303,
        "filesystem",
        json!({"action":"read","path":outside_file.to_string_lossy()}),
    );
    assert_tool_error(&awaiting, "WorkspaceDenied");
    assert_eq!(fake.structured_filesystem_count(), 0);

    fake.set_state(PrivilegeState::Active {
        broker_generation: crate::state::GenerationId::new(78),
    });
    let elevated = public_tool_call(
        pep.port(),
        &elevated_session,
        4304,
        "filesystem",
        json!({"action":"read","path":outside_file.to_string_lossy(),"max_bytes":4096}),
    );
    assert_eq!(
        elevated.body["result"]["isError"], false,
        "{:#?}",
        elevated.body
    );
    assert_eq!(
        fake.structured_filesystem_count(),
        1,
        "converged Elevated filesystem must route through the administrator Broker"
    );
    for (request_id, arguments) in [
        (
            43041,
            json!({
                "action":"write",
                "path":outside.join("written.txt").to_string_lossy(),
                "content":"must-not-be-written"
            }),
        ),
        (
            43042,
            json!({"action":"delete","path":outside_file.to_string_lossy()}),
        ),
    ] {
        let mutation = public_tool_call(
            pep.port(),
            &elevated_session,
            request_id,
            "filesystem",
            arguments,
        );
        assert_eq!(
            mutation.body["result"]["isError"], false,
            "{:#?}",
            mutation.body
        );
    }
    assert!(!outside.join("written.txt").exists());
    assert!(outside_file.exists());
    assert_eq!(fake.structured_filesystem_count(), 3);

    let inside = public_tool_call(
        pep.port(),
        &elevated_session,
        4305,
        "filesystem",
        json!({"action":"read","path":"probe.txt"}),
    );
    assert_eq!(
        inside.body["result"]["isError"], false,
        "{:#?}",
        inside.body
    );
    assert_eq!(
        inside.body["result"]["structuredContent"]["data"]["content"],
        "LB43_FAKE_ADMIN"
    );
    assert_eq!(
        fake.structured_filesystem_count(),
        4,
        "converged Elevated authority owns workspace-contained structured paths too"
    );

    let control_plane = public_tool_call(
        pep.port(),
        &elevated_session,
        4306,
        "filesystem",
        json!({"action":"delete","path":"C:\\ProgramData\\LocalBridge\\settings.json"}),
    );
    assert_tool_error(&control_plane, "PolicyDenied");
    assert_eq!(fake.structured_filesystem_count(), 4);

    let mut coding = pep
        .stop()
        .expect("PEP stop after schema43 filesystem routing");
    coding
        .stop()
        .expect("MCP stop after schema43 filesystem routing");
    drop(coding);
    cleanup_test_directory(&workspace);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn typed_administrator_process_shell_and_filesystem_routes_are_broker_only() {
    let root = repo_root();
    let workspace = temp_workspace();
    let coding = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(
            &root,
            &workspace,
            free_port(),
            CodingToolsPermissionMode::Trusted,
        ),
        InternalBearer::new(SYNTHETIC_BEARER).unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled MCP ready");
    let fake = Arc::new(FakePrivilegedExecution::active());
    fake.complete.store(true, Ordering::Release);
    let privileged: Arc<dyn PrivilegedExecution> = fake.clone();
    let pep = PolicyEnforcementRuntime::start_with_privilege(
        coding,
        policy(&root),
        PermissionMode::Elevated,
        privileged,
    )
    .expect("PEP with typed administrator route ready");
    let initialized = initialize(pep.port(), 500);
    let session = initialized.session.expect("downstream MCP session");

    let tools = post(
        pep.port(),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":501,"method":"tools/list","params":{}}),
    );
    let schema = &tools.body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "elevated_exec")
        .unwrap()["inputSchema"];
    assert_eq!(schema["type"], "object");
    assert!(schema.get("oneOf").is_none());
    assert_eq!(
        schema["properties"]["operation"]["enum"],
        json!(["process", "shell", "filesystem"])
    );
    for property in [
        "program",
        "shell",
        "action",
        "path",
        "timeout_ms",
        "max_output_bytes",
    ] {
        assert!(
            schema["properties"][property].is_object(),
            "elevated_exec schema lost {property}"
        );
    }
    let output_schema = &tools.body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "elevated_exec")
        .unwrap()["outputSchema"];
    let execution_schema = &output_schema["oneOf"][1]["properties"];
    for property in [
        "stdout",
        "stderr",
        "stdout_truncated",
        "stderr_truncated",
        "output_refs",
    ] {
        assert!(
            execution_schema[property].is_object(),
            "elevated_exec output schema lost {property}"
        );
    }

    let reviewed_program = crate::execution::policy::reviewed_elevated_program()
        .expect("trusted System32 diagnostic exists")
        .to_string_lossy()
        .into_owned();
    let process = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":502,"method":"tools/call",
            "params":{"name":"elevated_exec","arguments":{
                "operation":"process","program":reviewed_program,"args":["/user"],
                "workdir":null,"timeout_ms":1000,"max_output_bytes":4096
            }}
        }),
    );
    assert_eq!(
        process.body["result"]["structuredContent"]["outcome"],
        "completed"
    );
    assert_eq!(
        process.body["result"]["structuredContent"]["stdout"],
        "LB012_FAKE_PRIVILEGED_OK"
    );
    assert_eq!(
        process.body["result"]["structuredContent"]["stderr"],
        "LB012_FAKE_PRIVILEGED_ERR"
    );
    assert_eq!(fake.start_count(), 1);

    let shell = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":503,"method":"tools/call",
            "params":{"name":"elevated_exec","arguments":{
                "operation":"shell","shell":"cmd","command":"whoami /user",
                "workdir":"C:\\Windows\\Temp","timeout_ms":1000,"max_output_bytes":4096
            }}
        }),
    );
    assert_eq!(
        shell.body["result"]["structuredContent"]["outcome"],
        "completed"
    );
    assert_eq!(fake.start_count(), 2);

    let shell_path_denied = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":504,"method":"tools/call",
            "params":{"name":"elevated_exec","arguments":{
                "operation":"process","program":"C:\\Windows\\System32\\cmd.exe",
                "args":["/c","whoami"],"workdir":null,
                "timeout_ms":1000,"max_output_bytes":4096
            }}
        }),
    );
    assert_tool_error(&shell_path_denied, "ElevatedOperationNotReviewed");
    assert_eq!(fake.start_count(), 2);

    let opaque_helper = std::env::current_exe().expect("test helper executable");
    let opaque_helper_denied = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":5041,"method":"tools/call",
            "params":{"name":"elevated_exec","arguments":{
                "operation":"process","program":opaque_helper.to_string_lossy(),
                "args":[],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096
            }}
        }),
    );
    assert_tool_error(&opaque_helper_denied, "ElevatedOperationNotReviewed");
    assert_eq!(
        fake.start_count(),
        2,
        "opaque administrator helper must be denied before Broker dispatch"
    );

    let filesystem = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":505,"method":"tools/call",
            "params":{"name":"elevated_exec","arguments":{
                "operation":"filesystem","action":"read_file","path":"C:\\Windows\\win.ini",
                "destination":null,"content_base64":null,"recursive":false
            }}
        }),
    );
    assert_eq!(
        filesystem.body["result"]["structuredContent"]["operation"],
        "filesystem"
    );
    assert_eq!(
        filesystem.body["result"]["structuredContent"]["result"]["path"],
        "C:\\Windows\\win.ini"
    );
    assert_eq!(
        fake.start_count(),
        2,
        "filesystem incorrectly used process execution"
    );

    let control_plane_denied = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":506,"method":"tools/call",
            "params":{"name":"elevated_exec","arguments":{
                "operation":"filesystem","action":"delete","path":"C:\\ProgramData\\LocalBridge",
                "destination":null,"content_base64":null,"recursive":true
            }}
        }),
    );
    assert_tool_error(&control_plane_denied, "ElevatedOperationNotReviewed");
    assert_eq!(fake.start_count(), 2);

    let retained = post(
        pep.port(),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":507,"method":"tools/call",
            "params":{"name":"elevated_exec","arguments":{
                "operation":"process","program":reviewed_program,"args":["/user"],
                "workdir":null,"timeout_ms":1000,"max_output_bytes":20000
            }}
        }),
    );
    let retained_data = &retained.body["result"]["structuredContent"];
    assert_eq!(retained_data["stdout"].as_str().unwrap().len(), 8 * 1024);
    assert_eq!(retained_data["stderr"].as_str().unwrap().len(), 8 * 1024);
    assert_eq!(retained_data["stdout_truncated"], true);
    assert_eq!(retained_data["stderr_truncated"], true);
    let stdout_ref = retained_data["output_refs"]["stdout"].as_str().unwrap();
    let stderr_ref = retained_data["output_refs"]["stderr"].as_str().unwrap();
    assert!(stdout_ref.starts_with("lb-output-"));
    assert!(stderr_ref.starts_with("lb-output-"));
    let stdout_page = public_tool_call(
        pep.port(),
        &session,
        508,
        "command_control",
        json!({"action":"read","output_ref":stdout_ref,"stream":"stdout","offset":0,"limit":20000}),
    );
    assert_eq!(
        stdout_page.body["result"]["structuredContent"]["data"]["content"]
            .as_str()
            .unwrap()
            .len(),
        9_000
    );
    let stderr_page = public_tool_call(
        pep.port(),
        &session,
        509,
        "command_control",
        json!({"action":"read","output_ref":stderr_ref,"stream":"stderr","offset":0,"limit":20000}),
    );
    assert_eq!(
        stderr_page.body["result"]["structuredContent"]["data"]["content"]
            .as_str()
            .unwrap()
            .len(),
        9_000
    );
    assert_eq!(fake.start_count(), 3);

    let mut coding = pep
        .stop()
        .expect("PEP stop after typed administrator routing");
    coding
        .stop()
        .expect("MCP stop after typed administrator routing");
    drop(coding);
    cleanup_test_directory(&workspace);
}
