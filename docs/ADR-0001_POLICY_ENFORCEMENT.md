# ADR-0001 — MCP Policy Enforcement

Status: **ACCEPTED FOR IMPLEMENTATION**
Decision owner: LB-000
Evidence date: 2026-08-11

## Context

LocalBridge requires three user-visible permission modes, but the security boundary is stricter than a cosmetic `tools/list` filter. Edit mode must never obtain process execution; Full may execute reviewed current-user capabilities; Elevated adds only explicitly reviewed Broker-routed privileged operations. Unknown capabilities and control-plane mutation are always denied.

LB-000 launched the pinned public `coding-tools-mcp` v0.2.2 (`311c1f2529d0f047ad2a8b68db6bf92dbb93d6bc`) through real Streamable HTTP MCP. The exact live catalog contains 20 tools and has canonical schema SHA256 `d187db7ad6729893f17fceb539f030eb0c6139528ab35cb3f9564d27383d3674`.

The decisive observation is that upstream `safe` and `trusted` modes publish an identical catalog, and `safe` mode successfully executed a bounded `exec_command`. Therefore upstream permission mode is not equivalent to the LocalBridge Edit/Full authorization model.

## Options considered

### A — Upstream-native policy

Rejected as the primary enforcement point. v0.2.2 does not provide the required per-mode catalog/call boundary and cannot guarantee LocalBridge `tools/call` policy semantics.

### B — Thin list/filter adapter

Rejected if it only filters `tools/list`. A client can cache or directly invoke a known tool. The mandatory enforcement point is every `tools/call`, not discovery.

### C — First-party Rust MCP Guard / PEP

Accepted.

## Decision

LocalBridge will place a first-party Rust, protocol-aware MCP Guard between Tunnel and the pinned coding-tools runtime:

```text
Tunnel
  ↓ loopback MCP
Rust MCP Guard / PEP
  ↓ authenticated loopback MCP
coding-tools-mcp
```

The Guard is a stable adapter and policy-enforcement point, not a fork of upstream tool implementations. It must preserve MCP transport/session semantics while intercepting the minimum policy-relevant surface.

LB-000 additionally verified the upstream HTTP authentication/session contract: v0.2.2 accepts a bearer token through `CODING_TOOLS_MCP_AUTH_TOKEN`; an unauthenticated `initialize` returned HTTP 401, while the authenticated Streamable HTTP session issued and reused an MCP session ID. LocalBridge will inject a runtime-generated internal bearer through the child environment rather than placing it on the upstream process command line.

Mandatory behavior:

- filter `tools/list` for UX only;
- classify and enforce **every** `tools/call` independently of prior discovery;
- unknown tool/capability → deny;
- Edit → no process-exec/process-session capability;
- Full → reviewed current-user execution only;
- Elevated → the same ordinary MCP capability ceiling as Full; privileged execution is a separate Broker route;
- LocalBridge control-plane mutation → deny always;
- `request_permissions` → deny in all LocalBridge modes so upstream can never expand LocalBridge authorization;
- classify transitive command capability, including `docker`, `podman`, and `wsl` as `privileged-external-runtime` requiring review;
- enforce the single active workspace identity before forwarding filesystem calls;
- project actual allow/deny/execution state into redacted `CurrentTaskStatus`;
- bind Guard and upstream listener to loopback only;
- prevent Tunnel from targeting coding-tools-mcp directly;
- authenticate the Guard→upstream hop with runtime-generated internal authorization material;
- launch upstream with anonymous telemetry disabled.

The upstream runtime may run in its normal `trusted` mode behind the Guard so Full mode remains usable, but this is defense-in-depth configuration only. `--dangerously-skip-all-permissions` is forbidden.

## Stable adapter boundary

Domain and UI code must not depend on upstream private tool classes, Python module layout, FastMCP internals, or raw third-party fault objects. The adapter exposes stable LocalBridge concepts: tool identity, classified capability, redacted task summary, execution result/fault, session lifecycle, and active workspace boundary.

Any upstream upgrade must regenerate structural and capability diffs before the Guard policy is changed. New tools remain denied until reviewed.

## Consequences

This adds a small first-party MCP protocol boundary, but avoids both insecure list-only filtering and a long-lived fork of upstream. It also makes cached `tools/list`, future workflow-like tools, and upstream permission semantic changes fail closed.

The LB-000 live Tunnel credential blocker does not affect this decision because the PEP conclusion derives from direct local execution of the pinned coding-tools runtime.

## LB-007 production boundary

LB-007 implements the accepted decision inside `src-tauri/src/mcp/**` in front of the LB-006 stable runtime adapter. The production authorization identity is now the **LocalBridge public tool/action**, not an upstream primitive name. `policy.rs` owns the stable public classifier and transitive capability declarations, `AgentFacade` applies that policy before adapter dispatch, and `server.rs` is the first-party loopback Streamable HTTP PEP consumed by Tunnel. `runtime-policy.toml` schema 6 contains a `localbridge_public` policy that may narrow the frozen public maxima but may not widen them.

- `tools/list` filtering is UX-only and never grants authority.
- every `tools/call` receives the current `PermissionMode` and current LocalBridge public policy and is re-authorized immediately before adapter dispatch; a cached Full-mode catalog cannot bypass a later switch to Edit or a later policy narrowing.
- the public classifier recognizes only the versioned LocalBridge Registry/action vocabulary. Raw upstream names are not public authorization anchors and cannot be called as a bypass.
- the exact upstream v0.2.2 surface remains an **internal adapter compatibility baseline** for LB-006 capability negotiation only. It is not the public PEP identity and upstream additions never become public automatically.
- `request_permissions` and LocalBridge workspace/permission/credential/tunnel/MCP configuration names are `ControlPlane` and denied in every mode.
- high-level public workflows declare their complete transitive LocalBridge read/write/process/git/network/privilege requirements before execution. Edit denies any workflow containing process execution; unreviewed network and privilege routes fail closed; unknown actions/capabilities fail closed.
- shell execution review is **fail-closed at the invocation boundary**, not a literal three-word substring filter. Explicit `docker` / `podman` / `wsl` targets enter `privileged-external-runtime` review, and shell-time target construction or indirection that prevents a stable target review (for example a PowerShell call operator over a variable, dot-sourcing, a secondary shell, `Invoke-Expression`, `Start-Process`, alias/function creation, or cmd variable expansion) is also review-required before dispatch. The same rule is applied to `agent_workflow.commands` before any workflow side effect.
- PowerShell process-target review also treats arbitrary static .NET member invocation as unreviewable by default because it can reach `System.Diagnostics.Process.Start`, reflection/Activator, P/Invoke helpers, COM construction, and equivalent runtime launch surfaces without a native command token. The only static-member exception is a narrow `System.Console` I/O allowlist needed by the public incremental stdin/session contract: statically rooted Console input supports `ReadLine` and `ReadToEnd`, while statically rooted Console output/error supports the existing `Write`/`WriteLine` seam. Other instance-member calls remain review-required unless separately ratified. Process-capable instance member calls such as `Start`, `Run`, `Exec`, `Create`, `ShellExecute`, and reflection `Invoke*` are review-required; `New-Object`/COM and WMI construction surfaces are likewise classified for review.
- PowerShell subexpression syntax `$()` is itself an executable runtime-evaluation surface. Any executable `$()` in unquoted or expandable double-quoted text (including a double-quoted here-string) is review-required rather than recursively guessed by a partial parser. A `$()` sequence inside a single-quoted literal, or escaped with PowerShell backtick escaping, remains inert data and is not escalated solely for containing those characters.
- PowerShell target-delegating aliases/cmdlets (`start`, `Invoke-Item` / `ii`) are review-required like `Start-Process`. An unquoted PowerShell backtick escape is also review-required because it can reconstruct a command/cmdlet token at parse time (including newer Unicode escape forms); backtick escapes that remain inside quoted data continue to be treated as data.
- PowerShell provider mutation is a dynamic command-surface boundary. Cmdlets that can create, replace, rename, copy, move, clear, import, export, or otherwise mutate provider items (`Set-Item`, `New-Item`, `Remove-Item`, `Rename-Item`, `Move-Item`, `Copy-Item`, `Clear-Item`, `Set-Content`, `Add-Content`, `Clear-Content`, `Import-Alias`, `Export-Alias`, `Set-Alias`, `New-Alias`) and their built-in aliases are review-required before execution. Direct command-namespace assignment such as `$Alias:name=...` or `$Function:name=...` is the same boundary and is review-required as well. This is intentionally conservative for every provider target: a partial parser cannot safely prove that a runtime-selected provider/path will not resolve to `Alias:` or `Function:`. Ordinary environment assignment can use direct `$env:NAME=...` syntax and workspace file changes use the stable document/file workflow instead of weakening this boundary.
- PowerShell object-member mutation is a dynamic command-engine boundary and is review-required for any visible member assignment, compound assignment, or increment/decrement. This structurally covers command-resolution hooks such as `$ExecutionContext.InvokeCommand.CommandNotFoundAction = ...` even if the engine object is first obtained indirectly, without relying on an `ExecutionContext` or property-name denylist. Ordinary property reads remain allowed; member-looking text inside quoted data remains inert.
- PowerShell command-definition and dynamic command-resolution surfaces are review-required before ordinary execution. Function-like language forms (`function`, `filter`, Windows PowerShell `workflow`, DSC `configuration`), module/session/snap-in import-definition surfaces, `using` and `#requires` cannot be used to move the eventual executable target outside the reviewed command text. `Get-Command`/`gcm` remains review-required when the command name is runtime-selected, when the result participates in a pipeline or follow-on execution, when executable metadata such as `.ScriptBlock` is extracted, or when any additional syntax makes the resolution path non-literal. The narrow exception is a standalone read-only diagnostic exactly shaped as `Get-Command|gcm <single literal command name>`; it does not execute the discovered command and must not be escalated solely for using `Get-Command`. Script-like targets include `.ps1`, `.psm1`, and `.psd1`. A visible `.ScriptBlock` property read is review-required because the resulting executable object can be handed directly to scriptblock-consuming cmdlets (for example `ForEach-Object -Process`) without `&` or `.Invoke()`. Every trusted PowerShell launch additionally sets `PSModuleAutoLoadingPreference` to **Constant `None` before user code**, so changing `PSModulePath` cannot make an otherwise ordinary unknown command auto-load executable module code. Direct or cmdlet-based attempts to mutate that preference/variable state are review-required as defense in depth. Quoted documentation strings remain data.
- this review boundary does not claim to turn Full mode into an OS sandbox. Full remains reviewed ordinary execution under the current Windows user token as required by the numbered security authority; descendants of an already reviewed ordinary coding process remain governed by the OS/user boundary. The policy guarantee here is that LocalBridge itself never treats an **unreviewable shell invocation target** as ordinary merely because the strings `docker` / `podman` / `wsl` were hidden or assembled at shell runtime.
- schema32 separates Windows OS system management from the LocalBridge control plane. A statically selected `reg.exe`, `schtasks.exe`, `sc.exe`, or `netsh.exe` command target is privilege-required on the ordinary public route in both Full and Elevated; an Active Broker never changes that ordinary-route token or decision. This is target classification rather than a global word ban: documentation/data arguments such as `Write-Output reg.exe` or `echo reg.exe`, and the narrow read-only `Get-Command reg.exe` diagnostic, remain ordinary. The separate reviewed `elevated_exec` route is the only administrator path and is owned by LB-012; its exact trusted System32 identity/argv policy is not widened by LB-007.
- denied calls project `Blocked` without first projecting `Running`; allowed calls project `Running` only immediately before the real upstream call, then return to `Idle` (or briefly `Failed` then `Idle` on runtime failure).
- task summaries use minimal request fields and always pass through `SafeTaskSummary`; patch bodies and stdin payloads are never UI summaries.
- MCP policy modules do not mutate `WorkspaceRegistry` or active workspace state. Session-local `set_default_cwd` is distinct from LocalBridge workspace authorization and remains constrained by the upstream active-workspace runtime.
- the PEP owns one downstream Tunnel MCP session, accepts bounded concurrent HTTP connections, and preserves downstream JSON-RPC request IDs when forwarding `tools/call`; `notifications/cancelled` is forwarded promptly over the authenticated upstream session without waiting for the serialized Guard execution lock. Actual guarded tool execution remains serialized so the product still has exactly one ephemeral `CurrentTaskStatus` projection.

The older private-name `McpGuard` remains only as a compatibility/test layer for the pinned upstream review surface. Production public traffic enters through `PolicyEnforcementRuntime → AgentFacade → CapabilityPolicy::decide_public → WorkspaceRuntimeAdapter`; the adapter translates an already-authorized stable LocalBridge action to private runtime primitives and cannot grant public authority itself.
