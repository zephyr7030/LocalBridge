pub mod error;
pub mod execution;
pub mod identity;
pub mod lifecycle;
pub mod session;
pub mod task;

pub use error::{ErrorCategory, FaultSource, OperationError, PersistentFault};
pub use execution::{ExecutionRecord, ExecutionState, ExecutionTerminal};
pub use identity::{ExecutionId, McpSessionId, PublicSessionId, RequestKey, RpcRequestId, TaskId};
pub use lifecycle::{LifecycleState, TerminalOutcome};
pub use session::McpSessionState;
pub use task::{SafeTaskSummary, TaskKind, TaskRecord};
