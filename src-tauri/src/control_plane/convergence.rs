use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::state::{PermissionMode, PrivilegeState, RuntimeState};
use crate::tunnel::TunnelId;
use crate::workspace::WorkspaceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceIntent {
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredWorkspace {
    pub id: Option<WorkspaceId>,
    pub execution_path: PathBuf,
}

impl DesiredWorkspace {
    pub fn new(id: WorkspaceId, execution_path: impl Into<PathBuf>) -> Self {
        Self {
            id: Some(id),
            execution_path: execution_path.into(),
        }
    }

    pub fn for_runtime_path(execution_path: impl Into<PathBuf>) -> Self {
        Self {
            id: None,
            execution_path: execution_path.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionProfile {
    pub tunnel_id: TunnelId,
    pub credential_epoch: u64,
}

impl ConnectionProfile {
    pub const fn new(tunnel_id: TunnelId, credential_epoch: u64) -> Self {
        Self {
            tunnel_id,
            credential_epoch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredState {
    pub permission: PermissionMode,
    pub workspace: Option<DesiredWorkspace>,
    pub services: ServiceIntent,
    pub connection: Option<ConnectionProfile>,
}

impl Default for DesiredState {
    fn default() -> Self {
        Self {
            permission: PermissionMode::Edit,
            workspace: None,
            services: ServiceIntent::Disabled,
            connection: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredStateSnapshot {
    pub revision: u64,
    pub state: DesiredState,
}

#[derive(Debug, Clone)]
struct VersionedDesiredState {
    revision: u64,
    state: DesiredState,
}

impl Default for VersionedDesiredState {
    fn default() -> Self {
        Self {
            revision: 1,
            state: DesiredState::default(),
        }
    }
}

/// The only mutable owner of accepted control-plane intent.
///
/// Clones are handles to the same owner; persistence files are durable backing,
/// not a second live state that transports or runtimes may mutate.
#[derive(Debug, Clone, Default)]
pub struct DesiredStateOwner {
    inner: Arc<RwLock<VersionedDesiredState>>,
}

impl DesiredStateOwner {
    pub fn snapshot(&self) -> DesiredStateSnapshot {
        let current = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        DesiredStateSnapshot {
            revision: current.revision,
            state: current.state.clone(),
        }
    }

    pub fn replace(&self, state: DesiredState) -> u64 {
        self.mutate(|current| *current = state)
    }

    pub fn set_permission(&self, permission: PermissionMode) -> u64 {
        self.mutate(|state| state.permission = permission)
    }

    pub fn set_workspace(&self, workspace: Option<DesiredWorkspace>) -> u64 {
        self.mutate(|state| state.workspace = workspace)
    }

    pub fn set_services(&self, services: ServiceIntent) -> u64 {
        self.mutate(|state| state.services = services)
    }

    pub fn set_connection(&self, connection: Option<ConnectionProfile>) -> u64 {
        self.mutate(|state| state.connection = connection)
    }

    pub fn mark_credentials_changed(&self) -> u64 {
        self.mutate(|state| {
            if let Some(connection) = state.connection.as_mut() {
                connection.credential_epoch = connection.credential_epoch.saturating_add(1);
            }
        })
    }

    fn mutate(&self, update: impl FnOnce(&mut DesiredState)) -> u64 {
        let mut current = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = current.state.clone();
        update(&mut current.state);
        if current.state != before {
            current.revision = current.revision.saturating_add(1);
        }
        current.revision
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedState {
    pub broker: PrivilegeState,
    pub runtime: RuntimeState,
    pub workspace: Option<PathBuf>,
    pub connection: Option<ConnectionProfile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityReconciliation {
    Converged,
    AuthorizationRequired,
    AwaitingAuthorization,
    BrokerUnavailable,
    DisablePending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuredPathAuthority {
    ActiveWorkspace,
    AdministratorBroker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveAuthority {
    pub configured: PermissionMode,
    pub execution: PermissionMode,
    pub structured_paths: StructuredPathAuthority,
    pub reconciliation: AuthorityReconciliation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceUnavailableReason {
    NotConfigured,
    RuntimeNotReady,
    DesiredObservedDiverged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectiveWorkspaceAuthority {
    Available(DesiredWorkspace),
    Unavailable(WorkspaceUnavailableReason),
}

impl EffectiveWorkspaceAuthority {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveServiceState {
    Disabled,
    Reconciling,
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionUnavailableReason {
    NotConfigured,
    RuntimeNotReady,
    DesiredObservedDiverged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectiveConnection {
    Available(ConnectionProfile),
    Unavailable(ConnectionUnavailableReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveState {
    pub authority: EffectiveAuthority,
    pub workspace: EffectiveWorkspaceAuthority,
    pub services: EffectiveServiceState,
    pub connection: EffectiveConnection,
}

impl EffectiveState {
    pub fn work_is_authorized(&self) -> bool {
        self.workspace.is_available()
            && self.services == EffectiveServiceState::Available
            && !matches!(
                self.connection,
                EffectiveConnection::Unavailable(
                    ConnectionUnavailableReason::RuntimeNotReady
                        | ConnectionUnavailableReason::DesiredObservedDiverged
                )
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvergenceSnapshot {
    pub desired_revision: u64,
    pub desired: DesiredState,
    pub observed: ObservedState,
    pub effective: EffectiveState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionReconcileAction {
    None,
    RequestAuthorization,
    DisableBroker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeReconcileAction {
    None,
    Start,
    Stop,
    ApplyWorkspace(PathBuf),
    RestartConnection,
    WaitForObservation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcilePlan {
    pub permission: PermissionReconcileAction,
    pub runtime: RuntimeReconcileAction,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Reconciler;

impl Reconciler {
    pub fn plan(snapshot: &ConvergenceSnapshot) -> ReconcilePlan {
        let permission = match (snapshot.desired.permission, &snapshot.observed.broker) {
            (
                PermissionMode::Elevated,
                PrivilegeState::Disabled | PrivilegeState::Requested | PrivilegeState::Faulted(_),
            ) => PermissionReconcileAction::RequestAuthorization,
            (PermissionMode::Elevated, _) => PermissionReconcileAction::None,
            (_, PrivilegeState::Disabled) => PermissionReconcileAction::None,
            (_, _) => PermissionReconcileAction::DisableBroker,
        };

        let runtime = match snapshot.desired.services {
            ServiceIntent::Disabled if snapshot.observed.runtime != RuntimeState::Stopped => {
                RuntimeReconcileAction::Stop
            }
            ServiceIntent::Disabled => RuntimeReconcileAction::None,
            ServiceIntent::Enabled => match &snapshot.observed.runtime {
                RuntimeState::Stopped | RuntimeState::Faulted(_) => RuntimeReconcileAction::Start,
                RuntimeState::Ready => match &snapshot.effective.workspace {
                    EffectiveWorkspaceAuthority::Unavailable(
                        WorkspaceUnavailableReason::DesiredObservedDiverged,
                    ) => snapshot
                        .desired
                        .workspace
                        .as_ref()
                        .map(|workspace| {
                            RuntimeReconcileAction::ApplyWorkspace(workspace.execution_path.clone())
                        })
                        .unwrap_or(RuntimeReconcileAction::WaitForObservation),
                    EffectiveWorkspaceAuthority::Unavailable(_) => {
                        RuntimeReconcileAction::WaitForObservation
                    }
                    EffectiveWorkspaceAuthority::Available(_) => {
                        if matches!(
                            snapshot.effective.connection,
                            EffectiveConnection::Unavailable(
                                ConnectionUnavailableReason::DesiredObservedDiverged
                            )
                        ) {
                            RuntimeReconcileAction::RestartConnection
                        } else {
                            RuntimeReconcileAction::None
                        }
                    }
                },
                RuntimeState::StartingMcp
                | RuntimeState::WaitingMcpReady
                | RuntimeState::StartingPolicyEnforcement
                | RuntimeState::WaitingPolicyReady
                | RuntimeState::StartingTunnel
                | RuntimeState::WaitingTunnelReady
                | RuntimeState::Recovering { .. }
                | RuntimeState::SwitchingWorkspace { .. } => {
                    RuntimeReconcileAction::WaitForObservation
                }
            },
        };
        ReconcilePlan {
            permission,
            runtime,
        }
    }
}

impl ConvergenceSnapshot {
    pub fn derive(desired: DesiredStateSnapshot, observed: ObservedState) -> Self {
        let effective = EffectiveState {
            authority: derive_authority(desired.state.permission, &observed.broker),
            workspace: derive_workspace(
                desired.state.workspace.as_ref(),
                &observed.runtime,
                observed.workspace.as_deref(),
            ),
            services: derive_services(desired.state.services, &observed.runtime),
            connection: derive_connection(
                desired.state.connection.as_ref(),
                &observed.runtime,
                observed.connection.as_ref(),
            ),
        };
        Self {
            desired_revision: desired.revision,
            desired: desired.state,
            observed,
            effective,
        }
    }
}

pub fn derive_authority(configured: PermissionMode, broker: &PrivilegeState) -> EffectiveAuthority {
    if configured != PermissionMode::Elevated {
        return EffectiveAuthority {
            configured,
            execution: configured,
            structured_paths: StructuredPathAuthority::ActiveWorkspace,
            reconciliation: if matches!(broker, PrivilegeState::Disabled) {
                AuthorityReconciliation::Converged
            } else {
                AuthorityReconciliation::DisablePending
            },
        };
    }
    match broker {
        PrivilegeState::Active { .. } => EffectiveAuthority {
            configured,
            execution: PermissionMode::Elevated,
            structured_paths: StructuredPathAuthority::AdministratorBroker,
            reconciliation: AuthorityReconciliation::Converged,
        },
        PrivilegeState::Disabled => EffectiveAuthority {
            configured,
            execution: PermissionMode::Full,
            structured_paths: StructuredPathAuthority::ActiveWorkspace,
            reconciliation: AuthorityReconciliation::AuthorizationRequired,
        },
        PrivilegeState::Requested | PrivilegeState::AwaitingUac => EffectiveAuthority {
            configured,
            execution: PermissionMode::Full,
            structured_paths: StructuredPathAuthority::ActiveWorkspace,
            reconciliation: AuthorityReconciliation::AwaitingAuthorization,
        },
        PrivilegeState::Faulted(_) => EffectiveAuthority {
            configured,
            execution: PermissionMode::Full,
            structured_paths: StructuredPathAuthority::ActiveWorkspace,
            reconciliation: AuthorityReconciliation::BrokerUnavailable,
        },
    }
}

fn derive_workspace(
    desired: Option<&DesiredWorkspace>,
    runtime: &RuntimeState,
    observed: Option<&Path>,
) -> EffectiveWorkspaceAuthority {
    let Some(desired) = desired else {
        return EffectiveWorkspaceAuthority::Unavailable(WorkspaceUnavailableReason::NotConfigured);
    };
    if !runtime.is_ready() {
        return EffectiveWorkspaceAuthority::Unavailable(
            WorkspaceUnavailableReason::RuntimeNotReady,
        );
    }
    if observed != Some(desired.execution_path.as_path()) {
        return EffectiveWorkspaceAuthority::Unavailable(
            WorkspaceUnavailableReason::DesiredObservedDiverged,
        );
    }
    EffectiveWorkspaceAuthority::Available(desired.clone())
}

fn derive_services(intent: ServiceIntent, runtime: &RuntimeState) -> EffectiveServiceState {
    if intent == ServiceIntent::Disabled {
        return EffectiveServiceState::Disabled;
    }
    match runtime {
        RuntimeState::Ready => EffectiveServiceState::Available,
        RuntimeState::Faulted(_) => EffectiveServiceState::Unavailable,
        RuntimeState::Stopped => EffectiveServiceState::Reconciling,
        RuntimeState::StartingMcp
        | RuntimeState::WaitingMcpReady
        | RuntimeState::StartingPolicyEnforcement
        | RuntimeState::WaitingPolicyReady
        | RuntimeState::StartingTunnel
        | RuntimeState::WaitingTunnelReady
        | RuntimeState::Recovering { .. }
        | RuntimeState::SwitchingWorkspace { .. } => EffectiveServiceState::Reconciling,
    }
}

fn derive_connection(
    desired: Option<&ConnectionProfile>,
    runtime: &RuntimeState,
    observed: Option<&ConnectionProfile>,
) -> EffectiveConnection {
    let Some(desired) = desired else {
        return EffectiveConnection::Unavailable(ConnectionUnavailableReason::NotConfigured);
    };
    if !runtime.is_ready() {
        return EffectiveConnection::Unavailable(ConnectionUnavailableReason::RuntimeNotReady);
    }
    if observed != Some(desired) {
        return EffectiveConnection::Unavailable(
            ConnectionUnavailableReason::DesiredObservedDiverged,
        );
    }
    EffectiveConnection::Available(desired.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{GenerationId, PrivilegeFault};

    fn workspace(id: &str, path: &str) -> DesiredWorkspace {
        DesiredWorkspace::new(
            WorkspaceId::from_validated(id).unwrap(),
            PathBuf::from(path),
        )
    }

    fn connection(epoch: u64) -> ConnectionProfile {
        ConnectionProfile::new(
            TunnelId::new("tunnel_0123456789abcdef0123456789abcdef").unwrap(),
            epoch,
        )
    }

    fn observed(
        broker: PrivilegeState,
        runtime: RuntimeState,
        workspace: Option<&str>,
        connection: Option<ConnectionProfile>,
    ) -> ObservedState {
        ObservedState {
            broker,
            runtime,
            workspace: workspace.map(PathBuf::from),
            connection,
        }
    }

    #[test]
    fn desired_owner_has_one_revisioned_mutation_point() {
        let owner = DesiredStateOwner::default();
        let initial = owner.snapshot();
        owner.set_permission(PermissionMode::Full);
        let changed = owner.snapshot();
        assert_eq!(changed.revision, initial.revision + 1);
        owner.set_permission(PermissionMode::Full);
        assert_eq!(owner.snapshot().revision, changed.revision);
        let clone = owner.clone();
        clone.set_services(ServiceIntent::Enabled);
        assert_eq!(owner.snapshot(), clone.snapshot());
    }

    #[test]
    fn desired_elevated_with_offline_broker_is_observable_and_non_elevated() {
        let owner = DesiredStateOwner::default();
        owner.set_permission(PermissionMode::Elevated);
        let snapshot = ConvergenceSnapshot::derive(
            owner.snapshot(),
            observed(
                PrivilegeState::Faulted(PrivilegeFault::BrokerExited),
                RuntimeState::Stopped,
                None,
                None,
            ),
        );
        assert_eq!(snapshot.desired.permission, PermissionMode::Elevated);
        assert_eq!(snapshot.effective.authority.execution, PermissionMode::Full);
        assert_eq!(
            snapshot.effective.authority.structured_paths,
            StructuredPathAuthority::ActiveWorkspace
        );
        assert_eq!(
            snapshot.effective.authority.reconciliation,
            AuthorityReconciliation::BrokerUnavailable
        );
    }

    #[test]
    fn effective_authority_owns_structured_path_route() {
        let full = derive_authority(PermissionMode::Full, &PrivilegeState::Disabled);
        assert_eq!(
            full.structured_paths,
            StructuredPathAuthority::ActiveWorkspace
        );

        let awaiting = derive_authority(PermissionMode::Elevated, &PrivilegeState::AwaitingUac);
        assert_eq!(
            awaiting.structured_paths,
            StructuredPathAuthority::ActiveWorkspace
        );

        let active = derive_authority(
            PermissionMode::Elevated,
            &PrivilegeState::Active {
                broker_generation: GenerationId::new(91),
            },
        );
        assert_eq!(
            active.structured_paths,
            StructuredPathAuthority::AdministratorBroker
        );
    }

    #[test]
    fn desired_and_observed_workspace_divergence_fails_closed() {
        let owner = DesiredStateOwner::default();
        owner.replace(DesiredState {
            permission: PermissionMode::Full,
            workspace: Some(workspace("b", "B")),
            services: ServiceIntent::Enabled,
            connection: Some(connection(0)),
        });
        let snapshot = ConvergenceSnapshot::derive(
            owner.snapshot(),
            observed(
                PrivilegeState::Disabled,
                RuntimeState::Ready,
                Some("A"),
                Some(connection(0)),
            ),
        );
        assert_eq!(
            snapshot.effective.workspace,
            EffectiveWorkspaceAuthority::Unavailable(
                WorkspaceUnavailableReason::DesiredObservedDiverged
            )
        );
        assert!(!snapshot.effective.work_is_authorized());
    }

    #[test]
    fn all_inputs_must_converge_before_work_is_authorized() {
        let owner = DesiredStateOwner::default();
        owner.replace(DesiredState {
            permission: PermissionMode::Elevated,
            workspace: Some(workspace("a", "A")),
            services: ServiceIntent::Enabled,
            connection: Some(connection(2)),
        });
        let snapshot = ConvergenceSnapshot::derive(
            owner.snapshot(),
            observed(
                PrivilegeState::Active {
                    broker_generation: GenerationId::new(4),
                },
                RuntimeState::Ready,
                Some("A"),
                Some(connection(2)),
            ),
        );
        assert!(snapshot.effective.work_is_authorized());
        assert_eq!(
            snapshot.effective.authority.structured_paths,
            StructuredPathAuthority::AdministratorBroker
        );
    }

    #[test]
    fn connection_epoch_divergence_is_fail_closed_until_restart_observes_it() {
        let owner = DesiredStateOwner::default();
        owner.replace(DesiredState {
            permission: PermissionMode::Full,
            workspace: Some(workspace("a", "A")),
            services: ServiceIntent::Enabled,
            connection: Some(connection(1)),
        });
        let snapshot = ConvergenceSnapshot::derive(
            owner.snapshot(),
            observed(
                PrivilegeState::Disabled,
                RuntimeState::Ready,
                Some("A"),
                Some(connection(0)),
            ),
        );
        assert_eq!(
            snapshot.effective.connection,
            EffectiveConnection::Unavailable(ConnectionUnavailableReason::DesiredObservedDiverged)
        );
        assert!(!snapshot.effective.work_is_authorized());
        assert_eq!(
            Reconciler::plan(&snapshot).runtime,
            RuntimeReconcileAction::RestartConnection
        );
    }

    #[test]
    fn reconciler_plans_workspace_apply_without_authorizing_the_old_workspace() {
        let owner = DesiredStateOwner::default();
        owner.replace(DesiredState {
            permission: PermissionMode::Full,
            workspace: Some(workspace("b", "B")),
            services: ServiceIntent::Enabled,
            connection: Some(connection(0)),
        });
        let snapshot = ConvergenceSnapshot::derive(
            owner.snapshot(),
            observed(
                PrivilegeState::Disabled,
                RuntimeState::Ready,
                Some("A"),
                Some(connection(0)),
            ),
        );
        assert_eq!(
            Reconciler::plan(&snapshot).runtime,
            RuntimeReconcileAction::ApplyWorkspace(PathBuf::from("B"))
        );
        assert!(!snapshot.effective.work_is_authorized());
    }
}
