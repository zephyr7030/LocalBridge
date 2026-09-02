mod bundle;
mod client_auth;
mod driver;
mod facade;
mod guard;
mod http;
mod observation;
mod public_contract;
mod resource_access;
mod runtime;
mod server;
#[cfg(all(test, windows))]
mod test_support;

pub use driver::{ProductionRuntimeConfig, ProductionRuntimeDriver};
pub use facade::{
    AGENT_API_VERSION, AgentFacade, CodingRuntimeHealth, CodingRuntimeHealthState,
    CodingToolsRuntimeAdapter, CommandControlAction, FacadeCallError, FacadeDenied, FacadeError,
    FacadeErrorCode, GitWorkflowAction, ShellCommandRequest, ToolRegistry, V1_CORE_TOOL_NAMES,
    WorkspaceRuntimeAdapter, validate_runtime_capabilities,
};
pub use guard::{GuardError, GuardRuntime, McpGuard, PolicyDenied, ToolCallRequest};
pub use runtime::{
    CodingToolsPermissionMode, CodingToolsRuntime, CodingToolsRuntimeConfig,
    CodingToolsRuntimeError, InternalBearer,
};
pub use server::{
    CurrentTaskProjection, CurrentTaskWake, PolicyEnforcementError, PolicyEnforcementRuntime,
};
