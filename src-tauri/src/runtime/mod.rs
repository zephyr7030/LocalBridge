#[cfg(windows)]
mod windows_supervisor;

mod cancellation;
mod orchestrator;
mod recovery;

pub use cancellation::{RecoveryCancellation, RecoveryPermit};

pub use orchestrator::{
    OrchestratorError, OutageGeneration, OutageGenerationId, OutageTracker,
    ProductionRuntimeConfig, ProductionRuntimeDriver, RecoveryScope, RuntimeDriver,
    RuntimeHealthFailure, RuntimeOrchestrator, WorkspaceSwitchError,
};
pub use recovery::{
    AutoRecoveryRuntime, RECONNECT_BACKOFF_SECONDS, RecoveryClock, RecoveryController,
    RecoveryAttemptEvent, RecoveryAttemptResult, RecoveryDisposition, RecoveryOutcome,
    RuntimeOutage, STABILITY_RESET_SECONDS, SystemRecoveryClock,
};

#[cfg(windows)]
pub use windows_supervisor::{
    BoundedCommandOutput, ManagedProcessSpec, ProcessGeneration, ProcessSnapshot,
    SnapshotDisposition, StopDisposition, SupervisorError, WindowsProcessSupervisor,
    classify_persisted_snapshot, run_bounded_command,
};
