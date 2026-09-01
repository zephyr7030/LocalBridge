use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::{ValidatedWorkspace, WorkspaceValidator};

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

/// Untrusted metadata recording the identity observed when a workspace was last validated.
/// Deserializing this value never grants workspace authority.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PersistedWorkspaceIdentity(String);

impl PersistedWorkspaceIdentity {
    fn from_validated(validated: &ValidatedWorkspace) -> Self {
        Self(validated.identity().as_str().to_owned())
    }

    pub(crate) fn from_persisted_claim(
        value: impl Into<String>,
    ) -> Result<Self, WorkspaceRegistryError> {
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
    pub validated_identity: PersistedWorkspaceIdentity,
    pub last_opened_at: u64,
}

impl WorkspaceEntry {
    fn from_validated(
        workspace_id: WorkspaceId,
        display_path: impl Into<PathBuf>,
        validated: &ValidatedWorkspace,
        last_opened_at: u64,
    ) -> Result<Self, WorkspaceRegistryError> {
        let entry = Self {
            workspace_id,
            display_path: display_path.into(),
            validated_identity: PersistedWorkspaceIdentity::from_validated(validated),
            last_opened_at,
        };
        entry.validate()?;
        Ok(entry)
    }

    pub(crate) fn from_persisted_claim(
        workspace_id: WorkspaceId,
        display_path: impl Into<PathBuf>,
        identity_claim: impl Into<String>,
        last_opened_at: u64,
    ) -> Result<Self, WorkspaceRegistryError> {
        let entry = Self {
            workspace_id,
            display_path: display_path.into(),
            validated_identity: PersistedWorkspaceIdentity::from_persisted_claim(identity_claim)?,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkspace {
    pub workspace_id: WorkspaceId,
    pub validated: ValidatedWorkspace,
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

    /// Removes LocalBridge metadata only. This operation never touches the filesystem.
    pub fn remove(&mut self, id: &WorkspaceId) -> Option<WorkspaceEntry> {
        let index = self
            .entries
            .iter()
            .position(|entry| &entry.workspace_id == id)?;
        Some(self.entries.remove(index))
    }

    /// Adds metadata only after the filesystem path has been validated in this process.
    pub fn upsert_validated(
        &mut self,
        workspace_id: WorkspaceId,
        display_path: impl Into<PathBuf>,
        validated: &ValidatedWorkspace,
        last_opened_at: u64,
    ) -> Result<WorkspaceId, WorkspaceRegistryError> {
        self.upsert_entry(WorkspaceEntry::from_validated(
            workspace_id,
            display_path,
            validated,
            last_opened_at,
        )?)
    }

    /// Migration-only path: preserves a historical identity as an untrusted claim. It must never
    /// be used as authorization; resolve_active always revalidates against the filesystem.
    pub(crate) fn upsert_persisted_claim(
        &mut self,
        incoming: WorkspaceEntry,
    ) -> Result<WorkspaceId, WorkspaceRegistryError> {
        self.upsert_entry(incoming)
    }

    fn upsert_entry(
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

    /// Returns persisted selection metadata only. It is not an authorization proof.
    pub fn active_entry(&self) -> Option<&WorkspaceEntry> {
        self.active_workspace_id
            .as_ref()
            .and_then(|id| self.registry.get(id))
    }

    /// Selects persisted metadata only. Authorization is established later by resolve_active.
    pub fn set_active_reference(&mut self, id: WorkspaceId) -> Result<(), WorkspaceRegistryError> {
        if self.registry.get(&id).is_none() {
            return Err(WorkspaceRegistryError::ActiveWorkspaceMissingFromRegistry);
        }
        self.active_workspace_id = Some(id);
        Ok(())
    }

    pub fn clear_active(&mut self) {
        self.active_workspace_id = None;
    }

    /// Revalidates persisted selection metadata without creating a second mutable
    /// workspace state. The caller may submit the immutable result to DesiredState.
    pub fn resolve_active(
        &self,
        validator: &WorkspaceValidator,
    ) -> Result<Option<ResolvedWorkspace>, WorkspaceRegistryError> {
        self.validate()?;
        let Some(active) = self.active_entry() else {
            return Ok(None);
        };
        let validated = validator.validate(&active.display_path)?;
        if active.validated_identity.as_str() != validated.identity().as_str() {
            return Err(WorkspaceRegistryError::PersistedIdentityMismatch);
        }
        Ok(Some(ResolvedWorkspace {
            workspace_id: active.workspace_id.clone(),
            validated,
        }))
    }

    pub fn is_no_active_workspace(&self) -> bool {
        self.active_workspace_id.is_none()
    }

    pub fn domain_is_no_active_workspace(&self) -> Result<bool, WorkspaceRegistryError> {
        self.validate()?;
        Ok(self.active_workspace_id.is_none())
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
    WorkspaceIdMissing,
    PersistedIdentityMismatch,
    ExecutionPathUnavailable,
    ExecutionPathIdentityMismatch,
    WorkspaceNotDirectory,
    WorkspaceValidationWindowsApi { operation: &'static str, code: u32 },
    UnsupportedPlatform,
}
