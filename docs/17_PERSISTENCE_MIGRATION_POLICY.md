# 17 — Persistence & Migration Policy

## 范围

所有长期持久化数据必须具有 schema_version：

- settings；
- recent projects；
- workspace metadata；
- runtime desired state；
- permission preference；
- credential references；
- diagnostics metadata。

Credential secret 本身不进入普通 schema 文件。

## 迁移模型

```text
load
→ detect schema_version
→ migrate sequentially
→ validate
→ atomic write
→ use
```

禁止跨版本猜测。

例如：

```text
v1 → v2 → v3
```

而不是：

```text
v1 → current using ad-hoc fallback
```

## 原子性

迁移必须：

1. 读取旧文件；
2. 保留原始备份或安全回滚点；
3. 在临时文件完成新 schema；
4. validate；
5. atomic replace。

迁移失败：

- 不覆盖旧数据；
- 返回 typed migration fault；
- 不自动 reset 用户设置。

## Credential 引用

允许迁移：

```text
credential_id
credential_backend_version
```

禁止：

```text
plaintext secret
Runtime API Key
admin password
```

进入 migration file。

## 测试

每次 schema 改动至少测试：

- previous → current；
- two historical versions → current；
- malformed old state；
- unknown future schema；
- interrupted migration；
- rollback / original preserved。

## Future Schema

如果检测到：

```text
schema_version > supported
```

必须 fail-safe：

```text
ConfigurationVersionUnsupported
```

不得尝试降级覆盖。

## Project Registry Persistence

项目列表属于持久化 metadata，可以迁移：

```text
workspace_id
display_path
validated_identity
last_opened_at
```

不得把项目列表等同于 authorized roots。

迁移必须保持：

- active workspace reference（若仍有效）；
- registry 去重；
- secret absence。

如果旧版本只有单一 workspace：

```text
single workspace
→ one registry entry
→ active workspace points to that entry
```

若迁移后项目不可验证：

- 不隐式授权；
- 保留可诊断 metadata；
- 要求用户重新选择/确认。

## Credential Migration

普通 schema 只能迁移 credential reference / backend metadata。

禁止把 secure backend secret 解密后写入迁移文件。

如果未来 secure backend 变更：

```text
old secure backend
→ in-memory secret
→ new secure backend
→ verify
→ delete old reference if policy permits
```

整个过程不得经过明文文件。
