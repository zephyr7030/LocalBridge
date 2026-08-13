
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

窗口默认 900×620、最小 720×500 且可缩放；向导随 viewport 高度响应，主体内容区在空间不足时滚动，不得以固定卡片 `min-height` 保证布局。

窗口缩放验收必须基于真实运行：native main-window client area 与主 WebView bounds 在 resize/maximize 后保持一致，Dashboard 和 onboarding 随 live viewport 重排。仅检查 `.inner_size/.min_inner_size/.resizable`、`100dvh`、`overflow-y:auto` 或固定 `min-height` 不存在，不能证明响应式正确。LB-016 必须运行 Windows Tauri/WebView2 resize E2E，至少覆盖两个 native size 与 maximize，并比较 Tauri `inner_size()`、live JS `window.innerWidth/innerHeight × devicePixelRatio` 与 Dashboard/onboarding rect。
