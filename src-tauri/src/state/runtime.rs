use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeComponent {
    CodingRuntime,
    PolicyEnforcement,
    Tunnel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFault {
    WorkspaceMissing,
    WorkspaceInvalid,
    RuntimeMissing,
    RuntimeChecksumMismatch,
    ProcessOwnershipFailed,
    McpSpawnFailed,
    McpHealthTimeout,
    McpExited,
    PolicyBindFailed,
    PolicyInvalid,
    PolicyCapabilityUnknown,
    TunnelIdMissing,
    RuntimeKeyMissing,
    SecretStoreFailed,
    SecretInjectionUnsupported,
    TunnelAuthFailed,
    TunnelSpawnFailed,
    TunnelHealthTimeout,
    TunnelExited,
    PortUnavailable,
    ConfigurationInvalid,
    UserStopped,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum RuntimeState {
    Stopped,
    StartingMcp,
    WaitingMcpReady,
    StartingPolicyEnforcement,
    WaitingPolicyReady,
    StartingTunnel,
    WaitingTunnelReady,
    Ready,
    Recovering {
        component: RuntimeComponent,
        attempt: u32,
    },
    SwitchingWorkspace {
        from: PathBuf,
        candidate: PathBuf,
    },
    Faulted(RuntimeFault),
}

impl RuntimeState {
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    pub const fn is_faulted(&self) -> bool {
        matches!(self, Self::Faulted(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentLifecycle {
    Stopped,
    Starting,
    WaitingReady,
    Ready,
    Stopping,
    Faulted(RuntimeFault),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatus {
    pub component: RuntimeComponent,
    pub lifecycle: ComponentLifecycle,
}

impl ComponentStatus {
    pub const fn new(component: RuntimeComponent, lifecycle: ComponentLifecycle) -> Self {
        Self {
            component,
            lifecycle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_fault_is_typed_and_not_ready() {
        let state = RuntimeState::Faulted(RuntimeFault::WorkspaceMissing);
        assert!(state.is_faulted());
        assert!(!state.is_ready());
    }

    #[test]
    fn component_status_keeps_component_identity_separate_from_state() {
        let status = ComponentStatus::new(RuntimeComponent::Tunnel, ComponentLifecycle::Starting);
        assert_eq!(status.component, RuntimeComponent::Tunnel);
        assert_eq!(status.lifecycle, ComponentLifecycle::Starting);
    }

    #[test]
    fn aggregate_runtime_state_represents_staged_start_and_workspace_switch() {
        for state in [
            RuntimeState::StartingMcp,
            RuntimeState::WaitingMcpReady,
            RuntimeState::StartingPolicyEnforcement,
            RuntimeState::WaitingPolicyReady,
            RuntimeState::StartingTunnel,
            RuntimeState::WaitingTunnelReady,
        ] {
            assert!(!state.is_ready());
            assert!(!state.is_faulted());
        }
        let switching = RuntimeState::SwitchingWorkspace {
            from: PathBuf::from(r"D:\old"),
            candidate: PathBuf::from(r"D:\new"),
        };
        assert!(matches!(switching, RuntimeState::SwitchingWorkspace { .. }));
    }
}
