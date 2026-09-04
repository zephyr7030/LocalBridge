use std::path::Path;

use serde_json::{Value, json};

use crate::control_plane::execution_registry::ExecutionRegistry;
use crate::control_plane::task_registry::TaskRegistry;
use crate::control_plane::workflow_checkpoint::WorkflowCheckpointStore;
use crate::domain::{McpSessionId, TaskId};

use super::facade::{FacadeError, FacadeErrorCode};

/// Resolve a public task against its owner before exposing any associated resource.
/// Durable workflow state comes from its checkpoint owner, never the completed prepare request.
pub(crate) fn owned_task_detail(
    task_id: &TaskId,
    owner: &McpSessionId,
    workspace: &Path,
    tasks: &TaskRegistry,
    executions: &ExecutionRegistry,
) -> Result<Value, FacadeError> {
    let checkpoint = WorkflowCheckpointStore::for_workspace(workspace)
        .and_then(|store| store.load::<Value>())
        .map_err(|_| {
            FacadeError::new(
                FacadeErrorCode::RuntimeUnavailable,
                "durable task ownership is unavailable",
                true,
            )
        })?;
    let related = executions
        .all()
        .into_iter()
        .filter(|execution| {
            &execution.task_id == task_id && execution.owner_session.as_ref() == Some(owner)
        })
        .collect::<Vec<_>>();
    if let Some(checkpoint) =
        checkpoint.filter(|checkpoint| checkpoint.workflow_id == task_id.as_str())
    {
        if checkpoint.owner_session_id.as_deref() != Some(owner.as_str()) {
            return Err(task_not_owned());
        }
        return Ok(json!({
            "state":if checkpoint.completed { "idle" } else { "active" },
            "task":{
                "id":checkpoint.workflow_id,
                "owner_session":checkpoint.owner_session_id,
                "kind":if checkpoint.is_coding_task() { "coding_workflow" } else { "workflow" },
                "completed":checkpoint.completed,
                "current_step":checkpoint.current_step,
                "next_step":checkpoint.next_step,
                "failure":checkpoint.failure
            },
            "executions":related
        }));
    }
    let task = tasks.get(task_id);
    let owned_task = task.as_ref().filter(|task| &task.owner_session == owner);
    if owned_task.is_none() && related.is_empty() {
        return Err(if task.is_some() {
            task_not_owned()
        } else {
            FacadeError::new(
                FacadeErrorCode::NotFound,
                "task is unavailable or reaped",
                false,
            )
        });
    }
    let active = owned_task.is_some_and(|task| !task.lifecycle.is_terminal())
        || related
            .iter()
            .any(|execution| !execution.state.is_terminal());
    Ok(json!({
        "state":if active { "active" } else { "idle" },
        "task":owned_task,
        "executions":related
    }))
}

fn task_not_owned() -> FacadeError {
    FacadeError::new(
        FacadeErrorCode::TaskNotOwned,
        "task is not owned by the current MCP session",
        false,
    )
}
