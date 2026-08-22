use std::fmt;

use serde_json::Value;

use crate::state::{
    Capability, CurrentTaskStatus, PermissionMode, SafeTaskSummary, TaskExecutionState, TaskKind,
};

use super::runtime::{CodingToolsRuntime, CodingToolsRuntimeError};
use crate::execution::policy::{
    CapabilityPolicy, DenyReason, PolicyDecision, ToolDescriptor, command_task_kind,
};

pub trait GuardRuntime {
    fn raw_list_tools(&mut self) -> Result<Value, CodingToolsRuntimeError>;
    fn raw_call_tool(
        &mut self,
        name: &str,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, CodingToolsRuntimeError>;

    fn root_is_running(&self) -> Result<Option<bool>, CodingToolsRuntimeError> {
        Ok(None)
    }
}

impl GuardRuntime for CodingToolsRuntime {
    fn raw_list_tools(&mut self) -> Result<Value, CodingToolsRuntimeError> {
        self.list_tools()
    }

    fn raw_call_tool(
        &mut self,
        name: &str,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, CodingToolsRuntimeError> {
        self.call_tool_with_request_id(name, arguments, request_id)
    }

    fn root_is_running(&self) -> Result<Option<bool>, CodingToolsRuntimeError> {
        CodingToolsRuntime::root_is_running(self).map(Some)
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallRequest {
    pub name: String,
    pub arguments: Value,
    pub indirect_capabilities: Vec<Capability>,
    request_id: Option<Value>,
}

impl ToolCallRequest {
    pub fn new(name: impl Into<String>, arguments: Value) -> Self {
        Self {
            name: name.into(),
            arguments,
            indirect_capabilities: Vec::new(),
            request_id: None,
        }
    }

    pub fn with_indirect_capabilities<I>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = Capability>,
    {
        self.indirect_capabilities = capabilities.into_iter().collect();
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDenied {
    pub reason: DenyReason,
    pub capability: Capability,
}

#[derive(Debug)]
pub enum GuardError {
    Denied(PolicyDenied),
    Runtime(CodingToolsRuntimeError),
    MalformedToolsList,
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied(denied) => write!(
                f,
                "MCP call denied by LocalBridge policy: {:?}",
                denied.reason
            ),
            Self::Runtime(error) => write!(f, "MCP runtime call failed: {error}"),
            Self::MalformedToolsList => f.write_str("upstream tools/list response is malformed"),
        }
    }
}

impl std::error::Error for GuardError {}

pub struct McpGuard<R> {
    runtime: R,
    policy: CapabilityPolicy,
}

impl<R: GuardRuntime> McpGuard<R> {
    pub fn new(runtime: R, policy: CapabilityPolicy) -> Self {
        Self { runtime, policy }
    }

    pub fn filtered_tools(&mut self, mode: PermissionMode) -> Result<Value, GuardError> {
        let mut response = self.runtime.raw_list_tools().map_err(GuardError::Runtime)?;
        let tools = response
            .get_mut("tools")
            .and_then(Value::as_array_mut)
            .ok_or(GuardError::MalformedToolsList)?;
        tools.retain(|tool| {
            tool.get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| {
                    name != "elevated_exec" && self.policy.tool_allowed_for_list(mode, name)
                })
        });
        Ok(response)
    }

    pub fn call_tool<F>(
        &mut self,
        mode: PermissionMode,
        request: ToolCallRequest,
        mut project: F,
    ) -> Result<Value, GuardError>
    where
        F: FnMut(CurrentTaskStatus),
    {
        let indirect_capabilities = effective_indirect_capabilities(&request);
        let decision = self.policy.decide_request(
            mode,
            &request.name,
            &indirect_capabilities,
            &request.arguments,
        );
        let kind = refined_task_kind(decision.descriptor, &request.arguments);
        let summary = safe_summary(&request.name, &request.arguments);
        if has_verbatim_execution_path(&request) {
            let blocked = CurrentTaskStatus::project(kind, summary, TaskExecutionState::Blocked)
                .expect("Blocked is a valid active task state");
            project(blocked);
            project(CurrentTaskStatus::Idle);
            return Err(GuardError::Denied(PolicyDenied {
                reason: DenyReason::VerbatimExecutionPath,
                capability: decision.descriptor.capability,
            }));
        }
        if decision.descriptor.capability == Capability::ElevatedExec {
            let blocked = CurrentTaskStatus::project(kind, summary, TaskExecutionState::Blocked)
                .expect("Blocked is a valid active task state");
            project(blocked);
            project(CurrentTaskStatus::Idle);
            return Err(GuardError::Denied(PolicyDenied {
                reason: DenyReason::PrivilegedRouteNotAvailable,
                capability: Capability::ElevatedExec,
            }));
        }
        if !decision.allowed {
            let blocked = CurrentTaskStatus::project(kind, summary, TaskExecutionState::Blocked)
                .expect("Blocked is a valid active task state");
            project(blocked);
            project(CurrentTaskStatus::Idle);
            return Err(GuardError::Denied(PolicyDenied {
                reason: decision.deny_reason.expect("denied decision has a reason"),
                capability: decision.descriptor.capability,
            }));
        }
        project(
            CurrentTaskStatus::project(kind, summary, TaskExecutionState::Running)
                .expect("Running is a valid active task state"),
        );
        match self.runtime.raw_call_tool(
            &request.name,
            request.arguments,
            request.request_id.as_ref(),
        ) {
            Ok(result) => {
                project(CurrentTaskStatus::Idle);
                Ok(result)
            }
            Err(error) => {
                project(
                    CurrentTaskStatus::project(
                        kind,
                        SafeTaskSummary::Omitted,
                        TaskExecutionState::Failed,
                    )
                    .expect("Failed is a valid active task state"),
                );
                project(CurrentTaskStatus::Idle);
                Err(GuardError::Runtime(error))
            }
        }
    }

    pub fn decision(&self, mode: PermissionMode, request: &ToolCallRequest) -> PolicyDecision {
        let indirect_capabilities = effective_indirect_capabilities(request);
        let decision = decide_actual_request(&self.policy, mode, request, indirect_capabilities);
        if has_verbatim_execution_path(request) {
            return PolicyDecision {
                descriptor: decision.descriptor,
                allowed: false,
                deny_reason: Some(DenyReason::VerbatimExecutionPath),
            };
        }
        decision
    }

    pub fn privileged_tool_visible(&self, mode: PermissionMode, name: &str) -> bool {
        self.policy.privileged_tool_visible(mode, name)
    }

    pub fn runtime_root_is_running(&self) -> Result<Option<bool>, CodingToolsRuntimeError> {
        self.runtime.root_is_running()
    }
}

// ARCH-026 audits this exact actual-arguments call as a stable machine-readable seam.
#[rustfmt::skip]
fn decide_actual_request(
    policy: &CapabilityPolicy,
    mode: PermissionMode,
    request: &ToolCallRequest,
    indirect_capabilities: Vec<Capability>,
) -> PolicyDecision {
    policy.decide_request(mode, &request.name, &indirect_capabilities, &request.arguments)
}

fn effective_indirect_capabilities(request: &ToolCallRequest) -> Vec<Capability> {
    request.indirect_capabilities.clone()
}

fn safe_summary(name: &str, arguments: &Value) -> SafeTaskSummary {
    let candidate = match name {
        "read_file" | "list_dir" | "list_files" | "view_image" | "set_default_cwd" => {
            string_argument(arguments, &["path", "cwd"])
        }
        "search_text" => string_argument(arguments, &["query", "pattern"]),
        "exec_command" => string_argument(arguments, &["cmd", "command"]),
        "git_status" => Some("git status".to_string()),
        "git_diff" => Some("git diff".to_string()),
        "git_log" => Some("git log".to_string()),
        "git_show" => Some("git show".to_string()),
        "git_blame" => Some("git blame".to_string()),
        "apply_patch" => Some("修改项目文件".to_string()),
        "write_stdin" | "kill_session" | "read_output" => {
            string_argument(arguments, &["session_id", "session"])
        }
        "server_info" => Some("运行环境信息".to_string()),
        "check_exec_environment" => Some("检查执行环境".to_string()),
        "get_default_cwd" => Some("当前工作目录".to_string()),
        _ => None,
    };
    candidate
        .as_deref()
        .map(SafeTaskSummary::from_untrusted)
        .unwrap_or(SafeTaskSummary::Omitted)
}

fn string_argument(arguments: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| arguments.get(*key).and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

fn has_verbatim_execution_path(request: &ToolCallRequest) -> bool {
    let Some(arguments) = request.arguments.as_object() else {
        return false;
    };
    let path_keys: &[&str] = match request.name.as_str() {
        "set_default_cwd" | "read_file" | "list_dir" | "list_files" | "search_text"
        | "view_image" | "git_status" | "git_log" | "git_blame" => &["path"],
        "git_diff" | "git_show" => &["path", "paths"],
        "exec_command" => &["cwd", "workdir"],
        _ => &[],
    };
    if path_keys
        .iter()
        .filter_map(|key| arguments.get(*key))
        .any(value_has_verbatim_path)
    {
        return true;
    }
    match request.name.as_str() {
        "exec_command" => {
            ["cmd", "stdin"]
                .iter()
                .filter_map(|key| arguments.get(*key).and_then(Value::as_str))
                .any(contains_verbatim_path_text)
                || arguments
                    .get("env")
                    .and_then(Value::as_object)
                    .is_some_and(|env| {
                        env.values()
                            .filter_map(Value::as_str)
                            .any(contains_verbatim_path_text)
                    })
        }
        "write_stdin" => arguments
            .get("chars")
            .and_then(Value::as_str)
            .is_some_and(contains_verbatim_path_text),
        _ => false,
    }
}

fn value_has_verbatim_path(value: &Value) -> bool {
    match value {
        Value::String(value) => starts_with_verbatim_path(value),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .any(starts_with_verbatim_path),
        _ => false,
    }
}

fn starts_with_verbatim_path(value: &str) -> bool {
    value.starts_with(r"\\?\") || value.starts_with("//?/")
}

fn contains_verbatim_path_text(value: &str) -> bool {
    value.contains(r"\\?\") || value.contains("//?/")
}

fn refined_task_kind(descriptor: ToolDescriptor, arguments: &Value) -> TaskKind {
    if descriptor.name != "exec_command" {
        return descriptor.task_kind;
    }
    let Some(command) = string_argument(arguments, &["cmd", "command"]) else {
        return TaskKind::ExecuteCommand;
    };
    command_task_kind(&command)
}
