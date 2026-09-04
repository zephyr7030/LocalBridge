# 项目目录清理建议表

日期：2026-09-04。对应发布：v0.1.5 Pre-release。

原则：先完成本地测试、打包、公开发布及资产哈希复核，再删除可再生缓存。禁止全目录清空或无差别 `git clean`；保留源码、用户状态、发布证据和 Git 历史。

## 清理清单

容量为本轮清理前的逻辑文件字节总量，GiB 按 1024³ 计算。

| 路径 | 清理前容量 | 用途 | 本轮结果 | 恢复方式 |
|---|---:|---|---|---|
| `src-tauri/target/debug/` | 46,481,276,595 bytes（43.29 GiB） | Rust 调试构建与增量缓存 | 已通过 `cargo clean --profile dev` 清理 | 重新运行本地测试或开发构建 |
| `src-tauri/target-fixed-window-e2e/` | 8,422,785,288 bytes（7.84 GiB） | 独立窗口测试重复构建 | 已通过 Cargo 定向清理 | 重跑原生窗口测试 |
| `src-tauri/target/lb019pre-no-console-test/` | 6,554,328,080 bytes（6.10 GiB） | 历史无控制台测试重复构建 | 已通过 Cargo 定向清理；发布脚本已复用默认缓存 | 正常发布测试即可，不再需要该目录 |
| `release-artifacts/preflight/public-source/src-tauri/target/` | 1,874,545,888 bytes（1.75 GiB） | 公开源码门禁构建缓存 | 补充标准 `CACHEDIR.TAG` 后由 Cargo 定向清理 | 在公开源码目录重新准备资源并运行门禁 |
| `src/node_modules/` | 523 bytes | Vitest 结果缓存 | 缓存文件已清理；仅保留 0-byte 空目录壳 | 测试自动生成 |
| `src-tauri/target/release/` | 2,300,950,227 bytes | 最新程序、安装包及增量发布缓存 | 保留 | 可在保留独立发布资产后重建 |
| `src-tauri/target/release-stage/`、`toolbox-stage/` | 小型暂存目录 | Broker 与内置工具暂存 | 保留，避免重复准备 | 运行资源准备脚本 |
| `src-tauri/target/toolbox-downloads/`、`toolbox-extract/` | 固定版本缓存 | 内置工具下载与解压缓存 | 保留，避免重复联网下载 | 运行资源准备脚本并校验固定哈希 |
| `release-artifacts/preflight/public-source/` | 源码导出本体约 66 MiB | 已发布公开源码的独立复核工作区 | 保留源码与独立 Git 历史，构建缓存已删除 | 重新按 allowlist 导出 |
| `release-artifacts/v0.1.5/` | 22,624,454 bytes | 当前版本安装包、SBOM、来源与验证证据 | 必须保留 | 可从 GitHub 同版本资产恢复并复核 SHA-256 |
| `tests/artifacts/` | 小型测试证据 | 截图、配置及测试结果 | 保留最后一次结果 | 重跑对应测试 |
| `node_modules/` | 约 86 MiB | 当前本地开发依赖 | 保留，避免下次开发重新安装 | `npm ci` |
| `.git/` | 私有开发历史 | 本地开发历史和对象 | 必须保留，不重写、不强制清理 | 不适用 |
| `runtime/` | 受哈希约束的分发资源 | Runtime、Tunnel 与内置工具 | 必须保留 | 按锁定清单恢复 |
| `REVIEW_GUIDE.md` | 用户既有未跟踪文件 | 用户审查规则 | 原样保留，SHA-256 未变 | 不适用 |
| `release-artifacts/LB-019PRE/` | 用户既有旧版本资产 | 旧安装包与证据 | 7 个文件原样保留，SHA-256 未变 | 不适用 |

## 本轮执行结果

- 实际清理：63,332,936,374 bytes（58.98 GiB）。
- 所有删除目标均先验证位于 `D:\project\LocalBridge\` 内、不是重解析点且不含 Git 跟踪文件。
- 系统策略拒绝直接递归删除后，未绕过保护；Rust 构建目录改由 Cargo 定向清理，Vitest 的单个缓存文件通过精确文件修改清除。
- 保留的 `src-tauri/target/release/` 与 `release-artifacts/v0.1.5/` 复核存在；正式安装包未参与清理。
- `REVIEW_GUIDE.md` 与 `release-artifacts/LB-019PRE/` 共 8 个既有文件在清理前后逐文件 SHA-256 一致。
- 用户核心配置 `settings.json`、`settings.json.bak`、`startup-profile.json` 与备份逐文件 SHA-256 一致。
- 发布测试遗留的自启动路径已恢复为 `"C:\Program Files\LocalBridge\localbridge.exe" --background`；安装版 LocalBridge 已重新启动。

## v0.1.5 发布与验收

- 私有源码提交：`9d89607cae09024836a42c306b2c7ce30a35344c`。
- 公开源码提交：`c8118e73e96b05ef9090b260837a44788aeafaff`。
- 公开父提交：`803e9916fe791145e7a8b0e37aa182504587bd15`。
- 公开树：`9fbe14459167ff715ecbcc6c9710456ca2888b28`。
- 公开导出由 `scripts/public-release/public-files.allowlist` 控制；公开树校验通过，敏感信息扫描结果为 3 commits、331 tracked、0 warnings。
- 本地统一门禁通过：Rust 汇总 466 passed、4 ignored，前端 18 项、Node 14 项、Clippy、Runtime 清单及使用端黑盒测试均通过。
- 真实发布程序的六个无控制台场景通过：前台启动、托管命令、后台启动、恢复、Tunnel 重连、自启动。
- 首次 CI `33880342449` 唯一失败来自测试错误地把 HTTP Header 名称当作大小写敏感；修复后先在本地对真实 Tunnel 连续复跑 30 次，再推送。
- 最终 CI `33884610967` 一次通过，用时 33m24s；未在云端循环试错。
- GitHub Release：[LocalBridge v0.1.5](https://github.com/zephyr7030/LocalBridge/releases/tag/v0.1.5)，状态为 Pre-release，标签指向公开提交 `c8118e73e96b05ef9090b260837a44788aeafaff`。

## 已发布资产

| 资产 | 大小 | SHA-256 |
|---|---:|---|
| `LocalBridge_0.1.5_x64-setup.exe` | 22,306,989 bytes | `03f664142910d98343532bba1e3183a5d032fa39734e43b42efc303ef35cceb7` |
| `release-provenance.json` | 2,173 bytes | `ba6ab53f32e1c797ebb9d7125957d9aef96f6c2d321fef43458d6e369ac1d352` |
| `sbom.cdx.json` | 202,740 bytes | `2618595dfcf8be7299f660869e0c4ebae3a7fd6c408569e7206983814b97c2cd` |

GitHub 回读的三个资产均为 `uploaded`，大小和 SHA-256 与本地一致。安装包还与 `src-tauri/target/release/bundle/nsis/LocalBridge_0.1.5_x64-setup.exe` 字节一致。

## 后续清理建议

1. 下次需要释放更多空间时，可在确认独立发布资产已备份后清理 `src-tauri/target/release/`，预计还可释放约 2.14 GiB。
2. 长期不开发时可删除根目录 `node_modules/`，预计释放约 86 MiB；恢复方式为 `npm ci`。
3. `environment-backup/` 含本机恢复证据，不应上传公开仓库；确认无需回滚后可专项删除。
4. 不建议自动清理 `.git/`、`runtime/`、`docs/`、`scripts/` 或 `tests/`。旧脚本必须先证明无调用者，再单独移除。
