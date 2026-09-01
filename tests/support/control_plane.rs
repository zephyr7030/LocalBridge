use std::path::Path;

use localbridge_lib::control_plane::convergence::{
    ConvergenceSnapshot, DesiredStateOwner, EffectiveConnection, EffectiveWorkspaceAuthority,
    ObservedState,
};
use localbridge_lib::control_plane::snapshot::{
    AuthorityProjection, ConnectionProjection, ControlPlaneSnapshotOwner,
    ControlPlaneSnapshotReader, EffectiveAvailability, ProjectionSection, RuntimeProjection,
    SnapshotDraft, WorkspaceProjection,
};
use localbridge_lib::state::{PrivilegeState, RuntimeState};

pub fn ready_control_plane(
    desired: &DesiredStateOwner,
    workspace: &Path,
    broker: PrivilegeState,
) -> ControlPlaneSnapshotReader {
    let convergence = ConvergenceSnapshot::derive(
        desired.snapshot(),
        ObservedState {
            broker: broker.clone(),
            runtime: RuntimeState::Ready,
            workspace: Some(workspace.to_path_buf()),
            connection: None,
        },
    );
    let owner = ControlPlaneSnapshotOwner::default();
    let previous = owner.read();
    owner
        .initialize(SnapshotDraft {
            runtime: ProjectionSection::ready(RuntimeProjection {
                active: true,
                state: RuntimeState::Ready,
                local_environment_available: Some(true),
                current_task_elapsed_ms: None,
                last_tool: None,
                outage: None,
            }),
            authority: ProjectionSection::ready(AuthorityProjection {
                desired: convergence.effective.authority.configured,
                effective: convergence.effective.authority.execution,
                broker,
                structured_paths: convergence.effective.authority.structured_paths,
                reconciliation: convergence.effective.authority.reconciliation,
            }),
            workspace: ProjectionSection::ready(WorkspaceProjection {
                desired_id: convergence
                    .desired
                    .workspace
                    .as_ref()
                    .and_then(|workspace| workspace.id.as_ref())
                    .map(|id| id.as_str().to_owned()),
                desired_path: convergence
                    .desired
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.execution_path.to_string_lossy().into_owned()),
                observed_path: Some(workspace.to_string_lossy().into_owned()),
                effective: match convergence.effective.workspace {
                    EffectiveWorkspaceAuthority::Available(_) => EffectiveAvailability::Available,
                    EffectiveWorkspaceAuthority::Unavailable(_) => {
                        EffectiveAvailability::Unavailable
                    }
                },
            }),
            connection: ProjectionSection::ready(ConnectionProjection {
                desired_tunnel_id: None,
                desired_credential_epoch: None,
                observed_tunnel_id: None,
                observed_credential_epoch: None,
                effective: match convergence.effective.connection {
                    EffectiveConnection::Available(_) => EffectiveAvailability::Available,
                    EffectiveConnection::Unavailable(_) => EffectiveAvailability::Unavailable,
                },
            }),
            scheduler: previous.scheduler,
            settings: previous.settings,
            activity: previous.activity,
            update: previous.update,
            active_faults: previous.active_faults,
        })
        .expect("test control-plane snapshot initializes exactly once");
    owner.reader()
}
