# LocalBridge Agent Rules

This is the root execution instruction file. Keep it small and stable.

## 1. Read live authority first

Read in this order:

1. `AGENTS.md`
2. `CONTROL_PLANE_REFACTOR_CONTRACT.json`
3. `PROJECT_STATE.json`
4. `docs/06_CONTROL_PLANE_REFACTOR_EXECUTION.md`
5. Current source/tests and only the additional baseline documents needed for the active phase.

`PR_INDEX.json`, `PR_CONTRACTS.json`, `docs/06_PR_GROUPS_AND_EXECUTION.md`, old G0-G4 review records, and old LB-PR status are legacy history only. They must never be used to resolve current work, current phase, acceptance, or writable scope.

Current disk code and real tests define implementation reality. The schema44 control-plane contract defines required behavior. If current live authorities contradict each other, stop and report the conflict instead of guessing.

## 2. Execute one refactor phase at a time

The live sequence is strictly:

`R1 → R2 → R3 → R4 → R5`

`PROJECT_STATE.json.current_phase` is the only current-phase pointer. Single agent, serial execution. Do not start the next phase automatically after acceptance.

Do not add unrelated product features while the contract freezes them. Keep product changes and governance/provenance changes separate. Never fabricate PASS, review, authorization, provenance, or human evidence.

## 3. Ownership migration is the primary rule

Every authoritative fact must have exactly one mutable owner.

For each migrated fact use strangler migration only:

`new owner → migrate readers → migrate writers → disable old writer → delete old truth`

Long-lived dual-write between legacy state and new ControlPlane state is forbidden. Migrate ownership before moving files or reorganizing modules.

## 4. Preserve hard boundaries

- Public API and authorization fail closed.
- Request identity is MCP-session scoped.
- Task and Execution have stable identities and exactly one terminal outcome.
- Cancellation affects only the declared ownership scope.
- Accepted Work is explicitly Queued, Running, or Terminal; Mutex contention is not business state.
- Desired, Observed, and Effective state are distinct; Effective authority/workspace is derived fail-closed.
- UI reads revisioned ControlPlane snapshots; lock contention must never fabricate Running.
- Transport code cannot own domain lifecycle state.
- Core lifecycle/control-plane state must be typed Rust state, not opaque `serde_json::Value`.

Existing Edit / Full / Elevated security boundaries remain in force unless the current refactor contract explicitly changes how their state is owned or derived.

## 5. Verify behavior, not names

Target the current phase and the contract invariants first. Concurrency, cancellation, session lifecycle, terminal convergence, recovery, and snapshot consistency require executable behavior tests; source markers or function names are not substitutes.

Any command/session/operation returning a non-terminal state must be followed to a durable terminal state before completion is claimed. Once the current phase reaches its required terminal acceptance state, report and stop.
