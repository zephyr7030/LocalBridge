# LocalBridge test structure

The test tree is organized by behavior boundary, not by refactor generation:

- `src/__tests__`: typed UI model and presentation unit tests.
- `src-tauri/src/**`: unit tests that need private module access.
- `tests/unit`: public Rust and source-contract unit tests.
- `tests/integration`: process, MCP, control-plane, privilege, recovery, and packaging integration tests.
- `tests/e2e`: native-window and user-flow tests.
- `tests/black-box/chatgpt`: an external JSONL client and scenarios that exercise
  the same public MCP session protocol as ChatGPT.
- `scripts/test`: the single ordered gate used both locally and by GitHub Actions.

The ChatGPT simulator is not a production command, debug endpoint, or tool. It
must be given the real loopback PEP or public Tunnel MCP URL and never imports
LocalBridge internals. One client process owns one MCP session so detached
command ownership, cancellation, transport failures, and terminal convergence
remain observable without automatic reconnects or projection fallbacks.

Shared rules:

1. Add a gate stage once in `scripts/test/ci-gate.mjs`; do not duplicate it in the workflow.
2. Use the MCP helpers in `src-tauri/src/mcp/test_support.rs` for session identities, JSON-RPC calls, detached-command convergence, and output accumulation.
3. Assert causal state transitions and terminal outcomes. Do not infer behavior from fixed startup delays or an exact number of transport polls.
4. Every process-backed test must stop owned runtimes and clean its workspace even on failure.
5. Keep one behavior family per test. New acceptance coverage should extend a fixture/driver before copying setup code.
6. Do not add source-marker tests named after historical schema or ARCH generations. Architecture claims require executable behavior in Rust/Vitest or a narrow residue scan.
7. Test scripts must never launch Cargo or npm recursively. Add the behavior to the shared gate once and select that stage for local diagnosis.

Useful local commands:

```text
npm run test:ci:list
npm run test:ci -- --only rust-test
npm run test:ci -- --through rust-clippy
npm run test:ci -- --from schema44 --through rust-test
npm run test:ci -- --from rust-clippy
npm run test:ci
npm run test:mcp-client -- --url https://example.test/mcp
node tests/e2e/onboarding/fixed_window_runtime_e2e.mjs dashboard --production-assets
```

If the desktop app is already running from Cargo's default debug directory, set
`LOCALBRIDGE_CARGO_TARGET_DIR=src-tauri/target/local-gate` for the shared gate. This changes
only Cargo's build-output location; it does not skip or replace any declared stage.

After the client emits `type=ready`, write one JSON object per line:

```json
{"op":"tools/list"}
{"op":"tools/call","name":"workspace_context","arguments":{}}
{"op":"close"}
```
