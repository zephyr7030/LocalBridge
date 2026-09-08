use std::collections::BTreeSet;

use super::*;

#[test]
fn schema42_unified_error_diagnostics_preserve_detail_and_map_transport() {
    let denied = FacadeError::new(FacadeErrorCode::PolicyDenied, "denied", false).to_mcp_result();
    let error = &denied["structuredContent"]["error"];
    assert_eq!(error["code"], "PolicyDenied");
    assert_eq!(error["error_code"], "Denied");
    assert_eq!(error["phase"], "policy");
    assert_eq!(error["cause"], "policy_denied");

    let transport =
        normalize_runtime_error(CodingToolsRuntimeError::HttpStatus(400)).to_mcp_result();
    let error = &transport["structuredContent"]["error"];
    assert_eq!(error["code"], "SessionUnavailable");
    assert_eq!(error["error_code"], "Unavailable");
    assert_eq!(error["phase"], "transport");
    assert_eq!(error["cause"], "http_400");
    assert_eq!(error["http_status"], 400);

    let unknown = FacadeError::new(FacadeErrorCode::Internal, "internal", false).to_mcp_result();
    assert_eq!(
        unknown["structuredContent"]["error"]["error_code"],
        "Unknown"
    );
    assert_eq!(unknown["structuredContent"]["error"]["phase"], "unknown");
}

#[test]
fn schema42_policy_diagnostics_preserve_canonical_codes() {
    for code in [
        FacadeErrorCode::PolicyDenied,
        FacadeErrorCode::WorkspaceDenied,
        FacadeErrorCode::CapabilityDenied,
        FacadeErrorCode::PrivilegedRouteNotAvailable,
        FacadeErrorCode::ElevationRequired,
    ] {
        let result = FacadeError::new(code, "denied", false).to_mcp_result();
        let error = &result["structuredContent"]["error"];
        assert_eq!(error["code"], code.as_str());
        assert_eq!(error["error_code"], "Denied");
        assert_eq!(error["phase"], "policy");
        assert!(
            error["cause"]
                .as_str()
                .is_some_and(|cause| !cause.is_empty())
        );
    }
}

#[cfg(windows)]
#[test]
fn schema42_toolbox_runs_pinned_tools_through_public_exec_without_ambient_shadowing() {
    use crate::mcp::{CodingToolsPermissionMode, CodingToolsRuntimeConfig, InternalBearer};
    use std::net::{Ipv4Addr, TcpListener};
    use std::time::{Duration, Instant};

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!(
        "localbridge-lb012-toolbox-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
    for name in ["aria2c.cmd", "7z.cmd", "jq.cmd", "curl.cmd"] {
        std::fs::write(workspace.join(name), b"@echo LB_TOOLBOX_FAKE_SHADOW\r\n").unwrap();
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let runtime = CodingToolsRuntime::start(
        CodingToolsRuntimeConfig::new(&root, &workspace, port, CodingToolsPermissionMode::Trusted),
        InternalBearer::new("LB012_TOOLBOX_SYNTHETIC_BEARER").unwrap(),
        Duration::from_secs(10),
    )
    .expect("bundled coding runtime for Toolbox acceptance");
    let executions = ExecutionRegistry::for_workspace(runtime.workspace()).unwrap();
    let mut facade =
        AgentFacade::from_coding_runtime_with_executions(runtime, policy(), executions).unwrap();
    assert_eq!(facade.public_tools()["tools"].as_array().unwrap().len(), 9);
    let context = facade
        .call_tool(
            PermissionMode::Full,
            "workspace_context",
            json!({"detail":"compact"}),
            None,
            |_| {},
        )
        .unwrap();
    let toolbox = &context["structuredContent"]["data"]["runtime_availability"]["toolbox"];
    for name in ["aria2c", "7z", "jq", "curl"] {
        assert_eq!(
            toolbox[name]["status"], "ready",
            "{name} probe was not ready: {toolbox:#}"
        );
    }

    for (command, shell, expected) in [
        ("aria2c --version", "cmd", "aria2 version 1.37.0"),
        ("7z", "cmd", "7-Zip (a) 26.02"),
        ("jq --version", "cmd", "jq-1.8.2"),
        ("curl --version", "cmd", "curl "),
        ("curl.exe --version", "windows_powershell", "curl "),
    ] {
        let mut result = facade
            .call_tool(
                PermissionMode::Full,
                "exec_command",
                json!({"command":command,"shell":shell,"yield_time_ms":0,"timeout_ms":120000,"max_output_bytes":65536}),
                None,
                |_| {},
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(150);
        let mut output = String::new();
        loop {
            output.push_str(
                result["structuredContent"]["data"]["output"]
                    .as_str()
                    .unwrap_or_default(),
            );
            if result["structuredContent"]["data"]["status"] != "running" {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{command} ({shell}) did not converge: {result:#}"
            );
            let public_session = result["structuredContent"]["data"]["session_id"]
                .as_str()
                .expect("running command has PublicSessionId")
                .to_string();
            result = facade
                .call_tool(
                    PermissionMode::Full,
                    "command_control",
                    json!({"action":"poll","session_id":public_session,"wait_ms":1000}),
                    None,
                    |_| {},
                )
                .unwrap();
        }
        assert_eq!(result["isError"], false, "{command} ({shell}): {result:#}");
        assert!(
            output.contains(expected),
            "{command} ({shell}) did not use expected Toolbox executable: {output}"
        );
        assert!(
            !output.contains("LB_TOOLBOX_FAKE_SHADOW"),
            "{command} resolved through workspace shadow: {output}"
        );
    }

    let mut runtime = facade.into_runtime();
    runtime.stop().unwrap();
    drop(runtime);
    std::fs::remove_dir_all(workspace).unwrap();
}

fn test_task_state(label: &str) -> ExecutionRegistry {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    ExecutionRegistry::open_at(std::env::temp_dir().join(format!(
        "localbridge-facade-task-state-{label}-{}-{nonce}.json",
        std::process::id()
    )))
    .unwrap()
}

fn bind_test_session(
    sessions: &mut PublicCommandSessions,
    task_state: &ExecutionRegistry,
    private: &str,
) -> String {
    let public = sessions
        .start_session(task_state, None, None)
        .unwrap()
        .public_session_id;
    sessions
        .bind_private_session(task_state, &public, private)
        .unwrap();
    public
}

struct FakeAdapter {
    catalog: Value,
    checkpoint: std::sync::Arc<std::sync::Mutex<Option<Value>>>,
}

impl FakeAdapter {
    fn new(catalog: Value) -> Self {
        Self {
            catalog,
            checkpoint: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

fn fake_document_result(request: DocumentRequest) -> DocumentResult {
    match request {
        DocumentRequest::Inspect {
            path, start_block, ..
        } => DocumentResult::Inspect {
            path,
            format: DocumentFormat::Text,
            sha256: "a".repeat(64),
            total_bytes: 4,
            start_block,
            end_block: Some(start_block),
            total_blocks: 1,
            blocks: vec![crate::document::DocumentBlock {
                id: "block-1".into(),
                kind: crate::document::DocumentBlockKind::Paragraph,
                text: "old".into(),
                level: None,
            }],
            text: "old".into(),
            truncated: false,
        },
        DocumentRequest::Search { path, .. } => DocumentResult::Search {
            path,
            format: DocumentFormat::Text,
            sha256: "a".repeat(64),
            matches: Vec::new(),
            total_blocks: 1,
            truncated: false,
        },
        DocumentRequest::Create { path, .. } => DocumentResult::Create {
            path,
            format: DocumentFormat::Text,
            sha256: "b".repeat(64),
            bytes: 3,
        },
        DocumentRequest::Edit { path, edits, .. } => DocumentResult::Edit {
            path,
            format: DocumentFormat::Text,
            sha256: "c".repeat(64),
            applied_edits: edits.len(),
        },
        DocumentRequest::Convert { source, path } => DocumentResult::Convert {
            source,
            path,
            source_format: DocumentFormat::Text,
            format: DocumentFormat::Markdown,
            source_sha256: "a".repeat(64),
            sha256: "d".repeat(64),
            bytes: 3,
        },
        DocumentRequest::Rebuild { path, .. } => DocumentResult::Rebuild {
            path,
            format: DocumentFormat::Text,
            sha256: "e".repeat(64),
            bytes: 7,
        },
    }
}

impl WorkspaceRuntimeAdapter for FakeAdapter {
    fn negotiate(&mut self) -> Result<(), FacadeError> {
        validate_runtime_capabilities(&self.catalog)
    }

    fn validate_workspace_identity(&self) -> Result<(), FacadeError> {
        Ok(())
    }

    fn workspace_context(&mut self, _request_id: Option<&Value>) -> Result<Value, FacadeError> {
        Ok(stable_success(json!({}), "ok"))
    }

    fn normalize_workspace_path(
        &self,
        path: &str,
        allow_missing_leaf: bool,
    ) -> Result<String, FacadeError> {
        if !allow_missing_leaf && path == "new.txt" {
            return Err(FacadeError::new(
                FacadeErrorCode::NotFound,
                "missing",
                false,
            ));
        }
        Ok(path.replace('\\', "/"))
    }

    fn project_context(&self, path: &str) -> Result<Value, FacadeError> {
        Ok(json!({"selected_path":path}))
    }

    fn coding_context(&self, _project_path: &str, _objective: &str) -> Result<Value, FacadeError> {
        Ok(json!({
            "instructions":[],
            "important_files":["safe/doc.txt"],
            "related_files":["safe/doc.txt"],
            "relevant_ranges":[],
            "files_read":[{
                "path":"safe/doc.txt",
                "start_line":1,
                "end_line":1,
                "content_sha256":"a".repeat(64)
            }]
        }))
    }

    fn coding_verification_plan(&self, _project_path: &str) -> Result<Vec<Value>, FacadeError> {
        Ok(vec![json!({
            "command":"echo verify",
            "shell":"cmd",
            "workdir":"."
        })])
    }

    fn verify_coding_edit_preconditions(
        &self,
        _expected: &Map<String, Value>,
    ) -> Result<(), FacadeError> {
        Ok(())
    }

    fn apply_coding_patch(
        &self,
        _patch: &str,
        _expected: &Map<String, Value>,
    ) -> Result<Vec<String>, FacadeError> {
        Ok(vec!["safe/doc.txt".into()])
    }

    fn apply_directory_change(&mut self, action: &str, path: &str) -> Result<Value, FacadeError> {
        Ok(json!({"action":action,"path":path,"changed":true}))
    }

    fn execute_shell(
        &mut self,
        _request: ShellCommandRequest,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        Ok(stable_success(json!({}), "ok"))
    }

    fn control_command(
        &mut self,
        _action: CommandControlAction,
        _arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        Ok(stable_success(json!({}), "ok"))
    }

    fn git_workflow(
        &mut self,
        _action: GitWorkflowAction,
        _arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        Ok(stable_success(json!({}), "ok"))
    }

    fn execute_document(&self, request: DocumentRequest) -> Result<DocumentResult, FacadeError> {
        Ok(fake_document_result(request))
    }

    fn apply_workflow_patch(
        &mut self,
        _arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        Ok(stable_success(json!({"applied":true}), "ok"))
    }

    fn inspect_image(
        &mut self,
        _arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        Ok(stable_success(json!({}), "ok"))
    }

    fn root_is_running(&self) -> Result<Option<bool>, CodingToolsRuntimeError> {
        Ok(Some(true))
    }

    fn reap_command_sessions(&mut self) -> Result<(), FacadeError> {
        Ok(())
    }

    fn has_running_execution(&self) -> bool {
        false
    }

    fn load_workflow_checkpoint(&self) -> Result<Option<Value>, FacadeError> {
        Ok(self
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone())
    }

    fn save_workflow_checkpoint(&self, checkpoint: &Value) -> Result<(), FacadeError> {
        *self
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(checkpoint.clone());
        Ok(())
    }

    fn clear_workflow_checkpoint(&self) -> Result<(), FacadeError> {
        *self
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }
}

#[derive(Clone)]
struct ResumeFixtureState {
    checkpoint: std::sync::Arc<std::sync::Mutex<Option<Value>>>,
    directory_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    patch_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    execute_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    last_stdin: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    git_head: std::sync::Arc<std::sync::Mutex<String>>,
    terminal_status: std::sync::Arc<std::sync::Mutex<String>>,
    failure_stage: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl ResumeFixtureState {
    fn new() -> Self {
        Self {
            checkpoint: std::sync::Arc::new(std::sync::Mutex::new(None)),
            directory_calls: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            patch_calls: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            execute_calls: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            last_stdin: std::sync::Arc::new(std::sync::Mutex::new(None)),
            git_head: std::sync::Arc::new(std::sync::Mutex::new("HEAD-STABLE".into())),
            terminal_status: std::sync::Arc::new(std::sync::Mutex::new("completed".into())),
            failure_stage: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn fails_at(&self, stage: &str) -> bool {
        self.failure_stage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_deref()
            == Some(stage)
    }
}

struct ResumeAdapter {
    catalog: Value,
    state: ResumeFixtureState,
    first_execution_runs: bool,
}

impl WorkspaceRuntimeAdapter for ResumeAdapter {
    fn negotiate(&mut self) -> Result<(), FacadeError> {
        validate_runtime_capabilities(&self.catalog)
    }

    fn validate_workspace_identity(&self) -> Result<(), FacadeError> {
        Ok(())
    }

    fn workspace_context(&mut self, _request_id: Option<&Value>) -> Result<Value, FacadeError> {
        if self.state.fails_at("workspace") {
            return Err(FacadeError::new(
                FacadeErrorCode::RuntimeUnavailable,
                "fixture failure",
                false,
            ));
        }
        Ok(stable_success(json!({}), "ok"))
    }

    fn normalize_workspace_path(
        &self,
        path: &str,
        _allow_missing_leaf: bool,
    ) -> Result<String, FacadeError> {
        Ok(path.replace('\\', "/"))
    }

    fn project_context(&self, path: &str) -> Result<Value, FacadeError> {
        Ok(json!({"selected_path":path}))
    }

    fn coding_context(&self, _project_path: &str, _objective: &str) -> Result<Value, FacadeError> {
        Ok(json!({
            "instructions":[],
            "important_files":["safe/doc.txt"],
            "related_files":["safe/doc.txt"],
            "relevant_ranges":[],
            "files_read":[{
                "path":"safe/doc.txt",
                "start_line":1,
                "end_line":1,
                "content_sha256":"a".repeat(64)
            }]
        }))
    }

    fn coding_verification_plan(&self, _project_path: &str) -> Result<Vec<Value>, FacadeError> {
        Ok(Vec::new())
    }

    fn verify_coding_edit_preconditions(
        &self,
        _expected: &Map<String, Value>,
    ) -> Result<(), FacadeError> {
        Ok(())
    }

    fn apply_coding_patch(
        &self,
        _patch: &str,
        _expected: &Map<String, Value>,
    ) -> Result<Vec<String>, FacadeError> {
        Ok(vec!["safe/doc.txt".into()])
    }

    fn apply_directory_change(&mut self, action: &str, path: &str) -> Result<Value, FacadeError> {
        if self.state.fails_at("directory") {
            return Err(FacadeError::new(
                FacadeErrorCode::RuntimeUnavailable,
                "fixture failure",
                false,
            ));
        }
        self.state
            .directory_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({"action":action,"path":path,"changed":true}))
    }

    fn execute_shell(
        &mut self,
        request: ShellCommandRequest,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        if self.state.fails_at("command") {
            return Err(FacadeError::new(
                FacadeErrorCode::RuntimeUnavailable,
                "fixture failure",
                false,
            ));
        }
        *self
            .state
            .last_stdin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = request.stdin.clone();
        let call = self
            .state
            .execute_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.first_execution_runs && call == 0 {
            return Ok(stable_success(
                json!({"status":"running","session_id":"lb-session-resume-fixture","output":""}),
                "running",
            ));
        }
        Ok(stable_success(
            json!({
                "status":"completed",
                "session_id":format!("lb-session-completed-{call}"),
                "output":request.execution.command
            }),
            "completed",
        ))
    }

    fn control_command(
        &mut self,
        _action: CommandControlAction,
        _arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        Err(session_unavailable())
    }

    fn git_workflow(
        &mut self,
        _action: GitWorkflowAction,
        _arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        if self.state.fails_at("git") {
            return Err(FacadeError::new(
                FacadeErrorCode::RuntimeUnavailable,
                "fixture failure",
                false,
            ));
        }
        let head = self
            .state
            .git_head
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        Ok(stable_success(
            json!({"is_repo":true,"repository_root":".","branch":"main","head":head,"clean":true,"entries":[]}),
            "ok",
        ))
    }

    fn apply_workflow_patch(
        &mut self,
        _arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        if self.state.fails_at("patch") {
            return Err(FacadeError::new(
                FacadeErrorCode::RuntimeUnavailable,
                "fixture failure",
                false,
            ));
        }
        self.state
            .patch_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(stable_success(json!({"applied":true}), "ok"))
    }

    fn inspect_image(
        &mut self,
        _arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        Ok(stable_success(json!({}), "ok"))
    }

    fn root_is_running(&self) -> Result<Option<bool>, CodingToolsRuntimeError> {
        Ok(Some(true))
    }

    fn reap_command_sessions(&mut self) -> Result<(), FacadeError> {
        Ok(())
    }

    fn has_running_execution(&self) -> bool {
        false
    }

    fn load_workflow_checkpoint(&self) -> Result<Option<Value>, FacadeError> {
        Ok(self
            .state
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone())
    }

    fn save_workflow_checkpoint(&self, checkpoint: &Value) -> Result<(), FacadeError> {
        *self
            .state
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(checkpoint.clone());
        Ok(())
    }

    fn clear_workflow_checkpoint(&self) -> Result<(), FacadeError> {
        *self
            .state
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }

    fn durable_command_terminal(&self, session_id: &str) -> Option<Value> {
        if session_id != "lb-session-resume-fixture" {
            return None;
        }
        let status = self
            .state
            .terminal_status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let data = Map::from_iter([
            ("status".into(), Value::String(status.clone())),
            ("session_id".into(), Value::String(session_id.to_string())),
        ]);
        if status == "completed" {
            Some(stable_success(Value::Object(data), "completed"))
        } else {
            Some(stable_command_error(
                FacadeErrorCode::SessionUnavailable,
                "session lost",
                data,
            ))
        }
    }
}

fn policy() -> CapabilityPolicy {
    CapabilityPolicy::from_toml(include_str!("../../../runtime-policy.toml")).unwrap()
}

fn compatible_catalog() -> Value {
    serde_json::from_str(include_str!(
        "../../../compatibility/coding-tools/0.2.2/tools-list.json"
    ))
    .unwrap()
}

#[test]
fn agent_workflow_resume_from_fresh_facade_continues_only_missing_steps() {
    let state = ResumeFixtureState::new();
    let initial_adapter = ResumeAdapter {
        catalog: compatible_catalog(),
        state: state.clone(),
        first_execution_runs: true,
    };
    let mut initial = AgentFacade::with_adapter(initial_adapter, policy()).unwrap();
    let started = initial
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({
                "action":"bugfix",
                "objective":"schema39 resume fixture",
                "path":".",
                "directory_changes":[{"action":"create_directory","path":"resume-dir"}],
                "patch":"*** Begin Patch\n*** Update File: safe/doc.txt\n@@\n-old\n+new\n*** End Patch",
                "commands":[
                    {"command":"echo first","shell":"cmd","yield_time_ms":0},
                    {"command":"echo second","shell":"cmd","yield_time_ms":1000}
                ]
            }),
            None,
        )
        .unwrap();
    assert_eq!(stable_data(&started)["state"], "running", "{started:#?}");
    assert!(
        stable_data(&started)["workflow_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("lb-task-"))
    );
    assert_eq!(
        state
            .directory_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        state.patch_calls.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        state
            .execute_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    drop(initial);

    let resumed_adapter = ResumeAdapter {
        catalog: compatible_catalog(),
        state: state.clone(),
        first_execution_runs: false,
    };
    let mut resumed = AgentFacade::with_adapter(resumed_adapter, policy()).unwrap();
    let completed = resumed
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"resume"}),
            None,
        )
        .unwrap();
    let data = stable_data(&completed);
    assert_eq!(data["action"], "resume", "{completed:#?}");
    assert_eq!(data["state"], "completed", "{completed:#?}");
    assert_eq!(
        state
            .directory_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "completed directory step was replayed"
    );
    assert_eq!(
        state.patch_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "completed patch was replayed"
    );
    assert_eq!(
        state
            .execute_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        2,
        "resume did not execute exactly the one missing command"
    );
    assert_eq!(data["commands"].as_array().map(Vec::len), Some(2));
    assert!(
        state
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none()
    );
}

#[test]
fn legacy_agent_workflow_failures_after_checkpoint_creation_are_terminal() {
    for stage in ["workspace", "git", "directory", "patch", "command"] {
        let state = ResumeFixtureState::new();
        *state
            .failure_stage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(stage.into());
        let adapter = ResumeAdapter {
            catalog: compatible_catalog(),
            state: state.clone(),
            first_execution_runs: false,
        };
        let mut facade = AgentFacade::with_adapter(adapter, policy()).unwrap();
        let error = facade
            .dispatch(
                PermissionMode::Full,
                "agent_workflow",
                json!({
                    "action":"bugfix",
                    "objective":"legacy terminal regression",
                    "path":".",
                    "directory_changes":[{"action":"create_directory","path":"probe-dir"}],
                    "patch":"*** Begin Patch\n*** Update File: safe/doc.txt\n@@\n-old\n+new\n*** End Patch",
                    "commands":[{"command":"echo probe","shell":"cmd","yield_time_ms":1000}]
                }),
                None,
            )
            .expect_err(stage);
        assert_eq!(error.code, FacadeErrorCode::RuntimeUnavailable, "{stage}");
        let stored: WorkflowCheckpoint = serde_json::from_value(
            state
                .checkpoint
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
                .expect("terminal checkpoint"),
        )
        .unwrap();
        assert!(stored.completed, "{stage}");
        assert_eq!(stored.current_step.as_deref(), Some("failed"), "{stage}");
        assert!(stored.next_step.is_none(), "{stage}");
        assert!(stored.current_session_id.is_none(), "{stage}");
        assert!(!stored.directory_inflight, "{stage}");
        assert!(!stored.patch_inflight, "{stage}");
        assert!(!stored.command_inflight, "{stage}");
        assert_eq!(
            stored
                .failure
                .as_ref()
                .and_then(|failure| failure.status.as_deref()),
            Some("failed"),
            "{stage}"
        );
        assert_eq!(facade.task_aggregate_snapshot()["state"], "idle", "{stage}");
    }
}

#[test]
fn background_reap_terminalizes_lost_durable_workflow_instead_of_waiting_forever() {
    let state = ResumeFixtureState::new();
    *state
        .terminal_status
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = "lost".into();
    let mut checkpoint = WorkflowCheckpoint::new(
        "workflow-lost-session".into(),
        json!({"action":"bugfix","objective":"lost session regression","path":"."}),
    );
    checkpoint.current_step = Some("command 1/1".into());
    checkpoint.next_step = Some("complete".into());
    checkpoint.command_inflight = true;
    checkpoint.current_session_id = Some("lb-session-resume-fixture".into());
    *state
        .checkpoint
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(serde_json::to_value(checkpoint).unwrap());

    let adapter = ResumeAdapter {
        catalog: compatible_catalog(),
        state: state.clone(),
        first_execution_runs: false,
    };
    let mut facade = AgentFacade::with_adapter(adapter, policy()).unwrap();
    facade.reap_command_sessions().unwrap();

    let stored: WorkflowCheckpoint = serde_json::from_value(
        state
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("terminal checkpoint retained"),
    )
    .unwrap();
    assert!(stored.completed);
    assert_eq!(stored.current_step.as_deref(), Some("failed"));
    assert!(stored.next_step.is_none());
    assert!(stored.current_session_id.is_none());
    assert!(!stored.command_inflight);
    assert_eq!(
        stored
            .failure
            .as_ref()
            .and_then(|failure| failure.status.as_deref()),
        Some("lost")
    );
    let aggregate = facade.task_aggregate_snapshot();
    assert_eq!(aggregate["state"], "idle");
    assert!(aggregate["current_workflow"].is_null());
}

#[test]
fn durable_checkpoint_omits_stdin_while_initial_execution_still_receives_it() {
    let state = ResumeFixtureState::new();
    let adapter = ResumeAdapter {
        catalog: compatible_catalog(),
        state: state.clone(),
        first_execution_runs: true,
    };
    let mut facade = AgentFacade::with_adapter(adapter, policy()).unwrap();
    let result = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({
                "action":"bugfix",
                "path":".",
                "commands":[{
                    "command":"echo stdin-probe",
                    "shell":"cmd",
                    "stdin":"SECRET_STDIN_SENTINEL",
                    "yield_time_ms":0
                }]
            }),
            None,
        )
        .unwrap();
    assert_eq!(stable_data(&result)["state"], "running");
    assert_eq!(
        state
            .last_stdin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_deref(),
        Some("SECRET_STDIN_SENTINEL")
    );
    let stored = state
        .checkpoint
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .unwrap();
    assert!(
        stored.pointer("/arguments/commands/0/stdin").is_none(),
        "{stored:#?}"
    );
    assert_eq!(stored["redacted_stdin_command_indices"], json!([0]));
    assert!(
        !serde_json::to_string(&stored)
            .unwrap()
            .contains("SECRET_STDIN_SENTINEL")
    );
}

#[test]
fn agent_workflow_resume_fails_closed_for_uncertain_inflight_file_step() {
    let state = ResumeFixtureState::new();
    let mut checkpoint = WorkflowCheckpoint::new(
        "lb-workflow-uncertain".into(),
        json!({
            "action":"bugfix",
            "path":".",
            "patch":"*** Begin Patch\n*** Update File: safe/doc.txt\n@@\n-old\n+new\n*** End Patch"
        }),
    );
    checkpoint.patch_inflight = true;
    *state
        .checkpoint
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(serde_json::to_value(checkpoint).unwrap());
    let adapter = ResumeAdapter {
        catalog: compatible_catalog(),
        state: state.clone(),
        first_execution_runs: false,
    };
    let mut facade = AgentFacade::with_adapter(adapter, policy()).unwrap();
    let error = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"resume"}),
            None,
        )
        .expect_err("uncertain file completion must never be blindly replayed");
    assert_eq!(error.code, FacadeErrorCode::SessionUnavailable);
    assert_eq!(
        state.patch_calls.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert_eq!(
        state
            .execute_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

fn durable_task_snapshot(next_step: Option<&str>) -> Value {
    let state = ResumeFixtureState::new();
    let mut checkpoint = WorkflowCheckpoint::new_coding(
        "lb-task-settlement".into(),
        json!({"action":"bugfix","path":"."}),
        "settlement invariant".into(),
    );
    checkpoint.next_step = next_step.map(str::to_string);
    *state
        .checkpoint
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(serde_json::to_value(checkpoint).unwrap());
    AgentFacade::with_adapter(
        ResumeAdapter {
            catalog: compatible_catalog(),
            state,
            first_execution_runs: false,
        },
        policy(),
    )
    .unwrap()
    .durable_coding_task_snapshot()
    .unwrap()
}

#[test]
fn schema42_task_aggregate_waiting_is_not_idle() {
    let state = ResumeFixtureState::new();
    let mut checkpoint = WorkflowCheckpoint::new_coding(
        "lb-schema42".into(),
        json!({"action":"bugfix","path":"."}),
        "schema42".into(),
    );
    checkpoint.next_step = Some("edit".into());
    *state.checkpoint.lock().unwrap() = Some(serde_json::to_value(checkpoint).unwrap());
    let facade = AgentFacade::with_adapter(
        ResumeAdapter {
            catalog: compatible_catalog(),
            state,
            first_execution_runs: false,
        },
        policy(),
    )
    .unwrap();
    let aggregate = facade.task_aggregate_snapshot();
    assert_eq!(aggregate["state"], "waiting");
    assert_eq!(aggregate["current_workflow"]["state"], "waiting");
    assert!(aggregate.get("current_command").is_none());
}

#[test]
fn schema42_command_summary_is_status_derived() {
    assert_eq!(command_summary("running"), "Command running");
    assert_eq!(command_summary("completed"), "Command completed");
    assert_ne!(command_summary("running"), command_summary("completed"));
}

#[test]
fn schema42_command_kill_leaves_workflow_waiting() {
    let mut checkpoint =
        WorkflowCheckpoint::new_coding("lb-kill".into(), json!({"action":"bugfix"}), "kill".into());
    checkpoint.current_session_id = Some("s1".into());
    checkpoint.command_inflight = true;
    checkpoint.next_step = None;
    checkpoint.settle_command_kill("s1");
    assert!(checkpoint.current_session_id.is_none());
    assert!(!checkpoint.command_inflight);
    assert_eq!(checkpoint.next_step.as_deref(), Some("verify"));
    assert!(!checkpoint.completed);
}

#[test]
fn schema41_durable_task_waiting_requires_next_step() {
    let snapshot = durable_task_snapshot(Some("edit"));
    assert_eq!(
        (&snapshot["state"], &snapshot["completed"]),
        (&json!("waiting"), &json!(false))
    );
}

#[test]
fn schema41_durable_task_without_next_step_settles_completed() {
    let snapshot = durable_task_snapshot(None);
    assert_eq!(
        (&snapshot["state"], &snapshot["completed"]),
        (&json!("completed"), &json!(true))
    );
}

#[test]
fn schema41_phased_coding_task_keeps_one_identity_and_resume_never_replays_effects() {
    let state = ResumeFixtureState::new();
    let adapter = ResumeAdapter {
        catalog: compatible_catalog(),
        state: state.clone(),
        first_execution_runs: false,
    };
    let mut facade = AgentFacade::with_adapter(adapter, policy()).unwrap();
    let prepared = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({
                "action":"bugfix",
                "phase":"prepare",
                "objective":"schema41 durable coding task",
                "path":"."
            }),
            None,
        )
        .unwrap();
    let task_id = stable_data(&prepared)["task_id"]
        .as_str()
        .expect("prepare task id")
        .to_string();
    assert!(task_id.starts_with("lb-task-"));
    assert_eq!(stable_data(&prepared)["state"], "prepared");
    assert_eq!(prepared["structuredContent"]["task_id"], task_id);

    let resumed_prepare = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"resume"}),
            None,
        )
        .unwrap();
    assert_eq!(stable_data(&resumed_prepare)["task_id"], task_id);
    assert_eq!(stable_data(&resumed_prepare)["state"], "prepared");
    assert_eq!(
        state
            .execute_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert_eq!(
        state.patch_calls.load(std::sync::atomic::Ordering::SeqCst),
        0
    );

    let edited = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({
                "action":"bugfix",
                "phase":"edit",
                "task_id":task_id,
                "expected_files":{"safe/doc.txt":"a".repeat(64)},
                "patch":"*** Begin Patch\n*** Update File: safe/doc.txt\n@@\n-old\n+new\n*** End Patch"
            }),
            None,
        )
        .unwrap();
    assert_eq!(stable_data(&edited)["task_id"], task_id);
    assert_eq!(stable_data(&edited)["next_step"], "verify");

    let verified = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"bugfix","phase":"verify","task_id":task_id}),
            None,
        )
        .unwrap();
    assert_eq!(stable_data(&verified)["task_id"], task_id);
    assert_eq!(stable_data(&verified)["next_step"], "persist");
    assert_eq!(
        state
            .execute_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );

    let persisted = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"bugfix","phase":"persist","task_id":task_id}),
            None,
        )
        .unwrap();
    assert_eq!(stable_data(&persisted)["state"], "persisted");
    assert_eq!(stable_data(&persisted)["completed"], true);

    let resumed_terminal = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"resume"}),
            None,
        )
        .unwrap();
    assert_eq!(stable_data(&resumed_terminal)["state"], "persisted");
    assert_eq!(stable_data(&resumed_terminal)["task_id"], task_id);
    assert_eq!(
        state
            .execute_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert_eq!(
        state.patch_calls.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    let checkpoint = state
        .checkpoint
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .expect("terminal coding checkpoint remains durable");
    assert_eq!(checkpoint["completed"], true);
    assert_eq!(checkpoint["workflow_id"], task_id);
}

#[test]
fn schema41_phased_coding_task_rejects_skipped_verify_or_persist() {
    let state = ResumeFixtureState::new();
    let adapter = ResumeAdapter {
        catalog: compatible_catalog(),
        state,
        first_execution_runs: false,
    };
    let mut facade = AgentFacade::with_adapter(adapter, policy()).unwrap();
    let prepared = facade.dispatch(
        PermissionMode::Full,
        "agent_workflow",
        json!({"action":"bugfix","phase":"prepare","objective":"strict phase order","path":"."}),
        None,
    ).unwrap();
    let task_id = stable_data(&prepared)["task_id"]
        .as_str()
        .unwrap()
        .to_string();
    for phase in ["verify", "persist"] {
        let error = facade
            .dispatch(
                PermissionMode::Full,
                "agent_workflow",
                json!({"action":"bugfix","phase":phase,"task_id":task_id}),
                None,
            )
            .unwrap_err();
        assert_eq!(error.code, FacadeErrorCode::SessionUnavailable);
    }
    facade.dispatch(
        PermissionMode::Full,
        "agent_workflow",
        json!({
            "action":"bugfix","phase":"edit","task_id":task_id,
            "expected_files":{"safe/doc.txt":"a".repeat(64)},
            "patch":"*** Begin Patch\n*** Update File: safe/doc.txt\n@@\n-old\n+new\n*** End Patch"
        }),
        None,
    ).unwrap();
    let error = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"bugfix","phase":"persist","task_id":task_id}),
            None,
        )
        .unwrap_err();
    assert_eq!(error.code, FacadeErrorCode::SessionUnavailable);
}

#[test]
fn schema41_stale_schema39_client_can_complete_durable_coding_task() {
    let state = ResumeFixtureState::new();
    let adapter = ResumeAdapter {
        catalog: compatible_catalog(),
        state: state.clone(),
        first_execution_runs: false,
    };
    let mut facade = AgentFacade::with_adapter(adapter, policy()).unwrap();

    let prepared = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"bugfix","objective":"stale projection compatibility","path":"."}),
            None,
        )
        .unwrap();
    let task_id = stable_data(&prepared)["task_id"]
        .as_str()
        .expect("compat prepare task id")
        .to_string();
    assert_eq!(stable_data(&prepared)["state"], "prepared");

    let edited = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({
                "action":"bugfix",
                "patch":"*** Begin Patch\n*** Update File: safe/doc.txt\n@@\n-old\n+new\n*** End Patch"
            }),
            None,
        )
        .unwrap();
    assert_eq!(stable_data(&edited)["task_id"], task_id);
    assert_eq!(stable_data(&edited)["next_step"], "verify");
    assert_eq!(
        state.patch_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "coding edit uses internal adapter path rather than legacy patch counter"
    );

    let verified = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"resume"}),
            None,
        )
        .unwrap();
    assert_eq!(stable_data(&verified)["task_id"], task_id);
    assert_eq!(stable_data(&verified)["next_step"], "persist");

    let persisted = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"resume"}),
            None,
        )
        .unwrap();
    assert_eq!(stable_data(&persisted)["task_id"], task_id);
    assert_eq!(stable_data(&persisted)["state"], "persisted");
    assert_eq!(stable_data(&persisted)["completed"], true);
    assert_eq!(
        state
            .execute_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[test]
fn schema41_stale_schema39_resume_rechecks_verify_policy_in_edit_mode() {
    let state = ResumeFixtureState::new();
    let adapter = ResumeAdapter {
        catalog: compatible_catalog(),
        state: state.clone(),
        first_execution_runs: false,
    };
    let mut facade = AgentFacade::with_adapter(adapter, policy()).unwrap();
    let prepared = facade
        .dispatch(
            PermissionMode::Edit,
            "agent_workflow",
            json!({"action":"bugfix","objective":"edit mode stale projection","path":"."}),
            None,
        )
        .unwrap();
    let task_id = stable_data(&prepared)["task_id"]
        .as_str()
        .unwrap()
        .to_string();
    let edited = facade
        .dispatch(
            PermissionMode::Edit,
            "agent_workflow",
            json!({
                "action":"bugfix",
                "patch":"*** Begin Patch\n*** Update File: safe/doc.txt\n@@\n-old\n+new\n*** End Patch"
            }),
            None,
        )
        .unwrap();
    assert_eq!(stable_data(&edited)["task_id"], task_id);
    let error = facade
        .dispatch(
            PermissionMode::Edit,
            "agent_workflow",
            json!({"action":"resume"}),
            None,
        )
        .expect_err("resume must not bypass verify ProcessExec policy in Edit mode");
    assert_eq!(error.code, FacadeErrorCode::CapabilityDenied);
    assert_eq!(
        state
            .execute_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[test]
fn schema41_incomplete_checkpoint_cannot_be_overwritten_by_unrelated_workflow() {
    let state = ResumeFixtureState::new();
    let mut c = WorkflowCheckpoint::new_coding(
        "lb-task-existing".into(),
        json!({"action":"bugfix","path":"."}),
        "existing".into(),
    );
    c.git_before = Some(json!({"is_repo":true,"repository_root":".","head":"HEAD-STABLE"}));
    *state.checkpoint.lock().unwrap() = Some(serde_json::to_value(&c).unwrap());
    let mut f = AgentFacade::with_adapter(
        ResumeAdapter {
            catalog: compatible_catalog(),
            state: state.clone(),
            first_execution_runs: false,
        },
        policy(),
    )
    .unwrap();
    let e = f
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"custom","commands":[{"command":"echo unrelated","shell":"cmd"}]}),
            None,
        )
        .unwrap_err();
    assert_eq!(e.code, FacadeErrorCode::SessionUnavailable);
    let stored: WorkflowCheckpoint =
        serde_json::from_value(state.checkpoint.lock().unwrap().clone().unwrap()).unwrap();
    assert_eq!(stored.workflow_id, "lb-task-existing");
    assert_eq!(
        state
            .execute_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[test]
fn schema41_resume_rejects_changed_git_head_before_side_effects() {
    let state = ResumeFixtureState::new();
    let mut c = WorkflowCheckpoint::new_coding(
        "lb-task-stale".into(),
        json!({"action":"bugfix","path":"."}),
        "stale".into(),
    );
    c.git_before = Some(json!({"is_repo":true,"repository_root":".","head":"HEAD-OLD"}));
    *state.checkpoint.lock().unwrap() = Some(serde_json::to_value(&c).unwrap());
    *state.git_head.lock().unwrap() = "HEAD-NEW".into();
    let mut f = AgentFacade::with_adapter(
        ResumeAdapter {
            catalog: compatible_catalog(),
            state: state.clone(),
            first_execution_runs: false,
        },
        policy(),
    )
    .unwrap();
    let e = f
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"resume"}),
            None,
        )
        .unwrap_err();
    assert_eq!(e.code, FacadeErrorCode::FileChanged);
    assert_eq!(
        state
            .execute_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[test]
fn schema41_workspace_context_projects_durable_task_truth() {
    let state = ResumeFixtureState::new();
    let mut c = WorkflowCheckpoint::new_coding(
        "lb-task-context".into(),
        json!({"action":"bugfix","path":"."}),
        "context".into(),
    );
    c.git_before = Some(json!({"is_repo":true,"repository_root":".","head":"HEAD-STABLE"}));
    *state.checkpoint.lock().unwrap() = Some(serde_json::to_value(&c).unwrap());
    let mut f = AgentFacade::with_adapter(
        ResumeAdapter {
            catalog: compatible_catalog(),
            state,
            first_execution_runs: false,
        },
        policy(),
    )
    .unwrap();
    let r = f
        .dispatch(PermissionMode::Full, "workspace_context", json!({}), None)
        .unwrap();
    let d = stable_data(&r);
    let task = &d["current_task"];
    assert_eq!(task["task_id"], "lb-task-context");
    assert_eq!(task["state"], "waiting");
    assert_eq!(task["next_step"], "edit");
}

#[test]
fn schema41_stderr_pages_share_one_sanitized_public_byte_space() {
    let body = (1..=100)
        .map(|n| format!("ERR-{n}"))
        .collect::<Vec<_>>()
        .join("_x000D__x000A_");
    let raw = json!({"structuredContent":{"content":format!("#< CLIXML\r\n<Objs><S S=\"Error\">{body}</S></Objs>")}});
    let expected = public_command_stderr(
        raw.pointer("/structuredContent/content")
            .unwrap()
            .as_str()
            .unwrap(),
    );
    let mut offset = 0u64;
    let mut rebuilt = String::new();
    loop {
        let page = public_stderr_page(&raw, "lb-output-stderr", offset, 50).unwrap();
        let data = stable_data(&page);
        assert_eq!(data["offset"].as_u64(), Some(offset));
        rebuilt.push_str(data["content"].as_str().unwrap());
        if let Some(next) = data["next_offset"].as_u64() {
            assert!(next > offset);
            offset = next;
        } else {
            assert_eq!(data["total_bytes"].as_u64(), Some(expected.len() as u64));
            break;
        }
    }
    assert_eq!(rebuilt, expected);
}

#[test]
fn schema41_private_patch_errors_keep_canonical_conflict_codes() {
    for (code, expected) in [
        ("PATCH_CONTEXT_NOT_FOUND", FacadeErrorCode::PatchConflict),
        ("PATCH_CONTEXT_AMBIGUOUS", FacadeErrorCode::AmbiguousMatch),
        ("PATCH_CONFLICT", FacadeErrorCode::FileChanged),
    ] {
        let raw = json!({"structuredContent":{"error":{"code":code}},"isError":true});
        assert_eq!(normalize_private_error(&raw).code, expected);
    }
}

#[test]
fn schema41_workflow_workdir_and_wait_budget_are_discoverable() {
    let workflow = public_tool_schema("agent_workflow");
    let wd=workflow["inputSchema"]["properties"]["commands"]["items"]["properties"]["workdir"]["description"].as_str().unwrap_or_default();
    assert!(
        wd.contains("agent_workflow.path")
            && wd.contains("selected project")
            && wd.contains("Do not repeat")
    );
    let control = public_tool_schema("command_control");
    let wait = control["inputSchema"]["properties"]["wait_ms"]["description"]
        .as_str()
        .unwrap_or_default();
    assert!(wait.contains("1000ms") && wait.contains("end-to-end"));
    assert_eq!(
        command_control_transport_timeout(500),
        std::time::Duration::from_millis(1000)
    );
}

#[test]
fn schema49_workflow_workdir_is_resolved_once_from_the_selected_project() {
    let adapter = FakeAdapter::new(compatible_catalog());
    assert_eq!(
        resolve_project_workdir(&adapter, "project/app", "tests")
            .unwrap()
            .to_string_lossy(),
        "project/app/tests"
    );
    assert_eq!(
        resolve_project_workdir(&adapter, "project/app", ".")
            .unwrap()
            .to_string_lossy(),
        "project/app/."
    );
    assert_eq!(
        resolve_project_workdir(&adapter, "project/app", r"D:\\outside"),
        Err(FacadeError::new(
            FacadeErrorCode::WorkspaceDenied,
            "工作区路径参数无效",
            false,
        ))
    );
}

#[test]
fn schema41_common_result_envelope_is_stable_for_success_and_error() {
    let success = stable_success(json!({"state":"completed","task_id":"lb-task-1"}), "done");
    let structured = &success["structuredContent"];
    for field in [
        "ok",
        "state",
        "summary",
        "task_id",
        "warnings",
        "next_step",
        "output_refs",
        "data",
        "error",
    ] {
        assert!(
            structured.get(field).is_some(),
            "missing success envelope field {field}"
        );
    }
    assert_eq!(structured["ok"], true);
    assert!(structured["error"].is_null());

    let command = stable_success(
        json!({"status":"running","output_refs":{"stdout":"lb-output-a"}}),
        "running",
    );
    assert_eq!(
        command["structuredContent"]["output_refs"],
        json!(["lb-output-a"])
    );
    assert_eq!(
        command["structuredContent"]["data"]["output_refs"]["stdout"],
        "lb-output-a"
    );

    let error = FacadeError::new(FacadeErrorCode::FileChanged, "changed", false).to_mcp_result();
    let structured = &error["structuredContent"];
    for field in [
        "ok",
        "state",
        "summary",
        "task_id",
        "warnings",
        "next_step",
        "output_refs",
        "data",
        "error",
    ] {
        assert!(
            structured.get(field).is_some(),
            "missing error envelope field {field}"
        );
    }
    assert_eq!(structured["ok"], false);
    assert!(structured["data"].is_null());
    assert_eq!(structured["error"]["code"], "FileChanged");
}

#[test]
fn registry_is_exactly_eight_localbridge_owned_tools() {
    let registry = ToolRegistry;
    assert_eq!(registry.version(), 1);
    let tools = registry.core_tools();
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, V1_CORE_TOOL_NAMES);
    for private in [
        "read_file",
        "apply_patch",
        "git_status",
        "write_stdin",
        "server_info",
    ] {
        assert!(!names.contains(&private));
    }
    assert!(tools.iter().all(|tool| tool.get("inputSchema").is_some()));
    assert!(tools.iter().all(|tool| tool.get("outputSchema").is_some()));
    for tool in tools {
        let schema = &tool["outputSchema"];
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["ok"].is_object());
        assert!(schema["properties"]["data"].is_object());
        assert!(schema["properties"]["error"].is_object());
        let required = schema["required"]
            .as_array()
            .expect("common required fields");
        for field in [
            "ok",
            "state",
            "summary",
            "task_id",
            "warnings",
            "next_step",
            "output_refs",
            "data",
            "error",
        ] {
            assert!(
                required.iter().any(|item| item == field),
                "{field} missing from common envelope"
            );
        }
    }
}

#[test]
fn upstream_extra_tool_or_irrelevant_private_schema_does_not_change_public_registry() {
    let before = ToolRegistry.core_tools();
    let mut catalog = compatible_catalog();
    catalog["tools"].as_array_mut().unwrap().push(json!({
        "name":"malicious_new_private_tool",
        "description":"must never become public",
        "inputSchema":{"type":"object","properties":{"danger":{"type":"string"}},"required":[]}
    }));
    catalog["tools"][0]["description"] = Value::String("private description changed".into());
    catalog["tools"][0]["inputSchema"]["properties"]["future_optional_private_field"] =
        json!({"type":"string"});
    assert!(validate_runtime_capabilities(&catalog).is_ok());
    assert_eq!(ToolRegistry.core_tools(), before);
}

#[test]
fn missing_or_incompatible_required_private_capability_fails_closed() {
    let mut missing = compatible_catalog();
    missing["tools"]
        .as_array_mut()
        .unwrap()
        .retain(|tool| tool["name"] != "exec_command");
    assert_eq!(
        validate_runtime_capabilities(&missing).unwrap_err().code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );

    let mut incompatible = compatible_catalog();
    let exec = incompatible["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "exec_command")
        .unwrap();
    exec["inputSchema"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("cmd");
    assert_eq!(
        validate_runtime_capabilities(&incompatible)
            .unwrap_err()
            .code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );
    assert!(matches!(
        AgentFacade::with_adapter(FakeAdapter::new(missing), policy()),
        Err(FacadeError {
            code: FacadeErrorCode::RuntimeCapabilityMismatch,
            ..
        })
    ));
    assert!(matches!(
        AgentFacade::with_adapter(FakeAdapter::new(incompatible), policy()),
        Err(FacadeError {
            code: FacadeErrorCode::RuntimeCapabilityMismatch,
            ..
        })
    ));
}

#[test]
fn private_capability_type_requiredness_and_array_item_drift_fail_closed() {
    let mut wrong_type = compatible_catalog();
    let exec = wrong_type["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "exec_command")
        .unwrap();
    exec["inputSchema"]["properties"]["cmd"]["type"] = Value::String("integer".into());
    assert_eq!(
        validate_runtime_capabilities(&wrong_type).unwrap_err().code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );

    let mut missing_requiredness = compatible_catalog();
    let exec = missing_requiredness["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "exec_command")
        .unwrap();
    exec["inputSchema"]["required"] = json!([]);
    assert_eq!(
        validate_runtime_capabilities(&missing_requiredness)
            .unwrap_err()
            .code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );

    let mut unexpected_required = compatible_catalog();
    let exec = unexpected_required["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "exec_command")
        .unwrap();
    exec["inputSchema"]["properties"]["future_required"] = json!({"type":"string"});
    exec["inputSchema"]["required"] = json!(["cmd", "future_required"]);
    assert_eq!(
        validate_runtime_capabilities(&unexpected_required)
            .unwrap_err()
            .code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );

    let mut wrong_array_items = compatible_catalog();
    let diff = wrong_array_items["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "git_diff")
        .unwrap();
    diff["inputSchema"]["properties"]["paths"]["items"]["type"] = Value::String("integer".into());
    assert_eq!(
        validate_runtime_capabilities(&wrong_array_items)
            .unwrap_err()
            .code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );

    let mut compatible_union = compatible_catalog();
    let exec = compatible_union["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "exec_command")
        .unwrap();
    exec["inputSchema"]["properties"]["cmd"]["type"] = json!(["string", "null"]);
    assert!(validate_runtime_capabilities(&compatible_union).is_ok());
}

#[test]
fn private_capability_constraint_narrowing_fails_closed_while_widening_is_compatible() {
    let mut min_length_narrowed = compatible_catalog();
    let exec = min_length_narrowed["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "exec_command")
        .unwrap();
    exec["inputSchema"]["properties"]["cmd"]["minLength"] = json!(2);
    assert_eq!(
        validate_runtime_capabilities(&min_length_narrowed)
            .unwrap_err()
            .code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );

    let mut maximum_narrowed = compatible_catalog();
    let exec = maximum_narrowed["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "exec_command")
        .unwrap();
    exec["inputSchema"]["properties"]["timeout_ms"]["maximum"] = json!(599_999);
    assert_eq!(
        validate_runtime_capabilities(&maximum_narrowed)
            .unwrap_err()
            .code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );

    let mut signal_narrowed = compatible_catalog();
    let kill = signal_narrowed["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "kill_session")
        .unwrap();
    kill["inputSchema"]["properties"]["signal"]["enum"] = json!(["TERM", "KILL"]);
    assert_eq!(
        validate_runtime_capabilities(&signal_narrowed)
            .unwrap_err()
            .code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );

    let mut stream_narrowed = compatible_catalog();
    let read = stream_narrowed["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "read_output")
        .unwrap();
    read["inputSchema"]["properties"]["stream"]["enum"] = json!(["stdout"]);
    assert_eq!(
        validate_runtime_capabilities(&stream_narrowed)
            .unwrap_err()
            .code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );

    let mut widened = compatible_catalog();
    let exec = widened["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "exec_command")
        .unwrap();
    exec["inputSchema"]["properties"]["cmd"]
        .as_object_mut()
        .unwrap()
        .remove("minLength");
    exec["inputSchema"]["properties"]["timeout_ms"]["maximum"] = json!(900_000);
    let kill = widened["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "kill_session")
        .unwrap();
    kill["inputSchema"]["properties"]["signal"]["enum"] = json!(["TERM", "KILL", "INT", "BREAK"]);
    assert!(validate_runtime_capabilities(&widened).is_ok());
}

#[test]
fn private_error_is_normalized_without_private_message_or_shape() {
    let raw = json!({
        "content":[{"type":"text","text":"SECRET_PRIVATE_RUNTIME_DETAIL"}],
        "structuredContent":{
            "ok":false,
            "error":{"code":"OUTSIDE_WORKSPACE","message":"SECRET_PRIVATE_RUNTIME_DETAIL","private":{"schema":true}}
        },
        "isError":true
    });
    let public = normalize_private_error(&raw).to_mcp_result();
    let rendered = serde_json::to_string(&public).unwrap();
    assert!(rendered.contains("WorkspaceDenied"));
    assert!(!rendered.contains("SECRET_PRIVATE_RUNTIME_DETAIL"));
    assert!(!rendered.contains("private"));

    let filesystem_permission = json!({
        "structuredContent":{
            "ok":false,
            "error":{
                "code":"PERMISSION_REQUIRED",
                "message":"SECRET_PRIVATE_RUNTIME_DETAIL",
                "details":{"permission":"filesystem_escape","path":"C:\\private"}
            }
        },
        "isError":true
    });
    assert_eq!(
        normalize_private_error(&filesystem_permission).code,
        FacadeErrorCode::WorkspaceDenied
    );
    let generic_permission = json!({
        "structuredContent":{
            "ok":false,
            "error":{"code":"PERMISSION_REQUIRED","details":{"permission":"network"}}
        },
        "isError":true
    });
    assert_eq!(
        normalize_private_error(&generic_permission).code,
        FacadeErrorCode::CapabilityDenied
    );
}

#[test]
fn command_control_schema_exposes_all_action_fields_at_top_level() {
    let schema = public_tool_schema("command_control")["inputSchema"].clone();
    assert_eq!(schema["type"], "object");
    assert!(schema.get("oneOf").is_none());
    assert_eq!(schema["required"], json!(["action"]));
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["action"]["enum"],
        json!(["adopt", "poll", "read", "write", "kill"])
    );
    for property in [
        "action",
        "session_id",
        "output_ref",
        "chars",
        "signal",
        "wait_ms",
        "stream",
        "offset",
        "limit",
    ] {
        assert!(
            schema["properties"][property].is_object(),
            "command_control top-level property missing: {property}"
        );
    }
    assert!(
        schema["properties"]["session_id"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("poll")
                && value.contains("write")
                && value.contains("kill"))
    );
    assert!(
        schema["properties"]["output_ref"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("read"))
    );
}

#[test]
fn document_schema_discloses_fixed_actions_and_hash_guarded_mutations() {
    let tool = public_tool_schema("document_workflow");
    let schema = &tool["inputSchema"];
    assert_eq!(schema["type"], "object");
    assert!(schema.get("oneOf").is_none());
    assert_eq!(
        schema["properties"]["action"]["enum"],
        json!(["inspect", "search", "create", "edit", "convert", "rebuild"])
    );
    assert!(
        tool["description"]
            .as_str()
            .is_some_and(|value| value.contains("DocumentIR") && value.contains("expected_sha256"))
    );
    assert_eq!(schema["properties"]["expected_sha256"]["minLength"], 64);
    assert_eq!(
        schema["properties"]["edits"]["items"]["properties"]["operation"]["enum"],
        json!(["replace", "insert_before", "insert_after", "delete"])
    );
}

#[test]
fn private_success_shape_is_allowlisted_before_publication() {
    let raw = json!({
        "content":[{"type":"text","text":"SECRET_PRIVATE_RENDERED_TEXT"}],
        "structuredContent":{
            "exit_code":0,
            "stdout":"safe output",
            "truncated":false,
            "future_private_field":"SECRET_PRIVATE_SCHEMA_VALUE"
        },
        "isError":false
    });
    assert_eq!(safe_command_output(&raw), "safe output");
    let rendered = safe_command_output(&raw);
    assert!(!rendered.contains("SECRET_PRIVATE_RENDERED_TEXT"));
    assert!(!rendered.contains("SECRET_PRIVATE_SCHEMA_VALUE"));

    let task_state = test_task_state("success-allowlist");
    let mut sessions = PublicCommandSessions::default();
    let public_session = bind_test_session(&mut sessions, &task_state, "PRIVATE_SESSION_SECRET");
    let public_output =
        sessions.public_output_for_private("PRIVATE_OUTPUT_SECRET", &public_session, "stdout");
    assert!(public_session.starts_with("lb-session-"));
    assert!(public_output.starts_with("lb-output-"));
    assert_ne!(public_session, "PRIVATE_SESSION_SECRET");
    assert_ne!(public_output, "PRIVATE_OUTPUT_SECRET");
}

#[test]
fn local_retained_output_handles_are_fifo_bounded_without_evicting_private_handles() {
    let mut sessions = PublicCommandSessions::default();
    let private =
        sessions.public_output_for_private("PRIVATE_OUTPUT_SECRET", "session-a", "stdout");
    let first = sessions.retain_local_output(McpSessionId::new("owner"), "stdout", "first".into());
    let mut latest = String::new();
    for index in 1..=MAX_LOCAL_RETAINED_OUTPUT_HANDLES {
        latest = sessions.retain_local_output(
            McpSessionId::new("owner"),
            "stderr",
            format!("retained-{index}"),
        );
    }

    assert!(sessions.local_output(&first).is_none());
    assert_eq!(
        sessions.private_output(&private).as_deref(),
        Some("PRIVATE_OUTPUT_SECRET")
    );
    assert_eq!(
        sessions.local_output(&latest),
        Some((
            "stderr".into(),
            format!("retained-{MAX_LOCAL_RETAINED_OUTPUT_HANDLES}")
        ))
    );
}

#[test]
fn command_adapter_mappings_reap_when_the_execution_owner_reaps() {
    let executions = test_task_state("mapping-reap");
    let mut sessions = PublicCommandSessions::default();
    let public = "expired-public-session".to_string();
    sessions.sessions.insert(
        public.clone(),
        PublicCommandSession {
            execution_id: ExecutionId::new("expired-execution"),
            started_at: Instant::now(),
            pending_output: String::new(),
            pending_output_truncated: false,
            stderr_protocol_buffer: String::new(),
        },
    );
    let output = sessions.public_output_for_private("expired-private-output", &public, "stdout");

    sessions.reap_expired_mappings(&executions);

    assert!(!sessions.sessions.contains_key(&public));
    assert!(sessions.private_output(&output).is_none());
}

#[test]
fn powershell_startup_progress_clixml_is_not_public_command_output_but_errors_are_preserved() {
    let progress = "#< CLIXML\r\n<Objs><Obj S=\"progress\"><S>Preparing modules</S></Obj></Objs>";
    assert_eq!(public_command_stderr(progress), "");
    let error = "#< CLIXML\r\n<Objs><Obj S=\"progress\"/><Obj S=\"Error\"><S>boom</S></Obj></Objs>";
    assert_eq!(public_command_stderr(error), "boom");
    assert_eq!(public_command_stderr("plain error\r\n"), "plain error\r\n");

    let wrapped_error = "#< CLIXML\r\n<Objs><S S=\"Error\">Set-Variable -Name PSModuleAutoLoadingPreference -Value None -Option Constant -Force;Import-Module -Name 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\Modules\\Microsoft.PowerShell.Management\\Microsoft.PowerShell.Management.psd1' -ErrorAction Stop;[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::OutputEncodi_x000D__x000A_</S><S S=\"Error\">ng;Write-Error 'READERR _xD83D__xDE80_' : READERR _xD83D__xDE80__x000D__x000A_</S><S S=\"Error\">    + CategoryInfo : NotSpecified_x000D__x000A_</S></Objs>";
    let public = public_command_stderr(wrapped_error);
    assert!(public.contains("READERR 🚀"), "{public:?}");
    assert!(public.contains("Write-Error"), "{public:?}");
    for private in [
        "PSModuleAutoLoadingPreference",
        "Microsoft.PowerShell.Management",
        "OutputEncoding",
        "_xD83D_",
        "_xDE80_",
    ] {
        assert!(!public.contains(private), "{public:?}");
    }
}

#[test]
fn fragmented_powershell_progress_clixml_is_buffered_until_safely_classified() {
    let task_state = test_task_state("clixml");
    let mut sessions = PublicCommandSessions::default();
    let public = bind_test_session(&mut sessions, &task_state, "PRIVATE_CLIXML");
    assert_eq!(sessions.filter_private_stderr(&public, "#< CLIXML\r\n"), "");
    assert_eq!(
        sessions.filter_private_stderr(
            &public,
            "<Objs><Obj S=\"progress\"><S>Preparing modules</S></Obj>"
        ),
        ""
    );
    assert_eq!(sessions.filter_private_stderr(&public, "</Objs>"), "");
    assert_eq!(
        sessions.filter_private_stderr(&public, "real stderr\r\n"),
        "real stderr\r\n"
    );

    let public_error = bind_test_session(&mut sessions, &task_state, "PRIVATE_CLIXML_ERROR");
    assert_eq!(
        sessions.filter_private_stderr(&public_error, "#< CLIXML\r\n"),
        ""
    );
    let visible = sessions.filter_private_stderr(
        &public_error,
        "<Objs><Obj S=\"Error\"><S>boom</S></Obj></Objs>tail\r\n",
    );
    assert_eq!(visible, "boomtail\r\n");
    assert!(!visible.contains("CLIXML"));
    assert!(!visible.contains("</Obj"));
}

#[test]
fn consecutive_headerless_powershell_progress_envelopes_never_leak() {
    let task_state = test_task_state("clixml-consecutive");
    let mut sessions = PublicCommandSessions::default();
    let public = bind_test_session(&mut sessions, &task_state, "PRIVATE_CLIXML_CONSECUTIVE");
    let visible = sessions.filter_private_stderr(
        &public,
        "#< CLIXML\r\n<Objs><Obj S=\"progress\"><S>Preparing</S></Obj></Objs>\r\n<Objs Version=\"1.1.0.1\" xmlns=\"http://schemas.microsoft.com/powershell/2004/04\"><Obj S=\"progress\"><MS><S>Preparing modules for first use.</S></MS></Obj></Objs>real stderr\r\n",
    );
    assert!(visible.contains("real stderr"));
    assert!(!visible.contains("CLIXML"));
    assert!(!visible.contains("<Objs"));
    assert!(!visible.contains("<Obj"));
    assert!(!visible.contains("<MS"));
    assert!(!visible.contains("</Obj"));
}

#[test]
fn retained_stderr_clixml_and_mid_envelope_pages_never_expose_private_framing() {
    let progress = "#< CLIXML\r\n<Objs><Obj S=\"progress\"><S>Preparing modules</S></Obj></Objs>";
    assert_eq!(public_command_stderr(progress), "");

    let error = "#< CLIXML\r\n<Objs><Obj S=\"Error\"><S>boom &amp; detail_x000D__x000A_next</S></Obj></Objs>";
    assert_eq!(public_command_stderr(error), "boom & detail\r\nnext");

    for page in [
        "MS></Obj></Objs>",
        "<MS><S>private-progress</S></MS></Obj></Objs>",
        "m &amp; detail</S></Obj></Objs>",
        "<Obj S=\"progress\"><S>Preparing modules</S></Obj></Objs>",
    ] {
        let public = public_command_stderr(page);
        assert!(!public.contains("CLIXML"), "{public:?}");
        assert!(!public.contains("<Obj"), "{public:?}");
        assert!(!public.contains("</Obj"), "{public:?}");
        assert!(!public.contains("<MS"), "{public:?}");
        assert!(!public.contains("</MS"), "{public:?}");
    }

    for retained_content in [progress, "MS></Obj></Objs>"] {
        let raw = json!({
            "structuredContent":{
                "stream":"stderr",
                "offset":64,
                "requested_offset":64,
                "limit":128,
                "content":retained_content,
                "next_offset":192,
                "truncated":false
            }
        });
        let normalized =
            CodingToolsRuntimeAdapter::normalize_read_output(&raw, "lb-output-retained-regression");
        let content = normalized["structuredContent"]["data"]["content"]
            .as_str()
            .unwrap_or_default();
        assert_eq!(content, "");
        let rendered = serde_json::to_string(&normalized).unwrap();
        assert!(!rendered.contains("CLIXML"));
        assert!(!rendered.contains("</Obj"));
    }
}

#[test]
fn schema36_retained_output_maps_total_stream_bytes_without_faking_page_end() {
    let raw = json!({
        "structuredContent":{
            "stream":"stdout",
            "offset":512,
            "requested_offset":512,
            "limit":512,
            "content":"hello",
            "next_offset":1024,
            "total_stream_bytes":8192,
            "truncated":true
        }
    });
    let normalized = CodingToolsRuntimeAdapter::normalize_read_output(&raw, "lb-output-schema36");
    let data = &normalized["structuredContent"]["data"];
    assert_eq!(data["returned_bytes"], 5);
    assert_eq!(data["total_bytes"], 8192);
    assert_eq!(data["offset"], 512);
    assert_eq!(data["next_offset"], 1024);
}

#[test]
fn git_success_shape_is_rebuilt_from_typed_allowlists() {
    let cases = [
        (
            GitWorkflowAction::Status,
            json!({
                "structuredContent":{
                    "ok":true,"is_repo":true,"branch":"main","ahead":0,"behind":0,
                    "clean":false,"truncated":false,"future_private":"SECRET_TOP",
                    "entries":[{"path":"a.txt","original_path":null,"index_status":"M","worktree_status":" ","private":"SECRET_NESTED"}]
                }
            }),
        ),
        (
            GitWorkflowAction::Diff,
            json!({
                "structuredContent":{
                    "ok":true,"diff":"diff --git a/a b/a","truncated":false,"warnings":["safe"],
                    "future_private":"SECRET_TOP","files":[{"path":"a","status":"modified","binary":false,"private":"SECRET_NESTED"}]
                }
            }),
        ),
        (
            GitWorkflowAction::Log,
            json!({
                "structuredContent":{
                    "ok":true,"is_repo":true,"ref":"HEAD","path":".","max_count":20,"skip":0,"truncated":false,
                    "warnings":[],"next_action":{"tool":"git_log","private":"SECRET_NAV"},"future_private":"SECRET_TOP",
                    "commits":[{"hash":"abc","short_hash":"abc","author_name":"A","author_email":"a@example.invalid","author_date":"2026-01-01","subject":"s","private":"SECRET_NESTED"}]
                }
            }),
        ),
        (
            GitWorkflowAction::Show,
            json!({
                "structuredContent":{
                    "ok":true,"is_repo":true,"rev":"HEAD","content":"safe","truncated":false,"warnings":[],
                    "future_private":"SECRET_TOP","files":[{"path":"a","status":"modified","binary":false,"private":"SECRET_NESTED"}]
                }
            }),
        ),
        (
            GitWorkflowAction::Blame,
            json!({
                "structuredContent":{
                    "ok":true,"is_repo":true,"path":"a","rev":null,"start_line":1,"end_line":1,"max_lines":200,
                    "truncated":false,"warnings":[],"next_action":{"tool":"git_blame","private":"SECRET_NAV"},"future_private":"SECRET_TOP",
                    "lines":[{"commit":"abc","original_line":1,"line":1,"author":"A","author_mail":"<a@example.invalid>","author_time":"1","summary":"s","content":"safe","private":"SECRET_NESTED"}]
                }
            }),
        ),
    ];
    for (action, raw) in cases {
        let public = normalize_git_success(action, &raw);
        let rendered = serde_json::to_string(&public).unwrap();
        assert!(!rendered.contains("future_private"));
        assert!(!rendered.contains("SECRET_TOP"));
        assert!(!rendered.contains("SECRET_NESTED"));
        assert!(!rendered.contains("SECRET_NAV"));
        assert!(!rendered.contains("next_action"));
    }
}

#[test]
fn git_error_is_not_projected_as_empty_success() {
    let raw = json!({
        "isError":true,
        "structuredContent":{
            "ok":false,
            "error":{
                "code":"GIT_ERROR",
                "message":"fatal: ambiguous argument 'definitely-not-a-ref'"
            }
        }
    });
    let error = normalize_git_error(&raw);
    assert_eq!(error.code, FacadeErrorCode::ProcessFailed);
    let public = error.to_mcp_result();
    assert_eq!(public["isError"], true);
    assert_eq!(
        public["structuredContent"]["error"]["details"]["git_error_code"],
        "GIT_ERROR"
    );
    assert!(
        public["structuredContent"]["error"]["details"]["git_message"]
            .as_str()
            .is_some_and(|message| message.contains("definitely-not-a-ref"))
    );
}

#[test]
fn image_success_content_is_rebuilt_without_private_item_fields() {
    let raw = json!({
        "content":[
            {"type":"image","data":"BASE64","mimeType":"image/png","private":"SECRET_IMAGE"},
            {"type":"text","text":"safe text","private":"SECRET_TEXT"},
            {"type":"resource","uri":"SECRET_RESOURCE"}
        ],
        "structuredContent":{"ok":true,"future_private":"SECRET_STRUCTURED"},
        "isError":false
    });
    let public = normalize_image_success(&raw);
    assert_eq!(public["content"].as_array().unwrap().len(), 2);
    assert_eq!(
        public["content"][0],
        json!({"type":"image","data":"BASE64","mimeType":"image/png"})
    );
    assert_eq!(
        public["content"][1],
        json!({"type":"text","text":"safe text"})
    );
    let rendered = serde_json::to_string(&public).unwrap();
    for private in [
        "SECRET_IMAGE",
        "SECRET_TEXT",
        "SECRET_RESOURCE",
        "SECRET_STRUCTURED",
        "future_private",
    ] {
        assert!(!rendered.contains(private));
    }
}

#[test]
fn internal_workspace_context_defers_authority_to_the_control_plane_mapper() {
    let mut facade =
        AgentFacade::with_adapter(FakeAdapter::new(compatible_catalog()), policy()).unwrap();
    let result = facade
        .call_tool(
            PermissionMode::Full,
            "workspace_context",
            json!({}),
            None,
            |_| {},
        )
        .unwrap();
    let data = stable_data(&result);
    assert!(data.get("permission_mode").is_none());
    assert!(data.get("authority").is_none());
    assert!(
        data["capabilities"]["public_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "exec_command")
    );
    assert!(data["shell_discovery"].is_object());
}

#[test]
fn schema39_exec_dry_run_completes_without_creating_public_session() {
    let mut facade =
        AgentFacade::with_adapter(FakeAdapter::new(compatible_catalog()), policy()).unwrap();
    let result = facade
        .call_tool(
            PermissionMode::Full,
            "exec_command",
            json!({"command":"where cmd","shell":"cmd","workdir":".","dry_run":true}),
            None,
            |_| {},
        )
        .unwrap();
    let data = stable_data(&result);
    assert_eq!(data["status"], "completed");
    assert_eq!(data["would_execute"], false);
    assert_eq!(data["route"], "ordinary");
    assert!(data.get("session_id").is_none());

    let schema = public_tool_schema("exec_command");
    let schema_fields = schema["inputSchema"]["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        schema_fields,
        EXEC_COMMAND_FIELDS.into_iter().collect::<BTreeSet<_>>()
    );

    for invalid in [
        json!({"command":"where cmd","unknown_field":true}),
        json!({"command":"where cmd","dry_run":"true"}),
        json!({"command":"where cmd","timeout_ms":0}),
    ] {
        let rejected = facade
            .call_tool(PermissionMode::Full, "exec_command", invalid, None, |_| {})
            .unwrap();
        assert_eq!(
            rejected["structuredContent"]["error"]["code"],
            "InvalidArgument"
        );
    }
}

#[test]
fn schema36_agent_dry_run_can_explain_process_restriction_in_edit() {
    let mut facade =
        AgentFacade::with_adapter(FakeAdapter::new(compatible_catalog()), policy()).unwrap();
    let result = facade
        .call_tool(
            PermissionMode::Edit,
            "agent_workflow",
            json!({
                "action":"diagnose",
                "path":".",
                "commands":[{"command":"echo hello","shell":"cmd","workdir":"."}],
                "dry_run":true
            }),
            None,
            |_| {},
        )
        .unwrap();
    let data = stable_data(&result);
    assert_eq!(data["state"], "completed");
    assert_eq!(data["commands"][0]["would_execute"], false);
    assert_eq!(data["commands"][0]["route"], "workspace_restricted");
}

#[test]
fn raw_upstream_name_is_not_a_public_registry_entry() {
    let registry = ToolRegistry;
    assert!(registry.contains("exec_command"));
    assert!(!registry.contains("read_file"));
    assert!(!registry.contains("request_permissions"));
}

#[test]
fn every_advertised_agent_and_document_action_has_an_executable_facade_contract() {
    let mut facade =
        AgentFacade::with_adapter(FakeAdapter::new(compatible_catalog()), policy()).unwrap();
    for action in [
        "diagnose",
        "bugfix",
        "feature",
        "refactor",
        "test_failure",
        "build_release",
        "document",
        "custom",
    ] {
        let result = facade
            .dispatch(
                PermissionMode::Full,
                "agent_workflow",
                json!({"action":action}),
                None,
            )
            .unwrap();
        assert_eq!(result["isError"], false, "action={action}: {result:#?}");
        assert_eq!(
            result["structuredContent"]["data"]["state"], "context_ready",
            "action={action}"
        );
    }
    let resume = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({"action":"resume"}),
            None,
        )
        .expect_err("resume without a durable checkpoint must not masquerade as context_ready");
    assert_eq!(resume.code, FacadeErrorCode::NotFound);
    let diagnose_command = facade
        .dispatch(
            PermissionMode::Full,
            "agent_workflow",
            json!({
                "action":"diagnose",
                "commands":[{"command":"echo diagnose","shell":"cmd"}]
            }),
            None,
        )
        .unwrap();
    assert_eq!(diagnose_command["isError"], false, "{diagnose_command:#?}");
    assert_eq!(
        diagnose_command["structuredContent"]["data"]["state"],
        "completed"
    );
    for (action, arguments) in [
        ("inspect", json!({"action":"inspect","path":"doc.txt"})),
        (
            "search",
            json!({"action":"search","path":"doc.txt","query":"old"}),
        ),
        (
            "create",
            json!({"action":"create","path":"new.txt","content":"new"}),
        ),
        (
            "edit",
            json!({"action":"edit","path":"doc.txt","expected_sha256":"a".repeat(64),"edits":[{"operation":"replace","block_id":"block-1","content":"new"}]}),
        ),
        (
            "convert",
            json!({"action":"convert","source":"doc.txt","path":"copy.txt"}),
        ),
        (
            "rebuild",
            json!({"action":"rebuild","path":"doc.txt","content":"rebuilt","expected_sha256":"a".repeat(64)}),
        ),
    ] {
        let result = facade
            .dispatch(PermissionMode::Full, "document_workflow", arguments, None)
            .unwrap();
        assert_eq!(result["isError"], false, "action={action}: {result:#?}");
    }
}

#[test]
fn session_handles_are_opaque_terminal_monotonic_and_runtime_loss_is_stable() {
    let task_state = test_task_state("terminal-monotonic");
    let mut sessions = PublicCommandSessions::default();
    let public = bind_test_session(&mut sessions, &task_state, "PRIVATE_SESSION");
    assert!(public.starts_with("lb-session-"));
    assert_ne!(public, "PRIVATE_SESSION");
    let completed = stable_success(json!({"status":"completed"}), "done");
    sessions
        .mark_terminal(&public, completed.clone(), &task_state)
        .unwrap();
    sessions
        .mark_terminal(
            &public,
            stable_command_error(FacadeErrorCode::ProcessFailed, "failed", Map::new()),
            &task_state,
        )
        .unwrap();
    let terminal = task_state
        .execution_for_public_session(&PublicSessionId::new(public.clone()))
        .expect("completed execution remains authoritative");
    assert!(matches!(
        terminal.state,
        ExecutionState::Terminal(ExecutionTerminal {
            outcome: TerminalOutcome::Completed,
            ..
        })
    ));

    let lost = bind_test_session(&mut sessions, &task_state, "PRIVATE_LOST");
    sessions.mark_all_running_lost(&task_state).unwrap();
    assert!(task_state.running().is_empty());
    let terminal = task_state
        .execution_for_public_session(&PublicSessionId::new(lost))
        .expect("lost execution remains authoritative");
    assert!(matches!(
        terminal.state,
        ExecutionState::Terminal(ExecutionTerminal {
            outcome: TerminalOutcome::Lost,
            ..
        })
    ));

    let cancelled = bind_test_session(&mut sessions, &task_state, "PRIVATE_CANCELLED");
    task_state
        .request_cancellation(&PublicSessionId::new(cancelled.clone()), "KILL")
        .expect("record cancellation before touching the runtime");
    sessions
        .mark_error_terminal(&cancelled, &session_unavailable(), &task_state)
        .expect("runtime disappearance settles through the execution owner");
    let terminal = task_state
        .execution_for_public_session(&PublicSessionId::new(cancelled))
        .expect("cancelled execution remains authoritative");
    assert!(matches!(
        terminal.state,
        ExecutionState::Terminal(ExecutionTerminal {
            outcome: TerminalOutcome::Cancelled,
            error_code: Some(ref code),
            ..
        }) if code == "ProcessCancelled"
    ));
}

#[test]
fn public_session_pending_output_is_incremental_and_terminal_does_not_replay() {
    let task_state = test_task_state("incremental");
    let mut sessions = PublicCommandSessions::default();
    let public = bind_test_session(&mut sessions, &task_state, "PRIVATE_INCREMENTAL");
    sessions.append_pending(&public, "poll-1\n");
    let first = sessions.running_with_pending(&public).unwrap();
    assert_eq!(first["structuredContent"]["data"]["output"], "poll-1\n");
    assert!(sessions.running_with_pending(&public).is_none());

    sessions.append_pending(&public, "poll-2\n");
    let second = sessions.running_with_pending(&public).unwrap();
    assert_eq!(second["structuredContent"]["data"]["output"], "poll-2\n");
    assert!(sessions.running_with_pending(&public).is_none());

    sessions.append_pending(&public, "tail\n");
    let terminal_response = stable_command_error(
        FacadeErrorCode::ProcessCancelled,
        "命令已取消",
        Map::from_iter([
            ("status".into(), Value::String("cancelled".into())),
            ("output".into(), Value::String("private-final".into())),
        ]),
    );
    sessions
        .mark_terminal(&public, terminal_response.clone(), &task_state)
        .unwrap();
    let terminal = sessions.terminal_with_pending(&public, terminal_response.clone());
    assert_eq!(terminal["structuredContent"]["data"]["output"], "tail\n");
    let replay = sessions.terminal_with_pending(&public, terminal_response);
    assert_eq!(replay["structuredContent"]["data"]["output"], "");
    assert_eq!(
        replay["structuredContent"]["error"]["code"],
        "ProcessCancelled"
    );
}

#[test]
fn durable_terminal_output_handles_retain_their_stream_identity() {
    let task_state = test_task_state("terminal-output-streams");
    let mut sessions = PublicCommandSessions::default();
    let public = bind_test_session(&mut sessions, &task_state, "PRIVATE_OUTPUT_STREAMS");
    let raw = json!({
        "output_ref":"private-stderr",
        "output_stream":"stderr",
        "output_refs":{"stdout":"private-stdout","stderr":"private-stderr"}
    });
    let primary_stream = primary_output_stream(raw.as_object().unwrap(), "private-stderr");
    assert_eq!(primary_stream, "stderr");
    let primary = sessions.public_output_for_private("private-stderr", &public, primary_stream);
    let stdout = sessions.public_output_for_private("private-stdout", &public, "stdout");
    let stderr = sessions.public_output_for_private("private-stderr", &public, "stderr");
    let refs = sessions.output_refs_by_stream(&[stdout.clone(), stderr.clone()]);

    assert_eq!(refs["stdout"], stdout);
    assert_eq!(refs["stderr"], stderr);
    assert_eq!(primary, stderr);
}

#[test]
fn stable_runtime_and_session_errors_persist_as_lost_terminal_snapshots() {
    for error in [
        session_unavailable(),
        FacadeError::new(
            FacadeErrorCode::RuntimeUnavailable,
            "runtime unavailable",
            true,
        ),
        FacadeError::new(
            FacadeErrorCode::RuntimeProtocolMismatch,
            "protocol mismatch",
            false,
        ),
    ] {
        let terminal = execution_terminal_from_result(&error.to_mcp_result());
        assert_eq!(terminal.outcome, TerminalOutcome::Lost);
    }
}

#[test]
fn private_result_semantic_validators_fail_closed_on_consumed_field_drift() {
    let command = json!({
        "structuredContent":{
            "session_id":"private-session",
            "status":"running",
            "stdout":"",
            "stderr":"",
            "timed_out":false,
            "truncated":false,
            "exit_code":null,
            "output_ref":"private-output",
            "output_refs":{"stdout":"private-stdout","stderr":"private-stderr"}
        }
    });
    assert!(validate_private_command_result_semantics(&command, true).is_ok());
    for mut drift in [command.clone(), command.clone(), command.clone()] {
        if drift["structuredContent"]["status"] == "running" {
            drift["structuredContent"]["status"] = Value::from(7);
        } else {
            unreachable!();
        }
        assert_eq!(
            validate_private_command_result_semantics(&drift, true)
                .unwrap_err()
                .code,
            FacadeErrorCode::RuntimeCapabilityMismatch
        );
    }
    let mut missing_refs = command.clone();
    missing_refs["structuredContent"]
        .as_object_mut()
        .unwrap()
        .remove("output_refs");
    assert_eq!(
        validate_private_command_result_semantics(&missing_refs, true)
            .unwrap_err()
            .code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );

    let read = json!({"structuredContent":{
        "output_ref":"private-output","stream":"stdout","offset":0,
        "requested_offset":0,"limit":4096,"content":"probe","next_offset":5,"truncated":false
    }});
    assert!(validate_private_read_output_semantics(&read).is_ok());
    let mut bad_read = read;
    bad_read["structuredContent"]["next_offset"] = Value::String("five".into());
    assert_eq!(
        validate_private_read_output_semantics(&bad_read)
            .unwrap_err()
            .code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );
}

#[test]
fn command_exit_status_and_workspace_probe_fail_closed() {
    assert_eq!(command_public_status("exited", Some(0), false), "completed");
    assert_eq!(command_public_status("exited", Some(7), false), "failed");
    assert_eq!(command_public_status("killed", Some(7), false), "cancelled");
    assert_eq!(command_public_status("running", None, false), "running");
    assert_eq!(command_public_status("exited", None, true), "timed_out");

    let expected = PathBuf::from(r"C:\workspace-a");
    let matching = json!({
        "structuredContent":{"workspace":r"C:\workspace-a","default_cwd":"."}
    });
    let mismatch = json!({
        "structuredContent":{"workspace":r"C:\workspace-b","default_cwd":"."}
    });
    assert!(validate_workspace_context_probe(&matching, &expected).is_ok());
    assert_eq!(
        validate_workspace_context_probe(&mismatch, &expected)
            .unwrap_err()
            .code,
        FacadeErrorCode::RuntimeCapabilityMismatch
    );
}

#[cfg(windows)]
#[test]
fn workspace_probe_accepts_ntfs_short_path_alias_for_same_directory_object() {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::time::{SystemTime, UNIX_EPOCH};
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

    fn short_path(path: &Path) -> Option<PathBuf> {
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let needed = unsafe { GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0) };
        if needed == 0 {
            return None;
        }
        let mut buffer = vec![0u16; needed as usize];
        let written =
            unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) };
        if written == 0 {
            return None;
        }
        buffer.truncate(written as usize);
        Some(PathBuf::from(OsString::from_wide(&buffer)))
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "LocalBridge Workspace Identity Alias Probe {} {nonce}",
        std::process::id()
    ));
    let other = root.with_extension("other");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&other).unwrap();

    if let Some(short) = short_path(&root) {
        if short != root {
            assert!(ordinary_workspace_paths_match(
                &root,
                &short.to_string_lossy()
            ));
            assert!(!ordinary_workspace_paths_match(
                &other,
                &short.to_string_lossy()
            ));
        }
    }

    std::fs::remove_dir_all(&root).unwrap();
    std::fs::remove_dir_all(&other).unwrap();
}

#[test]
fn public_workspace_paths_are_target_bound() {
    for denied in [
        r"C:\absolute.txt",
        r"\\server\share\file.txt",
        r"\\?\C:\verbatim.txt",
        "/posix/absolute",
        "../escape.txt",
        "sub/../../escape.txt",
        "file.txt:ads",
    ] {
        assert!(!workspace_relative_path_valid(denied), "{denied}");
    }
    for allowed in [".", "file.txt", "sub/file.txt", r"sub\file.txt"] {
        assert!(workspace_relative_path_valid(allowed), "{allowed}");
    }

    assert!(public_patch_targets_valid(
        "*** Begin Patch\n*** Update File: safe/doc.txt\n@@\n-old\n+new\n*** End Patch"
    ));
    assert!(public_patch_targets_valid(
        "*** Begin Patch\n*** Update File: safe/doc.txt\n*** Move to: safe/moved.txt\n@@\n-old\n+new\n*** End Patch"
    ));
    for denied in [
        "*** Begin Patch\n*** Update File: ../escape.txt\n@@\n-old\n+new\n*** End Patch",
        "*** Begin Patch\n*** Add File: C:\\escape.txt\n+x\n*** End Patch",
        "*** Begin Patch\n*** Delete File: \\\\server\\share\\escape.txt\n*** End Patch",
        "*** Begin Patch\n*** Update File: \\\\?\\C:\\escape.txt\n@@\n-old\n+new\n*** End Patch",
        "*** Begin Patch\n*** Update File: safe.txt:ads\n@@\n-old\n+new\n*** End Patch",
        "*** Begin Patch\n*** Update File: safe.txt\n*** Move to: ../escape.txt\n@@\n-old\n+new\n*** End Patch",
        "*** Begin Patch\n*** Unknown File: safe.txt\n*** End Patch",
    ] {
        assert!(!public_patch_targets_valid(denied), "{denied}");
    }
}

#[test]
fn ordinary_commands_are_not_classified_from_argument_substrings() {
    for command in [
        "Write-Output test",
        "Get-ChildItem D:\\project\\test",
        "echo build-result",
    ] {
        assert_eq!(
            public_task_kind("exec_command", &json!({"command":command})),
            TaskKind::ExecuteCommand,
            "{command}"
        );
    }
    assert_eq!(
        public_task_kind("exec_command", &json!({"command":"cargo test --workspace"})),
        TaskKind::Test
    );
    assert_eq!(
        public_task_kind("exec_command", &json!({"command":"npm run build"})),
        TaskKind::Build
    );
}

#[test]
fn transport_timeout_never_terminalizes_a_command_that_is_still_running() {
    // 传输预算耗尽只说明我们没等到回答，不说明命令死了。上一版三个调用方里
    // 只有 command_control 记得这一点，于是 exec 和后台回收都会把活着的进程
    // 标成失败，它的输出从此取不回来。规则现在归 mark_error_terminal 所有。
    let task_state = test_task_state("timeout-not-terminal");
    let mut sessions = PublicCommandSessions::default();
    let public = bind_test_session(&mut sessions, &task_state, "PRIVATE_TIMEOUT");

    let timed_out = normalize_runtime_error(CodingToolsRuntimeError::RequestTimeout);
    assert_eq!(timed_out.code, FacadeErrorCode::OperationTimedOut);
    sessions
        .mark_error_terminal(&public, &timed_out, &task_state)
        .expect("a transport timeout is not an error to record");

    let execution = task_state
        .execution_for_public_session(&PublicSessionId::new(public.clone()))
        .expect("the execution survives the timeout");
    assert!(
        matches!(execution.state, ExecutionState::Running),
        "a timed-out transport left the execution as {:?}",
        execution.state
    );

    // 而真正的失败仍然照常落地，否则上面那条就变成了"永不终结"。
    sessions
        .mark_error_terminal(&public, &session_unavailable(), &task_state)
        .expect("a real failure still settles");
    let execution = task_state
        .execution_for_public_session(&PublicSessionId::new(public))
        .expect("the execution remains authoritative");
    assert!(matches!(execution.state, ExecutionState::Terminal(_)));
}

#[test]
fn a_timed_out_command_hands_back_a_session_id_to_poll_with() {
    // "稍后 poll 以观察同一 Execution"这句补救建议，在载荷里没有 session_id
    // 的时候是空头支票——模型手上没有任何可 poll 的句柄。
    let error = normalize_runtime_error(CodingToolsRuntimeError::RequestTimeout)
        .with_message("命令已超出 yield_time_ms 传输预算，仍在后台运行")
        .with_details(json!({"session_id": "PUBLIC_ABC"}));
    let payload = error.to_mcp_result();
    let error_object = &payload["structuredContent"]["error"];

    assert_eq!(error_object["code"], "OperationTimedOut");
    assert_eq!(error_object["details"]["session_id"], "PUBLIC_ABC");
    assert_eq!(error_object["retryable"], true);
    // 契约要求错误响应的 data 为 null，所以 session_id 只能落在 details 里。
    assert!(payload["structuredContent"]["data"].is_null());
}
