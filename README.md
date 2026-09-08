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
- 在主窗口里回看 ChatGPT 对这台机器做过的每一件事

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

1. 前往 **[Releases](../../releases)**，下载最新的 `LocalBridge_<版本>_x64-setup.exe`。
2. 安装后选择本地项目并完成连接设置。
3. 根据应用引导创建 **Local Bridge** ChatGPT 插件连接。
4. 回到 ChatGPT，开始处理本地开发或系统维护任务。

## 权限与安全

LocalBridge 提供编辑、完整和管理员三种权限模式。管理员模式需要经过 Windows UAC，并且所有管理员操作都走一个独立的特权 Broker 进程，而不是由主程序自己提权。

**每一条管理员命令都会原样记入本机的审计账本**，包括被拒绝和未执行的。账本只留在这台机器上。

**破坏性命令不会直接执行。** 当一条命令落入这几类——批量删除、磁盘格式化、注册表删除、关闭本机防护、破坏恢复手段、增删管理员账户——LocalBridge 不会执行它，而是交回一个确认令牌，并把命令原文摆到主窗口里等你点头。批准与那一条命令、那一个工作目录绑定，用过即失效，超时自动作废；换一条命令就得重新问。

这里有一点需要说清楚：**识别命令靠的是文本匹配，而文本匹配不是安全边界**——一条命令完全可以在运行时拼出自己的目标。真正的边界是 UAC 与 Broker 授权，确认弹窗是在此之上的一层礼貌。所以判定刻意偏向沉默：读取、列举、查询、构建、安装、启动一律不打扰你，只有不可撤销的、以及削弱本机自我防护的操作才会拦下来。漏判的代价是少一次弹窗，不是越过了边界。

Runtime API Key 保存在 Windows 安全凭据中，不写入普通配置文件。无遥测、无使用统计、无崩溃信息上传。

完整的权限边界和安全设计见 [SECURITY.md](SECURITY.md)。

---

## 从源码构建

仅开发者需要准备 Node.js 和 rustup。Rust 版本由仓库根目录的 `rust-toolchain.toml` 锁定，rustup 会自动取用，不要手工指定——测试与发布包必须由同一个编译器产出。

```powershell
npm ci
node scripts/prepare-lb018-resources.mjs
node scripts/test/ci-gate.mjs
```

`ci-gate.mjs` 是唯一的门禁入口，串起工具链核对、格式、许可证审计、前端单测与构建、Rust 测试与 clippy，直到产出 NSIS 安装包。窗口行为另有一个真实渲染的端到端检查：

```powershell
node tests/e2e/onboarding/fixed_window_runtime_e2e.mjs
```

---

## License

LocalBridge 自有源码使用 [MIT License](LICENSE)。第三方组件保持各自许可证，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
