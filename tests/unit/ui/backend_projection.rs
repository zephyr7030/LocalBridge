use super::*;
use crate::runtime::{RuntimeDriver, RuntimeOrchestrator};
use crate::state::{CurrentTaskStatus, RuntimeFault, SafeTaskSummary, TaskExecutionState, TaskKind};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn presentation_codes_are_stable_and_never_direct_internal_enum_names() {
    assert_eq!(stored_permission_code(StoredPermissionMode::Edit), "edit");
    assert_eq!(stored_permission_code(StoredPermissionMode::Full), "full");
    assert_eq!(stored_permission_code(StoredPermissionMode::Elevated), "admin");
    assert_eq!(privilege_code(&PrivilegeState::Disabled), "off");
    assert_eq!(privilege_code(&PrivilegeState::Requested), "requested");
    assert_eq!(privilege_code(&PrivilegeState::AwaitingUac), "awaiting");
    assert_eq!(privilege_code(&PrivilegeState::Active { broker_generation: crate::state::GenerationId::new(99) }), "active");
    assert_eq!(privilege_code(&PrivilegeState::Faulted(crate::state::PrivilegeFault::BrokerExited)), "fault");
    for (state, expected) in [
        (RuntimeState::Stopped, ("off", "off")),
        (RuntimeState::StartingMcp, ("off", "starting")),
        (RuntimeState::StartingTunnel, ("starting", "online")),
        (RuntimeState::Ready, ("online", "online")),
        (RuntimeState::Recovering { component: RuntimeComponent::Tunnel, attempt: 2 }, ("recovering", "online")),
        (RuntimeState::Recovering { component: RuntimeComponent::PolicyEnforcement, attempt: 1 }, ("recovering", "recovering")),
        (RuntimeState::Recovering { component: RuntimeComponent::CodingRuntime, attempt: 0 }, ("recovering", "recovering")),
        (RuntimeState::Faulted(RuntimeFault::Unknown), ("fault", "fault")),
    ] {
        assert_eq!(service_codes(&state), expected);
    }
    for (state, expected) in [
        (RuntimeState::Stopped, "off"),
        (RuntimeState::StartingMcp, "starting"),
        (RuntimeState::WaitingMcpReady, "starting"),
        (RuntimeState::StartingPolicyEnforcement, "online"),
        (RuntimeState::StartingTunnel, "online"),
        (RuntimeState::Ready, "online"),
        (
            RuntimeState::Recovering {
                component: RuntimeComponent::CodingRuntime,
                attempt: 1,
            },
            "recovering",
        ),
        (
            RuntimeState::Recovering {
                component: RuntimeComponent::Tunnel,
                attempt: 1,
            },
            "online",
        ),
        (RuntimeState::Faulted(RuntimeFault::Unknown), "fault"),
    ] {
        assert_eq!(local_environment_service_code(&state), expected);
    }
    let rendered = serde_json::to_string(&MainProjection {
        permission: "admin", privilege: "active", local_environment_service: "online", tunnel_service: "online", coding_service: "online",
        current_project: None, projects: vec![], current_task: None, current_workflow: None, current_command: None, last_command: None, last_tool: None,
        current_activity: None, last_activity: None, projection_revision: 7, tunnel_id: Some("tunnel_01401401401401401401401401401401".to_owned()),
        runtime_key_saved: true, auto_start: true, close_window_continue_running: true,
        reconnect: None,
    }).unwrap();
    for forbidden in ["Elevated", "AwaitingUac", "BrokerExited", "RuntimeState", "PrivilegeState", "broker_generation", "nonce", "pid"] {
        assert!(!rendered.contains(forbidden));
    }
}

#[test]
fn schema42_task_aggregate_projection_separates_current_and_history() {
    let waiting = serde_json::json!({
        "state":"waiting",
        "current_workflow":{"state":"waiting"},
        "current_command":null,
        "last_command":{"status":"cancelled","completed_at_ms":0},
        "current_activity":{"kind":"other","state":"waiting","summary":null,"elapsed_ms":null,"step":"verify","progress_current":2,"progress_total":4},
        "last_activity":{"kind":"command","summary":"cargo test","outcome":"cancelled","completed_at_ms":7}
    });
    assert_eq!(current_workflow_projection(&waiting).unwrap().state, "waiting");
    assert!(current_command_projection(&waiting).is_none());
    assert_eq!(last_command_projection(&waiting).unwrap().status, "cancelled");
    let current = current_activity_projection(&waiting).unwrap();
    assert_eq!(current.kind, "other");
    assert_eq!(current.state, "waiting");
    assert_eq!(current.step.as_deref(), Some("verify"));
    assert_eq!(current.progress_current, Some(2));
    assert_eq!(current.progress_total, Some(4));
    let last = last_activity_projection(&waiting).unwrap();
    assert_eq!(last.kind, "command");
    assert_eq!(last.summary.as_deref(), Some("cargo test"));
    assert_eq!(last.outcome, "cancelled");
    assert_eq!(last.completed_at_ms, 7);
    let idle = serde_json::json!({"state":"idle","current_workflow":null,"current_command":null,"last_command":null});
    assert!(current_workflow_projection(&idle).is_none());
    assert!(current_command_projection(&idle).is_none());
    assert!(last_command_projection(&idle).is_none());
    let running = serde_json::json!({"state":"active","current_workflow":{"state":"running"},"current_command":{"state":"running"},"last_command":{"status":"completed","completed_at_ms":0}});
    assert_eq!(current_command_projection(&running).unwrap().state, "running");
    assert_eq!(legacy_task_projection_from_aggregate(&running, Some(10)).unwrap().kind, "command");
}

#[test]
fn current_task_projection_uses_only_pre_redacted_summary() {
    let safe = CurrentTaskStatus::project(TaskKind::Test, SafeTaskSummary::from_untrusted("cargo test"), TaskExecutionState::Running).unwrap();
    let projected = task_projection(&safe, Some(1234)).unwrap();
    assert_eq!(projected.kind, "test");
    assert_eq!(projected.summary.as_deref(), Some("cargo test"));
    assert_eq!(projected.state, "running");
    assert_eq!(projected.elapsed_ms, Some(1234));
    let secret = CurrentTaskStatus::project(TaskKind::ExecuteCommand, SafeTaskSummary::from_untrusted("--api-key=synthetic-secret"), TaskExecutionState::Blocked).unwrap();
    let projected = task_projection(&secret, None).unwrap();
    assert_eq!(projected.summary, None);
    assert_eq!(projected.state, "blocked");
    assert_eq!(task_projection(&CurrentTaskStatus::Idle, None), None);
}

#[derive(Clone)]
struct ModeDriver { observed: Rc<RefCell<Vec<PermissionMode>>> }
impl RuntimeDriver for ModeDriver {
    type Mcp = (); type Pep = (); type Tunnel = ();
    fn start_mcp(&mut self) -> Result<Self::Mcp, RuntimeFault> { Ok(()) }
    fn confirm_mcp_ready(&mut self, _: &mut Self::Mcp) -> Result<(), RuntimeFault> { Ok(()) }
    fn start_pep(&mut self, _: Self::Mcp) -> Result<Self::Pep, RuntimeFault> { Ok(()) }
    fn confirm_pep_ready(&mut self, _: &Self::Pep) -> Result<(), RuntimeFault> { Ok(()) }
    fn start_tunnel(&mut self, _: &Self::Pep) -> Result<Self::Tunnel, RuntimeFault> { Ok(()) }
    fn confirm_tunnel_ready(&mut self, _: &mut Self::Tunnel) -> Result<(), RuntimeFault> { Ok(()) }
    fn stop_tunnel(&mut self, _: &mut Self::Tunnel) -> Result<(), RuntimeFault> { Ok(()) }
    fn stop_pep(&mut self, _: Self::Pep) -> Result<Self::Mcp, RuntimeFault> { Ok(()) }
    fn stop_mcp(&mut self, _: &mut Self::Mcp) -> Result<(), RuntimeFault> { Ok(()) }
    fn current_task(&self, _: &Self::Pep) -> CurrentTaskStatus { CurrentTaskStatus::Idle }
    fn set_permission_mode(&mut self, _: &Self::Pep, mode: PermissionMode) -> Result<(), RuntimeFault> {
        self.observed.borrow_mut().push(mode); Ok(())
    }
}

#[test]
fn live_permission_port_immediately_reaches_ready_driver() {
    let observed = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = RuntimeOrchestrator::new(ModeDriver { observed: observed.clone() });
    runtime.start().unwrap();
    runtime.set_permission_mode(PermissionMode::Full).unwrap();
    runtime.set_permission_mode(PermissionMode::Elevated).unwrap();
    assert_eq!(&*observed.borrow(), &[PermissionMode::Full, PermissionMode::Elevated]);
}
