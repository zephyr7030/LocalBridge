use super::*;
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(windows)]
use crate::credentials::{
    CredentialMetadata, CredentialStore, CredentialStoreError, RUNTIME_API_KEY_CREDENTIAL_ID,
    SecretString,
};
#[cfg(windows)]
use crate::mcp::InternalBearer;
#[cfg(windows)]
use crate::mcp::{ProductionRuntimeConfig, ProductionRuntimeDriver};
use crate::runtime::RuntimeDriver;
#[cfg(windows)]
use crate::state::RuntimeFault;
#[cfg(windows)]
use crate::tunnel::{PreparedTunnelStart, TunnelId, TunnelRuntimeConfig};
#[cfg(windows)]
use std::fs;
#[cfg(windows)]
use std::io::{Read, Write};
#[cfg(windows)]
use std::net::{Ipv4Addr, TcpListener, TcpStream};
#[cfg(windows)]
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;
#[cfg(windows)]
use std::sync::Arc;
#[cfg(windows)]
use std::sync::mpsc::{self, Sender};
#[cfg(windows)]
use std::thread::{self, JoinHandle};
#[cfg(windows)]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn startup_mode_is_decided_before_tauri_window_creation() {
    assert_eq!(
        StartupMode::from_args(["LocalBridge.exe", "--background"]),
        StartupMode::Background
    );
    assert!(!StartupMode::Background.creates_main_window_at_startup());
    assert!(StartupMode::Foreground.creates_main_window_at_startup());
    assert_eq!(
        StartupMode::from_args(["LocalBridge.exe", "--other"]),
        StartupMode::Foreground
    );
}

#[test]
fn background_attention_is_silent_until_runtime_requests_final_user_attention() {
    assert_eq!(attention_action(false), BackgroundRecoveryAction::None);
    assert_eq!(
        attention_action(true),
        BackgroundRecoveryAction::ShowFinalErrorWindow
    );
}

struct FakeRuntime {
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl ExitRuntime for FakeRuntime {
    fn stop_tunnel_for_exit(&mut self) -> Result<(), DesktopExitError> {
        self.events.borrow_mut().push("tunnel.stop");
        Ok(())
    }

    fn finish_exit_after_tunnel(&mut self) -> Result<(), DesktopExitError> {
        self.events.borrow_mut().push("pep.stop");
        self.events.borrow_mut().push("mcp.stop");
        Ok(())
    }
}

struct FakePrivilege {
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl PrivilegeExit for FakePrivilege {
    fn close_gate_and_stop_broker(&self) -> Result<(), DesktopExitError> {
        self.events.borrow_mut().push("privileged_gate.close");
        self.events.borrow_mut().push("broker.stop");
        Ok(())
    }
}

#[test]
fn tray_exit_security_order_is_tunnel_gate_broker_pep_mcp() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = FakeRuntime {
        events: Rc::clone(&events),
    };
    let privilege = FakePrivilege {
        events: Rc::clone(&events),
    };
    let report = shutdown_in_security_order(Some(&mut runtime), &privilege);
    assert_eq!(report, ShutdownReport::default());
    assert_eq!(
        &*events.borrow(),
        &[
            "tunnel.stop",
            "privileged_gate.close",
            "broker.stop",
            "pep.stop",
            "mcp.stop",
        ]
    );
}

#[test]
fn shutdown_continues_after_each_stage_failure() {
    struct FailingRuntime(Rc<RefCell<Vec<&'static str>>>);
    impl ExitRuntime for FailingRuntime {
        fn stop_tunnel_for_exit(&mut self) -> Result<(), DesktopExitError> {
            self.0.borrow_mut().push("tunnel.stop");
            Err(DesktopExitError::Runtime)
        }
        fn finish_exit_after_tunnel(&mut self) -> Result<(), DesktopExitError> {
            self.0.borrow_mut().push("lower.stop");
            Err(DesktopExitError::Runtime)
        }
    }
    struct FailingPrivilege(Rc<RefCell<Vec<&'static str>>>);
    impl PrivilegeExit for FailingPrivilege {
        fn close_gate_and_stop_broker(&self) -> Result<(), DesktopExitError> {
            self.0.borrow_mut().push("privilege.stop");
            Err(DesktopExitError::Privilege)
        }
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = FailingRuntime(Rc::clone(&events));
    let privilege = FailingPrivilege(Rc::clone(&events));
    let report = shutdown_in_security_order(Some(&mut runtime), &privilege);
    assert!(report.tunnel_stop_failed);
    assert!(report.privilege_stop_failed);
    assert!(report.lower_runtime_stop_failed);
    assert_eq!(
        &*events.borrow(),
        &["tunnel.stop", "privilege.stop", "lower.stop"]
    );
}

#[test]
fn revision_cursor_advances_only_after_snapshot_publication() {
    let owner = ControlPlaneSnapshotOwner::default();
    let initial = owner.read().revision;
    let worker_owner = owner.clone();
    let worker = std::thread::spawn(move || {
        worker_owner.mark_activity_stale();
    });
    let revision = owner.wait_after(initial, std::time::Duration::from_secs(1));
    worker.join().unwrap();
    assert_eq!(revision, initial + 1);
    assert_eq!(owner.read().revision, revision);
}

#[test]
fn runtime_owner_lock_contention_marks_activity_stale_without_running_compensation() {
    let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
    lifecycle.publish_current_observation();
    let ready = lifecycle.control_plane_snapshot();
    assert!(!ready.activity.stale);
    let _runtime_owner = lifecycle
        .runtime
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    lifecycle.publish_current_observation();
    let contended = lifecycle.control_plane_snapshot();
    assert!(contended.activity.stale);
    assert_eq!(
        contended.activity.availability,
        crate::control_plane::snapshot::ProjectionAvailability::TemporarilyUnavailable
    );
    let aggregate = contended.activity.value.unwrap();
    assert!(aggregate.foreground_task.is_none());
    assert!(aggregate.detached_execution.is_none());
}

#[test]
fn snapshot_read_is_side_effect_free_and_does_not_assemble_live_state() {
    let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
    lifecycle.publish_current_observation();
    let first = lifecycle.control_plane_snapshot();
    let second = lifecycle.control_plane_snapshot();
    assert_eq!(second, first);
    assert_eq!(second.revision, first.revision);
}

#[test]
fn partial_runtime_observation_does_not_upgrade_a_stale_runtime_section() {
    let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
    {
        let _runtime_owner = lifecycle
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        lifecycle.publish_current_observation();
    }
    let stale = lifecycle.control_plane_snapshot();
    assert!(stale.runtime.stale);

    lifecycle.publish_local_environment_observation(true);
    let updated = lifecycle.control_plane_snapshot();
    assert!(updated.runtime.stale);
    assert_eq!(updated.runtime.availability, stale.runtime.availability);
    assert_eq!(
        updated
            .runtime
            .value
            .and_then(|runtime| runtime.local_environment_available),
        Some(true)
    );
}

#[test]
fn close_window_policy_defaults_to_continue_running_and_is_memory_cached() {
    let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
    assert!(lifecycle.close_window_continue_running());
    lifecycle.set_close_window_continue_running(false);
    assert!(!lifecycle.close_window_continue_running());
    lifecycle.set_close_window_continue_running(true);
    assert!(lifecycle.close_window_continue_running());
}

#[test]
fn backend_shutdown_dispatch_returns_before_deliberately_slow_cleanup_finishes() {
    struct SlowRuntime;
    impl ExitRuntime for SlowRuntime {
        fn stop_tunnel_for_exit(&mut self) -> Result<(), DesktopExitError> {
            std::thread::sleep(std::time::Duration::from_millis(250));
            Ok(())
        }

        fn finish_exit_after_tunnel(&mut self) -> Result<(), DesktopExitError> {
            Ok(())
        }
    }

    let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
    lifecycle.install_runtime_for_test(SlowRuntime).unwrap();
    let backend = lifecycle.backend_handle();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let started = std::time::Instant::now();
    let handle = backend
        .spawn_shutdown_then(move |report| {
            let _ = done_tx.send(report);
        })
        .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_millis(100),
        "dispatch must not wait for blocking lifecycle cleanup"
    );
    let report = done_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    assert_eq!(report, ShutdownReport::default());
    handle.join().unwrap();
}

#[cfg(windows)]
struct BlockingMonitorRuntime {
    entered: Option<Sender<()>>,
    release: mpsc::Receiver<()>,
}

#[cfg(windows)]
impl ExitRuntime for BlockingMonitorRuntime {
    fn stop_tunnel_for_exit(&mut self) -> Result<(), DesktopExitError> {
        Ok(())
    }

    fn finish_exit_after_tunnel(&mut self) -> Result<(), DesktopExitError> {
        Ok(())
    }

    fn runtime_snapshot(&self) -> DesktopRuntimeSnapshot {
        DesktopRuntimeSnapshot {
            active: true,
            state: RuntimeState::Ready,
            current_task: CurrentTaskStatus::Idle,
            current_task_elapsed_ms: None,
            last_tool: None,
            configured_workspace: None,
            connection_profile: None,
            outage: None,
        }
    }

    fn monitor_recovery(&mut self) -> Option<RecoveryOutcome> {
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
            let _ = self.release.recv_timeout(Duration::from_secs(5));
        }
        None
    }
}

#[cfg(windows)]
#[test]
fn explicit_control_cancels_before_owner_lock_and_snapshot_remains_responsive() {
    let lifecycle = Arc::new(DesktopLifecycle::new(PrivilegeController::new()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    lifecycle
        .install_runtime_for_test(BlockingMonitorRuntime {
            entered: Some(entered_tx),
            release: release_rx,
        })
        .unwrap();
    let permit = lifecycle.recovery_cancellation.permit();

    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("watchdog deliberately holds runtime owner during monitor");

    let control_lifecycle = Arc::clone(&lifecycle);
    let (control_done_tx, control_done_rx) = mpsc::channel();
    let control_thread = thread::spawn(move || {
        let result = control_lifecycle.stop_runtime_for_control_plane();
        let _ = control_done_tx.send(result);
    });
    let cancel_deadline = Instant::now() + Duration::from_secs(1);
    while !permit.is_cancelled() && Instant::now() < cancel_deadline {
        thread::sleep(Duration::from_millis(5));
    }
    let cancelled_before_release = permit.is_cancelled();

    let snapshot_lifecycle = Arc::clone(&lifecycle);
    let (snapshot_tx, snapshot_rx) = mpsc::channel();
    let snapshot_thread = thread::spawn(move || {
        let _ = snapshot_tx.send(snapshot_lifecycle.runtime_snapshot());
    });
    let snapshot_result = snapshot_rx.recv_timeout(Duration::from_secs(1));

    let _ = release_tx.send(());
    let control_result = control_done_rx.recv_timeout(Duration::from_secs(2));
    snapshot_thread.join().unwrap();
    control_thread.join().unwrap();

    assert!(
        cancelled_before_release,
        "explicit control must cancel recovery before waiting for runtime_operation"
    );
    let snapshot = snapshot_result.expect("snapshot cache must not wait for runtime owner mutex");
    assert!(snapshot.active);
    assert_eq!(snapshot.state, RuntimeState::Ready);
    assert!(control_result.expect("control thread returns").is_ok());
    assert!(!lifecycle.runtime_snapshot().active);
}

#[cfg(windows)]
struct GenerationTestRuntime {
    events: Arc<Mutex<Vec<&'static str>>>,
    workspace: PathBuf,
}

#[cfg(windows)]
impl ExitRuntime for GenerationTestRuntime {
    fn stop_tunnel_for_exit(&mut self) -> Result<(), DesktopExitError> {
        self.events.lock().unwrap().push("tunnel.stop");
        Ok(())
    }

    fn finish_exit_after_tunnel(&mut self) -> Result<(), DesktopExitError> {
        self.events.lock().unwrap().push("lower.stop");
        Ok(())
    }

    fn runtime_snapshot(&self) -> DesktopRuntimeSnapshot {
        DesktopRuntimeSnapshot {
            active: true,
            state: RuntimeState::Ready,
            current_task: CurrentTaskStatus::Idle,
            current_task_elapsed_ms: None,
            last_tool: None,
            configured_workspace: Some(self.workspace.clone()),
            connection_profile: None,
            outage: None,
        }
    }
}

#[cfg(windows)]
fn ui_ready_test_config(workspace: &str) -> ProductionRuntimeConfig {
    ProductionRuntimeConfig::new(
        PathBuf::from(r"C:\LocalBridge"),
        PathBuf::from(workspace),
        PathBuf::from(r"C:\LocalBridge-health"),
        TunnelId::new("tunnel_0123456789abcdef0123456789abcdef").unwrap(),
    )
}

#[cfg(windows)]
#[test]
fn foreground_ui_ready_stage_remains_stopped_and_is_one_shot() {
    let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
    assert!(lifecycle.stage_foreground_start(ui_ready_test_config(r"C:\staged")));
    assert!(lifecycle.foreground_start_is_pending());
    let before_ready = lifecycle.runtime_snapshot();
    assert!(!before_ready.active);
    assert_eq!(before_ready.state, RuntimeState::Stopped);
    assert!(before_ready.configured_workspace.is_none());

    let first = lifecycle
        .foreground_start_pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    let second = lifecycle
        .foreground_start_pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    assert!(first.is_some());
    assert!(
        second.is_none(),
        "UI-ready startup intent must be consumable only once"
    );
    assert_eq!(lifecycle.runtime_snapshot().state, RuntimeState::Stopped);
}

#[cfg(windows)]
#[test]
fn duplicate_ui_ready_does_not_restart_existing_healthy_runtime_owner() {
    let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
    assert!(lifecycle.stage_foreground_start(ui_ready_test_config(r"C:\staged")));
    let events = Arc::new(Mutex::new(Vec::new()));
    lifecycle
        .install_runtime_for_test(GenerationTestRuntime {
            events: Arc::clone(&events),
            workspace: PathBuf::from(r"C:\healthy"),
        })
        .unwrap();

    assert!(!lifecycle.start_staged_foreground_after_ui_ready().unwrap());
    assert!(!lifecycle.start_staged_foreground_after_ui_ready().unwrap());
    assert!(events.lock().unwrap().is_empty());
    let ready = lifecycle.runtime_snapshot();
    assert!(ready.active);
    assert_eq!(ready.state, RuntimeState::Ready);
    assert_eq!(
        ready.configured_workspace.as_deref(),
        Some(Path::new(r"C:\healthy"))
    );
}

#[cfg(windows)]
#[test]
fn stale_built_runtime_is_cleaned_and_cannot_replace_newer_generation_owner_or_snapshot() {
    let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
    let backend = lifecycle.backend_handle();
    let old_generation = backend
        .runtime_control_generation
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    backend.publish_starting_if_current(old_generation, PathBuf::from(r"C:\old"));

    let new_generation = backend
        .runtime_control_generation
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    backend.publish_starting_if_current(new_generation, PathBuf::from(r"C:\new"));

    thread::sleep(RUNTIME_WATCHDOG_INTERVAL + Duration::from_millis(100));
    let after_watchdog = backend
        .runtime_snapshot_cache
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(after_watchdog.state, RuntimeState::StartingMcp);
    assert_eq!(
        after_watchdog.configured_workspace.as_deref(),
        Some(Path::new(r"C:\new"))
    );

    let stale_events = Arc::new(Mutex::new(Vec::new()));
    let _operation = backend
        .runtime_operation
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    backend
        .activate_runtime_if_current_locked(
            Box::new(GenerationTestRuntime {
                events: Arc::clone(&stale_events),
                workspace: PathBuf::from(r"C:\old"),
            }),
            old_generation,
        )
        .unwrap();
    assert_eq!(
        &*stale_events.lock().unwrap(),
        &["tunnel.stop", "lower.stop"]
    );
    assert!(!backend.runtime.lock().unwrap().is_active());
    let still_new = lifecycle.runtime_snapshot();
    assert_eq!(still_new.state, RuntimeState::StartingMcp);
    assert_eq!(
        still_new.configured_workspace.as_deref(),
        Some(Path::new(r"C:\new"))
    );

    let current_events = Arc::new(Mutex::new(Vec::new()));
    backend
        .activate_runtime_if_current_locked(
            Box::new(GenerationTestRuntime {
                events: Arc::clone(&current_events),
                workspace: PathBuf::from(r"C:\new"),
            }),
            new_generation,
        )
        .unwrap();
    assert!(backend.runtime.lock().unwrap().is_active());
    let ready = lifecycle.runtime_snapshot();
    assert_eq!(ready.state, RuntimeState::Ready);
    assert_eq!(
        ready.configured_workspace.as_deref(),
        Some(Path::new(r"C:\new"))
    );
    assert!(current_events.lock().unwrap().is_empty());
}

#[cfg(windows)]
#[test]
fn control_plane_stop_invalidates_pending_start_without_active_owner() {
    let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
    let backend = lifecycle.backend_handle();
    let pending_generation = backend
        .runtime_control_generation
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    backend.publish_starting_if_current(pending_generation, PathBuf::from(r"C:\pending"));
    assert_eq!(
        lifecycle.runtime_snapshot().state,
        RuntimeState::StartingMcp
    );

    lifecycle
        .stop_runtime_for_control_plane()
        .expect("pending generation without active owner must stop idempotently");
    let stopped = lifecycle.runtime_snapshot();
    assert_eq!(stopped.state, RuntimeState::Stopped);
    assert!(!stopped.active);
    assert!(stopped.configured_workspace.is_none());

    let stale_events = Arc::new(Mutex::new(Vec::new()));
    backend
        .activate_runtime_if_current_locked(
            Box::new(GenerationTestRuntime {
                events: Arc::clone(&stale_events),
                workspace: PathBuf::from(r"C:\pending"),
            }),
            pending_generation,
        )
        .unwrap();
    assert_eq!(
        &*stale_events.lock().unwrap(),
        &["tunnel.stop", "lower.stop"]
    );
    assert_eq!(lifecycle.runtime_snapshot().state, RuntimeState::Stopped);
}

#[cfg(windows)]
#[test]
fn manual_service_stop_invalidates_pending_start_without_active_owner() {
    let lifecycle = DesktopLifecycle::new(PrivilegeController::new());
    let backend = lifecycle.backend_handle();
    let pending_generation = backend
        .runtime_control_generation
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    backend
        .publish_starting_if_current(pending_generation, PathBuf::from(r"C:\manual-stop-pending"));
    assert_eq!(
        lifecycle.runtime_snapshot().state,
        RuntimeState::StartingMcp
    );

    assert_eq!(
        lifecycle.stop_services_for_manual_action(),
        ShutdownReport::default()
    );
    let stopped = lifecycle.runtime_snapshot();
    assert_eq!(stopped.state, RuntimeState::Stopped);
    assert!(!stopped.active);
    assert!(stopped.configured_workspace.is_none());

    let stale_events = Arc::new(Mutex::new(Vec::new()));
    backend
        .activate_runtime_if_current_locked(
            Box::new(GenerationTestRuntime {
                events: Arc::clone(&stale_events),
                workspace: PathBuf::from(r"C:\manual-stop-pending"),
            }),
            pending_generation,
        )
        .unwrap();
    assert_eq!(
        &*stale_events.lock().unwrap(),
        &["tunnel.stop", "lower.stop"]
    );
    assert_eq!(lifecycle.runtime_snapshot().state, RuntimeState::Stopped);
}

#[cfg(windows)]
const ACTUAL_TUNNEL_ID: &str = "tunnel_01301301301301301301301301301301";
#[cfg(windows)]
const ACTUAL_RUNTIME_KEY: &str = "LB013_SYNTHETIC_RUNTIME_KEY_DO_NOT_LEAK";
#[cfg(windows)]
const ACTUAL_INTERNAL_BEARER: &str = "LB013_SYNTHETIC_INTERNAL_BEARER_DO_NOT_LEAK";

#[cfg(windows)]
#[derive(Clone, Copy)]
struct ActualAdapterCredentialStore;

#[cfg(windows)]
impl CredentialStore for ActualAdapterCredentialStore {
    fn save_runtime_api_key(
        &self,
        _secret: &SecretString,
    ) -> Result<CredentialMetadata, CredentialStoreError> {
        unreachable!("LB-013 actual shutdown test never writes credentials")
    }

    fn read_runtime_api_key(&self) -> Result<Option<SecretString>, CredentialStoreError> {
        Ok(Some(SecretString::new(ACTUAL_RUNTIME_KEY)?))
    }

    fn delete_runtime_api_key(&self) -> Result<bool, CredentialStoreError> {
        Ok(false)
    }

    fn runtime_api_key_metadata(&self) -> Result<CredentialMetadata, CredentialStoreError> {
        Ok(CredentialMetadata::runtime_api_key(
            RUNTIME_API_KEY_CREDENTIAL_ID,
            true,
        ))
    }
}

#[cfg(windows)]
fn actual_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has repository parent")
        .to_path_buf()
}

#[cfg(windows)]
fn actual_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "localbridge-lb013-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("create LB-013 temp directory");
    path
}

#[cfg(windows)]
fn actual_process_is_running(pid: u32) -> bool {
    let script = format!(
        "if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ exit 0 }} else {{ exit 1 }}"
    );
    Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn actual_blocked_control_plane() -> (String, Sender<()>, JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind local control plane");
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = [0u8; 4096];
                    let count = stream.read(&mut request).unwrap_or(0);
                    let request = String::from_utf8_lossy(&request[..count]);
                    assert!(request.starts_with("GET /v1/tunnels/"));
                    let _ = release_rx.recv_timeout(Duration::from_secs(10));
                    let body = r#"{"error":"LB-013 synthetic blocked control plane"}"#;
                    let response = format!(
                        "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("LB-013 control plane accept failed: {error}"),
            }
        }
    });
    (format!("http://127.0.0.1:{port}"), release_tx, handle)
}

#[cfg(windows)]
fn actual_cleanup_dir(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match fs::remove_dir_all(path) {
            Ok(()) => return,
            Err(error) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25));
                if error.kind() == std::io::ErrorKind::NotFound {
                    return;
                }
            }
            Err(error) => panic!("remove LB-013 temp dir {}: {error}", path.display()),
        }
    }
}

#[cfg(windows)]
struct ActualMidpointPrivilege {
    controller: PrivilegeController,
    tunnel_pid: u32,
    pep_port: u16,
    mcp_port: u16,
}

#[cfg(windows)]
impl PrivilegeExit for ActualMidpointPrivilege {
    fn close_gate_and_stop_broker(&self) -> Result<(), DesktopExitError> {
        assert!(
            !actual_process_is_running(self.tunnel_pid),
            "Tunnel must already be stopped before the privileged gate/Broker stage"
        );
        assert!(
            TcpStream::connect((Ipv4Addr::LOCALHOST, self.pep_port)).is_ok(),
            "PEP must remain alive until after the privileged gate/Broker stage"
        );
        assert!(
            TcpStream::connect((Ipv4Addr::LOCALHOST, self.mcp_port)).is_ok(),
            "MCP must remain alive until after the privileged gate/Broker stage"
        );
        self.controller
            .disable()
            .map_err(|_| DesktopExitError::Privilege)
    }
}

#[cfg(windows)]
#[test]
fn production_tray_exit_owns_actual_adapter_and_stops_tunnel_gate_pep_mcp() {
    let root = actual_repo_root();
    let workspace = actual_temp_dir("workspace");
    let health = actual_temp_dir("health");
    fs::write(workspace.join("probe.txt"), b"LB-013 actual adapter\n").unwrap();

    let controller = PrivilegeController::new();
    let config = ProductionRuntimeConfig::new(
        &root,
        &workspace,
        &health,
        TunnelId::new(ACTUAL_TUNNEL_ID).unwrap(),
    );
    let mut driver =
        ProductionRuntimeDriver::new_owned(config, ActualAdapterCredentialStore, || {
            InternalBearer::new(ACTUAL_INTERNAL_BEARER)
                .map_err(|_| RuntimeFault::ConfigurationInvalid)
        })
        .with_privileged_execution(Arc::new(controller.gateway()));

    let mut mcp = driver.start_mcp().expect("actual bundled MCP starts");
    driver
        .confirm_mcp_ready(&mut mcp)
        .expect("actual bundled MCP ready");
    let mcp_port = mcp.port();
    let pep = driver.start_pep(mcp).expect("actual PEP starts");
    driver.confirm_pep_ready(&pep).expect("actual PEP ready");
    let pep_port = pep.port();

    let (control_plane, release_control_plane, control_plane_thread) =
        actual_blocked_control_plane();
    let tunnel_config = TunnelRuntimeConfig::new(
        &root,
        &health,
        TunnelId::new(ACTUAL_TUNNEL_ID).unwrap(),
        pep_port,
    )
    .unwrap()
    .with_test_control_plane_base_url(&control_plane)
    .unwrap();
    let tunnel = PreparedTunnelStart::prepare(tunnel_config, &ActualAdapterCredentialStore)
        .and_then(PreparedTunnelStart::spawn)
        .expect("actual vendored Tunnel starts");
    assert!(tunnel.root_is_running().unwrap());
    let tunnel_pid = tunnel.process_snapshot().pid;

    let runtime = RuntimeOrchestrator::from_ready_for_test(driver, pep, tunnel);
    let lifecycle = DesktopLifecycle::new(controller.clone());
    lifecycle
        .install_runtime_for_test(runtime)
        .expect("actual production runtime registered under DesktopLifecycle");

    let midpoint = ActualMidpointPrivilege {
        controller,
        tunnel_pid,
        pep_port,
        mcp_port,
    };
    let report = lifecycle.shutdown_with_privilege_for_test(&midpoint);
    assert_eq!(report, ShutdownReport::default());
    assert!(!actual_process_is_running(tunnel_pid));
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, pep_port)).is_err());
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, mcp_port)).is_err());

    let _ = release_control_plane.send(());
    control_plane_thread.join().unwrap();
    actual_cleanup_dir(&workspace);
    actual_cleanup_dir(&health);
}
