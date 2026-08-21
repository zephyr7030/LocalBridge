use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{
    ErrorCategory, GitHubRepository, OperationError, ProductVersion, ReleaseDiscovery,
    UpdateCheckTrigger, UpdateLifecycle,
};

static UPDATE_OPERATION_GENERATION: AtomicU64 = AtomicU64::new(1);

type UpdatePublisher = Arc<dyn Fn(UpdateLifecycle) + Send + Sync + 'static>;

struct UpdateStateInner {
    current_version: ProductVersion,
    repository: Option<GitHubRepository>,
    state: Mutex<UpdateLifecycle>,
    publisher: Mutex<Option<UpdatePublisher>>,
}

#[derive(Clone)]
pub struct UpdateStateOwner(Arc<UpdateStateInner>);

impl fmt::Debug for UpdateStateOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpdateStateOwner")
            .field("state", &self.snapshot())
            .finish()
    }
}

impl Default for UpdateStateOwner {
    fn default() -> Self {
        Self::new(
            ProductVersion::current(),
            GitHubRepository::from_build_metadata(),
        )
    }
}

impl UpdateStateOwner {
    pub fn new(current_version: ProductVersion, repository: Option<GitHubRepository>) -> Self {
        let state = match repository.as_ref() {
            Some(repository) => UpdateLifecycle::Idle {
                current_version: current_version.clone(),
                releases_url: repository.releases_url(),
            },
            None => UpdateLifecycle::SourceUnavailable {
                current_version: current_version.clone(),
                reason: "github_repository_metadata_unavailable".into(),
            },
        };
        Self(Arc::new(UpdateStateInner {
            current_version,
            repository,
            state: Mutex::new(state),
            publisher: Mutex::new(None),
        }))
    }

    pub fn bind_publisher(&self, publisher: UpdatePublisher) -> Result<(), UpdatePublisherError> {
        let mut slot = self
            .0
            .publisher
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.is_some() {
            return Err(UpdatePublisherError::AlreadyBound);
        }
        *slot = Some(Arc::clone(&publisher));
        drop(slot);
        publisher(self.snapshot());
        Ok(())
    }

    pub fn snapshot(&self) -> UpdateLifecycle {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn repository(&self) -> Option<GitHubRepository> {
        self.0.repository.clone()
    }

    pub fn begin(&self, trigger: UpdateCheckTrigger) -> Result<UpdateCheckLease, UpdateStartError> {
        let repository = self
            .repository()
            .ok_or(UpdateStartError::SourceUnavailable)?;
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(&*state, UpdateLifecycle::Checking { .. }) {
            return Err(UpdateStartError::AlreadyChecking);
        }
        let operation_id = next_operation_id();
        *state = UpdateLifecycle::Checking {
            current_version: self.0.current_version.clone(),
            releases_url: repository.releases_url(),
            operation_id: operation_id.clone(),
            trigger,
            attempt: 1,
            started_at_ms: now_unix_ms(),
        };
        let next = state.clone();
        drop(state);
        self.publish(next);
        Ok(UpdateCheckLease {
            owner: self.clone(),
            operation_id,
            attempts: 1,
            terminal: false,
        })
    }

    fn set_attempt(&self, operation_id: &str, attempt: u8) -> Result<(), UpdateTransitionError> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &mut *state {
            UpdateLifecycle::Checking {
                operation_id: active,
                attempt: active_attempt,
                ..
            } if active == operation_id => *active_attempt = attempt,
            _ => return Err(UpdateTransitionError::NotOwned),
        }
        let next = state.clone();
        drop(state);
        self.publish(next);
        Ok(())
    }

    fn finish_success(
        &self,
        operation_id: &str,
        release: ReleaseDiscovery,
    ) -> Result<(), UpdateTransitionError> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let releases_url = match &*state {
            UpdateLifecycle::Checking {
                operation_id: active,
                releases_url,
                ..
            } if active == operation_id => releases_url.clone(),
            _ => return Err(UpdateTransitionError::NotOwned),
        };
        let checked_at_ms = now_unix_ms();
        *state = if release.version > self.0.current_version {
            UpdateLifecycle::Available {
                current_version: self.0.current_version.clone(),
                latest_version: release.version,
                release_url: release.release_url,
                operation_id: operation_id.to_owned(),
                checked_at_ms,
            }
        } else {
            UpdateLifecycle::Current {
                current_version: self.0.current_version.clone(),
                releases_url,
                operation_id: operation_id.to_owned(),
                checked_at_ms,
            }
        };
        let next = state.clone();
        drop(state);
        self.publish(next);
        Ok(())
    }

    fn finish_failed(
        &self,
        operation_id: &str,
        attempts: u8,
        error: OperationError,
    ) -> Result<(), UpdateTransitionError> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let releases_url = match &*state {
            UpdateLifecycle::Checking {
                operation_id: active,
                releases_url,
                ..
            } if active == operation_id => releases_url.clone(),
            _ => return Err(UpdateTransitionError::NotOwned),
        };
        *state = UpdateLifecycle::Failed {
            current_version: self.0.current_version.clone(),
            releases_url,
            operation_id: operation_id.to_owned(),
            attempts,
            checked_at_ms: now_unix_ms(),
            error: error.for_operation(operation_id),
        };
        let next = state.clone();
        drop(state);
        self.publish(next);
        Ok(())
    }

    fn publish(&self, state: UpdateLifecycle) {
        let publisher = self
            .0
            .publisher
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(publisher) = publisher {
            publisher(state);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePublisherError {
    AlreadyBound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStartError {
    SourceUnavailable,
    AlreadyChecking,
    ThreadSpawnFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateTransitionError {
    NotOwned,
}

#[derive(Debug)]
pub struct UpdateCheckLease {
    owner: UpdateStateOwner,
    operation_id: String,
    attempts: u8,
    terminal: bool,
}

impl UpdateCheckLease {
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub fn set_attempt(&mut self, attempt: u8) -> Result<(), UpdateTransitionError> {
        self.owner.set_attempt(&self.operation_id, attempt)?;
        self.attempts = attempt;
        Ok(())
    }

    pub fn succeed(mut self, release: ReleaseDiscovery) -> Result<(), UpdateTransitionError> {
        self.owner.finish_success(&self.operation_id, release)?;
        self.terminal = true;
        Ok(())
    }

    pub fn fail(mut self, error: OperationError) -> Result<(), UpdateTransitionError> {
        self.owner
            .finish_failed(&self.operation_id, self.attempts, error)?;
        self.terminal = true;
        Ok(())
    }
}

impl Drop for UpdateCheckLease {
    fn drop(&mut self) {
        if self.terminal {
            return;
        }
        let _ = self.owner.finish_failed(
            &self.operation_id,
            self.attempts,
            OperationError::new(
                "Update.CheckLost",
                ErrorCategory::Internal,
                "update check ended without a terminal result",
                true,
            ),
        );
    }
}

fn next_operation_id() -> String {
    let generation = UPDATE_OPERATION_GENERATION.fetch_add(1, Ordering::Relaxed);
    format!("update-{:x}-{:x}", now_unix_ms(), generation)
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> UpdateStateOwner {
        UpdateStateOwner::new(
            ProductVersion::parse("1.0.0").unwrap(),
            Some(GitHubRepository::new("owner/repo").unwrap()),
        )
    }

    #[test]
    fn one_owner_enforces_explicit_checking_and_exactly_one_terminal_result() {
        let owner = owner();
        let lease = owner.begin(UpdateCheckTrigger::Manual).unwrap();
        assert_eq!(
            owner.begin(UpdateCheckTrigger::Startup).unwrap_err(),
            UpdateStartError::AlreadyChecking
        );
        let release = ReleaseDiscovery::new(
            &owner.repository().unwrap(),
            ProductVersion::parse("1.1.0").unwrap(),
            "https://github.com/owner/repo/releases/tag/v1.1.0",
        )
        .unwrap();
        lease.succeed(release).unwrap();
        assert!(matches!(
            owner.snapshot(),
            UpdateLifecycle::Available { .. }
        ));
    }

    #[test]
    fn abandoned_check_converges_to_lost_instead_of_remaining_checking() {
        let owner = owner();
        drop(owner.begin(UpdateCheckTrigger::Startup).unwrap());
        assert!(matches!(
            owner.snapshot(),
            UpdateLifecycle::Failed { error, .. } if error.code == "Update.CheckLost"
        ));
    }
}
