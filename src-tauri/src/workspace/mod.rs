pub(crate) mod context;
pub(crate) mod git_adapter;
pub(crate) mod path_authority;
mod registry;
mod validator;

pub use path_authority::{PathAuthorityError, PathAuthorityScope, WorkspaceResolver};
pub use registry::{
    PendingWorkspaceConfirmation, PendingWorkspaceReason, PersistedWorkspaceIdentity,
    ResolvedWorkspace, WorkspaceEntry, WorkspaceId, WorkspacePersistence, WorkspaceRegistry,
    WorkspaceRegistryError,
};
pub use validator::{ValidatedWorkspace, ValidatedWorkspaceIdentity, WorkspaceValidator};
