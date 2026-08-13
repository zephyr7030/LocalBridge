
# 01 — Product & UX

## 产品基线

```text
ChatGPT
  ↓ OpenAI Secure MCP Tunnel
tunnel-client
  ↓
LocalBridge policy boundary
  ↓
coding-tools-mcp
  ↓
唯一当前项目
```

Windows 11 x64；Tauri 2 + React/TypeScript + Rust；捆绑 Python Embedded、coding-tools-mcp、tunnel-client。

不内嵌 ChatGPT，不读取 ChatGPT cookie/session。

## UI 原则

- 简体中文优先，只保留必要专业缩写/专有名词。
- 不显示内部 enum、fault code、raw tool id。
- 主控界面极简，不做工程控制台。
- 用户只看到当前必须理解或操作的信息。

## Apple-inspired：不增加视觉依赖

使用现有：

```text
React
Tauri WebView
HTML
原生 CSS / CSS variables / CSS animation
inline/local SVG
system font stack
```

推荐：

```css
font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
```

可使用留白、少量圆角、轻阴影、细分隔线、支持时的 blur/backdrop-filter 及 solid fallback。

禁止仅为视觉效果新增：

```text
UI component library
Tailwind / CSS framework
Framer Motion / animation library
icon package
第三方字体包
```

不要求 SF Pro，不捆绑字体。

## 主控界面

只保留：

```text
当前项目
安全隧道状态
编码服务状态
权限模式
管理员权限实际状态
当前执行状态
```

必要动作按状态出现：

```text
打开 ChatGPT
切换项目
启用/关闭管理员权限
设置
诊断
```

## 当前执行状态

执行中只有一行：

```text
●  运行测试  cargo test
```

- 绿色小圆点低干扰脉冲；
- 无“当前任务 / 类型 / 任务 / 状态”标题；
- 无冗余“执行中”；
- reduced-motion 下静态绿色点；
- 无消息流、最近活动、时间线、历史列表；
- 摘要来自真实 MCP/Broker 执行并脱敏。

空闲可显示：

```text
○  空闲
```

也允许隐藏。

## 自动重连 UI

自动重连默认不增加 UI：

- 无开关；
- 无重试计数；
- 无 toast；
- 无 banner；
- 无 tray 通知；
- 无专门重连卡片。

若主控界面本来已打开，已有连接状态可以真实反映恢复过程，但不得增加重连专用 UI。

只有同一故障周期 **5 次自动重连全部失败** 后才显示一次：

```text
连接失败
已自动重试 5 次。

[重试]  [查看诊断]
```

规则：

- `重试` 开启新的 5 次周期；
- 同一故障周期只弹一次；
- 不循环弹窗；
- 后台模式最终失败后也允许显示该错误窗口；
- 5 次内成功则无额外成功提示。

## 首次引导

首次启动固定为 **6 屏**，不得自行增加第 7 屏、说明卡片或冗余按钮。

顺序：

```text
欢迎 → OpenAI → 项目与权限 → Local Bridge 设置 → Local Bridge 使用确认 → 启动检查 → 主界面
```

### 第 1 屏

```text
1 / 6


简单设置 即可开始

LocalBridge是链接ChatGPT与本地代码的工具


                                      开始
```

只允许一个主按钮 `开始`；不增加技术解释、帮助或次要按钮。

### 第 2 屏

字段标签固定为 `Tunnel ID` 与 `Runtime API Key`。

```text
2 / 6


连接 OpenAI


Tunnel ID
[________________________________]

Runtime API Key
[••••••••••••••••••••••••••••]

运行密钥仅保存在 Windows 安全凭据中，不会以明文写入配置文件、日志或命令行。


                                      继续
```

`继续` 只做基础校验和安全保存；Runtime API Key 不进入 settings、日志、CLI 或 browser storage。

### 第 3 屏

```text
3 / 6


项目与权限


当前项目：D:\project\LocalBridge
                                      选择文件夹


● 编辑模式
  读取和修改项目文件

○ 完整模式
  允许运行本地命令

○ 管理员模式
  允许管理员操作


                                      继续
```

项目与权限必须位于同一屏。新项目选择以原生 Windows 文件夹选择器为主交互，不以手填绝对路径作为主流程。管理员模式只保存偏好，不自动弹 UAC。

### 第 4 屏

```text
4 / 6


Local Bridge 设置


1. 点击“打开 Local Bridge 设置”
2. 新建 Local Bridge
3. 保存后返回 LocalBridge

                              打开 Local Bridge 设置


                                      继续
```

只允许系统默认浏览器打开固定 URL：

`https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`

URL 由 Rust allowlisted 常量控制；前端不能提供任意 URL；不使用 WebView。引导只保留完成配置所需的最少步骤。

### 第 5 屏

```text
5 / 6


Local Bridge 使用确认

返回 ChatGPT 后即可尝试选择 Local Bridge；LocalBridge 不判断 ChatGPT 是否已连接。

                                      继续
```

LocalBridge 不伪造或猜测 ChatGPT 是否已创建 Local Bridge，不读取 ChatGPT 会话。若本屏需要展示或复制 connector endpoint，必须来自 Rust 的 typed projection，并以已验证 Tunnel/control-plane metadata 为依据；禁止前端从 Tunnel ID 推导或自行拼接 URL。复制成功反馈预留固定空间，不得推动布局。

### 第 6 屏

检查中：

```text
6 / 6


正在准备


● 本地运行环境
● 编码服务
○ OpenAI Tunnel


                                  确定
                                  灰色
```

后台：

```text
MCP start → ready
PEP start → ready
Tunnel start → ready
```

三项未全部通过前：

- `确定` 必须 disabled；
- 按钮保持灰色；
- 不显示完成提示；
- 不允许跳过。

三项全部通过后：

```text
6 / 6


正在准备


● 本地运行环境
● 编码服务
● OpenAI Tunnel


配置完成，在插件中选择刚刚添加的Local Bridge试试吧


                                  确定
```

此时才：

- 显示完成提示；
- 启用 `确定`；
- 用户点击 `确定` 后设置 `onboarding_complete = true` 并进入主界面。

禁止：

- 自动跳转；
- 第 7 屏；
- “ChatGPT 连接”第四个检查项；
- 第二个完成按钮；
- 测试通过前显示完成提示。

失败时只显示一句最关键错误和一个必要动作。所有按钮共享一致、可辨识的视觉规则；白色或近白背景上不得出现难以识别的纯白/近白按钮。所有提示遵循最小必要原则。

主窗口默认 900×620，最小 720×500，保持可缩放。向导卡片不得依赖固定 `min-height`；布局必须随可用 viewport 高度收缩，主体内容区在空间不足时内部滚动，标题和底部操作保持可达。

窗口 resize/maximize 后，主 WebView 必须重新绑定到 native main-window 的完整 client area；不能只让原生窗口外壳变大。主控界面与 onboarding 都必须按 live WebView viewport 重新布局。`.inner_size/.min_inner_size/.resizable`、`100dvh`、`overflow-y:auto`、无固定 `min-height` 等静态源码检查只属于辅助回归，不能单独构成响应式验收。必须有真实 Windows Tauri/WebView2 resize E2E：至少设置两个不同 native size 并执行 maximize，交叉比较 Tauri `inner_size()` 与 live JS `window.innerWidth/innerHeight × devicePixelRatio`，同时证明 Dashboard/onboarding 的实际 DOM rect 随 viewport 改变。
## 权限

```text
编辑模式   → reviewed read/write；无 process exec
完整模式   → + 当前 Windows 用户权限的普通命令
管理员模式 → + 独立 Privileged Broker
```

管理员模式无 TTL。

管理员实际状态独立于用户偏好：

```text
未启用
等待授权
等待系统授权
已启用
故障
```

## 项目

可以记住多个项目，但同时最多一个项目获得 MCP 授权：

```text
项目列表 ≠ 授权列表
```

支持新增、选择、切换、移除。

移除只删除 LocalBridge 记录，**永远不删除磁盘文件**。

移除当前项目只做一次确认：

```text
从 LocalBridge 移除此项目？
不会删除项目文件。

[取消] [移除]
```

随后进入正常 `NoActiveWorkspace`，不自动授权其他项目。

不增加独立“项目管理中心”。


## 品牌图标

正式图标资产固定为：

```text
assets/icons/localbridge.png
assets/icons/localbridge.ico
```

用途：

```text
localbridge.png → 高分辨率产品/UI/安装资源源图
localbridge.ico → Windows 应用、安装包、快捷方式、系统托盘
```

已验证：

```text
PNG  = 1024×1024 RGBA
ICO  = 16/24/32/48/64/128/256 px
```

开发智能体不得：

- 自行重绘或替换图标；
- 用 emoji 代替正式应用/托盘图标；
- 为图标引入第三方 icon library；
- 修改图标颜色、构图或透明区域；
- 从网络下载“临时图标”进入正式构建。

除非后续有明确品牌变更合同，否则这两份文件是 LocalBridge v0.1 的唯一正式图标源。
