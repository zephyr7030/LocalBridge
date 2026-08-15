use std::collections::HashMap;
use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine as _;
use serde_json::{Map, Value, json};

use crate::state::{
    Capability, CurrentTaskStatus, PermissionMode, SafeTaskSummary, TaskExecutionState, TaskKind,
};

use super::http::McpCancellationClient;
use super::path_authority::{PathAuthority, PathAuthorityError, workspace_relative_path_valid};
use super::policy::{CapabilityPolicy, DenyReason, PolicyDecision};
use super::runtime::{CodingToolsRuntime, CodingToolsRuntimeError};
use super::shell::{ShellExecutionSpec, ShellExecutor, ShellResolveError, ShellSelector};
use super::task_state::{
    CommandOwner, CommandTaskStateError, CommandTaskStateStore, CommandTerminalStatus,
    TerminalCommandSnapshot,
};

pub const AGENT_API_VERSION: u32 = 1;
pub const AGENT_API_REVISION: u32 = 31;
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
    SessionUnavailable,
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
            Self::SessionUnavailable => "SessionUnavailable",
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
                    "objective":{"type":"string"},
                    "path":{"type":"string","default":"."},
                    "patch":{"type":"string","minLength":1},
                    "directory_changes":{
                        "type":"array","maxItems":32,
                        "items":{
                            "type":"object",
                            "properties":{
                                "action":{"type":"string","enum":["create_directory","remove_empty_directory"]},
                                "path":{"type":"string","minLength":1}
                            },
                            "required":["action","path"],
                            "additionalProperties":false
                        }
                    },
                    "commands":{
                        "type":"array","maxItems":8,
                        "items":{
                            "type":"object",
                            "properties":{
                                "command":{"type":"string","minLength":1},
                                "shell":{"type":"string","enum":["auto","powershell","pwsh","windows_powershell","cmd"],"default":"auto"},
                                "workdir":{"type":"string","default":"."},
                                "timeout_ms":{"type":"integer","minimum":1,"maximum":600000,"default":30000},
                                "yield_time_ms":{"type":"integer","minimum":0,"maximum":30000,"default":10000},
                                "max_output_bytes":{"type":"integer","minimum":1,"maximum":1048576,"default":65536},
                                "stdin":{"type":"string"}
                            },
                            "required":["command"],
                            "additionalProperties":false
                        }
                    }
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
                "oneOf":[
                    {
                        "type":"object",
                        "properties":{
                            "action":{"const":"poll"},
                            "session_id":{"type":"string","minLength":1},
                            "wait_ms":{"type":"integer","minimum":0,"maximum":30000}
                        },
                        "required":["action","session_id"],
                        "additionalProperties":false
                    },
                    {
                        "type":"object",
                        "properties":{
                            "action":{"const":"read"},
                            "output_ref":{"type":"string","minLength":1},
                            "stream":{"type":"string","enum":["stdout","stderr"]},
                            "offset":{"type":"integer","minimum":0},
                            "limit":{"type":"integer","minimum":1,"maximum":1048576}
                        },
                        "required":["action","output_ref"],
                        "additionalProperties":false
                    },
                    {
                        "type":"object",
                        "properties":{
                            "action":{"const":"write"},
                            "session_id":{"type":"string","minLength":1},
                            "chars":{"type":"string","minLength":1},
                            "wait_ms":{"type":"integer","minimum":0,"maximum":30000}
                        },
                        "required":["action","session_id","chars"],
                        "additionalProperties":false
                    },
                    {
                        "type":"object",
                        "properties":{
                            "action":{"const":"kill"},
                            "session_id":{"type":"string","minLength":1},
                            "signal":{"type":"string","enum":["TERM","KILL","INT"]},
                            "wait_ms":{"type":"integer","minimum":0,"maximum":30000}
                        },
                        "required":["action","session_id"],
                        "additionalProperties":false
                    }
                ]
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
            "Inspect, create, convert, or rebuild UTF-8 workspace documents through a stable LocalBridge workflow.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":["inspect","create","convert","rebuild"]},
                    "path":{"type":"string"},
                    "source":{"type":"string"},
                    "content":{"type":"string"},
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
    StringMap,
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
            private_parameter("env", PrivateParameterKind::StringMap),
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
        PrivateParameterKind::StringMap => {
            schema_accepts_type(schema, "object")
                && schema
                    .get("additionalProperties")
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
    fn workspace_context(&mut self, request_id: Option<&Value>) -> Result<Value, FacadeError>;
    fn project_context(&self, path: &str) -> Result<Value, FacadeError>;
    fn apply_directory_change(&mut self, action: &str, path: &str) -> Result<Value, FacadeError>;
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
    fn apply_document_patch(
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
    fn reap_command_sessions(&mut self) -> Result<(), FacadeError>;
    fn has_running_command_session(&self) -> bool;
}

static PUBLIC_COMMAND_HANDLE_GENERATION: AtomicU64 = AtomicU64::new(1);

fn next_public_handle(prefix: &str) -> String {
    let generation = PUBLIC_COMMAND_HANDLE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}-{:x}-{nonce:x}-{generation:x}", std::process::id())
}

#[derive(Debug, Clone)]
struct PublicCommandSession {
    owner: CommandOwner,
    private_session_id: Option<String>,
    terminal: Option<Value>,
    pending_output: String,
    stderr_protocol_buffer: String,
}

#[derive(Debug, Clone)]
struct PublicOutputHandle {
    private_output_ref: String,
}

#[derive(Debug, Default)]
struct PublicCommandSessions {
    sessions: HashMap<String, PublicCommandSession>,
    private_sessions: HashMap<String, String>,
    outputs: HashMap<String, PublicOutputHandle>,
    private_outputs: HashMap<String, String>,
}

impl PublicCommandSessions {
    fn start_session(&mut self, task_state: &CommandTaskStateStore) -> Result<String, FacadeError> {
        let public = next_public_handle("lb-session");
        let owner = CommandOwner::new(next_public_handle("lb-task"), public.clone());
        task_state
            .begin(owner.clone())
            .map_err(normalize_task_state_error)?;
        self.sessions.insert(
            public.clone(),
            PublicCommandSession {
                owner,
                private_session_id: None,
                terminal: None,
                pending_output: String::new(),
                stderr_protocol_buffer: String::new(),
            },
        );
        Ok(public)
    }

    fn bind_private_session(
        &mut self,
        public_session_id: &str,
        private_session_id: &str,
    ) -> Result<(), FacadeError> {
        if self
            .private_sessions
            .get(private_session_id)
            .is_some_and(|existing| existing != public_session_id)
        {
            return Err(command_state_internal_error());
        }
        let session = self
            .sessions
            .get_mut(public_session_id)
            .ok_or_else(session_unavailable)?;
        if session
            .private_session_id
            .as_deref()
            .is_some_and(|existing| existing != private_session_id)
        {
            return Err(command_state_internal_error());
        }
        session.private_session_id = Some(private_session_id.to_string());
        self.private_sessions.insert(
            private_session_id.to_string(),
            public_session_id.to_string(),
        );
        Ok(())
    }

    fn public_output_for_private(&mut self, private_output_ref: &str) -> String {
        if let Some(public) = self.private_outputs.get(private_output_ref) {
            return public.clone();
        }
        let public = next_public_handle("lb-output");
        self.private_outputs
            .insert(private_output_ref.to_string(), public.clone());
        self.outputs.insert(
            public.clone(),
            PublicOutputHandle {
                private_output_ref: private_output_ref.to_string(),
            },
        );
        public
    }

    fn terminal(&self, public_session_id: &str) -> Option<Value> {
        self.sessions
            .get(public_session_id)
            .and_then(|session| session.terminal.clone())
    }

    fn private_session(&self, public_session_id: &str) -> Option<String> {
        self.sessions
            .get(public_session_id)
            .and_then(|session| session.private_session_id.clone())
    }

    fn private_output(&self, public_output_ref: &str) -> Option<String> {
        self.outputs
            .get(public_output_ref)
            .map(|output| output.private_output_ref.clone())
    }

    fn mark_terminal(
        &mut self,
        public_session_id: &str,
        result: Value,
        task_state: &CommandTaskStateStore,
    ) -> Result<(), FacadeError> {
        let owner = self
            .sessions
            .get(public_session_id)
            .ok_or_else(session_unavailable)?
            .owner
            .clone();
        if self
            .sessions
            .get(public_session_id)
            .is_some_and(|session| session.terminal.is_some())
        {
            return Ok(());
        }
        task_state
            .finalize(terminal_snapshot_from_result(owner, &result))
            .map_err(normalize_task_state_error)?;
        if let Some(session) = self.sessions.get_mut(public_session_id) {
            if session.terminal.is_none() {
                session.terminal = Some(command_result_with_output(result, String::new()));
            }
        }
        Ok(())
    }

    fn mark_error_terminal(
        &mut self,
        public_session_id: &str,
        error: &FacadeError,
        task_state: &CommandTaskStateStore,
    ) -> Result<(), FacadeError> {
        self.mark_terminal(public_session_id, error.to_mcp_result(), task_state)
    }

    fn append_pending(&mut self, public_session_id: &str, output: &str) {
        if output.is_empty() {
            return;
        }
        if let Some(session) = self.sessions.get_mut(public_session_id) {
            session.pending_output.push_str(output);
        }
    }

    fn take_pending(&mut self, public_session_id: &str) -> String {
        self.sessions
            .get_mut(public_session_id)
            .map(|session| std::mem::take(&mut session.pending_output))
            .unwrap_or_default()
    }

    fn terminal_with_pending(&mut self, public_session_id: &str) -> Option<Value> {
        let terminal = self.terminal(public_session_id)?;
        let pending = self.take_pending(public_session_id);
        Some(command_result_with_output(terminal, pending))
    }

    fn running_with_pending(&mut self, public_session_id: &str) -> Option<Value> {
        if self.terminal(public_session_id).is_some() {
            return None;
        }
        let pending = self.take_pending(public_session_id);
        (!pending.is_empty()).then(|| {
            stable_success(
                json!({
                    "status":"running",
                    "session_id":public_session_id,
                    "output":pending
                }),
                "Command running",
            )
        })
    }

    fn filter_private_stderr(&mut self, public_session_id: &str, stderr: &str) -> String {
        if stderr.is_empty() {
            return String::new();
        }
        let Some(session) = self.sessions.get_mut(public_session_id) else {
            return public_command_stderr(stderr);
        };
        session.stderr_protocol_buffer.push_str(stderr);
        drain_public_stderr_protocol_buffer(&mut session.stderr_protocol_buffer)
    }

    fn mark_all_running_lost(
        &mut self,
        task_state: &CommandTaskStateStore,
    ) -> Result<(), FacadeError> {
        let running = self
            .sessions
            .iter()
            .filter(|(_, session)| session.terminal.is_none())
            .map(|(public, _)| public.clone())
            .collect::<Vec<_>>();
        for public in running {
            self.mark_terminal(&public, session_unavailable().to_mcp_result(), task_state)?;
        }
        Ok(())
    }

    fn running_sessions(&self) -> Vec<(String, String)> {
        self.sessions
            .iter()
            .filter(|(_, session)| session.terminal.is_none())
            .filter_map(|(public, session)| {
                session
                    .private_session_id
                    .as_ref()
                    .map(|private| (public.clone(), private.clone()))
            })
            .collect()
    }

    fn has_running_session(&self) -> bool {
        self.sessions
            .values()
            .any(|session| session.terminal.is_none())
    }
}

pub struct CodingToolsRuntimeAdapter {
    runtime: CodingToolsRuntime,
    workspace: PathBuf,
    shell_executor: ShellExecutor,
    public_commands: PublicCommandSessions,
    task_state: CommandTaskStateStore,
}

impl CodingToolsRuntimeAdapter {
    fn new(runtime: CodingToolsRuntime) -> Result<Self, FacadeError> {
        let workspace = runtime.workspace().to_path_buf();
        let task_state =
            CommandTaskStateStore::for_workspace(&workspace).map_err(normalize_task_state_error)?;
        Ok(Self {
            runtime,
            workspace,
            shell_executor: ShellExecutor::default(),
            public_commands: PublicCommandSessions::default(),
            task_state,
        })
    }

    pub(crate) fn cancellation_client(
        &self,
    ) -> Result<McpCancellationClient, CodingToolsRuntimeError> {
        self.runtime.cancellation_client()
    }

    pub(crate) fn command_task_state(&self) -> CommandTaskStateStore {
        self.task_state.clone()
    }

    pub fn into_runtime(self) -> CodingToolsRuntime {
        self.runtime
    }

    fn stable_workspace_relative_path(&self, absolute: &Path) -> Result<String, FacadeError> {
        PathAuthority::active_workspace(&self.workspace)
            .map_err(normalize_path_authority_error)?
            .display_path(absolute)
            .map_err(normalize_path_authority_error)
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

    fn private_call_with_timeout(
        &mut self,
        name: &str,
        arguments: Value,
        request_id: Option<&Value>,
        transport_timeout: std::time::Duration,
    ) -> Result<Value, FacadeError> {
        let raw = self
            .runtime
            .call_tool_with_request_id_and_timeout(name, arguments, request_id, transport_timeout)
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

    fn resolve_existing_workspace_path(&self, relative: &str) -> Result<PathBuf, FacadeError> {
        PathAuthority::active_workspace(&self.workspace)
            .map_err(normalize_path_authority_error)?
            .resolve_existing(relative)
            .map_err(normalize_path_authority_error)
    }

    fn probe_private_result_semantics(&mut self) -> Result<(), FacadeError> {
        let invocation = self
            .shell_executor
            .runtime_invocation(&ShellExecutionSpec {
                shell: ShellSelector::Auto,
                command: "$line=[Console]::In.ReadLine(); Write-Output ('LB_SEMANTIC_PROBE:'+ $line); Start-Sleep -Seconds 30".into(),
                cwd: PathBuf::from("."),
                timeout_ms: 45_000,
                max_output_bytes: 65_536,
            })
            .map_err(normalize_shell_error)?;
        let exec = self.private_call(
            "exec_command",
            json!({
                "cmd":invocation.command_line,
                "workdir":".",
                "timeout_ms":45_000,
                "yield_time_ms":0,
                "max_output_bytes":65_536,
                "verbosity":"full",
                "env":{"COMSPEC":invocation.comspec.to_string_lossy()}
            }),
            None,
        )?;
        let (session_id, stdout_ref) = validate_private_command_result_semantics(&exec, true)?;
        let result = (|| {
            let written = self.private_call(
                "write_stdin",
                json!({
                    "session_id":session_id,
                    "chars":"probe\n",
                    "yield_time_ms":1_000,
                    "max_output_bytes":65_536,
                    "verbosity":"full"
                }),
                None,
            )?;
            validate_private_command_result_semantics(&written, true)?;
            let retained = self.private_call(
                "read_output",
                json!({"output_ref":stdout_ref,"stream":"stdout","offset":0,"limit":4096}),
                None,
            )?;
            validate_private_read_output_semantics(&retained)?;
            let killed = self.private_call_with_timeout(
                "kill_session",
                json!({
                    "session_id":session_id,
                    "signal":"TERM",
                    "wait_ms":1_000,
                    "max_output_bytes":65_536,
                    "verbosity":"full"
                }),
                None,
                std::time::Duration::from_secs(4),
            )?;
            validate_private_command_result_semantics(&killed, false)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = self.private_call_with_timeout(
                "kill_session",
                json!({"session_id":session_id,"signal":"KILL","wait_ms":1_000,"max_output_bytes":4096}),
                None,
                std::time::Duration::from_secs(4),
            );
        }
        result
    }
}

impl WorkspaceRuntimeAdapter for CodingToolsRuntimeAdapter {
    fn negotiate(&mut self) -> Result<(), FacadeError> {
        let catalog = self.runtime.list_tools().map_err(normalize_runtime_error)?;
        validate_runtime_capabilities(&catalog)?;
        let probe = self.private_call("get_default_cwd", json!({}), None)?;
        validate_workspace_context_probe(&probe, &self.workspace)?;
        self.probe_private_result_semantics()
    }

    fn workspace_context(&mut self, request_id: Option<&Value>) -> Result<Value, FacadeError> {
        let cwd = self.private_call("get_default_cwd", json!({}), request_id)?;
        validate_workspace_context_probe(&cwd, &self.workspace)?;
        let structured = cwd
            .get("structuredContent")
            .and_then(Value::as_object)
            .ok_or_else(runtime_capability_mismatch)?;
        let default_cwd = structured
            .get("default_cwd")
            .and_then(Value::as_str)
            .ok_or_else(runtime_capability_mismatch)?;
        let data = json!({
            "api_version": AGENT_API_VERSION,
            "facade_revision": AGENT_API_REVISION,
            "workspace": self.workspace.to_string_lossy(),
            "default_cwd": default_cwd,
            "runtime": "ready"
        });
        Ok(stable_success(data, "LocalBridge workspace context ready"))
    }

    fn project_context(&self, path: &str) -> Result<Value, FacadeError> {
        let selected = self.resolve_existing_workspace_path(path)?;
        if !selected.is_dir() {
            return Err(FacadeError::new(
                FacadeErrorCode::InvalidArgument,
                "项目路径必须是目录",
                false,
            ));
        }
        Ok(json!({"selected_path":self.stable_workspace_relative_path(&selected)?}))
    }

    fn apply_directory_change(&mut self, action: &str, path: &str) -> Result<Value, FacadeError> {
        if !workspace_relative_path_valid(path) || path == "." {
            return Err(FacadeError::new(
                FacadeErrorCode::WorkspaceDenied,
                "目录路径必须位于当前工作区内",
                false,
            ));
        }
        let root = std::fs::canonicalize(&self.workspace).map_err(|_| {
            FacadeError::new(FacadeErrorCode::WorkspaceDenied, "工作区不可用", false)
        })?;
        let target = self.workspace.join(path);
        let public_path = path.replace('\\', "/");
        match action {
            "create_directory" => {
                if std::fs::symlink_metadata(&target).is_ok() {
                    return Err(FacadeError::new(
                        FacadeErrorCode::InvalidArgument,
                        "目标目录已存在",
                        false,
                    ));
                }
                let parent = target.parent().ok_or_else(invalid_argument)?;
                let canonical_parent = std::fs::canonicalize(parent).map_err(|_| {
                    FacadeError::new(FacadeErrorCode::NotFound, "父目录不存在", false)
                })?;
                if !canonical_parent.starts_with(&root) || !canonical_parent.is_dir() {
                    return Err(FacadeError::new(
                        FacadeErrorCode::WorkspaceDenied,
                        "目录路径越出当前工作区",
                        false,
                    ));
                }
                std::fs::create_dir(&target).map_err(|_| {
                    FacadeError::new(FacadeErrorCode::Internal, "创建目录失败", false)
                })?;
                let canonical_target = match std::fs::canonicalize(&target) {
                    Ok(value) if value.starts_with(&root) && value != root => value,
                    _ => {
                        let _ = std::fs::remove_dir(&target);
                        return Err(FacadeError::new(
                            FacadeErrorCode::WorkspaceDenied,
                            "目录路径越出当前工作区",
                            false,
                        ));
                    }
                };
                if !canonical_target.is_dir() {
                    let _ = std::fs::remove_dir(&target);
                    return Err(invalid_argument());
                }
                Ok(json!({"action":action,"path":public_path,"changed":true}))
            }
            "remove_empty_directory" => {
                let metadata = std::fs::symlink_metadata(&target).map_err(|_| {
                    FacadeError::new(FacadeErrorCode::NotFound, "目录不存在", false)
                })?;
                if metadata.file_type().is_symlink() {
                    return Err(FacadeError::new(
                        FacadeErrorCode::WorkspaceDenied,
                        "拒绝清理重解析目录",
                        false,
                    ));
                }
                let canonical_target = std::fs::canonicalize(&target).map_err(|_| {
                    FacadeError::new(FacadeErrorCode::NotFound, "目录不存在", false)
                })?;
                if !canonical_target.starts_with(&root) || canonical_target == root {
                    return Err(FacadeError::new(
                        FacadeErrorCode::WorkspaceDenied,
                        "目录路径越出当前工作区",
                        false,
                    ));
                }
                if !canonical_target.is_dir() {
                    return Err(invalid_argument());
                }
                if std::fs::read_dir(&canonical_target)
                    .map_err(|_| {
                        FacadeError::new(FacadeErrorCode::Internal, "读取目录失败", false)
                    })?
                    .next()
                    .is_some()
                {
                    return Err(FacadeError::new(
                        FacadeErrorCode::InvalidArgument,
                        "仅允许删除空目录",
                        false,
                    ));
                }
                std::fs::remove_dir(&canonical_target).map_err(|_| {
                    FacadeError::new(FacadeErrorCode::Internal, "删除空目录失败", false)
                })?;
                Ok(json!({"action":action,"path":public_path,"changed":true}))
            }
            _ => Err(invalid_argument()),
        }
    }

    fn execute_shell(
        &mut self,
        request: ShellCommandRequest,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let public_session_id = self.public_commands.start_session(&self.task_state)?;
        let outcome = (|| {
            let invocation = self
                .shell_executor
                .runtime_invocation(&request.execution)
                .map_err(normalize_shell_error)?;
            let mut private = json!({
                "cmd": invocation.command_line,
                "workdir": request.execution.cwd,
                "timeout_ms": request.execution.timeout_ms,
                "yield_time_ms": request.yield_time_ms,
                "max_output_bytes": request.execution.max_output_bytes,
                "verbosity":"full",
                "env":{"COMSPEC":invocation.comspec.to_string_lossy()}
            });
            if let Some(stdin) = request.stdin {
                private["stdin"] = Value::String(stdin);
            }
            let raw = self.private_call_with_timeout(
                "exec_command",
                private,
                request_id,
                command_transport_timeout(request.yield_time_ms),
            )?;
            if let Some(private_session_id) = raw
                .get("structuredContent")
                .and_then(Value::as_object)
                .and_then(|object| object.get("session_id"))
                .and_then(Value::as_str)
            {
                self.public_commands
                    .bind_private_session(&public_session_id, private_session_id)?;
            }
            self.normalize_command_result(&raw, &public_session_id, None)
        })();
        match outcome {
            Ok(result) => Ok(result),
            Err(error) => {
                self.public_commands.mark_error_terminal(
                    &public_session_id,
                    &error,
                    &self.task_state,
                )?;
                Err(error)
            }
        }
    }

    fn control_command(
        &mut self,
        action: CommandControlAction,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let object = arguments.as_object().ok_or_else(invalid_argument)?;
        if action == CommandControlAction::Read {
            let public_output_ref = required_string(object, "output_ref")?;
            let private_output_ref = self
                .public_commands
                .private_output(public_output_ref)
                .ok_or_else(session_unavailable)?;
            let mut private = Map::new();
            private.insert("output_ref".into(), Value::String(private_output_ref));
            for key in ["stream", "offset", "limit"] {
                if let Some(value) = object.get(key) {
                    private.insert(key.into(), value.clone());
                }
            }
            let raw = self.private_call("read_output", Value::Object(private), request_id)?;
            return Ok(Self::normalize_read_output(&raw, public_output_ref));
        }

        let public_session_id = required_string(object, "session_id")?.to_string();
        if action == CommandControlAction::Poll {
            if let Some(terminal) = self
                .public_commands
                .terminal_with_pending(&public_session_id)
            {
                return Ok(terminal);
            }
            if let Some(running) = self
                .public_commands
                .running_with_pending(&public_session_id)
            {
                return Ok(running);
            }
        } else if self.public_commands.terminal(&public_session_id).is_some() {
            return Err(session_unavailable());
        }
        let private_session_id = self
            .public_commands
            .private_session(&public_session_id)
            .ok_or_else(session_unavailable)?;
        let mut private = Map::new();
        private.insert("session_id".into(), Value::String(private_session_id));
        let private_name = match action {
            CommandControlAction::Poll => {
                private.insert("chars".into(), Value::String(String::new()));
                private.insert(
                    "yield_time_ms".into(),
                    Value::from(object.get("wait_ms").and_then(Value::as_u64).unwrap_or(0)),
                );
                private.insert("verbosity".into(), Value::String("full".into()));
                "write_stdin"
            }
            CommandControlAction::Write => {
                let chars = object
                    .get("chars")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(invalid_argument)?;
                private.insert("chars".into(), Value::String(chars.to_string()));
                private.insert(
                    "yield_time_ms".into(),
                    Value::from(object.get("wait_ms").and_then(Value::as_u64).unwrap_or(0)),
                );
                private.insert("verbosity".into(), Value::String("full".into()));
                "write_stdin"
            }
            CommandControlAction::Kill => {
                if let Some(signal) = object.get("signal") {
                    private.insert("signal".into(), signal.clone());
                }
                if let Some(wait_ms) = object.get("wait_ms") {
                    private.insert("wait_ms".into(), wait_ms.clone());
                }
                private.insert("verbosity".into(), Value::String("full".into()));
                "kill_session"
            }
            CommandControlAction::Read => unreachable!(),
        };
        let pending =
            if action == CommandControlAction::Write || action == CommandControlAction::Kill {
                self.public_commands.take_pending(&public_session_id)
            } else {
                String::new()
            };
        let call = match action {
            CommandControlAction::Poll | CommandControlAction::Write => {
                let wait_ms = object
                    .get("wait_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(30_000);
                self.private_call_with_timeout(
                    private_name,
                    Value::Object(private),
                    request_id,
                    command_transport_timeout(wait_ms),
                )
            }
            CommandControlAction::Kill => {
                let wait_ms = object
                    .get("wait_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(5_000)
                    .min(30_000);
                self.private_call_with_timeout(
                    private_name,
                    Value::Object(private),
                    request_id,
                    command_transport_timeout(wait_ms),
                )
            }
            CommandControlAction::Read => unreachable!(),
        };
        match call {
            Ok(raw) => {
                let result =
                    self.normalize_command_result(&raw, &public_session_id, Some(action))?;
                Ok(command_result_prepend_output(result, pending))
            }
            Err(error) => {
                self.public_commands
                    .append_pending(&public_session_id, &pending);
                self.public_commands.mark_error_terminal(
                    &public_session_id,
                    &error,
                    &self.task_state,
                )?;
                Err(error)
            }
        }
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
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let object = arguments.as_object().ok_or_else(invalid_argument)?;
        let relative = required_string(object, "path")?;
        let path = self.resolve_existing_workspace_path(relative)?;
        let raw = std::fs::read(&path)
            .map_err(|_| FacadeError::new(FacadeErrorCode::NotFound, "文档不可读", false))?;
        let source = std::str::from_utf8(&raw).map_err(|_| {
            FacadeError::new(FacadeErrorCode::InvalidArgument, "文档不是 UTF-8", false)
        })?;
        let start = object
            .get("start_line")
            .and_then(Value::as_u64)
            .unwrap_or(1) as usize;
        let requested_end = object
            .get("end_line")
            .and_then(Value::as_u64)
            .map(|v| v as usize);
        if requested_end.is_some_and(|end| start > end) {
            return Err(invalid_argument());
        }
        let max_lines = object
            .get("max_lines")
            .and_then(Value::as_u64)
            .unwrap_or(10_000) as usize;
        let max_bytes = object
            .get("max_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(1_048_576) as usize;
        let lines = if source.is_empty() {
            Vec::new()
        } else {
            source.split_inclusive('\n').collect::<Vec<_>>()
        };
        let total_lines = lines.len();
        let natural_end = start.saturating_add(max_lines.saturating_sub(1));
        let end = requested_end
            .unwrap_or(natural_end)
            .min(natural_end)
            .min(total_lines);
        let mut text = if start <= end && start > 0 {
            lines[start - 1..end].concat()
        } else {
            String::new()
        };
        let mut truncated =
            requested_end.is_some_and(|value| value > end) || natural_end < total_lines;
        if text.len() > max_bytes {
            let mut boundary = max_bytes.min(text.len());
            while boundary > 0 && !text.is_char_boundary(boundary) {
                boundary -= 1;
            }
            text.truncate(boundary);
            truncated = true;
        }
        let bytes_read = text.len();
        Ok(stable_success(
            json!({
                "text":text,
                "path":relative,
                "encoding":"utf-8",
                "start_line":start,
                "end_line":end,
                "total_lines":total_lines,
                "total_bytes":raw.len(),
                "bytes_read":bytes_read,
                "truncated":truncated
            }),
            "Document inspected",
        ))
    }

    fn apply_document_patch(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        self.private_call("apply_patch", arguments, request_id)?;
        Ok(stable_success(
            json!({"applied":true}),
            "Document workflow applied",
        ))
    }

    fn inspect_image(
        &mut self,
        arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let object = arguments.as_object().ok_or_else(invalid_argument)?;
        let relative = required_string(object, "path")?;
        let path = self.resolve_existing_workspace_path(relative)?;
        let bytes = std::fs::read(path)
            .map_err(|_| FacadeError::new(FacadeErrorCode::NotFound, "图像不可读", false))?;
        let image = image::load_from_memory(&bytes).map_err(|_| {
            FacadeError::new(FacadeErrorCode::InvalidArgument, "图像格式不受支持", false)
        })?;
        let original_width = image.width();
        let original_height = image.height();
        let max_width = object
            .get("max_width")
            .and_then(Value::as_u64)
            .unwrap_or(original_width as u64) as u32;
        let max_height = object
            .get("max_height")
            .and_then(Value::as_u64)
            .unwrap_or(original_height as u64) as u32;
        let auto_resize = object
            .get("auto_resize")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let resized = auto_resize && (original_width > max_width || original_height > max_height);
        let image = if resized {
            image.resize(max_width, max_height, image::imageops::FilterType::Lanczos3)
        } else {
            image
        };
        let width = image.width();
        let height = image.height();
        let mut cursor = Cursor::new(Vec::new());
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|_| {
                FacadeError::new(FacadeErrorCode::RuntimeUnavailable, "图像编码失败", false)
            })?;
        let encoded_bytes = cursor.into_inner();
        let max_bytes = object
            .get("max_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(10_485_760) as usize;
        if encoded_bytes.len() > max_bytes {
            return Err(FacadeError::new(
                FacadeErrorCode::OutputTruncated,
                "图像超过输出大小限制",
                false,
            ));
        }
        let encoded = base64::engine::general_purpose::STANDARD.encode(encoded_bytes);
        Ok(json!({
            "content":[{"type":"image","data":encoded,"mimeType":"image/png"}],
            "structuredContent":{"ok":true,"data":{
                "kind":"image",
                "path":relative,
                "mime_type":"image/png",
                "original_width":original_width,
                "original_height":original_height,
                "width":width,
                "height":height,
                "resized":resized
            }},
            "isError":false
        }))
    }

    fn root_is_running(&self) -> Result<Option<bool>, CodingToolsRuntimeError> {
        self.runtime.root_is_running().map(Some)
    }

    fn reap_command_sessions(&mut self) -> Result<(), FacadeError> {
        match self.runtime.root_is_running() {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                self.public_commands
                    .mark_all_running_lost(&self.task_state)?;
                return Ok(());
            }
        }
        let running = self.public_commands.running_sessions();
        for (public_session_id, private_session_id) in running {
            let private = json!({
                "session_id": private_session_id,
                "chars": "",
                "yield_time_ms": 0,
                "max_output_bytes": 65536,
                "verbosity":"full"
            });
            match self.private_call("write_stdin", private, None) {
                Ok(raw) => {
                    let normalized = self.normalize_command_result(
                        &raw,
                        &public_session_id,
                        Some(CommandControlAction::Poll),
                    )?;
                    let delta = normalized
                        .pointer("/structuredContent/data/output")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    self.public_commands
                        .append_pending(&public_session_id, &delta);
                }
                Err(error) => {
                    self.public_commands.mark_error_terminal(
                        &public_session_id,
                        &error,
                        &self.task_state,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn has_running_command_session(&self) -> bool {
        self.public_commands.has_running_session()
    }
}

impl CodingToolsRuntimeAdapter {
    fn normalize_command_result(
        &mut self,
        raw: &Value,
        public_session_id: &str,
        action: Option<CommandControlAction>,
    ) -> Result<Value, FacadeError> {
        let structured = raw.get("structuredContent").and_then(Value::as_object);
        let private_status = structured
            .and_then(|object| object.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("exited");
        let exit_code = structured
            .and_then(|object| object.get("exit_code"))
            .and_then(Value::as_i64);
        let timed_out = structured
            .and_then(|object| object.get("timed_out"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let public_status = if action == Some(CommandControlAction::Kill) {
            "cancelled"
        } else {
            command_public_status(private_status, exit_code, timed_out)
        };

        let mut data = Map::new();
        data.insert("status".into(), Value::String(public_status.into()));
        if let Some(exit_code) = exit_code {
            data.insert("exit_code".into(), Value::from(exit_code));
        }
        if let Some(signal) = structured
            .and_then(|object| object.get("signal"))
            .and_then(Value::as_str)
        {
            data.insert("signal".into(), Value::String(signal.to_string()));
        }
        data.insert(
            "session_id".into(),
            Value::String(public_session_id.to_string()),
        );
        if let Some(value) = structured.and_then(|object| object.get("truncated")) {
            if value.is_boolean() {
                data.insert("truncated".into(), value.clone());
            }
        }
        let output = self.safe_command_output_for_session(raw, public_session_id);
        data.insert("output".into(), Value::String(output));
        self.map_private_output_refs(structured, &mut data);

        let successful_kill_data = (action == Some(CommandControlAction::Kill)
            && public_status == "cancelled")
            .then(|| data.clone());
        let result = match public_status {
            "failed" => stable_command_error(FacadeErrorCode::ProcessFailed, "命令执行失败", data),
            "timed_out" => {
                stable_command_error(FacadeErrorCode::ProcessTimedOut, "命令执行超时", data)
            }
            "cancelled" => {
                stable_command_error(FacadeErrorCode::ProcessCancelled, "命令已取消", data)
            }
            _ => stable_success(Value::Object(data), "Command completed"),
        };
        if public_status != "running" {
            self.public_commands.mark_terminal(
                public_session_id,
                result.clone(),
                &self.task_state,
            )?;
        }
        if let Some(data) = successful_kill_data {
            return Ok(stable_success(Value::Object(data), "Command terminated"));
        }
        Ok(result)
    }

    fn safe_command_output_for_session(&mut self, raw: &Value, public_session_id: &str) -> String {
        let Some(structured) = raw.get("structuredContent").and_then(Value::as_object) else {
            return String::new();
        };
        let stdout = structured
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let stderr = structured
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let stderr = self
            .public_commands
            .filter_private_stderr(public_session_id, stderr);
        [stdout, stderr.as_str()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(if stdout.is_empty() || stderr.is_empty() {
                ""
            } else {
                "\n"
            })
    }

    fn normalize_read_output(raw: &Value, public_output_ref: &str) -> Value {
        let structured = raw.get("structuredContent").and_then(Value::as_object);
        let mut data = Map::new();
        data.insert(
            "output_ref".into(),
            Value::String(public_output_ref.to_string()),
        );
        for key in [
            "stream",
            "offset",
            "requested_offset",
            "limit",
            "next_offset",
            "truncated",
        ] {
            if let Some(value) = structured.and_then(|object| object.get(key)) {
                data.insert(key.into(), value.clone());
            }
        }
        if let Some(content) = structured
            .and_then(|object| object.get("content"))
            .and_then(Value::as_str)
        {
            let stream = structured
                .and_then(|object| object.get("stream"))
                .and_then(Value::as_str)
                .unwrap_or("stdout");
            let content = if stream == "stderr" {
                public_command_stderr(content)
            } else {
                content.to_string()
            };
            data.insert("content".into(), Value::String(content));
        }
        stable_success(Value::Object(data), "Command output read")
    }

    fn map_private_output_refs(
        &mut self,
        structured: Option<&Map<String, Value>>,
        data: &mut Map<String, Value>,
    ) {
        let Some(structured) = structured else {
            return;
        };
        if let Some(private) = structured.get("output_ref").and_then(Value::as_str) {
            data.insert(
                "output_ref".into(),
                Value::String(self.public_commands.public_output_for_private(private)),
            );
        }
        if let Some(private_refs) = structured.get("output_refs").and_then(Value::as_object) {
            let mut public_refs = Map::new();
            for stream in ["stdout", "stderr"] {
                if let Some(private) = private_refs.get(stream).and_then(Value::as_str) {
                    public_refs.insert(
                        stream.into(),
                        Value::String(self.public_commands.public_output_for_private(private)),
                    );
                }
            }
            if !public_refs.is_empty() {
                data.insert("output_refs".into(), Value::Object(public_refs));
            }
        }
    }
}

fn command_public_status(
    private_status: &str,
    exit_code: Option<i64>,
    timed_out: bool,
) -> &'static str {
    if timed_out || private_status == "timeout" {
        "timed_out"
    } else if matches!(private_status, "terminated" | "killed") {
        "cancelled"
    } else if matches!(private_status, "terminating" | "running") {
        "running"
    } else if exit_code.is_some_and(|code| code != 0) {
        "failed"
    } else {
        "completed"
    }
}

fn terminal_snapshot_from_result(owner: CommandOwner, result: &Value) -> TerminalCommandSnapshot {
    let data = result
        .pointer("/structuredContent/data")
        .and_then(Value::as_object);
    let error_code = result
        .pointer("/structuredContent/error/code")
        .and_then(Value::as_str)
        .map(str::to_string);
    let status = match data
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
    {
        Some("completed") => CommandTerminalStatus::Completed,
        Some("timed_out") => CommandTerminalStatus::TimedOut,
        Some("cancelled") => CommandTerminalStatus::Cancelled,
        Some("failed") => CommandTerminalStatus::Failed,
        _ => match error_code.as_deref() {
            Some("ProcessTimedOut") => CommandTerminalStatus::TimedOut,
            Some("ProcessCancelled") => CommandTerminalStatus::Cancelled,
            Some(
                "SessionUnavailable"
                | "RuntimeUnavailable"
                | "RuntimeProtocolMismatch"
                | "RuntimeCapabilityMismatch",
            ) => CommandTerminalStatus::Lost,
            _ => CommandTerminalStatus::Failed,
        },
    };
    let mut output_refs = Vec::new();
    if let Some(value) = data
        .and_then(|value| value.get("output_ref"))
        .and_then(Value::as_str)
    {
        output_refs.push(value.to_string());
    }
    if let Some(values) = data
        .and_then(|value| value.get("output_refs"))
        .and_then(Value::as_object)
    {
        for stream in ["stdout", "stderr"] {
            if let Some(value) = values.get(stream).and_then(Value::as_str) {
                if !output_refs.iter().any(|existing| existing == value) {
                    output_refs.push(value.to_string());
                }
            }
        }
    }
    TerminalCommandSnapshot::new(
        owner,
        status,
        data.and_then(|value| value.get("exit_code"))
            .and_then(Value::as_i64),
        data.and_then(|value| value.get("signal"))
            .and_then(Value::as_str)
            .map(str::to_string),
        status == CommandTerminalStatus::TimedOut,
        status == CommandTerminalStatus::Cancelled,
        output_refs,
        error_code,
    )
}

fn normalize_task_state_error(_error: CommandTaskStateError) -> FacadeError {
    command_state_internal_error()
}

fn command_state_internal_error() -> FacadeError {
    FacadeError::new(
        FacadeErrorCode::RuntimeUnavailable,
        "命令任务状态持久化失败",
        false,
    )
}

fn command_transport_timeout(wait_ms: u64) -> std::time::Duration {
    std::time::Duration::from_millis(wait_ms.min(30_000).saturating_add(3_000))
}

fn validate_private_command_result_semantics(
    raw: &Value,
    running_required: bool,
) -> Result<(String, String), FacadeError> {
    let structured = raw
        .get("structuredContent")
        .and_then(Value::as_object)
        .ok_or_else(runtime_capability_mismatch)?;
    let session_id = structured
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(runtime_capability_mismatch)?;
    let status = structured
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(runtime_capability_mismatch)?;
    if running_required && status != "running" {
        return Err(runtime_capability_mismatch());
    }
    if !matches!(
        status,
        "running" | "exited" | "terminated" | "killed" | "terminating" | "timeout"
    ) {
        return Err(runtime_capability_mismatch());
    }
    for key in ["stdout", "stderr"] {
        if !structured.get(key).is_some_and(Value::is_string) {
            return Err(runtime_capability_mismatch());
        }
    }
    for key in ["timed_out", "truncated"] {
        if !structured.get(key).is_some_and(Value::is_boolean) {
            return Err(runtime_capability_mismatch());
        }
    }
    if !structured
        .get("exit_code")
        .is_some_and(|value| value.is_null() || value.is_i64() || value.is_u64())
    {
        return Err(runtime_capability_mismatch());
    }
    let output_ref = structured
        .get("output_ref")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(runtime_capability_mismatch)?;
    let refs = structured
        .get("output_refs")
        .and_then(Value::as_object)
        .ok_or_else(runtime_capability_mismatch)?;
    let stdout_ref = refs
        .get("stdout")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(runtime_capability_mismatch)?;
    if refs
        .get("stderr")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .is_none()
        || output_ref.is_empty()
    {
        return Err(runtime_capability_mismatch());
    }
    Ok((session_id.to_string(), stdout_ref.to_string()))
}

fn validate_private_read_output_semantics(raw: &Value) -> Result<(), FacadeError> {
    let structured = raw
        .get("structuredContent")
        .and_then(Value::as_object)
        .ok_or_else(runtime_capability_mismatch)?;
    for key in ["output_ref", "stream", "content"] {
        if !structured.get(key).is_some_and(Value::is_string) {
            return Err(runtime_capability_mismatch());
        }
    }
    for key in ["offset", "requested_offset", "limit"] {
        if !structured
            .get(key)
            .is_some_and(|value| value.is_u64() || value.is_i64())
        {
            return Err(runtime_capability_mismatch());
        }
    }
    if !structured
        .get("next_offset")
        .is_some_and(|value| value.is_null() || value.is_u64() || value.is_i64())
        || !structured.get("truncated").is_some_and(Value::is_boolean)
    {
        return Err(runtime_capability_mismatch());
    }
    Ok(())
}

fn validate_workspace_context_probe(
    raw: &Value,
    expected_workspace: &Path,
) -> Result<(), FacadeError> {
    let structured = raw
        .get("structuredContent")
        .and_then(Value::as_object)
        .ok_or_else(runtime_capability_mismatch)?;
    let workspace = structured
        .get("workspace")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(runtime_capability_mismatch)?;
    let default_cwd = structured
        .get("default_cwd")
        .and_then(Value::as_str)
        .ok_or_else(runtime_capability_mismatch)?;
    if !expected_workspace.is_absolute()
        || expected_workspace.to_string_lossy().starts_with(r"\\?\")
        || expected_workspace.to_string_lossy().starts_with("//?/")
        || !Path::new(workspace).is_absolute()
        || workspace.starts_with(r"\\?\")
        || workspace.starts_with("//?/")
        || !ordinary_workspace_paths_match(expected_workspace, workspace)
        || !workspace_relative_path_valid(default_cwd)
    {
        return Err(runtime_capability_mismatch());
    }
    Ok(())
}

fn ordinary_workspace_paths_match(expected: &Path, actual: &str) -> bool {
    fn normalize(value: &str) -> String {
        value.replace('/', "\\").trim_end_matches('\\').to_string()
    }
    let expected = normalize(&expected.to_string_lossy());
    let actual = normalize(actual);
    #[cfg(windows)]
    {
        expected.eq_ignore_ascii_case(&actual)
    }
    #[cfg(not(windows))]
    {
        expected == actual
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
        let adapter = CodingToolsRuntimeAdapter::new(runtime)?;
        Self::with_adapter(adapter, policy)
    }

    pub(crate) fn cancellation_client(
        &self,
    ) -> Result<McpCancellationClient, CodingToolsRuntimeError> {
        self.adapter.cancellation_client()
    }

    pub(crate) fn command_task_state(&self) -> CommandTaskStateStore {
        self.adapter.command_task_state()
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
            .filter(|name| self.policy.public_tool_allowed_for_list(mode, name))
            .map(|name| public_tool_schema(name))
            .collect::<Vec<_>>();
        json!({"tools":tools})
    }

    pub fn replace_policy(&mut self, policy: CapabilityPolicy) {
        self.policy = policy;
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

    pub fn reap_command_sessions(&mut self) -> Result<(), FacadeError> {
        self.adapter.reap_command_sessions()
    }

    pub fn has_running_command_session(&self) -> bool {
        self.adapter.has_running_command_session()
    }

    pub fn authorize_public_request(
        &self,
        mode: PermissionMode,
        name: &str,
        arguments: &Value,
    ) -> Result<(), FacadeCallError> {
        if !self.registry.contains(name) {
            return Err(FacadeCallError::Denied(FacadeDenied {
                reason: DenyReason::UnknownTool,
                capability: Capability::Unknown,
            }));
        }
        let decision = self.policy.decide_public(mode, name, arguments);
        if !decision.allowed {
            return Err(FacadeCallError::Denied(FacadeDenied {
                reason: decision
                    .deny_reason
                    .expect("denied policy decision contains reason"),
                capability: decision.descriptor.capability,
            }));
        }
        Ok(())
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
        let kind = public_task_kind(name, &arguments);
        let summary = public_safe_summary(name, &arguments);
        if let Err(FacadeCallError::Denied(denied)) =
            self.authorize_public_request(mode, name, &arguments)
        {
            project(
                CurrentTaskStatus::project(kind, summary, TaskExecutionState::Blocked)
                    .expect("Blocked is valid"),
            );
            project(CurrentTaskStatus::Idle);
            return Err(FacadeCallError::Denied(denied));
        }
        if !public_workspace_paths_valid(name, &arguments) {
            project(
                CurrentTaskStatus::project(kind, summary, TaskExecutionState::Blocked)
                    .expect("Blocked is valid"),
            );
            project(CurrentTaskStatus::Idle);
            return Ok(FacadeError::new(
                FacadeErrorCode::WorkspaceDenied,
                "工作区路径参数无效",
                false,
            )
            .to_mcp_result());
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
        let command_lifecycle_remains_running =
            matches!(kind, TaskKind::ExecuteCommand) && self.adapter.has_running_command_session();
        if !command_lifecycle_remains_running {
            project(CurrentTaskStatus::Idle);
        }
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
            "agent_workflow" => self.agent_workflow(arguments, request_id),
            "exec_command" => self.exec_command(arguments, request_id),
            "command_control" => self.command_control(arguments, request_id),
            "git_workflow" => self.git_workflow(arguments, request_id),
            "document_workflow" => self.document_workflow(arguments, request_id),
            "view_image" => self.view_image(arguments, request_id),
            "task_control" => Ok(stable_success(
                json!({"delegated_to":"localbridge_server_task_controller"}),
                "Task control delegated",
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

    fn agent_workflow(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let object = object_args(&arguments)?;
        let action = required_string(object, "action")?;
        if !matches!(
            action,
            "diagnose"
                | "bugfix"
                | "feature"
                | "refactor"
                | "test_failure"
                | "build_release"
                | "document"
                | "resume"
                | "custom"
        ) {
            return Err(invalid_argument());
        }
        let patch = object.get("patch").and_then(Value::as_str);
        let project_path = match object.get("path") {
            Some(value) => value.as_str().ok_or_else(invalid_argument)?,
            None => ".",
        };
        let mut project = self.adapter.project_context(project_path)?;
        let selected_path = project
            .get("selected_path")
            .and_then(Value::as_str)
            .ok_or_else(|| FacadeError::new(FacadeErrorCode::Internal, "项目上下文无效", false))?
            .to_string();
        let directory_changes = parse_directory_changes(object)?;
        let commands = object
            .get("commands")
            .map(|value| value.as_array().ok_or_else(invalid_argument))
            .transpose()?
            .cloned()
            .unwrap_or_default();
        if patch.is_some() && !agent_action_allows_write(action) {
            return Err(invalid_argument());
        }
        if !commands.is_empty() && !agent_action_allows_process(action) {
            return Err(invalid_argument());
        }
        let workspace = self.adapter.workspace_context(request_id)?;
        let git_before = self.adapter.git_workflow(
            GitWorkflowAction::Status,
            json!({"path":selected_path}),
            request_id,
        )?;
        if let Some(project_object) = project.as_object_mut() {
            let git_data = stable_data(&git_before);
            project_object.insert(
                "is_repo".into(),
                git_data
                    .get("is_repo")
                    .cloned()
                    .unwrap_or(Value::Bool(false)),
            );
            project_object.insert(
                "repository_root".into(),
                git_data
                    .get("repository_root")
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }

        let mut directory_results = Vec::with_capacity(directory_changes.len());
        for (directory_action, directory_path) in &directory_changes {
            directory_results.push(
                self.adapter
                    .apply_directory_change(directory_action, directory_path)?,
            );
        }

        let mut applied_patch = false;
        if let Some(patch) = patch {
            if !public_patch_targets_valid(patch) {
                return Err(FacadeError::new(
                    FacadeErrorCode::WorkspaceDenied,
                    "补丁目标不在当前工作区内",
                    false,
                ));
            }
            self.adapter
                .apply_document_patch(json!({"patch":patch,"dry_run":false}), request_id)?;
            applied_patch = true;
        }

        let mut command_results = Vec::new();
        for command in commands {
            let command = command.as_object().ok_or_else(invalid_argument)?;
            let text = required_string(command, "command")?;
            let shell: ShellSelector = serde_json::from_value(
                command
                    .get("shell")
                    .cloned()
                    .unwrap_or_else(|| Value::String("auto".into())),
            )
            .map_err(|_| invalid_argument())?;
            let workdir = command
                .get("workdir")
                .and_then(Value::as_str)
                .unwrap_or(".");
            if !workspace_relative_path_valid(workdir) {
                return Err(FacadeError::new(
                    FacadeErrorCode::WorkspaceDenied,
                    "工作区路径参数无效",
                    false,
                ));
            }
            let effective_workdir = join_project_workdir(&selected_path, workdir)?;
            let result = self.adapter.execute_shell(
                ShellCommandRequest {
                    execution: ShellExecutionSpec {
                        shell,
                        command: text.to_string(),
                        cwd: effective_workdir,
                        timeout_ms: command
                            .get("timeout_ms")
                            .and_then(Value::as_u64)
                            .unwrap_or(30_000),
                        max_output_bytes: command
                            .get("max_output_bytes")
                            .and_then(Value::as_u64)
                            .unwrap_or(65_536) as usize,
                    },
                    yield_time_ms: command
                        .get("yield_time_ms")
                        .and_then(Value::as_u64)
                        .unwrap_or(10_000),
                    stdin: command
                        .get("stdin")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                },
                request_id,
            )?;
            if result.get("isError").and_then(Value::as_bool) == Some(true) {
                return Ok(result);
            }
            let data = stable_data(&result);
            let running = data.get("status").and_then(Value::as_str) == Some("running");
            command_results.push(data.clone());
            if running {
                return Ok(stable_success(
                    json!({
                        "action":action,
                        "objective":object.get("objective").and_then(Value::as_str),
                        "state":"running",
                        "workspace":stable_data(&workspace),
                        "project":project,
                        "git_before":stable_data(&git_before),
                        "patch_applied":applied_patch,
                        "directory_changes":directory_results,
                        "commands":command_results
                    }),
                    "Agent workflow command is running",
                ));
            }
        }

        let git_after = self.adapter.git_workflow(
            GitWorkflowAction::Status,
            json!({"path":selected_path}),
            request_id,
        )?;
        let state = if applied_patch || !directory_results.is_empty() || !command_results.is_empty()
        {
            "completed"
        } else {
            "context_ready"
        };
        Ok(stable_success(
            json!({
                "action":action,
                "objective":object.get("objective").and_then(Value::as_str),
                "state":state,
                "workspace":stable_data(&workspace),
                "project":project,
                "git_before":stable_data(&git_before),
                "git_after":stable_data(&git_after),
                "patch_applied":applied_patch,
                "directory_changes":directory_results,
                "commands":command_results
            }),
            "Agent workflow completed",
        ))
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
        match action {
            CommandControlAction::Read => {
                required_string(object, "output_ref")?;
            }
            CommandControlAction::Write => {
                required_string(object, "session_id")?;
                required_string(object, "chars")?;
            }
            CommandControlAction::Poll | CommandControlAction::Kill => {
                required_string(object, "session_id")?;
            }
        }
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
                if let (Some(start), Some(end)) = (
                    object.get("start_line").and_then(Value::as_u64),
                    object.get("end_line").and_then(Value::as_u64),
                ) {
                    if start > end {
                        return Err(invalid_argument());
                    }
                }
                let mut private = json!({"path":path});
                for key in ["start_line", "end_line", "max_lines", "max_bytes"] {
                    if let Some(value) = object.get(key) {
                        private[key] = value.clone();
                    }
                }
                self.adapter.inspect_document(private, request_id)
            }
            "create" => {
                let path = required_string(object, "path")?;
                let content = object
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid_argument)?;
                let patch = document_add_patch(path, content);
                self.adapter
                    .apply_document_patch(json!({"patch":patch,"dry_run":false}), request_id)?;
                Ok(stable_success(
                    json!({"action":"create","path":path,"created":true}),
                    "Document created",
                ))
            }
            "convert" => {
                let source = required_string(object, "source")?;
                let path = required_string(object, "path")?;
                let inspected = self.adapter.inspect_document(
                    json!({"path":source,"start_line":1,"max_lines":100000,"max_bytes":1048576}),
                    request_id,
                )?;
                let content = stable_document_text(&inspected)?;
                let patch = document_add_patch(path, content);
                self.adapter
                    .apply_document_patch(json!({"patch":patch,"dry_run":false}), request_id)?;
                Ok(stable_success(
                    json!({"action":"convert","source":source,"path":path,"converted":true}),
                    "Document converted",
                ))
            }
            "rebuild" => {
                let path = required_string(object, "path")?;
                let content = object
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid_argument)?;
                let inspected = self.adapter.inspect_document(
                    json!({"path":path,"start_line":1,"max_lines":100000,"max_bytes":1048576}),
                    request_id,
                )?;
                let existing = stable_document_text(&inspected)?;
                let patch = document_rebuild_patch(path, existing, content);
                self.adapter
                    .apply_document_patch(json!({"patch":patch,"dry_run":false}), request_id)?;
                Ok(stable_success(
                    json!({"action":"rebuild","path":path,"rebuilt":true}),
                    "Document rebuilt",
                ))
            }
            _ => Err(invalid_argument()),
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

fn stable_document_text(value: &Value) -> Result<&str, FacadeError> {
    value
        .get("structuredContent")
        .and_then(|structured| structured.get("data"))
        .and_then(|data| data.get("text"))
        .and_then(Value::as_str)
        .ok_or_else(runtime_capability_mismatch)
}

fn document_add_patch(path: &str, content: &str) -> String {
    let normalized = normalize_document_text(content);
    let mut patch = format!("*** Begin Patch\n*** Add File: {path}\n");
    for line in normalized.split_terminator('\n') {
        patch.push('+');
        patch.push_str(line);
        patch.push('\n');
    }
    patch.push_str("*** End Patch");
    patch
}

fn document_rebuild_patch(path: &str, existing: &str, content: &str) -> String {
    let existing = normalize_document_text(existing);
    let content = normalize_document_text(content);
    let mut patch = format!("*** Begin Patch\n*** Update File: {path}\n@@\n");
    for line in existing.split_terminator('\n') {
        patch.push('-');
        patch.push_str(line);
        patch.push('\n');
    }
    for line in content.split_terminator('\n') {
        patch.push('+');
        patch.push_str(line);
        patch.push('\n');
    }
    patch.push_str("*** End Patch");
    patch
}

fn normalize_document_text(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn public_workspace_paths_valid(name: &str, arguments: &Value) -> bool {
    let Some(object) = arguments.as_object() else {
        return true;
    };
    let keys: &[&str] = match name {
        "exec_command" => &["workdir"],
        "git_workflow" | "view_image" => &["path"],
        "document_workflow" => &["path", "source"],
        "agent_workflow" => &["path"],
        _ => &[],
    };
    if keys.iter().any(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|value| !workspace_relative_path_valid(value))
    }) {
        return false;
    }
    if name == "git_workflow"
        && object
            .get("paths")
            .and_then(Value::as_array)
            .is_some_and(|values| {
                values.iter().any(|value| {
                    value
                        .as_str()
                        .is_none_or(|value| !workspace_relative_path_valid(value))
                })
            })
    {
        return false;
    }
    if name == "agent_workflow"
        && object
            .get("directory_changes")
            .and_then(Value::as_array)
            .is_some_and(|changes| {
                changes.iter().any(|change| {
                    change
                        .as_object()
                        .and_then(|change| change.get("path"))
                        .and_then(Value::as_str)
                        .is_none_or(|path| !workspace_relative_path_valid(path))
                })
            })
    {
        return false;
    }
    true
}

fn parse_directory_changes(
    object: &Map<String, Value>,
) -> Result<Vec<(String, String)>, FacadeError> {
    let Some(value) = object.get("directory_changes") else {
        return Ok(Vec::new());
    };
    let changes = value.as_array().ok_or_else(invalid_argument)?;
    if changes.len() > 32 {
        return Err(invalid_argument());
    }
    changes
        .iter()
        .map(|change| {
            let change = change.as_object().ok_or_else(invalid_argument)?;
            if change.len() != 2 || !change.contains_key("action") || !change.contains_key("path") {
                return Err(invalid_argument());
            }
            let action = required_string(change, "action")?;
            if !matches!(action, "create_directory" | "remove_empty_directory") {
                return Err(invalid_argument());
            }
            let path = required_string(change, "path")?;
            if !workspace_relative_path_valid(path) || path == "." {
                return Err(FacadeError::new(
                    FacadeErrorCode::WorkspaceDenied,
                    "目录路径必须位于当前工作区内",
                    false,
                ));
            }
            Ok((action.to_string(), path.to_string()))
        })
        .collect()
}

fn join_project_workdir(project: &str, workdir: &str) -> Result<PathBuf, FacadeError> {
    if !workspace_relative_path_valid(project) || !workspace_relative_path_valid(workdir) {
        return Err(FacadeError::new(
            FacadeErrorCode::WorkspaceDenied,
            "工作区路径参数无效",
            false,
        ));
    }
    let combined = match (project, workdir) {
        (".", ".") => PathBuf::from("."),
        (".", workdir) => PathBuf::from(workdir),
        (project, ".") => PathBuf::from(project),
        (project, workdir) => Path::new(project).join(workdir),
    };
    let rendered = combined.to_string_lossy();
    workspace_relative_path_valid(&rendered)
        .then_some(combined)
        .ok_or_else(|| {
            FacadeError::new(
                FacadeErrorCode::WorkspaceDenied,
                "工作区路径参数无效",
                false,
            )
        })
}

fn agent_action_allows_write(action: &str) -> bool {
    matches!(
        action,
        "bugfix" | "feature" | "refactor" | "test_failure" | "document" | "resume" | "custom"
    )
}

fn agent_action_allows_process(action: &str) -> bool {
    matches!(
        action,
        "diagnose"
            | "bugfix"
            | "feature"
            | "refactor"
            | "test_failure"
            | "build_release"
            | "resume"
            | "custom"
    )
}

fn public_patch_targets_valid(patch: &str) -> bool {
    let mut lines = patch.lines();
    if lines.next() != Some("*** Begin Patch") {
        return false;
    }
    let mut saw_operation = false;
    let mut saw_end = false;
    for line in lines {
        if line == "*** End Patch" {
            saw_end = true;
            break;
        }
        let target = [
            "*** Add File: ",
            "*** Update File: ",
            "*** Delete File: ",
            "*** Move to: ",
        ]
        .into_iter()
        .find_map(|prefix| line.strip_prefix(prefix));
        if let Some(target) = target {
            saw_operation = true;
            if !workspace_relative_path_valid(target) {
                return false;
            }
        } else if line.starts_with("*** ") && line != "*** End of File" {
            return false;
        }
    }
    saw_operation
        && saw_end
        && !patch
            .lines()
            .skip_while(|line| *line != "*** End Patch")
            .skip(1)
            .any(|line| !line.is_empty())
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

fn normalize_path_authority_error(error: PathAuthorityError) -> FacadeError {
    match error {
        PathAuthorityError::InvalidPath | PathAuthorityError::OutsideAuthority => FacadeError::new(
            FacadeErrorCode::WorkspaceDenied,
            "工作区路径参数无效",
            false,
        ),
        PathAuthorityError::NotFound => FacadeError::new(
            FacadeErrorCode::NotFound,
            "工作区文件不存在",
            false,
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
    let permission = raw
        .pointer("/structuredContent/error/details/permission")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let public = if code.contains("SESSION_NOT_FOUND") || code.contains("SESSION_CLOSED") {
        FacadeErrorCode::SessionUnavailable
    } else if code.contains("INVALID") {
        FacadeErrorCode::InvalidArgument
    } else if code.contains("NOT_FOUND") || code.contains("MISSING") {
        FacadeErrorCode::NotFound
    } else if code.contains("OUTSIDE_WORKSPACE")
        || code.contains("WORKSPACE_DENIED")
        || code.contains("ABSOLUTE_PATH_DENIED")
        || code.contains("SYMLINK_ESCAPE")
        || permission == "filesystem_escape"
    {
        FacadeErrorCode::WorkspaceDenied
    } else if code.contains("PERMISSION") || code.contains("CAPABILITY") {
        FacadeErrorCode::CapabilityDenied
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

fn session_unavailable() -> FacadeError {
    FacadeError::new(FacadeErrorCode::SessionUnavailable, "命令会话不可用", false)
}

fn stable_success(data: Value, text: &str) -> Value {
    json!({
        "content":[{"type":"text","text":text}],
        "structuredContent":{"ok":true,"data":data},
        "isError":false
    })
}

fn stable_data(value: &Value) -> Value {
    value
        .get("structuredContent")
        .and_then(|structured| structured.get("data"))
        .cloned()
        .unwrap_or(Value::Null)
}

fn stable_command_error(code: FacadeErrorCode, message: &str, data: Map<String, Value>) -> Value {
    json!({
        "content":[{"type":"text","text":message}],
        "structuredContent":{
            "ok":false,
            "error":{"code":code.as_str(),"message":message,"retryable":false},
            "data":Value::Object(data)
        },
        "isError":true
    })
}

fn command_result_with_output(mut result: Value, output: String) -> Value {
    if let Some(data) = result
        .get_mut("structuredContent")
        .and_then(|value| value.get_mut("data"))
        .and_then(Value::as_object_mut)
    {
        data.insert("output".into(), Value::String(output));
    }
    result
}

fn command_result_prepend_output(result: Value, prefix: String) -> Value {
    if prefix.is_empty() {
        return result;
    }
    let suffix = result
        .pointer("/structuredContent/data/output")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    command_result_with_output(result, format!("{prefix}{suffix}"))
}

#[cfg(test)]
fn safe_command_output(raw: &Value) -> String {
    let Some(structured) = raw.get("structuredContent").and_then(Value::as_object) else {
        return String::new();
    };
    let stdout = structured
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let stderr = structured
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let stderr = public_command_stderr(stderr);
    if !stdout.is_empty() || !stderr.is_empty() {
        return [stdout, stderr.as_str()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(if stdout.is_empty() || stderr.is_empty() {
                ""
            } else {
                "\n"
            });
    }
    for key in ["content", "preview", "summary"] {
        if let Some(value) = structured.get(key).and_then(Value::as_str) {
            return value.to_string();
        }
    }
    String::new()
}

fn public_command_stderr(stderr: &str) -> String {
    if !looks_like_clixml_protocol(stderr) {
        return stderr.to_string();
    }
    let lower = stderr.to_ascii_lowercase();
    if !lower.contains("s=\"error\"") {
        return String::new();
    }
    strip_private_powershell_prologue(&extract_clixml_error_strings(stderr))
}

fn strip_private_powershell_prologue(value: &str) -> String {
    let contains_private_prologue = value.contains("PSModuleAutoLoadingPreference")
        || value.contains("Microsoft.PowerShell.Management.psd1")
        || value.contains("System.Text.UTF8Encoding")
        || value.contains("[Console]::OutputEncoding");
    if !contains_private_prologue {
        return value.to_string();
    }
    const END: &str = "$OutputEncoding=[Console]::OutputEncoding;";
    let Some(end) = find_ignoring_line_breaks(value, END) else {
        return String::new();
    };
    value[end..].trim_start_matches(['\r', '\n']).to_string()
}

fn find_ignoring_line_breaks(value: &str, needle: &str) -> Option<usize> {
    let expected = needle.as_bytes();
    let mut matched = 0usize;
    for (index, ch) in value.char_indices() {
        if matches!(ch, '\r' | '\n') {
            continue;
        }
        if ch.is_ascii() && expected.get(matched).copied() == Some(ch as u8) {
            matched += 1;
            if matched == expected.len() {
                return Some(index + ch.len_utf8());
            }
        } else {
            matched = usize::from(ch.is_ascii() && expected.first().copied() == Some(ch as u8));
        }
    }
    None
}

fn drain_public_stderr_protocol_buffer(buffer: &mut String) -> String {
    let mut visible = String::new();
    loop {
        if buffer.is_empty() {
            break;
        }

        if let Some(start) = clixml_envelope_start(buffer) {
            if start > 0 {
                visible.push_str(&buffer[..start]);
                buffer.drain(..start);
                continue;
            }
            let lower = buffer.to_ascii_lowercase();
            let Some(end_start) = lower.find("</objs>") else {
                break;
            };
            let end = end_start + "</objs>".len();
            let envelope = buffer[..end].to_string();
            visible.push_str(&public_command_stderr(&envelope));
            buffer.drain(..end);
            continue;
        }

        if looks_like_clixml_protocol(buffer) {
            let fragment = std::mem::take(buffer);
            visible.push_str(&public_command_stderr(&fragment));
            break;
        }

        let hold = clixml_marker_prefix_suffix_len(buffer);
        if hold > 0 {
            let emit = buffer.len() - hold;
            visible.push_str(&buffer[..emit]);
            buffer.drain(..emit);
            break;
        }

        visible.push_str(buffer);
        buffer.clear();
        break;
    }
    visible
}

fn clixml_envelope_start(value: &str) -> Option<usize> {
    let lower = value.to_ascii_lowercase();
    [lower.find("#< clixml"), lower.find("<objs")]
        .into_iter()
        .flatten()
        .min()
}

fn clixml_marker_prefix_suffix_len(value: &str) -> usize {
    let lower = value.to_ascii_lowercase();
    ["#< clixml", "<objs"]
        .into_iter()
        .map(|marker| {
            (1..marker.len())
                .rev()
                .find(|length| lower.ends_with(&marker[..*length]))
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0)
}

fn looks_like_clixml_protocol(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("#< clixml")
        || lower.contains("<objs")
        || lower.contains("</objs>")
        || lower.contains("<obj")
        || lower.contains("</obj>")
        || lower.contains("<ms")
        || lower.contains("</ms>")
        || lower.contains("s=\"progress\"")
        || lower.contains("s=\"error\"")
}

fn extract_clixml_error_strings(stderr: &str) -> String {
    let mut values = Vec::new();
    let lower = stderr.to_ascii_lowercase();
    let mut cursor = 0usize;
    while cursor < lower.len() {
        let Some(relative_start) = lower[cursor..].find("<s") else {
            break;
        };
        let start = cursor + relative_start;
        let Some(relative_open_end) = lower[start..].find('>') else {
            break;
        };
        let open_end = start + relative_open_end;
        let open = &lower[start..=open_end];
        let Some(relative_close) = lower[open_end + 1..].find("</s>") else {
            break;
        };
        let close = open_end + 1 + relative_close;
        if open == "<s>" || open.contains("s=\"error\"") {
            let decoded = decode_clixml_text(&stderr[open_end + 1..close]);
            if !decoded.trim().is_empty() {
                values.push(decoded);
            }
        }
        cursor = close + "</s>".len();
    }
    values.concat()
}

fn decode_clixml_text(value: &str) -> String {
    let xml = value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    decode_clixml_utf16_escapes(&xml)
}

fn decode_clixml_utf16_escapes(value: &str) -> String {
    fn escaped_unit(value: &str, index: usize) -> Option<u16> {
        let token = value.get(index..index + 7)?;
        (token.starts_with("_x") && token.ends_with('_'))
            .then(|| u16::from_str_radix(&token[2..6], 16).ok())
            .flatten()
    }

    let mut decoded = String::with_capacity(value.len());
    let mut index = 0usize;
    while index < value.len() {
        if let Some(unit) = escaped_unit(value, index) {
            if (0xD800..=0xDBFF).contains(&unit) {
                if let Some(low) = escaped_unit(value, index + 7) {
                    if (0xDC00..=0xDFFF).contains(&low) {
                        let scalar =
                            0x10000 + (((unit as u32 - 0xD800) << 10) | (low as u32 - 0xDC00));
                        if let Some(ch) = char::from_u32(scalar) {
                            decoded.push(ch);
                            index += 14;
                            continue;
                        }
                    }
                }
            } else if !(0xDC00..=0xDFFF).contains(&unit) {
                if let Some(ch) = char::from_u32(unit as u32) {
                    decoded.push(ch);
                    index += 7;
                    continue;
                }
            }
        }
        let ch = value[index..].chars().next().expect("valid UTF-8 boundary");
        decoded.push(ch);
        index += ch.len_utf8();
    }
    decoded
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
                ("path", PublicFieldKind::String),
                ("repository_root", PublicFieldKind::NullableString),
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

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_task_state(label: &str) -> CommandTaskStateStore {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        CommandTaskStateStore::open_at(std::env::temp_dir().join(format!(
            "localbridge-facade-task-state-{label}-{}-{nonce}.json",
            std::process::id()
        )))
        .unwrap()
    }

    fn bind_test_session(
        sessions: &mut PublicCommandSessions,
        task_state: &CommandTaskStateStore,
        private: &str,
    ) -> String {
        let public = sessions.start_session(task_state).unwrap();
        sessions.bind_private_session(&public, private).unwrap();
        public
    }

    struct FakeAdapter {
        catalog: Value,
    }

    impl WorkspaceRuntimeAdapter for FakeAdapter {
        fn negotiate(&mut self) -> Result<(), FacadeError> {
            validate_runtime_capabilities(&self.catalog)
        }

        fn workspace_context(&mut self, _request_id: Option<&Value>) -> Result<Value, FacadeError> {
            Ok(stable_success(json!({}), "ok"))
        }

        fn project_context(&self, path: &str) -> Result<Value, FacadeError> {
            Ok(json!({"selected_path":path}))
        }

        fn apply_directory_change(
            &mut self,
            action: &str,
            path: &str,
        ) -> Result<Value, FacadeError> {
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

        fn inspect_document(
            &mut self,
            _arguments: Value,
            _request_id: Option<&Value>,
        ) -> Result<Value, FacadeError> {
            Ok(stable_success(json!({"text":"old\n"}), "ok"))
        }

        fn apply_document_patch(
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

        fn has_running_command_session(&self) -> bool {
            false
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
    fn command_control_schema_encodes_action_specific_handles() {
        let schema = public_tool_schema("command_control")["inputSchema"].clone();
        let variants = schema["oneOf"].as_array().expect("command_control oneOf");
        assert_eq!(variants.len(), 4);
        let branch = |action: &str| {
            variants
                .iter()
                .find(|variant| variant["properties"]["action"]["const"] == action)
                .expect("action branch")
        };
        let read = branch("read");
        assert!(
            read["required"]
                .as_array()
                .unwrap()
                .contains(&json!("output_ref"))
        );
        assert!(read["properties"].get("session_id").is_none());
        let poll = branch("poll");
        assert!(
            poll["required"]
                .as_array()
                .unwrap()
                .contains(&json!("session_id"))
        );
        assert!(poll["properties"].get("output_ref").is_none());
        let write = branch("write");
        assert!(
            write["required"]
                .as_array()
                .unwrap()
                .contains(&json!("session_id"))
        );
        assert!(
            write["required"]
                .as_array()
                .unwrap()
                .contains(&json!("chars"))
        );
        let kill = branch("kill");
        assert!(
            kill["required"]
                .as_array()
                .unwrap()
                .contains(&json!("session_id"))
        );
        assert!(
            variants
                .iter()
                .all(|variant| variant["additionalProperties"] == false)
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
        let public_session =
            bind_test_session(&mut sessions, &task_state, "PRIVATE_SESSION_SECRET");
        let public_output = sessions.public_output_for_private("PRIVATE_OUTPUT_SECRET");
        assert!(public_session.starts_with("lb-session-"));
        assert!(public_output.starts_with("lb-output-"));
        assert_ne!(public_session, "PRIVATE_SESSION_SECRET");
        assert_ne!(public_output, "PRIVATE_OUTPUT_SECRET");
    }

    #[test]
    fn powershell_startup_progress_clixml_is_not_public_command_output_but_errors_are_preserved() {
        let progress =
            "#< CLIXML\r\n<Objs><Obj S=\"progress\"><S>Preparing modules</S></Obj></Objs>";
        assert_eq!(public_command_stderr(progress), "");
        let error =
            "#< CLIXML\r\n<Objs><Obj S=\"progress\"/><Obj S=\"Error\"><S>boom</S></Obj></Objs>";
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
        let progress =
            "#< CLIXML\r\n<Objs><Obj S=\"progress\"><S>Preparing modules</S></Obj></Objs>";
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
            let normalized = CodingToolsRuntimeAdapter::normalize_read_output(
                &raw,
                "lb-output-retained-regression",
            );
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

    #[test]
    fn every_advertised_agent_and_document_action_has_an_executable_facade_contract() {
        let mut facade = AgentFacade::with_adapter(
            FakeAdapter {
                catalog: compatible_catalog(),
            },
            policy(),
        )
        .unwrap();
        for action in [
            "diagnose",
            "bugfix",
            "feature",
            "refactor",
            "test_failure",
            "build_release",
            "document",
            "resume",
            "custom",
        ] {
            let result = facade
                .dispatch("agent_workflow", json!({"action":action}), None)
                .unwrap();
            assert_eq!(result["isError"], false, "action={action}: {result:#?}");
            assert_eq!(
                result["structuredContent"]["data"]["state"], "context_ready",
                "action={action}"
            );
        }
        let diagnose_command = facade
            .dispatch(
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
                "create",
                json!({"action":"create","path":"new.txt","content":"new"}),
            ),
            (
                "convert",
                json!({"action":"convert","source":"doc.txt","path":"copy.txt"}),
            ),
            (
                "rebuild",
                json!({"action":"rebuild","path":"doc.txt","content":"rebuilt"}),
            ),
        ] {
            let result = facade
                .dispatch("document_workflow", arguments, None)
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
        let terminal = sessions
            .terminal(&public)
            .expect("completed terminal snapshot");
        assert_eq!(terminal["isError"], false);
        assert_eq!(terminal["structuredContent"]["data"]["status"], "completed");
        assert_eq!(terminal["structuredContent"]["data"]["output"], "");

        let lost = bind_test_session(&mut sessions, &task_state, "PRIVATE_LOST");
        sessions.mark_all_running_lost(&task_state).unwrap();
        let terminal = sessions.terminal(&lost).expect("lost terminal snapshot");
        assert_eq!(terminal["isError"], true);
        assert_eq!(
            terminal["structuredContent"]["error"]["code"],
            "SessionUnavailable"
        );
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
        sessions
            .mark_terminal(
                &public,
                stable_command_error(
                    FacadeErrorCode::ProcessCancelled,
                    "命令已取消",
                    Map::from_iter([
                        ("status".into(), Value::String("cancelled".into())),
                        ("output".into(), Value::String("private-final".into())),
                    ]),
                ),
                &task_state,
            )
            .unwrap();
        let terminal = sessions.terminal_with_pending(&public).unwrap();
        assert_eq!(terminal["structuredContent"]["data"]["output"], "tail\n");
        let replay = sessions.terminal_with_pending(&public).unwrap();
        assert_eq!(replay["structuredContent"]["data"]["output"], "");
        assert_eq!(
            replay["structuredContent"]["error"]["code"],
            "ProcessCancelled"
        );
    }

    #[test]
    fn stable_runtime_and_session_errors_persist_as_lost_terminal_snapshots() {
        let owner = CommandOwner::new("task-runtime-loss", "lb-session-runtime-loss");
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
            let snapshot = terminal_snapshot_from_result(owner.clone(), &error.to_mcp_result());
            assert_eq!(snapshot.status, CommandTerminalStatus::Lost);
            assert_eq!(snapshot.owner, owner);
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

    #[test]
    fn public_paths_and_document_patches_are_target_bound() {
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

        let create = document_add_patch("safe/doc.txt", "hello\nworld\n");
        assert!(create.contains("*** Add File: safe/doc.txt"));
        assert!(!create.contains("other.txt"));
        let rebuild = document_rebuild_patch("safe/doc.txt", "old\n", "new\n");
        assert!(rebuild.contains("*** Update File: safe/doc.txt"));
        assert!(rebuild.contains("-old"));
        assert!(rebuild.contains("+new"));

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
}
