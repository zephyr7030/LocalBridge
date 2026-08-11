# 03 — Security Model

## 核心原则

LocalBridge 不把“工具看起来安全”当成安全边界。

安全决策基于 capability：

```text
read
write
process-exec
git
workflow
control-plane
```

## 权限模式

### Edit

允许：

- 经验证的 read；
- 经验证的 workspace-scoped write；
- 经验证且不会间接启动命令的操作。

拒绝：

- process-exec；
- 可间接执行命令的 workflow；
- 未知 capability；
- LocalBridge control-plane。

### Full

允许：

- pinned runtime 中经 compatibility review 的 coding capability；
- process-exec；
- tests/build commands。

仍然拒绝：

- LocalBridge control-plane；
- 未经 review 的未知工具；
- 提权策略修改；
- secret / tunnel / workspace policy 修改。

Full 模式必须明确显示：

> 由 Coding Tools 启动的 Windows 进程拥有当前登录用户实际拥有的操作系统权限。

禁止使用“完整模式仍严格限制在 workspace”之类表述。

### Elevated（管理员模式）

管理员模式 = 完整模式 + 独立 Privileged Broker。

允许：

- 经审核的 `elevated_exec`；
- 经审核的管理员能力。

仍然拒绝：

- LocalBridge control-plane；
- 未知 capability；
- 未激活 Broker 时的管理员调用。

LocalBridge 主程序、编码服务和安全隧道不得因管理员模式整体提权。

## `tools/list` 与 `tools/call`

`tools/list`：

- 可过滤用于减少 UI 噪音；
- 不是安全边界。

`tools/call`：

- 每次都必须执行 capability policy；
- 即使客户端缓存了旧 tools/list；
- 即使编辑/完整/管理员模式切换后未重新扫描；
- 即使客户端直接构造被隐藏工具调用。

## Workspace 文件边界

必须对 Windows 特性做 adversarial tests：

- symlink；
- junction；
- reparse point；
- canonicalization；
- parent traversal；
- case-insensitive path；
- UNC/drive edge cases（若允许）；
- race between validation and use（可测范围内）。

只有实际验证后才能描述 workspace 文件隔离能力。

## Secrets

Runtime API Key：

- Windows Credential Manager / DPAPI-backed；
- 不进 settings；
- 不进 JSON/TOML；
- 不进 CLI args；
- 日志统一 redaction；
- crash/error message 统一 redaction。

LB-000 必须验证 tunnel-client 的**真实 secret injection 方式**，不得假设某个 env/flag 一定存在或安全。

## Supply Chain

所有 vendored artifact：

- exact version；
- exact provenance；
- SHA256；
- build-time verify；
- release-time verify。

checksum mismatch：

```text
RuntimeChecksumMismatch
```

且禁止启动。

## Windows Process Security

Process ownership 优先 Windows Job Object。

必须验证：

- LocalBridge crash 后 child cleanup；
- Tray exit 清理；
- uninstall 前清理；
- PID reuse 不误杀其他进程；
- manual stop 不被 supervisor 拉起。

## 自动恢复

可重试：

- transient process crash；
- transient health timeout；
- network/tunnel temporary disconnect。

不可无限重试：

- bad Runtime Key；
- invalid Tunnel ID；
- workspace missing；
- checksum mismatch；
- invalid policy；
- dependency missing。

backoff：

```text
1s → 2s → 5s → 10s → 30s
```

达到阈值进入 typed Faulted。

## Renderer

- 最小 Tauri capability；
- 不开放 arbitrary shell；
- URL open 走 Rust allowlist；
- 不接收任意 URL；
- 不访问 secret；
- 不拥有 PID/process handles。

## 安全验收

至少包括：

- Edit tools/list 无 process-exec；
- Edit tools/call 绕过 list 仍拒绝；
- Edit workflow 间接 exec 被拒绝；
- unknown capability 拒绝；
- Full control-plane 拒绝；
- mode switch 后旧客户端缓存不能绕过；
- junction/symlink/reparse escape；
- loopback-only；
- checksum mismatch；
- secret log scan；
- bad auth no restart storm；
- process tree cleanup；
- PID reuse/stale state。


## Elevated Security Boundary

管理员能力不得通过提升整个 LocalBridge 获得。

必须：

```text
ordinary LocalBridge
→ authenticated local IPC
→ elevated Broker
```

管理员模式没有自动时间限制；用户主动控制开关。

但以下不视为“时效”，属于安全生命周期：

- App 退出；
- Windows 注销/重启；
- Broker 崩溃；
- 用户关闭 Elevated；
- UAC 拒绝。

Broker 必须使用最小本地 IPC ACL、generation secret/nonce、typed schema 和 fail-closed policy。

即使 Elevated：

```text
control-plane = deny_always
```


## Runtime Supply-chain Boundary

Python/coding-tools/tunnel-client 均随 installer 固定。

禁止 runtime 自更新，防止：

```text
tool surface drift
→ policy stale
→ security boundary bypass
```

任何 runtime 版本变化必须重新跑 compatibility/security suite。

## Telemetry Boundary

产品没有 telemetry endpoint。

- 不自动上传 crash；
- 不上传工具使用统计；
- 不上传 workspace metadata；
- 不上传 diagnostics。

只有用户主动导出的本地诊断包可离开机器，且导出前必须 redaction。

## Credential At-rest Boundary

Runtime API Key 不属于普通配置。

```text
settings
→ only credential reference / presence

Windows secure credential backend
→ actual secret
```

明文 credential persistence、CLI secret argument、日志 secret 都是 release-blocking security failure。

## Workspace Registry Boundary

```text
remembered projects ≠ authorized projects
```

同一时间只有 `active_workspace` 是授权根。

MCP control-plane 永远无权新增、切换、移除项目或改变 authorized root。
