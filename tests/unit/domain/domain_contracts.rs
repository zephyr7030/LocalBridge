#[path = "../../../src-tauri/src/state/mod.rs"]
mod state;

use state::{
    CurrentTaskContractError, CurrentTaskStatus, PrivilegeState, SafeTaskSummary,
    TaskExecutionState, TaskKind,
};

#[test]
fn required_current_task_states_are_representable() {
    assert_eq!(CurrentTaskStatus::default(), CurrentTaskStatus::Idle);
    for state in [
        TaskExecutionState::Running,
        TaskExecutionState::Blocked,
        TaskExecutionState::Failed,
        TaskExecutionState::Cancelled,
    ] {
        assert!(
            CurrentTaskStatus::project(TaskKind::Other, SafeTaskSummary::Omitted, state).is_ok()
        );
    }
}

#[test]
fn task_kind_contract_contains_no_upstream_tool_identifiers() {
    let source = include_str!("../../../src-tauri/src/state/task.rs");
    for forbidden in ["mcp__coding_tools__", "tools/call", "apply_patch"] {
        assert!(
            !source.contains(forbidden),
            "domain source depends on upstream tool id: {forbidden}"
        );
    }
}

#[test]
fn task_summary_rejects_common_secret_assignments_without_overmatching_tokenizer() {
    for raw in [
        "token=secret",
        "password = secret",
        "?access_token=secret",
        "?refresh_token=secret",
        "{\"api_key\":\"secret\"}",
        "Authorization: Bearer secret",
        "--client-secret secret",
        "--passphrase secret",
    ] {
        assert_eq!(
            SafeTaskSummary::from_untrusted(raw),
            SafeTaskSummary::Omitted
        );
    }
    assert_eq!(
        SafeTaskSummary::from_untrusted("search src/tokenizer.rs"),
        SafeTaskSummary::Text("search src/tokenizer.rs".to_string())
    );
}

#[test]
fn terminal_current_task_cannot_transition_back_to_running() {
    for terminal in [
        TaskExecutionState::Blocked,
        TaskExecutionState::Failed,
        TaskExecutionState::Cancelled,
    ] {
        let mut current = CurrentTaskStatus::project(
            TaskKind::Other,
            SafeTaskSummary::from_untrusted("safe operation"),
            terminal,
        )
        .unwrap();
        assert_eq!(
            current.set_state(TaskExecutionState::Running),
            Err(CurrentTaskContractError::InvalidStateTransition {
                from: terminal,
                to: TaskExecutionState::Running,
            })
        );
        current.set_state(TaskExecutionState::Idle).unwrap();
        assert_eq!(current, CurrentTaskStatus::Idle);
    }
}

#[test]
fn privilege_default_is_fail_closed() {
    assert!(!PrivilegeState::Disabled.accepts_privileged_calls());
}
