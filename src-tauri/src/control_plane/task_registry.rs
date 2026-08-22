use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{
    LifecycleState, McpSessionId, RequestKey, SafeTaskSummary, TaskId, TaskKind, TaskRecord,
    TerminalOutcome,
};

const MAX_RETAINED_TASKS: usize = 256;

static TASK_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskTransitionError {
    UnknownTask(TaskId),
    AlreadyTerminal {
        task_id: TaskId,
        outcome: TerminalOutcome,
    },
}

#[derive(Debug, Default)]
struct TaskRegistryState {
    tasks: HashMap<TaskId, TaskRecord>,
    order: VecDeque<TaskId>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TaskRegistry(Arc<Mutex<TaskRegistryState>>);

impl TaskRegistry {
    pub(crate) fn queue(
        &self,
        owner_session: McpSessionId,
        request: RequestKey,
        kind: TaskKind,
        summary: SafeTaskSummary,
    ) -> TaskId {
        let now = now_unix_ms();
        let task_id = next_task_id();
        let record = TaskRecord {
            id: task_id.clone(),
            owner_session,
            request,
            kind,
            summary,
            lifecycle: LifecycleState::Queued,
            created_at_ms: now,
            updated_at_ms: now,
            error: None,
        };
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.order.push_back(task_id.clone());
        state.tasks.insert(task_id.clone(), record);
        trim_terminal_history(&mut state);
        task_id
    }

    pub(crate) fn mark_running(&self, task_id: &TaskId) -> Result<(), TaskTransitionError> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let task = state
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| TaskTransitionError::UnknownTask(task_id.clone()))?;
        match task.lifecycle {
            LifecycleState::Queued => {
                task.lifecycle = LifecycleState::Running;
                task.updated_at_ms = now_unix_ms();
                Ok(())
            }
            LifecycleState::Running => Ok(()),
            LifecycleState::Terminal(outcome) => Err(TaskTransitionError::AlreadyTerminal {
                task_id: task_id.clone(),
                outcome,
            }),
        }
    }

    pub(crate) fn finish(
        &self,
        task_id: &TaskId,
        outcome: TerminalOutcome,
    ) -> Result<(), TaskTransitionError> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let task = state
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| TaskTransitionError::UnknownTask(task_id.clone()))?;
        match task.lifecycle {
            LifecycleState::Queued | LifecycleState::Running => {
                task.lifecycle = LifecycleState::Terminal(outcome);
                task.updated_at_ms = now_unix_ms();
                trim_terminal_history(&mut state);
                Ok(())
            }
            LifecycleState::Terminal(existing) if existing == outcome => Ok(()),
            LifecycleState::Terminal(existing) => Err(TaskTransitionError::AlreadyTerminal {
                task_id: task_id.clone(),
                outcome: existing,
            }),
        }
    }

    pub(crate) fn finish_with_error(
        &self,
        task_id: &TaskId,
        outcome: TerminalOutcome,
        error: crate::domain::OperationError,
    ) -> Result<(), TaskTransitionError> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let task = state
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| TaskTransitionError::UnknownTask(task_id.clone()))?;
        match task.lifecycle {
            LifecycleState::Queued | LifecycleState::Running => {
                task.lifecycle = LifecycleState::Terminal(outcome);
                task.updated_at_ms = now_unix_ms();
                task.error = Some(error.for_task(task_id.clone()));
                trim_terminal_history(&mut state);
                Ok(())
            }
            LifecycleState::Terminal(existing) if existing == outcome => Ok(()),
            LifecycleState::Terminal(existing) => Err(TaskTransitionError::AlreadyTerminal {
                task_id: task_id.clone(),
                outcome: existing,
            }),
        }
    }

    pub(crate) fn get(&self, task_id: &TaskId) -> Option<TaskRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tasks
            .get(task_id)
            .cloned()
    }

    pub(crate) fn active_owned_by(&self, owner: &McpSessionId) -> Vec<TaskRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tasks
            .values()
            .filter(|task| &task.owner_session == owner && !task.lifecycle.is_terminal())
            .cloned()
            .collect()
    }

    pub(crate) fn owned_by(&self, owner: &McpSessionId) -> Vec<TaskRecord> {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .order
            .iter()
            .filter_map(|task_id| state.tasks.get(task_id))
            .filter(|task| &task.owner_session == owner)
            .cloned()
            .collect()
    }

    pub(crate) fn latest_active(&self) -> Option<TaskRecord> {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for lifecycle in [LifecycleState::Running, LifecycleState::Queued] {
            if let Some(task) = state.order.iter().rev().find_map(|task_id| {
                state
                    .tasks
                    .get(task_id)
                    .filter(|task| task.lifecycle == lifecycle)
                    .cloned()
            }) {
                return Some(task);
            }
        }
        None
    }

    pub(crate) fn latest_terminal(&self) -> Option<TaskRecord> {
        self.latest_terminal_excluding(&HashSet::new())
    }

    pub(crate) fn latest_terminal_excluding(
        &self,
        excluded: &HashSet<TaskId>,
    ) -> Option<TaskRecord> {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.order.iter().rev().find_map(|task_id| {
            state
                .tasks
                .get(task_id)
                .filter(|task| task.lifecycle.is_terminal() && !excluded.contains(&task.id))
                .cloned()
        })
    }
}

fn trim_terminal_history(state: &mut TaskRegistryState) {
    while state.tasks.len() > MAX_RETAINED_TASKS {
        let Some(index) = state.order.iter().position(|task_id| {
            state
                .tasks
                .get(task_id)
                .is_some_and(|task| task.lifecycle.is_terminal())
        }) else {
            break;
        };
        if let Some(task_id) = state.order.remove(index) {
            state.tasks.remove(&task_id);
        }
    }
}

fn next_task_id() -> TaskId {
    let generation = TASK_GENERATION.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    TaskId::new(format!(
        "lb-task-{:x}-{now:x}-{generation:x}",
        std::process::id()
    ))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::RpcRequestId;

    fn queue_task(registry: &TaskRegistry) -> TaskId {
        let session = McpSessionId::new("session-a");
        registry.queue(
            session.clone(),
            RequestKey::new(session, RpcRequestId::Number(1)),
            TaskKind::Test,
            SafeTaskSummary::from_untrusted("cargo test"),
        )
    }

    #[test]
    fn task_has_stable_identity_and_exactly_one_terminal_outcome() {
        let registry = TaskRegistry::default();
        let task_id = queue_task(&registry);
        registry.mark_running(&task_id).unwrap();
        registry
            .finish(&task_id, TerminalOutcome::Completed)
            .unwrap();
        registry
            .finish(&task_id, TerminalOutcome::Completed)
            .unwrap();
        assert!(matches!(
            registry.get(&task_id).unwrap().lifecycle,
            LifecycleState::Terminal(TerminalOutcome::Completed)
        ));
        assert_eq!(
            registry.finish(&task_id, TerminalOutcome::Failed),
            Err(TaskTransitionError::AlreadyTerminal {
                task_id,
                outcome: TerminalOutcome::Completed,
            })
        );
    }

    #[test]
    fn terminal_task_cannot_resurrect() {
        let registry = TaskRegistry::default();
        let task_id = queue_task(&registry);
        registry
            .finish(&task_id, TerminalOutcome::Cancelled)
            .unwrap();
        assert_eq!(
            registry.mark_running(&task_id),
            Err(TaskTransitionError::AlreadyTerminal {
                task_id,
                outcome: TerminalOutcome::Cancelled,
            })
        );
    }

    #[test]
    fn failed_task_retains_typed_operation_error() {
        let registry = TaskRegistry::default();
        let task_id = queue_task(&registry);
        registry
            .finish_with_error(
                &task_id,
                TerminalOutcome::Failed,
                crate::domain::OperationError::new(
                    "Task.Failed",
                    crate::domain::ErrorCategory::Internal,
                    "task failed",
                    false,
                ),
            )
            .unwrap();
        let task = registry.get(&task_id).unwrap();
        assert_eq!(task.error.unwrap().task_id, Some(task_id));
    }
}
