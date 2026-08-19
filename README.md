# LocalBridge

LocalBridge 是面向 Windows 11 x64 的本地 MCP 桥接应用，用于让 ChatGPT 等 MCP 客户端在用户明确授权的边界内访问本地开发工作区、执行开发任务，并在需要时通过独立的 UAC/Broker 路径执行管理员操作。

## 工具结构

LocalBridge 的公共 Agent API 保持 **8 个普通核心工具 + 1 个特权扩展**：

- `workspace_context`：工作区、项目、运行环境与权限状态发现。
- `agent_workflow`：诊断、修复、功能、重构、测试、构建、文档与恢复编排。
- `exec_command`：受权限模式约束的普通命令/进程执行。
- `command_control`：运行中命令的 poll / read / write / kill。
- `task_control`：当前高层任务的 get / cancel。
- `git_workflow`：Git status / log / show / diff / blame。
- `document_workflow`：受控文档读取与生成。
- `view_image`：受控图片读取与缩放。
- `elevated_exec`：特权扩展；只有管理员模式且 Broker 经用户 UAC 授权后才可执行管理员路由。

## 权限模式

**编辑模式**只允许当前活动工作区内的结构化文件、Git、编辑等操作，不允许启动普通 Shell/进程。

**完整模式**允许 `cmd`、PowerShell 和开发进程，但进程始终使用当前 Windows 普通用户 Token。LocalBridge 的结构化路径、workdir、Git、文档、图片和编辑输入仍限制在活动工作区；LocalBridge 不声称对子进程提供额外的 OS 级文件系统沙箱。

**管理员模式**不会把整个 LocalBridge 提权。普通路线仍使用当前用户 Token；只有显式 `elevated_exec → Privileged Broker → Windows UAC` 路线可获得管理员 Token。LocalBridge 自身控制面始终不允许通过 MCP 修改。

## 安全模型

- Runtime API Key 只保存在 Windows Credential Manager，不写入普通配置、命令行或日志。
- 本地监听保持 loopback-only。
- 未知能力与未审核的特权行为 fail closed。
- 管理员能力必须经过应用内风险确认和 Windows UAC；后台启动不会自动弹出 UAC。
- 运行时与第三方工具固定版本和校验值；应用运行时不自动下载更新自身。
- 无遥测、无崩溃自动上传。

更完整的安全边界与漏洞报告方式见 [SECURITY.md](SECURITY.md)。

## 安装

首个稳定版发布后，建议从 GitHub Releases 获取 Windows 11 x64 安装包并校验发布信息。稳定发布前可按下方步骤从源码构建。

LocalBridge 使用系统 WebView2，不捆绑 WebView2 Runtime；不依赖系统 Python，发布包携带固定的 Python Embedded Runtime。

## 开发构建

要求：

- Windows 11 x64
- Node.js 24
- Rust 1.85 或更新的兼容稳定工具链（MSVC）
- Windows 系统 `curl.exe` 与常规 Windows SDK/MSVC 构建环境

```powershell
npm ci
node scripts/prepare-lb018-resources.mjs
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --locked -- --test-threads=1
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
node node_modules/@tauri-apps/cli/tauri.js build --bundles nsis
```

`aria2c`、`7z` 和 `jq` 只在构建准备阶段按固定来源与 SHA-256 获取；运行时不会自动下载或更新它们。

## 已知限制

- 当前只支持 Windows 11 x64。
- v0.1 不提供 WSL/container/remote shell、自定义 shell registry 或通用任务历史/快照回滚。
- 完整模式的子进程拥有当前 Windows 用户本身可访问的 OS 资源；活动工作区限制只约束 LocalBridge 自己的结构化输入，不构成额外 OS 沙箱。
- Tunnel/ChatGPT 连接需要用户自己的有效配置。
- 应用不包含自动更新器；升级以完整 LocalBridge 版本发布为单位。

## 许可证

LocalBridge 自有源码使用 [MIT License](LICENSE)。第三方运行时与依赖保持各自许可证，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
