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
