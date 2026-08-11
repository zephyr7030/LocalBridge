#[cfg(windows)]
mod windows_supervisor;

#[cfg(windows)]
pub use windows_supervisor::{
    classify_persisted_snapshot, ManagedProcessSpec, ProcessGeneration, ProcessSnapshot,
    SnapshotDisposition, StopDisposition, SupervisorError, WindowsProcessSupervisor,
};
