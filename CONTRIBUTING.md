# Contributing to LocalBridge

感谢参与 LocalBridge。公共仓库采用普通 GitHub Issue / Pull Request 流程；内部开发合同、审查状态机和发布授权记录不属于公共贡献接口，也不要求贡献者理解这些内部材料。

## 开始之前

1. 使用 Windows 11 x64、Node.js 24 与 Rust 1.85+ MSVC 工具链。
2. `npm ci`，不要手工修改 lockfile 来掩盖依赖问题。
3. 不要在 Issue、PR、测试、截图或日志中提交真实 API Key、Tunnel 凭据、Token、Cookie、私钥或本机敏感信息。
4. 安全漏洞请按 `SECURITY.md` 私下报告，不要先公开 PoC 或凭据。

## 本地验证

```powershell
node scripts/public-release/preflight.mjs format-check
node scripts/public-release/preflight.mjs verify-license
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --locked -- --test-threads=1
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
```

修改 Rust 文件时，`format-check` 会对本次变更涉及的 Rust 文件执行 `rustfmt --check`。仓库仍有少量历史格式债务，因此公共 CI 不通过一次大规模格式化来制造与功能无关的 diff；新改动必须保持自身格式正确。

## Pull Request

- 一个 PR 聚焦一个可解释的目标。
- 说明行为变化、安全边界影响和验证方法。
- 优先补行为测试，不以源码 marker 代替真实执行。
- 不削弱 fail-closed、凭据保护、工作区边界或 Broker/UAC 权限模型来让测试通过。
- 不提交生成目录、日志、dump、密钥、本机配置或 release 临时产物。

提交贡献即表示你有权提交该内容，并同意其按项目 MIT License 分发。
