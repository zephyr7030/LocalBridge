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
管理员模式：+ 用户完成安全确认与 UAC 后，由 Broker 路由 reviewed elevated operations。

`control-plane` 所有模式永久 deny。

MCP 永远不能修改：

- 权限模式；
- 管理员设置；
- 管理员安全确认状态；
- 批准 Windows UAC；
- 启用/关闭 Broker；
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

Schema27 进一步冻结 public path 语义：LocalBridge workspace-bound MCP 参数默认都是 active-workspace-relative，而不是“任意绝对路径，只要最终看起来在 workspace 内”。`exec_command.workdir`、`git_workflow.path/paths`、`document_workflow.path`、`view_image.path` 等入口必须在 public boundary 拒绝 drive-letter absolute、UNC absolute、Win32 verbatim、POSIX-leading-slash 与 `..` traversal，并统一映射成 LocalBridge typed error；upstream `ABSOLUTE_PATH_DENIED`、private canonical path 或 private resolver 细节不得穿透。

唯一明确例外是 `workspace_context.workspace` 的只读信息投影：它可以显示/返回当前 active workspace 的普通 Win32 **绝对**路径，例如 `D:\project`，但必须与 freshly validated filesystem identity 绑定、非空且不得包含 `\\?\`。该绝对路径是上下文信息，不改变其他工具“输入必须相对 active workspace”的授权合同。

Git nested-repository discovery 必须限制在 active workspace 内：从请求目录/文件 parent 向上找最近 repo root 时最多走到 active workspace root，不允许借 Git discovery 跨出授权根。对已发现的 repo，任何 Git action 不得再静默切换到非 Git fallback。

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

LB-011 Broker 基础协议仅包含 `Ping` / `Shutdown`，不包含管理员执行操作；管理员能力由后续权限 PR 在同一认证边界内单独接入。Broker 不开放 TCP/UDP listener；LocalBridge pipe 会话断开后 Broker 退出。UAC 路径只提供给 Rust 内部显式用户授权流程调用，普通启动与 `--background` 不调用该路径。

`elevated_exec` 必须 structured program/args/workdir，no shell default，timeout/cancel/output limit/redaction。

### Schema30 workspace write 与 PowerShell capability 边界

active workspace 是**授权根**，不是只读根。Edit/Full 对授权根内普通文件与目录的 reviewed read/write 都是合法能力；禁止的是改变 WorkspaceRegistry、切换 active workspace、扩大授权根或越界访问。结构化目录 mutation 固定为 `agent_workflow.directory_changes[]`，每项只有 `action/path`，action 仅 `create_directory` / `remove_empty_directory`，使目录创建与空目录清理无需借 shell/process exec 完成。所有 path 都必须 active-workspace-relative 并经过 final-identity/reparse 检查；absolute、`..`、junction/symlink/reparse escape 必须 fail-closed。

PowerShell provider mutation 与“允许工作区写入”不是同一件事。由于 `Alias:`/`Function:` 等 provider 可改变命令解析面，`New-Item`、`Set-Item`、`Set-Content` 等通用 provider mutation 可以继续 conservative review-required；不得为了允许 `D:\project\test` 而整体放宽它们。反之，也不得因为这组 shell cmdlet 被严格审查，就让 Edit/Full 失去结构化 workspace write 能力。

PowerShell 标准 cmdlet baseline 必须来自固定/身份验证的系统模块或等价可信来源；任意 module autoload 继续关闭，用户可控 `PSModulePath` 不得重定向 preload。`Get-Location`、`Get-ChildItem`、`Test-Path` 等基础能力必须可用，否则 explicit `windows_powershell`/`auto` 不能称为可用 shell backend。

`agent_workflow.path` 只选择 active workspace 内 nested project context，不是 control-plane。它不得改变 active workspace；repo/project discovery 最多向上到 active workspace root，并与 `git_workflow` 使用等价 resolver 语义。

### Schema32 Windows 系统管理权限边界

Windows 操作系统级系统管理与 LocalBridge 自身 `control-plane` 是两个不同的权限域。`reg.exe`、`schtasks.exe`、`sc.exe`、`netsh.exe` 等受审计 Windows system-management program **不得被全局禁用**，但需要管理员权限的系统管理操作也不得从普通进程路由绕过管理员边界。

- `Full` 的 ordinary `exec_command` 遇到静态可识别的上述系统管理目标时必须要求 privileged route 并拒绝直接执行；
- `Elevated` 的 ordinary `exec_command` 同样不继承 Broker administrator token，Broker Active 不能把普通进程路由隐式升级为管理员执行；
- 管理员执行只能走 `Elevated + Active Broker + reviewed elevated_exec`，并继续经过既有安全确认 / Windows UAC / Broker 权限边界；
- reviewed system-management program 必须绑定 exact trusted `%SystemRoot%\System32` identity，不能使用 PATH、workspace 同名程序或其他 non-System32 替代物；
- 请求保持 structured direct `program + argv`，禁止借 `cmd.exe`、PowerShell 或其他 shell/interpreter fallback 把 Broker 退化为任意管理员 shell；
- Windows OS system management 本身不按 LocalBridge control-plane mutation 拒绝；但任何借这些 utility 修改 LocalBridge `PermissionMode`、管理员 consent/UAC/Broker activation、WorkspaceRegistry/active workspace、credential、Tunnel/MCP 配置、runtime/PEP/Broker policy 或 LocalBridge autostart 的请求仍永久 deny。

因此权限模型是：**Full 拒绝管理员系统管理直通；管理员模式通过受控 Broker reviewed route 放行；LocalBridge 自身控制面始终不可由 MCP 修改。**

### Schema26 管理员模式安全确认

所有用户可见 `管理员模式` 入口统一使用橙色 `#ff9500` 警告语义。Broker 未 Active 时，用户点击/重新点击管理员模式不能直接触发 UAC，必须先显示以下固定内容：

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

安全合同：

- 确认按钮**整个按钮为红色**；
- fresh open 从 `确认9` 开始，依次到 `确认1`；
- 完整 9000ms 内按钮 disabled；
- 9000ms 后同一红色按钮才可 enabled，标签精确为 `确认`；
- only enabled `确认` 才允许继续 backend security validation，然后才可发起 Windows `runas` / UAC；
- React/frontend 的 setTimeout/setInterval 只能用于显示，不能成为授权依据；backend 或等价可信单调计时必须维护 not-before/eligibility；
- pointer、keyboard、synthetic/repeated click、rerender、focus change、stale frontend state、旧 challenge replay 均不能提前授权；
- 取消、Esc、close/dismiss 无 PermissionMode/Broker/UAC 副作用；
- fresh open 重新计满 9000ms，不允许 remember/skip/don't-show-again；
- `--background` preference restore 不显示 warning、不 UAC；
- Broker 已 Active 时，单纯重选当前已激活管理员模式不得重复 UAC；
- AI/MCP 不能批准该弹窗、批准 UAC、修改 PermissionMode、启用 Broker 或修改授权 policy；
- High/Critical 单操作确认与本模式进入警告是两个独立 Gate，后者不能替代前者。

该 9 秒是**进入管理员授权流程前的安全等待期**，不是管理员模式 TTL；Broker Active 后仍按 capability/risk policy 工作。

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
