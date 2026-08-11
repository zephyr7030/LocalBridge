use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::state::{ActiveWorkspaceState, WorkspaceControlState, WorkspaceIdentity, WorkspaceRef};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    pub fn from_validated(value: impl Into<String>) -> Result<Self, WorkspaceRegistryError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(WorkspaceRegistryError::EmptyWorkspaceId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ValidatedWorkspaceIdentity(String);

impl ValidatedWorkspaceIdentity {
    /// Accepts only an opaque identity that has already been produced by the workspace validator.
    /// LB-003 deliberately does not derive identity from a path string.
    pub fn from_validator(value: impl Into<String>) -> Result<Self, WorkspaceRegistryError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(WorkspaceRegistryError::EmptyValidatedIdentity);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceEntry {
    pub workspace_id: WorkspaceId,
    pub display_path: PathBuf,
    pub validated_identity: ValidatedWorkspaceIdentity,
    pub last_opened_at: u64,
}

impl WorkspaceEntry {
    pub fn from_validator(
        workspace_id: WorkspaceId,
        display_path: impl Into<PathBuf>,
        validated_identity: ValidatedWorkspaceIdentity,
        last_opened_at: u64,
    ) -> Result<Self, WorkspaceRegistryError> {
        let entry = Self {
            workspace_id,
            display_path: display_path.into(),
            validated_identity,
            last_opened_at,
        };
        entry.validate()?;
        Ok(entry)
    }

    fn validate(&self) -> Result<(), WorkspaceRegistryError> {
        if self.display_path.as_os_str().is_empty() {
            return Err(WorkspaceRegistryError::EmptyDisplayPath);
        }
        if self.workspace_id.0.trim().is_empty() {
            return Err(WorkspaceRegistryError::EmptyWorkspaceId);
        }
        if self.validated_identity.0.trim().is_empty() {
            return Err(WorkspaceRegistryError::EmptyValidatedIdentity);
        }
        Ok(())
    }

    pub fn to_domain_ref(&self) -> Result<WorkspaceRef, WorkspaceRegistryError> {
        let identity = WorkspaceIdentity::from_validated(self.validated_identity.as_str().to_owned())
            .map_err(|_| WorkspaceRegistryError::DomainMappingFailed)?;
        WorkspaceRef::from_validated(identity, self.display_path.clone())
            .map_err(|_| WorkspaceRegistryError::DomainMappingFailed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRegistry {
    entries: Vec<WorkspaceEntry>,
}

impl WorkspaceRegistry {
    pub fn entries(&self) -> &[WorkspaceEntry] {
        &self.entries
    }

    pub fn get(&self, id: &WorkspaceId) -> Option<&WorkspaceEntry> {
        self.entries.iter().find(|entry| &entry.workspace_id == id)
    }

    /// De-duplicates only by validator-produced identity. Display paths are presentation metadata.
    pub fn upsert_validated(
        &mut self,
        incoming: WorkspaceEntry,
    ) -> Result<WorkspaceId, WorkspaceRegistryError> {
        incoming.validate()?;
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|entry| entry.validated_identity == incoming.validated_identity)
        {
            existing.display_path = incoming.display_path;
            existing.last_opened_at = incoming.last_opened_at;
            return Ok(existing.workspace_id.clone());
        }
        if self
            .entries
            .iter()
            .any(|entry| entry.workspace_id == incoming.workspace_id)
        {
            return Err(WorkspaceRegistryError::WorkspaceIdCollision);
        }
        let id = incoming.workspace_id.clone();
        self.entries.push(incoming);
        Ok(id)
    }

    pub fn validate(&self) -> Result<(), WorkspaceRegistryError> {
        for (index, entry) in self.entries.iter().enumerate() {
            entry.validate()?;
            if self.entries[..index]
                .iter()
                .any(|other| other.workspace_id == entry.workspace_id)
            {
                return Err(WorkspaceRegistryError::DuplicateWorkspaceId);
            }
            if self.entries[..index]
                .iter()
                .any(|other| other.validated_identity == entry.validated_identity)
            {
                return Err(WorkspaceRegistryError::DuplicateValidatedIdentity);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingWorkspaceReason {
    ValidatedIdentityMissing,
    WorkspaceIdMissing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingWorkspaceConfirmation {
    pub workspace_id: Option<String>,
    pub display_path: PathBuf,
    pub reason: PendingWorkspaceReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspacePersistence {
    pub registry: WorkspaceRegistry,
    pub active_workspace_id: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_workspace_confirmation: Option<PendingWorkspaceConfirmation>,
}

impl WorkspacePersistence {
    pub fn validate(&self) -> Result<(), WorkspaceRegistryError> {
        self.registry.validate()?;
        if let Some(active_id) = &self.active_workspace_id {
            if self.registry.get(active_id).is_none() {
                return Err(WorkspaceRegistryError::ActiveWorkspaceMissingFromRegistry);
            }
        }
        if let Some(pending) = &self.pending_workspace_confirmation {
            if pending.display_path.as_os_str().is_empty() {
                return Err(WorkspaceRegistryError::EmptyDisplayPath);
            }
        }
        Ok(())
    }

    pub fn remembered_entries(&self) -> &[WorkspaceEntry] {
        self.registry.entries()
    }

    /// Returns exactly zero or one active workspace. Remembered entries are never authorization roots.
    pub fn active_entry(&self) -> Option<&WorkspaceEntry> {
        self.active_workspace_id
            .as_ref()
            .and_then(|id| self.registry.get(id))
    }

    pub fn set_active(&mut self, id: WorkspaceId) -> Result<(), WorkspaceRegistryError> {
        if self.registry.get(&id).is_none() {
            return Err(WorkspaceRegistryError::ActiveWorkspaceMissingFromRegistry);
        }
        self.active_workspace_id = Some(id);
        Ok(())
    }

    pub fn clear_active(&mut self) {
        self.active_workspace_id = None;
    }

    pub fn to_control_state(&self) -> Result<WorkspaceControlState, WorkspaceRegistryError> {
        self.validate()?;
        let mut state = WorkspaceControlState::default();
        if let Some(active) = self.active_entry() {
            state.begin_switch(active.to_domain_ref()?);
            state
                .commit_candidate()
                .map_err(|_| WorkspaceRegistryError::DomainMappingFailed)?;
        }
        Ok(state)
    }

    pub fn is_no_active_workspace(&self) -> bool {
        self.active_workspace_id.is_none()
    }

    pub fn domain_is_no_active_workspace(&self) -> Result<bool, WorkspaceRegistryError> {
        Ok(matches!(
            self.to_control_state()?.active(),
            ActiveWorkspaceState::NoActiveWorkspace
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRegistryError {
    EmptyWorkspaceId,
    EmptyValidatedIdentity,
    EmptyDisplayPath,
    WorkspaceIdCollision,
    DuplicateWorkspaceId,
    DuplicateValidatedIdentity,
    ActiveWorkspaceMissingFromRegistry,
    DomainMappingFailed,
}
