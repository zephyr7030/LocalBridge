use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

use crate::filesystem::policy::FilesystemPathPolicy;
use crate::privilege::{
    ElevatedExecSpec, MAX_ELEVATED_OUTPUT_BYTES, MAX_ELEVATED_STRING_BYTES,
    MAX_ELEVATED_TIMEOUT_MS, PrivilegedFilesystemSpec,
};
use crate::state::{Capability, PermissionMode, TaskKind};

use super::shell::ShellSelector;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicCapabilityDeclaration {
    pub read: bool,
    pub write: bool,
    pub process_exec: bool,
    pub git: bool,
    pub network: bool,
    pub privilege: bool,
    pub control_plane: bool,
}

impl PublicCapabilityDeclaration {
    const READ: Self = Self {
        read: true,
        write: false,
        process_exec: false,
        git: false,
        network: false,
        privilege: false,
        control_plane: false,
    };
    const PROCESS: Self = Self {
        read: false,
        write: false,
        process_exec: true,
        git: false,
        network: false,
        privilege: false,
        control_plane: false,
    };
    const GIT: Self = Self {
        read: false,
        write: false,
        process_exec: false,
        git: true,
        network: false,
        privilege: false,
        control_plane: false,
    };
    const fn workflow(
        write: bool,
        process_exec: bool,
        git: bool,
        network: bool,
        privilege: bool,
    ) -> Self {
        Self {
            read: true,
            write,
            process_exec,
            git,
            network,
            privilege,
            control_plane: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicActionDescriptor {
    pub tool: &'static str,
    pub action: &'static str,
    pub descriptor: ToolDescriptor,
    pub transitive: PublicCapabilityDeclaration,
}

const PUBLIC_CORE_TOOLS: &[&str] = &[
    "workspace_context",
    "agent_workflow",
    "filesystem",
    "exec_command",
    "command_control",
    "task_control",
    "git_workflow",
    "document_workflow",
    "view_image",
];
const PUBLIC_EDIT_MAX: &[&str] = &[
    "workspace_context",
    "agent_workflow",
    "filesystem",
    "task_control",
    "git_workflow",
    "document_workflow",
    "view_image",
];

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
    NetworkRouteNotAvailable,
    PrivilegedRouteNotAvailable,
    ElevatedExecNotReviewed,
    VerbatimExecutionPath,
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
    public_edit_allowed: HashSet<String>,
    public_full_allowed: HashSet<String>,
    public_elevated_allowed: HashSet<String>,
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
    administrator_gateway: AdministratorGatewaySection,
    localbridge_public: LocalBridgePublicSection,
}

#[derive(Debug, Deserialize)]
struct LocalBridgePublicSection {
    edit_tools: Vec<String>,
    full_tools: Vec<String>,
    elevated_tools: Vec<String>,
    unknown_action: String,
    network: String,
    privilege: String,
    control_plane: String,
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
struct AdministratorGatewaySection {
    route: String,
    token_scope: String,
    direct_process: String,
    shell: String,
    filesystem: String,
    system_management_identity: String,
    arbitrary_shell_executable_path: String,
    control_plane_mutation: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdministratorShellRequest {
    operation: String,
    shell: ShellSelector,
    command: String,
    workdir: String,
    timeout_ms: u32,
    max_output_bytes: u32,
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
    transitive_tool_capability_declaration: String,
}

#[derive(Debug, Deserialize)]
struct UpstreamSection {
    permission_mode: String,
    dangerously_skip_all_permissions: bool,
    telemetry: String,
    listener: String,
    internal_auth: String,
    direct_external_exposure: bool,
    authority_owner: String,
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
        let mut public_edit_allowed = document
            .localbridge_public
            .edit_tools
            .into_iter()
            .collect::<HashSet<_>>();
        let mut public_full_allowed = document
            .localbridge_public
            .full_tools
            .into_iter()
            .collect::<HashSet<_>>();
        let mut public_elevated_allowed = document
            .localbridge_public
            .elevated_tools
            .into_iter()
            .collect::<HashSet<_>>();
        for allowed in [
            &mut public_edit_allowed,
            &mut public_full_allowed,
            &mut public_elevated_allowed,
        ] {
            if allowed.contains("filesystem") {
                allowed.insert("task_control".to_string());
            }
        }
        Ok(Self {
            edit_allowed: document.edit_allowed_tools.into_iter().collect(),
            full_allowed: document.full_allowed_tools.into_iter().collect(),
            elevated_allowed: document.elevated_allowed_tools.into_iter().collect(),
            blocked: document.blocked_tools.into_iter().collect(),
            public_edit_allowed,
            public_full_allowed,
            public_elevated_allowed,
        })
    }

    pub fn classify_public_action(
        &self,
        tool_name: &str,
        arguments: &Value,
    ) -> Option<PublicActionDescriptor> {
        classify_public_action(tool_name, arguments)
    }

    pub fn decide_public(
        &self,
        mode: PermissionMode,
        tool_name: &str,
        arguments: &Value,
    ) -> PolicyDecision {
        let Some(action) = classify_public_action(tool_name, arguments) else {
            return denied(
                ToolDescriptor {
                    name: "localbridge-public-unknown",
                    capability: Capability::Unknown,
                    task_kind: TaskKind::Other,
                },
                DenyReason::UnknownTool,
            );
        };
        let declaration = action.transitive;
        if declaration.control_plane {
            return denied(action.descriptor, DenyReason::ControlPlane);
        }
        if declaration.privilege {
            return denied(action.descriptor, DenyReason::PrivilegedRouteNotAvailable);
        }
        if declaration.network {
            return denied(action.descriptor, DenyReason::NetworkRouteNotAvailable);
        }
        if mode == PermissionMode::Edit && declaration.process_exec {
            return denied(action.descriptor, DenyReason::IndirectProcessExecInEdit);
        }
        let allowed = match mode {
            PermissionMode::Edit => &self.public_edit_allowed,
            PermissionMode::Full => &self.public_full_allowed,
            PermissionMode::Elevated => &self.public_elevated_allowed,
        };
        if !allowed.contains(tool_name) {
            return denied(action.descriptor, DenyReason::ToolNotAllowedInMode);
        }
        PolicyDecision {
            descriptor: action.descriptor,
            allowed: true,
            deny_reason: None,
        }
    }

    pub fn public_tool_allowed_in_mode(&self, mode: PermissionMode, tool_name: &str) -> bool {
        let allowed = match mode {
            PermissionMode::Edit => &self.public_edit_allowed,
            PermissionMode::Full => &self.public_full_allowed,
            PermissionMode::Elevated => &self.public_elevated_allowed,
        };
        PUBLIC_CORE_TOOLS.contains(&tool_name) && allowed.contains(tool_name)
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
        if tool_name == "elevated_exec" && mode != PermissionMode::Elevated {
            return denied(
                self.classify(tool_name),
                DenyReason::PrivilegedRouteNotAvailable,
            );
        }
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

    pub fn privileged_tool_visible(&self, _mode: PermissionMode, tool_name: &str) -> bool {
        tool_name == "elevated_exec"
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

fn public_descriptor(
    tool: &'static str,
    action: &'static str,
    capability: Capability,
    task_kind: TaskKind,
    transitive: PublicCapabilityDeclaration,
) -> PublicActionDescriptor {
    PublicActionDescriptor {
        tool,
        action,
        descriptor: ToolDescriptor {
            name: tool,
            capability,
            task_kind,
        },
        transitive,
    }
}

fn classify_public_action(tool_name: &str, arguments: &Value) -> Option<PublicActionDescriptor> {
    let action = arguments.get("action").and_then(Value::as_str);
    let dry_run = arguments
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match tool_name {
        "workspace_context" if action.is_none() => Some(public_descriptor(
            "workspace_context",
            "context",
            Capability::Read,
            TaskKind::ReadFile,
            PublicCapabilityDeclaration::READ,
        )),
        "filesystem" => {
            let filesystem_action: &'static str = match action? {
                "list" => "list",
                "stat" => "stat",
                "read" => "read",
                "write" => "write",
                "replace" => "replace",
                "patch" => "patch",
                "search" => "search",
                "search_content" => "search_content",
                "copy" => "copy",
                "move" => "move",
                "delete" => "delete",
                "hash" => "hash",
                _ => return None,
            };
            let (capability, task_kind, declaration) = match filesystem_action {
                "search" | "search_content" => (
                    Capability::Read,
                    TaskKind::SearchCode,
                    PublicCapabilityDeclaration::READ,
                ),
                "list" | "stat" | "read" | "hash" => (
                    Capability::Read,
                    TaskKind::ReadFile,
                    PublicCapabilityDeclaration::READ,
                ),
                _ => (
                    Capability::Write,
                    TaskKind::ModifyFile,
                    PublicCapabilityDeclaration::workflow(true, false, false, false, false),
                ),
            };
            Some(public_descriptor(
                "filesystem",
                filesystem_action,
                capability,
                task_kind,
                declaration,
            ))
        }
        "exec_command" if action.is_none() => Some(public_descriptor(
            "exec_command",
            if dry_run { "explain" } else { "execute" },
            Capability::ProcessExec,
            if dry_run {
                TaskKind::Other
            } else {
                TaskKind::ExecuteCommand
            },
            if dry_run {
                PublicCapabilityDeclaration::READ
            } else {
                PublicCapabilityDeclaration::PROCESS
            },
        )),
        "command_control" => match action? {
            "adopt" => Some(public_descriptor(
                "command_control",
                "adopt",
                Capability::ProcessExec,
                TaskKind::ExecuteCommand,
                PublicCapabilityDeclaration::PROCESS,
            )),
            "poll" => Some(public_descriptor(
                "command_control",
                "poll",
                Capability::ProcessExec,
                TaskKind::ExecuteCommand,
                PublicCapabilityDeclaration::PROCESS,
            )),
            "read" => Some(public_descriptor(
                "command_control",
                "read",
                Capability::ProcessExec,
                TaskKind::ExecuteCommand,
                PublicCapabilityDeclaration::PROCESS,
            )),
            "write" => Some(public_descriptor(
                "command_control",
                "write",
                Capability::ProcessExec,
                TaskKind::ExecuteCommand,
                PublicCapabilityDeclaration::PROCESS,
            )),
            "kill" => Some(public_descriptor(
                "command_control",
                "kill",
                Capability::ProcessExec,
                TaskKind::ExecuteCommand,
                PublicCapabilityDeclaration::PROCESS,
            )),
            _ => None,
        },
        "task_control" => match action? {
            "list" => Some(public_descriptor(
                "task_control",
                "list",
                Capability::Workflow,
                TaskKind::Other,
                PublicCapabilityDeclaration::READ,
            )),
            "get" => Some(public_descriptor(
                "task_control",
                "get",
                Capability::Workflow,
                TaskKind::Other,
                PublicCapabilityDeclaration::READ,
            )),
            "cancel" => Some(public_descriptor(
                "task_control",
                "cancel",
                Capability::Workflow,
                TaskKind::ExecuteCommand,
                PublicCapabilityDeclaration::workflow(true, false, false, false, false),
            )),
            _ => None,
        },
        "git_workflow" => match action? {
            "status" => Some(public_descriptor(
                "git_workflow",
                "status",
                Capability::Git,
                TaskKind::GitOperation,
                PublicCapabilityDeclaration::GIT,
            )),
            "diff" => Some(public_descriptor(
                "git_workflow",
                "diff",
                Capability::Git,
                TaskKind::GitOperation,
                PublicCapabilityDeclaration::GIT,
            )),
            "log" => Some(public_descriptor(
                "git_workflow",
                "log",
                Capability::Git,
                TaskKind::GitOperation,
                PublicCapabilityDeclaration::GIT,
            )),
            "show" => Some(public_descriptor(
                "git_workflow",
                "show",
                Capability::Git,
                TaskKind::GitOperation,
                PublicCapabilityDeclaration::GIT,
            )),
            "blame" => Some(public_descriptor(
                "git_workflow",
                "blame",
                Capability::Git,
                TaskKind::GitOperation,
                PublicCapabilityDeclaration::GIT,
            )),
            _ => None,
        },
        "document_workflow" => match action? {
            "inspect" => Some(public_descriptor(
                "document_workflow",
                "inspect",
                Capability::Read,
                TaskKind::ReadFile,
                PublicCapabilityDeclaration::READ,
            )),
            "search" => Some(public_descriptor(
                "document_workflow",
                "search",
                Capability::Read,
                TaskKind::ReadFile,
                PublicCapabilityDeclaration::READ,
            )),
            "create" => Some(public_descriptor(
                "document_workflow",
                "create",
                Capability::Write,
                TaskKind::ModifyFile,
                PublicCapabilityDeclaration::workflow(true, false, false, false, false),
            )),
            "edit" => Some(public_descriptor(
                "document_workflow",
                "edit",
                Capability::Write,
                TaskKind::ModifyFile,
                PublicCapabilityDeclaration::workflow(true, false, false, false, false),
            )),
            "rebuild" => Some(public_descriptor(
                "document_workflow",
                "rebuild",
                Capability::Write,
                TaskKind::ModifyFile,
                PublicCapabilityDeclaration::workflow(true, false, false, false, false),
            )),
            "convert" => Some(public_descriptor(
                "document_workflow",
                "convert",
                Capability::Workflow,
                TaskKind::ModifyFile,
                PublicCapabilityDeclaration::workflow(true, true, false, false, false),
            )),
            _ => None,
        },
        "view_image" if action.is_none() => Some(public_descriptor(
            "view_image",
            "inspect",
            Capability::Read,
            TaskKind::ReadFile,
            PublicCapabilityDeclaration::READ,
        )),
        "agent_workflow" => {
            let workflow_action = action?;
            let object = arguments.as_object()?;
            let phase = object.get("phase").and_then(Value::as_str);
            if object.get("phase").is_some()
                && !matches!(phase, Some("prepare" | "edit" | "verify" | "persist"))
            {
                return None;
            }
            let commands_present = phase == Some("verify")
                || match object.get("commands") {
                    None => false,
                    Some(commands) => !commands.as_array()?.is_empty(),
                };
            let patch_present = match object.get("patch") {
                None => false,
                Some(value) => {
                    value.as_str()?;
                    true
                }
            };
            let directory_changes_present = match object.get("directory_changes") {
                None => false,
                Some(value) => {
                    let changes = value.as_array()?;
                    if changes.is_empty() || changes.len() > 32 {
                        return None;
                    }
                    for change in changes {
                        let change = change.as_object()?;
                        if change.len() != 2
                            || !change.contains_key("action")
                            || !change.contains_key("path")
                            || !matches!(
                                change.get("action").and_then(Value::as_str),
                                Some("create_directory" | "remove_empty_directory")
                            )
                            || change
                                .get("path")
                                .and_then(Value::as_str)
                                .is_none_or(str::is_empty)
                        {
                            return None;
                        }
                    }
                    !changes.is_empty()
                }
            };
            let name = match workflow_action {
                "diagnose" => "diagnose",
                "document" => "document",
                "bugfix" => "bugfix",
                "feature" => "feature",
                "refactor" => "refactor",
                "test_failure" => "test_failure",
                "build_release" => "build_release",
                "resume" => "resume",
                "custom" => "custom",
                _ => return None,
            };
            let declaration = if dry_run {
                PublicCapabilityDeclaration::workflow(false, false, false, false, false)
            } else {
                PublicCapabilityDeclaration::workflow(
                    patch_present || directory_changes_present,
                    commands_present,
                    true,
                    false,
                    false,
                )
            };
            Some(public_descriptor(
                "agent_workflow",
                name,
                Capability::Workflow,
                TaskKind::Other,
                declaration,
            ))
        }
        _ => None,
    }
}

pub(crate) fn command_task_kind(command: &str) -> TaskKind {
    let command = command.to_ascii_lowercase();
    if command_invokes_any(
        &command,
        &[
            "cargo test",
            "npm test",
            "npm run test",
            "pnpm test",
            "pnpm run test",
            "yarn test",
            "bun test",
            "dotnet test",
            "go test",
            "pytest",
            "vitest",
        ],
    ) {
        TaskKind::Test
    } else if command_invokes_any(
        &command,
        &[
            "cargo build",
            "npm run build",
            "pnpm build",
            "pnpm run build",
            "yarn build",
            "bun run build",
            "dotnet build",
            "go build",
            "tauri build",
        ],
    ) {
        TaskKind::Build
    } else {
        TaskKind::ExecuteCommand
    }
}

fn command_invokes_any(command: &str, invocations: &[&str]) -> bool {
    command
        .split([';', '\n'])
        .flat_map(|segment| segment.split("&&"))
        .flat_map(|segment| segment.split("||"))
        .map(str::trim_start)
        .any(|segment| {
            invocations.iter().any(|invocation| {
                segment == *invocation
                    || segment
                        .strip_prefix(invocation)
                        .is_some_and(|suffix| suffix.starts_with(char::is_whitespace))
            })
        })
}

fn static_nested_cmd_inner(command: &str) -> Option<&str> {
    let trimmed = command.trim();
    let (program, rest) = if let Some(quoted) = trimmed.strip_prefix('"') {
        let end = quoted.find('"')?;
        (&quoted[..end], &quoted[end + 1..])
    } else {
        let end = trimmed.find(char::is_whitespace)?;
        (&trimmed[..end], &trimmed[end..])
    };
    let program = program.rsplit(['\\', '/']).next().unwrap_or(program);
    if !matches!(program.to_ascii_lowercase().as_str(), "cmd" | "cmd.exe") {
        return None;
    }
    let rest = rest.trim_start();
    let switch_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    if !matches!(
        rest[..switch_end].to_ascii_lowercase().as_str(),
        "/c" | "/k"
    ) {
        return None;
    }
    let inner = rest[switch_end..].trim_start();
    (!inner.is_empty()).then_some(inner)
}

fn nested_cmd_body(inner: &str) -> &str {
    let inner = inner.trim();
    inner
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .filter(|value| !value.is_empty())
        .unwrap_or(inner)
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
    if document.schema_version != 7
        || document.runtime_version != PINNED_RUNTIME_VERSION
        || document.status != "SCHEMA46_CURRENT_USER_EXECUTION_POLICY"
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
    let public_edit = document
        .localbridge_public
        .edit_tools
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let public_full = document
        .localbridge_public
        .full_tools
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let public_elevated = document
        .localbridge_public
        .elevated_tools
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let public_core = PUBLIC_CORE_TOOLS.iter().copied().collect::<HashSet<_>>();
    let public_edit_max = PUBLIC_EDIT_MAX.iter().copied().collect::<HashSet<_>>();
    if !public_edit.is_subset(&public_edit_max)
        || !public_full.is_subset(&public_core)
        || !public_elevated.is_subset(&public_core)
        || !public_edit.is_subset(&public_full)
        || !public_full.is_subset(&public_elevated)
        || document.localbridge_public.unknown_action != "deny"
        || document.localbridge_public.network != "deny_unreviewed"
        || document.localbridge_public.privilege != "broker_only"
        || document.localbridge_public.control_plane != "deny_always"
    {
        return Err(PolicyError::ContractMismatch("localbridge_public"));
    }
    if document.capabilities.unknown != "deny"
        || document.capabilities.process_exec_in_edit != "deny"
        || document.capabilities.process_exec_in_full != "current_user_token"
        || document.capabilities.elevated_exec_in_edit != "deny"
        || document.capabilities.elevated_exec_in_full != "deny"
        || document.capabilities.elevated_exec_in_elevated != "allow_if_reviewed_and_broker_active"
        || document.capabilities.workflow_with_process_exec_in_edit != "deny"
        || document.capabilities.control_plane != "deny_always"
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
    if document.administrator_gateway.route != "broker_only"
        || document.administrator_gateway.token_scope != "administrator_token"
        || document.administrator_gateway.direct_process != "structured_absolute_program_argv"
        || document.administrator_gateway.shell != "trusted_logical_selector_only"
        || document.administrator_gateway.filesystem != "structured_absolute_path_broker"
        || document.administrator_gateway.system_management_identity != "exact_system32"
        || document
            .administrator_gateway
            .arbitrary_shell_executable_path
            != "deny"
        || document.administrator_gateway.control_plane_mutation != "deny_always"
    {
        return Err(PolicyError::ContractMismatch("administrator_gateway"));
    }
    if document.enforcement.tools_list_filter != "ux_only"
        || document.enforcement.tools_call_check != "mandatory"
        || document.enforcement.implementation != "first_party_rust_mcp_guard"
        || document.enforcement.privileged_route != "broker_only"
        || document.enforcement.upstream_direct_tunnel_target != "forbidden"
        || document.enforcement.unknown_tool != "deny"
        || document.enforcement.request_permissions != "deny_always"
        || document.enforcement.transitive_tool_capability_declaration != "required"
    {
        return Err(PolicyError::ContractMismatch("enforcement"));
    }
    if document.upstream_coding_tools.permission_mode != "policy_neutral_behind_authenticated_guard"
        || !document
            .upstream_coding_tools
            .dangerously_skip_all_permissions
        || document.upstream_coding_tools.telemetry != "disabled"
        || document.upstream_coding_tools.listener != "loopback_ephemeral"
        || document.upstream_coding_tools.internal_auth != "runtime_generated_bearer_required"
        || document.upstream_coding_tools.direct_external_exposure
        || document.upstream_coding_tools.authority_owner != "localbridge_guard_only"
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
    let Some(operation) = arguments.get("operation").and_then(Value::as_str) else {
        return reviewed_legacy_elevated_exec(arguments);
    };
    match operation {
        "process" => reviewed_administrator_process(arguments),
        "shell" => reviewed_administrator_shell(arguments),
        "filesystem" => reviewed_administrator_filesystem(arguments),
        _ => false,
    }
}

fn reviewed_legacy_elevated_exec(arguments: &Value) -> bool {
    let Ok(spec) = serde_json::from_value::<ElevatedExecSpec>(arguments.clone()) else {
        return false;
    };
    if spec.validate().is_err() || spec.workdir.is_some() {
        return false;
    }
    let Some(trusted_program) = reviewed_elevated_program() else {
        return false;
    };
    let Ok(trusted_program) = trusted_program.canonicalize() else {
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

fn reviewed_administrator_process(arguments: &Value) -> bool {
    let Some(mut object) = arguments.as_object().cloned() else {
        return false;
    };
    if object.remove("operation").as_ref().and_then(Value::as_str) != Some("process") {
        return false;
    }
    let Ok(spec) = serde_json::from_value::<ElevatedExecSpec>(Value::Object(object)) else {
        return false;
    };
    if spec.validate().is_err()
        || explicit_control_plane_reference(&spec.program)
        || spec
            .args
            .iter()
            .any(|arg| explicit_control_plane_reference(arg))
        || spec
            .workdir
            .as_deref()
            .is_some_and(explicit_control_plane_reference)
    {
        return false;
    }
    let Some(requested) = canonical_regular_file(Path::new(&spec.program)) else {
        return false;
    };
    let Some(trusted_program) =
        reviewed_elevated_program().and_then(|path| canonical_regular_file(&path))
    else {
        return false;
    };
    // An arbitrary Administrator-token executable is opaque to the PEP: it can
    // locate LocalBridge state internally without placing that target in argv.
    // Therefore direct process execution is intentionally limited to the one
    // frozen read-only identity diagnostic. General administrator maintenance
    // remains available through the statically reviewed shell route.
    if !same_windows_path(&requested, &trusted_program) {
        return false;
    }
    if let Some(workdir) = spec.workdir.as_deref() {
        let workdir = Path::new(workdir);
        if !workdir.is_absolute()
            || !workdir.is_dir()
            || explicit_control_plane_reference(workdir.to_string_lossy().as_ref())
        {
            return false;
        }
    }
    spec.args.is_empty()
        || matches!(
            spec.args.as_slice(),
            [arg] if matches!(arg.as_str(), "/all" | "/groups" | "/priv" | "/user")
        )
}

fn reviewed_administrator_shell(arguments: &Value) -> bool {
    let Ok(request) = serde_json::from_value::<AdministratorShellRequest>(arguments.clone()) else {
        return false;
    };
    if request.operation != "shell"
        || request.command.is_empty()
        || request.command.len() > MAX_ELEVATED_STRING_BYTES
        || request.command.as_bytes().contains(&0)
        || request.timeout_ms == 0
        || request.timeout_ms > MAX_ELEVATED_TIMEOUT_MS
        || request.max_output_bytes == 0
        || request.max_output_bytes > MAX_ELEVATED_OUTPUT_BYTES
        || explicit_control_plane_reference_obfuscated(&request.command)
        || explicit_control_plane_reference(&request.workdir)
        || administrator_shell_dynamic_target_construction(request.shell, &request.command)
    {
        return false;
    }
    let workdir = Path::new(&request.workdir);
    if !workdir.is_absolute()
        || !workdir.is_dir()
        || request.workdir.starts_with(r"\\?\")
        || request.workdir.contains(['\n', '\r'])
        || request.workdir.as_bytes().contains(&0)
    {
        return false;
    }
    true
}

fn administrator_shell_dynamic_target_construction(shell: ShellSelector, command: &str) -> bool {
    match shell {
        ShellSelector::Cmd => administrator_cmd_dynamic_target_construction(command),
        ShellSelector::Auto
        | ShellSelector::Powershell
        | ShellSelector::Pwsh
        | ShellSelector::WindowsPowershell => {
            administrator_powershell_dynamic_target_construction(command)
                || static_nested_cmd_inner(command).is_some_and(|inner| {
                    administrator_cmd_dynamic_target_construction(nested_cmd_body(inner))
                })
        }
    }
}

fn administrator_powershell_dynamic_target_construction(command: &str) -> bool {
    // Administrator Shell requests must be statically reviewable. These are
    // PowerShell language surfaces that can synthesize a path/command after the
    // PEP decision, so allowing them would make control_plane_mutation=deny_always
    // depend on request spelling rather than the executed target.
    if command.chars().any(|ch| {
        matches!(
            ch,
            '$' | '`' | '+' | '@' | '(' | ')' | '{' | '}' | '[' | ']'
        )
    }) {
        return true;
    }
    let lower = command.to_ascii_lowercase();
    [
        "invoke-expression",
        " iex ",
        "set-variable",
        "new-variable",
        "set-alias",
        "new-alias",
        "invoke-command",
        "foreach-object",
        "start-process",
        " -join ",
        " -f ",
        "function ",
        "filter ",
        "workflow ",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn administrator_cmd_dynamic_target_construction(command: &str) -> bool {
    if command.chars().any(|ch| matches!(ch, '%' | '!' | '^')) {
        return true;
    }
    command
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '&' | '|' | '(' | ')' | ';'))
        .filter(|word| !word.is_empty())
        .any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "call" | "for" | "setlocal"
            )
        })
}

fn reviewed_administrator_filesystem(arguments: &Value) -> bool {
    let Some(mut object) = arguments.as_object().cloned() else {
        return false;
    };
    if object.remove("operation").as_ref().and_then(Value::as_str) != Some("filesystem") {
        return false;
    }
    let Ok(spec) = serde_json::from_value::<PrivilegedFilesystemSpec>(Value::Object(object)) else {
        return false;
    };
    spec.validate().is_ok()
        && FilesystemPathPolicy::allows(&spec.path)
        && spec
            .destination
            .as_deref()
            .is_none_or(FilesystemPathPolicy::allows)
}

pub(crate) fn explicit_control_plane_reference(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "localbridge",
        "com.localbridge.desktop",
        "runtime-policy.toml",
        "runtime-manifest.toml",
        "startup-profile.json",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn explicit_control_plane_reference_obfuscated(value: &str) -> bool {
    if explicit_control_plane_reference(value) {
        return true;
    }
    let compact = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "localbridge",
        "comlocalbridgedesktop",
        "runtimepolicytoml",
        "runtimemanifesttoml",
        "startupprofilejson",
    ]
    .iter()
    .any(|marker| compact.contains(marker))
}

fn canonical_regular_file(path: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    path.canonicalize().ok()
}

fn trusted_system_program(name: &str) -> Option<PathBuf> {
    let mut buffer = [0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return None;
    }
    let root = PathBuf::from(OsString::from_wide(&buffer[..length]));
    let program = root.join(name);
    canonical_regular_file(&program).map(|_| program)
}

pub fn reviewed_elevated_program() -> Option<PathBuf> {
    // Legacy profile retained for schema32/architecture compatibility; schema33 typed routes use
    // the administrator gateway above rather than expanding this diagnostic allowlist.
    trusted_system_program("whoami.exe")
}

fn same_windows_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(test)]
mod administrator_gateway_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn typed_routes_allow_administrator_os_scope_and_deny_localbridge_control_plane() {
        let whoami = reviewed_elevated_program()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(reviewed_elevated_exec(&json!({
            "operation":"process","program":whoami,"args":["/user"],"workdir":null,
            "timeout_ms":1000,"max_output_bytes":4096
        })));
        assert!(reviewed_elevated_exec(&json!({
            "operation":"shell","shell":"cmd","command":"whoami /user",
            "workdir":"C:\\Windows\\Temp","timeout_ms":1000,"max_output_bytes":4096
        })));
        assert!(reviewed_elevated_exec(&json!({
            "operation":"filesystem","action":"read_file","path":"C:\\Windows\\win.ini",
            "destination":null,"content_base64":null,"recursive":false
        })));
        assert!(!reviewed_elevated_exec(&json!({
            "operation":"shell","shell":"powershell",
            "command":"Set-Content C:/ProgramData/LocalBridge/settings.json x",
            "workdir":"C:\\Windows\\Temp","timeout_ms":1000,"max_output_bytes":4096
        })));
        assert!(!reviewed_elevated_exec(&json!({
            "operation":"filesystem","action":"delete","path":"C:\\ProgramData\\LocalBridge",
            "destination":null,"content_base64":null,"recursive":true
        })));
    }

    #[test]
    fn administrator_shell_rejects_dynamic_or_obfuscated_control_plane_targets() {
        for command in [
            "$a='Local'; $b='Bridge'; Set-Content ('C:\\ProgramData\\'+$a+$b+'\\settings.json') x",
            "Set-Content ('C:\\ProgramData\\Loc'+'alBridge\\settings.json') x",
        ] {
            assert!(
                !reviewed_elevated_exec(&json!({
                    "operation":"shell","shell":"powershell","command":command,
                    "workdir":"C:\\Windows\\Temp","timeout_ms":1000,"max_output_bytes":4096
                })),
                "{command}"
            );
        }
        for command in [
            "set a=Local&set b=Bridge&del C:\\ProgramData\\%a%%b%\\settings.json",
            "del C:\\ProgramData\\Loc\"alBri\"dge\\settings.json",
        ] {
            assert!(
                !reviewed_elevated_exec(&json!({
                    "operation":"shell","shell":"cmd","command":command,
                    "workdir":"C:\\Windows\\Temp","timeout_ms":1000,"max_output_bytes":4096
                })),
                "{command}"
            );
        }
        for (shell, command) in [
            ("cmd", "whoami /user"),
            ("cmd", "sc.exe query wuauserv"),
            ("powershell", "Get-Service wuauserv"),
        ] {
            assert!(
                reviewed_elevated_exec(&json!({
                    "operation":"shell","shell":shell,"command":command,
                    "workdir":"C:\\Windows\\Temp","timeout_ms":1000,"max_output_bytes":4096
                })),
                "{shell}: {command}"
            );
        }
    }

    #[test]
    fn direct_process_is_frozen_to_read_only_identity_diagnostic() {
        assert!(!reviewed_elevated_exec(&json!({
            "operation":"process","program":"C:\\Windows\\System32\\cmd.exe",
            "args":["/c","whoami"],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096
        })));
        let system_reg = trusted_system_program("reg.exe").unwrap();
        assert!(!reviewed_elevated_exec(&json!({
            "operation":"process","program":system_reg.to_string_lossy(),
            "args":["query","HKLM\\Software\\Microsoft"],"workdir":null,
            "timeout_ms":1000,"max_output_bytes":4096
        })));
        let opaque_helper = std::env::current_exe().unwrap();
        assert!(!reviewed_elevated_exec(&json!({
            "operation":"process","program":opaque_helper.to_string_lossy(),
            "args":[],"workdir":null,"timeout_ms":1000,"max_output_bytes":4096
        })));
        assert!(reviewed_elevated_exec(&json!({
            "operation":"shell","shell":"cmd","command":"reg.exe query HKLM\\Software\\Microsoft",
            "workdir":"C:\\Windows\\Temp","timeout_ms":1000,"max_output_bytes":4096
        })));
    }
}
