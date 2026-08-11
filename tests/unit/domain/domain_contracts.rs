#[path = "../../../src-tauri/src/state/mod.rs"]
mod state;

use state::{
    ActiveWorkspaceState, CurrentTaskStatus, PrivilegeState, SafeTaskSummary, TaskExecutionState,
    TaskKind, WorkspaceControlState,
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
fn privilege_and_workspace_defaults_are_fail_closed() {
    assert!(!PrivilegeState::Disabled.accepts_privileged_calls());
    let workspace = WorkspaceControlState::default();
    assert_eq!(workspace.active(), &ActiveWorkspaceState::NoActiveWorkspace);
}
