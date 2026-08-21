# LocalBridge test structure

The test tree is organized by behavior boundary, not by refactor generation:

- `src/__tests__`: typed UI model and presentation unit tests.
- `src-tauri/src/**`: unit tests that need private module access.
- `tests/unit`: public Rust and source-contract unit tests.
- `tests/integration`: process, MCP, control-plane, privilege, recovery, and packaging integration tests.
- `tests/e2e`: native-window and user-flow tests.
- `scripts/test`: the single ordered gate used both locally and by GitHub Actions.

Shared rules:

1. Add a gate stage once in `scripts/test/ci-gate.mjs`; do not duplicate it in the workflow.
2. Use the MCP helpers in `src-tauri/src/mcp/test_support.rs` for session identities, JSON-RPC calls, detached-command convergence, and output accumulation.
3. Assert causal state transitions and terminal outcomes. Do not infer behavior from fixed startup delays or an exact number of transport polls.
4. Every process-backed test must stop owned runtimes and clean its workspace even on failure.
5. Keep one behavior family per test. New acceptance coverage should extend a fixture/driver before copying setup code.

Useful local commands:

```text
npm run test:ci:list
npm run test:ci -- --only rust-test
npm run test:ci -- --from rust-clippy
npm run test:ci
```
