
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
- single-line green-pulse execution status；
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

Additional UI freeze (superseded by explicit user contract amendment on 2026-08-13):

- onboarding = exactly 6 screens;
- 1/6 = `简单设置 即可开始`;
- OpenAI = screen 2 with `Tunnel ID` and `Runtime API Key` labels;
- workspace + permission = screen 3 and uses the native Windows folder picker as the primary new-project interaction;
- screen 4 = `Local Bridge 设置`, system browser only, fixed ChatGPT custom-connector URL `https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`;
- screen 5 = `Local Bridge 使用确认`, with minimum guidance and no fabricated ChatGPT-state detection;
- no screen 7;
- screen 6 confirm disabled until all three checks are green;
- completion hint appears only after readiness and is exactly `配置完成，在插件中选择刚刚添加的Local Bridge试试吧`;
- main window is fixed at 900×620; minimum and maximum inner size are both 900×620;
- `resizable=false` and `maximizable=false`; ordinary user interaction cannot change the main-window size;
- native Windows decorations are disabled; exactly one custom edge-to-edge chrome fills the client area and provides drag/minimize/close without maximize or a double frame;
- Dashboard/onboarding must remain complete and operable inside that fixed client area; no resize/maximize responsive E2E is required;
- onboarding itself is a full-page single-content layout inside the custom chrome content area; a centered floating wizard card/modal/dialog surrounded by a large empty canvas is forbidden;
- user clicks confirm to enter main UI;
- buttons use one coherent visible affordance system; white-on-white ambiguous controls and layout-shifting copy feedback are forbidden.


Frozen brand icon:

- `assets/icons/localbridge.png` — 1024×1024 RGBA;
- `assets/icons/localbridge.ico` — 16/24/32/48/64/128/256 px;
- Windows app, installer and tray use this asset;
- no icon-library dependency or placeholder replacement is permitted.
