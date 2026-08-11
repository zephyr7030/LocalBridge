# coding-tools-mcp v0.2.2 behavior baseline

Pinned source: `311c1f2529d0f047ad2a8b68db6bf92dbb93d6bc`.

LB-000 launched the pinned runtime over real Streamable HTTP MCP on Windows and performed `initialize`, `tools/list`, and `tools/call` operations. The live catalog contains 20 tools and has canonical schema SHA256 `d187db7ad6729893f17fceb539f030eb0c6139528ab35cb3f9564d27383d3674`.

`safe` and `trusted` publish the same catalog. In `safe`, `exec_command` remains visible and a bounded `cmd.exe /d /c echo LB000_EXEC` call executes successfully. `apply_patch` also remains available and mutates a workspace fixture. Therefore upstream permission modes are not LocalBridge Edit/Full authorization modes.

Direct path traversal, an actual Windows junction to an outside directory, and an actual Windows directory symlink to an outside directory were all denied by `read_file`; the outside sentinel did not leak. This is useful defense in depth, but LocalBridge must still enforce its own active-workspace identity before forwarding calls because the upstream boundary is versioned third-party behavior.

Unknown tools fail closed with JSON-RPC `reason=unknown_tool`.

`request_permissions` currently returns `ELICITATION_UNSUPPORTED` in safe/trusted and only reports an automatic grant in upstream dangerous mode. LocalBridge must nevertheless hide/deny it so a future upstream permission implementation can never mutate LocalBridge authorization semantics.

Upstream anonymous telemetry is on by default in v0.2.2. LocalBridge must launch it with telemetry disabled and must regression-test this on every runtime upgrade.
