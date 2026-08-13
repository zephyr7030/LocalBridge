use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

use crate::privilege::ElevatedExecSpec;
use crate::state::{Capability, PermissionMode, TaskKind};

const PINNED_RUNTIME_VERSION: &str = "0.2.2";
const CONTROL_PLANE_NAMES: &[&str] = &[
    "request_permissions",
    "workspace_select",
    "workspace_add",
    "workspace_remove",
    "permission_mode_change",
    "credential_reset",
    "tunnel_config_write",
    "mcp_config_write",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub capability: Capability,
    pub task_kind: TaskKind,
}

pub const PINNED_TOOLS: &[ToolDescriptor] = &[
    ToolDescriptor {
        name: "server_info",
        capability: Capability::Read,
        task_kind: TaskKind::Other,
    },
    ToolDescriptor {
        name: "check_exec_environment",
        capability: Capability::Read,
        task_kind: TaskKind::Other,
    },
    ToolDescriptor {
        name: "get_default_cwd",
        capability: Capability::Read,
        task_kind: TaskKind::Other,
    },
    ToolDescriptor {
        name: "set_default_cwd",
        capability: Capability::Write,
        task_kind: TaskKind::Other,
    },
    ToolDescriptor {
        name: "read_file",
        capability: Capability::Read,
        task_kind: TaskKind::ReadFile,
    },
    ToolDescriptor {
        name: "list_dir",
        capability: Capability::Read,
        task_kind: TaskKind::ReadFile,
    },
    ToolDescriptor {
        name: "list_files",
        capability: Capability::Read,
        task_kind: TaskKind::ReadFile,
    },
    ToolDescriptor {
        name: "search_text",
        capability: Capability::Read,
        task_kind: TaskKind::SearchCode,
    },
    ToolDescriptor {
        name: "apply_patch",
        capability: Capability::Write,
        task_kind: TaskKind::ModifyFile,
    },
    ToolDescriptor {
        name: "exec_command",
        capability: Capability::ProcessExec,
        task_kind: TaskKind::ExecuteCommand,
    },
    ToolDescriptor {
        name: "write_stdin",
        capability: Capability::ProcessExec,
        task_kind: TaskKind::ExecuteCommand,
    },
    ToolDescriptor {
        name: "kill_session",
        capability: Capability::ProcessExec,
        task_kind: TaskKind::ExecuteCommand,
    },
    ToolDescriptor {
        name: "read_output",
        capability: Capability::ProcessExec,
        task_kind: TaskKind::ExecuteCommand,
    },
    ToolDescriptor {
        name: "git_status",
        capability: Capability::Git,
        task_kind: TaskKind::GitOperation,
    },
    ToolDescriptor {
        name: "git_diff",
        capability: Capability::Git,
        task_kind: TaskKind::GitOperation,
    },
    ToolDescriptor {
        name: "git_log",
        capability: Capability::Git,
        task_kind: TaskKind::GitOperation,
    },
    ToolDescriptor {
        name: "git_show",
        capability: Capability::Git,
        task_kind: TaskKind::GitOperation,
    },
    ToolDescriptor {
        name: "git_blame",
        capability: Capability::Git,
        task_kind: TaskKind::GitOperation,
    },
    ToolDescriptor {
        name: "request_permissions",
        capability: Capability::ControlPlane,
        task_kind: TaskKind::Other,
    },
    ToolDescriptor {
        name: "view_image",
        capability: Capability::Read,
        task_kind: TaskKind::ReadFile,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    ReadFailed,
    InvalidToml,
    ContractMismatch(&'static str),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadFailed => f.write_str("runtime policy could not be read"),
            Self::InvalidToml => f.write_str("runtime policy TOML is invalid"),
            Self::ContractMismatch(field) => write!(f, "runtime policy contract mismatch: {field}"),
        }
    }
}

impl std::error::Error for PolicyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    UnknownTool,
    ControlPlane,
    ToolNotAllowedInMode,
    IndirectProcessExecInEdit,
    IndirectControlPlane,
    IndirectUnknownCapability,
    PrivilegedRouteNotAvailable,
    ElevatedExecNotReviewed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDecision {
    pub descriptor: ToolDescriptor,
    pub allowed: bool,
    pub deny_reason: Option<DenyReason>,
}

#[derive(Debug, Clone)]
pub struct CapabilityPolicy {
    edit_allowed: HashSet<String>,
    full_allowed: HashSet<String>,
    elevated_allowed: HashSet<String>,
    blocked: HashSet<String>,
}

#[derive(Debug, Deserialize)]
struct PolicyDocument {
    schema_version: u32,
    runtime_version: String,
    status: String,
    edit_allowed_tools: Vec<String>,
    full_allowed_tools: Vec<String>,
    elevated_allowed_tools: Vec<String>,
    blocked_tools: Vec<String>,
    capabilities: CapabilitySection,
    enforcement: EnforcementSection,
    upstream_coding_tools: UpstreamSection,
    workspace_registry: WorkspaceSection,
    elevated_exec: ElevatedExecSection,
}

#[derive(Debug, Deserialize)]
struct CapabilitySection {
    unknown: String,
    process_exec_in_edit: String,
    process_exec_in_full: String,
    elevated_exec_in_edit: String,
    elevated_exec_in_full: String,
    elevated_exec_in_elevated: String,
    workflow_with_process_exec_in_edit: String,
    control_plane: String,
    privileged_external_runtime: String,
}

#[derive(Debug, Deserialize)]
struct ElevatedExecSection {
    enabled: bool,
    canonical_request: String,
    shell_true_default: bool,
    requires_broker: bool,
    requires_explicit_elevated_mode: bool,
    timeout_required: bool,
    output_limit_required: bool,
    cancellation_required: bool,
    redaction_required: bool,
    review_model: String,
    arbitrary_programs: String,
    shells_and_interpreters: String,
    control_plane_mutation: String,
    workdir_policy: String,
    reviewed_actions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EnforcementSection {
    tools_list_filter: String,
    tools_call_check: String,
    implementation: String,
    privileged_route: String,
    upstream_direct_tunnel_target: String,
    unknown_tool: String,
    request_permissions: String,
    transitive_exec_classification: String,
}

#[derive(Debug, Deserialize)]
struct UpstreamSection {
    permission_mode: String,
    dangerously_skip_all_permissions: bool,
    telemetry: String,
    listener: String,
    internal_auth: String,
    direct_external_exposure: bool,
}

#[derive(Debug, Deserialize)]
struct WorkspaceSection {
    remembered_projects_are_authorized_roots: bool,
    max_active_authorized_roots: u32,
    mcp_mutation: String,
    filesystem_delete_on_remove: String,
    auto_select_other_after_active_remove: bool,
}

impl CapabilityPolicy {
    pub fn load(path: &Path) -> Result<Self, PolicyError> {
        let text = fs::read_to_string(path).map_err(|_| PolicyError::ReadFailed)?;
        Self::from_toml(&text)
    }

    pub fn from_toml(text: &str) -> Result<Self, PolicyError> {
        let document: PolicyDocument =
            toml::from_str(text).map_err(|_| PolicyError::InvalidToml)?;
        validate_document(&document)?;
        Ok(Self {
            edit_allowed: document.edit_allowed_tools.into_iter().collect(),
            full_allowed: document.full_allowed_tools.into_iter().collect(),
            elevated_allowed: document.elevated_allowed_tools.into_iter().collect(),
            blocked: document.blocked_tools.into_iter().collect(),
        })
    }

    pub fn classify(&self, name: &str) -> ToolDescriptor {
        if name == "elevated_exec" {
            return ToolDescriptor {
                name: "elevated_exec",
                capability: Capability::ElevatedExec,
                task_kind: TaskKind::ElevatedOperation,
            };
        }
        if is_control_plane_name(name) {
            return ToolDescriptor {
                name: "control-plane",
                capability: Capability::ControlPlane,
                task_kind: TaskKind::Other,
            };
        }
        PINNED_TOOLS
            .iter()
            .copied()
            .find(|descriptor| descriptor.name == name)
            .unwrap_or(ToolDescriptor {
                name: "unknown",
                capability: Capability::Unknown,
                task_kind: TaskKind::Other,
            })
    }

    pub fn decide(
        &self,
        mode: PermissionMode,
        tool_name: &str,
        indirect_capabilities: &[Capability],
    ) -> PolicyDecision {
        let descriptor = self.classify(tool_name);
        if descriptor.capability == Capability::Unknown {
            return denied(descriptor, DenyReason::UnknownTool);
        }
        if descriptor.capability == Capability::ControlPlane || self.blocked.contains(tool_name) {
            return denied(descriptor, DenyReason::ControlPlane);
        }
        if indirect_capabilities.contains(&Capability::ControlPlane) {
            return denied(descriptor, DenyReason::IndirectControlPlane);
        }
        if indirect_capabilities.contains(&Capability::Unknown) {
            return denied(descriptor, DenyReason::IndirectUnknownCapability);
        }
        if indirect_capabilities.iter().any(|cap| {
            matches!(
                cap,
                Capability::ElevatedExec | Capability::PrivilegedExternalRuntime
            )
        }) {
            return denied(descriptor, DenyReason::PrivilegedRouteNotAvailable);
        }
        if mode == PermissionMode::Edit && indirect_capabilities.contains(&Capability::ProcessExec)
        {
            return denied(descriptor, DenyReason::IndirectProcessExecInEdit);
        }
        let allowed = match mode {
            PermissionMode::Edit => &self.edit_allowed,
            PermissionMode::Full => &self.full_allowed,
            PermissionMode::Elevated => &self.elevated_allowed,
        };
        if !allowed.contains(tool_name) {
            return denied(descriptor, DenyReason::ToolNotAllowedInMode);
        }
        PolicyDecision {
            descriptor,
            allowed: true,
            deny_reason: None,
        }
    }

    pub fn decide_request(
        &self,
        mode: PermissionMode,
        tool_name: &str,
        indirect_capabilities: &[Capability],
        arguments: &Value,
    ) -> PolicyDecision {
        let decision = self.decide(mode, tool_name, indirect_capabilities);
        if !decision.allowed || decision.descriptor.capability != Capability::ElevatedExec {
            return decision;
        }
        if !reviewed_elevated_exec(arguments) {
            return denied(decision.descriptor, DenyReason::ElevatedExecNotReviewed);
        }
        decision
    }

    pub fn tool_allowed_for_list(&self, mode: PermissionMode, tool_name: &str) -> bool {
        self.decide(mode, tool_name, &[]).allowed
    }

    pub fn privileged_tool_visible(&self, mode: PermissionMode, tool_name: &str) -> bool {
        mode == PermissionMode::Elevated
            && tool_name == "elevated_exec"
            && self.elevated_allowed.contains(tool_name)
            && self.classify(tool_name).capability == Capability::ElevatedExec
    }
}

fn denied(descriptor: ToolDescriptor, reason: DenyReason) -> PolicyDecision {
    PolicyDecision {
        descriptor,
        allowed: false,
        deny_reason: Some(reason),
    }
}

fn is_control_plane_name(name: &str) -> bool {
    CONTROL_PLANE_NAMES.contains(&name)
        || name.starts_with("localbridge.")
        || name.starts_with("localbridge_")
}

fn exact_set(actual: &[String], expected: &[&str]) -> bool {
    let actual = actual.iter().map(String::as_str).collect::<HashSet<_>>();
    let expected = expected.iter().copied().collect::<HashSet<_>>();
    actual.len() == expected.len() && actual == expected
}

fn validate_document(document: &PolicyDocument) -> Result<(), PolicyError> {
    if document.schema_version != 5
        || document.runtime_version != PINNED_RUNTIME_VERSION
        || document.status != "LB_000_VERIFIED"
    {
        return Err(PolicyError::ContractMismatch("identity"));
    }
    let edit = [
        "server_info",
        "check_exec_environment",
        "get_default_cwd",
        "set_default_cwd",
        "read_file",
        "list_dir",
        "list_files",
        "search_text",
        "apply_patch",
        "git_status",
        "git_diff",
        "git_log",
        "git_show",
        "git_blame",
        "view_image",
    ];
    let full = [
        "server_info",
        "check_exec_environment",
        "get_default_cwd",
        "set_default_cwd",
        "read_file",
        "list_dir",
        "list_files",
        "search_text",
        "apply_patch",
        "exec_command",
        "write_stdin",
        "kill_session",
        "read_output",
        "git_status",
        "git_diff",
        "git_log",
        "git_show",
        "git_blame",
        "view_image",
    ];
    let elevated = [
        "server_info",
        "check_exec_environment",
        "get_default_cwd",
        "set_default_cwd",
        "read_file",
        "list_dir",
        "list_files",
        "search_text",
        "apply_patch",
        "exec_command",
        "write_stdin",
        "kill_session",
        "read_output",
        "git_status",
        "git_diff",
        "git_log",
        "git_show",
        "git_blame",
        "view_image",
        "elevated_exec",
    ];
    if !exact_set(&document.edit_allowed_tools, &edit) {
        return Err(PolicyError::ContractMismatch("edit_allowed_tools"));
    }
    if !exact_set(&document.full_allowed_tools, &full) {
        return Err(PolicyError::ContractMismatch("full_allowed_tools"));
    }
    if !exact_set(&document.elevated_allowed_tools, &elevated) {
        return Err(PolicyError::ContractMismatch("elevated_allowed_tools"));
    }
    if !exact_set(&document.blocked_tools, &["request_permissions"]) {
        return Err(PolicyError::ContractMismatch("blocked_tools"));
    }
    if document.capabilities.unknown != "deny"
        || document.capabilities.process_exec_in_edit != "deny"
        || document.capabilities.process_exec_in_full != "allow_if_reviewed"
        || document.capabilities.elevated_exec_in_edit != "deny"
        || document.capabilities.elevated_exec_in_full != "deny"
        || document.capabilities.elevated_exec_in_elevated != "allow_if_reviewed_and_broker_active"
        || document.capabilities.workflow_with_process_exec_in_edit != "deny"
        || document.capabilities.control_plane != "deny_always"
        || document.capabilities.privileged_external_runtime != "review_required"
    {
        return Err(PolicyError::ContractMismatch("capabilities"));
    }
    if !document.elevated_exec.enabled
        || document.elevated_exec.canonical_request != "structured_program_args"
        || document.elevated_exec.shell_true_default
        || !document.elevated_exec.requires_broker
        || !document.elevated_exec.requires_explicit_elevated_mode
        || !document.elevated_exec.timeout_required
        || !document.elevated_exec.output_limit_required
        || !document.elevated_exec.cancellation_required
        || !document.elevated_exec.redaction_required
        || document.elevated_exec.review_model != "exact_trusted_program_and_args"
        || document.elevated_exec.arbitrary_programs != "deny"
        || document.elevated_exec.shells_and_interpreters != "deny"
        || document.elevated_exec.control_plane_mutation != "deny_always"
        || document.elevated_exec.workdir_policy != "deny_unless_reviewed"
        || document.elevated_exec.reviewed_actions != ["windows_whoami_identity"]
    {
        return Err(PolicyError::ContractMismatch("elevated_exec"));
    }
    if document.enforcement.tools_list_filter != "ux_only"
        || document.enforcement.tools_call_check != "mandatory"
        || document.enforcement.implementation != "first_party_rust_mcp_guard"
        || document.enforcement.privileged_route != "broker_only"
        || document.enforcement.upstream_direct_tunnel_target != "forbidden"
        || document.enforcement.unknown_tool != "deny"
        || document.enforcement.request_permissions != "deny_always"
        || document.enforcement.transitive_exec_classification != "required"
    {
        return Err(PolicyError::ContractMismatch("enforcement"));
    }
    if document.upstream_coding_tools.permission_mode != "trusted_behind_guard"
        || document
            .upstream_coding_tools
            .dangerously_skip_all_permissions
        || document.upstream_coding_tools.telemetry != "disabled"
        || document.upstream_coding_tools.listener != "loopback_ephemeral"
        || document.upstream_coding_tools.internal_auth != "runtime_generated_bearer_required"
        || document.upstream_coding_tools.direct_external_exposure
    {
        return Err(PolicyError::ContractMismatch("upstream_coding_tools"));
    }
    if document
        .workspace_registry
        .remembered_projects_are_authorized_roots
        || document.workspace_registry.max_active_authorized_roots != 1
        || document.workspace_registry.mcp_mutation != "deny_always"
        || document.workspace_registry.filesystem_delete_on_remove != "deny_always"
        || document
            .workspace_registry
            .auto_select_other_after_active_remove
    {
        return Err(PolicyError::ContractMismatch("workspace_registry"));
    }
    Ok(())
}

fn reviewed_elevated_exec(arguments: &Value) -> bool {
    let Ok(spec) = serde_json::from_value::<ElevatedExecSpec>(arguments.clone()) else {
        return false;
    };
    if spec.validate().is_err() || spec.workdir.is_some() {
        return false;
    }
    let Some(trusted_program) = reviewed_elevated_program() else {
        return false;
    };
    let requested = Path::new(&spec.program);
    let Ok(metadata) = fs::symlink_metadata(requested) else {
        return false;
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    let Ok(requested) = requested.canonicalize() else {
        return false;
    };
    if !same_windows_path(&requested, &trusted_program) {
        return false;
    }
    spec.args.is_empty()
        || matches!(
            spec.args.as_slice(),
            [arg] if matches!(arg.as_str(), "/all" | "/groups" | "/priv" | "/user")
        )
}

pub fn reviewed_elevated_program() -> Option<PathBuf> {
    let mut buffer = [0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return None;
    }
    let root = PathBuf::from(OsString::from_wide(&buffer[..length]));
    let program = root.join("whoami.exe");
    let metadata = fs::symlink_metadata(&program).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    program.canonicalize().ok()
}

fn same_windows_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}
