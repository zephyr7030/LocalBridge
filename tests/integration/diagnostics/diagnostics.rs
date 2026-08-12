use super::*;
use crate::state::{GenerationId, PrivilegeFault, RuntimeFault};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(1);

struct TempDir(PathBuf);
impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("localbridge-lb017-{label}-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path { &self.0 }
}
impl Drop for TempDir { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

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
        active_workspace: true,
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
        &PrivilegeState::Active { broker_generation: GenerationId::new(7) },
        true,
    );
    assert_eq!(snapshot.schema_version, 1);
    assert!(snapshot.checks.iter().all(|check| check.level == DiagnosticLevel::Ok));
    assert_eq!(snapshot.broker.state, BrokerDiagnosticState::Active);
    assert_eq!(snapshot.broker.generation, Some(7));
    let json = serde_json::to_string(&snapshot).unwrap().to_ascii_lowercase();
    for forbidden in ["nonce", "pipe", "sid", "pid", "secret", "credential_id", "broker_generation"] {
        assert!(!json.contains(forbidden), "diagnostics leaked {forbidden}");
    }
}

#[test]
fn exhausted_recoverable_generation_reports_exact_five_attempts_but_nonrecoverable_does_not_fake_history() {
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
    assert_eq!(reconnect.attempts.iter().map(|item| item.attempt).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
    assert!(reconnect.attempts.iter().all(|item| item.state == ReconnectAttemptState::Failed));

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
fn user_triggered_export_contains_allowlisted_projection_only() {
    let root = TempDir::new("export");
    complete_runtime(root.path());
    let snapshot = build_snapshot(root.path(), &runtime(RuntimeState::Ready, None), &PrivilegeState::Disabled, true);
    let path = export_snapshot(root.path(), &snapshot).unwrap();
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains("schemaVersion"));
    assert!(!text.contains(r"C:\project\redacted"));
    for forbidden in ["Runtime API Key", "Authorization", "CODING_TOOLS_MCP_AUTH_TOKEN", "nonce", "pipeName", "processId"] {
        assert!(!text.contains(forbidden));
    }
}
