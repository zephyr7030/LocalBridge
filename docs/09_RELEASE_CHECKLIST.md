# 09 — Release Checklist v9

## Deterministic CI

- [ ] frontend production build
- [ ] cargo test
- [ ] frontend unit tests
- [ ] domain tests
- [ ] process supervisor tests
- [ ] policy adversarial tests
- [ ] fake MCP/Tunnel orchestrator tests
- [ ] recovery/workspace switch tests
- [ ] no real OpenAI key required

## Supply Chain

- [ ] runtime manifest valid
- [ ] Python SHA256 verified
- [ ] coding-tools provenance pinned
- [ ] tunnel-client SHA256 verified
- [ ] checksum mismatch blocks launch

## Security

- [ ] no secret in git
- [ ] no secret in installer fixtures
- [ ] no Authorization in logs
- [ ] no Runtime Key in CLI args
- [ ] listener loopback only
- [ ] Edit indirect exec denied
- [ ] unknown capability denied
- [ ] Full control-plane denied
- [ ] cached tools/list cannot bypass tools/call
- [ ] junction/symlink/reparse tests pass
- [ ] renderer arbitrary URL denied

## Process Lifecycle

- [ ] Job Object / ADR-defined ownership verified
- [ ] PID reuse safe
- [ ] stale state reconciled
- [ ] 关闭窗口后继续运行=true: close hides and keeps Ready
- [ ] 关闭窗口后继续运行=false: close performs orderly cleanup and exits
- [ ] close behavior preference is versioned/migration-safe
- [ ] Tray exit kills owned runtime
- [ ] app crash behavior matches ADR
- [ ] `--background` has no visible main window
- [ ] existing background instance is reused
- [ ] manual stop survives restart

## Workspace

- [ ] desired/candidate/active semantics pass
- [ ] shutdown order Tunnel→PEP→MCP
- [ ] candidate only commits after Ready
- [ ] failure rollback works

## UX

- [ ] clean install opens Wizard
- [ ] Screen 2 ChatGPT button present
- [ ] system browser only
- [ ] no WebView
- [ ] Wizard finishes without PowerShell
- [ ] one Dashboard
- [ ] minimal necessary buttons/prompts
- [ ] advanced internals hidden

## LB-018 Packaging

- [ ] final bundle has no cloudflared.exe/cloudflared-manifest.json
- [ ] runtime manifest/packaging inventory/launcher/fallback has no Cloudflare managed tunnel activation
- [ ] historical compatibility evidence mentioning cloudflared is not packaged as executable runtime
- [ ] packaging gate fails closed on cloudflared reintroduction
- [ ] no Python preinstall required
- [ ] dummy/real sidecar packaged correctly
- [ ] runtime resources present
- [ ] installer succeeds
- [ ] uninstall cleanup succeeds

## LB-019 Live Release Gate

- [ ] clean Windows 11 x64 VM
- [ ] real Tunnel
- [ ] real ChatGPT MCP call
- [ ] close-to-tray
- [ ] reboot
- [ ] silent autostart
- [ ] reconnect
- [ ] crash recovery
- [ ] uninstall
- [ ] no orphan owned sidecars


## Elevated

- [ ] LocalBridge main process remains non-elevated
- [ ] only Broker receives Administrator token
- [ ] visible 管理员模式 selection/reselection is the explicit UAC activation; no separate enable-admin button
- [ ] no TTL/time selector
- [ ] disable closes privileged gate immediately
- [ ] no automatic UAC on background startup
- [ ] Broker IPC ACL/auth/replay tests
- [ ] no Broker network listener
- [ ] Broker crash clears Active state
- [ ] Tray exit leaves no elevated Broker
- [ ] reboot requires explicit reactivation


## Distribution

- [ ] Windows 11 x64 only
- [ ] Python Embedded bundled
- [ ] coding-tools-mcp bundled
- [ ] tunnel-client bundled
- [ ] no system Python dependency
- [ ] no pip/venv/runtime install
- [ ] no bundled WebView2
- [ ] runtime self-update absent
- [ ] application auto-updater absent
- [ ] telemetry absent
- [ ] crash auto-upload absent
- [ ] installer size measured
- [ ] installed size measured
- [ ] size attribution if thresholds exceeded

## Generic Elevated Exec

- [ ] Edit denies elevated_exec
- [ ] Full denies elevated_exec
- [ ] Elevated requires Active Broker
- [ ] program/args structured
- [ ] no default shell=true
- [ ] timeout
- [ ] cancellation
- [ ] stdout/stderr limit
- [ ] redaction
- [ ] control-plane remains denied


## Dashboard Privilege State

- [ ] Dashboard always exposes administrator privilege runtime status
- [ ] status comes from PrivilegeState
- [ ] Elevated preference alone does not display Active
- [ ] Requested shows waiting authorization; selecting/reselecting 管理员模式 triggers UAC without a separate enable action
- [ ] leaving 管理员模式 for Edit/Full closes the privileged gate and Broker
- [ ] Faulted is visible immediately
- [ ] no Broker PID/nonce/SID/IPC internals exposed

## UI Language

- [ ] all normal user-facing copy is zh-CN
- [ ] no ordinary Dashboard/Settings/Diagnostics/Edit/Full/Elevated/Broker/Runtime labels; exact `Tunnel ID` / `Runtime API Key` are required field-name exceptions
- [ ] professional abbreviations only where justified
- [ ] no direct enum/fault-code rendering
- [ ] no bilingual label clutter
- [ ] errors are concise Chinese and actionable

## Maintainability

- [ ] architecture verification passes
- [ ] compatibility baseline matches packaged runtimes
- [ ] stable adapter boundary respected
- [ ] settings schema/migrations pass historical fixtures
- [ ] future schema fails safely
- [ ] dependency lockfiles frozen
- [ ] SBOM generated
- [ ] THIRD_PARTY_NOTICES generated/verified
- [ ] release provenance generated
- [ ] migration failure preserves previous config
- [ ] changelog contains user-visible changes only

## Current Task Status

- [ ] dashboard has one current-task status region
- [ ] idle/no-task state is explicit `等待命令`, always visible, never `空闲`
- [ ] tool category uses stable domain classification
- [ ] task summary is secret-redacted
- [ ] raw MCP tool IDs are not shown
- [ ] denied call never appears as running
- [ ] elevated waiting state matches privilege state
- [ ] terminal task returns to `等待命令`
- [ ] no recent activity list
- [ ] no activity feed/timeline
- [ ] no model thought/chat-response projection

## Minimal Inline Activity UI

- [ ] active task is one inline row
- [ ] no section title
- [ ] no type/task/status labels
- [ ] no redundant running text
- [ ] active indicator is a small green pulse
- [ ] animation does not shift layout
- [ ] prefers-reduced-motion uses static indicator
- [ ] `等待命令` is neutral, low-presence and always visible

## Credentials & Projects

- [ ] Runtime API Key stored only in secure credential backend
- [ ] no plaintext credential fallback
- [ ] no secret in settings/JSON/TOML/.env/browser storage
- [ ] no secret in process command line
- [ ] no secret in logs/diagnostics
- [ ] saved key cannot be revealed by UI
- [ ] remembered projects are not simultaneous authorized roots
- [ ] exactly zero or one active workspace
- [ ] project add/select/remove tests pass
- [ ] workspace identity de-dup uses validated security identity
- [ ] removing a project never deletes disk files
- [ ] removing active project enters NoActiveWorkspace
- [ ] no automatic authorization of another remembered project
- [ ] MCP cannot mutate project registry/current project
## G3 UI/Backend Responsiveness

- [ ] configured foreground launch auto-starts runtime asynchronously
- [ ] UI remains responsive during intentionally slow backend startup/lifecycle work
- [ ] React/WebView owns no runtime readiness/retry/UAC state machine
- [ ] production MCP/Broker CurrentTaskStatus reaches Dashboard end-to-end
- [ ] Dashboard folder add uses native Windows folder picker
- [ ] Settings matches frozen 常规/连接/权限 layout and has no 测试连接
- [ ] Diagnostics matches frozen 运行状态/项目/日志 layout and exact action set
