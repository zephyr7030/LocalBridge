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
