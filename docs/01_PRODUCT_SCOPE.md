# 01 — Product Scope

## 定位

LocalBridge 是一个 Windows 本地代码桥，不是 ChatGPT 客户端，也不是 AI IDE。

核心价值：

> 让用户用尽可能少的配置，把 ChatGPT 的 MCP 调用可靠地桥接到一个明确授权的本地 Coding Tools Runtime，并允许应用在后台长期运行。

## v0.1.0 Persona

目标用户：

- 已经会使用 ChatGPT；
- 想让 ChatGPT 处理本地代码；
- 不想安装 Docker；
- 不想维护 Python / Tunnel CLI；
- 能理解“完整编码模式会执行本地命令”的风险；
- Windows 11 x64。

## 成功指标

第一次安装后：

1. 用户能在 5 分钟内完成连接；
2. 不要求用户打开 PowerShell；
3. 不要求用户手动安装 Python；
4. 不要求用户手动拼 tunnel-client 命令；
5. 用户关闭窗口后服务继续；
6. Windows 重启后可静默恢复；
7. MCP 或 Tunnel 异常退出能自动恢复；
8. 错误凭据不会进入无限重启；
9. 退出托盘后不遗留由 LocalBridge 启动的 sidecar。

## 主界面只回答四个问题

- 当前项目是什么？
- ChatGPT / Tunnel 是否在线？
- Coding Runtime 是否在线？
- 我现在是 Edit 还是 Full 权限？

其余信息进入“诊断”。

## v0.1.0 非目标

- 项目任务管理
- AI Chat UI
- 内嵌浏览器
- 自研 build runner
- 自动代码审查 dashboard
- 多工作区同时在线
- 多 Tunnel 并发
- 远程控制电脑


## 管理员模式产品原则

管理员模式：

- 用户可自行启用/关闭；
- 不设置自动失效倒计时；
- 不提升整个 LocalBridge；
- 不绕过 Windows UAC；
- 开机后台启动不自动弹 UAC；
- 只有独立 Privileged Broker 获得 Administrator Token。


## v0.1 分发冻结

- Windows 11 x64 only；
- 完整携带 Python Embedded / coding-tools-mcp / tunnel-client；
- 用户无外部 Python 依赖；
- WebView2 使用系统 Runtime；
- 零遥测；
- 无自动 updater；
- third-party runtime 只随 LocalBridge 整包升级；
- Elevated 正式允许经 Broker/PEP 的 `elevated_exec`。

## 项目目录管理

v0.1 必须支持：

- 新增项目；
- 选择已有项目；
- 切换当前项目；
- 移除非当前项目；
- 移除当前项目并进入无当前项目状态；
- 项目列表去重。

移除只删除 LocalBridge 记录，不删除任何磁盘文件。

同一时间只有一个项目获得 MCP 授权。
