use super::*;
#[cfg(windows)]
use crate::control_plane::convergence::{
    DesiredState, DesiredStateOwner, DesiredWorkspace, ServiceIntent,
};
#[cfg(windows)]
use crate::mcp::{InternalBearer, ProductionRuntimeConfig, ProductionRuntimeDriver};
#[cfg(windows)]
use crate::state::PermissionMode;
use crate::state::{SafeTaskSummary, TaskExecutionState, TaskKind};
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(windows)]
use crate::credentials::{CredentialMetadata, CredentialStore, CredentialStoreError, SecretString};
#[cfg(windows)]
use crate::tunnel::TunnelId;
#[cfg(windows)]
use std::fs;
#[cfg(windows)]
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type SharedEvents = Rc<RefCell<Vec<&'static str>>>;
type SharedTask = Rc<RefCell<CurrentTaskStatus>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailurePoint {
    StartMcp,
    ConfirmMcp,
    StartPep,
    ConfirmPep,
    StartTunnel,
    ConfirmTunnel,
    StopTunnel,
    StopPep,
    StopMcp,
}

#[derive(Clone)]
struct FakeDriver {
    events: SharedEvents,
    failure: Option<FailurePoint>,
    task: SharedTask,
}

impl FakeDriver {
    fn new(failure: Option<FailurePoint>) -> (Self, SharedEvents, SharedTask) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let task = Rc::new(RefCell::new(CurrentTaskStatus::Idle));
        (
            Self {
                events: events.clone(),
                failure,
                task: task.clone(),
            },
            events,
            task,
        )
    }

    fn event(&self, event: &'static str) {
        self.events.borrow_mut().push(event);
    }

    fn fail(&self, point: FailurePoint) -> Result<(), RuntimeFault> {
        if self.failure == Some(point) {
            Err(RuntimeFault::Unknown)
        } else {
            Ok(())
        }
    }
}

impl RuntimeDriver for FakeDriver {
    type Mcp = &'static str;
    type Pep = &'static str;
    type Tunnel = &'static str;

    fn start_mcp(&mut self) -> Result<Self::Mcp, RuntimeFault> {
        self.event("mcp.start");
        self.fail(FailurePoint::StartMcp)?;
        Ok("mcp")
    }

    fn confirm_mcp_ready(&mut self, _mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> {
        self.event("mcp.ready");
        self.fail(FailurePoint::ConfirmMcp)
    }

    fn start_pep(&mut self, _mcp: Self::Mcp) -> Result<Self::Pep, RuntimeFault> {
        self.event("pep.start");
        if self.failure == Some(FailurePoint::StartPep) {
            self.event("pep.start.failure-owned-mcp-cleanup");
            return Err(RuntimeFault::Unknown);
        }
        Ok("pep")
    }

    fn confirm_pep_ready(&mut self, _pep: &Self::Pep) -> Result<(), RuntimeFault> {
        self.event("pep.ready");
        self.fail(FailurePoint::ConfirmPep)
    }

    fn start_tunnel(&mut self, _pep: &Self::Pep) -> Result<Self::Tunnel, RuntimeFault> {
        self.event("tunnel.start");
        self.fail(FailurePoint::StartTunnel)?;
        Ok("tunnel")
    }

    fn confirm_tunnel_ready(&mut self, _tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> {
        self.event("tunnel.ready");
        self.fail(FailurePoint::ConfirmTunnel)
    }

    fn stop_tunnel(&mut self, _tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> {
        self.event("tunnel.stop");
        self.fail(FailurePoint::StopTunnel)
    }

    fn stop_pep(&mut self, _pep: Self::Pep) -> Result<Self::Mcp, RuntimeFault> {
        self.event("pep.stop");
        self.fail(FailurePoint::StopPep)?;
        Ok("mcp")
    }

    fn stop_mcp(&mut self, _mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> {
        self.event("mcp.stop");
        self.fail(FailurePoint::StopMcp)
    }

    fn current_task(&self, _pep: &Self::Pep) -> CurrentTaskStatus {
        self.task.borrow().clone()
    }
}

#[test]
fn fake_sidecars_follow_mcp_pep_tunnel_ready_and_reverse_manual_stop() {
    let (driver, events, _) = FakeDriver::new(None);
    let mut orchestrator = RuntimeOrchestrator::new(driver);
    let mut states = Vec::new();
    orchestrator
        .start_with_state_projection(|state| states.push(state.clone()))
        .unwrap();
    assert_eq!(
        states,
        vec![
            RuntimeState::StartingMcp,
            RuntimeState::WaitingMcpReady,
            RuntimeState::StartingPolicyEnforcement,
            RuntimeState::WaitingPolicyReady,
            RuntimeState::StartingTunnel,
            RuntimeState::WaitingTunnelReady,
            RuntimeState::Ready,
        ]
    );
    assert_eq!(orchestrator.state(), &RuntimeState::Ready);
    assert_eq!(
        &*events.borrow(),
        &[
            "mcp.start",
            "mcp.ready",
            "pep.start",
            "pep.ready",
            "tunnel.start",
            "tunnel.ready"
        ]
    );

    orchestrator.stop().unwrap();
    assert_eq!(orchestrator.state(), &RuntimeState::Stopped);
    assert_eq!(
        &*events.borrow(),
        &[
            "mcp.start",
            "mcp.ready",
            "pep.start",
            "pep.ready",
            "tunnel.start",
            "tunnel.ready",
            "tunnel.stop",
            "pep.stop",
            "mcp.stop",
        ]
    );
}

#[test]
fn repeated_start_is_rejected_without_state_or_resource_divergence_and_restart_after_stop_works() {
    let (driver, events, _) = FakeDriver::new(None);
    let mut orchestrator = RuntimeOrchestrator::new(driver);
    orchestrator.start().unwrap();
    let before = events.borrow().clone();

    let error = orchestrator.start().unwrap_err();
    assert_eq!(error.fault, RuntimeFault::ConfigurationInvalid);
    assert_eq!(orchestrator.state(), &RuntimeState::Ready);
    assert_eq!(&*events.borrow(), &before);

    orchestrator.stop().unwrap();
    assert_eq!(orchestrator.state(), &RuntimeState::Stopped);
    orchestrator.start().unwrap();
    assert_eq!(orchestrator.state(), &RuntimeState::Ready);
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| **event == "mcp.start")
            .count(),
        2
    );
    orchestrator.stop().unwrap();
}

#[test]
fn staged_start_failures_cleanup_every_owned_lower_layer_in_reverse_order() {
    let cases: &[(FailurePoint, &[&str])] = &[
        (FailurePoint::StartMcp, &["mcp.start"]),
        (
            FailurePoint::ConfirmMcp,
            &["mcp.start", "mcp.ready", "mcp.stop"],
        ),
        (
            FailurePoint::StartPep,
            &[
                "mcp.start",
                "mcp.ready",
                "pep.start",
                "pep.start.failure-owned-mcp-cleanup",
            ],
        ),
        (
            FailurePoint::ConfirmPep,
            &[
                "mcp.start",
                "mcp.ready",
                "pep.start",
                "pep.ready",
                "pep.stop",
                "mcp.stop",
            ],
        ),
        (
            FailurePoint::StartTunnel,
            &[
                "mcp.start",
                "mcp.ready",
                "pep.start",
                "pep.ready",
                "tunnel.start",
                "pep.stop",
                "mcp.stop",
            ],
        ),
        (
            FailurePoint::ConfirmTunnel,
            &[
                "mcp.start",
                "mcp.ready",
                "pep.start",
                "pep.ready",
                "tunnel.start",
                "tunnel.ready",
                "tunnel.stop",
                "pep.stop",
                "mcp.stop",
            ],
        ),
    ];
    for (failure, expected) in cases {
        let (driver, events, _) = FakeDriver::new(Some(*failure));
        let mut orchestrator = RuntimeOrchestrator::new(driver);
        let error = orchestrator.start().unwrap_err();
        assert_eq!(error.fault, RuntimeFault::Unknown);
        assert!(matches!(
            orchestrator.state(),
            RuntimeState::Faulted(RuntimeFault::Unknown)
        ));
        assert_eq!(&*events.borrow(), *expected, "failure point: {failure:?}");
        assert_eq!(orchestrator.current_task(), CurrentTaskStatus::Idle);
    }
}

#[test]
fn cleanup_failure_does_not_prevent_remaining_reverse_shutdown() {
    let (driver, events, _) = FakeDriver::new(Some(FailurePoint::StopTunnel));
    let mut orchestrator = RuntimeOrchestrator::new(driver);
    orchestrator.start().unwrap();
    let error = orchestrator.stop().unwrap_err();
    assert_eq!(error.fault, RuntimeFault::Unknown);
    assert_eq!(
        &events.borrow()[events.borrow().len() - 3..],
        &["tunnel.stop", "pep.stop", "mcp.stop"]
    );
    assert!(matches!(
        orchestrator.state(),
        RuntimeState::Faulted(RuntimeFault::Unknown)
    ));
    assert_eq!(orchestrator.current_task(), CurrentTaskStatus::Idle);
}

#[test]
fn current_task_is_single_live_pep_projection_and_terminal_state_can_clear_to_idle() {
    let (driver, _, task) = FakeDriver::new(None);
    let mut orchestrator = RuntimeOrchestrator::new(driver);
    orchestrator.start().unwrap();
    *task.borrow_mut() = CurrentTaskStatus::project(
        TaskKind::Test,
        SafeTaskSummary::from_untrusted("cargo test"),
        TaskExecutionState::Failed,
    )
    .unwrap();
    assert!(matches!(
        orchestrator.current_task(),
        CurrentTaskStatus::Active(crate::state::CurrentTask {
            state: TaskExecutionState::Failed,
            ..
        })
    ));
    *task.borrow_mut() = CurrentTaskStatus::Idle;
    assert_eq!(orchestrator.current_task(), CurrentTaskStatus::Idle);
    orchestrator.stop().unwrap();
    assert_eq!(orchestrator.current_task(), CurrentTaskStatus::Idle);
}

#[test]
fn outage_generation_emits_final_user_attention_once_per_generation_only() {
    let (driver, _, _) = FakeDriver::new(None);
    let mut orchestrator = RuntimeOrchestrator::new(driver);
    let first = orchestrator.begin_outage(RuntimeComponent::Tunnel, RuntimeFault::TunnelExited);
    assert_eq!(first.get(), 1);
    assert!(orchestrator.mark_user_attention_required(first));
    assert!(!orchestrator.mark_user_attention_required(first));
    assert!(
        orchestrator
            .active_outage()
            .unwrap()
            .user_attention_emitted()
    );

    let second = orchestrator.begin_outage(
        RuntimeComponent::PolicyEnforcement,
        RuntimeFault::PolicyInvalid,
    );
    assert_eq!(second.get(), 2);
    assert!(!orchestrator.mark_user_attention_required(first));
    assert!(orchestrator.mark_user_attention_required(second));
    assert!(!orchestrator.mark_user_attention_required(second));
    assert!(orchestrator.clear_outage(second));
    assert!(orchestrator.active_outage().is_none());
}

#[test]
fn stop_pep_or_mcp_failures_are_typed_without_inventing_user_visible_history() {
    for failure in [FailurePoint::StopPep, FailurePoint::StopMcp] {
        let (driver, events, _) = FakeDriver::new(Some(failure));
        let mut orchestrator = RuntimeOrchestrator::new(driver);
        orchestrator.start().unwrap();
        let error = orchestrator.stop().unwrap_err();
        assert_eq!(error.fault, RuntimeFault::Unknown);
        assert!(events.borrow().contains(&"tunnel.stop"));
        assert!(events.borrow().contains(&"pep.stop"));
        if failure == FailurePoint::StopMcp {
            assert!(events.borrow().contains(&"mcp.stop"));
        }
        assert_eq!(orchestrator.current_task(), CurrentTaskStatus::Idle);
    }
}

#[cfg(windows)]
struct NoopCredentialStore;

#[cfg(windows)]
impl CredentialStore for NoopCredentialStore {
    fn save_runtime_api_key(
        &self,
        _secret: &SecretString,
    ) -> Result<CredentialMetadata, CredentialStoreError> {
        unreachable!("LB-009 production composition test never starts Tunnel")
    }

    fn read_runtime_api_key(&self) -> Result<Option<SecretString>, CredentialStoreError> {
        unreachable!("LB-009 production composition test never starts Tunnel")
    }

    fn delete_runtime_api_key(&self) -> Result<bool, CredentialStoreError> {
        unreachable!("LB-009 production composition test never starts Tunnel")
    }

    fn runtime_api_key_metadata(&self) -> Result<CredentialMetadata, CredentialStoreError> {
        unreachable!("LB-009 production composition test never starts Tunnel")
    }
}

#[cfg(windows)]
fn production_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has repository parent")
        .to_path_buf()
}

#[cfg(windows)]
fn production_temp_workspace() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "localbridge-lb009-production-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("probe.txt"), b"LB009 production composition\n").unwrap();
    path
}

#[cfg(windows)]
fn cleanup_production_test_directory(path: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match fs::remove_dir_all(path) {
            Ok(()) => return,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Other
                ) && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => panic!(
                "cleanup LB-009 production test directory {}: {error}",
                path.display()
            ),
        }
    }
}

#[cfg(windows)]
#[test]
fn production_driver_composes_actual_bundled_mcp_then_loopback_pep_and_recovers_ownership() {
    const SYNTHETIC_BEARER: &str = "LB009_PRODUCTION_DRIVER_BEARER_SYNTHETIC_DO_NOT_LEAK";
    let root = production_repo_root();
    let workspace = production_temp_workspace();
    let health = workspace.join("health");
    let config = ProductionRuntimeConfig::new(
        &root,
        &workspace,
        &health,
        TunnelId::new("tunnel_0123456789abcdef0123456789abcdef").unwrap(),
    );
    let store = NoopCredentialStore;
    let desired = DesiredStateOwner::default();
    desired.replace(DesiredState {
        permission: PermissionMode::Full,
        workspace: Some(DesiredWorkspace::for_runtime_path(&workspace)),
        services: ServiceIntent::Enabled,
        connection: None,
    });
    let mut driver = ProductionRuntimeDriver::new(config, &store, || {
        InternalBearer::new(SYNTHETIC_BEARER).map_err(|_| RuntimeFault::ConfigurationInvalid)
    })
    .with_control_plane_state(desired, None);

    let mut mcp = driver.start_mcp().expect("actual bundled MCP start");
    let mcp_pid = mcp.process_snapshot().pid;
    driver
        .confirm_mcp_ready(&mut mcp)
        .expect("actual bundled MCP remains ready");
    let pep = driver.start_pep(mcp).expect("actual loopback PEP start");
    driver
        .confirm_pep_ready(&pep)
        .expect("actual PEP listener ready");
    assert!(pep.endpoint().starts_with("http://127.0.0.1:"));
    assert!(pep.port() > 0);
    assert_eq!(driver.current_task(&pep), CurrentTaskStatus::Idle);
    assert!(!format!("{pep:?}").contains(SYNTHETIC_BEARER));

    let mut mcp = driver.stop_pep(pep).expect("PEP returns owned MCP");
    assert_eq!(mcp.process_snapshot().pid, mcp_pid);
    driver.stop_mcp(&mut mcp).expect("MCP Job stop");
    assert_eq!(mcp.active_processes().unwrap(), 0);
    drop(mcp);
    cleanup_production_test_directory(&workspace);
}
