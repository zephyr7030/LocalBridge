# 08 — Final Predevelopment Review

Review date: 2026-08-10  
Baseline: `LocalBridge-Dev-Preflight-v15-FINAL`  
Result: **PASS — ready for G0 / LB-000**

已冻结：

- Windows 11 x64；
- Tauri 2 + React/TS + Rust；
- bundled Python/coding-tools/tunnel-client；
- no external Python；
- no updater v0.1；
- zero telemetry；
- loopback-only；
- Edit / Full / Elevated；
- separate Privileged Broker；
- Runtime API Key secure store，no plaintext/CLI；
- project registry + zero/one active root；
- project remove never deletes files；
- true `--background`；
- wake-driven current execution row + one last-tool row, no history/feed；
- Apple-inspired UI using native CSS only，no visual dependency；
- auto reconnect exactly 5 attempts with 1/2/5/10/30s；
- no new reconnect UI until exhaustion；
- 20 PRs / 5 groups；
- mandatory independent adversarial review between groups；
- G3→G4 additionally requires a human manual/detail review after adversarial PASS；
- during that human Gate, user/executor factual claims are challengeable evidence rather than automatic truth；
- executor pre-authorization is permitted only as a concrete recorded authorization and must be user-audited PASS before the human Gate can PASS.

LB-000 仍实证决定：

- actual coding-tools capability surface；
- PEP implementation；
- tunnel secure secret injection；
- Job Object；
- reparse/junction；
- real Tunnel compatibility。

开发入口（历史 predevelopment snapshot）：

```text
current_group = G0
current_pr    = LB-000
```

实时执行入口不得从本文件上述历史快照读取，只能读取 `PR_INDEX.json` + `PROJECT_STATE.json`。当前 schema25/schema26 返工入口保持 `G2 / LB-006`。

## Current UI freeze

以下为当前有效 UI 合同；其中 schema26 管理员模式安全确认明确覆盖任何历史 amber/yellow admin-mode 或 direct-UAC 语义：

- onboarding = exactly 5 screens: 欢迎 → OpenAI → 项目与权限 → 创建自定义插件 → 启动检查; there is no screen 6 and the former `Local Bridge 使用确认` page is removed;
- 1/5 = `简单设置 即可开始`; screen 1 is the only screen without a back action;
- OpenAI = screen 2 with `Tunnel ID` and `Runtime API Key` labels and an explicit back action; persisted Tunnel ID is prefilled; a saved key displays exact `已安全保存至windows安全凭据`, focus uses only same-length `*` generated from backend length metadata, plaintext is never returned, and the only helper sentence is `Runtime API Key 仅保存在 Windows 安全凭据中。`;
- workspace + permission = screen 3 and uses the native Windows folder picker as the primary new-project interaction; any title+description two-line permission button must have actual rendered height at least 2× the ordinary single-line control at 780×620, with both line boxes complete; static CSS markers do not constitute PASS and human 780×620 visual acceptance remains required; ordinary selected state uses blue `#0071e3`, all administrator-mode controls use orange `#ff9500` warning semantics in onboarding and Settings; screen 3 has an explicit back action;
- when Broker is not Active, visible administrator-mode selection/reselection opens the fixed schema26 safety warning before any UAC/runas or mode/Broker activation side effect; the whole confirmation button is red, disabled from `确认9` through `确认1` for a full trustworthy/monotonic 9000ms, then remains red and becomes enabled with exact label `确认`; only enabled confirmation may proceed to backend security validation and then Windows UAC;
- cancel/Escape/close/dismiss causes no PermissionMode/Broker/UAC side effect; each fresh warning opens with a fresh 9000ms countdown; no remember/skip bypass; background preference restore shows no warning/UAC; Broker-Active reselection causes no duplicate UAC; High/Critical per-operation confirmation remains separate;
- after screen 3 saves project and permission, it starts selected project/runtime/MCP/OpenAI Tunnel and reaches real all-ready before screen 4; the unique runtime startup edge must not be deferred until screen 5;
- screen 4 = `创建自定义插件`; exact developer-mode hint; `打开 ChatGPT插件设置` is in the left-side action flow and is an argument-free frontend action backed by a Rust fixed allowlist + system browser for `https://chatgpt.com/plugins#settings/Plugins`; directly beneath it display `打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件`;
- screen 4 has exactly two information rows `名称 = Local Bridge` / `Tunnel ID = current persisted saved value`, no `本地服务`; each row has independent stable green `已复制` feedback for exactly 3 seconds with no layout shift;
- `打开插件管理页` is also in the left-side action flow and is an argument-free frontend action backed by a Rust fixed allowlist + system browser for `https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`; both browser actions forbid WebView and arbitrary/frontend-provided URLs; screen 4 has explicit `返回` and `继续`;
- screen 5 is `启动检查`, containing only 本地运行环境 / 编码服务 / OpenAI Tunnel; status dots consume the same typed status source as Dashboard and map Ready=green, Starting=amber/yellow, Fault=red, Unknown=gray;
- screen 5 confirm is disabled and completion hint hidden until all three checks are green; completion hint is exactly `配置完成，在插件中选择刚刚添加的Local Bridge试试吧`; no auto-advance; screen 5 has an explicit back action to screen 4;
- screens 2/3/4/5 all have explicit back paths; save/start/configuration failures must never trap the user;
- ordinary primary/selected/product-accent UI uses original blue `#0071e3`; black is not the ordinary product accent; administrator mode is the orange `#ff9500` warning exception; Starting status dots and Dashboard restart-service retain their separate amber/yellow semantic;
- Dashboard primary service status dots use the same typed source and Ready/Starting/Fault/Unknown color semantics as onboarding; independent conflicting state is forbidden;
- main window is fixed at 780×620; minimum and maximum inner size are both 780×620;
- `resizable=false` and `maximizable=false`; ordinary user interaction cannot change the main-window size;
- native Windows decorations are disabled; exactly one custom edge-to-edge chrome fills the client area and provides drag/minimize/close without maximize or a double frame;
- Dashboard/onboarding must remain complete and operable inside that fixed 780×620 client area; no resize/maximize responsive E2E is required;
- onboarding itself is a full-page single-content layout inside the custom chrome content area; a centered floating wizard card/modal/dialog surrounded by a large empty canvas is forbidden; the administrator warning is only a narrowly-scoped safety-consent dialog and does not permit the wizard shell itself to regress;
- user clicks screen-5 confirm to enter main UI;
- buttons use one coherent visible affordance system; white-on-white ambiguous controls and layout-shifting copy feedback are forbidden.

## Schema26 fixed administrator warning

The visible warning copy is exact and must not be expanded/reworded by implementation agents:

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

The entire confirmation button is red. It remains disabled for the complete 9000ms interval while labels progress `确认9` → … → `确认1`; after the not-before condition is satisfied it remains red, becomes enabled, and displays exactly `确认`. Frontend timers are presentation-only; backend/equivalent trustworthy monotonic eligibility must reject early/stale/replayed confirmation. AI/MCP cannot approve the dialog, UAC, PermissionMode or Broker activation.

Frozen brand icon:

- `assets/icons/localbridge.png` — 1024×1024 RGBA;
- `assets/icons/localbridge.ico` — 16/24/32/48/64/128/256 px;
- Windows app, installer and tray use this asset;
- no icon-library dependency or placeholder replacement is permitted.

## G3 human-review amendment — generation 2 (historical provenance, superseded where schema26 conflicts)

This section preserves prior G3 provenance but schema26 above supersedes any conflicting administrator-mode color/direct-UAC wording.

- effective rework entry is `LB-014`; G4/LB-018 remains blocked until required re-execution/review under current authority is complete;
- `关闭窗口后继续运行` is a persisted setting: enabled = close hides and keeps runtime/tray, disabled = orderly runtime/Broker cleanup then exit;
- configured normal foreground launch is UI-first: create/show an interactive UI, emit one typed `UI-ready`, then backend asynchronously starts any stopped selected project/runtime/MCP/OpenAI Tunnel. Service startup before UI-ready is forbidden; duplicate ready is backend-idempotent/single-owner; `--background` does not wait for UI-ready and waking a healthy background runtime does not restart it merely for this gate; `开机启动` controls Windows login launch only;
- frontend/WebView is presentation-only: typed projection + typed user intent. Runtime/readiness/retry/workspace/UAC/current-task state machines belong to backend workers/async tasks; intentionally slow backend operations must not make the UI unresponsive;
- administrator-mode selection is no longer a direct-UAC action under current authority: schema26 warning + full red 9-second confirmation gate applies first; selecting Edit/Full still closes the privileged gate/Broker;
- Dashboard/home contains no `权限模式` row and no Edit/Full/Admin selection controls, cannot change PermissionMode or trigger mode UAC, and retains only read-only administrator privilege runtime status from `PrivilegeState`; permission editing is allowed in Settings and an explicitly reopened onboarding screen 3;
- production MCP/Broker execution must wake Dashboard through backend push/event or equivalent rather than relying on periodic polling for short calls; every real tool call receives at least 500ms visible presentation without delaying its real response. The first row is current execution/`等待命令`; a second `上次执行工具：...` row retains only one secret-redacted last-tool label/summary with relative age aligned to the far right;
- Dashboard add-project uses the native Windows folder picker, not a raw path-entry primary flow;
- screen-3 `min-height >= 80px` remains only a minimum guard; real 780×620 computed geometry must prove every title+description button is at least 2× a single-line control with complete line boxes. The user accepted this scoped visual item on 2026-08-14; a later Screen3 layout change invalidates that evidence and requires re-review;
- Settings is exactly `常规 / 连接 / 权限`: general = `开机启动 / 关闭窗口后继续运行`; connection = exact `Tunnel ID / Runtime API Key`; both `更换` actions keep one rightmost action column, and a saved Runtime API Key adds `清除` immediately to the left of its `更换`. Clear deletes the Windows secure credential without revealing it and follows the same controlled active/connecting connection-change lifecycle. Page-level footer remains `打开欢迎页 / 完成`; no `测试连接` button;
- any scrollable rounded sheet/dialog/card must preserve all four outer rounded corners; its scrollbar is clipped or inset inside the rounded shell and cannot flatten the right-side radius; vertical scrollbar top/bottom arrow, triangle, or equivalent increment/decrement buttons are forbidden while wheel/track/thumb scrolling remains usable;
- Diagnostics is exactly `运行状态 / 项目 / 日志`; runtime rows = 本地运行环境 / 编码服务 / OpenAI Tunnel / 管理员权限; project = actual path; logs = bounded recent redacted events; page actions only `打开日志 / 导出诊断 / 完成`; engineering generation/attempt/PID/SID/nonce/IPC details are not normal UI;
- LB-018 must retire Cloudflare/cloudflared from the final LocalBridge distribution: no `cloudflared.exe`, cloudflared manifest, Cloudflare managed-tunnel activation, launcher argument or fallback in final bundle/runtime manifest/installer. Historical compatibility evidence may retain the upstream fact but must not become executable packaged runtime.

## Schema25 — LocalBridge Agent Runtime facade ratification（2026-08-14）

`docs/LOCALBRIDGE_TOOL_WRAPPER_AND_SHELL_RESOLVER.md` is the retained final Agent Runtime/System Maintenance design guidance. LocalBridge owns its versioned public Tool Registry/API; coding-tools-mcp is a replaceable internal workspace runtime; upstream tools/list/schema/name/error/new tool cannot automatically pass through; the currently frozen v1 non-privileged Registry has eight core tools, actual `tools/list` remains policy-filtered, and `elevated_exec` remains a Broker-governed conditional privileged extension; runtime adapter must perform mandatory capability negotiation and normalize result/error; PEP uses stable LocalBridge capability/action and workflow transitive capability.

ShellResolver accepts only logical selector `auto/powershell/pwsh/windows_powershell/cmd`. `auto` only considers trusted candidates and selects highest compatible PowerShell Core by semantic version, then Windows PowerShell 5.1, then `cmd.exe`. PATH is not authority; candidate identity/trusted installation must be validated before version probe or execution. Command-text guessing, arbitrary shell executable and automatic installation/update are forbidden; DirectProcessExecutor and ShellExecutor remain structurally separate.

Unfrozen v0.1 non-goals remain WSL/container/remote shell, custom shell registry, environment-manager abstraction and exact internal directory layout. The final design guidance also defines the longer-term System Maintenance domain (`system_inspect` / `system_manage`, structured privileged operations and user authorization), but those future capabilities do not override current PR sequencing or claim implementation before their contracts are reached.

The Agent Runtime foundation currently reopens execution at `G2 / LB-006`. G2 adversarial generation 9 has been consumed as **FAIL / REWORK_REQUIRED**. Schema27 freezes the resulting public-facade/session/workspace-path/nested-Git corrections into LB-006: LocalBridge-owned public session/output handles with terminal convergence, correct `command_control` live-poll/read/write/kill semantics, non-empty ordinary `workspace_context.workspace`, deterministic private-result semantic probes where upstream outputSchema is insufficient, nonzero-exit `ProcessFailed`, no advertised-but-unavailable v1 action, relative-only workspace-bound public path inputs, and one workspace-bounded nested-repository resolver for all Git actions. LB-006→LB-012 must be strictly revalidated, followed by fresh G2 adversarial generation 10; historical G2 generation 8 and G3 generation 6 are provenance only. G3 must then be re-executed on the new G2 baseline, and G3→G4 human Gate is not currently actionable. Schema26 administrator safety confirmation is queued for LB-015/LB-016 when strict sequencing reaches them; it does not move the current pointer.

## Schema28 — Test Orchestration + Public Runtime Corrections（historical）

The schema27 LB-006 implementation acceptance remains historical evidence, but fresh public-plugin tests supersede it as an unlock decision. Current execution is again `G2 / LB-006 = REWORK_REQUIRED`; LB-007..LB-012 and all G3/G4 work remain blocked, and G2 adversarial generation 10 has **not** been consumed.

LB-006 must now prove exact single-parse shell quoting; ordered incremental/no-replay poll; post-start live stdin write; kill→stable cancelled without healthy-runtime `RuntimeUnavailable` or later `SessionUnavailable`; real `view_image` resize; UTF-8 Windows PowerShell Chinese output; 1-based inclusive Git blame/document ranges with `start>end → InvalidArgument`; and serving-time semantic compatibility for every adapter-consumed private result field not guaranteed by upstream outputSchema.

Verification is tiered: cheap deterministic current-PR tests form `PR Fast Gate`; real bundled runtime/PEP/process checks form `PR Runtime Gate` and heavy assertions sharing one topology must be compressed into a shared lifecycle unless isolation itself is under test; complete cargo/npm/build/clippy/architecture/release checks belong to formal PR/group/release Gate rather than every inner-loop edit. Static source markers cannot substitute for executable behavioral proof. Test commands require bounded timeout/cancel and lost sessions are terminal.

Development/test command windows are explicitly tolerated and are not product-failure evidence. Final packaged/normal GUI LocalBridge nevertheless must not expose unintended console windows from its owned managed runtime/Tunnel/Broker/helper/shell/direct-command children; that property is proven only by LB-018/LB-019 release-style/clean-machine execution while retaining Job/process ownership.

## Schema29 — Durable command task-state / G2 generation 17（current）

Schema29 ratifies A289–A292. A289–A291 require every command terminal route to execute an unconditional `finally`/finally-equivalent finalizer that, under a `(task_id, session_id)` owner transaction, persists the terminal snapshot, appends exactly one `command_finished`, and clears `current_command` atomically. task-state terminal truth must survive private-session pruning/retention expiry; delayed or duplicate callbacks cannot mutate another owner. A292 separately queues default-centered first visible 780×620 main-window creation for LB-015 while preserving a user-moved existing window on tray reopen.

G2 independent adversarial **generation 17 = FAIL / REWORK_REQUIRED** against schema29 baseline `003e85ed5da5944fba8bc89ba7051e55f23333a1`. Empirical task-history evidence found 52 inconsistent terminal tasks in the latest 100 entries, including completed tasks that still retained running `current_command`; source inspection found `task_control` consuming only in-memory `CurrentTaskStatus` and `PublicCommandSession.terminal` stored only in in-memory maps, with no durable task-state `command_finished` journal or `(task_id, session_id)` CAS finalizer.

Current execution therefore reopens at `G2 / LB-006 = REWORK_REQUIRED`; LB-007..LB-012 and all G3 PRs are blocked. After A289–A291 are repaired, LB-007 through LB-012 must be strictly reaccepted and a fresh **G2 adversarial generation 18** is required. A292 remains queued for LB-015 only after G2 passes again. G4 remains blocked.

## Schema30 — Nested project / PowerShell baseline / workspace directory write（current）

After schema29 task-state implementation was accepted, a fresh LB-006 scoped product-plugin review still reproduced two MAJOR defects: with active workspace `D:\project`, `git_workflow(path="LocalBridge")` recognizes the nested repository while `agent_workflow` reports `is_repo=false` because it has no project-path selector and uses workspace-root Git context; and trusted `windows_powershell`/`auto` breaks basic `Get-Location`/`Get-ChildItem` because arbitrary module autoload is locked before user code without a trusted standard cmdlet preload.

Schema30 also clarifies the permission model: active workspace is an authorized read/write root, not a read-only root. Creating `D:\project\test` and removing it when empty is an ordinary reviewed workspace write and is frozen as process-free `agent_workflow.directory_changes[]`, with only `create_directory` / `remove_empty_directory`. This does **not** require weakening PowerShell provider review: generic provider mutations such as `New-Item`/`Set-Content` may remain review-required because they can target `Alias:`/`Function:`; arbitrary module autoload remains forbidden and standard PowerShell capability must come from a fixed/identity-validated trusted preload.

Schema30 ratifies A293–A295. Execution remains `G2 / LB-006 = REWORK_REQUIRED`; LB-007 stays blocked until LB-006 passes the new runtime behavior, after which LB-007 must reaccept the structured-write transitive capability boundary. G2 adversarial generation 18 remains unconsumed.


## Schema36 — Shell fidelity / runtime observability / admin consent（current）

2026-08-17 使用端黑盒已真实复现：Full 下 `where cmd`、`where pwsh`、`echo %PATH%` 被错误返回 `PrivilegedRouteNotAvailable`。同时当前源码静态确认 Dashboard 仍渲染 PermissionMode 行，管理员 9 秒 eligibility 仍由 React `performance.now()` 决定并直接进入 `set_permission_mode → request_explicit_admin`。这些事实触发 schema36。

schema36 不新增 public core tool；以 enriched `workspace_context`、现有 `agent_workflow diagnose` 与既有工具 dry_run/explain 承担可观测性。Shell 分类必须基于执行目标/参数/操作语义而不是关键词；Windows 原生常用 redirection/NUL/pipeline/quotes/conditional/env/script 语义恢复。错误合同、session/output metadata、single path authority 同步冻结为 A316–A333。

实时治理必须回开到 G2/LB-006；G2 generation23 与 G3 generation12 保留历史但失去当前 unlock authority。完成 LB-006→LB-012 后执行 fresh G2 generation24，再重新 LB-013→LB-017 与 fresh G3 generation13。G3 结束仍进入独立 human Gate REQUIRED，不能由执行/审查智能体自行 PASS。
