# 10 — Risk Register v9

## R-001 Upstream tool-surface mismatch — HIGH

公开 upstream 与 GPT-WebCodex vendored 版本不一致。

Mitigation：

- LB-000 capability snapshot；
- ADR；
- policy versioning。

## R-002 Windows command filesystem authority — HIGH

Full 模式 child process 拥有当前 Windows 用户 OS 权限。

Mitigation：

- 不宣称 OS sandbox；
- Edit deny process-exec；
- Full 明示风险；
- control-plane always deny。

## R-003 Guard 演变成半套 MCP Server — HIGH

若过早锁死 Rust Guard，可能需要承担 session/transport/notifications 等复杂协议语义。

Mitigation：

- LB-000 PEP go/no-go；
- upstream-first；
- adapter-second；
- Guard-last。

## R-004 Windows reparse escape — HIGH

junction/symlink/reparse 可能绕过 workspace path policy。

Mitigation：

- LB-000 adversarial spike；
- canonicalization contract；
- regression tests。

## R-005 Portable Python late failure — HIGH

系统 Python 能跑不代表 Windows Embeddable Python 能跑。

Mitigation：

- LB-000 feasibility；
- LB-006 直接使用最终 portable 模型；
- LB-018 只负责 bundle，不首次验证 runtime。

## R-006 Process ownership / zombie — HIGH

PID-only 管理可能误杀、漏杀或 zombie。

Mitigation：

- Job Object；
- process generation；
- stale-state reconciliation；
- PID reuse tests。

## R-007 Restart storm — MEDIUM

bad auth/config 被错误分类为 transient。

Mitigation：

- typed retryability；
- bounded backoff；
- max attempts。

## R-008 Secret leakage — HIGH

Runtime Key 经 args/log/error 泄露。

Mitigation：

- secret store；
- verified injection；
- no CLI key；
- redaction tests。

## R-009 Workspace switch partial commit — HIGH

提前更新 active workspace 可造成 UI/runtime 不一致。

Mitigation：

- desired/candidate/active；
- two-phase commit；
- rollback。

## R-010 ChatGPT deep-link drift — MEDIUM

Apps/MCP 页面 URL 可能变化。

Mitigation：

- 单一 Rust allowlisted constant；
- system browser；
- failure non-blocking；
- release smoke。

## R-011 External test instability — MEDIUM

真实 Tunnel/API/ChatGPT 配额使 CI 不稳定。

Mitigation：

- deterministic fake-sidecar suite；
- live 仅 LB-000/LB-019。

## R-012 Scope creep — MEDIUM

产品变成 IDE / Chat wrapper。

Mitigation：

- PRODUCT_SCOPE；
- minimal-ui skill；
- machine-readable PR contracts。


## R-013 Privileged Broker IPC impersonation — CRITICAL

高权限 Broker 若 IPC 认证不足，会变成本地提权入口。

Mitigation：

- Named Pipe/local IPC；
- SID/ACL；
- generation nonce；
- typed protocol；
- replay/stale/malformed adversarial tests；
- no network listener。

## R-014 Whole-app elevation regression — HIGH

开发者可能为“省事”把整个 Tauri App 提权。

Mitigation：

- AGENTS hard rule；
- manifest/release test；
- process token verification；
- Broker-only elevation。

## R-015 Silent startup vs UAC conflict — MEDIUM

Elevated 偏好若在开机时自动触发 UAC，会破坏静默后台。

Mitigation：

- preference 与 active runtime 分离；
- background only enters Requested；
- UAC only from explicit visible `管理员模式` selection/reselection；禁止 separate enable-admin button。

## R-016 No-TTL Elevated persistence — HIGH

用户选择不自动失效意味着 Broker 可持续保持管理员能力。

Mitigation：

- 仅用户主动启用；
- 清晰状态；
- 一键关闭；
- App exit/Windows session end 停止；
- control-plane 永远拒绝；
- Broker IPC 强认证；
- privileged capabilities reviewed allowlist。


## R-017 Runtime self-update policy drift — HIGH

第三方 runtime 若独立升级，可能新增工具/capability 并绕过已验证 policy。

Mitigation：

- runtime 不自更新；
- exact manifest；
- 整包升级；
- upgrade PR 重新跑 compatibility/security suite。

## R-018 System Python contamination — MEDIUM

若正式 runtime fallback 到 PATH Python，会产生版本/依赖/用户 site-packages 漂移。

Mitigation：

- absolute bundled Python path；
- no fallback；
- no pip-at-runtime；
- clean machine test。

## R-019 Bundled WebView2 size explosion — MEDIUM

错误打包 WebView2 会显著增加 installer。

Mitigation：

- Windows 11 only；
- system WebView2；
- packaging test 检查无 Fixed/Offline payload。

## R-020 elevated_exec blast radius — CRITICAL

通用管理员命令具有最高主机影响范围。

Mitigation：

- Elevated only；
- explicit UAC；
- Broker-only；
- tools/call PEP；
- structured program/args；
- no default shell=true；
- timeout/cancel/output limits；
- control-plane deny always；
- user-controlled enable/disable。


## R-021 Privilege UI state ambiguity — HIGH

如果 Dashboard 只显示用户选择的 `Elevated`，却不显示 Broker 实际是否 Active，用户可能误判 AI 当前是否拥有管理员权限。

Mitigation：

- Dashboard 同时显示 PermissionMode 与 PrivilegeState；
- Rust 是唯一 runtime truth；
- Broker crash/disable 实时更新；
- 不把“已选择 Elevated”等同于“管理员已启用”。

## R-022 Internal terminology leakage — MEDIUM

内部英文 enum、Broker/Runtime 等工程术语若直接显示，会降低易用性并造成界面风格不一致。

Mitigation：

- 独立 presentation/i18n mapper；
- 冻结中文术语表；
- UI snapshot/text tests；
- 禁止组件直接渲染 enum/fault code。

## R-023 Documentation-only governance drift — HIGH

规则只存在于文档时，长期开发容易绕过。

Mitigation：

- verify scripts（针对行为，不针对源码文本）；
- CI hard gate；
- adversarial fixtures。

## R-024 Upstream replacement lock-in — HIGH

Domain 若依赖上游私有结构，未来替换 runtime 会扩散到整个项目。

Mitigation：

- stable adapter boundary；
- compatibility baseline；
- adapter contract tests。

## R-025 Persistence migration loss — HIGH

settings schema 演进若无迁移会导致用户配置丢失或不可恢复。

Mitigation：

- schema_version；
- sequential migration；
- atomic replace；
- rollback fixtures；
- future-schema fail-safe。

## R-026 Release provenance loss — MEDIUM

长期维护后无法确认安装包由哪些依赖/源码组成。

Mitigation：

- lockfiles；
- runtime manifest；
- SBOM；
- THIRD_PARTY_NOTICES；
- exact commit/checksum；
- release acceptance report。

## R-027 Task summary secret leakage — HIGH

raw tools/call 参数可能包含密钥、令牌或敏感命令参数。

Mitigation：safe task summarizer + secret redaction + length limit；不安全 payload 使用泛化摘要。

## R-028 Activity feed scope creep — MEDIUM

最近活动会逐渐形成日志/消息系统，增加 UI、隐私、存储和维护成本。

Mitigation：v0.1 只允许单一 backend CurrentTaskStatus；无历史 UI；terminal 后 domain 回到 Idle，UI 映射固定为 `等待命令`。

## R-029 False model-intent projection — MEDIUM

根据模型文字猜测“正在执行什么”会造成状态失真。

Mitigation：只使用真实 MCP/Broker execution events；没有工具调用时 backend 保持 Idle，UI 必须映射为 `等待命令`，frontend 不得伪造。

## R-030 Credential plaintext persistence — CRITICAL

Runtime API Key 如果进入 settings、日志、diagnostics、browser storage 或 CLI，会扩大凭据泄漏面。

Mitigation：

- Windows secure CredentialStore；
- no plaintext fallback；
- CLI secret forbidden；
- automated secret scans；
- redaction；
- metadata-only settings。

## R-031 Remembered projects become authorization set — CRITICAL

如果“项目列表”被错误转换为 MCP 多根授权集合，切换便利功能会扩大文件访问范围。

Mitigation：

- registry 与 active workspace 分离；
- max authorized roots = 1；
- PEP 只消费 active workspace；
- architecture contract tests。

## R-032 Project removal deletes user files — CRITICAL

如果 UI 的“移除项目”被实现成文件系统删除，会造成灾难性数据损失。

Mitigation：

- remove operation domain 只删除 metadata；
- filesystem delete on removal deny-always；
- adversarial tests verify disk tree unchanged；
- UI 明确当前项目移除不会删除文件。

## R-033 Active removal silently authorizes another project — HIGH

自动选下一个 remembered project 会在没有明确用户动作时改变授权根。

Mitigation：

- active removal → NoActiveWorkspace；
- no auto-selection；
- user explicitly selects next project。

## R-034 UI thread blocked by backend lifecycle — HIGH

同步 Tauri command 若在 WebView/UI 事件线程等待 process、workspace switch、credential、UAC、readiness/recovery，可能导致前端无响应。

Mitigation：frontend projection-only；backend worker/async execution boundary；onboarding/startup 状态机归 Rust；人为慢 backend responsiveness Gate。

## R-035 Cloudflared distribution dependency drift — MEDIUM

当前 tunnel-client 兼容/运行资产包含 cloudflared 能力；若继续打包会引入不再需要的第三方 runtime、体积和额外生命周期/供应链面。

Mitigation：LB-018 最终 bundle/runtime manifest/installer/launcher/fallback 明确禁止 cloudflared/Cloudflare managed tunnel；历史 compatibility 证据只读保留；packaging fail-closed regression Gate。
