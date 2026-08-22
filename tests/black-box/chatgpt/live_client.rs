#![cfg(windows)]

use std::fs;
use std::io::Write;
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use localbridge_lib::control_plane::convergence::{
    DesiredState, DesiredStateOwner, DesiredWorkspace, ServiceIntent,
};
use localbridge_lib::execution::CapabilityPolicy;
use localbridge_lib::mcp::{
    CodingToolsPermissionMode, CodingToolsRuntime, CodingToolsRuntimeConfig, InternalBearer,
    PolicyEnforcementRuntime,
};
use localbridge_lib::state::PermissionMode;
use serde_json::{Value, json};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct LiveRuntime {
    pep: Option<PolicyEnforcementRuntime>,
    workspace: PathBuf,
}

impl LiveRuntime {
    fn start(repo: &Path) -> Self {
        let workspace = std::env::temp_dir().join(format!(
            "localbridge-chatgpt-client-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&workspace).expect("create live client workspace");
        fs::write(
            workspace.join("range.txt"),
            b"line1\nline2\nline3\nline4\nline5\n",
        )
        .expect("write document range fixture");
        fs::write(
            workspace.join("policy_probe.cmd"),
            b"@echo off\r\nsc query EventLog\r\n",
        )
        .expect("write descendant policy probe");
        for args in [
            &["init"][..],
            &["config", "user.email", "black-box@example.invalid"][..],
            &["config", "user.name", "LocalBridge Black Box"][..],
            &["config", "core.autocrlf", "false"][..],
            &["add", "-A"][..],
            &["commit", "-m", "black-box fixture"][..],
        ] {
            run_git(&workspace, args);
        }
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                repo,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new("LOCALBRIDGE_CHATGPT_CLIENT_TEST_BEARER")
                .expect("valid test bearer"),
            Duration::from_secs(10),
        )
        .expect("bundled coding runtime starts");
        let desired = DesiredStateOwner::default();
        desired.replace(DesiredState {
            permission: PermissionMode::Full,
            workspace: Some(DesiredWorkspace::for_runtime_path(&workspace)),
            services: ServiceIntent::Enabled,
            connection: None,
        });
        let pep = PolicyEnforcementRuntime::start_with_control_plane(
            coding,
            CapabilityPolicy::load(&repo.join("runtime-policy.toml"))
                .expect("load public capability policy"),
            desired,
            None,
            None,
            None,
        )
        .expect("public MCP runtime starts");
        Self {
            pep: Some(pep),
            workspace,
        }
    }

    fn endpoint(&self) -> String {
        format!(
            "http://127.0.0.1:{}/mcp",
            self.pep.as_ref().expect("runtime active").port()
        )
    }

    fn workspace(&self) -> &Path {
        &self.workspace
    }
}

impl Drop for LiveRuntime {
    fn drop(&mut self) {
        if let Some(pep) = self.pep.take() {
            if let Ok(mut coding) = pep.stop() {
                let _ = coding.stop();
            }
        }
        let _ = fs::remove_dir_all(&self.workspace);
    }
}

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("bind ephemeral port")
        .local_addr()
        .expect("ephemeral address")
        .port()
}

fn run_git(workspace: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .expect("run Git fixture command");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn external_client_reaches_terminal_through_the_public_mcp_protocol() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repository root")
        .to_path_buf();
    let runtime = LiveRuntime::start(&repo);
    let client_path = repo.join("tests/black-box/chatgpt/client.mjs");
    let mut child = Command::new("node")
        .arg(client_path)
        .args(["--url", &runtime.endpoint(), "--timeout-ms", "30000"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .expect("start external ChatGPT simulator");
    {
        let input = child.stdin.as_mut().expect("client stdin");
        for command in [
            json!({"op":"tools/list","request_id":"list-live"}),
            json!({
                "op":"tools/call",
                "request_id":"exec-live",
                "name":"exec_command",
                "arguments":{
                    "command":"Write-Output LB_CHATGPT_BLACK_BOX",
                    "shell":"windows_powershell",
                    "yield_time_ms":10000
                }
            }),
            json!({"op":"close"}),
        ] {
            writeln!(input, "{command}").expect("write JSONL command");
        }
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("wait for external client");
    assert!(
        output.status.success(),
        "client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = String::from_utf8(output.stdout)
        .expect("client output is UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("client emits JSONL"))
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 4, "{events:#?}");
    assert_eq!(events[0]["type"], "ready", "{events:#?}");
    assert_eq!(events[1]["method"], "tools/list", "{events:#?}");
    assert!(
        events[1]["body"]["result"]["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == "exec_command")),
        "{events:#?}"
    );
    assert!(
        events[1]["body"]["result"]["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().all(|tool| {
                tool["name"]
                    .as_str()
                    .is_some_and(|name| !name.contains("chatgpt") && !name.contains("test_client"))
            })),
        "test-only capability leaked into tools/list: {events:#?}"
    );
    assert_eq!(events[2]["method"], "tools/call", "{events:#?}");
    assert_eq!(
        events[2]["body"]["result"]["structuredContent"]["data"]["status"], "completed",
        "{events:#?}"
    );
    assert!(
        events[2]["body"]["result"]["structuredContent"]["data"]["output"]
            .as_str()
            .is_some_and(|output| output.contains("LB_CHATGPT_BLACK_BOX")),
        "{events:#?}"
    );
    assert_eq!(events[3]["type"], "closed", "{events:#?}");
    assert_eq!(events[3]["transport"]["status"], 204, "{events:#?}");
}

#[test]
fn revision46_reported_failures_are_rechecked_through_the_external_client() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repository root")
        .to_path_buf();
    let runtime = LiveRuntime::start(&repo);
    let scenario = repo.join("tests/black-box/chatgpt/revision46.mjs");
    let output = Command::new("node")
        .arg(scenario)
        .args(["--url", &runtime.endpoint(), "--workspace"])
        .arg(runtime.workspace())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .expect("run revision46 external scenario");
    assert!(
        output.status.success(),
        "revision46 scenario failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("scenario emits JSON report");
    for check in [
        "public_schema",
        "filesystem_enumeration",
        "workspace_path_equivalence",
        "command_session_ownership",
        "cancelled_envelope",
        "task_cancel_detached",
        "prepared_workflow_cancel",
        "cross_session_workflow_resume",
        "git_error_propagation",
        "document_range",
        "output_error_taxonomy",
        "final_projection",
    ] {
        assert_eq!(report["checks"][check], "PASS", "{check}: {report:#?}");
    }
    assert_eq!(
        report["checks"]["command_wait_budget"]["status"], "PASS",
        "{report:#?}"
    );
    assert_eq!(
        report["checks"]["descendant_process_authority"], "PASS_CURRENT_USER_PARITY",
        "direct and descendant shell execution must share current-user authority: {report:#?}"
    );
    assert_eq!(report["tunnel"], "NOT_RUN_LOCAL_PEP_ONLY", "{report:#?}");
    assert_eq!(
        report["chunked_local_transport"], "PASS_50_CHUNKED_50_EMPTY_PRECONNECTS",
        "{report:#?}"
    );
    eprintln!("REVISION46_BLACK_BOX_REPORT={report}");
}
