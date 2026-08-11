# LB-000 — Upstream + Security Compatibility Report

Date: 2026-08-11
Overall LB-000 status: **PASS**
Deterministic/security spike status: **PASS**

The required live OpenAI Tunnel PoC also passed. LB-000 acceptance is complete, but this is the end of G0: LB-001 remains blocked until the mandatory independent G0 adversarial review passes.

## Pinned upstream identities

### coding-tools-mcp

- public version: `0.2.2`;
- commit: `311c1f2529d0f047ad2a8b68db6bf92dbb93d6bc`;
- Git tree: `5ef5e638a12aa74dc8836ca02a88490c2fe019a6`;
- deterministic `git archive --format=tar HEAD` SHA256: `227b94128eaacda8d63d391911db1918e0bd813718d9ea5d276b5aab7eac73fd`;
- live tool count: 20;
- canonical live `tools/list` schema SHA256: `d187db7ad6729893f17fceb539f030eb0c6139528ab35cb3f9564d27383d3674`.

The GPT-WebCodex vendored self-reported `0.4.1` is not used as upstream truth.

### openai/tunnel-client

- version: `0.0.11`;
- commit: `8d55683eeef80bc5e360d95abf4692454fafc615`;
- Git tree: `f55ee74dd024b5e69714ec25c94a579276798b7f`;
- official Windows amd64 asset: `tunnel-client-v0.0.11-windows-amd64.zip`;
- official and locally recomputed asset SHA256: `eb912c86c6ccde90cda805cb17009507176a656725cf86c36fabe1901a12e29b`;
- bundled `tunnel-client.exe` SHA256: `7d3c7d492ce84b52835e11865a835a8a5bcd4a669dee84e169aa11b314dc952a`;
- bundled cloudflared: `2026.7.2`.

## coding-tools capability findings

Real Streamable HTTP MCP initialization, `tools/list`, and `tools/call` were exercised on Windows. `safe` and `trusted` expose the same catalog, and `safe` successfully runs `exec_command`. Upstream permission mode therefore cannot implement LocalBridge Edit/Full isolation.

The selected internal hop was also exercised with upstream bearer authentication enabled through `CODING_TOOLS_MCP_AUTH_TOKEN`. An unauthenticated `initialize` was rejected with HTTP 401; authenticated initialization issued an MCP session ID that was reused on subsequent calls. This validates the planned authenticated loopback Guard→coding-tools hop without embedding the bearer in the process command line.

Direct `..` traversal, a real Windows junction pointing outside the workspace and a real Windows directory symlink pointing outside the workspace were all rejected by `read_file`; the outside sentinel never leaked. This behavior is retained as defense in depth, not delegated as LocalBridge's sole authorization boundary.

Unknown tools fail closed. `request_permissions` currently reports `ELICITATION_UNSUPPORTED` under ordinary modes, but LocalBridge will deny the tool because LocalBridge authorization must remain non-delegable across upstream upgrades.

Decision: `docs/ADR-0001_POLICY_ENFORCEMENT.md` selects a first-party Rust MCP Guard/PEP with mandatory `tools/call` enforcement and stable capability classification.

## Process ownership finding

The Windows Job Object PoC passed. Closing a Job configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` terminated both the assigned root and its nested child. PID-only ownership is rejected.

Decision: `docs/ADR-0002_PROCESS_OWNERSHIP.md` selects per-generation Windows Job Object ownership.

## Portable Python finding

A Python.org Windows embeddable `3.12.10` sample was used for feasibility. Archive SHA256: `4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3`.

The embedded `python.exe` directly imported coding-tools-mcp v0.2.2 and PyJWT, displayed the real CLI, and executed the same live HTTP MCP/path probe successfully without runtime pip installation. Dependency staging was a build-time step only.

This proves the packaging model is feasible; LB-006 still owns the final exact Python 3.12.x patch, hash and packaged dependency layout.

## Tunnel secret injection and health contract

`tunnel-client` v0.0.11 accepts `--control-plane.api-key env:VARNAME` or `file:/path`. The selected LocalBridge contract is a child-process-only environment value referenced on the command line as `env:LOCALBRIDGE_RUNTIME_API_KEY`. A generated sentinel was absent from argv, OS-observed command line, stdout and stderr.

Health/admin was successfully bound to `127.0.0.1:0` using `--health.url-file`.

A negative control-plane probe discovered an important semantic boundary: `/readyz` returned HTTP 200 while the configured control-plane endpoint was deliberately unreachable. `/api/status` simultaneously exposed `tunnel_metadata_error`. Therefore LocalBridge must not treat `/readyz` alone as Tunnel/OpenAI Ready. The stable Tunnel adapter must combine local process/startup readiness with typed control-plane evidence.

## Stable adapter decision

Third-party private structures do not cross into LocalBridge domain contracts. The coding runtime is accessed through the Rust MCP Guard/stable adapter; the Tunnel runtime is accessed through a Tunnel adapter that owns CLI construction, child-only secret injection, health/status parsing and typed fault mapping; Windows process ownership is exposed through the supervisor abstraction rather than raw PIDs.

## Gate result

| Required LB-000 gate | Result | Evidence |
|---|---|---|
| real upstream tools/list | PASS | `compatibility/coding-tools/0.2.2/tools-list.json` |
| junction/symlink/reparse adversarial spike | PASS | `compatibility/coding-tools/0.2.2/path-probe.json` |
| Job Object PoC | PASS | `spikes/lb-000/job-object-result.json` |
| portable Python PoC | PASS | `spikes/lb-000/portable-python-result.json` |
| live Tunnel PoC | PASS | `spikes/lb-000/live-tunnel-result.json` |
| structural diff baseline generation | PASS | `compatibility/coding-tools/0.2.2/structural-diff.json` |
| capability baseline serialization | PASS | `compatibility/coding-tools/0.2.2/capability-map.json` |

The live probe observed authenticated control-plane metadata using transient stdin-to-child-environment credential injection. Neither the API key nor Tunnel ID was placed on the tunnel-client command line, and the retained result contains no credential value. G0 must now stop for independent adversarial review.
