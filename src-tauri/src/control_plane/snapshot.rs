use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::domain::{ExecutionRecord, PersistentFault, TaskRecord, UpdateLifecycle};
use crate::settings::AppData;
use crate::state::{PermissionMode, PrivilegeState, RuntimeState, TaskKind};
use crate::workspace::WorkspaceValidator;

use super::scheduler::SchedulerSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionAvailability {
    Ready,
    TemporarilyUnavailable,
    Fault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionSection<T> {
    availability: ProjectionAvailability,
    stale: bool,
    value: Option<T>,
}

impl<T> ProjectionSection<T> {
    pub fn ready(value: T) -> Self {
        Self {
            availability: ProjectionAvailability::Ready,
            stale: false,
            value: Some(value),
        }
    }

    pub fn unavailable() -> Self {
        Self {
            availability: ProjectionAvailability::TemporarilyUnavailable,
            stale: false,
            value: None,
        }
    }

    pub fn fault() -> Self {
        Self {
            availability: ProjectionAvailability::Fault,
            stale: false,
            value: None,
        }
    }

    pub fn faulted(previous: Option<T>) -> Self {
        Self {
            availability: ProjectionAvailability::Fault,
            stale: previous.is_some(),
            value: previous,
        }
    }

    pub fn stale(previous: Option<T>) -> Self {
        Self {
            availability: ProjectionAvailability::TemporarilyUnavailable,
            stale: true,
            value: previous,
        }
    }

    pub const fn availability(&self) -> ProjectionAvailability {
        self.availability
    }

    pub const fn is_stale(&self) -> bool {
        self.stale
    }

    pub const fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }

    pub fn into_value(self) -> Option<T> {
        self.value
    }

    pub fn map<U>(self, project: impl FnOnce(T) -> U) -> ProjectionSection<U> {
        ProjectionSection {
            availability: self.availability,
            stale: self.stale,
            value: self.value.map(project),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeProjection {
    pub active: bool,
    pub state: RuntimeState,
    pub local_environment_available: Option<bool>,
    pub current_task_elapsed_ms: Option<u64>,
    pub last_tool: Option<LastToolProjection>,
    pub outage: Option<OutageProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastToolProjection {
    pub kind: TaskKind,
    pub summary: Option<String>,
    pub age_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutageProjection {
    pub generation: u64,
    pub operation_id: String,
    pub user_attention_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityProjection {
    pub desired: PermissionMode,
    pub effective: PermissionMode,
    pub broker: PrivilegeState,
    pub elevated_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectiveAvailability {
    Available,
    Disabled,
    Reconciling,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProjection {
    pub desired_id: Option<String>,
    pub desired_path: Option<String>,
    pub observed_path: Option<String>,
    pub effective: EffectiveAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionProjection {
    pub desired_tunnel_id: Option<String>,
    pub desired_credential_epoch: Option<u64>,
    pub observed_tunnel_id: Option<String>,
    pub observed_credential_epoch: Option<u64>,
    pub effective: EffectiveAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectProjection {
    pub id: String,
    pub display_path: String,
    pub accessible_path: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsProjection {
    pub projects: Vec<ProjectProjection>,
    pub runtime_key_saved: bool,
    pub runtime_key_length: Option<usize>,
    pub auto_start: bool,
    pub close_window_continue_running: bool,
    pub onboarding_complete: bool,
}

impl Default for SettingsProjection {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            runtime_key_saved: false,
            runtime_key_length: None,
            auto_start: false,
            close_window_continue_running: true,
            onboarding_complete: false,
        }
    }
}

impl SettingsProjection {
    pub fn from_app_data(
        data: &AppData,
        runtime_key_saved: bool,
        runtime_key_length: Option<usize>,
    ) -> Self {
        let active_id = data.workspace.active_workspace_id.as_ref();
        let projects = data
            .workspace
            .remembered_entries()
            .iter()
            .map(|entry| {
                let accessible_path = WorkspaceValidator
                    .validate(&entry.display_path)
                    .ok()
                    .filter(|validated| {
                        entry.validated_identity.as_str() == validated.identity().as_str()
                    })
                    .map(|validated| validated.execution_path().to_string_lossy().into_owned());
                ProjectProjection {
                    id: entry.workspace_id.as_str().to_owned(),
                    display_path: entry.display_path.to_string_lossy().into_owned(),
                    accessible_path,
                    active: active_id == Some(&entry.workspace_id),
                }
            })
            .collect();
        Self {
            projects,
            runtime_key_saved,
            runtime_key_length,
            auto_start: data.settings.auto_start_services,
            close_window_continue_running: data.settings.close_window_continue_running,
            onboarding_complete: data.settings.onboarding_complete,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskAggregate {
    pub foreground_task: Option<TaskRecord>,
    pub detached_execution: Option<ExecutionRecord>,
    pub last_task: Option<TaskRecord>,
    pub last_execution: Option<ExecutionRecord>,
    pub scheduler: SchedulerSnapshot,
}

impl TaskAggregate {
    pub fn idle() -> Self {
        Self {
            foreground_task: None,
            detached_execution: None,
            last_task: None,
            last_execution: None,
            scheduler: SchedulerSnapshot::idle(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlPlaneSnapshot {
    pub revision: u64,
    pub captured_at_ms: u64,
    pub runtime: ProjectionSection<RuntimeProjection>,
    pub authority: ProjectionSection<AuthorityProjection>,
    pub scheduler: ProjectionSection<SchedulerSnapshot>,
    pub workspace: ProjectionSection<WorkspaceProjection>,
    pub connection: ProjectionSection<ConnectionProjection>,
    pub settings: ProjectionSection<SettingsProjection>,
    pub activity: ProjectionSection<TaskAggregate>,
    pub update: ProjectionSection<UpdateLifecycle>,
    pub active_faults: Vec<PersistentFault>,
}

impl Default for ControlPlaneSnapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            captured_at_ms: now_unix_ms(),
            runtime: ProjectionSection::unavailable(),
            authority: ProjectionSection::unavailable(),
            scheduler: ProjectionSection::unavailable(),
            workspace: ProjectionSection::unavailable(),
            connection: ProjectionSection::unavailable(),
            settings: ProjectionSection::unavailable(),
            activity: ProjectionSection::unavailable(),
            update: ProjectionSection::unavailable(),
            active_faults: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotDraft {
    pub runtime: ProjectionSection<RuntimeProjection>,
    pub authority: ProjectionSection<AuthorityProjection>,
    pub scheduler: ProjectionSection<SchedulerSnapshot>,
    pub workspace: ProjectionSection<WorkspaceProjection>,
    pub connection: ProjectionSection<ConnectionProjection>,
    pub settings: ProjectionSection<SettingsProjection>,
    pub activity: ProjectionSection<TaskAggregate>,
    pub update: ProjectionSection<UpdateLifecycle>,
    pub active_faults: Vec<PersistentFault>,
}

#[derive(Debug)]
struct SnapshotState {
    current: ControlPlaneSnapshot,
}

#[derive(Debug, Clone)]
pub struct ControlPlaneSnapshotOwner(Arc<(Mutex<SnapshotState>, Condvar)>);

impl Default for ControlPlaneSnapshotOwner {
    fn default() -> Self {
        Self(Arc::new((
            Mutex::new(SnapshotState {
                current: ControlPlaneSnapshot::default(),
            }),
            Condvar::new(),
        )))
    }
}

impl ControlPlaneSnapshotOwner {
    pub fn read(&self) -> ControlPlaneSnapshot {
        self.0
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .current
            .clone()
    }

    pub fn publish(&self, draft: SnapshotDraft) -> ControlPlaneSnapshot {
        let (state, changed) = &*self.0;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.current.runtime == draft.runtime
            && state.current.authority == draft.authority
            && state.current.scheduler == draft.scheduler
            && state.current.workspace == draft.workspace
            && state.current.connection == draft.connection
            && state.current.settings == draft.settings
            && state.current.activity == draft.activity
            && state.current.update == draft.update
            && state.current.active_faults == draft.active_faults
        {
            return state.current.clone();
        }
        let next = ControlPlaneSnapshot {
            revision: state.current.revision.saturating_add(1),
            captured_at_ms: now_unix_ms(),
            runtime: draft.runtime,
            authority: draft.authority,
            scheduler: draft.scheduler,
            workspace: draft.workspace,
            connection: draft.connection,
            settings: draft.settings,
            activity: draft.activity,
            update: draft.update,
            active_faults: draft.active_faults,
        };
        state.current = next.clone();
        changed.notify_all();
        next
    }

    pub fn mark_activity_stale(&self) -> ControlPlaneSnapshot {
        let previous = self.read();
        self.publish(SnapshotDraft {
            runtime: previous.runtime,
            authority: previous.authority,
            scheduler: ProjectionSection::stale(previous.scheduler.value),
            workspace: previous.workspace,
            connection: previous.connection,
            settings: previous.settings,
            activity: ProjectionSection::stale(previous.activity.value),
            update: previous.update,
            active_faults: previous.active_faults,
        })
    }

    pub fn mark_observation_stale(&self) -> ControlPlaneSnapshot {
        let previous = self.read();
        self.publish(SnapshotDraft {
            runtime: ProjectionSection::stale(previous.runtime.into_value()),
            authority: ProjectionSection::stale(previous.authority.into_value()),
            scheduler: ProjectionSection::stale(previous.scheduler.into_value()),
            workspace: ProjectionSection::stale(previous.workspace.into_value()),
            connection: ProjectionSection::stale(previous.connection.into_value()),
            settings: previous.settings,
            activity: ProjectionSection::stale(previous.activity.into_value()),
            update: previous.update,
            active_faults: previous.active_faults,
        })
    }

    pub fn publish_update(
        &self,
        update: ProjectionSection<UpdateLifecycle>,
    ) -> ControlPlaneSnapshot {
        let previous = self.read();
        self.publish(SnapshotDraft {
            runtime: previous.runtime,
            authority: previous.authority,
            scheduler: previous.scheduler,
            workspace: previous.workspace,
            connection: previous.connection,
            settings: previous.settings,
            activity: previous.activity,
            update,
            active_faults: previous.active_faults,
        })
    }

    pub fn wait_after(&self, revision: u64, timeout: Duration) -> u64 {
        let (state, changed) = &*self.0;
        let state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.current.revision != revision {
            return state.current.revision;
        }
        let (state, _) = changed
            .wait_timeout_while(state, timeout, |state| state.current.revision == revision)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.current.revision
    }
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

    fn draft(settings: ProjectionSection<SettingsProjection>) -> SnapshotDraft {
        SnapshotDraft {
            runtime: ProjectionSection::unavailable(),
            authority: ProjectionSection::unavailable(),
            scheduler: ProjectionSection::ready(SchedulerSnapshot::idle()),
            workspace: ProjectionSection::unavailable(),
            connection: ProjectionSection::unavailable(),
            settings,
            activity: ProjectionSection::ready(TaskAggregate::idle()),
            update: ProjectionSection::ready(UpdateLifecycle::SourceUnavailable {
                current_version: crate::domain::ProductVersion::current(),
                reason: "test".into(),
            }),
            active_faults: Vec::new(),
        }
    }

    #[test]
    fn publication_is_one_revisioned_atomic_snapshot() {
        let owner = ControlPlaneSnapshotOwner::default();
        let first = owner.publish(draft(ProjectionSection::ready(
            SettingsProjection::default(),
        )));
        let second = owner.publish(draft(ProjectionSection::ready(SettingsProjection {
            auto_start: true,
            ..SettingsProjection::default()
        })));
        assert_eq!(second.revision, first.revision + 1);
        assert!(!first.settings.value.unwrap().auto_start);
        assert!(second.settings.value.unwrap().auto_start);
    }

    #[test]
    fn update_publication_advances_one_revision_and_preserves_every_other_section() {
        let owner = ControlPlaneSnapshotOwner::default();
        let first = owner.publish(draft(ProjectionSection::ready(SettingsProjection {
            auto_start: true,
            ..SettingsProjection::default()
        })));
        let update = UpdateLifecycle::Idle {
            current_version: crate::domain::ProductVersion::parse("1.2.3").unwrap(),
            releases_url: "https://github.com/owner/repo/releases".into(),
        };

        let second = owner.publish_update(ProjectionSection::ready(update.clone()));

        assert_eq!(second.revision, first.revision + 1);
        assert_eq!(second.runtime, first.runtime);
        assert_eq!(second.authority, first.authority);
        assert_eq!(second.scheduler, first.scheduler);
        assert_eq!(second.workspace, first.workspace);
        assert_eq!(second.connection, first.connection);
        assert_eq!(second.settings, first.settings);
        assert_eq!(second.activity, first.activity);
        assert_eq!(second.active_faults, first.active_faults);
        assert_eq!(second.update, ProjectionSection::ready(update));
    }

    #[test]
    fn contention_marks_previous_activity_stale_without_fabricating_running() {
        let owner = ControlPlaneSnapshotOwner::default();
        owner.publish(draft(ProjectionSection::ready(
            SettingsProjection::default(),
        )));
        let stale = owner.mark_activity_stale();
        assert!(stale.activity.stale);
        assert_eq!(
            stale.activity.availability,
            ProjectionAvailability::TemporarilyUnavailable
        );
        assert!(stale.activity.value.unwrap().foreground_task.is_none());
    }

    #[test]
    fn a_faulted_section_does_not_blind_ready_sections() {
        let owner = ControlPlaneSnapshotOwner::default();
        let mut value = draft(ProjectionSection::fault());
        value.scheduler = ProjectionSection::ready(SchedulerSnapshot::idle());
        let snapshot = owner.publish(value);
        assert_eq!(
            snapshot.settings.availability,
            ProjectionAvailability::Fault
        );
        assert_eq!(
            snapshot.scheduler.availability,
            ProjectionAvailability::Ready
        );
        assert!(snapshot.scheduler.value.is_some());
    }
}
