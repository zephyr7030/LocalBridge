
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

Schema35 冻结绿色化存储布局：LocalBridge 主程序、Python Embedded、coding-tools-mcp、tunnel-client、Privileged Broker 与静态资源等不可变产品/运行时载荷都保留并直接从 canonical LocalBridge 安装根目录运行；普通启动不得仅为执行而把 bundled runtime 再复制或解压到 `%LOCALAPPDATA%`、`%ProgramData%`、Windows 系统目录或持久 Temp。受保护的 per-machine Program Files 安装与 Broker 信任边界继续保留。

可变但非密钥的产品状态只集中到一个 `%LOCALAPPDATA%\LocalBridge` 根目录，包括 settings、workspace registry、task state、logs 与 diagnostics；不建立无必要的 `%ProgramData%\LocalBridge` 持久 footprint，也不把可变产品状态散落到 Windows 系统目录或持久 Temp。Runtime API Key 仍只保存在 Windows Credential Manager。Windows 正常维护的 installer、shortcut 与 autostart registration metadata 不视为违反该绿色化边界。

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

### Packaged GUI managed-child console contract

开发/测试时，测试 runner、shell 或调试命令出现后台进程/命令窗口是允许的，不能据此判断最终产品是否合规，也不得建立“开发环境必须完全无 console window”的要求。

最终 packaged/normal GUI LocalBridge 必须通过 release-style launcher 实测确认：由 LocalBridge 拥有并启动的 bundled coding runtime、Tunnel、Broker/background helper、PEP-adjacent managed command path 与 shell/direct command child 不会意外弹出可见 console window。若未来需要真正交互式 terminal，必须另立显式产品合同。隐藏 console 不得破坏 Job Object/process ownership、取消/超时或 clean shutdown；可直接 structured process execution 的路径不得无理由增加 shell wrapper。

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

首次 G0 对抗审查已否决原 live Tunnel 证据：仅成功读取 control-plane metadata 不足以证明 poll/实际 Tunnel 工作状态，且旧 live probe 的命令行与输出 secret 结论包含常量断言。完成无凭据 preflight 与 probe 加固后，最新 schema-v2 实测已通过：authenticated metadata 为真，同一 `tunnel-client` 进程观察到 2 次 poll cycle、0 次 poll error，`commands_poll_last_successful_timestamp_seconds` 为正；Windows OS command line 与 stdout/stderr 的 Runtime API Key 泄漏测量均通过，且一次性明文 credential 文件在外部请求前已删除。LB-000 现为 PASS，G0 进入 REVIEW_REQUIRED；LB-001 仍保持 BLOCKED，等待独立对抗审查 PASS。
