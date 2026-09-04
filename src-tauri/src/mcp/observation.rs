use serde_json::{Value, json};

use crate::execution::CapabilityPolicy;
use crate::state::{PermissionMode, RuntimeState};

use super::facade::{
    AGENT_API_REVISION, AGENT_API_VERSION, FacadeError, FacadeErrorCode, V1_CORE_TOOL_NAMES,
    stable_success,
};

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceObservationSeed {
    pub workspace: String,
    pub default_cwd: String,
    pub project_discovery: Value,
    pub runtime_discovery: Value,
}

pub(crate) fn render_workspace_context(
    seed: &WorkspaceObservationSeed,
    policy: &CapabilityPolicy,
    mode: PermissionMode,
    runtime: Option<&RuntimeState>,
    arguments: &Value,
) -> Result<Value, FacadeError> {
    let object = arguments.as_object().ok_or_else(invalid_argument)?;
    if object.keys().any(|key| key != "detail") {
        return Err(invalid_argument());
    }
    let detail = match object.get("detail") {
        None => "compact",
        Some(Value::String(value)) if matches!(value.as_str(), "compact" | "full") => value,
        _ => return Err(invalid_argument()),
    };
    let (runtime_state, root_process_alive, authenticated_mcp, fault) = match runtime {
        Some(RuntimeState::Ready) => ("ready", true, true, Value::Null),
        Some(RuntimeState::Faulted(fault)) => (
            "fault",
            false,
            false,
            serde_json::to_value(fault).unwrap_or(Value::Null),
        ),
        Some(RuntimeState::Stopped) | None => ("unavailable", false, false, Value::Null),
        Some(_) => ("recovering", false, false, Value::Null),
    };
    let policy_allowed_tools = V1_CORE_TOOL_NAMES
        .iter()
        .filter(|name| policy.public_tool_allowed_in_mode(mode, name))
        .copied()
        .collect::<Vec<_>>();
    let mut public_tools = V1_CORE_TOOL_NAMES.to_vec();
    public_tools.push("elevated_exec");
    let shells = seed
        .runtime_discovery
        .get("shells")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut data = json!({
        "api_version": AGENT_API_VERSION,
        "facade_revision": AGENT_API_REVISION,
        "workspace": seed.workspace,
        "default_cwd": seed.default_cwd,
        "runtime": runtime_state,
        "runtime_health": {
            "root_process_alive": root_process_alive,
            "authenticated_mcp": authenticated_mcp,
            "fault": fault,
        },
        "coding_profile": "coding-agent-v1",
        "shell_discovery": shells,
        "capabilities": {
            "public_tools": public_tools,
            "policy_allowed_tools": policy_allowed_tools,
            "tool_schema_projection": "stable",
            "shells": seed.runtime_discovery.get("shells").cloned().unwrap_or_else(|| json!({})),
            "git": seed.runtime_discovery.get("git").cloned().unwrap_or_else(|| json!({"available":false})),
            "bundled_python": seed.runtime_discovery.get("bundled_python").cloned().unwrap_or_else(|| json!({"available":false})),
            "bundled_node": seed.runtime_discovery.get("bundled_node").cloned().unwrap_or_else(|| json!({"available":false})),
            "elevated_route": {"available":false,"reason":"broker_state_required"},
        },
        "current_task": {"state":"idle"},
        "detail": detail,
    });
    if let (Some(target), Some(discovery)) =
        (data.as_object_mut(), seed.project_discovery.as_object())
    {
        for (key, value) in discovery {
            target.insert(key.clone(), value.clone());
        }
    }
    if detail == "full" {
        data.as_object_mut()
            .expect("workspace context object")
            .insert(
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
    Ok(stable_success(data, "LocalBridge workspace context ready"))
}

fn invalid_argument() -> FacadeError {
    FacadeError::new(FacadeErrorCode::InvalidArgument, "参数无效", false)
}
