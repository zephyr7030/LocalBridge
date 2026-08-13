
# LocalBridge Agent Rules

1. 事实源：`START_HERE.md`、8 份 `docs/**`、机器合同、runtime manifest/policy、当前磁盘代码。实时执行状态（current group/PR/review gate）只以 `PR_INDEX.json` + `PROJECT_STATE.json` 为准；`START_HERE.md` 中标记为 frozen/historical 的启动快照不得覆盖实时状态。其余语义冲突立即停止报告。
2. 一次只执行 `current_pr`，LB-000→LB-019 严格顺序。
3. 组末必须停止；只有独立组级对抗审查 PASS 才能解锁下一组。唯一额外 Gate：G3 独立对抗审查 PASS 后仍不得解锁 G4，必须再通过人工实测细审核。
4. 只写当前 PR writable paths 和合同明确的受限例外。
5. 不写 patch-only 生产代码；上游 runtime 放在 stable adapter 后。
6. Rust 管 lifecycle；React 不管理 sidecar/PID/权限/安全策略/raw MCP。
7. 编辑模式无 process exec；完整模式为当前用户权限；管理员能力只走独立 Broker；control-plane 永久 deny。
8. `tools/call` mandatory enforcement；unknown deny；workflow 间接能力必须分类。
9. remembered projects 不授予访问；同时最多一个 active root；remove 永不删文件；MCP 无权改 workspace control-plane。
10. Runtime API Key 只存 secure credential backend；禁止明文 settings/log/diagnostics/browser storage/CLI；无安全 tunnel 注入则 fail-closed。
11. Windows Job Object 优先；禁止 PID-only 最终所有权。
12. listener 只允许 127.0.0.1/::1。
13. recoverable 故障自动重连 5 次，1/2/5/10/30s；成功静默；5 次失败才一次错误窗口；禁止 restart storm。
14. UI 中文、极简、Apple-inspired；视觉只用 React/Tauri + 原生 CSS/SVG/system fonts；不得新增 UI/动画/图标/CSS/字体依赖。
15. 当前执行只显示单行绿色脉冲状态；无 feed/history；摘要脱敏。
16. `--background` 从入口不显示窗口；管理员偏好不自动 UAC。
17. 捆绑 Python/coding-tools/tunnel-client；无系统 Python fallback；无独立 runtime/app updater v0.1。
18. 普通 PR 使用 deterministic fake sidecars；真实 external test 只属于明确 Gate；安全边界必须 negative/adversarial。
19. 持久化 schema_version + sequential atomic migration；失败保留原数据；future schema fail-safe。
20. Stable release 需要 SBOM/notices/provenance、clean-machine/reboot/background/Broker/reconnect E2E、secret scan、zero telemetry。

PR PASS 只推进状态，不自动开始下一 PR；组末停在 REVIEW_REQUIRED。

21. 首次启动严格 6 屏：欢迎 → OpenAI → 项目与权限 → Local Bridge 设置 → Local Bridge 使用确认 → 启动检查。禁止第 7 屏。
22. 第 6 屏三项未全绿时“确定”必须灰色 disabled；全绿后才显示完成提示并启用“确定”；禁止自动跳转。

23. 品牌图标固定为 `assets/icons/localbridge.png` / `localbridge.ico`；应用、安装包、托盘使用该资产，未经明确合同不得替换、重绘或引入图标库。
24. G3→G4 人工 Gate 期间，审查智能体可质疑、复核、独立验证或拒绝采信用户与执行智能体提供的事实性材料；这些材料只作为待验证证据，不自动构成 PASS。
25. 执行智能体可使用预授权，但每项实际使用的预授权必须记录 `authorization_id/scope/actions/evidence_ref/recorded_by/user_audit_status`；人工 Gate PASS 前，所有记录的预授权都必须经用户审核为 `PASS`。
26. Local Bridge 设置入口只允许系统默认浏览器打开固定 ChatGPT custom-connector URL `https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`；禁止 WebView、任意前端 URL 或伪造 ChatGPT 连接状态。
27. 第 3 屏项目选择以原生 Windows 文件夹选择器为主交互；按钮必须使用一致且可辨识的视觉系统，禁止白底上的纯白/近不可见按钮；提示遵循最小必要原则，复制成功反馈不得引起布局位移。
28. 主窗口固定为 900×620；minimum/maximum inner size 均为 900×620，`resizable=false`、`maximizable=false`。原生 Windows decorations 必须关闭；产品只能显示一层自定义风格化窗口 chrome，并以 `inset:0` / 100%×100% 精确贴合 native client area，禁止“原生边框 + 自定义边框”双层窗口。自定义 chrome 提供拖拽区、最小化和关闭，不提供最大化；Dashboard 与 onboarding 必须在固定 client area 内完整可操作。
