
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

无活动任务时必须稳定显示：

```text
○  等待命令
```

该行不得隐藏，也不得由前端本地状态伪造；真实 MCP/Broker 调用的 Running/Waiting/Blocked/Failed/Cancelled 必须由 backend `CurrentTaskStatus` typed projection 驱动，terminal 后回到 `等待命令`。

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

首次启动固定为 **严格 5 屏**，不得增加第 6 屏、说明卡片或冗余步骤。旧 `Local Bridge 使用确认` 整页已经废弃并删除。

首次引导本身就是当前窗口的唯一主要内容，视觉上必须采用**整页布局**：5 屏共同页面直接使用自定义 window chrome 下的完整内容区。禁止先铺一块大面积空白背景，再在中央放置一个带圆角、阴影或边框的“向导卡片/弹窗/对话框”作为整个页面；不得形成“窗口里又套一个窗口”的观感。页面级 padding、字段分组、状态行和局部控件可以保留，但不得重新构造一个浮动的整体向导外壳。

顺序：

```text
欢迎 → OpenAI → 项目与权限 → 创建自定义插件 → 启动检查 → 主界面
```

### 第 1 屏

```text
1 / 5


简单设置 即可开始

LocalBridge是链接ChatGPT与本地代码的工具


                                      开始
```

只允许一个主按钮 `开始`；不增加技术解释、帮助或次要按钮。

### 第 2 屏

字段标签固定为 `Tunnel ID` 与 `Runtime API Key`。

```text
2 / 5


连接 OpenAI


Tunnel ID
[________________________________]

Runtime API Key
[••••••••••••••••••••••••••••]

运行密钥仅保存在 Windows 安全凭据中，不会以明文写入配置文件、日志或命令行。


返回                                  继续
```

`继续` 只做基础校验和安全保存；Runtime API Key 不进入 settings、日志、CLI 或 browser storage。第 2 屏必须提供明确 `返回` 到第 1 屏。

### 第 3 屏

```text
3 / 5


项目与权限


当前项目：D:\project\LocalBridge
                                      选择文件夹


● 编辑模式
  读取和修改项目文件

○ 完整模式
  允许运行本地命令

○ 管理员模式
  允许管理员操作


返回                                  继续
```

项目与权限必须位于同一屏。新项目选择以原生 Windows 文件夹选择器为主交互，不以手填绝对路径作为主流程。可见用户点击/重新点击 `管理员模式` 本身就是显式提权动作；若 Broker 尚未 Active，必须立即发起 Windows UAC，仅提升 Privileged Broker，禁止额外“启用管理员权限”按钮。后台 `--background` 恢复管理员偏好仍不得自动 UAC。第 3 屏必须提供明确 `返回` 到第 2 屏。

三个权限模式按钮的标题、说明文字与上下左右边框之间必须有清晰且均衡的视觉留白；禁止固定高度压缩说明，说明换行时按钮必须安全自动增高，并提供 `min-height >= 80px` 或等效结构证明，确保“标题 + 两行说明 + 上下留白”不会被压扁。固定 900×620 实机渲染不得出现文字贴边。此项仍是人工视觉 Gate，不能仅凭 CSS 存在 `padding/min-height` 自动判 PASS。普通选中项使用统一蓝色 `#0071e3`；管理员模式在 onboarding 与 Dashboard 均使用黄色/琥珀逻辑色，不得被普通蓝色 selected 规则覆盖。

### 第 4 屏

```text
4 / 5


创建自定义插件

在插件设置页面最底端，打开“开发者模式”
打开 ChatGPT插件设置

打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件

名称       Local Bridge                         复制
Tunnel ID  <当前已保存 Tunnel ID>               复制

打开插件管理页


返回                                  继续
```

`打开 ChatGPT插件设置` 只允许 Rust 固定 allowlist 通过系统默认浏览器打开：

`https://chatgpt.com/plugins#settings/Plugins`

`打开插件管理页` 只允许 Rust 固定 allowlist 通过系统默认浏览器打开：

`https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`

前端不能传入、拼接或修改上述 URL；不使用 WebView。两个“打开”按钮都属于左侧操作流，不得右对齐。信息区严格只有 `名称` 与 `Tunnel ID` 两行，不显示“本地服务”。名称固定为 `Local Bridge`；Tunnel ID 必须来自当前已持久化的 StartupProfile，而非尚未保存的输入框值。两行复制按钮状态独立，成功后绿色显示 `已复制` 精确 3 秒并保持按钮几何尺寸不变。第 4 屏底部必须同时有明确 `返回` 与 `继续`。

第 3 屏保存项目与权限后，必须先启动 selected project、本地 runtime / MCP / OpenAI Tunnel，并等待三项 readiness 全部真实就绪，之后才能进入第 4 屏。不得把唯一 runtime 启动边沿留到第 5 屏。第 4 屏必须是用户能够真实创建插件的可执行步骤，而不是服务尚未启动时的说明页。

### 第 5 屏

检查中：

```text
5 / 5


正在准备


● 本地运行环境
● 编码服务
○ OpenAI Tunnel


返回                              确定
                                  灰色/disabled
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
5 / 5


正在准备


● 本地运行环境
● 编码服务
● OpenAI Tunnel


配置完成，在插件中选择刚刚添加的Local Bridge试试吧


返回                              确定
```

此时才：

- 显示完成提示；
- 启用 `确定`；
- 用户点击 `确定` 后设置 `onboarding_complete = true` 并进入主界面。

第 5 屏必须提供明确 `返回` 到第 4 屏。启动检查的状态圆点必须直接跟随同源 typed 状态：`Ready / 正常 → 绿色`、`Starting / 等待 → 黄色/琥珀色`、`Fault / 失败 → 红色`、`Unknown / 未启动 → 灰色`。Dashboard 的主要服务状态旁必须使用完全相同的状态来源与颜色语义，禁止两处各维护一套状态或出现“文字已变化但圆点颜色固定”。

禁止：

- 自动跳转；
- 第 6 屏；
- “ChatGPT 连接”第四个检查项；
- 第二个完成按钮；
- 测试通过前显示完成提示。

除第 1 屏外，第 2/3/4/5 屏都必须有明确 `返回`。任何保存、启动或配置失败都不能把用户锁死，最终启动检查页必须能返回第 4 屏重新配置。

失败时只显示一句最关键错误和一个必要动作。所有按钮共享一致、可辨识的视觉规则；白色或近白背景上不得出现难以识别的纯白/近白按钮。普通产品 primary、普通 selected 与主要交互统一使用原方案蓝色 `#0071e3`，黑色不得作为普通产品 accent；管理员模式是黄色/琥珀逻辑色例外。所有提示遵循最小必要原则。

主窗口固定为 900×620。minimum inner size 与 maximum inner size 均固定为 900×620，`resizable=false`、`maximizable=false`。原生 Windows 窗口 decorations 必须关闭；LocalBridge 只允许一层自定义风格化窗口 chrome，并且外框必须从 client area 的 `(0,0)` 开始、以 100% 宽高贴合整个窗口，不能在原生边框内部再绘制一个内缩“假窗口”。自定义 chrome 必须提供窗口拖拽区、最小化和关闭；不提供最大化。Dashboard 与 onboarding 必须在该固定 client area 内完整可操作。
## UI / Backend 分离

WebView/React 只负责展示 backend typed projection 与发送 typed user intent。runtime 启停、readiness、retry/recovery、workspace switch、credential 写入、UAC 与 CurrentTask truth 均由 Rust/backend 状态机负责；可能耗时的操作必须运行在独立 worker/async 执行上下文，禁止占用 UI/WebView 事件线程。正常前台启动在 onboarding 已完成且持久化配置有效时自动异步启动 selected project/runtime/MCP/OpenAI Tunnel，窗口必须先保持可交互并实时反映 Starting/Ready/Fault。

必须有故意延迟 backend 工作时 UI 仍可响应并持续读取 typed projection 的回归 Gate。

## 设置

固定三组：

```text
常规
────────────────────────────────────────
开机启动                               ○
关闭窗口后继续运行                      ●

连接
────────────────────────────────────────
Tunnel ID          tunnel_6a7ae9...     更换
Runtime API Key    已保存               更换

权限
────────────────────────────────────────
编辑模式       完整模式       管理员模式
                            打开欢迎页    完成
```

`Runtime API Key` 是冻结英文专有字段名，不翻译为“运行密钥”；完整值永不回显。默认连接区只显示持久化摘要和“更换”。点“更换”后才进入编辑态，Tunnel ID 与 Runtime API Key 独立修改；只改其中一个时另一个不得要求重输或被覆盖。Runtime API Key 输入不得用已保存 secret 预填，只显示密码输入控件与提示 `Runtime API Key 仅保存在 Windows 安全凭据中。`。

保存顺序固定：基础格式校验 → 安全写入 → 若当前正在运行/连接且有效连接配置发生变化则执行受控重连。禁止额外“测试连接”按钮。`开机启动` 只控制 Windows 登录启动；手动打开已配置应用仍自动启动 runtime。`关闭窗口后继续运行=true` 时 X 仅 hide 并保持 runtime/tray；false 时 X 有序停止受管 runtime/Broker 后退出。

## 诊断

固定为：

```text
运行状态
────────────────────────────────────────
本地运行环境                         正常
编码服务                             已就绪
OpenAI Tunnel                        故障
管理员权限                           未启用

项目
────────────────────────────────────────
D:\project\LocalBridge

日志
────────────────────────────────────────
<最近、限量、脱敏用户日志>

                         打开日志  导出诊断  完成
```

普通诊断 UI 不显示 Broker/reconnect generation、attempt counter、PID/SID/nonce/IPC，也不提供刷新、重试连接、打开欢迎页等重复/工程动作。导出诊断继续执行严格 secret redaction。

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
