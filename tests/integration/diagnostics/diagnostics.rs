use super::*;
use crate::state::{GenerationId, PrivilegeFault, RuntimeFault};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(1);

struct TempDir(PathBuf);
impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "localbridge-lb017-{label}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn complete_runtime(root: &Path) {
    for relative in [
        "runtime/python/python.exe",
        "runtime/coding-tools-mcp/coding_tools_mcp/__init__.py",
        "runtime/tunnel-client/tunnel-client.exe",
    ] {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"present").unwrap();
    }
}

fn runtime(state: RuntimeState, outage: Option<DiagnosticsOutageInput>) -> DiagnosticsRuntimeInput {
    DiagnosticsRuntimeInput {
        active: !matches!(state, RuntimeState::Stopped),
        state,
        active_workspace: Some(PathBuf::from(r"C:\project\redacted")),
        outage,
    }
}

#[test]
fn typed_checks_and_broker_generation_expose_no_broker_internals() {
    let root = TempDir::new("checks");
    complete_runtime(root.path());
    let snapshot = build_snapshot(
        root.path(),
        &runtime(RuntimeState::Ready, None),
        &PrivilegeState::Active {
            broker_generation: GenerationId::new(7),
        },
        true,
    );
    assert_eq!(snapshot.schema_version, 1);
    assert!(
        snapshot
            .checks
            .iter()
            .all(|check| check.level == DiagnosticLevel::Ok)
    );
    assert_eq!(snapshot.broker.state, BrokerDiagnosticState::Active);
    assert_eq!(snapshot.broker.generation, Some(7));
    let json = serde_json::to_string(&snapshot)
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in [
        "nonce",
        "pipe",
        "sid",
        "pid",
        "secret",
        "credential_id",
        "broker_generation",
    ] {
        assert!(!json.contains(forbidden), "diagnostics leaked {forbidden}");
    }
}

#[test]
fn exhausted_recoverable_generation_reports_exact_five_attempts_but_nonrecoverable_does_not_fake_history()
 {
    let root = TempDir::new("reconnect");
    complete_runtime(root.path());
    let exhausted = build_snapshot(
        root.path(),
        &runtime(
            RuntimeState::Faulted(RuntimeFault::TunnelExited),
            Some(DiagnosticsOutageInput {
                generation: 11,
                component: RuntimeComponent::Tunnel,
                fault: RuntimeFault::TunnelExited,
                user_attention_required: true,
            }),
        ),
        &PrivilegeState::Requested,
        true,
    );
    let reconnect = exhausted.reconnect.unwrap();
    assert_eq!(reconnect.generation, 11);
    assert_eq!(reconnect.attempts.len(), 5);
    assert_eq!(
        reconnect
            .attempts
            .iter()
            .map(|item| item.attempt)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert!(
        reconnect
            .attempts
            .iter()
            .all(|item| item.state == ReconnectAttemptState::Failed)
    );

    let nonrecoverable = build_snapshot(
        root.path(),
        &runtime(
            RuntimeState::Faulted(RuntimeFault::TunnelAuthFailed),
            Some(DiagnosticsOutageInput {
                generation: 12,
                component: RuntimeComponent::Tunnel,
                fault: RuntimeFault::TunnelAuthFailed,
                user_attention_required: true,
            }),
        ),
        &PrivilegeState::Faulted(PrivilegeFault::BrokerExited),
        false,
    );
    assert!(nonrecoverable.reconnect.unwrap().attempts.is_empty());
}

#[test]
fn recent_user_events_are_backend_typed_bounded_timestamped_and_redacted() {
    recent_event_log()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    for index in 0..12 {
        let state = if index % 3 == 0 {
            RuntimeState::Ready
        } else if index % 3 == 1 {
            RuntimeState::Recovering {
                component: RuntimeComponent::Tunnel,
                attempt: 1,
            }
        } else {
            RuntimeState::Faulted(RuntimeFault::TunnelExited)
        };
        record_runtime_user_events(&state, None, &PrivilegeState::Disabled);
    }
    let events = recent_user_events();
    assert_eq!(events.len(), RECENT_EVENT_LIMIT);
    assert!(events.iter().all(|event| event.timestamp_ms > 0));
    let text = serde_json::to_string(&events).unwrap();
    for forbidden in [
        "Runtime API Key",
        "Authorization",
        "synthetic-secret",
        "nonce",
        r"C:\project\redacted",
    ] {
        assert!(
            !text.contains(forbidden),
            "recent user events leaked {forbidden}"
        );
    }
}

#[test]
fn user_triggered_export_contains_allowlisted_projection_only() {
    let root = TempDir::new("export");
    complete_runtime(root.path());
    let snapshot = build_snapshot(
        root.path(),
        &runtime(RuntimeState::Ready, None),
        &PrivilegeState::Disabled,
        true,
    );
    let path = export_snapshot(root.path(), &snapshot).unwrap();
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains("schemaVersion"));
    assert!(!text.contains(r"C:\project\redacted"));
    for forbidden in [
        "Runtime API Key",
        "Authorization",
        "CODING_TOOLS_MCP_AUTH_TOKEN",
        "nonce",
        "pipeName",
        "processId",
    ] {
        assert!(!text.contains(forbidden));
    }
}

#[test]
fn schema42_request_diagnostics_keep_retry_correlation_and_export_engineering_fields() {
    *request_diagnostic_log()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = RequestDiagnosticState::default();
    let root = TempDir::new("request-correlation");
    complete_runtime(root.path());
    let outage = DiagnosticsOutageInput {
        generation: 42,
        component: RuntimeComponent::Tunnel,
        fault: RuntimeFault::TunnelExited,
        user_attention_required: false,
    };

    let first = build_snapshot(
        root.path(),
        &runtime(
            RuntimeState::Recovering {
                component: RuntimeComponent::Tunnel,
                attempt: 1,
            },
            Some(outage.clone()),
        ),
        &PrivilegeState::Disabled,
        true,
    );
    assert_eq!(first.request_diagnostics.len(), 1);
    assert_eq!(first.request_diagnostics[0].kind, RequestDiagnosticKind::Start);
    assert_eq!(first.request_diagnostics[0].attempt, 1);
    let request_id = first.request_diagnostics[0].request_id.clone();
    let first_connection = first.request_diagnostics[0].connection_id.clone();

    let retry = build_snapshot(
        root.path(),
        &runtime(
            RuntimeState::Recovering {
                component: RuntimeComponent::Tunnel,
                attempt: 2,
            },
            Some(outage.clone()),
        ),
        &PrivilegeState::Disabled,
        true,
    );
    let retry_start = retry
        .request_diagnostics
        .iter()
        .find(|event| event.kind == RequestDiagnosticKind::Start && event.attempt == 2)
        .unwrap();
    let first_end = retry
        .request_diagnostics
        .iter()
        .find(|event| event.kind == RequestDiagnosticKind::End && event.attempt == 1)
        .unwrap();
    assert_eq!(retry_start.request_id, request_id);
    assert_ne!(retry_start.connection_id, first_connection);
    assert_eq!(first_end.request_id, request_id);
    assert_eq!(first_end.connection_id, first_connection);
    assert_eq!(first_end.outcome.as_deref(), Some("failed"));
    assert_eq!(first_end.error_code.as_deref(), Some("Unavailable"));
    assert_eq!(first_end.phase.as_deref(), Some("transport"));
    assert_eq!(first_end.cause.as_deref(), Some("tunnel_exited"));
    assert!(first_end.duration_ms.is_some());

    let recovered = build_snapshot(
        root.path(),
        &runtime(RuntimeState::Ready, Some(outage)),
        &PrivilegeState::Disabled,
        true,
    );
    let retry_end = recovered
        .request_diagnostics
        .iter()
        .find(|event| event.kind == RequestDiagnosticKind::End && event.attempt == 2)
        .unwrap();
    assert_eq!(retry_end.request_id, request_id);
    assert_eq!(retry_end.connection_id, retry_start.connection_id);
    assert_eq!(retry_end.outcome.as_deref(), Some("success"));
    assert!(retry_end.error_code.is_none());
    assert!(retry_end.phase.is_none());
    assert!(retry_end.cause.is_none());
    assert!(retry_end.duration_ms.is_some());

    let path = export_snapshot(root.path(), &recovered).unwrap();
    let export = fs::read_to_string(path).unwrap();
    for field in [
        "requestDiagnostics",
        "requestId",
        "connectionId",
        "attempt",
        "errorCode",
        "phase",
        "cause",
        "httpStatus",
        "durationMs",
    ] {
        assert!(export.contains(field), "diagnostic export lost {field}");
    }
    for forbidden in ["Runtime API Key", "Authorization", "synthetic-secret"] {
        assert!(!export.contains(forbidden));
    }
}
