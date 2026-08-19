# LocalBridge Agent Rules

This is the root agent instruction file. Keep it small and stable; do not duplicate evolving schema/history here.

## 1. Start from live facts

- Load `AGENTS.md` first.
- Read `PR_INDEX.json` and `PROJECT_STATE.json` to resolve the live `current_group`, `current_pr`, status, and gate state. `START_HERE.md` is an authority index/bootstrap document, not live state.
- For the current task, read only the relevant current-PR contract in `PR_CONTRACTS.json` and the authority documents referenced by `START_HERE.md`; inspect source, tests, Git history, runtime policy, and skills only as needed.
- Current disk code and real tests define implementation reality. `PR_CONTRACTS.json` defines current machine requirements. `PR_INDEX.json` + `PROJECT_STATE.json` define live governance state. Historical reports, old PASS/FAIL, and chat are clues only.
- If current authoritative sources contradict each other, stop and report the conflict instead of guessing.

## 2. Execute narrowly

- Single agent, serial execution. Work only on `current_pr` and its `writable_paths` or explicit exceptions.
- Do not start the next PR automatically. Do not modify unrelated files, clean unknown untracked files, or stage other work.
- Keep product changes and governance/provenance changes separate. Never fabricate review, PASS, authorization, provenance, or human approval.
- Any command/session/operation that returns a non-terminal state must be followed to a durable terminal state before declaring completion. If it makes no bounded progress, terminate it and record the reason.

## 3. Prefer the smallest complete implementation

Use this order: no change if unnecessary → reuse existing code → standard library → native platform capability → already-installed dependency → minimal new implementation.

Fix shared root causes rather than isolated symptoms. Avoid speculative abstractions, scaffolding, duplicate services, and unnecessary dependencies. Do not simplify explicit requirements, trust boundaries, security, data-loss protection, accessibility, required tests, or maintainability. Read `skills/ponytail/SKILL.md` only when deeper guidance is needed.

## 4. Preserve hard boundaries

- Public API and policy fail closed.
- Edit: structured workspace operations only; no ordinary process/Shell execution.
- Full: structured LocalBridge path/workdir operations remain active-workspace-bound; ordinary development processes run with the current non-admin Windows token and do not imply OS-level child-process filesystem confinement.
- Elevated: administrator capability is available only through the explicit reviewed Broker/UAC privileged route; ordinary routes remain non-admin. LocalBridge control-plane mutation remains denied.
- Never expose secrets in plaintext. Backend owns lifecycle, permission, runtime-health, task, retry, and readiness truth; frontend is typed projection/intent only.

## 5. Verify and stop

Run targeted tests first, then the current contract's required PR/Group gates. Static markers and historical evidence never replace real behavior tests. Update governance only when the current contract authorizes it. Once the current PR/review step reaches its required terminal state, report the result and stop.

The current schema/revision must always be read from `PR_CONTRACTS.json`; do not hard-code changing schema details into this file.
