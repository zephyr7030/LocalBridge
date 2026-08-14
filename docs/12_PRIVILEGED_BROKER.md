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

启用 Elevated 时的**当前 schema26 顺序**：

1. 用户在 Settings 或 onboarding Screen3 明确选择/重新选择 `管理员模式`；
2. 若 Broker 已 Active，仅保持当前状态，不因该次重选重复 UAC；
3. 若 Broker 未 Active，LocalBridge 先创建一次新的管理员安全确认 challenge，并显示固定后果警告；
4. 红色确认按钮从 `确认9` 开始，在完整 9000ms 内 disabled；到达可信 not-before 后仍保持红色、标签变为 `确认` 并才可点击；
5. 用户点击 enabled `确认` 后，LocalBridge 才执行既有 Broker 来源/路径/ACL 等安全校验；
6. 校验通过后才发起 Windows `runas` / 等价 UAC；
7. 用户由 Windows UAC 决定允许/拒绝；
8. 只有 Broker 得到管理员令牌。

固定安全确认内容：

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

管理员模式入口按钮统一橙色 `#ff9500`；弹窗**整个确认按钮**统一红色，而非只把数字染红。倒计时标签顺序固定：

```text
确认9 → 确认8 → 确认7 → 确认6 → 确认5 → 确认4 → 确认3 → 确认2 → 确认1 → 确认
```

前九个状态覆盖完整 9000ms 且全部 disabled。授权资格必须由 backend/等价可信单调计时维护，frontend timer 只能作为 display projection。提前 pointer/keyboard/synthetic/repeated click、rerender、focus change、stale frontend state 或旧 challenge replay 都必须 fail-closed。取消、Esc、close/dismiss 必须取消 challenge 且不得改变 PermissionMode、启动 Broker 或触发 UAC。fresh open 重新计满 9 秒，无 remember/skip/don't-show-again。

正式安装必须是机器级 `perMachine`，Broker 位于 Program Files 安装根。发起 `runas` 前，LocalBridge 必须 canonicalize 当前自身路径和 Broker 路径，只允许当前安装目录的精确 sibling `LocalBridge-Privileged-Broker.exe`。路径身份检查后还必须验证实际 Windows security descriptor：Broker、安装目录以及正式安装中一直到 Program Files 根的祖先目录 owner 必须属于受信任 system/admin installer principal，DACL 不得向其他 principal 授予任何内容写入、增加/删除子项、删除、改 DACL/owner 等 mutation right；未知/不可安全解释的 ACL 状态直接拒绝。随后再对当前未提权 token 做非破坏性逐项有效访问探测，只有每个危险 right 都明确 `ACCESS_DENIED` 才可继续；任一 mutation right 可获得或探测出现其他错误都 fail-closed。这样即使目录名处于 Program Files 下，只要其实际 ACL 可被低权限主体修改，也不能成为 UAC Broker 来源。当前 v0.1 不依赖 Authenticode 或 Broker 文件哈希，文档与产品不得暗示存在未实现的签名/hash binding。

禁止：

- LocalBridge 自行绕过 UAC；
- AI/MCP 批准管理员安全确认或批准 UAC；
- AI/MCP 修改 PermissionMode 或直接启用 Broker；
- 前端本地倒计时直接充当授权真相；
- Scheduled Task “最高权限”作为默认绕过方案；
- Windows Service 常驻管理员作为 v0.1 默认方案；
- 保存管理员密码；
- 自动模拟 UAC 点击。

---

# 不设管理员模式时效

产品不提供管理员模式 TTL：

- 15 分钟；
- 30 分钟；
- 60 分钟；
- 自动倒计时失效。

schema26 的 9 秒只属于**每次 fresh 管理员授权流程的安全确认等待期**，不是 Elevated 的有效期。

管理员模式由用户主动决定。

状态：

```text
Disabled
Requested
AwaitingConsent
AwaitingUac
Active
Faulted
```

用户只通过设置页“权限”或首次 onboarding 第 3 屏中的三种权限模式本身控制；Dashboard 不提供权限模式控件：

```text
点击/重新点击 管理员模式 + Broker 未 Active
→ 固定安全警告
→ 红色确认按钮完整 9 秒 disabled
→ enabled 确认
→ 安全校验
→ 显式 UAC / 激活 Broker

切换 编辑模式/完整模式
→ 关闭 privileged call gate / 停止 Broker
```

禁止额外“启用管理员权限”按钮。

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

但后台开机启动 **不得显示安全确认、不得自动弹出 UAC**。

用户打开控制中心后重新选择 `管理员模式` 时，若 Broker 未 Active，必须重新经过完整 schema26 9 秒安全确认；不得沿用上一次已看过警告的状态。

这样同时满足：

- 没有强制管理员模式时间限制；
- 用户自己决定；
- Windows 重启后不绕过 UAC；
- `--background` 仍可真正静默；
- 每次新的管理员激活流程都要求明确理解后果。

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
- consent challenge/confirmation；
- workspace；
- authorized roots；
- Runtime API Key；
- Tunnel ID；
- autostart；
- Broker policy。

High/Critical 单操作确认是独立安全 Gate；完成管理员模式 schema26 警告不等于自动批准任意后续高风险系统操作。

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

v0.1 暴露通用结构化管理员执行网关：

```text
elevated_exec
```

它是最高风险 capability，但**不代表任意管理员程序或任意 shell**。每个可执行文件/动作必须有明确 review profile；v0.1 初始 profile 仅包含受信任 Windows System32 `whoami.exe` 的窄只读身份查询动作，用于验证完整 Broker 路径。未来扩展必须逐项增加 contract + adversarial tests。

`elevated_exec` 必须：

- 仅在 Elevated + Broker Active；
- PEP 使用真实 `program / args / workdir` 明确 review 后 allow；
- 通过 PEP capability policy；
- arbitrary program、shell/interpreter、未审核 workdir/参数 fail-closed；
- 通过外部程序间接修改 LocalBridge control-plane 仍为 deny-always；
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
User selects Elevated
→ schema26 safety consent challenge
→ full 9000ms whole-red disabled countdown
→ user clicks enabled 确认
→ Broker source/security validation
→ UAC
→ Broker spawn
→ authenticated handshake
→ Active
```

离开 Elevated（用户选择 Edit/Full）：

```text
close privileged call gate
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
