# 14 — UI Terminology & Localization Policy

## 总原则

LocalBridge 面向用户的界面默认使用简体中文。内部代码、协议、文件名和日志字段可以使用英文，但用户界面必须经过本地化映射。

只有行业标准缩写、产品名和正式专有名词可以直接保留英文，例如：

- MCP
- API
- URL
- UAC
- HTTP / HTTPS
- OpenAI
- ChatGPT
- Windows
- LocalBridge
- SHA256

以下普通英文不得作为面向用户的标题、按钮或状态名：

- Dashboard
- Settings
- Diagnostics
- Edit
- Full
- Elevated
- Broker
- Runtime
- Ready
- Faulted
- Requested
- Active
- Stopped
- Starting
- Reconnect
- Workspace

## 冻结术语

| Internal / English | 用户界面 |
|---|---|
| Dashboard | 主控界面 |
| Settings | 设置 |
| Diagnostics | 诊断 |
| Edit | 编辑模式 |
| Full | 完整模式 |
| Elevated | 管理员模式 |
| Privileged Broker | 管理员代理 |
| Runtime | 运行服务 / 运行时（按上下文） |
| Coding Runtime | 编码服务 |
| Tunnel | 安全隧道 |
| OpenAI Tunnel | OpenAI 安全隧道 |
| Workspace | 项目目录 |
| Ready | 已就绪 |
| Starting | 正在启动 |
| Stopped | 已停止 |
| Recovering | 正在恢复 |
| Faulted | 故障 |
| Requested | 等待授权 |
| Awaiting UAC | 等待系统授权 |
| Active | 已启用 |
| Reconnect | 重新连接 |
| Switch Workspace | 切换项目 |
| Open ChatGPT | 打开 ChatGPT |
| Runtime API Key | Runtime API Key（冻结英文专有字段名，不翻译） |
| Tunnel ID | Tunnel ID（冻结英文专有字段名） |

## 专有字段例外

中文优先不代表强制翻译所有技术标识。用户配置中的 `Tunnel ID` 与 `Runtime API Key` 是本产品冻结专有字段名，必须原样显示；`Runtime API Key` 尤其不得改写为“运行密钥”。完整 secret 永不回显。

管理员权限没有“启用管理员权限”独立按钮文案；可见用户点击/重新点击“管理员模式”本身就是 UAC 动作，离开管理员模式即关闭 Broker。

无活动任务的唯一待机文案为 `等待命令`；禁止 `空闲`。

## 状态映射

内部：

```text
RuntimeState::Ready
PrivilegeState::Active
PermissionMode::Elevated
```

用户只能看到：

```text
编码服务：已就绪
管理员权限：已启用
权限模式：管理员模式
```

禁止把 enum / fault code / process name 原样显示。

## 错误文案

错误必须：

1. 中文；
2. 简短；
3. 可行动；
4. 不泄露内部实现。

例如内部：

```text
TunnelAuthFailed
```

用户显示：

```text
安全隧道认证失败
请检查 Runtime API Key。
```

## 按钮

优先使用中文动词：

```text
打开 ChatGPT
切换项目
重新连接
查看诊断
```

禁止无必要的：

```text
Open
Retry
Reconnect
Enable
Disable
Advanced
```

## 技术字段

主控界面默认不显示：

```text
PID
SID
nonce
IPC
port
process generation
integrity level
transport type
```

## 实现要求

React 不得直接渲染内部 enum 字符串。

必须经过统一层：

```text
domain state
→ presentation mapper
→ localized zh-CN string
```

目录：

```text
src/lib/i18n/
src/lib/presentation/
```

即使 v0.1 只有中文，也不得在组件中散落内部英文状态。

## 当前任务术语

| Internal / English | 用户界面 |
|---|---|
| Current Task | 当前任务 |
| Task Kind | 类型 |
| Task Summary | 任务 |
| Idle | 等待命令 |
| Requested | 准备执行 |
| Running | 执行中 |
| Awaiting Permission | 等待授权 |
| Succeeded | 已完成 |
| Failed | 执行失败 |
| Blocked | 已阻止 |
| Cancelled | 已取消 |

不得显示 `Activity Feed`、`Recent Activity`、`Timeline` 等活动流概念。

## 极简执行状态文案

主控界面执行状态直接组合：

```text
{工具类别} {任务摘要}
```

执行中不额外显示：

```text
当前任务
类型
任务
状态
执行中
```

示例：

```text
运行测试 cargo test
读取文件 src-tauri/src/runtime/mod.rs
```
