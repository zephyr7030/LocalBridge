use serde::{Deserialize, Serialize};

use super::{ExecutionId, McpSessionId, PublicSessionId, TaskId, TerminalOutcome};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionTerminal {
    pub outcome: TerminalOutcome,
    pub exit_code: Option<i64>,
    pub signal: Option<String>,
    pub output_refs: Vec<String>,
    pub error_code: Option<String>,
    pub completed_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "terminal", rename_all = "snake_case")]
pub enum ExecutionState {
    Queued,
    Running,
    Terminal(ExecutionTerminal),
}

impl ExecutionState {
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub id: ExecutionId,
    pub task_id: TaskId,
    pub public_session_id: PublicSessionId,
    pub owner_session: Option<McpSessionId>,
    pub state: ExecutionState,
    pub started_at_ms: u64,
}
