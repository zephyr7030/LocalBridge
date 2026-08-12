use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::bundle::CODING_TOOLS_VERSION;
use super::runtime::{CodingToolsRuntimeError, InternalBearer};

const PROTOCOL_VERSION: &str = "2025-11-25";
const MAX_HTTP_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

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
            || result.pointer("/serverInfo/name").and_then(Value::as_str) != Some("coding-tools-mcp")
            || result.pointer("/serverInfo/version").and_then(Value::as_str) != Some(CODING_TOOLS_VERSION)
        {
            return Err(CodingToolsRuntimeError::ProtocolMismatch);
        }
        if self.session_id.is_none() {
            return Err(CodingToolsRuntimeError::ProtocolMismatch);
        }
        self.notify("notifications/initialized", json!({}))?;
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

    pub(crate) fn cancellation_client(&self) -> Result<McpCancellationClient, CodingToolsRuntimeError> {
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
            if self.session_id.as_deref().is_some_and(|existing| existing != session) {
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
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let mut stream = TcpStream::connect_timeout(&address.into(), Duration::from_millis(500))
        .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;

    let mut body = serde_json::to_vec(payload).map_err(|_| CodingToolsRuntimeError::ProtocolMismatch)?;
    let mut request = Vec::with_capacity(body.len() + 512 + bearer.map_or(0, |value| value.expose().len()));
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

    let write_result = stream.write_all(&request).and_then(|_| stream.flush());
    zero_bytes(&mut request);
    zero_bytes(&mut body);
    write_result.map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;

    let mut response = Vec::new();
    stream
        .take((MAX_HTTP_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut response)
        .map_err(|_| CodingToolsRuntimeError::ConnectionUnavailable)?;
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
    let headers = std::str::from_utf8(header_bytes)
        .map_err(|_| CodingToolsRuntimeError::ProtocolMismatch)?;
    let mut lines = headers.split("\r\n");
    let status_line = lines.next().ok_or(CodingToolsRuntimeError::ProtocolMismatch)?;
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
