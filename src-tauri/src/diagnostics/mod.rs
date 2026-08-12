use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime::{RecoveryDisposition, RuntimeOutage};
use crate::state::{PrivilegeState, RuntimeComponent, RuntimeFault, RuntimeState};

pub const DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;
static EXPORT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsOutageInput {
    pub generation: u64,
    pub component: RuntimeComponent,
    pub fault: RuntimeFault,
    pub user_attention_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsRuntimeInput {
    pub active: bool,
    pub state: RuntimeState,
    pub active_workspace: bool,
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
    pub active_workspace: bool,
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
            label: "运行密钥",
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

    DiagnosticsSnapshot {
        schema_version: DIAGNOSTICS_SCHEMA_VERSION,
        checks,
        broker: broker_diagnostics(privilege),
        reconnect: reconnect_diagnostics(runtime),
        runtime_key_present,
        active_workspace: runtime.active_workspace,
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
                == RecoveryDisposition::Recoverable => (1..=5)
            .map(|attempt| ReconnectAttempt {
                attempt,
                state: ReconnectAttemptState::Failed,
            })
            .collect(),
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
    let bytes = serde_json::to_vec_pretty(snapshot).map_err(std::io::Error::other)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(&path)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/diagnostics/diagnostics.rs"
    ));
}
