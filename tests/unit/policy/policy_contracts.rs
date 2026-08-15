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
    let no_shell_review = include_str!("../../../runtime-policy.toml").replace(
        "unreviewable_shell_indirection = \"review_required\"",
        "unreviewable_shell_indirection = \"allow\"",
    );
    assert!(CapabilityPolicy::from_toml(&no_shell_review).is_err());
}

#[test]
fn stable_public_classifier_declares_and_enforces_transitive_capabilities() {
    let policy = policy();
    let diagnose = json!({"action":"diagnose"});
    let diagnose_descriptor = policy
        .classify_public_action("agent_workflow", &diagnose)
        .expect("minimal diagnose action");
    assert!(!diagnose_descriptor.transitive.process_exec);
    assert!(diagnose_descriptor.transitive.git);
    assert!(
        policy
            .decide_public(PermissionMode::Edit, "agent_workflow", &diagnose)
            .allowed
    );

    let diagnose_with_command = json!({
        "action":"diagnose",
        "commands":[{"command":"Write-Output ok","shell":"windows_powershell"}]
    });
    let edit_diagnose_with_command = policy.decide_public(
        PermissionMode::Edit,
        "agent_workflow",
        &diagnose_with_command,
    );
    assert!(!edit_diagnose_with_command.allowed);
    assert_eq!(
        edit_diagnose_with_command.deny_reason,
        Some(DenyReason::IndirectProcessExecInEdit)
    );
    assert!(
        policy
            .decide_public(
                PermissionMode::Full,
                "agent_workflow",
                &diagnose_with_command,
            )
            .allowed
    );
    assert!(
        !policy
            .decide_public(
                PermissionMode::Edit,
                "agent_workflow",
                &json!({"action":"diagnose","commands":"invalid"}),
            )
            .allowed
    );

    let directory_only = json!({
        "action":"document",
        "directory_changes":[
            {"action":"create_directory","path":"test"},
            {"action":"remove_empty_directory","path":"test"}
        ]
    });
    let directory_descriptor = policy
        .classify_public_action("agent_workflow", &directory_only)
        .expect("schema30 structured directory workflow");
    assert!(directory_descriptor.transitive.read);
    assert!(directory_descriptor.transitive.write);
    assert!(directory_descriptor.transitive.git);
    assert!(!directory_descriptor.transitive.process_exec);
    assert!(!directory_descriptor.transitive.network);
    assert!(!directory_descriptor.transitive.privilege);
    assert!(!directory_descriptor.transitive.control_plane);
    assert!(
        policy
            .decide_public(PermissionMode::Edit, "agent_workflow", &directory_only)
            .allowed
    );
    assert!(
        policy
            .decide_public(PermissionMode::Full, "agent_workflow", &directory_only)
            .allowed
    );

    for malformed in [
        json!({"action":"document","directory_changes":[]}),
        json!({"action":"document","directory_changes":[{"action":"create_directory","path":"test","extra":true}]}),
        json!({"action":"document","directory_changes":[{"action":"recursive_delete","path":"test"}]}),
        json!({"action":"document","directory_changes":[{"action":"create_directory","path":"test"}],"commands":[{"command":"echo process"}]}),
    ] {
        let decision = policy.decide_public(PermissionMode::Edit, "agent_workflow", &malformed);
        assert!(
            !decision.allowed,
            "malformed/mixed request widened Edit: {malformed:#?}"
        );
    }

    for protected_action in ["build_release", "custom"] {
        let decision = policy.decide_public(
            PermissionMode::Full,
            "agent_workflow",
            &json!({
                "action":protected_action,
                "directory_changes":[{"action":"create_directory","path":"test"}]
            }),
        );
        assert!(
            !decision.allowed,
            "directory_changes removed protected capability for {protected_action}"
        );
    }

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

    for arguments in [
        json!({"command":"$x=('do'+'cker'); & $x ps","shell":"windows_powershell"}),
        json!({"command":"$x='docker'; Start-Process $x","shell":"powershell"}),
        json!({"command":"Set-Alias d docker; d ps","shell":"pwsh"}),
        json!({"command":"sal d docker; d ps","shell":"pwsh"}),
        json!({"command":"nal d docker; d ps","shell":"pwsh"}),
        json!({"command":"$x='docker'; Set-Item Alias:lbgen12 $x; lbgen12 ps","shell":"windows_powershell"}),
        json!({"command":"$x='docker'; si Alias:lbgen12 $x; lbgen12 ps","shell":"powershell"}),
        json!({"command":"$Alias:lbgen12='docker'; lbgen12 ps","shell":"windows_powershell"}),
        json!({"command":"$ExecutionContext.InvokeCommand.CommandNotFoundAction = { param($name,$eventArgs); $eventArgs.Command = Get-Command Write-Output }; lbgen13 'hook'","shell":"windows_powershell"}),
        json!({"command":"$ExecutionContext.InvokeCommand.InvokeScript('Write-Output should-not-run')","shell":"windows_powershell"}),
        json!({"command":"$value='abc'; $value.Trim()","shell":"windows_powershell"}),
        json!({"command":"filter lbgen14 { Write-Output ok }; lbgen14","shell":"windows_powershell"}),
        json!({"command":"workflow lbgen14 { Write-Output ok }; lbgen14","shell":"windows_powershell"}),
        json!({"command":"configuration lbgen14 { Node localhost {} }","shell":"windows_powershell"}),
        json!({"command":"$p='probe.ps1'; $sb=(Get-Command $p).ScriptBlock; 1 | ForEach-Object -Process $sb","shell":"windows_powershell"}),
        json!({"command":"$p='probe.ps1'; $sb=(gcm $p).ScriptBlock; 1 | ForEach-Object -Process $sb","shell":"windows_powershell"}),
        json!({"command":"$PSModuleAutoLoadingPreference='All'","shell":"windows_powershell"}),
        json!({"command":"$n='PSModuleAutoLoadingPreference'; Set-Variable -Name $n -Value All","shell":"windows_powershell"}),
        json!({"command":"#requires -Modules FutureModule\nWrite-Output ok","shell":"windows_powershell"}),
        json!({"command":"New-Item Function:lbgen12 -Value { Write-Output ok }; lbgen12","shell":"windows_powershell"}),
        json!({"command":"sc Function:lbgen12 -Value 'Write-Output ok'; lbgen12","shell":"powershell"}),
        json!({"command":"Set-Item Env:LB_GEN12 harmless","shell":"windows_powershell"}),
        json!({"command":"cmd /c echo safe","shell":"windows_powershell"}),
        json!({"command":"set x=docker & %x% ps","shell":"cmd"}),
        json!({"command":"$x='C:\\Program Files\\Docker\\Docker\\resources\\bin\\docker.exe'; $p=[System.Diagnostics.Process]::Start($x,'ps'); $p.WaitForExit()","shell":"windows_powershell"}),
        json!({"command":"$t=[type]::GetType('System.Diagnostics.Process'); $t::Start($x,'ps')","shell":"powershell"}),
        json!({"command":"$sh=New-Object -ComObject Shell.Application; $sh.ShellExecute($x)","shell":"powershell"}),
        json!({"command":"$w=[wmiclass]'Win32_Process'; $w.Create($x)","shell":"windows_powershell"}),
        json!({"command":"$x='C:\\Program Files\\Docker\\Docker\\resources\\bin\\docker.exe'; Write-Output \"$([System.Diagnostics.Process]::Start($x,'ps'))\"","shell":"windows_powershell"}),
        json!({"command":"Write-Output \"value=$(1+1)\"","shell":"powershell"}),
        json!({"command":"$x='C:\\tools\\runtime.exe'; start $x","shell":"windows_powershell"}),
        json!({"command":"$x='C:\\tools\\runtime.exe'; ii $x","shell":"powershell"}),
        json!({"command":"Sta`rt-Process $x","shell":"powershell"}),
    ] {
        let decision = policy.decide_public(PermissionMode::Full, "exec_command", &arguments);
        assert!(
            !decision.allowed,
            "shell indirection unexpectedly allowed: {arguments}"
        );
        assert_eq!(
            decision.deny_reason,
            Some(DenyReason::PrivilegedRouteNotAvailable)
        );
    }

    for arguments in [
        json!({"command":"Write-Output \"a|b\"; Write-Output \"a&b\"; Write-Output 'docker is text'","shell":"windows_powershell"}),
        json!({"command":"Start-Sleep -Milliseconds 10; $line=[Console]::In.ReadLine(); Write-Output ('write:'+ $line)","shell":"auto"}),
        json!({"command":"$line=[System.Console]::ReadLine(); Write-Output $line","shell":"windows_powershell"}),
        json!({"command":"Write-Output '$(' ; Write-Output \"`$(`\"","shell":"windows_powershell"}),
        json!({"command":"Write-Output '$ExecutionContext is documentation text'","shell":"windows_powershell"}),
        json!({"command":"Write-Output 'filter workflow Get-Command .ScriptBlock probe.ps1 are documentation text'","shell":"windows_powershell"}),
    ] {
        assert!(
            policy
                .decide_public(PermissionMode::Full, "exec_command", &arguments)
                .allowed,
            "review rejected ordinary schema28 shell semantics: {arguments}"
        );
    }

    let workflow_indirection = policy.decide_public(
        PermissionMode::Full,
        "agent_workflow",
        &json!({
            "action":"bugfix",
            "commands":[{"command":"$x=('do'+'cker'); & $x ps","shell":"windows_powershell"}]
        }),
    );
    assert!(!workflow_indirection.allowed);
    assert_eq!(
        workflow_indirection.deny_reason,
        Some(DenyReason::PrivilegedRouteNotAvailable)
    );
}

#[test]
fn schema33_system_management_workflow_indirection_stays_broker_only() {
    let policy = policy();
    for mode in [PermissionMode::Full, PermissionMode::Elevated] {
        for command in ["bcdedit.exe /enum", "dism.exe /Online /Get-Features"] {
            let decision = policy.decide_public(
                mode,
                "agent_workflow",
                &json!({"action":"diagnose","commands":[{"command":command,"shell":"cmd"}]}),
            );
            assert!(!decision.allowed);
            assert_eq!(decision.deny_reason, Some(DenyReason::PrivilegedRouteNotAvailable));
        }
    }
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
            "edit_tools = [\"workspace_context\", \"agent_workflow\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
            "edit_tools = [\"workspace_context\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
        )
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
        "edit_tools = [\"workspace_context\", \"agent_workflow\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
        "edit_tools = [\"workspace_context\", \"agent_workflow\", \"exec_command\", \"git_workflow\", \"document_workflow\", \"view_image\"]",
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
