use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::privilege::{
    ElevatedExecOutcome, ElevatedExecSpec, PrivilegedExecError, PrivilegedExecution,
};
use crate::state::{
    Capability, CurrentTaskStatus, PermissionMode, PrivilegeState, SafeTaskSummary,
    TaskExecutionState, TaskKind,
};

use super::guard::{GuardError, McpGuard, ToolCallRequest};
use super::http::McpCancellationClient;
use super::policy::CapabilityPolicy;
use super::runtime::CodingToolsRuntime;

const CURRENT_PROTOCOL_VERSION: &str = "2025-11-25";
const COMPATIBLE_PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);
const ACCEPT_IDLE: Duration = Duration::from_millis(10);
const MAX_CONNECTION_WORKERS: usize = 32;
static SESSION_GENERATION: AtomicU64 = AtomicU64::new(1);
static PRIVILEGED_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(1);

struct ConnectionContext<'a> {
    guard: &'a Mutex<McpGuard<CodingToolsRuntime>>,
    cancellation: &'a McpCancellationClient,
    permission_mode: &'a RwLock<PermissionMode>,
    current_task: &'a CurrentTaskProjection,
    sessions: &'a Mutex<HashMap<String, String>>,
    active_requests: &'a Mutex<Vec<Value>>,
    privileged: Option<&'a Arc<dyn PrivilegedExecution>>,
    privileged_requests: &'a Mutex<Vec<(Value, String)>>,
    stopping: &'a AtomicBool,
}

struct ElevatedCallContext<'a> {
    guard: &'a Mutex<McpGuard<CodingToolsRuntime>>,
    privileged: Option<&'a Arc<dyn PrivilegedExecution>>,
    current_task: &'a CurrentTaskProjection,
    active_requests: &'a Mutex<Vec<Value>>,
    privileged_requests: &'a Mutex<Vec<(Value, String)>>,
    stopping: &'a AtomicBool,
}

#[derive(Clone, Default)]
pub struct CurrentTaskProjection(Arc<Mutex<CurrentTaskStatus>>);

impl fmt::Debug for CurrentTaskProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CurrentTaskProjection")
            .field(&self.snapshot())
            .finish()
    }
}

impl CurrentTaskProjection {
    pub fn snapshot(&self) -> CurrentTaskStatus {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn project(&self, status: CurrentTaskStatus) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = status;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyEnforcementError {
    BindFailed,
    UpstreamSessionUnavailable,
    ThreadSpawnFailed,
    ThreadTerminated,
}

impl fmt::Display for PolicyEnforcementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindFailed => f.write_str("policy enforcement loopback bind failed"),
            Self::UpstreamSessionUnavailable => f.write_str("policy enforcement upstream MCP session is unavailable"),
            Self::ThreadSpawnFailed => f.write_str("policy enforcement thread could not start"),
            Self::ThreadTerminated => f.write_str("policy enforcement thread terminated unexpectedly"),
        }
    }
}

impl std::error::Error for PolicyEnforcementError {}

pub struct PolicyEnforcementRuntime {
    port: u16,
    permission_mode: Arc<RwLock<PermissionMode>>,
    current_task: CurrentTaskProjection,
    shutdown: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<McpGuard<CodingToolsRuntime>>>,
}

impl fmt::Debug for PolicyEnforcementRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PolicyEnforcementRuntime")
            .field("endpoint", &self.endpoint())
            .field("running", &self.is_running())
            .field("current_task", &self.current_task.snapshot())
            .finish()
    }
}

impl PolicyEnforcementRuntime {
    pub fn start(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_inner(coding_runtime, policy, permission_mode, None)
    }

    pub fn start_with_privilege(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
        privileged: Arc<dyn PrivilegedExecution>,
    ) -> Result<Self, PolicyEnforcementError> {
        Self::start_inner(coding_runtime, policy, permission_mode, Some(privileged))
    }

    fn start_inner(
        coding_runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        permission_mode: PermissionMode,
        privileged: Option<Arc<dyn PrivilegedExecution>>,
    ) -> Result<Self, PolicyEnforcementError> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| PolicyEnforcementError::BindFailed)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| PolicyEnforcementError::BindFailed)?;
        let port = listener
            .local_addr()
            .map_err(|_| PolicyEnforcementError::BindFailed)?
            .port();
        let permission_mode = Arc::new(RwLock::new(permission_mode));
        let current_task = CurrentTaskProjection::default();
        let thread_mode = Arc::clone(&permission_mode);
        let thread_task = current_task.clone();
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let cancellation = coding_runtime
            .cancellation_client()
            .map_err(|_| PolicyEnforcementError::UpstreamSessionUnavailable)?;
        let guard = Arc::new(Mutex::new(McpGuard::new(coding_runtime, policy)));
        let thread_guard = Arc::clone(&guard);
        let thread = thread::Builder::new()
            .name("localbridge-mcp-policy".into())
            .spawn(move || {
                serve(
                    listener,
                    thread_guard,
                    cancellation,
                    thread_mode,
                    thread_task,
                    privileged,
                    shutdown_rx,
                )
            })
            .map_err(|_| PolicyEnforcementError::ThreadSpawnFailed)?;
        Ok(Self {
            port,
            permission_mode,
            current_task,
            shutdown: Some(shutdown_tx),
            thread: Some(thread),
        })
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.port)
    }

    pub fn set_permission_mode(&self, mode: PermissionMode) {
        *self
            .permission_mode
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = mode;
    }

    pub fn current_task_projection(&self) -> CurrentTaskProjection {
        self.current_task.clone()
    }

    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|thread| !thread.is_finished())
    }

    pub fn stop(mut self) -> Result<CodingToolsRuntime, PolicyEnforcementError> {
        self.signal_shutdown();
        let thread = self.thread.take().ok_or(PolicyEnforcementError::ThreadTerminated)?;
        let guard = thread.join().map_err(|_| PolicyEnforcementError::ThreadTerminated)?;
        Ok(guard.into_runtime())
    }

    fn signal_shutdown(&mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
    }
}

impl Drop for PolicyEnforcementRuntime {
    fn drop(&mut self) {
        self.signal_shutdown();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(
    listener: TcpListener,
    guard: Arc<Mutex<McpGuard<CodingToolsRuntime>>>,
    cancellation: McpCancellationClient,
    permission_mode: Arc<RwLock<PermissionMode>>,
    current_task: CurrentTaskProjection,
    privileged: Option<Arc<dyn PrivilegedExecution>>,
    shutdown: mpsc::Receiver<()>,
) -> McpGuard<CodingToolsRuntime> {
    let sessions = Arc::new(Mutex::new(HashMap::<String, String>::new()));
    let active_requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let privileged_requests = Arc::new(Mutex::new(Vec::<(Value, String)>::new()));
    let stopping = Arc::new(AtomicBool::new(false));
    let mut workers = Vec::<JoinHandle<()>>::new();
    loop {
        if shutdown.try_recv().is_ok() {
            stopping.store(true, Ordering::Release);
            break;
        }
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                let worker = workers.swap_remove(index);
                let _ = worker.join();
            } else {
                index += 1;
            }
        }
        match listener.accept() {
            Ok((mut stream, _)) if workers.len() >= MAX_CONNECTION_WORKERS => {
                let _ = write_empty(&mut stream, 503, None);
            }
            Ok((stream, _)) => {
                let worker_guard = Arc::clone(&guard);
                let worker_mode = Arc::clone(&permission_mode);
                let worker_task = current_task.clone();
                let worker_sessions = Arc::clone(&sessions);
                let worker_active = Arc::clone(&active_requests);
                let worker_privileged = privileged.as_ref().map(Arc::clone);
                let worker_privileged_requests = Arc::clone(&privileged_requests);
                let worker_stopping = Arc::clone(&stopping);
                let worker_cancellation = cancellation.clone();
                if let Ok(worker) = thread::Builder::new()
                    .name("localbridge-mcp-policy-request".into())
                    .spawn(move || {
                        let context = ConnectionContext {
                            guard: &worker_guard,
                            cancellation: &worker_cancellation,
                            permission_mode: &worker_mode,
                            current_task: &worker_task,
                            sessions: &worker_sessions,
                            active_requests: &worker_active,
                            privileged: worker_privileged.as_ref(),
                            privileged_requests: &worker_privileged_requests,
                            stopping: &worker_stopping,
                        };
                        let _ = handle_connection(stream, context);
                    })
                {
                    workers.push(worker);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_IDLE);
            }
            Err(_) => break,
        }
    }
    for _ in 0..3 {
        let active = active_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if active.is_empty() {
            break;
        }
        for request_id in &active {
            if let Some(broker_request_id) = privileged_request_id(&privileged_requests, request_id) {
                if let Some(privileged) = privileged.as_ref() {
                    let _ = privileged.cancel_execute(broker_request_id);
                }
            } else {
                let _ = cancellation.cancel_request(request_id);
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    if let Some(privileged) = privileged.as_ref() {
        let requests = privileged_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(_, broker_request_id)| broker_request_id.clone())
            .collect::<Vec<_>>();
        for broker_request_id in requests {
            let _ = privileged.cancel_execute(broker_request_id);
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
    let guard = Arc::try_unwrap(guard).unwrap_or_else(|_| {
        panic!("policy enforcement guard still shared after worker shutdown")
    });
    guard
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn handle_connection(
    mut stream: TcpStream,
    context: ConnectionContext<'_>,
) -> Result<(), ()> {
    let ConnectionContext {
        guard,
        cancellation,
        permission_mode,
        current_task,
        sessions,
        active_requests,
        privileged,
        privileged_requests,
        stopping,
    } = context;
    stream.set_read_timeout(Some(CONNECTION_TIMEOUT)).map_err(|_| ())?;
    stream.set_write_timeout(Some(CONNECTION_TIMEOUT)).map_err(|_| ())?;
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(status) => return write_empty(&mut stream, status, None),
    };

    if request.path != "/mcp" {
        return write_empty(&mut stream, 404, None);
    }
    if request.method == "DELETE" {
        let Some(session) = request.header("mcp-session-id") else {
            return write_empty(&mut stream, 400, None);
        };
        if sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(session)
            .is_some()
        {
            return write_empty(&mut stream, 204, None);
        }
        return write_empty(&mut stream, 404, None);
    }
    if request.method != "POST" {
        return write_empty(&mut stream, 405, None);
    }

    let payload: Value = match serde_json::from_slice(&request.body) {
        Ok(payload) => payload,
        Err(_) => return write_rpc_error(&mut stream, Value::Null, -32700, "Parse error", None),
    };
    if payload.is_array() {
        return write_rpc_error(&mut stream, Value::Null, -32600, "Batch requests are not supported", None);
    }
    let Some(object) = payload.as_object() else {
        return write_rpc_error(&mut stream, Value::Null, -32600, "Invalid Request", None);
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return write_rpc_error(&mut stream, request_id(object), -32600, "Invalid Request", None);
    }
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return write_rpc_error(&mut stream, request_id(object), -32600, "Invalid Request", None);
    };
    let id = request_id(object);

    if method == "initialize" {
        if request.header("mcp-session-id").is_some() {
            return write_rpc_error(&mut stream, id, -32600, "initialize must not include Mcp-Session-Id", None);
        }
        let protocol = object
            .get("params")
            .and_then(Value::as_object)
            .and_then(|params| params.get("protocolVersion"))
            .and_then(Value::as_str);
        let Some(protocol) = protocol.filter(|value| {
            *value == CURRENT_PROTOCOL_VERSION || *value == COMPATIBLE_PROTOCOL_VERSION
        }) else {
            return write_rpc_error(&mut stream, id, -32602, "Unsupported MCP protocol version", None);
        };
        let session = new_session_id();
        {
            let mut sessions = sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            sessions.clear();
            sessions.insert(session.clone(), protocol.to_string());
        }
        return write_rpc_result(
            &mut stream,
            id,
            json!({
                "protocolVersion": protocol,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "localbridge-mcp-guard", "version": env!("CARGO_PKG_VERSION")}
            }),
            Some(&session),
        );
    }

    if method == "ping" && request.header("mcp-session-id").is_none() && !id.is_null() {
        return write_rpc_result(&mut stream, id, json!({}), None);
    }

    let Some(session) = request.header("mcp-session-id") else {
        return write_rpc_error(&mut stream, id, -32600, "Mcp-Session-Id is required", None);
    };
    let protocol = sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(session)
        .cloned();
    let Some(protocol) = protocol else {
        return write_rpc_error(&mut stream, id, -32600, "Unknown MCP session", None);
    };
    if request
        .header("mcp-protocol-version")
        .is_some_and(|version| version != protocol)
    {
        return write_rpc_error(&mut stream, id, -32600, "MCP protocol version mismatch", Some(session));
    }

    if id.is_null() {
        if method == "notifications/cancelled" {
            if let Some(request_id) = object
                .get("params")
                .and_then(Value::as_object)
                .and_then(|params| params.get("requestId"))
                .filter(|request_id| valid_downstream_request_id(request_id))
            {
                if let Some(broker_request_id) = privileged_request_id(privileged_requests, request_id) {
                    let Some(privileged) = privileged else {
                        return write_empty(&mut stream, 503, Some(session));
                    };
                    if privileged.cancel_execute(broker_request_id).is_err() {
                        return write_empty(&mut stream, 503, Some(session));
                    }
                    return write_empty(&mut stream, 202, Some(session));
                }
                if cancellation.cancel_request(request_id).is_err() {
                    return write_empty(&mut stream, 503, Some(session));
                }
                for _ in 0..2 {
                    thread::sleep(Duration::from_millis(25));
                    if !active_request_exists(active_requests, request_id) {
                        break;
                    }
                    let _ = cancellation.cancel_request(request_id);
                }
                return write_empty(&mut stream, 202, Some(session));
            }
        }
        return write_empty(&mut stream, 202, Some(session));
    }

    match method {
        "ping" => write_rpc_result(&mut stream, id, json!({}), Some(session)),
        "tools/list" => {
            let mode = *permission_mode
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut guard = guard
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match guard.filtered_tools(mode) {
                Ok(mut result) => {
                    if privileged.is_some_and(|gateway| gateway.state().accepts_privileged_calls())
                        && guard.privileged_tool_visible(mode, "elevated_exec")
                    {
                        append_elevated_exec_tool(&mut result);
                    }
                    write_rpc_result(&mut stream, id, result, Some(session))
                }
                Err(_) => write_rpc_error(&mut stream, id, -32603, "Policy enforcement failed", Some(session)),
            }
        }
        "tools/call" => {
            if !valid_downstream_request_id(&id) {
                return write_rpc_error(&mut stream, Value::Null, -32600, "Invalid Request", Some(session));
            }
            let Some(params) = object.get("params").and_then(Value::as_object) else {
                return write_rpc_error(&mut stream, id, -32602, "Invalid tools/call params", Some(session));
            };
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return write_rpc_error(&mut stream, id, -32602, "Invalid tools/call name", Some(session));
            };
            let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            if !arguments.is_object() {
                return write_rpc_error(&mut stream, id, -32602, "Invalid tools/call arguments", Some(session));
            }
            if stopping.load(Ordering::Acquire) {
                return write_empty(&mut stream, 503, Some(session));
            }
            let mode = *permission_mode
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if name == "elevated_exec" {
                return handle_elevated_exec(
                    &mut stream,
                    id,
                    session,
                    mode,
                    arguments,
                    ElevatedCallContext {
                        guard,
                        privileged,
                        current_task,
                        active_requests,
                        privileged_requests,
                        stopping,
                    },
                );
            }
            active_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(id.clone());
            let mut guard = guard
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if stopping.load(Ordering::Acquire) {
                remove_active_request(active_requests, &id);
                return write_empty(&mut stream, 503, Some(session));
            }
            let result = guard.call_tool(
                mode,
                ToolCallRequest::new(name, arguments).with_request_id(id.clone()),
                |status| {
                current_task.project(status);
                },
            );
            remove_active_request(active_requests, &id);
            match result {
                Ok(result) => write_rpc_result(&mut stream, id, result, Some(session)),
                Err(GuardError::Denied(_)) => write_rpc_error(
                    &mut stream,
                    id,
                    -32001,
                    "Tool call denied by LocalBridge policy",
                    Some(session),
                ),
                Err(_) => write_rpc_error(&mut stream, id, -32603, "MCP runtime call failed", Some(session)),
            }
        }
        _ => write_rpc_error(&mut stream, id, -32601, "Method not found", Some(session)),
    }
}

fn append_elevated_exec_tool(result: &mut Value) {
    let Some(tools) = result.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    if tools
        .iter()
        .any(|tool| tool.get("name").and_then(Value::as_str) == Some("elevated_exec"))
    {
        return;
    }
    tools.push(json!({
        "name": "elevated_exec",
        "description": "Run a reviewed structured program through the active LocalBridge privileged broker.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "program": {"type": "string"},
                "args": {"type": "array", "items": {"type": "string"}},
                "workdir": {"type": ["string", "null"]},
                "timeout_ms": {"type": "integer", "minimum": 1},
                "max_output_bytes": {"type": "integer", "minimum": 1}
            },
            "required": ["program", "args", "timeout_ms", "max_output_bytes"],
            "additionalProperties": false
        }
    }));
}

fn privileged_request_id(
    requests: &Mutex<Vec<(Value, String)>>,
    downstream_request_id: &Value,
) -> Option<String> {
    requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|(candidate, _)| candidate == downstream_request_id)
        .map(|(_, broker_request_id)| broker_request_id.clone())
}

fn remove_privileged_request(
    requests: &Mutex<Vec<(Value, String)>>,
    downstream_request_id: &Value,
) {
    requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|(candidate, _)| candidate != downstream_request_id);
}

fn project_elevated_task(current_task: &CurrentTaskProjection, state: TaskExecutionState) {
    current_task.project(
        CurrentTaskStatus::project(
            TaskKind::ElevatedOperation,
            SafeTaskSummary::Omitted,
            state,
        )
        .expect("elevated task state is a valid active task state"),
    );
}

fn finish_elevated_task(current_task: &CurrentTaskProjection, terminal: Option<TaskExecutionState>) {
    if let Some(state) = terminal {
        project_elevated_task(current_task, state);
    }
    current_task.project(CurrentTaskStatus::Idle);
}

fn elevated_exec_spec(arguments: Value) -> Result<ElevatedExecSpec, ()> {
    let spec: ElevatedExecSpec = serde_json::from_value(arguments).map_err(|_| ())?;
    spec.validate().map_err(|_| ())?;
    Ok(spec)
}

fn handle_elevated_exec(
    stream: &mut TcpStream,
    id: Value,
    session: &str,
    mode: PermissionMode,
    arguments: Value,
    context: ElevatedCallContext<'_>,
) -> Result<(), ()> {
    let ElevatedCallContext {
        guard,
        privileged,
        current_task,
        active_requests,
        privileged_requests,
        stopping,
    } = context;
    let decision = guard
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .decision(
            mode,
            &ToolCallRequest::new("elevated_exec", json!({})),
        );
    if !decision.allowed || decision.descriptor.capability != Capability::ElevatedExec {
        finish_elevated_task(current_task, Some(TaskExecutionState::Blocked));
        return write_rpc_error(
            stream,
            id,
            -32001,
            "Tool call denied by LocalBridge policy",
            Some(session),
        );
    }

    let Some(privileged) = privileged else {
        finish_elevated_task(current_task, Some(TaskExecutionState::AwaitingAuthorization));
        return write_rpc_error(stream, id, -32002, "ElevationRequired", Some(session));
    };
    if !matches!(privileged.state(), PrivilegeState::Active { .. }) {
        finish_elevated_task(current_task, Some(TaskExecutionState::AwaitingAuthorization));
        return write_rpc_error(stream, id, -32002, "ElevationRequired", Some(session));
    }
    let spec = match elevated_exec_spec(arguments) {
        Ok(spec) => spec,
        Err(()) => {
            finish_elevated_task(current_task, Some(TaskExecutionState::Blocked));
            return write_rpc_error(
                stream,
                id,
                -32602,
                "Invalid elevated_exec arguments",
                Some(session),
            );
        }
    };

    let generation = PRIVILEGED_REQUEST_GENERATION.fetch_add(1, Ordering::Relaxed);
    let broker_request_id = format!("mcp-elevated-{generation:x}");
    active_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(id.clone());
    privileged_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((id.clone(), broker_request_id.clone()));
    project_elevated_task(current_task, TaskExecutionState::Running);

    if let Err(error) = privileged.start_execute(broker_request_id.clone(), spec) {
        remove_active_request(active_requests, &id);
        remove_privileged_request(privileged_requests, &id);
        return match error {
            PrivilegedExecError::GateClosed(_) => {
                finish_elevated_task(current_task, Some(TaskExecutionState::AwaitingAuthorization));
                write_rpc_error(stream, id, -32002, "ElevationRequired", Some(session))
            }
            PrivilegedExecError::Broker(_) => {
                finish_elevated_task(current_task, Some(TaskExecutionState::Failed));
                write_rpc_error(stream, id, -32603, "Privileged broker execution failed", Some(session))
            }
        };
    }

    let execution = loop {
        if stopping.load(Ordering::Acquire) {
            let _ = privileged.cancel_execute(broker_request_id.clone());
        }
        match privileged.poll_execute(broker_request_id.clone()) {
            Ok(Some(result)) => break Ok(result),
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => break Err(error),
        }
    };
    remove_active_request(active_requests, &id);
    remove_privileged_request(privileged_requests, &id);

    let execution = match execution {
        Ok(execution) => execution,
        Err(PrivilegedExecError::GateClosed(_)) => {
            finish_elevated_task(current_task, Some(TaskExecutionState::AwaitingAuthorization));
            return write_rpc_error(stream, id, -32002, "ElevationRequired", Some(session));
        }
        Err(PrivilegedExecError::Broker(_)) => {
            finish_elevated_task(current_task, Some(TaskExecutionState::Failed));
            return write_rpc_error(stream, id, -32603, "Privileged broker execution failed", Some(session));
        }
    };

    let outcome = match execution.outcome {
        ElevatedExecOutcome::Completed => "completed",
        ElevatedExecOutcome::TimedOut => "timed_out",
        ElevatedExecOutcome::Cancelled => "cancelled",
    };
    let terminal = match execution.outcome {
        ElevatedExecOutcome::Completed => None,
        ElevatedExecOutcome::TimedOut => Some(TaskExecutionState::Failed),
        ElevatedExecOutcome::Cancelled => Some(TaskExecutionState::Cancelled),
    };
    let is_error = !matches!(execution.outcome, ElevatedExecOutcome::Completed);
    let response = json!({
        "content": [{"type": "text", "text": execution.output}],
        "structuredContent": {
            "outcome": outcome,
            "exit_code": execution.exit_code,
            "truncated": execution.truncated
        },
        "isError": is_error
    });
    finish_elevated_task(current_task, terminal);
    write_rpc_result(stream, id, response, Some(session))
}

fn request_id(object: &serde_json::Map<String, Value>) -> Value {
    object.get("id").cloned().unwrap_or(Value::Null)
}

fn valid_downstream_request_id(request_id: &Value) -> bool {
    request_id.is_string()
        || request_id
            .as_number()
            .is_some_and(|number| number.as_i64().is_some() || number.as_u64().is_some())
}

fn remove_active_request(active_requests: &Mutex<Vec<Value>>, request_id: &Value) {
    active_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|active| active != request_id);
}

fn active_request_exists(active_requests: &Mutex<Vec<Value>>, request_id: &Value) -> bool {
    active_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .any(|active| active == request_id)
}

struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, u16> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(431);
        }
        let count = stream.read(&mut chunk).map_err(|_| 400u16)?;
        if count == 0 {
            return Err(400);
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header_text = std::str::from_utf8(&bytes[..header_end - 4]).map_err(|_| 400u16)?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().ok_or(400u16)?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().ok_or(400u16)?.to_string();
    let path = request_parts.next().ok_or(400u16)?.to_string();
    if request_parts.next() != Some("HTTP/1.1") || request_parts.next().is_some() {
        return Err(400);
    }
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(400u16)?;
        let name = name.trim().to_string();
        let value = value.trim().to_string();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse::<usize>().map_err(|_| 400u16)?;
            if content_length > MAX_BODY_BYTES {
                return Err(413);
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(400);
        }
        headers.push((name, value));
    }
    let mut body = bytes[header_end..].to_vec();
    if body.len() > content_length {
        body.truncate(content_length);
    }
    while body.len() < content_length {
        let remaining = content_length - body.len();
        let read_limit = remaining.min(chunk.len());
        let count = stream
            .read(&mut chunk[..read_limit])
            .map_err(|_| 400u16)?;
        if count == 0 {
            return Err(400);
        }
        body.extend_from_slice(&chunk[..count]);
    }
    Ok(HttpRequest { method, path, headers, body })
}

fn new_session_id() -> String {
    let generation = SESSION_GENERATION.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("lb-{:x}-{nanos:x}-{generation:x}", std::process::id())
}

fn write_rpc_result(
    stream: &mut TcpStream,
    id: Value,
    result: Value,
    session: Option<&str>,
) -> Result<(), ()> {
    write_json(stream, 200, &json!({"jsonrpc":"2.0","id":id,"result":result}), session)
}

fn write_rpc_error(
    stream: &mut TcpStream,
    id: Value,
    code: i64,
    message: &str,
    session: Option<&str>,
) -> Result<(), ()> {
    write_json(
        stream,
        200,
        &json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}),
        session,
    )
}

fn write_json(
    stream: &mut TcpStream,
    status: u16,
    value: &Value,
    session: Option<&str>,
) -> Result<(), ()> {
    let body = serde_json::to_vec(value).map_err(|_| ())?;
    write_response(stream, status, Some("application/json"), &body, session)
}

fn write_empty(stream: &mut TcpStream, status: u16, session: Option<&str>) -> Result<(), ()> {
    write_response(stream, status, None, &[], session)
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: Option<&str>,
    body: &[u8],
    session: Option<&str>,
) -> Result<(), ()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(content_type) = content_type {
        response.push_str("Content-Type: ");
        response.push_str(content_type);
        response.push_str("\r\n");
    }
    if let Some(session) = session {
        response.push_str("Mcp-Session-Id: ");
        response.push_str(session);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes()).map_err(|_| ())?;
    stream.write_all(body).map_err(|_| ())?;
    stream.flush().map_err(|_| ())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::runtime::{CodingToolsPermissionMode, CodingToolsRuntimeConfig, InternalBearer};

    const SYNTHETIC_BEARER: &str = "LB009_PEP_INTERNAL_BEARER_SYNTHETIC_DO_NOT_LEAK";

    #[derive(Debug)]
    struct FakePrivilegedExecution {
        state: RwLock<PrivilegeState>,
        starts: Mutex<Vec<ElevatedExecSpec>>,
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
                    output: String::new(),
                    truncated: false,
                }));
            }
            if self.complete.load(Ordering::Acquire) {
                return Ok(Some(crate::privilege::ElevatedExecResult {
                    outcome: ElevatedExecOutcome::Completed,
                    exit_code: Some(0),
                    output: "LB012_FAKE_PRIVILEGED_OK".to_string(),
                    truncated: false,
                }));
            }
            Ok(None)
        }

        fn cancel_execute(&self, _request_id: String) -> Result<(), PrivilegedExecError> {
            self.cancelled.store(true, Ordering::Release);
            Ok(())
        }
    }

    struct ClientResponse {
        status: u16,
        session: Option<String>,
        body: Value,
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri has repository parent")
            .to_path_buf()
    }

    fn temp_workspace() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("localbridge-lb009-pep-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("probe.txt"), b"LB009 PEP\n").unwrap();
        path
    }

    fn free_port() -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.local_addr().unwrap().port()
    }

    fn cleanup_test_directory(path: &Path) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match fs::remove_dir_all(path) {
                Ok(()) => return,
                Err(error)
                    if matches!(error.kind(), std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Other)
                        && std::time::Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(error) => panic!("remove test workspace {}: {error}", path.display()),
            }
        }
    }

    fn policy(root: &Path) -> CapabilityPolicy {
        CapabilityPolicy::load(&root.join("runtime-policy.toml")).unwrap()
    }

    fn post(port: u16, session: Option<&str>, payload: &Value) -> ClientResponse {
        let body = serde_json::to_vec(payload).unwrap();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
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

    fn delete(port: u16, session: &str) -> u16 {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let request = format!(
            "DELETE /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nMcp-Session-Id: {session}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        parse_client_response(stream).status
    }

    fn parse_client_response(mut stream: TcpStream) -> ClientResponse {
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        let split = bytes.windows(4).position(|window| window == b"\r\n\r\n").unwrap();
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
        ClientResponse { status, session, body }
    }

    fn initialize(port: u16, id: u64) -> ClientResponse {
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
                    "clientInfo":{"name":"lb009-pep-test","version":"1"}
                }
            }),
        )
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
        assert_eq!(initialized.body["result"]["protocolVersion"], CURRENT_PROTOCOL_VERSION);
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
        assert_eq!(full_catalog.len(), 19);
        assert!(full_catalog.iter().any(|tool| tool["name"] == "exec_command"));

        pep.set_permission_mode(PermissionMode::Edit);
        let denied = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":3,"method":"tools/call",
                "params":{"name":"exec_command","arguments":{"cmd":"echo must-not-run"}}
            }),
        );
        assert_eq!(denied.body["error"]["code"], -32001);
        assert_eq!(pep.current_task_projection().snapshot(), CurrentTaskStatus::Idle);

        let edit_tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/list","params":{}}),
        );
        assert_eq!(edit_tools.body["result"]["tools"].as_array().unwrap().len(), 15);

        let read = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":5,"method":"tools/call",
                "params":{"name":"read_file","arguments":{"path":"probe.txt"}}
            }),
        );
        assert!(read.body.get("result").is_some(), "allowed read_file must forward: {}", read.body);
        assert_eq!(pep.current_task_projection().snapshot(), CurrentTaskStatus::Idle);

        let reinitialized = initialize(pep.port(), 6);
        let new_session = reinitialized.session.unwrap();
        assert_ne!(new_session, session);
        let stale = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":7,"method":"ping","params":{}}),
        );
        assert_eq!(stale.body["error"]["code"], -32600);
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
        let call_started = std::time::Instant::now();
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
                            "cmd":"powershell.exe -NoProfile -Command \"Start-Sleep -Seconds 10\"",
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
            assert!(std::time::Instant::now() < running_deadline, "tool call never projected Running");
            thread::sleep(Duration::from_millis(10));
        }
        thread::sleep(Duration::from_millis(100));

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
        assert!(cancel_started.elapsed() < Duration::from_secs(2), "cancellation transport was blocked");

        let call_result = call.join().expect("tools/call client thread");
        assert!(call_started.elapsed() < Duration::from_secs(5), "cancelled command ran near natural 10 second duration");
        assert!(
            call_result.body.get("result").is_some() || call_result.body.get("error").is_some(),
            "cancelled tools/call must terminate with a JSON-RPC response: {}",
            call_result.body
        );
        assert_eq!(pep.current_task_projection().snapshot(), CurrentTaskStatus::Idle);

        let mut coding = pep.stop().expect("PEP stop after cancellation");
        coding.stop().expect("MCP Job stop after cancellation");
        assert_eq!(coding.active_processes().unwrap(), 0);
        drop(coding);
        cleanup_test_directory(&workspace);
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
                .all(|tool| tool["name"] != "elevated_exec")
        );
        fake.set_state(PrivilegeState::Active {
            broker_generation: crate::state::GenerationId::new(77),
        });

        let secret = "LB012_SYNTHETIC_PEP_SECRET";
        let port = pep.port();
        let call_session = session.clone();
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
                            "program":"C:/Windows/System32/cmd.exe",
                            "args":["/d","/c",format!("echo {secret}"),"--api-key",secret],
                            "workdir":"C:/Windows/Temp",
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
                        && task.state == TaskExecutionState::Running =>
                {
                    assert_eq!(task.summary, SafeTaskSummary::Omitted);
                    assert!(!format!("{task:?}").contains(secret));
                    break;
                }
                _ => {}
            }
            assert!(
                std::time::Instant::now() < running_deadline,
                "elevated call never projected Running"
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
        assert_eq!(pep.current_task_projection().snapshot(), CurrentTaskStatus::Idle);

        pep.set_permission_mode(PermissionMode::Full);
        let full_tools = post(
            pep.port(),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":307,"method":"tools/list","params":{}}),
        );
        assert!(
            full_tools.body["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .all(|tool| tool["name"] != "elevated_exec")
        );
        let full_denied = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":302,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "program":"C:/Windows/System32/cmd.exe","args":[],
                    "timeout_ms":1000,"max_output_bytes":1024
                }}
            }),
        );
        assert_eq!(full_denied.body["error"]["code"], -32001);
        assert_eq!(fake.start_count(), 1);

        pep.set_permission_mode(PermissionMode::Elevated);
        fake.set_state(PrivilegeState::AwaitingUac);
        let awaiting = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":303,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "program":"C:/Windows/System32/cmd.exe","args":[],
                    "timeout_ms":1000,"max_output_bytes":1024
                }}
            }),
        );
        assert_eq!(awaiting.body["error"]["code"], -32002);
        assert_eq!(awaiting.body["error"]["message"], "ElevationRequired");
        assert_eq!(fake.start_count(), 1);
        assert_eq!(pep.current_task_projection().snapshot(), CurrentTaskStatus::Idle);

        let control_plane = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":304,"method":"tools/call",
                "params":{"name":"request_permissions","arguments":{"permission":"admin"}}
            }),
        );
        assert_eq!(control_plane.body["error"]["code"], -32001);

        fake.set_state(PrivilegeState::Active {
            broker_generation: crate::state::GenerationId::new(78),
        });
        fake.complete.store(true, Ordering::Release);
        let completed = post(
            pep.port(),
            Some(&session),
            &json!({
                "jsonrpc":"2.0","id":305,"method":"tools/call",
                "params":{"name":"elevated_exec","arguments":{
                    "program":"C:/Windows/System32/cmd.exe","args":["/c","echo ok"],
                    "timeout_ms":1000,"max_output_bytes":1024
                }}
            }),
        );
        assert_eq!(
            completed.body["result"]["structuredContent"]["outcome"],
            "completed"
        );
        assert_eq!(
            completed.body["result"]["content"][0]["text"],
            "LB012_FAKE_PRIVILEGED_OK"
        );
        assert_eq!(fake.start_count(), 2);

        let mut coding = pep.stop().expect("PEP stop after privileged routing");
        coding.stop().expect("MCP stop after privileged routing");
        drop(coding);
        cleanup_test_directory(&workspace);
    }
}
