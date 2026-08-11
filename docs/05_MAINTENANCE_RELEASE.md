
# 05 — Maintenance, Persistence & Release

## 可执行治理

能机器检查的规则不得只留文档。

逐步实现：

```text
verify-architecture
verify-runtime-manifest
verify-ui-language
diff-upstream-surface
generate-sbom
```

`ARCHITECTURE_RULES.json` 是机器约束源。

## 持久化

所有长期数据有 `schema_version`：

- settings；
- project registry；
- recent metadata；
- permission preference；
- credential references；
- runtime desired state。

迁移：

```text
load
→ sequential migration
→ validate
→ temp write
→ atomic replace
```

失败不覆盖旧数据、不自动 reset。

future schema fail-safe。

Credential secret 不进入 migration file。

## Workspace migration

metadata 可持久化：

```text
workspace_id
display_path
validated_identity
last_opened_at
```

旧 single workspace → one registry entry + active reference。

迁移后安全 identity 无法验证时不隐式授权。

LB-003 当前 app-data schema 为 `v3`：

```text
v1 single-workspace preferences
→ v2 nested settings + single workspace
→ v3 WorkspaceRegistry + zero-or-one active_workspace_id
```

- migration 必须逐版本执行，禁止跨版本跳迁；
- remembered registry 仅为便利性元数据，不是 MCP authorization roots；
- registry 只按 WorkspaceValidator 已确认的 `validated_identity` 去重，不按大小写路径字符串去重；
- 无法确认 historical identity 时保留 `pending_workspace_confirmation`，同时保持 `active_workspace_id = null`；
- Windows 写入先写同目录临时文件并 `sync_all`，已有文件通过原子 replace 替换；旧目标保留为 `.bak` 回滚点；
- migration/validation/future-schema 任一失败均不覆盖原文件，也不自动 reset。

## Release

```text
development
→ release candidate
→ stable
```

Stable Gate：

- all PR/group gates PASS；
- architecture verification；
- compatibility baseline；
- lockfiles；
- SBOM/notices/provenance；
- clean Windows 11 E2E；
- installer/uninstaller；
- reboot/background；
- Broker；
- reconnect exhaustion/error-window；
- no orphan process；
- secret scan；
- zero telemetry；
- migration/rollback。

## Rollback

安装/迁移失败：

- 不破坏旧 settings；
- 不删除 credential；
- 保留回滚点；
- 不自动清空 app data。

## Workaround

临时方案必须记录：

```text
reason
scope
owner PR
exit condition
```
