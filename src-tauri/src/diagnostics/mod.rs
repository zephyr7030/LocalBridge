use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::runtime::{
    RecoveryAttemptEvent, RecoveryAttemptResult, RecoveryDisposition, RuntimeOutage,
};
use crate::state::{PrivilegeState, RuntimeComponent, RuntimeFault, RuntimeState};
use error::{DiagnosticErrorCode, DiagnosticPhase, ErrorDiagnostic, transport_unavailable};

pub mod error;

pub const DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;
static EXPORT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
pub(crate) const RECENT_DIAGNOSTIC_EVENT_LIMIT: usize = 8;
const RECENT_EVENT_LIMIT: usize = RECENT_DIAGNOSTIC_EVENT_LIMIT;
static RECENT_USER_EVENTS: OnceLock<Mutex<VecDeque<DiagnosticEvent>>> = OnceLock::new();
static RECENT_USER_OBSERVATIONS: OnceLock<Mutex<RecentUserObservationState>> = OnceLock::new();
pub(crate) const REQUEST_DIAGNOSTIC_LIMIT: usize = 16;
static REQUEST_DIAGNOSTICS: OnceLock<Mutex<RequestDiagnosticState>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsOutageInput {
    pub generation: u64,
    pub request_id: String,
    pub component: RuntimeComponent,
    pub fault: RuntimeFault,
    pub user_attention_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsRuntimeInput {
    pub active: bool,
    pub state: RuntimeState,
    pub active_workspace: Option<PathBuf>,
    pub outage: Option<DiagnosticsOutageInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCheck {
    pub code: &'static str,
    pub label: &'static str,
    pub level: DiagnosticLevel,
    pub detail: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerDiagnosticState {
    Off,
    Requested,
    Awaiting,
    Active,
    Fault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerDiagnostics {
    pub state: BrokerDiagnosticState,
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconnectAttemptState {
    Running,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconnectAttempt {
    pub attempt: u32,
    pub state: ReconnectAttemptState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconnectDiagnostics {
    pub generation: u64,
    pub component: &'static str,
    pub attention_required: bool,
    pub attempts: Vec<ReconnectAttempt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSnapshot {
    pub schema_version: u32,
    pub checks: Vec<DiagnosticCheck>,
    pub broker: BrokerDiagnostics,
    pub reconnect: Option<ReconnectDiagnostics>,
    pub runtime_key_present: bool,
    pub active_workspace_path: Option<String>,
    pub recent_events: Vec<DiagnosticEvent>,
    pub request_diagnostics: Vec<RequestDiagnosticEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    pub level: DiagnosticLevel,
    pub message: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestDiagnosticKind {
    Start,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDiagnosticEvent {
    pub kind: RequestDiagnosticKind,
    #[serde(rename = "timestamp")]
    pub timestamp_ms: u64,
    pub request_id: String,
    pub connection_id: String,
    pub attempt: u32,
    pub tool: String,
    pub outcome: Option<String>,
    pub error_code: Option<String>,
    pub phase: Option<String>,
    pub cause: Option<String>,
    pub http_status: Option<u16>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct ActiveRequestDiagnostic {
    attempt: u32,
    request_id: String,
    connection_id: String,
    tool: String,
    started_at: Instant,
}

#[derive(Debug, Clone)]
struct ActiveRecoveryDiagnostic {
    generation: u64,
    request: ActiveRequestDiagnostic,
}

#[derive(Debug, Default)]
struct RecentUserObservationState {
    runtime: Option<(DiagnosticLevel, String)>,
    broker: Option<(DiagnosticLevel, String)>,
}

#[derive(Debug, Default)]
struct RequestDiagnosticState {
    recovery_active: Option<ActiveRecoveryDiagnostic>,
    events: VecDeque<RequestDiagnosticEvent>,
}

pub fn build_snapshot(
    install_root: &Path,
    runtime: &DiagnosticsRuntimeInput,
    privilege: &PrivilegeState,
    runtime_key_present: bool,
) -> DiagnosticsSnapshot {
    let local_runtime_present = install_root.join("runtime/python/python.exe").is_file()
        && install_root
            .join("runtime/coding-tools-mcp/coding_tools_mcp/__init__.py")
            .is_file()
        && install_root
            .join("runtime/tunnel-client/tunnel-client.exe")
            .is_file();
    let checks = vec![
        DiagnosticCheck {
            code: "local_runtime",
            label: "本地运行环境",
            level: if local_runtime_present {
                DiagnosticLevel::Ok
            } else {
                DiagnosticLevel::Error
            },
            detail: if local_runtime_present {
                "捆绑运行环境已就绪"
            } else {
                "捆绑运行环境不完整"
            },
        },
        DiagnosticCheck {
            code: "runtime_key",
            label: "Runtime API Key",
            level: if runtime_key_present {
                DiagnosticLevel::Ok
            } else {
                DiagnosticLevel::Warning
            },
            detail: if runtime_key_present {
                "已安全保存"
            } else {
                "尚未保存"
            },
        },
        DiagnosticCheck {
            code: "coding_service",
            label: "编码服务",
            level: if coding_service_ready(&runtime.state) {
                DiagnosticLevel::Ok
            } else if runtime.active {
                DiagnosticLevel::Warning
            } else {
                DiagnosticLevel::Error
            },
            detail: if coding_service_ready(&runtime.state) {
                "服务可用"
            } else if runtime.active {
                "服务尚未就绪"
            } else {
                "服务未启动"
            },
        },
        DiagnosticCheck {
            code: "openai_tunnel",
            label: "OpenAI Tunnel",
            level: if matches!(&runtime.state, RuntimeState::Ready) {
                DiagnosticLevel::Ok
            } else if runtime.active {
                DiagnosticLevel::Warning
            } else {
                DiagnosticLevel::Error
            },
            detail: if matches!(&runtime.state, RuntimeState::Ready) {
                "连接已就绪"
            } else if runtime.active {
                "连接尚未就绪"
            } else {
                "连接未启动"
            },
        },
    ];

    let privilege_check = broker_diagnostics(privilege);
    let recent_events = recent_user_events();
    let request_diagnostics = recent_request_diagnostics();

    DiagnosticsSnapshot {
        schema_version: DIAGNOSTICS_SCHEMA_VERSION,
        checks,
        broker: privilege_check,
        reconnect: reconnect_diagnostics(runtime),
        runtime_key_present,
        active_workspace_path: runtime
            .active_workspace
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        recent_events,
        request_diagnostics,
    }
}

fn recent_event_log() -> &'static Mutex<VecDeque<DiagnosticEvent>> {
    RECENT_USER_EVENTS.get_or_init(|| Mutex::new(VecDeque::with_capacity(RECENT_EVENT_LIMIT)))
}

fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn record_recent_event(level: DiagnosticLevel, message: String) {
    let mut events = recent_event_log()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if events
        .front()
        .is_some_and(|event| event.level == level && event.message == message)
    {
        return;
    }
    events.push_front(DiagnosticEvent {
        level,
        message,
        timestamp_ms: timestamp_ms(),
    });
    events.truncate(RECENT_EVENT_LIMIT);
}

#[derive(Debug, Clone, Copy)]
enum RecentEventSource {
    Runtime,
    Broker,
}

fn record_recent_transition(source: RecentEventSource, event: Option<(DiagnosticLevel, String)>) {
    let mut observations = RECENT_USER_OBSERVATIONS
        .get_or_init(|| Mutex::new(RecentUserObservationState::default()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let slot = match source {
        RecentEventSource::Runtime => &mut observations.runtime,
        RecentEventSource::Broker => &mut observations.broker,
    };
    if slot.as_ref() == event.as_ref() {
        return;
    }
    *slot = event.clone();
    drop(observations);
    if let Some((level, message)) = event {
        record_recent_event(level, message);
    }
}

pub fn record_runtime_user_events(
    state: &RuntimeState,
    outage: Option<&DiagnosticsOutageInput>,
    privilege: &PrivilegeState,
) {
    let runtime_event = if let Some(outage) = outage {
        Some((
            DiagnosticLevel::Error,
            format!(
                "{}：{}",
                component_label(outage.component),
                runtime_fault_label(&outage.fault)
            ),
        ))
    } else {
        match state {
            RuntimeState::Ready => Some((DiagnosticLevel::Ok, "本地运行服务：已就绪".to_string())),
            RuntimeState::Recovering { component, .. } => Some((
                DiagnosticLevel::Warning,
                format!("{}：正在自动恢复", component_label(*component)),
            )),
            RuntimeState::SwitchingWorkspace { .. } => Some((
                DiagnosticLevel::Warning,
                "本地运行服务：正在切换项目".to_string(),
            )),
            RuntimeState::Faulted(fault) => Some((
                DiagnosticLevel::Error,
                format!("本地运行服务：{}", runtime_fault_label(fault)),
            )),
            RuntimeState::StartingMcp
            | RuntimeState::WaitingMcpReady
            | RuntimeState::StartingPolicyEnforcement
            | RuntimeState::WaitingPolicyReady
            | RuntimeState::StartingTunnel
            | RuntimeState::WaitingTunnelReady => Some((
                DiagnosticLevel::Warning,
                "本地运行服务：正在启动".to_string(),
            )),
            RuntimeState::Stopped => None,
        }
    };
    record_recent_transition(RecentEventSource::Runtime, runtime_event);

    let broker = broker_diagnostics(privilege);
    let broker_event = match broker.state {
        BrokerDiagnosticState::Active => {
            Some((DiagnosticLevel::Ok, "管理员权限：已启用".to_string()))
        }
        BrokerDiagnosticState::Requested | BrokerDiagnosticState::Awaiting => Some((
            DiagnosticLevel::Warning,
            format!("管理员权限：{}", broker_state_label(broker.state)),
        )),
        BrokerDiagnosticState::Fault => {
            Some((DiagnosticLevel::Error, "管理员权限：故障".to_string()))
        }
        BrokerDiagnosticState::Off => None,
    };
    record_recent_transition(RecentEventSource::Broker, broker_event);
}

fn request_diagnostic_log() -> &'static Mutex<RequestDiagnosticState> {
    REQUEST_DIAGNOSTICS.get_or_init(|| Mutex::new(RequestDiagnosticState::default()))
}

pub fn record_recovery_attempt_event(event: &RecoveryAttemptEvent) {
    let mut log = request_diagnostic_log()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match event {
        RecoveryAttemptEvent::Started {
            generation,
            request_id,
            component,
            fault: _,
            attempt,
        } => {
            let generation = generation.get();
            if log.recovery_active.as_ref().is_some_and(|active| {
                active.generation == generation
                    && active.request.attempt == *attempt
                    && active.request.request_id == *request_id
            }) {
                return;
            }
            if let Some(active) = log.recovery_active.take() {
                push_request_end(
                    &mut log,
                    active.request,
                    "lost",
                    Some(ErrorDiagnostic::new(
                        DiagnosticErrorCode::Unknown,
                        DiagnosticPhase::Runtime,
                        "recovery_attempt_replaced",
                    )),
                );
            }
            let request_id = request_id.clone();
            let connection_id = format!("conn-{request_id}-{attempt}");
            let tool = request_tool(*component).to_string();
            push_request_event(
                &mut log,
                RequestDiagnosticEvent {
                    kind: RequestDiagnosticKind::Start,
                    timestamp_ms: timestamp_ms(),
                    request_id: request_id.clone(),
                    connection_id: connection_id.clone(),
                    attempt: *attempt,
                    tool: tool.clone(),
                    outcome: None,
                    error_code: None,
                    phase: None,
                    cause: None,
                    http_status: None,
                    duration_ms: None,
                },
            );
            log.recovery_active = Some(ActiveRecoveryDiagnostic {
                generation,
                request: ActiveRequestDiagnostic {
                    attempt: *attempt,
                    request_id,
                    connection_id,
                    tool,
                    started_at: Instant::now(),
                },
            });
        }
        RecoveryAttemptEvent::Finished {
            generation,
            request_id,
            attempt,
            result,
            ..
        } => {
            let Some(active) = log.recovery_active.take() else {
                return;
            };
            if active.generation != generation.get()
                || active.request.attempt != *attempt
                || active.request.request_id != *request_id
            {
                log.recovery_active = Some(active);
                return;
            }
            match result {
                RecoveryAttemptResult::Recovered => {
                    push_request_end(&mut log, active.request, "success", None)
                }
                RecoveryAttemptResult::Failed(fault) => push_request_end(
                    &mut log,
                    active.request,
                    "failed",
                    Some(runtime_fault_diagnostic(fault)),
                ),
                RecoveryAttemptResult::Cancelled => push_request_end(
                    &mut log,
                    active.request,
                    "cancelled",
                    Some(ErrorDiagnostic::new(
                        DiagnosticErrorCode::Cancelled,
                        DiagnosticPhase::Runtime,
                        "recovery_cancelled",
                    )),
                ),
            }
        }
    }
}

fn mcp_diagnostic_request_id(request_key: &str, connection_id: &str) -> String {
    format!("mcp:{connection_id}:{request_key}")
}

pub fn record_mcp_request_start(request_key: &str, connection_id: &str, tool: &str) {
    let mut log = request_diagnostic_log()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    push_request_event(
        &mut log,
        RequestDiagnosticEvent {
            kind: RequestDiagnosticKind::Start,
            timestamp_ms: timestamp_ms(),
            request_id: mcp_diagnostic_request_id(request_key, connection_id),
            connection_id: connection_id.to_string(),
            attempt: 1,
            tool: tool.to_string(),
            outcome: None,
            error_code: None,
            phase: None,
            cause: None,
            http_status: None,
            duration_ms: None,
        },
    );
}

pub fn record_mcp_request_result(request_key: &str, connection_id: &str, result: &Value) {
    let mut log = request_diagnostic_log()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let diagnostic = result
        .pointer("/structuredContent/error")
        .unwrap_or(&Value::Null);
    let top = result.pointer("/structuredContent").unwrap_or(&Value::Null);
    let field = |name: &str| diagnostic.get(name).or_else(|| top.get(name));
    let error_code = field("error_code")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let phase = field("phase").and_then(Value::as_str).map(str::to_owned);
    let cause = field("cause").and_then(Value::as_str).map(str::to_owned);
    let http_status = field("http_status")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok());
    let outcome = if !is_error {
        "success"
    } else {
        match error_code.as_deref() {
            Some("Timeout") => "timed_out",
            Some("Cancelled") => "cancelled",
            _ => "failed",
        }
    };
    push_mcp_request_end(
        &mut log,
        McpRequestEnd {
            request_key,
            connection_id,
            outcome,
            error_code,
            phase,
            cause,
            http_status,
        },
    );
}

pub fn record_mcp_request_error(
    request_key: &str,
    connection_id: &str,
    diagnostic: ErrorDiagnostic,
) {
    let mut log = request_diagnostic_log()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    push_mcp_request_end(
        &mut log,
        McpRequestEnd {
            request_key,
            connection_id,
            outcome: "failed",
            error_code: Some(diagnostic.error_code.as_str().to_string()),
            phase: Some(diagnostic.phase.as_str().to_string()),
            cause: Some(diagnostic.cause),
            http_status: diagnostic.http_status,
        },
    );
}

struct McpRequestEnd<'a> {
    request_key: &'a str,
    connection_id: &'a str,
    outcome: &'a str,
    error_code: Option<String>,
    phase: Option<String>,
    cause: Option<String>,
    http_status: Option<u16>,
}

fn push_mcp_request_end(log: &mut RequestDiagnosticState, end: McpRequestEnd<'_>) {
    push_request_event(
        log,
        RequestDiagnosticEvent {
            kind: RequestDiagnosticKind::End,
            timestamp_ms: timestamp_ms(),
            request_id: mcp_diagnostic_request_id(end.request_key, end.connection_id),
            connection_id: end.connection_id.to_string(),
            attempt: 1,
            tool: String::new(),
            outcome: Some(end.outcome.to_string()),
            error_code: end.error_code,
            phase: end.phase,
            cause: end.cause,
            http_status: end.http_status,
            duration_ms: None,
        },
    );
}

fn push_request_end(
    log: &mut RequestDiagnosticState,
    active: ActiveRequestDiagnostic,
    outcome: &str,
    diagnostic: Option<ErrorDiagnostic>,
) {
    push_request_end_fields(
        log,
        active,
        outcome,
        diagnostic
            .as_ref()
            .map(|value| value.error_code.as_str().to_string()),
        diagnostic
            .as_ref()
            .map(|value| value.phase.as_str().to_string()),
        diagnostic.as_ref().map(|value| value.cause.clone()),
        diagnostic.as_ref().and_then(|value| value.http_status),
    );
}

fn push_request_end_fields(
    log: &mut RequestDiagnosticState,
    active: ActiveRequestDiagnostic,
    outcome: &str,
    error_code: Option<String>,
    phase: Option<String>,
    cause: Option<String>,
    http_status: Option<u16>,
) {
    let duration_ms = active
        .started_at
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    push_request_event(
        log,
        RequestDiagnosticEvent {
            kind: RequestDiagnosticKind::End,
            timestamp_ms: timestamp_ms(),
            request_id: active.request_id,
            connection_id: active.connection_id,
            attempt: active.attempt,
            tool: active.tool,
            outcome: Some(outcome.to_string()),
            error_code,
            phase,
            cause,
            http_status,
            duration_ms: Some(duration_ms),
        },
    );
}

fn push_request_event(log: &mut RequestDiagnosticState, event: RequestDiagnosticEvent) {
    log.events.push_front(event);
    log.events.truncate(REQUEST_DIAGNOSTIC_LIMIT);
}

fn recent_request_diagnostics() -> Vec<RequestDiagnosticEvent> {
    request_diagnostic_log()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .events
        .iter()
        .cloned()
        .collect()
}

#[cfg(test)]
pub(crate) fn request_diagnostics_for_test() -> Vec<RequestDiagnosticEvent> {
    recent_request_diagnostics()
}

#[cfg(test)]
pub(crate) fn reset_request_diagnostics_for_test() {
    *request_diagnostic_log()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = RequestDiagnosticState::default();
}

#[cfg(test)]
pub(crate) fn reset_recent_user_events_for_test() {
    recent_event_log()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    *RECENT_USER_OBSERVATIONS
        .get_or_init(|| Mutex::new(RecentUserObservationState::default()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = RecentUserObservationState::default();
}

fn request_tool(component: RuntimeComponent) -> &'static str {
    match component {
        RuntimeComponent::CodingRuntime => "coding_runtime",
        RuntimeComponent::PolicyEnforcement => "policy_enforcement",
        RuntimeComponent::Tunnel => "tunnel",
    }
}

fn runtime_fault_diagnostic(fault: &RuntimeFault) -> ErrorDiagnostic {
    match fault {
        RuntimeFault::McpHealthTimeout => transport_unavailable("mcp_health_timeout", None),
        RuntimeFault::McpExited => transport_unavailable("mcp_exited", None),
        RuntimeFault::TunnelHealthTimeout => transport_unavailable("tunnel_health_timeout", None),
        RuntimeFault::TunnelExited => transport_unavailable("tunnel_exited", None),
        RuntimeFault::PortUnavailable => transport_unavailable("port_unavailable", None),
        RuntimeFault::TunnelAuthFailed => ErrorDiagnostic::new(
            DiagnosticErrorCode::Denied,
            DiagnosticPhase::Transport,
            "tunnel_auth_failed",
        ),
        RuntimeFault::UserStopped => ErrorDiagnostic::new(
            DiagnosticErrorCode::Cancelled,
            DiagnosticPhase::Runtime,
            "user_stopped",
        ),
        _ => ErrorDiagnostic::new(
            DiagnosticErrorCode::Unavailable,
            DiagnosticPhase::Runtime,
            format!("runtime_fault_{fault:?}").to_ascii_lowercase(),
        ),
    }
}

fn recent_user_events() -> Vec<DiagnosticEvent> {
    recent_event_log()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .cloned()
        .collect()
}

fn broker_state_label(state: BrokerDiagnosticState) -> &'static str {
    match state {
        BrokerDiagnosticState::Off => "未启用",
        BrokerDiagnosticState::Requested => "等待授权",
        BrokerDiagnosticState::Awaiting => "等待系统授权",
        BrokerDiagnosticState::Active => "已启用",
        BrokerDiagnosticState::Fault => "故障",
    }
}

fn runtime_fault_label(fault: &RuntimeFault) -> &'static str {
    match fault {
        RuntimeFault::WorkspaceMissing | RuntimeFault::WorkspaceInvalid => "项目目录不可用",
        RuntimeFault::RuntimeMissing | RuntimeFault::RuntimeChecksumMismatch => {
            "本地运行环境不可用"
        }
        RuntimeFault::McpSpawnFailed | RuntimeFault::McpHealthTimeout | RuntimeFault::McpExited => {
            "编码服务不可用"
        }
        RuntimeFault::TunnelAuthFailed
        | RuntimeFault::TunnelSpawnFailed
        | RuntimeFault::TunnelHealthTimeout
        | RuntimeFault::TunnelExited => "OpenAI Tunnel 不可用",
        RuntimeFault::UserStopped => "服务已停止",
        _ => "服务发生故障",
    }
}

fn coding_service_ready(state: &RuntimeState) -> bool {
    matches!(
        state,
        RuntimeState::StartingTunnel | RuntimeState::WaitingTunnelReady | RuntimeState::Ready
    )
}

fn broker_diagnostics(state: &PrivilegeState) -> BrokerDiagnostics {
    match state {
        PrivilegeState::Disabled => BrokerDiagnostics {
            state: BrokerDiagnosticState::Off,
            generation: None,
        },
        PrivilegeState::Requested => BrokerDiagnostics {
            state: BrokerDiagnosticState::Requested,
            generation: None,
        },
        PrivilegeState::AwaitingUac => BrokerDiagnostics {
            state: BrokerDiagnosticState::Awaiting,
            generation: None,
        },
        PrivilegeState::Active { broker_generation } => BrokerDiagnostics {
            state: BrokerDiagnosticState::Active,
            generation: Some(broker_generation.get()),
        },
        PrivilegeState::Faulted(_) => BrokerDiagnostics {
            state: BrokerDiagnosticState::Fault,
            generation: None,
        },
    }
}

fn reconnect_diagnostics(runtime: &DiagnosticsRuntimeInput) -> Option<ReconnectDiagnostics> {
    let outage = runtime.outage.as_ref()?;
    let attempts = match &runtime.state {
        RuntimeState::Recovering { attempt, .. } if *attempt > 0 => (1..=*attempt)
            .map(|number| ReconnectAttempt {
                attempt: number,
                state: if number == *attempt {
                    ReconnectAttemptState::Running
                } else {
                    ReconnectAttemptState::Failed
                },
            })
            .collect(),
        _ if outage.user_attention_required
            && RuntimeOutage::classify(outage.component, outage.fault.clone()).disposition
                == RecoveryDisposition::Recoverable =>
        {
            (1..=5)
                .map(|attempt| ReconnectAttempt {
                    attempt,
                    state: ReconnectAttemptState::Failed,
                })
                .collect()
        }
        _ => Vec::new(),
    };

    Some(ReconnectDiagnostics {
        generation: outage.generation,
        component: component_label(outage.component),
        attention_required: outage.user_attention_required,
        attempts,
    })
}

fn component_label(component: RuntimeComponent) -> &'static str {
    match component {
        RuntimeComponent::CodingRuntime => "编码服务",
        RuntimeComponent::PolicyEnforcement => "策略执行",
        RuntimeComponent::Tunnel => "OpenAI Tunnel",
    }
}

pub fn export_snapshot(
    app_data_dir: &Path,
    snapshot: &DiagnosticsSnapshot,
) -> std::io::Result<PathBuf> {
    let directory = app_data_dir.join("diagnostics");
    fs::create_dir_all(&directory)?;
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let sequence = EXPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = directory.join(format!("localbridge-diagnostics-{seconds}-{sequence}.json"));
    let mut export = serde_json::to_value(snapshot).map_err(std::io::Error::other)?;
    if let Some(object) = export.as_object_mut() {
        object.remove("activeWorkspacePath");
        object.remove("runtimeKeyPresent");
        if let Some(checks) = object
            .get_mut("checks")
            .and_then(serde_json::Value::as_array_mut)
        {
            checks.retain(|check| {
                check.get("code").and_then(serde_json::Value::as_str) != Some("runtime_key")
            });
        }
    }
    let bytes = serde_json::to_vec_pretty(&export).map_err(std::io::Error::other)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(path)
}

pub fn materialize_log_directory(
    app_data_dir: &Path,
    snapshot: &DiagnosticsSnapshot,
) -> std::io::Result<PathBuf> {
    let artifact = export_snapshot(app_data_dir, snapshot)?;
    artifact
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| std::io::Error::other("diagnostics artifact has no parent directory"))
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/diagnostics/diagnostics.rs"
    ));
}
