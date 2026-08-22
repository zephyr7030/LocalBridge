# LocalBridge 架构与实现缺陷审查记录

> 状态：持续审查中；本文保存当前已经确认的缺陷，后续审查只能追加、修正证据或在真实修复并验证后标记为已解决，不应无依据删除结论。
>
> 事实优先级：当前磁盘源码 / 真实黑盒测试 > 当前合同与状态文档 > 历史测试、旧报告、聊天结论。
>
> 目标读者：后续接手 LocalBridge 的 AI / 架构工程师。
>
> 审查基线：2026-08-22；分支 `codex/release-0.1.2`；HEAD `12df18f3ed9c06f9aa860f90c9372f3258bbba05`。下方“本地修复核验”记录基线之后尚未提交的工作树结果。
>
> 状态标签：`CONFIRMED` = 当前源码或黑盒可直接证明；`PARTIAL` = 核心事实成立但原措辞/严重度需校正；`DESIGN_DEBT` = 设计债/简化候选，不作为独立 runtime bug；`RESOLVED` = 只有真实修复并验证后才可使用。

### 严重度定义

- **P0**：已存在可复现的核心控制、安全、生命周期或资源治理失败，能够导致任务失控、能力事实错误、资源耗尽路径或 GPT 无法管理真实执行；应阻断发布或优先修复。
- **P1**：显著正确性/架构缺陷；当前可能仍 fail-closed、受容量限制或主要表现为错误状态/高维护成本，但会持续制造不一致和恢复问题。
- **P2**：低直接风险的合同、可维护性和复杂度问题；其中标记 `DESIGN_DEBT` 的条目不得冒充已复现 runtime bug。

### 2026-08-22 核验索引

| 条目 | 状态 | 当前证据摘要 |
|---|---|---|
| P0-01～02 | CONFIRMED | `PrivilegeController` 同时维护 `PrivilegeState + gate_open + ActiveBroker`，执行能力不是单一状态。 |
| P1-03 | CONFIRMED | `ControlPlane` 不拥有 Privilege/Broker lifecycle；当前只完成 Desired owner 收口。 |
| P1-04 | PARTIAL | PEP route selection 仍消费 `configured`，但真正 privileged execution 仍检查 `PrivilegeState::Active + gate + Broker`，未证明直接越权。 |
| P1-05～09 | CONFIRMED | UI 仍用 desired admin 展示能力；权限切换/error/schema44 仍存在语义和治理漂移。 |
| P0-10～14 | CONFIRMED | detached execution 的 session ownership 与真实 GPT 重连不兼容；本轮真实工具 schema 仍不能传 `task_id`。 |
| P1-15 | CONFIRMED | 内部集合事实继续被 latest/current 单槽投影；本轮观察到 terminal activity 与 running current projection 同时存在。 |
| P0-16 | CONFIRMED | Observation/Control 无 worker admission；listener 仍按连接直接创建线程。 |
| P1-17～23 | CONFIRMED | 阻塞线程队列、无界 pending/protocol buffer、主动 polling、orphan/stale/TTL/workflow 闭环均未完成。 |
| P1-24～26 | CONFIRMED | 500ms UI retention 仍由后端 shadow queue + 每任务短命线程实现。 |
| P1-27～29 / P2-30 | CONFIRMED | lifecycle catalog 不驱动统一行为；`WorkflowDatum` 是为绕 Value shape gate 形成的 JSON AST。 |
| P1-31～38 | CONFIRMED | production negotiate 真启动 semantic probe；Adapter 有假成功默认；God object/多层 projection/schema source drift 存在。 |
| P2-39～40 | CONFIRMED | projection 状态组合过宽，错误映射已丢失 policy 语义。 |
| P2-41 | DESIGN_DEBT | `UiError(Box<...>)` 与 size 测试存在，但是否值得删除属于 profile/维护权衡。 |
| P2-42 / P1-43 / P2-44～45 | CONFIRMED | Scheduler lane 命名与能力不一致；旧 constructor 与字符串复用协议存在。 |
| P2-46～48 | DESIGN_DEBT | Update mini state machine / 低价值类型测试事实存在，但不作为独立 runtime failure。 |
| P1-49 / P2-50 | CONFIRMED | schema44 可在上述问题仍存在时 PASS；历史治理巨物仍在可执行路径。 |
| 综合判断 A/B | SUMMARY | 原 P1-51/P1-52 改为综合结论，不再重复计入 defect。 |
| P1-53 | CONFIRMED | NSIS `PREUNINSTALL` 无条件 `CredDeleteW`，且 release verifier 反向要求该行为存在。 |
| P1-54 | CONFIRMED | `START_HERE.md` 仍描述 R1→R5/当前 R1，而 `PROJECT_STATE.json` 已是 R7。 |

### 2026-08-22 本地修复核验

本表取代上面的审查基线状态，但保留原证据供追溯。`RESOLVED` 只用于已由当前源码和本地行为测试验证的条目。

| 条目 | 当前状态 | 修复与证据 |
|---|---|---|
| P0-01～02 | RESOLVED | `PrivilegeController` 只写一个类型化 `BrokerLifecycle`；`Active` 必然持有真实 Broker，gate/process 不再是平行 truth。 |
| P1-03 | PARTIAL | Desired 与 Broker lifecycle 已各自收口为单一 owner，UI/PEP 读取相同 owner；`ControlPlane` 仍未成为覆盖所有产品域的唯一组合根。 |
| P1-04～09 | RESOLVED | PEP 只按 Effective authority 选路；UI 使用 `effective/elevated_active`；权限意图接受与 broker reconcile 错误分离；新增独立管理员审核错误码和行为验证。 |
| P0-10～14 | RESOLVED | 增加 session-scoped task list/get/cancel、显式 PublicSession adopt、真实 schema 字段与多任务返回；取消不再影响非 owner 资源。 |
| P1-15 | RESOLVED | 公共控制面可列举完整 owned task/execution；latest/current 只保留为从 registry 派生的 UI presentation，不再可写。 |
| P0-16 | RESOLVED | Observation/Control 有独立并发容量，连接 worker 有硬上限，容量错误进入类型化错误与 snapshot。 |
| P1-17 | PARTIAL | Work FIFO、队列和连接线程均有硬上限；queued Work 仍占用一个有界连接 worker，尚未迁移为纯 task-data executor。 |
| P1-18～23 | RESOLVED | 输出/protocol buffer 有界；detached observation 自适应；Execution orphan/stale TTL、active-request-aware Session reaper、workflow stale/cancel 闭环已执行。 |
| P1-24～26 | RESOLVED | 删除后端 500ms shadow presentation、第二任务状态机和每任务短命线程；UI 仅展示 revisioned terminal observation。 |
| P1-27～29 / P2-30 | RESOLVED | 删除描述性 lifecycle catalog 和 shape gate；删除 `WorkflowDatum`，opaque workflow payload 回到明确 adapter/persistence 边界。 |
| P1-31～32 | RESOLVED | 生产 negotiate 不再启动真实 shell semantic probe；Adapter 未实现能力默认 fail-closed，测试夹具必须显式声明能力。 |
| P1-33～34 | PARTIAL | 已删除 presentation/compatibility 大段旧职责和历史巨型测试，但 `mcp/facade.rs`、`mcp/server.rs` 仍需继续按真实职责拆分。 |
| P1-35～36 | RESOLVED | 删除 `runtime_snapshot_cache`；非活动 Runtime observation 归唯一 Runtime owner，锁竞争只发布 revisioned stale snapshot；Diagnostics 不再维护第二套 active-request registry。 |
| P1-37～38 | PARTIAL | Task/Execution 核心状态保持 typed，公共 action 表已统一常量；MCP output JSON mapping 和全部 schema 尚未迁到独立单一协议模块。 |
| P2-39～40 | RESOLVED | `ProjectionSection` 字段私有且只能经合法构造器变迁；管理员未审核错误映射为 policy/Denied，不再降级为 Unknown。 |
| P2-41 | DESIGN_DEBT | 保留现状；不属于本轮已复现 runtime failure。 |
| P2-42 / P1-43 / P2-44～45 | RESOLVED | 三 lane 有真实 admission；旧 PEP constructor 仅测试可见；管理员确认使用独立 RPC 和后端 deadline。 |
| P2-46～48 | DESIGN_DEBT | 保留现状；后续以 profile 和维护收益决定是否简化。 |
| P1-49 / P2-50 | RESOLVED | schema44 只做禁止残留扫描，行为由 Rust gate 证明；删除旧 G4/PR provenance 巨型 verifier 和旧 `verify:lb001` 可执行链。 |
| P1-53～54 | RESOLVED | 卸载默认保留凭据，仅显式 `/DELETEUSERDATA=1` 删除；入口文档只引用 live phase authority。 |
| P0-55 | RESOLVED | 删除 ordinary Shell 首层字符串权限分类、静态脚本/重定向/rmdir 补偿与 toolbox 命令改写；Full 明确以当前 Windows 用户令牌执行 Shell 及全部后代，结构化路径授权与 Shell execution 分离。内层 authenticated loopback runtime 改为 policy-neutral adapter，LocalBridge Guard 成为唯一授权 owner；direct `sc` 与脚本后代 `sc` 黑盒权限一致。 |

本地门禁（2026-08-22 当前工作树）：frontend 11 tests、Rust 351 library tests 与全部已启用 migration/integration/policy/privilege/packaging targets、Clippy `-D warnings`、schema44 residue scan、release preflight、license、runtime resources、测试基座 8 项结构约束均取得明确退出码 0。3 个 process helper entry point 和 1 个需要人工 UAC 的用例显式 ignored，均不计入自动验收。最终 socket 修复后的完整 revision46 黑盒矩阵 2/2 通过（合计 100 次 chunked request + 100 次 empty preconnect），且两次都与基础外部客户端在同一测试进程中串行运行；没有自动 reconnect 或 retry。另有确定性 delayed-request 时序测试固定 accepted-socket 行为。本轮按用户限定只执行本地验证，没有重新打包、push 或运行云端 CI；真实 cloud Tunnel 未运行。

上述证据只证明本表标记为 `RESOLVED` 的修复和当前本地门禁，不恢复 schema44 统一架构验收。`P1-03`、`P1-17`、`P1-33～34`、`P1-37～38` 仍为 `PARTIAL`，因此 `PROJECT_STATE.json.unified_acceptance` 必须保持 `INVALIDATED`；在这些边界缺陷被真实迁移并完成统一行为验收前，不得使用“架构重构完成”或“统一验收通过”。

### 2026-08-22 API revision 46 增量核验

| 用户复现 | 当前状态 | 本地修复与行为证据 |
|---|---|---|
| detached command Session ownership | RESOLVED | `poll/write/kill` 使用同一 MCP Session 的稳定 ownership；增量读取不重复，terminal output 可继续按 `output_ref` 分页读取。 |
| `command_control.wait_ms` 长时间失效 | RESOLVED | Runtime I/O 使用覆盖完整请求链的 deadline；超时返回 `OperationTimedOut`，且不会伪造 Execution terminal。真实 bundled runtime 的 `write/kill(wait_ms=0)` 行为测试要求 1.5 秒内返回。 |
| `task_control(cancel)` 假成功 | RESOLVED | 当前 Session 没有可取消 owned target 时返回 `NotFound`；存在其他 live Session 的 workflow 时返回带 `task_id` 的 `TaskIdRequired`，不再返回 `ok=true` 空取消。 |
| catalog revision 导致 MCP Session terminated | RESOLVED | 工具 catalog 签名变化只更新 Session 内 pending notification，不再关闭或删除 Session；缩权后同一 Session 立即按新 Effective authority fail-closed。 |
| Tunnel HTTP 400/502 透传 | RESOLVED（已定位路径） | 修复 bounded chunked decoder；拒绝 `Content-Length + Transfer-Encoding` 歧义、压缩 body、坏 chunk、超限 header/body，并由行为测试固定 framing。仍需真实前台 tunnel 长稳黑盒验收。 |
| Git invalid ref / blame 越界吞错 | RESOLVED | adapter error 不再被空对象投影覆盖；facade 归一化为 typed error。 |
| document 越界与 output handle taxonomy | RESOLVED | document 拒绝非法行范围并明确 `eof`；不存在的 output handle 返回 `OutputNotFound`，stream 不匹配的 `InvalidArgument.details` 指明字段、期望值和实际值。 |
| `filesystem` 绕过 active-workspace Path Authority | RESOLVED | 删除 public `filesystem` 的隐式 Broker 升权分支及对应 cancellation target；Full/Elevated/Broker Active 均只走统一 `WorkspaceResolver + PathAuthority`。工作区外 read/write/delete 的行为测试全部返回 `WorkspaceDenied`，Broker 未被调用。 |
| workspace 卷根 list/search 返回 `NotFound` | RESOLVED | Windows 枚举在已验证根目录之下遇到不可打开、reparse 或竞争消失的子项时标记 `truncated` 并继续，不再把单个系统目录失败投影成整个卷根不存在；`.`、绝对卷根和递归 search 行为测试通过。 |
| Task 可观察但不可取消 / cancel 假终态 | RESOLVED | detached `Execution` 以稳定 `task_id` 作为控制 capability；`task_control(cancel)` 只在 owned target 接受取消时返回成功，并显式返回 `cancellation_requested=true`。该字段不伪造 terminal；测试继续 poll 到 ExecutionRegistry 的唯一 `cancelled` 终态。 |
| `KILL` timeout 后投影为 `failed/ProcessFailed` | RESOLVED | cancellation intent 由 ExecutionRegistry 唯一持有；KILL 的 transport timeout 不删除 intent，后续观察到进程退出时统一提交 `Cancelled/ProcessCancelled`，TERM/KILL 和即时/后续 poll 不再生成两种 domain outcome。 |
| Elevated 本地化控制台输出乱码 | RESOLVED | Broker 输出先保持合法 UTF-8；非 UTF-8 字节按当前 Windows OEM code page 经 Win32 转为 Unicode，再进入脱敏与公开映射。真实 `whoami.exe /user` 测试确认无替换字符。 |
| 本地 PEP 随机 `ECONNRESET` | RESOLVED | 确认 Windows 上非阻塞 listener 的 accepted socket 会继承非阻塞模式；请求字节稍晚到达时旧实现把 `WouldBlock` 误判为断开。生产连接边界现显式切回 blocking 并设置 deadline；确定性 delayed-request 测试在修复前失败、修复后通过，测试客户端未增加 retry。真实 cloud Tunnel 仍未运行。 |

### 2026-08-23 API revision 47 本轮复核

| 用户报告 | 当前状态 | 根因修复和行为证据 |
|---|---|---|
| phased durable workflow 跨调用 `TaskNotOwned` | RESOLVED | `TaskId` 成为显式可转移 capability；重连后的 MCP Session 只有提交精确稳定 `task_id` 才能原子接管同一 checkpoint owner，省略或错误 ID 仍 fail-closed。外部 ChatGPT 风格客户端已完成 prepare → 跨 Session resume → cancel → terminal。 |
| `task_control(cancel)` 无法控制可见 detached Task | RESOLVED | 当前 Session 没有所有权且请求未携带 `task_id` 时返回 `TaskIdRequired` 并给出可管理 identity；显式 TaskId 只取消该 Task/Execution，不存在 cancel-all 回退。 |
| Runtime unavailable 被投影为 Lost | RESOLVED | Runtime unavailable/protocol/capability failure 统一提交 Task `Failed`；只有 Session 生命周期丢失提交 `Lost`。 |
| 五屏引导权限维护本地默认值 | RESOLVED | `OnboardingState.permission + projection_revision` 来自一次 revisioned 后端 snapshot；React 不再持有平行 permission state，准备项目也不再重复写权限。切换后同时复读 onboarding/MainProjection 并确认后端 desired 值。 |
| workspace 根 search 扫描 10000 项 | RESOLVED | `filesystem.search/list` 默认非递归，只扫描明确根层；递归必须显式请求，截断不再被默认行为意外触发。 |
| 检查更新无论如何返回空 | RESOLVED | `retry_update_check` 从 `UiResult<()>` 改为返回真实 `UpdateProjection`；生命周期 owner 在启动网络线程前同步进入 `Checking`，因此调用至少返回可观察状态、版本、release URL 和 operation identity。 |
| GitHub 页面后端返回空 | RESOLVED | `open_github_releases` 返回 typed `OpenReleaseProjection { release_url }`；生产构建固定使用 `zephyr7030/LocalBridge`，不再依赖可缺失的 build env。URL 经过 repository allowlist 后才交给系统浏览器。 |

本轮本地证据：Rust 356 个 library tests 与全部非 ignored integration targets 通过，Clippy `-D warnings` 通过，frontend 11 tests/build、test-base、schema44 residue、public-release、license、runtime-resource gate 均通过；外部 revision47 客户端通过跨 Session workflow/task、command budget、filesystem 和终态一致性矩阵。Task cancel 竞态矩阵在定点修复后连续 5 次通过，并再次通过包含所有 Rust targets 的最终共享门禁。GitHub 官方 latest-release API 实际返回 `v0.1.2` 和非空 release URL。这里仍不恢复统一架构验收：真实 authenticated cloud Tunnel 未运行，且本文件标记的结构性 `PARTIAL` 项仍未完成。

### P0-55｜后代进程可绕过 Shell Policy（RESOLVED：删除虚假 Shell 子权限层）

原 Shell Policy 只能分类顶层命令文本。工作区脚本通过顶层分类后，可在后代进程中启动系统管理程序；Windows Job Object 只能约束进程归属、终止与资源，不能对后代 image 或系统调用实施所需的选择性权限策略。因此继续扩充危险命令字符串匹配不构成修复。

本轮做了隔离边界原型验证：普通 AppContainer 仍允许该后代调用；LPAC 能阻断调用，但同时阻断普通 `cmd`、PowerShell、`where`、批处理和编译器后代，破坏 LocalBridge 承诺的 shell/build 工作负载。该临时原型、测试探针和依赖已全部移除，没有把 preview 隔离组件带入产品。

本轮选择与产品所需任意 coding/build Shell 相容的明确边界：Full Shell 的 authority 就是当前 Windows 用户令牌，Shell、脚本、解释器、工具与全部后代一致；需要管理员令牌的 structured administrator work 仍只能走 Broker。结构化 filesystem/document/Git/image 继续使用 active-workspace Path Authority，但不再解析或补偿 Shell command text。为消除第二个可写策略 owner，bundled runtime 的 Guard-backed production route 改为 authenticated loopback policy-neutral adapter；Edit/Full/Elevated、transitive capability、WorkspaceResolver 与 Broker route 均由 LocalBridge Guard 授权。

已删除的旧补偿包括 ordinary command classifier、system-management executable blacklist、静态 workspace script scanner、CMD absolute-redirection rewrite、`rmdir→rd` alias rewrite、PowerShell ordinary provider/autoload restriction和 toolbox command parser。真实外部 MCP client 复核 direct `sc query EventLog` 与 workspace `.cmd → sc.exe` 都以 current-user authority 完成；结构化 workspace 外读取仍为 `WorkspaceDenied`，管理员 route 仍要求 Active Broker。该修复不声称提供进程树 sandbox；UI/schema 已明确 Full Shell 可使用当前用户本来拥有的 OS/文件权限。

## 0. 总结

当前 LocalBridge 最大的问题不是某几个孤立 bug，而是以下结构性模式同时存在：

1. **事实唯一所有者没有真正落实。** Desired permission 已有所收口，但 Authority、Runtime、Task presentation、Diagnostics request tracking 等仍存在影子事实或多份 live state。
2. **内部已变成 multi-task，公共控制协议仍停留在 single-current。** GPT 无法可靠列举、定位、取消、接管自己创建的 detached Execution。
3. **资源治理只覆盖部分路径。** Work queue 有界，但 Observation / Control worker 无界；MCP Session 有 TTL，但 orphan Execution、durable workflow、pending output 没有完整回收闭环。
4. **UI presentation 侵入后端真实生命周期。** “任务至少可见 500ms”是合理 UX，但当前通过后端影子队列和线程实现，污染真实 Task 生命周期。
5. **架构验证过度检查源码形状，反过来催生炫技代码。** 典型包括 `WorkflowDatum`、不驱动行为的 lifecycle catalog、重复 Projection / Owner / Lease。
6. **抽象层增加，但 God object 没有消失。** `mcp/facade.rs` 与 `mcp/server.rs` 仍承担极多职责，形成“旧复杂度 + 新抽象 + 转换层”的复杂度净增加。

### 修复总原则

- 不再新增新的 `Owner / Manager / Coordinator / Facade / Projection / State enum`，除非能明确删除旧 owner 或旧状态。
- 每个 live fact 只有一个 mutable owner；其他位置只能读取原子快照或派生值。
- UI 展示策略不得改变后端真实生命周期。
- GPT 可见 API 必须能完整管理后端允许存在的多任务 / 多 Execution。
- 资源必须有明确 owner、上限、terminal condition、断连策略和 reaper；这些规则必须由代码执行，不只是文档矩阵。
- Schema / CI 优先验证行为和 invariant，不验证“某个名字是否存在”。
- 修公共根因，不给每个调用者补同步逻辑。

---

# A. 权限 / Authority

## P0-01｜`PrivilegeController` 内仍存在多份管理员能力 live truth

### 现状

管理员权限实际至少由以下状态共同决定：

```text
PrivilegeState
+ gate_open: AtomicBool
+ active: Option<ActiveBroker>
```

真正具备管理员执行能力需要近似满足：

```text
PrivilegeState == Active
&& gate_open == true
&& ActiveBroker/session 实际存在且可用
```

启用路径对这些字段分别写入，例如先保存 Broker、再 `set_state(Active)`、再打开 gate。关闭 / 故障路径也分别更新。

### 风险

类型系统允许出现理论上不应该存在的组合，例如：

```text
PrivilegeState = Active
Gate = Closed
Broker = Some(...)
```

或者状态与 Broker/session 短暂分裂。

### 根因

“都在一个 `PrivilegeController` 对象里”不等于“只有一个事实”。当前实际上是多个需要人工同步的事实变量。

### 正确方向

将真实 Broker 生命周期收口为一个不可表达非法组合的 owner，例如单一：

```text
BrokerLifecycle =
  Disabled
  | Requested
  | AwaitingUac
  | Active { generation, broker }
  | Faulted { ... }
```

执行能力只能从这个 owner 的同一状态匹配得到，不再有独立 `gate_open` 真相。

---

## P0-02｜`PrivilegeState::Active` 与真实执行条件语义不一致

### 现状

公开状态中的 `accepts_privileged_calls()` 基本只依据 `PrivilegeState::Active`。

但真正执行还需要 gate 与 ActiveBroker/session。

### 风险

任何 UI、MCP、Diagnostics 或 Policy 消费者只读取 `PrivilegeState::Active`，都可能错误认为管理员执行能力可用。

### 正确方向

禁止对外暴露一个语义不完整的 `Active` 作为 capability truth。对外 capability 必须来自同一 Authority snapshot 中的 `effective/elevated_active`。

---

## P1-03｜新 `ControlPlane` 没有成为真正的 Authority owner

**Status: CONFIRMED**

### 现状

`ControlPlane` 主要拥有：

```text
DesiredStateOwner
RequestRegistry
TaskRegistry
ExecutionRegistry
Scheduler
SessionRegistry
```

但没有真正拥有 Privilege/Broker lifecycle。

`DesktopLifecycle` 仍独立持有：

```text
PrivilegeController
DesiredStateOwner
```

然后读取两者并派生 Authority projection。

### 证据

- `src-tauri/src/control_plane/owner.rs`：`ControlPlane` 字段中没有 Privilege/Broker owner。
- `src-tauri/src/app/background.rs`：`DesktopLifecycle` 独立持有 `privilege: PrivilegeController` 与 `desired: DesiredStateOwner`。

### 结论

schema44 真正完成的是 **Desired permission owner 收口**，不是完整 **Authority owner 收口**。

当前执行链仍总体 fail-closed，因此该条按 P1 处理；它是结构性 Authority 违约，不等同于已经证明的直接越权。

---

## P1-04｜PEP route selection 仍消费 `configured`，EffectiveAuthority 未成为唯一授权输入

**Status: PARTIAL（原 P0 描述过重，已校正）**

### 已确认事实

MCP 路径当前仍存在：

```rust
let effective = policy_effective_state(...);
let mode = effective.authority.configured;
```

随后 `mode` 被送入 public policy / elevated handler。也就是说，Desired/configured intent 仍参与 route classification，而不是所有 capability decision 都直接从一个 EffectiveAuthority capability truth 派生。

### 当前安全边界

本轮源码核对没有证明 `configured = Elevated` 可以单独授予管理员执行能力。真正 privileged execution 仍至少经过：

```text
policy/review
→ PrivilegeState::Active
→ PrivilegedExecutionGateway.require_gate()
→ gate_open
→ ActiveBroker/session
→ Broker execution
```

因此当前更准确的结论是：

```text
Configured intent 仍参与授权路径选择
+
实际能力又由 PrivilegeController/Gateway 二次判断
```

这是 **capability truth 未收口**，而不是已证明的直接越权。

### 风险

- route classification、错误解释和真实执行能力依赖不同事实；
- 新调用者容易误把 configured 当 capability；
- EffectiveAuthority 不能成为全系统唯一、可审计的授权输入；
- 后续继续加补丁会扩大“configured + broker + gate”多阶段判断。

### 正确方向

PEP capability authorization **只读取 EffectiveAuthority**。Configured 只用于 reconciliation / error explanation，例如说明“用户希望 Elevated，但 Broker 尚未 active”，不得作为 capability grant 的独立输入。

---

## P1-05｜UI 把 Desired Elevated 当成真实管理员能力

**Status: CONFIRMED**

### 现状

后端 `AuthorityProjection` 已有：

```text
desired
effective
broker
elevated_active
```

但 `MainProjection` 面向前端主要输出 `permission` 与 `privilege`。其中 `permission` 当前取自 `authority.desired`。

前端 `src/App.tsx` 仍存在：

```ts
const adminModeFullAccess = projection?.permission === "admin";
```

并据此显示“全目录访问”、改变项目切换行为。

### 错误状态示例

```text
Desired = Elevated
Broker = Disabled / Awaiting / Faulted
Effective != Elevated
```

UI 仍可能把 desired admin 表现成已经具有管理员能力。

### 定级说明

当前执行层仍会重新检查 Broker/gate，未证明 UI 错报可以直接造成越权，因此由原 P0 校正为 P1；它仍是明确的权限事实一致性缺陷。

### 正确方向

UI 的能力展示与真正执行 gate 必须消费同一 Authority snapshot。Desired 只表示用户配置目标，不表示实际 capability。

---

## P1-06｜`set_permission_mode` 存在“RPC 返回失败，但 Desired 已改变”的语义

### 现状

大致顺序：

```text
保存 settings
→ 修改 Desired
→ 刷新 snapshot
→ 请求/关闭 Broker
→ reconciliation
```

如果 Broker 请求失败，调用最终可以返回错误，但 settings + Desired 已成功变更。

### 风险

调用者看到的是：

```text
set_permission_mode failed
```

真实事实却是：

```text
configured 已改变
reconciliation 失败
effective 尚未达到 configured
```

### 正确方向

权限设置应返回 typed result / Authority snapshot，例如：

```text
configured
effective
reconciliation_state
broker_state
```

“失败”应仅代表 intent 没有被接受/保存，而不是“intent 成功但 reconcile 未完成”。

---

## P1-07｜管理员错误码把“策略未审核”和“路由不可用”混为一谈

### 已验证事实

黑盒调用未被 elevated policy review 的请求时可得到 `PrivilegedRouteUnavailable`。

但使用 policy 允许的 `whoami.exe /user` 实测成功，证明当时 Broker route 本身可用。

### 风险

`PrivilegedRouteUnavailable` 无法区分：

- Broker 真正不可用；
- Gate 关闭；
- Broker session 丢失；
- 当前 elevated operation 未被 review；
- policy deny。

### 正确方向

至少区分：

```text
PolicyDenied / ElevatedOperationNotReviewed
ElevationRequired
PrivilegedRouteUnavailable
```

错误解释不能通过牺牲授权模型来实现。

---

## P1-08｜历史权限 / Dashboard 测试已与现行目标语义漂移

**Status: CONFIRMED（校正原“旧测试保存错误权限语义”的宽泛表述）**

### 当前证据

历史 `tests/e2e/dashboard/lb015_contract.test.mjs` / `lb015_hardening.test.mjs` 仍通过源码字符串形状约束权限切换和 Dashboard 行为；其中部分断言要求当前实现里已经不存在的 `previous == PermissionMode::Elevated` 等源码结构。

同一组历史测试还明确要求后端保留：

```text
MIN_TASK_PRESENTATION = 500ms
VecDeque<QueuedTask>
```

这说明旧治理测试会把历史实现形状当成合同，容易反向阻止当前架构收口。

### 风险

- 后续 AI 为让旧测试 PASS，可能重新引入已经淘汰的同步/状态结构；
- 代码行为与历史 shape test 发生冲突时，治理层无法说明谁才是事实源；
- “测试存在”被误认为“当前语义正确”。

### 正确方向

删除、归档或明确降级历史 shape tests；真正约束权限的测试应验证 EffectiveAuthority / Broker fail-closed / UI capability 一致性，而不是函数体里是否出现某段字符串。

---

## P1-09｜schema44 权限验证存在严重假阴性

### 现状

验证器禁止：

```text
RwLock<PermissionMode>
Mutex<PermissionMode>
```

但完全识别不到：

```text
PrivilegeState + gate_open + ActiveBroker
```

仍是多份 live truth。

### 结论

当前 schema44 可以 PASS，同时 Authority 仍未真正单 owner。

### 正确方向

改成行为/invariant 测试：

- Desired Elevated + Broker non-active ⇒ Effective 绝不能 Elevated；
- UI 与 PEP 消费同一 authority snapshot；
- 不存在 `Active + gate closed` 这种可观察组合；
- capability grant 只由 effective 决定。

---

# B. 多任务 / Execution / Session / GPT 管理

## P0-10｜GPT 无法可靠管理自己启动的 detached Execution

### 黑盒复现

同时启动两个 detached command A/B 后：

- `task_control(get)` 只显示最新的一个；
- `task_control(cancel)` 返回成功但 `cancelled_requests=0`，命令仍在运行；
- 使用返回的 `session_id` 调 `command_control(kill)`，可得到：

```text
Public command session is not owned by this MCP session
```

### 根因

LocalBridge 假定：创建 Execution 后，GPT 后续调用会一直使用同一个 MCP Session。

真实 ChatGPT 端不保证这一点。

---

## P0-11｜Execution ownership 没有 reconnect / adopt / transfer

### 现状

Execution 绑定：

```text
owner_session: McpSessionId
```

另一个 session 尝试绑定会 `OwnerConflict`。

没有：

```text
adopt
transfer
reconnect token
client ownership
conversation ownership
```

### 后果

```text
GPT 创建 detached Execution
→ 下一次工具调用换 MCP Session
→ Execution 仍归旧 Session
→ 新调用无法控制
→ orphan Execution
```

---

## P0-12｜MCP Session 会被回收，但 detached Execution 被故意保留且无法接管

### 现状

MCP Session：

```text
max ≈ 64
idle TTL ≈ 5 min
```

Session close/reap 会取消 active request、queued task，但 Running detached Execution 按设计保留。

### 问题

Execution 的 owner_session 仍指向已经消失的 MCP Session，系统又没有 transfer/adopt。

因此 `RetainDetached` 实际制造 orphan。

---

## P0-13｜公共 API 没有真正的多任务管理能力

当前缺少完整的：

```text
tasks/list
executions/list
sessions/list

task/get(id)
task/cancel(id)
execution/get(id)
execution/kill(id)
execution/adopt(id)   // 若保留 session ownership 模型
```

当前 `task_control(get)` 本质是 current/latest projection，不是任务管理器。

### 结论

内部已经 multi-task，GPT 端仍是 single-current 控制模型。

---

## P0-14｜真实暴露给 GPT 的 `task_control` Schema 与源码能力不一致

**Status: CONFIRMED｜2026-08-22 再次黑盒核验**

源码 `src-tauri/src/mcp/facade.rs` 已定义：

```text
task_control(action=get|cancel, task_id?)
```

并明确：当当前 MCP Session 拥有多个 cancellable task 时，`task_id` 用于精确选择。

但本轮真实暴露给 GPT 的 LocalBridge `task_control` 工具合同仍只有：

```text
action = get | cancel
```

没有可传的 `task_id` 字段。

### 后果

同一 Session 存在多个可取消任务时：

```text
后端要求精确 task_id
GPT 实际 schema 无法表达 task_id
```

因此 source schema 与真正运行时/产品边界 schema 已发生 drift。

### 额外治理反证

`scripts/verify-schema44/index.mjs` 只检查 `facade.rs` 源码是否包含 `task_id` schema 字符串；它不会验证 GPT 最终获得的真实 tool schema。

### 结论

这是公共 schema drift 的直接实证，必须保留 P0。

---

## P1-15｜内部 multi-task，公共投影仍只保留 latest/current

**Status: CONFIRMED｜含本轮黑盒矛盾状态**

当前内部存在：

```text
TaskRegistry
ExecutionRegistry
SessionRegistry
Scheduler
```

但 UI/MCP 聚合仍大量读取：

```text
latest_active
latest_running
latest_terminal
current_workflow
current_command
last_command
```

真实事实是集合，公开事实却被压缩成 latest/current 单槽。

### 2026-08-22 黑盒证据

本轮执行一个验证命令时，`task_control(get)` 曾同时返回同一任务/执行的：

```text
last_activity = completed
current_activity = running
current_command = running
```

即真实 terminal observation 已出现，而 current/latest presentation 仍投影为 running。

### 结论

这不是单纯 UI 延迟问题，而是多个生命周期/投影来源在同一 aggregate 中竞争“当前事实”。

---

# C. 生命周期与系统资源

## P0-16｜Observation / Control 没有真正资源背压

### 现状

Scheduler 定义：

```text
Observation
Control
Work
```

但只有 Work 有 admission / queue / capacity。

Observation / Control 只是 active counter。

MCP server 每接受一个请求仍直接 `thread::spawn`。

### 风险

可持续创建：

```text
TCP socket
OS thread
RequestRegistry entry
```

Work queue 完全限制不了这些请求。

### 正确方向

要么统一固定 worker pool / bounded executor，要么至少给所有请求类型建立真实 connection/worker admission 上限。

---

## P1-17｜Work queue 是“阻塞线程队列”，不是轻量任务队列

### 现状

一个 queued Work 仍占：

```text
OS worker thread
TCP socket
Request
Session refs
Condvar waiter
TaskRecord
```

虽然 queue 最多约 32，避免了无限增长，但资源模型仍然偏重。

### 正确方向

parse/auth 后应尽量只 enqueue task data，由固定 worker 执行，而不是“一个请求一个线程，在 Condvar 上睡着”。

---

## P1-18｜`PublicCommandSession.pending_output` 无字节上限

### 现状

```rust
pending_output: String
```

后台会持续 `push_str(output)`。

### 风险

GPT 断开或不再消费时，持续输出命令可让 LocalBridge 内存持续增长。

`OutputHandleRegistry` 的上限无法保护该独立 String。

### 正确方向

使用 bounded ring buffer、truncate + output_ref、或直接把输出统一交给有界 Output store。

---

## P1-19｜`stderr_protocol_buffer` 在不完整协议片段下可能无界增长

**Status: CONFIRMED（补充触发条件）**

Public command session 还有独立：

```text
stderr_protocol_buffer: String
```

普通 stderr 在 `drain_public_stderr_protocol_buffer()` 中会被及时 drain；风险主要出现在看起来像 CLIXML/protocol envelope、但长期收不到完整结束标记的输入。

此时当前逻辑会保留未完成片段等待后续数据，而该 String 没有明确 max bytes / truncation policy。

### 风险

恶意、异常或损坏的长协议片段可持续扩大 retained buffer。该风险比 `pending_output` 更有条件性，但仍缺失明确容量边界。

### 正确方向

为 protocol assembly 设置独立 hard cap；超过后终止解析或降级为可见 stderr，并记录 truncation/protocol fault。

---

## P1-20｜所有 running detached command 被固定约 100ms 主动轮询

### 现状

server 主循环约每 100ms reap/poll running command。

最大 active execution ≈ 64。

### 风险

即使没有任何 GPT 消费者，也持续触发上游状态/输出轮询，造成不必要 CPU、IPC、MCP 请求负担。

### 正确方向

优先使用进程 watcher/event；若必须 poll，采用 adaptive interval，并区分“有消费者”和“仅保底 reaper”。

---

## P1-21｜Active Execution 没有独立 stale/orphan TTL

Execution Registry 有 active/terminal 数量上限，但 Running record 没有：

```text
deadline
last_observed_at
orphan_since
stale_after
max_detached_age
```

底层 command timeout 能覆盖正常路径，但若 Runtime terminal observation 丢失，ControlPlane 可长期保留 Running，直到重启才被收敛成 Lost。

---

## P1-22｜MCP Session idle TTL 不检查 active request

Session idle 主要依据“多久没有新请求”。

一个真实运行超过 5 分钟的同步请求理论上仍可能被 session reaper 判定 idle。

### 正确语义

```text
idle = 无 active request / control operation
       && 超过 idle threshold
```

而不是单纯“没有收到新 HTTP 请求”。

---

## P1-23｜durable workflow 可以永久占据 waiting

### 已观察事实

真实 `task_control(get)` 曾长期返回旧 workflow：

```text
state = waiting
current_step = edit
next_step = verify
```

但没有对应 Running command。

### 当前缺失

WorkflowCheckpoint 没有：

```text
created_at
updated_at
TTL
owner conversation/client
stale policy
```

并缺乏清晰的公共 cancel/clear 管理闭环。

### 后果

- 永久污染 CurrentTask；
- 阻塞后续 side-effect workflow；
- 不占 OS 进程，但形成逻辑资源泄漏。

---

# D. UI Presentation 与后端生命周期混杂

## P1-24｜“任务至少可见 500ms”被错误实现于后端

### 需求判断

“极短任务至少让用户看到约 500ms”是合理 UX，**不是缺陷**。

缺陷是该需求被写进后端真实任务 projection/lifecycle。

### 当前实现

`CurrentTaskProjection` 为 500ms 展示维护：

```text
latest_presentation
current
queued
active_sequence
next_sequence
last_tool
visible_since
completed_at
```

### 正确职责

```text
后端：任务真实结束 → 立即发布 terminal / next state
UI：保留 displayedActivity 到 visibleSince + 500ms，再切换视觉
```

UI 最短展示时间不得延迟或修改后端真实终态。

---

## P1-25｜`CurrentTaskProjection` 已成为第二套任务状态机

虽然源码注释声称 authoritative lifecycle 在 `TaskRegistry`，但 `CurrentTaskProjection` 自己维护 queue、sequence、current、completed、last tool、retirement。

这已经不是纯 projection。

实际形成：

```text
TaskRegistry
+
CurrentTaskProjection shadow lifecycle
```

应删除后者的生命周期职责，只保留纯 DTO 派生；500ms 交给 UI。

---

## P1-26｜为了 UI 500ms，每个完成任务创建短命 OS 线程

`schedule_retirement()` 会 `thread::spawn` 后 sleep 到展示期结束。

高频工具调用时会制造大量短命线程。

这是完全不必要的系统资源消耗；前端 timer 就能实现同样视觉目标。

---

# E. “炫技”式架构复杂度

## P1-27｜`ResourceLifecyclePolicy` 是描述性矩阵，不驱动真实行为

`resource_lifecycle.rs` 为资源建立多组枚举：

```text
ResourceKind
ResourceOwner
ActiveRetention
TerminalRetention
TerminalCondition
ReapingCondition
RestartBehavior
DisconnectBehavior
```

并维护多项 policy catalog。

### 问题

Session / Task / Execution / Output / Workflow 的真正回收逻辑仍分别在 Registry / Server 内手写。

因此这套 catalog 不是真正 lifecycle engine，只是一份必须人工同步的“代码化文档”。

### 已有反证

系统仍出现：

- orphan Execution；
- durable workflow 永久 waiting；
- unbounded pending output；
- GPT 失去 detached session 控制权。

说明 catalog 并未形成行为约束。

### 正确方向

二选一：

1. 真正让生命周期策略驱动统一实现；或
2. 删除 catalog，用实际 registry/reaper 行为 + 行为测试表达规则。

当前状态最差。

---

## P1-28｜schema44 强制要求无效 lifecycle catalog 存在

验证器直接要求类似：

```text
RESOURCE_LIFECYCLE_POLICIES: [ResourceLifecyclePolicy; 9]
```

只证明“矩阵存在”，不证明它执行。

这是典型 architecture-by-shape。

---

## P1-29｜schema44 禁止 `serde_json::Value`，反而催生 `WorkflowDatum`

### 当前实现

`WorkflowDatum` 自己重造：

```text
Null
Boolean
Signed
Unsigned
Float
Text
Array
Object
```

本质几乎等同 `serde_json::Value`。

### 根因

CI 禁止某个具体类型名，而不是要求 checkpoint 拥有真正 typed domain schema。

### 结论

这是“换名字绕源码扫描”的典型炫技代码。

---

## P2-30｜`WorkflowDatum` 带来大量无收益转换

调用链反复：

```text
serde_json::Value
→ WorkflowDatum
→ serialize
→ deserialize
→ WorkflowDatum
→ serde_json::Value
```

增加代码与错误路径，但没有获得更强业务约束。

### 正确方向

Checkpoint 只持久化 resume 真正需要的稳定字段；能明确类型的字段直接 typed，必须 opaque 的少数字段才使用 Value。

---

## P1-31｜生产 Runtime negotiate 会启动真实进程做“接口语义单元测试”

### 现状

`AgentFacade::with_adapter()` 无条件调用 `adapter.negotiate()`。

Production adapter 在 negotiate 中不仅校验 schema/workspace，还真的启动 PowerShell，测试：

```text
exec
stdin
output_ref
read_output
kill_session
```

命令甚至包含 `Start-Sleep -Seconds 30`。

### 问题

这是 integration/compatibility test 应承担的职责，却进入每次 Runtime 初始化 / 恢复生产路径。

### 成本

- 多创建真实进程；
- 增加 AV/EDR 扫描与启动延迟；
- 增加失败点；
- 增加后台进程/窗口风险；
- Runtime 恢复时重复执行无业务价值的探针。

### 正确方向

生产只做版本/协议/capability 握手和轻量 readiness；完整 stdin/output/kill 行为留给 CI 集成测试。

---

## P1-32｜`WorkspaceRuntimeAdapter` 默认实现会伪造成功

### 当前危险默认值

部分 trait 方法默认：

```text
runtime health = Ready
verification plan = []
edit preconditions = Ok
coding context = empty success
apply_coding_patch = expected.keys()
```

### 风险

未来新增 Adapter 忘记实现方法时：

```text
编译成功
运行也可能“看起来成功”
```

违反 fail-closed。

### 正确方向

关键 capability 不应提供假成功默认值。要求实现者显式实现，或默认返回 `CapabilityUnavailable`。

---

## P1-33｜`mcp/facade.rs` 是约 404 KB 级 God object

当前单文件同时承担：

```text
public schemas
private runtime capability contracts
runtime negotiation
project discovery
workspace path normalization
filesystem
shell
command sessions
public/private session mapping
output refs
workflow checkpoint
git/document/image
error normalization
task aggregate
execution projection
```

### 结论

增加 Domain / Adapter / ControlPlane 后，Facade 职责没有真正被切走。

---

## P1-34｜`mcp/server.rs` 是约 348 KB 级 God object

同时负责：

```text
TCP listener
HTTP parsing
MCP protocol
Session lifecycle
Request lifecycle
Scheduler
Task lifecycle
Task presentation
Permission policy
Elevated execution
Filesystem
Task control
Connection worker
Errors/Diagnostics wiring
```

### 结论

不是“还缺抽象”，而是现有抽象没有真正把职责从 server 移除。

---

## P1-35｜Runtime 状态被重复缓存和投影多次

当前路径大致：

```text
RuntimeOrchestrator
→ DesktopRuntimeSnapshot
→ runtime_snapshot_cache
→ 500ms watchdog
→ ObservedState
→ ConvergenceSnapshot
→ ControlPlaneSnapshot
→ MainProjection
→ React state
```

### 风险

每层都可能 clone/cache/stale/fallback，使真实状态与 UI/Diagnostics 产生滞后或矛盾。

### 正确方向

减少中间 live cache。Snapshot 可以存在，但必须是单 owner 发布的 immutable snapshot，而不是多个缓存再互相同步。

---

## P1-36｜Diagnostics 重复维护 Request lifecycle tracking

**Status: CONFIRMED（弱化“第二个 authoritative RequestRegistry”的原表述）**

系统已经有：

```text
RequestRegistry
RequestKey(session_id + request_id)
```

Diagnostics 又维护：

```text
active_requests: HashMap<String, ActiveRequestDiagnostic>
```

并自行记录 Start / End / Lost / Eviction。

### 边界校正

当前 diagnostics registry 主要用于观测与事件记录，并不直接承担授权/取消，因此不应称为第二个 authoritative RequestRegistry。

问题在于 **Request lifecycle 被两套机制独立跟踪**：真实 registry 与 diagnostics tracking 各自决定何时 active、lost、evicted、terminal。

### 风险

可产生：

```text
真实 Request terminal
诊断仍 active / lost
```

或真实 request 仍存在，而 diagnostics 因自身容量策略先 eviction。

### 正确方向

Diagnostics 应消费 RequestRegistry 的 lifecycle event / immutable snapshot，避免自己重新推断 request truth。

---

## P1-37｜Execution 状态被连续多次 JSON projection

同一 Execution 大致经历：

```text
ExecutionRecord
→ latest_running_execution_snapshot() JSON
→ facade task_aggregate_snapshot() JSON
→ merge_control_plane_activity()
→ UI projection
```

字段如 task_id / execution_id / session_id / status 多次复制、覆盖、重建。

### 风险

任何一层漏同步字段都产生 schema / lifecycle drift。

---

## P1-38｜公共协议没有真正的单一 Schema source

同一接口至少存在：

```text
facade.rs input JSON Schema
facade.rs output JSON Schema
业务 json!() 返回体
Rust MainProjection DTO
TypeScript bridge.ts interface
```

这些并非由一个源自动生成。

### 已有实证

后端支持 `task_id`，真实 GPT 工具 schema 却曾无法传 `task_id`。

### 正确方向

至少对公共 MCP tools 采用单一 typed contract → 自动生成 schema / validator / DTO，减少手工平行维护。

---

## P2-39｜`ProjectionSection<T>` 允许大量无意义状态组合

当前通过类似：

```text
availability
stale: bool
value: Option<T>
```

表达状态。

理论上可出现：

```text
Ready + stale=true
Ready + value=None
Unavailable + value=Some(...)
```

### 正确方向

用闭合 enum 表达合法状态，例如：

```text
Ready(T)
Stale(T)
Unavailable
Fault(...)
```

让非法组合无法表示。

---

## P2-40｜错误模型层级过多且已损失语义

当前至少有：

```text
OperationError
ErrorDiagnostic
FacadeError / FacadeErrorCode
UiError
RuntimeFault
Policy errors
Registry errors
```

再由大量 `normalize_* / map_* / to_mcp_result` 转换。

### 已有后果

`ElevatedExecNotReviewed` 可最终表现为 `PrivilegedRouteUnavailable`。

### 正确方向

建立一个稳定 canonical operation error，边界只做必要 protocol encoding；不要每一层重新发明一套错误 taxonomy。

---

## P2-41｜`UiError(Box<UiErrorFields>)` 的微观优化复杂度（DESIGN_DEBT）

**Status: DESIGN_DEBT｜不作为独立 runtime bug。**

源码专门 Box UI error payload，并测试 `size_of::<UiError>()` 足够小。

### 问题

一个普通 UI 错误 DTO 减少几十字节栈大小基本无实际收益，反而每个错误增加 heap allocation 和额外 Deref/From 代码。

### 正确方向

保持普通 typed struct，除非有真实性能 profile 证明 Box 必要。

---

## P2-42｜Scheduler 名称暗示能力大于实际能力

```text
SchedulerLane::Observation
SchedulerLane::Control
SchedulerLane::Work
```

看起来是统一三 Lane Scheduler。

实际：

```text
Work = 真正调度器
Observation = counter
Control = counter
```

`SchedulerPermit::Immediate` 主要只是 RAII 计数器。

### 风险

让代码读者误以为 Observation / Control 已有背压，掩盖 P0-16。

---

## P1-43｜`PolicyEnforcementRuntime` 保留多套旧兼容入口

当前保留近似：

```text
start
start_with_wake
start_with_privilege
start_with_privilege_and_wake
start_with_control_plane
```

全部汇入 `start_inner`。

没有完整 ControlPlane 时仍可临时构造 DesiredStateOwner。

### 风险

新架构迁移后仍保留旁路，使“完整 ControlPlane 模式”无法成为强制 invariant。

### 正确方向

生产路径只保留一个明确构造入口；测试需要不同依赖时使用 test-only fixture/builder，不保留旧生产旁路。

---

## P2-44｜`set_permission_mode(mode: String)` 偷偷承载自定义字符串控制协议

实际 mode 还承载：

```text
admin-consent-begin:<id>
admin-consent-cancel:<id>
admin-consent-confirm:<id>
```

一个 PermissionMode 参数被扩展成隐式 RPC multiplexing。

### 损失

- 类型安全；
- 可发现性；
- 权限边界清晰度；
- 前后端合同稳定性。

### 正确方向

使用明确 command：

```text
begin_admin_consent
cancel_admin_consent
confirm_admin_consent
set_permission_mode
```

---

## P2-45｜管理员确认前后端重复维护 9 秒计时状态

后端用 `Instant/not_before` 做安全 gate 是必要的。

前端也维护 `performance.now + setInterval(100ms)` 做 countdown。

视觉倒计时可以存在，但当前依赖手工 challenge 字符串协议同步。

### 正确方向

`begin_admin_consent()` 返回 challenge + server not-before/deadline；UI 只做显示，后端永远是唯一安全判定源。

---

## P2-46｜更新检查对象体系偏重（DESIGN_DEBT）

**Status: DESIGN_DEBT｜复杂度评价，不表示当前更新检查已发生运行时失败。**

单一“检查 GitHub 最新 Release”功能被拆成多层，并引入类似：

```text
ProductVersion
GitHubRepository
ReleaseDiscovery
UpdateLifecycle
UpdateStateOwner
UpdateCheckLease
ReleaseSource
UpdateChecker
```

其中 semver、URL ownership、timeout/retry 有真实价值；但整体 owner/lease/lifecycle 复杂度明显高于功能规模。

### 修复原则

不要为了“统一架构感”继续扩张；能用一个小 state + request function 表达就不建立新的 mini control plane。

---

## P2-47｜`UpdateCheckLease` 的 mini transaction state machine（DESIGN_DEBT）

**Status: DESIGN_DEBT｜不作为独立 runtime bug。**

`begin → lease → attempt → succeed/fail → Drop 自动 Failed(CheckLost)` 逻辑完整，但对于一个小型、短时更新检查功能偏重。

单独看并非 bug，但它反映出项目反复使用“Owner + Lease + StateMachine”解决简单问题的倾向。

---

## P2-48｜部分身份测试价值偏低（DESIGN_DEBT）

**Status: DESIGN_DEBT｜测试收益问题，不是产品故障。**

`McpSessionId / PublicSessionId / TaskId / ExecutionId` 使用 newtype 本身是正确设计，应保留。

但部分测试仅证明“不同 newtype 内部可以有相同字符串”，价值很低，体现测试体系偏向验证架构形式而非行为。

---

## P1-49｜架构治理脚本大量验证源码形状，而不是 runtime invariant

schema44 大量检查：

```text
某文件是否存在
某字符串是否出现
某类型名是否被禁止
某函数名是否出现
```

无法证明：

- Authority 是否真正只有一个 truth；
- GPT 是否能管理所有 detached session；
- 所有资源是否有上限；
- UI 是否绑定 effective authority；
- queue/reaper 是否在真实异常路径收口。

### 结论

当前 CI 允许“形状 PASS、语义 FAIL”。

---

## P2-50｜仍存在规模异常的历史治理巨物

例如：

```text
g4-human-gate.mjs ≈ 318 KB
其测试 ≈ 88 KB
```

它验证大量 review provenance / commit scope / first-parent / generation 等治理信息。

### 校正

当前主 `test:ci` 并不直接执行这套 G4 verifier；它仍挂在 `verify:architecture` / 旧 `verify:lb001` 等路径。

### 问题

仍属于可执行历史治理复杂度，对当前 0.1.x 产品运行时正确性贡献有限，应考虑归档或大幅缩减。

---

### 综合判断 A｜抽象层增加，但核心代码没有减少

当前已有：

```text
domain
state
control_plane
runtime
app
mcp
facade
adapter
projection
diagnostics
registry
owner
lease
scheduler
resource lifecycle
```

同时核心文件仍约：

```text
mcp/facade.rs ≈ 404 KB
mcp/server.rs ≈ 348 KB
filesystem/service.rs ≈ 121 KB
execution/policy.rs ≈ 97 KB
```

### 结论

当前不是“缺少抽象”，而是：

```text
旧复杂度
+ 新抽象复杂度
+ 两者之间的 projection / mapping / compatibility complexity
```

复杂度净增加。

---

### 综合判断 B｜项目形成固定的“遇到问题就新造状态机”设计习惯

已观察到：

```text
权限：
DesiredState / ObservedState / EffectiveAuthority / PrivilegeState / gate / ActiveBroker

任务：
TaskRegistry / CurrentTaskProjection / WorkflowCheckpoint / TaskAggregate

执行：
ExecutionRegistry / PublicCommandSessions / private runtime session

请求：
RequestRegistry / Diagnostics active_requests

Runtime：
RuntimeState / DesktopRuntimeSnapshot / runtime_snapshot_cache / ControlPlaneSnapshot

更新：
UpdateLifecycle / UpdateStateOwner / UpdateCheckLease
```

### 风险

每增加一套状态机，就增加同步、转换、恢复、错误映射和测试成本，也增加理论上不应存在的状态组合。

### 修复原则

后续重构优先：

```text
删除旧状态
复用现有事实源
直接派生快照
```

而不是继续建立新的 Manager/Owner/Projection。

---

# F. 卸载 / 用户数据

## P1-53｜卸载时未勾选“删除用户数据”仍会删除系统 API Key

**Status: CONFIRMED**

### 已确认源码事实

当前 `scripts/public-release/nsis-hooks.nsh` 在 `NSIS_HOOK_PREUNINSTALL` 中无条件调用：

```text
CredDeleteW("LocalBridge/RuntimeApiKey/runtime-api-key", ...)
```

该 hook 没有“删除用户数据”checkbox / opt-in flag / 保留用户数据判断，因此正常卸载路径本身就会尝试删除 Runtime API Key。

更严重的是，`scripts/lb018-release.mjs::verifyUninstallCredentialCleanupInvariant()` 还把 `NSIS_HOOK_PREUNINSTALL + CredDeleteW + 固定 credential id` 作为发布 invariant 检查，等于当前 release verifier 正在反向固化这一错误行为。

### 正确产品语义

若用户未明确选择“删除用户数据”，卸载行为必须保留所有用户级持久数据，包括至少：

```text
Windows Credential Manager 中的 Runtime API Key
用户设置
工作区注册信息
可恢复的用户状态（若产品定义为用户数据）
```

只有用户明确勾选“删除用户数据”后，才允许删除 Credential Manager 中 LocalBridge 自己拥有的凭据。

### 风险

- 用户明确选择保留数据却仍丢失凭据；
- 重新安装后必须重新输入 API Key；
- 卸载 UI 语义与实际 destructive behavior 不一致；
- 属于用户数据完整性问题。

### 修复约束

1. NSIS uninstall 默认路径不得删除 Runtime API Key。
2. “删除用户数据”必须是显式 opt-in。
3. 只有 opt-in 路径调用 credential deletion。
4. 卸载程序只能删除 LocalBridge 自己命名空间下的凭据，禁止宽泛清理。
5. 增加真实安装/卸载集成测试：

```text
Case A: 不勾删除用户数据 → API Key 保留
Case B: 勾删除用户数据   → API Key 删除
```

6. 若卸载器无法直接读 UI checkbox 状态，必须通过明确 installer variable / uninstall flag 传递，不允许把“卸载程序正在运行”本身当作删除用户数据授权。

---

# G. 治理入口一致性

## P1-54｜项目入口治理文档与当前执行状态漂移

**Status: CONFIRMED**

### 当前事实

`START_HERE.md` 仍写有近似：

```text
Live sequence: R1 → R2 → R3 → R4 → R5
current phase: R1
```

但当前权威状态文件已经是：

```text
PROJECT_STATE.json.current_phase = R7
CONTROL_PLANE_REFACTOR_CONTRACT.json.strict_phase_order = R1 ... R7
unified_final_acceptance_after = R7
```

### 风险

`START_HERE.md` 又被定义为后续 AI 的首要入口文档，因此新智能体可能在开始执行前就拿到互相冲突的阶段模型：

```text
入口文档说 R1/R1→R5
状态文件说 R7/R1→R7
```

这会直接污染任务选择、允许修改范围、完成条件和历史阶段判断。

### 正确方向

入口文档不得复制易漂移的 live phase。应只写：

```text
当前阶段唯一读取 PROJECT_STATE.json.current_phase
阶段顺序唯一读取 CONTROL_PLANE_REFACTOR_CONTRACT.json
```

若必须显示人类可读摘要，应由权威状态自动生成，不再手工维护第二份阶段事实。

---

# H. 不应误删的复杂度

以下实现虽然有一定复杂度，但目前存在明确安全/可靠性价值，不应因为“反炫技”而机械删除：

- DPAPI 保护 workflow checkpoint；
- checkpoint 原子替换 / 写入完整性；
- Windows Job Object 用于进程树生命周期；
- typed ID newtype（TaskId / ExecutionId / SessionId 等）；
- workspace path identity / link 安全检查；
- bounded output handle；
- fail-closed 权限安全边界；
- terminal RAII 若它直接维护唯一 authoritative lifecycle，而不是影子状态。

“简单”指减少重复事实、重复层和无收益抽象，不是删除安全边界。

---

# I. 建议修复顺序

## 第一阶段：控制面收口（P0 + Authority P1）

1. Authority 单 owner：消除 `PrivilegeState + gate_open + ActiveBroker` 多真相。
2. PEP 只读取 EffectiveAuthority。
3. UI 暴露并使用 effective/elevated_active。
4. 建立真正 GPT 可用的 tasks/executions list + targeted control。
5. 解决 MCP Session ownership 与真实 GPT 重连模型不兼容的问题。
6. Observation / Control 增加真实 worker/resource admission。

## 第二阶段：生命周期闭环

1. detached Execution orphan policy / TTL / reconnect policy。
2. durable workflow cancel / clear / stale recovery。
3. pending_output / stderr buffer 有界化。
4. Session reaper 不回收仍有 active request 的 Session。
5. command poll 改为 event/adaptive polling。

## 第三阶段：删除炫技层

1. 把 500ms 最短可见时间移到 UI，删除后端 presentation lifecycle。
2. 删除或真正执行 ResourceLifecyclePolicy catalog。
3. 移除 `WorkflowDatum` 这种 Value 重实现，改为必要 typed checkpoint。
4. Diagnostics 直接消费 RequestRegistry lifecycle。
5. 删除 Production Adapter 的假成功默认实现。
6. 移除生产 startup 的真实 PowerShell semantic probe。
7. 收口 PolicyEnforcementRuntime 多套旧 constructor。
8. 缩短 Facade / Server，通过移动真实职责而不是增加 wrapper。

## 第四阶段：合同与 CI

1. schema44 从源码字符串扫描改为 executable invariant tests。
2. MCP public contract 建立单一 schema source。
3. 删除/归档与当前架构矛盾的历史测试和治理脚本。
4. 增加权限、多任务、disconnect/orphan、资源上限、卸载数据保留的端到端测试。

---

# J. 当前不可接受的“修复方式”

后续 AI 不得通过以下方式宣告修复：

- 再加一个缓存来同步两个 owner；
- 再加一个 Projection 来掩盖状态不一致；
- 再加一个 Mutex/RwLock 来维持平行 truth；
- 为每个调用者分别补同步代码；
- 仅修改 schema44 字符串检查让 CI PASS；
- 为了绕禁止 `serde_json::Value` 再造新的 JSON AST；
- 只把 orphan Execution 从 UI 隐藏，而不处理真实 owner/资源；
- 只把 500ms 调成别的数值，而不把展示职责移回 UI；
- 把未审核 elevated request 继续映射成 route unavailable；
- 通过降低/关闭测试解决失败；
- 用新的“大一统 Manager”包住所有旧状态但不删除旧 writer。

真正的修复标准是：**旧的平行事实、旁路和资源泄漏路径被删除，且黑盒行为与内部 invariant 同时成立。**
