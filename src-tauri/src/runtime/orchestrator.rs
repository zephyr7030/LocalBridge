use serde_json::{Value, json};
use std::fmt;
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::credentials::CredentialStore;
use crate::mcp::{
    CapabilityPolicy, CodingRuntimeHealthState, CodingToolsPermissionMode, CodingToolsRuntime,
    CodingToolsRuntimeConfig, CurrentTaskWake, InternalBearer, PolicyEnforcementError,
    PolicyEnforcementRuntime,
};
use crate::privilege::PrivilegedExecution;
use crate::state::{
    CurrentTaskStatus, CurrentTaskTiming, PermissionMode, RuntimeComponent, RuntimeFault,
    RuntimeState,
};
use crate::tunnel::{
    ConnectorEndpoint, PreparedTunnelStart, TunnelId, TunnelRuntime, TunnelRuntimeConfig,
};
use crate::workspace::WorkspaceValidator;

use super::RecoveryPermit;

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

    fn start_mcp_for_recovery(
        &mut self,
        permit: &RecoveryPermit,
    ) -> Result<Self::Mcp, RuntimeFault> {
        if permit.is_cancelled() {
            Err(RuntimeFault::UserStopped)
        } else {
            self.start_mcp()
        }
    }

    fn confirm_mcp_ready_for_recovery(
        &mut self,
        mcp: &mut Self::Mcp,
        permit: &RecoveryPermit,
    ) -> Result<(), RuntimeFault> {
        if permit.is_cancelled() {
            Err(RuntimeFault::UserStopped)
        } else {
            self.confirm_mcp_ready(mcp)
        }
    }

    fn confirm_pep_ready_for_recovery(
        &mut self,
        pep: &Self::Pep,
        permit: &RecoveryPermit,
    ) -> Result<(), RuntimeFault> {
        if permit.is_cancelled() {
            Err(RuntimeFault::UserStopped)
        } else {
            self.confirm_pep_ready(pep)
        }
    }

    fn start_tunnel_for_recovery(
        &mut self,
        pep: &Self::Pep,
        permit: &RecoveryPermit,
    ) -> Result<Self::Tunnel, RuntimeFault> {
        if permit.is_cancelled() {
            Err(RuntimeFault::UserStopped)
        } else {
            self.start_tunnel(pep)
        }
    }

    fn confirm_tunnel_ready_for_recovery(
        &mut self,
        tunnel: &mut Self::Tunnel,
        permit: &RecoveryPermit,
    ) -> Result<(), RuntimeFault> {
        if permit.is_cancelled() {
            Err(RuntimeFault::UserStopped)
        } else {
            self.confirm_tunnel_ready(tunnel)
        }
    }

    fn stop_tunnel(&mut self, tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault>;
    fn stop_pep(&mut self, pep: Self::Pep) -> Result<Self::Mcp, RuntimeFault>;
    fn stop_mcp(&mut self, mcp: &mut Self::Mcp) -> Result<(), RuntimeFault>;

    fn current_task(&self, pep: &Self::Pep) -> CurrentTaskStatus;

    fn task_aggregate(&self, pep: &Self::Pep) -> Value {
        match self.current_task(pep) {
            CurrentTaskStatus::Idle => {
                json!({"state":"idle","current_workflow":null,"current_command":null,"last_command":null})
            }
            CurrentTaskStatus::Active(_) => {
                json!({"state":"active","current_workflow":null,"current_command":{"state":"running"},"last_command":null})
            }
        }
    }

    fn current_task_timing(&self, pep: &Self::Pep) -> CurrentTaskTiming {
        CurrentTaskTiming {
            status: self.current_task(pep),
            ..CurrentTaskTiming::default()
        }
    }

    fn connector_endpoint(&self, _tunnel: &Self::Tunnel) -> Option<ConnectorEndpoint> {
        None
    }

    fn probe_mcp_health(&mut self, _pep: &Self::Pep) -> Result<(), RuntimeFault> {
        Ok(())
    }

    fn probe_pep_health(&mut self, pep: &Self::Pep) -> Result<(), RuntimeFault> {
        self.confirm_pep_ready(pep)
    }

    fn probe_tunnel_health(&mut self, tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> {
        self.confirm_tunnel_ready(tunnel)
    }

    fn current_workspace(&self) -> Option<&Path> {
        None
    }

    fn configure_workspace(&mut self, _workspace: PathBuf) -> Result<(), RuntimeFault> {
        Err(RuntimeFault::ConfigurationInvalid)
    }

    fn configure_permission_mode(&mut self, _mode: PermissionMode) -> Result<(), RuntimeFault> {
        Err(RuntimeFault::ConfigurationInvalid)
    }

    fn set_permission_mode(
        &mut self,
        _pep: &Self::Pep,
        _mode: PermissionMode,
    ) -> Result<(), RuntimeFault> {
        Err(RuntimeFault::ConfigurationInvalid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHealthFailure {
    pub component: RuntimeComponent,
    pub fault: RuntimeFault,
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
    pub candidate_cleanup_fault: Option<RuntimeFault>,
    pub rollback_fault: Option<RuntimeFault>,
    pub rollback_cleanup_fault: Option<RuntimeFault>,
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

    #[cfg(test)]
    pub(crate) fn from_ready_for_test(driver: D, pep: D::Pep, tunnel: D::Tunnel) -> Self {
        Self {
            driver,
            state: RuntimeState::Ready,
            ready: Some(ReadyHandles { pep, tunnel }),
            recovering_pep: None,
            recovering_mcp: None,
            outages: OutageTracker::default(),
        }
    }

    pub fn state(&self) -> &RuntimeState {
        &self.state
    }

    pub fn current_task(&self) -> CurrentTaskStatus {
        self.current_task_timing().status
    }

    pub fn task_aggregate(&self) -> Value {
        self.ready
            .as_ref()
            .map(|ready| self.driver.task_aggregate(&ready.pep))
            .or_else(|| self.recovering_pep.as_ref().map(|pep| self.driver.task_aggregate(pep)))
            .unwrap_or_else(|| json!({"state":"idle","current_workflow":null,"current_command":null,"last_command":null}))
    }

    pub fn current_task_timing(&self) -> CurrentTaskTiming {
        self.ready
            .as_ref()
            .map(|ready| self.driver.current_task_timing(&ready.pep))
            .or_else(|| {
                self.recovering_pep
                    .as_ref()
                    .map(|pep| self.driver.current_task_timing(pep))
            })
            .unwrap_or_default()
    }

    pub fn configured_workspace(&self) -> Option<&Path> {
        self.driver.current_workspace()
    }

    pub fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        self.ready
            .as_ref()
            .and_then(|ready| self.driver.connector_endpoint(&ready.tunnel))
    }

    pub fn probe_ready_health(&mut self) -> Result<(), RuntimeHealthFailure> {
        if self.state != RuntimeState::Ready {
            return Ok(());
        }
        let Some(ready) = self.ready.as_mut() else {
            return Err(RuntimeHealthFailure {
                component: RuntimeComponent::CodingRuntime,
                fault: RuntimeFault::ConfigurationInvalid,
            });
        };
        self.driver
            .probe_mcp_health(&ready.pep)
            .map_err(|fault| RuntimeHealthFailure {
                component: RuntimeComponent::CodingRuntime,
                fault,
            })?;
        self.driver
            .probe_pep_health(&ready.pep)
            .map_err(|fault| RuntimeHealthFailure {
                component: RuntimeComponent::PolicyEnforcement,
                fault,
            })?;
        self.driver
            .probe_tunnel_health(&mut ready.tunnel)
            .map_err(|fault| RuntimeHealthFailure {
                component: RuntimeComponent::Tunnel,
                fault,
            })?;
        Ok(())
    }

    pub fn set_permission_mode(&mut self, mode: PermissionMode) -> Result<(), RuntimeFault> {
        if let Some(ready) = self.ready.as_ref() {
            return self.driver.set_permission_mode(&ready.pep, mode);
        }
        if let Some(pep) = self.recovering_pep.as_ref() {
            return self.driver.set_permission_mode(pep, mode);
        }
        self.driver.configure_permission_mode(mode)
    }

    pub fn start(&mut self) -> Result<(), OrchestratorError> {
        self.start_with_state_projection(|_| {})
    }

    pub fn start_with_state_projection<F>(
        &mut self,
        mut project: F,
    ) -> Result<(), OrchestratorError>
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
        let tunnel_fault = self.stop_tunnel_for_exit().err().map(|error| error.fault);
        let remaining = self.finish_exit_after_tunnel_with_state_projection(&mut project);
        match (tunnel_fault, remaining) {
            (None, result) => result,
            (Some(fault), Ok(())) => Err(self.fail(fault, None, &mut project)),
            (Some(fault), Err(error)) => Err(self.fail(fault, Some(error.fault), &mut project)),
        }
    }

    /// Stops only Tunnel and retains lower-layer ownership for desktop exit ordering.
    pub fn stop_tunnel_for_exit(&mut self) -> Result<(), OrchestratorError> {
        let Some(ready) = self.ready.take() else {
            return Ok(());
        };
        let ReadyHandles { pep, mut tunnel } = ready;
        let tunnel_stop = self.driver.stop_tunnel(&mut tunnel);
        drop(tunnel);
        self.recovering_pep = Some(pep);
        tunnel_stop.map_err(|fault| OrchestratorError {
            fault,
            cleanup_fault: None,
        })
    }

    /// Completes desktop exit after Tunnel shutdown by releasing PEP then MCP.
    pub fn finish_exit_after_tunnel(&mut self) -> Result<(), OrchestratorError> {
        self.finish_exit_after_tunnel_with_state_projection(&mut |_| {})
    }

    fn finish_exit_after_tunnel_with_state_projection<F>(
        &mut self,
        project: &mut F,
    ) -> Result<(), OrchestratorError>
    where
        F: FnMut(&RuntimeState),
    {
        let mut cleanup_fault = None;
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
            Err(self.fail(fault, None, project))
        } else {
            self.transition(RuntimeState::Stopped, project);
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

    pub(crate) fn mark_detected_coding_runtime_recovery(&mut self) {
        self.state = RuntimeState::Recovering {
            component: RuntimeComponent::CodingRuntime,
            attempt: 0,
        };
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

    pub fn recover_minimal_cancellable(
        &mut self,
        scope: RecoveryScope,
        attempt: u32,
        permit: &RecoveryPermit,
    ) -> Result<(), OrchestratorError> {
        Self::check_recovery_permit(permit)?;
        let component = match scope {
            RecoveryScope::Tunnel => RuntimeComponent::Tunnel,
            RecoveryScope::PolicyAndTunnel => RuntimeComponent::PolicyEnforcement,
            RecoveryScope::FullRuntime => RuntimeComponent::CodingRuntime,
        };
        self.state = RuntimeState::Recovering { component, attempt };
        let result = match scope {
            RecoveryScope::Tunnel => self.recover_tunnel_only_cancellable(permit),
            RecoveryScope::PolicyAndTunnel => self.recover_policy_and_tunnel_cancellable(permit),
            RecoveryScope::FullRuntime => self.recover_full_runtime_cancellable(permit),
        };
        match result {
            Ok(()) => {
                self.state = RuntimeState::Ready;
                Ok(())
            }
            Err(error)
                if permit.is_cancelled()
                    && error.fault == RuntimeFault::UserStopped
                    && error.cleanup_fault.is_none() =>
            {
                self.state = RuntimeState::Recovering { component, attempt };
                Err(error)
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
                candidate_cleanup_fault: None,
                rollback_fault: None,
                rollback_cleanup_fault: None,
            });
        }
        let previous = rollback_workspace.map(Path::to_path_buf);
        if let Err(error) = self.stop() {
            return Err(WorkspaceSwitchError {
                candidate_fault: error.fault,
                candidate_cleanup_fault: error.cleanup_fault,
                rollback_fault: None,
                rollback_cleanup_fault: None,
            });
        }
        if let Err(fault) = self.driver.configure_workspace(candidate.to_path_buf()) {
            let (rollback_fault, rollback_cleanup_fault) = self.rollback_workspace(previous);
            return Err(WorkspaceSwitchError {
                candidate_fault: fault,
                candidate_cleanup_fault: None,
                rollback_fault,
                rollback_cleanup_fault,
            });
        }
        if let Err(error) = self.start() {
            let (rollback_fault, rollback_cleanup_fault) = self.rollback_workspace(previous);
            return Err(WorkspaceSwitchError {
                candidate_fault: error.fault,
                candidate_cleanup_fault: error.cleanup_fault,
                rollback_fault,
                rollback_cleanup_fault,
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

    fn recover_tunnel_only_cancellable(
        &mut self,
        permit: &RecoveryPermit,
    ) -> Result<(), OrchestratorError> {
        Self::check_recovery_permit(permit)?;
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
        Self::check_recovery_permit(permit)?;
        if self.recovering_pep.is_none() {
            return if self.recovering_mcp.is_some() {
                self.recover_policy_and_tunnel_cancellable(permit)
            } else {
                self.recover_full_runtime_cancellable(permit)
            };
        }
        let pep = self.recovering_pep.as_ref().expect("checked retained PEP");
        if let Err(fault) = self.driver.confirm_pep_ready_for_recovery(pep, permit) {
            if fault == RuntimeFault::UserStopped {
                return Err(OrchestratorError {
                    fault,
                    cleanup_fault: None,
                });
            }
            return self.recover_policy_and_tunnel_cancellable(permit);
        }
        Self::check_recovery_permit(permit)?;
        self.start_tunnel_from_recovering_pep_cancellable(permit)
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

    fn start_tunnel_from_recovering_pep_cancellable(
        &mut self,
        permit: &RecoveryPermit,
    ) -> Result<(), OrchestratorError> {
        Self::check_recovery_permit(permit)?;
        let pep = self
            .recovering_pep
            .as_ref()
            .expect("tunnel recovery requires retained PEP");
        let mut tunnel = self
            .driver
            .start_tunnel_for_recovery(pep, permit)
            .map_err(|fault| OrchestratorError {
                fault,
                cleanup_fault: None,
            })?;
        if permit.is_cancelled() {
            let cleanup_fault = self.driver.stop_tunnel(&mut tunnel).err();
            return Err(OrchestratorError {
                fault: RuntimeFault::UserStopped,
                cleanup_fault,
            });
        }
        if let Err(fault) = self
            .driver
            .confirm_tunnel_ready_for_recovery(&mut tunnel, permit)
        {
            let cleanup_fault = self.driver.stop_tunnel(&mut tunnel).err();
            return Err(OrchestratorError {
                fault,
                cleanup_fault,
            });
        }
        if permit.is_cancelled() {
            let cleanup_fault = self.driver.stop_tunnel(&mut tunnel).err();
            return Err(OrchestratorError {
                fault: RuntimeFault::UserStopped,
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

    fn recover_policy_and_tunnel_cancellable(
        &mut self,
        permit: &RecoveryPermit,
    ) -> Result<(), OrchestratorError> {
        Self::check_recovery_permit(permit)?;
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
        Self::check_recovery_permit(permit)?;
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
        Self::check_recovery_permit(permit)?;
        let Some(mut mcp) = self.recovering_mcp.take() else {
            return self.recover_full_runtime_cancellable(permit);
        };
        if let Err(fault) = self.driver.confirm_mcp_ready_for_recovery(&mut mcp, permit) {
            self.recovering_mcp = Some(mcp);
            if fault == RuntimeFault::UserStopped {
                return Err(OrchestratorError {
                    fault,
                    cleanup_fault: None,
                });
            }
            return self.recover_full_runtime_cancellable(permit);
        }
        if permit.is_cancelled() {
            self.recovering_mcp = Some(mcp);
            return Self::check_recovery_permit(permit);
        }
        let pep = self
            .driver
            .start_pep(mcp)
            .map_err(|fault| OrchestratorError {
                fault,
                cleanup_fault: None,
            })?;
        if permit.is_cancelled() {
            return match self.driver.stop_pep(pep) {
                Ok(mcp) => {
                    self.recovering_mcp = Some(mcp);
                    Err(OrchestratorError {
                        fault: RuntimeFault::UserStopped,
                        cleanup_fault: None,
                    })
                }
                Err(cleanup_fault) => Err(OrchestratorError {
                    fault: RuntimeFault::UserStopped,
                    cleanup_fault: Some(cleanup_fault),
                }),
            };
        }
        if let Err(fault) = self.driver.confirm_pep_ready_for_recovery(&pep, permit) {
            return match self.driver.stop_pep(pep) {
                Ok(mcp) => {
                    self.recovering_mcp = Some(mcp);
                    Err(OrchestratorError {
                        fault,
                        cleanup_fault: None,
                    })
                }
                Err(cleanup_fault) => Err(OrchestratorError {
                    fault,
                    cleanup_fault: Some(cleanup_fault),
                }),
            };
        }
        self.recovering_pep = Some(pep);
        Self::check_recovery_permit(permit)?;
        self.start_tunnel_from_recovering_pep_cancellable(permit)
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

    fn recover_full_runtime_cancellable(
        &mut self,
        permit: &RecoveryPermit,
    ) -> Result<(), OrchestratorError> {
        let cleanup_fault = self.stop().err().map(|error| error.fault);
        if let Some(fault) = cleanup_fault {
            return Err(OrchestratorError {
                fault,
                cleanup_fault: None,
            });
        }
        Self::check_recovery_permit(permit)?;
        self.state = RuntimeState::Stopped;
        self.start_for_recovery(permit)
    }

    fn start_for_recovery(&mut self, permit: &RecoveryPermit) -> Result<(), OrchestratorError> {
        Self::check_recovery_permit(permit)?;
        let mut mcp = self
            .driver
            .start_mcp_for_recovery(permit)
            .map_err(|fault| OrchestratorError {
                fault,
                cleanup_fault: None,
            })?;
        if permit.is_cancelled() {
            let cleanup_fault = self.driver.stop_mcp(&mut mcp).err();
            return Err(OrchestratorError {
                fault: RuntimeFault::UserStopped,
                cleanup_fault,
            });
        }
        if let Err(fault) = self.driver.confirm_mcp_ready_for_recovery(&mut mcp, permit) {
            let cleanup_fault = self.driver.stop_mcp(&mut mcp).err();
            return Err(OrchestratorError {
                fault,
                cleanup_fault,
            });
        }
        if permit.is_cancelled() {
            let cleanup_fault = self.driver.stop_mcp(&mut mcp).err();
            return Err(OrchestratorError {
                fault: RuntimeFault::UserStopped,
                cleanup_fault,
            });
        }

        let pep = self
            .driver
            .start_pep(mcp)
            .map_err(|fault| OrchestratorError {
                fault,
                cleanup_fault: None,
            })?;
        if permit.is_cancelled() {
            let cleanup_fault = self.cleanup_pep(pep);
            return Err(OrchestratorError {
                fault: RuntimeFault::UserStopped,
                cleanup_fault,
            });
        }
        if let Err(fault) = self.driver.confirm_pep_ready_for_recovery(&pep, permit) {
            let cleanup_fault = self.cleanup_pep(pep);
            return Err(OrchestratorError {
                fault,
                cleanup_fault,
            });
        }
        if permit.is_cancelled() {
            let cleanup_fault = self.cleanup_pep(pep);
            return Err(OrchestratorError {
                fault: RuntimeFault::UserStopped,
                cleanup_fault,
            });
        }

        let mut tunnel = match self.driver.start_tunnel_for_recovery(&pep, permit) {
            Ok(tunnel) => tunnel,
            Err(fault) => {
                let cleanup_fault = self.cleanup_pep(pep);
                return Err(OrchestratorError {
                    fault,
                    cleanup_fault,
                });
            }
        };
        if permit.is_cancelled() {
            let mut cleanup_fault = self.driver.stop_tunnel(&mut tunnel).err();
            merge_cleanup_fault(&mut cleanup_fault, self.cleanup_pep(pep));
            return Err(OrchestratorError {
                fault: RuntimeFault::UserStopped,
                cleanup_fault,
            });
        }
        if let Err(fault) = self
            .driver
            .confirm_tunnel_ready_for_recovery(&mut tunnel, permit)
        {
            let mut cleanup_fault = self.driver.stop_tunnel(&mut tunnel).err();
            merge_cleanup_fault(&mut cleanup_fault, self.cleanup_pep(pep));
            return Err(OrchestratorError {
                fault,
                cleanup_fault,
            });
        }
        if permit.is_cancelled() {
            let mut cleanup_fault = self.driver.stop_tunnel(&mut tunnel).err();
            merge_cleanup_fault(&mut cleanup_fault, self.cleanup_pep(pep));
            return Err(OrchestratorError {
                fault: RuntimeFault::UserStopped,
                cleanup_fault,
            });
        }
        self.ready = Some(ReadyHandles { pep, tunnel });
        Ok(())
    }

    fn check_recovery_permit(permit: &RecoveryPermit) -> Result<(), OrchestratorError> {
        if permit.is_cancelled() {
            Err(OrchestratorError {
                fault: RuntimeFault::UserStopped,
                cleanup_fault: None,
            })
        } else {
            Ok(())
        }
    }

    fn rollback_workspace(
        &mut self,
        previous: Option<PathBuf>,
    ) -> (Option<RuntimeFault>, Option<RuntimeFault>) {
        if let Err(error) = self.stop() {
            return (Some(error.fault), error.cleanup_fault);
        }
        let Some(previous) = previous else {
            return (None, None);
        };
        if let Err(fault) = self.driver.configure_workspace(previous) {
            self.state = RuntimeState::Faulted(fault.clone());
            return (Some(fault), None);
        }
        match self.start() {
            Ok(()) => (None, None),
            Err(error) => (Some(error.fault), error.cleanup_fault),
        }
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
        OrchestratorError {
            fault,
            cleanup_fault,
        }
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
    pub request_id: String,
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
    pub fn begin(
        &mut self,
        component: RuntimeComponent,
        fault: RuntimeFault,
    ) -> OutageGenerationId {
        self.next_generation = self.next_generation.saturating_add(1).max(1);
        let id = OutageGenerationId(self.next_generation);
        static REQUEST_GENERATION: AtomicU64 = AtomicU64::new(1);
        let request_generation = REQUEST_GENERATION.fetch_add(1, Ordering::Relaxed);
        self.active = Some(OutageGeneration {
            id,
            request_id: format!("req-recovery-{}-{request_generation}", std::process::id()),
            component,
            fault,
            user_attention_emitted: false,
        });
        id
    }

    pub fn mark_user_attention_required(&mut self, generation: OutageGenerationId) -> bool {
        let Some(active) = self
            .active
            .as_mut()
            .filter(|active| active.id == generation)
        else {
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
        let Some(active) = self
            .active
            .as_mut()
            .filter(|active| active.id == generation)
        else {
            return false;
        };
        active.component = component;
        active.fault = fault;
        true
    }

    pub fn clear(&mut self, generation: OutageGenerationId) -> bool {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.id == generation)
        {
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
    workspace_identity: Option<String>,
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
            permission_mode,
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
        match (
            self.privileged_execution.as_ref(),
            self.task_projection_wake.as_ref(),
        ) {
            (Some(privileged_execution), Some(wake)) => {
                PolicyEnforcementRuntime::start_with_privilege_and_wake(
                    mcp,
                    policy,
                    self.config.permission_mode,
                    Arc::clone(privileged_execution),
                    Arc::clone(wake),
                )
            }
            (Some(privileged_execution), None) => PolicyEnforcementRuntime::start_with_privilege(
                mcp,
                policy,
                self.config.permission_mode,
                Arc::clone(privileged_execution),
            ),
            (None, Some(wake)) => PolicyEnforcementRuntime::start_with_wake(
                mcp,
                policy,
                self.config.permission_mode,
                Arc::clone(wake),
            ),
            (None, None) => {
                PolicyEnforcementRuntime::start(mcp, policy, self.config.permission_mode)
            }
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

    fn task_aggregate(&self, pep: &Self::Pep) -> Value {
        pep.task_aggregate_snapshot()
    }

    fn connector_endpoint(&self, tunnel: &Self::Tunnel) -> Option<ConnectorEndpoint> {
        tunnel.connector_endpoint()
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

    fn configure_permission_mode(&mut self, mode: PermissionMode) -> Result<(), RuntimeFault> {
        self.config.permission_mode = mode;
        Ok(())
    }

    fn set_permission_mode(
        &mut self,
        pep: &Self::Pep,
        mode: PermissionMode,
    ) -> Result<(), RuntimeFault> {
        pep.set_permission_mode(mode);
        self.config.permission_mode = mode;
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
        PermissionMode::Edit,
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

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/integration/orchestrator/orchestrator.rs"
    ));
}
