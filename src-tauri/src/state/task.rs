pub use crate::domain::{SafeTaskSummary, TaskKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskExecutionState {
    Idle,
    Running,
    AwaitingAuthorization,
    Blocked,
    Failed,
    Cancelled,
}

impl TaskExecutionState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Blocked | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Presentation payload only; authoritative tasks are stored by TaskRegistry with TaskId.
pub struct CurrentTask {
    pub kind: TaskKind,
    pub summary: SafeTaskSummary,
    pub state: TaskExecutionState,
}

impl CurrentTask {
    pub fn new(
        kind: TaskKind,
        summary: SafeTaskSummary,
        state: TaskExecutionState,
    ) -> Result<Self, CurrentTaskContractError> {
        if state == TaskExecutionState::Idle {
            return Err(CurrentTaskContractError::ActiveTaskCannotBeIdle);
        }
        Ok(Self {
            kind,
            summary,
            state,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CurrentTaskStatus {
    #[default]
    Idle,
    Active(CurrentTask),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastToolTiming {
    pub kind: TaskKind,
    pub summary: SafeTaskSummary,
    pub age_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentTaskTiming {
    pub status: CurrentTaskStatus,
    pub elapsed_ms: Option<u64>,
    pub last_tool: Option<LastToolTiming>,
}

impl Default for CurrentTaskTiming {
    fn default() -> Self {
        Self {
            status: CurrentTaskStatus::Idle,
            elapsed_ms: None,
            last_tool: None,
        }
    }
}

impl CurrentTaskStatus {
    pub fn project(
        kind: TaskKind,
        summary: SafeTaskSummary,
        state: TaskExecutionState,
    ) -> Result<Self, CurrentTaskContractError> {
        if state == TaskExecutionState::Idle {
            return Ok(Self::Idle);
        }
        Ok(Self::Active(CurrentTask::new(kind, summary, state)?))
    }

    pub fn start(kind: TaskKind, raw_summary: &str) -> Self {
        Self::Active(CurrentTask {
            kind,
            summary: SafeTaskSummary::from_untrusted(raw_summary),
            state: TaskExecutionState::Running,
        })
    }

    pub fn set_state(&mut self, state: TaskExecutionState) -> Result<(), CurrentTaskContractError> {
        match state {
            TaskExecutionState::Idle => {
                *self = Self::Idle;
                Ok(())
            }
            _ => match self {
                Self::Idle => Err(CurrentTaskContractError::NoActiveTask),
                Self::Active(task) if task.state.is_terminal() => {
                    Err(CurrentTaskContractError::InvalidStateTransition {
                        from: task.state,
                        to: state,
                    })
                }
                Self::Active(task) => {
                    task.state = state;
                    Ok(())
                }
            },
        }
    }

    pub fn clear(&mut self) {
        *self = Self::Idle;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentTaskContractError {
    ActiveTaskCannotBeIdle,
    NoActiveTask,
    InvalidStateTransition {
        from: TaskExecutionState,
        to: TaskExecutionState,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_required_execution_states_are_projectable() {
        assert_eq!(CurrentTaskStatus::default(), CurrentTaskStatus::Idle);
        for state in [
            TaskExecutionState::Running,
            TaskExecutionState::AwaitingAuthorization,
            TaskExecutionState::Blocked,
            TaskExecutionState::Failed,
            TaskExecutionState::Cancelled,
        ] {
            let projected = CurrentTaskStatus::project(
                TaskKind::Other,
                SafeTaskSummary::from_untrusted("safe operation"),
                state,
            )
            .unwrap();
            assert!(matches!(projected, CurrentTaskStatus::Active(_)));
        }
    }

    #[test]
    fn task_state_transitions_do_not_create_history() {
        let mut current = CurrentTaskStatus::start(TaskKind::Test, "cargo test");
        current.set_state(TaskExecutionState::Failed).unwrap();
        assert!(matches!(
            current,
            CurrentTaskStatus::Active(CurrentTask {
                state: TaskExecutionState::Failed,
                ..
            })
        ));
        current.clear();
        assert_eq!(current, CurrentTaskStatus::Idle);
    }

    #[test]
    fn secret_like_summary_is_omitted_and_long_summary_is_bounded() {
        assert_eq!(
            SafeTaskSummary::from_untrusted("Authorization: Bearer hidden-value"),
            SafeTaskSummary::Omitted
        );
        for raw in [
            "token=hidden-value",
            "password = hidden-value",
            "?access_token=hidden-value",
            "{\"refresh_token\":\"hidden-value\"}",
            "{\"access_token\":\"hidden-value\"}",
            "ACCESS_TOKEN : hidden-value",
            "--api-key=hidden-value",
            "--password hidden-value",
            "--passphrase hidden-value",
            "authorization = Bearer hidden-value",
            "client_secret:hidden-value",
            "nonce = hidden-value",
        ] {
            assert_eq!(
                SafeTaskSummary::from_untrusted(raw),
                SafeTaskSummary::Omitted,
                "secret-bearing task summary was not omitted: {raw}"
            );
        }
        assert_eq!(
            SafeTaskSummary::from_untrusted("read src/tokenizer.rs"),
            SafeTaskSummary::Text("read src/tokenizer.rs".to_string())
        );
        let long = "a".repeat(400);
        let summary = SafeTaskSummary::from_untrusted(&long);
        let value = summary.as_deref().unwrap();
        assert!(value.chars().count() <= 161);
        assert!(value.ends_with('…'));
    }

    #[test]
    fn terminal_task_states_cannot_resurrect_but_can_clear_to_idle() {
        for terminal in [
            TaskExecutionState::Blocked,
            TaskExecutionState::Failed,
            TaskExecutionState::Cancelled,
        ] {
            for next in [
                TaskExecutionState::Running,
                TaskExecutionState::AwaitingAuthorization,
            ] {
                let mut current = CurrentTaskStatus::project(
                    TaskKind::Other,
                    SafeTaskSummary::from_untrusted("safe operation"),
                    terminal,
                )
                .unwrap();
                assert_eq!(
                    current.set_state(next),
                    Err(CurrentTaskContractError::InvalidStateTransition {
                        from: terminal,
                        to: next,
                    })
                );
                assert!(matches!(
                    current,
                    CurrentTaskStatus::Active(CurrentTask { state, .. }) if state == terminal
                ));
                current.set_state(TaskExecutionState::Idle).unwrap();
                assert_eq!(current, CurrentTaskStatus::Idle);
            }
        }
    }

    #[test]
    fn stable_task_kinds_are_semantic_not_upstream_tool_identifiers() {
        let kinds = [
            TaskKind::ReadFile,
            TaskKind::SearchCode,
            TaskKind::ModifyFile,
            TaskKind::ExecuteCommand,
            TaskKind::GitOperation,
            TaskKind::Build,
            TaskKind::Test,
            TaskKind::ElevatedOperation,
            TaskKind::Other,
        ];
        let names = kinds.map(|kind| format!("{kind:?}"));
        assert_eq!(
            names,
            [
                "ReadFile",
                "SearchCode",
                "ModifyFile",
                "ExecuteCommand",
                "GitOperation",
                "Build",
                "Test",
                "ElevatedOperation",
                "Other",
            ]
        );
    }
}
