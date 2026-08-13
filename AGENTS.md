
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

21. 首次启动严格 5 屏：欢迎 → OpenAI → 项目与权限 → 创建自定义插件 → 启动检查。`Local Bridge 使用确认` 整页已废弃并删除；禁止第 6 屏。
22. 第 5 屏三项未全绿时“确定”必须灰色 disabled；全绿后才显示完成提示并启用“确定”；禁止自动跳转。

23. 品牌图标固定为 `assets/icons/localbridge.png` / `localbridge.ico`；应用、安装包、托盘使用该资产，未经明确合同不得替换、重绘或引入图标库。
24. G3→G4 人工 Gate 期间，审查智能体可质疑、复核、独立验证或拒绝采信用户与执行智能体提供的事实性材料；这些材料只作为待验证证据，不自动构成 PASS。
25. 执行智能体可使用预授权，但每项实际使用的预授权必须记录 `authorization_id/scope/actions/evidence_ref/recorded_by/user_audit_status`；人工 Gate PASS 前，所有记录的预授权都必须经用户审核为 `PASS`。
26. 第 4 屏标题固定为“创建自定义插件”，提示固定表达“在插件设置页面最底端，打开‘开发者模式’”。`打开 ChatGPT插件设置` 必须位于左侧操作流，只允许 Rust 固定 allowlist 通过系统默认浏览器打开 `https://chatgpt.com/plugins#settings/Plugins`，前端不得传入、拼接或修改 URL，禁止 WebView。其下必须显示提示 `打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件`。信息区严格只有两行可复制信息：`名称：Local Bridge`、`Tunnel ID：<当前持久化保存值>`；禁止“本地服务”行。Tunnel ID 必须来自 Rust 当前已持久化 StartupProfile，而非 React/input 临时值。两行各有独立复制按钮，成功后按钮以绿色稳定显示“已复制”精确 3 秒再恢复“复制”，切换不得造成布局位移。`打开插件管理页` 同样必须位于左侧操作流，只允许 Rust 固定 allowlist 通过系统默认浏览器打开 `https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`；禁止任意 URL、WebView 或伪造 ChatGPT 连接状态。第 4 屏底部必须明确提供“返回”和“继续”。
27. 第 3 屏项目选择以原生 Windows 文件夹选择器为主交互；三个权限模式按钮必须为标题与说明文字预留清晰、均衡的上下/左右内容边距，文字不得在 900×620 实际渲染中贴近或碰触按钮边框，且换行时按钮高度必须随内容安全增长；该项必须保留为 900×620 实机人工视觉验收项，自动化只能检查结构性防回退，禁止仅凭 CSS 存在 padding 等标记判定视觉 PASS。普通选中项使用统一蓝色强调色；管理员模式在 onboarding 与 Dashboard 均使用黄色/琥珀逻辑色，禁止被全局蓝色 selected 规则覆盖。按钮必须一致且可辨识，禁止白底上的纯白/近不可见按钮；提示遵循最小必要原则，复制成功反馈不得引起布局位移。
28. 主窗口固定为 900×620；minimum/maximum inner size 均为 900×620，`resizable=false`、`maximizable=false`。原生 Windows decorations 必须关闭；产品只能显示一层自定义风格化窗口 chrome，并以 `inset:0` / 100%×100% 精确贴合 native client area，禁止“原生边框 + 自定义边框”双层窗口。自定义 chrome 提供拖拽区、最小化和关闭，不提供最大化；Dashboard 与 onboarding 必须在固定 client area 内完整可操作。
29. 首次 onboarding 本身就是窗口内容页，必须直接占满自定义 chrome 的可用内容区；禁止在大面积空白背景中再居中放置作为整个向导外壳的 card/modal/dialog，也禁止用圆角、阴影或边框制造“窗口里的弹窗”。允许页面级 padding、字段分组和局部控件，但 5 屏共同外壳必须是整页布局。
30. 第 4 屏是创建插件的真实可执行步骤，而不是提前展示的说明页：第 3 屏保存项目与权限后必须启动 selected project、本地 runtime / MCP / OpenAI Tunnel，并等待 `本地运行环境 + 编码服务 + OpenAI Tunnel` 全部真实就绪后才允许进入第 4 屏；不得把唯一 runtime 启动边沿延迟到第 5 屏。
31. 除第 1 屏外，onboarding 第 2/3/4/5 屏都必须有明确“返回”路径；任何保存、启动或配置失败均不得锁死用户，最终启动检查页也必须能返回第 4 屏重新配置。
32. 普通产品强调色固定恢复为原 onboarding 蓝色 `#0071e3`：primary、普通 selected 与主要交互统一使用蓝色体系，黑色 `#1d1d1f` 仅可作为文字/中性色，不得作为普通产品 accent。管理员模式是唯一逻辑色例外，使用黄色/琥珀色。
33. Onboarding 启动检查与 Dashboard 主要服务状态必须消费同源 typed 状态投影并使用同一颜色语义：Ready/正常=绿色，Starting/等待=黄色或琥珀色，Fault/失败=红色，Unknown/未启动=灰色。禁止文字变化而圆点颜色固定，也禁止两处各维护一套互相冲突的状态。
