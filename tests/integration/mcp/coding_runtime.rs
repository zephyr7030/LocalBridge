#![cfg(windows)]

use std::fs;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use localbridge_lib::mcp::{
    CapabilityPolicy, CodingToolsPermissionMode, CodingToolsRuntime, CodingToolsRuntimeConfig,
    CodingToolsRuntimeError, DenyReason, GuardError, InternalBearer, McpGuard, ToolCallRequest,
};
use localbridge_lib::state::{CurrentTaskStatus, PermissionMode};
use serde_json::json;

const SYNTHETIC_BEARER: &str = "LB006_INTERNAL_BEARER_SYNTHETIC_DO_NOT_LEAK_9b7a4c11";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must have repository parent")
        .to_path_buf()
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "localbridge-lb006-{label}-{}-{nonce}",
        std::process::id()
    ))
}

fn create_workspace(label: &str) -> PathBuf {
    let path = unique_temp_dir(label);
    fs::create_dir_all(&path).expect("create integration workspace");
    fs::write(path.join("probe.txt"), b"LB006\n").expect("write probe file");
    path
}

fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind ephemeral loopback");
    listener.local_addr().expect("local addr").port()
}

fn config(install_root: &Path, workspace: &Path, port: u16) -> CodingToolsRuntimeConfig {
    CodingToolsRuntimeConfig::new(
        install_root,
        workspace,
        port,
        CodingToolsPermissionMode::Safe,
    )
}

fn os_command_line(pid: u32) -> String {
    let script = format!(
        "$p=Get-CimInstance Win32_Process -Filter \"ProcessId = {pid}\"; if(-not $p){{exit 4}}; [Console]::Out.Write($p.CommandLine)"
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .expect("query Win32 process command line");
    assert!(output.status.success(), "CIM command-line query failed");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn visible_descendant_windows(root_pid: u32) -> Vec<String> {
    let script = format!(
        "$all=Get-CimInstance Win32_Process; $ids=@({root_pid}); $out=@(); for($i=0;$i -lt 4;$i++){{ $next=@(); foreach($id in $ids){{ foreach($p in $all | Where-Object {{$_.ParentProcessId -eq $id}}){{ $gp=Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue; if($gp -and $gp.MainWindowHandle -ne 0){{ $out += ($p.ProcessId.ToString()+'|'+$p.Name+'|'+$gp.MainWindowHandle.ToString()) }}; $next += $p.ProcessId }} }}; $ids=$next }}; $out"
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .expect("query managed process visible windows");
    assert!(output.status.success(), "visible-window process query failed");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn actual_bundled_runtime_is_authenticated_loopback_owned_and_secret_redacted() {
    let root = repo_root();
    let workspace = create_workspace("runtime");
    let bearer = InternalBearer::new(SYNTHETIC_BEARER).expect("synthetic bearer");
    let mut runtime = CodingToolsRuntime::start(
        config(&root, &workspace, free_port()),
        bearer,
        Duration::from_secs(10),
    )
    .expect("bundled coding runtime must become ready");

    assert!(runtime.endpoint().starts_with("http://127.0.0.1:"));
    assert!(runtime.endpoint().ends_with("/mcp"));
    assert!(runtime.unauthenticated_initialize_is_rejected().unwrap());
    assert!(runtime.root_is_running().unwrap());
    assert!(
        runtime.active_processes().unwrap() >= 1,
        "supervised Job must own the coding runtime process tree"
    );
    let startup_windows = visible_descendant_windows(runtime.process_snapshot().pid);
    assert!(
        startup_windows.is_empty(),
        "coding runtime startup created visible console descendants: {startup_windows:?}"
    );

    let managed_command = runtime
        .call_tool(
            "exec_command",
            json!({
                "cmd":"cmd.exe /d /c ping -n 10 127.0.0.1 >nul",
                "timeout_ms":15000,
                "yield_time_ms":0,
                "max_output_bytes":4096
            }),
        )
        .expect("start managed command no-console probe");
    let managed_session = managed_command
        .get("structuredContent")
        .and_then(|value| value.get("session_id"))
        .and_then(|value| value.as_str())
        .expect("managed command session id")
        .to_string();
    std::thread::sleep(Duration::from_millis(250));
    let command_windows = visible_descendant_windows(runtime.process_snapshot().pid);
    assert!(
        command_windows.is_empty(),
        "managed command created visible console descendants: {command_windows:?}"
    );
    let killed = runtime
        .call_tool(
            "kill_session",
            json!({"session_id":managed_session,"signal":"KILL","wait_ms":1000,"max_output_bytes":4096}),
        )
        .expect("kill managed command no-console probe");
    assert_ne!(killed.get("isError").and_then(|value| value.as_bool()), Some(true));

    let tools = runtime.list_tools().expect("tools/list");
    let catalog = tools
        .get("tools")
        .and_then(|value| value.as_array())
        .expect("tools array");
    assert_eq!(catalog.len(), 20);
    assert!(
        catalog
            .iter()
            .any(|tool| tool.get("name").and_then(|v| v.as_str()) == Some("exec_command"))
    );

    let debug = format!("{runtime:?}");
    assert!(!debug.contains(SYNTHETIC_BEARER));

    let command_line = os_command_line(runtime.process_snapshot().pid);
    assert!(command_line.contains("python.exe"));
    assert!(command_line.contains("-m coding_tools_mcp"));
    assert!(command_line.contains("--host 127.0.0.1"));
    assert!(!command_line.contains(SYNTHETIC_BEARER));
    assert!(!command_line.contains("CODING_TOOLS_MCP_AUTH_TOKEN"));

    let child_env = runtime
        .call_tool(
            "exec_command",
            json!({
                "cmd": "cmd.exe /d /c set CODING_TOOLS_MCP_AUTH_TOKEN",
                "timeout_ms": 5000,
                "yield_time_ms": 5000,
                "max_output_bytes": 10000
            }),
        )
        .expect("exec_command call must return structured result");
    let structured = child_env
        .get("structuredContent")
        .and_then(|value| value.as_object())
        .expect("structuredContent");
    assert_eq!(
        structured.get("exit_code").and_then(|value| value.as_i64()),
        Some(1)
    );
    let rendered = serde_json::to_string(&child_env).unwrap();
    assert!(!rendered.contains(SYNTHETIC_BEARER));
    assert!(!rendered.contains("CODING_TOOLS_MCP_AUTH_TOKEN="));

    let stopped_port = runtime.port();
    runtime.stop().expect("Job-owned runtime stop");
    assert!(!runtime.root_is_running().unwrap());
    assert_eq!(runtime.active_processes().unwrap(), 0);
    let stopped_endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, stopped_port);
    assert!(
        TcpStream::connect_timeout(&stopped_endpoint.into(), Duration::from_millis(200)).is_err(),
        "MCP listener must not accept new connections after stop"
    );
    drop(runtime);
    fs::remove_dir_all(workspace).expect("cleanup workspace");
}

#[test]
fn missing_or_corrupt_bundle_fails_closed_without_system_python_fallback() {
    let workspace = create_workspace("missing");
    let missing_root = unique_temp_dir("missing-install");
    fs::create_dir_all(&missing_root).unwrap();
    let missing = CodingToolsRuntime::start(
        config(&missing_root, &workspace, free_port()),
        InternalBearer::new("LB006_MISSING_SENTINEL").unwrap(),
        Duration::from_millis(100),
    )
    .unwrap_err();
    assert!(matches!(
        missing,
        CodingToolsRuntimeError::RuntimeMissing(_)
    ));

    let corrupt_root = unique_temp_dir("corrupt-install");
    let python_dir = corrupt_root.join("runtime").join("python");
    let coding_dir = corrupt_root
        .join("runtime")
        .join("coding-tools-mcp")
        .join("coding_tools_mcp");
    fs::create_dir_all(&python_dir).unwrap();
    fs::create_dir_all(&coding_dir).unwrap();
    fs::write(python_dir.join("python.exe"), b"not-python").unwrap();
    fs::write(coding_dir.join("__init__.py"), b"__version__='0.2.2'\n").unwrap();
    let corrupt = CodingToolsRuntime::start(
        config(&corrupt_root, &workspace, free_port()),
        InternalBearer::new("LB006_CORRUPT_SENTINEL").unwrap(),
        Duration::from_millis(100),
    )
    .unwrap_err();
    assert!(matches!(
        corrupt,
        CodingToolsRuntimeError::RuntimeChecksumMismatch(_)
    ));

    fs::remove_dir_all(workspace).unwrap();
    fs::remove_dir_all(missing_root).unwrap();
    fs::remove_dir_all(corrupt_root).unwrap();
}

#[test]
fn bundled_python_is_isolated_offline_and_contains_no_runtime_pip() {
    let python = repo_root()
        .join("runtime")
        .join("python")
        .join("python.exe");
    let output = Command::new(&python)
        .args([
            "-I",
            "-B",
            "-c",
            "import importlib.util,sys,coding_tools_mcp,jwt; print(sys.version.split()[0]); print(coding_tools_mcp.__version__); print(jwt.__version__); print(sys.flags.isolated,sys.flags.no_user_site); print(importlib.util.find_spec('pip'))",
        ])
        .output()
        .expect("direct bundled Python probe");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.first(), Some(&"3.12.10"));
    assert_eq!(lines.get(1), Some(&"0.2.2"));
    assert_eq!(lines.get(2), Some(&"2.10.1"));
    assert_eq!(lines.get(3), Some(&"1 1"));
    assert_eq!(lines.get(4), Some(&"None"));
}

fn git_fixture(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git.exe")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("run git fixture command");
    assert!(
        output.status.success(),
        "git fixture command failed: git -C {} {}\n{}",
        repo.display(),
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn guarded_git_call(
    guard: &mut McpGuard<CodingToolsRuntime>,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let mut states = Vec::new();
    let result = guard
        .call_tool(
            PermissionMode::Edit,
            ToolCallRequest::new(name, arguments),
            |status| states.push(status),
        )
        .expect("Git tool must pass LocalBridge policy and return a tool result");
    assert!(
        matches!(states.first(), Some(CurrentTaskStatus::Active(_))),
        "real Git call must project Running before execution: {name}"
    );
    assert_eq!(states.last(), Some(&CurrentTaskStatus::Idle));
    result
}

fn cleanup_nested_git_workspace(path: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match fs::remove_dir_all(path) {
            Ok(()) => return,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => panic!("cleanup {}: {error}", path.display()),
        }
    }
}

#[test]
fn dedicated_git_tools_share_nested_repository_resolution_behind_policy() {
    let root = repo_root();
    let workspace = unique_temp_dir("nested-git-parent");
    let repository = workspace.join("LocalBridge");
    fs::create_dir_all(repository.join("src")).expect("create nested repository directories");
    fs::create_dir_all(workspace.join("non-git")).expect("create non-git directory");

    git_fixture(&repository, &["init"]);
    git_fixture(&repository, &["config", "user.name", "LocalBridge Test"]);
    git_fixture(
        &repository,
        &["config", "user.email", "localbridge-test@example.invalid"],
    );
    fs::write(repository.join("package.json"), b"{\"name\":\"before\"}\n").unwrap();
    fs::write(repository.join("src").join("probe.txt"), b"nested repo\n").unwrap();
    fs::write(repository.join("deleted.txt"), b"delete me\n").unwrap();
    git_fixture(&repository, &["add", "."]);
    git_fixture(&repository, &["commit", "-m", "nested initial"]);
    let head = git_fixture(&repository, &["rev-parse", "HEAD"]);

    fs::write(repository.join("package.json"), b"{\"name\":\"after\"}\n").unwrap();
    fs::remove_file(repository.join("deleted.txt")).unwrap();

    let runtime = CodingToolsRuntime::start(
        config(&root, &workspace, free_port()),
        InternalBearer::new("LB015_NESTED_GIT_BEARER_SYNTHETIC").unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled coding runtime must become ready for nested Git regression");
    let policy = CapabilityPolicy::load(&root.join("runtime-policy.toml")).unwrap();
    let mut guard = McpGuard::new(runtime, policy);

    let status = guarded_git_call(&mut guard, "git_status", json!({"path":"LocalBridge"}));
    assert_eq!(status["structuredContent"]["is_repo"], true);
    assert_eq!(status["structuredContent"]["head"], head);

    let log = guarded_git_call(&mut guard, "git_log", json!({"path":"LocalBridge"}));
    assert_eq!(log["structuredContent"]["is_repo"], true);
    assert!(
        log["structuredContent"]["commits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|commit| commit["hash"] == head)
    );

    let show = guarded_git_call(&mut guard, "git_show", json!({"path":"LocalBridge"}));
    assert_eq!(show["structuredContent"]["is_repo"], true);
    assert!(
        show["structuredContent"]["content"]
            .as_str()
            .unwrap()
            .contains("nested initial")
    );

    let diff = guarded_git_call(&mut guard, "git_diff", json!({"path":"LocalBridge"}));
    assert!(
        diff["structuredContent"]["diff"]
            .as_str()
            .unwrap()
            .contains("package.json")
    );
    assert!(
        !serde_json::to_string(&diff["structuredContent"]["warnings"])
            .unwrap()
            .contains("non-git diff fallback")
    );

    let blame = guarded_git_call(
        &mut guard,
        "git_blame",
        json!({"path":"LocalBridge/package.json"}),
    );
    assert_eq!(blame["structuredContent"]["is_repo"], true);
    assert!(
        !blame["structuredContent"]["lines"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    for path in ["LocalBridge\\src", "LocalBridge\\package.json"] {
        let nested = guarded_git_call(&mut guard, "git_status", json!({"path":path}));
        assert_eq!(nested["structuredContent"]["is_repo"], true, "path={path}");
        assert_eq!(nested["structuredContent"]["head"], head, "path={path}");
    }

    let parent = guarded_git_call(&mut guard, "git_status", json!({"path":"."}));
    assert_eq!(parent["structuredContent"]["is_repo"], false);
    let non_git = guarded_git_call(&mut guard, "git_status", json!({"path":"non-git"}));
    assert_eq!(non_git["structuredContent"]["is_repo"], false);

    let missing = guarded_git_call(&mut guard, "git_status", json!({"path":"missing"}));
    assert_eq!(missing["isError"], true);
    assert_eq!(missing["structuredContent"]["ok"], false);
    assert_eq!(missing["structuredContent"]["error"]["code"], "NOT_FOUND");

    let deleted = guarded_git_call(
        &mut guard,
        "git_diff",
        json!({"path":"LocalBridge","paths":["LocalBridge/deleted.txt"]}),
    );
    assert!(
        deleted["structuredContent"]["diff"]
            .as_str()
            .unwrap()
            .contains("deleted.txt")
    );
    assert!(
        !serde_json::to_string(&deleted["structuredContent"]["warnings"])
            .unwrap()
            .contains("non-git diff fallback")
    );

    drop(guard);
    cleanup_nested_git_workspace(&workspace);
}

#[test]
fn verbatim_execution_paths_are_denied_before_real_upstream_runtime() {
    let root = repo_root();
    let workspace = create_workspace("verbatim-boundary");
    let runtime = CodingToolsRuntime::start(
        config(&root, &workspace, free_port()),
        InternalBearer::new("LB015_VERBATIM_BOUNDARY_SYNTHETIC").unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled coding runtime must become ready");
    let policy = CapabilityPolicy::load(&root.join("runtime-policy.toml")).unwrap();
    let mut guard = McpGuard::new(runtime, policy);
    let verbatim_workspace = format!(r"\\?\{}", workspace.display());
    let sentinel = workspace.join("must-not-exist.txt");

    for request in [
        ToolCallRequest::new(
            "read_file",
            json!({"path":format!(r"{}\probe.txt", verbatim_workspace)}),
        ),
        ToolCallRequest::new(
            "exec_command",
            json!({
                "cmd":"cmd.exe /d /c echo should-not-run>must-not-exist.txt",
                "cwd":verbatim_workspace.clone()
            }),
        ),
    ] {
        let result = guard.call_tool(PermissionMode::Full, request, |_| {});
        assert!(matches!(
            result,
            Err(GuardError::Denied(denied))
                if denied.reason == DenyReason::VerbatimExecutionPath
        ));
    }
    assert!(!sentinel.exists(), "denied command reached the real upstream runtime");

    drop(guard);
    cleanup_nested_git_workspace(&workspace);
}
