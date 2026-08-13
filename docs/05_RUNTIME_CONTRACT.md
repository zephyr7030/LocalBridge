# 05 — Runtime Contract

## Desired / Candidate / Active

Workspace 状态必须分离：

```text
desired_workspace
candidate_workspace
active_workspace
```

仅在完整 Runtime Ready 后：

```text
candidate → active
```

不得在启动前提前覆盖 active。

## RuntimeState

```rust
enum RuntimeState {
    Stopped,
    StartingMcp,
    WaitingMcpReady,
    StartingPolicyEnforcement,
    WaitingPolicyReady,
    StartingTunnel,
    WaitingTunnelReady,
    Ready,
    Recovering { component: Component, attempt: u32 },
    SwitchingWorkspace { from: PathBuf, candidate: PathBuf },
    Faulted(RuntimeFault),
}
```

## RuntimeFault

最小 typed faults：

```text
WorkspaceMissing
WorkspaceInvalid
RuntimeMissing
RuntimeChecksumMismatch
ProcessOwnershipFailed
McpSpawnFailed
McpHealthTimeout
McpExited
PolicyBindFailed
PolicyInvalid
PolicyCapabilityUnknown
TunnelIdMissing
RuntimeKeyMissing
SecretStoreFailed
SecretInjectionUnsupported
TunnelAuthFailed
TunnelSpawnFailed
TunnelHealthTimeout
TunnelExited
PortUnavailable
ConfigurationInvalid
UserStopped
Unknown
```

禁止把所有错误折叠为“连接失败”。

## Start

前置：

- workspace exists；
- runtime manifest valid；
- vendor checksums valid；
- credentials available；
- Tunnel ID present；
- capability policy valid。

顺序：

```text
MCP ↑
→ MCP readiness
→ Policy Enforcement ↑
→ Policy readiness
→ Tunnel ↑
→ Tunnel readiness
→ Ready
```

任何阶段失败：

- 清理由本次 generation 创建的后续组件；
- 保留 typed fault；
- 不假装 Ready。

### Foreground Configured Launch

`onboarding_complete=true` 且 active workspace、Tunnel ID、Runtime API Key metadata 有效时，普通前台 UI 启动必须自动**异步**发起 runtime Start。窗口创建/交互不得等待 Start 完成；Starting/Ready/Fault 只经 backend typed projection 反映。`开机启动` 只控制 OS login launch，不得阻止手动前台启动后的 runtime 自动启动。

所有可能阻塞的 runtime/process/credential/UAC/recovery 工作必须运行于 backend worker/async execution context；WebView/UI event thread 不得执行长生命周期工作。

## Stop

```text
manual_stop = true
→ Tunnel ↓
→ Policy Enforcement ↓
→ MCP ↓
→ state = Stopped
```

Supervisor 不得再次自动拉起。

## Workspace Switch

```text
validate(candidate)
→ state = SwitchingWorkspace
→ Tunnel ↓
→ Policy Enforcement ↓
→ MCP ↓
→ MCP(candidate) ↑
→ PEP(candidate) ↑
→ Tunnel(candidate) ↑
→ Ready
→ active_workspace = candidate
```

失败：

- candidate 不 commit；
- 保留 previous active metadata；
- 可显式 rollback。
- candidate cleanup fault、rollback fault、rollback cleanup fault 必须分别保留为 typed 状态，不得吞掉 cleanup 后伪装成正常 `Stopped`。

成功的显式 workspace switch 是 recovery generation 边界：旧 workspace 的 outage、pending/exhausted retry budget、stable timer 与 user-attention 状态全部退役；新 workspace 后续发生故障时必须获得独立的新 generation 和完整 5 次恢复额度。切换失败且**干净 rollback 并实际恢复到 Ready** 的旧 workspace 才可继续旧 generation，不重置其已消耗额度。若 candidate cleanup、rollback 或 rollback cleanup 失败，或切换错误后运行时没有恢复到 Ready，则必须 fail-closed：旧 pending/outage/retry generation 全部退役，保持 typed Faulted/Stopped，禁止 watchdog 自动重启；需要新的显式用户控制动作重新建立授权 workspace/runtime。

## Process Ownership

不得仅靠 PID。

至少跟踪：

- role；
- pid；
- generation；
- process start identity；
- parent/job ownership；
- started_at。

Windows 首选 Job Object。

LB-004 必须测试：

- KILL_ON_JOB_CLOSE；
- PID reuse；
- stale state；
- nested child；
- forced kill；
- graceful timeout。

## Readiness

Process alive ≠ Ready。

MCP：

- 使用真实 protocol readiness / supported health。

Tunnel：

- 使用官方 tunnel-client readiness/health。

## Recovery

transient：

```text
1s, 2s, 5s, 10s, 30s
```

连续 5 次失败：

```text
Faulted
```

稳定 60s 后 reset retry counter。

认证/配置/校验类 fault：

```text
non-retryable
```

## Deterministic Test Contract

Orchestrator/recovery 测试默认使用 fake sidecars：

- fake-mcp；
- fake-policy；
- fake-tunnel。

可控制：

- delayed ready；
- crash；
- exit code；
- invalid health；
- auth fault；
- hang；
- child process spawn。

真实 OpenAI 只用于 LB-000 / LB-019。


## PrivilegeState

Elevated 与基础 RuntimeState 分离：

```rust
enum PrivilegeState {
    Disabled,
    Requested,
    AwaitingUac,
    Active { broker_generation: GenerationId },
    Faulted(PrivilegeFault),
}
```

不加入 timer / expires_at / TTL。

### 启用

```text
visible user selects/reselects Elevated
→ Requested
→ immediately validate broker source/security boundary
→ AwaitingUac
→ Windows UAC / runas
→ Broker handshake
→ Active
```

### 关闭

```text
user disables Elevated
→ privileged call gate closes
→ Broker stops
→ Disabled
```

### Background Startup

如果用户上次选择 Elevated：

```text
permission preference = Elevated
privilege runtime = Requested
```

但：

- 不自动弹 UAC；
- 不假装 Active；
- 普通 MCP/Tunnel 可继续后台 Ready；
- 用户打开控制中心后点击/重新点击管理员模式即主动完成 UAC；禁止额外“启用管理员权限”按钮。

### Elevated Call

```text
MCP tool
→ capability policy
→ requires elevated
→ PrivilegeState == Active ?
   yes → Broker
   no  → typed ElevationRequired
```


## Python Runtime Contract

Coding Tools Runtime 的正式启动必须来自：

```text
<install>/runtime/python/python.exe
```

或 manifest 指定的等价 Embedded Python launcher。

禁止 fallback 到：

```text
python.exe from PATH
py.exe
system site-packages
user site-packages
```

如果 bundled Python 缺失或 checksum 不匹配：

```text
RuntimeMissing
RuntimeChecksumMismatch
```

并停止启动。

## ElevatedExec Contract

```text
tool call
→ PEP capability = elevated_exec
→ review actual program / args / workdir
→ exact reviewed action profile ?
→ mode == Elevated
→ PrivilegeState == Active
→ Broker typed request
→ process result
```

任一前置不满足时 fail-closed。

`elevated_exec` 是 generic structured gateway，不等于 arbitrary administrator shell/program execution。v0.1 只允许明确登记并可机器验证的 reviewed action；shell、解释器、任意注册表/服务/任务修改器和无法证明不触及 LocalBridge control-plane 的请求在 Broker 启动前拒绝。当前任务投影与普通 MCP 调用共享单一 execution gate，多个管理员执行不得并发覆盖同一个 `CurrentTaskStatus`；取消请求走独立控制通道。

没有 TTL / expires_at。


## Dashboard Projection Contract

主控界面必须同时收到：

```text
PermissionMode
PrivilegeState
```

两者语义不同：

```text
PermissionMode = 用户选择的策略档位
PrivilegeState = Broker 实际生命周期状态
```

允许组合：

```text
Edit     + Disabled
Full     + Disabled
Elevated + Requested
Elevated + AwaitingUac
Elevated + Active
Elevated + Faulted
```

Dashboard 状态不能仅由 PermissionMode 推断。

Broker crash：

```text
Active
→ Faulted
```

UI 必须立即从“已启用”切换为“故障”。

Broker 正常关闭：

```text
Active
→ Disabled / Requested
```

具体取决于用户是否仍选择 Elevated。

## Current Task Status Contract

真实工具调用产生：

```text
request
→ allowed / blocked
→ running
→ terminal
→ idle
```

主控界面只消费 backend `CurrentTaskStatus | Idle`；frontend 不维护平行任务状态。

- Idle / 无活动任务 → `等待命令`（必须可见，不得显示“空闲”）
- PEP deny → `已阻止`
- 等待 UAC → `等待授权`
- process 已启动 → `执行中`
- terminal → 最终回到 `等待命令`

任务摘要不得成为 secret 泄漏通道。

## NoActiveWorkspace Contract

“没有当前项目”是正常应用状态，不是 runtime fault。

在该状态：

```text
MCP      stopped
PEP      stopped
Tunnel   stopped
App      running
Tray     running
```

用户选择项目后才启动运行链。

移除当前项目必须先停止 Tunnel → PEP → MCP，再清理 active workspace。

不得自动切换到另一个 remembered project。

## Runtime Secret Contract

Runtime API Key：

```text
CredentialStore
→ transient in-memory retrieval
→ supported secret injection
→ target process/auth
```

禁止：

```text
command-line argument
plain settings
logs
diagnostic export
```

若 tunnel-client 的真实兼容性只允许不安全 CLI secret：

```text
SecretInjectionUnsupported
```
