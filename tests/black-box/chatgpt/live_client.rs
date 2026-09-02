#![cfg(windows)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
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
use serde_json::Value;

#[path = "../../support/control_plane.rs"]
mod control_plane_support;
#[path = "../../support/document_fixtures.rs"]
mod document_fixtures;

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
        fs::write(
            workspace.join("document-probe.pdf"),
            simple_pdf("PDF_SEARCH_NEEDLE"),
        )
        .expect("write PDF document fixture");
        fs::write(workspace.join("rich.docx"), document_fixtures::rich_docx())
            .expect("write rich DOCX fidelity fixture");
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
            control_plane_support::ready_control_plane(
                &desired,
                &workspace,
                localbridge_lib::state::PrivilegeState::Disabled,
            ),
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

    fn client_headers(&self) -> String {
        serde_json::json!({
            "Authorization": self
                .pep
                .as_ref()
                .expect("runtime active")
                .test_client_authorization_header()
                .expect("authenticated public endpoint")
        })
        .to_string()
    }
}

fn simple_pdf(text: &str) -> Vec<u8> {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    let stream = format!("BT /F1 12 Tf 72 720 Td ({escaped}) Tj ET");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
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

fn unauthenticated_initialize_status(port: u16) -> u16 {
    let body = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"unauthenticated-probe","version":"1"}}}"#;
    let mut stream = std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .expect("connect unauthenticated probe");
    write!(
        stream,
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("write unauthenticated probe head");
    stream
        .write_all(body)
        .expect("write unauthenticated probe body");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read unauthenticated probe response");
    response
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .and_then(|status| status.parse().ok())
        .expect("HTTP response status")
}

#[test]
fn revision46_reported_failures_are_rechecked_through_the_external_client() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repository root")
        .to_path_buf();
    let runtime = LiveRuntime::start(&repo);
    assert_eq!(
        unauthenticated_initialize_status(runtime.pep.as_ref().unwrap().port()),
        401,
        "the public loopback MCP endpoint must authenticate before creating a session"
    );
    let scenario = repo.join("tests/black-box/chatgpt/revision46.mjs");
    let output = Command::new("node")
        .arg(scenario)
        .args(["--url", &runtime.endpoint(), "--workspace"])
        .arg(runtime.workspace())
        .env("LOCALBRIDGE_TEST_MCP_HEADERS", runtime.client_headers())
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
        "exec_contract",
        "patch_crash_boundary",
        "queued_request_cancel",
        "observation_during_work",
        "filesystem_enumeration",
        "workspace_path_equivalence",
        "command_session_ownership",
        "cancelled_envelope",
        "task_cancel_detached",
        "prepared_workflow_cancel",
        "cross_session_workflow_resume",
        "workflow_execution_ownership",
        "git_error_propagation",
        "document_workflow",
        "docx_fidelity",
        "output_error_taxonomy",
        "final_projection",
    ] {
        assert_eq!(report["checks"][check], "PASS", "{check}: {report:#?}");
    }
    let original = document_fixtures::rich_docx();
    let edited = fs::read(runtime.workspace().join("rich.docx")).expect("read edited DOCX");
    for part in [
        "[Content_Types].xml",
        "_rels/.rels",
        "word/_rels/document.xml.rels",
        "docProps/custom.xml",
    ] {
        assert_eq!(
            document_fixtures::zip_entry(&edited, part),
            document_fixtures::zip_entry(&original, part),
            "external client edit changed unrelated package part {part}"
        );
    }
    let document = String::from_utf8(document_fixtures::zip_entry(&edited, "word/document.xml"))
        .expect("UTF-8 document XML");
    assert!(document.contains("<w:t>new</w:t>"));
    assert!(document.contains("<w:rPr><w:b/></w:rPr>"));
    assert!(document.contains("<w:hyperlink r:id=\"rId7\">"));
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

#[test]
fn stopping_public_runtime_cancels_queued_work_before_side_effects() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut runtime = LiveRuntime::start(&repo);
    let mut client = Command::new("node")
        .arg(repo.join("tests/black-box/chatgpt/shutdown_queue.mjs"))
        .arg(runtime.endpoint())
        .env("LOCALBRIDGE_TEST_MCP_HEADERS", runtime.client_headers())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .expect("start shutdown probe");
    let mut stdout = BufReader::new(client.stdout.take().unwrap());
    let mut marker = String::new();
    stdout
        .read_line(&mut marker)
        .expect("wait for real queued work");
    if marker.trim() != "SHUTDOWN_QUEUE_READY" {
        let output = client.wait_with_output().unwrap();
        panic!(
            "queue setup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let coding = runtime
        .pep
        .take()
        .unwrap()
        .stop()
        .expect("stop public runtime");
    let mut remainder = String::new();
    stdout.read_to_string(&mut remainder).unwrap();
    let output = client.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(remainder.contains("SHUTDOWN_QUEUE_PASS"));
    assert!(
        !runtime
            .workspace()
            .join("shutdown-should-not-exist.txt")
            .exists()
    );
    drop(coding);
}
