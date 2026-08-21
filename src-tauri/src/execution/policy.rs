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
const WINDOWS_SYSTEM_MANAGEMENT_PROGRAMS: &[&str] = &[
    "reg.exe",
    "schtasks.exe",
    "sc.exe",
    "netsh.exe",
    "bcdedit.exe",
    "dism.exe",
    "pnputil.exe",
    "powercfg.exe",
    "wevtutil.exe",
    "net.exe",
    "net1.exe",
    "fsutil.exe",
    "mountvol.exe",
    "reagentc.exe",
    "manage-bde.exe",
    "fltmc.exe",
    "auditpol.exe",
    "vssadmin.exe",
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
    transitive_exec_classification: String,
    shell_invocation_review: String,
    unreviewable_shell_indirection: String,
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

    pub fn public_tool_allowed_for_list(&self, mode: PermissionMode, tool_name: &str) -> bool {
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
                "search" => "search",
                "copy" => "copy",
                "move" => "move",
                "delete" => "delete",
                "hash" => "hash",
                _ => return None,
            };
            let (capability, task_kind, declaration) = match filesystem_action {
                "search" => (
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
                PublicCapabilityDeclaration {
                    privilege: shell_request_requires_review(arguments),
                    ..PublicCapabilityDeclaration::PROCESS
                }
            },
        )),
        "command_control" => match action? {
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
            "create" => Some(public_descriptor(
                "document_workflow",
                "create",
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
            let shell_review_required = workflow_commands_require_review(arguments);
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
                    shell_review_required,
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

fn workflow_commands_require_review(arguments: &Value) -> bool {
    let Some(commands) = arguments.get("commands") else {
        return false;
    };
    let Some(commands) = commands.as_array() else {
        return true;
    };
    commands.iter().any(shell_request_requires_review)
}

fn shell_request_requires_review(arguments: &Value) -> bool {
    let Some(command) = arguments.get("command").and_then(Value::as_str) else {
        return false;
    };
    let shell = arguments
        .get("shell")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    shell_invocation_requires_review(shell, command)
}

pub(crate) fn shell_invocation_requires_review(shell: &str, command: &str) -> bool {
    if let Some((_, consumed)) = static_workspace_script_invocation(shell, command) {
        let remainder = &command[consumed..];
        return match shell {
            "powershell" | "pwsh" | "windows_powershell" => {
                powershell_invocation_requires_review(remainder)
            }
            "cmd" => cmd_invocation_requires_review(remainder),
            "auto" => {
                powershell_invocation_requires_review(remainder)
                    || cmd_invocation_requires_review(remainder)
            }
            _ => true,
        };
    }
    match shell {
        "powershell" | "pwsh" | "windows_powershell" => {
            powershell_invocation_requires_review(command)
        }
        "cmd" => cmd_invocation_requires_review(command),
        // `auto` may legitimately fall back to cmd when no trusted PowerShell exists,
        // so its review is the conservative union of both supported grammars.
        "auto" => {
            powershell_invocation_requires_review(command)
                || cmd_invocation_requires_review(command)
        }
        // Invalid/unknown selectors are rejected by the public schema/Facade, but policy
        // must never turn an unknown execution grammar into an allow decision.
        _ => true,
    }
}

pub(crate) fn static_workspace_script_target(shell: &str, command: &str) -> Option<String> {
    static_workspace_script_invocation(shell, command).map(|(target, _)| target)
}

fn static_workspace_script_invocation(shell: &str, command: &str) -> Option<(String, usize)> {
    let powershell = matches!(shell, "powershell" | "pwsh" | "windows_powershell" | "auto");
    let cmd = matches!(shell, "cmd" | "auto");
    if !powershell && !cmd {
        return None;
    }

    fn parse_token(input: &str, allow_single_quote: bool) -> Option<(String, usize)> {
        let leading = input.len().saturating_sub(input.trim_start().len());
        let value = &input[leading..];
        let first = value.chars().next()?;
        if first == '"' || (allow_single_quote && first == '\'') {
            let quote_len = first.len_utf8();
            let body = &value[quote_len..];
            let end = body.find(first)?;
            let token = &body[..end];
            if token.is_empty() {
                return None;
            }
            return Some((token.to_string(), leading + quote_len + end + quote_len));
        }
        let end = value
            .find(|ch: char| {
                ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '(' | ')' | '{' | '}')
            })
            .unwrap_or(value.len());
        let token = &value[..end];
        (!token.is_empty()).then(|| (token.to_string(), leading + end))
    }

    fn literal_script_target(target: &str) -> bool {
        if target.is_empty()
            || target.chars().any(|ch| {
                matches!(
                    ch,
                    '$' | '%' | '!' | '`' | '*' | '?' | '[' | ']' | '{' | '}'
                )
            })
        {
            return false;
        }
        let lower = target.to_ascii_lowercase();
        lower.ends_with(".ps1") || lower.ends_with(".cmd") || lower.ends_with(".bat")
    }

    let leading = command.len().saturating_sub(command.trim_start().len());
    let trimmed = &command[leading..];

    if powershell {
        if trimmed.starts_with('.') && trimmed.chars().nth(1).is_some_and(char::is_whitespace) {
            return None;
        }
        if let Some(after_call) = trimmed.strip_prefix('&') {
            if !after_call.chars().next().is_some_and(char::is_whitespace) {
                return None;
            }
            let (target, consumed) = parse_token(after_call, true)?;
            if literal_script_target(&target) {
                return Some((target, leading + 1 + consumed));
            }
            return None;
        }
        if !matches!(trimmed.chars().next(), Some('\'' | '"')) {
            if let Some((target, consumed)) = parse_token(trimmed, false) {
                if literal_script_target(&target) {
                    return Some((target, leading + consumed));
                }
            }
        }
    }

    if cmd {
        let mut cmd_input = trimmed;
        let mut prefix = leading;
        if let Some(rest) = cmd_input.strip_prefix('@') {
            cmd_input = rest;
            prefix += 1;
        }
        if cmd_input
            .get(..4)
            .is_some_and(|value| value.eq_ignore_ascii_case("call"))
            && cmd_input[4..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
        {
            let after_call = &cmd_input[4..];
            let (target, consumed) = parse_token(after_call, false)?;
            if literal_script_target(&target) {
                return Some((target, prefix + 4 + consumed));
            }
            return None;
        }
        if let Some((target, consumed)) = parse_token(cmd_input, false) {
            if literal_script_target(&target) {
                return Some((target, prefix + consumed));
            }
        }
    }
    None
}

fn powershell_review_word(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "docker"
            | "docker.exe"
            | "podman"
            | "podman.exe"
            | "wsl"
            | "wsl.exe"
            | "powershell"
            | "powershell.exe"
            | "pwsh"
            | "pwsh.exe"
            | "cmd"
            | "cmd.exe"
            | "invoke-expression"
            | "iex"
            | "start-process"
            | "saps"
            | "start"
            | "invoke-item"
            | "ii"
            | "invoke-command"
            | "icm"
            | "start-job"
            | "start-threadjob"
            | "enter-pssession"
            | "new-pssession"
            | "import-pssession"
            | "add-pssnapin"
            | "new-module"
            | "using"
            | "requires"
            | "get-command"
            | "gcm"
            | "psmoduleautoloadingpreference"
            | "set-variable"
            | "set"
            | "sv"
            | "new-variable"
            | "nv"
            | "remove-variable"
            | "rv"
            | "clear-variable"
            | "clv"
            | "set-alias"
            | "sal"
            | "new-alias"
            | "nal"
            | "set-item"
            | "si"
            | "new-item"
            | "ni"
            | "remove-item"
            | "rd"
            | "ri"
            | "rm"
            | "rmdir"
            | "rename-item"
            | "ren"
            | "rni"
            | "move-item"
            | "mi"
            | "move"
            | "mv"
            | "copy-item"
            | "copy"
            | "cp"
            | "cpi"
            | "clear-item"
            | "cli"
            | "set-content"
            | "sc"
            | "add-content"
            | "ac"
            | "clear-content"
            | "clc"
            | "import-alias"
            | "ipal"
            | "export-alias"
            | "epal"
            | "import-module"
            | "ipmo"
            | "invoke-cimmethod"
            | "invoke-wmimethod"
            | "get-wmiobject"
            | "gwmi"
            | "new-object"
            | "add-type"
            | "comobject"
            | "wmiclass"
            | "managementclass"
            | "wmic"
            | "rundll32"
            | "rundll32.exe"
            | "regsvr32"
            | "regsvr32.exe"
            | "mshta"
            | "mshta.exe"
            | "wscript"
            | "wscript.exe"
            | "cscript"
            | "cscript.exe"
            | "scriptblock"
            | "createprocess"
            | "win32_process"
            | "alias"
            | "function"
            | "filter"
            | "workflow"
            | "configuration"
            | "call"
    )
}

fn cmd_review_word(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "docker"
            | "docker.exe"
            | "podman"
            | "podman.exe"
            | "wsl"
            | "wsl.exe"
            | "powershell"
            | "powershell.exe"
            | "pwsh"
            | "pwsh.exe"
            | "rundll32"
            | "rundll32.exe"
            | "regsvr32"
            | "regsvr32.exe"
            | "mshta"
            | "mshta.exe"
            | "wscript"
            | "wscript.exe"
            | "cscript"
            | "cscript.exe"
            | "call"
    )
}

fn simple_static_command_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for ch in command.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => word.push(ch),
            None if matches!(ch, '\'' | '"') => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            None if matches!(
                ch,
                '&' | '|' | ';' | '<' | '>' | '\r' | '\n' | '%' | '!' | '$' | '`' | '^'
            ) =>
            {
                return None;
            }
            None => word.push(ch),
        }
    }
    if quote.is_some() {
        return None;
    }
    if !word.is_empty() {
        words.push(word);
    }
    (!words.is_empty()).then_some(words)
}

fn frozen_readonly_system_management_invocation(command: &str) -> bool {
    let Some(words) = simple_static_command_words(command) else {
        return false;
    };
    let program = words[0].rsplit(['\\', '/']).next().unwrap_or(&words[0]);
    let program = program
        .strip_suffix(".exe")
        .unwrap_or(program)
        .to_ascii_lowercase();
    let args = &words[1..];
    match program.as_str() {
        "pnputil" => matches!(args, [op] if op.eq_ignore_ascii_case("/enum-drivers")),
        "powercfg" => {
            matches!(args, [op] if ["/query","/getactivescheme","/list","/a"].iter().any(|allowed| op.eq_ignore_ascii_case(allowed)))
        }
        "wevtutil" => {
            matches!(args, [op] if op.eq_ignore_ascii_case("el"))
                || matches!(args, [op, _log] if op.eq_ignore_ascii_case("gl"))
        }
        "reagentc" => matches!(args, [op] if op.eq_ignore_ascii_case("/info")),
        "manage-bde" => matches!(args, [op] | [op, _] if op.eq_ignore_ascii_case("-status")),
        "fltmc" => {
            args.is_empty()
                || matches!(args, [op] if ["filters", "instances", "volumes"].iter().any(|allowed| op.eq_ignore_ascii_case(allowed)))
        }
        "auditpol" => args
            .first()
            .is_some_and(|op| op.eq_ignore_ascii_case("/get")),
        "vssadmin" => args
            .first()
            .is_some_and(|op| op.eq_ignore_ascii_case("list")),
        _ => false,
    }
}

fn windows_system_management_program_token(token: &str) -> bool {
    let token = token.trim_matches(['\'', '"']);
    let basename = token.rsplit(['\\', '/']).next().unwrap_or(token);
    WINDOWS_SYSTEM_MANAGEMENT_PROGRAMS.iter().any(|program| {
        basename.eq_ignore_ascii_case(program)
            || program
                .strip_suffix(".exe")
                .is_some_and(|stem| basename.eq_ignore_ascii_case(stem))
    })
}

fn powershell_static_system_management_target(command: &str) -> bool {
    if frozen_readonly_system_management_invocation(command) {
        return false;
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    fn finish_target(token: &mut String, is_target: &mut bool) -> bool {
        let requires_privilege = *is_target && windows_system_management_program_token(token);
        token.clear();
        *is_target = false;
        requires_privilege
    }

    let mut quote = Quote::None;
    let mut token = String::new();
    let mut token_is_target = false;
    let mut command_boundary = true;
    let mut comment = false;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if comment {
            if matches!(ch, '\r' | '\n') {
                comment = false;
                command_boundary = true;
            }
            continue;
        }
        match quote {
            Quote::Single => {
                if ch == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                    } else {
                        quote = Quote::None;
                    }
                }
                continue;
            }
            Quote::Double => {
                if ch == '`' {
                    chars.next();
                } else if ch == '"' {
                    quote = Quote::None;
                }
                continue;
            }
            Quote::None => {}
        }

        if ch == '#' {
            if finish_target(&mut token, &mut token_is_target) {
                return true;
            }
            comment = true;
            continue;
        }
        if matches!(ch, '\'' | '"') {
            if finish_target(&mut token, &mut token_is_target) {
                return true;
            }
            quote = if ch == '\'' {
                Quote::Single
            } else {
                Quote::Double
            };
            command_boundary = false;
            continue;
        }
        if ch == '`' {
            if finish_target(&mut token, &mut token_is_target) {
                return true;
            }
            chars.next();
            command_boundary = false;
            continue;
        }
        if matches!(ch, ';' | '|' | '&' | '\r' | '\n' | '{' | '}') {
            if finish_target(&mut token, &mut token_is_target) {
                return true;
            }
            command_boundary = true;
            continue;
        }
        if ch.is_whitespace() {
            if finish_target(&mut token, &mut token_is_target) {
                return true;
            }
            continue;
        }
        if token.is_empty() {
            token_is_target = command_boundary;
            command_boundary = false;
        }
        token.push(ch);
    }
    finish_target(&mut token, &mut token_is_target)
}

fn cmd_static_system_management_target(command: &str) -> bool {
    if frozen_readonly_system_management_invocation(command) {
        return false;
    }
    fn finish_target(token: &mut String, is_target: &mut bool) -> bool {
        let requires_privilege = *is_target && windows_system_management_program_token(token);
        token.clear();
        *is_target = false;
        requires_privilege
    }

    let mut quoted = false;
    let mut token = String::new();
    let mut token_is_target = false;
    let mut command_boundary = true;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            if token.is_empty() && command_boundary {
                token_is_target = true;
                command_boundary = false;
            }
            quoted = !quoted;
            if !quoted && finish_target(&mut token, &mut token_is_target) {
                return true;
            }
            continue;
        }
        if quoted {
            token.push(ch);
            continue;
        }
        if ch == '^' {
            if let Some(escaped) = chars.next() {
                if token.is_empty() {
                    token_is_target = command_boundary;
                    command_boundary = false;
                }
                token.push(escaped);
            }
            continue;
        }
        if matches!(ch, '&' | '|' | '\r' | '\n' | '(' | ')') {
            if finish_target(&mut token, &mut token_is_target) {
                return true;
            }
            command_boundary = true;
            continue;
        }
        if ch.is_whitespace() {
            if finish_target(&mut token, &mut token_is_target) {
                return true;
            }
            continue;
        }
        if command_boundary && ch == '@' && token.is_empty() {
            continue;
        }
        if token.is_empty() {
            token_is_target = command_boundary;
            command_boundary = false;
        }
        token.push(ch);
    }
    finish_target(&mut token, &mut token_is_target)
}

fn cmd_if_segment_system_management_target(command: &str) -> bool {
    fn finish_word(words: &mut Vec<String>, word: &mut String) {
        if !word.is_empty() {
            words.push(std::mem::take(word));
        }
    }

    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if ch == '^' && !quoted {
            if let Some(escaped) = chars.next() {
                word.push(escaped);
            }
            continue;
        }
        if !quoted && (ch.is_whitespace() || matches!(ch, '&' | '|' | '(' | ')')) {
            finish_word(&mut words, &mut word);
            continue;
        }
        if word.is_empty() && ch == '@' {
            continue;
        }
        word.push(ch);
    }
    finish_word(&mut words, &mut word);

    if !words
        .first()
        .is_some_and(|word| word.eq_ignore_ascii_case("if"))
    {
        return false;
    }
    let mut condition = 1usize;
    while words
        .get(condition)
        .is_some_and(|word| word.eq_ignore_ascii_case("not") || word.eq_ignore_ascii_case("/i"))
    {
        condition += 1;
    }
    let Some(first_condition) = words.get(condition) else {
        return false;
    };
    let command_start = if matches!(
        first_condition.to_ascii_lowercase().as_str(),
        "errorlevel" | "cmdextversion" | "exist" | "defined"
    ) {
        condition + 2
    } else if first_condition.contains("==") {
        condition + 1
    } else if words.get(condition + 1).is_some_and(|operator| {
        matches!(
            operator.to_ascii_lowercase().as_str(),
            "equ" | "neq" | "lss" | "leq" | "gtr" | "geq"
        )
    }) {
        condition + 3
    } else {
        // Unknown IF grammar stays fail-closed if a protected system-management
        // executable appears later in the static command text.
        condition + 1
    };
    let Some(target) = words.get(command_start) else {
        return false;
    };
    if windows_system_management_program_token(target) {
        return true;
    }
    let target = target.rsplit(['\\', '/']).next().unwrap_or(target);
    if matches!(target.to_ascii_lowercase().as_str(), "cmd" | "cmd.exe")
        && words
            .get(command_start + 1)
            .is_some_and(|switch| matches!(switch.to_ascii_lowercase().as_str(), "/c" | "/k"))
    {
        let inner = words[command_start + 2..].join(" ");
        return !inner.is_empty() && cmd_invocation_requires_review(&inner);
    }
    false
}

fn cmd_if_system_management_target(command: &str) -> bool {
    let mut quoted = false;
    let mut segment = String::new();
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '^' && !quoted {
            segment.push(ch);
            if let Some(escaped) = chars.next() {
                segment.push(escaped);
            }
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            segment.push(ch);
            continue;
        }
        if !quoted && matches!(ch, '&' | '|' | '\r' | '\n') {
            if cmd_if_segment_system_management_target(&segment) {
                return true;
            }
            segment.clear();
            continue;
        }
        segment.push(ch);
    }
    cmd_if_segment_system_management_target(&segment)
}

fn powershell_static_member_is_safe(chars: &[char], operator: usize) -> bool {
    let mut left = operator;
    while left > 0 && chars[left - 1].is_whitespace() {
        left -= 1;
    }
    if left == 0 || chars[left - 1] != ']' {
        return false;
    }
    let mut type_start = left - 1;
    while type_start > 0 && chars[type_start] != '[' {
        type_start -= 1;
    }
    if chars.get(type_start) != Some(&'[') || type_start + 1 >= left - 1 {
        return false;
    }
    let type_name = chars[type_start + 1..left - 1]
        .iter()
        .collect::<String>()
        .trim()
        .to_ascii_lowercase();

    let mut member_start = operator + 2;
    while member_start < chars.len() && chars[member_start].is_whitespace() {
        member_start += 1;
    }
    let mut member_end = member_start;
    while member_end < chars.len()
        && (chars[member_end].is_ascii_alphanumeric() || chars[member_end] == '_')
    {
        member_end += 1;
    }
    let member = chars[member_start..member_end]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();

    matches!(type_name.as_str(), "console" | "system.console")
        && matches!(
            member.as_str(),
            "in" | "out" | "error" | "readline" | "write" | "writeline"
        )
}

fn powershell_console_instance_member_is_safe(
    chars: &[char],
    operator: usize,
    member: &str,
) -> bool {
    let mut left = operator;
    while left > 0 && chars[left - 1].is_whitespace() {
        left -= 1;
    }
    let prefix = chars[..left]
        .iter()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    match member.to_ascii_lowercase().as_str() {
        "readline" | "readtoend" => {
            prefix.ends_with("[console]::in") || prefix.ends_with("[system.console]::in")
        }
        "write" | "writeline" => {
            prefix.ends_with("[console]::out")
                || prefix.ends_with("[system.console]::out")
                || prefix.ends_with("[console]::error")
                || prefix.ends_with("[system.console]::error")
        }
        _ => false,
    }
}

fn powershell_member_mutation_starts(chars: &[char], index: usize) -> bool {
    if chars.get(index) == Some(&'=') {
        return true;
    }
    if matches!(chars.get(index), Some('+' | '-' | '*' | '/' | '%'))
        && chars.get(index + 1) == Some(&'=')
    {
        return true;
    }
    if chars.get(index) == Some(&'?')
        && chars.get(index + 1) == Some(&'?')
        && chars.get(index + 2) == Some(&'=')
    {
        return true;
    }
    matches!(
        (chars.get(index), chars.get(index + 1)),
        (Some('+'), Some('+')) | (Some('-'), Some('-'))
    )
}

fn powershell_subexpression_requires_review(command: &str) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let mut quote = Quote::None;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        match quote {
            Quote::None => match ch {
                '\'' => quote = Quote::Single,
                '"' => quote = Quote::Double,
                '`' => {
                    chars.next();
                }
                '$' if chars.peek() == Some(&'(') => return true,
                _ => {}
            },
            Quote::Single => {
                if ch == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                    } else {
                        quote = Quote::None;
                    }
                }
            }
            Quote::Double => {
                if ch == '`' {
                    chars.next();
                } else if ch == '"' {
                    quote = Quote::None;
                } else if ch == '$' && chars.peek() == Some(&'(') {
                    return true;
                }
            }
        }
    }
    false
}

fn powershell_member_invocation_requires_review(command: &str) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let mut visible = Vec::with_capacity(command.chars().count());
    let mut quote = Quote::None;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        match quote {
            Quote::None => match ch {
                '\'' => {
                    quote = Quote::Single;
                    visible.push(' ');
                }
                '"' => {
                    quote = Quote::Double;
                    visible.push(' ');
                }
                '`' => {
                    visible.push(' ');
                    if chars.next().is_some() {
                        visible.push(' ');
                    }
                }
                _ => visible.push(ch),
            },
            Quote::Single => {
                visible.push(' ');
                if ch == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        visible.push(' ');
                    } else {
                        quote = Quote::None;
                    }
                }
            }
            Quote::Double => {
                visible.push(' ');
                if ch == '`' {
                    if chars.next().is_some() {
                        visible.push(' ');
                    }
                } else if ch == '"' {
                    quote = Quote::None;
                }
            }
        }
    }

    let mut index = 0usize;
    while index < visible.len() {
        if visible[index] == ':'
            && visible.get(index + 1) == Some(&':')
            && !powershell_static_member_is_safe(&visible, index)
        {
            // Static .NET dispatch can select a process target through Process.Start,
            // reflection/Activator, P/Invoke helpers, or equivalent runtime APIs. The
            // only static exception is narrow Console I/O required by public sessions.
            return true;
        }

        if visible[index] == '.' {
            let mut member_start = index + 1;
            while member_start < visible.len() && visible[member_start].is_whitespace() {
                member_start += 1;
            }
            if visible.get(member_start) == Some(&'$') {
                return true;
            }
            let mut member_end = member_start;
            while member_end < visible.len()
                && (visible[member_end].is_ascii_alphanumeric() || visible[member_end] == '_')
            {
                member_end += 1;
            }
            let member = visible[member_start..member_end].iter().collect::<String>();
            if member.eq_ignore_ascii_case("scriptblock") {
                // ScriptBlock is executable code. Exposing it from CommandInfo/function
                // metadata lets later cmdlets execute code without a call operator or an
                // Invoke member, so the code target is no longer statically reviewable.
                return true;
            }
            let mut call = member_end;
            while call < visible.len() && visible[call].is_whitespace() {
                call += 1;
            }
            if powershell_member_mutation_starts(&visible, call) {
                // Member mutation can rewrite command-engine state after target review
                // (for example InvokeCommand.CommandNotFoundAction). Fail closed for the
                // mutation grammar instead of enumerating engine property names.
                return true;
            }
            if call < visible.len()
                && visible[call] == '('
                && !powershell_console_instance_member_is_safe(&visible, index, &member)
            {
                // Instance-member invocation is a dynamic dispatch surface. The runtime
                // type and selected implementation cannot be proven from the public
                // request; fail closed by grammar instead of method-name deny lists. The
                // only instance-call exception is the statically rooted Console I/O chain
                // required by LocalBridge's public session protocol.
                return true;
            }
        }
        index += 1;
    }
    false
}

fn flush_powershell_review_word(word: &mut String) -> bool {
    if word.is_empty() {
        return false;
    }
    let requires_review = powershell_review_word(word);
    word.clear();
    requires_review
}

fn flush_cmd_review_word(word: &mut String) -> bool {
    let lower = word.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "set" | "copy" | "move" | "ren" | "rename" | "rmdir" | "rd"
    ) {
        word.clear();
        return false;
    }
    let requires_review = cmd_review_word(word);
    word.clear();
    requires_review
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

fn powershell_readonly_identity_diagnostic(command: &str) -> bool {
    let compact = command
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    compact.eq_ignore_ascii_case("[System.Security.Principal.WindowsIdentity]::GetCurrent()")
        || compact.eq_ignore_ascii_case("[WindowsIdentity]::GetCurrent()")
}

fn powershell_provider_target(token: &str) -> bool {
    let lower = token.trim_matches(['\'', '"']).to_ascii_lowercase();
    [
        "alias:",
        "function:",
        "variable:",
        "env:",
        "registry::",
        "hklm:",
        "hkcu:",
        "hkcr:",
        "hku:",
        "hkcc:",
        "cert:",
        "wsman:",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
}

fn powershell_ordinary_development_invocation(command: &str) -> bool {
    let Some(words) = simple_static_command_words(command) else {
        return false;
    };
    let verb = words[0].to_ascii_lowercase();
    if !matches!(
        verb.as_str(),
        "set-variable" | "set-content" | "new-item" | "copy-item" | "move-item" | "remove-item"
    ) {
        return false;
    }
    if words
        .iter()
        .skip(1)
        .any(|word| powershell_provider_target(word))
    {
        return false;
    }
    if verb == "set-variable"
        && words
            .iter()
            .any(|word| word.eq_ignore_ascii_case("PSModuleAutoLoadingPreference"))
    {
        return false;
    }
    true
}

fn powershell_simple_get_command_diagnostic(command: &str) -> bool {
    if command
        .chars()
        .any(|ch| ch.is_whitespace() && !matches!(ch, ' ' | '\t'))
    {
        return false;
    }
    let mut words = command.split_ascii_whitespace();
    let Some(verb) = words.next() else {
        return false;
    };
    if !verb.eq_ignore_ascii_case("get-command") && !verb.eq_ignore_ascii_case("gcm") {
        return false;
    }
    let Some(target) = words.next() else {
        return false;
    };
    if words.next().is_some() {
        return false;
    }
    !target.is_empty()
        && target
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn powershell_readonly_command_discovery(command: &str) -> bool {
    let mut segments = command.split('|').map(str::trim);
    let Some(discovery) = segments.next() else {
        return false;
    };
    let Some(projection) = segments.next() else {
        return false;
    };
    if segments.next().is_some() || !powershell_simple_get_command_diagnostic(discovery) {
        return false;
    }
    let mut words = projection.split_ascii_whitespace();
    let Some(select) = words.next() else {
        return false;
    };
    if !select.eq_ignore_ascii_case("select-object") && !select.eq_ignore_ascii_case("select") {
        return false;
    }
    let Some(expand) = words.next() else {
        return false;
    };
    if !expand.eq_ignore_ascii_case("-expandproperty") {
        return false;
    }
    let Some(property) = words.next() else {
        return false;
    };
    if words.next().is_some() {
        return false;
    }
    matches!(
        property.to_ascii_lowercase().as_str(),
        "source" | "path" | "name" | "commandtype"
    )
}

fn powershell_readonly_version_diagnostic(command: &str) -> bool {
    let command = command.trim();
    command.eq_ignore_ascii_case("$PSVersionTable.PSVersion")
        || command.eq_ignore_ascii_case("$PSVersionTable.PSVersion.ToString()")
}

fn powershell_invocation_requires_review(command: &str) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    // Static command discovery is data inspection, not execution of the discovered target.
    // Keep the exception deliberately narrow: only one literal target and an optional
    // Select-Object projection of non-executable metadata are accepted.
    if powershell_simple_get_command_diagnostic(command)
        || powershell_readonly_command_discovery(command)
        || powershell_readonly_version_diagnostic(command)
        || powershell_readonly_identity_diagnostic(command)
        || powershell_ordinary_development_invocation(command)
    {
        return false;
    }

    if let Some(inner) = static_nested_cmd_inner(command) {
        let body = nested_cmd_body(inner);
        if (inner.trim().starts_with('"') && inner.trim().ends_with('"'))
            || !command
                .chars()
                .any(|ch| matches!(ch, ';' | '|' | '&' | '\r' | '\n' | '{' | '}'))
        {
            return cmd_invocation_requires_review(body);
        }
    }

    if powershell_static_system_management_target(command)
        || powershell_subexpression_requires_review(command)
        || powershell_member_invocation_requires_review(command)
    {
        return true;
    }

    let mut quote = Quote::None;
    let mut chars = command.chars().peekable();
    let mut word = String::new();
    let mut command_boundary = true;
    let mut word_is_command = false;
    while let Some(ch) = chars.next() {
        match quote {
            Quote::Single => {
                if ch == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                    } else {
                        quote = Quote::None;
                    }
                }
                continue;
            }
            Quote::Double => {
                if ch == '`' {
                    chars.next();
                } else if ch == '"' {
                    quote = Quote::None;
                }
                continue;
            }
            Quote::None => {}
        }

        if ch == '\'' {
            if word_is_command && flush_powershell_review_word(&mut word) {
                return true;
            }
            word.clear();
            word_is_command = false;
            quote = Quote::Single;
            continue;
        }
        if ch == '"' {
            if word_is_command && flush_powershell_review_word(&mut word) {
                return true;
            }
            word.clear();
            word_is_command = false;
            quote = Quote::Double;
            continue;
        }
        if ch == '`' {
            return true;
        }
        if ch == '&' {
            if word_is_command && flush_powershell_review_word(&mut word) {
                return true;
            }
            word.clear();
            word_is_command = false;
            if chars.peek() == Some(&'&') {
                chars.next();
                command_boundary = true;
                continue;
            }
            return true;
        }
        if ch == '.' && command_boundary {
            return true;
        }
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            if word.is_empty() {
                word_is_command = command_boundary;
            }
            word.push(ch);
            command_boundary = false;
            continue;
        }
        if word_is_command && flush_powershell_review_word(&mut word) {
            return true;
        }
        word.clear();
        word_is_command = false;
        if matches!(ch, ';' | '|' | '\n' | '\r' | '{' | '}') {
            command_boundary = true;
        }
    }
    word_is_command && flush_powershell_review_word(&mut word)
}

fn cmd_chained_literal_script_requires_review(command: &str) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in command.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '^' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted || !matches!(ch, '&' | '|') {
            continue;
        }
        let mut start = index + ch.len_utf8();
        while command[start..]
            .chars()
            .next()
            .is_some_and(|next| next.is_whitespace() || matches!(next, '&' | '|'))
        {
            start += command[start..].chars().next().unwrap().len_utf8();
        }
        if start < command.len()
            && static_workspace_script_invocation("cmd", &command[start..]).is_some()
        {
            return true;
        }
    }
    false
}

fn cmd_chained_nested_shell_requires_review(command: &str) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in command.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '^' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted || !matches!(ch, '&' | '|' | '\r' | '\n' | '(') {
            continue;
        }
        let mut start = index + ch.len_utf8();
        while command[start..]
            .chars()
            .next()
            .is_some_and(|next| next.is_whitespace() || matches!(next, '&' | '|' | '('))
        {
            start += command[start..].chars().next().unwrap().len_utf8();
        }
        if start < command.len() {
            if let Some(inner) = static_nested_cmd_inner(&command[start..]) {
                if cmd_invocation_requires_review(nested_cmd_body(inner)) {
                    return true;
                }
            }
        }
    }
    false
}

fn cmd_invocation_requires_review(command: &str) -> bool {
    if let Some(inner) = static_nested_cmd_inner(command) {
        return cmd_invocation_requires_review(nested_cmd_body(inner));
    }
    if cmd_static_system_management_target(command) || cmd_if_system_management_target(command) {
        return true;
    }
    if cmd_chained_literal_script_requires_review(command) {
        return true;
    }
    if cmd_chained_nested_shell_requires_review(command) {
        return true;
    }
    let mut chars = command.chars().peekable();
    let mut word = String::new();
    let mut command_boundary = true;
    let mut word_is_command = false;
    let mut control_flow_command = false;
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if matches!(ch, '%' | '!') {
            // Expansion is authority-sensitive when it can construct the command target.
            // Ordinary data arguments such as `echo %PATH%` remain Full-mode diagnostics.
            if command_boundary || control_flow_command {
                return true;
            }
            continue;
        }
        if ch == '^' {
            if let Some(escaped) = chars.next() {
                if escaped.is_ascii_alphanumeric() || matches!(escaped, '_' | '-' | '.') {
                    if word.is_empty() {
                        word_is_command = command_boundary;
                    }
                    word.push(escaped);
                    command_boundary = false;
                } else if word_is_command && flush_cmd_review_word(&mut word) {
                    return true;
                }
            }
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            if word_is_command && flush_cmd_review_word(&mut word) {
                return true;
            }
            word.clear();
            word_is_command = false;
            continue;
        }
        if quoted {
            continue;
        }
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            if word.is_empty() {
                word_is_command = command_boundary;
            }
            word.push(ch);
            command_boundary = false;
        } else {
            if !word.is_empty() && word_is_command {
                let lower = word.to_ascii_lowercase();
                control_flow_command = matches!(lower.as_str(), "if" | "for");
                if flush_cmd_review_word(&mut word) {
                    return true;
                }
            } else {
                word.clear();
            }
            word_is_command = false;
            if matches!(ch, '&' | '|' | '\n' | '\r' | '(') {
                command_boundary = true;
                control_flow_command = false;
            }
        }
    }
    word_is_command && flush_cmd_review_word(&mut word)
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
    if document.schema_version != 6
        || document.runtime_version != PINNED_RUNTIME_VERSION
        || document.status != "LB_007_STABLE_PUBLIC_POLICY"
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
        || document.enforcement.transitive_exec_classification != "required"
        || document.enforcement.shell_invocation_review != "static_target_fail_closed"
        || document.enforcement.unreviewable_shell_indirection != "review_required"
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

#[cfg(test)]
mod schema36_shell_classifier_tests {
    use super::*;

    #[test]
    fn full_style_diagnostics_are_not_privileged_by_argument_tokens() {
        for command in ["where cmd", "where pwsh", "echo %PATH%", "echo %TEMP%"] {
            assert!(
                !shell_invocation_requires_review("cmd", command),
                "{command}"
            );
        }
    }

    #[test]
    fn command_position_and_dynamic_control_flow_remain_fail_closed() {
        for command in [
            "pwsh -NoProfile -Command whoami",
            "%COMSPEC% /c whoami",
            "if 1==1 %COMSPEC% /c whoami",
        ] {
            assert!(
                shell_invocation_requires_review("cmd", command),
                "{command}"
            );
        }
        for command in ["cmd /c echo nested", "echo ok && cmd /c whoami"] {
            assert!(
                !shell_invocation_requires_review("cmd", command),
                "{command}"
            );
        }
        for command in [
            "echo ok && cmd /c sc.exe query",
            "if 1==1 cmd /c net.exe user",
        ] {
            assert!(
                shell_invocation_requires_review("cmd", command),
                "{command}"
            );
        }
    }

    #[test]
    fn powershell_literal_command_discovery_allows_safe_projection_only() {
        assert!(!shell_invocation_requires_review(
            "windows_powershell",
            "Get-Command cmd | Select-Object -ExpandProperty Source"
        ));
        assert!(!shell_invocation_requires_review(
            "windows_powershell",
            "gcm git | select -ExpandProperty Path"
        ));
        assert!(shell_invocation_requires_review(
            "windows_powershell",
            "Get-Command cmd | ForEach-Object { & $_.Source }"
        ));
        assert!(shell_invocation_requires_review(
            "windows_powershell",
            "Get-Command cmd | Select-Object -ExpandProperty ScriptBlock"
        ));
    }

    #[test]
    fn powershell_version_probe_is_read_only_but_suffixes_remain_reviewed() {
        for command in [
            "$PSVersionTable.PSVersion",
            "$PSVersionTable.PSVersion.ToString()",
            "  $psversiontable.psversion.tostring()  ",
        ] {
            assert!(
                !shell_invocation_requires_review("pwsh", command),
                "{command}"
            );
        }
        for command in [
            "$PSVersionTable.PSVersion.ToString(); Start-Process cmd",
            "$PSVersionTable.PSVersion.ToString() | ForEach-Object { & cmd }",
            "$env:COMSPEC.ToString()",
        ] {
            assert!(
                shell_invocation_requires_review("pwsh", command),
                "{command}"
            );
        }
    }
}
