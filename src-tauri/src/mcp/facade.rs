use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::state::{
    Capability, CurrentTaskStatus, PermissionMode, SafeTaskSummary, TaskExecutionState, TaskKind,
};

use super::http::McpCancellationClient;
use super::policy::{CapabilityPolicy, DenyReason, PolicyDecision};
use super::runtime::{CodingToolsRuntime, CodingToolsRuntimeError};
use super::shell::{ShellExecutionSpec, ShellExecutor, ShellResolveError, ShellSelector};

pub const AGENT_API_VERSION: u32 = 1;
pub const V1_CORE_TOOL_NAMES: [&str; 8] = [
    "workspace_context",
    "agent_workflow",
    "exec_command",
    "command_control",
    "task_control",
    "git_workflow",
    "document_workflow",
    "view_image",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacadeErrorCode {
    InvalidArgument,
    NotFound,
    WorkspaceDenied,
    CapabilityDenied,
    ProcessFailed,
    ProcessTimedOut,
    ProcessCancelled,
    OutputTruncated,
    RuntimeUnavailable,
    RuntimeProtocolMismatch,
    RuntimeCapabilityMismatch,
    Internal,
}

impl FacadeErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArgument => "InvalidArgument",
            Self::NotFound => "NotFound",
            Self::WorkspaceDenied => "WorkspaceDenied",
            Self::CapabilityDenied => "CapabilityDenied",
            Self::ProcessFailed => "ProcessFailed",
            Self::ProcessTimedOut => "ProcessTimedOut",
            Self::ProcessCancelled => "ProcessCancelled",
            Self::OutputTruncated => "OutputTruncated",
            Self::RuntimeUnavailable => "RuntimeUnavailable",
            Self::RuntimeProtocolMismatch => "RuntimeProtocolMismatch",
            Self::RuntimeCapabilityMismatch => "RuntimeCapabilityMismatch",
            Self::Internal => "Internal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacadeError {
    pub code: FacadeErrorCode,
    pub message: &'static str,
    pub retryable: bool,
}

impl FacadeError {
    pub const fn new(code: FacadeErrorCode, message: &'static str, retryable: bool) -> Self {
        Self {
            code,
            message,
            retryable,
        }
    }

    pub fn to_mcp_result(&self) -> Value {
        json!({
            "content": [{"type":"text","text":self.message}],
            "structuredContent": {
                "ok": false,
                "error": {
                    "code": self.code.as_str(),
                    "message": self.message,
                    "retryable": self.retryable
                }
            },
            "isError": true
        })
    }
}

impl fmt::Display for FacadeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for FacadeError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FacadeDenied {
    pub reason: DenyReason,
    pub capability: Capability,
}

#[derive(Debug)]
pub enum FacadeCallError {
    Denied(FacadeDenied),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ToolRegistry;

impl ToolRegistry {
    pub const fn version(&self) -> u32 {
        AGENT_API_VERSION
    }

    pub fn contains(&self, name: &str) -> bool {
        V1_CORE_TOOL_NAMES.contains(&name)
    }

    pub fn core_tools(&self) -> Vec<Value> {
        V1_CORE_TOOL_NAMES
            .iter()
            .map(|name| public_tool_schema(name))
            .collect()
    }
}

fn public_tool_schema(name: &str) -> Value {
    let (description, input_schema) = match name {
        "workspace_context" => (
            "Return stable LocalBridge workspace/runtime context.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        "agent_workflow" => (
            "Run a LocalBridge engineering workflow using stable actions.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":["diagnose","bugfix","feature","refactor","test_failure","build_release","document","resume","custom"]},
                    "objective":{"type":"string"}
                },
                "required":["action"],
                "additionalProperties":false
            }),
        ),
        "exec_command" => (
            "Execute a command through a LocalBridge trusted logical shell selector.",
            json!({
                "type":"object",
                "properties":{
                    "command":{"type":"string","minLength":1},
                    "shell":{"type":"string","enum":["auto","powershell","pwsh","windows_powershell","cmd"],"default":"auto"},
                    "workdir":{"type":"string"},
                    "timeout_ms":{"type":"integer","minimum":1,"maximum":600000,"default":30000},
                    "yield_time_ms":{"type":"integer","minimum":0,"maximum":30000,"default":10000},
                    "max_output_bytes":{"type":"integer","minimum":1,"maximum":1048576,"default":65536},
                    "stdin":{"type":"string","default":""}
                },
                "required":["command"],
                "additionalProperties":false
            }),
        ),
        "command_control" => (
            "Read, write, poll, or terminate an existing LocalBridge command session.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":["poll","read","write","kill"]},
                    "session_id":{"type":"string"},
                    "output_ref":{"type":"string"},
                    "stream":{"type":"string","enum":["stdout","stderr"]},
                    "offset":{"type":"integer","minimum":0},
                    "limit":{"type":"integer","minimum":1,"maximum":1048576},
                    "chars":{"type":"string"},
                    "signal":{"type":"string","enum":["TERM","KILL","INT"]},
                    "wait_ms":{"type":"integer","minimum":0,"maximum":30000}
                },
                "required":["action"],
                "additionalProperties":false
            }),
        ),
        "task_control" => (
            "Control the current LocalBridge task using stable task actions.",
            json!({
                "type":"object",
                "properties":{"action":{"type":"string","enum":["get","cancel"]}},
                "required":["action"],
                "additionalProperties":false
            }),
        ),
        "git_workflow" => (
            "Run a stable LocalBridge Git workflow action.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":["status","diff","log","show","blame"]},
                    "path":{"type":"string"},
                    "paths":{"type":"array","items":{"type":"string"}},
                    "ref":{"type":"string"},
                    "rev":{"type":"string"},
                    "staged":{"type":"boolean"},
                    "unstaged":{"type":"boolean"},
                    "include_untracked":{"type":"boolean"},
                    "max_entries":{"type":"integer","minimum":1},
                    "max_count":{"type":"integer","minimum":1},
                    "skip":{"type":"integer","minimum":0},
                    "start_line":{"type":"integer","minimum":1},
                    "end_line":{"type":"integer","minimum":1},
                    "max_lines":{"type":"integer","minimum":1},
                    "context_lines":{"type":"integer","minimum":0},
                    "max_bytes":{"type":"integer","minimum":1}
                },
                "required":["action"],
                "additionalProperties":false
            }),
        ),
        "document_workflow" => (
            "Inspect or mutate workspace documents through a stable LocalBridge workflow.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":["inspect","create","convert","rebuild"]},
                    "path":{"type":"string"},
                    "patch":{"type":"string"},
                    "start_line":{"type":"integer","minimum":1},
                    "end_line":{"type":"integer","minimum":1},
                    "max_lines":{"type":"integer","minimum":1},
                    "max_bytes":{"type":"integer","minimum":1}
                },
                "required":["action"],
                "additionalProperties":false
            }),
        ),
        "view_image" => (
            "Inspect a workspace image through the stable LocalBridge image contract.",
            json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string","minLength":1},
                    "max_bytes":{"type":"integer","minimum":1024,"maximum":10485760},
                    "max_width":{"type":"integer","minimum":1,"maximum":10000},
                    "max_height":{"type":"integer","minimum":1,"maximum":10000},
                    "auto_resize":{"type":"boolean"}
                },
                "required":["path"],
                "additionalProperties":false
            }),
        ),
        _ => unreachable!("registry only requests frozen public tools"),
    };
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateParameterKind {
    String,
    Integer,
    Boolean,
    StringArray,
}

#[derive(Debug, Clone, Copy)]
struct PrivateParameterContract {
    name: &'static str,
    kind: PrivateParameterKind,
    min_length: Option<u64>,
    minimum: Option<i64>,
    maximum: Option<i64>,
    enum_values: &'static [&'static str],
}

#[derive(Debug)]
struct PrivateCapabilityContract {
    name: &'static str,
    required: &'static [&'static str],
    parameters: &'static [PrivateParameterContract],
}

const fn private_parameter(
    name: &'static str,
    kind: PrivateParameterKind,
) -> PrivateParameterContract {
    PrivateParameterContract {
        name,
        kind,
        min_length: None,
        minimum: None,
        maximum: None,
        enum_values: &[],
    }
}

const fn private_string(
    name: &'static str,
    min_length: Option<u64>,
    enum_values: &'static [&'static str],
) -> PrivateParameterContract {
    PrivateParameterContract {
        name,
        kind: PrivateParameterKind::String,
        min_length,
        minimum: None,
        maximum: None,
        enum_values,
    }
}

const fn private_integer(
    name: &'static str,
    minimum: Option<i64>,
    maximum: Option<i64>,
) -> PrivateParameterContract {
    PrivateParameterContract {
        name,
        kind: PrivateParameterKind::Integer,
        min_length: None,
        minimum,
        maximum,
        enum_values: &[],
    }
}

const REQUIRED_PRIVATE_CAPABILITIES: &[PrivateCapabilityContract] = &[
    PrivateCapabilityContract {
        name: "server_info",
        required: &[],
        parameters: &[],
    },
    PrivateCapabilityContract {
        name: "check_exec_environment",
        required: &[],
        parameters: &[],
    },
    PrivateCapabilityContract {
        name: "get_default_cwd",
        required: &[],
        parameters: &[],
    },
    PrivateCapabilityContract {
        name: "set_default_cwd",
        required: &[],
        parameters: &[private_parameter("path", PrivateParameterKind::String)],
    },
    PrivateCapabilityContract {
        name: "read_file",
        required: &["path"],
        parameters: &[
            private_string("path", Some(1), &[]),
            private_integer("start_line", Some(1), None),
            private_integer("end_line", Some(1), None),
            private_integer("max_lines", Some(1), None),
            private_integer("max_bytes", Some(1), Some(1_048_576)),
        ],
    },
    PrivateCapabilityContract {
        name: "list_dir",
        required: &[],
        parameters: &[
            private_parameter("path", PrivateParameterKind::String),
            private_parameter("include_hidden", PrivateParameterKind::Boolean),
            private_parameter("include_ignored", PrivateParameterKind::Boolean),
            private_parameter("max_depth", PrivateParameterKind::Integer),
            private_parameter("max_entries", PrivateParameterKind::Integer),
            private_parameter("recursive", PrivateParameterKind::Boolean),
            private_parameter("sort", PrivateParameterKind::String),
        ],
    },
    PrivateCapabilityContract {
        name: "list_files",
        required: &[],
        parameters: &[
            private_parameter("path", PrivateParameterKind::String),
            private_parameter("patterns", PrivateParameterKind::StringArray),
            private_parameter("glob", PrivateParameterKind::String),
            private_parameter("exclude_patterns", PrivateParameterKind::StringArray),
            private_parameter("include_hidden", PrivateParameterKind::Boolean),
            private_parameter("include_ignored", PrivateParameterKind::Boolean),
            private_parameter("max_results", PrivateParameterKind::Integer),
            private_parameter("sort", PrivateParameterKind::String),
        ],
    },
    PrivateCapabilityContract {
        name: "search_text",
        required: &["query"],
        parameters: &[
            private_string("query", Some(1), &[]),
            private_parameter("path", PrivateParameterKind::String),
            private_parameter("regex", PrivateParameterKind::Boolean),
            private_parameter("case_sensitive", PrivateParameterKind::Boolean),
            private_parameter("include_globs", PrivateParameterKind::StringArray),
            private_parameter("exclude_globs", PrivateParameterKind::StringArray),
            private_parameter("glob", PrivateParameterKind::String),
            private_parameter("context_lines", PrivateParameterKind::Integer),
            private_parameter("max_results", PrivateParameterKind::Integer),
            private_parameter("max_preview_bytes", PrivateParameterKind::Integer),
        ],
    },
    PrivateCapabilityContract {
        name: "apply_patch",
        required: &["patch"],
        parameters: &[
            private_string("patch", Some(1), &[]),
            private_parameter("dry_run", PrivateParameterKind::Boolean),
        ],
    },
    PrivateCapabilityContract {
        name: "exec_command",
        required: &["cmd"],
        parameters: &[
            private_string("cmd", Some(1), &[]),
            private_parameter("workdir", PrivateParameterKind::String),
            private_integer("timeout_ms", Some(1), Some(600_000)),
            private_integer("yield_time_ms", Some(0), Some(30_000)),
            private_integer("max_output_bytes", Some(1), Some(1_048_576)),
            private_parameter("stdin", PrivateParameterKind::String),
        ],
    },
    PrivateCapabilityContract {
        name: "write_stdin",
        required: &["session_id"],
        parameters: &[
            private_string("session_id", Some(1), &[]),
            private_parameter("chars", PrivateParameterKind::String),
            private_integer("yield_time_ms", Some(0), Some(30_000)),
            private_integer("max_output_bytes", Some(1), Some(1_048_576)),
        ],
    },
    PrivateCapabilityContract {
        name: "kill_session",
        required: &["session_id"],
        parameters: &[
            private_string("session_id", Some(1), &[]),
            private_string("signal", None, &["TERM", "KILL", "INT"]),
            private_integer("wait_ms", Some(0), Some(30_000)),
            private_integer("max_output_bytes", Some(1), Some(1_048_576)),
        ],
    },
    PrivateCapabilityContract {
        name: "read_output",
        required: &["output_ref"],
        parameters: &[
            private_string("output_ref", Some(1), &[]),
            private_string("stream", None, &["stdout", "stderr"]),
            private_integer("offset", Some(0), None),
            private_integer("limit", Some(1), Some(1_048_576)),
        ],
    },
    PrivateCapabilityContract {
        name: "git_status",
        required: &[],
        parameters: &[
            private_parameter("path", PrivateParameterKind::String),
            private_parameter("include_untracked", PrivateParameterKind::Boolean),
            private_parameter("max_entries", PrivateParameterKind::Integer),
        ],
    },
    PrivateCapabilityContract {
        name: "git_diff",
        required: &[],
        parameters: &[
            private_parameter("path", PrivateParameterKind::String),
            private_parameter("paths", PrivateParameterKind::StringArray),
            private_parameter("staged", PrivateParameterKind::Boolean),
            private_parameter("unstaged", PrivateParameterKind::Boolean),
            private_parameter("context_lines", PrivateParameterKind::Integer),
            private_parameter("max_bytes", PrivateParameterKind::Integer),
        ],
    },
    PrivateCapabilityContract {
        name: "git_log",
        required: &[],
        parameters: &[
            private_parameter("path", PrivateParameterKind::String),
            private_parameter("ref", PrivateParameterKind::String),
            private_parameter("max_count", PrivateParameterKind::Integer),
            private_parameter("skip", PrivateParameterKind::Integer),
        ],
    },
    PrivateCapabilityContract {
        name: "git_show",
        required: &[],
        parameters: &[
            private_parameter("rev", PrivateParameterKind::String),
            private_parameter("path", PrivateParameterKind::String),
            private_parameter("paths", PrivateParameterKind::StringArray),
            private_parameter("context_lines", PrivateParameterKind::Integer),
            private_parameter("max_bytes", PrivateParameterKind::Integer),
        ],
    },
    PrivateCapabilityContract {
        name: "git_blame",
        required: &["path"],
        parameters: &[
            private_parameter("path", PrivateParameterKind::String),
            private_parameter("rev", PrivateParameterKind::String),
            private_parameter("start_line", PrivateParameterKind::Integer),
            private_parameter("end_line", PrivateParameterKind::Integer),
            private_parameter("max_lines", PrivateParameterKind::Integer),
        ],
    },
    PrivateCapabilityContract {
        name: "view_image",
        required: &["path"],
        parameters: &[
            private_string("path", Some(1), &[]),
            private_integer("max_bytes", Some(1_024), Some(10_485_760)),
            private_integer("max_width", Some(1), Some(10_000)),
            private_integer("max_height", Some(1), Some(10_000)),
            private_parameter("auto_resize", PrivateParameterKind::Boolean),
        ],
    },
];

pub fn validate_runtime_capabilities(catalog: &Value) -> Result<(), FacadeError> {
    let tools = catalog
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(runtime_capability_mismatch)?;
    let by_name = tools
        .iter()
        .filter_map(|tool| Some((tool.get("name")?.as_str()?.to_owned(), tool)))
        .collect::<HashMap<_, _>>();
    for contract in REQUIRED_PRIVATE_CAPABILITIES {
        let tool = by_name
            .get(contract.name)
            .copied()
            .ok_or_else(runtime_capability_mismatch)?;
        let schema = tool
            .get("inputSchema")
            .and_then(Value::as_object)
            .ok_or_else(runtime_capability_mismatch)?;
        if schema.get("type").and_then(Value::as_str) != Some("object") {
            return Err(runtime_capability_mismatch());
        }
        let schema_properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .ok_or_else(runtime_capability_mismatch)?;
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .ok_or_else(runtime_capability_mismatch)?;
        let mut actual_required = required
            .iter()
            .map(|value| value.as_str().ok_or_else(runtime_capability_mismatch))
            .collect::<Result<Vec<_>, _>>()?;
        actual_required.sort_unstable();
        let mut expected_required = contract.required.to_vec();
        expected_required.sort_unstable();
        if actual_required != expected_required {
            return Err(runtime_capability_mismatch());
        }
        for parameter in contract.parameters {
            let property = schema_properties
                .get(parameter.name)
                .ok_or_else(runtime_capability_mismatch)?;
            if !private_parameter_schema_compatible(property, *parameter) {
                return Err(runtime_capability_mismatch());
            }
        }
    }
    Ok(())
}

fn private_parameter_schema_compatible(schema: &Value, contract: PrivateParameterContract) -> bool {
    let type_compatible = match contract.kind {
        PrivateParameterKind::String => schema_accepts_type(schema, "string"),
        PrivateParameterKind::Integer => schema_accepts_type(schema, "integer"),
        PrivateParameterKind::Boolean => schema_accepts_type(schema, "boolean"),
        PrivateParameterKind::StringArray => {
            schema_accepts_type(schema, "array")
                && schema
                    .get("items")
                    .is_some_and(|items| schema_accepts_type(items, "string"))
        }
    };
    type_compatible
        && min_length_compatible(schema, contract.min_length)
        && integer_bounds_compatible(schema, contract.minimum, contract.maximum)
        && enum_values_compatible(schema, contract.enum_values)
}

fn min_length_compatible(schema: &Value, expected: Option<u64>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    match schema.get("minLength") {
        None => true,
        Some(value) => value.as_u64().is_some_and(|actual| actual <= expected),
    }
}

fn integer_bounds_compatible(
    schema: &Value,
    expected_minimum: Option<i64>,
    expected_maximum: Option<i64>,
) -> bool {
    if let Some(expected) = expected_minimum {
        if let Some(actual) = schema.get("minimum") {
            let Some(actual) = actual.as_f64() else {
                return false;
            };
            if actual > expected as f64 {
                return false;
            }
        }
    }
    if let Some(expected) = expected_maximum {
        if let Some(actual) = schema.get("maximum") {
            let Some(actual) = actual.as_f64() else {
                return false;
            };
            if actual < expected as f64 {
                return false;
            }
        }
    }
    true
}

fn enum_values_compatible(schema: &Value, expected: &[&str]) -> bool {
    if expected.is_empty() {
        return true;
    }
    let Some(actual) = schema.get("enum") else {
        return true;
    };
    let Some(actual) = actual.as_array() else {
        return false;
    };
    expected
        .iter()
        .all(|expected| actual.iter().any(|value| value.as_str() == Some(*expected)))
}

fn schema_accepts_type(schema: &Value, expected: &str) -> bool {
    match schema.get("type") {
        Some(Value::String(actual)) => actual == expected,
        Some(Value::Array(actual)) => actual.iter().any(|value| value.as_str() == Some(expected)),
        _ => false,
    }
}

fn runtime_capability_mismatch() -> FacadeError {
    FacadeError::new(
        FacadeErrorCode::RuntimeCapabilityMismatch,
        "编码运行时能力与 LocalBridge facade 不兼容",
        false,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandControlAction {
    Poll,
    Read,
    Write,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitWorkflowAction {
    Status,
    Diff,
    Log,
    Show,
    Blame,
}

#[derive(Debug, Clone)]
pub struct ShellCommandRequest {
    pub execution: ShellExecutionSpec,
    pub yield_time_ms: u64,
    pub stdin: Option<String>,
}

pub trait WorkspaceRuntimeAdapter {
    fn negotiate(&mut self) -> Result<(), FacadeError>;
    fn public_tool_allowed_for_list(
        &self,
        policy: &CapabilityPolicy,
        mode: PermissionMode,
        public_name: &str,
    ) -> bool;
    fn public_policy_decision(
        &self,
        policy: &CapabilityPolicy,
        mode: PermissionMode,
        public_name: &str,
        arguments: &Value,
    ) -> Option<PolicyDecision>;
    fn workspace_context(&mut self, request_id: Option<&Value>) -> Result<Value, FacadeError>;
    fn execute_shell(
        &mut self,
        request: ShellCommandRequest,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError>;
    fn control_command(
        &mut self,
        action: CommandControlAction,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError>;
    fn git_workflow(
        &mut self,
        action: GitWorkflowAction,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError>;
    fn inspect_document(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError>;
    fn inspect_image(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError>;
    fn root_is_running(&self) -> Result<Option<bool>, CodingToolsRuntimeError>;
}

pub struct CodingToolsRuntimeAdapter {
    runtime: CodingToolsRuntime,
    shell_executor: ShellExecutor,
}

impl CodingToolsRuntimeAdapter {
    fn new(runtime: CodingToolsRuntime) -> Self {
        Self {
            runtime,
            shell_executor: ShellExecutor::default(),
        }
    }

    pub(crate) fn cancellation_client(
        &self,
    ) -> Result<McpCancellationClient, CodingToolsRuntimeError> {
        self.runtime.cancellation_client()
    }

    pub fn into_runtime(self) -> CodingToolsRuntime {
        self.runtime
    }

    fn private_call(
        &mut self,
        name: &str,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let raw = self
            .runtime
            .call_tool_with_request_id(name, arguments, request_id)
            .map_err(normalize_runtime_error)?;
        if raw.get("isError").and_then(Value::as_bool) == Some(true)
            || raw
                .get("structuredContent")
                .and_then(|value| value.get("ok"))
                .and_then(Value::as_bool)
                == Some(false)
        {
            return Err(normalize_private_error(&raw));
        }
        Ok(raw)
    }
}

impl WorkspaceRuntimeAdapter for CodingToolsRuntimeAdapter {
    fn negotiate(&mut self) -> Result<(), FacadeError> {
        let catalog = self.runtime.list_tools().map_err(normalize_runtime_error)?;
        validate_runtime_capabilities(&catalog)
    }

    fn public_tool_allowed_for_list(
        &self,
        policy: &CapabilityPolicy,
        mode: PermissionMode,
        public_name: &str,
    ) -> bool {
        coding_tools_policy_anchor_for_list(public_name)
            .is_some_and(|anchor| policy.tool_allowed_for_list(mode, anchor))
    }

    fn public_policy_decision(
        &self,
        policy: &CapabilityPolicy,
        mode: PermissionMode,
        public_name: &str,
        arguments: &Value,
    ) -> Option<PolicyDecision> {
        coding_tools_policy_anchor_for_call(public_name, arguments)
            .map(|anchor| policy.decide(mode, anchor, &[]))
    }

    fn workspace_context(&mut self, request_id: Option<&Value>) -> Result<Value, FacadeError> {
        let cwd = self.private_call("get_default_cwd", json!({}), request_id)?;
        let data = json!({
            "api_version": AGENT_API_VERSION,
            "workspace": extract_stable_string(&cwd, &["cwd", "path"]).unwrap_or_default(),
            "runtime": "ready"
        });
        Ok(stable_success(data, "LocalBridge workspace context ready"))
    }

    fn execute_shell(
        &mut self,
        request: ShellCommandRequest,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let direct = self
            .shell_executor
            .direct_spec(&request.execution)
            .map_err(normalize_shell_error)?;
        let command_line = structured_command_line(&direct.program, &direct.args);
        let mut private = json!({
            "cmd": command_line,
            "workdir": request.execution.cwd,
            "timeout_ms": request.execution.timeout_ms,
            "yield_time_ms": request.yield_time_ms,
            "max_output_bytes": request.execution.max_output_bytes
        });
        if let Some(stdin) = request.stdin {
            private["stdin"] = Value::String(stdin);
        }
        let raw = self.private_call("exec_command", private, request_id)?;
        Ok(normalize_command_success(&raw))
    }

    fn control_command(
        &mut self,
        action: CommandControlAction,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let private_name = match action {
            CommandControlAction::Poll | CommandControlAction::Read => "read_output",
            CommandControlAction::Write => "write_stdin",
            CommandControlAction::Kill => "kill_session",
        };
        let raw = self.private_call(private_name, arguments, request_id)?;
        Ok(normalize_command_success(&raw))
    }

    fn git_workflow(
        &mut self,
        action: GitWorkflowAction,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let private_name = match action {
            GitWorkflowAction::Status => "git_status",
            GitWorkflowAction::Diff => "git_diff",
            GitWorkflowAction::Log => "git_log",
            GitWorkflowAction::Show => "git_show",
            GitWorkflowAction::Blame => "git_blame",
        };
        let raw = self.private_call(private_name, arguments, request_id)?;
        Ok(normalize_git_success(action, &raw))
    }

    fn inspect_document(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let raw = self.private_call("read_file", arguments, request_id)?;
        Ok(stable_success(
            json!({"text": first_text(&raw).unwrap_or_default()}),
            "Document inspected",
        ))
    }

    fn inspect_image(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let raw = self.private_call("view_image", arguments, request_id)?;
        Ok(normalize_image_success(&raw))
    }

    fn root_is_running(&self) -> Result<Option<bool>, CodingToolsRuntimeError> {
        self.runtime.root_is_running().map(Some)
    }
}

pub struct AgentFacade<A = CodingToolsRuntimeAdapter> {
    adapter: A,
    policy: CapabilityPolicy,
    registry: ToolRegistry,
}

impl AgentFacade<CodingToolsRuntimeAdapter> {
    pub fn from_coding_runtime(
        runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
    ) -> Result<Self, FacadeError> {
        Self::with_adapter(CodingToolsRuntimeAdapter::new(runtime), policy)
    }

    pub(crate) fn cancellation_client(
        &self,
    ) -> Result<McpCancellationClient, CodingToolsRuntimeError> {
        self.adapter.cancellation_client()
    }

    pub fn into_runtime(self) -> CodingToolsRuntime {
        self.adapter.into_runtime()
    }
}

impl<A: WorkspaceRuntimeAdapter> AgentFacade<A> {
    pub fn with_adapter(mut adapter: A, policy: CapabilityPolicy) -> Result<Self, FacadeError> {
        adapter.negotiate()?;
        Ok(Self {
            adapter,
            policy,
            registry: ToolRegistry,
        })
    }

    pub fn public_tools(&self, mode: PermissionMode) -> Value {
        let tools = V1_CORE_TOOL_NAMES
            .iter()
            .filter(|name| {
                self.adapter
                    .public_tool_allowed_for_list(&self.policy, mode, name)
            })
            .map(|name| public_tool_schema(name))
            .collect::<Vec<_>>();
        json!({"tools":tools})
    }

    pub fn privileged_tool_visible(&self, mode: PermissionMode, name: &str) -> bool {
        self.policy.privileged_tool_visible(mode, name)
    }

    pub fn elevated_decision(&self, mode: PermissionMode, arguments: &Value) -> PolicyDecision {
        self.policy
            .decide_request(mode, "elevated_exec", &[], arguments)
    }

    pub fn runtime_root_is_running(&self) -> Result<Option<bool>, CodingToolsRuntimeError> {
        self.adapter.root_is_running()
    }

    pub fn call_tool<F>(
        &mut self,
        mode: PermissionMode,
        name: &str,
        arguments: Value,
        request_id: Option<&Value>,
        mut project: F,
    ) -> Result<Value, FacadeCallError>
    where
        F: FnMut(CurrentTaskStatus),
    {
        if !self.registry.contains(name) {
            return Err(FacadeCallError::Denied(FacadeDenied {
                reason: DenyReason::UnknownTool,
                capability: Capability::Unknown,
            }));
        }
        let Some(decision) =
            self.adapter
                .public_policy_decision(&self.policy, mode, name, &arguments)
        else {
            return Ok(FacadeError::new(
                FacadeErrorCode::InvalidArgument,
                "LocalBridge 工具参数无效或该动作尚不可用",
                false,
            )
            .to_mcp_result());
        };
        let kind = public_task_kind(name, &arguments);
        let summary = public_safe_summary(name, &arguments);
        if !decision.allowed {
            project(
                CurrentTaskStatus::project(kind, summary, TaskExecutionState::Blocked)
                    .expect("Blocked is valid"),
            );
            project(CurrentTaskStatus::Idle);
            return Err(FacadeCallError::Denied(FacadeDenied {
                reason: decision
                    .deny_reason
                    .expect("denied policy decision contains reason"),
                capability: decision.descriptor.capability,
            }));
        }
        if public_contains_verbatim_execution_path(name, &arguments) {
            project(
                CurrentTaskStatus::project(kind, summary, TaskExecutionState::Blocked)
                    .expect("Blocked is valid"),
            );
            project(CurrentTaskStatus::Idle);
            return Err(FacadeCallError::Denied(FacadeDenied {
                reason: DenyReason::VerbatimExecutionPath,
                capability: decision.descriptor.capability,
            }));
        }
        project(
            CurrentTaskStatus::project(kind, summary, TaskExecutionState::Running)
                .expect("Running is valid"),
        );
        let result = self.dispatch(name, arguments, request_id);
        match &result {
            Ok(value) if value.get("isError").and_then(Value::as_bool) == Some(true) => project(
                CurrentTaskStatus::project(
                    kind,
                    SafeTaskSummary::Omitted,
                    TaskExecutionState::Failed,
                )
                .expect("Failed is valid"),
            ),
            Err(_) => project(
                CurrentTaskStatus::project(
                    kind,
                    SafeTaskSummary::Omitted,
                    TaskExecutionState::Failed,
                )
                .expect("Failed is valid"),
            ),
            _ => {}
        }
        project(CurrentTaskStatus::Idle);
        Ok(result.unwrap_or_else(|error| error.to_mcp_result()))
    }

    fn dispatch(
        &mut self,
        name: &str,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        match name {
            "workspace_context" => self.workspace_context(request_id),
            "exec_command" => self.exec_command(arguments, request_id),
            "command_control" => self.command_control(arguments, request_id),
            "git_workflow" => self.git_workflow(arguments, request_id),
            "document_workflow" => self.document_workflow(arguments, request_id),
            "view_image" => self.view_image(arguments, request_id),
            "agent_workflow" | "task_control" => Err(FacadeError::new(
                FacadeErrorCode::InvalidArgument,
                "该 LocalBridge facade 动作当前不可用",
                false,
            )),
            _ => Err(FacadeError::new(
                FacadeErrorCode::CapabilityDenied,
                "未知 LocalBridge public tool",
                false,
            )),
        }
    }

    fn workspace_context(&mut self, request_id: Option<&Value>) -> Result<Value, FacadeError> {
        self.adapter.workspace_context(request_id)
    }

    fn exec_command(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let object = object_args(&arguments)?;
        let command = required_string(object, "command")?;
        let shell: ShellSelector = serde_json::from_value(
            object
                .get("shell")
                .cloned()
                .unwrap_or_else(|| Value::String("auto".into())),
        )
        .map_err(|_| invalid_argument())?;
        let workdir = object.get("workdir").and_then(Value::as_str).unwrap_or(".");
        let spec = ShellExecutionSpec {
            shell,
            command: command.to_string(),
            cwd: Path::new(workdir).to_path_buf(),
            timeout_ms: object
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(30_000),
            max_output_bytes: object
                .get("max_output_bytes")
                .and_then(Value::as_u64)
                .unwrap_or(65_536) as usize,
        };
        self.adapter.execute_shell(
            ShellCommandRequest {
                execution: spec,
                yield_time_ms: object
                    .get("yield_time_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(10_000),
                stdin: object
                    .get("stdin")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            },
            request_id,
        )
    }

    fn command_control(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let object = object_args(&arguments)?;
        let action = required_string(object, "action")?;
        let action = match action {
            "poll" => CommandControlAction::Poll,
            "read" => CommandControlAction::Read,
            "write" => CommandControlAction::Write,
            "kill" => CommandControlAction::Kill,
            _ => return Err(invalid_argument()),
        };
        let mut stable = object.clone();
        stable.remove("action");
        self.adapter
            .control_command(action, Value::Object(stable), request_id)
    }

    fn git_workflow(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let object = object_args(&arguments)?;
        let action = match required_string(object, "action")? {
            "status" => GitWorkflowAction::Status,
            "diff" => GitWorkflowAction::Diff,
            "log" => GitWorkflowAction::Log,
            "show" => GitWorkflowAction::Show,
            "blame" => GitWorkflowAction::Blame,
            _ => return Err(invalid_argument()),
        };
        let mut stable = object.clone();
        stable.remove("action");
        self.adapter
            .git_workflow(action, Value::Object(stable), request_id)
    }

    fn document_workflow(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let object = object_args(&arguments)?;
        match required_string(object, "action")? {
            "inspect" => {
                let path = required_string(object, "path")?;
                let mut private = json!({"path":path});
                for key in ["start_line", "end_line", "max_lines", "max_bytes"] {
                    if let Some(value) = object.get(key) {
                        private[key] = value.clone();
                    }
                }
                self.adapter.inspect_document(private, request_id)
            }
            _ => Err(FacadeError::new(
                FacadeErrorCode::InvalidArgument,
                "该文档动作当前 adapter 尚未实现",
                false,
            )),
        }
    }

    fn view_image(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        self.adapter.inspect_image(arguments, request_id)
    }
}

fn coding_tools_policy_anchor_for_list(name: &str) -> Option<&'static str> {
    match name {
        "workspace_context" => Some("server_info"),
        "agent_workflow" => Some("exec_command"),
        "exec_command" => Some("exec_command"),
        "command_control" => Some("read_output"),
        "task_control" => Some("read_output"),
        "git_workflow" => Some("git_status"),
        "document_workflow" => Some("apply_patch"),
        "view_image" => Some("view_image"),
        _ => None,
    }
}

fn coding_tools_policy_anchor_for_call(name: &str, arguments: &Value) -> Option<&'static str> {
    match name {
        "workspace_context" => Some("server_info"),
        "exec_command" => Some("exec_command"),
        "command_control" => match arguments.get("action").and_then(Value::as_str) {
            Some("poll" | "read") => Some("read_output"),
            Some("write") => Some("write_stdin"),
            Some("kill") => Some("kill_session"),
            _ => None,
        },
        "git_workflow" => match arguments.get("action").and_then(Value::as_str) {
            Some("status") => Some("git_status"),
            Some("diff") => Some("git_diff"),
            Some("log") => Some("git_log"),
            Some("show") => Some("git_show"),
            Some("blame") => Some("git_blame"),
            _ => None,
        },
        "document_workflow" => match arguments.get("action").and_then(Value::as_str) {
            Some("inspect") => Some("read_file"),
            Some("create" | "rebuild") => Some("apply_patch"),
            _ => None,
        },
        "view_image" => Some("view_image"),
        "agent_workflow" => Some("exec_command"),
        "task_control" => Some("read_output"),
        _ => None,
    }
}

fn public_task_kind(name: &str, arguments: &Value) -> TaskKind {
    match name {
        "exec_command" | "agent_workflow" | "command_control" | "task_control" => {
            let text = arguments
                .get("command")
                .or_else(|| arguments.get("objective"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if text.contains("test") || text.contains("cargo test") || text.contains("vitest") {
                TaskKind::Test
            } else if text.contains("build") || text.contains("cargo build") {
                TaskKind::Build
            } else {
                TaskKind::ExecuteCommand
            }
        }
        "git_workflow" => TaskKind::GitOperation,
        "document_workflow" => {
            if arguments.get("action").and_then(Value::as_str) == Some("inspect") {
                TaskKind::ReadFile
            } else {
                TaskKind::ModifyFile
            }
        }
        "workspace_context" | "view_image" => TaskKind::ReadFile,
        _ => TaskKind::Other,
    }
}

fn public_safe_summary(name: &str, arguments: &Value) -> SafeTaskSummary {
    let value = match name {
        "exec_command" => arguments.get("command").and_then(Value::as_str),
        "git_workflow" | "document_workflow" | "view_image" => {
            arguments.get("path").and_then(Value::as_str)
        }
        "agent_workflow" => arguments.get("objective").and_then(Value::as_str),
        _ => None,
    };
    value
        .map(SafeTaskSummary::from_untrusted)
        .unwrap_or(SafeTaskSummary::Omitted)
}

fn public_contains_verbatim_execution_path(name: &str, arguments: &Value) -> bool {
    let Some(object) = arguments.as_object() else {
        return false;
    };
    let keys: &[&str] = match name {
        "exec_command" => &["workdir"],
        "git_workflow" | "document_workflow" | "view_image" => &["path"],
        _ => &[],
    };
    keys.iter().any(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with(r"\\?\") || value.starts_with("//?/"))
    })
}

fn object_args(value: &Value) -> Result<&Map<String, Value>, FacadeError> {
    value.as_object().ok_or_else(invalid_argument)
}

fn required_string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, FacadeError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_argument)
}

fn invalid_argument() -> FacadeError {
    FacadeError::new(
        FacadeErrorCode::InvalidArgument,
        "LocalBridge 工具参数无效",
        false,
    )
}

fn normalize_shell_error(_error: ShellResolveError) -> FacadeError {
    FacadeError::new(
        FacadeErrorCode::RuntimeUnavailable,
        "没有可用的可信命令 Shell",
        false,
    )
}

fn normalize_runtime_error(error: CodingToolsRuntimeError) -> FacadeError {
    match error {
        CodingToolsRuntimeError::ProtocolMismatch => FacadeError::new(
            FacadeErrorCode::RuntimeProtocolMismatch,
            "编码运行时协议不兼容",
            false,
        ),
        CodingToolsRuntimeError::Cancelled => {
            FacadeError::new(FacadeErrorCode::ProcessCancelled, "命令已取消", false)
        }
        CodingToolsRuntimeError::HealthTimeout => FacadeError::new(
            FacadeErrorCode::RuntimeUnavailable,
            "编码运行时未就绪",
            true,
        ),
        _ => FacadeError::new(
            FacadeErrorCode::RuntimeUnavailable,
            "编码运行时不可用",
            true,
        ),
    }
}

fn normalize_private_error(raw: &Value) -> FacadeError {
    let code = raw
        .get("structuredContent")
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_uppercase();
    let public = if code.contains("INVALID") {
        FacadeErrorCode::InvalidArgument
    } else if code.contains("NOT_FOUND") || code.contains("MISSING") {
        FacadeErrorCode::NotFound
    } else if code.contains("OUTSIDE_WORKSPACE") || code.contains("WORKSPACE_DENIED") {
        FacadeErrorCode::WorkspaceDenied
    } else if code.contains("TIMEOUT") {
        FacadeErrorCode::ProcessTimedOut
    } else if code.contains("CANCEL") {
        FacadeErrorCode::ProcessCancelled
    } else if code.contains("TRUNCAT") {
        FacadeErrorCode::OutputTruncated
    } else {
        FacadeErrorCode::ProcessFailed
    };
    FacadeError::new(public, "LocalBridge 工具执行失败", false)
}

fn stable_success(data: Value, text: &str) -> Value {
    json!({
        "content":[{"type":"text","text":text}],
        "structuredContent":{"ok":true,"data":data},
        "isError":false
    })
}

fn normalize_command_success(raw: &Value) -> Value {
    let structured = raw.get("structuredContent").and_then(Value::as_object);
    let mut data = Map::new();
    for key in [
        "exit_code",
        "status",
        "session_id",
        "output_ref",
        "stdout_ref",
        "stderr_ref",
        "truncated",
        "timed_out",
        "offset",
        "next_offset",
    ] {
        if let Some(value) = structured.and_then(|object| object.get(key)) {
            data.insert(key.into(), value.clone());
        }
    }
    data.insert(
        "output".into(),
        Value::String(first_text(raw).unwrap_or_default()),
    );
    stable_success(Value::Object(data), "Command completed")
}

fn first_text(raw: &Value) -> Option<String> {
    raw.get("content")
        .and_then(Value::as_array)
        .and_then(|content| {
            content.iter().find_map(|item| {
                (item.get("type").and_then(Value::as_str) == Some("text"))
                    .then(|| item.get("text").and_then(Value::as_str).map(str::to_string))
                    .flatten()
            })
        })
}

fn extract_stable_string(raw: &Value, keys: &[&str]) -> Option<String> {
    let structured = raw.get("structuredContent")?;
    keys.iter()
        .find_map(|key| structured.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

#[derive(Debug, Clone, Copy)]
enum PublicFieldKind {
    String,
    NullableString,
    Boolean,
    Integer,
    NullableInteger,
    StringArray,
}

fn normalize_git_success(action: GitWorkflowAction, raw: &Value) -> Value {
    let source = raw
        .get("structuredContent")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut data = match action {
        GitWorkflowAction::Status => sanitize_public_fields(
            &source,
            &[
                ("is_repo", PublicFieldKind::Boolean),
                ("branch", PublicFieldKind::NullableString),
                ("head", PublicFieldKind::NullableString),
                ("upstream", PublicFieldKind::NullableString),
                ("ahead", PublicFieldKind::Integer),
                ("behind", PublicFieldKind::Integer),
                ("clean", PublicFieldKind::Boolean),
                ("truncated", PublicFieldKind::Boolean),
            ],
        ),
        GitWorkflowAction::Diff => sanitize_public_fields(
            &source,
            &[
                ("diff", PublicFieldKind::String),
                ("truncated", PublicFieldKind::Boolean),
                ("truncated_by", PublicFieldKind::NullableString),
                ("output_bytes", PublicFieldKind::Integer),
                ("output_lines", PublicFieldKind::Integer),
                ("warnings", PublicFieldKind::StringArray),
            ],
        ),
        GitWorkflowAction::Log => sanitize_public_fields(
            &source,
            &[
                ("is_repo", PublicFieldKind::Boolean),
                ("ref", PublicFieldKind::String),
                ("path", PublicFieldKind::String),
                ("max_count", PublicFieldKind::Integer),
                ("skip", PublicFieldKind::Integer),
                ("truncated", PublicFieldKind::Boolean),
                ("warnings", PublicFieldKind::StringArray),
            ],
        ),
        GitWorkflowAction::Show => sanitize_public_fields(
            &source,
            &[
                ("is_repo", PublicFieldKind::Boolean),
                ("rev", PublicFieldKind::String),
                ("content", PublicFieldKind::String),
                ("truncated", PublicFieldKind::Boolean),
                ("truncated_by", PublicFieldKind::NullableString),
                ("output_bytes", PublicFieldKind::Integer),
                ("output_lines", PublicFieldKind::Integer),
                ("warnings", PublicFieldKind::StringArray),
            ],
        ),
        GitWorkflowAction::Blame => sanitize_public_fields(
            &source,
            &[
                ("is_repo", PublicFieldKind::Boolean),
                ("path", PublicFieldKind::String),
                ("rev", PublicFieldKind::NullableString),
                ("start_line", PublicFieldKind::Integer),
                ("end_line", PublicFieldKind::NullableInteger),
                ("max_lines", PublicFieldKind::Integer),
                ("truncated", PublicFieldKind::Boolean),
                ("warnings", PublicFieldKind::StringArray),
            ],
        ),
    };
    match action {
        GitWorkflowAction::Status => sanitize_object_array(
            &source,
            &mut data,
            "entries",
            &[
                ("path", PublicFieldKind::String),
                ("original_path", PublicFieldKind::NullableString),
                ("index_status", PublicFieldKind::String),
                ("worktree_status", PublicFieldKind::String),
            ],
        ),
        GitWorkflowAction::Diff | GitWorkflowAction::Show => sanitize_object_array(
            &source,
            &mut data,
            "files",
            &[
                ("path", PublicFieldKind::String),
                ("status", PublicFieldKind::String),
                ("binary", PublicFieldKind::Boolean),
            ],
        ),
        GitWorkflowAction::Log => sanitize_object_array(
            &source,
            &mut data,
            "commits",
            &[
                ("hash", PublicFieldKind::String),
                ("short_hash", PublicFieldKind::String),
                ("author_name", PublicFieldKind::String),
                ("author_email", PublicFieldKind::String),
                ("author_date", PublicFieldKind::String),
                ("subject", PublicFieldKind::String),
            ],
        ),
        GitWorkflowAction::Blame => sanitize_object_array(
            &source,
            &mut data,
            "lines",
            &[
                ("commit", PublicFieldKind::String),
                ("original_line", PublicFieldKind::Integer),
                ("line", PublicFieldKind::Integer),
                ("author", PublicFieldKind::String),
                ("author_mail", PublicFieldKind::String),
                ("author_time", PublicFieldKind::String),
                ("summary", PublicFieldKind::String),
                ("content", PublicFieldKind::String),
            ],
        ),
    }
    stable_success(Value::Object(data), "Git workflow completed")
}

fn normalize_image_success(raw: &Value) -> Value {
    let content = raw
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| match item.get("type").and_then(Value::as_str) {
            Some("image") => Some(json!({
                "type":"image",
                "data": item.get("data")?.as_str()?,
                "mimeType": item.get("mimeType")?.as_str()?
            })),
            Some("text") => Some(json!({
                "type":"text",
                "text": item.get("text")?.as_str()?
            })),
            _ => None,
        })
        .collect::<Vec<_>>();
    json!({
        "content": content,
        "structuredContent":{"ok":true,"data":{"kind":"image"}},
        "isError":false
    })
}

fn sanitize_public_fields(
    source: &Map<String, Value>,
    fields: &[(&str, PublicFieldKind)],
) -> Map<String, Value> {
    fields
        .iter()
        .filter_map(|(name, kind)| {
            sanitize_public_value(source.get(*name)?, *kind).map(|value| ((*name).into(), value))
        })
        .collect()
}

fn sanitize_public_value(value: &Value, kind: PublicFieldKind) -> Option<Value> {
    match kind {
        PublicFieldKind::String => value.as_str().map(|value| Value::String(value.into())),
        PublicFieldKind::NullableString => value
            .is_null()
            .then_some(Value::Null)
            .or_else(|| value.as_str().map(|value| Value::String(value.into()))),
        PublicFieldKind::Boolean => value.as_bool().map(Value::Bool),
        PublicFieldKind::Integer => value
            .as_u64()
            .map(Value::from)
            .or_else(|| value.as_i64().map(Value::from)),
        PublicFieldKind::NullableInteger => value
            .is_null()
            .then_some(Value::Null)
            .or_else(|| value.as_u64().map(Value::from))
            .or_else(|| value.as_i64().map(Value::from)),
        PublicFieldKind::StringArray => value.as_array().map(|values| {
            Value::Array(
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|value| Value::String(value.into()))
                    .collect(),
            )
        }),
    }
}

fn sanitize_object_array(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    name: &str,
    fields: &[(&str, PublicFieldKind)],
) {
    let Some(values) = source.get(name).and_then(Value::as_array) else {
        return;
    };
    target.insert(
        name.into(),
        Value::Array(
            values
                .iter()
                .filter_map(Value::as_object)
                .map(|value| Value::Object(sanitize_public_fields(value, fields)))
                .collect(),
        ),
    );
}

fn structured_command_line(program: &Path, args: &[std::ffi::OsString]) -> String {
    std::iter::once(program.as_os_str())
        .chain(args.iter().map(std::ffi::OsString::as_os_str))
        .map(windows_quote_argument)
        .collect::<Vec<_>>()
        .join(" ")
}

fn windows_quote_argument(value: &std::ffi::OsStr) -> String {
    let value = value.to_string_lossy();
    if !value.contains([' ', '\t', '"']) {
        return value.into_owned();
    }
    let mut quoted = String::from("\"");
    let mut slashes = 0usize;
    for ch in value.chars() {
        match ch {
            '\\' => slashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(slashes * 2 + 1));
                quoted.push('"');
                slashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(slashes));
                slashes = 0;
                quoted.push(ch);
            }
        }
    }
    quoted.push_str(&"\\".repeat(slashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeAdapter {
        catalog: Value,
    }

    impl WorkspaceRuntimeAdapter for FakeAdapter {
        fn negotiate(&mut self) -> Result<(), FacadeError> {
            validate_runtime_capabilities(&self.catalog)
        }

        fn public_tool_allowed_for_list(
            &self,
            policy: &CapabilityPolicy,
            mode: PermissionMode,
            public_name: &str,
        ) -> bool {
            coding_tools_policy_anchor_for_list(public_name)
                .is_some_and(|anchor| policy.tool_allowed_for_list(mode, anchor))
        }

        fn public_policy_decision(
            &self,
            policy: &CapabilityPolicy,
            mode: PermissionMode,
            public_name: &str,
            arguments: &Value,
        ) -> Option<PolicyDecision> {
            coding_tools_policy_anchor_for_call(public_name, arguments)
                .map(|anchor| policy.decide(mode, anchor, &[]))
        }

        fn workspace_context(&mut self, _request_id: Option<&Value>) -> Result<Value, FacadeError> {
            Ok(stable_success(json!({}), "ok"))
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

        fn inspect_document(
            &mut self,
            _arguments: Value,
            _request_id: Option<&Value>,
        ) -> Result<Value, FacadeError> {
            Ok(stable_success(json!({}), "ok"))
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
            AgentFacade::with_adapter(FakeAdapter { catalog: missing }, policy()),
            Err(FacadeError {
                code: FacadeErrorCode::RuntimeCapabilityMismatch,
                ..
            })
        ));
        assert!(matches!(
            AgentFacade::with_adapter(
                FakeAdapter {
                    catalog: incompatible
                },
                policy()
            ),
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
        diff["inputSchema"]["properties"]["paths"]["items"]["type"] =
            Value::String("integer".into());
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
        kill["inputSchema"]["properties"]["signal"]["enum"] =
            json!(["TERM", "KILL", "INT", "BREAK"]);
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
    }

    #[test]
    fn private_success_shape_is_allowlisted_before_publication() {
        let raw = json!({
            "content":[{"type":"text","text":"safe output"}],
            "structuredContent":{
                "exit_code":0,
                "truncated":false,
                "future_private_field":"SECRET_PRIVATE_SCHEMA_VALUE"
            },
            "isError":false
        });
        let public = normalize_command_success(&raw);
        let rendered = serde_json::to_string(&public).unwrap();
        assert!(rendered.contains("safe output"));
        assert!(rendered.contains("exit_code"));
        assert!(!rendered.contains("future_private_field"));
        assert!(!rendered.contains("SECRET_PRIVATE_SCHEMA_VALUE"));
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
    fn raw_upstream_name_is_not_a_public_registry_entry() {
        let registry = ToolRegistry;
        assert!(registry.contains("exec_command"));
        assert!(!registry.contains("read_file"));
        assert!(!registry.contains("request_permissions"));
    }
}
