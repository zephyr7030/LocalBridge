# 项目目录清理建议表

日期：2026-09-03。对应发布：v0.1.5 Pre-release。

原则：先完成本地测试、打包、公开发布及资产哈希复核，再删除可再生缓存。禁止使用全目录清空或无差别 `git clean`；保留源码、用户状态、发布证据和 Git 历史。

## 清理清单

容量为清理前盘点的逻辑文件字节总量，GiB 按 1024³ 计算；后续构建会改变缓存大小。

| 路径 | 盘点容量 | 用途 | 建议与本轮状态 | 恢复方式 |
|---|---:|---|---|---|
| `src-tauri/target/debug/` | 38.82 GiB | Rust 调试构建与增量缓存 | 发布验证后清理；待执行 | 再次运行本地测试或开发构建 |
| `src-tauri/target-fixed-window-e2e/` | 7.84 GiB | 独立窗口测试重复构建 | 发布验证后清理；待执行 | 重跑原生窗口测试 |
| `src-tauri/target/lb019pre-no-console-test/` | 6.10 GiB | 历史无控制台测试重复构建 | 发布验证后清理；待执行；发布脚本已改为复用默认构建缓存 | 正常发布测试即可，不再需要此目录 |
| `src-tauri/target/release/` | 2.08 GiB | 最新程序、安装包及发布缓存 | 本轮保留，方便复核与增量打包 | 可在保留独立发布资产后按需重建 |
| `src-tauri/target/release-stage/`、`toolbox-stage/` | 8.46 MiB | 打包所需 Broker、内置工具暂存 | 本轮保留 | 运行资源准备脚本 |
| `src-tauri/target/toolbox-downloads/`、`toolbox-extract/` | 11.79 MiB | 固定版本工具下载及解压缓存 | 本轮保留，避免重复联网下载 | 运行资源准备脚本，校验固定哈希 |
| `tests/artifacts/` | 0.26 MiB | 前端构建、窗口测试截图与临时配置 | 本轮保留最后一次验证产物；下次测试可覆盖 | 重跑对应测试或前端构建 |
| `src-tauri/gen/` | 0.28 MiB | Tauri 自动生成 schema | 本轮保留，收益很小 | Tauri 构建自动生成 |
| `release-artifacts/preflight/public-source/` | 包含在 66.25 MiB preflight 目录中 | 公开源码导出工作目录 | 本轮保留，作为待发布源码复核入口 | 重新按 allowlist 导出；不得将本地私有历史直接推送 |
| `release-artifacts/preflight/public-source/src-tauri/target/`、`release-artifacts/preflight/public-source/node_modules/` | 本轮公开源码验证新生成，未计入初始盘点 | 从导出源码独立运行完整 CI 门禁的构建缓存 | 发布完成并保存验证结果后可清理；待执行 | 在公开源码目录重新安装依赖、运行同一门禁 |
| `node_modules/` | 85.79 MiB | 本地开发依赖 | 本轮保留；无开发需求时可选删除 | `npm ci` |
| `src/node_modules/` | 524 B | 构建工具生成的临时配置缓存 | 发布验证后清理；待执行 | 构建自动生成 |
| `.git/` | 834.34 MiB | 私有开发历史和对象 | 必须保留；不重写、不删分支、不自动强制 GC | 不适用 |
| `runtime/` | 41.73 MiB | 受版本和哈希约束的分发资源 | 必须保留，不属于普通缓存 | 按锁定清单恢复，不能临时下载替代 |
| `REVIEW_GUIDE.md` | 用户既有未跟踪文件 | 用户审查规则 | 原样保留，不擅自加入提交 | 不适用 |
| `release-artifacts/LB-019PRE/` | 21.47 MiB | 用户既有旧版本安装包与证据 | 原样保留，不覆盖 | 不适用 |
| `release-artifacts/v0.1.5/` | 发布后记录 | 当前版本安装包、SBOM、来源与验证证据 | 必须保留 | GitHub 同版本资产，可用 SHA-256 复核 |
| `docs/`、`governance/`、`compatibility/`、`spikes/`、`scripts/`、`tests/` | 未作为缓存统计 | 合同、历史决策、测试和验证基座 | 本轮不删；过时脚本须先证明无调用者再专项移除 | Git 历史只能恢复已提交文件 |

## 执行与验收记录

- 当前状态：已完成清理前盘点，尚未删除上述目录；本地发布门禁已解除，等待生成最终来源证明后推送。
- 删除前须逐项验证：绝对路径位于本仓库内、非链接/重解析点、无跟踪文件、无正在使用该路径的本项目进程。
- 发布资产必须来自 0.1.5 重新构建，不能把 0.1.4 安装包改名后上传。
- 最终记录实际删除字节、发布链接和保留项；缓存删除不能恢复原文件，但可以通过上述方式重建。

## v0.1.5 发布门禁记录

- 本地候选源码提交：`ef2255705851dc80d71c3eb83e502550de9906f2`。
- 待推送公开提交：`f1cb8757f2f79c8ed73b64aa781cc2987fe0a6e2`，接续现有公开提交 `6614ffeee26e552d61bcd187002623240bc6841d`；仅复制公开 allowlist，77 个运行时文件字节一致。
- 本地源树和公开导出树均完成 Rust、前端、Node、Clippy 与正式 NSIS 构建。公开树使用 Node 24 / Rust 1.85.0，在本地独立执行 CI 的全部 11 个阶段，通过后仍未推送。
- 两份源树的 Cargo 汇总均为 465 passed、4 ignored，前端 18 项、Node 14 项通过。其中使用端黑盒 2 项实际通过。注意：未提供发布 EXE 时，历史发布 GUI 测试会直接返回，Cargo 的 passed 统计**不代表**完成真实发布 GUI 验证。
- 第一次真实发布 EXE 检查被已安装实例持有的 `Local\LocalBridge.SingleInstance.v1` 正确阻止，没有把实例早退误报为通过。
- Tauri 的 `app_data_dir()` 经 Windows Known Folder API 解析，单纯设置测试子进程的 `APPDATA` 不能隔离真实配置；生产启动还会同步用户自启动注册项。因此在用户授权后，先保存用户配置和 Run 注册值，再退出已安装实例，把真实配置移出 Known Folder 后运行测试。
- 六个真实发布程序场景全部通过：前台启动、后台启动、自启动、Runtime 恢复、Tunnel 重连、后台命令无可见控制台。
- 测试结束后恢复 155 个原配置文件并逐文件核对 SHA-256，自启动值恢复为已安装程序路径；测试生成配置留在忽略目录作为证据，没有并入用户数据。LocalBridge 拥有的 Runtime/Tunnel/Python 进程均已终止，未结束无关 Python 进程。
- 暂未生成最终发布来源证明，未推送 GitHub、未创建 Release、未进行目录删除。释放空间：0 bytes。
- `REVIEW_GUIDE.md` 和 `release-artifacts/LB-019PRE/` 共 8 个既有文件的 SHA-256 复核未变。

已构建但尚未发布的安装包（两份构建环境不同，不能混用各自哈希）：

| 来源 | 大小 | SHA-256 |
|---|---:|---|
| 本地源树 `src-tauri/target/release/bundle/nsis/LocalBridge_0.1.5_x64-setup.exe` | 22,306,901 bytes | `b906f31cbf2b2d1571ba9600994c330351e82fde95bf63c0a669f9398423c78f` |
| 公开导出树 `release-artifacts/preflight/public-source/src-tauri/target/release/bundle/nsis/LocalBridge_0.1.5_x64-setup.exe` | 22,496,304 bytes | `0f3403e748a1b1f527b03019b81bfbe9868a40325c2dc138432d0e052cdd39fc` |

完整日志保存在 `release-artifacts/v0.1.5/local-validation.log` 与 `public-validation.log`。这些日志不作为公开 Release 附件上传。
