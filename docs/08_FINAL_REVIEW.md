
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

开发入口：

```text
current_group = G0
current_pr    = LB-000
```

G0 审查 PASS 前 G1 不得开始。

Current UI freeze (explicit user contract amendment on 2026-08-13; supersedes all earlier onboarding/UI freezes):

- onboarding = exactly 5 screens: 欢迎 → OpenAI → 项目与权限 → 创建自定义插件 → 启动检查; there is no screen 6 and the former `Local Bridge 使用确认` page is removed;
- 1/5 = `简单设置 即可开始`; screen 1 is the only screen without a back action;
- OpenAI = screen 2 with `Tunnel ID` and `Runtime API Key` labels and an explicit back action; persisted Tunnel ID is prefilled; a saved key displays exact `已安全保存至windows安全凭据`, focus uses only same-length `*` generated from backend length metadata, plaintext is never returned, and the only helper sentence is `Runtime API Key 仅保存在 Windows 安全凭据中。`;
- workspace + permission = screen 3 and uses the native Windows folder picker as the primary new-project interaction; any title+description two-line permission button must have actual rendered height at least 2× the ordinary single-line control at 780×620, with both line boxes complete; static CSS markers do not constitute PASS and human 780×620 visual acceptance remains required; ordinary selected state uses blue `#0071e3`, administrator mode uses amber/yellow in onboarding and Settings; screen 3 has an explicit back action;
- after screen 3 saves project and permission, it starts selected project/runtime/MCP/OpenAI Tunnel and reaches real all-ready before screen 4; the unique runtime startup edge must not be deferred until screen 5;
- screen 4 = `创建自定义插件`; exact developer-mode hint; `打开 ChatGPT插件设置` is in the left-side action flow and is an argument-free frontend action backed by a Rust fixed allowlist + system browser for `https://chatgpt.com/plugins#settings/Plugins`; directly beneath it display `打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件`;
- screen 4 has exactly two information rows `名称 = Local Bridge` / `Tunnel ID = current persisted saved value`, no `本地服务`; each row has independent stable green `已复制` feedback for exactly 3 seconds with no layout shift;
- `打开插件管理页` is also in the left-side action flow and is an argument-free frontend action backed by a Rust fixed allowlist + system browser for `https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`; both browser actions forbid WebView and arbitrary/frontend-provided URLs; screen 4 has explicit `返回` and `继续`;
- screen 5 is `启动检查`, containing only 本地运行环境 / 编码服务 / OpenAI Tunnel; status dots consume the same typed status source as Dashboard and map Ready=green, Starting=amber/yellow, Fault=red, Unknown=gray;
- screen 5 confirm is disabled and completion hint hidden until all three checks are green; completion hint is exactly `配置完成，在插件中选择刚刚添加的Local Bridge试试吧`; no auto-advance; screen 5 has an explicit back action to screen 4;
- screens 2/3/4/5 all have explicit back paths; save/start/configuration failures must never trap the user;
- ordinary primary/selected/product-accent UI uses original blue `#0071e3`; black is not the ordinary product accent; administrator mode is the amber/yellow logical-color exception;
- Dashboard primary service status dots use the same typed source and Ready/Starting/Fault/Unknown color semantics as onboarding; independent conflicting state is forbidden;
- main window is fixed at 780×620; minimum and maximum inner size are both 780×620;
- `resizable=false` and `maximizable=false`; ordinary user interaction cannot change the main-window size;
- native Windows decorations are disabled; exactly one custom edge-to-edge chrome fills the client area and provides drag/minimize/close without maximize or a double frame;
- Dashboard/onboarding must remain complete and operable inside that fixed 780×620 client area; no resize/maximize responsive E2E is required;
- onboarding itself is a full-page single-content layout inside the custom chrome content area; a centered floating wizard card/modal/dialog surrounded by a large empty canvas is forbidden;
- user clicks screen-5 confirm to enter main UI;
- buttons use one coherent visible affordance system; white-on-white ambiguous controls and layout-shifting copy feedback are forbidden.


Frozen brand icon:

- `assets/icons/localbridge.png` — 1024×1024 RGBA;
- `assets/icons/localbridge.ico` — 16/24/32/48/64/128/256 px;
- Windows app, installer and tray use this asset;
- no icon-library dependency or placeholder replacement is permitted.

## G3 human-review amendment — generation 2 (2026-08-13)

This section supersedes conflicting older G3 UI/lifecycle wording while preserving the already-correct five-screen and Screen3→4 readiness contract above.

- effective rework entry is `LB-014`; G4/LB-018 remains blocked until LB-014→LB-017 are re-executed, G3 adversarial generation 6 passes, and a fresh G3→G4 human Gate passes;
- `关闭窗口后继续运行` is a persisted setting: enabled = close hides and keeps runtime/tray, disabled = orderly runtime/Broker cleanup then exit;
- configured normal foreground launch is UI-first: create/show an interactive UI, emit one typed `UI-ready`, then backend asynchronously starts any stopped selected project/runtime/MCP/OpenAI Tunnel. Service startup before UI-ready is forbidden; duplicate ready is backend-idempotent/single-owner; `--background` does not wait for UI-ready and waking a healthy background runtime does not restart it merely for this gate; `开机启动` controls Windows login launch only;
- frontend/WebView is presentation-only: typed projection + typed user intent. Runtime/readiness/retry/workspace/UAC/current-task state machines belong to backend workers/async tasks; intentionally slow backend operations must not make the UI unresponsive;
- visible selection/reselection of `管理员模式` in Settings or onboarding screen 3 itself is the explicit UAC action when Broker is not Active; the separate `启用管理员权限` button is forbidden; selecting Edit/Full closes the privileged gate/Broker;
- Dashboard/home contains no `权限模式` row and no Edit/Full/Admin selection controls, cannot change PermissionMode or trigger mode UAC, and retains only read-only administrator privilege runtime status from `PrivilegeState`; permission editing is allowed in Settings and an explicitly reopened onboarding screen 3;
- production MCP/Broker execution must wake Dashboard through backend push/event or equivalent rather than relying on periodic polling for short calls; every real tool call receives at least 500ms visible presentation without delaying its real response. The first row is current execution/`等待命令`; a second `上次执行工具：...` row retains only one secret-redacted last-tool label/summary with relative age aligned to the far right;
- Dashboard add-project uses the native Windows folder picker, not a raw path-entry primary flow;
- screen-3 `min-height >= 80px` remains only a minimum guard; real 780×620 computed geometry must prove every title+description button is at least 2× a single-line control with complete line boxes. The user accepted this scoped visual item on 2026-08-14; a later Screen3 layout change invalidates that evidence and requires re-review;
- Settings is exactly `常规 / 连接 / 权限`: general = `开机启动 / 关闭窗口后继续运行`; connection = exact `Tunnel ID / Runtime API Key`; both `更换` actions keep one rightmost action column, and a saved Runtime API Key adds `清除` immediately to the left of its `更换`. Clear deletes the Windows secure credential without revealing it and follows the same controlled active/connecting connection-change lifecycle. Page-level footer remains `打开欢迎页 / 完成`; no `测试连接` button;
- any scrollable rounded sheet/dialog/card must preserve all four outer rounded corners; its scrollbar is clipped or inset inside the rounded shell and cannot flatten the right-side radius; vertical scrollbar top/bottom arrow, triangle, or equivalent increment/decrement buttons are forbidden while wheel/track/thumb scrolling remains usable;
- Diagnostics is exactly `运行状态 / 项目 / 日志`; runtime rows = 本地运行环境 / 编码服务 / OpenAI Tunnel / 管理员权限; project = actual path; logs = bounded recent redacted events; page actions only `打开日志 / 导出诊断 / 完成`; engineering generation/attempt/PID/SID/nonce/IPC details are not normal UI;
- LB-018 must retire Cloudflare/cloudflared from the final LocalBridge distribution: no `cloudflared.exe`, cloudflared manifest, Cloudflare managed-tunnel activation, launcher argument or fallback in final bundle/runtime manifest/installer. Historical compatibility evidence may retain the upstream fact but must not become executable packaged runtime.
