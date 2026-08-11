# 12 — Privileged Broker Architecture

## 目标

LocalBridge 支持三种用户可见权限档：

```text
Edit
Full
Elevated
```

但内部不把整个 LocalBridge 提升为管理员。

内部模型：

```text
Edit
  = permission_mode: edit
  + elevation: disabled

Full
  = permission_mode: full
  + elevation: disabled

Elevated
  = permission_mode: full
  + elevation: enabled
  + privileged broker active
```

这样可避免把：

- Tauri UI；
- OpenAI Tunnel；
- coding-tools-mcp；
- 普通文件操作；
- 普通命令；

全部永久运行在管理员令牌下。

---

# 进程结构

正常：

```text
Windows User Token
      │
LocalBridge.exe
├─ coding-tools-mcp
├─ tunnel-client
└─ Rust Policy Enforcement
```

Elevated：

```text
Windows User Token
      │
LocalBridge.exe
├─ coding-tools-mcp
├─ tunnel-client
├─ Rust Policy Enforcement
│
└──── authenticated IPC ────► LocalBridge-Privileged-Broker.exe
                                  │
                                  │ Administrator Token
                                  ▼
                             privileged operation
```

Privileged Broker 必须是独立二进制/独立进程。

---

# UAC 规则

LocalBridge 主程序：

- manifest 不要求 Administrator；
- 正常启动不触发 UAC；
- 开机后台启动不触发 UAC；
- Tunnel/MCP 默认普通用户权限。

启用 Elevated 时：

1. 用户明确选择 `Elevated`；
2. LocalBridge 发起 Windows `runas` / 等价 UAC 提权；
3. 用户由 Windows UAC 决定允许/拒绝；
4. 只有 Broker 得到管理员令牌。

禁止：

- LocalBridge 自行绕过 UAC；
- Scheduled Task “最高权限”作为默认绕过方案；
- Windows Service 常驻管理员作为 v0.1 默认方案；
- 保存管理员密码；
- 自动模拟 UAC 点击。

---

# 不设管理员模式时效

产品不提供：

- 15 分钟；
- 30 分钟；
- 60 分钟；
- 自动倒计时失效。

管理员模式由用户主动决定。

状态：

```text
Disabled
Requested
AwaitingUac
Active
Faulted
```

用户可以：

```text
启用管理员模式
关闭管理员模式
```

`Active` 不因时间自动失效。

## 失活条件

不是“时效”，而是生命周期边界：

- 用户主动关闭 Elevated；
- 用户从托盘退出 LocalBridge；
- Privileged Broker 崩溃/退出；
- Windows 注销/重启；
- UAC 启动被拒绝。

这些情况之后：

```text
elevated_active = false
```

如果用户配置仍选择 Elevated，UI 可以显示：

```text
管理员模式已选择 · 需要重新授权
```

但后台开机启动 **不得自动弹出 UAC**。

用户打开控制中心后显式点击：

```text
启用管理员权限
```

才启动 Broker。

这样同时满足：

- 没有强制时间限制；
- 用户自己决定；
- Windows 重启后不绕过 UAC；
- `--background` 仍可真正静默。

---

# 权限面

## Edit

允许：

```text
read
search
reviewed workspace write
```

拒绝：

```text
process-exec
elevated-exec
control-plane
unknown capability
```

## Full

允许：

```text
Edit capabilities
process-exec
git
test
build
reviewed coding workflows
```

这些命令使用：

```text
current Windows user token
```

拒绝：

```text
elevated-exec
LocalBridge control-plane
unknown capability
```

## Elevated

允许：

```text
Full capabilities
reviewed privileged operations
```

管理员操作必须显式路由到 Broker。

即使 Elevated：

```text
LocalBridge control-plane = DENY
```

MCP 仍不能自行修改：

- permission mode；
- elevation setting；
- workspace；
- authorized roots；
- Runtime API Key；
- Tunnel ID；
- autostart；
- Broker policy。

---

# Broker API

第一原则：Broker 不成为一个“任意管理员 RPC 后门”。

优先提供 typed capabilities，例如：

```text
service_control
driver_install
driver_remove
system_registry_write
network_admin
package_install
privileged_file_operation
```

v0.1 正式支持任意管理员命令能力：

```text
elevated_exec
```

它是最高风险 capability，不是 Broker 的默认无条件 RPC。

`elevated_exec` 必须：

- 仅在 Elevated + Broker Active；
- PEP policy 明确 allow；
- 通过 PEP capability policy；
- 不允许 MCP 改写 Broker policy；
- 完整审计命令元数据；
- 日志 secret redaction；
- 明确返回 exit code/stdout/stderr；
- 不使用 shell=true 作为默认实现。

---

# IPC 安全

因为 Broker 是高权限进程，IPC 是核心安全边界。

不得只做：

```text
localhost TCP + 无认证
```

首选 Windows Named Pipe 或同等级本地 IPC。

最低合同：

1. 只允许当前 LocalBridge 用户 SID；
2. pipe ACL 最小化；
3. 每次 Broker generation 使用随机 nonce/session secret；
4. Broker 校验请求来自合法 LocalBridge session；
5. 请求具有 protocol version；
6. 消息长度有上限；
7. typed request schema；
8. unknown operation fail-closed；
9. Broker 不接受任意文件路径/命令，除对应 capability 明确允许；
10. Broker 不暴露网络 listener。

需在实现 PR 中验证：

- 低权限同用户进程伪造请求；
- 另一用户连接；
- stale session；
- replay；
- malformed request；
- oversized payload；
- Broker 重启后旧 nonce；
- LocalBridge crash 后 Broker 生命周期。

---

# Broker 生命周期

```text
User chooses Elevated
→ UAC
→ Broker spawn
→ authenticated handshake
→ Active
```

关闭 Elevated：

```text
disable elevation
→ stop accepting privileged calls
→ Broker graceful shutdown
→ forced cleanup if needed
→ elevation = Disabled
```

Tray Exit：

```text
Tunnel ↓
→ privileged request gate closed
→ Broker ↓
→ PEP ↓
→ MCP ↓
→ LocalBridge exit
```

Broker 不应比 LocalBridge 长期独立存活。

---

# Job Object 注意事项

普通 sidecars：

```text
MCP
Tunnel
```

优先由普通用户 Job Object 管理。

Elevated Broker 的 Job Object/ownership 需要独立验证，因为权限边界和 Windows Job assignment 可能不同。

不得假设低权限父进程一定可以任意控制高权限 Broker。

必须通过专门 ADR/测试确定：

- Broker process handle ownership；
- shutdown strategy；
- crash cleanup；
- Windows integrity-level restrictions。

---

# Docker / WSL / Container Runtime

Full 模式可调用：

```text
docker
podman
wsl
```

但这类工具可能拥有接近管理员的主机能力。

Compatibility/Policy 层应把它们识别为：

```text
privileged-external-runtime
```

而不是普通低风险 shell。

v0.1 不需要理解容器内部权限模型，但 UI/安全文档不得把 Docker 容器等同于 LocalBridge workspace sandbox。

---

# Windows AppContainer

LocalBridge v0.1 不运行于 Windows AppContainer。

当前安全模型是：

```text
普通 Win32/Tauri
+ Rust capability PEP
+ Windows ACL/UAC
+ Privileged Broker
```

若未来采用 AppContainer，需要单独架构评估，不作为现阶段兼容目标。


## elevated_exec 参数合同

首选 typed request：

```text
program
args[]
workdir
timeout
environment allowlist/overrides
stdin mode
```

禁止以单一拼接 shell string 作为内部 canonical representation。

输出：

```text
exit_code
stdout
stderr
duration
termination_reason
```

必须具有：

- stdout/stderr 大小上限；
- cancellation；
- timeout；
- secret redaction；
- process generation；
- Broker lifecycle ownership。
