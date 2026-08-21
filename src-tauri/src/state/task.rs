const MAX_SAFE_SUMMARY_CHARS: usize = 160;
const SENSITIVE_MARKERS: &[&str] = &[
    "authorization:",
    "bearer ",
    "api_key",
    "api-key",
    "runtime api key",
    "credential",
    "session secret",
    "broker nonce",
    "nonce=",
    "sk-",
];
const SENSITIVE_KEYS: &[&str] = &[
    "token",
    "access_token",
    "access-token",
    "refresh_token",
    "refresh-token",
    "id_token",
    "id-token",
    "auth_token",
    "auth-token",
    "session_token",
    "session-token",
    "client_token",
    "client-token",
    "api_key",
    "api-key",
    "apikey",
    "runtime_api_key",
    "runtime-api-key",
    "password",
    "passwd",
    "passphrase",
    "pwd",
    "secret",
    "private_key",
    "private-key",
    "secret_key",
    "secret-key",
    "client_secret",
    "client-secret",
    "session_secret",
    "session-secret",
    "authorization",
    "nonce",
    "credential",
];

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn contains_sensitive_key_value(lower: &str) -> bool {
    for key in SENSITIVE_KEYS {
        let mut search_from = 0;
        while let Some(relative_index) = lower[search_from..].find(key) {
            let index = search_from + relative_index;
            let end = index + key.len();
            let before = lower[..index].chars().next_back();
            let after = lower[end..].chars().next();
            let bounded_before = before.is_none_or(|ch| !is_identifier_char(ch));
            let bounded_after = after.is_none_or(|ch| !is_identifier_char(ch));

            if bounded_before && bounded_after {
                let suffix = lower[end..].trim_start_matches(|ch: char| {
                    ch.is_ascii_whitespace() || ch == '\'' || ch == '"'
                });
                let assigned = suffix.starts_with('=') || suffix.starts_with(':');
                let cli_switch = lower[..index].ends_with("--");
                if assigned || cli_switch {
                    return true;
                }
            }

            search_from = end;
        }
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    ReadFile,
    SearchCode,
    ModifyFile,
    ExecuteCommand,
    GitOperation,
    Build,
    Test,
    ElevatedOperation,
    Other,
}

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
pub enum SafeTaskSummary {
    Omitted,
    Text(String),
}

impl SafeTaskSummary {
    pub fn from_untrusted(raw: &str) -> Self {
        let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            return Self::Omitted;
        }
        let lower = normalized.to_ascii_lowercase();
        if SENSITIVE_MARKERS
            .iter()
            .any(|marker| lower.contains(marker))
            || contains_sensitive_key_value(&lower)
        {
            return Self::Omitted;
        }

        let mut chars = normalized.chars();
        let text: String = chars.by_ref().take(MAX_SAFE_SUMMARY_CHARS).collect();
        if chars.next().is_some() {
            Self::Text(format!("{text}…"))
        } else {
            Self::Text(text)
        }
    }

    pub fn as_deref(&self) -> Option<&str> {
        match self {
            Self::Omitted => None,
            Self::Text(value) => Some(value.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        assert!(value.chars().count() <= MAX_SAFE_SUMMARY_CHARS + 1);
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
