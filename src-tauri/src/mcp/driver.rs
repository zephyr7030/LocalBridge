use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::control_plane::convergence::{ConnectionProfile, DesiredStateOwner};
use crate::control_plane::snapshot::TaskAggregate;
use crate::credentials::CredentialStore;
use crate::execution::CapabilityPolicy;
use crate::privilege::PrivilegedExecution;
use crate::runtime::{RecoveryPermit, RuntimeDriver};
use crate::state::{CurrentTaskStatus, CurrentTaskTiming, PermissionMode, RuntimeFault};
use crate::tunnel::{
    ConnectorEndpoint, PreparedTunnelStart, TunnelId, TunnelRuntime, TunnelRuntimeConfig,
};
use crate::workspace::WorkspaceValidator;

use super::{
    CodingRuntimeHealthState, CodingToolsPermissionMode, CodingToolsRuntime,
    CodingToolsRuntimeConfig, CurrentTaskWake, InternalBearer, PolicyEnforcementError,
    PolicyEnforcementRuntime,
};

#[derive(Debug, Clone)]
pub struct ProductionRuntimeConfig {
    pub install_root: PathBuf,
    pub workspace: PathBuf,
    workspace_identity: Option<String>,
    pub health_state_dir: PathBuf,
    pub tunnel_id: TunnelId,
    pub mcp_readiness_timeout: Duration,
    pub tunnel_readiness_timeout: Duration,
}

impl ProductionRuntimeConfig {
    pub fn new(
        install_root: impl Into<PathBuf>,
        workspace: impl Into<PathBuf>,
        health_state_dir: impl Into<PathBuf>,
        tunnel_id: TunnelId,
    ) -> Self {
        let workspace = workspace.into();
        let workspace_identity = WorkspaceValidator
            .validate(&workspace)
            .ok()
            .map(|validated| validated.identity().as_str().to_owned());
        Self {
            install_root: install_root.into(),
            workspace,
            workspace_identity,
            health_state_dir: health_state_dir.into(),
            tunnel_id,
            mcp_readiness_timeout: Duration::from_secs(10),
            tunnel_readiness_timeout: Duration::from_secs(15),
        }
    }

    pub(crate) fn workspace_identity(&self) -> Option<&str> {
        self.workspace_identity.as_deref()
    }
}

enum CredentialStoreHandle<'a, C> {
    Borrowed(&'a C),
    Owned(C),
}

impl<C> CredentialStoreHandle<'_, C> {
    fn as_ref(&self) -> &C {
        match self {
            Self::Borrowed(store) => store,
            Self::Owned(store) => store,
        }
    }
}

pub struct ProductionRuntimeDriver<'a, C, B>
where
    C: CredentialStore,
    B: FnMut() -> Result<InternalBearer, RuntimeFault>,
{
    config: ProductionRuntimeConfig,
    credential_store: CredentialStoreHandle<'a, C>,
    bearer_factory: B,
    privileged_execution: Option<Arc<dyn PrivilegedExecution>>,
    task_projection_wake: Option<CurrentTaskWake>,
    desired_state: Option<DesiredStateOwner>,
    observed_connection: Option<ConnectionProfile>,
}

impl<'a, C, B> ProductionRuntimeDriver<'a, C, B>
where
    C: CredentialStore,
    B: FnMut() -> Result<InternalBearer, RuntimeFault>,
{
    pub fn new(
        config: ProductionRuntimeConfig,
        credential_store: &'a C,
        bearer_factory: B,
    ) -> Self {
        Self {
            config,
            credential_store: CredentialStoreHandle::Borrowed(credential_store),
            bearer_factory,
            privileged_execution: None,
            task_projection_wake: None,
            desired_state: None,
            observed_connection: None,
        }
    }

    pub fn new_owned(
        config: ProductionRuntimeConfig,
        credential_store: C,
        bearer_factory: B,
    ) -> ProductionRuntimeDriver<'static, C, B>
    where
        C: 'static,
    {
        ProductionRuntimeDriver {
            config,
            credential_store: CredentialStoreHandle::Owned(credential_store),
            bearer_factory,
            privileged_execution: None,
            task_projection_wake: None,
            desired_state: None,
            observed_connection: None,
        }
    }

    pub fn with_privileged_execution(
        mut self,
        privileged_execution: Arc<dyn PrivilegedExecution>,
    ) -> Self {
        self.privileged_execution = Some(privileged_execution);
        self
    }

    pub fn with_task_projection_wake(mut self, wake: CurrentTaskWake) -> Self {
        self.task_projection_wake = Some(wake);
        self
    }

    pub fn with_control_plane_state(
        mut self,
        desired_state: DesiredStateOwner,
        observed_connection: Option<ConnectionProfile>,
    ) -> Self {
        self.desired_state = Some(desired_state);
        self.observed_connection = observed_connection;
        self
    }

    pub fn config(&self) -> &ProductionRuntimeConfig {
        &self.config
    }
}

impl<C, B> RuntimeDriver for ProductionRuntimeDriver<'_, C, B>
where
    C: CredentialStore,
    B: FnMut() -> Result<InternalBearer, RuntimeFault>,
{
    type Mcp = CodingToolsRuntime;
    type Pep = PolicyEnforcementRuntime;
    type Tunnel = TunnelRuntime;

    fn start_mcp(&mut self) -> Result<Self::Mcp, RuntimeFault> {
        let port = available_loopback_port()?;
        let bearer = (self.bearer_factory)()?;
        let workspace_identity = self
            .config
            .workspace_identity()
            .map(str::to_owned)
            .ok_or(RuntimeFault::WorkspaceInvalid)?;
        CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &self.config.install_root,
                &self.config.workspace,
                port,
                CodingToolsPermissionMode::Trusted,
            )
            .with_workspace_identity(workspace_identity),
            bearer,
            self.config.mcp_readiness_timeout,
        )
        .map_err(|error| error.runtime_fault())
    }

    fn confirm_mcp_ready(&mut self, mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> {
        if !mcp
            .root_is_running()
            .map_err(|error| error.runtime_fault())?
        {
            return Err(RuntimeFault::McpExited);
        }
        Ok(())
    }

    fn start_mcp_for_recovery(
        &mut self,
        permit: &RecoveryPermit,
    ) -> Result<Self::Mcp, RuntimeFault> {
        if permit.is_cancelled() {
            return Err(RuntimeFault::UserStopped);
        }
        let port = available_loopback_port()?;
        let bearer = (self.bearer_factory)()?;
        let workspace_identity = self
            .config
            .workspace_identity()
            .map(str::to_owned)
            .ok_or(RuntimeFault::WorkspaceInvalid)?;
        CodingToolsRuntime::start_for_recovery(
            CodingToolsRuntimeConfig::new(
                &self.config.install_root,
                &self.config.workspace,
                port,
                CodingToolsPermissionMode::Trusted,
            )
            .with_workspace_identity(workspace_identity),
            bearer,
            self.config.mcp_readiness_timeout,
            Duration::from_millis(250),
            || permit.is_cancelled(),
        )
        .map_err(|error| error.runtime_fault())
    }

    fn start_pep(&mut self, mcp: Self::Mcp) -> Result<Self::Pep, RuntimeFault> {
        let policy = CapabilityPolicy::load(&self.config.install_root.join("runtime-policy.toml"))
            .map_err(|_| RuntimeFault::PolicyInvalid)?;
        if let Some(desired_state) = self.desired_state.as_ref() {
            return PolicyEnforcementRuntime::start_with_control_plane(
                mcp,
                policy,
                desired_state.clone(),
                self.observed_connection.clone(),
                self.privileged_execution.clone(),
                self.task_projection_wake.clone(),
            )
            .map_err(policy_runtime_fault);
        }
        match (
            self.privileged_execution.as_ref(),
            self.task_projection_wake.as_ref(),
        ) {
            (Some(privileged_execution), Some(wake)) => {
                PolicyEnforcementRuntime::start_with_privilege_and_wake(
                    mcp,
                    policy,
                    PermissionMode::Edit,
                    Arc::clone(privileged_execution),
                    Arc::clone(wake),
                )
            }
            (Some(privileged_execution), None) => PolicyEnforcementRuntime::start_with_privilege(
                mcp,
                policy,
                PermissionMode::Edit,
                Arc::clone(privileged_execution),
            ),
            (None, Some(wake)) => PolicyEnforcementRuntime::start_with_wake(
                mcp,
                policy,
                PermissionMode::Edit,
                Arc::clone(wake),
            ),
            (None, None) => PolicyEnforcementRuntime::start(mcp, policy, PermissionMode::Edit),
        }
        .map_err(policy_runtime_fault)
    }

    fn confirm_pep_ready(&mut self, pep: &Self::Pep) -> Result<(), RuntimeFault> {
        if pep.is_running() {
            Ok(())
        } else {
            Err(RuntimeFault::PolicyInvalid)
        }
    }

    fn start_tunnel(&mut self, pep: &Self::Pep) -> Result<Self::Tunnel, RuntimeFault> {
        let config = TunnelRuntimeConfig::new(
            &self.config.install_root,
            &self.config.health_state_dir,
            self.config.tunnel_id.clone(),
            pep.port(),
        )
        .map_err(|error| error.runtime_fault())?;
        PreparedTunnelStart::prepare(config, self.credential_store.as_ref())
            .and_then(PreparedTunnelStart::spawn)
            .map_err(|error| error.runtime_fault())
    }

    fn confirm_tunnel_ready(&mut self, tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> {
        tunnel
            .wait_ready(self.config.tunnel_readiness_timeout)
            .map_err(|error| error.runtime_fault())
    }

    fn start_tunnel_for_recovery(
        &mut self,
        pep: &Self::Pep,
        permit: &RecoveryPermit,
    ) -> Result<Self::Tunnel, RuntimeFault> {
        if permit.is_cancelled() {
            return Err(RuntimeFault::UserStopped);
        }
        let config = TunnelRuntimeConfig::new(
            &self.config.install_root,
            &self.config.health_state_dir,
            self.config.tunnel_id.clone(),
            pep.port(),
        )
        .map_err(|error| error.runtime_fault())?;
        let tunnel = PreparedTunnelStart::prepare(config, self.credential_store.as_ref())
            .and_then(PreparedTunnelStart::spawn)
            .map_err(|error| error.runtime_fault())?;
        if permit.is_cancelled() {
            let mut tunnel = tunnel;
            let _ = tunnel.stop();
            return Err(RuntimeFault::UserStopped);
        }
        Ok(tunnel)
    }

    fn confirm_tunnel_ready_for_recovery(
        &mut self,
        tunnel: &mut Self::Tunnel,
        permit: &RecoveryPermit,
    ) -> Result<(), RuntimeFault> {
        let result = tunnel.wait_ready_for_recovery(
            self.config.tunnel_readiness_timeout,
            Duration::from_millis(250),
            || permit.is_cancelled(),
        );
        if permit.is_cancelled() {
            Err(RuntimeFault::UserStopped)
        } else {
            result.map_err(|error| error.runtime_fault())
        }
    }

    fn stop_tunnel(&mut self, tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> {
        tunnel
            .stop()
            .map(|_| ())
            .map_err(|error| error.runtime_fault())
    }

    fn stop_pep(&mut self, pep: Self::Pep) -> Result<Self::Mcp, RuntimeFault> {
        pep.stop().map_err(policy_runtime_fault)
    }

    fn stop_mcp(&mut self, mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> {
        mcp.stop()
            .map(|_| ())
            .map_err(|error| error.runtime_fault())
    }

    fn current_task(&self, pep: &Self::Pep) -> CurrentTaskStatus {
        pep.current_task_projection().snapshot()
    }

    fn current_task_timing(&self, pep: &Self::Pep) -> CurrentTaskTiming {
        pep.current_task_projection().timing_snapshot()
    }

    fn task_aggregate(&self, pep: &Self::Pep) -> TaskAggregate {
        pep.control_plane_activity_snapshot()
    }

    fn connector_endpoint(&self, tunnel: &Self::Tunnel) -> Option<ConnectorEndpoint> {
        tunnel.connector_endpoint()
    }

    fn connection_profile(&self) -> Option<ConnectionProfile> {
        self.observed_connection.clone()
    }

    fn probe_mcp_health(&mut self, pep: &Self::Pep) -> Result<(), RuntimeFault> {
        if let Some(fault) = pep.take_coding_runtime_fault() {
            return Err(fault);
        }
        match pep.coding_runtime_health() {
            Ok(Some(health))
                if health.state == CodingRuntimeHealthState::Ready && health.authenticated_mcp =>
            {
                Ok(())
            }
            Ok(Some(health)) => Err(health.fault.unwrap_or(RuntimeFault::McpHealthTimeout)),
            Ok(None) => Ok(()),
            Err(_) => Err(RuntimeFault::McpHealthTimeout),
        }
    }

    fn probe_pep_health(&mut self, pep: &Self::Pep) -> Result<(), RuntimeFault> {
        if pep.is_running() {
            Ok(())
        } else {
            Err(RuntimeFault::PolicyBindFailed)
        }
    }

    fn probe_tunnel_health(&mut self, tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> {
        tunnel
            .wait_ready_for_recovery(Duration::ZERO, Duration::from_millis(250), || false)
            .map_err(|error| error.runtime_fault())
    }

    fn current_workspace(&self) -> Option<&Path> {
        Some(&self.config.workspace)
    }

    fn configure_workspace(&mut self, workspace: PathBuf) -> Result<(), RuntimeFault> {
        if workspace.as_os_str().is_empty() {
            return Err(RuntimeFault::WorkspaceInvalid);
        }
        let validated = WorkspaceValidator
            .validate(&workspace)
            .map_err(|_| RuntimeFault::WorkspaceInvalid)?;
        self.config.workspace_identity = Some(validated.identity().as_str().to_owned());
        self.config.workspace = workspace;
        Ok(())
    }
}

fn available_loopback_port() -> Result<u16, RuntimeFault> {
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_| RuntimeFault::PortUnavailable)?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|_| RuntimeFault::PortUnavailable)
}

fn policy_runtime_fault(error: PolicyEnforcementError) -> RuntimeFault {
    match error {
        PolicyEnforcementError::BindFailed => RuntimeFault::PolicyBindFailed,
        PolicyEnforcementError::UpstreamCancellationUnavailable
        | PolicyEnforcementError::UpstreamHealthUnavailable
        | PolicyEnforcementError::UpstreamFacadeNegotiationFailed
        | PolicyEnforcementError::ThreadSpawnFailed
        | PolicyEnforcementError::ThreadTerminated => RuntimeFault::PolicyInvalid,
    }
}

#[cfg(all(test, windows))]
#[test]
fn production_runtime_config_keeps_workspace_identity_after_same_path_replacement() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!(
        "localbridge-runtime-identity-{}-{nonce}",
        std::process::id()
    ));
    let displaced = workspace.with_extension("original");
    std::fs::create_dir(&workspace).unwrap();
    let config = ProductionRuntimeConfig::new(
        std::env::temp_dir(),
        &workspace,
        workspace.join("health"),
        TunnelId::new("tunnel_0123456789abcdef0123456789abcdef").unwrap(),
    );
    let original_identity = config.workspace_identity().unwrap().to_owned();

    std::fs::rename(&workspace, &displaced).unwrap();
    std::fs::create_dir(&workspace).unwrap();
    let replacement_identity = WorkspaceValidator
        .validate(&workspace)
        .unwrap()
        .identity()
        .as_str()
        .to_owned();
    assert_ne!(original_identity, replacement_identity);
    assert_eq!(
        config.workspace_identity(),
        Some(original_identity.as_str())
    );

    std::fs::remove_dir_all(&workspace).unwrap();
    std::fs::rename(&displaced, &workspace).unwrap();
    std::fs::remove_dir_all(&workspace).unwrap();
}
