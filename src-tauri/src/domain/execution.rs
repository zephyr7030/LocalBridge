use std::fmt;

use serde::{Deserialize, Serialize};

use super::{
    AdoptionTokenHash, ExecutionId, McpSessionId, PublicSessionId, TaskId, TerminalOutcome,
};

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuntimeCommandHandle(String);

impl RuntimeCommandHandle {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RuntimeCommandHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RuntimeCommandHandle(<redacted>)")
    }
}

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
    #[serde(default, skip_serializing)]
    pub adoption_token_hash: Option<AdoptionTokenHash>,
    #[serde(skip)]
    pub runtime_handle: Option<RuntimeCommandHandle>,
    pub state: ExecutionState,
    pub started_at_ms: u64,
    #[serde(default)]
    pub last_observed_at_ms: u64,
    #[serde(default)]
    pub orphaned_at_ms: Option<u64>,
}
