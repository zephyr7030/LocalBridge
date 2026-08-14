
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

开机启动使用 `--background`；管理员偏好不自动 UAC。
