use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::server::CURRENT_PROTOCOL_VERSION;
use super::{
    CodingToolsPermissionMode, CodingToolsRuntime, CodingToolsRuntimeConfig, InternalBearer,
    PolicyEnforcementRuntime,
};
use crate::execution::policy::CapabilityPolicy;
use crate::state::PermissionMode;

const TEST_BEARER: &str = "LOCALBRIDGE_TEST_RUNTIME_BEARER_DO_NOT_LEAK";

pub(crate) struct ClientResponse {
    pub(crate) status: u16,
    pub(crate) session: Option<String>,
    pub(crate) body: Value,
}

pub(crate) struct RawHttpResponse {
    pub(crate) status: u16,
    pub(crate) session: Option<String>,
    pub(crate) content_type: Option<String>,
    pub(crate) body: Vec<u8>,
}

pub(crate) fn assert_tool_error(response: &ClientResponse, expected_code: &str) {
    assert_eq!(response.status, 200, "{:#?}", response.body);
    assert!(response.body.get("error").is_none(), "{:#?}", response.body);
    assert_eq!(
        response.body["result"]["isError"], true,
        "{:#?}",
        response.body
    );
    assert_eq!(
        response.body["result"]["structuredContent"]["error"]["code"], expected_code,
        "{:#?}",
        response.body
    );
}

pub(crate) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has repository parent")
        .to_path_buf()
}

pub(crate) fn temp_workspace() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "localbridge-mcp-test-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    // This content is part of the shared black-box fixture contract. Keep it
    // stable so behavior tests assert the filesystem route, not fixture drift.
    fs::write(path.join("probe.txt"), b"LB009 PEP\n").unwrap();
    path
}

pub(crate) fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.local_addr().unwrap().port()
}

pub(crate) fn cleanup_test_directory(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match fs::remove_dir_all(path) {
            Ok(()) => return,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Other
                ) && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => panic!("remove test workspace {}: {error}", path.display()),
        }
    }
}

pub(crate) fn policy(root: &Path) -> CapabilityPolicy {
    CapabilityPolicy::load(&root.join("runtime-policy.toml")).unwrap()
}

pub(crate) struct PublicRuntimeFixture {
    workspace: PathBuf,
    runtime: Option<PolicyEnforcementRuntime>,
    cleaned: bool,
}

impl PublicRuntimeFixture {
    pub(crate) fn start(permission: PermissionMode) -> Self {
        Self::start_in(temp_workspace(), permission)
    }

    pub(crate) fn start_in(workspace: PathBuf, permission: PermissionMode) -> Self {
        let root = repo_root();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(TEST_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP test runtime ready");
        let runtime = PolicyEnforcementRuntime::start(coding, policy(&root), permission)
            .expect("public MCP test runtime ready");
        Self {
            workspace,
            runtime: Some(runtime),
            cleaned: false,
        }
    }

    pub(crate) fn runtime(&self) -> &PolicyEnforcementRuntime {
        self.runtime.as_ref().expect("test runtime is active")
    }

    pub(crate) fn shutdown(mut self) {
        let runtime = self.runtime.take().expect("test runtime is active");
        let mut coding = runtime.stop().expect("public MCP test runtime stops");
        coding.stop().expect("bundled MCP test runtime stops");
        assert_eq!(
            coding.active_processes().unwrap(),
            0,
            "test fixture leaked a managed process"
        );
        cleanup_test_directory(&self.workspace);
        self.cleaned = true;
    }

    fn stop_best_effort(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            if let Ok(mut coding) = runtime.stop() {
                let _ = coding.stop();
            }
        }
    }
}

impl Drop for PublicRuntimeFixture {
    fn drop(&mut self) {
        self.stop_best_effort();
        if !self.cleaned {
            let _ = fs::remove_dir_all(&self.workspace);
        }
    }
}

pub(crate) fn post(port: u16, session: Option<&str>, payload: &Value) -> ClientResponse {
    // Process-backed requests may include cold process creation and antivirus
    // scanning. The socket budget is intentionally larger than command budgets;
    // lifecycle deadlines belong to the command driver below.
    post_with_read_timeout(port, session, payload, Duration::from_secs(30))
}

pub(crate) fn post_with_read_timeout(
    port: u16,
    session: Option<&str>,
    payload: &Value,
    read_timeout: Duration,
) -> ClientResponse {
    let body = serde_json::to_vec(payload).unwrap();
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(read_timeout)).unwrap();
    let mut request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: {CURRENT_PROTOCOL_VERSION}\r\nConnection: close\r\nContent-Length: {}\r\n",
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
    parse_client_response(stream)
}

pub(crate) fn delete(port: u16, session: &str) -> u16 {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let request = format!(
        "DELETE /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nMcp-Session-Id: {session}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    parse_client_response(stream).status
}

pub(crate) fn get_sse(port: u16, session: &str) -> RawHttpResponse {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let request = format!(
        "GET /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: text/event-stream\r\nMCP-Protocol-Version: {CURRENT_PROTOCOL_VERSION}\r\nMcp-Session-Id: {session}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    parse_raw_http_response(stream)
}

fn parse_raw_http_response(mut stream: TcpStream) -> RawHttpResponse {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let headers = std::str::from_utf8(&bytes[..split]).unwrap();
    let mut lines = headers.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let mut session = None;
    let mut content_type = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Mcp-Session-Id") {
                session = Some(value.trim().to_string());
            } else if name.eq_ignore_ascii_case("Content-Type") {
                content_type = Some(value.trim().to_string());
            }
        }
    }
    RawHttpResponse {
        status,
        session,
        content_type,
        body: bytes[(split + 4)..].to_vec(),
    }
}

pub(crate) fn parse_client_response(mut stream: TcpStream) -> ClientResponse {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let headers = std::str::from_utf8(&bytes[..split]).unwrap();
    let mut lines = headers.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let session = lines.find_map(|line| {
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
    ClientResponse {
        status,
        session,
        body,
    }
}

pub(crate) fn initialize(port: u16, id: u64) -> ClientResponse {
    post(
        port,
        None,
        &json!({
            "jsonrpc":"2.0",
            "id":id,
            "method":"initialize",
            "params":{
                "protocolVersion":CURRENT_PROTOCOL_VERSION,
                "capabilities":{},
                "clientInfo":{"name":"localbridge-test-client","version":"1"}
            }
        }),
    )
}

pub(crate) fn public_tool_call(
    port: u16,
    session: &str,
    id: u64,
    name: &str,
    arguments: Value,
) -> ClientResponse {
    post(
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

#[derive(Debug)]
pub(crate) struct PublicMcpClient {
    port: u16,
    session: String,
    next_request_id: AtomicU64,
}

impl PublicMcpClient {
    pub(crate) fn connect(port: u16, first_request_id: u64) -> (Self, ClientResponse) {
        let initialized = initialize(port, first_request_id);
        let session = initialized
            .session
            .clone()
            .expect("initialize returns an MCP-session-scoped identity");
        (
            Self {
                port,
                session,
                next_request_id: AtomicU64::new(first_request_id.saturating_add(1)),
            },
            initialized,
        )
    }

    pub(crate) fn call_tool(&self, name: &str, arguments: Value) -> ClientResponse {
        public_tool_call(
            self.port,
            &self.session,
            self.next_request_id.fetch_add(1, Ordering::Relaxed),
            name,
            arguments,
        )
    }

    pub(crate) fn start_detached_command(&self, arguments: Value) -> DetachedCommand {
        let response = self.call_tool("exec_command", arguments);
        DetachedCommand::from_response(self, response)
    }
}

pub(crate) struct DetachedCommand<'a> {
    client: &'a PublicMcpClient,
    session_id: String,
    output: String,
    last_response: ClientResponse,
}

impl<'a> DetachedCommand<'a> {
    fn from_response(client: &'a PublicMcpClient, response: ClientResponse) -> Self {
        let data = &response.body["result"]["structuredContent"]["data"];
        assert_eq!(data["status"], "running", "{:#?}", response.body);
        let session_id = data["session_id"]
            .as_str()
            .expect("running command has PublicSessionId")
            .to_string();
        let output = data["output"].as_str().unwrap_or_default().to_string();
        Self {
            client,
            session_id,
            output,
            last_response: response,
        }
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn output(&self) -> &str {
        &self.output
    }

    pub(crate) fn poll(&mut self, wait_ms: u64) -> &ClientResponse {
        let response = self.client.call_tool(
            "command_control",
            json!({"action":"poll","session_id":self.session_id,"wait_ms":wait_ms}),
        );
        self.observe(response)
    }

    pub(crate) fn wait_for_output(&mut self, marker: &str, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !self.output.contains(marker) {
            assert!(
                Instant::now() < deadline,
                "detached command did not emit {marker:?}; output={:?}; response={:#?}",
                self.output,
                self.last_response.body
            );
            self.poll(1_000);
            assert_eq!(
                self.status(),
                Some("running"),
                "command terminated before emitting {marker:?}: {:#?}",
                self.last_response.body
            );
        }
    }

    pub(crate) fn assert_next_poll_empty(&mut self) {
        let before = self.output.len();
        self.poll(0);
        assert_eq!(
            self.output.len(),
            before,
            "poll replayed previously observed output: {:#?}",
            self.last_response.body
        );
    }

    pub(crate) fn write(&mut self, chars: &str, wait_ms: u64) -> &ClientResponse {
        let response = self.client.call_tool(
            "command_control",
            json!({
                "action":"write",
                "session_id":self.session_id,
                "chars":chars,
                "wait_ms":wait_ms
            }),
        );
        self.observe(response)
    }

    pub(crate) fn kill(&mut self, signal: &str, wait_ms: u64) -> &ClientResponse {
        let response = self.client.call_tool(
            "command_control",
            json!({
                "action":"kill",
                "session_id":self.session_id,
                "signal":signal,
                "wait_ms":wait_ms
            }),
        );
        self.observe(response)
    }

    pub(crate) fn status(&self) -> Option<&str> {
        self.last_response.body["result"]["structuredContent"]["data"]["status"].as_str()
    }

    fn observe(&mut self, response: ClientResponse) -> &ClientResponse {
        self.output.push_str(
            response.body["result"]["structuredContent"]["data"]["output"]
                .as_str()
                .unwrap_or_default(),
        );
        self.last_response = response;
        &self.last_response
    }
}

pub(crate) fn settle_public_command(
    port: u16,
    session: &str,
    mut poll_id: u64,
    mut response: ClientResponse,
) -> (ClientResponse, String) {
    let deadline = Instant::now() + Duration::from_secs(150);
    let mut output = String::new();
    loop {
        let data = &response.body["result"]["structuredContent"]["data"];
        output.push_str(data["output"].as_str().unwrap_or_default());
        match data["status"].as_str() {
            Some("running") => {
                assert!(
                    Instant::now() < deadline,
                    "public command did not converge: {:#?}",
                    response.body
                );
                let public_session = data["session_id"]
                    .as_str()
                    .expect("running command has PublicSessionId")
                    .to_string();
                response = public_tool_call(
                    port,
                    session,
                    poll_id,
                    "command_control",
                    json!({"action":"poll","session_id":public_session,"wait_ms":1000}),
                );
                poll_id = poll_id.saturating_add(1);
            }
            Some(_) => return (response, output),
            None => panic!(
                "public command response has no status: {:#?}",
                response.body
            ),
        }
    }
}
