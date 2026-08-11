# 02 — Architecture

## 总体架构

```text
                           Internet
                              │
                         ChatGPT Web
                              │
                   OpenAI Secure MCP Tunnel
                              │
                 ┌────────────▼────────────┐
                 │ openai/tunnel-client    │  sidecar
                 └────────────┬────────────┘
                              │ loopback
                 ┌────────────▼────────────┐
                 │ Policy Enforcement Point│
                 │ upstream / adapter /    │
                 │ Rust Guard              │
                 └────────────┬────────────┘
                              │ loopback + auth
                 ┌────────────▼────────────┐
                 │ coding-tools-mcp        │  Python sidecar
                 └────────────┬────────────┘
                              │
                       Authorized Workspace
```

## PEP 实现不提前锁死

LB-000 必须回答：

1. upstream 是否可原生强制 capability/tool policy；
2. 是否可阻止组合 workflow 间接触发 process execution；
3. 是否可在 `tools/call` 强制执行；
4. 若不足，Rust Guard 需要代理哪些 MCP transport/session 语义。

决策优先级：

```text
upstream native enforcement
        ↓ 不足
thin adapter
        ↓ 不足
Rust MCP Guard
```

禁止为“架构看起来完整”而提前重写半套 MCP Server。

## Desktop

```text
React/TypeScript
      │
      │ Tauri invoke/events
      ▼
Rust Core
├─ AppState
├─ SettingsStore
├─ CredentialStore
├─ WorkspaceManager
├─ PrivilegeController
├─ PrivilegedBrokerClient
├─ WindowsProcessSupervisor
├─ CodingToolsRuntime
├─ PolicyEnforcement
├─ TunnelRuntime
├─ RuntimeOrchestrator
├─ RecoveryController
├─ HealthMonitor
├─ TrayController
└─ Diagnostics
```

## React 边界

允许：

- 展示状态；
- 发出用户意图；
- 路由 Wizard；
- 展示 typed errors。

禁止：

- spawn/kill 进程；
- 直接读取 credential；
- 保存 Runtime API Key；
- 自己计算 runtime truth；
- 通过 frontend timer 决定重启 sidecar。

## Rust 边界

负责：

- 生命周期；
- process ownership；
- Job Object / process generation；
- secret injection；
- 端口；
- readiness；
- retry/backoff；
- tray；
- autostart；
- app-data；
- runtime manifest；
- capability policy。

## Control Plane 与 Data Plane

LocalBridge Control Plane 永远不通过 MCP 暴露：

- active workspace / candidate workspace；
- authorized roots；
- Runtime API Key；
- Tunnel ID；
- LocalBridge permission mode；
- autostart；
- admin/elevation policy；
- runtime manifest；
- Guard/PEP policy。

即使 Full 模式，也不允许 MCP 修改这些项。

## 端口模型

所有 listener 只允许：

```text
127.0.0.1 / ::1
```

默认优先动态选择可用 loopback port。

禁止：

```text
0.0.0.0
LAN bind
```

## Windows Process Ownership

首选：

```text
LocalBridge.exe
    │
    └─ Windows Job Object
       ├─ python.exe -m coding_tools_mcp
       └─ tunnel-client.exe
```

需要在 LB-000/LB-004 明确验证：

- `KILL_ON_JOB_CLOSE`；
- PID reuse；
- stale runtime state；
- child process tree；
- coding-tools-mcp 启动的测试/构建进程是否继承 Job；
- 退出/崩溃/注销行为。

## 关闭语义

窗口关闭：

```text
Window → hide
Runtime → unchanged
Tray → remains
```

托盘“退出”：

```text
mark manual stop
→ Tunnel ↓
→ Policy Enforcement ↓
→ MCP ↓
→ flush state
→ exit app
```

## Workspace Switch

必须使用 candidate/active 两阶段：

```text
validate candidate
→ Tunnel ↓
→ PEP ↓
→ MCP ↓
→ start MCP(candidate)
→ start PEP(candidate)
→ start Tunnel(candidate)
→ Ready
→ commit candidate as active
```

如果失败：

- active 不得被提前覆盖；
- UI 显示 candidate fault；
- 允许回滚上一个 active workspace。

## 开机后台

```text
LocalBridge.exe --background
```

后台模式从进程入口决定是否创建/展示窗口，不允许先创建再 hide 作为唯一实现。

## ChatGPT 页面

不内嵌 ChatGPT。

Rust 只暴露：

```text
open_chatgpt_mcp_page()
```

URL 来自单一 allowlisted 常量：

```text
CHATGPT_MCP_SETTINGS_URL
```

Renderer 不得传任意 URL。


## Elevated 扩展

用户可见：

```text
Edit / Full / Elevated
```

内部：

```text
Elevated = Full + Privileged Broker
```

主程序、Tunnel、MCP 保持普通用户令牌。

详细合同见：

`docs/12_PRIVILEGED_BROKER.md`


## Dashboard Privilege Projection

React 只读取 Rust 暴露的投影状态：

```text
permission_mode
privilege_state
```

Dashboard 不自行推导“管理员是否已启用”。

例如：

```text
permission_mode = Elevated
privilege_state = Requested
```

必须显示：

```text
权限：Elevated
管理员权限：等待授权
```

不得错误显示“管理员权限已启用”。

Rust Core 是 privilege runtime truth 的唯一来源。

## UI Presentation Boundary

内部状态必须经过中文展示映射：

```text
domain state
→ presentation mapper
→ zh-CN UI string
```

例如：

```text
RuntimeState::Ready      → 编码服务：已就绪
PrivilegeState::Active   → 管理员权限：已启用
PermissionMode::Elevated → 权限模式：管理员模式
```

组件不得直接把 Rust/domain enum 转成字符串显示。

## Stable Runtime Adapter Boundary

Domain 不直接依赖 coding-tools-mcp / tunnel-client 私有结构。

```text
Domain
  ↓
Stable Port / Adapter Trait
  ↓
Concrete Runtime Adapter
  ↓
External Runtime
```

上游替换时，应主要修改 adapter 和 compatibility layer，而不是 UI、settings、tray 或 domain state。

## Current Task Projection

当前任务是 Rust/domain 的临时投影：

```text
MCP / Broker event
      ↓
Capability classification
      ↓
Safe Task Summarizer
      ↓
CurrentTaskState
      ↓
中文展示层
      ↓
主控界面单状态区
```

React 不得解析 raw MCP payload 猜任务类型，不得累积事件形成 feed。

## Minimal Activity Presentation

Domain 仍维护完整 `CurrentTaskStatus`，但 presentation 层压缩为单行：

```text
TaskKind + SafeSummary + ExecutionState
→ MinimalTaskPresentation
```

运行中的用户呈现不需要把 `Running` 文本显示出来，活动状态由绿色脉冲指示器表达。

业务状态不得依赖视觉颜色或动画；颜色/动效只属于 UI presentation token。

## Project Registry Boundary

```text
WorkspaceRegistry
├─ WorkspaceEntry A
├─ WorkspaceEntry B
└─ WorkspaceEntry C

ActiveWorkspace
└─ exactly zero or one authorized root
```

Registry 是用户便利性数据；ActiveWorkspace 才是授权边界。

Domain 必须支持：

```text
NoActiveWorkspace
ActiveWorkspace
CandidateWorkspace
```

不得把 remembered entries 传给 MCP 作为多根授权集合。

## Credential Store Boundary

```text
React input
→ one-shot Tauri command
→ Rust CredentialStore
→ Windows secure backend
```

React 不持久化 secret。

SettingsStore 只能持有：

```text
credential reference
credential presence
```

不能持有 secret value。
