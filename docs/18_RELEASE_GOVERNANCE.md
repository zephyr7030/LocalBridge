# 18 — Release Governance

## Release Stages

内部流程：

```text
development
→ release candidate
→ stable
```

v0.1 UI 不需要暴露“更新频道”。

## Stable Release Gate

Stable 必须满足：

- PR_INDEX 全部 required PR PASS；
- PR_CONTRACTS 全部 acceptance PASS；
- architecture verification PASS；
- compatibility snapshot frozen；
- runtime manifest frozen；
- dependency lockfiles frozen；
- SBOM generated；
- THIRD_PARTY_NOTICES updated；
- clean Windows 11 x64 E2E PASS；
- installer/uninstaller PASS；
- reboot/background PASS；
- privileged broker PASS；
- no telemetry network activity；
- no orphan processes；
- diagnostics redaction PASS。

## Release Provenance

每个 release 必须可回答：

```text
What source commit?
What Rust dependencies?
What npm dependencies?
What Python dependencies?
What coding-tools-mcp commit?
What tunnel-client binary?
What SHA256?
What policy baseline?
```

## Rollback

升级/迁移失败时：

- 不破坏旧 settings；
- 不删除旧 credential；
- 保留上一个可读配置备份；
- installer 不主动清空 app-data。

## Changelog

CHANGELOG 只记录用户可感知变化和重要兼容/安全变化。

禁止把 commit log 原样当 changelog。

## Security

高影响安全修复：

- 可以跳过普通功能节奏；
- 但仍需最小 compatibility / release regression；
- 不允许以“紧急”为由绕过 secret、PEP、Broker 边界测试。
