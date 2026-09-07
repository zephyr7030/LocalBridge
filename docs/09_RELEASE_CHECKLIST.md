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

## LB-018PRE Public Repository Hygiene

- [ ] `.coding-tools` generated task/performance state is untracked and ignored while local copies may remain
- [ ] current tracked tree and all reachable private Git history scanned for high-confidence credentials
- [ ] absolute user/machine paths, debug dumps and machine-specific residue reported without exposing matched secret values
- [ ] clean committed checkout completes locked frontend + Rust release-style build without developer Python/venv assumptions
- [ ] root LICENSE exists; Cargo/npm/runtime/toolbox license metadata and THIRD_PARTY_NOTICES are verified
- [ ] public README / SECURITY / CONTRIBUTING / Issue / PR templates present
- [ ] public Windows GitHub Actions runs format hygiene, unit tests, Rust tests, Clippy and release build
- [ ] public source export uses explicit allow/deny policy and a one-commit fresh Git history
- [ ] public source contains no internal contracts, review state, authorization records, internal skills/templates or private Git history
- [ ] distributable payload checker rejects caches/tests/logs/secrets/dev Tunnel config/private governance/machine paths
- [ ] `node tests/e2e/onboarding/fixed_window_runtime_e2e.mjs` PASS（真实窗口渲染验证；需要 GUI 会话，因此不在共享门禁内，发布前手动跑一次）

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
- [ ] workspace-bound inputs accept safe relative paths and ordinary Win32 absolute paths resolving to the same validated active workspace identity
- [ ] outside-root absolute/UNC/verbatim/POSIX absolute/ADS-like/parent-traversal/reparse escape inputs fail closed
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
- [ ] visible 管理员模式 selection/reselection in Settings/onboarding opens the fixed safety warning; backend monotonic 9000ms not-before + enabled user confirmation must complete before any UAC request; no separate enable-admin button
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
- [ ] Dashboard has exactly one read-only 权限模式 row sourced from backend PermissionMode and no 编辑/完整/管理员 mode selection controls
- [ ] Dashboard cannot change PermissionMode or trigger UAC through a permission-mode control
- [ ] Requested shows read-only waiting authorization; mode activation is performed in Settings/onboarding, not Dashboard
- [ ] Dashboard has no permission-mode editing; Settings and an explicitly reopened onboarding screen 3 remain the allowed permission-mode editing surfaces
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
- [ ] public privileged-route unavailable error is canonically `PrivilegedRouteUnavailable`; legacy `PrivilegedRouteNotAvailable` is not a public expected code
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

- [ ] backend push/event or equivalent wakeup is the primary delivery path for real tool-call state; periodic polling is not the short-task transport
- [ ] every real tool call is visibly represented for at least 500ms without delaying the tool's real response
- [ ] dashboard has one current-task status region
- [ ] first-row idle/no-task state is explicit `空闲`, always visible, and does not append relative age
- [ ] second row uses exact prefix `上次执行：`, one secret-redacted user-facing activity label/summary, and far-right relative age
- [ ] tool category uses stable domain classification
- [ ] task summary is secret-redacted
- [ ] raw MCP tool IDs are not shown
- [ ] denied call never appears as running
- [ ] elevated waiting state matches privilege state
- [ ] terminal task returns to `空闲` when no current workflow and no current command remain
- [ ] no recent activity list
- [ ] no activity feed/timeline
- [ ] only one last-tool metadata row is retained; it does not become history/feed/list
- [ ] no model thought/chat-response projection

## Minimal Inline Activity UI

- [ ] active task is one inline row
- [ ] no section title
- [ ] no type/task/status labels
- [ ] no redundant running text
- [ ] active indicator is a small green pulse
- [ ] animation does not shift layout
- [ ] prefers-reduced-motion uses static indicator
- [ ] `空闲` is neutral, low-presence and always visible

## Credentials & Projects

- [ ] Runtime API Key stored only in secure credential backend
- [ ] onboarding pre-fills the current persisted Tunnel ID when present
- [ ] onboarding saved-key text is exactly `已安全保存至windows安全凭据`
- [ ] focusing an already-saved Runtime API Key shows only a same-length `*` mask derived from backend length metadata; plaintext is never returned and an untouched mask is never submitted as a replacement key
- [ ] onboarding Runtime API Key helper is exactly `Runtime API Key 仅保存在 Windows 安全凭据中。`
- [ ] Settings saved Runtime API Key shows `清除` immediately left of `更换`; clear deletes the Windows secure credential without revealing it and follows the controlled active/connecting connection-change lifecycle
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

- [ ] configured foreground launch creates/shows interactive UI first, emits one typed UI-ready, and only then backend asynchronously starts any stopped runtime/MCP/Tunnel
- [ ] no stopped managed service starts before UI-ready; duplicate UI-ready is backend-idempotent/single-owner; `--background` does not wait for ready and a healthy background runtime is not restarted solely for this gate
- [ ] UI remains responsive during intentionally slow backend startup/lifecycle work
- [ ] React/WebView owns no runtime readiness/retry/UAC state machine
- [ ] production MCP/Broker CurrentTaskStatus reaches Dashboard end-to-end
- [ ] Dashboard folder add uses native Windows folder picker
- [ ] Settings matches frozen 常规/连接/权限 layout and has no 测试连接
- [ ] Diagnostics matches frozen 运行状态/项目/日志 layout and exact action set
- [ ] main window default/minimum/maximum inner size are all exactly 780×620; resizable/maximizable/decorations/custom-chrome semantics remain frozen
- [x] title+description permission buttons render at least 2× the actual height of a single-line control at 780×620 with both line boxes complete; user scoped visual review PASS 2026-08-14 (reopen if Screen3 layout changes)
- [ ] a scrollable rounded sheet/dialog/card preserves all four outer corners, clips/insets the scrollbar inside the rounded shell, shows no top/bottom arrow or triangle buttons, and retains wheel/track/thumb scrolling


## Schema38 Public Control / Runtime

- [ ] detached running public session can be cancelled by task_control and converges through the same terminator/finalizer as command_control kill
- [ ] git show/diff file metadata uses NUL-delimited machine Git output and Unicode deleted paths retain correct status independently from patch text
- [ ] command_control and elevated_exec expose directly projectable top-level properties without client-hostile top-level oneOf; strict server call validation remains
- [ ] document_workflow 只暴露六个固定 action，edit 只暴露四个 block 原子操作，edit/rebuild 强制 expected_sha256
- [ ] Windows 300ms timeout converges within 1800ms, TERM does not map to CTRL_BREAK/debug mode, and forced tree kill is bounded
- [ ] Full cmd/PowerShell aliases and scripts execute with unchanged current-user authority; no alias rewrite or top-level command-string privilege classifier remains
- [ ] direct system tools and descendant-invoked system tools have identical current-user authority; administrator-token work is available only through the structured Broker route

## Schema39 Agent Execution Platform

- [ ] Workflow owns Task lifecycle; only process-backed Tasks allocate Execution/public Session/process tree and non-process Tasks do not fabricate session handles
- [ ] task_control remains exactly get/cancel and cancel resolves the owned execution without requiring AI to choose session/process handles
- [ ] every public schema passes real downstream MCP-client projection; legal top-level fields/enum/bounds remain discoverable without trial-and-error
- [ ] existing canonical typed errors are normalized consistently across direct tools and agent_workflow indirection
- [ ] agent_workflow reuses shared filesystem/Git/process/document/image/privilege services, Session Manager, terminal finalizer, path authority and capability classifier
- [ ] workspace_context returns compact cached project/Git/runtime/build-test/shell/permission/current-task discovery without redundant probe processes
- [ ] public process states are running/completed/failed/cancelled/timed_out/lost and every non-running state is durable terminal truth independent of polling
- [ ] workflow resume + retained-output continuation survive reconnect/interruption without adding generic pause/history/snapshot/rollback semantics
- [ ] public surface remains exactly eight non-privileged core tools plus elevated_exec; no file_workflow or parallel maturity Gate taxonomy is introduced
