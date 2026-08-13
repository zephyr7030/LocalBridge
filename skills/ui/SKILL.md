
# Minimal UI Discipline
- 中文优先、最小必要。
- Apple-inspired 只用原生 CSS、system font、local/inline SVG，不新增视觉依赖。
- 当前执行只显示 `● 运行测试 cargo test` 一行。
- 自动恢复 5 次内不新增 UI，全部失败才一个错误窗口。
- 动效低干扰并尊重 `prefers-reduced-motion`。

## 首次启动 6 屏

固定：

```text
欢迎 → OpenAI → 项目与权限 → Local Bridge 设置 → Local Bridge 使用确认 → 启动检查
```

第 3 屏新项目使用原生 Windows 文件夹选择器。

第 4 屏用户可见术语统一为 `Local Bridge`，只允许系统浏览器打开固定 custom-connector URL：
`https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`。

第 5 屏标题为 `Local Bridge 使用确认`，不伪造 ChatGPT 状态；如展示/复制 endpoint，只能使用 Rust typed projection 的已验证 Tunnel/control-plane metadata，禁止根据 Tunnel ID 推导；复制反馈不得造成布局位移。

第 6 屏未 Ready：`确定` 灰色 disabled，完成提示隐藏。

全 Ready：三项绿色，显示 `配置完成，在插件中选择刚刚添加的Local Bridge试试吧`，启用 `确定`。

必须由用户点击“确定”进入主界面，不自动跳转。

按钮必须统一且可辨识，禁止白底白按钮；提示只保留当前动作所需的最少信息。

窗口固定 900×620；minimum/maximum inner size 都是 900×620，`resizable=false`、`maximizable=false`。native `decorations=false`；只允许一层 edge-to-edge 自定义 chrome，必须贴满 client area，禁止双边框。自定义 chrome 提供拖拽、最小化、关闭，无最大化。Dashboard 和 onboarding 必须在固定 client area 内完整可操作。

首次 onboarding 必须是整页内容，不是弹窗：直接使用 custom chrome 内容区，禁止空白画布中再套居中的整体 `.card` / modal / dialog，也禁止用大圆角、整体阴影或边框形成二级窗口。正常页面 padding、字段和局部分组不受此限制。
