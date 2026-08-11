
# 04 — Upstream, Compatibility & Distribution

## 基线

```text
coding-tools-mcp v0.2.2
openai/tunnel-client v0.0.11
```

exact commit/asset/checksum 由 LB-000 实证写入 compatibility baseline/runtime manifest。

旧 GPT-WebCodex vendored Coding Tools MCP 自报 `0.4.1`，不能作为公开上游事实。

## LB-000 必须验证

- tools/list snapshot；
- tool schema/capability；
- workflow 间接执行；
- auth/session/transport；
- symlink/junction/reparse；
- PEP feasibility；
- Tunnel Windows asset/checksum；
- health/readiness；
- **安全 secret injection**；
- real Tunnel PoC；
- Job Object；
- Embedded Python；
- 与旧 vendored 0.4.1 差异。

PEP 决策必须基于 spike：

```text
upstream-native
thin adapter
Rust Guard
```

## 上游升级

```text
old baseline
→ new snapshot
→ structural diff
→ capability diff
→ security impact
→ policy update
→ regression
→ frozen new baseline
```

新增 unknown tool/capability 默认 deny。

## Distribution

Windows 11 x64 only。

安装包自带：

```text
Python Embedded
coding-tools-mcp
tunnel-client
LocalBridge binaries
```

无系统 Python/pip/Node/Docker runtime 要求。

WebView2 使用 Windows 系统 runtime。

一个 LocalBridge 版本对应一个 exact runtime manifest。

v0.1：

- 无 runtime 独立自更新；
- 无 app updater；
- 零遥测。

体积目标：

```text
installer 45–70 MiB
installed 90–150 MiB
```

若 installer >100 MiB 或 installed >250 MiB，LB-018 必须 attribution。

Stable release 产出 exact manifest、lockfiles、SBOM、THIRD_PARTY_NOTICES、provenance、checksums。
