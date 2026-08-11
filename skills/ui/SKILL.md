
# Minimal UI Discipline
- 中文优先、最小必要。
- Apple-inspired 只用原生 CSS、system font、local/inline SVG，不新增视觉依赖。
- 当前执行只显示 `● 运行测试 cargo test` 一行。
- 自动恢复 5 次内不新增 UI，全部失败才一个错误窗口。
- 动效低干扰并尊重 `prefers-reduced-motion`。

## 首次启动 5 屏

固定：

```text
欢迎 → OpenAI → 项目与权限 → ChatGPT → 启动检查
```

第 5 屏未 Ready：`确定` 灰色 disabled，完成提示隐藏。

全 Ready：三项绿色，显示 `设置完成，尝试在插件中选择刚刚添加的工具吧！`，启用 `确定`。

必须由用户点击“确定”进入主界面，不自动跳转。
