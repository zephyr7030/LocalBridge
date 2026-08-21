use std::path::Path;

use crate::domain::{
    ExecutionRecord, ExecutionState, ExecutionTerminal, PublicSessionId, RpcRequestId, TaskId,
    TerminalOutcome,
};

use super::execution_registry::{ExecutionRegistry, ExecutionRegistryError};
use super::workflow_checkpoint::WorkflowCheckpointStore;

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
    pub(crate) checkpoint_settled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandControlError {
    InvalidRequest,
    SessionUnavailable,
    RuntimeUnavailable,
    RuntimeCapabilityMismatch,
    ExecutionConflict,
}

pub(crate) fn control_command_during_work(
    request: CommandControlRequest,
    executions: &ExecutionRegistry,
    runtime: &dyn RuntimeCommandControl,
    workspace: &Path,
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
    let observation = runtime
        .control_command(&RuntimeCommandRequest {
            runtime_handle: runtime_handle.as_str().to_string(),
            action: request.action,
            chars: request.chars,
            signal: request.signal,
            wait_ms: request.wait_ms.min(30_000),
            request_id: request.request_id,
        })
        .map_err(map_runtime_error)?;

    if let Some(outcome) = observation.status.terminal_outcome() {
        executions
            .finish(
                &execution.id,
                ExecutionTerminal {
                    outcome,
                    exit_code: observation.exit_code,
                    signal: observation.signal.clone(),
                    output_refs: Vec::new(),
                    error_code: terminal_error_code(outcome).map(str::to_string),
                    completed_at_ms: unix_time_ms(),
                },
            )
            .map_err(map_execution_error)?;
    }

    let checkpoint_settled = request.action != CommandControlAction::Kill
        || observation.status == RuntimeCommandStatus::Running
        || WorkflowCheckpointStore::for_workspace(workspace)
            .and_then(|store| store.settle_command_kill(request.public_session_id.as_str()))
            .is_ok();

    Ok(CommandControlResult {
        status: observation.status,
        public_session_id: request.public_session_id,
        task_id: execution.task_id,
        stdout: observation.stdout,
        stderr: observation.stderr,
        elapsed_ms: unix_time_ms().saturating_sub(execution.started_at_ms),
        exit_code: observation.exit_code,
        signal: observation.signal,
        truncated: observation.truncated,
        checkpoint_settled,
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
        checkpoint_settled: true,
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
        RuntimeCommandControlError::SessionUnavailable => {
            CommandControlError::SessionUnavailable
        }
        RuntimeCommandControlError::CapabilityMismatch => {
            CommandControlError::RuntimeCapabilityMismatch
        }
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

    #[test]
    fn terminal_observation_is_committed_by_control_plane_owner() {
        let root = std::env::temp_dir().join(format!(
            "localbridge-command-control-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&root).expect("test workspace");
        let registry = ExecutionRegistry::open_at(root.join("executions.json"))
            .expect("execution registry");
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
            status: RuntimeCommandStatus::Cancelled,
            exit_code: Some(1),
            signal: Some("SIGKILL".into()),
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
            &root,
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
}
