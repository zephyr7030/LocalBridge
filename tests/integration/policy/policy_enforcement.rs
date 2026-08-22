use std::cell::RefCell;
use std::rc::Rc;

use localbridge_lib::execution::{CapabilityPolicy, DenyReason};
use localbridge_lib::mcp::{
    CodingToolsRuntimeError, GuardError, GuardRuntime, McpGuard, ToolCallRequest,
};
use localbridge_lib::state::{
    Capability, CurrentTask, CurrentTaskStatus, PermissionMode, SafeTaskSummary, TaskExecutionState,
};
use serde_json::{Value, json};

const ALL_TOOLS: &[&str] = &[
    "server_info",
    "check_exec_environment",
    "get_default_cwd",
    "set_default_cwd",
    "read_file",
    "list_dir",
    "list_files",
    "search_text",
    "apply_patch",
    "exec_command",
    "write_stdin",
    "kill_session",
    "read_output",
    "git_status",
    "git_diff",
    "git_log",
    "git_show",
    "git_blame",
    "request_permissions",
    "view_image",
    "future_unknown_tool",
];

#[derive(Debug)]
struct FakeRuntime {
    calls: Rc<RefCell<Vec<String>>>,
    fail_calls: bool,
}

impl FakeRuntime {
    fn new(calls: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            calls,
            fail_calls: false,
        }
    }
    fn failing(calls: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            calls,
            fail_calls: true,
        }
    }
}

impl GuardRuntime for FakeRuntime {
    fn raw_list_tools(&mut self) -> Result<Value, CodingToolsRuntimeError> {
        Ok(json!({"tools": ALL_TOOLS.iter().map(|name| json!({"name":name})).collect::<Vec<_>>() }))
    }

    fn raw_call_tool(
        &mut self,
        name: &str,
        _arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, CodingToolsRuntimeError> {
        self.calls.borrow_mut().push(name.to_string());
        if self.fail_calls {
            Err(CodingToolsRuntimeError::ProtocolMismatch)
        } else {
            Ok(json!({"ok":true,"called":name}))
        }
    }
}

fn policy() -> CapabilityPolicy {
    CapabilityPolicy::from_toml(include_str!("../../../runtime-policy.toml")).unwrap()
}

fn active_state(status: &CurrentTaskStatus) -> Option<&CurrentTask> {
    match status {
        CurrentTaskStatus::Active(task) => Some(task),
        CurrentTaskStatus::Idle => None,
    }
}

#[test]
fn tools_list_is_ux_only_and_cached_full_list_cannot_bypass_edit_call_policy() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut guard = McpGuard::new(FakeRuntime::new(calls.clone()), policy());
    let full = guard.filtered_tools(PermissionMode::Full).unwrap();
    let full_names = full["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(full_names.len(), 19);
    assert!(full_names.contains(&"exec_command"));
    assert!(!full_names.contains(&"request_permissions"));
    assert!(!full_names.contains(&"future_unknown_tool"));

    let edit = guard.filtered_tools(PermissionMode::Edit).unwrap();
    assert_eq!(edit["tools"].as_array().unwrap().len(), 15);

    let mut states = Vec::new();
    let denied = guard.call_tool(
        PermissionMode::Edit,
        ToolCallRequest::new("exec_command", json!({"cmd":"echo should-not-run"})),
        |status| states.push(status),
    );
    assert!(matches!(denied, Err(GuardError::Denied(_))));
    assert!(calls.borrow().is_empty());
    assert!(states.iter().all(|state| !matches!(active_state(state), Some(task) if task.state == TaskExecutionState::Running)));
    assert!(
        matches!(active_state(&states[0]), Some(task) if task.state == TaskExecutionState::Blocked)
    );
    assert_eq!(states.last(), Some(&CurrentTaskStatus::Idle));
}

#[test]
fn workflow_indirect_exec_and_control_plane_are_blocked_before_upstream() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut guard = McpGuard::new(FakeRuntime::new(calls.clone()), policy());
    for request in [
        ToolCallRequest::new("server_info", json!({}))
            .with_indirect_capabilities([Capability::Workflow, Capability::ProcessExec]),
        ToolCallRequest::new("request_permissions", json!({"permission":"network"})),
        ToolCallRequest::new("workspace_select", json!({"path":"D:/other"})),
        ToolCallRequest::new("future_unknown_tool", json!({})),
    ] {
        let mut states = Vec::new();
        assert!(matches!(
            guard.call_tool(PermissionMode::Edit, request, |s| states.push(s)),
            Err(GuardError::Denied(_))
        ));
        assert!(states.iter().all(
            |s| !matches!(active_state(s), Some(task) if task.state == TaskExecutionState::Running)
        ));
    }
    assert!(calls.borrow().is_empty());
}

#[test]
fn win32_verbatim_execution_paths_are_blocked_before_upstream_without_scanning_patch_content() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut guard = McpGuard::new(FakeRuntime::new(calls.clone()), policy());
    let denied = [
        ToolCallRequest::new("read_file", json!({"path":r"\\?\C:\project\probe.txt"})),
        ToolCallRequest::new("git_diff", json!({"paths":[r"\\?\C:\project\probe.txt"]})),
        ToolCallRequest::new("set_default_cwd", json!({"path":r"\\?\C:\project"})),
        ToolCallRequest::new(
            "exec_command",
            json!({"cmd":"echo safe","cwd":r"\\?\C:\project"}),
        ),
        ToolCallRequest::new(
            "exec_command",
            json!({"cmd":r"type \\?\C:\project\probe.txt"}),
        ),
        ToolCallRequest::new(
            "exec_command",
            json!({"cmd":"powershell -Command -","stdin":r"Get-Item \\?\C:\project"}),
        ),
        ToolCallRequest::new(
            "exec_command",
            json!({"cmd":"echo %LB_PATH%","env":{"LB_PATH":r"\\?\C:\project"}}),
        ),
        ToolCallRequest::new(
            "write_stdin",
            json!({"session_id":"synthetic","chars":r"cd \\?\C:\project\n"}),
        ),
    ];
    for request in denied {
        assert_eq!(
            guard.decision(PermissionMode::Full, &request).deny_reason,
            Some(DenyReason::VerbatimExecutionPath)
        );
        let mut states = Vec::new();
        let result = guard.call_tool(PermissionMode::Full, request, |state| states.push(state));
        assert!(matches!(
            result,
            Err(GuardError::Denied(denied))
                if denied.reason == DenyReason::VerbatimExecutionPath
        ));
        assert!(matches!(
            active_state(&states[0]),
            Some(task) if task.state == TaskExecutionState::Blocked
        ));
        assert_eq!(states.last(), Some(&CurrentTaskStatus::Idle));
    }
    assert!(calls.borrow().is_empty());

    let allowed = guard.call_tool(
        PermissionMode::Edit,
        ToolCallRequest::new(
            "apply_patch",
            json!({"patch":r"*** Begin Patch\n*** Add File: docs/probe.txt\n+literal \\?\C:\project for documentation\n*** End Patch"}),
        ),
        |_| {},
    );
    assert!(
        allowed.is_ok(),
        "patch content is data, not an execution-path argument"
    );
    assert_eq!(&*calls.borrow(), &["apply_patch"]);
}

#[test]
fn ordinary_shell_content_never_changes_current_user_authority() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut guard = McpGuard::new(FakeRuntime::new(calls.clone()), policy());
    let commands = [
        ("cmd", "sc query EventLog"),
        ("cmd", "call .\\policy_probe.cmd"),
        (
            "windows_powershell",
            "$program='sc'; & $program query EventLog",
        ),
        ("cmd", "docker version"),
    ];

    for (shell, command) in commands {
        let request = ToolCallRequest::new("exec_command", json!({"cmd":command,"shell":shell}));
        for mode in [PermissionMode::Full, PermissionMode::Elevated] {
            let decision = guard.decision(mode, &request);
            assert!(
                decision.allowed,
                "ordinary current-user command was content-classified in {mode:?}: {command}"
            );
        }
        let mut states = Vec::new();
        let response = guard
            .call_tool(PermissionMode::Full, request, |state| states.push(state))
            .expect("current-user command forwards");
        assert_eq!(response["called"], "exec_command");
        assert!(matches!(
            active_state(&states[0]),
            Some(task) if task.state == TaskExecutionState::Running
        ));
        assert_eq!(states.last(), Some(&CurrentTaskStatus::Idle));
    }
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "exec_command",
            "exec_command",
            "exec_command",
            "exec_command"
        ]
    );

    let explicit_privileged = ToolCallRequest::new(
        "exec_command",
        json!({"cmd":"opaque structured capability"}),
    )
    .with_indirect_capabilities([Capability::PrivilegedExternalRuntime]);
    let decision = guard.decision(PermissionMode::Full, &explicit_privileged);
    assert!(!decision.allowed);
    assert_eq!(
        decision.deny_reason,
        Some(DenyReason::PrivilegedRouteNotAvailable)
    );
}

#[test]
fn allowed_call_projects_actual_running_then_idle_and_redacts_secret_summary() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut guard = McpGuard::new(FakeRuntime::new(calls.clone()), policy());
    let mut states = Vec::new();
    let result = guard
        .call_tool(
            PermissionMode::Full,
            ToolCallRequest::new(
                "exec_command",
                json!({"cmd":"tool --api-key=synthetic-secret"}),
            ),
            |status| states.push(status),
        )
        .unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(&*calls.borrow(), &["exec_command"]);
    assert!(matches!(
        active_state(&states[0]),
        Some(CurrentTask {
            state: TaskExecutionState::Running,
            summary: SafeTaskSummary::Omitted,
            ..
        })
    ));
    assert_eq!(states.last(), Some(&CurrentTaskStatus::Idle));
}

#[test]
fn runtime_failure_is_grounded_in_real_forwarding_event_then_returns_idle() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut guard = McpGuard::new(FakeRuntime::failing(calls.clone()), policy());
    let mut states = Vec::new();
    let result = guard.call_tool(
        PermissionMode::Edit,
        ToolCallRequest::new("read_file", json!({"path":"src/lib.rs"})),
        |status| states.push(status),
    );
    assert!(matches!(result, Err(GuardError::Runtime(_))));
    assert_eq!(&*calls.borrow(), &["read_file"]);
    assert!(
        matches!(active_state(&states[0]), Some(task) if task.state == TaskExecutionState::Running)
    );
    assert!(
        matches!(active_state(&states[1]), Some(task) if task.state == TaskExecutionState::Failed)
    );
    assert_eq!(states[2], CurrentTaskStatus::Idle);
}
