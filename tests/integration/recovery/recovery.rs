use super::*;
use crate::state::CurrentTaskStatus;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

type SharedEvents = Rc<RefCell<Vec<&'static str>>>;
type SharedFailures = Rc<RefCell<usize>>;
type SharedHealth = Rc<RefCell<bool>>;
type SharedFault = Rc<RefCell<RuntimeFault>>;

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
    fail_mcp_starts: SharedFailures,
    fail_tunnel_starts: SharedFailures,
    tunnel_start_fault: SharedFault,
    pep_healthy: SharedHealth,
    mcp_healthy: SharedHealth,
    mcp_fault: SharedFault,
    tunnel_healthy: SharedHealth,
    workspace: PathBuf,
}

impl RecoveryDriver {
    fn new() -> (Self, SharedEvents, SharedFailures, SharedHealth, SharedHealth) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let fail_mcp_starts = Rc::new(RefCell::new(0));
        let fail_tunnel_starts = Rc::new(RefCell::new(0));
        let tunnel_start_fault = Rc::new(RefCell::new(RuntimeFault::TunnelExited));
        let pep_healthy = Rc::new(RefCell::new(true));
        let mcp_healthy = Rc::new(RefCell::new(true));
        let mcp_fault = Rc::new(RefCell::new(RuntimeFault::McpExited));
        let tunnel_healthy = Rc::new(RefCell::new(true));
        (
            Self {
                events: events.clone(),
                fail_mcp_starts,
                fail_tunnel_starts: fail_tunnel_starts.clone(),
                tunnel_start_fault,
                pep_healthy: pep_healthy.clone(),
                mcp_healthy: mcp_healthy.clone(),
                mcp_fault,
                tunnel_healthy,
                workspace: PathBuf::from(r"D:\project\active"),
            },
            events,
            fail_tunnel_starts,
            pep_healthy,
            mcp_healthy,
        )
    }
    fn event(&self, value: &'static str) { self.events.borrow_mut().push(value); }
    fn set_tunnel_start_fault(&self, fault: RuntimeFault) {
        *self.tunnel_start_fault.borrow_mut() = fault;
    }
    fn set_mcp_fault(&self, fault: RuntimeFault) {
        *self.mcp_fault.borrow_mut() = fault;
    }
}

impl RuntimeDriver for RecoveryDriver {
    type Mcp = &'static str;
    type Pep = &'static str;
    type Tunnel = &'static str;

    fn start_mcp(&mut self) -> Result<Self::Mcp, RuntimeFault> {
        self.event("mcp.start");
        let mut remaining = self.fail_mcp_starts.borrow_mut();
        if *remaining > 0 {
            *remaining -= 1;
            return Err(RuntimeFault::McpSpawnFailed);
        }
        Ok("mcp")
    }
    fn confirm_mcp_ready(&mut self, _mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> {
        self.event("mcp.ready");
        if *self.mcp_healthy.borrow() { Ok(()) } else { Err(self.mcp_fault.borrow().clone()) }
    }
    fn start_pep(&mut self, _mcp: Self::Mcp) -> Result<Self::Pep, RuntimeFault> { self.event("pep.start"); Ok("pep") }
    fn confirm_pep_ready(&mut self, _pep: &Self::Pep) -> Result<(), RuntimeFault> {
        self.event("pep.ready");
        if *self.pep_healthy.borrow() { Ok(()) } else { Err(RuntimeFault::PolicyBindFailed) }
    }
    fn start_tunnel(&mut self, _pep: &Self::Pep) -> Result<Self::Tunnel, RuntimeFault> {
        self.event("tunnel.start");
        let mut remaining = self.fail_tunnel_starts.borrow_mut();
        if *remaining > 0 {
            *remaining -= 1;
            return Err(self.tunnel_start_fault.borrow().clone());
        }
        *self.tunnel_healthy.borrow_mut() = true;
        Ok("tunnel")
    }
    fn confirm_tunnel_ready(&mut self, _tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> { self.event("tunnel.ready"); Ok(()) }
    fn confirm_tunnel_ready_for_recovery(
        &mut self,
        _tunnel: &mut Self::Tunnel,
        _permit: &RecoveryPermit,
    ) -> Result<(), RuntimeFault> {
        self.event("tunnel.ready");
        Ok(())
    }
    fn stop_tunnel(&mut self, _tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> { self.event("tunnel.stop"); Ok(()) }
    fn stop_pep(&mut self, _pep: Self::Pep) -> Result<Self::Mcp, RuntimeFault> { self.event("pep.stop"); Ok("mcp") }
    fn stop_mcp(&mut self, _mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> { self.event("mcp.stop"); Ok(()) }
    fn current_task(&self, _pep: &Self::Pep) -> CurrentTaskStatus { CurrentTaskStatus::Idle }
    fn probe_mcp_health(&mut self, _pep: &Self::Pep) -> Result<(), RuntimeFault> {
        self.event("mcp.monitor");
        if *self.mcp_healthy.borrow() { Ok(()) } else { Err(self.mcp_fault.borrow().clone()) }
    }
    fn probe_pep_health(&mut self, _pep: &Self::Pep) -> Result<(), RuntimeFault> {
        self.event("pep.monitor");
        if *self.pep_healthy.borrow() { Ok(()) } else { Err(RuntimeFault::PolicyBindFailed) }
    }
    fn probe_tunnel_health(&mut self, _tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> {
        self.event("tunnel.monitor");
        if *self.tunnel_healthy.borrow() { Ok(()) } else { Err(RuntimeFault::TunnelExited) }
    }
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
    let (driver, events, fail_counter, _, _) = RecoveryDriver::new();
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
    let request_id = runtime.active_outage().unwrap().request_id.clone();
    assert_eq!(controller.clock().sleeps, [1,2,5,10,30].map(Duration::from_secs));
    assert_eq!(controller.current_attempt(), 5);
    assert_eq!(runtime.active_outage().unwrap().request_id, request_id);
    assert!(!runtime.mark_user_attention_required(generation));

    let sleeps_after_exhaustion = controller.clock().sleeps.len();
    let events_after_exhaustion = events.borrow().len();
    let repeated = controller.recover_auto(
        &mut runtime,
        RuntimeOutage::classify(RuntimeComponent::Tunnel, RuntimeFault::TunnelExited),
    );
    match repeated {
        RecoveryOutcome::Exhausted { generation: repeated_generation, user_attention_required, .. } => {
            assert_eq!(repeated_generation, generation);
            assert!(!user_attention_required);
        }
        other => panic!("same exhausted generation must not restart recovery, got {other:?}"),
    }
    assert_eq!(controller.clock().sleeps.len(), sleeps_after_exhaustion);
    assert_eq!(events.borrow().len(), events_after_exhaustion);
    assert_eq!(controller.current_attempt(), 5);

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
    assert_ne!(runtime.active_outage().unwrap().request_id, request_id);
}

#[test]
fn recoverable_generation_stops_immediately_when_attempt_becomes_nonrecoverable() {
    let (driver, events, fail_counter, _, _) = RecoveryDriver::new();
    driver.set_tunnel_start_fault(RuntimeFault::RuntimeKeyMissing);
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    events.borrow_mut().clear();
    *fail_counter.borrow_mut() = 5;
    let mut controller = RecoveryController::new(FakeClock::default());

    let outcome = controller.recover_auto(
        &mut runtime,
        RuntimeOutage::classify(RuntimeComponent::Tunnel, RuntimeFault::TunnelExited),
    );
    let generation = match outcome {
        RecoveryOutcome::NonRecoverable {
            generation,
            fault,
            user_attention_required,
        } => {
            assert_eq!(fault, RuntimeFault::RuntimeKeyMissing);
            assert!(user_attention_required);
            generation
        }
        other => panic!("expected post-attempt nonrecoverable stop, got {other:?}"),
    };

    assert_eq!(controller.current_attempt(), 1);
    assert_eq!(controller.clock().sleeps, [Duration::from_secs(1)]);
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| **event == "tunnel.start")
            .count(),
        1
    );
    let outage = runtime.active_outage().expect("terminal outage remains observable");
    assert_eq!(outage.id, generation);
    assert_eq!(outage.fault, RuntimeFault::RuntimeKeyMissing);
    assert!(outage.user_attention_emitted());
    assert!(!runtime.mark_user_attention_required(generation));
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
fn monitor_automatically_detects_post_ready_tunnel_failure_and_resets_after_stability() {
    let (driver, events, _, _, _) = RecoveryDriver::new();
    let tunnel_healthy = driver.tunnel_healthy.clone();
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    let mut monitored = AutoRecoveryRuntime::new(runtime, FakeClock::default());
    events.borrow_mut().clear();
    *tunnel_healthy.borrow_mut() = false;

    assert!(monitored.monitor_once().is_none(), "detection schedules attempt 1 instead of sleeping in the watchdog");
    assert!(monitored.recovery_clock().sleeps.is_empty());
    assert_eq!(
        &*events.borrow(),
        &["mcp.monitor", "pep.monitor", "tunnel.monitor"]
    );
    monitored.recovery_clock_mut().advance(Duration::from_secs(1));
    let outcome = monitored.monitor_once().expect("deadline performs exactly one recovery attempt");
    let generation = match outcome {
        RecoveryOutcome::Recovered { generation, attempt } => {
            assert_eq!(attempt, 1);
            generation
        }
        other => panic!("expected automatic recovery, got {other:?}"),
    };
    assert_eq!(
        &*events.borrow(),
        &[
            "mcp.monitor",
            "pep.monitor",
            "tunnel.monitor",
            "tunnel.stop",
            "pep.ready",
            "tunnel.start",
            "tunnel.ready",
        ]
    );
    assert_eq!(monitored.runtime().active_outage().map(|outage| outage.id), Some(generation));

    monitored.recovery_clock_mut().advance(Duration::from_secs(59));
    assert!(monitored.monitor_once().is_none());
    assert!(monitored.runtime().active_outage().is_some());
    monitored.recovery_clock_mut().advance(Duration::from_secs(1));
    assert!(monitored.monitor_once().is_none());
    assert!(monitored.runtime().active_outage().is_none());
}

#[test]
fn monitor_exhaustion_is_terminal_until_persistent_controller_manual_retry() {
    let (driver, events, fail_counter, _, _) = RecoveryDriver::new();
    let tunnel_healthy = driver.tunnel_healthy.clone();
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    let mut monitored = AutoRecoveryRuntime::new(runtime, FakeClock::default());
    events.borrow_mut().clear();
    *tunnel_healthy.borrow_mut() = false;
    *fail_counter.borrow_mut() = 5;

    assert!(monitored.monitor_once().is_none(), "outage detection only schedules attempt 1");
    let delays = [1, 2, 5, 10, 30];
    let mut exhausted = None;
    for (index, delay) in delays.into_iter().enumerate() {
        monitored
            .recovery_clock_mut()
            .advance(Duration::from_secs(delay));
        let outcome = monitored.monitor_once();
        if index < 4 {
            assert!(outcome.is_none(), "attempt {} schedules the next deadline", index + 1);
        } else {
            exhausted = outcome;
        }
        assert!(monitored.recovery_clock().sleeps.is_empty(), "cooperative auto recovery never sleeps");
    }
    let exhausted_generation = match exhausted.expect("fifth cooperative attempt exhausts the generation") {
        RecoveryOutcome::Exhausted { generation, user_attention_required, .. } => {
            assert!(user_attention_required);
            generation
        }
        other => panic!("expected exhausted automatic recovery, got {other:?}"),
    };
    assert!(monitored.recovery_clock().sleeps.is_empty());
    let event_count = events.borrow().len();

    assert!(monitored.monitor_once().is_none(), "Faulted exhausted runtime must not auto-start another generation");
    assert_eq!(events.borrow().len(), event_count);
    assert!(monitored.recovery_clock().sleeps.is_empty());

    *fail_counter.borrow_mut() = 0;
    let manual_generation = match monitored
        .manual_retry_current_outage()
        .expect("manual retry must use retained outage")
    {
        RecoveryOutcome::Recovered { generation, attempt } => {
            assert_eq!(attempt, 1);
            generation
        }
        other => panic!("expected manual recovery, got {other:?}"),
    };
    assert_ne!(manual_generation, exhausted_generation);
    assert_eq!(monitored.runtime().state(), &RuntimeState::Ready);
}

#[test]
fn cooperative_auto_cancellation_during_backoff_consumes_no_attempt_or_attention() {
    let (driver, events, _, _, _) = RecoveryDriver::new();
    let tunnel_healthy = driver.tunnel_healthy.clone();
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    let mut monitored = AutoRecoveryRuntime::new(runtime, FakeClock::default());
    events.borrow_mut().clear();
    *tunnel_healthy.borrow_mut() = false;

    assert!(monitored.monitor_once().is_none());
    let generation = monitored
        .runtime()
        .active_outage()
        .expect("detection creates one outage generation")
        .id;
    let cancellation = monitored.cancellation();
    cancellation.cancel();
    monitored.recovery_clock_mut().advance(Duration::from_secs(30));
    assert!(monitored.monitor_once().is_none());
    assert_eq!(monitored.controller.current_attempt(), 0);
    assert!(monitored.recovery_clock().sleeps.is_empty());
    assert_eq!(
        events.borrow().iter().filter(|event| **event == "tunnel.start").count(),
        0
    );
    let outage = monitored.runtime().active_outage().unwrap();
    assert_eq!(outage.id, generation);
    assert!(!outage.user_attention_emitted());
}

#[test]
fn successful_workspace_switch_retires_exhausted_generation_and_new_workspace_gets_full_budget() {
    let (driver, _, fail_counter, _, _) = RecoveryDriver::new();
    let tunnel_healthy = driver.tunnel_healthy.clone();
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    let mut monitored = AutoRecoveryRuntime::new(runtime, FakeClock::default());
    *tunnel_healthy.borrow_mut() = false;
    *fail_counter.borrow_mut() = 5;

    assert!(monitored.monitor_once().is_none());
    let mut exhausted = None;
    for (index, delay) in [1, 2, 5, 10, 30].into_iter().enumerate() {
        monitored.recovery_clock_mut().advance(Duration::from_secs(delay));
        let outcome = monitored.monitor_once();
        if index == 4 { exhausted = outcome; } else { assert!(outcome.is_none()); }
    }
    let old_generation = match exhausted.expect("old workspace exhausts") {
        RecoveryOutcome::Exhausted { generation, user_attention_required, .. } => {
            assert!(user_attention_required);
            generation
        }
        other => panic!("expected old exhausted generation, got {other:?}"),
    };
    assert_eq!(monitored.controller.current_attempt(), 5);
    assert!(monitored.runtime().active_outage().unwrap().user_attention_emitted());

    *fail_counter.borrow_mut() = 0;
    monitored.cancellation().cancel();
    monitored
        .switch_workspace_after_control_cancellation(Path::new(r"D:\project\replacement"))
        .expect("explicit switch to replacement workspace succeeds");
    assert_eq!(monitored.runtime().state(), &RuntimeState::Ready);
    assert!(monitored.runtime().active_outage().is_none());
    assert_eq!(monitored.controller.active_generation(), None);
    assert_eq!(monitored.controller.current_attempt(), 0);

    *tunnel_healthy.borrow_mut() = false;
    *fail_counter.borrow_mut() = 5;
    assert!(monitored.monitor_once().is_none());
    let new_generation = monitored.runtime().active_outage().unwrap().id;
    assert_ne!(new_generation, old_generation);
    let mut new_exhausted = None;
    for (index, delay) in [1, 2, 5, 10, 30].into_iter().enumerate() {
        monitored.recovery_clock_mut().advance(Duration::from_secs(delay));
        let outcome = monitored.monitor_once();
        if index == 4 { new_exhausted = outcome; } else { assert!(outcome.is_none()); }
    }
    assert!(matches!(
        new_exhausted,
        Some(RecoveryOutcome::Exhausted { generation, user_attention_required: true, .. })
            if generation == new_generation
    ));
    assert_eq!(monitored.controller.current_attempt(), 5);
    assert!(monitored.recovery_clock().sleeps.is_empty());
}

#[test]
fn failed_workspace_switch_retires_stale_auto_recovery_fail_closed() {
    let (driver, events, _, _, _) = RecoveryDriver::new();
    let fail_mcp_starts = driver.fail_mcp_starts.clone();
    let tunnel_healthy = driver.tunnel_healthy.clone();
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    let mut monitored = AutoRecoveryRuntime::new(runtime, FakeClock::default());
    events.borrow_mut().clear();
    *tunnel_healthy.borrow_mut() = false;

    assert!(monitored.monitor_once().is_none());
    assert!(monitored.runtime().active_outage().is_some());
    assert!(monitored.pending_auto.is_some());

    monitored.cancellation().cancel();
    *fail_mcp_starts.borrow_mut() = 1;
    let error = monitored
        .switch_workspace_after_control_cancellation(Path::new(r"D:\project\replacement"))
        .expect_err("candidate start fails without rollback compensation");
    assert_eq!(error.candidate_fault, RuntimeFault::McpSpawnFailed);
    assert_eq!(
        monitored.runtime().state(),
        &RuntimeState::Faulted(RuntimeFault::McpSpawnFailed)
    );
    assert!(monitored.pending_auto.is_none(), "stale automatic recovery must be retired");
    assert_eq!(monitored.controller.active_generation(), None);
    assert!(monitored.runtime().active_outage().is_none());
    assert_eq!(monitored.controller.current_attempt(), 0);

    let starts_after_failure = events
        .borrow()
        .iter()
        .filter(|event| **event == "mcp.start")
        .count();
    for _ in 0..3 {
        monitored.recovery_clock_mut().advance(Duration::from_secs(60));
        assert!(monitored.monitor_once().is_none());
    }
    assert_eq!(
        events.borrow().iter().filter(|event| **event == "mcp.start").count(),
        starts_after_failure,
        "Faulted candidate failure must not reactivate the stale generation"
    );
    assert!(
        monitored.manual_retry_current_outage().is_none(),
        "retired stale outage cannot be replayed against an uncertain workspace configuration"
    );
}

#[test]
fn cooperative_attempt_stops_on_new_nonrecoverable_fault_without_later_deadlines() {
    let (driver, events, fail_counter, _, _) = RecoveryDriver::new();
    let tunnel_healthy = driver.tunnel_healthy.clone();
    driver.set_tunnel_start_fault(RuntimeFault::RuntimeKeyMissing);
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    let mut monitored = AutoRecoveryRuntime::new(runtime, FakeClock::default());
    events.borrow_mut().clear();
    *tunnel_healthy.borrow_mut() = false;
    *fail_counter.borrow_mut() = 5;

    assert!(monitored.monitor_once().is_none());
    monitored.recovery_clock_mut().advance(Duration::from_secs(1));
    let outcome = monitored.monitor_once().expect("attempt 1 becomes terminal");
    assert!(matches!(
        outcome,
        RecoveryOutcome::NonRecoverable {
            fault: RuntimeFault::RuntimeKeyMissing,
            user_attention_required: true,
            ..
        }
    ));
    assert!(monitored.recovery_clock().sleeps.is_empty());
    assert_eq!(
        events.borrow().iter().filter(|event| **event == "tunnel.start").count(),
        1
    );
    monitored.recovery_clock_mut().advance(Duration::from_secs(60));
    assert!(monitored.monitor_once().is_none());
    assert_eq!(
        events.borrow().iter().filter(|event| **event == "tunnel.start").count(),
        1
    );
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
fn process_alive_but_authenticated_mcp_unresponsive_recovers_through_existing_full_runtime_path() {
    let (driver, events, _, _, mcp_healthy) = RecoveryDriver::new();
    driver.set_mcp_fault(RuntimeFault::McpHealthTimeout);
    let mut runtime = RuntimeOrchestrator::new(driver);
    runtime.start().unwrap();
    let mut monitored = AutoRecoveryRuntime::new(runtime, FakeClock::default());
    events.borrow_mut().clear();

    *mcp_healthy.borrow_mut() = false;
    assert!(monitored.monitor_once().is_none());
    assert!(matches!(
        monitored.runtime().state(),
        RuntimeState::Recovering {
            component: RuntimeComponent::CodingRuntime,
            attempt: 0,
        }
    ));
    assert_eq!(
        monitored.runtime().active_outage().unwrap().fault,
        RuntimeFault::McpHealthTimeout
    );

    *mcp_healthy.borrow_mut() = true;
    monitored.recovery_clock_mut().advance(Duration::from_secs(1));
    let outcome = monitored.monitor_once().expect("first MCP recovery attempt completes");
    assert!(matches!(outcome, RecoveryOutcome::Recovered { attempt: 1, .. }));
    assert_eq!(monitored.runtime().state(), &RuntimeState::Ready);

    let observed = events.borrow();
    for marker in [
        "tunnel.stop", "pep.stop", "mcp.stop", "mcp.start", "mcp.ready", "pep.start",
        "pep.ready", "tunnel.start", "tunnel.ready",
    ] {
        assert!(observed.contains(&marker), "missing recovery step {marker}: {observed:?}");
    }
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
