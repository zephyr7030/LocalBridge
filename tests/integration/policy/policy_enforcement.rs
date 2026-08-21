use std::cell::RefCell;
use std::rc::Rc;

use localbridge_lib::mcp::{
    CodingToolsRuntimeError, GuardError, GuardRuntime, McpGuard, ToolCallRequest,
};
use localbridge_lib::execution::{CapabilityPolicy, DenyReason};
use localbridge_lib::state::{
    Capability, CurrentTask, CurrentTaskStatus, PermissionMode, SafeTaskSummary, TaskExecutionState,
};
use serde_json::{Value, json};

const ALL_TOOLS: &[&str] = &[
    "server_info","check_exec_environment","get_default_cwd","set_default_cwd","read_file","list_dir","list_files","search_text","apply_patch","exec_command","write_stdin","kill_session","read_output","git_status","git_diff","git_log","git_show","git_blame","request_permissions","view_image","future_unknown_tool",
];

#[derive(Debug)]
struct FakeRuntime {
    calls: Rc<RefCell<Vec<String>>>,
    fail_calls: bool,
}

impl FakeRuntime {
    fn new(calls: Rc<RefCell<Vec<String>>>) -> Self { Self { calls, fail_calls: false } }
    fn failing(calls: Rc<RefCell<Vec<String>>>) -> Self { Self { calls, fail_calls: true } }
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
        if self.fail_calls { Err(CodingToolsRuntimeError::ProtocolMismatch) }
        else { Ok(json!({"ok":true,"called":name})) }
    }
}

fn policy() -> CapabilityPolicy {
    CapabilityPolicy::from_toml(include_str!("../../../runtime-policy.toml")).unwrap()
}

fn active_state(status: &CurrentTaskStatus) -> Option<&CurrentTask> {
    match status { CurrentTaskStatus::Active(task) => Some(task), CurrentTaskStatus::Idle => None }
}

#[test]
fn tools_list_is_ux_only_and_cached_full_list_cannot_bypass_edit_call_policy() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut guard = McpGuard::new(FakeRuntime::new(calls.clone()), policy());
    let full = guard.filtered_tools(PermissionMode::Full).unwrap();
    let full_names = full["tools"].as_array().unwrap().iter().filter_map(|v| v["name"].as_str()).collect::<Vec<_>>();
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
    assert!(matches!(active_state(&states[0]), Some(task) if task.state == TaskExecutionState::Blocked));
    assert_eq!(states.last(), Some(&CurrentTaskStatus::Idle));
}

#[test]
fn workflow_indirect_exec_and_control_plane_are_blocked_before_upstream() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut guard = McpGuard::new(FakeRuntime::new(calls.clone()), policy());
    for request in [
        ToolCallRequest::new("server_info", json!({})).with_indirect_capabilities([Capability::Workflow, Capability::ProcessExec]),
        ToolCallRequest::new("request_permissions", json!({"permission":"network"})),
        ToolCallRequest::new("workspace_select", json!({"path":"D:/other"})),
        ToolCallRequest::new("future_unknown_tool", json!({})),
    ] {
        let mut states = Vec::new();
        assert!(matches!(guard.call_tool(PermissionMode::Edit, request, |s| states.push(s)), Err(GuardError::Denied(_))));
        assert!(states.iter().all(|s| !matches!(active_state(s), Some(task) if task.state == TaskExecutionState::Running)));
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
        ToolCallRequest::new("exec_command", json!({"cmd":"echo safe","cwd":r"\\?\C:\project"})),
        ToolCallRequest::new("exec_command", json!({"cmd":r"type \\?\C:\project\probe.txt"})),
        ToolCallRequest::new("exec_command", json!({"cmd":"powershell -Command -","stdin":r"Get-Item \\?\C:\project"})),
        ToolCallRequest::new("exec_command", json!({"cmd":"echo %LB_PATH%","env":{"LB_PATH":r"\\?\C:\project"}})),
        ToolCallRequest::new("write_stdin", json!({"session_id":"synthetic","chars":r"cd \\?\C:\project\n"})),
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
    assert!(allowed.is_ok(), "patch content is data, not an execution-path argument");
    assert_eq!(&*calls.borrow(), &["apply_patch"]);
}

#[test]
fn privileged_external_runtime_commands_require_review_and_never_forward() {
    for mode in [PermissionMode::Full, PermissionMode::Elevated] {
        for (command, shell) in [
            ("docker ps", "auto"),
            ("podman.exe run image", "auto"),
            ("powershell -Command wsl.exe --status", "auto"),
            ("$x=('do'+'cker'); & $x ps", "windows_powershell"),
            ("$x='docker'; Start-Process $x", "powershell"),
            ("Set-Alias d docker; d ps", "pwsh"),
            ("sal d docker; d ps", "pwsh"),
            ("nal d docker; d ps", "pwsh"),
            ("$x='docker'; Set-Item Alias:lbgen12 $x; lbgen12 ps", "windows_powershell"),
            ("$x='docker'; si Alias:lbgen12 $x; lbgen12 ps", "powershell"),
            ("$Alias:lbgen12='docker'; lbgen12 ps", "windows_powershell"),
            ("$ExecutionContext.InvokeCommand.CommandNotFoundAction = { param($name,$eventArgs); $eventArgs.Command = Get-Command Write-Output }; lbgen13 'hook'", "windows_powershell"),
            ("$ExecutionContext.InvokeCommand.InvokeScript('Write-Output should-not-run')", "windows_powershell"),
            ("$value='abc'; $value.Trim()", "windows_powershell"),
            ("filter lbgen14 { Write-Output ok }; lbgen14", "windows_powershell"),
            ("workflow lbgen14 { Write-Output ok }; lbgen14", "windows_powershell"),
            ("configuration lbgen14 { Node localhost {} }", "windows_powershell"),
            ("$p='probe.ps1'; $sb=(Get-Command $p).ScriptBlock; 1 | ForEach-Object -Process $sb", "windows_powershell"),
            ("$p='probe.ps1'; $sb=(gcm $p).ScriptBlock; 1 | ForEach-Object -Process $sb", "windows_powershell"),
            ("$PSModuleAutoLoadingPreference='All'", "windows_powershell"),
            ("$n='PSModuleAutoLoadingPreference'; Set-Variable -Name $n -Value All", "windows_powershell"),
            ("#requires -Modules FutureModule\nWrite-Output ok", "windows_powershell"),
            ("New-Item Function:lbgen12 -Value { Write-Output ok }; lbgen12", "windows_powershell"),
            ("sc Function:lbgen12 -Value 'Write-Output ok'; lbgen12", "powershell"),
            ("Set-Item Env:LB_GEN12 harmless", "windows_powershell"),
            ("set x=docker & %x% ps", "cmd"),
            ("$x='C:\\Program Files\\Docker\\Docker\\resources\\bin\\docker.exe'; $p=[System.Diagnostics.Process]::Start($x,'ps'); $p.WaitForExit()", "windows_powershell"),
            ("$t=[type]::GetType('System.Diagnostics.Process'); $t::Start($x,'ps')", "powershell"),
            ("$sh=New-Object -ComObject Shell.Application; $sh.ShellExecute($x)", "powershell"),
            ("$w=[wmiclass]'Win32_Process'; $w.Create($x)", "windows_powershell"),
            ("$x='C:\\Program Files\\Docker\\Docker\\resources\\bin\\docker.exe'; Write-Output \"$([System.Diagnostics.Process]::Start($x,'ps'))\"", "windows_powershell"),
            ("Write-Output \"value=$(1+1)\"", "powershell"),
            ("$x='C:\\tools\\runtime.exe'; start $x", "windows_powershell"),
            ("$x='C:\\tools\\runtime.exe'; ii $x", "powershell"),
            ("Sta`rt-Process $x", "powershell"),
        ] {
            let calls = Rc::new(RefCell::new(Vec::new()));
            let mut guard = McpGuard::new(FakeRuntime::new(calls.clone()), policy());
            let request = ToolCallRequest::new(
                "exec_command",
                json!({"cmd":command,"shell":shell}),
            );
            assert!(!guard.decision(mode, &request).allowed);
            let mut states = Vec::new();
            let result = guard.call_tool(mode, request, |status| states.push(status));
            assert!(matches!(result, Err(GuardError::Denied(_))));
            assert!(calls.borrow().is_empty(), "external runtime forwarded: {command}");
            assert!(matches!(
                active_state(&states[0]),
                Some(task) if task.state == TaskExecutionState::Blocked
            ));
        }
    }
}

#[test]
fn public_shell_diagnostics_allow_only_narrow_static_console_and_command_discovery_seams() {
    let policy = policy();
    for mode in [PermissionMode::Full, PermissionMode::Elevated] {
        for arguments in [
            json!({"command":"Get-Command git","shell":"windows_powershell"}),
            json!({"command":"gcm git.exe","shell":"powershell"}),
            json!({"command":"[Console]::In.ReadToEnd()","shell":"windows_powershell","stdin":"probe\n"}),
        ] {
            let decision = policy.decide_public(mode, "exec_command", &arguments);
            assert!(decision.allowed, "ordinary diagnostic was over-classified: {arguments}");
        }
    }

    for arguments in [
        json!({"command":"Get-Command $dynamicName","shell":"windows_powershell"}),
        json!({"command":"Get-Command\r\ngit","shell":"windows_powershell"}),
        json!({"command":"Get-Command git | ForEach-Object { $_.ScriptBlock }","shell":"windows_powershell"}),
        json!({"command":"$p='probe.ps1'; $sb=(Get-Command $p).ScriptBlock; 1 | ForEach-Object -Process $sb","shell":"windows_powershell"}),
        json!({"command":"$ExecutionContext.InvokeCommand.CommandNotFoundAction = { param($name,$eventArgs); $eventArgs.Command = Get-Command Write-Output }; lbgen13 'hook'","shell":"windows_powershell"}),
        json!({"command":"$value='abc'; $value.Trim()","shell":"windows_powershell"}),
    ] {
        let decision = policy.decide_public(PermissionMode::Full, "exec_command", &arguments);
        assert!(!decision.allowed, "dynamic PowerShell surface escaped review: {arguments}");
    }
}

#[test]
fn full_ordinary_workflow_scripts_and_file_cleanup_do_not_require_privileged_route() {
    let policy = policy();
    for arguments in [
        json!({"command":"call test\\lb_broad_tmp.cmd","shell":"cmd"}),
        json!({"command":"del /q test\\lb_broad_tmp.cmd","shell":"cmd"}),
        json!({"command":"rmdir /s /q test\\lb_broad_tmp_dir","shell":"cmd"}),
        json!({"command":"python -c \"import os; os.remove(r'test\\lb_broad_tmp.cmd')\"","shell":"cmd"}),
    ] {
        let decision = policy.decide_public(PermissionMode::Full, "exec_command", &arguments);
        assert!(decision.allowed, "ordinary Full operation was over-classified: {arguments}");
    }

    let powershell_alias = policy.decide_public(
        PermissionMode::Full,
        "exec_command",
        &json!({"command":"rmdir test\\lb_broad_tmp_dir","shell":"windows_powershell"}),
    );
    assert!(
        !powershell_alias.allowed,
        "PowerShell rmdir alias must remain review-required rather than inheriting cmd cleanup semantics"
    );

    let ordinary_workflow = json!({
        "action":"custom",
        "commands":[{"command":"echo WORKFLOW_CMD_OK","shell":"cmd","workdir":"."}]
    });
    assert!(
        policy
            .decide_public(PermissionMode::Full, "agent_workflow", &ordinary_workflow)
            .allowed,
        "ordinary custom workflow was over-classified"
    );
    assert!(
        !policy
            .decide_public(PermissionMode::Edit, "agent_workflow", &ordinary_workflow)
            .allowed,
        "Edit workflow process execution escaped the process boundary"
    );

    for arguments in [
        json!({"command":"call %SCRIPT%","shell":"cmd"}),
        json!({"command":"Remove-Item HKLM:\\SOFTWARE\\probe","shell":"windows_powershell"}),
    ] {
        assert!(
            !policy
                .decide_public(PermissionMode::Full, "exec_command", &arguments)
                .allowed,
            "dynamic/provider mutation unexpectedly escaped review: {arguments}"
        );
    }
}

#[test]
fn ordinary_system_management_targets_require_the_privileged_route_in_full_and_elevated() {
    let policy = policy();
    for mode in [PermissionMode::Full, PermissionMode::Elevated] {
        for arguments in [
            json!({"command":"reg.exe query HKLM\\SOFTWARE","shell":"cmd"}),
            json!({"command":"SCHTASKS.EXE /Query","shell":"cmd"}),
            json!({"command":"sc.exe query","shell":"cmd"}),
            json!({"command":"netsh.exe interface show interface","shell":"cmd"}),
            json!({"command":"C:\\Windows\\System32\\REG.EXE query HKLM\\SOFTWARE","shell":"cmd"}),
            json!({"command":"\"C:\\Windows\\System32\\reg.exe\" query HKLM\\SOFTWARE","shell":"cmd"}),
            json!({"command":"reg query HKLM\\SOFTWARE","shell":"cmd"}),
            json!({"command":"echo before && netsh.exe interface show interface","shell":"cmd"}),
            json!({"command":"if 1==1 reg.exe query HKLM\\SOFTWARE","shell":"cmd"}),
            json!({"command":"echo before & if 1==1 reg.exe query HKLM\\SOFTWARE","shell":"cmd"}),
            json!({"command":"if exist C:\\Windows schtasks.exe /Query","shell":"cmd"}),
            json!({"command":"reg.exe query HKLM:\\SOFTWARE","shell":"windows_powershell"}),
            json!({"command":"C:\\Windows\\System32\\netsh.exe interface show interface","shell":"powershell"}),
            json!({"command":"Write-Output before; schtasks.exe /Query","shell":"windows_powershell"}),
        ] {
            let decision = policy.decide_public(mode, "exec_command", &arguments);
            assert!(
                !decision.allowed,
                "system-management target escaped privileged route: {arguments}"
            );
            assert_eq!(
                decision.deny_reason,
                Some(DenyReason::PrivilegedRouteNotAvailable),
                "system-management target used wrong denial: {arguments}"
            );
        }

        for arguments in [
            json!({"command":"echo reg.exe","shell":"cmd"}),
            json!({"command":"if exist reg.exe echo filename-only","shell":"cmd"}),
            json!({"command":"if 1==1 echo reg.exe","shell":"cmd"}),
            json!({"command":"Write-Output reg.exe","shell":"windows_powershell"}),
            json!({"command":"Get-Command reg.exe","shell":"windows_powershell"}),
        ] {
            assert!(
                policy
                    .decide_public(mode, "exec_command", &arguments)
                    .allowed,
                "system-management program name used as data was globally banned: {arguments}"
            );
        }
    }
}

#[test]
fn schema33_bcdedit_and_dism_require_privileged_route_in_direct_and_workflow_paths() {
    let policy = policy();
    for mode in [PermissionMode::Full, PermissionMode::Elevated] {
        for arguments in [
            json!({"command":"bcdedit.exe /enum","shell":"cmd"}),
            json!({"command":"DISM.EXE /Online /Get-Features","shell":"cmd"}),
            json!({"command":"C:\\Windows\\System32\\bcdedit.exe /enum","shell":"cmd"}),
            json!({"command":"\"C:\\Windows\\System32\\dism.exe\" /Online /Get-Features","shell":"cmd"}),
            json!({"command":"bcdedit /enum","shell":"cmd"}),
            json!({"command":"dism /Online /Get-Features","shell":"cmd"}),
            json!({"command":"echo before && dism.exe /Online /Get-Features","shell":"cmd"}),
            json!({"command":"if 1==1 bcdedit.exe /enum","shell":"cmd"}),
            json!({"command":"bcdedit.exe /enum","shell":"windows_powershell"}),
            json!({"command":"C:\\Windows\\System32\\dism.exe /Online /Get-Features","shell":"powershell"}),
        ] {
            let decision = policy.decide_public(mode, "exec_command", &arguments);
            assert!(!decision.allowed, "schema33 system-management target escaped: {arguments}");
            assert_eq!(decision.deny_reason, Some(DenyReason::PrivilegedRouteNotAvailable));
        }

        for command in ["bcdedit.exe /enum", "dism.exe /Online /Get-Features"] {
            let decision = policy.decide_public(
                mode,
                "agent_workflow",
                &json!({"action":"diagnose","commands":[{"command":command,"shell":"cmd"}]}),
            );
            assert!(!decision.allowed, "workflow escaped privileged route: {command}");
            assert_eq!(decision.deny_reason, Some(DenyReason::PrivilegedRouteNotAvailable));
        }

        for arguments in [
            json!({"command":"echo dism.exe","shell":"cmd"}),
            json!({"command":"Write-Output bcdedit.exe","shell":"windows_powershell"}),
            json!({"command":"Get-Command dism.exe","shell":"windows_powershell"}),
        ] {
            assert!(policy.decide_public(mode, "exec_command", &arguments).allowed);
        }
    }
}

#[test]
fn allowed_call_projects_actual_running_then_idle_and_redacts_secret_summary() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut guard = McpGuard::new(FakeRuntime::new(calls.clone()), policy());
    let mut states = Vec::new();
    let result = guard.call_tool(
        PermissionMode::Full,
        ToolCallRequest::new("exec_command", json!({"cmd":"tool --api-key=synthetic-secret"})),
        |status| states.push(status),
    ).unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(&*calls.borrow(), &["exec_command"]);
    assert!(matches!(active_state(&states[0]), Some(CurrentTask { state: TaskExecutionState::Running, summary: SafeTaskSummary::Omitted, .. })));
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
    assert!(matches!(active_state(&states[0]), Some(task) if task.state == TaskExecutionState::Running));
    assert!(matches!(active_state(&states[1]), Some(task) if task.state == TaskExecutionState::Failed));
    assert_eq!(states[2], CurrentTaskStatus::Idle);
}

#[test]
fn schema42_shell_specific_ordinary_commands_forward_but_system_management_does_not() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut guard = McpGuard::new(FakeRuntime::new(calls.clone()), policy());
    for (command, shell) in [
        ("set X=value", "cmd"),
        ("copy test\\a.txt test\\b.txt", "cmd"),
        ("move test\\a.txt test\\b.txt", "cmd"),
        ("ren test\\a.txt b.txt", "cmd"),
        ("cmd /c echo nested-ok", "cmd"),
        ("Set-Content test\\probe.txt ok", "windows_powershell"),
        ("New-Item test\\probe.txt", "windows_powershell"),
        ("[System.Security.Principal.WindowsIdentity]::GetCurrent()", "windows_powershell"),
    ] {
        let result = guard.call_tool(
            PermissionMode::Full,
            ToolCallRequest::new("exec_command", json!({"cmd":command,"shell":shell})),
            |_| {},
        );
        assert!(result.is_ok(), "ordinary command did not forward: {command}");
    }
    let forwarded = calls.borrow().len();
    for (command, shell) in [
        ("net.exe user", "cmd"),
        ("fsutil.exe fsinfo drives", "cmd"),
        ("cmd /c sc.exe query", "cmd"),
        ("New-Item Function:lb42 -Value { Write-Output ok }", "windows_powershell"),
    ] {
        let result = guard.call_tool(
            PermissionMode::Full,
            ToolCallRequest::new("exec_command", json!({"cmd":command,"shell":shell})),
            |_| {},
        );
        assert!(
            matches!(result, Err(GuardError::Denied(_))),
            "review-required command forwarded: {command}"
        );
    }
    assert_eq!(calls.borrow().len(), forwarded);
}
