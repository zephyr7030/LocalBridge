use serde::{Deserialize, Serialize};

use super::{RequestKey, TaskId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Validation,
    Authorization,
    Capacity,
    Conflict,
    Timeout,
    Unavailable,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationError {
    pub code: String,
    pub category: ErrorCategory,
    pub message: String,
    pub retryable: bool,
    pub operation_id: Option<String>,
    pub request: Option<RequestKey>,
    pub task_id: Option<TaskId>,
}

impl OperationError {
    pub fn new(
        code: impl Into<String>,
        category: ErrorCategory,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            code: code.into(),
            category,
            message: message.into(),
            retryable,
            operation_id: None,
            request: None,
            task_id: None,
        }
    }

    pub fn for_operation(mut self, operation_id: impl Into<String>) -> Self {
        self.operation_id = Some(operation_id.into());
        self
    }

    pub fn for_request(mut self, request: RequestKey) -> Self {
        self.request = Some(request);
        self
    }

    pub fn for_task(mut self, task_id: TaskId) -> Self {
        self.task_id = Some(task_id);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultSource {
    Runtime,
    Authority,
    Scheduler,
    Workspace,
    Connection,
    Settings,
    Snapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentFault {
    pub id: String,
    pub source: FaultSource,
    pub error: OperationError,
    pub first_seen_at_ms: u64,
    pub last_seen_at_ms: u64,
}
