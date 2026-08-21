use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::control_plane::command_control::{
    CommandControlAction, CommandKillSignal, RuntimeCommandControl, RuntimeCommandControlError,
    RuntimeCommandObservation, RuntimeCommandRequest, RuntimeCommandStatus,
};
use crate::domain::RpcRequestId;

use super::bundle::CODING_TOOLS_VERSION;
use super::runtime::{CodingToolsRuntimeError, InternalBearer};

const PROTOCOL_VERSION: &str = "2025-11-25";
const MAX_HTTP_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const COMMAND_CONTROL_TRANSPORT_HEADROOM_MS: u64 = 2_000;
static HEALTH_REQUEST_ID: AtomicU64 = AtomicU64::new(1_000_000);
static CONTROL_REQUEST_ID: AtomicI64 = AtomicI64::new(-1);

pub(crate) struct McpSession {
    port: u16,
    bearer: Arc<InternalBearer>,
    session_id: Option<Arc<str>>,
    next_id: u64,
}

#[derive(Clone)]
pub(crate) struct McpCancellationClient {
    port: u16,
    bearer: Arc<InternalBearer>,
    session_id: Arc<str>,
}

#[derive(Clone)]
pub(crate) struct McpHealthClient {
    port: u16,
    bearer: Arc<InternalBearer>,
    session_id: Arc<str>,
}

impl std::fmt::Debug for McpHealthClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpHealthClient")
            .field("endpoint", &format_args!("127.0.0.1:{}/mcp", self.port))
            .field("authenticated", &true)
            .finish()
    }
}

impl McpHealthClient {
    pub(crate) fn probe_default_cwd(
        &self,
        timeout: Duration,
    ) -> Result<Value, CodingToolsRuntimeError> {
        let request_id = HEALTH_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let mut session = McpSession {
            port: self.port,
            bearer: Arc::clone(&self.bearer),
            session_id: Some(Arc::clone(&self.session_id)),
            next_id: request_id,
        };
        session.call_tool_with_timeout("get_default_cwd", json!({}), timeout)
    }
}

impl std::fmt::Debug for McpCancellationClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpCancellationClient")
            .field("endpoint", &format_args!("127.0.0.1:{}/mcp", self.port))
            .field("authenticated", &true)
            .finish()
    }
}

impl McpCancellationClient {
    pub(crate) fn cancel_request(&self, request_id: &Value) -> Result<(), CodingToolsRuntimeError> {
        if !valid_request_id(request_id) {
            return Err(CodingToolsRuntimeError::ProtocolMismatch);
        }
        let payload = json!({
            "jsonrpc":"2.0",
            "method":"notifications/cancelled",
            "params":{"requestId":request_id.clone()}
        });
        let response = post_json(
            self.port,
            Some(&self.bearer),
            Some(&self.session_id),
            &payload,
        )?;
        if response.status == 202 {
            Ok(())
        } else {
            Err(CodingToolsRuntimeError::HttpStatus(response.status))
        }
    }

    pub(crate) fn kill_command_session(
        &self,
        runtime_session_id: &str,
        wait_ms: u64,
    ) -> Result<Value, CodingToolsRuntimeError> {
        self.control_command_session(runtime_session_id, "kill", None, wait_ms)
    }

    pub(crate) fn control_command_session(
        &self,
        runtime_session_id: &str,
        action: &str,
        chars: Option<&str>,
        wait_ms: u64,
    ) -> Result<Value, CodingToolsRuntimeError> {
        let request_id = Value::from(CONTROL_REQUEST_ID.fetch_sub(1, Ordering::Relaxed));
        self.control_command_session_with_request_id(
            runtime_session_id,
            action,
            chars,
            (action == "kill").then_some("KILL"),
            wait_ms,
            &request_id,
        )
    }

    pub(crate) fn control_command_session_with_request_id(
        &self,
        runtime_session_id: &str,
        action: &str,
        chars: Option<&str>,
        signal: Option<&str>,
        wait_ms: u64,
        request_id: &Value,
    ) -> Result<Value, CodingToolsRuntimeError> {
        if runtime_session_id.is_empty() {
            return Err(CodingToolsRuntimeError::ProtocolMismatch);
        }
        let (tool, arguments) = match action {
            "poll" => (
                "write_stdin",
                json!({
                    "session_id":runtime_session_id,
                    "chars":"",
                    "yield_time_ms":wait_ms.min(30_000),
                    "max_output_bytes":65_536,
                    "verbosity":"full"
                }),
            ),
            "write" => {
                let chars = chars
                    .filter(|value| !value.is_empty())
                    .ok_or(CodingToolsRuntimeError::ProtocolMismatch)?;
                (
                    "write_stdin",
                    json!({
                        "session_id":runtime_session_id,
                        "chars":chars,
                        "yield_time_ms":wait_ms.min(30_000),
                        "max_output_bytes":65_536,
                        "verbosity":"full"
                    }),
                )
            }
            "kill" => (
                "kill_session",
                json!({
                    "session_id":runtime_session_id,
                    "signal":signal.unwrap_or("TERM"),
                    "wait_ms":wait_ms.min(30_000),
                    "verbosity":"full"
                }),
            ),
            _ => return Err(CodingToolsRuntimeError::ProtocolMismatch),
        };
        let mut session = McpSession {
            port: self.port,
            bearer: Arc::clone(&self.bearer),
            session_id: Some(Arc::clone(&self.session_id)),
            next_id: 1,
        };
        session.call_tool_with_request_id_and_timeout(
            tool,
            arguments,
            request_id,
            Duration::from_millis(
                wait_ms
                    .min(30_000)
                    .saturating_add(COMMAND_CONTROL_TRANSPORT_HEADROOM_MS),
            ),
        )
    }
}

impl RuntimeCommandControl for McpCancellationClient {
    fn control_command(
        &self,
        request: &RuntimeCommandRequest,
    ) -> Result<RuntimeCommandObservation, RuntimeCommandControlError> {
        let action = match request.action {
            CommandControlAction::Poll => "poll",
            CommandControlAction::Write => "write",
            CommandControlAction::Kill => "kill",
        };
        let request_id = match &request.request_id {
            RpcRequestId::Number(value) => Value::from(*value),
            RpcRequestId::String(value) => Value::String(value.clone()),
        };
        let raw = self
            .control_command_session_with_request_id(
                &request.runtime_handle,
                action,
                request.chars.as_deref(),
                request.signal.map(CommandKillSignal::as_str),
                request.wait_ms,
                &request_id,
            )
            .map_err(map_command_transport_error)?;
        if raw.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(RuntimeCommandControlError::SessionUnavailable);
        }
        let structured = raw
            .get("structuredContent")
            .and_then(Value::as_object)
            .ok_or(RuntimeCommandControlError::CapabilityMismatch)?;
        let private_status = structured
            .get("status")
            .and_then(Value::as_str)
            .ok_or(RuntimeCommandControlError::CapabilityMismatch)?;
        let exit_code = structured.get("exit_code").and_then(Value::as_i64);
        let timed_out = structured
            .get("timed_out")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let status = if timed_out || private_status == "timeout" {
            RuntimeCommandStatus::TimedOut
        } else {
            match private_status {
                "running" | "terminating" => RuntimeCommandStatus::Running,
                "terminated" | "killed" | "cancelled" => RuntimeCommandStatus::Cancelled,
                "exited" if exit_code.is_some_and(|code| code != 0) => RuntimeCommandStatus::Failed,
                "exited" => RuntimeCommandStatus::Completed,
                _ => RuntimeCommandStatus::Lost,
            }
        };
        Ok(RuntimeCommandObservation {
            status,
            exit_code,
            signal: structured
                .get("signal")
                .or_else(|| structured.get("signal_sent"))
                .and_then(Value::as_str)
                .map(str::to_string),
            stdout: structured
                .get("stdout")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            stderr: structured
                .get("stderr")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            truncated: structured.get("truncated").and_then(Value::as_bool),
        })
    }
}

fn map_command_transport_error(error: CodingToolsRuntimeError) -> RuntimeCommandControlError {
    match error {
        CodingToolsRuntimeError::ProtocolMismatch => RuntimeCommandControlError::InvalidRequest,
        CodingToolsRuntimeError::UpstreamRpcError => RuntimeCommandControlError::CapabilityMismatch,
        CodingToolsRuntimeError::HttpStatus(404) => RuntimeCommandControlError::SessionUnavailable,
        _ => RuntimeCommandControlError::Unavailable,
    }
}

impl std::fmt::Debug for McpSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpSession")
            .field("endpoint", &format_args!("127.0.0.1:{}/mcp", self.port))
            .field("authenticated", &true)
            .field("initialized", &self.session_id.is_some())
            .finish()
    }
}

impl McpSession {
    pub(crate) fn new(port: u16, bearer: InternalBearer) -> Self {
        Self {
            port,
            bearer: Arc::new(bearer),
            session_id: None,
            next_id: 1,
        }
    }

    pub(crate) fn initialize(&mut self) -> Result<Value, CodingToolsRuntimeError> {
        let result = self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "localbridge", "version": "0.1.0"}
            }),
        )?;
        if result.get("protocolVersion").and_then(Value::as_str) != Some(PROTOCOL_VERSION)
            || result.pointer("/serverInfo/name").and_then(Value::as_str)
                != Some("coding-tools-mcp")
            || result
                .pointer("/serverInfo/version")
                .and_then(Value::as_str)
                != Some(CODING_TOOLS_VERSION)
        {
            return Err(CodingToolsRuntimeError::ProtocolMismatch);
        }
        if self.session_id.is_none() {
            return Err(CodingToolsRuntimeError::ProtocolMismatch);
        }
        self.notify("notifications/initialized", json!({}))?;
        Ok(result)
    }

    pub(crate) fn initialize_with_timeout(
        &mut self,
        transport_timeout: Duration,
    ) -> Result<Value, CodingToolsRuntimeError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let result = self.request_with_id_and_timeout(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "localbridge", "version": "0.1.0"}
            }),
            Value::from(id),
            transport_timeout,
        )?;
        if result.get("protocolVersion").and_then(Value::as_str) != Some(PROTOCOL_VERSION)
            || result.pointer("/serverInfo/name").and_then(Value::as_str)
                != Some("coding-tools-mcp")
            || result
                .pointer("/serverInfo/version")
                .and_then(Value::as_str)
                != Some(CODING_TOOLS_VERSION)
            || self.session_id.is_none()
        {
            return Err(CodingToolsRuntimeError::ProtocolMismatch);
        }
        self.notify_with_timeout("notifications/initialized", json!({}), transport_timeout)?;
        Ok(result)
    }

    pub(crate) fn list_tools(&mut self) -> Result<Value, CodingToolsRuntimeError> {
        self.request("tools/list", json!({}))
    }

    pub(crate) fn call_tool(
        &mut self,
        name: &str,
        arguments: Value,
    ) -> Result<Value, CodingToolsRuntimeError> {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
    }

    pub(crate) fn call_tool_with_timeout(
        &mut self,
        name: &str,
        arguments: Value,
        transport_timeout: Duration,
    ) -> Result<Value, CodingToolsRuntimeError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.request_with_id_and_timeout(
            "tools/call",
            json!({"name": name, "arguments": arguments}),
            Value::from(id),
            transport_timeout,
        )
    }

    pub(crate) fn call_tool_with_request_id(
        &mut self,
        name: &str,
        arguments: Value,
        request_id: &Value,
    ) -> Result<Value, CodingToolsRuntimeError> {
        if !valid_request_id(request_id) {
            return Err(CodingToolsRuntimeError::ProtocolMismatch);
        }
        self.request_with_id(
            "tools/call",
            json!({"name": name, "arguments": arguments}),
            request_id.clone(),
        )
    }

    pub(crate) fn call_tool_with_request_id_and_timeout(
        &mut self,
        name: &str,
        arguments: Value,
        request_id: &Value,
        transport_timeout: Duration,
    ) -> Result<Value, CodingToolsRuntimeError> {
        if !valid_request_id(request_id) {
            return Err(CodingToolsRuntimeError::ProtocolMismatch);
        }
        self.request_with_id_and_timeout(
            "tools/call",
            json!({"name": name, "arguments": arguments}),
            request_id.clone(),
            transport_timeout,
        )
    }

    pub(crate) fn cancellation_client(
        &self,
    ) -> Result<McpCancellationClient, CodingToolsRuntimeError> {
        let session_id = self
            .session_id
            .as_ref()
            .cloned()
            .ok_or(CodingToolsRuntimeError::ProtocolMismatch)?;
        Ok(McpCancellationClient {
            port: self.port,
            bearer: Arc::clone(&self.bearer),
            session_id,
        })
    }

    pub(crate) fn health_client(&self) -> Result<McpHealthClient, CodingToolsRuntimeError> {
        let mut health_session = McpSession {
            port: self.port,
            bearer: Arc::clone(&self.bearer),
            session_id: None,
            next_id: HEALTH_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
        };
        health_session.initialize_with_timeout(Duration::from_millis(750))?;
        let session_id = health_session
            .session_id
            .ok_or(CodingToolsRuntimeError::ProtocolMismatch)?;
        Ok(McpHealthClient {
            port: self.port,
            bearer: Arc::clone(&self.bearer),
            session_id,
        })
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, CodingToolsRuntimeError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.request_with_id(method, params, Value::from(id))
    }

    fn request_with_id(
        &mut self,
        method: &str,
        params: Value,
        id: Value,
    ) -> Result<Value, CodingToolsRuntimeError> {
        let payload = json!({"jsonrpc":"2.0","method":method,"params":params,"id":id});
        let response = post_json(
            self.port,
            Some(&self.bearer),
            self.session_id.as_deref(),
            &payload,
        )?;
        if response.status != 200 {
            return Err(CodingToolsRuntimeError::HttpStatus(response.status));
        }
        if let Some(session) = response.session_id {
            if self
                .session_id
                .as_deref()
                .is_some_and(|existing| existing != session)
            {
                return Err(CodingToolsRuntimeError::ProtocolMismatch);
            }
            self.session_id = Some(Arc::from(session));
        }
        let reply: Value = serde_json::from_slice(&response.body)
            .map_err(|_| CodingToolsRuntimeError::ProtocolMismatch)?;
        if reply.get("error").is_some() {
            return Err(CodingToolsRuntimeError::UpstreamRpcError);
        }
        reply
            .get("result")
            .cloned()
            .ok_or(CodingToolsRuntimeError::ProtocolMismatch)
    }

    fn request_with_id_and_timeout(
        &mut self,
        method: &str,
        params: Value,
        id: Value,
        transport_timeout: Duration,
    ) -> Result<Value, CodingToolsRuntimeError> {
        let payload = json!({"jsonrpc":"2.0","method":method,"params":params,"id":id});
        let response = post_json_with_timeouts(
            self.port,
            Some(&self.bearer),
            self.session_id.as_deref(),
            &payload,
            transport_timeout,
            transport_timeout,
            Some(transport_timeout),
        )?;
        if response.status != 200 {
            return Err(CodingToolsRuntimeError::HttpStatus(response.status));
        }
        if let Some(session) = response.session_id {
            if self
                .session_id
                .as_deref()
                .is_some_and(|existing| existing != session)
            {
                return Err(CodingToolsRuntimeError::ProtocolMismatch);
            }
            self.session_id = Some(Arc::from(session));
        }
        let reply: Value = serde_json::from_slice(&response.body)
            .map_err(|_| CodingToolsRuntimeError::ProtocolMismatch)?;
        if reply.get("error").is_some() {
            return Err(CodingToolsRuntimeError::UpstreamRpcError);
        }
        reply
            .get("result")
            .cloned()
            .ok_or(CodingToolsRuntimeError::ProtocolMismatch)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), CodingToolsRuntimeError> {
        let payload = json!({"jsonrpc":"2.0","method":method,"params":params});
        let response = post_json(
            self.port,
            Some(&self.bearer),
            self.session_id.as_deref(),
            &payload,
        )?;
        if response.status == 202 {
            Ok(())
        } else {
            Err(CodingToolsRuntimeError::HttpStatus(response.status))
        }
    }

    fn notify_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        transport_timeout: Duration,
    ) -> Result<(), CodingToolsRuntimeError> {
        let payload = json!({"jsonrpc":"2.0","method":method,"params":params});
        let response = post_json_with_timeouts(
            self.port,
            Some(&self.bearer),
            self.session_id.as_deref(),
            &payload,
            transport_timeout,
            transport_timeout,
            None,
        )?;
        if response.status == 202 {
            Ok(())
        } else {
            Err(CodingToolsRuntimeError::HttpStatus(response.status))
        }
    }
}

fn valid_request_id(request_id: &Value) -> bool {
    request_id.is_string()
        || request_id
            .as_number()
            .is_some_and(|number| number.as_i64().is_some() || number.as_u64().is_some())
}

pub(crate) fn unauthenticated_initialize_status(port: u16) -> Result<u16, CodingToolsRuntimeError> {
    let payload = json!({
        "jsonrpc":"2.0",
        "method":"initialize",
        "params":{
            "protocolVersion":PROTOCOL_VERSION,
            "capabilities":{},
            "clientInfo":{"name":"localbridge-auth-probe","version":"1"}
        },
        "id":1
    });
    Ok(post_json(port, None, None, &payload)?.status)
}

struct HttpResponse {
    status: u16,
    session_id: Option<String>,
    body: Vec<u8>,
}

fn post_json(
    port: u16,
    bearer: Option<&InternalBearer>,
    session_id: Option<&str>,
    payload: &Value,
) -> Result<HttpResponse, CodingToolsRuntimeError> {
    post_json_with_timeouts(
        port,
        bearer,
        session_id,
        payload,
        Duration::from_millis(500),
        Duration::from_secs(2),
        None,
    )
}

fn remaining_until(deadline: Instant) -> Result<Duration, CodingToolsRuntimeError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or(CodingToolsRuntimeError::ConnectionUnavailable)
}

fn post_json_with_timeouts(
    port: u16,
    bearer: Option<&InternalBearer>,
    session_id: Option<&str>,
    payload: &Value,
    connect_timeout: Duration,
    io_timeout: Duration,
    total_timeout: Option<Duration>,
) -> Result<HttpResponse, CodingToolsRuntimeError> {
    let deadline = total_timeout.and_then(|timeout| Instant::now().checked_add(timeout));
    let connect_timeout = match deadline {
        Some(deadline) => remaining_until(deadline)?,
        None => connect_timeout,
    };
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let mut stream = TcpStream::connect_timeout(&address.into(), connect_timeout)
        .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;
    if deadline.is_none() {
        stream
            .set_read_timeout(Some(io_timeout))
            .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;
        stream
            .set_write_timeout(Some(io_timeout))
            .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;
    }

    let mut body =
        serde_json::to_vec(payload).map_err(|_| CodingToolsRuntimeError::ProtocolMismatch)?;
    let mut request =
        Vec::with_capacity(body.len() + 512 + bearer.map_or(0, |value| value.expose().len()));
    request.extend_from_slice(b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:");
    request.extend_from_slice(port.to_string().as_bytes());
    request.extend_from_slice(b"\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: 2025-11-25\r\nConnection: close\r\nContent-Length: ");
    request.extend_from_slice(body.len().to_string().as_bytes());
    request.extend_from_slice(b"\r\n");
    if let Some(session) = session_id {
        request.extend_from_slice(b"Mcp-Session-Id: ");
        request.extend_from_slice(session.as_bytes());
        request.extend_from_slice(b"\r\n");
    }
    if let Some(secret) = bearer {
        request.extend_from_slice(b"Authorization: Bearer ");
        request.extend_from_slice(secret.expose().as_bytes());
        request.extend_from_slice(b"\r\n");
    }
    request.extend_from_slice(b"\r\n");
    request.extend_from_slice(&body);

    let write_result = if let Some(deadline) = deadline {
        (|| {
            let mut written = 0usize;
            while written < request.len() {
                stream
                    .set_write_timeout(Some(remaining_until(deadline)?))
                    .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;
                let count = stream
                    .write(&request[written..])
                    .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;
                if count == 0 {
                    return Err(CodingToolsRuntimeError::ConnectionUnavailable);
                }
                written += count;
            }
            stream
                .set_write_timeout(Some(remaining_until(deadline)?))
                .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;
            stream
                .flush()
                .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)
        })()
    } else {
        stream
            .write_all(&request)
            .and_then(|_| stream.flush())
            .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)
    };
    zero_bytes(&mut request);
    zero_bytes(&mut body);
    write_result?;

    let mut response = Vec::new();
    if let Some(deadline) = deadline {
        let mut buffer = [0u8; 8192];
        loop {
            stream
                .set_read_timeout(Some(remaining_until(deadline)?))
                .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    response.extend_from_slice(&buffer[..count]);
                    if response.len() > MAX_HTTP_RESPONSE_BYTES {
                        return Err(CodingToolsRuntimeError::ProtocolMismatch);
                    }
                }
                Err(_) => return Err(CodingToolsRuntimeError::ConnectionUnavailable),
            }
        }
    } else {
        stream
            .take((MAX_HTTP_RESPONSE_BYTES + 1) as u64)
            .read_to_end(&mut response)
            .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;
    }
    if response.len() > MAX_HTTP_RESPONSE_BYTES {
        return Err(CodingToolsRuntimeError::ProtocolMismatch);
    }
    parse_response(response)
}

fn parse_response(response: Vec<u8>) -> Result<HttpResponse, CodingToolsRuntimeError> {
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(CodingToolsRuntimeError::ProtocolMismatch)?;
    let header_bytes = &response[..split];
    let body = response[(split + 4)..].to_vec();
    let headers =
        std::str::from_utf8(header_bytes).map_err(|_| CodingToolsRuntimeError::ProtocolMismatch)?;
    let mut lines = headers.split("\r\n");
    let status_line = lines
        .next()
        .ok_or(CodingToolsRuntimeError::ProtocolMismatch)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(CodingToolsRuntimeError::ProtocolMismatch)?;
    let mut content_length = None;
    let mut session_id = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(CodingToolsRuntimeError::ProtocolMismatch);
        };
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("Content-Length") {
            content_length = value.parse::<usize>().ok();
        } else if name.eq_ignore_ascii_case("Mcp-Session-Id") {
            if value.is_empty() || value.len() > 256 {
                return Err(CodingToolsRuntimeError::ProtocolMismatch);
            }
            session_id = Some(value.to_string());
        }
    }
    if let Some(expected) = content_length {
        if expected != body.len() {
            return Err(CodingToolsRuntimeError::ProtocolMismatch);
        }
    } else if status != 202 {
        return Err(CodingToolsRuntimeError::ProtocolMismatch);
    }
    Ok(HttpResponse {
        status,
        session_id,
        body,
    })
}

fn zero_bytes(bytes: &mut [u8]) {
    for byte in bytes {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
}
