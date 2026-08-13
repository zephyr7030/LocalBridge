
# LocalBridge v15 FINAL — Start Here

> 本文件是开发前 **冻结启动快照（historical bootstrap snapshot）**，用于说明读取顺序与冻结约束，不承载推进中的实时 PR/Gate 状态。
> 实时 `current_group`、`current_pr`、group review 状态只读取 `PR_INDEX.json` 与 `PROJECT_STATE.json`；若二者互相冲突，立即停止并报告。

```text
bootstrap_phase        = PRE_CODE
bootstrap_group        = G0
bootstrap_pr           = LB-000
predevelopment_review  = PASS
```

读取顺序：

1. `AGENTS.md`
2. `docs/01_PRODUCT_UX.md`
3. `docs/02_ARCHITECTURE_RUNTIME.md`
4. `docs/03_SECURITY.md`
5. `docs/04_UPSTREAM_DISTRIBUTION.md`
6. `docs/05_MAINTENANCE_RELEASE.md`
7. `docs/06_PR_GROUPS_AND_EXECUTION.md`
8. `docs/07_ACCEPTANCE.md`
9. `docs/08_FINAL_REVIEW.md`
10. `skills/**`
11. `PR_INDEX.json`
12. `PR_CONTRACTS.json`
13. `PROJECT_STATE.json`
14. `ARCHITECTURE_RULES.json`
15. `COMPATIBILITY_BASELINE.json`
16. `FINAL_REVIEW.json`
17. `runtime-manifest.toml`
18. `runtime-policy.toml`

机器合同与文档冲突：立即停止并报告。冻结/历史快照中的旧状态字段不属于实时状态冲突。

历史启动任务（冻结快照）：

```text
G0
LB-000 — Upstream + Security Compatibility Spike
```

LB-000 PASS 后不能直接执行 LB-001：

```text
G0 PR PASS
→ G0 独立对抗审查 PASS
→ 才解锁 G1 / LB-001
```

冻结：

```text
自动重连       = 5 次；1/2/5/10/30s
重连 UI        = 5 次内不新增；全失败后一次错误窗口
UI             = Apple-inspired 极简
视觉新增依赖   = 禁止
Runtime API Key= secure store；无明文/CLI
授权项目       = 同时最多一个
项目移除       = 永不删除磁盘文件
PR             = 20
Groups         = 5
组间审查       = mandatory independent adversarial PASS
```

UI 冻结补充：

```text
首次启动       = 严格 5 屏
顺序           = 欢迎 → OpenAI → 项目与权限 → 创建自定义插件 → 启动检查
第 6 屏        = 禁止；`Local Bridge 使用确认` 页面已删除
第 5 屏确认    = 三项全绿后才启用
自动进入主界面 = 禁止
创建自定义插件 = 两个 Rust 固定 allowlist 系统浏览器入口且均位于左侧操作流；插件设置按钮下显示“打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件”；信息仅“名称 / Tunnel ID”；禁止 WebView/任意 URL/本地服务行；底部有返回/继续
第3→4屏       = 保存项目与权限后先启动项目/runtime/MCP/Tunnel并全就绪，再进入第4屏
返回路径       = 除第1屏外，第2/3/4/5屏均必须明确可返回；失败不得锁死
项目选择       = 原生 Windows 文件夹选择器为主交互
按钮           = 统一且可辨识；禁止白底白按钮
普通强调色     = 蓝色 #0071e3；黑色不得作为普通 primary/selected accent
管理员逻辑色   = onboarding 与设置页均为黄色/琥珀色，不得被蓝色 selected 覆盖
服务状态点     = Ready绿 / Starting琥珀 / Fault红 / Unknown灰；onboarding 与 Dashboard 使用同源状态
窗口           = 固定 900×620；minimum=maximum=900×620；禁止缩放与最大化
窗口验收       = `resizable=false`、`maximizable=false`；拖拽边框/最大化均不能改变 client size
窗口边框       = `decorations=false`；唯一自定义 chrome 必须贴满 client area；禁止原生+自定义双边框；保留拖拽/最小化/关闭
引导页布局     = 整页；直接使用 custom chrome 内容区；禁止空白页面中居中再套 card/modal/dialog 式向导外壳
管理员选择     = 仅设置页或 onboarding 第3屏可见选择；点击“管理员模式”即为显式 UAC 动作；无单独“启用管理员权限”按钮；后台恢复偏好仍不自动 UAC
主页权限       = 不显示“权限模式”及编辑/完整/管理员三档选项；不得从主页修改 PermissionMode 或触发模式 UAC；只读显示管理员权限实际状态；权限编辑允许设置页与用户显式重新打开的 onboarding 第3屏
前台启动       = onboarding 完成且配置有效时自动、异步启动 runtime/MCP/Tunnel；UI 不等待后端阻塞工作
任务状态       = backend 真实执行绑定；短任务不得被 UI 轮询漏掉；执行中显示持续时间；待机显示“等待命令”，有上一条命令时追加 nS前/n分钟前/大于1小时/大于n天；无历史列表
临时提示       = “无法准备管理员权限”等一次性操作提示默认 3 秒自动消失；持续 runtime fault 仍由 typed 状态表达
项目路径边界   = \\?\D:\project 仅限内部 filesystem identity 校验；UI 与 MCP/Broker/sidecar/process/command/tool 的 cwd/workdir/current_dir/路径参数必须使用同一 validated identity 对应的普通 D:\project；verbatim 工作目录不得传给命令工具，execution/display 转换不得参与授权
按钮对齐       = 同级动作使用同一左基线/动作列；900×620 几何差≤1 CSS px；设置两项“更换”为强制样例
服务控制       = Dashboard 提供黄色/琥珀“重启服务”与红色“关闭服务”；backend 真实 lifecycle，统一按钮设计与对齐；重启不自动 UAC
设置           = 常规/连接/权限三组；连接字段固定英文 `Tunnel ID` / `Runtime API Key`；独立“更换”；保存即验证并按需受控重连；无“测试连接”
诊断           = 运行状态/项目/最近脱敏日志；只保留“打开日志 / 导出诊断 / 完成”
关闭窗口       = 设置项“关闭窗口后继续运行”；开=hide+后台继续，关=有序退出
UI/backend     = frontend 纯投影；耗时 lifecycle/process/credential/UAC/recovery 工作必须离开 UI/WebView 线程
权限按钮高度   = 900×620 下结构基线 min-height≥80px 或等效证明，且仍需人工视觉 Gate
Cloudflare     = LB-018 从最终 bundle/manifest/installer/启动参数/fallback 移除 cloudflared；历史兼容证据可留但不可执行/打包
```

```text
品牌图标       = assets/icons/localbridge.ico
品牌 PNG       = assets/icons/localbridge.png
替换/重绘      = 未经明确合同禁止
```
