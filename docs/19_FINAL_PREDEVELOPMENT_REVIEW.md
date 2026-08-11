# 19 — Final Predevelopment Review

Review date: 2026-08-10  
Baseline: `LocalBridge-Dev-Preflight-v12-FINAL`  
Result: **PASS — ready for LB-000**

## Audited

- package inventory integrity;
- JSON/TOML parseability;
- PR_INDEX ↔ PR_CONTRACTS one-to-one coverage;
- PR DAG cycle/dependency validity;
- strict sequential execution authority;
- writable/forbidden path authority;
- governance state-transition authority;
- dependency manifest write authority;
- UI terminology consistency;
- Edit / Full / Elevated permission consistency;
- Privileged Broker boundary;
- runtime packaging/update policy;
- live-vs-deterministic test gates;
- persistence/migration policy;
- compatibility baseline policy;
- release/SBOM/provenance policy.

## Findings corrected during final review

1. Removed stale `LB-017` live-release references; release live gate is `LB-019`.
2. Corrected stale `LB-016` packaging references; packaging gate is `LB-018`.
3. Rewrote UX spec so it no longer specifies only Edit/Full and no longer exposes unnecessary English UI terms.
4. Added the administrator mode to the primary permission contract.
5. Added legal `spikes/lb-000/**` authority so LB-000 can perform PoCs before repository bootstrap.
6. Added restricted dependency-manifest write authority so later PRs can add only their own necessary dependencies.
7. Added restricted governance write authority so a PR can legally mark itself PASS and advance exactly one sequential PR.
8. Added strict machine-readable execution order.
9. Added release-artifact authority for SBOM/provenance/acceptance evidence.
10. Added LB-019 authority to finalize changelog/security/compatibility release evidence.
11. Corrected loopback architecture verification semantics to exact parsed addresses.
12. Cleaned execution/review templates so all required maintenance authorities are read before work begins.

## Remaining dynamic decisions

These are intentionally **not** blockers in the predevelopment package. LB-000/LB-006 must resolve them from real evidence:

- exact coding-tools-mcp capability surface;
- PEP implementation: upstream enforcement / adapter / Rust Guard;
- tunnel-client secure secret injection mechanism;
- Job Object behavior;
- Windows reparse-point behavior;
- exact Python 3.12.x embedded patch/runtime layout;
- real Tunnel live compatibility.

If live Tunnel credentials are unavailable, LB-000 may report an external credential blocker only for that live sub-gate; it must still complete every deterministic/spike item that does not require the credential.

## Execution authority

The development agent must start with:

```text
current_pr = LB-000
```

and execute no other PR until LB-000 acceptance passes.


## v10 Addendum — Current task status UX

开发前新增并冻结：

- 主控界面只显示一个当前任务状态；
- 内容为类型 / 安全摘要 / 执行状态；
- 不做最近活动、消息流、时间线；
- 不显示模型隐藏思考或 ChatGPT 最终回答；
- 数据源必须是真实 MCP/Broker 执行；
- 摘要必须脱敏。

不增加 PR 数量，职责并入 LB-002/LB-007/LB-009/LB-012/LB-015。


## v11 Addendum — Minimal inline activity state

Current execution UI was further reduced before development:

```text
●  运行测试  cargo test
```

- no section title;
- no field labels;
- no redundant “执行中” text;
- active green dot uses subtle pulse;
- reduced-motion uses static green dot;
- no feed/history.


## v12 Addendum — Credentials and project registry

Frozen before development:

### Runtime API Key

- secure Windows credential backend only;
- plaintext persistence forbidden;
- no CLI secret argument;
- UI never reveals saved secret;
- logs/diagnostics redacted;
- unsupported safe tunnel injection fails closed.

### Project management

- remembered project registry supports add/select/remove;
- only zero or one active workspace is authorized;
- remembered projects do not become multiple authorized roots;
- remove operation never deletes disk files;
- removing active project stops runtime and enters NoActiveWorkspace;
- no automatic authorization of another remembered project;
- MCP cannot mutate project registry/current workspace.

No additional PR was added. Responsibilities are integrated into LB-003/LB-005/LB-008/LB-010/LB-015.
