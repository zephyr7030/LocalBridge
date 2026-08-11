# Security Policy

## Scope

Security-sensitive areas include:

- MCP policy enforcement;
- workspace path confinement;
- command execution;
- Privileged Broker and elevated_exec;
- secret storage and redaction;
- loopback listeners;
- tunnel authentication;
- runtime supply chain.

## Rules

- Never include real secrets in issues, fixtures, logs, screenshots, or diagnostic samples.
- Reproduce security problems with synthetic credentials.
- Do not weaken fail-closed behavior to make a failing test pass.
- Unknown capabilities remain denied until reviewed.
- LocalBridge control-plane remains non-delegable in all permission modes.
- Privileged Broker must never expose an unauthenticated network listener.

## Disclosure

Before public release, define the final vulnerability reporting channel. Until then, security findings remain project-internal and must be documented without secret material.
