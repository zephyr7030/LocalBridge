# 06 — Control Plane Refactor Execution

Status: **CURRENT HUMAN AUTHORITY**  
Effective date: 2026-08-21  
Supersedes: `docs/06_PR_GROUPS_AND_EXECUTION.md`, the live LB-PR/G0–G4 execution model, and all live `current_pr/current_group` semantics.

## 1. 重构目标

下一轮暂停继续增加 Update Checker、窗口控制、浏览器能力等上层功能，优先完成底层控制面收口。

本轮不推倒重写、不拆 Rust crate、不更换现有 runtime，不引入数据库或复杂事件系统。

目标只解决 7 个项目级根因：

1. 缺少明确的事实唯一所有者。
2. Request / Task / Execution / Session identity 不完整。
3. 缺少显式 Scheduler，当前依赖 Mutex 隐式串行化。
4. Task / Execution / Public Session 生命周期分裂。
5. Desired / Observed 状态没有分离。
6. Projection / Error 不是控制面的一等状态。
7. MCP 模块承担过多 Domain / Runtime 职责。

核心原则：

> Every authoritative fact has exactly one mutable owner.

不是让一个巨大 ControlPlane 持有所有状态，而是每种事实只能有一个权威写入者。

## 2. 目标架构

```text
                 MCP Transport
                      │
                 Tauri Transport
                      │
                      ▼
               ┌──────────────┐
               │ ControlPlane │
               │   typed API  │
               └──────┬───────┘
                      │
       ┌──────────────┼──────────────┐
       ▼              ▼              ▼
 SessionRegistry   Scheduler      DesiredState
 RequestRegistry   TaskRegistry       │
                   ExecutionRegistry  │
                       │              ▼
                       │         Reconciler
                       │              │
       ┌───────────────┼──────────────┼────────────┐
       ▼               ▼              ▼            ▼
 ExecutionManager  RuntimeController Authority   Workspace
       │
       ├─ Command
       ├─ Filesystem
       ├─ Git
       ├─ Workflow
       └─ Broker
                      │
                      ▼
             ControlPlaneSnapshot
               revision = N
                      │
             ┌────────┴────────┐
             ▼                 ▼
         Dashboard         Diagnostics
```

职责：

- Transport 只负责协议解析和映射。
- Scheduler 只负责 admission / queue。
- TaskRegistry 负责任务生命周期。
- ExecutionRegistry 负责真实执行实体。
- SessionRegistry 负责 MCP Session 生命周期。
- RequestRegistry 负责请求身份和取消。
- Runtime / Broker / Workspace 各自维护真实 observed state。
- ControlPlaneSnapshot 是 UI 唯一读取入口。

## 3. 统一 Identity Model

新增强类型 identity：

```rust
struct McpSessionId(...);
enum RpcRequestId {
    Number(i64),
    String(String),
}
struct RequestKey {
    session_id: McpSessionId,
    request_id: RpcRequestId,
}
struct TaskId(...);
struct ExecutionId(...);
struct PublicSessionId(...);
```

完整关系：

```text
McpSessionId
    ↓
RequestKey
    ↓
TaskId
    ↓
ExecutionId
    ↓
PublicSessionId
```

规则：

- JSON-RPC id 永远不能单独作为内部 request identity。
- `(session_id, request_id)` 才是唯一 RequestKey。
- 每个 Task 有稳定 TaskId。
- 每次真实执行有独立 ExecutionId。
- detached command 才拥有 PublicSessionId。
- ConnectionId 仅用于 diagnostics，不作为业务 identity。
- 暂不增加 ClientId，除非未来存在跨 MCP Session 的稳定客户端身份需求。

## 4. Task / Execution 生命周期

### Task

删除“Running → Idle，由 UI 猜 Completed”的设计。

```rust
enum TaskState {
    Queued,
    Running,
    AwaitingAuthorization,
    Terminal(TaskOutcome),
}
enum TaskOutcome {
    Completed,
    Blocked,
    Failed,
    Cancelled,
    TimedOut,
    Lost,
}

struct Task {
    id: TaskId,
    owner_session: McpSessionId,
    request: RequestKey,
    kind: TaskKind,
    summary: SafeTaskSummary,
    state: TaskState,
    created_at: ...,
    started_at: Option<...>,
    completed_at: Option<...>,
}
```

必须满足：

```text
Accepted
→ Queued / Running
→ exactly once Terminal
```

Idle 不属于 Task。Idle 只表示“没有 foreground task”的 Aggregate 状态。

### Execution

Task 和 Execution 分离。

```text
exec_command(dev server)

Task A
Running
→ Completed

Execution X
Running

PublicSession S
Running
```

请求完成不代表启动的进程结束。长期 detached process 不允许占住 Scheduler。

```rust
struct Execution {
    id: ExecutionId,
    task_id: TaskId,
    kind: ExecutionKind,
    state: ExecutionState,
    public_session_id: Option<PublicSessionId>,
}
```

Execution terminal vocabulary 与 TaskOutcome 统一：`Completed / Failed / Cancelled / TimedOut / Lost`。

## 5. 废除单槽 CurrentCommand 真相

删除：

```text
PersistedTaskState {
    current_command: Option<...>
}
```

改为：

```text
ExecutionRegistry
├─ Execution A running
├─ Execution B completed
├─ Execution C running
└─ ...
```

持久化只保存真正需要 durable 的信息。

`CurrentTask / CurrentCommand` 不再是事实源，而只是：

```text
TaskRegistry + ExecutionRegistry
        ↓
Presentation Projection
```

因此：

```text
A running
B starts
B completed
A still running
```

不得产生 `B running` 的错误状态。

## 6. Dashboard 保留单一当前活动状态

内部允许多个实体，但 Dashboard 仍只显示一个当前活动状态，且必须是纯投影：

1. 当前 foreground Running / Queued Task；
2. 若无，则显示 active detached Execution；
3. 若均无，则 Idle。

例如 A 为 detached dev server，B 为 filesystem read：B 执行时显示“读取文件”，B 完成后恢复 A 的“运行命令”。

禁止继续使用：

```text
any command session running
→ 当前 Task 不清除
```

这种全局布尔补偿。

## 7. 显式 Scheduler

第一版只设计三类 lane：

### Observation

`workspace_context / task_control(get) / diagnostics / snapshot` 等，不进入普通任务队列。

### Control

`task_control(cancel) / command_control(poll|write|kill)` 等，高优先级，不得被普通执行队列阻塞。

### Work

`filesystem / git / document / exec_command / agent_workflow / elevated_exec` 等，使用 bounded FIFO。

第一阶段只允许 1 个 foreground work slot。

关键要求：detached Execution 建立后立即释放 foreground slot。因此启动 dev server 不会阻塞之后的文件读取、Git、构建等任务。

## 8. 废除 Mutex 隐式排队

`Mutex<AgentFacade>` 不得继续承担 Scheduler 职责。

目标流程：

```text
Request
→ Scheduler.admit()
→ Queued
→ Running
→ Terminal
```

禁止：

```text
Request
→ lock()
→ 不知道在等什么
```

Queue 必须有明确容量 `MAX_WORK_QUEUE`。容量满时返回 `QueueCapacityExceeded`，并进入 Diagnostics 与 `ControlPlaneSnapshot.active_faults / admission state`，不得只返回 HTTP 503 而 UI 完全不知道。

## 9. Request 跨 Session 隔离

第一优先级替换：

```text
active_requests: Vec<Value>
```

为：

```text
HashMap<RequestKey, ActiveRequest>
```

同时替换 `local_filesystem_requests / privileged_requests / privileged_filesystem_requests`，全部统一使用 `RequestKey`。

必须保证：Session A/request 1 与 Session B/request 1 是两个完全不同的请求；Session A 请求结束时 `remove(RequestKey(A,1))` 不得影响 B。

## 10. task_control(cancel)

禁止“遍历所有 active_requests → 全部 cancel”。

接口保持尽量兼容：

```text
task_control(
    action = "cancel",
    task_id?
)
```

规则：

- 提供 task_id：只能取消“当前 MCP Session + 指定 TaskId”拥有的 Task；跨 Session 返回 `TaskNotOwned`。
- 未提供 task_id：当前 Session 0 个 cancellable task → no-op；1 个 → cancel；>1 个 → `TaskIdRequired`。
- 绝不允许 fallback 为 cancel all。
- Dashboard 如果未来需要“停止全部”，使用独立 desktop intent，不复用 MCP session-scoped task_control。

## 11. MCP Session 生命周期

```rust
struct McpSession {
    id: McpSessionId,
    protocol: ...,
    tool_catalog_signature: ...,
    created_at: ...,
    last_seen: ...,
    owned_requests: HashSet<RequestKey>,
    owned_tasks: HashSet<TaskId>,
}
```

增加统一 `SessionRegistry` 与 `SessionReaper`。

生命周期：

```text
Created
→ Active
→ Closing
→ Closed
```

客户端 DELETE：remove session → cancel transient owned requests → settle transient tasks → detached/durable resources 按明确 policy detach 或保留。

异常断线：`last_seen → TTL → Reaper`。

达到 `MAX_DOWNSTREAM_MCP_SESSIONS` 前，先 reap expired，再判断 capacity，避免死 session 堆积最终 503。

## 12. 统一 Resource Lifecycle Policy

所有长期资源统一定义：Owner / Retention / Terminal condition / Reaping condition / Restart behavior / Disconnect behavior。

至少覆盖：MCP Session、Request、Task、Execution、PublicCommandSession、OutputHandle、WorkflowCheckpoint、Diagnostics Entry、Broker Request。

禁止各模块自行决定生命周期。

逻辑终态、内存释放、持久状态、public handle 失效必须具有清晰关系。

## 13. Desired / Observed / Effective

正式建立：

```text
DesiredState
ObservedState
EffectiveState
```

DesiredState：用户配置目标，如 `permission_mode / active_workspace / services_enabled / connection_profile`。

ObservedState：实际运行状态，如 `runtime / broker / applied_workspace / tunnel / coding_runtime`。

EffectiveState：根据 Desired + Observed 纯函数派生。

示例：

```text
desired.permission = Elevated
observed.broker = Offline

configured_mode = Elevated
effective_authority = Full
reconciliation = AwaitingAuthorization / BrokerUnavailable
```

这是明确、合法、可观察的状态，不再视作“状态不一致 bug”。

## 14. 权限状态改成派生结果

避免继续维护 `settings.permission_mode / runtime.permission_mode / MCP RwLock<PermissionMode> / broker gate / PrivilegeState` 多份必须同步的权限真相。

最终：

```text
ConfiguredPermission
    owner = DesiredState
BrokerState
    owner = PrivilegeController
EffectiveAuthority
    = derive(configured_permission, broker_state)
```

例如：

```text
ElevatedActive =
configured_mode == Elevated
&& broker_state == Active
```

PEP 每次 authorization 只读取 `EffectiveAuthority`，不再分别读取多个平行状态自行组合。

## 15. Workspace 使用 Desired / Observed

切换 A → B：

```text
Desired.workspace = B
↓
Reconciler
↓
Runtime apply B
↓
Observed.workspace = B
```

当 `Desired = B / Observed = A` 时：

```text
EffectiveWorkspaceAuthority = Unavailable
```

禁止继续授权 A。

失败后不再依赖“切 B → 保存失败 → 切回 A”的大量 rollback。Reconciler 持续尝试将 Observed 收敛到 Desired。

## 16. ControlPlaneSnapshot 是 UI 唯一状态源

停止 `get_main_projection()` 每次现场读取 Settings / Runtime / Task / Broker / Credential / StartupProfile / Workspace 再拼装。

改为 owner 状态变化后发布：

```rust
struct ControlPlaneSnapshot {
    revision: u64,
    captured_at_ms: u64,
    runtime: RuntimeProjection,
    authority: AuthorityProjection,
    scheduler: SchedulerProjection,
    workspace: WorkspaceProjection,
    connection: ConnectionProjection,
    settings: SettingsProjection,
    active_faults: Vec<UiFault>,
}
```

一次 publication：

```text
mutation
→ derive snapshot
→ revision += 1
→ wake UI
```

UI 获取 Snapshot revision N，所有字段必须属于 N。

## 17. 禁止 fabricated state

硬规则：`lock contention != running`。

删除类似：

```text
TryLockError::WouldBlock
→ state=active
→ workflow=running
```

只能使用 revision N-1 的上一份 snapshot 并 `stale=true`，或返回 `availability=TemporarilyUnavailable`。绝不允许根据锁竞争猜测业务状态。

## 18. Projection 局部故障隔离

`ControlPlaneSnapshot` 必须尽可能 total。

例如 CredentialStore 故障时：`credential = Fault(...)`，但 runtime/task/scheduler/broker/workspace 继续正常输出。

禁止 `Credential Err → MainProjection 整体 Err → Dashboard 完全失明`。

每个 subsystem projection 支持：`Ready(...) / Unavailable(...) / Fault(...)`。

## 19. 统一 UI Typed Error

删除 Tauri 大量 `Result<T, String>`，改为：

```rust
struct UiError {
    code: UiErrorCode,
    category: UiErrorCategory,
    message: String,
    retryable: bool,
    operation_id: Option<...>,
    request_id: Option<...>,
    task_id: Option<TaskId>,
}
```

Tauri 使用 `Result<T, UiError>`。

React 不再 `errorText(value: unknown)` 后只显示三秒字符串。

错误分为：

- OperationError：一次操作失败，Toast；
- PersistentFault：持续故障，进入 `ControlPlaneSnapshot.active_faults`，真实恢复后才删除。

UI 不拥有 fault 生命周期。

## 20. 模块职责调整

暂时不拆 crate。目标目录：

```text
src-tauri/src/
domain/
    identity.rs
    task.rs
    execution.rs
    authority.rs
    snapshot.rs
control_plane/
    controller.rs
    scheduler.rs
    session_registry.rs
    request_registry.rs
    task_registry.rs
    execution_registry.rs
    reconciliation.rs
execution/
    command.rs
    filesystem.rs
    shell.rs
    git.rs
    workflow.rs
runtime/
privilege/
workspace/
transport/
    mcp/
        server.rs
        protocol.rs
        mapping.rs
    tauri/
        commands.rs
        mapping.rs
```

`mcp/` 最终只负责：

```text
MCP request
→ typed Intent
→ ControlPlane
→ typed Result
→ MCP response
```

逐步迁出 `filesystem_service / path_authority / shell / task_state / workflow_checkpoint / verification_planner / context_service / edit_service` 等实际 Domain / Execution 模块。

迁移顺序：先迁移 ownership，再移动文件。禁止边改语义边大规模移动目录。

## 21. 限制 serde_json::Value

允许范围：Transport boundary、Opaque tool arguments、Adapter private payload。

禁止核心状态使用：Task lifecycle、Execution lifecycle、Request identity、Scheduler state、Authority state、ControlPlaneSnapshot。

`RuntimeDriver::task_aggregate() -> Value` 应最终删除。Domain 内使用 Rust 类型系统表达 invariant。

## 22. 实施阶段

不再使用 LB-PR 或 G 包。严格按 R1 → R5 串行推进。

### R1｜Identity / Isolation

完成：`McpSessionId / RpcRequestId / RequestKey`；替换 active_requests、filesystem cancellation、privileged requests、privileged filesystem requests、notifications/cancelled；修复 `task_control(cancel)` 为 session scoped。

目标：消灭跨 MCP Session 请求碰撞和取消串台 P0。

### R2｜Task / Execution Convergence

新增：`TaskRegistry / ExecutionRegistry / TaskId / ExecutionId`；统一 terminal vocabulary；删除单 current_command 真相、无 identity CurrentTask、Running→Idle→UI 猜 Completed；CurrentTaskProjection 降级为纯 UI presentation。

目标：彻底解决 A/B command 错态、resume/terminal 分裂。

### R3｜Explicit Scheduler + Session Lifecycle

增加 Observation lane / Control lane / Work FIFO，替代 Mutex 隐式等待；加入 Session ownership / last_seen / TTL / Reaper / Queue capacity。

目标：多窗口请求明确排队，而不是 worker/Mutex 饱和后 502/503。

### R4｜Desired / Observed Convergence

迁移 Permission / Workspace / Runtime service state / Connection profile，采用 `Intent → Desired → Reconcile → Observed → Effective`。

目标：删除同步调用链和 rollback 补偿模式。

### R5｜Snapshot / Typed UI / Module Cleanup

建立 `ControlPlaneSnapshot / revision / UiError / active_faults`；删除 fabricated running、live composite MainProjection、`Result<T,String>`、TaskAggregate Value；最后整理 mcp/ 模块边界。

## 23. 迁移纪律

必须使用 strangler migration。每迁移一个事实：

```text
建立新 owner
→ 迁移 reader
→ 迁移 writer
→ 禁止旧 owner 写入
→ 删除旧 truth
```

绝不允许长期 `旧状态 + 新 ControlPlane 状态` dual-write。

## 24. 架构 Invariant

- **INV-01** Every authoritative fact has exactly one mutable owner.
- **INV-02** Every request identity is scoped by MCP session.
- **INV-03** Every Task and Execution has stable identity and exactly one terminal outcome.
- **INV-04** Cancellation may only affect resources owned by its declared scope.
- **INV-05** Accepted Work must be explicitly Queued, Running or Terminal; implicit Mutex waiting is not task state.
- **INV-06** UI snapshots are published under one revision; fabricated business state on lock contention is forbidden.
- **INV-07** Every runtime resource type has an explicit bounded lifecycle and reaping policy.
- **INV-08** Desired, Observed and Effective state are distinct; Effective authority/workspace is derived fail-closed.
- **INV-09** Transport layers cannot own domain lifecycle state.

合同测试重点验证这些 invariant，而不是函数名和字符串。

## 25. 必须新增的对抗性测试

- Session A request id=1；Session B request id=1 → 两请求完全独立。
- A cancel request 1 → B request 1 不受影响。
- A/B/C 多 MCP Session 同时提交任务 → 明确 queue，不返回 busy 错误代替排队。
- A detached command running；B short command completed → 不得显示 B running，正确恢复 A projection。
- B `task_control(cancel)` → 只能取消 B 自己的任务。
- Session DELETE → owned transient request 正确 terminal；detached/durable resource 按 policy 处理。
- 客户端异常退出 → TTL/reaper 最终释放 MCP Session。
- accepted Task → exactly once terminal。
- Runtime restart → unfinished Execution → Lost，不允许永久 Running。
- lock contention → stale/unavailable，绝不能 fake Running。
- CredentialStore fault → Runtime/Task/Broker 状态仍可观察。
- desired Elevated + broker Offline → configured Elevated；effective non-elevated；reconciliation 可观察。
- desired workspace B + observed workspace A → execution authority fail-closed。
- Snapshot revision N → 所有 section 属于同一 N。

## 26. 暂缓功能

完成 R1–R3 前，暂缓新增会扩大并发面的功能：多窗口增强、窗口观察/操作、浏览器自动化、复杂 Workflow、更多后台长生命周期能力。

Update Checker 等独立、低耦合功能后置，不插入核心重构。

Filesystem 已有功能以修 bug 为主，不继续扩大职责。

## 27. 最终完成标准

本轮完成后必须达到：

- 一个 Request 有唯一 RequestKey；
- 一个 Task 有唯一 TaskId；
- 一个 Execution 有唯一 ExecutionId；
- 一个 PublicSession 有唯一 Execution owner；
- 没有全局裸 JSON-RPC request id registry；
- 没有 cancel-all 伪装成 `task_control(cancel)`；
- 没有单 current_command 作为多 execution 真相；
- 没有 presentation 层猜 Completed；
- 没有 lock contention → fake Running；
- 没有 MainProjection live composite read；
- 没有 UI `Result<T,String>` 核心边界；
- 没有 Permission/Workspace 多份状态人工同步；
- 没有 MCP Transport 持有 Domain 生命周期。

达到这些条件后，再继续新增功能。

最终目标不是代码“更漂亮”，而是建立一个能够长期承载多 MCP Session、多窗口、detached command、任务排队、权限切换、断线恢复、长期后台运行，而不会产生组合状态爆炸的执行平台底座。
