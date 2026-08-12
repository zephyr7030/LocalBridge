use std::fmt;

use serde_json::Value;

use crate::state::{
    Capability, CurrentTaskStatus, PermissionMode, SafeTaskSummary, TaskExecutionState, TaskKind,
};

use super::policy::{CapabilityPolicy, DenyReason, PolicyDecision, ToolDescriptor};
use super::runtime::{CodingToolsRuntime, CodingToolsRuntimeError};

pub trait GuardRuntime {
    fn raw_list_tools(&mut self) -> Result<Value, CodingToolsRuntimeError>;
    fn raw_call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, CodingToolsRuntimeError>;
}

impl GuardRuntime for CodingToolsRuntime {
    fn raw_list_tools(&mut self) -> Result<Value, CodingToolsRuntimeError> {
        self.list_tools()
    }

    fn raw_call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, CodingToolsRuntimeError> {
        self.call_tool(name, arguments)
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallRequest {
    pub name: String,
    pub arguments: Value,
    pub indirect_capabilities: Vec<Capability>,
}

impl ToolCallRequest {
    pub fn new(name: impl Into<String>, arguments: Value) -> Self {
        Self { name: name.into(), arguments, indirect_capabilities: Vec::new() }
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
            Self::Denied(denied) => write!(f, "MCP call denied by LocalBridge policy: {:?}", denied.reason),
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
                .is_some_and(|name| self.policy.tool_allowed_for_list(mode, name))
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
        let decision = self
            .policy
            .decide(mode, &request.name, &indirect_capabilities);
        let kind = refined_task_kind(decision.descriptor, &request.arguments);
        let summary = safe_summary(&request.name, &request.arguments);
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

        project(CurrentTaskStatus::project(kind, summary, TaskExecutionState::Running)
            .expect("Running is a valid active task state"));
        match self.runtime.raw_call_tool(&request.name, request.arguments) {
            Ok(result) => {
                project(CurrentTaskStatus::Idle);
                Ok(result)
            }
            Err(error) => {
                project(CurrentTaskStatus::project(kind, SafeTaskSummary::Omitted, TaskExecutionState::Failed)
                    .expect("Failed is a valid active task state"));
                project(CurrentTaskStatus::Idle);
                Err(GuardError::Runtime(error))
            }
        }
    }

    pub fn decision(&self, mode: PermissionMode, request: &ToolCallRequest) -> PolicyDecision {
        let indirect_capabilities = effective_indirect_capabilities(request);
        self.policy
            .decide(mode, &request.name, &indirect_capabilities)
    }
}

fn effective_indirect_capabilities(request: &ToolCallRequest) -> Vec<Capability> {
    let mut capabilities = request.indirect_capabilities.clone();
    if request.name == "exec_command"
        && string_argument(&request.arguments, &["cmd", "command"])
            .as_deref()
            .is_some_and(contains_privileged_external_runtime)
    {
        capabilities.push(Capability::PrivilegedExternalRuntime);
    }
    capabilities
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
        "write_stdin" | "kill_session" | "read_output" => string_argument(arguments, &["session_id", "session"]),
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

fn refined_task_kind(descriptor: ToolDescriptor, arguments: &Value) -> TaskKind {
    if descriptor.name != "exec_command" {
        return descriptor.task_kind;
    }
    let Some(command) = string_argument(arguments, &["cmd", "command"]) else {
        return TaskKind::ExecuteCommand;
    };
    let lower = command.to_ascii_lowercase();
    if lower.contains("cargo test") || lower.contains("npm test") || lower.contains("vitest") || lower.contains("pytest") {
        TaskKind::Test
    } else if lower.contains("cargo build") || lower.contains("npm run build") || lower.contains("tauri build") {
        TaskKind::Build
    } else {
        TaskKind::ExecuteCommand
    }
}

fn contains_privileged_external_runtime(command: &str) -> bool {
    command
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')))
        .any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "docker" | "docker.exe" | "podman" | "podman.exe" | "wsl" | "wsl.exe"
            )
        })
}
