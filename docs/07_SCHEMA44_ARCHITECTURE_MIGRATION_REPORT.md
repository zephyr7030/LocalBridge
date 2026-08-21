# LocalBridge schema44 架构迁移报告

验收日期：2026-08-21  
验收结论：通过  
合同：`CONTROL_PLANE_REFACTOR_CONTRACT.json`  
实现范围：R1 → R7，以及统一验收加固

## 1. 结果摘要

schema44 已将“多份平行状态 + 隐式串行化 + Projection 补偿”替换为以下执行模型：

```text
MCP / Tauri transport
        ↓ typed intent
ControlPlane owners + Scheduler
        ↓
Domain / Runtime / Privilege / Workspace
        ↓ typed observation
one revisioned ControlPlaneSnapshot
```

权威事实不再由 UI、transport 或兼容投影写入。Task、Execution、MCP Session、Request、Desired State、Snapshot、Fault 和 Update lifecycle 均有一个明确 mutable owner。`current_command` 仅保留为从 `ExecutionRegistry` 计算的兼容展示字段和旧持久化格式迁移 reader，不是可写 truth，也不存在单 current-command slot。

## 2. Phase 验收记录

### R1 — Identity / Isolation

Phase: R1  
Changes: 建立 `McpSessionId`、`RpcRequestId`、`RequestKey`；活动请求和取消目标按 MCP Session 隔离。  
Removed old truth: 裸 request-id 活动表、平行 cancellation vectors、跨 Session workflow cancel fallback。  
Invariant verification: INV-02、INV-04。  
Tests: 同 JSON-RPC id 双 Session 碰撞；跨 Session cancellation 隔离；Public Session owner 校验。  
Remaining risks: 无。  
Next phase: R2。

### R2 — Task / Execution

Phase: R2  
Changes: 建立稳定 `TaskId`、`ExecutionId`、`PublicSessionId`；Task 与 Execution 生命周期分离；统一 terminal vocabulary。  
Removed old truth: 单 current-command slot、detached Execution 对前台 Task 的 Running 补偿、Running → Idle 后由 UI 猜 Completed。  
Invariant verification: INV-03、INV-05、INV-07。  
Tests: Exactly-once terminal；多 Execution；Task Completed / Execution Running；重启 Running → Lost。  
Remaining risks: 无。  
Next phase: R3。

### R3 — Scheduler / Session

Phase: R3  
Changes: 建立 Observation、Control、Work 三条 lane；Work 使用 bounded FIFO；建立 MCP Session 生命周期、TTL 和 Reaper。  
Removed old truth: Facade mutex 充当调度器、worker 饱和映射为 502/503、无生命周期 Session 删除。  
Invariant verification: INV-04、INV-05、INV-09。  
Tests: 多窗口 FIFO；QueueCapacityExceeded；Control 不受 Work 阻塞；Session scoped queue cancellation；TTL reaping。  
Remaining risks: 无。  
Next phase: R4。

### R4 — Desired / Observed / Effective

Phase: R4  
Changes: Permission、Workspace、Runtime、Tunnel、Connection 采用 Desired → Reconciler → Observed → Effective；Effective fail closed。  
Removed old truth: MCP permission lock、runtime permission copy、workspace rollback state、AppState mirrors。  
Invariant verification: INV-01、INV-08、INV-09。  
Tests: Broker 不可用时 Elevated 不生效；Workspace divergence 拒绝 Work；connection revision mismatch；切换失败不 rollback Desired。  
Remaining risks: 无。  
Next phase: R5。

### R5 — Snapshot / UI

Phase: R5  
Changes: 建立单 revision `ControlPlaneSnapshot`、typed `UiError`、`OperationError`、`PersistentFault`；UI 只读一次发布快照。  
Removed old truth: live composite MainProjection、单独 wake revision、TaskAggregate opaque Value、UI `Result<T, String>`、锁竞争 fabricated Running。  
Invariant verification: INV-01、INV-06、INV-07。  
Tests: 单 revision 原子发布；读快照无副作用；fault section 隔离；lock contention → stale。  
Remaining risks: 无。  
Next phase: R6。

### R6 — Boundary Cleanup

Phase: R6  
Changes: Domain/Execution/Filesystem/Workspace 实现迁出 MCP；统一 `WorkspaceResolver`；Structured Filesystem policy 与 Shell policy 分离；建立依赖方向静态扫描。  
Removed old truth: MCP 下的 filesystem/path/task/workflow 实现文件、runtime → MCP 依赖、PathAuthority 兼容 owner、workflow checkpoint `serde_json::Value`。  
Invariant verification: INV-01、INV-02、INV-04、INV-09。  
Tests: absolute/relative/default-cwd 一致；path authorization；shell policy separation；core no-Value；dependency scan。  
Remaining risks: `mcp/facade.rs` 和 `mcp/server.rs` 仍是较大的 transport/mapping 文件，但不再拥有相应可写事实；后续可做纯物理拆文件，不需要再次迁移 ownership。  
Next phase: R7。

### R7 — Product Lifecycle

Phase: R7  
Changes: 建立 typed update lifecycle、语义版本检测、受限 GitHub release link、startup asynchronous check、bounded retry。  
Removed old truth: 无旧 update mutable truth，因此没有双写迁移期；删除 UI 边界的非类型化版本和链接处理。  
Invariant verification: INV-01、INV-06、INV-07、INV-09。  
Tests: startup non-blocking；一次 terminal outcome；两次 bounded retry；update-only snapshot revision。  
Remaining risks: 未配置 GitHub repository 时 release source 按设计进入 `source_unavailable`，不会伪造“已是最新”。  
Next phase: Unified Acceptance。

### Unified Acceptance Hardening

Phase: Unified Acceptance  
Changes: 补齐 9 类 resource lifecycle catalog；收紧 task-scoped cancellation；PublicSession 归并至 Execution owner；RuntimeCommandHandle 类型化；Control lane 在真实 foreground Work 期间直接控制 detached command；命令终态由 ControlPlane service 提交。  
Removed old truth: PublicSession terminal payload、隐式 cancel-one/cancel-all、session-wide task fallback、broker `cancel_all` 表述、Control 对 facade mutex 的隐藏依赖。  
Invariant verification: INV-01 至 INV-09 全部通过。  
Tests: 351 个 Rust library tests；全部 migration/integration/policy/privilege/packaging tests；11 个 frontend tests；clippy `-D warnings`；build；runtime/UI/schema44 verifiers。  
Remaining risks: 4 个既有显式 ignored 项不属于自动验收路径——3 个 process helper entry points、1 个需要人工 UAC 的测试。  
Next phase: Maintenance。

## 3. 权威 Owner Map

| Fact | 唯一 mutable owner | 只读消费者 |
|---|---|---|
| Desired Permission / Workspace / Runtime / Tunnel / Connection | `DesiredStateOwner` | Reconciler、Snapshot publisher |
| Observed / Effective | ControlPlane convergence owner | Scheduler admission、UI snapshot |
| MCP Session | `SessionRegistry` | transport mapping、Reaper |
| Request | `RequestRegistry<RequestKey, ActiveRequest>` | cancellation mapping、Snapshot faults |
| Task | `TaskRegistry` | Scheduler、TaskAggregate projection |
| Execution + PublicSession + RuntimeCommandHandle | `ExecutionRegistry` | command control、UI projection |
| Work admission | `Scheduler` | transport typed intent |
| Workflow checkpoint | `WorkflowCheckpointStore` | workflow service |
| Output handle | `OutputHandleRegistry` | output mapping |
| Persistent snapshot | ControlPlane snapshot publisher | Tauri UI |
| Broker request | `BrokerRequestOwner` / PrivilegeBroker | privileged adapter |
| Update lifecycle | `UpdateStateOwner` | snapshot publisher、UI |

## 4. Resource Lifecycle Catalog

| Resource | Owner | Retention / terminal | Reaping | Restart / disconnect |
|---|---|---|---|---|
| MCP Session | SessionRegistry | idle TTL / Closed | TTL | transient discard / settle transient |
| Request | RequestRegistry | until terminal / bounded errors | immediate active removal | transient discard / cancel owned |
| Task | TaskRegistry | until terminal / bounded history | oldest terminal | transient discard / settle transient, retain detached |
| Execution | ExecutionRegistry | max 64 active / max 64 terminal | oldest terminal | unfinished → Lost / retain detached |
| PublicSession | ExecutionRegistry | owned by Execution | with Execution | rebuild from Execution / retain detached |
| OutputHandle | OutputHandleRegistry | bounded local/private | oldest handle | expire / retain bounded |
| WorkflowCheckpoint | WorkflowCheckpointStore | one bounded durable checkpoint | clear or replace | preserve durable |
| Diagnostics | DiagnosticsRegistry | bounded active/history | oldest entry | transient discard / retain bounded |
| BrokerRequest | PrivilegeBroker | max 32 active / no terminal copy | response or disconnect | transient discard / cancel broker-owned |

## 5. Invariant 证明

- INV-01：owner map、composition root 与 no-dual-write scans 证明每个 authoritative fact 只有一个 writer。
- INV-02：内部请求使用 `RequestKey(session_id, request_id)`；裸 `RpcRequestId` 只存在于 transport adapter。
- INV-03：Task / Execution stable id；registry 拒绝 terminal resurrection 和冲突终态。
- INV-04：task cancellation 同时校验 MCP Session owner 与 TaskId；Session close 只释放该 Session 自有资源；broker shutdown 只取消 broker-owned requests。
- INV-05：Work 只有 Queued、Running、Terminal；mutex contention 不进入 Task 状态。
- INV-06：UI 从一次 `ControlPlaneSnapshot` 读取所有 section，同次发布共享 revision。
- INV-07：锁竞争、timeout、cache miss 只能形成 stale/unknown/unavailable；不会形成 fabricated Running。
- INV-08：Desired、Observed、Effective 类型和 owner 分离，divergence fail closed。
- INV-09：transport 只提交 typed intent 和映射 typed result；Task / Execution / Session 终态由 ControlPlane registry/service 写入。

## 6. 必须测试矩阵

| 验收项 | Executable proof | 结果 |
|---|---|---|
| Session collision | `r1_same_rpc_id_in_distinct_sessions_has_isolated_cancellation` | PASS |
| Multi-window queue | `multi_window_work_queue_is_bounded_fifo_with_one_foreground_slot` | PASS |
| Detached execution | `completed_foreground_task_restores_detached_execution_projection` | PASS |
| Permission failure | `desired_elevated_with_offline_broker_is_observable_and_non_elevated` | PASS |
| Workspace divergence | `desired_workspace_change_denies_work_until_runtime_observes_the_same_workspace` | PASS |
| Snapshot revision | `publication_is_one_revisioned_atomic_snapshot` | PASS |
| Fault isolation | `a_faulted_section_does_not_blind_ready_sections` | PASS |
| Runtime restart Lost | `runtime_restart_converges_every_unfinished_execution_to_lost` | PASS |
| Lock contention stale | `runtime_owner_lock_contention_marks_activity_stale_without_running_compensation` | PASS |
| Control vs Work | `command_control_kill_is_not_blocked_by_unrelated_foreground_work`（同时验证 poll） | PASS |

## 7. 禁止残留验收

`npm run verify:schema44` 对当前源代码执行静态失败闭合扫描，结果 PASS：

- 无 global bare request-id registry；
- 无 unscoped `cancel_all` operation；
- 无可写 current-command truth；同名字段仅为 ExecutionRegistry 的兼容 projection 或旧格式 migration reader；
- 无 UI idle → completed 猜测；
- 无 lock contention → Running 补偿；
- 无 mutable permission mirror；
- 无 mutable workspace mirror；
- 无 UI live composite projection；
- 无 Tauri `Result<T, String>` UI boundary；
- 无 domain-side → MCP dependency；
- 无 core Domain / ControlPlane `serde_json::Value` state。

## 8. 最终验证命令

```text
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
npm run verify:runtime-manifest
npm run verify:ui-language
npm run verify:schema44
```

最终结果：Rust 351/351 library tests 通过，所有非 ignored 集成测试通过；frontend 11/11 通过；build 和全部当前 schema44 verifier 通过。

## 9. Commit History

| Phase | Implementation | Governance / acceptance |
|---|---|---|
| R1 | `4289b79` | `ff71007` |
| R2 | `8bac267` | `e1071d9` |
| R3 | `6fc5215` | `0af0a48` |
| R4 | `6dded95` | `9e4d777` |
| R4 extension | `8a768c8` | — |
| R5 | `db7f0a4` | `e47a893` |
| R6 | `e76b644` | `0998b90` |
| R7 | `7c14cac` | `505a1ec` |
| Unified hardening | `818f186`, `c89d474` | final acceptance is recorded by the commit that adds this report |

## 10. 最终结论

schema44 的目标不是增加一层 Projection，而是收回所有事实写权限。当前实现已满足该目标：兼容 projection 可以存在，但不能写；transport 可以映射，但不能拥有生命周期；runtime 可以报告 observation，但不能决定 Desired；UI 可以展示 snapshot，但不能补偿或猜测业务状态。

统一验收通过，项目进入维护阶段。
