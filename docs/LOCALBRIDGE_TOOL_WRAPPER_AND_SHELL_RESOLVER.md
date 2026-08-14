# LocalBridge Agent Runtime 最终设计合同

> 本文是 LocalBridge 后续 **Coding/Workspace Agent、Windows 系统维护、命令环境、权限边界与第三方 runtime 包装**的长期最终设计指导。只保留目标架构、稳定接口、安全合同、迁移顺序与验收标准；讨论稿、被否决方案和旧 duplicate design 不再构成事实源。
>
> 本文是未来能力设计基线，但任何实现仍必须服从 `START_HERE.md`、当前 8 份 numbered human authority、机器合同、`PR_INDEX.json` / `PROJECT_STATE.json` 的严格 PR 顺序与 writable paths。

---

## 1. 产品定位

LocalBridge 最终定义为：

```text
LocalBridge = Windows Local Agent Runtime
            + Stable LocalBridge Agent API
            + Coding/Workspace Capabilities
            + Windows System Maintenance Capabilities
            + Capability/Permission Enforcement
            + User-controlled Privileged Execution
            + Windows Runtime Integration
```

LocalBridge **不是** `coding-tools-mcp 0.2.2` 的 GUI、启动器或直接透传层。

`coding-tools-mcp 0.2.2` 当前只作为一个成熟、可替换的 **Coding/Workspace primitive runtime**。LocalBridge 对 ChatGPT 暴露自己的稳定 API；第三方 runtime 的 tool name、JSON Schema、private error、session id 或 future tool 都不得自动成为产品 API。

永久原则：

```text
冻结 LocalBridge Public API + Capability Contract
不冻结第三方 runtime 的工具名和内部实现
```

系统权限原则：

> **LocalBridge 可以代表用户执行系统级操作，但不能代表用户决定是否授予系统级权限。**

AI 可以判断“完成任务需要某项管理员操作”；是否授权由用户明确决定。AI/MCP 永远不能修改 LocalBridge control-plane 或自行提权。

---

## 2. 最终总体架构

```text
                              ChatGPT
                                 │ MCP
                                 ▼
┌────────────────────────────────────────────────────────────┐
│                  LocalBridge Agent API                     │
│                                                            │
│ workspace_context     agent_workflow                       │
│ exec_command          command_control                      │
│ task_control          git_workflow                         │
│ document_workflow     view_image                           │
│ system_inspect        system_manage      (target extension)│
└────────────────────────────┬───────────────────────────────┘
                             │
                      Tool Registry / Facade
                             │
                      Capability Classifier
                             │
                             ▼
┌────────────────────────────────────────────────────────────┐
│                    LocalBridge Core                        │
│ Workspace Guard        PEP                                 │
│ CurrentTask            Session Manager                     │
│ ShellResolver          Process Supervisor                  │
│ Credential Store       Runtime Orchestrator                │
│ Risk Classifier        User Authorization Coordinator      │
└──────────────┬──────────────────┬──────────────────────────┘
               │                  │
      ordinary capabilities   privileged capabilities
               │                  │
        ┌──────┴──────┐           ▼
        ▼             ▼    ┌──────────────────────────────┐
 WorkspaceRuntime   System  │ Privileged Broker           │
     Adapter        Runtime │ reviewed structured ops     │
        │             │     │ no unrestricted admin shell│
   ┌────┴────┐        │     └──────────────────────────────┘
   ▼         ▼        ▼
coding-   Native   Windows Native APIs /
tools-mcp Rust     reviewed system adapters
```

强制要求：

1. Tunnel 只连接 LocalBridge MCP/PEP，不能直接连接 upstream runtime。
2. LocalBridge 自己维护 public `tools/list`。
3. 每个公开 `tools/call` 都经过 LocalBridge capability 分类和 PEP；`tools/list` 过滤不是安全边界。
4. Workspace/Coding 普通执行经过 `WorkspaceRuntimeAdapter`。
5. Windows 系统只读和普通用户修改优先经过 `SystemRuntime`。
6. 真正需要管理员 token 的操作只能进入 `Privileged Broker`。
7. Broker 只接受 reviewed structured operation 或严格 reviewed structured program/args，不提供 unrestricted administrator shell。
8. UI/Domain/public MCP Schema 不依赖 upstream private structures。

---

## 3. Public API：当前基线与最终目标

### 3.1 当前 schema25 v1 core

当前已机器冻结的非特权 core Registry 为 8 个：

```text
workspace_context
agent_workflow
exec_command
command_control
task_control
git_workflow
document_workflow
view_image
```

当前 `elevated_exec` 仅是既有 Broker-governed conditional privileged extension，不属于普通八工具 core。

### 3.2 最终系统维护扩展

长期目标在上述稳定 core 上增加：

```text
system_inspect
system_manage
```

因此最终推荐 public surface 约 10 个高语义工具，而不是把几十个 primitive tools 暴露给模型。

系统维护扩展必须先有独立 machine contract / capability policy / adversarial tests，不能仅凭本文提前宣布当前 runtime 已实现。

---

## 4. `coding-tools-mcp 0.2.2` 的最终角色

内部可继续使用的 primitive 能力包括：

```text
server_info
check_exec_environment
get_default_cwd
set_default_cwd
read_file
list_dir
list_files
search_text
apply_patch
exec_command
write_stdin
kill_session
read_output
git_status
git_diff
git_log
git_show
git_blame
view_image
```

这些 primitive **保留能力、隐藏接口**。

禁止：

```text
ChatGPT
  ↓
LocalBridge
  ↓
upstream tools/list passthrough
  ↓
raw upstream tool call
```

目标：

```text
ChatGPT
  ↓
LocalBridge Tool Registry / Facade
  ↓
LocalBridge Capability + PEP
  ↓
Stable Runtime Adapter
  ↓
coding-tools-mcp primitives / Native implementation
```

### 4.1 Capability，不以 upstream tool name 为合同

示例：

```text
read_file      → file.read
list_files     → file.enumerate
search_text    → file.search
apply_patch    → file.patch
exec_command   → process.shell_exec
write_stdin    ┐
read_output    ├→ process.session_control
kill_session   ┘
git_*          → git.*
view_image     → image.inspect
```

upstream 升级允许 tool name/schema 变化，只要 adapter 仍能满足 LocalBridge capability contract。

### 4.2 Stable Runtime Adapter

逻辑接口应类似：

```rust
trait WorkspaceRuntime {
    inspect_workspace(...)
    read_file(...)
    enumerate_files(...)
    search_text(...)
    apply_patch(...)
    execute_process(...)
    execute_shell(...)
    control_session(...)
    git(...)
    inspect_image(...)
}
```

当前实现：`CodingToolsRuntimeAdapter`。未来可逐项替换成 `NativeRustRuntimeAdapter`，public API 不变。

### 4.3 Capability Negotiation

bundled runtime 启动时：

```text
initialize
→ upstream tools/list
→ structural/schema validation
→ capability mapping/probe
→ required LocalBridge capability baseline
```

缺失必需能力：

```text
RuntimeCapabilityMismatch
→ fail-closed
```

upstream 新增 tool/capability 默认 deny，不得自动扩大公开 API 或权限。

---

## 5. 高语义 Facade

### 5.1 `workspace_context`

一次聚合模型进入项目真正需要的上下文：

```text
workspace identity
project metadata
root summary
Git summary
Agent/Instructions
CurrentTask summary
command environment
default/available shells
LocalBridge capability summary
privilege state summary
```

其内部可组合多个 primitive，但返回必须是 LocalBridge 自有 typed schema。

当前 schema27 冻结最小稳定字段语义：存在 active workspace 时，`workspace` 是与 freshly validated filesystem identity 绑定的非空 ordinary Win32 绝对路径（例如 `D:\project`，禁止 `\\?\`）；`default_cwd` 是该 workspace 内的相对路径（`.` 或子路径）。上游 `get_default_cwd` 的字段名/shape 只是 adapter 私有细节，字段不兼容不能静默投影为空字符串。

### 5.2 `agent_workflow`

负责常见工程编排：

```text
diagnose
bugfix
feature
refactor
test_failure
build_release
document
resume
custom
```

`agent_workflow` **不是超级权限**。内部 read/search/patch/process/session/system/privileged 子动作必须分别经过 capability + PEP；Edit 模式不能借 workflow 间接 process exec，任何 privileged route 都不能借 workflow 绕过 Broker/user authorization。

### 5.3 `command_control`

对外统一：

```text
poll
read
write
kill
```

内部可映射 upstream `write_stdin/read_output/kill_session`，但 public session/output handle 必须由 LocalBridge 自己定义并通过 Session Manager 映射，禁止 raw upstream handle 穿透。

稳定 action 语义：

```text
poll(session_id)  → live session state / incremental output
write(session_id) → stdin
kill(session_id)  → terminate/cancel
read(output_ref)  → retained output pagination
```

poll 不得被实现成只接受 `output_ref` 的 retained-output read。Session Manager 必须主动观察/reap 进程生命周期并保存 terminal snapshot，使 session 在客户端不持续 poll 的情况下也能从 `running` 收敛到 `completed/failed/timed_out/cancelled/lost`。private session pruning、runtime restart 或 handle 丢失不能造成永久 Running；无法恢复 terminal outcome 时使用精确稳定错误 `SessionUnavailable`。

### 5.4 `git_workflow`

对外统一：

```text
status
diff
log
show
blame
```

底层可从 coding-tools adapter 逐步替换为 `git.exe` / git2-rs；workspace/subrepository/path 修正在 LocalBridge adapter 层完成。

schema27 要求五个 action 共享一个 repository resolver。对 active workspace 内 nested repo，status/diff/log/show 从请求目录向上寻找最近 repo root，blame 从请求文件 parent 向上寻找最近 repo root；搜索最多到 active workspace root。directory action 的 `path` 选择 repository context，`paths` 才是可选 path filter/pathspec。一个 repo 一旦被 resolver 确认，diff 不得 silent fallback 到 non-git diff。

### 5.5 `document_workflow`

高层支持至少：

```text
inspect
create
convert
rebuild
```

覆盖 PDF/DOCX/Markdown/TXT 等产品级文档工作流，避免模型临时拼接 Python/PowerShell/LibreOffice/pandoc。文件仍受 Workspace Guard / capability policy。

Public Registry 的 action 不能先广告再恒定返回 unavailable。当前冻结的 `inspect/create/convert/rebuild` 与 `agent_workflow` 九 action、`task_control get/cancel` 都必须在对应当前 v1 schema 对外提供时真实可执行；未来 action 必须先实现、分类、测试，再进入 public schema。

---

## 6. 命令执行模型

LocalBridge 内部必须区分：

```text
DirectProcessExecutor
ShellExecutor
```

### 6.1 Direct process

结构化：

```json
{
  "program": "git",
  "args": ["status"]
}
```

优点：无 shell expansion、quoting 更确定、参数可审计、容易 capability 分类。

典型：`git/node/npm/cargo/dotnet/java/python(受控 runtime)`。

### 6.2 Shell execution

只有真正需要 shell 语义时才使用，例如 PowerShell pipeline。

普通 `exec_command` 即使在管理员模式，也默认使用普通用户 token。禁止：

```text
管理员模式
→ exec_command 自动升级成 Administrator PowerShell
```

需要管理员权限的系统操作走 `system_manage → capability/risk → user authorization → Broker`。

---

## 7. ShellResolver / PowerShell 最终合同

### 7.1 默认策略

```text
DefaultShell = Auto

Auto:
1. 用户显式注册且重新验证的默认 Shell（未来扩展）
2. 当前机器已安装、可信候选中的最高兼容 PowerShell Core / pwsh.exe
3. Windows PowerShell 5.1 / powershell.exe
4. cmd.exe
5. typed NoShellAvailable
```

“最高版本”只指机器**已经安装**的版本；LocalBridge 不自动下载、安装或更新 PowerShell。

### 7.2 Trust before probe

PATH 只能用于发现候选，不能赋予信任。

候选必须先通过可信系统/安装位置或显式注册 executable identity 的重新验证，**之后**才允许执行 version probe。恶意 PATH 前置 `pwsh.exe` 在未建立信任前不能执行，甚至不能为了版本比较被 probe。

PowerShell 版本探测可使用等价结构化调用：

```text
pwsh.exe -NoLogo -NoProfile -NonInteractive -Command "$PSVersionTable.PSVersion.ToString()"
```

版本比较使用 semantic version，而不是字符串排序。

### 7.3 Public shell selector

当前只接受逻辑值：

```text
auto
powershell
pwsh
windows_powershell
cmd
```

禁止：

- 根据 command text 猜 Shell；
- MCP 直接传任意 executable path；
- 自动下载安装 Shell。

若未来支持 custom shell，必须先由 LocalBridge UI/控制面注册并验证，MCP 只能引用 `custom:<id>`，不能修改 registry。

---

## 8. CurrentTask / Process Lifecycle

所有公开工具都进入统一 execution envelope：

```text
LocalBridge tool invocation
→ TaskExecution::begin
→ capability classification
→ PEP
→ runtime/system/Broker route
→ TaskExecution::finish
```

Dashboard 只显示：

```text
Current Task
+
Single Last Tool Metadata
```

禁止 task history/feed/timeline/model thoughts/raw MCP tool id。

短调用必须 backend wake-driven：真实工具可 50ms 返回，但 UI presentation 至少可见 500ms；UI 最低可见期绝不能拖慢真实 MCP 返回。

长进程必须有 timeout/cancel/bounded output/session control，并受 Process Supervisor / Windows Job Object 管理；PID-only 不能作为最终 ownership。

普通命令结果按稳定进程终态解释：`exit_code == 0` 才是 `completed`；`exit_code != 0` 一律为 `failed` / `ProcessFailed`，不能因为 stdout/stderr 为空就包装成 `ok=true`。timeout/cancel 分别投影 timed-out/cancelled；CurrentTask 使用同一 terminal truth。

---

## 9. System Maintenance 最终能力域

`system_inspect` 与 `system_manage` 是 LocalBridge 长期一等能力，不依赖 coding-tools 是否有对应 tool。

### 9.1 `system_inspect`

优先结构化只读：

```text
system/process/service/network/disk/hardware
event_log/package/startup/driver/environment/windows_update
security_status (safe read-only)
```

普通用户可读信息不要为了“系统工具”概念而不必要 UAC。

实现优先：

```text
Windows Native API / controlled WMI/CIM equivalent
→ reviewed direct process
→ normal-user shell fallback
```

### 9.2 `system_manage`

推荐领域：

```text
service
package
registry
firewall
scheduled_task
startup
environment
windows_update
driver
power
filesystem_maintenance
network_configuration
```

系统修改优先使用结构化 request：

```text
target
action
resource identity
validated arguments
precondition
capability
risk class
```

核心系统维护不依赖 arbitrary PowerShell string。

### 9.3 Capability 应细分

禁止单一 `system.admin` 万能能力。示例：

```text
system.service.inspect
system.service.manage
system.registry.read
system.registry.write
system.firewall.inspect
system.firewall.manage
system.task.inspect
system.task.manage
system.driver.inspect
system.driver.manage
system.update.inspect
system.update.manage
system.power.manage
...
```

---

## 10. Risk Class

系统修改至少区分：

```text
Low
Medium
High
Critical
```

- **Low**：只读状态/诊断，通常不额外确认。
- **Medium**：可恢复的普通修改，如 reviewed service restart、用户级配置。
- **High**：HKLM、Firewall、系统计划任务、系统网络配置、系统级安装等；必须 structured review，按合同要求用户确认。
- **Critical**：驱动、引导配置、核心 ACL、安全策略、显著降低防护的操作；默认 deny，只有独立机器合同和 adversarial tests 后逐项开放，并要求逐操作确认。

风险等级由 LocalBridge policy/classifier 决定，不能信任模型自报风险。

---

## 11. 权限模型

### Edit

```text
workspace/file reviewed read/write    allow
system read-only reviewed inspect     allow as contracted
process exec                           deny
system mutation                        deny
privileged                             deny
control-plane                          deny_always
```

### Full

```text
Edit + reviewed current-user process/session/git/document
reviewed normal-user system_manage
privileged requires explicit Broker route
control-plane deny_always
```

### Elevated / 管理员模式

```text
Elevated = Full + Active Privileged Broker
```

它只表示用户已经激活一个可接受 reviewed privileged operation 的通道，**不表示 AI 获得 unrestricted Administrator**。

即使 Broker Active，仍然必须通过：

```text
capability classification
risk classification
structured request validation
policy review
per-operation confirmation when required
```

整个 LocalBridge、Tauri/WebView、Tunnel、coding runtime 和普通 `exec_command` 不整体提权。

---

## 12. Schema26 管理员模式安全确认合同

所有 Settings 与 onboarding Screen3 的 `管理员模式` 入口按钮统一使用**橙色 `#ff9500`**。该颜色只表示管理员权限警告；Starting 状态点、Dashboard `重启服务` 的既有 amber/yellow 语义不受影响。

当 Broker **未 Active**，用户点击/重新点击管理员模式不能直接请求 UAC，必须先显示以下固定内容，禁止增删改写：

```text
启用管理员权限后，错误或恶意操作可能导致：

* 删除或覆盖重要文件
* 修改系统关键配置
* 软件或系统无法正常启动
* 数据永久丢失
* 安全机制被绕过或关闭
* 凭据、密钥等敏感信息泄露
* 恶意程序获得更高权限
* 系统被破坏，严重时可能需要重装 Windows

仅在你明确理解操作后果时授权。

[取消] [确认9]
```

### 12.1 红色确认按钮

确认按钮的**整个按钮为红色**，不是仅倒计时数字红色。

固定状态：

```text
确认9 → 确认8 → 确认7 → 确认6 → 确认5
→ 确认4 → 确认3 → 确认2 → 确认1
→ 确认
```

前 9 个状态覆盖完整 **9000ms** 且始终 disabled。只有达到完整 9000ms 后，同一红色按钮才 enabled，标签精确为 `确认`。

### 12.2 倒计时不能只信任 frontend

推荐 backend/等价可信状态：

```text
AdminConsentChallenge
- challenge_id
- created_at_monotonic
- not_before = created_at_monotonic + 9000ms
- state = pending / cancelled / consumed
```

frontend interval/setTimeout 只负责显示 `确认9…确认1`，不是授权来源。backend 在处理 confirm intent 时重新检查：

```text
challenge 正确
未取消
未消费
当前 monotonic elapsed >= not_before
```

以下均不得绕过：

```text
pointer click
keyboard submit
synthetic/repeated click
rerender
focus change
stale frontend state
old challenge replay
```

### 12.3 UAC 顺序

```text
User selects 管理员模式
→ Broker Active? yes: no duplicate UAC merely due to reselection
→ no: create fresh challenge
→ show fixed warning
→ whole-red button disabled for full 9000ms
→ user clicks enabled 确认
→ backend Broker source/path/ACL/security validation
→ Windows runas / UAC
→ Broker handshake
→ Active
```

`取消`、Esc、close/dismiss：challenge 取消，无 PermissionMode/Broker/UAC 副作用。每次 fresh open 重计完整 9 秒；无 remember/skip/don't-show-again。

`--background` 恢复管理员偏好：不创建 warning challenge、不显示 dialog、不 UAC，只进入 Requested/等价未授权状态。

该 dialog 是窄安全 consent dialog，不允许 onboarding 本身重新变成“窗口里的居中 card/modal shell”。

High/Critical **per-operation confirmation** 与模式进入警告是不同 Gate；模式警告不能自动批准后续高风险具体操作。

AI/MCP 不能批准该 dialog、UAC、PermissionMode 或 Broker activation。

---

## 13. Privileged Broker / Operation Ticket

Privileged Broker 不提供 unrestricted shell。

普通优先级：

```text
1. Native structured Windows API
2. Reviewed direct process with structured argv
3. Normal-user shell fallback
4. Privileged Broker structured operation
```

管理员 operation 应使用 bounded authorization，例如：

```text
PrivilegedOperationTicket
- operation_id
- capability
- action
- target identity
- canonical arguments digest
- risk class
- authorization scope
- broker generation/session binding
- nonce/anti-replay
- one-shot/bounded use
```

Ticket 由 LocalBridge backend 在用户授权后生成，不能由 ChatGPT/MCP 伪造；默认一次性消费、防重放、不向 UI/Agent 暴露 raw secret payload。

Broker 执行 exact authorized structured operation，而不是“管理员已经开了，所以运行任意字符串”。

---

## 14. Control Plane 永久不可委托

MCP/AI 永远不得：

```text
新增/删除/授权 workspace
切换 active workspace control-plane
修改 PermissionMode
批准 schema26 warning
批准 UAC
直接启用/关闭 Broker
修改 Tunnel ID / Runtime API Key
注册/修改 Shell Registry
修改 LocalBridge policy/runtime authorization root
签发/伪造 privileged operation ticket
```

AI 可以请求一个**具体操作**，系统可以返回 `UserAuthorizationRequired` / `ElevationRequired`；但 AI 不能请求“把我升级为管理员”。

---

## 15. Stable Error Contract

upstream/System/Broker private errors 必须映射成 LocalBridge typed errors，例如：

```text
InvalidArgument
NotFound
WorkspaceDenied
WorkspaceChanged
WorkspaceIdentityMismatch
CapabilityDenied
UserAuthorizationRequired
ProcessFailed
ProcessTimedOut
ProcessCancelled
OutputTruncated
SessionUnavailable
RuntimeUnavailable
RuntimeProtocolMismatch
RuntimeCapabilityMismatch
ElevationRequired
BrokerInactive
PrivilegeDenied
OperationRiskDenied
OperationAuthorizationExpired
OperationAuthorizationReplayRejected
SystemResourceNotFound
SystemOperationFailed
SystemStateChanged
Internal
```

公开错误可包含 `retryable / safe user summary / safe diagnostic code / required_user_action`，但不得泄露 secret、raw Authorization、nonce、ticket payload 或 upstream private schema。

---

## 16. Workspace Path / Identity

必须区分：

```text
internal filesystem identity: \\?\D:\project
execution path:              D:\project
UI display path:             D:\project
```

`\\?\` 只允许用于 filesystem identity、reparse/junction/symlink 防护、去重和授权身份比较；不得进入 MCP/Broker/sidecar/process/tool 的 path/cwd/workdir/current_dir，也不得显示给 UI。

Public workspace-bound **输入**进一步统一为 active-workspace-relative：包括 `exec_command.workdir`、`git_workflow.path/paths`、`document_workflow.path`、`view_image.path` 等。drive-letter absolute、UNC absolute、verbatim、POSIX-leading-slash 和 `..` traversal 由 LocalBridge public boundary typed deny；不得把 upstream `ABSOLUTE_PATH_DENIED` 私有错误穿透，也不得通过 normalization 将任意绝对路径变成新授权。`workspace_context.workspace` 返回 ordinary absolute path 只是只读上下文投影，不改变这一输入合同。

system tools 作用于 workspace 外的 Windows 系统资源时必须走独立 system capability，workspace 授权不能自动扩大为系统级文件权限。

---

## 17. Secrets

以下不得进入 model/UI/normal logs/diagnostics：

```text
Runtime API Key plaintext
Authorization/bearer
Broker session nonce/secret
Privileged Operation Ticket raw payload
secret-bearing stdin/patch body summary
```

Runtime API Key 不进入 CLI、browser storage、plaintext settings。

---

## 18. 迁移顺序

不要 Big Bang rewrite。

### Phase 1 — Facade foundation

```text
LocalBridge Tool Registry
workspace_context
ShellResolver
command_control
LocalBridge typed errors
```

### Phase 2 — Stable runtime boundary

```text
WorkspaceRuntime Adapter
CodingToolsRuntimeAdapter
Capability Contract
Capability Negotiation
no upstream public passthrough
```

### Phase 3 — Workflow

```text
agent_workflow
task_control
CurrentTask execution envelope
transitive capability enforcement
```

### Phase 4 — Specialized coding workflows

```text
git_workflow
document_workflow
view_image stabilization
```

### Phase 5 — System inspect

优先 Low-risk structured read-only capability。

### Phase 6 — System manage / authorization

```text
system_manage
RiskClass
UserAuthorizationRequired
Authorization Coordinator
Privileged Operation Ticket
structured Broker operations
```

### Phase 7 — High/Critical system capability

逐项机器合同 + adversarial review 后加入 registry write / firewall / scheduled tasks / driver / network / power 等；Critical 默认 deny。

### Phase 8 — Native replacement

按收益逐步把 Shell/process、Git、filesystem、patch、image/document primitive 替换为 Native Rust 或专用 adapter。每替换一项只改变内部 backend，不改变 public API。

---

## 19. 明确禁止的退化方案

```text
直接暴露 coding-tools 20 个 primitive tools
直接转发 upstream tools/list/schema/error
AI 自己 request_permissions 获得管理员权限
管理员模式 = entire LocalBridge elevated
管理员模式 = unrestricted Administrator PowerShell
管理员模式使普通 exec_command 自动提权
Broker Active 绕过 capability/risk policy
前端倒计时自己决定 UAC eligibility
黄色/琥珀管理员按钮恢复为当前 UI 语义
点击管理员模式直接 UAC 而跳过 schema26 warning
MCP 指定任意 shell executable
根据命令文本猜 shell
自动下载安装 PowerShell
一次性重写整个 coding-tools runtime
一次性开放全部 Critical system capabilities
```

---

## 20. 最终验收标准

### Public API

- public `tools/list` 只来自 LocalBridge Tool Registry。
- upstream private tool name/schema/error 不穿透。
- public actions 以 stable LocalBridge capability 分类。
- public schema 不广告恒定 unavailable 的 action；当前冻结 action 必须真实可执行。
- public session/output handles 由 LocalBridge Session Manager 拥有；上游 session/output handles 不穿透。
- `workspace_context.workspace` 非空且为 ordinary active-workspace absolute path；`default_cwd` 独立为 workspace-relative。
- workspace-bound public path inputs 统一 relative-only；absolute/traversal typed deny。
- nested Git repo 在五个 git_workflow action 中使用同一 resolver；已发现 repo 时 diff 不得 non-git fallback。
- silent nonzero exit 必须 `ProcessFailed`，不能 public success。

### Shell

- `shell=auto` 只选择可信已安装候选。
- highest compatible installed PowerShell Core → Windows PowerShell 5.1 → cmd。
- PATH 不是 trust authority；probe 前先验证 identity。
- no arbitrary shell path / no text guessing / no auto-install。
- direct process 与 shell structurally separate。

### Workflow

- workflow 原子动作逐项 PEP。
- Edit 无 process exec bypass。
- privileged/system mutation 无 indirect bypass。

### System maintenance

- `system_inspect` 普通可读能力不要求无意义 UAC。
- `system_manage` 结构化，capability + risk first。
- 需要管理员 token 只路由 Broker。
- High/Critical 按独立合同逐操作确认。
- unrestricted administrator shell 永久禁止。

### Administrator mode UX/security

- Settings/onboarding admin entry = orange `#ff9500`。
- 固定八条风险 warning 文案无漂移。
- confirmation entire button = red。
- full 9000ms `确认9…确认1` disabled。
- trustworthy monotonic/backend not-before；frontend-only timer 不能授权。
- after 9000ms red enabled label exact `确认`。
- UAC only after enabled explicit confirm + backend security validation。
- cancel/Esc/close no side effects；fresh open resets 9s；no skip/remember。
- background no warning/UAC；Active reselection no duplicate UAC。
- AI/MCP cannot approve warning/UAC/control-plane。
- High/Critical confirmation remains separate。

### Process / Task / Workspace / Secrets

- process ownership uses Job Object/equivalent, not PID-only。
- CurrentTask is backend-grounded and wake-driven；short tool presentation ≥500ms without delaying response。
- only one active authorized workspace；identity/execution/display paths separated。
- no secret plaintext in CLI/settings/browser/log/task/diagnostics。

---

## 21. 最终技术决策

```text
ChatGPT
  ↓
LocalBridge stable high-level Agent API
  ↓
Tool Registry / Facade
  ↓
Capability Classification
  ↓
PEP + Risk Classification
  ↓
┌───────────────────────┬────────────────────────┐
│ ordinary route        │ privileged route       │
│ Workspace/System      │ explicit user consent  │
│ Runtime               │ + UAC                  │
│                       │ + bounded Broker op    │
└───────────────────────┴────────────────────────┘
  ↓
coding-tools-mcp / Native Rust / Windows Native APIs
```

最终目标：

> **LocalBridge 保留成熟 Coding primitive runtime 的执行效率，但拥有自己的稳定 Agent API、Windows 系统维护能力、安全边界、可信 Shell 环境、用户手动授权模型和可替换执行内核；第三方 runtime 只作为内部实现细节存在。**

管理员能力最终合同：

> **AI 可以提出并执行经过结构化、分类和审查的管理员系统操作；是否授予管理员执行权始终由用户明确决定。进入管理员模式必须先通过橙色入口触发的固定风险警告与 whole-red 9 秒确认门，再由用户显式确认后请求 Windows UAC；管理员模式只激活受控 Broker，不授予 unrestricted Administrator shell，AI 永远不能修改 LocalBridge 自身控制面或自行提权。**
