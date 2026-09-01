# 02 — Architecture & Runtime

## Rust 唯一真相源

Rust Core 负责：

```text
AppState
SettingsStore
CredentialStore
WorkspaceRegistry
WindowsProcessSupervisor
CodingToolsRuntime
PolicyEnforcement
TunnelRuntime
RuntimeOrchestrator
RecoveryController
PrivilegeController
Diagnostics
Tray
```

React 只展示状态和提交用户动作，不管理 sidecar、PID、权限、安全策略或 raw MCP。

## Stable Adapter

```text
Domain
  ↓
stable port / adapter
  ↓
runtime adapter
  ↓
external runtime
```

Domain 不依赖 coding-tools-mcp / tunnel-client 私有结构。

### Schema25 — LocalBridge Agent Runtime Facade

Public MCP 不再以 upstream tool catalog 为合同：

```text
ChatGPT Agent
  ↓
LocalBridge versioned Tool Registry
  ↓
stable LocalBridge capability/action classification
  ↓
PEP
  ↓
WorkspaceRuntimeAdapter / Local executors / Privileged Broker
```

v1 非特权 core Registry 固定为：`workspace_context`、`agent_workflow`、`exec_command`、`command_control`、`task_control`、`git_workflow`、`document_workflow`、`view_image`。实际 `tools/list` 只返回当前 policy 允许的 Registry 子集，并可附加当前 policy 允许的 LocalBridge 特权扩展；`elevated_exec` 保持为现有 Broker-governed conditional privileged extension。upstream `tools/list`、私有 tool schema/name/error 和新增 tool 都不得自动成为 public API。

runtime adapter 初始化必须协商 facade mandatory capabilities；缺失能力或 adapter schema 不兼容时 fail-closed。adapter 负责把 upstream result/error 转成稳定 LocalBridge result/error，CurrentTask 也只使用稳定 LocalBridge public tool/capability identity 与脱敏摘要。

ShellResolver 只接受 `auto/powershell/pwsh/windows_powershell/cmd`。`auto` 在已验证可信候选中按 semantic version 选择最高兼容 PowerShell Core，再 Windows PowerShell 5.1，再 `cmd.exe`。PATH 只能发现候选，不能赋予信任；任何候选在 version probe/执行前必须先通过可信安装位置或显式注册 executable identity 的重新验证。禁止 command-text guessing、MCP 任意 shell executable、shell auto-install/update。

DirectProcessExecutor 与 ShellExecutor 是两个结构化边界；direct/privileged process 不以 shell string 为 canonical。WSL/container/remote、custom shell registry、environment-manager abstraction 与精确目录结构延后，不属于 v0.1 Gate。

### Schema27 — Public Facade Session / Workspace / Git Semantics

G2 generation 9 与随后 public plugin 实测进一步冻结 LB-006 的 stable facade 边界。第三方 runtime 的 session/output handle、结果字段命名、Git repository discovery 语义都只能是 adapter 私有实现细节。

#### `workspace_context`

存在 active workspace 时，public `workspace_context` 必须返回 LocalBridge 自己的 typed projection：

```text
workspace   = 非空、普通 Win32 绝对路径，例如 D:\project
default_cwd = active workspace 内的相对路径，例如 . 或 LocalBridge
```

`workspace` 必须来自与当前 freshly validated filesystem identity 绑定的 ordinary execution/display path，不得为空，不得返回 `\\?\`。adapter 不能因上游字段名变化而静默 `.unwrap_or_default()` 成空字符串；其读取的 private result 字段若未被 upstream `outputSchema` 精确保证，必须在 public facade 启动前通过确定性的 compatibility/result semantic probe 验证，不兼容则 `RuntimeCapabilityMismatch` fail-closed。

#### Public command session ownership

LocalBridge 必须拥有 public Session Manager：

```text
public session_id != upstream private session_id
public output_ref  != upstream private output_ref
```

public opaque handle 只由 LocalBridge 发行并映射到当前 adapter/private runtime。private session id/output ref 不得直接进入 public result、CurrentTask、日志或长期状态。

`command_control` 固定语义：

```text
poll  → public session_id，读取 live-session 最新状态/增量输出；不得误用只接受 output_ref 的 retained-output read
write → public session_id + chars
kill  → public session_id + signal/wait
read  → public output_ref + stream/offset/limit
```

当前 bundled runtime 的 live polling 可以由 adapter 映射到 `write_stdin(session_id, chars="")`，但该 private tool name 不是 public 合同。

public session 生命周期必须后台收敛，不依赖 Agent 主动 poll 才能结束：

```text
running → completed | failed | timed_out | cancelled | lost
```

上游 private session 完成、被 pruning、runtime 重启或 handle 丢失时，LocalBridge 必须保留/生成 terminal public snapshot；不得永久停留 `running`。如果 private handle 在 terminal outcome 被可靠观察前丢失，public session 进入 `lost`/失败终态并返回精确稳定错误 `SessionUnavailable`，而不是继续假装运行。

普通 process exit 的稳定语义：`exit_code == 0` 才是 `completed`；任何 `exit_code != 0` 均为 `failed` / `ProcessFailed`，即使 stdout/stderr 为空也必须 `ok=false` / `isError=true`，并把 CurrentTask 投影为 Failed。timeout/cancel 分别使用稳定 timed-out/cancelled 终态。

#### Public action completeness

`tools/list` 不得广告调用后恒定返回“当前不可用”的 action。当前 v1 已冻结 action 必须在 LB-006 PASS 前真实可执行：

```text
agent_workflow: diagnose / bugfix / feature / refactor / test_failure / build_release / document / resume / custom
task_control: list / get / cancel
document_workflow: inspect / search / create / edit / convert / rebuild
```

未来若新增 action，必须先有实现、typed contract、PEP classification 与测试再进入 public schema；禁止“先暴露、后实现”。

#### Workspace-relative public path inputs

Schema33 起，路径授权按模式分层。Edit/Full 的 workspace-bound public path/workdir 参数仍采用 **active-workspace-relative** 语义；Elevated 的普通用户 route 也不因 Broker Active 而静默扩大。但 Elevated 的明确 privileged filesystem / administrator execution route 可以接受管理员 Token 可访问的 workspace 外路径。至少包括：

```text
exec_command.workdir
git_workflow.path / paths
document_workflow.path / source
view_image.path
```

在 Edit/Full 与 Elevated 普通 route 中，drive-letter absolute、UNC absolute、Win32 verbatim、POSIX-leading-slash 与包含 `..` traversal 的输入继续在 LocalBridge public boundary fail-closed，并映射为稳定 LocalBridge `WorkspaceDenied`/`InvalidArgument`。只有已经满足管理员模式安全确认 + UAC + Active Broker 的 privileged route 才能使用 workspace 外绝对路径，且授权来源是管理员 Token，而不是字符串 normalization。

#### Nested Git repository discovery

`git_workflow` 的五个 action 必须共享 LocalBridge-owned repository resolver。对 active workspace 内的 nested repository：

```text
status / diff / log / show:
  从请求 path 所在目录向上寻找最近 repository root，最多到 active workspace root

blame:
  从请求文件 parent 向上寻找最近 repository root，并保留该文件相对 repo root 的 path
```

`path` 用于选择 repository context；action-specific `paths` 才作为 path filter/pathspec。`git_status(path="LocalBridge")` 能识别的 nested repo，`git_log/git_show/git_diff` 必须识别同一 repo，`git_blame(path="LocalBridge/package.json")` 必须识别其 enclosing repo。若 resolver 已确认 repository，`git_diff` 禁止静默降级成 `non-git diff fallback`。Edit/Full 的 repository discovery 永远不能越过 active workspace root；Elevated 只有通过明确 privileged scope 才能解析管理员 Token 可访问的 workspace 外 repository。

### Schema28 — Public Runtime Fidelity / Testable Session Semantics

Schema27 的稳定 facade 只在真实 public 行为满足以下语义后才可重新 PASS：

- Shell command text 只经过**一次预期的 shell parse**。LocalBridge 可以选择可信 shell executable 并添加固定启动 flags，但不得把用户 command 再拼接进第二层 shell string。`Write-Output "a|b"`、`Write-Output "a&b"` 以及单引号等价形式都必须输出原字面值。
- public command session 维护 LocalBridge-owned incremental cursor/delta。`poll` 每次只返回自上次 public poll/cursor 后新增的输出；中间 chunk 不丢失，已经交付的 chunk 不重复。无新输出时返回空 delta + 当前状态/terminal metadata。
- `write` 对 exec 已返回但仍为 running 的 public session 保持有效；post-start stdin 必须真正到达进程。
- `kill` 在 valid running session + healthy runtime 下不得误报 `RuntimeUnavailable`。成功 kill 必须收敛到稳定 `cancelled` terminal snapshot；后续 `poll` 返回同一 terminal truth，不能退化成 `SessionUnavailable`。
- `view_image(auto_resize=true)` 在请求上限小于源图时必须执行真实 resize，保持比例并使结果 dimensions 不超过 max；需要 resize 本身不能映射为 `ProcessFailed`。
- public command output 统一投影为有效 UTF-8。Windows PowerShell/`auto` 解析到 PowerShell 时，中文 `中文输出测试` 必须精确保真，不允许 mojibake/replacement characters。`shell=cmd` 保留原生 `cmd.exe` 当前代码页语义：当代码页本身无法表示中文/Emoji 时，不要求精确 non-ASCII round-trip；LocalBridge 不得仅为强制 Unicode 而注入 `chcp`、切换代码页或增加第二层 shell reparse。`cmd.exe` 自身代码页造成的替换/乱码不单独构成 LocalBridge 缺陷，但 LocalBridge 在捕获后额外引入的损坏仍属于缺陷。
- `git_workflow.blame start_line/end_line` 和 document line range 都是 **1-based inclusive**。`5..5` 精确一行，`1..3` 精确三行；`start_line > end_line` 在 LocalBridge boundary 返回 `InvalidArgument`，不能成功返回空结果。
- capability negotiation 不只验证 input schema。任何 adapter 实际消费、但 upstream `outputSchema` 未精确保证的 private result field，都必须在 facade serving 前通过 deterministic、non-destructive semantic compatibility probe 或等价 fail-closed 证据验证。

真实 command/session Runtime Gate 必须用一个共享 bundled runtime + PEP 生命周期覆盖：

```text
exec → incremental poll → write → read → kill → stable terminal convergence
```

failure/timeout/private-session-lost 可在 isolation 本身为被测行为时独立 fixture；不得为了独立 assertion 无理由反复启动同一套 Python/MCP/PEP 拓扑。

### Development console 与 packaged GUI

开发/测试 harness 可以出现后台命令进程或可见 console window；这不是产品行为证据，也不构成产品缺陷。正式打包/正常 GUI 运行时，LocalBridge-owned managed children（bundled coding runtime、PEP-adjacent managed command route、Tunnel、Broker/background helper、shell/direct command）不得意外创建可见 console window，除非未来显式 interactive-terminal 合同允许。该要求必须由 release-style/packaged launcher 实测，同时保留 Job/process ownership 与最小必要进程拓扑。

### Schema29 — Durable task-state terminal commit

public command/session 的内存 terminal snapshot 仍然不足以承担 workflow/task-state 的最终真相。每个 command 一旦从 running 进入任一 terminal outcome，必须走一个不可跳过的 `finally`（Python/tool-host）或语义等价的 unconditional finalizer（其它实现语言）。该 finalizer 持有 task-state owner transaction/lock，并在**同一原子状态提交**中完成：

```text
verify owner == (task_id, session_id)
→ persist bounded/redacted terminal snapshot
→ append exactly one command_finished
→ current_command = null
→ commit
```

finalizer 覆盖 success、nonzero exit、failure、timeout、cancel、kill、runtime error、transport/tool exception；不得依赖调用方随后再执行 cleanup。terminal snapshot 至少能够稳定表达 terminal classification、exit/signal/timeout/cancel/error 与可安全保存的 output reference/summary，因此即使 private runtime session 被约 300 秒 retention 清理，task-state 仍能回答最终结果。private session retention 只能作为短期输出读取资源，不能成为 terminal truth source。

command state 的 start/replace/finish/clear 都必须比较 `(task_id, session_id)` owner。延迟到达的旧 task finalizer、旧 session watcher、timeout callback 或 kill callback 若 owner 已变化，不得清空/覆盖新 command；owner mismatch 只能 no-op 或返回稳定 typed conflict。相同 owner 的重复 terminal callback 幂等：保留第一份稳定 terminal snapshot、不重复 `command_finished`、不重新建立 `current_command`。

### Schema30 — Nested project / trusted PowerShell / structured workspace write

`agent_workflow` 增加 workspace-relative `path` 作为**项目上下文选择器**，默认 `.`。active workspace 仍是唯一授权根；`path` 只选择该授权根内的工程上下文，不得切换 WorkspaceRegistry、不得新增授权根。`path="LocalBridge"` 必须直接选择 `D:\project\LocalBridge`；若 `path` 指向 `LocalBridge/src` 等后代目录，则向上寻找最近 enclosing repository/project root，搜索上限仍是 active workspace root。workflow 的 Git before/after、默认 command workdir 与稳定结果中的 selected/project context 必须基于该选择，且与 `git_workflow` 的 nested-repository resolver 一致。

可信 PowerShell 解析必须稳定选择受信任的已安装 executable，且不能破坏正常 coding shell；至少 `Get-Location`、`Get-ChildItem`、`Test-Path` 在 `windows_powershell` 与 `auto`→PowerShell 下可用。Full 的 PowerShell 与其他任意 Shell 一样使用当前 Windows 用户令牌，不再维护一套只能检查首层文本、无法约束脚本与后代的 provider/command matcher。

active workspace 内普通文件/目录 mutation 属于 reviewed workspace write，而不是 WorkspaceRegistry/control-plane mutation。该结构化路线固定在现有 `agent_workflow` 的 optional `directory_changes` 字段：bounded array 中每项只能是 `{ action, path }`，`action` 只允许 `create_directory` / `remove_empty_directory`，`path` 必须 active-workspace-relative。它无需 process exec，可在 `D:\project` 授权下创建 `test/`，并在为空时清理该目录；Edit 与 Full 都可授权此类 reviewed write。该路线必须执行 final identity/reparse 边界验证，禁止 absolute/`..`/reparse escape；本合同不自动授权非空递归目录删除，也不增加第九个 public core tool。

### Schema33 — Elevated administrator-token scope / UI geometry

权限边界分为三个明确表面：Edit/Full 的结构化文件工具以 active workspace 为授权根；Edit 不提供 process exec；Full 的任意 Shell/进程使用当前 Windows 用户令牌，Shell 与全部后代具有相同 OS 权限，不声称受结构化 workspace path authority 约束。管理员操作由 Active Privileged Broker 单独承载。Elevated 在固定风险警告、完整 9000ms 红色确认倒计时、用户明确确认和 Windows UAC 完成后，允许 Broker 在管理员 Token 范围内访问 workspace 外文件系统、执行 reviewed 管理员进程与可信 shell 命令、执行系统维护。

`reg.exe / sc.exe / schtasks.exe / netsh.exe / bcdedit.exe / dism.exe` 在 Full 中与脚本、解释器和后代一样使用当前用户令牌：Windows 允许的操作可执行，需要管理员令牌的操作由 OS 拒绝。LocalBridge 不按 executable 名称制造一套可被脚本绕过的平行权限真相。Elevated 的 reviewed administrator route 可以使用管理员令牌；ordinary `exec_command` 不得因为 Broker Active 而隐式继承该 token。整个 LocalBridge 主进程仍禁止整体提权。

管理员范围扩大不扩大 LocalBridge control-plane：AI/MCP 仍不能修改 PermissionMode、批准警告/UAC、启停 Broker、修改 WorkspaceRegistry/active workspace、credential、Tunnel/MCP/runtime/PEP/Broker policy 或 LocalBridge autostart。

UI 同步采用两条全局规则：含文字按钮宽度由最大单行可见字数决定、高度由实际渲染行数决定；字体大小严格只有标题、正文、辅助三级，普通正文/字段标签/按钮/主要状态统一使用正文字号，helper/元数据/时间/次要状态统一使用辅助字号，禁止第四级字号。Dashboard 在 Elevated + Active Broker 时以黄色 `全目录访问` 表示当前文件访问范围，并阻止项目切换，显示固定说明弹窗。

### Schema26 — Administrator Mode Safety Consent Gate

管理员模式可见入口的视觉与授权状态机由 LocalBridge 自己拥有，不能由 frontend 直接跳到 UAC：

```text
User selects 管理员模式
  ↓
Broker Active?
  ├─ yes → keep current active state; no duplicate UAC merely due to reselection
  └─ no
      ↓
create AdminConsentChallenge / equivalent typed backend state
      ↓
show fixed warning dialog
      ↓
whole-red confirm disabled for full 9000ms
      ↓
backend monotonic elapsed >= 9000ms ?
  ├─ no → reject confirmation fail-closed
  └─ yes
      ↓
user clicks enabled 确认
      ↓
backend security validation
      ↓
Windows runas / UAC
      ↓
Privileged Broker activation
```

固定可见警告内容由 schema26 机器合同提供；UI 不得自行增删或改写八条后果。管理员模式入口控件统一橙色 `#ff9500`，确认弹窗的**整个确认按钮**统一红色。首次显示标签 `确认9`，依次到 `确认1`；9000ms 前 disabled，9000ms 后仍为红色、标签精确为 `确认` 并才可点击。

前端倒计时只负责 presentation，不是授权真相。推荐 backend 持有等价结构：

```text
AdminConsentChallenge
- challenge_id
- created_at_monotonic
- not_before = created_at_monotonic + 9000ms
- consumed / cancelled
```

confirm typed intent 必须携带当前 challenge identity 或等价不可混淆关联，backend 在处理 intent 时重新检查 challenge 仍有效、未取消、未消费、达到 not-before，任何 stale frontend state、rerender、重复/合成 click、键盘提前提交或旧 challenge 重放均 fail-closed。challenge fresh open 必须重新开始完整 9 秒；取消、Esc、close/dismiss 使 challenge 失效且不得改变 PermissionMode/Broker/UAC。

`--background` 恢复管理员偏好不得创建 challenge、不得显示 warning、不得 UAC，只能保持 Requested/等价未授权状态。High/Critical 单操作授权是独立后续 Gate，不能复用“已看过管理员模式警告”作为单操作批准。

## 启动/停止

普通 configured 前台入口先完成 UI milestone：

```text
create/show main UI → interactive → typed UI-ready
→ backend managed-service start
```

若 runtime 原本停止，`UI-ready` 前不得启动 MCP/PEP/Tunnel；前端只发送一次 typed intent，不拥有 lifecycle。backend 对重复 ready 幂等并维持 single owner。`--background` 不依赖 UI-ready；唤醒已有健康后台 runtime 不因 UI-ready gate 重启。

启动：

```text
MCP → ready
→ Policy Enforcement → ready
→ Tunnel → ready
→ Ready
```

停止：

```text
Tunnel ↓ → Policy Enforcement ↓ → MCP ↓
```

## 进程所有权

Windows 优先使用 Job Object。

必须处理 stale PID、PID reuse、child tree、异常退出；禁止 PID-only 生产所有权。

## 自动重连

recoverable：

- Tunnel 断开/异常；
- PEP 异常；
- MCP 异常；
- 可恢复依赖链故障。

non-retryable：

- credential/config；
- checksum/runtime integrity；
- 明确 policy fault；
- `SecretInjectionUnsupported`；
- 需要用户修复的配置错误。

一次 outage generation 最多 **5 次重连**；最初掉线不计入：

```text
attempt 1 → 1s
attempt 2 → 2s
attempt 3 → 5s
attempt 4 → 10s
attempt 5 → 30s
```

只重启最小必要层：

```text
Tunnel fault → Tunnel
PEP fault    → PEP + Tunnel
MCP fault    → MCP + PEP + Tunnel
```

任一次成功：

- Ready；
- 清除本次 retry counter；
- 无成功 toast；
- 稳定 60s 后清除故障 generation/退避历史。

第 5 次失败：

```text
Faulted
→ stop automatic retry loop
→ emit one UserAttentionRequired
```

同一 generation 只允许一次最终用户注意事件。

用户点击 `重试`：

```text
new outage generation
→ new 5-attempt budget
```

RecoveryController 不创建 UI；只有 `UserAttentionRequired` 允许 UI 显示最终错误窗口。

## Workspace 切换

```text
candidate validate
→ Tunnel ↓
→ PEP ↓
→ MCP ↓
→ MCP(candidate) ↑
→ PEP(candidate) ↑
→ Tunnel(candidate) ↑
→ Ready
→ commit active
```

失败时 candidate 不成为 active。

移除当前项目：

```text
Tunnel ↓ → PEP ↓ → MCP ↓
→ clear active
→ NoActiveWorkspace
```

不自动切换 remembered project。

## 当前执行投影

```text
MCP/Broker event
→ capability classification
→ safe summary
→ CurrentTaskStatus
→ 单行 UI
```

不从模型思考/聊天回答推断。

## Background

```text
LocalBridge.exe
LocalBridge.exe --background
```

`--background` 从入口不创建/显示主窗口。

开机启动使用 `--background`；管理员偏好不自动显示安全确认、不自动 UAC。


### Schema36 — Runtime observability / shell fidelity / stable diagnostics

Public v1 core Registry 仍严格为 8 个工具；不得为了 capability discovery、policy explain 或环境自检新增第九个 core tool。

`workspace_context` 在原 workspace/default_cwd 之外增加只读 typed projection：`permission_mode / workspace_scope / ordinary_route_token / elevated_route_available / privilege_state / shell_discovery / capabilities`。该 snapshot 只用于观察，不能授权；PermissionMode/Broker 状态变化后必须刷新，同一 `tools/call` 仍在服务端重新判定。capabilities 至少安全表达当前 public tools/actions、trusted cmd、PowerShell Core、Windows PowerShell、Git、bundled Python/Node（若存在）以及 elevated route 可用性/原因。

Full ordinary Shell 的权限只由 Effective Permission 与 `current_windows_user` execution token 决定，不根据 executable、参数、脚本扩展名、别名、重定向或后代表面文本改变。cmd/PowerShell 必须保持 Windows 原生常用语义，包括 NUL 重定向、管道、`>`/`2>`、引号、`&&`/`||`、环境变量、Unicode 路径以及 `.cmd/.bat/.ps1`；禁止把 Shell 收缩成自定义 DSL。结构化 filesystem/document/Git/image 的 path authority 不得解析或补偿 Shell command text。

canonical public errors 至少稳定区分 `PolicyDenied / WorkspaceDenied / RuntimeUnavailable / InvalidShellSyntax / PrivilegedRouteUnavailable / ProcessTimedOut`。可附带脱敏 `rule_category` 与 remediation；不得泄漏私有 policy internals。历史 `PrivilegedRouteNotAvailable` 仅可作内部/兼容别名，新的 public canonical 输出使用 `PrivilegedRouteUnavailable`。显式 `shell=pwsh` 不可用时，`RuntimeUnavailable` 同时返回可信发现摘要和可用 fallback。

`exec_command` 与 `agent_workflow` 可在既有 schema 上提供只读 `dry_run/explain`，但 ordinary Shell 只能如实返回 current-user execution authority，不能从首层命令文本推断后代是否“需要管理员权限”。结构化动作仍可返回 `workspace_restricted / elevated_required / permanently_denied`。统一环境自检由 enriched `workspace_context` 与/或 `agent_workflow(action=diagnose)` 承担。

command/session 稳定结果在适用时统一 `task_id/session_id/status/elapsed_ms/exit_code/output_ref`；retained output 分页增加 `total_bytes/offset/returned_bytes/truncated`。public contract 不要求暴露 OS PID。所有 document/image/Git/workflow/workspace structured path authority 继续由单一 LocalBridge-owned canonical containment/path-authority 实现。

### Schema39 — Workflow / Task / Execution maturity boundary

LocalBridge 的 Agent 执行模型统一为 `Workflow → Task → optional Execution → public Session → process tree`。只有 process-backed Task 才分配 Execution/public Session/process tree；Git、document、image 与纯结构化 workspace Task 不得为了形式统一伪造 session handle。`task_control` 的 public action 继续严格只有 `get/cancel`；高层 cancel 通过当前 Task owner 自动解析其 Execution/Session 并调用共享 terminator/finalizer，模型不负责选择 session/process。

Public schema 的 authority 是真实 downstream MCP client 最终可消费的投影，而不是 facade/server 内部 JSON Schema 单测。多 action/operation 工具必须保留直接可发现的顶层 properties/enum/bounds；合法调用不得要求模型先触发 `InvalidArgument` 猜字段。服务端继续做严格 action/operation-specific validation，不能为了客户端兼容而降低验证。

typed error 继续沿用现有 LocalBridge canonical names，不创建同义错误体系。同一失败条件经 direct tool 或 `agent_workflow` 间接路径必须进入同一 normalization table。`agent_workflow` 只承担 orchestration，必须复用与 direct tools 相同的 filesystem/Git/process/document/image/privilege service、Session Manager、ShellResolver、terminal finalizer、path authority 与显式 transitive capability declaration；不得维护第二套 Shell/File/Git 语义或命令字符串 classifier。

`workspace_context` 是 compact first-turn discovery：在可确定时直接投影 project name/type/version、Git branch/dirty/changed count、package manager、build/test system、runtime availability、trusted shells、permission mode、current task。稳定 discovery 优先读取已有/缓存 snapshot，禁止为了同一稳定信息在每次调用时重复拉起探测进程；无法确定的字段必须显式 unknown/unavailable，不能猜测。

public process/session 生命周期固定为 `running → completed | failed | cancelled | timed_out | lost`。所有非 `running` 状态都是 durable terminal truth，独立于客户端持续 poll。恢复边界限定为 durable workflow checkpoint/resume 与 retained output continuation；schema39 v0.1 不要求 generic process pause、用户可浏览 task history、自动 filesystem snapshot/rollback，也不新增 `file_workflow`。Public surface 继续严格 8 个非特权 core + `elevated_exec` privileged extension。
