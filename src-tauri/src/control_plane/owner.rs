use super::convergence::DesiredStateOwner;
use std::path::Path;

use super::execution_registry::{ExecutionRegistry, ExecutionRegistryError};
use super::request_registry::RequestRegistry;
use super::scheduler::Scheduler;
use super::session_registry::{SessionReaper, SessionRegistry};
use super::task_registry::TaskRegistry;

#[derive(Clone)]
pub(crate) struct ControlPlane {
    desired: DesiredStateOwner,
    requests: RequestRegistry,
    tasks: TaskRegistry,
    executions: ExecutionRegistry,
    scheduler: Scheduler,
    sessions: SessionRegistry,
}

impl ControlPlane {
    pub(crate) fn for_workspace(
        desired: DesiredStateOwner,
        workspace: &Path,
    ) -> Result<Self, ExecutionRegistryError> {
        Ok(Self::new(
            desired,
            ExecutionRegistry::for_workspace(workspace)?,
        ))
    }

    pub(crate) fn new(desired: DesiredStateOwner, executions: ExecutionRegistry) -> Self {
        Self {
            desired,
            requests: RequestRegistry::default(),
            tasks: TaskRegistry::default(),
            executions,
            scheduler: Scheduler::default(),
            sessions: SessionRegistry::default(),
        }
    }

    pub(crate) fn desired(&self) -> DesiredStateOwner {
        self.desired.clone()
    }

    pub(crate) fn requests(&self) -> RequestRegistry {
        self.requests.clone()
    }

    pub(crate) fn tasks(&self) -> TaskRegistry {
        self.tasks.clone()
    }

    pub(crate) fn executions(&self) -> ExecutionRegistry {
        self.executions.clone()
    }

    pub(crate) fn scheduler(&self) -> Scheduler {
        self.scheduler.clone()
    }

    pub(crate) fn sessions(&self) -> SessionRegistry {
        self.sessions.clone()
    }

    pub(crate) fn session_reaper(&self, ttl_ms: u64) -> SessionReaper {
        SessionReaper::new(self.sessions(), ttl_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane::request_registry::RequestCancellationTarget;
    use crate::domain::{McpSessionId, RequestKey, RpcRequestId};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn composition_root_shares_one_session_scoped_request_owner() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let state_path = std::env::temp_dir().join(format!(
            "localbridge-control-plane-owner-{}-{nonce}.json",
            std::process::id()
        ));
        let control_plane = ControlPlane::new(
            DesiredStateOwner::default(),
            ExecutionRegistry::open_at(state_path.clone()).unwrap(),
        );
        let requests_from_transport = control_plane.requests();
        let requests_from_controller = control_plane.requests();
        let session_a = McpSessionId::new("session-a");
        let session_b = McpSessionId::new("session-b");
        let request_a = RequestKey::new(session_a, RpcRequestId::Number(1));
        let request_b = RequestKey::new(session_b, RpcRequestId::Number(1));

        requests_from_transport
            .register(
                request_a.clone(),
                RequestCancellationTarget::Runtime(RpcRequestId::Number(101)),
            )
            .unwrap();
        requests_from_transport
            .register(
                request_b.clone(),
                RequestCancellationTarget::Runtime(RpcRequestId::Number(102)),
            )
            .unwrap();

        assert!(requests_from_controller.remove(&request_a).is_some());
        assert!(requests_from_controller.get(&request_a).is_none());
        assert!(requests_from_controller.get(&request_b).is_some());
        let _ = std::fs::remove_file(state_path);
    }
}
