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

## 当前文档权威边界

上述 2–9 共 **8 份编号文档**是当前 human design authority。`docs/**` 中未列入该 8 份清单的文件只可作为 supplemental / ADR / compatibility / governance / historical evidence 使用，**不得覆盖**这 8 份当前人类权威、机器合同或当前磁盘代码。若未列入清单的旧稿与当前权威冲突，执行/审查智能体必须忽略旧稿中的冲突语义，除非当前权威文件与机器合同显式将其重新提升为 authoritative。

`docs/LOCALBRIDGE_TOOL_WRAPPER_AND_SHELL_RESOLVER.md` 是 Agent Runtime、ShellResolver 与系统维护能力的**最终设计指导 / 未来能力基线**，但它不是第 9 份当前 human authority。只有其中被当前机器合同（例如 schema25/schema26）以及上述 8 份 numbered authority 显式提升/吸收的部分，才构成当前硬合同；其余未来 `system_inspect/system_manage` 等设计不得自行越过 PR 顺序、writable paths 或提前声称已实现。已废弃的 `docs/LOCALBRIDGE_FINAL_AGENT_RUNTIME_DESIGN.md` 不得被恢复为当前事实源。

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
管理员逻辑色   = onboarding 与设置页所有“管理员模式”控件统一橙色 #ff9500；不得被普通蓝色 selected 覆盖
管理员安全确认 = Broker 未 Active 时，点击/重新点击“管理员模式”先显示固定后果警告；红色“确认9→…→确认1”按钮在完整 9000ms 内 disabled，倒计时结束后仍为红色并显示“确认”才可点击；确认后才允许安全校验→Windows UAC；取消/Esc/关闭无 PermissionMode/Broker/UAC 副作用；每次重新打开重新计时；后台恢复无警告/无 UAC；Broker 已 Active 时重选不得重复 UAC
服务状态点     = Ready绿 / Starting琥珀 / Fault红 / Unknown灰；onboarding 与 Dashboard 使用同源状态
窗口           = 固定 780×620；minimum=maximum=780×620；禁止缩放与最大化
窗口验收       = `resizable=false`、`maximizable=false`；拖拽边框/最大化均不能改变 client size
窗口边框       = `decorations=false`；唯一自定义 chrome 必须贴满 client area；禁止原生+自定义双边框；保留拖拽/最小化/关闭
引导页布局     = 整页；直接使用 custom chrome 内容区；禁止空白页面中居中再套 card/modal/dialog 式向导外壳；管理员安全确认弹窗仅作为局部安全 consent dialog 例外，不得成为向导外壳
主页权限       = 显示且仅显示一个只读“权限模式”行，值直接来自 backend PermissionMode（编辑模式/完整模式/管理员模式）；无三档选择控件，不得从主页修改 PermissionMode 或触发模式 UAC；实际管理员/Broker 运行状态与模式投影分离；权限编辑允许设置页与用户显式重新打开的 onboarding 第3屏
前台启动       = UI-first：先创建/显示并达到可交互 UI → 单次 typed UI-ready → backend 再异步启动 runtime/MCP/Tunnel；UI-ready 前禁止启动原本停止的服务；--background 例外
任务状态       = backend 唤醒式绑定真实工具调用；轮询不得作为短任务传输；每次工具调用至少可见500ms且不延迟真实返回；首行当前状态/等待命令，第二行“上次执行工具：…”且 nS前/n分钟前/大于1小时/大于n天 靠右；无历史列表
临时提示       = “无法准备管理员权限”等一次性操作提示默认 3 秒自动消失；持续 runtime fault 仍由 typed 状态表达
项目路径边界   = \\?\D:\project 仅限内部 filesystem identity 校验；UI 与 MCP/Broker/sidecar/process/command/tool 的 cwd/workdir/current_dir/路径参数必须使用同一 validated identity 对应的普通 D:\project；verbatim 工作目录不得传给命令工具，execution/display 转换不得参与授权
按钮对齐       = 同级动作使用同一左基线/动作列；780×620 几何差≤1 CSS px；设置两项“更换”为强制样例，Key“清除”紧邻位于 Key“更换”左侧
服务控制       = Dashboard 提供黄色/琥珀“重启服务”与红色“关闭服务”；backend 真实 lifecycle，统一按钮设计与对齐；重启不自动 UAC
设置           = 常规/连接/权限三组；连接字段固定英文 `Tunnel ID` / `Runtime API Key`；Key 已保存时“清除”紧邻“更换”左侧并真实删除 Windows 安全凭据；保存/清除均按需受控重连；无“测试连接”
诊断           = 运行状态/项目/最近脱敏日志；只保留“打开日志 / 导出诊断 / 完成”
关闭窗口       = 设置项“关闭窗口后继续运行”；开=hide+后台继续，关=有序退出
UI/backend     = frontend 纯投影；耗时 lifecycle/process/credential/UAC/recovery 工作必须离开 UI/WebView 线程
权限按钮高度   = 780×620 下两行按钮真实 rendered 高度≥单行控件2倍且两行均完整可见；min-height≥80px 仅作最低保护；2026-08-14 用户人工视觉审核已 PASS，后续若该布局变化须复验
OpenAI已保存项 = Tunnel ID 显示当前持久化值；Key 固定提示“已安全保存至windows安全凭据”，聚焦时按已存key位数显示同位数*掩码且不回传明文；安全提示仅“Runtime API Key 仅保存在 Windows 安全凭据中。”
滚动圆角       = sheet/dialog/card 出现纵向滚动条时仍保留四角圆角，scrollbar 必须裁切或内缩；顶部/底部箭头/三角按钮禁止显示，滚轮/轨道/滑块仍可用
Cloudflare     = LB-018 从最终 bundle/manifest/installer/启动参数/fallback 移除 cloudflared；历史兼容证据可留但不可执行/打包
```

管理员模式警告固定文案（schema26）：

```text
启用管理员权限后，错误或恶意操作可能导致：

* 删除或覆盖重要文件
* 修改系统关键配置
* 软件或系统无法正常启动
* 数据永久丢失
* 安全机制被绕过或关闭
* 凭据、密钥等敏感信息泄露
* 恶意程序获得更高权限
* 系统被破坏，严重时可能需要重装 Windows

仅在你明确理解操作后果时授权。

[取消] [确认9]
```

确认按钮**整个按钮为红色**，不是只有数字为红色。`确认9 → 确认8 → … → 确认1` 的完整 9 秒内必须 disabled；达到 9000ms 后同一红色按钮变为 enabled 且标签精确为 `确认`。前端显示计时不能成为授权真相源；backend/等价可信单调计时必须拒绝提前确认。只有 enabled `确认` 的显式用户动作才能进入既有安全校验并随后请求 UAC。

Agent Runtime / ShellResolver 冻结补充（schema25）：

```text
Public Agent API = LocalBridge 自有、版本化；禁止直接转发 upstream tools/list/schema/error
v1 core registry = workspace_context / agent_workflow / exec_command / command_control / task_control / git_workflow / document_workflow / view_image
实际 tools/list  = 当前 policy 允许的 core registry 子集 + 当前 policy 允许的 LocalBridge 特权扩展
特权扩展         = elevated_exec；仅既有 Elevated + Broker policy 条件式开放，不属于普通 core
安全路由         = LocalBridge Tool Registry → stable capability/action → PEP → adapter/executor
Runtime adapter  = coding-tools-mcp 为内部可替换 backend；启动必须做 mandatory capability negotiation，缺失/不兼容 fail-closed
Public session    = LocalBridge-owned opaque session/output handles；禁止 raw upstream session/output handle；poll/read/write/kill 必须闭环且 terminal lifecycle 后台收敛
workspace_context = active workspace 时返回非空 ordinary absolute workspace + workspace-relative default_cwd；不得静默 workspace=""
Public path input = exec workdir / git / document / image 等 workspace-bound 参数统一 active-workspace-relative；absolute/traversal typed deny
Process outcome   = exit_code=0 才是普通 success；任何非零 exit（包括无 stdout/stderr）= ProcessFailed / Failed task
Facade completeness = public schema 已广告 action 必须真实可执行；禁止“先暴露、调用时 unavailable”
Git nested repo   = 五个 git_workflow action 共用 workspace-bounded repo resolver；已发现 repo 时 diff 禁止 non-git fallback
Shell selector   = auto / powershell / pwsh / windows_powershell / cmd；禁止 command-text guessing、任意 executable selector、自动安装/更新
auto shell       = 在已验证可信候选中按 semantic version 选最高兼容 PowerShell Core → Windows PowerShell 5.1 → cmd
PATH             = 仅发现线索，不是信任依据；version probe/执行前先验证可信安装位置或显式注册 executable identity
执行模型         = DirectProcessExecutor 与 ShellExecutor 分离，均使用 structured spec
v0.1 非目标      = WSL/container/remote shell、custom shell registry、environment manager、固定内部目录布局
当前返工入口     = G2 / LB-006；G2 adversarial generation 9 已 FAIL；LB-006→LB-012 严格重验，之后 fresh G2 adversarial generation 10；旧 G2 gen8/G3 gen6 仅保留历史证据
```

Schema28 测试编排与 Public Runtime 语义补充：

```text
PR Fast Gate     = 当前 PR 的廉价 deterministic unit/fake/static；开发内循环优先 targeted tests
PR Runtime Gate  = 真实 bundled runtime/PEP/process 重测试；同一拓扑按类压缩为共享 lifecycle，除非 isolation 本身就是被测行为
Group/Release    = 完整 cargo/npm/build/clippy/architecture/release Gate；不得每个小修改后反复全跑
静态合同测试     = 只证明固定文案/schema/allowlist/forbidden dependency/governance 等静态事实；不得替代真实行为执行，也不得仅因函数/测试名存在就宣告行为 PASS
Session E2E      = 一个真实 lifecycle 覆盖 exec → incremental poll → write → read → kill → terminal convergence；bounded timeout/cancel；lost 必须终态失败
开发命令窗口     = 允许出现；其存在本身不判产品失败
打包 GUI         = LocalBridge 自己启动的 runtime/Tunnel/Broker/后台 helper/shell/direct command 不得意外弹可见 console；只以 release-style/packaged launcher 实测判定
Shell quoting    = 只允许一次预期 shell parse；双引号中的 | / & 不得被 LocalBridge 额外重解析
poll             = live incremental delta；不丢中间输出、不重放旧 delta
write            = running public session 在 exec 返回后仍可写入 stdin
kill             = healthy runtime 下不得 RuntimeUnavailable；成功后稳定收敛 cancelled，后续 poll 不退化 SessionUnavailable
view_image       = auto_resize 必须真实缩放并保持比例
PowerShell output= UTF-8；中文输出不得乱码
Git blame range  = 1-based inclusive；5..5 只返回第5行；start>end = InvalidArgument
document range   = 1-based inclusive；start>end = InvalidArgument
result probe     = adapter 消费且 upstream outputSchema 未保证的 private result 字段全部在 serving 前做确定性语义兼容验证或等价 fail-closed 证明
当前返工入口     = G2 / LB-006；schema27 acceptance 仅保留历史 provenance；generation 10 未消费
```

Schema29 command task-state durability / window placement 补充：

```text
terminal finalizer = 每个 terminal 路径都必须进入 finally/finally-equivalent；一个 owner transaction 内原子追加且仅追加一次 command_finished + 清空 current_command
terminal snapshot  = task-state 自己持久化 bounded/redacted terminal truth；private session 约300秒 retention 只用于临时输出读取，不能决定最终结果
owner CAS          = command state 更新比较 (task_id, session_id)；旧 task/旧 session 不得覆盖或清除当前 owner；duplicate finalization 幂等
窗口首次显示       = 正常前台新建 780×620 主窗口默认在当前 monitor work area 居中；托盘重新显示已存在窗口保留用户位置
G2 owner           = 前三项归 LB-006；gen16 在 schema29 前完成，必须 fresh G2 adversarial review
G3 owner           = 居中项归 LB-015，不得提前混入 G2 产品修改
```

Schema30 nested-project / PowerShell / workspace-write 补充：

```text
agent_workflow.path = active-workspace-relative nested-project selector；默认 .；只改变 project context，不改变 active workspace authority
nested repo          = path=LocalBridge 或 LocalBridge/src 时与 git_workflow 使用一致 enclosing-repo 语义，最多向上到 active workspace
PowerShell baseline  = arbitrary module autoload 继续关闭；固定/身份验证的可信标准模块提供 Get-Location / Get-ChildItem / Test-Path 等基础 cmdlet
provider hardening   = New-Item / Set-Content / Set-Item / Alias / Function 等动态 provider surface 继续 review-required
workspace dir write  = agent_workflow.directory_changes[]；仅 create_directory / remove_empty_directory；active-root-relative；Edit/Full reviewed write；不依赖 process exec；absolute/../reparse escape deny
control-plane        = WorkspaceRegistry / active workspace mutation 仍永久 deny；nested-project/path 与目录写入均不能扩大授权根
current              = G2 / LB-006 REWORK_REQUIRED；LB-007 BLOCKED；generation18 未消费
```

Schema26 管理员模式安全确认是对未来 LB-015/LB-016 的合同修订，**不改变当前执行入口**：当前仍为 `G2 / LB-006`，必须继续严格顺序推进，不能因本合同提前跳到 G3。

```text
品牌图标       = assets/icons/localbridge.ico
品牌 PNG       = assets/icons/localbridge.png
替换/重绘      = 未经明确合同禁止
```
