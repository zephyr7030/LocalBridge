mod bundle;
mod context_service;
mod edit_service;
pub(crate) mod filesystem_service;
mod facade;
mod git_adapter;
mod guard;
mod http;
mod path_authority;
mod policy;
mod runtime;
mod server;
mod shell;
mod task_state;
mod toolbox;
mod verification_planner;
mod workflow_checkpoint;

pub use facade::{
    AGENT_API_VERSION, AgentFacade, CodingRuntimeHealth, CodingRuntimeHealthState,
    CodingToolsRuntimeAdapter, CommandControlAction, FacadeCallError, FacadeDenied, FacadeError,
    FacadeErrorCode, GitWorkflowAction,
    ShellCommandRequest, ToolRegistry, V1_CORE_TOOL_NAMES, WorkspaceRuntimeAdapter,
    validate_runtime_capabilities,
};
pub use guard::{GuardError, GuardRuntime, McpGuard, PolicyDenied, ToolCallRequest};
pub use path_authority::{PathAuthority, PathAuthorityError, PathAuthorityScope};
pub use policy::{
    CapabilityPolicy, DenyReason, PolicyDecision, PolicyError, PublicActionDescriptor,
    PublicCapabilityDeclaration, ToolDescriptor, reviewed_elevated_program,
};
pub use runtime::{
    CodingToolsPermissionMode, CodingToolsRuntime, CodingToolsRuntimeConfig,
    CodingToolsRuntimeError, InternalBearer,
};
pub use server::{
    CurrentTaskProjection, CurrentTaskWake, PolicyEnforcementError, PolicyEnforcementRuntime,
};
pub use shell::{
    DirectProcessExecutor, DirectProcessSpec, ResolvedShell, ResolvedShellKind, SemanticVersion,
    ShellDiscovery, ShellExecutionError, ShellExecutionSpec, ShellExecutor, ShellResolveError,
    ShellResolver, ShellSelector, ShellVersionProbe, SystemShellDiscovery, SystemShellVersionProbe,
};
