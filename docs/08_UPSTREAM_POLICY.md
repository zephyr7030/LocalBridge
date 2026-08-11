# 08 — Upstream Policy v9

## coding-tools-mcp

来源：

`xyTom/coding-tools-mcp`

初始基线：

- release: `v0.2.2`
- commit: `311c1f2529d0f047ad2a8b68db6bf92dbb93d6bc`

GPT-WebCodex v0.1.6 内嵌副本自报 `0.4.1`，与公开 upstream release 不一致。

因此 LB-000 必须：

- 直接启动公开 upstream；
- 保存 tools/list snapshot；
- 保存 capability matrix；
- 对 read/write/exec/workflow 做 adversarial tests；
- 决定 Policy Enforcement 方案。

禁止：

- 复制 GPT-WebCodex tool registry 作为事实；
- 仅因 vendored 版本号“更高”就采用；
- 静默修改 upstream。

## tunnel-client

来源：

`openai/tunnel-client`

初始基线：

- release `v0.0.11`
- Windows x64 official release asset；
- SHA256 来自官方 release manifest。

LB-000 必须验证：

- CLI contract；
- health/readiness；
- Runtime Key 安全注入方式；
- target URL 方式；
- error categorization。

不得预设 env/flag 名称。

## Python

最终使用 Windows Embeddable Python 3.12.x。

Portable runtime 不允许拖到最终 release 才验证。

LB-000：

- 做最小 embedded Python feasibility spike。

LB-006：

- 冻结真实 patch version、SHA256、依赖布局；
- 所有 Coding Tools Runtime 测试直接使用该最终模型。

## Policy Enforcement 决策

LB-000 产出 ADR：

```text
Option A — upstream native policy
Option B — thin adapter
Option C — Rust MCP Guard
```

决策标准：

1. `tools/call` 是否可强制；
2. capability 是否可识别；
3. workflow 间接 exec 是否可阻止；
4. unknown tool 是否 fail-closed；
5. MCP session/transport 实现成本；
6. 长期上游升级成本。

## 升级

任何 upstream 升级必须独立 PR：

- old/new；
- checksum；
- tool/capability diff；
- policy diff；
- deterministic suite；
- live compatibility；
- security regression。

禁止自动合并 runtime/sidecar 升级。


## 高权限外部 Runtime

LB-000 capability matrix 必须特别标记：

```text
docker
podman
wsl
```

若调用这些 runtime 可能获得接近主机管理员的能力，归类为：

`privileged-external-runtime`

不得仅因命令本身可由普通用户启动就认为低风险。


## Release Pinning

上游版本不进行独立在线升级。

每次 runtime 变化必须：

```text
new LocalBridge release
→ update runtime-manifest
→ compatibility suite
→ security suite
→ packaging
→ clean-machine E2E
```

运行中的应用不得查询并替换 upstream runtime。

## Compatibility Baseline

`COMPATIBILITY_BASELINE.json` 是当前 runtime compatibility 状态入口。

任何 runtime 升级 PR 必须：

1. 生成新 snapshot；
2. 与旧 baseline diff；
3. 分类 capability 变化；
4. 更新 policy；
5. 运行 security/adversarial suite；
6. 冻结新 baseline。

禁止直接覆盖 baseline 而不保留差异证据。
