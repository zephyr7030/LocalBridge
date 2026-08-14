use localbridge_lib::mcp::{CapabilityPolicy, DenyReason, ToolCallRequest};
use localbridge_lib::state::{Capability, PermissionMode};
use serde_json::json;

fn policy() -> CapabilityPolicy {
    CapabilityPolicy::from_toml(include_str!("../../../runtime-policy.toml")).unwrap()
}

#[test]
fn policy_file_is_exact_and_unknown_fails_closed() {
    let policy = policy();
    for mode in [
        PermissionMode::Edit,
        PermissionMode::Full,
        PermissionMode::Elevated,
    ] {
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
        assert_eq!(
            policy.classify(name).capability,
            capability,
            "capability drift: {name}"
        );
    }
    assert!(
        policy
            .decide(PermissionMode::Edit, "read_file", &[])
            .allowed
    );
    assert!(
        policy
            .decide(PermissionMode::Edit, "apply_patch", &[])
            .allowed
    );
    assert!(
        !policy
            .decide(PermissionMode::Edit, "exec_command", &[])
            .allowed
    );
    assert!(
        policy
            .decide(PermissionMode::Full, "exec_command", &[])
            .allowed
    );
    assert!(
        policy
            .decide(PermissionMode::Elevated, "exec_command", &[])
            .allowed
    );
    for mode in [
        PermissionMode::Edit,
        PermissionMode::Full,
        PermissionMode::Elevated,
    ] {
        let decision = policy.decide(mode, "request_permissions", &[]);
        assert!(!decision.allowed);
        assert_eq!(decision.deny_reason, Some(DenyReason::ControlPlane));
    }
}

#[test]
fn localbridge_control_plane_and_indirect_workflow_exec_are_denied() {
    let policy = policy();
    for name in [
        "workspace_select",
        "workspace_add",
        "workspace_remove",
        "permission_mode_change",
        "credential_reset",
        "tunnel_config_write",
        "mcp_config_write",
        "localbridge.workspace.select",
    ] {
        let decision = policy.decide(PermissionMode::Full, name, &[]);
        assert!(
            !decision.allowed,
            "control plane unexpectedly allowed: {name}"
        );
        assert_eq!(decision.deny_reason, Some(DenyReason::ControlPlane));
    }
    let decision = policy.decide(
        PermissionMode::Edit,
        "server_info",
        &[Capability::Workflow, Capability::ProcessExec],
    );
    assert!(!decision.allowed);
    assert_eq!(
        decision.deny_reason,
        Some(DenyReason::IndirectProcessExecInEdit)
    );
}

#[test]
fn malformed_or_semantically_widened_policy_is_rejected() {
    assert!(CapabilityPolicy::from_toml("not = [valid").is_err());
    let widened = include_str!("../../../runtime-policy.toml").replace(
        "blocked_tools = [\"request_permissions\"]",
        "blocked_tools = []",
    );
    assert!(CapabilityPolicy::from_toml(&widened).is_err());
    let unknown_allow = include_str!("../../../runtime-policy.toml")
        .replace("unknown = \"deny\"", "unknown = \"allow\"");
    assert!(CapabilityPolicy::from_toml(&unknown_allow).is_err());
    let arbitrary_elevated = include_str!("../../../runtime-policy.toml").replace(
        "arbitrary_programs = \"deny\"",
        "arbitrary_programs = \"allow\"",
    );
    assert!(CapabilityPolicy::from_toml(&arbitrary_elevated).is_err());
}

#[test]
fn stable_public_classifier_declares_and_enforces_transitive_capabilities() {
    let policy = policy();
    let workflow = policy
        .classify_public_action(
            "agent_workflow",
            &json!({"action":"bugfix","objective":"repair local tests"}),
        )
        .expect("stable workflow action");
    assert_eq!(workflow.tool, "agent_workflow");
    assert_eq!(workflow.action, "bugfix");
    assert_eq!(workflow.descriptor.name, "agent_workflow");
    assert_eq!(workflow.descriptor.capability, Capability::Workflow);
    assert!(workflow.transitive.read);
    assert!(workflow.transitive.write);
    assert!(workflow.transitive.process_exec);
    assert!(workflow.transitive.git);
    assert!(!workflow.transitive.network);
    assert!(!workflow.transitive.privilege);

    let edit = policy.decide_public(
        PermissionMode::Edit,
        "agent_workflow",
        &json!({"action":"bugfix","objective":"repair local tests"}),
    );
    assert!(!edit.allowed);
    assert_eq!(
        edit.deny_reason,
        Some(DenyReason::IndirectProcessExecInEdit)
    );

    let network = policy.decide_public(
        PermissionMode::Full,
        "agent_workflow",
        &json!({"action":"build_release","objective":"release"}),
    );
    assert!(!network.allowed);
    assert_eq!(
        network.deny_reason,
        Some(DenyReason::NetworkRouteNotAvailable)
    );

    let privilege = policy.decide_public(
        PermissionMode::Full,
        "agent_workflow",
        &json!({"action":"custom","objective":"unspecified"}),
    );
    assert!(!privilege.allowed);
    assert_eq!(
        privilege.deny_reason,
        Some(DenyReason::PrivilegedRouteNotAvailable)
    );

    let external = policy.decide_public(
        PermissionMode::Full,
        "exec_command",
        &json!({"command":"docker version"}),
    );
    assert!(!external.allowed);
    assert_eq!(
        external.deny_reason,
        Some(DenyReason::PrivilegedRouteNotAvailable)
    );
}

#[test]
fn unknown_public_actions_and_public_policy_widening_fail_closed() {
    let policy = policy();
    let unknown = policy.decide_public(
        PermissionMode::Full,
        "git_workflow",
        &json!({"action":"future_private_action"}),
    );
    assert!(!unknown.allowed);
    assert_eq!(unknown.deny_reason, Some(DenyReason::UnknownTool));
    assert_eq!(unknown.descriptor.capability, Capability::Unknown);

    let base = include_str!("../../../runtime-policy.toml");
    let narrowed = base
        .replace(
            "full_tools = [\"workspace_context\", \"agent_workflow\", \"exec_command\", \"command_control\", \"task_control\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
            "full_tools = [\"workspace_context\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
        )
        .replace(
            "elevated_tools = [\"workspace_context\", \"agent_workflow\", \"exec_command\", \"command_control\", \"task_control\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
            "elevated_tools = [\"workspace_context\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
        );
    let narrowed = CapabilityPolicy::from_toml(&narrowed).expect("stricter policy remains valid");
    assert!(!narrowed.public_tool_allowed_for_list(PermissionMode::Full, "exec_command"));
    assert!(
        !narrowed
            .decide_public(
                PermissionMode::Full,
                "exec_command",
                &json!({"command":"echo denied"})
            )
            .allowed
    );

    let widened = base.replace(
        "edit_tools = [\"workspace_context\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
        "edit_tools = [\"workspace_context\", \"exec_command\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
    );
    assert!(CapabilityPolicy::from_toml(&widened).is_err());
    let unknown_tool = base.replace(
        "full_tools = [\"workspace_context\", \"agent_workflow\", \"exec_command\", \"command_control\", \"task_control\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
        "full_tools = [\"workspace_context\", \"future_private_tool\"]",
    );
    assert!(CapabilityPolicy::from_toml(&unknown_tool).is_err());
}

#[test]
fn elevated_exec_review_consumes_real_program_args_and_workdir() {
    let policy = policy();
    let program = localbridge_lib::mcp::reviewed_elevated_program()
        .expect("Windows reviewed elevated diagnostic must exist");
    let allowed = ToolCallRequest::new(
        "elevated_exec",
        json!({
            "program": program.to_string_lossy(),
            "args": ["/user"],
            "workdir": null,
            "timeout_ms": 1000,
            "max_output_bytes": 4096
        }),
    );
    assert!(
        policy
            .decide_request(
                PermissionMode::Elevated,
                &allowed.name,
                &[],
                &allowed.arguments,
            )
            .allowed
    );

    for arguments in [
        json!({"program":"C:/Windows/System32/cmd.exe","args":["/c","whoami"],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
        json!({"program":"C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe","args":["-Command","whoami"],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
        json!({"program":"C:/Windows/System32/reg.exe","args":["add","HKLM\\Software\\LocalBridge"],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
        json!({"program":program.to_string_lossy(),"args":["/user"],"workdir":"C:/Windows/Temp","timeout_ms":1000,"max_output_bytes":4096}),
        json!({"program":program.to_string_lossy(),"args":["/user","extra"],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096}),
    ] {
        let decision =
            policy.decide_request(PermissionMode::Elevated, "elevated_exec", &[], &arguments);
        assert!(!decision.allowed);
        assert_eq!(
            decision.deny_reason,
            Some(DenyReason::ElevatedExecNotReviewed)
        );
    }
}
