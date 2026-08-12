use std::fmt;
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::credentials::CredentialStore;
use crate::mcp::{
    CapabilityPolicy, CodingToolsPermissionMode, CodingToolsRuntime, CodingToolsRuntimeConfig,
    InternalBearer, PolicyEnforcementError, PolicyEnforcementRuntime,
};
use crate::privilege::PrivilegedExecution;
use crate::state::{
    CurrentTaskStatus, PermissionMode, RuntimeComponent, RuntimeFault, RuntimeState,
};
use crate::tunnel::{PreparedTunnelStart, TunnelId, TunnelRuntime, TunnelRuntimeConfig};

pub trait RuntimeDriver {
    type Mcp;
    type Pep;
    type Tunnel;

    fn start_mcp(&mut self) -> Result<Self::Mcp, RuntimeFault>;
    fn confirm_mcp_ready(&mut self, mcp: &mut Self::Mcp) -> Result<(), RuntimeFault>;

    /// Ownership transfers to the driver. A failed PEP start must not leak the MCP runtime.
    fn start_pep(&mut self, mcp: Self::Mcp) -> Result<Self::Pep, RuntimeFault>;
    fn confirm_pep_ready(&mut self, pep: &Self::Pep) -> Result<(), RuntimeFault>;

    fn start_tunnel(&mut self, pep: &Self::Pep) -> Result<Self::Tunnel, RuntimeFault>;
    fn confirm_tunnel_ready(&mut self, tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault>;

    fn stop_tunnel(&mut self, tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault>;
    fn stop_pep(&mut self, pep: Self::Pep) -> Result<Self::Mcp, RuntimeFault>;
    fn stop_mcp(&mut self, mcp: &mut Self::Mcp) -> Result<(), RuntimeFault>;

    fn current_task(&self, pep: &Self::Pep) -> CurrentTaskStatus;

    fn current_workspace(&self) -> Option<&Path> {
        None
    }

    fn configure_workspace(&mut self, _workspace: PathBuf) -> Result<(), RuntimeFault> {
        Err(RuntimeFault::ConfigurationInvalid)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryScope {
    Tunnel,
    PolicyAndTunnel,
    FullRuntime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSwitchError {
    pub candidate_fault: RuntimeFault,
    pub rollback_fault: Option<RuntimeFault>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestratorError {
    pub fault: RuntimeFault,
    pub cleanup_fault: Option<RuntimeFault>,
}

impl fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(cleanup_fault) = &self.cleanup_fault {
            write!(
                f,
                "runtime orchestration failed: {:?}; cleanup also failed: {:?}",
                self.fault, cleanup_fault
            )
        } else {
            write!(f, "runtime orchestration failed: {:?}", self.fault)
        }
    }
}

impl std::error::Error for OrchestratorError {}

struct ReadyHandles<D: RuntimeDriver> {
    pep: D::Pep,
    tunnel: D::Tunnel,
}

pub struct RuntimeOrchestrator<D: RuntimeDriver> {
    driver: D,
    state: RuntimeState,
    ready: Option<ReadyHandles<D>>,
    recovering_pep: Option<D::Pep>,
    recovering_mcp: Option<D::Mcp>,
    outages: OutageTracker,
}

impl<D: RuntimeDriver> fmt::Debug for RuntimeOrchestrator<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeOrchestrator")
            .field("state", &self.state)
            .field("has_ready_runtime", &self.ready.is_some())
            .field("has_recovering_pep", &self.recovering_pep.is_some())
            .field("has_recovering_mcp", &self.recovering_mcp.is_some())
            .field("current_task", &self.current_task())
            .field("outage", &self.outages.active())
            .finish()
    }
}

impl<D: RuntimeDriver> RuntimeOrchestrator<D> {
    pub fn new(driver: D) -> Self {
        Self {
            driver,
            state: RuntimeState::Stopped,
            ready: None,
            recovering_pep: None,
            recovering_mcp: None,
            outages: OutageTracker::default(),
        }
    }

    pub fn state(&self) -> &RuntimeState {
        &self.state
    }

    pub fn current_task(&self) -> CurrentTaskStatus {
        self.ready
            .as_ref()
            .map(|ready| self.driver.current_task(&ready.pep))
            .or_else(|| {
                self.recovering_pep
                    .as_ref()
                    .map(|pep| self.driver.current_task(pep))
            })
            .unwrap_or(CurrentTaskStatus::Idle)
    }

    pub fn configured_workspace(&self) -> Option<&Path> {
        self.driver.current_workspace()
    }

    pub fn start(&mut self) -> Result<(), OrchestratorError> {
        self.start_with_state_projection(|_| {})
    }

    pub fn start_with_state_projection<F>(&mut self, mut project: F) -> Result<(), OrchestratorError>
    where
        F: FnMut(&RuntimeState),
    {
        if self.ready.is_some()
            || self.recovering_pep.is_some()
            || self.recovering_mcp.is_some()
            || self.state != RuntimeState::Stopped
        {
            return Err(OrchestratorError {
                fault: RuntimeFault::ConfigurationInvalid,
                cleanup_fault: None,
            });
        }

        self.transition(RuntimeState::StartingMcp, &mut project);
        let mut mcp = match self.driver.start_mcp() {
            Ok(mcp) => mcp,
            Err(fault) => return Err(self.fail_without_cleanup(fault, &mut project)),
        };

        self.transition(RuntimeState::WaitingMcpReady, &mut project);
        if let Err(fault) = self.driver.confirm_mcp_ready(&mut mcp) {
            let cleanup_fault = self.driver.stop_mcp(&mut mcp).err();
            return Err(self.fail(fault, cleanup_fault, &mut project));
        }

        self.transition(RuntimeState::StartingPolicyEnforcement, &mut project);
        let pep = match self.driver.start_pep(mcp) {
            Ok(pep) => pep,
            Err(fault) => return Err(self.fail_without_cleanup(fault, &mut project)),
        };

        self.transition(RuntimeState::WaitingPolicyReady, &mut project);
        if let Err(fault) = self.driver.confirm_pep_ready(&pep) {
            let cleanup_fault = self.cleanup_pep(pep);
            return Err(self.fail(fault, cleanup_fault, &mut project));
        }

        self.transition(RuntimeState::StartingTunnel, &mut project);
        let mut tunnel = match self.driver.start_tunnel(&pep) {
            Ok(tunnel) => tunnel,
            Err(fault) => {
                let cleanup_fault = self.cleanup_pep(pep);
                return Err(self.fail(fault, cleanup_fault, &mut project));
            }
        };

        self.transition(RuntimeState::WaitingTunnelReady, &mut project);
        if let Err(fault) = self.driver.confirm_tunnel_ready(&mut tunnel) {
            let mut cleanup_fault = self.driver.stop_tunnel(&mut tunnel).err();
            merge_cleanup_fault(&mut cleanup_fault, self.cleanup_pep(pep));
            return Err(self.fail(fault, cleanup_fault, &mut project));
        }

        self.ready = Some(ReadyHandles { pep, tunnel });
        self.transition(RuntimeState::Ready, &mut project);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), OrchestratorError> {
        self.stop_with_state_projection(|_| {})
    }

    pub fn stop_with_state_projection<F>(&mut self, mut project: F) -> Result<(), OrchestratorError>
    where
        F: FnMut(&RuntimeState),
    {
        let mut cleanup_fault = None;
        if let Some(mut ready) = self.ready.take() {
            merge_cleanup_fault(
                &mut cleanup_fault,
                self.driver.stop_tunnel(&mut ready.tunnel).err(),
            );
            match self.driver.stop_pep(ready.pep) {
                Ok(mcp) => self.recovering_mcp = Some(mcp),
                Err(fault) => merge_cleanup_fault(&mut cleanup_fault, Some(fault)),
            }
        }
        if let Some(pep) = self.recovering_pep.take() {
            match self.driver.stop_pep(pep) {
                Ok(mcp) => self.recovering_mcp = Some(mcp),
                Err(fault) => merge_cleanup_fault(&mut cleanup_fault, Some(fault)),
            }
        }
        if let Some(mut mcp) = self.recovering_mcp.take() {
            merge_cleanup_fault(&mut cleanup_fault, self.driver.stop_mcp(&mut mcp).err());
        }

        if let Some(fault) = cleanup_fault {
            Err(self.fail(fault, None, &mut project))
        } else {
            self.transition(RuntimeState::Stopped, &mut project);
            Ok(())
        }
    }

    pub fn begin_outage(
        &mut self,
        component: RuntimeComponent,
        fault: RuntimeFault,
    ) -> OutageGenerationId {
        self.outages.begin(component, fault)
    }

    pub fn mark_user_attention_required(&mut self, generation: OutageGenerationId) -> bool {
        self.outages.mark_user_attention_required(generation)
    }

    pub fn clear_outage(&mut self, generation: OutageGenerationId) -> bool {
        self.outages.clear(generation)
    }

    pub fn active_outage(&self) -> Option<&OutageGeneration> {
        self.outages.active()
    }

    pub fn refresh_outage(
        &mut self,
        generation: OutageGenerationId,
        component: RuntimeComponent,
        fault: RuntimeFault,
    ) -> bool {
        self.outages.refresh(generation, component, fault)
    }

    pub fn record_fault(&mut self, fault: RuntimeFault) {
        self.state = RuntimeState::Faulted(fault);
    }

    pub fn recover_minimal(
        &mut self,
        scope: RecoveryScope,
        attempt: u32,
    ) -> Result<(), OrchestratorError> {
        let component = match scope {
            RecoveryScope::Tunnel => RuntimeComponent::Tunnel,
            RecoveryScope::PolicyAndTunnel => RuntimeComponent::PolicyEnforcement,
            RecoveryScope::FullRuntime => RuntimeComponent::CodingRuntime,
        };
        self.state = RuntimeState::Recovering { component, attempt };
        let result = match scope {
            RecoveryScope::Tunnel => self.recover_tunnel_only(),
            RecoveryScope::PolicyAndTunnel => self.recover_policy_and_tunnel(),
            RecoveryScope::FullRuntime => self.recover_full_runtime(),
        };
        match result {
            Ok(()) => {
                self.state = RuntimeState::Ready;
                Ok(())
            }
            Err(error) => {
                self.state = RuntimeState::Faulted(error.fault.clone());
                Err(error)
            }
        }
    }

    pub fn switch_workspace_to(
        &mut self,
        candidate: &Path,
        rollback_workspace: Option<&Path>,
    ) -> Result<(), WorkspaceSwitchError> {
        if candidate.as_os_str().is_empty() {
            return Err(WorkspaceSwitchError {
                candidate_fault: RuntimeFault::WorkspaceInvalid,
                rollback_fault: None,
            });
        }
        let previous = rollback_workspace.map(Path::to_path_buf);
        if let Err(error) = self.stop() {
            return Err(WorkspaceSwitchError {
                candidate_fault: error.fault,
                rollback_fault: error.cleanup_fault,
            });
        }
        if let Err(fault) = self.driver.configure_workspace(candidate.to_path_buf()) {
            return Err(WorkspaceSwitchError {
                candidate_fault: fault,
                rollback_fault: self.rollback_workspace(previous),
            });
        }
        if let Err(error) = self.start() {
            return Err(WorkspaceSwitchError {
                candidate_fault: error.fault,
                rollback_fault: self.rollback_workspace(previous),
            });
        }
        Ok(())
    }

    pub fn into_driver(self) -> D {
        self.driver
    }

    fn cleanup_pep(&mut self, pep: D::Pep) -> Option<RuntimeFault> {
        match self.driver.stop_pep(pep) {
            Ok(mut mcp) => self.driver.stop_mcp(&mut mcp).err(),
            Err(fault) => Some(fault),
        }
    }

    fn recover_tunnel_only(&mut self) -> Result<(), OrchestratorError> {
        if let Some(mut ready) = self.ready.take() {
            if let Err(fault) = self.driver.stop_tunnel(&mut ready.tunnel) {
                self.ready = Some(ready);
                return Err(OrchestratorError {
                    fault,
                    cleanup_fault: None,
                });
            }
            self.recovering_pep = Some(ready.pep);
        }
        if self.recovering_pep.is_none() {
            return if self.recovering_mcp.is_some() {
                self.recover_policy_and_tunnel()
            } else {
                self.recover_full_runtime()
            };
        }
        let pep = self.recovering_pep.as_ref().expect("checked retained PEP");
        if self.driver.confirm_pep_ready(pep).is_err() {
            return self.recover_policy_and_tunnel();
        }
        self.start_tunnel_from_recovering_pep()
    }

    fn start_tunnel_from_recovering_pep(&mut self) -> Result<(), OrchestratorError> {
        let pep = self
            .recovering_pep
            .as_ref()
            .expect("tunnel recovery requires retained PEP");
        let mut tunnel = self
            .driver
            .start_tunnel(pep)
            .map_err(|fault| OrchestratorError {
                fault,
                cleanup_fault: None,
            })?;
        if let Err(fault) = self.driver.confirm_tunnel_ready(&mut tunnel) {
            let cleanup_fault = self.driver.stop_tunnel(&mut tunnel).err();
            return Err(OrchestratorError {
                fault,
                cleanup_fault,
            });
        }
        let pep = self
            .recovering_pep
            .take()
            .expect("PEP remains owned through tunnel-only recovery");
        self.ready = Some(ReadyHandles { pep, tunnel });
        Ok(())
    }

    fn recover_policy_and_tunnel(&mut self) -> Result<(), OrchestratorError> {
        if let Some(mut ready) = self.ready.take() {
            if let Err(fault) = self.driver.stop_tunnel(&mut ready.tunnel) {
                self.ready = Some(ready);
                return Err(OrchestratorError {
                    fault,
                    cleanup_fault: None,
                });
            }
            self.recovering_pep = Some(ready.pep);
        }
        if let Some(pep) = self.recovering_pep.take() {
            match self.driver.stop_pep(pep) {
                Ok(mcp) => self.recovering_mcp = Some(mcp),
                Err(fault) => {
                    return Err(OrchestratorError {
                        fault,
                        cleanup_fault: None,
                    });
                }
            }
        }
        let Some(mut mcp) = self.recovering_mcp.take() else {
            return self.recover_full_runtime();
        };
        if self.driver.confirm_mcp_ready(&mut mcp).is_err() {
            self.recovering_mcp = Some(mcp);
            return self.recover_full_runtime();
        }
        let pep = self
            .driver
            .start_pep(mcp)
            .map_err(|fault| OrchestratorError {
                fault,
                cleanup_fault: None,
            })?;
        if let Err(fault) = self.driver.confirm_pep_ready(&pep) {
            match self.driver.stop_pep(pep) {
                Ok(mcp) => self.recovering_mcp = Some(mcp),
                Err(cleanup_fault) => {
                    return Err(OrchestratorError {
                        fault,
                        cleanup_fault: Some(cleanup_fault),
                    });
                }
            }
            return Err(OrchestratorError {
                fault,
                cleanup_fault: None,
            });
        }
        self.recovering_pep = Some(pep);
        self.start_tunnel_from_recovering_pep()
    }

    fn recover_full_runtime(&mut self) -> Result<(), OrchestratorError> {
        let cleanup_fault = self.stop().err().map(|error| error.fault);
        if let Some(fault) = cleanup_fault {
            return Err(OrchestratorError {
                fault,
                cleanup_fault: None,
            });
        }
        self.state = RuntimeState::Stopped;
        self.start()
    }

    fn rollback_workspace(&mut self, previous: Option<PathBuf>) -> Option<RuntimeFault> {
        let _ = self.stop();
        self.state = RuntimeState::Stopped;
        let previous = previous?;
        if let Err(fault) = self.driver.configure_workspace(previous) {
            return Some(fault);
        }
        self.start().err().map(|error| error.fault)
    }

    fn transition<F>(&mut self, state: RuntimeState, project: &mut F)
    where
        F: FnMut(&RuntimeState),
    {
        self.state = state;
        project(&self.state);
    }

    fn fail_without_cleanup<F>(&mut self, fault: RuntimeFault, project: &mut F) -> OrchestratorError
    where
        F: FnMut(&RuntimeState),
    {
        self.fail(fault, None, project)
    }

    fn fail<F>(
        &mut self,
        fault: RuntimeFault,
        cleanup_fault: Option<RuntimeFault>,
        project: &mut F,
    ) -> OrchestratorError
    where
        F: FnMut(&RuntimeState),
    {
        self.state = RuntimeState::Faulted(fault.clone());
        project(&self.state);
        OrchestratorError { fault, cleanup_fault }
    }
}

fn merge_cleanup_fault(target: &mut Option<RuntimeFault>, candidate: Option<RuntimeFault>) {
    if target.is_none() {
        *target = candidate;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutageGenerationId(u64);

impl OutageGenerationId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutageGeneration {
    pub id: OutageGenerationId,
    pub component: RuntimeComponent,
    pub fault: RuntimeFault,
    user_attention_emitted: bool,
}

impl OutageGeneration {
    pub const fn user_attention_emitted(&self) -> bool {
        self.user_attention_emitted
    }
}

#[derive(Debug, Default)]
pub struct OutageTracker {
    next_generation: u64,
    active: Option<OutageGeneration>,
}

impl OutageTracker {
    pub fn begin(&mut self, component: RuntimeComponent, fault: RuntimeFault) -> OutageGenerationId {
        self.next_generation = self.next_generation.saturating_add(1).max(1);
        let id = OutageGenerationId(self.next_generation);
        self.active = Some(OutageGeneration {
            id,
            component,
            fault,
            user_attention_emitted: false,
        });
        id
    }

    pub fn mark_user_attention_required(&mut self, generation: OutageGenerationId) -> bool {
        let Some(active) = self.active.as_mut().filter(|active| active.id == generation) else {
            return false;
        };
        if active.user_attention_emitted {
            false
        } else {
            active.user_attention_emitted = true;
            true
        }
    }

    pub fn refresh(
        &mut self,
        generation: OutageGenerationId,
        component: RuntimeComponent,
        fault: RuntimeFault,
    ) -> bool {
        let Some(active) = self.active.as_mut().filter(|active| active.id == generation) else {
            return false;
        };
        active.component = component;
        active.fault = fault;
        true
    }

    pub fn clear(&mut self, generation: OutageGenerationId) -> bool {
        if self.active.as_ref().is_some_and(|active| active.id == generation) {
            self.active = None;
            true
        } else {
            false
        }
    }

    pub fn active(&self) -> Option<&OutageGeneration> {
        self.active.as_ref()
    }
}

#[derive(Debug, Clone)]
pub struct ProductionRuntimeConfig {
    pub install_root: PathBuf,
    pub workspace: PathBuf,
    pub health_state_dir: PathBuf,
    pub tunnel_id: TunnelId,
    pub permission_mode: PermissionMode,
    pub mcp_readiness_timeout: Duration,
    pub tunnel_readiness_timeout: Duration,
}

impl ProductionRuntimeConfig {
    pub fn new(
        install_root: impl Into<PathBuf>,
        workspace: impl Into<PathBuf>,
        health_state_dir: impl Into<PathBuf>,
        tunnel_id: TunnelId,
        permission_mode: PermissionMode,
    ) -> Self {
        Self {
            install_root: install_root.into(),
            workspace: workspace.into(),
            health_state_dir: health_state_dir.into(),
            tunnel_id,
            permission_mode,
            mcp_readiness_timeout: Duration::from_secs(10),
            tunnel_readiness_timeout: Duration::from_secs(15),
        }
    }
}

pub struct ProductionRuntimeDriver<'a, C, B>
where
    C: CredentialStore,
    B: FnMut() -> Result<InternalBearer, RuntimeFault>,
{
    config: ProductionRuntimeConfig,
    credential_store: &'a C,
    bearer_factory: B,
    privileged_execution: Option<Arc<dyn PrivilegedExecution>>,
}

impl<'a, C, B> ProductionRuntimeDriver<'a, C, B>
where
    C: CredentialStore,
    B: FnMut() -> Result<InternalBearer, RuntimeFault>,
{
    pub fn new(config: ProductionRuntimeConfig, credential_store: &'a C, bearer_factory: B) -> Self {
        Self {
            config,
            credential_store,
            bearer_factory,
            privileged_execution: None,
        }
    }

    pub fn with_privileged_execution(
        mut self,
        privileged_execution: Arc<dyn PrivilegedExecution>,
    ) -> Self {
        self.privileged_execution = Some(privileged_execution);
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
        CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &self.config.install_root,
                &self.config.workspace,
                port,
                CodingToolsPermissionMode::Trusted,
            ),
            bearer,
            self.config.mcp_readiness_timeout,
        )
        .map_err(|error| error.runtime_fault())
    }

    fn confirm_mcp_ready(&mut self, mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> {
        if !mcp.root_is_running().map_err(|error| error.runtime_fault())? {
            return Err(RuntimeFault::McpExited);
        }
        Ok(())
    }

    fn start_pep(&mut self, mcp: Self::Mcp) -> Result<Self::Pep, RuntimeFault> {
        let policy = CapabilityPolicy::load(&self.config.install_root.join("runtime-policy.toml"))
            .map_err(|_| RuntimeFault::PolicyInvalid)?;
        match self.privileged_execution.as_ref() {
            Some(privileged_execution) => PolicyEnforcementRuntime::start_with_privilege(
                mcp,
                policy,
                self.config.permission_mode,
                Arc::clone(privileged_execution),
            ),
            None => PolicyEnforcementRuntime::start(mcp, policy, self.config.permission_mode),
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
        PreparedTunnelStart::prepare(config, self.credential_store)
            .and_then(PreparedTunnelStart::spawn)
            .map_err(|error| error.runtime_fault())
    }

    fn confirm_tunnel_ready(&mut self, tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> {
        tunnel
            .wait_ready(self.config.tunnel_readiness_timeout)
            .map_err(|error| error.runtime_fault())
    }

    fn stop_tunnel(&mut self, tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> {
        tunnel.stop().map(|_| ()).map_err(|error| error.runtime_fault())
    }

    fn stop_pep(&mut self, pep: Self::Pep) -> Result<Self::Mcp, RuntimeFault> {
        pep.stop().map_err(policy_runtime_fault)
    }

    fn stop_mcp(&mut self, mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> {
        mcp.stop().map(|_| ()).map_err(|error| error.runtime_fault())
    }

    fn current_task(&self, pep: &Self::Pep) -> CurrentTaskStatus {
        pep.current_task_projection().snapshot()
    }

    fn current_workspace(&self) -> Option<&Path> {
        Some(&self.config.workspace)
    }

    fn configure_workspace(&mut self, workspace: PathBuf) -> Result<(), RuntimeFault> {
        if workspace.as_os_str().is_empty() {
            return Err(RuntimeFault::WorkspaceInvalid);
        }
        self.config.workspace = workspace;
        Ok(())
    }
}

fn available_loopback_port() -> Result<u16, RuntimeFault> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_| RuntimeFault::PortUnavailable)?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|_| RuntimeFault::PortUnavailable)
}

fn policy_runtime_fault(error: PolicyEnforcementError) -> RuntimeFault {
    match error {
        PolicyEnforcementError::BindFailed => RuntimeFault::PolicyBindFailed,
        PolicyEnforcementError::UpstreamSessionUnavailable
        | PolicyEnforcementError::ThreadSpawnFailed
        | PolicyEnforcementError::ThreadTerminated => RuntimeFault::PolicyInvalid,
    }
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/orchestrator/orchestrator.rs"
    ));
}
