# 11 — Project Directory Skeleton

这些目录在 PRE-CODE 包中已经建立，只用于冻结边界；`.gitkeep` 不代表允许提前实现后续 PR。

```text
LocalBridge/
├─ src/
│  ├─ app/                    # React composition / routing
│  ├─ components/             # 纯可复用 UI
│  ├─ features/
│  │  ├─ onboarding/          # First-run Wizard
│  │  ├─ dashboard/           # 单 Dashboard
│  │  ├─ settings/            # 设置
│  │  └─ diagnostics/         # 最小诊断
│  ├─ stores/                 # 前端展示状态，不持有 runtime truth
│  ├─ styles/                 # design tokens / CSS
│  └─ lib/                    # 纯前端通用工具
│
├─ src-tauri/
│  ├─ capabilities/           # Tauri 最小权限
│  └─ src/
│     ├─ app/                 # application composition
│     ├─ state/               # domain state
│     ├─ workspace/           # workspace manager
│     ├─ runtime/             # process supervisor
│     ├─ mcp/                 # MCP runtime + Guard
│     ├─ tunnel/              # OpenAI tunnel adapter
│     ├─ credentials/         # Windows secret storage
│     ├─ settings/            # settings persistence
│     ├─ tray/                # tray/background
│     ├─ diagnostics/         # typed diagnostics
│     └─ commands/            # Tauri command boundary
│
├─ runtime/
│  ├─ python/                 # vendored embeddable Python
│  ├─ coding-tools-mcp/       # pinned upstream source
│  └─ tunnel-client/          # pinned official binary
│
├─ scripts/                   # build/vendor/verify scripts
├─ assets/icons/
├─ tests/
│  ├─ unit/
│  ├─ integration/
│  ├─ e2e/
│  └─ fixtures/
├─ skills/
│  ├─ first-principles/
│  ├─ maintainable-code/
│  └─ minimal-ui/
└─ docs/
```

## 边界规则

- `src/` 不直接启动/停止进程。
- `src-tauri/src/runtime/` 不包含具体 Tunnel 协议逻辑。
- `mcp/` 与 `tunnel/` 通过 orchestrator 协作，不互相直接拥有生命周期。
- `runtime/` 只放 vendored 外部产物，不放产品业务代码。
- `tests/fixtures/` 不允许真实 secret。
- `components/` 不创建产品级状态。
- 新目录必须由当前 PR 合同要求；禁止为了“以后可能用到”继续扩展骨架。


## 根目录执行合同

```text
PR_INDEX.json       # PR DAG / Gate / 当前状态
PR_CONTRACTS.json   # 每 PR writable/forbidden/tests/artifacts/non-goals
PROJECT_STATE.json  # 全局 PRE-CODE / release 状态
```

`PR_CONTRACTS.json` 是防止路径漂移、提前实现和跨层修改的机器可读边界。


## Privileged Broker 目录

```text
src-tauri/
├─ src/privilege/
│  ├─ broker/
│  ├─ ipc/
│  └─ policy/
└─ bin/privileged-broker/

tests/
├─ unit/privilege/
├─ integration/privilege/
└─ fixtures/privilege/
```

`privilege/` 只负责权限边界，不能成为第二个 Runtime Orchestrator。


## Runtime 目录最终语义

```text
runtime/
├─ python/            # 产品私有 Windows Embedded Python
├─ coding-tools-mcp/  # pinned upstream
└─ tunnel-client/     # pinned official Windows x64 binary
```

这些目录不是开发环境缓存，而是最终安装包内容来源。

禁止把：

```text
node_modules
Rust toolchain
system Python venv
WebView2 Fixed Runtime
```

当成产品 runtime。

## UI 文案层

```text
src/lib/
├─ i18n/          # locale / terminology
└─ presentation/  # domain state → 用户文案
```

组件不得直接显示内部 enum。

## 长期维护目录

```text
compatibility/
├─ coding-tools/
└─ tunnel-client/

schema/
├─ settings/
└─ runtime-policy/

scripts/
├─ test/
├─ verify-schema44/
├─ verify-runtime-manifest/
├─ diff-upstream-surface/
├─ verify-ui-language/
└─ generate-sbom/

tests/
├─ contract/
├─ adversarial/
├─ fuzz/
└─ migrations/
```

这些目录不是为了增加形式，而是承载可执行长期维护合同。


## Spike 与 Release Evidence

```text
spikes/
└─ lb-000/              # 仅兼容性/可行性证据，不是生产代码

release-artifacts/
└─ <version>/           # SBOM / provenance / size / acceptance evidence
```

生产代码不得依赖 `spikes/`。

## Credential / Workspace Domain

建议实现边界：

```text
src-tauri/src/
├─ credentials/
│  ├─ store/
│  └─ redaction/
└─ workspace/
   ├─ registry/
   ├─ validator/
   └─ switching/
```

具体文件布局可由对应 PR 决定，但 domain 边界不得把 secret 放入 settings，也不得把 registry 等同于授权根。
