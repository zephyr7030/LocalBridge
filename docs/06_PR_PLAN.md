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

Tray、close-to-hide、`--background`、退出清理。

退出顺序需包含：

```text
Tunnel
→ privileged call gate close
→ Broker
→ PEP
→ MCP
```

## LB-014 — Autostart + Single Instance

后台开机、single instance、manual stop。

若权限偏好为 Elevated：

- 不自动弹 UAC；
- privilege runtime = Requested；
- 普通 runtime 仍可后台 Ready。

## LB-015 — UI Shell

单 Dashboard、settings、diagnostics入口、recent workspace、三档权限。

同时冻结全产品共享的按钮与提示基础层：

- 主/次/ghost 按钮使用一致的几何、状态和层级语言；
- 白色或近白背景上的次级按钮必须保持清晰可辨，禁止白底白按钮；
- 自解释操作不重复堆叠说明，只保留最小必要提示；
- 状态反馈不得造成布局位移。

Dashboard 必须直接显示：

- ChatGPT / Tunnel 状态；
- Coding Runtime 状态；
- 当前权限模式；
- **管理员权限实际运行状态**。

管理员状态至少支持：

```text
未启用
等待授权
等待 UAC
已启用
故障
```

管理员模式 UI 只保留必要操作：

- `启用管理员权限`
- `关闭管理员权限`
- 故障时复用现有 `查看` / diagnostics 入口

不加入时间选择器，不显示 Broker 内部技术字段。


### LB-015 额外 UI 语言硬要求

- 所有用户可见文案使用简体中文；
- 专业缩写/专有名词除外；
- domain enum 不直接渲染；
- 建立统一 presentation/i18n mapper；
- 主控界面不得出现 Dashboard / Settings / Diagnostics / Elevated / Broker / Runtime 等无必要英文。

## LB-016 — First-run Wizard

严格 6 屏：

```text
欢迎
→ OpenAI
→ 项目与权限
→ Local Bridge 设置
→ Local Bridge 使用确认
→ 启动检查
```

- Screen 2 字段：`Tunnel ID` / `Runtime API Key`；
- Screen 3 项目与权限合并，新项目以原生 Windows 文件夹选择器为主交互；
- Screen 4 用户可见术语统一为 `Local Bridge`，只允许系统默认浏览器打开 `https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`；
- Screen 4 不使用 WebView，前端不得传入任意 URL；
- Screen 5 只做最小必要的 Local Bridge 完成/使用引导，不伪造 ChatGPT 状态；
- 若展示/复制 connector endpoint，只能使用 Rust typed projection 的已验证 Tunnel/control-plane metadata，禁止根据 Tunnel ID 推导；
- 复制成功反馈不得造成布局位移；
- Screen 6 只检查本地运行环境、编码服务、OpenAI Tunnel，三项全绿前 `确定` disabled，不自动跳转；
- Screen 6 全绿后完成提示严格为 `配置完成，在插件中选择刚刚添加的Local Bridge试试吧`；
- 主窗口默认 900×620、最小 720×500 且可缩放；向导随 viewport 高度响应，主体可滚动，禁止固定 500/540px 卡片最小高度；
- native window resize/maximize 后，主 WebView bounds 必须等于完整 client area，Dashboard 与 onboarding 必须随 live viewport 重排；
- LB-016 必须有真实 Windows Tauri/WebView2 resize E2E：至少两个 native size + maximize，交叉验证 Tauri `inner_size()`、live JS viewport×DPR 与 Dashboard/onboarding rect；静态 CSS/Tauri 配置检查不得单独作为 PASS；
- 权限包含编辑/完整/管理员三档；向导不自动触发 UAC。

## LB-017 — Diagnostics

最小诊断 + redacted export。

增加：

- Broker selected/active/faulted；
- Broker generation；
- IPC health；
- 不显示 nonce/secret。

## LB-018 — Runtime Packaging

打包：

- Python Embedded；
- coding-tools-mcp；
- tunnel-client；
- privileged-broker；
- SHA256/provenance；
- installer；
- install/uninstall cleanup。

必须：

- Windows 11 x64 only；
- 不打包 WebView2；
- 不实现 runtime self-update；
- 不实现 app auto-updater；
- 生成安装包/安装后体积报告；
- installer >100 MiB 或 installed >250 MiB 时必须归因分析。

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
