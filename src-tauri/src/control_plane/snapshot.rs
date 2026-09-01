use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::domain::{ExecutionRecord, PersistentFault, TaskRecord, UpdateLifecycle};
use crate::settings::AppData;
use crate::state::{
    PermissionMode, PrivilegeState, RuntimeComponent, RuntimeFault, RuntimeState, TaskKind,
};
use crate::workspace::WorkspaceValidator;

use super::convergence::{AuthorityReconciliation, StructuredPathAuthority};
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingReadiness {
    pub local_environment: bool,
    pub coding_service: bool,
    pub openai_tunnel: bool,
}

impl OnboardingReadiness {
    pub fn from_runtime(runtime: &ProjectionSection<RuntimeProjection>) -> Self {
        if runtime.availability() != ProjectionAvailability::Ready || runtime.is_stale() {
            return Self::default();
        }
        let Some(runtime) = runtime.value() else {
            return Self::default();
        };
        Self {
            local_environment: runtime.local_environment_available.unwrap_or(false),
            coding_service: matches!(
                runtime.state,
                RuntimeState::StartingTunnel
                    | RuntimeState::WaitingTunnelReady
                    | RuntimeState::Ready
            ),
            openai_tunnel: matches!(runtime.state, RuntimeState::Ready),
        }
    }

    pub const fn all_ready(self) -> bool {
        self.local_environment && self.coding_service && self.openai_tunnel
    }
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
    pub component: RuntimeComponent,
    pub fault: RuntimeFault,
    pub user_attention_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityProjection {
    pub desired: PermissionMode,
    pub effective: PermissionMode,
    pub broker: PrivilegeState,
    pub structured_paths: StructuredPathAuthority,
    pub reconciliation: AuthorityReconciliation,
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

impl ControlPlaneSnapshot {
    pub fn onboarding_readiness(&self) -> OnboardingReadiness {
        OnboardingReadiness::from_runtime(&self.runtime)
    }

    pub fn work_is_authorized(&self) -> bool {
        let runtime_ready = self.runtime.availability() == ProjectionAvailability::Ready
            && !self.runtime.is_stale()
            && self
                .runtime
                .value()
                .is_some_and(|runtime| runtime.state == RuntimeState::Ready);
        let workspace_ready = self.workspace.availability() == ProjectionAvailability::Ready
            && !self.workspace.is_stale()
            && self
                .workspace
                .value()
                .is_some_and(|workspace| workspace.effective == EffectiveAvailability::Available);
        let connection_ready = self.connection.availability() == ProjectionAvailability::Ready
            && !self.connection.is_stale()
            && self.connection.value().is_some_and(|connection| {
                connection.desired_tunnel_id.is_none()
                    || connection.effective == EffectiveAvailability::Available
            });
        runtime_ready && workspace_ready && connection_ready
    }
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

#[derive(Debug, Clone)]
pub struct ControlPlaneSnapshotReader(Arc<(Mutex<SnapshotState>, Condvar)>);

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
    pub fn reader(&self) -> ControlPlaneSnapshotReader {
        ControlPlaneSnapshotReader(Arc::clone(&self.0))
    }

    pub fn read(&self) -> ControlPlaneSnapshot {
        self.0
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .current
            .clone()
    }

    pub fn initialize(&self, draft: SnapshotDraft) -> Option<ControlPlaneSnapshot> {
        let (state, changed) = &*self.0;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.current.revision != 0 {
            return None;
        }
        Some(Self::publish_locked(&mut state, changed, draft))
    }

    #[cfg(test)]
    fn publish(&self, draft: SnapshotDraft) -> ControlPlaneSnapshot {
        let (state, changed) = &*self.0;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::publish_locked(&mut state, changed, draft)
    }

    pub(crate) fn update(
        &self,
        update: impl FnOnce(&ControlPlaneSnapshot) -> SnapshotDraft,
    ) -> ControlPlaneSnapshot {
        let (state, changed) = &*self.0;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let draft = update(&state.current);
        Self::publish_locked(&mut state, changed, draft)
    }

    fn publish_locked(
        state: &mut SnapshotState,
        changed: &Condvar,
        draft: SnapshotDraft,
    ) -> ControlPlaneSnapshot {
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
        self.update(|previous| SnapshotDraft {
            runtime: previous.runtime.clone(),
            authority: previous.authority.clone(),
            scheduler: ProjectionSection::stale(previous.scheduler.value.clone()),
            workspace: previous.workspace.clone(),
            connection: previous.connection.clone(),
            settings: previous.settings.clone(),
            activity: ProjectionSection::stale(previous.activity.value.clone()),
            update: previous.update.clone(),
            active_faults: previous.active_faults.clone(),
        })
    }

    pub fn mark_observation_stale(&self) -> ControlPlaneSnapshot {
        self.update(|previous| SnapshotDraft {
            runtime: ProjectionSection::stale(previous.runtime.value.clone()),
            authority: ProjectionSection::stale(previous.authority.value.clone()),
            scheduler: ProjectionSection::stale(previous.scheduler.value.clone()),
            workspace: ProjectionSection::stale(previous.workspace.value.clone()),
            connection: ProjectionSection::stale(previous.connection.value.clone()),
            settings: previous.settings.clone(),
            activity: ProjectionSection::stale(previous.activity.value.clone()),
            update: previous.update.clone(),
            active_faults: previous.active_faults.clone(),
        })
    }

    pub fn publish_update(
        &self,
        update: ProjectionSection<UpdateLifecycle>,
    ) -> ControlPlaneSnapshot {
        self.update(|previous| SnapshotDraft {
            runtime: previous.runtime.clone(),
            authority: previous.authority.clone(),
            scheduler: previous.scheduler.clone(),
            workspace: previous.workspace.clone(),
            connection: previous.connection.clone(),
            settings: previous.settings.clone(),
            activity: previous.activity.clone(),
            update,
            active_faults: previous.active_faults.clone(),
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

impl ControlPlaneSnapshotReader {
    pub fn read(&self) -> ControlPlaneSnapshot {
        self.0
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .current
            .clone()
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
    fn onboarding_readiness_requires_one_fresh_runtime_projection() {
        let runtime = RuntimeProjection {
            active: true,
            state: RuntimeState::Ready,
            local_environment_available: Some(true),
            current_task_elapsed_ms: None,
            last_tool: None,
            outage: None,
        };
        assert!(
            OnboardingReadiness::from_runtime(&ProjectionSection::ready(runtime.clone()))
                .all_ready()
        );
        assert!(
            !OnboardingReadiness::from_runtime(&ProjectionSection::stale(Some(runtime)))
                .all_ready()
        );
        assert!(!OnboardingReadiness::from_runtime(&ProjectionSection::unavailable()).all_ready());
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
    fn section_updates_are_serialized_by_the_snapshot_owner_without_lost_state() {
        let owner = ControlPlaneSnapshotOwner::default();
        let first = owner.publish(draft(ProjectionSection::ready(
            SettingsProjection::default(),
        )));
        let settings_owner = owner.clone();
        let settings = std::thread::spawn(move || {
            settings_owner.update(|previous| SnapshotDraft {
                runtime: previous.runtime.clone(),
                authority: previous.authority.clone(),
                scheduler: previous.scheduler.clone(),
                workspace: previous.workspace.clone(),
                connection: previous.connection.clone(),
                settings: ProjectionSection::ready(SettingsProjection {
                    auto_start: true,
                    ..SettingsProjection::default()
                }),
                activity: previous.activity.clone(),
                update: previous.update.clone(),
                active_faults: previous.active_faults.clone(),
            })
        });
        let update_owner = owner.clone();
        let update = std::thread::spawn(move || {
            update_owner.publish_update(ProjectionSection::ready(UpdateLifecycle::Idle {
                current_version: crate::domain::ProductVersion::parse("1.2.3").unwrap(),
                releases_url: "https://github.com/owner/repo/releases".into(),
            }))
        });
        settings.join().unwrap();
        update.join().unwrap();

        let final_snapshot = owner.read();
        assert_eq!(final_snapshot.revision, first.revision + 2);
        assert!(final_snapshot.settings.value().unwrap().auto_start);
        assert!(matches!(
            final_snapshot.update.value(),
            Some(UpdateLifecycle::Idle { .. })
        ));
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

    #[test]
    fn read_only_consumer_observes_one_effective_revision_and_fails_closed_when_stale() {
        let owner = ControlPlaneSnapshotOwner::default();
        let reader = owner.reader();
        let mut ready = draft(ProjectionSection::ready(SettingsProjection::default()));
        ready.runtime = ProjectionSection::ready(RuntimeProjection {
            active: true,
            state: RuntimeState::Ready,
            local_environment_available: Some(true),
            current_task_elapsed_ms: None,
            last_tool: None,
            outage: None,
        });
        ready.authority = ProjectionSection::ready(AuthorityProjection {
            desired: PermissionMode::Full,
            effective: PermissionMode::Full,
            broker: PrivilegeState::Disabled,
            structured_paths: StructuredPathAuthority::ActiveWorkspace,
            reconciliation: AuthorityReconciliation::Converged,
        });
        ready.workspace = ProjectionSection::ready(WorkspaceProjection {
            desired_id: None,
            desired_path: Some("D:/workspace".into()),
            observed_path: Some("D:/workspace".into()),
            effective: EffectiveAvailability::Available,
        });
        ready.connection = ProjectionSection::ready(ConnectionProjection {
            desired_tunnel_id: None,
            desired_credential_epoch: None,
            observed_tunnel_id: None,
            observed_credential_epoch: None,
            effective: EffectiveAvailability::Unavailable,
        });

        let published = owner.publish(ready);
        let consumed = reader.read();
        assert_eq!(consumed.revision, published.revision);
        assert!(consumed.work_is_authorized());

        let mut stale = draft(consumed.settings.clone());
        stale.runtime = consumed.runtime;
        stale.authority = consumed.authority;
        stale.workspace = ProjectionSection::stale(consumed.workspace.into_value());
        stale.connection = consumed.connection;
        stale.scheduler = consumed.scheduler;
        stale.activity = consumed.activity;
        stale.update = consumed.update;
        stale.active_faults = consumed.active_faults;
        assert!(!owner.publish(stale).work_is_authorized());
    }
}
