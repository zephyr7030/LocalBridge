# LocalBridge

## 让 ChatGPT 直接参与本地开发与 Windows 维护

LocalBridge 将 ChatGPT 插件与 Windows 本地环境连接起来。无需反复上传文件或复制命令，就能让 ChatGPT 阅读和修改项目、运行开发任务，并协助完成常见的系统检查与维护工作。

安装包约 **21 MB**，内置 Python、Coding Runtime、Tunnel 和常用工具，无需另外配置系统 Python、Node.js、Rust 或 Docker。

---

## 一个插件，连接完整的本地工作流

- 阅读、搜索和修改项目文件
- 运行测试、构建及开发命令
- 查看 Git 状态、提交记录和代码差异
- 管理后台命令与长时间任务
- 检查 Windows 服务、日志和运行环境
- 执行常见系统诊断与管理员维护操作

无论是修复 Bug、重构项目、排查构建问题，还是检查 Windows 运行状态，都可以直接在 ChatGPT 对话中继续完成。

## 轻量安装，工具内置

LocalBridge 将运行所需的工具统一放入安装包，不依赖系统 PATH，也不会在使用过程中临时安装 Python 包。

- 内置固定版本的 Python Embedded Runtime
- 内置 Coding Runtime 和 OpenAI Tunnel 客户端
- 不需要安装 pip、venv 或 Docker
- 运行工具随 LocalBridge 版本统一更新，避免环境漂移
- 无遥测、无使用统计、无崩溃信息上传

当前提供 Windows 安装版。“自包含”表示无需额外准备开发运行环境，不代表免安装 Portable 版本。

---

## 下载与使用

当前支持 **Windows 11 x64**。

1. 前往 **[Releases](../../releases)**，下载 `LocalBridge_0.1.5_x64-setup.exe`。
2. 安装后选择本地项目并完成连接设置。
3. 根据应用引导创建 **Local Bridge** ChatGPT 插件连接。
4. 回到 ChatGPT，开始处理本地开发或系统维护任务。

## 权限与安全

LocalBridge 提供编辑、完整和管理员三种权限模式。管理员操作通过明确确认和 Windows UAC 启用；Runtime API Key 保存在 Windows 安全凭据中，不写入普通配置文件。

完整的权限边界和安全设计见 [SECURITY.md](SECURITY.md)。

---

## 从源码构建

仅开发者需要准备 Node.js 和 Rust：

```powershell
npm ci
node scripts/prepare-lb018-resources.mjs
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --locked -- --test-threads=1
```

---

## License

LocalBridge 自有源码使用 [MIT License](LICENSE)。第三方组件保持各自许可证，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
