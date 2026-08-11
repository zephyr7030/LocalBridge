# 16 — Compatibility Policy

## 上游兼容基线

每个正式 LocalBridge Release 必须保存：

- coding-tools-mcp tools/list snapshot；
- tool schema；
- capability classification；
- command/workflow behavior notes；
- tunnel-client CLI surface；
- tunnel-client health/readiness contract；
- runtime manifest；
- exact upstream versions / commits / SHA256。

## 升级规则

任何上游升级必须输出：

```text
old baseline
→ new baseline
→ structural diff
→ capability diff
→ security impact
→ migration/adapter impact
→ decision
```

禁止只写：

```text
upgrade v0.2.2 → v0.2.3
```

## Breaking Change 判定

下列任一变化都视为 breaking-risk：

- tool removed；
- tool renamed；
- input schema changed；
- output semantic changed；
- previously read-only tool now may execute process；
- workflow obtains new transitive capability；
- auth behavior changed；
- transport/session semantics changed；
- tunnel CLI flag changed；
- health/readiness semantics changed。

## Unknown Capability

任何新增工具：

```text
unknown → deny
```

直到 compatibility review 完成。

## Versioning

v0.x：

- 允许快速演进；
- 但 settings/runtime policy 仍必须迁移；
- 不得 silent data loss。

v1.0+：

- settings schema 不允许无迁移破坏；
- 删除功能需 deprecation；
- tool/runtime change 必须有兼容说明。

## Baseline Artifacts

建议：

```text
compatibility/
├─ coding-tools/
│  └─ <runtime-version>/
│     ├─ tools-list.json
│     ├─ capability-map.json
│     └─ behavior-notes.md
└─ tunnel-client/
   └─ <runtime-version>/
      ├─ cli-surface.json
      └─ health-contract.json
```
