use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Instant;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::control_plane::command_control::COMMAND_CONTROL_UPSTREAM_HEADROOM_MS;
use crate::control_plane::execution_registry::{ExecutionRegistry, ExecutionRegistryError};
use crate::diagnostics::error::{
    DiagnosticErrorCode, DiagnosticPhase, ErrorDiagnostic, from_canonical_code,
    transport_unavailable,
};
use crate::document::{
    DocumentEditOperation, DocumentError, DocumentFormat, DocumentRequest, DocumentResult,
    DocumentService,
};
use crate::domain::{
    AdoptionToken, AdoptionTokenHash, ExecutionId, ExecutionState, ExecutionTerminal, McpSessionId,
    PublicSessionId, RuntimeCommandHandle, TaskId, TerminalOutcome,
};
#[cfg(test)]
use crate::execution::output_handles::MAX_LOCAL_RETAINED_OUTPUT_HANDLES;
use crate::execution::output_handles::{OutputHandleRegistry, OutputOwner};
use crate::state::{
    Capability, CurrentTaskStatus, PermissionMode, RuntimeFault, SafeTaskSummary,
    TaskExecutionState, TaskKind,
};

use super::observation::WorkspaceObservationSeed;
#[cfg(test)]
use super::public_contract::EXEC_COMMAND_FIELDS;
use super::public_contract::ExecCommandArguments;
use super::runtime::{CodingToolsRuntime, CodingToolsRuntimeError};
use crate::control_plane::workflow_checkpoint::{
    WorkflowCheckpoint as StoredWorkflowCheckpoint, WorkflowCheckpointStore, WorkflowFailure,
};
use crate::execution::policy::{CapabilityPolicy, DenyReason, PolicyDecision, command_task_kind};
use crate::execution::shell::{
    ResolvedShellKind, ShellExecutionSpec, ShellExecutor, ShellResolveError, ShellSelector,
};
use crate::execution::toolbox::ToolboxResolver;
use crate::execution::verification::VerificationPlanner;
use crate::filesystem::edit::{CodingEditError, CodingEditService};
use crate::filesystem::service::{
    FilesystemCancellation, FilesystemContentSearchOptions, FilesystemError,
    FilesystemSearchOptions, FilesystemService,
};
use crate::workspace::context::ContextService;
use crate::workspace::git_adapter::handle_git_tool_with_authority;
use crate::workspace::path_authority::{
    PathAuthorityError, WorkspaceLifetimePin, WorkspaceResolver, workspace_input_path_valid,
    workspace_relative_path_valid,
};

type WorkflowCheckpoint = StoredWorkflowCheckpoint<Value>;

#[cfg(test)]
use super::public_contract::public_tool_schema;
pub use super::public_contract::{
    AGENT_API_REVISION, AGENT_API_VERSION, ToolRegistry, V1_CORE_TOOL_NAMES,
};
pub(crate) use super::public_contract::{public_error_output_schema, stable_public_tool_catalog};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacadeErrorCode {
    InvalidArgument,
    NotFound,
    WorkspaceDenied,
    CapabilityDenied,
    PolicyDenied,
    InvalidShellSyntax,
    ElevatedOperationNotReviewed,
    PrivilegedRouteNotAvailable,
    ElevationRequired,
    ProcessFailed,
    ProcessTimedOut,
    ProcessCancelled,
    OperationTimedOut,
    QueueCapacityExceeded,
    TaskIdRequired,
    TaskNotOwned,
    SessionUnavailable,
    OutputNotFound,
    OutputTruncated,
    RuntimeUnavailable,
    CapabilityUnavailable,
    RuntimeProtocolMismatch,
    RuntimeCapabilityMismatch,
    FileChanged,
    PatchConflict,
    AmbiguousMatch,
    Internal,
}

impl FacadeErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArgument => "InvalidArgument",
            Self::NotFound => "NotFound",
            Self::WorkspaceDenied => "WorkspaceDenied",
            Self::CapabilityDenied => "CapabilityDenied",
            Self::PolicyDenied => "PolicyDenied",
            Self::InvalidShellSyntax => "InvalidShellSyntax",
            Self::ElevatedOperationNotReviewed => "ElevatedOperationNotReviewed",
            Self::PrivilegedRouteNotAvailable => "PrivilegedRouteUnavailable",
            Self::ElevationRequired => "ElevationRequired",
            Self::ProcessFailed => "ProcessFailed",
            Self::ProcessTimedOut => "ProcessTimedOut",
            Self::ProcessCancelled => "ProcessCancelled",
            Self::OperationTimedOut => "OperationTimedOut",
            Self::QueueCapacityExceeded => "QueueCapacityExceeded",
            Self::TaskIdRequired => "TaskIdRequired",
            Self::TaskNotOwned => "TaskNotOwned",
            Self::SessionUnavailable => "SessionUnavailable",
            Self::OutputNotFound => "OutputNotFound",
            Self::OutputTruncated => "OutputTruncated",
            Self::RuntimeUnavailable => "RuntimeUnavailable",
            Self::CapabilityUnavailable => "CapabilityUnavailable",
            Self::RuntimeProtocolMismatch => "RuntimeProtocolMismatch",
            Self::RuntimeCapabilityMismatch => "RuntimeCapabilityMismatch",
            Self::FileChanged => "FileChanged",
            Self::PatchConflict => "PatchConflict",
            Self::AmbiguousMatch => "AmbiguousMatch",
            Self::Internal => "Internal",
        }
    }
}

impl FacadeErrorCode {
    const fn safe_rule_category(self) -> &'static str {
        match self {
            Self::WorkspaceDenied => "workspace_boundary",
            Self::PolicyDenied | Self::CapabilityDenied | Self::TaskNotOwned => "policy",
            Self::InvalidShellSyntax => "shell_syntax",
            Self::ElevatedOperationNotReviewed
            | Self::PrivilegedRouteNotAvailable
            | Self::ElevationRequired => "privileged_route",
            Self::RuntimeUnavailable
            | Self::CapabilityUnavailable
            | Self::RuntimeProtocolMismatch
            | Self::RuntimeCapabilityMismatch => "runtime",
            Self::ProcessTimedOut => "process_timeout",
            Self::OperationTimedOut => "command_runtime",
            Self::ProcessFailed
            | Self::ProcessCancelled
            | Self::QueueCapacityExceeded
            | Self::SessionUnavailable
            | Self::OutputTruncated => "command_runtime",
            Self::OutputNotFound => "request",
            Self::FileChanged | Self::PatchConflict | Self::AmbiguousMatch => "edit_conflict",
            Self::InvalidArgument | Self::NotFound | Self::TaskIdRequired | Self::Internal => {
                "request"
            }
        }
    }

    const fn safe_remediation(self) -> &'static str {
        match self {
            Self::WorkspaceDenied => "使用当前 active workspace 内的相对路径",
            Self::PolicyDenied | Self::CapabilityDenied => {
                "查看 workspace_context.capabilities 或使用 dry_run 获取允许路线"
            }
            Self::InvalidShellSyntax => "按所选 Windows Shell 的原生语法修正命令",
            Self::ElevatedOperationNotReviewed => {
                "使用允许列表中的程序、参数与工作目录提交管理员操作"
            }
            Self::PrivilegedRouteNotAvailable | Self::ElevationRequired => {
                "检查 workspace_context 中的权限模式与管理员路由状态"
            }
            Self::RuntimeUnavailable
            | Self::CapabilityUnavailable
            | Self::RuntimeProtocolMismatch
            | Self::RuntimeCapabilityMismatch => {
                "查看 workspace_context.shell_discovery 与运行时诊断"
            }
            Self::ProcessTimedOut => "提高 timeout_ms 或缩小单次任务",
            Self::OperationTimedOut => {
                "命令控制请求已达到 wait_ms 预算；稍后 poll 以观察同一 Execution"
            }
            Self::ProcessCancelled => "重新发起命令",
            Self::QueueCapacityExceeded => "等待其他任务完成后重试",
            Self::TaskIdRequired => "指定当前 MCP Session 拥有的 task_id",
            Self::TaskNotOwned => "使用当前 MCP Session 拥有且尚未终止的 task_id",
            Self::SessionUnavailable => "重新执行命令以创建新会话",
            Self::OutputNotFound => "使用命令返回的有效 output_ref，或重新执行命令生成新输出",
            Self::OutputTruncated => {
                "若返回 output_ref 则分页读取；否则提高 max_bytes 或 resize 后重试"
            }
            Self::FileChanged
            | Self::PatchConflict
            | Self::AmbiguousMatch
            | Self::ProcessFailed
            | Self::InvalidArgument
            | Self::NotFound
            | Self::Internal => "检查参数与返回的稳定错误信息",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacadeError {
    pub code: FacadeErrorCode,
    pub message: &'static str,
    pub retryable: bool,
    diagnostic: Option<ErrorDiagnostic>,
    details: Value,
}

impl FacadeError {
    pub const fn new(code: FacadeErrorCode, message: &'static str, retryable: bool) -> Self {
        Self {
            code,
            message,
            retryable,
            diagnostic: None,
            details: Value::Null,
        }
    }

    pub fn with_diagnostic(mut self, diagnostic: ErrorDiagnostic) -> Self {
        self.diagnostic = Some(diagnostic);
        self
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn to_mcp_result(&self) -> Value {
        let diagnostic = self
            .diagnostic
            .clone()
            .unwrap_or_else(|| from_canonical_code(self.code.as_str()));
        json!({
            "content": [{"type":"text","text":self.message}],
            "structuredContent": {
                "ok": false,
                "state":"failed",
                "summary":self.message,
                "task_id":Value::Null,
                "warnings":[],
                "next_step":Value::Null,
                "output_refs":[],
                "data":Value::Null,
                "error": {
                    "code": self.code.as_str(),
                    "error_code": diagnostic.error_code.as_str(),
                    "phase": diagnostic.phase.as_str(),
                    "cause": diagnostic.cause,
                    "http_status": diagnostic.http_status,
                    "message": self.message,
                    "retryable": self.retryable,
                    "rule_category": self.code.safe_rule_category(),
                    "remediation": self.code.safe_remediation(),
                    "details": self.details
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

impl FacadeDenied {
    pub fn to_mcp_result(self) -> Value {
        let (code, message, retryable) = match self.reason {
            DenyReason::VerbatimExecutionPath => (
                FacadeErrorCode::WorkspaceDenied,
                "工作区路径参数无效",
                false,
            ),
            DenyReason::ElevatedExecNotReviewed => (
                FacadeErrorCode::ElevatedOperationNotReviewed,
                "管理员操作未通过允许列表审核",
                false,
            ),
            DenyReason::PrivilegedRouteNotAvailable => (
                FacadeErrorCode::PrivilegedRouteNotAvailable,
                "该操作需要受控管理员路由",
                false,
            ),
            DenyReason::UnknownTool | DenyReason::NetworkRouteNotAvailable => (
                FacadeErrorCode::CapabilityDenied,
                "请求的能力当前不可用",
                false,
            ),
            DenyReason::ControlPlane
            | DenyReason::ToolNotAllowedInMode
            | DenyReason::IndirectProcessExecInEdit
            | DenyReason::IndirectControlPlane
            | DenyReason::IndirectUnknownCapability => (
                FacadeErrorCode::PolicyDenied,
                "请求被 LocalBridge 权限策略拒绝",
                false,
            ),
        };
        FacadeError::new(code, message, retryable).to_mcp_result()
    }
}

#[derive(Debug)]
pub enum FacadeCallError {
    Denied(FacadeDenied),
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
    pub owner_task_id: Option<String>,
    pub owner_session: Option<McpSessionId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingRuntimeHealthState {
    Ready,
    Recovering,
    Fault,
}

impl CodingRuntimeHealthState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Recovering => "recovering",
            Self::Fault => "fault",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingRuntimeHealth {
    pub state: CodingRuntimeHealthState,
    pub root_process_alive: bool,
    pub authenticated_mcp: bool,
    pub fault: Option<RuntimeFault>,
}

pub trait WorkspaceRuntimeAdapter {
    fn negotiate(&mut self) -> Result<(), FacadeError>;
    fn workspace_context(&mut self, request_id: Option<&Value>) -> Result<Value, FacadeError>;
    fn validate_workspace_identity(&self) -> Result<(), FacadeError> {
        Err(FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "工作区身份验证不可用",
            false,
        ))
    }
    fn runtime_discovery(&self) -> Value {
        json!({
            "shells": {
                "cmd":{"available":false},
                "powershell_core":{"available":false},
                "windows_powershell":{"available":false},
                "auto_resolved":null
            },
            "git":{"available":false},
            "bundled_python":{"available":false},
            "bundled_node":{"available":false,"reason":"not_bundled"}
        })
    }
    fn live_runtime_health(&mut self) -> Result<CodingRuntimeHealth, FacadeError> {
        Err(FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "运行时健康状态不可用",
            true,
        ))
    }
    fn take_runtime_fault(&mut self) -> Option<RuntimeFault> {
        None
    }
    fn coding_context(&self, _project_path: &str, _objective: &str) -> Result<Value, FacadeError> {
        Err(FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "编码上下文不可用",
            false,
        ))
    }
    fn coding_verification_plan(&self, _project_path: &str) -> Result<Vec<Value>, FacadeError> {
        Err(FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "验证计划不可用",
            false,
        ))
    }
    fn verify_coding_edit_preconditions(
        &self,
        _expected: &Map<String, Value>,
    ) -> Result<(), FacadeError> {
        Err(FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "编辑前置条件验证不可用",
            false,
        ))
    }
    fn apply_coding_patch(
        &self,
        _patch: &str,
        _expected: &Map<String, Value>,
    ) -> Result<Vec<String>, FacadeError> {
        Err(FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "补丁应用能力不可用",
            false,
        ))
    }
    fn normalize_workspace_path(
        &self,
        path: &str,
        allow_missing_leaf: bool,
    ) -> Result<String, FacadeError>;
    fn project_context(&self, path: &str) -> Result<Value, FacadeError>;
    fn filesystem(&mut self, _arguments: Value) -> Result<Value, FacadeError> {
        Err(FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "文件系统服务不可用",
            false,
        ))
    }
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
    fn authorize_command_resource(
        &self,
        _action: CommandControlAction,
        _arguments: &Map<String, Value>,
        _owner: &McpSessionId,
    ) -> Result<(), FacadeError> {
        Err(FacadeError::new(
            FacadeErrorCode::TaskNotOwned,
            "command resource is not owned by the current MCP session",
            false,
        ))
    }
    fn transfer_workflow_executions(
        &self,
        _task_id: &TaskId,
        _owner: &McpSessionId,
    ) -> Result<(), FacadeError> {
        Ok(())
    }
    fn git_workflow(
        &mut self,
        action: GitWorkflowAction,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError>;
    fn execute_document(&self, request: DocumentRequest) -> Result<DocumentResult, FacadeError> {
        let _ = request;
        Err(FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "文档服务不可用",
            false,
        ))
    }
    fn apply_workflow_patch(
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
    fn has_running_execution(&self) -> bool;
    fn load_workflow_checkpoint(&self) -> Result<Option<Value>, FacadeError> {
        Err(FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "工作流检查点不可用",
            false,
        ))
    }
    fn save_workflow_checkpoint(&self, _checkpoint: &Value) -> Result<(), FacadeError> {
        Err(FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "工作流检查点不可用",
            false,
        ))
    }
    fn clear_workflow_checkpoint(&self) -> Result<(), FacadeError> {
        Err(FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "工作流检查点不可用",
            false,
        ))
    }
    fn durable_command_terminal(&self, _session_id: &str) -> Option<Value> {
        None
    }
}

const MAX_PENDING_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_STDERR_PROTOCOL_BYTES: usize = 256 * 1024;

fn next_public_handle(prefix: &str) -> String {
    crate::security::random_prefixed_id(&format!("{prefix}-"))
}

#[derive(Debug, Clone)]
struct PublicCommandSession {
    execution_id: ExecutionId,
    started_at: Instant,
    pending_output: String,
    pending_output_truncated: bool,
    stderr_protocol_buffer: String,
}

#[derive(Debug, Default)]
struct PublicCommandSessions {
    sessions: HashMap<String, PublicCommandSession>,
    outputs: OutputHandleRegistry,
}

#[derive(Debug)]
struct StartedPublicCommand {
    public_session_id: String,
    adoption_token: AdoptionToken,
}

impl PublicCommandSessions {
    fn start_session(
        &mut self,
        executions: &ExecutionRegistry,
        owner_task_id: Option<String>,
        owner_session: Option<McpSessionId>,
    ) -> Result<StartedPublicCommand, FacadeError> {
        let public = next_public_handle("lb-session");
        let task_id = TaskId::new(owner_task_id.unwrap_or_else(|| next_public_handle("lb-task")));
        let started = executions
            .start_owned(task_id, PublicSessionId::new(public.clone()), owner_session)
            .map_err(normalize_execution_registry_error)?;
        self.sessions.insert(
            public.clone(),
            PublicCommandSession {
                execution_id: started.execution_id,
                started_at: Instant::now(),
                pending_output: String::new(),
                pending_output_truncated: false,
                stderr_protocol_buffer: String::new(),
            },
        );
        Ok(StartedPublicCommand {
            public_session_id: public,
            adoption_token: started.adoption_token,
        })
    }

    fn bind_private_session(
        &self,
        executions: &ExecutionRegistry,
        public_session_id: &str,
        private_session_id: &str,
    ) -> Result<(), FacadeError> {
        let execution_id = self
            .sessions
            .get(public_session_id)
            .map(|session| session.execution_id.clone())
            .ok_or_else(session_unavailable)?;
        executions
            .bind_runtime_handle(
                &execution_id,
                RuntimeCommandHandle::new(private_session_id.to_string()),
            )
            .map_err(normalize_execution_registry_error)
    }

    fn public_output_for_private(
        &mut self,
        private_output_ref: &str,
        owner_public_session_id: &str,
        stream: &str,
    ) -> String {
        self.outputs
            .public_for_private(private_output_ref, owner_public_session_id, stream)
    }

    fn retain_local_output(
        &mut self,
        owner_session: McpSessionId,
        stream: &str,
        content: String,
    ) -> String {
        self.outputs.retain_local(owner_session, stream, content)
    }

    fn reap_expired_mappings(&mut self, executions: &ExecutionRegistry) {
        let expired_sessions = self
            .sessions
            .keys()
            .filter(|public| {
                executions
                    .execution_for_public_session(&PublicSessionId::new((*public).clone()))
                    .is_none()
            })
            .cloned()
            .collect::<Vec<_>>();
        for public in &expired_sessions {
            self.sessions.remove(public);
        }
        self.outputs.reap_owned_by(&expired_sessions);
    }

    fn stable_metadata(
        &self,
        executions: &ExecutionRegistry,
        public_session_id: &str,
    ) -> Option<(String, String, u64)> {
        let session = self.sessions.get(public_session_id)?;
        let execution =
            executions.execution_for_public_session(&PublicSessionId::new(public_session_id))?;
        Some((
            execution.task_id.to_string(),
            execution.id.to_string(),
            session
                .started_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        ))
    }

    fn private_session(
        &self,
        executions: &ExecutionRegistry,
        public_session_id: &str,
    ) -> Option<String> {
        self.sessions.get(public_session_id)?;
        executions
            .execution_for_public_session(&PublicSessionId::new(public_session_id))
            .and_then(|execution| execution.runtime_handle)
            .map(|handle| handle.as_str().to_string())
    }

    fn private_output(&self, public_output_ref: &str) -> Option<String> {
        self.outputs.private(public_output_ref)
    }

    fn local_output(&self, public_output_ref: &str) -> Option<(String, String)> {
        self.outputs.local(public_output_ref)
    }

    fn output_stream(&self, public_output_ref: &str) -> Option<String> {
        self.outputs.stream(public_output_ref)
    }

    fn output_owned_by(
        &self,
        public_output_ref: &str,
        owner: &McpSessionId,
        executions: &ExecutionRegistry,
    ) -> bool {
        match self.outputs.owner(public_output_ref) {
            Some(OutputOwner::McpSession(output_owner)) => &output_owner == owner,
            Some(OutputOwner::PublicSession(public_session)) => executions
                .execution_for_public_session(&PublicSessionId::new(public_session))
                .is_some_and(|execution| execution.owner_session.as_ref() == Some(owner)),
            None => false,
        }
    }

    fn output_refs_by_stream(&self, output_refs: &[String]) -> Map<String, Value> {
        output_refs
            .iter()
            .filter_map(|output_ref| {
                self.output_stream(output_ref)
                    .map(|stream| (stream, Value::String(output_ref.clone())))
            })
            .collect()
    }

    fn mark_terminal(
        &mut self,
        public_session_id: &str,
        result: Value,
        executions: &ExecutionRegistry,
    ) -> Result<(), FacadeError> {
        let execution_id = self
            .sessions
            .get(public_session_id)
            .ok_or_else(session_unavailable)?
            .execution_id
            .clone();
        let public_session = PublicSessionId::new(public_session_id);
        let mut terminal = execution_terminal_from_result(&result);
        if let Some(signal) = executions.cancellation_signal(&public_session) {
            terminal.outcome = TerminalOutcome::Cancelled;
            terminal.signal = terminal.signal.or(Some(signal));
            terminal.error_code = Some("ProcessCancelled".to_string());
        }
        match executions.finish(&execution_id, terminal) {
            Ok(()) => {}
            Err(ExecutionRegistryError::AlreadyTerminal { .. }) => return Ok(()),
            Err(error) => return Err(normalize_execution_registry_error(error)),
        }
        Ok(())
    }

    fn mark_error_terminal(
        &mut self,
        public_session_id: &str,
        error: &FacadeError,
        executions: &ExecutionRegistry,
    ) -> Result<(), FacadeError> {
        self.mark_terminal(public_session_id, error.to_mcp_result(), executions)
    }

    fn append_pending(&mut self, public_session_id: &str, output: &str) {
        if output.is_empty() {
            return;
        }
        if let Some(session) = self.sessions.get_mut(public_session_id) {
            session.pending_output.push_str(output);
            session.pending_output_truncated |=
                trim_utf8_front(&mut session.pending_output, MAX_PENDING_OUTPUT_BYTES);
        }
    }

    fn take_pending(&mut self, public_session_id: &str) -> String {
        let Some(session) = self.sessions.get_mut(public_session_id) else {
            return String::new();
        };
        let mut output = std::mem::take(&mut session.pending_output);
        if std::mem::take(&mut session.pending_output_truncated) {
            output.insert_str(0, "[earlier command output truncated]\n");
        }
        output
    }

    fn terminal_with_pending(&mut self, public_session_id: &str, terminal: Value) -> Value {
        let pending = self.take_pending(public_session_id);
        command_result_with_output(terminal, pending)
    }

    fn running_with_pending(&mut self, public_session_id: &str) -> Option<Value> {
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
        if trim_utf8_front(
            &mut session.stderr_protocol_buffer,
            MAX_STDERR_PROTOCOL_BYTES,
        ) {
            let retained = std::mem::take(&mut session.stderr_protocol_buffer);
            return format!(
                "[stderr protocol fragment truncated]\n{}",
                public_command_stderr(&retained)
            );
        }
        drain_public_stderr_protocol_buffer(&mut session.stderr_protocol_buffer)
    }

    fn mark_all_running_lost(&mut self, executions: &ExecutionRegistry) -> Result<(), FacadeError> {
        let running = self
            .sessions
            .iter()
            .filter(|(public, _)| {
                executions
                    .execution_for_public_session(&PublicSessionId::new((*public).clone()))
                    .is_some_and(|execution| !execution.state.is_terminal())
            })
            .map(|(public, _)| public.clone())
            .collect::<Vec<_>>();
        for public in running {
            self.mark_terminal(&public, session_unavailable().to_mcp_result(), executions)?;
        }
        Ok(())
    }

    fn running_sessions(&self, executions: &ExecutionRegistry) -> Vec<(String, String)> {
        self.sessions
            .iter()
            .filter(|(public, _)| {
                executions
                    .execution_for_public_session(&PublicSessionId::new((*public).clone()))
                    .is_some_and(|execution| !execution.state.is_terminal())
            })
            .filter_map(|(public, _)| {
                executions
                    .execution_for_public_session(&PublicSessionId::new(public))
                    .and_then(|execution| execution.runtime_handle)
                    .map(|handle| (public.clone(), handle.as_str().to_string()))
            })
            .collect()
    }

    fn has_running_session(&self, executions: &ExecutionRegistry) -> bool {
        !executions.running().is_empty()
    }
}

pub struct CodingToolsRuntimeAdapter {
    runtime: CodingToolsRuntime,
    workspace: PathBuf,
    workspace_authority: WorkspaceResolver,
    workspace_lifetime_pin: WorkspaceLifetimePin,
    shell_executor: ShellExecutor,
    toolbox: ToolboxResolver,
    public_commands: PublicCommandSessions,
    executions: ExecutionRegistry,
    workflow_checkpoint: WorkflowCheckpointStore,
    cached_default_cwd: Option<String>,
    cached_project_discovery: Option<Value>,
    pending_runtime_fault: Option<RuntimeFault>,
}

impl CodingToolsRuntimeAdapter {
    fn new_with_executions(
        runtime: CodingToolsRuntime,
        executions: ExecutionRegistry,
    ) -> Result<Self, FacadeError> {
        let workspace = runtime.workspace().to_path_buf();
        let workspace_authority = runtime.workspace_authority();
        workspace_authority
            .input_path(".")
            .map_err(normalize_path_authority_error)?;
        let workspace_lifetime_pin = WorkspaceResolver::pin_active_workspace_lifetime(&workspace)
            .map_err(normalize_path_authority_error)?;
        workspace_authority
            .input_path(".")
            .map_err(normalize_path_authority_error)?;
        let toolbox = ToolboxResolver::probe(runtime.install_root());
        let workflow_checkpoint = WorkflowCheckpointStore::for_workspace(&workspace)
            .map_err(workflow_checkpoint_error)?;
        Ok(Self {
            runtime,
            workspace,
            workspace_authority,
            workspace_lifetime_pin,
            shell_executor: ShellExecutor::default(),
            toolbox,
            public_commands: PublicCommandSessions::default(),
            executions,
            workflow_checkpoint,
            cached_default_cwd: None,
            cached_project_discovery: None,
            pending_runtime_fault: None,
        })
    }

    pub fn into_runtime(self) -> CodingToolsRuntime {
        self.runtime
    }

    fn stable_workspace_relative_path(&self, absolute: &Path) -> Result<String, FacadeError> {
        self.workspace_authority
            .display_path(absolute)
            .map_err(normalize_path_authority_error)
    }

    fn private_call(
        &mut self,
        name: &str,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let raw = match self
            .runtime
            .call_tool_with_request_id(name, arguments, request_id)
        {
            Ok(raw) => raw,
            Err(error) => {
                if !matches!(error, CodingToolsRuntimeError::RequestTimeout) {
                    self.pending_runtime_fault = Some(error.runtime_fault());
                }
                return Err(normalize_runtime_error(error));
            }
        };
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
        let raw = match self.runtime.call_tool_with_request_id_and_timeout(
            name,
            arguments,
            request_id,
            transport_timeout,
        ) {
            Ok(raw) => raw,
            Err(error) => {
                if !matches!(error, CodingToolsRuntimeError::RequestTimeout) {
                    self.pending_runtime_fault = Some(error.runtime_fault());
                }
                return Err(normalize_runtime_error(error));
            }
        };
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
        self.workspace_authority
            .resolve_workspace_path(Some(relative), ".", false)
            .map_err(normalize_path_authority_error)
    }

    fn normalized_workspace_path(
        &self,
        raw: &str,
        allow_missing_leaf: bool,
    ) -> Result<String, FacadeError> {
        let resolved = self
            .workspace_authority
            .resolve_workspace_path(Some(raw), ".", allow_missing_leaf)
            .map_err(normalize_path_authority_error)?;
        self.workspace_authority
            .display_path(&resolved)
            .map_err(normalize_path_authority_error)
    }

    #[cfg(test)]
    #[allow(dead_code)]
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
            .map_err(|error| normalize_shell_error(error, ShellSelector::Auto))?;
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
        self.validate_workspace_identity()?;
        let catalog = self.runtime.list_tools().map_err(normalize_runtime_error)?;
        validate_runtime_capabilities(&catalog)?;
        let probe = self.private_call("get_default_cwd", json!({}), None)?;
        validate_workspace_context_probe(&probe, &self.workspace)?;
        let structured = probe
            .get("structuredContent")
            .and_then(Value::as_object)
            .ok_or_else(runtime_capability_mismatch)?;
        let default_cwd = structured
            .get("default_cwd")
            .and_then(Value::as_str)
            .ok_or_else(runtime_capability_mismatch)?
            .to_string();
        let runtime_discovery = self.runtime_discovery();
        let git_status = self
            .git_workflow(GitWorkflowAction::Status, json!({"path":default_cwd}), None)
            .ok();
        let mut project_discovery = compact_project_discovery(
            &self.workspace,
            &self.workspace_authority,
            &default_cwd,
            git_status.as_ref(),
            &runtime_discovery,
        );
        if let Some(object) = project_discovery.as_object_mut() {
            if let Ok(context) =
                ContextService::with_authority(self.workspace_authority.clone(), &default_cwd)
            {
                let metadata = context.discovery_metadata();
                object.insert(
                    "important_files".into(),
                    metadata
                        .get("important_files")
                        .cloned()
                        .unwrap_or_else(|| json!([])),
                );
                object.insert(
                    "instructions".into(),
                    metadata
                        .get("instructions")
                        .cloned()
                        .unwrap_or_else(|| json!([])),
                );
            }
            let git_root = git_status
                .as_ref()
                .map(stable_data)
                .and_then(|data| data.get("repository_root").cloned())
                .unwrap_or(Value::Null);
            object.insert("git_root".into(), git_root);
        }
        self.cached_project_discovery = Some(project_discovery);
        self.cached_default_cwd = Some(default_cwd);
        Ok(())
    }

    fn validate_workspace_identity(&self) -> Result<(), FacadeError> {
        self.workspace_authority
            .input_path(".")
            .map_err(normalize_path_authority_error)?;
        self.workspace_lifetime_pin
            .validate_current()
            .map_err(normalize_path_authority_error)
    }

    fn workspace_context(&mut self, _request_id: Option<&Value>) -> Result<Value, FacadeError> {
        let default_cwd = self
            .cached_default_cwd
            .clone()
            .ok_or_else(runtime_capability_mismatch)?;
        let health = self.live_runtime_health()?;
        let mut data = json!({
            "api_version": AGENT_API_VERSION,
            "facade_revision": AGENT_API_REVISION,
            "workspace": self.workspace.to_string_lossy(),
            "default_cwd": default_cwd,
            "runtime": health.state.as_str(),
            "runtime_health": {
                "root_process_alive": health.root_process_alive,
                "authenticated_mcp": health.authenticated_mcp,
                "fault": health.fault.as_ref().map(runtime_fault_name)
            },
            "coding_profile": "coding-agent-v1"
        });
        if let (Some(target), Some(discovery)) = (
            data.as_object_mut(),
            self.cached_project_discovery
                .as_ref()
                .and_then(Value::as_object),
        ) {
            for (key, value) in discovery {
                target.insert(key.clone(), value.clone());
            }
        }
        Ok(stable_success(data, "LocalBridge workspace context ready"))
    }

    fn live_runtime_health(&mut self) -> Result<CodingRuntimeHealth, FacadeError> {
        let root_process_alive = match self.runtime.root_is_running() {
            Ok(value) => value,
            Err(error) => {
                let fault = error.runtime_fault();
                self.pending_runtime_fault = Some(fault.clone());
                return Ok(CodingRuntimeHealth {
                    state: CodingRuntimeHealthState::Fault,
                    root_process_alive: false,
                    authenticated_mcp: false,
                    fault: Some(fault),
                });
            }
        };
        if !root_process_alive {
            let fault = RuntimeFault::McpExited;
            self.pending_runtime_fault = Some(fault.clone());
            return Ok(CodingRuntimeHealth {
                state: CodingRuntimeHealthState::Fault,
                root_process_alive: false,
                authenticated_mcp: false,
                fault: Some(fault),
            });
        }
        match self.runtime.call_tool_with_request_id_and_timeout(
            "get_default_cwd",
            json!({}),
            None,
            std::time::Duration::from_millis(750),
        ) {
            Ok(raw) if validate_workspace_context_probe(&raw, &self.workspace).is_ok() => {
                Ok(CodingRuntimeHealth {
                    state: CodingRuntimeHealthState::Ready,
                    root_process_alive: true,
                    authenticated_mcp: true,
                    fault: None,
                })
            }
            Ok(_) => {
                let fault = RuntimeFault::ConfigurationInvalid;
                self.pending_runtime_fault = Some(fault.clone());
                Ok(CodingRuntimeHealth {
                    state: CodingRuntimeHealthState::Fault,
                    root_process_alive: true,
                    authenticated_mcp: false,
                    fault: Some(fault),
                })
            }
            Err(error) => {
                let state = match error {
                    CodingToolsRuntimeError::ConnectionUnavailable
                    | CodingToolsRuntimeError::HttpStatus(_)
                    | CodingToolsRuntimeError::HealthTimeout => {
                        CodingRuntimeHealthState::Recovering
                    }
                    _ => CodingRuntimeHealthState::Fault,
                };
                let fault = error.runtime_fault();
                self.pending_runtime_fault = Some(fault.clone());
                Ok(CodingRuntimeHealth {
                    state,
                    root_process_alive: true,
                    authenticated_mcp: false,
                    fault: Some(fault),
                })
            }
        }
    }

    fn take_runtime_fault(&mut self) -> Option<RuntimeFault> {
        self.pending_runtime_fault.take()
    }

    fn coding_context(&self, project_path: &str, objective: &str) -> Result<Value, FacadeError> {
        ContextService::with_authority(self.workspace_authority.clone(), project_path)
            .map_err(normalize_path_authority_error)
            .map(|service| service.prepare(objective))
    }

    fn coding_verification_plan(&self, project_path: &str) -> Result<Vec<Value>, FacadeError> {
        let planner =
            VerificationPlanner::with_authority(self.workspace_authority.clone(), project_path)
                .map_err(normalize_path_authority_error)?;
        planner
            .plan()
            .into_iter()
            .map(|step| serde_json::to_value(step).map_err(|_| command_state_internal_error()))
            .collect()
    }

    fn verify_coding_edit_preconditions(
        &self,
        expected: &Map<String, Value>,
    ) -> Result<(), FacadeError> {
        let expected = typed_expected_files(expected)?;
        CodingEditService::with_authority(self.workspace_authority.clone())
            .map_err(normalize_coding_edit_error)?
            .apply_patch_preconditions(&expected)
            .map_err(normalize_coding_edit_error)
    }

    fn apply_coding_patch(
        &self,
        patch: &str,
        expected: &Map<String, Value>,
    ) -> Result<Vec<String>, FacadeError> {
        let expected = typed_expected_files(expected)?;
        CodingEditService::with_authority(self.workspace_authority.clone())
            .map_err(normalize_coding_edit_error)?
            .apply_patch(patch, &expected)
            .map_err(normalize_coding_edit_error)
    }

    fn runtime_discovery(&self) -> Value {
        let summary = self.shell_executor.discovery_summary();
        let core_version = summary.powershell_core_version.map(|version| {
            format!(
                "{}.{}.{}.{}",
                version.major, version.minor, version.patch, version.revision
            )
        });
        let auto_resolved = summary.auto_resolved.map(|kind| match kind {
            ResolvedShellKind::PowerShellCore => "pwsh",
            ResolvedShellKind::WindowsPowerShell => "windows_powershell",
            ResolvedShellKind::Cmd => "cmd",
        });
        json!({
            "shells": {
                "cmd":{"available":summary.cmd_available,"trusted":summary.cmd_available},
                "powershell_core":{"available":summary.powershell_core_available,"trusted":summary.powershell_core_available,"version":core_version},
                "windows_powershell":{"available":summary.windows_powershell_available,"trusted":summary.windows_powershell_available},
                "auto_resolved":auto_resolved
            },
            "git":{"available":true},
            "bundled_python":{"available":true},
            "bundled_node":{"available":false,"reason":"not_bundled"},
            "toolbox":self.toolbox.discovery()
        })
    }

    fn normalize_workspace_path(
        &self,
        path: &str,
        allow_missing_leaf: bool,
    ) -> Result<String, FacadeError> {
        self.normalized_workspace_path(path, allow_missing_leaf)
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

    fn filesystem(&mut self, arguments: Value) -> Result<Value, FacadeError> {
        run_workspace_filesystem_with_authority(
            self.workspace_authority.clone(),
            arguments,
            FilesystemCancellation::default(),
        )
    }

    fn apply_directory_change(&mut self, action: &str, path: &str) -> Result<Value, FacadeError> {
        if !workspace_relative_path_valid(path) || path == "." {
            return Err(FacadeError::new(
                FacadeErrorCode::WorkspaceDenied,
                "目录路径必须位于当前工作区内",
                false,
            ));
        }
        let service = FilesystemService::from_authority(self.workspace_authority.clone())
            .map_err(normalize_filesystem_error)?;
        match action {
            "create_directory" => {
                let result = service
                    .create_directory(path)
                    .map_err(normalize_filesystem_error)?;
                Ok(json!({"action":action,"path":result.path,"changed":result.changed}))
            }
            "remove_empty_directory" => {
                let result = service
                    .remove_empty_directory(path)
                    .map_err(normalize_filesystem_error)?;
                Ok(json!({"action":action,"path":result.path,"changed":result.changed}))
            }
            _ => Err(invalid_argument()),
        }
    }

    fn execute_shell(
        &mut self,
        mut request: ShellCommandRequest,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let normalized_cwd = self
            .normalized_workspace_path(request.execution.cwd.to_string_lossy().as_ref(), false)?;
        let resolved_cwd = self.resolve_existing_workspace_path(&normalized_cwd)?;
        if !resolved_cwd.is_dir() {
            return Err(FacadeError::new(
                FacadeErrorCode::InvalidArgument,
                "工作目录必须是目录",
                false,
            ));
        }
        request.execution.cwd = PathBuf::from(normalized_cwd);
        let selector = request.execution.shell;
        let started_public = self.public_commands.start_session(
            &self.executions,
            request.owner_task_id.clone(),
            request.owner_session.clone(),
        )?;
        let public_session_id = started_public.public_session_id;
        let adoption_token = started_public.adoption_token;
        let outcome = (|| {
            let invocation = self
                .shell_executor
                .runtime_invocation(&request.execution)
                .map_err(|error| normalize_shell_error(error, selector))?;
            let mut private = json!({
                "cmd": invocation.command_line,
                "workdir": request.execution.cwd,
                "timeout_ms": request.execution.timeout_ms,
                "yield_time_ms": request.yield_time_ms,
                "max_output_bytes": request.execution.max_output_bytes,
                "verbosity":"full",
                "env":{
                    "COMSPEC":invocation.comspec.to_string_lossy(),
                    "LOCALBRIDGE_OUTPUT_ENCODING":invocation.output_encoding,
                    "PATH":self.toolbox.child_path(),
                    "NoDefaultCurrentDirectoryInExePath":"1"
                }
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
                self.public_commands.bind_private_session(
                    &self.executions,
                    &public_session_id,
                    private_session_id,
                )?;
            }
            let mut result = self.normalize_command_result(&raw, &public_session_id, None)?;
            if let Some(data) = result
                .pointer_mut("/structuredContent/data")
                .and_then(Value::as_object_mut)
            {
                data.insert(
                    "adoption_token".into(),
                    Value::String(adoption_token.expose().to_string()),
                );
            }
            Ok(result)
        })();
        match outcome {
            Ok(result) => Ok(result),
            Err(error) => {
                self.public_commands.mark_error_terminal(
                    &public_session_id,
                    &error,
                    &self.executions,
                )?;
                Err(error)
            }
        }
    }

    fn authorize_command_resource(
        &self,
        action: CommandControlAction,
        arguments: &Map<String, Value>,
        owner: &McpSessionId,
    ) -> Result<(), FacadeError> {
        if action == CommandControlAction::Read {
            let output_ref = required_string(arguments, "output_ref")?;
            return self
                .public_commands
                .output_owned_by(output_ref, owner, &self.executions)
                .then_some(())
                .ok_or_else(|| {
                    FacadeError::new(
                        FacadeErrorCode::OutputNotFound,
                        "output handle is unavailable to the current MCP session",
                        false,
                    )
                });
        }
        let public_session = required_string(arguments, "session_id")?;
        let execution = self
            .executions
            .execution_for_public_session(&PublicSessionId::new(public_session))
            .ok_or_else(session_unavailable)?;
        if execution.owner_session.as_ref() == Some(owner) {
            Ok(())
        } else {
            Err(FacadeError::new(
                FacadeErrorCode::TaskNotOwned,
                "command session is not owned by the current MCP session",
                false,
            ))
        }
    }

    fn transfer_workflow_executions(
        &self,
        task_id: &TaskId,
        owner: &McpSessionId,
    ) -> Result<(), FacadeError> {
        self.executions
            .transfer_orphaned_workflow_executions(task_id, owner)
            .map_err(normalize_execution_registry_error)
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
            let stream = object
                .get("stream")
                .and_then(Value::as_str)
                .unwrap_or("stdout");
            let retained_stream = self
                .public_commands
                .output_stream(public_output_ref)
                .ok_or_else(|| {
                    FacadeError::new(
                        FacadeErrorCode::OutputNotFound,
                        "输出句柄不存在或已超过保留期",
                        false,
                    )
                    .with_details(json!({"output_ref":public_output_ref}))
                })?;
            if stream != retained_stream {
                return Err(FacadeError::new(
                    FacadeErrorCode::InvalidArgument,
                    "stream 与 output_ref 所属输出流不一致",
                    false,
                )
                .with_details(json!({
                    "field":"stream",
                    "output_ref":public_output_ref,
                    "expected":retained_stream,
                    "actual":stream
                })));
            }
            if let Some((_retained_stream, content)) =
                self.public_commands.local_output(public_output_ref)
            {
                return public_local_output_page(
                    public_output_ref,
                    stream,
                    &content,
                    object.get("offset").and_then(Value::as_u64).unwrap_or(0),
                    object
                        .get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or(65_536),
                );
            }
            let private_output_ref = self
                .public_commands
                .private_output(public_output_ref)
                .ok_or_else(|| {
                    FacadeError::new(
                        FacadeErrorCode::OutputNotFound,
                        "输出句柄不存在或已超过保留期",
                        false,
                    )
                    .with_details(json!({"output_ref":public_output_ref}))
                })?;
            if stream == "stderr" {
                let raw = self.private_call(
                    "read_output",
                    json!({"output_ref":private_output_ref,"stream":"stderr","offset":0,"limit":1048576}),
                    request_id,
                )?;
                return public_stderr_page(
                    &raw,
                    public_output_ref,
                    object.get("offset").and_then(Value::as_u64).unwrap_or(0),
                    object
                        .get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or(65_536),
                );
            }
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
            if let Some(terminal) = self.durable_command_terminal(&public_session_id) {
                return Ok(self
                    .public_commands
                    .terminal_with_pending(&public_session_id, terminal));
            }
            if let Some(running) = self
                .public_commands
                .running_with_pending(&public_session_id)
            {
                return Ok(running);
            }
        } else if self.durable_command_terminal(&public_session_id).is_some() {
            return Err(session_unavailable());
        }
        let private_session_id = self
            .public_commands
            .private_session(&self.executions, &public_session_id)
            .ok_or_else(session_unavailable)?;
        let public_session_key = PublicSessionId::new(public_session_id.clone());
        if action == CommandControlAction::Kill {
            let signal = object
                .get("signal")
                .and_then(Value::as_str)
                .unwrap_or("TERM");
            self.executions
                .request_cancellation(&public_session_key, signal)
                .map_err(normalize_execution_registry_error)?;
        }
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
                    command_control_transport_timeout(wait_ms),
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
                    command_control_transport_timeout(wait_ms),
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
                let cancellation_signal = self.executions.cancellation_signal(&public_session_key);
                if matches!(
                    error.code,
                    FacadeErrorCode::SessionUnavailable | FacadeErrorCode::RuntimeUnavailable
                ) && cancellation_signal.is_some()
                {
                    self.public_commands.mark_terminal(
                        &public_session_id,
                        stable_success(
                            json!({
                                "status":"cancelled",
                                "session_id":public_session_id,
                                "signal":cancellation_signal,
                                "output":""
                            }),
                            "Command cancelled",
                        ),
                        &self.executions,
                    )?;
                    let terminal = self
                        .durable_command_terminal(&public_session_id)
                        .ok_or_else(command_state_internal_error)?;
                    return Ok(self
                        .public_commands
                        .terminal_with_pending(&public_session_id, terminal));
                }
                if error.code != FacadeErrorCode::OperationTimedOut {
                    if action == CommandControlAction::Kill {
                        self.executions.clear_cancellation(&public_session_key);
                    }
                    self.public_commands.mark_error_terminal(
                        &public_session_id,
                        &error,
                        &self.executions,
                    )?;
                }
                Err(error)
            }
        }
    }

    fn git_workflow(
        &mut self,
        action: GitWorkflowAction,
        mut arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let normalized_project = match arguments.get("path").and_then(Value::as_str) {
            Some(path) => self.normalized_workspace_path(path, false)?,
            None => ".".to_string(),
        };
        arguments["path"] = Value::String(normalized_project.clone());
        if let Some(paths) = arguments.get_mut("paths").and_then(Value::as_array_mut) {
            for path_value in paths {
                let path = path_value.as_str().ok_or_else(invalid_argument)?;
                let qualified = if normalized_project == "." {
                    PathBuf::from(path)
                } else {
                    Path::new(&normalized_project).join(path)
                };
                let qualified = qualified.to_string_lossy().replace('\\', "/");
                if !workspace_relative_path_valid(&qualified) {
                    return Err(FacadeError::new(
                        FacadeErrorCode::WorkspaceDenied,
                        "Git path filter is outside the selected repository",
                        false,
                    ));
                }
                *path_value = Value::String(qualified);
            }
        }
        let private_name = match action {
            GitWorkflowAction::Status => "git_status",
            GitWorkflowAction::Diff => "git_diff",
            GitWorkflowAction::Log => "git_log",
            GitWorkflowAction::Show => "git_show",
            GitWorkflowAction::Blame => "git_blame",
        };
        let raw =
            handle_git_tool_with_authority(&self.workspace_authority, private_name, &arguments)
                .or_else(|| {
                    (action == GitWorkflowAction::Status).then(|| {
                        json!({
                            "structuredContent": {
                                "is_repo": false,
                                "path": normalized_project,
                                "repository_root": null,
                                "branch": null,
                                "head": null,
                                "upstream": null,
                                "ahead": 0,
                                "behind": 0,
                                "clean": true,
                                "entries": [],
                                "truncated": false
                            },
                            "isError": false
                        })
                    })
                })
                .ok_or_else(runtime_capability_mismatch)?;
        if raw.get("isError").and_then(Value::as_bool) == Some(true)
            || raw
                .pointer("/structuredContent/ok")
                .and_then(Value::as_bool)
                == Some(false)
        {
            return Err(normalize_git_error(&raw));
        }
        Ok(normalize_git_success(action, &raw))
    }

    fn execute_document(&self, request: DocumentRequest) -> Result<DocumentResult, FacadeError> {
        DocumentService::with_authority(self.workspace_authority.clone())
            .map_err(normalize_document_error)?
            .execute(request)
            .map_err(normalize_document_error)
    }

    fn apply_workflow_patch(
        &mut self,
        arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let object = object_args(&arguments)?;
        ensure_only_keys(object, &["patch", "dry_run"])?;
        let patch = required_string(object, "patch")?;
        if optional_bool(object, "dry_run", false)? {
            return Err(FacadeError::new(
                FacadeErrorCode::InvalidArgument,
                "dry_run is not supported on the committed document path",
                false,
            ));
        }
        let affected = CodingEditService::with_authority(self.workspace_authority.clone())
            .map_err(normalize_coding_edit_error)?
            .apply_patch_to_current(patch)
            .map_err(normalize_coding_edit_error)?;
        Ok(stable_success(
            json!({"applied":true,"affected_files":affected}),
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
        let bytes = FilesystemService::from_authority(self.workspace_authority.clone())
            .map_err(normalize_filesystem_error)?
            .read_bytes_bounded(relative, 10 * 1024 * 1024)
            .map_err(|error| match error {
                FilesystemError::NotFound | FilesystemError::Io => {
                    FacadeError::new(FacadeErrorCode::NotFound, "图像不可读", false)
                }
                other => normalize_filesystem_error(other),
            })?;
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
        self.public_commands.reap_expired_mappings(&self.executions);
        match self.runtime.root_is_running() {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                self.public_commands
                    .mark_all_running_lost(&self.executions)?;
                return Ok(());
            }
        }
        let running = self.public_commands.running_sessions(&self.executions);
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
                        &self.executions,
                    )?;
                }
            }
        }
        self.executions
            .observe_all_running()
            .map_err(normalize_execution_registry_error)?;
        Ok(())
    }

    fn has_running_execution(&self) -> bool {
        self.public_commands.has_running_session(&self.executions)
    }

    fn load_workflow_checkpoint(&self) -> Result<Option<Value>, FacadeError> {
        self.workflow_checkpoint
            .load::<Value>()
            .map_err(workflow_checkpoint_error)?
            .map(|checkpoint| serde_json::to_value(checkpoint).map_err(workflow_checkpoint_error))
            .transpose()
    }

    fn save_workflow_checkpoint(&self, checkpoint: &Value) -> Result<(), FacadeError> {
        let checkpoint: WorkflowCheckpoint =
            serde_json::from_value(checkpoint.clone()).map_err(workflow_checkpoint_error)?;
        self.workflow_checkpoint
            .save(&checkpoint)
            .map_err(workflow_checkpoint_error)
    }

    fn clear_workflow_checkpoint(&self) -> Result<(), FacadeError> {
        self.workflow_checkpoint
            .clear()
            .map_err(workflow_checkpoint_error)
    }

    fn durable_command_terminal(&self, session_id: &str) -> Option<Value> {
        let execution = self
            .executions
            .execution_for_public_session(&PublicSessionId::new(session_id))?;
        let ExecutionState::Terminal(terminal) = execution.state else {
            return None;
        };
        let mut data = Map::new();
        data.insert("session_id".into(), Value::String(session_id.to_string()));
        data.insert(
            "execution_id".into(),
            Value::String(execution.id.to_string()),
        );
        data.insert(
            "status".into(),
            Value::String(terminal.outcome.as_str().to_string()),
        );
        if let Some(exit_code) = terminal.exit_code {
            data.insert("exit_code".into(), Value::from(exit_code));
        }
        if let Some(signal) = terminal.signal {
            data.insert("signal".into(), Value::String(signal));
        }
        let output_refs = self
            .public_commands
            .output_refs_by_stream(&terminal.output_refs);
        if !output_refs.is_empty() {
            if let Some(stdout) = output_refs.get("stdout").cloned() {
                data.insert("output_ref".into(), stdout);
            }
            data.insert("output_refs".into(), Value::Object(output_refs));
        }
        match terminal.outcome {
            TerminalOutcome::Completed => Some(stable_success(
                Value::Object(data),
                command_summary("completed"),
            )),
            TerminalOutcome::Failed | TerminalOutcome::Blocked => Some(stable_command_error(
                FacadeErrorCode::ProcessFailed,
                command_summary("failed"),
                data,
            )),
            TerminalOutcome::TimedOut => Some(stable_command_error(
                FacadeErrorCode::ProcessTimedOut,
                command_summary("timed_out"),
                data,
            )),
            TerminalOutcome::Cancelled => Some(stable_success(
                Value::Object(data),
                command_summary("cancelled"),
            )),
            TerminalOutcome::Lost => Some(stable_command_error(
                FacadeErrorCode::SessionUnavailable,
                "命令会话不可用",
                data,
            )),
        }
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
        let observed_status = command_public_status(private_status, exit_code, timed_out);
        let cancellation_signal = self
            .executions
            .cancellation_signal(&PublicSessionId::new(public_session_id));
        let public_status = if cancellation_signal.is_some()
            && matches!(observed_status, "completed" | "failed" | "cancelled")
        {
            "cancelled"
        } else {
            observed_status
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
        } else if let Some(signal) = cancellation_signal {
            data.insert("signal".into(), Value::String(signal));
        }
        data.insert(
            "session_id".into(),
            Value::String(public_session_id.to_string()),
        );
        if let Some((task_id, execution_id, elapsed_ms)) = self
            .public_commands
            .stable_metadata(&self.executions, public_session_id)
        {
            data.insert("task_id".into(), Value::String(task_id));
            data.insert("execution_id".into(), Value::String(execution_id));
            data.insert("elapsed_ms".into(), Value::from(elapsed_ms));
        }
        if let Some(value) = structured.and_then(|object| object.get("truncated")) {
            if value.is_boolean() {
                data.insert("truncated".into(), value.clone());
            }
        }
        let output = self.safe_command_output_for_session(raw, public_session_id);
        data.insert("output".into(), Value::String(output));
        self.map_private_output_refs(structured, &mut data, public_session_id);

        let result = match public_status {
            "running" => stable_success(Value::Object(data), command_summary("running")),
            "completed" => stable_success(Value::Object(data), command_summary("completed")),
            "failed" => stable_command_error(
                FacadeErrorCode::ProcessFailed,
                command_summary("failed"),
                data,
            ),
            "timed_out" => stable_command_error(
                FacadeErrorCode::ProcessTimedOut,
                command_summary("timed_out"),
                data,
            ),
            "cancelled" => stable_success(Value::Object(data), command_summary("cancelled")),
            _ => stable_command_error(
                FacadeErrorCode::SessionUnavailable,
                command_summary("lost"),
                data,
            ),
        };
        if public_status != "running" {
            self.public_commands.mark_terminal(
                public_session_id,
                result.clone(),
                &self.executions,
            )?;
        }
        if action == Some(CommandControlAction::Kill) && public_status == "cancelled" {
            self.mark_owner_workflow_waiting_after_kill(public_session_id)?;
        }
        Ok(result)
    }

    fn mark_owner_workflow_waiting_after_kill(&self, session_id: &str) -> Result<(), FacadeError> {
        self.workflow_checkpoint
            .settle_command_kill::<Value>(session_id)
            .map(|_| ())
            .map_err(workflow_checkpoint_error)
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
            let returned_bytes = content.len() as u64;
            data.insert("returned_bytes".into(), Value::from(returned_bytes));
            data.insert("content".into(), Value::String(content));
            if let Some(total_bytes) = structured
                .and_then(|object| {
                    object
                        .get("total_stream_bytes")
                        .or_else(|| object.get("total_bytes"))
                })
                .and_then(Value::as_u64)
            {
                data.insert("total_bytes".into(), Value::from(total_bytes));
            }
        } else {
            data.insert("returned_bytes".into(), Value::from(0u64));
            if let Some(total_bytes) = structured
                .and_then(|object| {
                    object
                        .get("total_stream_bytes")
                        .or_else(|| object.get("total_bytes"))
                })
                .and_then(Value::as_u64)
            {
                data.insert("total_bytes".into(), Value::from(total_bytes));
            }
        }
        stable_success(Value::Object(data), "Command output read")
    }

    fn map_private_output_refs(
        &mut self,
        structured: Option<&Map<String, Value>>,
        data: &mut Map<String, Value>,
        public_session_id: &str,
    ) {
        let Some(structured) = structured else {
            return;
        };
        if let Some(private) = structured.get("output_ref").and_then(Value::as_str) {
            let stream = primary_output_stream(structured, private);
            data.insert(
                "output_ref".into(),
                Value::String(self.public_commands.public_output_for_private(
                    private,
                    public_session_id,
                    stream,
                )),
            );
        }
        if let Some(private_refs) = structured.get("output_refs").and_then(Value::as_object) {
            let mut public_refs = Map::new();
            for stream in ["stdout", "stderr"] {
                if let Some(private) = private_refs.get(stream).and_then(Value::as_str) {
                    public_refs.insert(
                        stream.into(),
                        Value::String(self.public_commands.public_output_for_private(
                            private,
                            public_session_id,
                            stream,
                        )),
                    );
                }
            }
            if !public_refs.is_empty() {
                data.insert("output_refs".into(), Value::Object(public_refs));
            }
        }
    }
}

fn primary_output_stream<'a>(structured: &'a Map<String, Value>, output_ref: &str) -> &'a str {
    // The private primary handle may select stderr. Its stream ownership must
    // not depend on whether this response or the terminal observer arrives first.
    structured
        .get("output_refs")
        .and_then(Value::as_object)
        .and_then(|refs| {
            ["stdout", "stderr"]
                .into_iter()
                .find(|stream| refs.get(*stream).and_then(Value::as_str) == Some(output_ref))
        })
        .or_else(|| structured.get("output_stream").and_then(Value::as_str))
        .unwrap_or("stdout")
}

fn public_local_output_page(
    output_ref: &str,
    stream: &str,
    content: &str,
    requested_offset: u64,
    limit: u64,
) -> Result<Value, FacadeError> {
    let total = content.len() as u64;
    let start = requested_offset.min(total) as usize;
    if !content.is_char_boundary(start) {
        return Err(invalid_argument());
    }
    let mut end = start
        .saturating_add(limit.min(1_048_576) as usize)
        .min(content.len());
    while end > start && !content.is_char_boundary(end) {
        end -= 1;
    }
    if end == start && start < content.len() {
        end = content[start..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| start + index)
            .unwrap_or(content.len());
    }
    let next_offset = (end < content.len()).then_some(end as u64);
    Ok(stable_success(
        json!({
            "output_ref":output_ref,
            "stream":stream,
            "offset":start as u64,
            "requested_offset":requested_offset,
            "limit":limit,
            "next_offset":next_offset,
            "returned_bytes":end - start,
            "total_bytes":total,
            "truncated":next_offset.is_some(),
            "content":&content[start..end]
        }),
        "Command output read",
    ))
}

fn public_stderr_page(
    raw: &Value,
    public_output_ref: &str,
    requested_offset: u64,
    limit: u64,
) -> Result<Value, FacadeError> {
    let raw_content = raw
        .pointer("/structuredContent/content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let public = public_command_stderr(raw_content);
    let total = public.len() as u64;
    let start = requested_offset.min(total) as usize;
    if !public.is_char_boundary(start) {
        return Err(invalid_argument());
    }
    let mut end = start
        .saturating_add(limit.min(1_048_576) as usize)
        .min(public.len());
    while end > start && !public.is_char_boundary(end) {
        end -= 1;
    }
    if end == start && start < public.len() {
        end = public[start..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| start + index)
            .unwrap_or(public.len());
    }
    let content = &public[start..end];
    let next_offset = (end < public.len()).then_some(end as u64);
    Ok(stable_success(
        json!({
            "output_ref":public_output_ref,
            "stream":"stderr",
            "offset":start as u64,
            "requested_offset":requested_offset,
            "limit":limit,
            "returned_bytes":content.len(),
            "content":content,
            "next_offset":next_offset,
            "total_bytes":total,
            "truncated":next_offset.is_some()
        }),
        "Command output read",
    ))
}

fn command_summary(status: &str) -> &'static str {
    match status {
        "running" => "Command running",
        "completed" => "Command completed",
        "failed" => "Command failed",
        "cancelled" => "Command cancelled",
        "timed_out" => "Command timed out",
        _ => "Command lost",
    }
}

fn command_public_status(
    private_status: &str,
    exit_code: Option<i64>,
    timed_out: bool,
) -> &'static str {
    if timed_out || private_status == "timeout" {
        "timed_out"
    } else if matches!(private_status, "terminated" | "killed" | "cancelled") {
        "cancelled"
    } else if matches!(private_status, "terminating" | "running") {
        "running"
    } else if exit_code.is_some_and(|code| code != 0) {
        "failed"
    } else {
        "completed"
    }
}

fn execution_terminal_from_result(result: &Value) -> ExecutionTerminal {
    let data = result
        .pointer("/structuredContent/data")
        .and_then(Value::as_object);
    let mut error_code = result
        .pointer("/structuredContent/error/code")
        .and_then(Value::as_str)
        .map(str::to_string);
    let status = match data
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
    {
        Some("completed") => TerminalOutcome::Completed,
        Some("timed_out") => TerminalOutcome::TimedOut,
        Some("cancelled") => TerminalOutcome::Cancelled,
        Some("failed") => TerminalOutcome::Failed,
        _ => match error_code.as_deref() {
            Some("ProcessTimedOut") => TerminalOutcome::TimedOut,
            Some("ProcessCancelled") => TerminalOutcome::Cancelled,
            Some(
                "SessionUnavailable"
                | "RuntimeUnavailable"
                | "RuntimeProtocolMismatch"
                | "RuntimeCapabilityMismatch",
            ) => TerminalOutcome::Lost,
            _ => TerminalOutcome::Failed,
        },
    };
    if error_code.is_none() {
        error_code = match status {
            TerminalOutcome::Completed => None,
            TerminalOutcome::Cancelled => Some("ProcessCancelled".to_string()),
            TerminalOutcome::TimedOut => Some("ProcessTimedOut".to_string()),
            TerminalOutcome::Lost => Some("SessionUnavailable".to_string()),
            TerminalOutcome::Failed | TerminalOutcome::Blocked => Some("ProcessFailed".to_string()),
        };
    }
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
    ExecutionTerminal {
        outcome: status,
        exit_code: data
            .and_then(|value| value.get("exit_code"))
            .and_then(Value::as_i64),
        signal: data
            .and_then(|value| value.get("signal"))
            .and_then(Value::as_str)
            .map(str::to_string),
        output_refs,
        error_code,
        completed_at_ms: unix_time_ms(),
    }
}

fn normalize_execution_registry_error(error: ExecutionRegistryError) -> FacadeError {
    match error {
        ExecutionRegistryError::CapacityExceeded => FacadeError::new(
            FacadeErrorCode::QueueCapacityExceeded,
            "执行容量已满，请等待已有命令结束后重试",
            true,
        ),
        ExecutionRegistryError::OwnerConflict { .. }
        | ExecutionRegistryError::NotOrphaned(_)
        | ExecutionRegistryError::AdoptionDenied => FacadeError::new(
            FacadeErrorCode::TaskNotOwned,
            "execution ownership transfer requires an orphaned resource and a valid credential",
            false,
        ),
        _ => command_state_internal_error(),
    }
}

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn workflow_checkpoint_error<E: std::fmt::Display>(_error: E) -> FacadeError {
    FacadeError::new(FacadeErrorCode::Internal, "工作流恢复状态不可用", false)
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

fn command_control_transport_timeout(wait_ms: u64) -> std::time::Duration {
    std::time::Duration::from_millis(
        wait_ms
            .min(30_000)
            .saturating_add(COMMAND_CONTROL_UPSTREAM_HEADROOM_MS),
    )
}

#[cfg(test)]
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

#[cfg(test)]
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

pub(crate) fn validate_workspace_context_probe(
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

fn compact_project_discovery(
    workspace: &Path,
    authority: &WorkspaceResolver,
    default_cwd: &str,
    git_status: Option<&Value>,
    runtime: &Value,
) -> Value {
    const MAX_COMPACT_MANIFEST_BYTES: usize = 128 * 1024;
    let project_root = authority
        .resolve_existing(default_cwd)
        .ok()
        .unwrap_or_else(|| workspace.to_path_buf());
    let filesystem = FilesystemService::from_authority(authority.clone()).ok();
    let manifest_text = |name: &str| {
        let relative = Path::new(default_cwd)
            .join(name)
            .to_string_lossy()
            .replace('\\', "/");
        filesystem
            .as_ref()?
            .read_bytes_bounded(&relative, MAX_COMPACT_MANIFEST_BYTES)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    };
    let package_json =
        manifest_text("package.json").and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let cargo_toml = manifest_text("Cargo.toml").and_then(|text| text.parse::<toml::Value>().ok());
    let pyproject =
        manifest_text("pyproject.toml").and_then(|text| text.parse::<toml::Value>().ok());

    let package_field = |name: &str| {
        package_json
            .as_ref()
            .and_then(|value| value.get(name))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let toml_project_field = |document: &Option<toml::Value>, table: &str, name: &str| {
        document
            .as_ref()
            .and_then(|value| value.get(table))
            .and_then(|value| value.get(name))
            .and_then(toml::Value::as_str)
            .map(str::to_string)
    };
    let project_name = package_field("name")
        .or_else(|| toml_project_field(&cargo_toml, "package", "name"))
        .or_else(|| toml_project_field(&pyproject, "project", "name"))
        .or_else(|| {
            project_root
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        });
    let project_version = package_field("version")
        .or_else(|| toml_project_field(&cargo_toml, "package", "version"))
        .or_else(|| toml_project_field(&pyproject, "project", "version"));
    let has_node = package_json.is_some();
    let has_rust = cargo_toml.is_some();
    let has_python = pyproject.is_some();
    let project_type = match (has_node, has_rust, has_python) {
        (true, true, _) => Some("node_rust"),
        (true, false, false) => Some("node"),
        (false, true, false) => Some("rust"),
        (false, false, true) => Some("python"),
        (true, false, true) => Some("node_python"),
        (false, true, true) => Some("rust_python"),
        (false, false, false) => None,
    };
    let project_file_exists = |name: &str| {
        let relative = Path::new(default_cwd)
            .join(name)
            .to_string_lossy()
            .replace('\\', "/");
        authority
            .resolve_existing(&relative)
            .is_ok_and(|path| path.is_file())
    };
    let package_manager = package_field("packageManager")
        .and_then(|value| value.split('@').next().map(str::to_string))
        .or_else(|| project_file_exists("pnpm-lock.yaml").then_some("pnpm".to_string()))
        .or_else(|| project_file_exists("yarn.lock").then_some("yarn".to_string()))
        .or_else(|| project_file_exists("package-lock.json").then_some("npm".to_string()))
        .or_else(|| has_rust.then_some("cargo".to_string()));
    let has_npm_build = package_json
        .as_ref()
        .and_then(|value| value.pointer("/scripts/build"))
        .and_then(Value::as_str)
        .is_some();
    let has_npm_test = package_json
        .as_ref()
        .and_then(|value| value.pointer("/scripts/test"))
        .and_then(Value::as_str)
        .is_some();
    let build_system = match (has_npm_build, has_rust) {
        (true, true) => Some("npm+cargo"),
        (true, false) => Some("npm"),
        (false, true) => Some("cargo"),
        _ if has_python => Some("python"),
        _ => None,
    };
    let test_system = match (has_npm_test, has_rust) {
        (true, true) => Some("npm+cargo"),
        (true, false) => Some("npm"),
        (false, true) => Some("cargo"),
        _ if has_python => Some("python"),
        _ => None,
    };
    let git_data = git_status.map(stable_data);
    let git_object = git_data.as_ref().and_then(Value::as_object);
    let git_branch = git_object
        .and_then(|data| data.get("branch"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let git_dirty = git_object
        .and_then(|data| data.get("clean"))
        .and_then(Value::as_bool)
        .map(|clean| !clean);
    let git_changed_count = git_object
        .and_then(|data| data.get("entries"))
        .and_then(Value::as_array)
        .map(|entries| entries.len() as u64);
    let trusted_shells = ["cmd", "powershell_core", "windows_powershell"]
        .into_iter()
        .filter(|name| {
            runtime
                .pointer(&format!("/shells/{name}/trusted"))
                .and_then(Value::as_bool)
                == Some(true)
        })
        .collect::<Vec<_>>();
    json!({
        "project_name": project_name,
        "project_type": project_type,
        "project_version": project_version,
        "git_branch": git_branch,
        "git_dirty": git_dirty,
        "git_changed_count": git_changed_count,
        "package_manager": package_manager,
        "build_system": build_system,
        "test_system": test_system,
        "runtime_availability": {
            "git": runtime.get("git").cloned().unwrap_or(Value::Null),
            "bundled_python": runtime.get("bundled_python").cloned().unwrap_or(Value::Null),
            "bundled_node": runtime.get("bundled_node").cloned().unwrap_or(Value::Null),
            "toolbox": runtime.get("toolbox").cloned().unwrap_or(Value::Null)
        },
        "trusted_shells": trusted_shells
    })
}

fn ordinary_workspace_paths_match(expected: &Path, actual: &str) -> bool {
    fn normalize(value: &str) -> String {
        value.replace('/', "\\").trim_end_matches('\\').to_string()
    }
    let expected = normalize(&expected.to_string_lossy());
    let actual = normalize(actual);
    #[cfg(windows)]
    {
        if expected.eq_ignore_ascii_case(&actual) {
            return true;
        }
        let Ok(expected_authority) =
            crate::workspace::WorkspaceResolver::active_workspace(Path::new(&expected))
        else {
            return false;
        };
        let Ok(actual_authority) =
            crate::workspace::WorkspaceResolver::active_workspace(Path::new(&actual))
        else {
            return false;
        };
        match (
            expected_authority.workspace_identity_token(),
            actual_authority.workspace_identity_token(),
        ) {
            (Some(expected_identity), Some(actual_identity)) => {
                expected_identity == actual_identity
            }
            _ => false,
        }
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

pub(crate) struct TaskCallIdentity<'a> {
    pub request_id: Option<&'a Value>,
    pub task_id: TaskId,
    pub owner_session: Option<McpSessionId>,
}

#[derive(Clone, Copy)]
struct WorkflowCallContext<'a> {
    request_id: Option<&'a Value>,
    task_id: &'a TaskId,
    owner_session: Option<&'a McpSessionId>,
}

impl AgentFacade<CodingToolsRuntimeAdapter> {
    pub(crate) fn from_coding_runtime_with_executions(
        runtime: CodingToolsRuntime,
        policy: CapabilityPolicy,
        executions: ExecutionRegistry,
    ) -> Result<Self, FacadeError> {
        let adapter = CodingToolsRuntimeAdapter::new_with_executions(runtime, executions)?;
        Self::with_adapter(adapter, policy)
    }

    pub(crate) fn workspace_authority(&self) -> WorkspaceResolver {
        self.adapter.workspace_authority.clone()
    }

    pub(crate) fn validate_workspace_identity(&self) -> Result<(), FacadeError> {
        self.adapter.validate_workspace_identity()
    }

    pub(crate) fn workspace_observation_seed(
        &self,
    ) -> Result<WorkspaceObservationSeed, FacadeError> {
        Ok(WorkspaceObservationSeed {
            workspace: self.adapter.workspace.to_string_lossy().into_owned(),
            default_cwd: self
                .adapter
                .cached_default_cwd
                .clone()
                .ok_or_else(runtime_capability_mismatch)?,
            project_discovery: self
                .adapter
                .cached_project_discovery
                .clone()
                .ok_or_else(runtime_capability_mismatch)?,
            runtime_discovery: self.adapter.runtime_discovery(),
        })
    }

    pub(crate) fn retain_local_output(
        &mut self,
        owner_session: McpSessionId,
        stream: &str,
        content: String,
    ) -> String {
        self.adapter
            .public_commands
            .retain_local_output(owner_session, stream, content)
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

    pub fn public_tools(&self) -> Value {
        stable_public_tool_catalog()
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

    pub fn live_runtime_health(&mut self) -> Result<CodingRuntimeHealth, FacadeError> {
        self.adapter.live_runtime_health()
    }

    pub fn take_runtime_fault(&mut self) -> Option<RuntimeFault> {
        self.adapter.take_runtime_fault()
    }

    #[cfg(test)]
    pub(crate) fn durable_coding_task_snapshot(&self) -> Option<Value> {
        let stored = self.adapter.load_workflow_checkpoint().ok().flatten()?;
        let checkpoint: WorkflowCheckpoint = serde_json::from_value(stored).ok()?;
        if !checkpoint.is_coding_task() {
            return None;
        }
        let settled = checkpoint.completed
            || (checkpoint.current_session_id.is_none() && checkpoint.next_step.is_none());
        let state = if settled {
            match checkpoint.current_step.as_deref() {
                Some("cancelled") => "cancelled",
                Some("failed") => "failed",
                _ => "completed",
            }
        } else if checkpoint.current_session_id.is_some() {
            "active"
        } else {
            "waiting"
        };
        Some(json!({
            "state":state,
            "kind":"coding_workflow",
            "task_id":checkpoint.workflow_id,
            "current_step":checkpoint.current_step,
            "next_step":checkpoint.next_step,
            "completed":settled,
            "output_refs":checkpoint.output_refs
        }))
    }

    fn durable_workflow_current_snapshot(&self) -> Option<Value> {
        let stored = self.adapter.load_workflow_checkpoint().ok().flatten()?;
        let checkpoint: WorkflowCheckpoint = serde_json::from_value(stored).ok()?;
        if checkpoint.completed {
            return None;
        }
        let state = if checkpoint.current_session_id.is_some()
            || checkpoint.command_inflight
            || checkpoint.patch_inflight
            || checkpoint.directory_inflight
        {
            "running"
        } else {
            "waiting"
        };
        Some(json!({
            "state":state, "task_id":checkpoint.workflow_id, "kind":if checkpoint.is_coding_task(){"coding_workflow"}else{"workflow"},
            "current_step":checkpoint.current_step, "next_step":checkpoint.next_step,
            "progress_current":if checkpoint.current_step.as_deref()==Some("verify"){Some(checkpoint.command_index)}else{None},
            "progress_total":if checkpoint.current_step.as_deref()==Some("verify"){Some(checkpoint.verification_plan.len())}else{None}
        }))
    }

    pub(crate) fn task_aggregate_snapshot(&self) -> Value {
        let current_workflow = self.durable_workflow_current_snapshot();
        let state = if current_workflow.is_some() {
            "waiting"
        } else {
            "idle"
        };
        let mut aggregate = json!({"state":state,"current_workflow":current_workflow});
        if let (Some(object), Some(workflow)) = (
            aggregate.as_object_mut(),
            current_workflow.as_ref().and_then(Value::as_object),
        ) {
            for key in ["task_id", "kind", "current_step", "next_step"] {
                if let Some(value) = workflow.get(key) {
                    object.insert(key.into(), value.clone());
                }
            }
        }
        aggregate
    }

    pub fn reap_command_sessions(&mut self) -> Result<(), FacadeError> {
        self.adapter.reap_command_sessions()?;
        self.reconcile_terminal_workflow_command()
    }

    fn reconcile_terminal_workflow_command(&mut self) -> Result<(), FacadeError> {
        let Some(stored) = self.adapter.load_workflow_checkpoint()? else {
            return Ok(());
        };
        let mut checkpoint: WorkflowCheckpoint =
            serde_json::from_value(stored).map_err(workflow_checkpoint_error)?;
        if checkpoint.completed {
            return Ok(());
        }
        let Some(session_id) = checkpoint.current_session_id.clone() else {
            return Ok(());
        };
        let Some(terminal) = self.adapter.durable_command_terminal(&session_id) else {
            return Ok(());
        };
        let data = stable_data(&terminal);
        let Some(status) = data.get("status").and_then(Value::as_str) else {
            return Ok(());
        };
        if status == "running" {
            return Ok(());
        }

        checkpoint.command_inflight = false;
        checkpoint.current_session_id = None;
        match status {
            "completed" => {
                checkpoint.command_index = checkpoint.command_index.saturating_add(1);
                checkpoint.command_results.push(data);
                checkpoint.failure = None;
                if checkpoint.is_coding_task() {
                    checkpoint.current_step = Some("verify".into());
                    checkpoint.next_step = Some(
                        if checkpoint.command_index < checkpoint.verification_plan.len() {
                            "verify"
                        } else {
                            "persist"
                        }
                        .into(),
                    );
                }
            }
            "cancelled" => {
                checkpoint.current_step = Some("cancelled".into());
                checkpoint.next_step = None;
                checkpoint.completed = true;
                checkpoint.failure = Some(WorkflowFailure::new("cancelled", Some(status)));
            }
            "failed" | "timed_out" | "lost" => {
                checkpoint.current_step = Some("failed".into());
                checkpoint.next_step = None;
                checkpoint.completed = true;
                checkpoint.failure = Some(WorkflowFailure::new(
                    if status == "lost" {
                        "SessionUnavailable"
                    } else {
                        "command_terminal"
                    },
                    Some(status),
                ));
            }
            _ => return Ok(()),
        }
        persist_agent_checkpoint(&self.adapter, &checkpoint)
    }

    pub fn has_running_execution(&self) -> bool {
        self.adapter.has_running_execution()
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
        project: F,
    ) -> Result<Value, FacadeCallError>
    where
        F: FnMut(CurrentTaskStatus),
    {
        self.call_tool_for_task(
            mode,
            name,
            arguments,
            TaskCallIdentity {
                request_id,
                task_id: TaskId::new(next_public_handle("lb-task")),
                owner_session: None,
            },
            project,
        )
    }

    pub(crate) fn call_tool_for_task<F>(
        &mut self,
        mode: PermissionMode,
        name: &str,
        arguments: Value,
        identity: TaskCallIdentity<'_>,
        mut project: F,
    ) -> Result<Value, FacadeCallError>
    where
        F: FnMut(CurrentTaskStatus),
    {
        let TaskCallIdentity {
            request_id,
            task_id,
            owner_session,
        } = identity;
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
        if name != "task_control" {
            if let Err(error) = self.adapter.validate_workspace_identity() {
                project(
                    CurrentTaskStatus::project(kind, summary, TaskExecutionState::Blocked)
                        .expect("Blocked is valid"),
                );
                project(CurrentTaskStatus::Idle);
                return Ok(error.to_mcp_result());
            }
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
        let result = self.dispatch_for_task(
            mode,
            name,
            arguments,
            request_id,
            &task_id,
            owner_session.as_ref(),
        );
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

    #[cfg(test)]
    fn dispatch(
        &mut self,
        mode: PermissionMode,
        name: &str,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        self.dispatch_for_task(
            mode,
            name,
            arguments,
            request_id,
            &TaskId::new(next_public_handle("lb-task")),
            None,
        )
    }

    fn dispatch_for_task(
        &mut self,
        mode: PermissionMode,
        name: &str,
        arguments: Value,
        request_id: Option<&Value>,
        task_id: &TaskId,
        owner_session: Option<&McpSessionId>,
    ) -> Result<Value, FacadeError> {
        match name {
            "workspace_context" => self.workspace_context(mode, arguments, request_id),
            "agent_workflow" => {
                self.agent_workflow(mode, arguments, request_id, task_id, owner_session)
            }
            "filesystem" => self.adapter.filesystem(arguments),
            "exec_command" => {
                self.exec_command(mode, arguments, request_id, task_id, owner_session)
            }
            "command_control" => self.command_control(arguments, request_id, owner_session),
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

    fn workspace_context(
        &mut self,
        mode: PermissionMode,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let detail = match arguments.get("detail") {
            None => "compact",
            Some(value) => match value.as_str() {
                Some("compact") => "compact",
                Some("full") => "full",
                _ => return Err(invalid_argument()),
            },
        };
        let mut result = self.adapter.workspace_context(request_id)?;
        let discovery = self.adapter.runtime_discovery();
        let policy_allowed_tools = V1_CORE_TOOL_NAMES
            .iter()
            .filter(|name| self.policy.public_tool_allowed_in_mode(mode, name))
            .map(|name| Value::String((*name).to_string()))
            .collect::<Vec<_>>();
        let mut public_tools = V1_CORE_TOOL_NAMES
            .iter()
            .map(|name| Value::String((*name).to_string()))
            .collect::<Vec<_>>();
        public_tools.push(Value::String("elevated_exec".into()));
        let data = result
            .pointer_mut("/structuredContent/data")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                FacadeError::new(FacadeErrorCode::Internal, "工作区上下文投影无效", false)
            })?;
        data.insert(
            "shell_discovery".into(),
            discovery
                .get("shells")
                .cloned()
                .unwrap_or_else(|| json!({})),
        );
        data.insert(
            "capabilities".into(),
            json!({
                "public_tools":public_tools,
                "policy_allowed_tools":policy_allowed_tools,
                "tool_schema_projection":"stable",
                "shells":discovery.get("shells").cloned().unwrap_or_else(|| json!({})),
                "git":discovery.get("git").cloned().unwrap_or_else(|| json!({"available":false})),
                "bundled_python":discovery.get("bundled_python").cloned().unwrap_or_else(|| json!({"available":false})),
                "bundled_node":discovery.get("bundled_node").cloned().unwrap_or_else(|| json!({"available":false})),
                "elevated_route":{"available":false,"reason":"broker_state_required"}
            }),
        );
        data.insert("current_task".into(), self.task_aggregate_snapshot());
        data.insert("detail".into(), Value::String(detail.into()));
        data.insert(
            "coding_profile".into(),
            Value::String("coding-agent-v1".into()),
        );
        if detail == "full" {
            data.insert(
                "coding_capabilities".into(),
                json!([
                    "workspace_discovery",
                    "project_instructions",
                    "context_search",
                    "command_execution",
                    "persistent_task",
                    "resume",
                    "patch_edit",
                    "test_build",
                    "git_status_diff",
                    "cancellation",
                    "output_continuation",
                    "typed_errors"
                ]),
            );
        }
        Ok(result)
    }

    fn policy_explanation(
        &self,
        mode: PermissionMode,
        tool_name: &str,
        arguments: &Value,
    ) -> Value {
        let decision = self.policy.decide_public(mode, tool_name, arguments);
        let (route, rule_category, remediation) = if decision.allowed {
            ("ordinary", "ordinary_allowed", "当前 ordinary route 可执行")
        } else {
            match decision.deny_reason {
                Some(
                    DenyReason::PrivilegedRouteNotAvailable | DenyReason::ElevatedExecNotReviewed,
                ) => (
                    "elevated_required",
                    "privileged_route",
                    "需要用户显式管理员授权与可用 Broker route",
                ),
                Some(DenyReason::ToolNotAllowedInMode | DenyReason::IndirectProcessExecInEdit) => (
                    "workspace_restricted",
                    "permission_mode",
                    "切换到允许该 ordinary capability 的用户权限模式",
                ),
                Some(DenyReason::VerbatimExecutionPath) => (
                    "workspace_restricted",
                    "workspace_boundary",
                    "使用普通 Win32 workspace 路径而不是 verbatim 路径",
                ),
                Some(DenyReason::ControlPlane | DenyReason::IndirectControlPlane) => (
                    "permanently_denied",
                    "control_plane",
                    "LocalBridge control-plane 不允许由 MCP/AI 修改",
                ),
                Some(DenyReason::NetworkRouteNotAvailable) => (
                    "permanently_denied",
                    "network_policy",
                    "该网络路线未获当前 public capability 授权",
                ),
                Some(DenyReason::UnknownTool | DenyReason::IndirectUnknownCapability) | None => (
                    "permanently_denied",
                    "unknown_capability",
                    "使用当前 workspace_context.capabilities 中声明的稳定能力",
                ),
            }
        };
        json!({
            "allowed":decision.allowed,
            "route":route,
            "rule_category":rule_category,
            "remediation":remediation,
            "would_execute":false
        })
    }

    fn agent_workflow(
        &mut self,
        mode: PermissionMode,
        arguments: Value,
        request_id: Option<&Value>,
        task_id: &TaskId,
        owner_session: Option<&McpSessionId>,
    ) -> Result<Value, FacadeError> {
        let object = object_args(&arguments)?;
        let action = required_string(object, "action")?;
        let workflow_context = WorkflowCallContext {
            request_id,
            task_id,
            owner_session,
        };
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
        if action == "resume" {
            ensure_only_keys(object, &["action", "task_id", "adoption_token"])?;
            let durable_task_id = object
                .get("task_id")
                .map(|value| {
                    value
                        .as_str()
                        .filter(|value| !value.trim().is_empty())
                        .ok_or_else(invalid_argument)
                })
                .transpose()?;
            return self.resume_agent_workflow(
                mode,
                request_id,
                task_id,
                owner_session,
                durable_task_id,
                object.get("adoption_token").and_then(Value::as_str),
            );
        }
        if let Some(phase) = object.get("phase").and_then(Value::as_str) {
            return self.coding_agent_workflow_phase(
                mode,
                action,
                phase,
                object,
                &workflow_context,
            );
        }

        let dry_run_requested = object
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let has_directory_changes = object
            .get("directory_changes")
            .and_then(Value::as_array)
            .is_some_and(|values| !values.is_empty());
        let has_commands = object
            .get("commands")
            .and_then(Value::as_array)
            .is_some_and(|values| !values.is_empty());
        let patch = object.get("patch").and_then(Value::as_str);

        // schema41 stale-projection compatibility: an older downstream client that only knows
        // action/objective/path/patch/resume can still drive the same durable coding task.
        if agent_action_allows_write(action)
            && !dry_run_requested
            && patch.is_none()
            && !has_directory_changes
            && !has_commands
            && object
                .get("objective")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        {
            let mut prepared = object.clone();
            prepared.insert("phase".into(), Value::String("prepare".into()));
            return self.coding_agent_workflow_phase(
                mode,
                action,
                "prepare",
                &prepared,
                &workflow_context,
            );
        }

        if !dry_run_requested && !has_directory_changes && !has_commands {
            if let Some(patch) = patch {
                if let Some(stored) = self.adapter.load_workflow_checkpoint()? {
                    let checkpoint: WorkflowCheckpoint =
                        serde_json::from_value(stored).map_err(workflow_checkpoint_error)?;
                    if checkpoint.is_coding_task()
                        && !checkpoint.completed
                        && checkpoint.next_step.as_deref() == Some("edit")
                    {
                        let original = workflow_object_args(&checkpoint.arguments)?;
                        let original_action = required_string(&original, "action")?;
                        let same_objective = object
                            .get("objective")
                            .and_then(Value::as_str)
                            .map(|value| checkpoint.objective.as_deref() == Some(value))
                            .unwrap_or(true);
                        let requested_path =
                            object.get("path").and_then(Value::as_str).unwrap_or(".");
                        let original_path =
                            original.get("path").and_then(Value::as_str).unwrap_or(".");
                        if original_action != action
                            || !same_objective
                            || requested_path != original_path
                        {
                            return Err(FacadeError::new(
                                FacadeErrorCode::SessionUnavailable,
                                "存在未完成的 coding task；旧客户端请求与该 task 不匹配",
                                false,
                            ));
                        }
                        let mut edited = Map::new();
                        edited.insert("action".into(), Value::String(action.to_string()));
                        edited.insert("phase".into(), Value::String("edit".into()));
                        edited.insert(
                            "task_id".into(),
                            Value::String(checkpoint.workflow_id.clone()),
                        );
                        edited.insert("patch".into(), Value::String(patch.to_string()));
                        edited.insert(
                            "expected_files".into(),
                            Value::Object(expected_files_from_checkpoint(&checkpoint)),
                        );
                        return self.coding_agent_workflow_phase(
                            mode,
                            action,
                            "edit",
                            &edited,
                            &workflow_context,
                        );
                    }
                }
            }
        }
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
        let dry_run = object
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if dry_run {
            let mut actual = arguments.clone();
            if let Some(actual) = actual.as_object_mut() {
                actual.remove("dry_run");
            }
            let explanation = self.policy_explanation(mode, "agent_workflow", &actual);
            let command_explanations = object
                .get("commands")
                .and_then(Value::as_array)
                .map(|commands| {
                    commands
                        .iter()
                        .filter_map(|command| command.as_object())
                        .map(|command| {
                            let args = json!({
                                "command":command.get("command").and_then(Value::as_str).unwrap_or_default(),
                                "shell":command.get("shell").and_then(Value::as_str).unwrap_or("auto"),
                                "workdir":command.get("workdir").and_then(Value::as_str).unwrap_or(".")
                            });
                            self.policy_explanation(mode, "exec_command", &args)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let workspace = self.workspace_context(mode, json!({}), request_id)?;
            return Ok(stable_success(
                json!({
                    "action":action,
                    "objective":object.get("objective").and_then(Value::as_str),
                    "state":"completed",
                    "workspace":stable_data(&workspace),
                    "project":project,
                    "git_before":{},
                    "patch_applied":false,
                    "directory_changes":[],
                    "commands":command_explanations,
                    "explain":explanation
                }),
                "Agent workflow policy explained without execution",
            ));
        }
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
        let has_work = !directory_changes.is_empty() || patch.is_some() || !commands.is_empty();
        let directory_count = directory_changes.len();
        let command_count = commands.len();
        if has_work {
            ensure_checkpoint_slot_available(&self.adapter)?;
        }
        let mut checkpoint = has_work.then(|| {
            let mut checkpoint = WorkflowCheckpoint::new(task_id.to_string(), arguments.clone());
            checkpoint.owner_session_id = owner_session.map(ToString::to_string);
            checkpoint
        });
        if let Some(checkpoint) = checkpoint.as_ref() {
            persist_agent_checkpoint(&self.adapter, checkpoint)?;
        }
        macro_rules! legacy_checkpoint_try {
            ($expression:expr) => {
                match $expression {
                    Ok(value) => value,
                    Err(error) => {
                        terminalize_legacy_checkpoint_failure(
                            &self.adapter,
                            &mut checkpoint,
                            error.code.as_str(),
                        )?;
                        return Err(error);
                    }
                }
            };
        }
        let workspace = legacy_checkpoint_try!(self.adapter.workspace_context(request_id));
        let git_before = legacy_checkpoint_try!(self.adapter.git_workflow(
            GitWorkflowAction::Status,
            json!({"path":selected_path}),
            request_id,
        ));
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
        for (index, (directory_action, directory_path)) in directory_changes.iter().enumerate() {
            if let Some(checkpoint) = checkpoint.as_mut() {
                checkpoint.current_step =
                    Some(format!("directory {}/{}", index + 1, directory_count));
                checkpoint.next_step = Some(
                    if index + 1 < directory_count {
                        "directory"
                    } else if patch.is_some() {
                        "patch"
                    } else if command_count > 0 {
                        "command"
                    } else {
                        "complete"
                    }
                    .into(),
                );
                checkpoint.directory_inflight = true;
                persist_agent_checkpoint(&self.adapter, checkpoint)?;
            }
            let result = legacy_checkpoint_try!(
                self.adapter
                    .apply_directory_change(directory_action, directory_path)
            );
            directory_results.push(result.clone());
            if let Some(checkpoint) = checkpoint.as_mut() {
                checkpoint.directory_inflight = false;
                checkpoint.directory_index = index + 1;
                checkpoint.directory_results.push(result);
                persist_agent_checkpoint(&self.adapter, checkpoint)?;
            }
        }

        let mut applied_patch = false;
        if let Some(patch) = patch {
            if !public_patch_targets_valid(patch) {
                let error = FacadeError::new(
                    FacadeErrorCode::WorkspaceDenied,
                    "补丁目标不在当前工作区内",
                    false,
                );
                terminalize_legacy_checkpoint_failure(
                    &self.adapter,
                    &mut checkpoint,
                    error.code.as_str(),
                )?;
                return Err(error);
            }
            if let Some(checkpoint) = checkpoint.as_mut() {
                checkpoint.current_step = Some("patch".into());
                checkpoint.next_step = Some(
                    if command_count > 0 {
                        "command"
                    } else {
                        "complete"
                    }
                    .into(),
                );
                checkpoint.patch_inflight = true;
                persist_agent_checkpoint(&self.adapter, checkpoint)?;
            }
            legacy_checkpoint_try!(
                self.adapter
                    .apply_workflow_patch(json!({"patch":patch,"dry_run":false}), request_id)
            );
            applied_patch = true;
            if let Some(checkpoint) = checkpoint.as_mut() {
                checkpoint.patch_inflight = false;
                checkpoint.patch_applied = true;
                persist_agent_checkpoint(&self.adapter, checkpoint)?;
            }
        }

        let mut command_results = Vec::new();
        for (index, command) in commands.into_iter().enumerate() {
            let command = legacy_checkpoint_try!(command.as_object().ok_or_else(invalid_argument));
            let text = legacy_checkpoint_try!(required_string(command, "command"));
            let shell: ShellSelector = legacy_checkpoint_try!(
                serde_json::from_value(
                    command
                        .get("shell")
                        .cloned()
                        .unwrap_or_else(|| Value::String("auto".into())),
                )
                .map_err(|_| invalid_argument())
            );
            let workdir = command
                .get("workdir")
                .and_then(Value::as_str)
                .unwrap_or(".");
            if !workspace_relative_path_valid(workdir) {
                let error = FacadeError::new(
                    FacadeErrorCode::WorkspaceDenied,
                    "工作区路径参数无效",
                    false,
                );
                terminalize_legacy_checkpoint_failure(
                    &self.adapter,
                    &mut checkpoint,
                    error.code.as_str(),
                )?;
                return Err(error);
            }
            let effective_workdir = legacy_checkpoint_try!(resolve_project_workdir(
                &self.adapter,
                &selected_path,
                workdir,
            ));
            if let Some(checkpoint) = checkpoint.as_mut() {
                checkpoint.current_step = Some(format!("command {}/{}", index + 1, command_count));
                checkpoint.next_step = Some(
                    if index + 1 < command_count {
                        "command"
                    } else {
                        "complete"
                    }
                    .into(),
                );
                checkpoint.command_inflight = true;
                checkpoint.current_session_id = None;
                persist_agent_checkpoint(&self.adapter, checkpoint)?;
            }
            let result = legacy_checkpoint_try!(
                self.adapter.execute_shell(
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
                                .unwrap_or(65_536)
                                as usize,
                        },
                        yield_time_ms: command
                            .get("yield_time_ms")
                            .and_then(Value::as_u64)
                            .unwrap_or(10_000),
                        stdin: command
                            .get("stdin")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        owner_task_id: checkpoint
                            .as_ref()
                            .map(|checkpoint| checkpoint.workflow_id.clone()),
                        owner_session: owner_session.cloned(),
                    },
                    request_id,
                )
            );
            if result.get("isError").and_then(Value::as_bool) == Some(true) {
                let code = result
                    .pointer("/structuredContent/error/code")
                    .and_then(Value::as_str)
                    .unwrap_or("ProcessFailed");
                terminalize_legacy_checkpoint_failure(&self.adapter, &mut checkpoint, code)?;
                return Ok(result);
            }
            let data = stable_data(&result);
            let running = data.get("status").and_then(Value::as_str) == Some("running");
            command_results.push(data.clone());
            if running {
                let session_id = legacy_checkpoint_try!(
                    data.get("session_id")
                        .and_then(Value::as_str)
                        .ok_or_else(command_state_internal_error)
                );
                if let Some(checkpoint) = checkpoint.as_mut() {
                    checkpoint.current_session_id = Some(session_id.to_string());
                    persist_agent_checkpoint(&self.adapter, checkpoint)?;
                }
                return Ok(stable_success(
                    json!({
                        "action":action,
                        "workflow_id":checkpoint.as_ref().map(|checkpoint| checkpoint.workflow_id.as_str()),
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
            if let Some(checkpoint) = checkpoint.as_mut() {
                checkpoint.command_inflight = false;
                checkpoint.current_session_id = None;
                checkpoint.command_index = index + 1;
                checkpoint.command_results.push(data);
                persist_agent_checkpoint(&self.adapter, checkpoint)?;
            }
        }

        let git_after = legacy_checkpoint_try!(self.adapter.git_workflow(
            GitWorkflowAction::Status,
            json!({"path":selected_path}),
            request_id,
        ));
        let state = if applied_patch || !directory_results.is_empty() || !command_results.is_empty()
        {
            "completed"
        } else {
            "context_ready"
        };
        if checkpoint.is_some() {
            self.adapter.clear_workflow_checkpoint()?;
        }
        let summary = if state == "context_ready" {
            "Agent workflow context ready"
        } else {
            "Agent workflow completed"
        };
        Ok(stable_success(
            json!({
                "action":action,
                "workflow_id":checkpoint.as_ref().map(|checkpoint| checkpoint.workflow_id.as_str()),
                "task_id":checkpoint.as_ref().map(|checkpoint| checkpoint.workflow_id.as_str()),
                "objective":object.get("objective").and_then(Value::as_str),
                "state":state,
                "summary":summary,
                "workspace":stable_data(&workspace),
                "project":project,
                "git_before":stable_data(&git_before),
                "git_after":stable_data(&git_after),
                "patch_applied":applied_patch,
                "directory_changes":directory_results,
                "commands":command_results
            }),
            summary,
        ))
    }

    fn coding_agent_workflow_phase(
        &mut self,
        _mode: PermissionMode,
        action: &str,
        phase: &str,
        object: &Map<String, Value>,
        context: &WorkflowCallContext<'_>,
    ) -> Result<Value, FacadeError> {
        let WorkflowCallContext {
            request_id,
            task_id: request_task_id,
            owner_session,
        } = *context;
        match phase {
            "prepare" => {
                ensure_only_keys(object, &["action", "phase", "objective", "path"])?;
                ensure_checkpoint_slot_available(&self.adapter)?;
                let objective = required_string(object, "objective")?;
                if objective.trim().is_empty() {
                    return Err(invalid_argument());
                }
                let project_path = object.get("path").and_then(Value::as_str).unwrap_or(".");
                let project = self.adapter.project_context(project_path)?;
                let selected_path = project
                    .get("selected_path")
                    .and_then(Value::as_str)
                    .ok_or_else(command_state_internal_error)?
                    .to_string();
                let context = self.adapter.coding_context(&selected_path, objective)?;
                let verification_plan = self.adapter.coding_verification_plan(&selected_path)?;
                let git_before = self.adapter.git_workflow(
                    GitWorkflowAction::Status,
                    json!({"path":selected_path}),
                    request_id,
                )?;
                let mut checkpoint = WorkflowCheckpoint::new_coding(
                    request_task_id.to_string(),
                    Value::Object(object.clone()),
                    objective.to_string(),
                );
                let adoption_token = new_workflow_adoption_token();
                checkpoint.adoption_token_hash =
                    Some(hash_workflow_adoption_token(&adoption_token));
                checkpoint.owner_session_id = owner_session.map(ToString::to_string);
                checkpoint.files_read = context
                    .get("files_read")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .collect();
                checkpoint.git_before = Some(stable_data(&git_before));
                checkpoint.verification_plan = verification_plan.clone().into_iter().collect();
                checkpoint.current_step = Some("prepare".into());
                checkpoint.next_step = Some("edit".into());
                persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                Ok(stable_success(
                    json!({
                        "action":action,
                        "phase":"prepare",
                        "workflow_id":checkpoint.workflow_id,
                        "task_id":checkpoint.workflow_id,
                        "adoption_token":adoption_token.expose(),
                        "objective":objective,
                        "state":"prepared",
                        "summary":"Coding task prepared",
                        "next_step":"edit",
                        "warnings":[],
                        "output_refs":[],
                        "project":project,
                        "context":context,
                        "verification_plan":verification_plan,
                        "git_before":checkpoint.git_before
                    }),
                    "Coding task prepared",
                ))
            }
            "edit" => {
                ensure_only_keys(
                    object,
                    &[
                        "action",
                        "phase",
                        "task_id",
                        "adoption_token",
                        "patch",
                        "expected_files",
                    ],
                )?;
                let task_id = required_string(object, "task_id")?;
                let patch = required_string(object, "patch")?;
                if !agent_action_allows_write(action) || !public_patch_targets_valid(patch) {
                    return Err(FacadeError::new(
                        FacadeErrorCode::WorkspaceDenied,
                        "补丁目标不在当前工作区内",
                        false,
                    ));
                }
                let empty_expected = Map::new();
                let expected = object
                    .get("expected_files")
                    .and_then(Value::as_object)
                    .unwrap_or(&empty_expected);
                let mut checkpoint = self.load_coding_checkpoint(
                    task_id,
                    action,
                    request_id,
                    owner_session,
                    object.get("adoption_token").and_then(Value::as_str),
                )?;
                if checkpoint.completed
                    || checkpoint.current_step.as_deref() != Some("prepare")
                    || checkpoint.next_step.as_deref() != Some("edit")
                    || checkpoint.patch_applied
                    || checkpoint.command_index != 0
                    || checkpoint.command_inflight
                    || checkpoint.current_session_id.is_some()
                {
                    return Err(FacadeError::new(
                        FacadeErrorCode::SessionUnavailable,
                        "coding task phase order requires prepare -> edit",
                        false,
                    ));
                }
                checkpoint.current_step = Some("edit".into());
                checkpoint.next_step = Some("verify".into());
                checkpoint.patch_inflight = true;
                persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                let modified_files = match self.adapter.apply_coding_patch(patch, expected) {
                    Ok(modified_files) => modified_files,
                    Err(error) => {
                        checkpoint.patch_inflight = false;
                        checkpoint.failure =
                            Some(WorkflowFailure::new(error.code.as_str(), None::<String>));
                        persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                        return Err(error);
                    }
                };
                checkpoint.patch_inflight = false;
                checkpoint.patch_applied = true;
                checkpoint.failure = None;
                for path in modified_files {
                    if !checkpoint.modified_files.contains(&path) {
                        checkpoint.modified_files.push(path);
                    }
                }
                persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                Ok(stable_success(
                    json!({
                        "action":action,
                        "phase":"edit",
                        "workflow_id":checkpoint.workflow_id,
                        "task_id":task_id,
                        "state":"editing",
                        "summary":"Coding edit applied",
                        "next_step":"verify",
                        "warnings":[],
                        "output_refs":checkpoint.output_refs,
                        "patch_applied":true,
                        "modified_files":checkpoint.modified_files
                    }),
                    "Coding edit applied",
                ))
            }
            "verify" => {
                ensure_only_keys(object, &["action", "phase", "task_id", "adoption_token"])?;
                let task_id = required_string(object, "task_id")?;
                let mut checkpoint = self.load_coding_checkpoint(
                    task_id,
                    action,
                    request_id,
                    owner_session,
                    object.get("adoption_token").and_then(Value::as_str),
                )?;
                if checkpoint.completed {
                    return Ok(coding_checkpoint_result(
                        &checkpoint,
                        "persisted",
                        "Coding task already persisted",
                    ));
                }
                if checkpoint.current_session_id.is_some() {
                    return Ok(coding_checkpoint_result(
                        &checkpoint,
                        "verifying",
                        "Verification session is already running; use resume",
                    ));
                }
                let entering_verify = checkpoint.current_step.as_deref() == Some("edit")
                    && checkpoint.next_step.as_deref() == Some("verify")
                    && checkpoint.patch_applied
                    && checkpoint.command_index == 0
                    && checkpoint.command_results.is_empty()
                    && checkpoint.failure.is_none();
                let retrying_verify = checkpoint.current_step.as_deref() == Some("verify")
                    && checkpoint.next_step.as_deref() == Some("verify")
                    && checkpoint.command_index <= checkpoint.verification_plan.len();
                if !entering_verify && !retrying_verify {
                    return Err(FacadeError::new(
                        FacadeErrorCode::SessionUnavailable,
                        "coding task phase order requires a completed edit before verify",
                        false,
                    ));
                }
                let original = workflow_object_args(&checkpoint.arguments)?;
                let project_path = original.get("path").and_then(Value::as_str).unwrap_or(".");
                let project = self.adapter.project_context(project_path)?;
                let selected_path = project
                    .get("selected_path")
                    .and_then(Value::as_str)
                    .ok_or_else(command_state_internal_error)?
                    .to_string();
                if entering_verify {
                    checkpoint.verification_plan = self
                        .adapter
                        .coding_verification_plan(&selected_path)?
                        .into_iter()
                        .collect();
                    checkpoint.commands = checkpoint.verification_plan.clone();
                }
                checkpoint.current_step = Some("verify".into());
                checkpoint.next_step = Some("persist".into());
                checkpoint.failure = None;
                persist_agent_checkpoint(&self.adapter, &checkpoint)?;

                for index in checkpoint.command_index..checkpoint.verification_plan.len() {
                    let step = checkpoint.verification_plan[index]
                        .as_object()
                        .ok_or_else(command_state_internal_error)?;
                    let command = required_workflow_string(step, "command")?;
                    let shell: ShellSelector = serde_json::from_value(
                        step.get("shell")
                            .cloned()
                            .unwrap_or_else(|| Value::String("cmd".into())),
                    )
                    .map_err(|_| invalid_argument())?;
                    checkpoint.command_inflight = true;
                    checkpoint.current_session_id = None;
                    persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                    let result = match self.adapter.execute_shell(
                        ShellCommandRequest {
                            execution: ShellExecutionSpec {
                                shell,
                                command: command.to_string(),
                                cwd: Path::new(&selected_path).to_path_buf(),
                                timeout_ms: 600_000,
                                max_output_bytes: 65_536,
                            },
                            yield_time_ms: 30_000,
                            stdin: None,
                            owner_task_id: Some(checkpoint.workflow_id.clone()),
                            owner_session: owner_session.cloned(),
                        },
                        request_id,
                    ) {
                        Ok(result) => result,
                        Err(error) => {
                            checkpoint.command_inflight = false;
                            checkpoint.current_session_id = None;
                            if error.code == FacadeErrorCode::ProcessCancelled {
                                checkpoint.current_step = Some("cancelled".into());
                                checkpoint.next_step = None;
                                checkpoint.completed = true;
                                checkpoint.failure =
                                    Some(WorkflowFailure::new("cancelled", None::<String>));
                            } else {
                                checkpoint.current_step = Some("verify".into());
                                checkpoint.next_step = Some("verify".into());
                                checkpoint.failure =
                                    Some(WorkflowFailure::new(error.code.as_str(), None::<String>));
                            }
                            persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                            return Err(error);
                        }
                    };
                    if result.get("isError").and_then(Value::as_bool) == Some(true) {
                        checkpoint.command_inflight = false;
                        checkpoint.failure =
                            Some(WorkflowFailure::at_step("verification_failed", index));
                        checkpoint.next_step = Some("verify".into());
                        persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                        return Ok(result);
                    }
                    let data = stable_data(&result);
                    if let Some(output_ref) = data.get("output_ref").and_then(Value::as_str) {
                        if !checkpoint
                            .output_refs
                            .iter()
                            .any(|value| value == output_ref)
                        {
                            checkpoint.output_refs.push(output_ref.to_string());
                        }
                    }
                    if data.get("status").and_then(Value::as_str) == Some("running") {
                        let session_id = data
                            .get("session_id")
                            .and_then(Value::as_str)
                            .ok_or_else(command_state_internal_error)?;
                        checkpoint.current_session_id = Some(session_id.to_string());
                        persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                        return Ok(coding_checkpoint_result(
                            &checkpoint,
                            "verifying",
                            "Verification command is running",
                        ));
                    }
                    checkpoint.command_inflight = false;
                    checkpoint.current_session_id = None;
                    checkpoint.command_index = index + 1;
                    checkpoint.command_results.push(data.clone());
                    let evidence = json!({
                        "kind":step.get("kind").and_then(Value::as_str).unwrap_or("project_gate"),
                        "command":command,
                        "status":data.get("status").cloned().unwrap_or_else(|| Value::String("completed".into())),
                        "exit_code":data.get("exit_code").cloned().unwrap_or(Value::Null),
                        "output_ref":data.get("output_ref").cloned().unwrap_or(Value::Null)
                    });
                    if command.contains("build") {
                        checkpoint.build_results.push(evidence);
                    } else {
                        checkpoint.test_results.push(evidence);
                    }
                    persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                }
                checkpoint.command_inflight = false;
                checkpoint.current_session_id = None;
                checkpoint.current_step = Some("verify".into());
                checkpoint.next_step = Some("persist".into());
                persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                Ok(coding_checkpoint_result(
                    &checkpoint,
                    "verifying",
                    "Verification plan completed",
                ))
            }
            "persist" => {
                ensure_only_keys(object, &["action", "phase", "task_id", "adoption_token"])?;
                let task_id = required_string(object, "task_id")?;
                let mut checkpoint = self.load_coding_checkpoint(
                    task_id,
                    action,
                    request_id,
                    owner_session,
                    object.get("adoption_token").and_then(Value::as_str),
                )?;
                if checkpoint.current_session_id.is_some()
                    || checkpoint.command_inflight
                    || checkpoint.current_step.as_deref() != Some("verify")
                    || checkpoint.next_step.as_deref() != Some("persist")
                    || checkpoint.failure.is_some()
                    || checkpoint.command_index != checkpoint.verification_plan.len()
                    || checkpoint.command_results.len() != checkpoint.verification_plan.len()
                    || checkpoint.command_results.iter().any(|result| {
                        result.get("status").and_then(Value::as_str) != Some("completed")
                    })
                {
                    return Err(FacadeError::new(
                        FacadeErrorCode::SessionUnavailable,
                        "coding task cannot persist until the full verification plan completes successfully",
                        true,
                    ));
                }
                if !checkpoint.completed {
                    let original = workflow_object_args(&checkpoint.arguments)?;
                    let project_path = original.get("path").and_then(Value::as_str).unwrap_or(".");
                    let project = self.adapter.project_context(project_path)?;
                    let selected_path = project
                        .get("selected_path")
                        .and_then(Value::as_str)
                        .ok_or_else(command_state_internal_error)?;
                    let git_after = self.adapter.git_workflow(
                        GitWorkflowAction::Status,
                        json!({"path":selected_path}),
                        request_id,
                    )?;
                    checkpoint.git_after = Some(stable_data(&git_after));
                    checkpoint.current_step = Some("persist".into());
                    checkpoint.next_step = None;
                    checkpoint.completed = true;
                    checkpoint.failure = None;
                    persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                }
                Ok(coding_checkpoint_result(
                    &checkpoint,
                    "persisted",
                    "Coding task persisted",
                ))
            }
            _ => Err(invalid_argument()),
        }
    }

    fn load_coding_checkpoint(
        &mut self,
        task_id: &str,
        action: &str,
        request_id: Option<&Value>,
        owner_session: Option<&McpSessionId>,
        adoption_token: Option<&str>,
    ) -> Result<WorkflowCheckpoint, FacadeError> {
        let stored = self.adapter.load_workflow_checkpoint()?.ok_or_else(|| {
            FacadeError::new(FacadeErrorCode::NotFound, "coding task 不存在", false)
        })?;
        let mut checkpoint: WorkflowCheckpoint =
            serde_json::from_value(stored).map_err(workflow_checkpoint_error)?;
        if !checkpoint.is_coding_task() || checkpoint.workflow_id != task_id {
            return Err(FacadeError::new(
                FacadeErrorCode::NotFound,
                "coding task 不存在",
                false,
            ));
        }
        if checkpoint.owner_session_id.as_deref() != owner_session.map(McpSessionId::as_str) {
            let Some(owner_session) = owner_session else {
                return Err(FacadeError::new(
                    FacadeErrorCode::TaskNotOwned,
                    "coding task 不属于当前 MCP Session",
                    false,
                ));
            };
            if !workflow_adoption_token_matches(&checkpoint, adoption_token) {
                return Err(FacadeError::new(
                    FacadeErrorCode::TaskNotOwned,
                    "coding task transfer requires its adoption_token",
                    false,
                ));
            }
            self.adapter.transfer_workflow_executions(
                &TaskId::new(checkpoint.workflow_id.clone()),
                owner_session,
            )?;
            checkpoint.owner_session_id = Some(owner_session.to_string());
            persist_agent_checkpoint(&self.adapter, &checkpoint)?;
        }
        let original = workflow_object_args(&checkpoint.arguments)?;
        if original.get("action").and_then(Value::as_str) != Some(action) {
            return Err(invalid_argument());
        }
        ensure_coding_git_baseline(&mut self.adapter, &checkpoint, request_id)?;
        Ok(checkpoint)
    }

    fn resume_coding_task(
        &mut self,
        mut checkpoint: WorkflowCheckpoint,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        if checkpoint.patch_inflight {
            return Err(FacadeError::new(
                FacadeErrorCode::SessionUnavailable,
                "coding task 的编辑步骤完成状态不可验证，已停止以避免重复编辑",
                false,
            ));
        }
        if checkpoint.completed {
            let state = match checkpoint.current_step.as_deref() {
                Some("cancelled") => "cancelled",
                Some("failed") => "failed",
                _ => "persisted",
            };
            return Ok(coding_checkpoint_result(
                &checkpoint,
                state,
                "Coding task is already terminal; no side effects were replayed",
            ));
        }
        if checkpoint.command_inflight && checkpoint.current_session_id.is_none() {
            return Err(FacadeError::new(
                FacadeErrorCode::SessionUnavailable,
                "coding task 的验证命令完成状态不可验证，已停止以避免重复执行",
                false,
            ));
        }
        let Some(session_id) = checkpoint.current_session_id.clone() else {
            let state = match checkpoint.current_step.as_deref() {
                Some("prepare") => "prepared",
                Some("edit") => "editing",
                Some("verify") => "verifying",
                Some("persist") => "persisted",
                Some("cancelled") => "cancelled",
                _ => "prepared",
            };
            return Ok(coding_checkpoint_result(
                &checkpoint,
                state,
                "Coding task resumed without replaying side effects",
            ));
        };

        let polled = match self.adapter.control_command(
            CommandControlAction::Poll,
            json!({"session_id":session_id,"wait_ms":0}),
            request_id,
        ) {
            Ok(result) => result,
            Err(error) if error.code == FacadeErrorCode::SessionUnavailable => self
                .adapter
                .durable_command_terminal(&session_id)
                .unwrap_or_else(|| error.to_mcp_result()),
            Err(error) => return Err(error),
        };
        let data = stable_data(&polled);
        match data.get("status").and_then(Value::as_str) {
            Some("running") => Ok(coding_checkpoint_result(
                &checkpoint,
                "verifying",
                "Verification session is still running",
            )),
            Some("completed") => {
                checkpoint.command_inflight = false;
                checkpoint.current_session_id = None;
                checkpoint.command_index = checkpoint.command_index.saturating_add(1);
                checkpoint.command_results.push(data.clone());
                if let Some(output_ref) = data.get("output_ref").and_then(Value::as_str) {
                    if !checkpoint
                        .output_refs
                        .iter()
                        .any(|value| value == output_ref)
                    {
                        checkpoint.output_refs.push(output_ref.to_string());
                    }
                }
                checkpoint.current_step = Some("verify".into());
                checkpoint.next_step = Some(
                    if checkpoint.command_index < checkpoint.verification_plan.len() {
                        "verify".into()
                    } else {
                        "persist".into()
                    },
                );
                persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                Ok(coding_checkpoint_result(
                    &checkpoint,
                    "verifying",
                    "Verification session completed; no next command was auto-started",
                ))
            }
            Some("failed" | "cancelled" | "timed_out" | "lost") => {
                let status = data
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("failed");
                checkpoint.command_inflight = false;
                checkpoint.current_session_id = None;
                checkpoint.failure =
                    Some(WorkflowFailure::new("verification_terminal", Some(status)));
                checkpoint.current_step = Some("verify".into());
                checkpoint.next_step = Some("verify".into());
                persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                Ok(coding_checkpoint_result(
                    &checkpoint,
                    "failed",
                    "Verification session ended unsuccessfully; no command was replayed",
                ))
            }
            _ => Err(FacadeError::new(
                FacadeErrorCode::SessionUnavailable,
                "coding task 验证会话状态不可恢复",
                false,
            )),
        }
    }

    fn resume_agent_workflow(
        &mut self,
        mode: PermissionMode,
        request_id: Option<&Value>,
        request_task_id: &TaskId,
        owner_session: Option<&McpSessionId>,
        durable_task_id: Option<&str>,
        adoption_token: Option<&str>,
    ) -> Result<Value, FacadeError> {
        let stored = self.adapter.load_workflow_checkpoint()?.ok_or_else(|| {
            FacadeError::new(FacadeErrorCode::NotFound, "没有可恢复的工作流", false)
        })?;
        let mut checkpoint: WorkflowCheckpoint =
            serde_json::from_value(stored).map_err(workflow_checkpoint_error)?;
        if let Some(durable_task_id) = durable_task_id {
            if checkpoint.workflow_id != durable_task_id {
                return Err(FacadeError::new(
                    FacadeErrorCode::NotFound,
                    "指定的 durable workflow 不存在或已回收",
                    false,
                ));
            }
        }
        if checkpoint.owner_session_id.as_deref() != owner_session.map(McpSessionId::as_str) {
            let Some(owner_session) = owner_session.filter(|_| durable_task_id.is_some()) else {
                return Err(FacadeError::new(
                    FacadeErrorCode::TaskNotOwned,
                    "工作流不属于当前 MCP Session；重连后必须使用 prepare 返回的 task_id",
                    false,
                ));
            };
            if !workflow_adoption_token_matches(&checkpoint, adoption_token) {
                return Err(FacadeError::new(
                    FacadeErrorCode::TaskNotOwned,
                    "workflow transfer requires its adoption_token",
                    false,
                ));
            }
            self.adapter.transfer_workflow_executions(
                &TaskId::new(checkpoint.workflow_id.clone()),
                owner_session,
            )?;
            checkpoint.owner_session_id = Some(owner_session.to_string());
            persist_agent_checkpoint(&self.adapter, &checkpoint)?;
        }
        let original = workflow_object_args(&checkpoint.arguments)?;
        let action = required_string(&original, "action")?;
        if action == "resume" {
            return Err(invalid_argument());
        }
        let policy_arguments = workflow_value(&checkpoint.arguments)?;
        if !self
            .policy
            .decide_public(mode, "agent_workflow", &policy_arguments)
            .allowed
        {
            return Err(FacadeError::new(
                FacadeErrorCode::CapabilityDenied,
                "当前权限模式不能恢复该工作流",
                false,
            ));
        }
        if checkpoint.is_coding_task() {
            ensure_coding_git_baseline(&mut self.adapter, &checkpoint, request_id)?;
            if checkpoint.current_session_id.is_none()
                && !checkpoint.command_inflight
                && !checkpoint.patch_inflight
                && !checkpoint.completed
            {
                if let Some(next_phase @ ("verify" | "persist")) = checkpoint.next_step.as_deref() {
                    let mut synthesized = Map::new();
                    synthesized.insert("action".into(), Value::String(action.to_string()));
                    synthesized.insert("phase".into(), Value::String(next_phase.to_string()));
                    synthesized.insert(
                        "task_id".into(),
                        Value::String(checkpoint.workflow_id.clone()),
                    );
                    let synthesized_value = Value::Object(synthesized.clone());
                    if !self
                        .policy
                        .decide_public(mode, "agent_workflow", &synthesized_value)
                        .allowed
                    {
                        return Err(FacadeError::new(
                            FacadeErrorCode::CapabilityDenied,
                            "当前权限模式不能继续 coding task 的下一阶段",
                            false,
                        ));
                    }
                    return self.coding_agent_workflow_phase(
                        mode,
                        action,
                        next_phase,
                        &synthesized,
                        &WorkflowCallContext {
                            request_id,
                            task_id: request_task_id,
                            owner_session,
                        },
                    );
                }
            }
            return self.resume_coding_task(checkpoint, request_id);
        }
        if checkpoint.completed {
            let state = match checkpoint.current_step.as_deref() {
                Some("cancelled") => "cancelled",
                Some("failed") => "failed",
                _ => "completed",
            };
            return Ok(stable_success(
                json!({
                    "action":"resume",
                    "workflow_id":checkpoint.workflow_id,
                    "objective":original.get("objective").and_then(Value::as_str),
                    "state":state,
                    "failure":checkpoint.failure
                }),
                "Agent workflow is already terminal; no side effects were replayed",
            ));
        }
        if checkpoint.directory_inflight || checkpoint.patch_inflight {
            return Err(FacadeError::new(
                FacadeErrorCode::SessionUnavailable,
                "工作流包含完成状态不可验证的文件步骤，已停止以避免重复执行",
                false,
            ));
        }
        if checkpoint.command_inflight && checkpoint.current_session_id.is_none() {
            return Err(FacadeError::new(
                FacadeErrorCode::SessionUnavailable,
                "工作流包含完成状态不可验证的命令步骤，已停止以避免重复执行",
                false,
            ));
        }

        let patch = original.get("patch").and_then(Value::as_str);
        let project_path = original.get("path").and_then(Value::as_str).unwrap_or(".");
        let mut project = self.adapter.project_context(project_path)?;
        let selected_path = project
            .get("selected_path")
            .and_then(Value::as_str)
            .ok_or_else(|| FacadeError::new(FacadeErrorCode::Internal, "项目上下文无效", false))?
            .to_string();
        let directory_changes = parse_directory_changes(&original)?;
        let commands = original
            .get("commands")
            .map(|value| value.as_array().ok_or_else(invalid_argument))
            .transpose()?
            .cloned()
            .unwrap_or_default();
        if checkpoint.directory_index > directory_changes.len()
            || checkpoint.command_index > commands.len()
        {
            return Err(workflow_checkpoint_error(
                "checkpoint progress out of range",
            ));
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

        if let Some(session_id) = checkpoint.current_session_id.clone() {
            let polled = match self.adapter.control_command(
                CommandControlAction::Poll,
                json!({"session_id":session_id,"wait_ms":0}),
                request_id,
            ) {
                Ok(result) => result,
                Err(error) if error.code == FacadeErrorCode::SessionUnavailable => self
                    .adapter
                    .durable_command_terminal(&session_id)
                    .unwrap_or_else(|| error.to_mcp_result()),
                Err(error) => return Err(error),
            };
            if polled.get("isError").and_then(Value::as_bool) == Some(true) {
                return Ok(polled);
            }
            let data = stable_data(&polled);
            match data.get("status").and_then(Value::as_str) {
                Some("running") => {
                    return Ok(stable_success(
                        json!({
                            "action":"resume",
                            "workflow_id":checkpoint.workflow_id,
                            "objective":original.get("objective").and_then(Value::as_str),
                            "state":"running",
                            "workspace":stable_data(&workspace),
                            "project":project,
                            "git_before":stable_data(&git_before),
                            "patch_applied":checkpoint.patch_applied,
                            "directory_changes":checkpoint.directory_results,
                            "commands":checkpoint.command_results,
                            "current_execution":data
                        }),
                        "Agent workflow command is still running",
                    ));
                }
                Some("completed") => {
                    checkpoint.command_inflight = false;
                    checkpoint.current_session_id = None;
                    checkpoint.command_index = checkpoint.command_index.saturating_add(1);
                    checkpoint.command_results.push(data);
                    persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                }
                _ => {
                    return Err(FacadeError::new(
                        FacadeErrorCode::SessionUnavailable,
                        "工作流命令状态不可恢复",
                        false,
                    ));
                }
            }
        }

        for (index, (directory_action, directory_path)) in directory_changes
            .iter()
            .enumerate()
            .skip(checkpoint.directory_index)
        {
            checkpoint.current_step = Some(format!(
                "directory {}/{}",
                index + 1,
                directory_changes.len()
            ));
            checkpoint.next_step = Some(
                if index + 1 < directory_changes.len() {
                    "directory"
                } else if patch.is_some_and(|_| !checkpoint.patch_applied) {
                    "patch"
                } else if checkpoint.command_index < commands.len() {
                    "command"
                } else {
                    "complete"
                }
                .into(),
            );
            checkpoint.directory_inflight = true;
            persist_agent_checkpoint(&self.adapter, &checkpoint)?;
            let result = self
                .adapter
                .apply_directory_change(directory_action, directory_path)?;
            checkpoint.directory_inflight = false;
            checkpoint.directory_index = index + 1;
            checkpoint.directory_results.push(result);
            persist_agent_checkpoint(&self.adapter, &checkpoint)?;
        }

        if let Some(patch) = patch.filter(|_| !checkpoint.patch_applied) {
            if !public_patch_targets_valid(patch) {
                return Err(FacadeError::new(
                    FacadeErrorCode::WorkspaceDenied,
                    "补丁目标不在当前工作区内",
                    false,
                ));
            }
            checkpoint.current_step = Some("patch".into());
            checkpoint.next_step = Some(
                if checkpoint.command_index < commands.len() {
                    "command"
                } else {
                    "complete"
                }
                .into(),
            );
            checkpoint.patch_inflight = true;
            persist_agent_checkpoint(&self.adapter, &checkpoint)?;
            self.adapter
                .apply_workflow_patch(json!({"patch":patch,"dry_run":false}), request_id)?;
            checkpoint.patch_inflight = false;
            checkpoint.patch_applied = true;
            persist_agent_checkpoint(&self.adapter, &checkpoint)?;
        }

        for (index, command) in commands.iter().enumerate().skip(checkpoint.command_index) {
            if checkpoint.redacted_stdin_command_indices.contains(&index) {
                return Err(FacadeError::new(
                    FacadeErrorCode::SessionUnavailable,
                    "durable workflow omitted sensitive stdin; start a new workflow to execute this pending command",
                    false,
                ));
            }
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
            let effective_workdir =
                resolve_project_workdir(&self.adapter, &selected_path, workdir)?;
            checkpoint.current_step = Some(format!("command {}/{}", index + 1, commands.len()));
            checkpoint.next_step = Some(
                if index + 1 < commands.len() {
                    "command"
                } else {
                    "complete"
                }
                .into(),
            );
            checkpoint.command_inflight = true;
            checkpoint.current_session_id = None;
            persist_agent_checkpoint(&self.adapter, &checkpoint)?;
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
                    owner_task_id: Some(checkpoint.workflow_id.clone()),
                    owner_session: owner_session.cloned(),
                },
                request_id,
            )?;
            if result.get("isError").and_then(Value::as_bool) == Some(true) {
                self.adapter.clear_workflow_checkpoint()?;
                return Ok(result);
            }
            let data = stable_data(&result);
            if data.get("status").and_then(Value::as_str) == Some("running") {
                let session_id = data
                    .get("session_id")
                    .and_then(Value::as_str)
                    .ok_or_else(command_state_internal_error)?;
                checkpoint.current_session_id = Some(session_id.to_string());
                persist_agent_checkpoint(&self.adapter, &checkpoint)?;
                let mut commands = checkpoint.command_results.clone();
                commands.push(data);
                return Ok(stable_success(
                    json!({
                        "action":"resume",
                        "workflow_id":checkpoint.workflow_id,
                        "objective":original.get("objective").and_then(Value::as_str),
                        "state":"running",
                        "workspace":stable_data(&workspace),
                        "project":project,
                        "git_before":stable_data(&git_before),
                        "patch_applied":checkpoint.patch_applied,
                        "directory_changes":checkpoint.directory_results,
                        "commands":commands
                    }),
                    "Agent workflow resumed and command is running",
                ));
            }
            checkpoint.command_inflight = false;
            checkpoint.current_session_id = None;
            checkpoint.command_index = index + 1;
            checkpoint.command_results.push(data);
            persist_agent_checkpoint(&self.adapter, &checkpoint)?;
        }

        let git_after = self.adapter.git_workflow(
            GitWorkflowAction::Status,
            json!({"path":selected_path}),
            request_id,
        )?;
        self.adapter.clear_workflow_checkpoint()?;
        Ok(stable_success(
            json!({
                "action":"resume",
                "workflow_id":checkpoint.workflow_id,
                "objective":original.get("objective").and_then(Value::as_str),
                "state":"completed",
                "workspace":stable_data(&workspace),
                "project":project,
                "git_before":stable_data(&git_before),
                "git_after":stable_data(&git_after),
                "patch_applied":checkpoint.patch_applied,
                "directory_changes":checkpoint.directory_results,
                "commands":checkpoint.command_results
            }),
            "Agent workflow resumed and completed",
        ))
    }

    fn exec_command(
        &mut self,
        mode: PermissionMode,
        arguments: Value,
        request_id: Option<&Value>,
        task_id: &TaskId,
        owner_session: Option<&McpSessionId>,
    ) -> Result<Value, FacadeError> {
        let parsed =
            ExecCommandArguments::parse(arguments.clone()).map_err(|()| invalid_argument())?;
        if parsed.dry_run {
            let mut actual = arguments.clone();
            if let Some(actual) = actual.as_object_mut() {
                actual.remove("dry_run");
            }
            let mut explanation = self.policy_explanation(mode, "exec_command", &actual);
            if let Some(data) = explanation.as_object_mut() {
                data.insert("status".into(), Value::String("completed".into()));
            }
            return Ok(stable_success(
                explanation,
                "Command policy explained without execution",
            ));
        }
        let spec = ShellExecutionSpec {
            shell: parsed.shell,
            command: parsed.command,
            cwd: parsed.workdir,
            timeout_ms: parsed.timeout_ms,
            max_output_bytes: parsed.max_output_bytes,
        };
        self.adapter.execute_shell(
            ShellCommandRequest {
                execution: spec,
                yield_time_ms: parsed.yield_time_ms,
                stdin: parsed.stdin,
                owner_task_id: Some(task_id.to_string()),
                owner_session: owner_session.cloned(),
            },
            request_id,
        )
    }

    fn command_control(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
        owner_session: Option<&McpSessionId>,
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
        let allowed = match action {
            CommandControlAction::Poll => &["action", "session_id", "wait_ms"][..],
            CommandControlAction::Read => {
                &["action", "output_ref", "stream", "offset", "limit"][..]
            }
            CommandControlAction::Write => &["action", "session_id", "chars", "wait_ms"][..],
            CommandControlAction::Kill => &["action", "session_id", "signal", "wait_ms"][..],
        };
        ensure_only_keys(object, allowed)?;
        if let Some(owner_session) = owner_session {
            self.adapter
                .authorize_command_resource(action, object, owner_session)?;
        }
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
        let allowed = match action {
            GitWorkflowAction::Status => {
                &["action", "path", "include_untracked", "max_entries"][..]
            }
            GitWorkflowAction::Diff => &[
                "action",
                "path",
                "paths",
                "staged",
                "unstaged",
                "context_lines",
                "max_bytes",
            ][..],
            GitWorkflowAction::Log => &["action", "path", "ref", "max_count", "skip"][..],
            GitWorkflowAction::Show => &[
                "action",
                "path",
                "paths",
                "rev",
                "context_lines",
                "max_bytes",
                "include_patch",
            ][..],
            GitWorkflowAction::Blame => &[
                "action",
                "path",
                "rev",
                "start_line",
                "end_line",
                "max_lines",
            ][..],
        };
        ensure_only_keys(object, allowed)?;
        if action == GitWorkflowAction::Blame {
            required_string(object, "path")?;
        }
        let mut stable = object.clone();
        stable.remove("action");
        self.adapter
            .git_workflow(action, Value::Object(stable), request_id)
    }

    fn document_workflow(
        &mut self,
        arguments: Value,
        _request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        let object = object_args(&arguments)?;
        let request = match required_string(object, "action")? {
            "inspect" => {
                ensure_only_keys(
                    object,
                    &["action", "path", "start_block", "max_blocks", "max_bytes"],
                )?;
                DocumentRequest::Inspect {
                    path: required_string(object, "path")?.to_string(),
                    start_block: optional_usize(object, "start_block", 1)?,
                    max_blocks: optional_usize(object, "max_blocks", 200)?,
                    max_bytes: optional_usize(object, "max_bytes", 1_048_576)?,
                }
            }
            "search" => {
                ensure_only_keys(
                    object,
                    &["action", "path", "query", "case_sensitive", "max_results"],
                )?;
                DocumentRequest::Search {
                    path: required_string(object, "path")?.to_string(),
                    query: required_string(object, "query")?.to_string(),
                    case_sensitive: optional_bool(object, "case_sensitive", false)?,
                    max_results: optional_usize(object, "max_results", 100)?,
                }
            }
            "create" => {
                ensure_only_keys(object, &["action", "path", "content", "source_format"])?;
                let path = required_string(object, "path")?;
                DocumentRequest::Create {
                    path: path.to_string(),
                    content: required_document_content(object)?.to_string(),
                    source_format: document_source_format(object, path)?,
                }
            }
            "edit" => {
                ensure_only_keys(object, &["action", "path", "expected_sha256", "edits"])?;
                DocumentRequest::Edit {
                    path: required_string(object, "path")?.to_string(),
                    expected_sha256: required_string(object, "expected_sha256")?.to_string(),
                    edits: document_edits(object)?,
                }
            }
            "convert" => {
                ensure_only_keys(object, &["action", "source", "path"])?;
                DocumentRequest::Convert {
                    source: required_string(object, "source")?.to_string(),
                    path: required_string(object, "path")?.to_string(),
                }
            }
            "rebuild" => {
                ensure_only_keys(
                    object,
                    &[
                        "action",
                        "path",
                        "content",
                        "source_format",
                        "expected_sha256",
                    ],
                )?;
                let path = required_string(object, "path")?;
                DocumentRequest::Rebuild {
                    path: path.to_string(),
                    content: required_document_content(object)?.to_string(),
                    source_format: document_source_format(object, path)?,
                    expected_sha256: required_string(object, "expected_sha256")?.to_string(),
                }
            }
            _ => return Err(invalid_argument()),
        };
        let action = match &request {
            DocumentRequest::Inspect { .. } => "inspect",
            DocumentRequest::Search { .. } => "search",
            DocumentRequest::Create { .. } => "create",
            DocumentRequest::Edit { .. } => "edit",
            DocumentRequest::Convert { .. } => "convert",
            DocumentRequest::Rebuild { .. } => "rebuild",
        };
        let result = self.adapter.execute_document(request)?;
        let data = serde_json::to_value(result).map_err(|_| {
            FacadeError::new(FacadeErrorCode::Internal, "文档结果无法序列化", false)
        })?;
        Ok(stable_success(data, document_success_summary(action)))
    }

    fn view_image(
        &mut self,
        arguments: Value,
        request_id: Option<&Value>,
    ) -> Result<Value, FacadeError> {
        self.adapter.inspect_image(arguments, request_id)
    }
}

pub(crate) fn public_task_kind(name: &str, arguments: &Value) -> TaskKind {
    match name {
        "elevated_exec" => TaskKind::ElevatedOperation,
        "exec_command" => command_task_kind(
            arguments
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
        "agent_workflow" | "command_control" | "task_control" => TaskKind::Other,
        "filesystem" => match arguments.get("action").and_then(Value::as_str) {
            Some("search" | "search_content") => TaskKind::SearchCode,
            Some("write" | "replace" | "patch" | "copy" | "move" | "delete") => {
                TaskKind::ModifyFile
            }
            Some("list" | "stat" | "read" | "hash") => TaskKind::ReadFile,
            _ => TaskKind::Other,
        },
        "git_workflow" => TaskKind::GitOperation,
        "document_workflow" => match arguments.get("action").and_then(Value::as_str) {
            Some("inspect" | "search") => TaskKind::ReadFile,
            Some("create" | "edit" | "convert" | "rebuild") => TaskKind::ModifyFile,
            _ => TaskKind::Other,
        },
        "workspace_context" | "view_image" => TaskKind::ReadFile,
        _ => TaskKind::Other,
    }
}

pub(crate) fn public_safe_summary(name: &str, arguments: &Value) -> SafeTaskSummary {
    let value = match name {
        "exec_command" => arguments.get("command").and_then(Value::as_str),
        "filesystem" => arguments
            .get("path")
            .or_else(|| arguments.get("source"))
            .and_then(Value::as_str),
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

fn required_document_content(object: &Map<String, Value>) -> Result<&str, FacadeError> {
    object
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(invalid_argument)
}

fn document_source_format(
    object: &Map<String, Value>,
    target_path: &str,
) -> Result<DocumentFormat, FacadeError> {
    match object.get("source_format") {
        Some(Value::String(value)) if value == "text" => Ok(DocumentFormat::Text),
        Some(Value::String(value)) if value == "markdown" => Ok(DocumentFormat::Markdown),
        Some(_) => Err(invalid_argument()),
        None => match DocumentFormat::from_path(target_path).map_err(normalize_document_error)? {
            DocumentFormat::Markdown | DocumentFormat::Docx => Ok(DocumentFormat::Markdown),
            DocumentFormat::Text | DocumentFormat::Pdf => Ok(DocumentFormat::Text),
        },
    }
}

fn document_edits(object: &Map<String, Value>) -> Result<Vec<DocumentEditOperation>, FacadeError> {
    let values = object
        .get("edits")
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
        .ok_or_else(invalid_argument)?;
    values
        .iter()
        .map(|value| {
            let edit = value.as_object().ok_or_else(invalid_argument)?;
            let operation = required_string(edit, "operation")?;
            let block_id = required_string(edit, "block_id")?.to_string();
            match operation {
                "replace" | "insert_before" | "insert_after" => {
                    ensure_only_keys(edit, &["operation", "block_id", "content"])?;
                    let content = edit
                        .get("content")
                        .and_then(Value::as_str)
                        .ok_or_else(invalid_argument)?
                        .to_string();
                    Ok(match operation {
                        "replace" => DocumentEditOperation::Replace { block_id, content },
                        "insert_before" => {
                            DocumentEditOperation::InsertBefore { block_id, content }
                        }
                        "insert_after" => DocumentEditOperation::InsertAfter { block_id, content },
                        _ => unreachable!(),
                    })
                }
                "delete" => {
                    ensure_only_keys(edit, &["operation", "block_id"])?;
                    Ok(DocumentEditOperation::Delete { block_id })
                }
                _ => Err(invalid_argument()),
            }
        })
        .collect()
}

fn document_success_summary(action: &str) -> &'static str {
    match action {
        "inspect" => "Document inspected",
        "search" => "Document searched",
        "create" => "Document created",
        "edit" => "Document edited",
        "convert" => "Document converted",
        "rebuild" => "Document rebuilt",
        _ => "Document operation completed",
    }
}

fn public_workspace_paths_valid(name: &str, arguments: &Value) -> bool {
    let Some(object) = arguments.as_object() else {
        return true;
    };
    let keys: &[&str] = match name {
        "exec_command" => &["workdir"],
        "filesystem" => &["path", "source", "destination"],
        "git_workflow" | "view_image" => &["path"],
        "document_workflow" => &["path", "source"],
        "agent_workflow" => &["path"],
        _ => &[],
    };
    if keys.iter().any(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|value| !workspace_input_path_valid(value))
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
    if name == "filesystem"
        && object.get("action").and_then(Value::as_str) == Some("patch")
        && object
            .get("patch")
            .and_then(Value::as_str)
            .is_some_and(|patch| !public_patch_targets_valid(patch))
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

fn resolve_project_workdir<A: WorkspaceRuntimeAdapter>(
    adapter: &A,
    project: &str,
    workdir: &str,
) -> Result<PathBuf, FacadeError> {
    if !workspace_relative_path_valid(project) || !workspace_relative_path_valid(workdir) {
        return Err(FacadeError::new(
            FacadeErrorCode::WorkspaceDenied,
            "工作区路径参数无效",
            false,
        ));
    }
    let requested = Path::new(project).join(workdir);
    let requested = requested.to_str().ok_or_else(|| {
        FacadeError::new(
            FacadeErrorCode::WorkspaceDenied,
            "工作区路径参数无效",
            false,
        )
    })?;
    adapter
        .normalize_workspace_path(requested, false)
        .map(PathBuf::from)
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

fn workflow_value(value: &Value) -> Result<Value, FacadeError> {
    Ok(value.clone())
}

fn workflow_object_args(value: &Value) -> Result<Map<String, Value>, FacadeError> {
    value.as_object().cloned().ok_or_else(invalid_argument)
}

fn ensure_checkpoint_slot_available<A: WorkspaceRuntimeAdapter>(
    adapter: &A,
) -> Result<(), FacadeError> {
    let Some(stored) = adapter.load_workflow_checkpoint()? else {
        return Ok(());
    };
    let checkpoint: WorkflowCheckpoint =
        serde_json::from_value(stored).map_err(workflow_checkpoint_error)?;
    if checkpoint.completed {
        return Ok(());
    }
    Err(FacadeError::new(
        FacadeErrorCode::SessionUnavailable,
        "an incomplete durable workflow already exists; resume or cancel it before starting another side-effecting workflow",
        false,
    ))
}

fn ensure_coding_git_baseline<A: WorkspaceRuntimeAdapter>(
    adapter: &mut A,
    checkpoint: &WorkflowCheckpoint,
    request_id: Option<&Value>,
) -> Result<(), FacadeError> {
    let Some(saved_datum) = checkpoint.git_before.as_ref() else {
        return Ok(());
    };
    let saved_value = workflow_value(saved_datum)?;
    let Some(saved) = saved_value.as_object() else {
        return Ok(());
    };
    if saved.get("is_repo").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }
    let original = workflow_object_args(&checkpoint.arguments)?;
    let project_path = original.get("path").and_then(Value::as_str).unwrap_or(".");
    let project = adapter.project_context(project_path)?;
    let selected_path = project
        .get("selected_path")
        .and_then(Value::as_str)
        .ok_or_else(command_state_internal_error)?;
    let current = adapter.git_workflow(
        GitWorkflowAction::Status,
        json!({"path":selected_path}),
        request_id,
    )?;
    let current = stable_data(&current);
    let same_root = saved.get("repository_root") == current.get("repository_root");
    let same_head =
        saved.get("head").and_then(Value::as_str) == current.get("head").and_then(Value::as_str);
    if same_root && same_head {
        return Ok(());
    }
    Err(FacadeError::new(
        FacadeErrorCode::FileChanged,
        "coding task Git baseline changed; run prepare again before continuing",
        false,
    ))
}

fn persist_agent_checkpoint<A: WorkspaceRuntimeAdapter>(
    adapter: &A,
    checkpoint: &WorkflowCheckpoint,
) -> Result<(), FacadeError> {
    let mut checkpoint = checkpoint.clone();
    let newly_redacted = sanitize_workflow_arguments(&mut checkpoint.arguments);
    checkpoint
        .redacted_stdin_command_indices
        .extend(newly_redacted);
    checkpoint.redacted_stdin_command_indices.sort_unstable();
    checkpoint.redacted_stdin_command_indices.dedup();
    let value = serde_json::to_value(checkpoint).map_err(workflow_checkpoint_error)?;
    adapter.save_workflow_checkpoint(&value)
}

fn sanitize_workflow_arguments(arguments: &mut Value) -> Vec<usize> {
    let mut redacted = Vec::new();
    let commands = match arguments {
        Value::Object(arguments) => arguments.get_mut("commands"),
        _ => None,
    };
    if let Some(Value::Array(commands)) = commands {
        for (index, command) in commands.iter_mut().enumerate() {
            if let Value::Object(object) = command {
                if object.remove("stdin").is_some() {
                    redacted.push(index);
                }
            }
        }
    }
    redacted
}

fn terminalize_legacy_checkpoint_failure<A: WorkspaceRuntimeAdapter>(
    adapter: &A,
    checkpoint: &mut Option<WorkflowCheckpoint>,
    code: &str,
) -> Result<(), FacadeError> {
    let Some(checkpoint) = checkpoint.as_mut() else {
        return Ok(());
    };
    checkpoint.current_step = Some("failed".into());
    checkpoint.next_step = None;
    checkpoint.completed = true;
    checkpoint.directory_inflight = false;
    checkpoint.patch_inflight = false;
    checkpoint.command_inflight = false;
    checkpoint.current_session_id = None;
    checkpoint.failure = Some(WorkflowFailure::new(code, Some("failed")));
    persist_agent_checkpoint(adapter, checkpoint)
}

fn expected_files_from_checkpoint(checkpoint: &WorkflowCheckpoint) -> Map<String, Value> {
    let mut expected = Map::new();
    for entry in &checkpoint.files_read {
        let Some(object) = entry.as_object() else {
            continue;
        };
        let Some(path) = object
            .get("path")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(identity) = object
            .get("content_sha256")
            .and_then(Value::as_str)
            .filter(|value| {
                value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        else {
            continue;
        };
        expected.insert(path.to_string(), Value::String(identity.to_string()));
    }
    expected
}

fn ensure_only_keys(object: &Map<String, Value>, allowed: &[&str]) -> Result<(), FacadeError> {
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid_argument());
    }
    Ok(())
}

fn required_string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, FacadeError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_argument)
}

fn required_workflow_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, FacadeError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_argument)
}

fn optional_bool(
    object: &Map<String, Value>,
    key: &str,
    default: bool,
) -> Result<bool, FacadeError> {
    match object.get(key) {
        None => Ok(default),
        Some(value) => value.as_bool().ok_or_else(invalid_argument),
    }
}

fn optional_u64(object: &Map<String, Value>, key: &str, default: u64) -> Result<u64, FacadeError> {
    match object.get(key) {
        None => Ok(default),
        Some(value) => value.as_u64().ok_or_else(invalid_argument),
    }
}

fn optional_u64_value(object: &Map<String, Value>, key: &str) -> Result<Option<u64>, FacadeError> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => value.as_u64().map(Some).ok_or_else(invalid_argument),
    }
}

fn optional_usize(
    object: &Map<String, Value>,
    key: &str,
    default: usize,
) -> Result<usize, FacadeError> {
    usize::try_from(optional_u64(object, key, default as u64)?).map_err(|_| invalid_argument())
}

fn optional_u32(object: &Map<String, Value>, key: &str, default: u32) -> Result<u32, FacadeError> {
    u32::try_from(optional_u64(object, key, u64::from(default))?).map_err(|_| invalid_argument())
}

fn optional_choice(
    object: &Map<String, Value>,
    key: &str,
    allowed: &[&str],
) -> Result<Option<String>, FacadeError> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    let value = value.as_str().ok_or_else(invalid_argument)?;
    allowed
        .contains(&value)
        .then(|| value.to_string())
        .ok_or_else(invalid_argument)
        .map(Some)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn typed_expected_files(
    values: &Map<String, Value>,
) -> Result<BTreeMap<String, String>, FacadeError> {
    values
        .iter()
        .map(|(path, hash)| {
            let hash = hash.as_str().filter(|value| valid_sha256(value))?;
            Some((path.clone(), hash.to_string()))
        })
        .collect::<Option<BTreeMap<_, _>>>()
        .ok_or_else(invalid_argument)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilesystemAction {
    List,
    Stat,
    Read,
    Write,
    Replace,
    Patch,
    Search,
    SearchContent,
    Copy,
    Move,
    Delete,
    Hash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilesystemRequest {
    pub(crate) action: FilesystemAction,
    pub(crate) path: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) destination: Option<String>,
    pub(crate) recursive: bool,
    pub(crate) max_depth: u32,
    pub(crate) max_entries: usize,
    pub(crate) max_results: usize,
    pub(crate) offset: u64,
    pub(crate) max_bytes: usize,
    pub(crate) read_encoding: Option<String>,
    pub(crate) content: Option<Vec<u8>>,
    pub(crate) expected_sha256: Option<String>,
    pub(crate) old: Option<String>,
    pub(crate) new: Option<String>,
    pub(crate) patch: Option<String>,
    pub(crate) expected_files: Option<Map<String, Value>>,
    pub(crate) pattern: Option<String>,
    pub(crate) case_sensitive: bool,
    pub(crate) max_file_bytes: usize,
    pub(crate) kind: Option<String>,
    pub(crate) min_size: Option<u64>,
    pub(crate) max_size: Option<u64>,
    pub(crate) modified_after_ms: Option<u64>,
    pub(crate) modified_before_ms: Option<u64>,
    pub(crate) sort_by: String,
    pub(crate) sort_order: String,
    pub(crate) overwrite: bool,
    pub(crate) calculate_size: bool,
}

pub(crate) fn parse_filesystem_request(
    arguments: &Value,
) -> Result<FilesystemRequest, FacadeError> {
    let object = object_args(arguments)?;
    let action = required_string(object, "action")?;
    let mut request = FilesystemRequest {
        action: FilesystemAction::List,
        path: None,
        source: None,
        destination: None,
        recursive: false,
        max_depth: 16,
        max_entries: 10_000,
        max_results: 1_000,
        offset: 0,
        max_bytes: 65_536,
        read_encoding: None,
        content: None,
        expected_sha256: None,
        old: None,
        new: None,
        patch: None,
        expected_files: None,
        pattern: None,
        case_sensitive: true,
        max_file_bytes: 1024 * 1024,
        kind: None,
        min_size: None,
        max_size: None,
        modified_after_ms: None,
        modified_before_ms: None,
        sort_by: "path".into(),
        sort_order: "asc".into(),
        overwrite: false,
        calculate_size: false,
    };
    match action {
        "list" => {
            ensure_only_keys(
                object,
                &[
                    "action",
                    "path",
                    "recursive",
                    "max_depth",
                    "max_entries",
                    "sort_by",
                    "sort_order",
                ],
            )?;
            request.action = FilesystemAction::List;
            request.path = Some(required_string(object, "path")?.to_string());
            request.recursive = optional_bool(object, "recursive", false)?;
            request.max_depth = optional_u32(object, "max_depth", 16)?;
            request.max_entries = optional_usize(object, "max_entries", 1_000)?;
            request.sort_by = optional_choice(object, "sort_by", &["path", "size", "modified"])?
                .unwrap_or_else(|| "path".into());
            request.sort_order = optional_choice(object, "sort_order", &["asc", "desc"])?
                .unwrap_or_else(|| "asc".into());
        }
        "stat" => {
            ensure_only_keys(
                object,
                &[
                    "action",
                    "path",
                    "calculate_size",
                    "max_depth",
                    "max_entries",
                ],
            )?;
            request.action = FilesystemAction::Stat;
            request.path = Some(required_string(object, "path")?.to_string());
            request.calculate_size = optional_bool(object, "calculate_size", false)?;
            request.max_depth = optional_u32(object, "max_depth", 16)?;
            request.max_entries = optional_usize(object, "max_entries", 10_000)?;
        }
        "read" => {
            ensure_only_keys(
                object,
                &["action", "path", "offset", "max_bytes", "encoding"],
            )?;
            request.action = FilesystemAction::Read;
            request.path = Some(required_string(object, "path")?.to_string());
            request.offset = optional_u64(object, "offset", 0)?;
            request.max_bytes = optional_usize(object, "max_bytes", 65_536)?;
            request.read_encoding = optional_choice(object, "encoding", &["utf8", "base64"])?;
        }
        "write" => {
            ensure_only_keys(
                object,
                &["action", "path", "content", "encoding", "overwrite"],
            )?;
            request.action = FilesystemAction::Write;
            request.path = Some(required_string(object, "path")?.to_string());
            let content = required_string(object, "content")?;
            let encoding = optional_choice(object, "encoding", &["utf8", "base64"])?
                .unwrap_or_else(|| "utf8".into());
            request.content = Some(if encoding == "base64" {
                STANDARD.decode(content).map_err(|_| invalid_argument())?
            } else {
                content.as_bytes().to_vec()
            });
            request.overwrite = optional_bool(object, "overwrite", false)?;
        }
        "replace" => {
            ensure_only_keys(object, &["action", "path", "expected_sha256", "old", "new"])?;
            request.action = FilesystemAction::Replace;
            request.path = Some(required_string(object, "path")?.to_string());
            let expected_sha256 = required_string(object, "expected_sha256")?;
            if !valid_sha256(expected_sha256) {
                return Err(invalid_argument());
            }
            request.expected_sha256 = Some(expected_sha256.to_string());
            let old = required_string(object, "old")?;
            if old.is_empty() {
                return Err(invalid_argument());
            }
            request.old = Some(old.to_string());
            request.new = Some(required_string(object, "new")?.to_string());
        }
        "patch" => {
            ensure_only_keys(object, &["action", "patch", "expected_files"])?;
            request.action = FilesystemAction::Patch;
            let patch = required_string(object, "patch")?;
            if patch.is_empty() || !public_patch_targets_valid(patch) {
                return Err(invalid_argument());
            }
            request.patch = Some(patch.to_string());
            request.expected_files = object
                .get("expected_files")
                .map(|value| {
                    let values = value.as_object().ok_or_else(invalid_argument)?;
                    if values.iter().any(|(path, hash)| {
                        !workspace_relative_path_valid(path)
                            || !hash.as_str().is_some_and(valid_sha256)
                    }) {
                        return Err(invalid_argument());
                    }
                    Ok(values.clone())
                })
                .transpose()?;
        }
        "search" => {
            ensure_only_keys(
                object,
                &[
                    "action",
                    "path",
                    "pattern",
                    "recursive",
                    "max_depth",
                    "max_entries",
                    "max_results",
                    "type",
                    "min_size",
                    "max_size",
                    "modified_after",
                    "modified_before",
                    "sort_by",
                    "sort_order",
                ],
            )?;
            request.action = FilesystemAction::Search;
            request.path = Some(required_string(object, "path")?.to_string());
            request.pattern = Some(required_string(object, "pattern")?.to_string());
            request.recursive = optional_bool(object, "recursive", false)?;
            request.max_depth = optional_u32(object, "max_depth", 16)?;
            request.max_entries = optional_usize(object, "max_entries", 10_000)?;
            request.max_results = optional_usize(object, "max_results", 1_000)?;
            request.kind = optional_choice(object, "type", &["file", "directory"])?;
            request.min_size = optional_u64_value(object, "min_size")?;
            request.max_size = optional_u64_value(object, "max_size")?;
            request.modified_after_ms = optional_u64_value(object, "modified_after")?;
            request.modified_before_ms = optional_u64_value(object, "modified_before")?;
            request.sort_by = optional_choice(object, "sort_by", &["path", "size", "modified"])?
                .unwrap_or_else(|| "path".into());
            request.sort_order = optional_choice(object, "sort_order", &["asc", "desc"])?
                .unwrap_or_else(|| "asc".into());
        }
        "search_content" => {
            ensure_only_keys(
                object,
                &[
                    "action",
                    "path",
                    "pattern",
                    "recursive",
                    "max_depth",
                    "max_entries",
                    "max_results",
                    "case_sensitive",
                    "max_file_bytes",
                ],
            )?;
            request.action = FilesystemAction::SearchContent;
            request.path = Some(required_string(object, "path")?.to_string());
            request.pattern = Some(required_string(object, "pattern")?.to_string());
            request.recursive = optional_bool(object, "recursive", false)?;
            request.max_depth = optional_u32(object, "max_depth", 16)?;
            request.max_entries = optional_usize(object, "max_entries", 10_000)?;
            request.max_results = optional_usize(object, "max_results", 100)?;
            request.case_sensitive = optional_bool(object, "case_sensitive", true)?;
            request.max_file_bytes = optional_usize(object, "max_file_bytes", 1024 * 1024)?;
        }
        "copy" | "move" => {
            ensure_only_keys(
                object,
                &[
                    "action",
                    "source",
                    "destination",
                    "recursive",
                    "overwrite",
                    "max_depth",
                    "max_entries",
                ],
            )?;
            request.action = if action == "copy" {
                FilesystemAction::Copy
            } else {
                FilesystemAction::Move
            };
            request.source = Some(required_string(object, "source")?.to_string());
            request.destination = Some(required_string(object, "destination")?.to_string());
            request.recursive = optional_bool(object, "recursive", false)?;
            request.overwrite = optional_bool(object, "overwrite", false)?;
            request.max_depth = optional_u32(object, "max_depth", 16)?;
            request.max_entries = optional_usize(object, "max_entries", 10_000)?;
        }
        "delete" => {
            ensure_only_keys(
                object,
                &["action", "path", "recursive", "max_depth", "max_entries"],
            )?;
            request.action = FilesystemAction::Delete;
            request.path = Some(required_string(object, "path")?.to_string());
            request.recursive = optional_bool(object, "recursive", false)?;
            request.max_depth = optional_u32(object, "max_depth", 16)?;
            request.max_entries = optional_usize(object, "max_entries", 10_000)?;
        }
        "hash" => {
            ensure_only_keys(object, &["action", "path"])?;
            request.action = FilesystemAction::Hash;
            request.path = Some(required_string(object, "path")?.to_string());
        }
        _ => return Err(invalid_argument()),
    }
    if request
        .min_size
        .zip(request.max_size)
        .is_some_and(|(min, max)| min > max)
        || request
            .modified_after_ms
            .zip(request.modified_before_ms)
            .is_some_and(|(min, max)| min > max)
    {
        return Err(invalid_argument());
    }
    Ok(request)
}

#[cfg(test)]
fn run_workspace_filesystem(workspace: &Path, arguments: Value) -> Result<Value, FacadeError> {
    let authority = crate::workspace::WorkspaceResolver::active_workspace(workspace)
        .map_err(normalize_path_authority_error)?;
    run_workspace_filesystem_with_authority(authority, arguments, FilesystemCancellation::default())
}

pub(crate) fn run_workspace_filesystem_with_authority(
    authority: WorkspaceResolver,
    arguments: Value,
    cancellation: FilesystemCancellation,
) -> Result<Value, FacadeError> {
    let request = parse_filesystem_request(&arguments)?;
    let edit_authority = authority.clone();
    let service = FilesystemService::from_authority(authority)
        .map_err(normalize_filesystem_error)?
        .with_cancellation(cancellation);
    let data = match request.action {
        FilesystemAction::List => {
            let mut result = service
                .list(
                    request.path.as_deref().expect("list path parsed"),
                    request.recursive,
                    request.max_depth,
                    request.max_entries,
                )
                .map_err(normalize_filesystem_error)?;
            sort_filesystem_entries(&mut result.entries, &request.sort_by, &request.sort_order);
            serde_json::to_value(result).map_err(|_| command_state_internal_error())?
        }
        FilesystemAction::Stat => {
            let result = service
                .stat(
                    request.path.as_deref().expect("stat path parsed"),
                    request.calculate_size,
                    request.max_depth,
                    request.max_entries,
                )
                .map_err(normalize_filesystem_error)?;
            serde_json::to_value(result).map_err(|_| command_state_internal_error())?
        }
        FilesystemAction::Read => {
            let mut result = service
                .read(
                    request.path.as_deref().expect("read path parsed"),
                    request.offset,
                    request.max_bytes,
                )
                .map_err(normalize_filesystem_error)?;
            match request.read_encoding.as_deref() {
                Some("base64") if result.encoding == "utf8" => {
                    result.content = STANDARD.encode(result.content.as_bytes());
                    result.encoding = "base64";
                }
                Some("utf8") if result.encoding == "base64" => {
                    return Err(FacadeError::new(
                        FacadeErrorCode::InvalidArgument,
                        "requested utf8 encoding for non-UTF-8 file content",
                        false,
                    ));
                }
                _ => {}
            }
            serde_json::to_value(result).map_err(|_| command_state_internal_error())?
        }
        FilesystemAction::Write => {
            let result = service
                .write(
                    request.path.as_deref().expect("write path parsed"),
                    request.content.as_deref().expect("write content parsed"),
                    request.overwrite,
                )
                .map_err(normalize_filesystem_error)?;
            serde_json::to_value(result).map_err(|_| command_state_internal_error())?
        }
        FilesystemAction::Replace => {
            let path = request.path.as_deref().expect("replace path parsed");
            let sha256 = CodingEditService::with_authority(edit_authority)
                .map_err(normalize_coding_edit_error)?
                .replace_exact(
                    path,
                    request
                        .expected_sha256
                        .as_deref()
                        .expect("replace identity parsed"),
                    request.old.as_deref().expect("replace old text parsed"),
                    request.new.as_deref().expect("replace new text parsed"),
                )
                .map_err(normalize_coding_edit_error)?;
            json!({"path":path,"changed":true,"sha256":sha256})
        }
        FilesystemAction::Patch => {
            let edit = CodingEditService::with_authority(edit_authority)
                .map_err(normalize_coding_edit_error)?;
            let patch = request.patch.as_deref().expect("patch parsed");
            let affected_files = match request.expected_files.as_ref() {
                Some(expected) => {
                    let expected = typed_expected_files(expected)?;
                    edit.apply_patch(patch, &expected)
                }
                None => edit.apply_patch_to_current(patch),
            }
            .map_err(normalize_coding_edit_error)?;
            json!({"affected_files":affected_files,"changed":true})
        }
        FilesystemAction::Search => {
            let options = FilesystemSearchOptions {
                recursive: request.recursive,
                max_depth: request.max_depth,
                max_entries: request.max_entries,
                max_results: request.max_results,
                pattern: request.pattern.clone().expect("search pattern parsed"),
                kind: request.kind.clone(),
                min_size: request.min_size,
                max_size: request.max_size,
                modified_after_ms: request.modified_after_ms,
                modified_before_ms: request.modified_before_ms,
                sort_by: request.sort_by.clone(),
                sort_order: request.sort_order.clone(),
            };
            let result = service
                .search(
                    request.path.as_deref().expect("search path parsed"),
                    &options,
                )
                .map_err(normalize_filesystem_error)?;
            serde_json::to_value(result).map_err(|_| command_state_internal_error())?
        }
        FilesystemAction::SearchContent => {
            let options = FilesystemContentSearchOptions {
                recursive: request.recursive,
                max_depth: request.max_depth,
                max_entries: request.max_entries,
                max_results: request.max_results,
                max_file_bytes: request.max_file_bytes,
                pattern: request
                    .pattern
                    .clone()
                    .expect("content search pattern parsed"),
                case_sensitive: request.case_sensitive,
            };
            let result = service
                .search_content(
                    request.path.as_deref().expect("content search path parsed"),
                    &options,
                )
                .map_err(normalize_filesystem_error)?;
            serde_json::to_value(result).map_err(|_| command_state_internal_error())?
        }
        FilesystemAction::Copy | FilesystemAction::Move => {
            let source = request.source.as_deref().expect("copy/move source parsed");
            let destination = request
                .destination
                .as_deref()
                .expect("copy/move destination parsed");
            let result = if request.action == FilesystemAction::Copy {
                service.copy(
                    source,
                    destination,
                    request.recursive,
                    request.overwrite,
                    request.max_depth,
                    request.max_entries,
                )
            } else {
                service.move_path(
                    source,
                    destination,
                    request.recursive,
                    request.overwrite,
                    request.max_depth,
                    request.max_entries,
                )
            }
            .map_err(normalize_filesystem_error)?;
            serde_json::to_value(result).map_err(|_| command_state_internal_error())?
        }
        FilesystemAction::Delete => {
            let result = service
                .delete(
                    request.path.as_deref().expect("delete path parsed"),
                    request.recursive,
                    request.max_depth,
                    request.max_entries,
                )
                .map_err(normalize_filesystem_error)?;
            serde_json::to_value(result).map_err(|_| command_state_internal_error())?
        }
        FilesystemAction::Hash => {
            let result = service
                .hash(request.path.as_deref().expect("hash path parsed"))
                .map_err(normalize_filesystem_error)?;
            serde_json::to_value(result).map_err(|_| command_state_internal_error())?
        }
    };
    Ok(stable_success(data, "Filesystem operation completed"))
}

fn sort_filesystem_entries(
    entries: &mut [crate::filesystem::service::FilesystemEntry],
    sort_by: &str,
    sort_order: &str,
) {
    match sort_by {
        "size" => entries.sort_by_key(|entry| (entry.size, entry.path.clone())),
        "modified" => {
            entries.sort_by_key(|entry| (entry.modified_ms.unwrap_or(0), entry.path.clone()))
        }
        _ => entries.sort_by(|left, right| left.path.cmp(&right.path)),
    }
    if sort_order == "desc" {
        entries.reverse();
    }
}

#[cfg(test)]
mod schema43_filesystem_facade_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "localbridge-schema43-facade-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn schema43_filesystem_is_flat_nine_core_contract() {
        assert_eq!(AGENT_API_REVISION, 50);
        assert_eq!(V1_CORE_TOOL_NAMES.len(), 9);
        assert_eq!(V1_CORE_TOOL_NAMES[2], "filesystem");
        let schema = public_tool_schema("filesystem");
        let input = &schema["inputSchema"];
        assert_eq!(input["type"], "object");
        assert!(input.get("oneOf").is_none());
        assert_eq!(input["additionalProperties"], false);
        assert_eq!(input["required"], json!(["action"]));
        assert_eq!(
            input["properties"]["action"]["enum"],
            json!([
                "list",
                "stat",
                "read",
                "write",
                "replace",
                "patch",
                "search",
                "search_content",
                "copy",
                "move",
                "delete",
                "hash"
            ])
        );
        for property in [
            "action",
            "path",
            "source",
            "destination",
            "recursive",
            "max_depth",
            "max_entries",
            "max_results",
            "offset",
            "max_bytes",
            "content",
            "encoding",
            "expected_sha256",
            "old",
            "new",
            "patch",
            "expected_files",
            "pattern",
            "case_sensitive",
            "max_file_bytes",
            "type",
            "min_size",
            "max_size",
            "modified_after",
            "modified_before",
            "sort_by",
            "sort_order",
            "overwrite",
            "calculate_size",
        ] {
            assert!(
                input["properties"].get(property).is_some(),
                "missing {property}"
            );
        }
    }

    #[test]
    fn schema43_workspace_filesystem_executes_and_fails_closed() {
        let root = temp_workspace("io");
        let outside = temp_workspace("outside");
        std::fs::write(outside.join("outside.txt"), b"outside").unwrap();

        let write = run_workspace_filesystem(
            &root,
            json!({"action":"write","path":"note.txt","content":"hello"}),
        )
        .unwrap();
        assert_eq!(write["isError"], false);
        assert_eq!(std::fs::read(root.join("note.txt")).unwrap(), b"hello");

        let read = run_workspace_filesystem(
            &root,
            json!({"action":"read","path":"note.txt","max_bytes":3}),
        )
        .unwrap();
        assert_eq!(read["structuredContent"]["data"]["content"], "hel");
        assert_eq!(read["structuredContent"]["data"]["eof"], false);
        let read_base64 = run_workspace_filesystem(
            &root,
            json!({"action":"read","path":"note.txt","encoding":"base64"}),
        )
        .unwrap();
        assert_eq!(
            read_base64["structuredContent"]["data"]["encoding"],
            "base64"
        );
        assert_eq!(
            STANDARD
                .decode(
                    read_base64["structuredContent"]["data"]["content"]
                        .as_str()
                        .unwrap()
                )
                .unwrap(),
            b"hello"
        );

        std::fs::write(root.join("larger.bin"), b"123456789").unwrap();
        let sorted = run_workspace_filesystem(
            &root,
            json!({"action":"list","path":".","sort_by":"size","sort_order":"desc"}),
        )
        .unwrap();
        assert_eq!(
            sorted["structuredContent"]["data"]["entries"][0]["path"],
            "larger.bin"
        );

        let search = run_workspace_filesystem(
            &root,
            json!({"action":"search","path":".","pattern":"*.txt"}),
        )
        .unwrap();
        assert_eq!(
            search["structuredContent"]["data"]["entries"][0]["path"],
            "note.txt"
        );
        std::fs::create_dir(root.join("deep")).unwrap();
        std::fs::write(root.join("deep/nested.txt"), b"nested").unwrap();
        let non_recursive = run_workspace_filesystem(
            &root,
            json!({"action":"search","path":".","pattern":"*.txt"}),
        )
        .unwrap();
        assert_eq!(
            non_recursive["structuredContent"]["data"]["entries"],
            json!([{
                "path":"note.txt",
                "kind":"file",
                "size":5,
                "modified_ms":non_recursive["structuredContent"]["data"]["entries"][0]["modified_ms"].clone()
            }])
        );
        let recursive = run_workspace_filesystem(
            &root,
            json!({"action":"search","path":".","pattern":"*.txt","recursive":true}),
        )
        .unwrap();
        assert_eq!(
            recursive["structuredContent"]["data"]["entries"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );

        std::fs::write(root.join("edit.txt"), b"before\r\ncontext\r\n").unwrap();
        let edit_hash = run_workspace_filesystem(&root, json!({"action":"hash","path":"edit.txt"}))
            .unwrap()["structuredContent"]["data"]["sha256"]
            .clone();
        let replaced = run_workspace_filesystem(
            &root,
            json!({
                "action":"replace",
                "path":"edit.txt",
                "expected_sha256":edit_hash,
                "old":"before",
                "new":"after"
            }),
        )
        .unwrap();
        assert_eq!(replaced["structuredContent"]["data"]["changed"], true);
        assert_eq!(
            std::fs::read(root.join("edit.txt")).unwrap(),
            b"after\r\ncontext\r\n"
        );
        let replaced_hash = replaced["structuredContent"]["data"]["sha256"].clone();
        let rejected = run_workspace_filesystem(
            &root,
            json!({
                "action":"patch",
                "patch":"*** Begin Patch\n*** Update File: edit.txt\n@@\n-after\n+final\n context\n*** Add File: added.txt\n+final added\n*** End Patch",
                "expected_files":{"edit.txt":replaced_hash}
            }),
        )
        .unwrap_err();
        assert_eq!(rejected.code, FacadeErrorCode::InvalidArgument);
        assert_eq!(
            std::fs::read(root.join("edit.txt")).unwrap(),
            b"after\r\ncontext\r\n"
        );
        assert!(!root.join("added.txt").exists());
        let patched = run_workspace_filesystem(
            &root,
            json!({
                "action":"patch",
                "patch":"*** Begin Patch\n*** Update File: edit.txt\n@@\n-after\n+final\n context\n*** End Patch",
                "expected_files":{"edit.txt":replaced_hash}
            }),
        )
        .unwrap();
        assert_eq!(
            patched["structuredContent"]["data"]["affected_files"],
            json!(["edit.txt"])
        );
        run_workspace_filesystem(
            &root,
            json!({
                "action":"patch",
                "patch":"*** Begin Patch\n*** Add File: added.txt\n+final added\n*** End Patch"
            }),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(root.join("edit.txt")).unwrap(),
            b"final\r\ncontext\r\n"
        );
        let content_search = run_workspace_filesystem(
            &root,
            json!({
                "action":"search_content",
                "path":".",
                "pattern":"FINAL",
                "case_sensitive":false,
                "recursive":true
            }),
        )
        .unwrap();
        assert_eq!(
            content_search["structuredContent"]["data"]["matches"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );

        let hash =
            run_workspace_filesystem(&root, json!({"action":"hash","path":"note.txt"})).unwrap();
        assert_eq!(hash["structuredContent"]["data"]["algorithm"], "sha256");

        std::fs::write(root.join("format-source.txt"), b"format").unwrap();
        let absolute_read = run_workspace_filesystem(
            &root,
            json!({"action":"read","path":root.join("format-source.txt").to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(
            absolute_read["structuredContent"]["data"]["path"],
            "format-source.txt"
        );
        let moved = run_workspace_filesystem(
            &root,
            json!({
                "action":"move",
                "source":root.join("format-source.txt").to_string_lossy(),
                "destination":"format-destination.txt"
            }),
        )
        .unwrap();
        assert_eq!(
            moved["structuredContent"]["data"]["path"],
            "format-source.txt"
        );
        assert_eq!(
            moved["structuredContent"]["data"]["destination"],
            "format-destination.txt"
        );
        let deleted = run_workspace_filesystem(
            &root,
            json!({
                "action":"delete",
                "path":root.join("format-destination.txt").to_string_lossy()
            }),
        )
        .unwrap();
        assert_eq!(
            deleted["structuredContent"]["data"]["path"],
            "format-destination.txt"
        );

        assert_eq!(
            run_workspace_filesystem(
                &root,
                json!({"action":"hash","path":"note.txt","recursive":true})
            )
            .unwrap_err()
            .code,
            FacadeErrorCode::InvalidArgument
        );
        assert_eq!(
            run_workspace_filesystem(&root, json!({"action":"write","path":"missing.txt"}))
                .unwrap_err()
                .code,
            FacadeErrorCode::InvalidArgument
        );
        assert_eq!(
            run_workspace_filesystem(
                &root,
                json!({"action":"read","path":outside.join("outside.txt").to_string_lossy()}),
            )
            .unwrap_err()
            .code,
            FacadeErrorCode::WorkspaceDenied
        );

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
}

fn invalid_argument() -> FacadeError {
    FacadeError::new(
        FacadeErrorCode::InvalidArgument,
        "LocalBridge 工具参数无效",
        false,
    )
}

fn normalize_filesystem_error(error: FilesystemError) -> FacadeError {
    match error {
        FilesystemError::InvalidArgument
        | FilesystemError::LimitExceeded
        | FilesystemError::Unsupported => invalid_argument(),
        FilesystemError::NotFound => {
            FacadeError::new(FacadeErrorCode::NotFound, "文件系统目标不存在", false)
        }
        FilesystemError::OutsideAuthority => FacadeError::new(
            FacadeErrorCode::WorkspaceDenied,
            "文件系统路径越出当前授权范围",
            false,
        ),
        FilesystemError::AlreadyExists => {
            FacadeError::new(FacadeErrorCode::FileChanged, "文件系统目标已存在", false)
        }
        FilesystemError::FileChanged => FacadeError::new(
            FacadeErrorCode::FileChanged,
            "文件系统目标自读取后已发生变化",
            false,
        ),
        FilesystemError::Cancelled => FacadeError::new(
            FacadeErrorCode::ProcessCancelled,
            "文件系统操作已取消",
            true,
        ),
        FilesystemError::Io => {
            FacadeError::new(FacadeErrorCode::Internal, "文件系统操作失败", false)
        }
    }
}

fn normalize_document_error(error: DocumentError) -> FacadeError {
    match error {
        DocumentError::InvalidArgument => invalid_argument(),
        DocumentError::NotFound => {
            FacadeError::new(FacadeErrorCode::NotFound, "文档不存在", false)
        }
        DocumentError::OutsideAuthority => FacadeError::new(
            FacadeErrorCode::WorkspaceDenied,
            "文档路径越出当前工作区",
            false,
        ),
        DocumentError::FileChanged => FacadeError::new(
            FacadeErrorCode::FileChanged,
            "文档自读取后已变化或目标已存在",
            false,
        ),
        DocumentError::LimitExceeded => FacadeError::new(
            FacadeErrorCode::OutputTruncated,
            "文档超过本地处理上限",
            false,
        ),
        DocumentError::UnsupportedFormat => {
            FacadeError::new(
                FacadeErrorCode::InvalidArgument,
                "文档格式或目标转换不受支持",
                false,
            )
            .with_details(json!({"field":"path","supported_formats":["txt","md","docx","pdf"],"pdf_mutation":false}))
        }
        DocumentError::UnsupportedContent => FacadeError::new(
            FacadeErrorCode::CapabilityUnavailable,
            "文档包含当前版本无法无损处理的结构",
            false,
        ),
        DocumentError::CorruptDocument => FacadeError::new(
            FacadeErrorCode::InvalidArgument,
            "文档格式损坏或无法解析",
            false,
        ),
        DocumentError::Io => {
            FacadeError::new(FacadeErrorCode::Internal, "文档操作失败", false)
        }
    }
}

fn normalize_shell_error(_error: ShellResolveError, selector: ShellSelector) -> FacadeError {
    let message = match selector {
        ShellSelector::Pwsh => {
            "未发现可信 PowerShell Core；可查看 workspace_context.shell_discovery 并使用 windows_powershell 或 auto"
        }
        ShellSelector::WindowsPowershell => {
            "未发现可信 Windows PowerShell；请查看 workspace_context.shell_discovery"
        }
        ShellSelector::Cmd => "未发现可信 cmd.exe；请查看 workspace_context.shell_discovery",
        ShellSelector::Powershell | ShellSelector::Auto => {
            "没有可用的可信命令 Shell；请查看 workspace_context.shell_discovery"
        }
    };
    FacadeError::new(FacadeErrorCode::RuntimeUnavailable, message, false)
}

fn normalize_runtime_error(error: CodingToolsRuntimeError) -> FacadeError {
    match error {
        CodingToolsRuntimeError::ProtocolMismatch => FacadeError::new(
            FacadeErrorCode::RuntimeProtocolMismatch,
            "编码运行时协议不兼容",
            false,
        )
        .with_diagnostic(ErrorDiagnostic::new(
            DiagnosticErrorCode::Unavailable,
            DiagnosticPhase::Mcp,
            "protocol_mismatch",
        )),
        CodingToolsRuntimeError::ConnectionUnavailable => FacadeError::new(
            FacadeErrorCode::SessionUnavailable,
            "编码运行时连接不可用",
            true,
        )
        .with_diagnostic(transport_unavailable("connection_unavailable", None)),
        CodingToolsRuntimeError::RequestTimeout => FacadeError::new(
            FacadeErrorCode::OperationTimedOut,
            "命令控制请求已达到 wait_ms 时间预算",
            true,
        )
        .with_diagnostic(ErrorDiagnostic::new(
            DiagnosticErrorCode::Timeout,
            DiagnosticPhase::Transport,
            "request_deadline_expired",
        )),
        CodingToolsRuntimeError::HttpStatus(status) => FacadeError::new(
            FacadeErrorCode::SessionUnavailable,
            "编码运行时传输返回异常 HTTP 状态",
            true,
        )
        .with_diagnostic(transport_unavailable(
            format!("http_{status}"),
            Some(status),
        )),
        CodingToolsRuntimeError::Cancelled => {
            FacadeError::new(FacadeErrorCode::ProcessCancelled, "命令已取消", false)
        }
        CodingToolsRuntimeError::HealthTimeout => FacadeError::new(
            FacadeErrorCode::SessionUnavailable,
            "编码运行时 MCP 健康检查超时",
            true,
        )
        .with_diagnostic(transport_unavailable("health_timeout", None)),
        CodingToolsRuntimeError::UpstreamRpcError => {
            FacadeError::new(FacadeErrorCode::Internal, "编码运行时 MCP 调用失败", false)
                .with_diagnostic(ErrorDiagnostic::new(
                    DiagnosticErrorCode::ExecutionFailed,
                    DiagnosticPhase::Mcp,
                    "upstream_rpc_error",
                ))
        }
        _ => FacadeError::new(
            FacadeErrorCode::RuntimeUnavailable,
            "编码运行时不可用",
            true,
        ),
    }
}

fn runtime_fault_name(fault: &RuntimeFault) -> &'static str {
    match fault {
        RuntimeFault::WorkspaceMissing => "workspace_missing",
        RuntimeFault::WorkspaceInvalid => "workspace_invalid",
        RuntimeFault::RuntimeMissing => "runtime_missing",
        RuntimeFault::RuntimeChecksumMismatch => "runtime_checksum_mismatch",
        RuntimeFault::ProcessOwnershipFailed => "process_ownership_failed",
        RuntimeFault::McpSpawnFailed => "mcp_spawn_failed",
        RuntimeFault::McpHealthTimeout => "mcp_health_timeout",
        RuntimeFault::McpExited => "mcp_exited",
        RuntimeFault::PolicyBindFailed => "policy_bind_failed",
        RuntimeFault::PolicyInvalid => "policy_invalid",
        RuntimeFault::PolicyCapabilityUnknown => "policy_capability_unknown",
        RuntimeFault::TunnelIdMissing => "tunnel_id_missing",
        RuntimeFault::RuntimeKeyMissing => "runtime_key_missing",
        RuntimeFault::SecretStoreFailed => "secret_store_failed",
        RuntimeFault::SecretInjectionUnsupported => "secret_injection_unsupported",
        RuntimeFault::TunnelAuthFailed => "tunnel_auth_failed",
        RuntimeFault::TunnelSpawnFailed => "tunnel_spawn_failed",
        RuntimeFault::TunnelHealthTimeout => "tunnel_health_timeout",
        RuntimeFault::TunnelExited => "tunnel_exited",
        RuntimeFault::PortUnavailable => "port_unavailable",
        RuntimeFault::ConfigurationInvalid => "configuration_invalid",
        RuntimeFault::UserStopped => "user_stopped",
        RuntimeFault::Unknown => "unknown",
    }
}

fn normalize_coding_edit_error(error: CodingEditError) -> FacadeError {
    match error {
        CodingEditError::FileChanged => FacadeError::new(
            FacadeErrorCode::FileChanged,
            "目标文件自读取后已发生变化",
            false,
        ),
        CodingEditError::PatchConflict => FacadeError::new(
            FacadeErrorCode::PatchConflict,
            "编辑上下文与当前文件不匹配",
            false,
        ),
        CodingEditError::AmbiguousMatch => {
            FacadeError::new(FacadeErrorCode::AmbiguousMatch, "编辑匹配不唯一", false)
        }
        CodingEditError::MultiFilePatchUnsupported => FacadeError::new(
            FacadeErrorCode::InvalidArgument,
            "patch 每次只能修改一个文件；请将多文件修改拆成独立请求",
            false,
        ),
        CodingEditError::NotFound => {
            FacadeError::new(FacadeErrorCode::NotFound, "目标文件不存在", false)
        }
        CodingEditError::InvalidPath => FacadeError::new(
            FacadeErrorCode::WorkspaceDenied,
            "目标路径不在当前工作区权限范围内",
            false,
        ),
        CodingEditError::Io => FacadeError::new(FacadeErrorCode::Internal, "编辑操作未完成", true),
    }
}

pub(crate) fn normalize_path_authority_error(error: PathAuthorityError) -> FacadeError {
    match error {
        PathAuthorityError::InvalidPath | PathAuthorityError::OutsideAuthority => FacadeError::new(
            FacadeErrorCode::WorkspaceDenied,
            "工作区路径参数无效",
            false,
        ),
        PathAuthorityError::NotFound => {
            FacadeError::new(FacadeErrorCode::NotFound, "工作区文件不存在", false)
        }
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
    let public = if code.contains("PATCH_CONTEXT_AMBIGUOUS") {
        FacadeErrorCode::AmbiguousMatch
    } else if code.contains("PATCH_CONTEXT_NOT_FOUND") || code.contains("PATCH_HUNKS_OVERLAP") {
        FacadeErrorCode::PatchConflict
    } else if code == "PATCH_CONFLICT" {
        FacadeErrorCode::FileChanged
    } else if code.contains("SESSION_NOT_FOUND") || code.contains("SESSION_CLOSED") {
        FacadeErrorCode::SessionUnavailable
    } else if code.contains("SHELL_SYNTAX")
        || code.contains("INVALID_SHELL")
        || code.contains("PARSE_ERROR")
    {
        FacadeErrorCode::InvalidShellSyntax
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

fn normalize_git_error(raw: &Value) -> FacadeError {
    let error = raw
        .pointer("/structuredContent/error")
        .and_then(Value::as_object);
    let private_code = error
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("GIT_ERROR");
    let private_message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Git operation failed");
    let code = match private_code {
        "INVALID_ARGUMENT" => FacadeErrorCode::InvalidArgument,
        "NOT_FOUND" => FacadeErrorCode::NotFound,
        "OUTSIDE_WORKSPACE" => FacadeErrorCode::WorkspaceDenied,
        "GIT_TIMEOUT" => FacadeErrorCode::ProcessTimedOut,
        _ => FacadeErrorCode::ProcessFailed,
    };
    FacadeError::new(code, "Git 操作失败", false).with_details(json!({
        "git_error_code":private_code,
        "git_message":private_message
    }))
}

fn session_unavailable() -> FacadeError {
    FacadeError::new(FacadeErrorCode::SessionUnavailable, "命令会话不可用", false)
}

fn coding_checkpoint_result(checkpoint: &WorkflowCheckpoint, state: &str, text: &str) -> Value {
    stable_success(
        json!({
            "action":checkpoint.arguments.get("action").and_then(Value::as_str),
            "phase":checkpoint.current_step,
            "workflow_id":checkpoint.workflow_id,
            "task_id":checkpoint.workflow_id,
            "state":state,
            "summary":text,
            "next_step":checkpoint.next_step,
            "warnings":[],
            "output_refs":checkpoint.output_refs,
            "modified_files":checkpoint.modified_files,
            "test_results":checkpoint.test_results,
            "build_results":checkpoint.build_results,
            "failure":checkpoint.failure,
            "git_before":checkpoint.git_before,
            "git_after":checkpoint.git_after,
            "completed":checkpoint.completed
        }),
        text,
    )
}

fn new_workflow_adoption_token() -> AdoptionToken {
    AdoptionToken::new(crate::security::random_prefixed_id("lb-workflow-adopt-"))
}

fn hash_workflow_adoption_token(token: &AdoptionToken) -> AdoptionTokenHash {
    let digest = Sha256::digest(token.expose().as_bytes());
    AdoptionTokenHash::new(format!("{digest:x}"))
}

fn workflow_adoption_token_matches(
    checkpoint: &WorkflowCheckpoint,
    candidate: Option<&str>,
) -> bool {
    let Some(candidate) = candidate.filter(|value| !value.is_empty()) else {
        return false;
    };
    checkpoint.adoption_token_hash.as_ref()
        == Some(&hash_workflow_adoption_token(&AdoptionToken::new(
            candidate,
        )))
}

pub(crate) fn stable_success(data: Value, text: &str) -> Value {
    let state = data.get("state").cloned().unwrap_or(Value::Null);
    let summary = data
        .get("summary")
        .cloned()
        .unwrap_or_else(|| Value::String(text.to_string()));
    let task_id = data
        .get("task_id")
        .or_else(|| data.get("workflow_id"))
        .cloned()
        .unwrap_or(Value::Null);
    let warnings = data.get("warnings").cloned().unwrap_or_else(|| json!([]));
    let next_step = data.get("next_step").cloned().unwrap_or(Value::Null);
    // The common envelope is always a list; command data additionally names
    // each stream. Do not copy that stream map into the list contract.
    let output_refs = match data.get("output_refs") {
        Some(Value::Array(refs)) => refs.clone(),
        Some(Value::Object(streams)) => streams.values().cloned().collect(),
        _ => Vec::new(),
    };
    json!({
        "content":[{"type":"text","text":text}],
        "structuredContent":{
            "ok":true,
            "state":state,
            "summary":summary,
            "task_id":task_id,
            "warnings":warnings,
            "next_step":next_step,
            "output_refs":output_refs,
            "data":data,
            "error":Value::Null
        },
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

pub(crate) fn stable_command_error(
    code: FacadeErrorCode,
    message: &str,
    data: Map<String, Value>,
) -> Value {
    let diagnostic = from_canonical_code(code.as_str());
    json!({
        "content":[{"type":"text","text":message}],
        "structuredContent":{
            "ok":false,
            "state":"failed",
            "summary":message,
            "task_id":Value::Null,
            "warnings":[],
            "next_step":Value::Null,
            "output_refs":[],
            "error":{
                "code":code.as_str(),
                "error_code":diagnostic.error_code.as_str(),
                "phase":diagnostic.phase.as_str(),
                "cause":diagnostic.cause,
                "http_status":diagnostic.http_status,
                "message":message,"retryable":false,
                "rule_category":code.safe_rule_category(),"remediation":code.safe_remediation()
            },
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

pub(crate) fn public_command_stderr(stderr: &str) -> String {
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

fn trim_utf8_front(value: &mut String, max_bytes: usize) -> bool {
    if value.len() <= max_bytes {
        return false;
    }
    let mut start = value.len().saturating_sub(max_bytes);
    while !value.is_char_boundary(start) {
        start += 1;
    }
    value.drain(..start);
    true
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
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn schema42_unified_error_diagnostics_preserve_detail_and_map_transport() {
        let denied =
            FacadeError::new(FacadeErrorCode::PolicyDenied, "denied", false).to_mcp_result();
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

        let unknown =
            FacadeError::new(FacadeErrorCode::Internal, "internal", false).to_mcp_result();
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
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                port,
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new("LB012_TOOLBOX_SYNTHETIC_BEARER").unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled coding runtime for Toolbox acceptance");
        let executions = ExecutionRegistry::for_workspace(runtime.workspace()).unwrap();
        let mut facade =
            AgentFacade::from_coding_runtime_with_executions(runtime, policy(), executions)
                .unwrap();
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

        fn coding_context(
            &self,
            _project_path: &str,
            _objective: &str,
        ) -> Result<Value, FacadeError> {
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

        fn execute_document(
            &self,
            request: DocumentRequest,
        ) -> Result<DocumentResult, FacadeError> {
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

        fn coding_context(
            &self,
            _project_path: &str,
            _objective: &str,
        ) -> Result<Value, FacadeError> {
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

        fn apply_directory_change(
            &mut self,
            action: &str,
            path: &str,
        ) -> Result<Value, FacadeError> {
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
        let mut checkpoint = WorkflowCheckpoint::new_coding(
            "lb-kill".into(),
            json!({"action":"bugfix"}),
            "kill".into(),
        );
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

        let error =
            FacadeError::new(FacadeErrorCode::FileChanged, "changed", false).to_mcp_result();
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
            tool["description"].as_str().is_some_and(
                |value| value.contains("DocumentIR") && value.contains("expected_sha256")
            )
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
        let public_session =
            bind_test_session(&mut sessions, &task_state, "PRIVATE_SESSION_SECRET");
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
        let first =
            sessions.retain_local_output(McpSessionId::new("owner"), "stdout", "first".into());
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
        let output =
            sessions.public_output_for_private("expired-private-output", &public, "stdout");

        sessions.reap_expired_mappings(&executions);

        assert!(!sessions.sessions.contains_key(&public));
        assert!(sessions.private_output(&output).is_none());
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
        let normalized =
            CodingToolsRuntimeAdapter::normalize_read_output(&raw, "lb-output-schema36");
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
            let written = unsafe {
                GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32)
            };
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
}
