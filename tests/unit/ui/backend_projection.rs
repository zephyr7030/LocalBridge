use super::*;
use crate::state::{
    CurrentTaskStatus, RuntimeFault, SafeTaskSummary, TaskExecutionState, TaskKind,
};

#[test]
fn production_update_projection_exposes_the_official_release_source() {
    let owner = crate::control_plane::update::UpdateStateOwner::default();
    let lifecycle = owner.snapshot();
    let projection = update_projection(Some(&lifecycle));

    assert_eq!(projection.state, "idle");
    assert_eq!(projection.current_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(
        projection.release_url.as_deref(),
        Some("https://github.com/zephyr7030/LocalBridge/releases")
    );
    assert!(projection.retryable);
    let serialized = serde_json::to_value(&projection).expect("update command result serializes");
    assert_eq!(serialized["state"], "idle");
    assert_eq!(serialized["currentVersion"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        serialized["releaseUrl"],
        "https://github.com/zephyr7030/LocalBridge/releases"
    );

    let release = release_projection(&crate::domain::GitHubRepository::official(), &lifecycle)
        .expect("the fixed official release URL must be allowed");
    assert_eq!(
        release.release_url,
        "https://github.com/zephyr7030/LocalBridge/releases"
    );
    assert_eq!(
        serde_json::to_value(&release).expect("open-release command result serializes")["releaseUrl"],
        "https://github.com/zephyr7030/LocalBridge/releases"
    );
}

#[test]
fn main_projection_json_contract_matches_the_frontend_fixture() {
    let projection = MainProjection {
        authority_status: "ready",
        runtime_status: "ready",
        settings_status: "ready",
        workspace_status: "ready",
        connection_status: "ready",
        activity_status: "ready",
        update_status: "ready",
        permission: Some("admin"),
        effective_permission: Some("full"),
        permission_reconciliation: Some("awaiting_authorization"),
        path_authority: Some("workspace"),
        privilege: Some("requested"),
        local_environment_service: Some("online"),
        tunnel_service: Some("recovering"),
        coding_service: Some("online"),
        onboarding_ready: Some(false),
        workspace: Some(UiWorkspaceProjection {
            desired_path: Some("D:/project/LocalBridge".into()),
            observed_path: Some("D:/project/LocalBridge".into()),
            effective: "available",
        }),
        projects: Some(vec![ProjectProjection {
            id: "workspace-1".into(),
            path: "D:/project/LocalBridge".into(),
            active: true,
        }]),
        current_task: Some(TaskProjection {
            kind: "command",
            summary: Some("running a local command".into()),
            state: "running",
            elapsed_ms: Some(1200),
        }),
        current_activity: Some(CurrentActivityProjection {
            kind: "command",
            state: "waiting_input",
            summary: Some("waiting for input".into()),
            elapsed_ms: Some(1200),
            step: None,
            progress_current: None,
            progress_total: None,
        }),
        last_activity: Some(LastActivityProjection {
            kind: "modify",
            summary: Some("policy rejected edit".into()),
            outcome: "blocked",
            completed_at_ms: 1000,
        }),
        projection_revision: 49,
        connection: Some(UiConnectionProjection {
            desired_tunnel_id: Some("tunnel_01401401401401401401401401401401".into()),
            observed_tunnel_id: Some("tunnel_01401401401401401401401401401401".into()),
            effective: "available",
        }),
        runtime_key_saved: Some(true),
        auto_start: Some(true),
        close_window_continue_running: Some(true),
        reconnect: Some(ReconnectProjection { generation: 3 }),
        update: Some(UpdateProjection {
            state: "current",
            current_version: "0.1.4".into(),
            latest_version: None,
            release_url: Some("https://github.com/zephyr7030/LocalBridge/releases".into()),
            operation_id: Some("update-1".into()),
            attempt: None,
            retryable: true,
        }),
        active_faults: vec![UiFaultProjection {
            code: "Authority.BrokerUnavailable".into(),
            category: "authorization",
            message: "Privilege broker is unavailable".into(),
            retryable: true,
        }],
    };
    let backend = serde_json::to_value(projection).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/fixtures/ui/main_projection.json"
    )))
    .unwrap();
    assert_eq!(backend, fixture);
}

#[test]
fn unavailable_and_stale_sections_never_become_live_ui_business_state() {
    let unavailable = ProjectionSection::<u8>::unavailable();
    assert_eq!(projection_section_code(&unavailable), "unavailable");
    assert_eq!(ready_section_value(&unavailable), None);

    let stale = ProjectionSection::stale(Some(PermissionMode::Elevated));
    assert_eq!(projection_section_code(&stale), "stale");
    assert_eq!(ready_section_value(&stale), None);

    let faulted = ProjectionSection::faulted(Some(RuntimeState::Ready));
    assert_eq!(projection_section_code(&faulted), "fault");
    assert_eq!(ready_section_value(&faulted), None);
}

#[test]
fn presentation_codes_are_stable_and_never_direct_internal_enum_names() {
    assert_eq!(permission_code(PermissionMode::Edit), "edit");
    assert_eq!(permission_code(PermissionMode::Full), "full");
    assert_eq!(permission_code(PermissionMode::Elevated), "admin");
    assert_eq!(
        authority_reconciliation_code(AuthorityReconciliation::Converged),
        "converged"
    );
    assert_eq!(
        authority_reconciliation_code(AuthorityReconciliation::AwaitingAuthorization),
        "awaiting_authorization"
    );
    assert_eq!(
        authority_reconciliation_code(AuthorityReconciliation::BrokerUnavailable),
        "broker_unavailable"
    );
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
    assert_eq!(terminal_outcome_code(TerminalOutcome::Blocked), "blocked");
    let rendered = serde_json::to_string(&MainProjection {
        authority_status: "ready",
        runtime_status: "ready",
        settings_status: "ready",
        workspace_status: "ready",
        connection_status: "ready",
        activity_status: "ready",
        update_status: "ready",
        permission: Some("admin"),
        effective_permission: Some("admin"),
        permission_reconciliation: Some("converged"),
        path_authority: Some("administrator"),
        privilege: Some("active"),
        local_environment_service: Some("online"),
        tunnel_service: Some("online"),
        coding_service: Some("online"),
        onboarding_ready: Some(true),
        workspace: Some(UiWorkspaceProjection {
            desired_path: Some("D:/project/LocalBridge".into()),
            observed_path: Some("D:/project/LocalBridge".into()),
            effective: "available",
        }),
        projects: Some(vec![]),
        current_task: None,
        current_activity: None,
        last_activity: None,
        projection_revision: 7,
        connection: Some(UiConnectionProjection {
            desired_tunnel_id: Some("tunnel_01401401401401401401401401401401".to_owned()),
            observed_tunnel_id: Some("tunnel_01401401401401401401401401401401".to_owned()),
            effective: "available",
        }),
        runtime_key_saved: Some(true),
        auto_start: Some(true),
        close_window_continue_running: Some(true),
        reconnect: None,
        update: Some(UpdateProjection {
            state: "current",
            current_version: "0.1.1".into(),
            latest_version: None,
            release_url: Some("https://github.com/owner/repo/releases".into()),
            operation_id: Some("update-1".into()),
            attempt: None,
            retryable: true,
        }),
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
        "currentWorkflow",
        "currentCommand",
        "lastCommand",
        "lastTool",
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
            adoption_token_hash: None,
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
            last_observed_at_ms: 7,
            orphaned_at_ms: None,
        }),
        scheduler: SchedulerSnapshot::idle(),
    };
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
    assert!(current_activity_projection(&idle).is_none());
    assert!(last_activity_projection(&idle).is_none());
    let running = TaskAggregate {
        foreground_task: None,
        detached_execution: Some(ExecutionRecord {
            id: ExecutionId::new("execution-running"),
            task_id,
            public_session_id: PublicSessionId::new("public-running"),
            owner_session: Some(McpSessionId::new("session-a")),
            adoption_token_hash: None,
            runtime_handle: None,
            state: ExecutionState::Running,
            started_at_ms: 1,
            last_observed_at_ms: 1,
            orphaned_at_ms: None,
        }),
        last_task: None,
        last_execution: None,
        scheduler: SchedulerSnapshot::idle(),
    };
    let running_activity = current_activity_projection(&running).unwrap();
    assert_eq!(running_activity.kind, "command");
    assert_eq!(running_activity.state, "running");
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
