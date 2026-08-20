# LocalBridge

让网页端 ChatGPT 直接读取、修改和运行你的 Windows 本地项目。

LocalBridge 把 ChatGPT 和本地开发环境连接起来：不需要反复复制代码、上传文件，也不需要为了让 AI 碰到本地项目而额外搭一整套 Docker / Python 环境。安装后选择项目目录、完成连接配置，就可以直接在 ChatGPT 里继续开发。

---

## 🚀 它主要解决什么问题？

### 🧠 网页 ChatGPT 也能直接做本地开发

不再来回复制粘贴。ChatGPT 可以直接读取项目、搜索代码、修改文件、查看 Git 状态、运行测试和开发命令，适合日常修 Bug、重构、补功能和排查问题。

### 📁 文件操作不必全靠 Shell

LocalBridge 提供结构化文件能力，可直接查找、读取、写入、复制、移动、删除和校验文件。常见文件工作不再需要 AI 临时拼接一长串 Shell 命令。

### 🔐 给 AI 的权限可以明确控制

提供编辑、完整、管理员三种权限模式。日常操作可以限制在当前项目；需要管理员能力时，必须经过明确的风险确认和 Windows UAC。Runtime API Key 保存在 Windows 安全凭据中，不写入普通配置文件。

### 🧰 安装即用，少折腾环境

Python、Coding Runtime、Tunnel 和常用辅助工具随 LocalBridge 一起管理，不依赖系统 Python，也不要求安装 Docker。面向普通 Windows 11 x64 用户，尽量把环境问题留在应用内部解决。

### 💤 后台运行尽量安静

支持托盘后台运行、开机启动和运行时自动恢复。正常 GUI、后台服务和受管命令不会不断弹出控制台窗口；Tunnel 或本地运行时短暂异常时会尝试自动恢复。

---

## 📦 下载和使用

普通使用不需要准备开发环境。

1. 打开右侧 **[Releases](../../releases)**，下载最新的 `LocalBridge_0.1.1_x64-setup.exe`。
2. 安装后按向导填写 Tunnel ID、Runtime API Key，并选择要授权的本地项目目录。
3. 按应用提示在 ChatGPT 中创建 **Local Bridge** 自定义连接器。
4. 之后即可直接让 ChatGPT 读取、修改和运行该项目。

> 当前版本面向 **Windows 11 x64**。完整模式下启动的开发进程拥有当前 Windows 用户本身的系统权限；管理员操作仅通过显式 Broker + UAC 路线提供。更完整的边界说明见 [SECURITY.md](SECURITY.md)。

---

## 🛠️ 从源码构建（可选）

仅开发者需要：

```powershell
npm ci
node scripts/prepare-lb018-resources.mjs
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --locked -- --test-threads=1
```

---

## 📄 License

LocalBridge 自有源码使用 [MIT License](LICENSE)。第三方组件保持各自许可证，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
