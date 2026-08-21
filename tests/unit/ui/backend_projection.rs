use super::*;
use crate::state::{
    CurrentTaskStatus, RuntimeFault, SafeTaskSummary, TaskExecutionState, TaskKind,
};

#[test]
fn presentation_codes_are_stable_and_never_direct_internal_enum_names() {
    assert_eq!(permission_code(PermissionMode::Edit), "edit");
    assert_eq!(permission_code(PermissionMode::Full), "full");
    assert_eq!(permission_code(PermissionMode::Elevated), "admin");
    assert_eq!(privilege_code(&PrivilegeState::Disabled), "off");
    assert_eq!(privilege_code(&PrivilegeState::Requested), "requested");
    assert_eq!(privilege_code(&PrivilegeState::AwaitingUac), "awaiting");
    assert_eq!(
        privilege_code(&PrivilegeState::Active {
            broker_generation: crate::state::GenerationId::new(99)
        }),
        "active"
    );
    assert_eq!(
        privilege_code(&PrivilegeState::Faulted(
            crate::state::PrivilegeFault::BrokerExited
        )),
        "fault"
    );
    for (state, expected) in [
        (RuntimeState::Stopped, ("off", "off")),
        (RuntimeState::StartingMcp, ("off", "starting")),
        (RuntimeState::StartingTunnel, ("starting", "online")),
        (RuntimeState::Ready, ("online", "online")),
        (
            RuntimeState::Recovering {
                component: RuntimeComponent::Tunnel,
                attempt: 2,
            },
            ("recovering", "online"),
        ),
        (
            RuntimeState::Recovering {
                component: RuntimeComponent::PolicyEnforcement,
                attempt: 1,
            },
            ("recovering", "recovering"),
        ),
        (
            RuntimeState::Recovering {
                component: RuntimeComponent::CodingRuntime,
                attempt: 0,
            },
            ("recovering", "recovering"),
        ),
        (
            RuntimeState::Faulted(RuntimeFault::Unknown),
            ("fault", "fault"),
        ),
    ] {
        assert_eq!(service_codes(&state), expected);
    }
    for (state, expected) in [
        (RuntimeState::Stopped, "off"),
        (RuntimeState::StartingMcp, "starting"),
        (RuntimeState::WaitingMcpReady, "starting"),
        (RuntimeState::StartingPolicyEnforcement, "online"),
        (RuntimeState::StartingTunnel, "online"),
        (RuntimeState::Ready, "online"),
        (
            RuntimeState::Recovering {
                component: RuntimeComponent::CodingRuntime,
                attempt: 1,
            },
            "recovering",
        ),
        (
            RuntimeState::Recovering {
                component: RuntimeComponent::Tunnel,
                attempt: 1,
            },
            "online",
        ),
        (RuntimeState::Faulted(RuntimeFault::Unknown), "fault"),
    ] {
        assert_eq!(local_environment_service_code(&state), expected);
    }
    let rendered = serde_json::to_string(&MainProjection {
        permission: "admin",
        privilege: "active",
        local_environment_service: "online",
        tunnel_service: "online",
        coding_service: "online",
        current_project: None,
        projects: vec![],
        current_task: None,
        current_workflow: None,
        current_command: None,
        last_command: None,
        last_tool: None,
        current_activity: None,
        last_activity: None,
        projection_revision: 7,
        tunnel_id: Some("tunnel_01401401401401401401401401401401".to_owned()),
        runtime_key_saved: true,
        auto_start: true,
        close_window_continue_running: true,
        reconnect: None,
        update: UpdateProjection {
            state: "current",
            current_version: "0.1.1".into(),
            latest_version: None,
            release_url: Some("https://github.com/owner/repo/releases".into()),
            operation_id: Some("update-1".into()),
            attempt: None,
            retryable: true,
        },
        active_faults: vec![],
    })
    .unwrap();
    for forbidden in [
        "Elevated",
        "AwaitingUac",
        "BrokerExited",
        "RuntimeState",
        "PrivilegeState",
        "broker_generation",
        "nonce",
        "pid",
    ] {
        assert!(!rendered.contains(forbidden));
    }
}
#[test]
fn schema44_typed_task_aggregate_separates_current_and_history() {
    use crate::control_plane::scheduler::SchedulerSnapshot;
    use crate::domain::{
        ExecutionId, ExecutionRecord, ExecutionState, ExecutionTerminal, LifecycleState,
        McpSessionId, PublicSessionId, RequestKey, RpcRequestId, TaskId, TaskRecord,
        TerminalOutcome,
    };
    let task_id = TaskId::new("task-workflow");
    let waiting = TaskAggregate {
        foreground_task: Some(TaskRecord {
            id: task_id.clone(),
            owner_session: McpSessionId::new("session-a"),
            request: RequestKey::new(McpSessionId::new("session-a"), RpcRequestId::Number(1)),
            kind: TaskKind::Other,
            summary: SafeTaskSummary::Omitted,
            lifecycle: LifecycleState::Queued,
            created_at_ms: 1,
            updated_at_ms: 1,
            error: None,
        }),
        detached_execution: None,
        last_task: None,
        last_execution: Some(ExecutionRecord {
            id: ExecutionId::new("execution-old"),
            task_id: TaskId::new("task-old"),
            public_session_id: PublicSessionId::new("public-old"),
            owner_session: Some(McpSessionId::new("session-a")),
            runtime_handle: None,
            state: ExecutionState::Terminal(ExecutionTerminal {
                outcome: TerminalOutcome::Cancelled,
                exit_code: None,
                signal: None,
                output_refs: vec![],
                error_code: None,
                completed_at_ms: 7,
            }),
            started_at_ms: 2,
        }),
        scheduler: SchedulerSnapshot::idle(),
    };
    assert_eq!(
        current_workflow_projection(&waiting).unwrap().state,
        "waiting"
    );
    assert!(current_command_projection(&waiting).is_none());
    assert_eq!(
        last_command_projection(&waiting).unwrap().status,
        "cancelled"
    );
    let current = current_activity_projection(&waiting).unwrap();
    assert_eq!(current.kind, "other");
    assert_eq!(current.state, "waiting");
    assert_eq!(current.step, None);
    assert_eq!(current.progress_current, None);
    assert_eq!(current.progress_total, None);
    let last = last_activity_projection(&waiting).unwrap();
    assert_eq!(last.kind, "command");
    assert_eq!(last.summary, None);
    assert_eq!(last.outcome, "cancelled");
    assert_eq!(last.completed_at_ms, 7);
    let idle = TaskAggregate::idle();
    assert!(current_workflow_projection(&idle).is_none());
    assert!(current_command_projection(&idle).is_none());
    assert!(last_command_projection(&idle).is_none());
    let running = TaskAggregate {
        foreground_task: None,
        detached_execution: Some(ExecutionRecord {
            id: ExecutionId::new("execution-running"),
            task_id,
            public_session_id: PublicSessionId::new("public-running"),
            owner_session: Some(McpSessionId::new("session-a")),
            runtime_handle: None,
            state: ExecutionState::Running,
            started_at_ms: 1,
        }),
        last_task: None,
        last_execution: None,
        scheduler: SchedulerSnapshot::idle(),
    };
    assert_eq!(
        current_command_projection(&running).unwrap().state,
        "running"
    );
    assert_eq!(
        task_projection_from_aggregate(&running, Some(10))
            .unwrap()
            .kind,
        "command"
    );
}

#[test]
fn current_task_projection_uses_only_pre_redacted_summary() {
    let safe = CurrentTaskStatus::project(
        TaskKind::Test,
        SafeTaskSummary::from_untrusted("cargo test"),
        TaskExecutionState::Running,
    )
    .unwrap();
    let projected = task_projection(&safe, Some(1234)).unwrap();
    assert_eq!(projected.kind, "test");
    assert_eq!(projected.summary.as_deref(), Some("cargo test"));
    assert_eq!(projected.state, "running");
    assert_eq!(projected.elapsed_ms, Some(1234));
    let secret = CurrentTaskStatus::project(
        TaskKind::ExecuteCommand,
        SafeTaskSummary::from_untrusted("--api-key=synthetic-secret"),
        TaskExecutionState::Blocked,
    )
    .unwrap();
    let projected = task_projection(&secret, None).unwrap();
    assert_eq!(projected.summary, None);
    assert_eq!(projected.state, "blocked");
    assert_eq!(task_projection(&CurrentTaskStatus::Idle, None), None);
}
