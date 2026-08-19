use std::path::{Path, PathBuf};

use crate::runtime::{OrchestratorError, RuntimeDriver, RuntimeOrchestrator, WorkspaceSwitchError};
use crate::settings::{AppData, SettingsStore, SettingsStoreError};
use crate::state::WorkspaceControlState;

use super::{WorkspaceEntry, WorkspaceId, WorkspaceRegistryError, WorkspaceValidator};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceControlError {
    Registry(WorkspaceRegistryError),
    Settings(SettingsStoreError),
    RuntimeSwitch(WorkspaceSwitchError),
    RuntimeStop(OrchestratorError),
    SettingsCommitFailed {
        error: SettingsStoreError,
        runtime_rollback_fault: Option<crate::state::RuntimeFault>,
    },
}

impl From<WorkspaceRegistryError> for WorkspaceControlError {
    fn from(value: WorkspaceRegistryError) -> Self {
        Self::Registry(value)
    }
}

impl From<SettingsStoreError> for WorkspaceControlError {
    fn from(value: SettingsStoreError) -> Self {
        Self::Settings(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRemoval {
    NotFound,
    RemovedRemembered,
    RemovedActive,
}

#[derive(Debug)]
pub struct WorkspaceCoordinator {
    validator: WorkspaceValidator,
    store: SettingsStore,
    data: AppData,
}

impl WorkspaceCoordinator {
    pub fn load(store: SettingsStore) -> Result<Self, WorkspaceControlError> {
        let data = store.load()?;
        Ok(Self {
            validator: WorkspaceValidator,
            store,
            data,
        })
    }

    pub fn data(&self) -> &AppData {
        &self.data
    }

    pub fn validate_active_state(&self) -> Result<WorkspaceControlState, WorkspaceControlError> {
        self.data
            .workspace
            .to_control_state(&self.validator)
            .map_err(Into::into)
    }

    pub fn add_and_select<D: RuntimeDriver>(
        &mut self,
        runtime: &mut RuntimeOrchestrator<D>,
        workspace_id: WorkspaceId,
        display_path: &Path,
        last_opened_at: u64,
    ) -> Result<WorkspaceId, WorkspaceControlError> {
        let validated = self.validator.validate(display_path)?;
        let before = self.data.clone();
        let previous_runtime = previous_authorized_runtime(&before, runtime);
        let selected_id = self.data.workspace.registry.upsert_validated(
            workspace_id,
            validated.execution_path(),
            &validated,
            last_opened_at,
        )?;
        if let Err(error) =
            runtime.switch_workspace_to(validated.execution_path(), previous_runtime.as_deref())
        {
            self.data = before;
            return Err(WorkspaceControlError::RuntimeSwitch(error));
        }
        self.data
            .workspace
            .set_active_reference(selected_id.clone())?;
        if let Err(error) = self.store.save(&self.data) {
            self.data = before;
            let runtime_rollback_fault = rollback_runtime(runtime, previous_runtime);
            return Err(WorkspaceControlError::SettingsCommitFailed {
                error,
                runtime_rollback_fault,
            });
        }
        Ok(selected_id)
    }

    pub fn select<D: RuntimeDriver>(
        &mut self,
        runtime: &mut RuntimeOrchestrator<D>,
        workspace_id: &WorkspaceId,
        last_opened_at: u64,
    ) -> Result<(), WorkspaceControlError> {
        let entry = self
            .data
            .workspace
            .registry
            .get(workspace_id)
            .cloned()
            .ok_or(WorkspaceRegistryError::WorkspaceIdMissing)?;
        let validated = validate_entry(&self.validator, &entry)?;
        let before = self.data.clone();
        let previous_runtime = previous_authorized_runtime(&before, runtime);
        self.data.workspace.registry.upsert_validated(
            workspace_id.clone(),
            validated.execution_path(),
            &validated,
            last_opened_at,
        )?;
        if let Err(error) =
            runtime.switch_workspace_to(validated.execution_path(), previous_runtime.as_deref())
        {
            self.data = before;
            return Err(WorkspaceControlError::RuntimeSwitch(error));
        }
        self.data
            .workspace
            .set_active_reference(workspace_id.clone())?;
        if let Err(error) = self.store.save(&self.data) {
            self.data = before;
            let runtime_rollback_fault = rollback_runtime(runtime, previous_runtime);
            return Err(WorkspaceControlError::SettingsCommitFailed {
                error,
                runtime_rollback_fault,
            });
        }
        Ok(())
    }

    pub fn remove<D: RuntimeDriver>(
        &mut self,
        runtime: &mut RuntimeOrchestrator<D>,
        workspace_id: &WorkspaceId,
    ) -> Result<WorkspaceRemoval, WorkspaceControlError> {
        let Some(_entry) = self.data.workspace.registry.get(workspace_id).cloned() else {
            return Ok(WorkspaceRemoval::NotFound);
        };
        let is_active = self.data.workspace.active_workspace_id.as_ref() == Some(workspace_id);
        let before = self.data.clone();
        let previous_runtime = previous_authorized_runtime(&before, runtime);
        if is_active {
            runtime.stop().map_err(WorkspaceControlError::RuntimeStop)?;
            self.data.workspace.clear_active();
        }
        let removed = self.data.workspace.registry.remove(workspace_id);
        debug_assert!(removed.is_some());
        if let Err(error) = self.store.save(&self.data) {
            self.data = before;
            let runtime_rollback_fault = if is_active {
                rollback_runtime(runtime, previous_runtime)
            } else {
                None
            };
            return Err(WorkspaceControlError::SettingsCommitFailed {
                error,
                runtime_rollback_fault,
            });
        }
        Ok(if is_active {
            WorkspaceRemoval::RemovedActive
        } else {
            WorkspaceRemoval::RemovedRemembered
        })
    }
}

fn validate_entry(
    validator: &WorkspaceValidator,
    entry: &WorkspaceEntry,
) -> Result<super::ValidatedWorkspace, WorkspaceControlError> {
    let validated = validator.validate(&entry.display_path)?;
    if entry.validated_identity.as_str() != validated.identity().as_str() {
        return Err(WorkspaceRegistryError::PersistedIdentityMismatch.into());
    }
    Ok(validated)
}

fn rollback_runtime<D: RuntimeDriver>(
    runtime: &mut RuntimeOrchestrator<D>,
    previous_runtime: Option<PathBuf>,
) -> Option<crate::state::RuntimeFault> {
    match previous_runtime {
        Some(previous) => runtime
            .switch_workspace_to(&previous, None)
            .err()
            .map(|error| {
                error
                    .rollback_cleanup_fault
                    .or(error.rollback_fault)
                    .or(error.candidate_cleanup_fault)
                    .unwrap_or(error.candidate_fault)
            }),
        None => runtime.stop().err().map(|error| error.fault),
    }
}

fn previous_authorized_runtime<D: RuntimeDriver>(
    data: &AppData,
    runtime: &RuntimeOrchestrator<D>,
) -> Option<PathBuf> {
    if data.workspace.active_workspace_id.is_none() || !runtime.state().is_ready() {
        return None;
    }
    runtime.configured_workspace().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/workspace_switch/workspace_switch.rs"
    ));
}
