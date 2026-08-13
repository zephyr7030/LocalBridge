# 04 — UX Specification

## UX 原则

1. 普通用户不看内部端口、PID、SID、nonce、IPC 等工程字段。
2. 主流程一次只要求一个决策。
3. 状态异常时必须给出最小必要下一步；正常状态不堆叠解释。
4. 高级技术信息默认只进入诊断。
5. 不内嵌 ChatGPT，不读取 ChatGPT cookie/session。
6. 窗口只是控制面板；后台运行服务与窗口生命周期分离。
7. 所有用户可见文案遵循 `docs/14_UI_TERMINOLOGY_POLICY.md`。

# 首次引导

严格 5 屏，禁止第 6 屏。顺序固定为：欢迎 → OpenAI → 项目与权限 → 创建自定义插件 → 启动检查。旧 `Local Bridge 使用确认` 页面已废弃并删除。所有步骤遵循“只提示当前必须完成的动作”。

## 整页视觉结构

- onboarding 是窗口本身的内容页，不是弹窗；
- 根页面直接占满自定义 chrome 的可用内容区；
- 禁止“大面积空白背景 + 居中向导卡片”的构图；
- 禁止以整个向导为对象使用 modal/dialog/floating-card 外壳，也禁止通过大圆角、整体阴影或外框制造二级窗口；
- 允许正常页面内边距、字段分组、局部状态块和按钮区域。

## 第 1 屏 — 欢迎

标题：`简单设置 即可开始`

说明：`LocalBridge是链接ChatGPT与本地代码的工具`

主按钮：`开始`

不增加非必要“了解更多”按钮。

## 第 2 屏 — OpenAI

字段：

- `Tunnel ID`
- `Runtime API Key`

运行密钥保存后只显示：

`已保存`

界面不得回显密钥，不提供显示完整密钥按钮，也不得把密钥放入前端持久化存储。

## 第 3 屏 — 项目与权限

项目与权限位于同一屏。新项目通过原生 Windows 文件夹选择器选择；存在已保存项目时可低干扰显示选择列表。手填路径不得作为主流程。

### 编辑模式（默认）

说明：

`允许读取、搜索和修改项目文件，不允许运行本地命令。`

### 完整模式

说明：

`允许运行测试、编译和其他本地命令。命令拥有当前 Windows 用户的实际系统权限。`

### 管理员模式

说明：

`在完整模式基础上允许管理员操作。启用时 Windows 会显示系统授权窗口。`

管理员模式：

- 无时间限制；
- 可见用户点击/重新点击“管理员模式”即显式请求 Windows UAC；不存在单独“启用管理员权限”按钮；
- 切换到编辑/完整模式立即关闭 privileged call gate 并停止 Broker；
- 后台开机恢复管理员偏好不会自动弹出系统授权；
- 只有管理员代理获得管理员令牌。

三个权限模式按钮不得设置会挤压说明文字的固定高度；标题与说明之间使用明确间距，四周保留均衡内容留白，说明换行时按钮随内容自动增高，并以 `min-height >= 80px` 或等效结构证明保证“标题 + 两行说明 + 上下留白”。最终视觉质量必须在固定 900×620 实际运行窗口中人工确认；自动化可防止压扁/缺少换行等结构回退，但不得仅因 CSS 出现 `padding/min-height` 就判定视觉 PASS。普通选中项使用蓝色 `#0071e3`；管理员模式在 onboarding 与设置页均使用黄色/琥珀逻辑色，禁止普通蓝色 selected 规则覆盖管理员色。第 3 屏必须有明确 `返回`。

## 第 4 屏 — 创建自定义插件

标题固定为 `创建自定义插件`。提示固定表达：`在插件设置页面最底端，打开“开发者模式”`。

`打开 ChatGPT插件设置` 只能由 Rust 固定 allowlist 通过系统默认浏览器打开：

`https://chatgpt.com/plugins#settings/Plugins`

该按钮必须位于左侧操作流。其下固定显示：`打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件`。

中部严格只有两行：`名称 = Local Bridge`、`Tunnel ID = 当前已持久化保存值`。禁止“本地服务”行；Tunnel ID 禁止直接使用尚未保存的 React/input 临时值。两行各有独立复制按钮，成功后绿色 `已复制` 精确保持 3 秒再恢复 `复制`，按钮几何尺寸不得变化。

`打开插件管理页` 只能由 Rust 固定 allowlist 通过系统默认浏览器打开：

`https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`

该按钮也必须位于左侧操作流。两个浏览器入口均禁止 WebView、禁止前端传入/拼接/修改 URL、禁止读取 ChatGPT 会话。第 4 屏底部必须同时提供明确 `返回` 与 `继续`。

第 3 屏保存项目与权限后必须先启动 selected project、本地 runtime / MCP / OpenAI Tunnel；只有本地运行环境、编码服务、OpenAI Tunnel 三项全部真实就绪后才进入第 4 屏。第 5 屏不得承担 onboarding 的唯一 runtime 启动边沿；第 4 屏必须是真实可执行的插件创建步骤。

## 第 5 屏 — 启动检查

严格只显示：

```text
本地运行环境
编码服务
OpenAI Tunnel
```

状态统一使用中文：

```text
等待检查
正在检查
已通过
需要处理
```

状态圆点必须与同源 typed 状态真实联动：`Ready / 正常 = 绿色`、`Starting / 等待 = 黄色/琥珀色`、`Fault / 失败 = 红色`、`Unknown / 未启动 = 灰色`。Dashboard 主要服务状态使用同一状态来源和同一颜色语义，不允许另建互相冲突的状态。

失败项只给一个最有价值的修复动作，不暴露 Guard、PID、端口、transport 等内部结构。

三项未全绿时 `确定` disabled 且不显示完成提示；全绿后才显示 `配置完成，在插件中选择刚刚添加的Local Bridge试试吧` 并启用 `确定`。禁止自动跳转。第 5 屏必须有明确 `返回` 到第 4 屏。

除第 1 屏外，第 2/3/4/5 屏全部必须有明确返回路径；任何保存、启动、配置失败均不得锁死用户。

## 固定窗口

- 主窗口固定 900×620；
- minimum inner size = 900×620；maximum inner size = 900×620；
- `resizable=false`，用户不能拖拽边框改变窗口尺寸；
- `maximizable=false`，最大化入口不可用；
- native `decorations=false`，不得同时显示 Windows 原生标题栏/边框与产品风格化边框；
- 自定义 chrome 必须是唯一窗口外框，`inset:0` 且覆盖 100% client area，不允许二次内缩形成“窗口里的窗口”；
- 自定义 chrome 提供可用拖拽区、最小化按钮、关闭按钮，不提供最大化按钮；
- Dashboard 与 onboarding 必须在固定 900×620 client area 内完整可操作，不再要求 resize/maximize 响应式布局或 resize E2E。

## 按钮与提示

- 产品 UI 使用一套一致的按钮几何、层级、边界和状态规则；
- 普通 primary、普通 selected 与主要交互统一使用原方案蓝色 `#0071e3`，黑色不得作为普通产品 accent；管理员模式使用黄色/琥珀逻辑色；
- 白色/近白背景上的次要按钮必须有清晰边界或足够对比，不得“白底白按钮”；
- 不为自解释操作堆叠重复说明；
- 成功/复制状态预留空间，不推动周围内容；
- 同一页面/分组的同级动作按钮必须共享动作列和水平左基线；固定 900×620 自动几何 Gate 容差 `<= 1 CSS px`。设置连接区两个“更换”按钮是强制样例，不得随 Tunnel ID/“已保存”文本宽度漂移；
- `无法准备管理员权限`、一次性保存/选择失败等 one-shot 提示默认 3 秒自动清除；持续 runtime/reconnect fault 仍通过 typed 状态/故障窗口保持，不得错误套用临时提示规则。

## UI / Backend 执行边界

React/WebView 只渲染 backend typed projection 并发送 typed user intent。runtime start/readiness、retry/recovery、workspace switch、credential/UAC、CurrentTask truth 必须由 backend 持有；耗时工作在独立 worker/async 执行上下文完成。故意延迟 backend 工作时窗口仍必须可交互、可刷新状态。onboarding 不得用 React polling loop 作为 runtime readiness 状态机。

# 主控界面

只保留一个主控界面，不做多页工程控制台。

必须直接显示：

```text
当前项目
OpenAI 安全隧道状态
编码服务状态
管理员权限实际运行状态
```

主控界面禁止显示 `权限模式` 行，也禁止出现 `编辑模式 / 完整模式 / 管理员模式` 三档选择控件。PermissionMode 编辑允许设置页“权限”以及用户显式重新打开欢迎/onboarding 后的第 3 屏；后者不构成冲突。主控界面的 `管理员权限实际运行状态` 仅为只读 `PrivilegeState` 投影；Dashboard 不得改变 PermissionMode，也不得通过权限模式触发 UAC。

必要操作按当前状态显示：

- `打开 ChatGPT`
- `切换项目`
- `重启服务`
- `关闭服务`
- `重新连接`（仅连接异常时）
- `设置`
- `诊断`

最近项目只有存在历史时才显示。

`重启服务` 固定使用黄色/琥珀逻辑色；`关闭服务` 固定使用红色逻辑色。两者复用既有 shared button tokens 与动作列，不得新建第二套形状、高度、字体或对齐规则；真实 lifecycle 由 backend 执行。

## 管理员权限状态

Rust `PrivilegeState` 是唯一真相源，界面不能根据权限偏好猜测。

映射：

```text
Disabled      → 未启用
Requested     → 等待授权
AwaitingUac   → 等待系统授权
Active        → 已启用
Faulted       → 故障
```

Dashboard 只读状态示例：

```text
管理员权限：等待授权
```

真正激活后：

```text
管理员权限：已启用
```

若用户要激活管理员模式或切换回编辑/完整模式，可进入设置页“权限”或显式重新打开欢迎/onboarding 后的第 3 屏；Dashboard 不提供这些操作。

故障：

```text
管理员权限：故障         [查看诊断]
```

不显示剩余时间，因为管理员模式没有 TTL。


## 当前执行状态

主控界面只保留一行，不显示标题和字段名。

执行中：

```text
●  运行测试  cargo test · 12S
```

要求：

- 绿色圆点；
- 圆点进行轻量脉冲动效，表示仍在活动；
- 不显示“当前任务”“类型”“任务”“状态”等标题；
- 不显示“执行中”字样；
- 不做卡片式消息；
- 不产生最近活动或历史列表；
- 持续时间来自 backend 真实任务起始时间，前端不得伪造开始/结束；
- 文件新建、删除、修改及普通命令即使短于 UI 刷新周期，也必须由 backend current/last timing projection 捕获，禁止把 frontend polling 当作唯一事件捕获机制。

其他执行示例：

```text
●  读取文件  src-tauri/src/runtime/mod.rs · 2S
●  搜索代码  RuntimeState · 5S
●  修改文件  src/lib/presentation/status.ts · 3S
●  构建  cargo build · 18S
●  管理员操作  安装设备驱动 · 等待授权 · 7S
```

无活动任务：

```text
○  等待命令
```

已有上一条真实命令时显示 `○ 等待命令 · <相对时间>`；相对时间至少覆盖 `nS前`、`n分钟前`、`大于1小时`、`大于n天`。从未执行命令时只显示 `等待命令`。只允许保留上一条 timing metadata，不形成用户可浏览 history/feed/list。

`等待命令` 为必须可见的中性静态状态，无动画；禁止显示“空闲”或隐藏整行。该状态及活动状态均来自 backend `CurrentTaskStatus`，前端不得自行维护任务 truth。

活动圆点必须尊重 `prefers-reduced-motion`：系统减少动态时改为静态绿色点。

终态只允许短暂显示，随后回到 `等待命令` 并保留上一条完成时间元数据，不形成消息流。


## 项目新增、选择与移除

主控界面保持极简，不增加“项目管理中心”。

当前项目区域：

```text
D:\project\LocalBridge   [切换]
```

点击“切换”打开项目选择器：

- 已保存项目；
- `选择其他文件夹`；
- 对已有项目提供低存在感 `移除` 操作。

新增项目：

```text
选择其他文件夹
→ 校验
→ 去重
→ 切换
```

非当前项目移除：

- 直接从 LocalBridge 项目列表移除；
- 不删除磁盘目录；
- 不需要确认弹窗。

当前项目移除：

只允许一次简短确认：

```text
从 LocalBridge 移除此项目？
不会删除项目文件。

[取消] [移除]
```

确认后停止本地运行服务，清除当前项目，主控界面回到：

```text
选择项目
```

不得自动切换到另一个已保存项目。

“没有当前项目”不是故障状态。

用户可见项目路径不得显示 Win32 verbatim 前缀。例如内部 resolved path 可为 `\\?\D:\project`，Dashboard/项目选择/诊断必须呈现 `D:\project`。显示转换不参与 validated identity、reparse 防护或授权边界。

# 托盘

菜单保持最少：

```text
LocalBridge
────────────
状态：已连接
当前项目：RocoSlayer-NG
权限模式：编辑模式
管理员权限：未启用

打开主控界面
打开 ChatGPT
切换项目
重新连接
────────────
退出 LocalBridge
```

其中 `重新连接` 可仅在异常状态出现。

单击托盘图标：

`恢复并聚焦主控界面`

关闭窗口：

`隐藏窗口，不停止后台服务`

退出 LocalBridge：

`停止受管运行服务和管理员代理后退出`

# 设置

只保留三组必要配置：

## 常规

```text
开机启动                               ○
关闭窗口后继续运行                      ●
```

`开机启动` 仅控制 Windows 登录启动注册；已完成配置的应用被用户正常打开时，runtime 必须自动异步启动。`关闭窗口后继续运行=true`：关闭 X 仅隐藏窗口，服务与托盘继续；false：有序停止 runtime/Broker 后退出。该偏好必须版本化持久化。禁止“开机后静默运行”“自动启动服务”等重复开关。

## 连接

默认固定态：

```text
Tunnel ID          tunnel_6a7ae9...     更换
Runtime API Key    已保存               更换
```

两个“更换”占用同一固定动作列，在 900×620 下左边缘差 `<= 1 CSS px`，不受左侧摘要宽度影响。

完整 `Runtime API Key` 永不显示。点击对应“更换”才进入连接编辑态：

```text
Tunnel ID
[________________________________]

Runtime API Key
[••••••••••••••••••••••••••••]

Runtime API Key 仅保存在 Windows 安全凭据中。

取消                                  保存
```

Tunnel ID 与 Runtime API Key 独立更新：只改 Tunnel ID 不要求重输/改动密钥；只改密钥不改 Tunnel ID。保存执行“基础格式校验 → 安全写入 → 若 runtime 正在运行/连接且有效连接配置变化则受控重连”。“正在连接”包括 `StartingMcp / WaitingMcpReady / StartingPolicyEnforcement / WaitingPolicyReady / StartingTunnel / WaitingTunnelReady` 等异步启动阶段；这些阶段配置变化不得因 `active=false` 直接返回并继续使用旧 captured config，最终运行实例必须使用最新持久化配置。禁止“测试连接”按钮。

## 权限

```text
编辑模式       完整模式       管理员模式
                            打开欢迎页    完成
```

可见点击/重新点击管理员模式即显式 UAC 动作；无单独“启用管理员权限”按钮。管理员模式无 TTL。

# 诊断

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
19:36:42  OpenAI Tunnel 连接失败
19:36:12  自动恢复失败
19:36:02  自动恢复失败

                         打开日志  导出诊断  完成
```

日志是 backend 提供的最近、限量、脱敏用户事件。普通诊断页不显示 Broker generation、reconnect generation/attempt、PID/SID/nonce/IPC；不提供刷新、重试连接或打开欢迎页。导出诊断可以包含更多安全 typed 信息，但必须 redacted。

# 界面语言硬规则

用户界面不得出现以下普通英文标签：

```text
Dashboard
Settings
Diagnostics
Edit
Full
Elevated
Broker
Runtime
Ready
Active
Faulted
Requested
```

专业缩写和正式专有名词可保留，例如：

```text
MCP
API
UAC
OpenAI
ChatGPT
Windows
SHA256
Tunnel ID
Runtime API Key
```

其中 `Tunnel ID` / `Runtime API Key` 是连接设置冻结字段名，必须原样显示；尤其禁止把 `Runtime API Key` 改写成“运行密钥”。

内部状态必须经过：

```text
domain state
→ presentation mapper
→ 简体中文用户文案
```

禁止中英双语堆叠，例如：

`管理员模式 (Elevated)`
