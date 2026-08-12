use localbridge_lib::mcp::{CapabilityPolicy, DenyReason, ToolCallRequest};
use localbridge_lib::state::{Capability, PermissionMode};
use serde_json::json;

fn policy() -> CapabilityPolicy {
    CapabilityPolicy::from_toml(include_str!("../../../runtime-policy.toml")).unwrap()
}

#[test]
fn policy_file_is_exact_and_unknown_fails_closed() {
    let policy = policy();
    for mode in [PermissionMode::Edit, PermissionMode::Full, PermissionMode::Elevated] {
        let request = ToolCallRequest::new("future_unreviewed_tool", json!({}));
        let decision = policy.decide(mode, &request.name, &request.indirect_capabilities);
        assert!(!decision.allowed);
        assert_eq!(decision.deny_reason, Some(DenyReason::UnknownTool));
        assert_eq!(decision.descriptor.capability, Capability::Unknown);
    }
}

#[test]
fn pinned_mode_matrix_matches_lb000_review() {
    let policy = policy();
    for (name, capability) in [
        ("server_info", Capability::Read),
        ("check_exec_environment", Capability::Read),
        ("get_default_cwd", Capability::Read),
        ("set_default_cwd", Capability::Write),
        ("read_file", Capability::Read),
        ("list_dir", Capability::Read),
        ("list_files", Capability::Read),
        ("search_text", Capability::Read),
        ("apply_patch", Capability::Write),
        ("exec_command", Capability::ProcessExec),
        ("write_stdin", Capability::ProcessExec),
        ("kill_session", Capability::ProcessExec),
        ("read_output", Capability::ProcessExec),
        ("git_status", Capability::Git),
        ("git_diff", Capability::Git),
        ("git_log", Capability::Git),
        ("git_show", Capability::Git),
        ("git_blame", Capability::Git),
        ("request_permissions", Capability::ControlPlane),
        ("view_image", Capability::Read),
    ] {
        assert_eq!(policy.classify(name).capability, capability, "capability drift: {name}");
    }
    assert!(policy.decide(PermissionMode::Edit, "read_file", &[]).allowed);
    assert!(policy.decide(PermissionMode::Edit, "apply_patch", &[]).allowed);
    assert!(!policy.decide(PermissionMode::Edit, "exec_command", &[]).allowed);
    assert!(policy.decide(PermissionMode::Full, "exec_command", &[]).allowed);
    assert!(policy.decide(PermissionMode::Elevated, "exec_command", &[]).allowed);
    for mode in [PermissionMode::Edit, PermissionMode::Full, PermissionMode::Elevated] {
        let decision = policy.decide(mode, "request_permissions", &[]);
        assert!(!decision.allowed);
        assert_eq!(decision.deny_reason, Some(DenyReason::ControlPlane));
    }
}

#[test]
fn localbridge_control_plane_and_indirect_workflow_exec_are_denied() {
    let policy = policy();
    for name in ["workspace_select","workspace_add","workspace_remove","permission_mode_change","credential_reset","tunnel_config_write","mcp_config_write","localbridge.workspace.select"] {
        let decision = policy.decide(PermissionMode::Full, name, &[]);
        assert!(!decision.allowed, "control plane unexpectedly allowed: {name}");
        assert_eq!(decision.deny_reason, Some(DenyReason::ControlPlane));
    }
    let decision = policy.decide(
        PermissionMode::Edit,
        "server_info",
        &[Capability::Workflow, Capability::ProcessExec],
    );
    assert!(!decision.allowed);
    assert_eq!(decision.deny_reason, Some(DenyReason::IndirectProcessExecInEdit));
}

#[test]
fn malformed_or_semantically_widened_policy_is_rejected() {
    assert!(CapabilityPolicy::from_toml("not = [valid").is_err());
    let widened = include_str!("../../../runtime-policy.toml")
        .replace("blocked_tools = [\"request_permissions\"]", "blocked_tools = []");
    assert!(CapabilityPolicy::from_toml(&widened).is_err());
    let unknown_allow = include_str!("../../../runtime-policy.toml")
        .replace("unknown = \"deny\"", "unknown = \"allow\"");
    assert!(CapabilityPolicy::from_toml(&unknown_allow).is_err());
}
