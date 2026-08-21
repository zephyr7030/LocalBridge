use crate::diagnostics::{MAX_ACTIVE_DIAGNOSTIC_REQUESTS, REQUEST_DIAGNOSTIC_LIMIT};
use crate::execution::output_handles::{
    MAX_LOCAL_RETAINED_OUTPUT_HANDLES, MAX_PRIVATE_RETAINED_OUTPUT_HANDLES,
};
use crate::privilege::MAX_ACTIVE_BROKER_REQUESTS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    McpSession,
    Request,
    Task,
    Execution,
    PublicCommandSession,
    OutputHandle,
    WorkflowCheckpoint,
    DiagnosticsEntry,
    BrokerRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceOwner {
    SessionRegistry,
    RequestRegistry,
    TaskRegistry,
    ExecutionRegistry,
    OutputHandleRegistry,
    WorkflowCheckpointStore,
    DiagnosticsRegistry,
    PrivilegeBroker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveRetention {
    UntilTerminal,
    IdleTtl {
        ttl_ms: u64,
    },
    BoundedConcurrent {
        maximum: usize,
    },
    BoundedHandles {
        local_maximum: usize,
        private_maximum: usize,
    },
    DurableSingle {
        maximum_bytes: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalRetention {
    None,
    BoundedHistory { maximum: usize },
    OwnedBy(ResourceKind),
    DurableUntilExplicitClear { maximum_bytes: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalCondition {
    Closed,
    ResponseDeliveredOrCancelled,
    TaskOutcome,
    ExecutionOutcome,
    OwnedExecutionOutcome,
    EvictedOrSourceExpired,
    CompletedOrCleared,
    Recorded,
    BrokerResponseOrChannelLoss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReapingCondition {
    IdleTtl,
    ImmediatelyAfterTerminal,
    OldestTerminalAboveLimit,
    WithOwnedResource,
    OldestHandleAboveLimit,
    ExplicitClearOrReplacement,
    OldestEntryAboveLimit,
    PollCompletionOrBrokerDisconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartBehavior {
    DiscardTransient,
    PreserveTerminalAndMarkUnfinishedLost,
    RebuildFromExecutionOwner,
    Expire,
    PreserveDurable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectBehavior {
    CloseAndSettleTransient,
    CancelOwned,
    SettleTransientRetainDetached,
    RetainDetached,
    Expire,
    RetainDurable,
    RetainBounded,
    CancelBrokerOwned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLifecyclePolicy {
    pub kind: ResourceKind,
    pub owner: ResourceOwner,
    pub active_retention: ActiveRetention,
    pub terminal_retention: TerminalRetention,
    pub terminal_condition: TerminalCondition,
    pub reaping_condition: ReapingCondition,
    pub restart_behavior: RestartBehavior,
    pub disconnect_behavior: DisconnectBehavior,
}

pub(crate) const MCP_SESSION_TTL_MS: u64 = 5 * 60 * 1_000;
pub(crate) const MAX_RETAINED_REQUEST_ERRORS: usize = 256;
pub(crate) const MAX_RETAINED_TASKS: usize = 256;
pub(crate) const MAX_ACTIVE_EXECUTIONS: usize = 64;
pub(crate) const MAX_TERMINAL_EXECUTIONS: usize = 64;
pub(crate) const MAX_CHECKPOINT_PLAINTEXT_BYTES: usize = 262_144;

pub const RESOURCE_LIFECYCLE_POLICIES: [ResourceLifecyclePolicy; 9] = [
    ResourceLifecyclePolicy {
        kind: ResourceKind::McpSession,
        owner: ResourceOwner::SessionRegistry,
        active_retention: ActiveRetention::IdleTtl {
            ttl_ms: MCP_SESSION_TTL_MS,
        },
        terminal_retention: TerminalRetention::None,
        terminal_condition: TerminalCondition::Closed,
        reaping_condition: ReapingCondition::IdleTtl,
        restart_behavior: RestartBehavior::DiscardTransient,
        disconnect_behavior: DisconnectBehavior::CloseAndSettleTransient,
    },
    ResourceLifecyclePolicy {
        kind: ResourceKind::Request,
        owner: ResourceOwner::RequestRegistry,
        active_retention: ActiveRetention::UntilTerminal,
        terminal_retention: TerminalRetention::BoundedHistory {
            maximum: MAX_RETAINED_REQUEST_ERRORS,
        },
        terminal_condition: TerminalCondition::ResponseDeliveredOrCancelled,
        reaping_condition: ReapingCondition::ImmediatelyAfterTerminal,
        restart_behavior: RestartBehavior::DiscardTransient,
        disconnect_behavior: DisconnectBehavior::CancelOwned,
    },
    ResourceLifecyclePolicy {
        kind: ResourceKind::Task,
        owner: ResourceOwner::TaskRegistry,
        active_retention: ActiveRetention::UntilTerminal,
        terminal_retention: TerminalRetention::BoundedHistory {
            maximum: MAX_RETAINED_TASKS,
        },
        terminal_condition: TerminalCondition::TaskOutcome,
        reaping_condition: ReapingCondition::OldestTerminalAboveLimit,
        restart_behavior: RestartBehavior::DiscardTransient,
        disconnect_behavior: DisconnectBehavior::SettleTransientRetainDetached,
    },
    ResourceLifecyclePolicy {
        kind: ResourceKind::Execution,
        owner: ResourceOwner::ExecutionRegistry,
        active_retention: ActiveRetention::BoundedConcurrent {
            maximum: MAX_ACTIVE_EXECUTIONS,
        },
        terminal_retention: TerminalRetention::BoundedHistory {
            maximum: MAX_TERMINAL_EXECUTIONS,
        },
        terminal_condition: TerminalCondition::ExecutionOutcome,
        reaping_condition: ReapingCondition::OldestTerminalAboveLimit,
        restart_behavior: RestartBehavior::PreserveTerminalAndMarkUnfinishedLost,
        disconnect_behavior: DisconnectBehavior::RetainDetached,
    },
    ResourceLifecyclePolicy {
        kind: ResourceKind::PublicCommandSession,
        owner: ResourceOwner::ExecutionRegistry,
        active_retention: ActiveRetention::BoundedConcurrent {
            maximum: MAX_ACTIVE_EXECUTIONS,
        },
        terminal_retention: TerminalRetention::OwnedBy(ResourceKind::Execution),
        terminal_condition: TerminalCondition::OwnedExecutionOutcome,
        reaping_condition: ReapingCondition::WithOwnedResource,
        restart_behavior: RestartBehavior::RebuildFromExecutionOwner,
        disconnect_behavior: DisconnectBehavior::RetainDetached,
    },
    ResourceLifecyclePolicy {
        kind: ResourceKind::OutputHandle,
        owner: ResourceOwner::OutputHandleRegistry,
        active_retention: ActiveRetention::BoundedHandles {
            local_maximum: MAX_LOCAL_RETAINED_OUTPUT_HANDLES,
            private_maximum: MAX_PRIVATE_RETAINED_OUTPUT_HANDLES,
        },
        terminal_retention: TerminalRetention::OwnedBy(ResourceKind::PublicCommandSession),
        terminal_condition: TerminalCondition::EvictedOrSourceExpired,
        reaping_condition: ReapingCondition::OldestHandleAboveLimit,
        restart_behavior: RestartBehavior::Expire,
        disconnect_behavior: DisconnectBehavior::RetainBounded,
    },
    ResourceLifecyclePolicy {
        kind: ResourceKind::WorkflowCheckpoint,
        owner: ResourceOwner::WorkflowCheckpointStore,
        active_retention: ActiveRetention::DurableSingle {
            maximum_bytes: MAX_CHECKPOINT_PLAINTEXT_BYTES,
        },
        terminal_retention: TerminalRetention::DurableUntilExplicitClear {
            maximum_bytes: MAX_CHECKPOINT_PLAINTEXT_BYTES,
        },
        terminal_condition: TerminalCondition::CompletedOrCleared,
        reaping_condition: ReapingCondition::ExplicitClearOrReplacement,
        restart_behavior: RestartBehavior::PreserveDurable,
        disconnect_behavior: DisconnectBehavior::RetainDurable,
    },
    ResourceLifecyclePolicy {
        kind: ResourceKind::DiagnosticsEntry,
        owner: ResourceOwner::DiagnosticsRegistry,
        active_retention: ActiveRetention::BoundedConcurrent {
            maximum: MAX_ACTIVE_DIAGNOSTIC_REQUESTS,
        },
        terminal_retention: TerminalRetention::BoundedHistory {
            maximum: REQUEST_DIAGNOSTIC_LIMIT,
        },
        terminal_condition: TerminalCondition::Recorded,
        reaping_condition: ReapingCondition::OldestEntryAboveLimit,
        restart_behavior: RestartBehavior::DiscardTransient,
        disconnect_behavior: DisconnectBehavior::RetainBounded,
    },
    ResourceLifecyclePolicy {
        kind: ResourceKind::BrokerRequest,
        owner: ResourceOwner::PrivilegeBroker,
        active_retention: ActiveRetention::BoundedConcurrent {
            maximum: MAX_ACTIVE_BROKER_REQUESTS,
        },
        terminal_retention: TerminalRetention::None,
        terminal_condition: TerminalCondition::BrokerResponseOrChannelLoss,
        reaping_condition: ReapingCondition::PollCompletionOrBrokerDisconnect,
        restart_behavior: RestartBehavior::DiscardTransient,
        disconnect_behavior: DisconnectBehavior::CancelBrokerOwned,
    },
];

pub fn resource_lifecycle_policy(kind: ResourceKind) -> &'static ResourceLifecyclePolicy {
    RESOURCE_LIFECYCLE_POLICIES
        .iter()
        .find(|policy| policy.kind == kind)
        .expect("every ResourceKind must have exactly one lifecycle policy")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_required_resource_has_exactly_one_complete_policy() {
        let required = [
            ResourceKind::McpSession,
            ResourceKind::Request,
            ResourceKind::Task,
            ResourceKind::Execution,
            ResourceKind::PublicCommandSession,
            ResourceKind::OutputHandle,
            ResourceKind::WorkflowCheckpoint,
            ResourceKind::DiagnosticsEntry,
            ResourceKind::BrokerRequest,
        ];
        let unique = RESOURCE_LIFECYCLE_POLICIES
            .iter()
            .map(|policy| policy.kind)
            .collect::<HashSet<_>>();
        assert_eq!(unique.len(), required.len());
        for kind in required {
            assert_eq!(resource_lifecycle_policy(kind).kind, kind);
        }
    }

    #[test]
    fn every_numeric_retention_boundary_is_nonzero() {
        for policy in RESOURCE_LIFECYCLE_POLICIES {
            match policy.active_retention {
                ActiveRetention::IdleTtl { ttl_ms } => assert!(ttl_ms > 0),
                ActiveRetention::BoundedConcurrent { maximum } => assert!(maximum > 0),
                ActiveRetention::BoundedHandles {
                    local_maximum,
                    private_maximum,
                } => {
                    assert!(local_maximum > 0);
                    assert!(private_maximum > 0);
                }
                ActiveRetention::DurableSingle { maximum_bytes } => {
                    assert!(maximum_bytes > 0)
                }
                ActiveRetention::UntilTerminal => {}
            }
            match policy.terminal_retention {
                TerminalRetention::BoundedHistory { maximum } => assert!(maximum > 0),
                TerminalRetention::DurableUntilExplicitClear { maximum_bytes } => {
                    assert!(maximum_bytes > 0)
                }
                TerminalRetention::None | TerminalRetention::OwnedBy(_) => {}
            }
        }
    }
}
