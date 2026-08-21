use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GenerationId(u64);

impl GenerationId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegeFault {
    UacDenied,
    BrokerLaunchFailed,
    HandshakeFailed,
    BrokerExited,
    IpcUnavailable,
    ProtocolMismatch,
    UnauthorizedPeer,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum PrivilegeState {
    Disabled,
    Requested,
    AwaitingUac,
    Active { broker_generation: GenerationId },
    Faulted(PrivilegeFault),
}

impl PrivilegeState {
    pub const fn accepts_privileged_calls(&self) -> bool {
        matches!(self, Self::Active { .. })
    }

    pub const fn needs_explicit_user_authorization(&self) -> bool {
        matches!(self, Self::Requested | Self::AwaitingUac)
    }

    pub const fn is_faulted(&self) -> bool {
        matches!(self, Self::Faulted(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_active_privilege_accepts_privileged_calls() {
        let states = [
            PrivilegeState::Disabled,
            PrivilegeState::Requested,
            PrivilegeState::AwaitingUac,
            PrivilegeState::Faulted(PrivilegeFault::BrokerExited),
        ];
        assert!(states.iter().all(|state| !state.accepts_privileged_calls()));
        assert!(
            PrivilegeState::Active {
                broker_generation: GenerationId::new(1),
            }
            .accepts_privileged_calls()
        );
    }

    #[test]
    fn requested_and_awaiting_uac_are_not_active() {
        assert!(PrivilegeState::Requested.needs_explicit_user_authorization());
        assert!(PrivilegeState::AwaitingUac.needs_explicit_user_authorization());
        assert!(
            !PrivilegeState::Active {
                broker_generation: GenerationId::new(1),
            }
            .needs_explicit_user_authorization()
        );
    }

    #[test]
    fn broker_failure_is_typed_without_exposing_session_internals() {
        let state = PrivilegeState::Faulted(PrivilegeFault::HandshakeFailed);
        assert!(state.is_faulted());
        assert!(!state.accepts_privileged_calls());
    }

    #[test]
    fn active_state_carries_typed_broker_generation_without_time_semantics() {
        let generation = GenerationId::new(7);
        let state = PrivilegeState::Active {
            broker_generation: generation,
        };
        assert_eq!(generation.get(), 7);
        assert!(state.accepts_privileged_calls());
    }
}
