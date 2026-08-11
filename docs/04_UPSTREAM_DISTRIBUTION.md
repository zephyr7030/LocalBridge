
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

## LB-000 实证基线（2026-08-11）

已对公开固定版本执行 Windows 实证：

- `coding-tools-mcp 0.2.2` 固定 commit `311c1f2529d0f047ad2a8b68db6bf92dbb93d6bc`；真实 `tools/list` 为 20 项，schema SHA256 为 `d187db7ad6729893f17fceb539f030eb0c6139528ab35cb3f9564d27383d3674`；
- upstream `safe` / `trusted` 工具目录相同，且 `safe` 实际可执行 `exec_command`，因此 LocalBridge 采用第一方 Rust MCP Guard 作为 mandatory `tools/call` PEP；
- Windows `..` / junction / symlink 越界读取实测被 upstream 拒绝，但该能力仅作为 defense-in-depth，LocalBridge 仍保留独立 active-workspace 边界；
- `tunnel-client 0.0.11` 固定 commit `8d55683eeef80bc5e360d95abf4692454fafc615`，官方 Windows amd64 ZIP SHA256 为 `eb912c86c6ccde90cda805cb17009507176a656725cf86c36fabe1901a12e29b`；
- Tunnel Runtime API Key 使用子进程环境变量 + CLI `env:VARNAME` 引用，实测 secret 不出现在 argv、OS command line 或日志；
- `/readyz` 只可作为本地 startup readiness，实测控制面不可达时仍为 200；LocalBridge Ready 还必须结合 control-plane 状态；
- Python 3.12.10 embeddable sample 已验证可直接运行 coding-tools-mcp v0.2.2；最终 patch/hash/layout 仍由 LB-006 冻结；
- Windows Job Object `KILL_ON_JOB_CLOSE` 已验证可清理根进程及嵌套子进程。

首次 G0 对抗审查已否决原 live Tunnel 证据：仅成功读取 control-plane metadata 不足以证明 poll/实际 Tunnel 工作状态，且旧 live probe 的命令行与输出 secret 结论包含常量断言。新 schema-v2 probe 已用真实凭据重新执行：authenticated metadata 为真，但仅观察到 1 次 poll cycle 且该次为 error，`commands_poll_last_successful_timestamp_seconds` 未变为正值，因此实际 Tunnel 工作态未成立；与此同时 Windows OS command line 与 stdout/stderr 的 Runtime API Key 泄漏测量均通过。LB-000 继续保持 REWORK_REQUIRED，LB-001 继续 BLOCKED；只有后续真实凭据运行观察到至少一次成功 poll 后才能重新进入 G0 审查。
