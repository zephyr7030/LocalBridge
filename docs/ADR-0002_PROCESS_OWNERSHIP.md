# ADR-0002 — Windows Process Ownership

Status: **ACCEPTED FOR IMPLEMENTATION**
Decision owner: LB-000
Evidence date: 2026-08-11

## Context

LocalBridge must own MCP, Guard, Tunnel and their descendant processes without relying on PID identity. PID-only cleanup is unsafe under PID reuse, stale persisted state and nested build/test process trees.

LB-000 executed a Windows Job Object PoC using `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. A root process was assigned to the Job before it spawned a nested child. Both were alive before Job closure and both terminated after the Job handle was closed.

Evidence: `spikes/lb-000/job-object-result.json`.

## Decision

The production Windows process supervisor will use a Job Object as the authoritative ownership primitive for each managed runtime generation.

Required behavior:

- create the Job before managed child work begins;
- apply `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`;
- assign the root process before it can spawn uncontrolled descendants; production implementation should use an appropriate suspended/controlled launch sequence;
- retain the Job handle for the complete runtime generation;
- treat PID as diagnostic metadata, never as final ownership proof;
- attempt graceful shutdown first, wait for a bounded interval, then terminate/close the owned Job tree when required;
- do not enable breakaway for ordinary runtime descendants;
- coding-tools-mcp build/test descendants inherit the MCP runtime ownership tree;
- tunnel-client-managed descendants such as bundled cloudflared inherit the Tunnel ownership tree;
- a new runtime generation receives a new Job identity and must not trust stale PID records.

LB-004 owns the production supervisor implementation and detailed PID-reuse/stale-state/graceful-stop tests. The Elevated Broker remains a separate security boundary and must be independently validated in LB-011/LB-012 because Windows elevation can alter Job assignment behavior.

## LB-004 implementation

The production supervisor implements the decision with this launch sequence:

```text
CreateJobObjectW
→ SetInformationJobObject(KILL_ON_JOB_CLOSE)
→ CreateProcessW(CREATE_SUSPENDED)
→ AssignProcessToJobObject
→ capture process creation FILETIME
→ ResumeThread
```

The Job handle is the ownership capability. A diagnostic snapshot contains role, PID, generation, process creation time and launch timestamp, but persisted/stale snapshots are reconciliation input only and are never accepted as authority to terminate a PID. A generation mismatch is stale; matching generation/PID with a different process creation time is treated as PID reuse / identity mismatch.

Shutdown is cooperative-first: the caller requests a typed/runtime-specific graceful stop through a callback, the supervisor polls Job accounting for a bounded interval, and only if owned processes remain does it call `TerminateJobObject`. Closing the supervisor also closes the `KILL_ON_JOB_CLOSE` Job, so nested descendants remain kernel-owned even if the root exits or fails to cooperate. No ordinary-runtime breakaway flag is enabled.

## Alternatives rejected

PID-only tracking is rejected because PID reuse makes it possible to target an unrelated process. Recursive process enumeration is rejected as the primary boundary because it races process creation/exit and is not kernel ownership. Unmanaged detached descendants are rejected because application exit/crash would leave undefined orphans.

## Consequences

Kernel Job ownership becomes the source of truth for process-tree cleanup. Persistent state may record process/generation diagnostics, but restart recovery must validate live ownership instead of killing by stale PID.
