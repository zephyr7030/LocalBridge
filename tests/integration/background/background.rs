use super::*;
use std::cell::RefCell;
use std::rc::Rc;

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
