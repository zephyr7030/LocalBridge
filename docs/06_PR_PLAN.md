# 06 — PR Plan v12

原则：单智能体、单 PR、严格顺序、验收后暂停。

## LB-000 — Upstream + Security Compatibility Spike

在既定 LB-000 合同基础上额外记录：

- 哪些 capability 可能需要管理员权限；
- Docker/WSL/Podman 等高权限外部 runtime；
- Broker 对未来 capability policy 的接口要求。

不实现正式 Broker。

## LB-001 — Repository Bootstrap + Packaging Smoke

Tauri 2 + React + TS + Vite，包含 dummy sidecar 打包 smoke。

## LB-002 — Domain Contracts

实现：

- Settings；
- desired/candidate/active workspace；
- RuntimeState / RuntimeFault；
- PermissionMode；
- Capability；
- `PrivilegeState / PrivilegeFault`；
- ComponentStatus。

不 spawn。

## LB-003 — Settings & App Data

versioned settings、migration、recent workspace、atomic app-data。

权限偏好可记录 `Edit / Full / Elevated`，但不得记录任何管理员密码/token。

## LB-004 — Windows Process Supervisor

Job Object / process generation / stale PID / child tree。

## LB-005 — Credentials

Windows credential backend；Runtime Key 不进 settings/log/args。

## LB-006 — Portable Coding Tools Runtime

最终 Windows Embeddable Python + pinned coding-tools-mcp。

硬性要求：

- 所有测试直接使用 bundled Embedded Python；
- 不允许系统 Python fallback；
- 不允许首次运行 pip install；
- runtime 可离线启动。

## LB-007 — MCP Policy Enforcement

capability-based PEP：

- Edit fail-closed；
- Full reviewed coding allowlist；
- Elevated capability routing contract；
- LocalBridge control-plane deny always；
- tools/call mandatory enforcement。

本 PR 不实现管理员 Broker。

## LB-008 — Tunnel Runtime

官方 pinned tunnel-client、checksum、secret injection、health。

## LB-009 — Orchestrator Core

只实现 MCP → PEP → Tunnel → Ready 的普通权限启动/停止状态机。

## LB-010 — Recovery + Workspace Switch

bounded recovery、two-phase workspace switch、rollback。

## LB-011 — Privileged Broker IPC Foundation

只做高权限边界，不做业务管理员命令。

实现：

- 独立 Broker binary；
- Windows UAC launch；
- Named Pipe / ADR 选定 IPC；
- SID/ACL；
- generation nonce；
- typed handshake；
- lifecycle；
- no network listener；
- malformed/replay/stale-session adversarial tests。

## LB-012 — Elevated Permission Mode

实现：

- Edit / Full / Elevated 用户语义；
- `PrivilegeState`；
- user enable/disable；
- 无 TTL/时效；
- background startup 不自动 UAC；
- reviewed privileged capabilities；
- 正式 `elevated_exec`；
- structured program/args；
- timeout/cancel/output-limit；
- Broker policy；
- elevated call routing；
- Full/Enhanced control-plane deny。

## LB-013 — Tray + Background

- 增加版本化持久化 `关闭窗口后继续运行`（允许该字段所需的 settings/schema/migration 窄例外）；true：X=hide、runtime/tray 不变；false：关闭 privileged gate、清理 Broker/Tunnel/PEP/MCP 后退出。
- 建立 UI/backend 非阻塞执行边界：可能耗时 lifecycle 工作不得在 WebView/UI 事件线程同步运行。
- 保留真实 `--background` 无可见窗口与 tray exit cleanup。

## LB-014 — Autostart + Single Instance

- `开机启动` 仅控制 Windows 登录启动。
- onboarding 已完成且 active workspace/Tunnel ID/Runtime API Key metadata 有效时，普通前台 UI 启动自动异步启动 selected project/runtime/MCP/OpenAI Tunnel；UI 先可响应并反映 Starting/Ready/Fault。
- 后台恢复管理员偏好只 Requested，不自动 UAC；single-instance/wake/manual-stop 语义继续保留。

## LB-015 — UI Shell

- Dashboard “选择其他文件夹”统一使用原生 Windows 文件夹选择器，禁止手填绝对路径主流程。
- Dashboard 删除 `权限模式` 行以及编辑/完整/管理员三种模式按钮，只保留只读的管理员权限实际运行状态；Dashboard 不得修改 PermissionMode 或触发模式 UAC。完成 onboarding 后，三种权限模式按钮只存在于设置页“权限”：可见点击/重新点击管理员模式若未 Active 立即发起 UAC；禁止单独“启用管理员权限”按钮；离开管理员模式关闭 Broker。
- 左下 CurrentTask 单行无任务固定显示 `等待命令`，禁止“空闲”/隐藏；实际 MCP/Broker 生产调用必须端到端驱动 backend `CurrentTaskStatus` → UI，frontend 不伪造。
- 设置页严格为常规/连接/权限：常规=`开机启动`、`关闭窗口后继续运行`；连接=`Tunnel ID`、`Runtime API Key` 各自“更换”；权限=三模式；底部=`打开欢迎页`、`完成`。
- `Runtime API Key` 为精确英文用户字段名，不翻译；完整 secret 永不回显/预填。Tunnel ID 与 Runtime API Key 独立更新，保存即校验→安全写入→按需受控重连；禁止“测试连接”。
- React/WebView 只消费 typed backend projection + user intent；故意延迟 backend 操作时 UI 必须保持响应。

### LB-015 额外 UI 语言硬要求

中文优先继续成立，但 `Tunnel ID` 与 `Runtime API Key` 是冻结专有字段名例外。Dashboard/Settings/Diagnostics/Edit/Full/Elevated 等普通英文仍禁止。

## LB-016 — First-run Wizard

- 保持严格 5 屏、Screen3 runtime ready-before-Screen4、Screen4 固定插件创建合同、Screen5 Gate 与全部返回路径。
- Screen3 新项目仍用原生 Windows folder picker。
- 三个权限按钮结构高度必须 `min-height >= 80px` 或等效证明，能够容纳标题+两行说明+上下留白；900×620 人工视觉 Gate 仍必须 PASS。
- 可见选择/重新选择管理员模式即显式 UAC 动作；禁止单独 enable-admin 按钮，后台 preference restore 仍无 UAC。
- runtime start + readiness wait 移到 backend 状态机；React 只观察 typed projection，不持有 60 秒 polling/start orchestration。慢 backend 时 onboarding UI 必须响应。

## LB-017 — Diagnostics

固定最小诊断：

```text
运行状态: 本地运行环境 / 编码服务 / OpenAI Tunnel / 管理员权限
项目: 当前实际项目路径
日志: 最近、限量、脱敏用户事件
动作: 打开日志 / 导出诊断 / 完成
```

普通诊断 UI 禁止 Broker/reconnect generation、attempt counter、PID/SID/nonce/IPC，也删除刷新、重试连接、打开欢迎页。导出仍严格 redacted。

## LB-018 — Runtime Packaging

- 最终 LocalBridge bundle/runtime manifest/packaging inventory/installer/launcher/fallback 必须移除 `cloudflared.exe`、`cloudflared-manifest.json` 与 Cloudflare managed tunnel activation。
- `compatibility/**` 中历史 upstream 快照允许保留 cloudflared 事实作为不可执行审计证据，但不得复制到最终发行 runtime。
- 增加 fail-closed packaging Gate，防止 Cloudflare/cloudflared 被重新引入。
- 其余 bundled Python/coding-tools/tunnel-client、SBOM/notices/provenance/icon/zero-telemetry 合同保持。

## LB-019 — Release / Clean-machine / Reboot E2E

最终 Gate：

- clean Windows 11 x64；
- install；
- Wizard；
- real Tunnel；
- ChatGPT MCP call；
- Edit/Full；
- Elevated UAC；
-管理员操作 smoke；
- disable Elevated；
- close-to-tray；
- reboot silent autostart；
- Elevated preference 不自动 UAC；
- manual activation；
- crash recovery；
- uninstall；
- no orphan normal/elevated processes；
- clean machine 无系统 Python 也可运行；
- 零 telemetry network activity；
- runtime manifest 与 release 完全一致。

# Maintenance Responsibilities by Existing PR

## LB-000

额外必须：

- 填充 `COMPATIBILITY_BASELINE.json`；
- 生成首个 upstream structural/capability baseline；
- 明确 stable adapter 边界；
- 记录任何无法自动验证的兼容性假设。

## LB-001

额外必须：

- 建立 architecture verification runner；
- 建立 CI baseline；
- 验证 `ARCHITECTURE_RULES.json` 可被自动消费；
- 锁定 package/Cargo dependency lockfiles；
- 不实现业务功能。

## LB-003

额外必须：

- settings schema_version；
- sequential migration framework；
- migration atomicity；
- historical migration fixtures/tests；
- future-schema fail-safe。

## LB-018

额外必须：

- 生成 SBOM；
- 更新 THIRD_PARTY_NOTICES；
- 生成 release provenance；
- 检查 lockfiles/runtime-manifest/compatibility baseline 一致；
- 生成 artifact size report。

## LB-019

额外必须：

- release candidate → stable gate；
- clean-machine E2E；
- migration/rollback smoke；
- release provenance verification；
- compatibility baseline freeze；
- final acceptance report。


# Current Task Status Responsibilities

不增加 PR 数量。

## LB-002

定义稳定的 task kind / task execution state / current-task projection / safe summary contract，不依赖上游具体 tool id。

## LB-007

在 `tools/call` 权限判定周围产生真实任务状态。policy deny 必须显示“已阻止”，不能显示“执行中”。

## LB-009

Rust Core 维护唯一 `CurrentTaskStatus`，负责 terminal → Idle 清理，不建立用户活动历史。

## LB-012

管理员调用投影管理员操作、等待授权、执行中、已阻止/失败/取消。

## LB-015

主控界面增加固定“当前任务”状态区，只显示类型 / 任务 / 状态。

禁止最近活动、消息流、时间线或历史列表。


## LB-015 极简展示补充

当前执行状态必须为单行极简展示：

```text
●  运行测试  cargo test
```

- 无“当前任务”标题；
- 无类型/任务/状态字段名；
- 运行中绿色点脉冲；
- reduced-motion 下静态绿色点；
- 无“执行中”文字；
- 无历史/消息流。


# Credential & Project Registry Responsibilities

不增加 PR 数量。

## LB-003 — Settings & App Data

增加：

- WorkspaceRegistry persistence；
- zero-or-one active workspace reference；
- schema migration；
- validated identity / de-dup metadata；
- NoActiveWorkspace persistence semantics。

禁止把项目列表作为多根授权列表。

## LB-005 — Credentials

必须实现：

- Windows secure credential backend；
- Runtime API Key secure save/read/delete；
- no plaintext fallback；
- settings 只保存 reference/presence；
- UI save 后不回显；
- logs/diagnostics redaction；
- tests proving secret absence from normal persistence。

## LB-008 — Tunnel Runtime

必须：

- 从 CredentialStore 临时读取 secret；
- 使用 LB-000 验证过的安全注入方式；
- secret 不进入 CLI；
- 无安全注入方式时 `SecretInjectionUnsupported`。

## LB-010 — Recovery + Workspace Switch

扩展：

- add/select/remove project；
- registry de-dup；
- remove non-active metadata only；
- remove active: stop Tunnel → PEP → MCP → clear active → NoActiveWorkspace；
- no automatic fallback workspace authorization；
- current missing workspace handling。

## LB-015 — UI Shell

项目 UX：

- 当前项目 + `切换`；
- 已保存项目选择；
- `选择其他文件夹`；
- 非当前项目可直接移除；
- 当前项目移除仅一次确认，明确“不会删除项目文件”；
- 无当前项目时显示 `选择项目`；
- 不新增复杂“项目管理中心”。

运行密钥 UX：

- 密码输入；
- 保存后只显示 `已保存`；
- 不提供明文显示按钮。
