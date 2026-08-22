# ADR-0001 — MCP Policy Enforcement

Status: **ACCEPTED FOR IMPLEMENTATION**
Decision owner: LB-000
Evidence date: 2026-08-11

## Schema46 authority amendment — 2026-08-22

The earlier ordinary-shell review model is superseded. An arbitrary shell, script, build tool,
test binary, or interpreter can create descendants and invoke current-user APIs after the initial
command text has been authorized. Command-string classification therefore cannot be a descendant
capability boundary.

The enforceable contract is now:

- Edit has no process execution capability.
- Full ordinary `exec_command` runs under the current Windows user token. The shell and every
  descendant have that same OS authority; command spelling does not create a narrower token.
- Elevated ordinary `exec_command` still uses the current Windows user token. Selecting Elevated
  never upgrades the ordinary route.
- Administrator-token execution is a separate structured Broker route with exact program/argument,
  workspace, lifecycle, and active-Broker checks.
- LocalBridge control-plane mutation remains denied on every shell and Broker route.
- Filesystem authorization remains path-based and is not inferred from shell text.

The removed classifier is not retained as an audit owner or compatibility writer. Job Objects remain
the kernel process-tree ownership/cancellation boundary, not an authorization boundary. LocalBridge
must not claim per-descendant capability isolation until a supported sandbox can preserve required
coding child-process workflows and pass the public black-box matrix without a weaker fallback.

## Context

LocalBridge requires three user-visible permission modes, but the security boundary is stricter than a cosmetic `tools/list` filter. Edit mode must never obtain process execution; Full may execute with the current Windows user token; Elevated adds only explicitly reviewed Broker-routed administrator operations. Unknown capabilities and control-plane mutation are always denied.

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
- Full → current-Windows-user execution; arbitrary shell descendants inherit the same token;
- Elevated → the same ordinary MCP capability ceiling as Full; privileged execution is a separate Broker route;
- LocalBridge control-plane mutation → deny always;
- `request_permissions` → deny in all LocalBridge modes so upstream can never expand LocalBridge authorization;
- declare transitive structured tool capabilities before execution; shell text is not a capability declaration;
- enforce the single active workspace identity before forwarding filesystem calls;
- project actual allow/deny/execution state into redacted `CurrentTaskStatus`;
- bind Guard and upstream listener to loopback only;
- prevent Tunnel from targeting coding-tools-mcp directly;
- authenticate the Guard→upstream hop with runtime-generated internal authorization material;
- launch upstream with anonymous telemetry disabled.

The upstream runtime is an authenticated, loopback-only execution adapter and is deliberately policy-neutral behind the Guard. Its own permission gates are disabled because they implement a second, partly command-string-based authority owner that cannot constrain descendants. The runtime-generated bearer is never exposed to Tunnel or public tools, Tunnel cannot target the upstream listener, and every public call is authorized and path-resolved by the Rust Guard before adapter dispatch. A `Safe` runtime remains available only for isolated direct runtime tests; it is not the production Guard-backed route.

## Stable adapter boundary

Domain and UI code must not depend on upstream private tool classes, Python module layout, FastMCP internals, or raw third-party fault objects. The adapter exposes stable LocalBridge concepts: tool identity, classified capability, redacted task summary, execution result/fault, session lifecycle, and active workspace boundary.

Any upstream upgrade must regenerate structural and capability diffs before the Guard policy is changed. New tools remain denied until reviewed.

## Consequences

This adds a small first-party MCP protocol boundary, but avoids both insecure list-only filtering and a long-lived fork of upstream. It also makes cached `tools/list`, future workflow-like tools, and upstream permission semantic changes fail closed.

The LB-000 live Tunnel credential blocker does not affect this decision because the PEP conclusion derives from direct local execution of the pinned coding-tools runtime.

## LB-007 production boundary

LB-007 implements the accepted decision inside `src-tauri/src/mcp/**` in front of the LB-006 stable runtime adapter. The production authorization identity is now the **LocalBridge public tool/action**, not an upstream primitive name. `policy.rs` owns the stable public classifier and transitive capability declarations, `AgentFacade` applies that policy before adapter dispatch, and `server.rs` is the first-party loopback Streamable HTTP PEP consumed by Tunnel. `runtime-policy.toml` schema 7 contains a `localbridge_public` policy that may narrow the frozen public maxima but may not widen them.

- `tools/list` filtering is UX-only and never grants authority.
- every `tools/call` receives the current `PermissionMode` and current LocalBridge public policy and is re-authorized immediately before adapter dispatch; a cached Full-mode catalog cannot bypass a later switch to Edit or a later policy narrowing.
- the public classifier recognizes only the versioned LocalBridge Registry/action vocabulary. Raw upstream names are not public authorization anchors and cannot be called as a bypass.
- the exact upstream v0.2.2 surface remains an **internal adapter compatibility baseline** for LB-006 capability negotiation only. It is not the public PEP identity and upstream additions never become public automatically.
- `request_permissions` and LocalBridge workspace/permission/credential/tunnel/MCP configuration names are `ControlPlane` and denied in every mode.
- high-level public workflows declare their complete transitive LocalBridge read/write/process/git/network/privilege requirements before execution. Edit denies any workflow containing process execution; unreviewed network and privilege routes fail closed; unknown actions/capabilities fail closed.
- ordinary shell authorization is based only on the effective mode and its declared current-user token; command text, aliases, scripts, interpreters, and descendants do not create a second authority fact.
- structured Broker operations retain exact target and argument review because that route owns an administrator token and does not accept arbitrary interpreters.
- denied accepted work terminalizes `Blocked` without first fabricating `Running`; admitted work follows explicit `Queued → Running → Terminal` state and terminal outcome is never inferred from a later `Idle` presentation.
- task summaries use minimal request fields and always pass through `SafeTaskSummary`; patch bodies and stdin payloads are never UI summaries.
- MCP policy modules do not mutate `WorkspaceRegistry` or active workspace state. Public structured path/workdir fields are resolved by LocalBridge WorkspaceResolver before adapter dispatch; the policy-neutral upstream runtime is not a second workspace authority owner.
- the PEP accepts multiple downstream MCP sessions, scopes JSON-RPC identity by `(McpSessionId, RequestId)`, and uses bounded Observation/Control/Work admission. Work enters the bounded FIFO as task data; cancellation and command control remain available through their dedicated lanes and affect only owned resources.

The older private-name `McpGuard` remains only as a compatibility/test layer for the pinned upstream review surface. Production public traffic enters through `PolicyEnforcementRuntime → AgentFacade → CapabilityPolicy::decide_public → WorkspaceRuntimeAdapter`; the adapter translates an already-authorized stable LocalBridge action to private runtime primitives and cannot grant public authority itself.
