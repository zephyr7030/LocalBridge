#[cfg(windows)]
mod windows_supervisor;

mod orchestrator;
mod recovery;

pub use orchestrator::{
    OrchestratorError, OutageGeneration, OutageGenerationId, OutageTracker,
    ProductionRuntimeConfig, ProductionRuntimeDriver, RecoveryScope, RuntimeDriver,
    RuntimeHealthFailure, RuntimeOrchestrator, WorkspaceSwitchError,
};
pub use recovery::{
    AutoRecoveryRuntime, RECONNECT_BACKOFF_SECONDS, RecoveryClock, RecoveryController,
    RecoveryDisposition, RecoveryOutcome, RuntimeOutage, STABILITY_RESET_SECONDS,
    SystemRecoveryClock,
};

#[cfg(windows)]
pub use windows_supervisor::{
    classify_persisted_snapshot, ManagedProcessSpec, ProcessGeneration, ProcessSnapshot,
    SnapshotDisposition, StopDisposition, SupervisorError, WindowsProcessSupervisor,
};
