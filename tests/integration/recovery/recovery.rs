use super::*;
use crate::state::CurrentTaskStatus;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

type SharedEvents = Rc<RefCell<Vec<&'static str>>>;
type SharedFailures = Rc<RefCell<usize>>;
type SharedHealth = Rc<RefCell<bool>>;

#[derive(Debug, Default)]
struct FakeClock {
    now: Duration,
    sleeps: Vec<Duration>,
}

impl FakeClock {
    fn advance(&mut self, duration: Duration) { self.now += duration; }
}

impl RecoveryClock for FakeClock {
    fn now(&self) -> Duration { self.now }
    fn sleep(&mut self, duration: Duration) { self.sleeps.push(duration); self.now += duration; }
}

#[derive(Clone)]
struct RecoveryDriver {
    events: SharedEvents,
    fail_tunnel_starts: SharedFailures,
    pep_healthy: SharedHealth,
    mcp_healthy: SharedHealth,
    workspace: PathBuf,
}

impl RecoveryDriver {
    fn new() -> (Self, SharedEvents, SharedFailures, SharedHealth, SharedHealth) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let fail_tunnel_starts = Rc::new(RefCell::new(0));
        let pep_healthy = Rc::new(RefCell::new(true));
        let mcp_healthy = Rc::new(RefCell::new(true));
        (
            Self {
                events: events.clone(),
                fail_tunnel_starts: fail_tunnel_starts.clone(),
                pep_healthy: pep_healthy.clone(),
                mcp_healthy: mcp_healthy.clone(),
                workspace: PathBuf::from(r"D:\project\active"),
            },
            events,
            fail_tunnel_starts,
            pep_healthy,
            mcp_healthy,
        )
    }
    fn event(&self, value: &'static str) { self.events.borrow_mut().push(value); }
}

impl RuntimeDriver for RecoveryDriver {
    type Mcp = &'static str;
    type Pep = &'static str;
    type Tunnel = &'static str;

    fn start_mcp(&mut self) -> Result<Self::Mcp, RuntimeFault> { self.event("mcp.start"); Ok("mcp") }
    fn confirm_mcp_ready(&mut self, _mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> {
        self.event("mcp.ready");
        if *self.mcp_healthy.borrow() { Ok(()) } else { Err(RuntimeFault::McpExited) }
    }
    fn start_pep(&mut self, _mcp: Self::Mcp) -> Result<Self::Pep, RuntimeFault> { self.event("pep.start"); Ok("pep") }
    fn confirm_pep_ready(&mut self, _pep: &Self::Pep) -> Result<(), RuntimeFault> {
        self.event("pep.ready");
        if *self.pep_healthy.borrow() { Ok(()) } else { Err(RuntimeFault::PolicyBindFailed) }
    }
    fn start_tunnel(&mut self, _pep: &Self::Pep) -> Result<Self::Tunnel, RuntimeFault> {
        self.event("tunnel.start");
        let mut remaining = self.fail_tunnel_starts.borrow_mut();
        if *remaining > 0 { *remaining -= 1; return Err(RuntimeFault::TunnelExited); }
        Ok("tunnel")
    }
    fn confirm_tunnel_ready(&mut self, _tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> { self.event("tunnel.ready"); Ok(()) }
    fn stop_tunnel(&mut self, _tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> { self.event("tunnel.stop"); Ok(()) }
    fn stop_pep(&mut self, _pep: Self::Pep) -> Result<Self::Mcp, RuntimeFault> { self.event("pep.stop"); Ok("mcp") }
    fn stop_mcp(&mut self, _mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> { self.event("mcp.stop"); Ok(()) }
    fn current_task(&self, _pep: &Self::Pep) -> CurrentTaskStatus { CurrentTaskStatus::Idle }
    fn current_workspace(&self) -> Option<&Path> { Some(&self.workspace) }
    fn configure_workspace(&mut self, workspace: PathBuf) -> Result<(), RuntimeFault> { self.workspace = workspace; Ok(()) }
}

#[test]
fn tunnel_outage_restarts_only_tunnel_when_lower_dependencies_are_healthy() {
    let (driver, events, _, _, _) = RecoveryDriver::new();
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    events.borrow_mut().clear();
    let mut controller = RecoveryController::new(FakeClock::default());
    let outcome = controller.recover_auto(
        &mut runtime,
        RuntimeOutage::classify(RuntimeComponent::Tunnel, RuntimeFault::TunnelExited),
    );
    assert!(matches!(outcome, RecoveryOutcome::Recovered { attempt: 1, .. }));
    assert_eq!(&*events.borrow(), &["tunnel.stop", "pep.ready", "tunnel.start", "tunnel.ready"]);
}

#[test]
fn policy_and_mcp_outages_restart_only_the_required_dependency_layers() {
    let (driver, events, _, _, _) = RecoveryDriver::new();
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    events.borrow_mut().clear();
    let mut controller = RecoveryController::new(FakeClock::default());
    let _ = controller.recover_auto(
        &mut runtime,
        RuntimeOutage::classify(RuntimeComponent::PolicyEnforcement, RuntimeFault::PolicyBindFailed),
    );
    assert_eq!(&*events.borrow(), &["tunnel.stop", "pep.stop", "mcp.ready", "pep.start", "pep.ready", "tunnel.start", "tunnel.ready"]);

    events.borrow_mut().clear();
    let _ = controller.recover_auto(
        &mut runtime,
        RuntimeOutage::classify(RuntimeComponent::CodingRuntime, RuntimeFault::McpExited),
    );
    assert_eq!(&*events.borrow(), &["tunnel.stop", "pep.stop", "mcp.stop", "mcp.start", "mcp.ready", "pep.start", "pep.ready", "tunnel.start", "tunnel.ready"]);
}

#[test]
fn exact_five_attempts_backoff_nonretryable_and_manual_generation_are_deterministic() {
    let (driver, _, fail_counter, _, _) = RecoveryDriver::new();
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    *fail_counter.borrow_mut() = 5;
    let mut controller = RecoveryController::new(FakeClock::default());
    let first = controller.recover_auto(
        &mut runtime,
        RuntimeOutage::classify(RuntimeComponent::Tunnel, RuntimeFault::TunnelExited),
    );
    let generation = match first {
        RecoveryOutcome::Exhausted { generation, user_attention_required, .. } => {
            assert!(user_attention_required);
            generation
        }
        other => panic!("expected exhaustion, got {other:?}"),
    };
    assert_eq!(controller.clock().sleeps, [1,2,5,10,30].map(Duration::from_secs));
    assert_eq!(controller.current_attempt(), 5);
    assert!(!runtime.mark_user_attention_required(generation));

    let before = controller.clock().sleeps.len();
    let non = controller.recover_auto(
        &mut runtime,
        RuntimeOutage::classify(RuntimeComponent::Tunnel, RuntimeFault::TunnelAuthFailed),
    );
    assert!(matches!(non, RecoveryOutcome::NonRecoverable { user_attention_required: false, .. }));
    assert_eq!(controller.clock().sleeps.len(), before);

    *fail_counter.borrow_mut() = 0;
    let manual = controller.manual_retry(
        &mut runtime,
        RuntimeOutage::classify(RuntimeComponent::Tunnel, RuntimeFault::TunnelExited),
    );
    let new_generation = match manual { RecoveryOutcome::Recovered { generation, .. } => generation, other => panic!("{other:?}") };
    assert_ne!(new_generation, generation);
}

#[test]
fn successful_recovery_keeps_generation_until_sixty_seconds_of_stable_ready() {
    let (driver, _, _, _, _) = RecoveryDriver::new();
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    let mut controller = RecoveryController::new(FakeClock::default());
    let outcome = controller.recover_auto(
        &mut runtime,
        RuntimeOutage::classify(RuntimeComponent::Tunnel, RuntimeFault::TunnelExited),
    );
    let generation = match outcome { RecoveryOutcome::Recovered { generation, .. } => generation, other => panic!("{other:?}") };
    assert_eq!(controller.active_generation(), Some(generation));
    controller.clock_mut().advance(Duration::from_secs(59));
    assert!(!controller.observe_stable_ready(&mut runtime));
    controller.clock_mut().advance(Duration::from_secs(1));
    assert!(controller.observe_stable_ready(&mut runtime));
    assert_eq!(controller.active_generation(), None);
    assert!(runtime.active_outage().is_none());
}

#[test]
fn unhealthy_dependencies_escalate_once_per_attempt_without_recursive_restart_storm() {
    let (driver, events, _, pep_healthy, mcp_healthy) = RecoveryDriver::new();
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    events.borrow_mut().clear();
    *pep_healthy.borrow_mut() = false;
    *mcp_healthy.borrow_mut() = false;
    let mut controller = RecoveryController::new(FakeClock::default());
    let outcome = controller.recover_auto(
        &mut runtime,
        RuntimeOutage::classify(RuntimeComponent::Tunnel, RuntimeFault::TunnelExited),
    );
    assert!(matches!(outcome, RecoveryOutcome::Exhausted { .. }));
    assert_eq!(controller.clock().sleeps, [1, 2, 5, 10, 30].map(Duration::from_secs));
    let observed = events.borrow();
    assert!(observed.windows(4).any(|window| window == ["tunnel.stop", "pep.ready", "pep.stop", "mcp.ready"]));
    assert_eq!(observed.iter().filter(|event| **event == "tunnel.stop").count(), 1);
    assert_eq!(observed.iter().filter(|event| **event == "mcp.start").count(), 5);
}

#[test]
fn retryability_is_derived_from_typed_fault_and_not_caller_override() {
    for fault in [
        RuntimeFault::TunnelExited,
        RuntimeFault::TunnelHealthTimeout,
        RuntimeFault::McpExited,
        RuntimeFault::McpHealthTimeout,
        RuntimeFault::PolicyBindFailed,
    ] {
        assert_eq!(
            RuntimeOutage::classify(RuntimeComponent::Tunnel, fault).disposition,
            RecoveryDisposition::Recoverable
        );
    }
    for fault in [
        RuntimeFault::TunnelAuthFailed,
        RuntimeFault::RuntimeKeyMissing,
        RuntimeFault::RuntimeChecksumMismatch,
        RuntimeFault::SecretInjectionUnsupported,
        RuntimeFault::ConfigurationInvalid,
        RuntimeFault::PolicyInvalid,
    ] {
        assert_eq!(
            RuntimeOutage::classify(RuntimeComponent::Tunnel, fault).disposition,
            RecoveryDisposition::NonRecoverable
        );
    }
}
