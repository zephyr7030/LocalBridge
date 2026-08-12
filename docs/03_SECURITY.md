
# 03 — Security

## 安全边界

LocalBridge 是受信任本地代码执行器，不虚假声明为 OS sandbox。

依赖：

- 唯一 active workspace；
- capability policy；
- mandatory `tools/call` enforcement；
- secure CredentialStore；
- Privileged Broker；
- loopback-only；
- exact runtime checksums；
- process ownership。

Unknown 默认 deny。

## Capability / 权限

```text
read
write
process-exec
git
workflow
privileged-external-runtime
elevated-exec
control-plane
```

编辑模式：reviewed read/write。  
完整模式：+ ordinary process execution。  
管理员模式：+ Broker-routed elevated operations。

`control-plane` 所有模式永久 deny。

MCP 永远不能修改：

- 权限模式；
- 管理员设置；
- 项目列表/active workspace；
- Runtime API Key；
- Tunnel ID；
- 开机启动；
- runtime manifest；
- PEP/Broker policy。

`tools/call` 是强制执行点；`tools/list` 只能用于 UX。

## 路径

Windows 必须覆盖：

```text
symlink
junction
reparse point
canonical/final path
alias
case/separator normalization
```

不能只做 lexical prefix/lowercase。

## Runtime API Key

secret 只进入 Windows Credential Manager 或等价 DPAPI-backed secure store。

普通配置仅保存 reference/backend/presence metadata。

禁止 secret 出现在：

```text
JSON/TOML/.env
browser storage
registry plaintext
logs
diagnostics
crash report
CLI/process arguments
```

UI 保存后不回显。

Tunnel 注入优先：

```text
stdin / secure inherited handle / dedicated channel
```

仅在上游真实要求且评估可接受时允许**目标子进程临时环境变量**，不得写系统/用户环境、不得持久化/记录/传给无关进程。

CLI secret 永远禁止。

没有安全注入方式：

```text
SecretInjectionUnsupported
```

fail-closed。

## Privileged Broker

LocalBridge 主程序永不整体提权。

```text
normal LocalBridge
  ↓ authenticated local IPC
Privileged Broker (Administrator)
```

Broker：

- first-party Rust；
- explicit UAC；
- no network listener；
- restricted local IPC；
- typed/versioned schema；
- generation/session secret；
- replay/stale/malformed defense；
- bounded request/output；
- timeout/cancel；
- app lifecycle ownership。

LB-011 选定的 Windows IPC 基线：

```text
LocalBridge (普通用户)
  → 创建单实例 duplex Named Pipe
  → DACL 仅允许当前 LocalBridge 用户 SID
  → PIPE_REJECT_REMOTE_CLIENTS
  → 显式 runas 启动 localbridge-privileged-broker.exe
  → 校验实际连接 PID = 本次 ShellExecuteEx 返回并仍持有 handle 的 Broker PID
  → 通过后才交换 generation + CSPRNG session nonce
  → 每条请求严格递增 sequence，stale/replay fail-closed
```

Pipe 名使用 CSPRNG 随机 suffix，并使用 first-pipe-instance 防止同名 server 抢占；pipe 名与 generation 可以出现在 Broker CLI，session nonce 不进入 CLI/日志/Debug。帧采用有上限的长度前缀协议，未知/畸形/超限消息在 dispatch 前拒绝。

LB-011 Broker 基础协议仅包含 `Ping` / `Shutdown`，不包含管理员执行操作；管理员能力由后续权限 PR 在同一认证边界内单独接入。Broker 不开放 TCP/UDP listener；LocalBridge pipe 会话断开后 Broker 退出。UAC 路径只提供给 Rust 内部显式用户操作调用，普通启动与 `--background` 不调用该路径。

`elevated_exec` 必须 structured program/args/workdir，no shell default，timeout/cancel/output limit/redaction。

## 网络

只允许 listener：

```text
127.0.0.1
::1
```

禁止：

```text
0.0.0.0
::
```

## 项目 Registry

remembered projects 只是 metadata：

```text
max active authorized roots = 1
```

MCP 无权变更 registry/active root。

项目 `移除` domain operation 禁止任何 filesystem delete。

## 日志/诊断

- 本地；
- 约 5 MiB × 5；
- secret redaction；
- 手动导出；
- 零遥测；
- 无自动 crash upload。
