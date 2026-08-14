# LocalBridge Agent Runtime 最终设计指导

> 本文是 LocalBridge 后续 Agent 工具能力开发的最终设计指导。仅保留目标架构、稳定接口、安全边界、Shell/PowerShell 方案、上游 runtime 包装方式、迁移顺序与验收标准；不保留方案比较、讨论过程或临时设计。

---

## 1. 产品定位

LocalBridge 的目标不是成为 `coding-tools-mcp 0.2.2` 的 GUI、启动器或工具透传层，而应定义为：

```text
LocalBridge = Windows Local Agent Runtime
            + Stable LocalBridge Agent API
            + Capability/Permission Enforcement
            + Windows Runtime Integration
```

`coding-tools-mcp 0.2.2` 在当前阶段仅作为一个**内部可替换执行后端**存在。

LocalBridge 对 ChatGPT 暴露自己的稳定 MCP 工具协议；ChatGPT 不依赖、也不需要知道底层当前使用 `coding-tools-mcp`、Native Rust 或其他 runtime。

核心原则：

```text
冻结 LocalBridge Public API + Capability Contract
不冻结第三方 runtime 的工具名、JSON Schema 或内部实现
```

---

## 2. 最终总体架构

```text
                           ChatGPT
                              │
                              │ MCP
                              ▼
┌─────────────────────────────────────────────────────┐
│              LocalBridge Agent API                  │
│                                                     │
│ workspace_context     agent_workflow                │
│ exec_command          command_control               │
│ task_control          git_workflow                  │
│ document_workflow     view_image                    │
└──────────────────────────┬──────────────────────────┘
                           │
                    Tool Intent Parser
                           │
                    Capability Classifier
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│                 LocalBridge Core                    │
│                                                     │
│ Workspace Guard      Capability / PEP               │
│ CurrentTask          Session Manager                │
│ Shell Resolver       Process Supervisor             │
│ Credential Store     Runtime Orchestrator           │
└─────────────┬─────────────────────────────┬─────────┘
              │                             │
      ordinary capability             privileged capability
              │                             │
              ▼                             ▼
┌─────────────────────────────┐   ┌─────────────────────────┐
│ Workspace Runtime Adapter   │   │ Privileged Broker       │
│                             │   │                         │
│ coding-tools adapter        │   │ UAC                     │
│ native Rust adapter         │   │ reviewed operations     │
│ future adapters             │   │ no arbitrary admin shell│
└──────────────┬──────────────┘   └─────────────────────────┘
               │
       ┌───────┴─────────┐
       ▼                 ▼
coding-tools-mcp     Native Rust
0.2.2 primitives    implementations
```

设计要求：

1. Tunnel 只能连接 LocalBridge MCP/PEP，不允许直接连接上游 runtime。
2. LocalBridge 自己维护公开 `tools/list`。
3. 所有公开工具调用都必须经过 capability 分类和 PEP。
4. 普通执行走 `WorkspaceRuntimeAdapter`。
5. 管理员能力只能走独立 `Privileged Broker`。
6. UI、Domain、公开 MCP Schema 不依赖第三方 runtime 私有结构。

---

## 3. LocalBridge Agent API v1

公开工具数量应保持少而稳定，推荐固定为 8 个：

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

原则：

- 公开工具提供高语义密度能力。
- 原子工具继续存在于内部，但不直接暴露给 ChatGPT。
- 工具名和 Schema 由 LocalBridge 自己版本化。
- PermissionMode 切换不应导致整套 API 形态剧烈变化；权限差异主要由 `tools/call` 强制执行。

---

## 4. `workspace_context`

### 4.1 目标

一次返回进入项目所需的核心上下文，减少模型反复调用低层工具。

推荐聚合：

```text
workspace identity
project metadata
root entries summary
Git summary
Agent/Instructions
CurrentTask summary
command environment
resolved default shell
available shells
LocalBridge capabilities
```

概念输出：

```json
{
  "workspace": "D:\\project\\LocalBridge",
  "project": {
    "name": "localbridge",
    "type": "node",
    "version": "0.1.0"
  },
  "git": {
    "branch": "main",
    "changed_count": 4
  },
  "task": {
    "status": "idle"
  },
  "command_environment": {
    "default_shell": {
      "kind": "powershell",
      "version": "7.x.x"
    },
    "available_shells": [
      "powershell",
      "windows_powershell",
      "cmd"
    ]
  }
}
```

### 4.2 内部实现

当前可组合：

```text
server_info
get_default_cwd
list_dir
git_status
CurrentTaskState
ShellResolver
ProjectContextLoader
```

返回结果必须转换为 LocalBridge 自有 Schema，不得直接透传上游响应。

---

## 5. `agent_workflow`

### 5.1 定位

`agent_workflow` 是 LocalBridge 的核心高层编排工具，用于减少 MCP round-trip，并将常见开发流程变成一个稳定工作流。

推荐工作流类型：

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

推荐阶段：

```text
prepare
execute
run
resume
```

概念调用：

```json
{
  "workflow": "bugfix",
  "phase": "run",
  "objective": "修复启动时继续使用旧连接配置的问题",
  "queries": ["startup", "connection config"],
  "verification": "all"
}
```

### 5.2 内部流程

```text
agent_workflow
      │
      ▼
Workspace Context
      │
      ▼
读取项目 Agent/Instruction
      │
      ▼
Search / Read
      │
      ▼
Change Plan
      │
      ▼
逐原子动作 Capability Classification + PEP
      │
      ▼
Patch
      │
      ▼
Command / Tests / Build
      │
      ▼
Git Diff / Verification
      │
      ▼
LocalBridge typed result
```

### 5.3 安全规则

`agent_workflow` 不是超级权限工具。

它内部执行的每一个原子动作都必须重新分类和授权：

```text
read/search      → capability check
patch            → capability check
process exec     → capability check
session control  → capability check
privileged op    → Broker route only
```

因此编辑模式下，即使 `agent_workflow` 可见，也不得通过 workflow 间接执行命令。

---

## 6. `exec_command` 与命令执行模型

LocalBridge 内部应区分两类执行：

```text
ProcessExecutor
ShellExecutor
```

### 6.1 直接进程执行

概念形式：

```json
{
  "program": "git",
  "args": ["status"]
}
```

直接通过 Windows CreateProcess / Rust process API 启动，不经过 Shell。

适用：

```text
git
node
npm
cargo
dotnet
java
python（若属于明确受控 runtime）
```

优势：

- 参数边界明确。
- 无 Shell expansion。
- quoting 更稳定。
- 更容易 capability 分类。
- 更容易做审计和安全策略。

`exec_process` 可以仅作为内部 primitive，不必暴露为公开 MCP 工具。

### 6.2 Shell 命令执行

只有需要 Shell 语义时才使用 `exec_command`，例如：

```powershell
Get-Process | Sort-Object CPU
```

此时通过 `ShellResolver` 选中的 Shell 执行。

---

## 7. 默认 Shell / PowerShell 最终方案

### 7.1 默认策略

```text
DefaultShell = Auto

Auto resolution:
1. 用户显式配置且已注册、已验证的 Shell
2. 当前机器已安装的最高版本 PowerShell Core / pwsh.exe
3. Windows PowerShell 5.1 / powershell.exe
4. cmd.exe
5. 均不可用则返回 typed NoShellAvailable
```

“最高版本 PowerShell”只指**当前机器已经安装版本中的最高版本**。

LocalBridge 不自动下载安装或升级 PowerShell。

### 7.2 ShellResolver 架构

```text
CommandExecutor
      │
      ▼
ShellResolver
      │
 ┌────┼──────────┬──────────┐
 ▼    ▼          ▼          ▼
pwsh powershell  cmd       custom
```

推荐数据结构：

```rust
struct ResolvedShell {
    kind: ShellKind,
    executable: PathBuf,
    version: Option<Version>,
    source: ShellSource,
}
```

`ShellResolver` 只负责：

- 枚举候选 Shell。
- 验证 executable。
- 获取版本。
- 比较版本。
- 选择默认 Shell。
- 将逻辑 Shell ID 映射到已验证 executable。

它不负责 PEP、UAC、Workspace 授权或进程所有权。

### 7.3 PowerShell 探测

Windows 至少探测：

```text
PATH 中的 pwsh.exe
C:\Program Files\PowerShell\*\pwsh.exe
PATH 中的 powershell.exe
Windows PowerShell 系统路径
cmd.exe
```

不要只依赖 `where pwsh`，因为机器可能存在多个 PowerShell Core 版本。

对候选 `pwsh.exe` 运行等价命令：

```text
pwsh.exe -NoLogo -NoProfile -NonInteractive -Command "$PSVersionTable.PSVersion.ToString()"
```

然后进行语义版本比较，选择最高版本。

### 7.4 缓存与重新探测

可缓存探测结果，但以下情况必须重新检测：

- LocalBridge 重启。
- 用户点击重新检测命令环境。
- 用户修改默认 Shell。
- 缓存 executable 已不存在。

### 7.5 `exec_command` 默认行为

```json
{
  "cmd": "Get-Process"
}
```

等价于：

```text
shell = auto
```

`auto` 的唯一语义：

> 使用 LocalBridge 当前已经解析出的默认 Shell。

禁止根据命令文本猜测 Shell。

### 7.6 显式 Shell 选择

公开枚举：

```text
auto
powershell
windows_powershell
cmd
```

未来可扩展：

```text
bash
wsl
nushell
custom:<registered-id>
```

Windows 默认顺序始终保持：

```text
PowerShell Core → Windows PowerShell 5.1 → cmd.exe
```

### 7.7 禁止任意 executable 注入

禁止 MCP 直接传入：

```json
{
  "shell": "C:\\arbitrary\\something.exe"
}
```

MCP 只允许引用逻辑 Shell ID。

如果支持自定义 Shell：

```text
用户 UI
   ↓
Shell Registry
   ↓
路径/executable identity 验证
   ↓
注册 Shell ID
```

MCP 只能使用已注册 ID，不得创建、修改或删除 Shell Registry 项。

---

## 8. `command_control`

公开工具：

```text
command_control
```

推荐动作：

```text
poll
write
read
kill
```

概念输入：

```json
{
  "action": "poll",
  "session_id": "..."
}
```

当前内部可映射：

```text
poll  → session state / upstream poll primitive
write → write_stdin
read  → read_output
kill  → kill_session
```

ChatGPT 不再需要感知多个底层 session 工具。

Session ID 由 LocalBridge 管理并形成稳定类型，不应直接依赖上游 session identifier 的格式。

---

## 9. `task_control`

任务系统分成两层。

### 9.1 Agent 内部任务

可以支持：

```text
get
start
update
pause
resume
stop
operation
```

用于长工作流恢复、执行操作状态和 Agent continuity。

### 9.2 Dashboard 用户界面

继续遵守 LocalBridge 极简状态合同，只展示：

```text
Current Task
+
Single Last Tool Metadata
```

示例：

```text
● 运行测试  cargo test · 8.2s

上次执行工具：运行测试                 12S前
```

Idle：

```text
○ 等待命令

上次执行工具：修改文件                 3分钟前
```

禁止用户界面出现：

```text
任务历史
activity feed
timeline
model thoughts
raw MCP tool id
```

内部 task continuity 可以存在，但不得直接变成 Dashboard 历史 UI。

---

## 10. CurrentTask Execution Envelope

所有公开工具调用都应进入统一执行包络：

```text
LocalBridge tool invocation
       │
       ▼
TaskExecution::begin()
       │
       ▼
Capability Classification
       │
       ▼
PEP
       │
   allow / deny
       │
       ▼
Runtime Adapter / Broker
       │
       ▼
TaskExecution::finish()
```

收益：

- CurrentTask truth 始终由 LocalBridge backend 维护。
- 不依赖上游 runtime 是否提供 trace。
- deny 可直接投影为 Blocked，而不是先伪装 Running。
- 所有工具共享一致的 timing、redaction 和 error semantics。

### 10.1 短工具调用

真实工具执行与 UI 可见期解耦：

```text
真实调用：50ms 完成并返回 ChatGPT
UI presentation：至少保持 500ms 可见
```

实现要求：

```text
MCP/Broker execution event
↓
backend wakeup/push
↓
UI typed projection
```

不得依赖周期 polling 捕获短任务。

UI 为满足 500ms 可见期延迟自身状态清理即可，不得延迟真实 MCP 响应。

---

## 11. `git_workflow`

公开工具：

```text
git_workflow
```

推荐动作：

```text
status
diff
log
show
blame
```

当前可映射：

```text
status → git_status
diff   → git_diff
log    → git_log
show   → git_show
blame  → git_blame
```

未来可替换为：

```text
git.exe adapter
git2-rs/libgit2 adapter
```

公开 API 不变。

LocalBridge 应在自己的 adapter 层负责 workspace/sub-repository/path normalization 等修正，而不是让上游 Git 行为直接成为产品合同。

---

## 12. `document_workflow`

公开工具：

```text
document_workflow
```

推荐动作：

```text
inspect
create
convert
rebuild
```

目标格式至少覆盖：

```text
PDF
DOCX
Markdown
TXT
```

原则：

- 文档操作是高层产品能力。
- 不要求 Agent 临时组合 Python、PowerShell、LibreOffice、pandoc 等外部链路。
- 底层实现可以替换，但公开 Schema 不变。
- 文件读写仍受 Workspace Guard 和 capability policy 约束。

---

## 13. `view_image`

保持单一高层图片读取能力：

```text
view_image
```

应由 LocalBridge 校验：

- workspace authorization。
- 路径 identity。
- 文件大小上限。
- 支持格式。
- 必要的缩放/输出限制。

底层可以暂时使用上游实现，也可后续迁移到 Native Rust。

---

## 14. `coding-tools-mcp 0.2.2` 的最终角色

当前上游约 20 个原子工具继续作为内部 runtime 使用，例如：

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

这些工具的问题不是能力不足，而是抽象层级偏低，不应成为 LocalBridge 的稳定公开产品 API。

禁止结构：

```text
ChatGPT
   ↓
coding-tools tools/list
   ↓
直接 tools/call
```

目标结构：

```text
ChatGPT
   ↓
LocalBridge MCP Server
   ↓
LocalBridge Tool Registry / Facade
   ↓
PEP
   ↓
Stable Runtime Adapter
   ↓
coding-tools-mcp primitives
```

---

## 15. Stable Runtime Adapter

LocalBridge Facade 不应直接依赖上游工具 JSON Schema。

推荐抽象：

```rust
#[async_trait]
pub trait WorkspaceRuntime {
    async fn inspect_workspace(...);
    async fn read_file(...);
    async fn enumerate_files(...);
    async fn search_text(...);
    async fn apply_patch(...);

    async fn execute_process(...);
    async fn execute_shell(...);
    async fn control_session(...);

    async fn git(...);
    async fn inspect_image(...);
}
```

当前实现：

```text
CodingToolsRuntimeAdapter
```

未来实现：

```text
NativeRustRuntimeAdapter
```

潜在扩展：

```text
WSLRuntimeAdapter
ContainerRuntimeAdapter
RemoteRuntimeAdapter
```

Facade 永远只处理 LocalBridge 自有 request/result/error 类型。

---

## 16. Capability Contract

LocalBridge 的永久兼容基线必须以 capability 为核心，而不是上游工具名称。

建议至少冻结：

```text
workspace.inspect

file.read
file.enumerate
file.search
file.patch

process.exec
process.shell_exec
process.session_control

git.status
git.diff
git.history
git.inspect

image.inspect

document.inspect
document.create
```

当前 `coding-tools-mcp 0.2.2` 可映射：

```text
read_file       → file.read
list_files      → file.enumerate
search_text     → file.search
apply_patch     → file.patch
exec_command    → process.shell_exec
write_stdin     → process.session_control
kill_session    → process.session_control
read_output     → process.session_control
git_status      → git.status
git_diff        → git.diff
git_log         → git.history
git_show/blame  → git.inspect
view_image      → image.inspect
```

当上游版本升级时，允许工具名或 Schema 改变，只要 Adapter 能继续满足 LocalBridge capability contract。

---

## 17. Runtime Capability Negotiation

启动 bundled coding runtime 后执行：

```text
initialize
↓
tools/list
↓
Schema/structural validation
↓
Adapter capability probe
↓
与 LocalBridge required capability baseline 比较
```

缺少必需能力时：

```text
RuntimeCapabilityMismatch
```

必须 fail-closed，不得等到业务调用时随机出现 unknown tool。

上游出现新工具或新 capability：

```text
默认 deny
↓
compatibility review
↓
capability map 更新
↓
LocalBridge release
```

不得运行时自动扩大权限。

---

## 18. LocalBridge 自有 `tools/list`

外部调用：

```text
ChatGPT tools/list
      │
      ▼
LocalBridge TOOL_REGISTRY
```

禁止：

```text
ChatGPT tools/list
      │
      ▼
coding-tools-mcp tools/list
```

上游 `tools/list` 仅用于：

- 启动时 compatibility check。
- capability negotiation。
- upgrade structural/capability diff。
- 内部 diagnostics。

公开工具目录始终由 LocalBridge 自己维护。

---

## 19. Error Adapter

禁止把第三方错误对象直接返回 ChatGPT。

路径：

```text
UpstreamError
      ↓
RuntimeAdapter
      ↓
LocalBridgeToolError
```

推荐统一错误分类：

```text
InvalidArgument
NotFound

WorkspaceDenied
WorkspaceChanged
WorkspaceIdentityMismatch

CapabilityDenied

ProcessFailed
ProcessTimedOut
ProcessCancelled
OutputTruncated

RuntimeUnavailable
RuntimeProtocolMismatch
RuntimeCapabilityMismatch

ElevationRequired
PrivilegeDenied

Internal
```

错误类型应稳定、可版本化，并尽可能包含：

```text
retryable
safe user summary
safe diagnostic code
```

不得透传 secret、raw Authorization、nonce 或上游敏感 payload。

---

## 20. 权限模型

保持 LocalBridge 的三模式模型：

```text
编辑模式
完整模式
管理员模式
```

### 20.1 编辑模式

```text
workspace.inspect      allow
file.read              allow
file.enumerate         allow
file.search            allow
file.patch             allow

process.exec            deny
process.shell_exec      deny
process.session_control deny

privileged.*            deny
control-plane           deny_always
```

### 20.2 完整模式

```text
workspace/file/git/image/document coding capabilities
    allow / reviewed allow

process.exec
process.shell_exec
process.session_control
    allow_if_reviewed

privileged.*
    deny

control-plane
    deny_always
```

### 20.3 管理员模式

管理员模式定义为：

```text
Elevated = Full + Active Privileged Broker
```

绝不是：

```text
整个 LocalBridge 以 Administrator 启动
```

普通 MCP capability ceiling 与 Full 保持一致；需要管理员权限的操作单独路由到 Broker。

---

## 21. PEP 位置

强制调用链：

```text
ChatGPT
   │
   ▼
LocalBridge Tool
   │
   ▼
Intent / Capability Classification
   │
   ▼
PEP
   │
   ├── deny → LocalBridge typed denial
   │
   ▼
Runtime Adapter / Broker
```

禁止事后授权：

```text
先执行上游工具
↓
再判断是否允许
```

`tools/list` 过滤只属于 UX，永远不是安全边界。

每一个 `tools/call` 都必须重新授权，防止客户端缓存旧工具目录后绕过模式切换。

---

## 22. Control Plane 永久不可委托

以下能力不得成为可由 MCP 修改的 LocalBridge control plane：

```text
选择/新增/移除授权项目
切换 active workspace
修改 PermissionMode
触发或批准 UAC
修改 Tunnel ID
修改 Runtime API Key
注册/修改 Shell
修改 LocalBridge policy
修改 runtime 配置或授权根
```

`agent_workflow` 等高层工具也不得通过间接调用绕过此规则。

```text
control-plane = deny_always
```

---

## 23. 管理员能力与 Privileged Broker

管理员执行必须完全绕开普通 Workspace Runtime Adapter：

```text
                    PEP
                     │
          ┌──────────┴──────────┐
          │                     │
     normal capability      privileged
          │                     │
          ▼                     ▼
 WorkspaceRuntime       Privileged Broker
          │                     │
 coding-tools/native           UAC
```

`elevated_exec` 必须保持：

```text
structured program
structured args
reviewed executable
reviewed workdir
timeout
cancellation
output limit
redaction
Broker Active required
```

禁止：

```text
arbitrary elevated program
arbitrary elevated shell
powershell -Command <arbitrary string> as generic admin gateway
control-plane mutation
```

MCP 无权自行提升权限。

管理员模式的激活仍必须来自用户明确 UI 操作和 Windows UAC。

---

## 24. Workspace Path / Identity 边界

LocalBridge 必须区分：

```text
filesystem identity path
execution path
display path
```

例如：

```text
内部 identity validation:
\\?\D:\project

实际 command/MCP/process execution:
D:\project

UI display:
D:\project
```

`\\?\` 形式仅允许用于：

- filesystem identity validation。
- reparse/junction/symlink 防护。
- de-dup。
- 授权身份比较。

禁止把 verbatim path 传入：

```text
MCP tool args
cwd/workdir/current_dir
Broker
sidecar process
command execution
UI
```

普通 execution path 必须重新绑定并验证为同一个已授权 filesystem identity；路径格式转换本身不得扩大授权。

---

## 25. Process 与生命周期

所有由 LocalBridge 启动的普通 runtime/command child process 应纳入既有 Process Supervisor / Windows Job Object 所有权模型。

不得以 PID-only 作为最终所有权判断。

公开工具只返回稳定 session/task ID，不直接暴露内部 PID ownership 细节。

长任务应具备：

```text
timeout
cancellation
bounded output
session read/poll/write/kill
child-tree ownership
cleanup on LocalBridge stop/crash policy
```

---

## 26. Secrets

以下规则对所有新工具和 workflow 同样生效：

```text
Runtime API Key 不进入 settings plaintext
Runtime API Key 不进入 browser storage
Runtime API Key 不进入 process CLI
Authorization/bearer 不进入用户日志
Broker nonce/session secret 不进入任务摘要
stdin/patch body 默认不进入 CurrentTask summary
```

所有公开结果、错误、CurrentTask、diagnostics 均先经过 redaction。

---

## 27. 推荐模块边界

逻辑模块建议：

```text
src-tauri/src/mcp/
├── server.rs
├── registry.rs
├── facade/
│   ├── workspace_context.rs
│   ├── agent_workflow.rs
│   ├── exec_command.rs
│   ├── command_control.rs
│   ├── task_control.rs
│   ├── git_workflow.rs
│   ├── document_workflow.rs
│   └── view_image.rs
│
├── capability/
│   ├── classification.rs
│   ├── policy.rs
│   └── negotiation.rs
│
├── adapter/
│   ├── mod.rs
│   ├── workspace_runtime.rs
│   ├── coding_tools.rs
│   └── native.rs
│
├── execution/
│   ├── envelope.rs
│   ├── task.rs
│   └── session.rs
│
└── errors.rs
```

命令层：

```text
src-tauri/src/command/
├── shell_resolver.rs
├── shell_registry.rs
├── environment.rs
├── executor.rs
├── shell_executor.rs
└── process_executor.rs
```

实际磁盘目录需服从项目现有 PR/architecture contract；这里表示最终逻辑边界，不授权越过当前 PR writable paths。

---

## 28. 迁移顺序

不应一次重写所有 `coding-tools-mcp` 原子能力。

### Phase 1 — LocalBridge Facade 基础

实现：

```text
LocalBridge Tool Registry
workspace_context
ShellResolver
command_control
LocalBridge typed errors
```

底层继续使用现有 coding-tools runtime。

### Phase 2 — Stable Runtime Adapter

实现：

```text
WorkspaceRuntime trait
CodingToolsRuntimeAdapter
Capability Contract
Capability Negotiation
禁止公开透传 upstream tools/list
```

### Phase 3 — Agent Workflow

实现：

```text
agent_workflow
task_control
CurrentTask Execution Envelope
workflow transitive capability classification
```

这是 Agent 使用效率提升最大的阶段。

### Phase 4 — Specialized Workflows

实现：

```text
git_workflow
document_workflow
view_image adapter stabilization
```

### Phase 5 — Native Replacement

按收益逐步替换：

```text
Shell/process execution → Native Rust
Git                     → git.exe / git2-rs
File read/list/search   → Native Rust
Patch                   → Native Rust
Image/document          → 视维护收益决定
```

每替换一项只更换 adapter backend，不改变 LocalBridge Public API。

最终可以完全移除 `coding-tools-mcp`，但这不是近期前置条件。

---

## 29. 不做的事情

本设计明确不采用：

```text
直接把 coding-tools-mcp 20 个工具全部暴露给 ChatGPT
直接把 upstream tools/list 作为 LocalBridge tools/list
根据命令文本猜测 Bash/PowerShell/cmd
允许 MCP 传任意 Shell executable
允许 AI 修改 LocalBridge control-plane
允许 agent_workflow 绕过 PEP
允许管理员能力进入普通 runtime adapter
一次性重写所有 coding-tools 原子实现
使用工具数量作为主要能力指标
```

---

## 30. Capability Parity 验收矩阵

LocalBridge 的目标指标应是能力覆盖，而不是工具名覆盖。

| Capability | LocalBridge 目标 |
|---|---|
| Workspace inspect | 必须 |
| File read | 必须 |
| File enumerate | 必须 |
| Search | 必须 |
| Atomic patch | 必须 |
| Command execution | 必须 |
| Long command session | 必须 |
| Git | 必须 |
| Image inspect | 必须 |
| Agent workflow | 必须 |
| Task resume/control | 必须 |
| Document workflow | 必须 |
| Shell discovery | 必须 |
| Highest installed PowerShell selection | 必须 |
| Windows Job Object ownership | 必须 |
| Secure Credential Store | 必须 |
| Workspace filesystem identity confinement | 必须 |
| Edit/Full/Elevated PEP | 必须 |
| UAC Privileged Broker | 必须 |
| Tunnel/runtime lifecycle | 必须 |
| Typed CurrentTask projection | 必须 |
| Short-call wake-driven UI projection | 必须 |
| Stable runtime adapter | 必须 |
| Runtime capability negotiation | 必须 |
| Upstream replacement without public API change | 必须 |

---

## 31. 最终验收标准

### Public API

- ChatGPT 只看到 LocalBridge 自有工具。
- 上游 primitive tool 名称不出现在公开 `tools/list`。
- Public API 具备明确 schema version / compatibility policy。

### Runtime Adapter

- Domain/UI 不依赖上游私有结构。
- coding-tools 升级只影响 adapter/compatibility 层。
- 必需 capability 缺失时启动 fail-closed。

### Shell

- `exec_command` 默认 `shell=auto`。
- Auto 选择机器已安装的最高 PowerShell Core。
- 无 PowerShell Core 时 fallback Windows PowerShell 5.1，再 fallback cmd。
- MCP 不能传任意 Shell executable。
- `workspace_context` 返回已解析命令环境。

### Workflow

- `agent_workflow` 可完成 diagnose/bugfix/feature/refactor/test/build 等常见任务。
- workflow 每个原子动作独立经过 PEP。
- Edit 模式无法通过 workflow 间接 process exec。

### Permissions

- unknown capability 默认 deny。
- control-plane 永久 deny。
- Full 只运行 reviewed current-user capability。
- Elevated 只通过 Broker 增加 reviewed privileged capability。
- 整个 LocalBridge 永不因管理员模式整体提权。

### Process / Task

- 普通进程受 Process Supervisor / Job Object 管理。
- 长命令具备 timeout/cancel/output/session control。
- CurrentTask 来自真实 MCP/Broker execution event。
- 短工具调用由 backend wake-driven delivery 捕获。
- 500ms UI 可见期不延迟真实工具响应。

### Workspace

- 同时最多一个 active authorized root。
- remembered project 不等于授权根。
- identity path 与 execution/display path 分离。
- `\\?\` 路径不得进入实际 tool/command/process workdir。

### Security

- Runtime API Key 等 secret 不进入 plaintext persistence、CLI、CurrentTask、normal logs 或 diagnostics。
- 管理员操作结构化、reviewed、Broker-only。
- MCP 不得修改 PermissionMode、workspace registry、credentials、Tunnel 配置或 Shell Registry。

---

## 32. 最终技术决策

LocalBridge 后续所有工具能力开发应遵守以下固定方向：

```text
LocalBridge Agent API
        ↓
LocalBridge Facade
        ↓
Capability Classification
        ↓
PEP
        ↓
TaskExecution Envelope
        ↓
Runtime Adapter / Privileged Broker
        ↓
coding-tools-mcp / Native Rust
```

近期最优先实现顺序：

```text
1. LocalBridge Tool Registry + Stable Runtime Adapter
2. workspace_context
3. ShellResolver + exec_command / command_control
4. Capability Negotiation + LocalBridge typed errors
5. agent_workflow + task_control
6. git_workflow + document_workflow
7. 按收益逐步 Native Rust 化
```

最终目标：

> LocalBridge 保留成熟原子 runtime 的执行效率，但拥有自己的稳定 Agent API、安全边界、Windows 命令环境、权限模型和可替换执行内核；第三方 runtime 只作为实现细节存在。