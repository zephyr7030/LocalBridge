use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkspaceIdentity(String);

impl WorkspaceIdentity {
    pub fn from_validated(value: impl Into<String>) -> Result<Self, WorkspaceModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(WorkspaceModelError::EmptyValidatedIdentity);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRef {
    identity: WorkspaceIdentity,
    display_path: PathBuf,
}

impl WorkspaceRef {
    pub fn from_validated(
        identity: WorkspaceIdentity,
        display_path: impl Into<PathBuf>,
    ) -> Result<Self, WorkspaceModelError> {
        let display_path = display_path.into();
        if display_path.as_os_str().is_empty() {
            return Err(WorkspaceModelError::EmptyDisplayPath);
        }
        Ok(Self {
            identity,
            display_path,
        })
    }

    pub fn identity(&self) -> &WorkspaceIdentity {
        &self.identity
    }

    pub fn display_path(&self) -> &Path {
        &self.display_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ActiveWorkspaceState {
    #[default]
    NoActiveWorkspace,
    Active(WorkspaceRef),
}

impl ActiveWorkspaceState {
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Active(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceControlState {
    desired: Option<WorkspaceRef>,
    candidate: Option<WorkspaceRef>,
    active: ActiveWorkspaceState,
}

impl WorkspaceControlState {
    pub fn desired(&self) -> Option<&WorkspaceRef> {
        self.desired.as_ref()
    }

    pub fn candidate(&self) -> Option<&WorkspaceRef> {
        self.candidate.as_ref()
    }

    pub const fn active(&self) -> &ActiveWorkspaceState {
        &self.active
    }

    pub fn begin_switch(&mut self, workspace: WorkspaceRef) {
        self.desired = Some(workspace.clone());
        self.candidate = Some(workspace);
    }

    pub fn commit_candidate(&mut self) -> Result<(), WorkspaceModelError> {
        let candidate = self
            .candidate
            .take()
            .ok_or(WorkspaceModelError::NoCandidateToCommit)?;
        self.desired = Some(candidate.clone());
        self.active = ActiveWorkspaceState::Active(candidate);
        Ok(())
    }

    pub fn cancel_candidate(&mut self) {
        self.candidate = None;
        self.desired = match &self.active {
            ActiveWorkspaceState::NoActiveWorkspace => None,
            ActiveWorkspaceState::Active(workspace) => Some(workspace.clone()),
        };
    }

    pub fn clear_active(&mut self) {
        self.desired = None;
        self.candidate = None;
        self.active = ActiveWorkspaceState::NoActiveWorkspace;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceModelError {
    EmptyValidatedIdentity,
    EmptyDisplayPath,
    NoCandidateToCommit,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(name: &str) -> WorkspaceRef {
        WorkspaceRef::from_validated(
            WorkspaceIdentity::from_validated(format!("validated:{name}")).unwrap(),
            format!(r"D:\project\{name}"),
        )
        .unwrap()
    }

    #[test]
    fn candidate_does_not_replace_active_until_commit() {
        let old = workspace("old");
        let new = workspace("new");
        let mut state = WorkspaceControlState::default();
        state.begin_switch(old.clone());
        state.commit_candidate().unwrap();
        state.begin_switch(new.clone());

        assert_eq!(state.active(), &ActiveWorkspaceState::Active(old));
        assert_eq!(state.candidate(), Some(&new));
    }

    #[test]
    fn cancelled_candidate_restores_desired_to_active() {
        let old = workspace("old");
        let mut state = WorkspaceControlState::default();
        state.begin_switch(old.clone());
        state.commit_candidate().unwrap();
        state.begin_switch(workspace("new"));
        state.cancel_candidate();

        assert_eq!(state.candidate(), None);
        assert_eq!(state.desired(), Some(&old));
        assert_eq!(state.active(), &ActiveWorkspaceState::Active(old));
    }

    #[test]
    fn no_active_workspace_is_normal_default_state() {
        let mut state = WorkspaceControlState::default();
        assert_eq!(state.active(), &ActiveWorkspaceState::NoActiveWorkspace);
        assert!(!state.active().is_active());
        assert_eq!(
            state.commit_candidate(),
            Err(WorkspaceModelError::NoCandidateToCommit)
        );
        state.clear_active();
        assert_eq!(state.active(), &ActiveWorkspaceState::NoActiveWorkspace);
    }
}
