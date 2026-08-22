use crate::domain::{
    ExecutionRecord, ExecutionState, ExecutionTerminal, PublicSessionId, RpcRequestId, TaskId,
    TerminalOutcome,
};

use super::execution_registry::{ExecutionRegistry, ExecutionRegistryError};

pub(crate) const COMMAND_CONTROL_TRANSPORT_HEADROOM_MS: u64 = 1_000;
pub(crate) const COMMAND_CONTROL_UPSTREAM_HEADROOM_MS: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandControlAction {
    Poll,
    Write,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandKillSignal {
    Term,
    Kill,
    Interrupt,
}

impl CommandKillSignal {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Term => "TERM",
            Self::Kill => "KILL",
            Self::Interrupt => "INT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCommandStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Lost,
}

impl RuntimeCommandStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Lost => "lost",
        }
    }

    const fn terminal_outcome(self) -> Option<TerminalOutcome> {
        match self {
            Self::Running => None,
            Self::Completed => Some(TerminalOutcome::Completed),
            Self::Failed => Some(TerminalOutcome::Failed),
            Self::Cancelled => Some(TerminalOutcome::Cancelled),
            Self::TimedOut => Some(TerminalOutcome::TimedOut),
            Self::Lost => Some(TerminalOutcome::Lost),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeCommandRequest {
    pub(crate) runtime_handle: String,
    pub(crate) action: CommandControlAction,
    pub(crate) chars: Option<String>,
    pub(crate) signal: Option<CommandKillSignal>,
    pub(crate) wait_ms: u64,
    pub(crate) request_id: RpcRequestId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeCommandObservation {
    pub(crate) status: RuntimeCommandStatus,
    pub(crate) exit_code: Option<i64>,
    pub(crate) signal: Option<String>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) truncated: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandControlRequest {
    pub(crate) action: CommandControlAction,
    pub(crate) chars: Option<String>,
    pub(crate) signal: Option<CommandKillSignal>,
    pub(crate) wait_ms: u64,
    pub(crate) request_id: RpcRequestId,
    pub(crate) public_session_id: PublicSessionId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCommandControlError {
    InvalidRequest,
    SessionUnavailable,
    CapabilityMismatch,
    TimedOut,
    Unavailable,
}

pub(crate) trait RuntimeCommandControl {
    fn control_command(
        &self,
        request: &RuntimeCommandRequest,
    ) -> Result<RuntimeCommandObservation, RuntimeCommandControlError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandControlResult {
    pub(crate) status: RuntimeCommandStatus,
    pub(crate) public_session_id: PublicSessionId,
    pub(crate) task_id: TaskId,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) elapsed_ms: u64,
    pub(crate) exit_code: Option<i64>,
    pub(crate) signal: Option<String>,
    pub(crate) truncated: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandControlError {
    InvalidRequest,
    SessionUnavailable,
    RuntimeUnavailable,
    RuntimeCapabilityMismatch,
    OperationTimedOut,
    ExecutionConflict,
}

pub(crate) fn control_command_during_work(
    request: CommandControlRequest,
    executions: &ExecutionRegistry,
    runtime: &dyn RuntimeCommandControl,
) -> Result<CommandControlResult, CommandControlError> {
    if request.action == CommandControlAction::Write
        && request.chars.as_deref().is_none_or(str::is_empty)
    {
        return Err(CommandControlError::InvalidRequest);
    }
    let execution = executions
        .execution_for_public_session(&request.public_session_id)
        .ok_or(CommandControlError::SessionUnavailable)?;
    if let ExecutionState::Terminal(terminal) = &execution.state {
        return if request.action == CommandControlAction::Poll {
            Ok(result_from_terminal(&execution, terminal))
        } else {
            Err(CommandControlError::SessionUnavailable)
        };
    }
    let runtime_handle = execution
        .runtime_handle
        .as_ref()
        .ok_or(CommandControlError::SessionUnavailable)?;
    if request.action == CommandControlAction::Kill {
        executions
            .request_cancellation(
                &request.public_session_id,
                request.signal.unwrap_or(CommandKillSignal::Term).as_str(),
            )
            .map_err(map_execution_error)?;
    }
    let observation = match runtime.control_command(&RuntimeCommandRequest {
        runtime_handle: runtime_handle.as_str().to_string(),
        action: request.action,
        chars: request.chars,
        signal: request.signal,
        wait_ms: request.wait_ms.min(30_000),
        request_id: request.request_id,
    }) {
        Ok(observation) => observation,
        Err(error) => {
            let cancellation_signal = executions.cancellation_signal(&request.public_session_id);
            if error == RuntimeCommandControlError::SessionUnavailable
                && cancellation_signal.is_some()
            {
                let terminal = ExecutionTerminal {
                    outcome: TerminalOutcome::Cancelled,
                    exit_code: None,
                    signal: cancellation_signal,
                    output_refs: Vec::new(),
                    error_code: Some("ProcessCancelled".to_string()),
                    completed_at_ms: unix_time_ms(),
                };
                match executions.finish(&execution.id, terminal) {
                    Ok(()) | Err(ExecutionRegistryError::AlreadyTerminal { .. }) => {}
                    Err(error) => return Err(map_execution_error(error)),
                }
                let settled = executions
                    .execution_for_public_session(&request.public_session_id)
                    .ok_or(CommandControlError::SessionUnavailable)?;
                let ExecutionState::Terminal(terminal) = &settled.state else {
                    return Err(CommandControlError::ExecutionConflict);
                };
                return Ok(result_from_terminal(&settled, terminal));
            }
            if matches!(
                error,
                RuntimeCommandControlError::InvalidRequest
                    | RuntimeCommandControlError::CapabilityMismatch
            ) {
                executions.clear_cancellation(&request.public_session_id);
            }
            return Err(map_runtime_error(error));
        }
    };

    let cancellation_signal = executions.cancellation_signal(&request.public_session_id);
    let terminal_outcome = observation.status.terminal_outcome().map(|outcome| {
        if cancellation_signal.is_some() {
            TerminalOutcome::Cancelled
        } else {
            outcome
        }
    });
    if let Some(outcome) = terminal_outcome {
        executions
            .finish(
                &execution.id,
                ExecutionTerminal {
                    outcome,
                    exit_code: observation.exit_code,
                    signal: observation
                        .signal
                        .clone()
                        .or_else(|| cancellation_signal.clone()),
                    output_refs: Vec::new(),
                    error_code: terminal_error_code(outcome).map(str::to_string),
                    completed_at_ms: unix_time_ms(),
                },
            )
            .map_err(map_execution_error)?;
    }

    Ok(CommandControlResult {
        status: if terminal_outcome == Some(TerminalOutcome::Cancelled) {
            RuntimeCommandStatus::Cancelled
        } else {
            observation.status
        },
        public_session_id: request.public_session_id,
        task_id: execution.task_id,
        stdout: observation.stdout,
        stderr: observation.stderr,
        elapsed_ms: unix_time_ms().saturating_sub(execution.started_at_ms),
        exit_code: observation.exit_code,
        signal: observation.signal.or(cancellation_signal),
        truncated: observation.truncated,
    })
}

fn result_from_terminal(
    execution: &ExecutionRecord,
    terminal: &ExecutionTerminal,
) -> CommandControlResult {
    CommandControlResult {
        status: match terminal.outcome {
            TerminalOutcome::Completed => RuntimeCommandStatus::Completed,
            TerminalOutcome::Failed | TerminalOutcome::Blocked => RuntimeCommandStatus::Failed,
            TerminalOutcome::Cancelled => RuntimeCommandStatus::Cancelled,
            TerminalOutcome::TimedOut => RuntimeCommandStatus::TimedOut,
            TerminalOutcome::Lost => RuntimeCommandStatus::Lost,
        },
        public_session_id: execution.public_session_id.clone(),
        task_id: execution.task_id.clone(),
        stdout: String::new(),
        stderr: String::new(),
        elapsed_ms: terminal
            .completed_at_ms
            .saturating_sub(execution.started_at_ms),
        exit_code: terminal.exit_code,
        signal: terminal.signal.clone(),
        truncated: None,
    }
}

fn terminal_error_code(outcome: TerminalOutcome) -> Option<&'static str> {
    match outcome {
        TerminalOutcome::Completed => None,
        TerminalOutcome::Cancelled => Some("ProcessCancelled"),
        TerminalOutcome::TimedOut => Some("ProcessTimedOut"),
        TerminalOutcome::Lost => Some("SessionUnavailable"),
        TerminalOutcome::Failed | TerminalOutcome::Blocked => Some("ProcessFailed"),
    }
}

fn map_runtime_error(error: RuntimeCommandControlError) -> CommandControlError {
    match error {
        RuntimeCommandControlError::InvalidRequest => CommandControlError::InvalidRequest,
        RuntimeCommandControlError::SessionUnavailable => CommandControlError::SessionUnavailable,
        RuntimeCommandControlError::CapabilityMismatch => {
            CommandControlError::RuntimeCapabilityMismatch
        }
        RuntimeCommandControlError::TimedOut => CommandControlError::OperationTimedOut,
        RuntimeCommandControlError::Unavailable => CommandControlError::RuntimeUnavailable,
    }
}

fn map_execution_error(error: ExecutionRegistryError) -> CommandControlError {
    match error {
        ExecutionRegistryError::AlreadyTerminal { .. } => CommandControlError::ExecutionConflict,
        ExecutionRegistryError::UnknownExecution(_) => CommandControlError::SessionUnavailable,
        _ => CommandControlError::RuntimeUnavailable,
    }
}

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::control_plane::execution_registry::ExecutionRegistry;
    use crate::domain::{McpSessionId, RuntimeCommandHandle};

    #[derive(Debug)]
    struct FakeRuntime(Mutex<Option<RuntimeCommandObservation>>);

    impl RuntimeCommandControl for FakeRuntime {
        fn control_command(
            &self,
            _request: &RuntimeCommandRequest,
        ) -> Result<RuntimeCommandObservation, RuntimeCommandControlError> {
            self.0
                .lock()
                .expect("fake runtime lock")
                .take()
                .ok_or(RuntimeCommandControlError::Unavailable)
        }
    }

    #[derive(Debug)]
    struct TimedOutRuntime;

    impl RuntimeCommandControl for TimedOutRuntime {
        fn control_command(
            &self,
            _request: &RuntimeCommandRequest,
        ) -> Result<RuntimeCommandObservation, RuntimeCommandControlError> {
            Err(RuntimeCommandControlError::TimedOut)
        }
    }

    #[derive(Debug)]
    struct DisappearedRuntime;

    impl RuntimeCommandControl for DisappearedRuntime {
        fn control_command(
            &self,
            _request: &RuntimeCommandRequest,
        ) -> Result<RuntimeCommandObservation, RuntimeCommandControlError> {
            Err(RuntimeCommandControlError::SessionUnavailable)
        }
    }

    #[test]
    fn terminal_observation_is_committed_by_control_plane_owner() {
        let root = std::env::temp_dir().join(format!(
            "localbridge-command-control-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&root).expect("test workspace");
        let registry =
            ExecutionRegistry::open_at(root.join("executions.json")).expect("execution registry");
        let public_session = PublicSessionId::new("public-1");
        let execution_id = registry
            .start(TaskId::new("task-1"), public_session.clone())
            .expect("start execution");
        registry
            .bind_owner(&execution_id, McpSessionId::new("mcp-1"))
            .expect("bind owner");
        registry
            .bind_runtime_handle(&execution_id, RuntimeCommandHandle::new("private-1"))
            .expect("bind runtime handle");
        let runtime = FakeRuntime(Mutex::new(Some(RuntimeCommandObservation {
            // Windows KILL may be reported by the runtime as a non-zero process
            // exit. The ControlPlane-owned cancellation intent, not that adapter
            // spelling, determines the domain terminal outcome.
            status: RuntimeCommandStatus::Failed,
            exit_code: Some(1),
            signal: None,
            stdout: String::new(),
            stderr: String::new(),
            truncated: Some(false),
        })));

        let result = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Kill,
                chars: None,
                signal: Some(CommandKillSignal::Kill),
                wait_ms: 100,
                request_id: RpcRequestId::String("private-request".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &runtime,
        )
        .expect("control result");
        assert_eq!(result.status, RuntimeCommandStatus::Cancelled);
        assert!(matches!(
            registry
                .execution_for_public_session(&public_session)
                .expect("execution")
                .state,
            ExecutionState::Terminal(ExecutionTerminal {
                outcome: TerminalOutcome::Cancelled,
                ..
            })
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn kill_timeout_preserves_intent_until_poll_observes_one_cancelled_terminal() {
        let root = std::env::temp_dir().join(format!(
            "localbridge-command-control-timeout-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&root).expect("test workspace");
        let registry =
            ExecutionRegistry::open_at(root.join("executions.json")).expect("execution registry");
        let public_session = PublicSessionId::new("public-timeout");
        let execution_id = registry
            .start(TaskId::new("task-timeout"), public_session.clone())
            .expect("start execution");
        registry
            .bind_runtime_handle(&execution_id, RuntimeCommandHandle::new("private-timeout"))
            .expect("bind runtime handle");

        let timed_out = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Kill,
                chars: None,
                signal: Some(CommandKillSignal::Kill),
                wait_ms: 0,
                request_id: RpcRequestId::String("kill-timeout".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &TimedOutRuntime,
        );
        assert_eq!(timed_out, Err(CommandControlError::OperationTimedOut));
        assert_eq!(
            registry.cancellation_signal(&public_session).as_deref(),
            Some("KILL")
        );

        let polled = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Poll,
                chars: None,
                signal: None,
                wait_ms: 0,
                request_id: RpcRequestId::String("poll-after-timeout".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &FakeRuntime(Mutex::new(Some(RuntimeCommandObservation {
                status: RuntimeCommandStatus::Failed,
                exit_code: Some(1),
                signal: None,
                stdout: String::new(),
                stderr: String::new(),
                truncated: Some(false),
            }))),
        )
        .expect("poll result");
        assert_eq!(polled.status, RuntimeCommandStatus::Cancelled);
        assert!(matches!(
            registry
                .execution_for_public_session(&public_session)
                .expect("execution")
                .state,
            ExecutionState::Terminal(ExecutionTerminal {
                outcome: TerminalOutcome::Cancelled,
                ..
            })
        ));
        assert_eq!(registry.cancellation_signal(&public_session), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn accepted_cancellation_wins_when_the_runtime_session_disappears_before_poll() {
        let root = std::env::temp_dir().join(format!(
            "localbridge-command-control-disappeared-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&root).expect("test workspace");
        let registry =
            ExecutionRegistry::open_at(root.join("executions.json")).expect("execution registry");
        let public_session = PublicSessionId::new("public-disappeared");
        let execution_id = registry
            .start(TaskId::new("task-disappeared"), public_session.clone())
            .expect("start execution");
        registry
            .bind_runtime_handle(
                &execution_id,
                RuntimeCommandHandle::new("private-disappeared"),
            )
            .expect("bind runtime handle");
        registry
            .request_cancellation(&public_session, "KILL")
            .expect("accept cancellation intent");

        let polled = control_command_during_work(
            CommandControlRequest {
                action: CommandControlAction::Poll,
                chars: None,
                signal: None,
                wait_ms: 0,
                request_id: RpcRequestId::String("poll-disappeared".into()),
                public_session_id: public_session.clone(),
            },
            &registry,
            &DisappearedRuntime,
        )
        .expect("a disappeared cancelled session has one terminal outcome");

        assert_eq!(polled.status, RuntimeCommandStatus::Cancelled);
        assert_eq!(polled.signal.as_deref(), Some("KILL"));
        assert!(matches!(
            registry
                .execution_for_public_session(&public_session)
                .expect("execution")
                .state,
            ExecutionState::Terminal(ExecutionTerminal {
                outcome: TerminalOutcome::Cancelled,
                ..
            })
        ));
        assert_eq!(registry.cancellation_signal(&public_session), None);
        let _ = std::fs::remove_dir_all(root);
    }
}
