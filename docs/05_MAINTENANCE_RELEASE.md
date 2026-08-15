
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
- `validated_identity` 的持久化值只是“上次验证时观察到的 identity claim”，反序列化后不具备授权能力；运行时 `ValidatedWorkspaceIdentity` 不支持 JSON 反序列化，只能由 `WorkspaceValidator` 从实际文件系统对象生成；
- 从持久化状态恢复 Active workspace 时必须重新打开 `display_path`，以 Windows 文件句柄取得 volume/file identity 与 final path，并与持久化 claim 匹配；不匹配、路径被替换或目录已消失时 fail-closed，不能生成 Active；
- Active domain root 使用本次验证得到的 final path，而不是直接信任持久化 `display_path`；
- 无法确认 historical identity 时保留 `pending_workspace_confirmation`，同时保持 `active_workspace_id = null`；
- Windows 写入先写同目录临时文件并 `sync_all`，已有文件通过原子 replace 替换；旧目标保留为 `.bak` 回滚点；
- migration/validation/future-schema 任一失败均不覆盖原文件，也不自动 reset。

## Schema28 测试编排

测试按成本和目的固定分三层：

```text
PR Fast Gate
  当前 PR 的廉价 deterministic unit / fake / static checks
  开发内循环优先 targeted execution

PR Runtime Gate
  真实 bundled runtime / PEP / process 行为
  每次正式 PR acceptance attempt 至多按所需拓扑启动一次共享 lifecycle

Group / Release Gate
  完整 cargo/npm/build/clippy/architecture/release 链
  组末、发布 Gate，以及合同明确要求的最终 PR acceptance 才执行
```

相同 external/bundled runtime、PEP、process topology 的重型 assertion 必须按类压缩进共享 fixture/lifecycle；若必须多次启动，测试必须说明 isolation 本身为何是被测行为。该规则不要求把快速、隔离性好的 unit tests 合成一个巨型测试。

Public command/session 的真实 Runtime Gate 至少在同一 lifecycle 连续覆盖 `exec → incremental poll → write → read → kill → terminal convergence`，避免分散测试造成 action 漏测。所有测试 runner 必须有 bounded timeout/cancel；session 丢失或控制通道不可寻址是 terminal failed/lost，不得继续等待为 running。

static/source-string contract test 只适合固定文案、schema、allowlist、禁止依赖、governance marker 等静态事实。能够通过 unit/integration/E2E 实际执行证明的行为，不得因为源码包含某函数名、测试名或 marker 就判 PASS；同一行为同时存在 static + executable check 时必须保护两个不同合同目的。

开发内循环不得在每个小修改后重复运行完整 `cargo test --locked` + all-target Clippy + frontend build + 历史 architecture/governance 链；先 targeted Fast Gate，准备正式接受时执行一次对应 Runtime Gate，最终 acceptance/group/release 再执行完整必需 Gate。测试优化不得删掉最终 required coverage。

## Schema29 task-state terminal durability

测试与实际 tool runner 都必须把 command terminal transition 当作持久状态事务，而不是 session retention 的副作用。任何 terminal exit 都由 `finally`/finally-equivalent finalizer 原子完成 terminal snapshot + 单个 `command_finished` + `current_command=null`；task-state 落盘后，即使 private command session 立即 prune，也必须能恢复同一 terminal truth。

并发测试至少覆盖 delayed-finalizer race：Task A / Session A 启动，随后 Task B / Session B 成为当前 owner；A 的迟到 completion/timeout/cancel callback 必须因 `(task_id, session_id)` CAS/owner mismatch 而不能清除 B。重复 A terminal callback 也不能生成第二个 `command_finished`。持久化写仍服从项目既有 atomic-write/replace/fail-safe 规则，terminal snapshot 与 owner metadata 必须 secret-redacted/bounded。

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
