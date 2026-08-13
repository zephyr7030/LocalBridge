
# Minimal UI Discipline
- 中文优先、最小必要。
- Apple-inspired 只用原生 CSS、system font、local/inline SVG，不新增视觉依赖。
- 当前执行只显示 `● 运行测试 cargo test` 一行。
- 自动恢复 5 次内不新增 UI，全部失败才一个错误窗口。
- 动效低干扰并尊重 `prefers-reduced-motion`。

## 首次启动严格 5 屏

固定：

```text
欢迎 → OpenAI → 项目与权限 → 创建自定义插件 → 启动检查
```

禁止第 6 屏；旧 `Local Bridge 使用确认` 整页已删除。除第 1 屏外，第 2/3/4/5 屏都必须有明确 `返回`，任何保存、启动或配置失败不得锁死用户。

第 3 屏新项目使用原生 Windows 文件夹选择器。三个权限模式按钮不得固定高度压缩说明；换行必须安全自动增高，900×620 实机内容与边框留白由人工视觉 Gate 验收，不能仅凭 CSS `padding` 判 PASS。普通 selected 使用蓝色 `#0071e3`；管理员模式在 onboarding 与 Dashboard 使用黄色/琥珀逻辑色，不得被普通蓝色 selected 覆盖。

第 4 屏标题固定为 `创建自定义插件`，提示固定表达 `在插件设置页面最底端，打开“开发者模式”`。`打开 ChatGPT插件设置` 必须位于左侧操作流，其下固定显示 `打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件`。中部严格只有 `名称 = Local Bridge` 与 `Tunnel ID = 当前持久化保存值` 两行，禁止“本地服务”；两行独立复制反馈绿色 `已复制` 精确保持 3 秒且不得位移。

`打开 ChatGPT插件设置` 只允许 Rust 固定 allowlist 通过系统浏览器打开 `https://chatgpt.com/plugins#settings/Plugins`；`打开插件管理页` 同样必须位于左侧操作流，只允许 Rust 固定 allowlist 通过系统浏览器打开 `https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`。前端不得提供、拼接或修改任意 URL，禁止 WebView。第 4 屏底部必须同时有 `返回` 与 `继续`。

第 3 屏保存后必须启动 selected project/runtime/MCP/OpenAI Tunnel，并在三项 readiness 全部真实就绪后才能进入第 4 屏；唯一 runtime 启动边沿不得延迟到第 5 屏。第 4 屏必须是真实可执行的插件创建步骤。

第 5 屏为 `启动检查`，严格只显示本地运行环境、编码服务、OpenAI Tunnel。状态点必须消费与 Dashboard 相同的 typed 状态来源，并映射 Ready=绿色、Starting=黄色/琥珀色、Fault=红色、Unknown=灰色。未 Ready：`确定` 灰色 disabled，完成提示隐藏；第 5 屏仍可返回第 4 屏。

全 Ready：三项绿色，显示 `配置完成，在插件中选择刚刚添加的Local Bridge试试吧`，启用 `确定`。

必须由用户点击“确定”进入主界面，不自动跳转。

按钮必须统一且可辨识，禁止白底白按钮；普通 primary、普通 selected 与主要交互统一使用原方案蓝色 `#0071e3`，黑色不得作为普通产品 accent；管理员模式使用黄色/琥珀逻辑色。提示只保留当前动作所需的最少信息，状态/复制反馈不得造成布局位移。

窗口固定 900×620；minimum/maximum inner size 都是 900×620，`resizable=false`、`maximizable=false`。native `decorations=false`；只允许一层 edge-to-edge 自定义 chrome，必须贴满 client area，禁止双边框。自定义 chrome 提供拖拽、最小化、关闭，无最大化。Dashboard 和 onboarding 必须在固定 client area 内完整可操作。

首次 onboarding 必须是整页内容，不是弹窗：直接使用 custom chrome 内容区，禁止空白画布中再套居中的整体 `.card` / modal / dialog，也禁止用大圆角、整体阴影或边框形成二级窗口。正常页面 padding、字段和局部分组不受此限制。
