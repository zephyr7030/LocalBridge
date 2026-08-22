# LocalBridge — Start Here

> Current governance model: **schema44 Control Plane Refactor**
> Effective: **2026-08-21**
> Live sequence and status: read from **`PROJECT_STATE.json`**

The former LB-PR sequence and G0-G4 group gates are abolished as live governance. Their records remain historical evidence in Git history only.

## Current read order

1. `AGENTS.md`
2. `CONTROL_PLANE_REFACTOR_CONTRACT.json`
3. `PROJECT_STATE.json`
4. `docs/06_CONTROL_PLANE_REFACTOR_EXECUTION.md`
5. Current source and tests relevant to the active phase
6. Baseline product/security documents only as needed:
   - `docs/01_PRODUCT_UX.md`
   - `docs/02_ARCHITECTURE_RUNTIME.md`
   - `docs/03_SECURITY.md`
   - `docs/04_UPSTREAM_DISTRIBUTION.md`
   - `docs/05_MAINTENANCE_RELEASE.md`
   - `docs/07_ACCEPTANCE.md`
   - `ARCHITECTURE_RULES.json`
   - `COMPATIBILITY_BASELINE.json`
   - `runtime-manifest.toml`
   - `runtime-policy.toml`

## Legacy-only files

The following files are not live execution authority:

- `PR_INDEX.json`
- `PR_CONTRACTS.json`
- `docs/06_PR_GROUPS_AND_EXECUTION.md`
- `docs/08_FINAL_REVIEW.md`
- `FINAL_REVIEW.json`
- historical G0-G4 review generations
- historical LB-000..LB-019 acceptance records

If a legacy document conflicts with the schema44 contract, the legacy statement is ignored for current refactor execution. Historical release facts remain historical facts; they simply do not define current work.

## Current objective

Pause new upper-layer capability work and first converge the LocalBridge control plane. This is not a rewrite: do not split the Rust crate, replace the existing runtime, add a database, or introduce a complex event system.

The refactor addresses seven root causes:

1. no single mutable owner for each authoritative fact;
2. incomplete Request / Task / Execution / Session identity;
3. implicit Mutex serialization instead of an explicit Scheduler;
4. split Task / Execution / Public Session lifecycles;
5. conflated Desired / Observed state;
6. Projection / Error not modeled as first-class control-plane state;
7. MCP code owning Domain / Runtime responsibilities.

Primary invariant:

> Every authoritative fact has exactly one mutable owner.

## Current phase

`PROJECT_STATE.json.current_phase` is the only phase pointer. This document intentionally does not copy its value. No PR or G pointer exists anymore.

## Phase map

```text
R1 Identity / Isolation
→ R2 Task / Execution Convergence
→ R3 Explicit Scheduler + Session Lifecycle
→ R4 Desired / Observed Convergence
→ R5 Snapshot / Typed UI
→ R6 Boundary Cleanup
→ R7 Product Lifecycle
```

Follow the advancement rule in `AGENTS.md` and the live execution contract.

## Feature freeze

Do not expand the concurrency surface outside the active contract with:

- multi-window enhancement;
- window observation/control;
- browser automation;
- complex workflow expansion;
- additional long-lived background execution capabilities.

Feature scope and freezes are defined only by the live contract and `PROJECT_STATE.json`.

## Migration discipline

For every authoritative fact:

```text
establish new owner
→ migrate readers
→ migrate writers
→ forbid old owner writes
→ delete old truth
```

Long-lived dual-write is forbidden. Ownership migration precedes file/module movement.

## Acceptance focus

Current acceptance is invariant-based rather than PR-name/string-marker based. The required core invariants are INV-01 through INV-09 in `CONTROL_PLANE_REFACTOR_CONTRACT.json`, with mandatory adversarial coverage for cross-session request isolation, scoped cancellation, bounded queueing, detached execution projection, session reaping, exactly-once terminal outcomes, runtime-loss convergence, lock contention, partial projection faults, Desired/Observed fail-closed derivation, and single-revision snapshots.

The refactor is complete only when there is no bare global JSON-RPC request registry, no cancel-all disguised as `task_control(cancel)`, no single `current_command` acting as multi-execution truth, no UI-guessed completion, no fabricated Running from lock contention, no live composite MainProjection, no core `Result<T,String>` UI boundary, no manually synchronized duplicate Permission/Workspace truths, and no MCP Transport ownership of domain lifecycle.
