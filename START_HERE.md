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
Public path input = exec workdir / git / document / image 等 workspace-bound 参数必须解析在 active workspace 内；安全 relative 与普通 Win32 absolute 经 identity validation 指向同一 active workspace 对象时等价；workspace 外 absolute / UNC / verbatim public input / POSIX absolute / ADS-like / 越界 traversal / reparse escape typed deny
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
agent_workflow.path = active-workspace-bound nested-project selector；默认 .；安全 relative 与普通 Win32 absolute 若解析到同一 active workspace 内对象则等价；只改变 project context，不改变 active workspace authority
nested repo          = path=LocalBridge 或 LocalBridge/src 时与 git_workflow 使用一致 enclosing-repo 语义，最多向上到 active workspace
PowerShell baseline  = arbitrary module autoload 继续关闭；固定/身份验证的可信标准模块提供 Get-Location / Get-ChildItem / Test-Path 等基础 cmdlet
provider hardening   = New-Item / Set-Content / Set-Item / Alias / Function 等动态 provider surface 继续 review-required
workspace dir write  = agent_workflow.directory_changes[]；仅 create_directory / remove_empty_directory；必须 active-root-bound；安全 relative 与同根普通 Win32 absolute 等价；Edit/Full reviewed write；不依赖 process exec；workspace 外 absolute / UNC / verbatim / POSIX absolute / ADS-like / 越界 ../reparse escape deny
control-plane        = WorkspaceRegistry / active workspace mutation 仍永久 deny；nested-project/path 与目录写入均不能扩大授权根
current              = G2 / LB-006 REWORK_REQUIRED；LB-007 BLOCKED；generation18 未消费
```

Schema26 管理员模式安全确认是对未来 LB-015/LB-016 的合同修订，**不改变当前执行入口**：当前仍为 `G2 / LB-006`，必须继续严格顺序推进，不能因本合同提前跳到 G3。

## Schema37 — Contract contradiction cleanup（current）

- active-workspace-bound public path/workdir 输入以 canonical containment + validated filesystem identity 为授权真相；安全相对路径与普通 Win32 绝对路径若指向同一 active workspace 对象则等价允许，禁止再使用 `relative-only` 或“一切 absolute 均拒绝”的旧规则。
- Dashboard/主控界面显示且仅显示一个 backend PermissionMode 驱动的只读 `权限模式` 行；“不显示三种模式”仅指不显示三档切换控件，不是隐藏当前模式状态。权限编辑仍只在 Settings 与用户显式重新打开的 onboarding 第3屏。
- public privileged-route unavailable canonical code 固定为 `PrivilegedRouteUnavailable`；`PrivilegedRouteNotAvailable` 不再允许作为 public required-test 期望。
- 管理员模式入口固定橙色 `#ff9500`；Broker 未 Active 时必须先走固定风险警告 + backend monotonic 9000ms not-before + 用户 enabled `确认`，之后才可请求 UAC；frontend 倒计时不具授权权威。
- 本次 schema37 只清理当前合同矛盾，不自动推进、回退或重开 `PR_INDEX.json` / `PROJECT_STATE.json` 中的 PR/Gate 状态；现有 G3 human review 要求保持不变。


## Schema38 — Public control/runtime audit repair（current）

- `task_control cancel` 必须在请求已异步化为 public command session 后仍可通过 current task owner 找到同一 session，并调用与 `command_control kill` 相同的 public-session terminator；terminal truth/finalizer 继续唯一归属 task-state。
- `git_workflow show/diff` 的正文 patch 与 `files[]` metadata 分离生成；metadata 必须来自 Git NUL-delimited machine output（`--name-status -z`/`--numstat -z`），不得反向解析 human patch，Unicode/quoted path 必须保持精确状态。
- public `facade_revision` 升为 38。`command_control` 与 `elevated_exec` 的 public inputSchema 必须是客户端可直接展开的顶层 `type: object + properties`；不得用顶层 `oneOf` 作为可发现性前提。action/operation-specific 合法组合仍由服务端/PEP/Broker 严格校验。
- `document_workflow rebuild` 对外明确要求“目标 path 已存在 + content 必填”；不存在目标仍为 NotFound，缺 content 仍为 InvalidArgument。
- Windows command timeout 的 TERM 不得映射为 CTRL_BREAK；300ms timeout 的真实 bundled-runtime 回归必须在 1800ms 内收敛为 ProcessTimedOut，必要时由短 graceful window 升级为 forced process-tree kill，且不得出现 PowerShell `Entering debug mode`。
- Full/cmd 下普通 workspace 临时目录 `rmdir /s /q` 不得仅因 `rmdir` 同时是 PowerShell alias 被判 privileged；PowerShell selector 下该 alias 仍按 provider/dynamic surface 保持 review-required。
- schema38 不扩展 system-management executable 集；schema37 当前六个静态目标继续生效。`pnputil/wevtutil/powercfg` 的 query/mutation operation-level 分类留作后续独立合同，不作为本轮违规修复。
- 本轮最早责任 PR 为 LB-006，policy classifier 补充归 LB-007；修复后必须重验 LB-006→LB-012、fresh G2 generation26，再重验 LB-013→LB-017、fresh G3 generation16。G3 结束仍停在 human review REQUIRED，G4 BLOCKED。

## Schema39 — Agent execution platform maturity contract

- 生命周期统一为 `Workflow → Task → optional Execution`。只有进程型 Task 才拥有 `public Session → process tree`；Git/document/image/纯结构化文件等非进程 Task 不得为了形式统一伪造 Session。`task_control` 继续严格只有 `get/cancel`，AI 只取消 Task，不负责判断底层 session/process。
- Public API 以“真实 MCP 客户端能直接消费”为验收标准，而不是只验证后端 JSON Schema 理论正确。顶层字段必须可发现，action/operation-specific 组合继续由服务端严格校验；不得要求模型先故意触发 `InvalidArgument` 才学习调用格式。
- 保留并扩展现有 LocalBridge typed-error taxonomy，不重命名已冻结 canonical code；同一失败条件经过 `exec_command`、`agent_workflow` 或其他 facade 间接路径时必须归一到同一 public error code。
- `agent_workflow` 是编排层，不是第二套 Shell/File/Git runtime：必须复用同一 filesystem/Git/process/document/image/privilege service、Session Manager、terminal finalizer、path authority 与 capability classifier。
- `workspace_context` 成为 compact first-turn discovery：在可确定时一次返回 project name/type/version、Git branch/dirty/changed count、package manager、build/test system、runtime availability、trusted shells、permission mode、current task；使用缓存/已知 discovery snapshot，禁止每次调用都重复拉起探测进程。
- public process/session 状态收敛为 `running/completed/failed/cancelled/timed_out/lost`；所有非 running 状态都是 durable terminal truth，且不依赖客户端持续 poll。恢复能力只要求 durable workflow resume + retained output continuation；v0.1 不要求 generic pause/history/snapshot/rollback。
- public surface 继续严格 8 个非特权 core + `elevated_exec` 特权扩展；本轮明确**不新增 `file_workflow`**。若未来真实黑盒证明结构化文件能力不足，必须另立合同后再扩展。
- schema39 不建立第二套 G1–G5 成熟度体系；所有成熟度验收映射回现有 LB/Group/Gate。最早责任 PR 仍为 LB-006，policy/path classifier 交叉项归 LB-007；schema38 generation26/16 只保留历史证据，新合同需要 fresh G2 generation27 / G3 generation17。合同修订本身不自动修改 `PR_INDEX.json` / `PROJECT_STATE.json` 或替人类通过 G3→G4 Gate。

## Schema40 — Live coding runtime health truth（current）

- `workspace_context.runtime` 是 backend-owned live health truth，禁止 facade 初始化后永久 hard-code `ready`。项目/构建发现可以缓存，但 runtime health 不能随 project discovery snapshot 一起缓存为 Ready。
- `shell_discovery` 只回答可信 shell 是否存在；`cmd.available=true` / `cmd.trusted=true` 不代表 private coding MCP 当前可调用。Shell trust、supervised root-process liveness、authenticated MCP transport/protocol health 是三个独立事实。
- runtime 只有在 bounded authenticated MCP health 成功时才可投影 `ready`；root process alive 本身不够。若 root process 仍 alive 但 MCP HTTP/protocol 已不可用，必须立即离开 Ready，投影 `recovering` 或 `fault`。
- recoverable private MCP transport-health failure（例如 connection unavailable / health timeout）不能只作为一次 `RuntimeUnavailable` 返回给调用者；必须反馈给 backend runtime/recovery owner，进入既有 typed fault + 五次 bounded minimal-layer recovery。协议/能力不兼容仍 fail-closed，不得伪装成健康。
- recovery 验收必须真实覆盖“进程仍 alive、MCP 不响应”的故障：先证明 `workspace_context`/Dashboard 不再假 Ready，再证明自动恢复后 `exec_command(shell="cmd", command="echo TEST_PLUGIN_OK")` 无需人工重启即可成功。
- Dashboard/onboarding 编码服务状态必须与同一 backend runtime truth 同源；MCP health 已 recovering/fault 时不得仅因进程仍存活继续显示绿色 Ready。
- 本修订责任分层：LB-006 负责 live health truth / public projection / transport fault feedback，LB-010 负责 process-alive-but-unresponsive recovery，LB-015 负责 UI truthful projection。现有 G2 generation27 / G3 generation17 只保留 schema39 历史证据，不能验收 schema40；修复后需 fresh G2 generation28，再 fresh G3 generation18。合同修订本身不改 `PR_INDEX.json` / `PROJECT_STATE.json`，不自动推进或通过任何 Gate。

## Schema41 — Coding Agent semantic compatibility（current）

- LocalBridge 的目标是实现 `coding-agent-v1` **兼容语义**，不是复制 coding-tools public API。继续严格 8 个 non-privileged core tools + `elevated_exec`；不新增 `file_workflow`，不建立第二套 Gate，也不在 LocalBridge 内嵌 LLM。代码诊断/修改方案的推理由 ChatGPT/Codex/Claude 等 host model 负责，LocalBridge 负责确定性的发现、检索、执行、验证、持久化、恢复和权限边界。
- `agent_workflow` 是 Coding Agent 主入口。`diagnose / bugfix / feature / refactor / test_failure / build_release / document` 必须支持 objective-driven non-mutating prepare：自动发现项目规则和相关上下文，不要求模型预先提供 patch/commands；同一 logical Workflow 可继续进入 model-directed edit → automatic verify → persist，并保持一个 durable Task identity。`resume` 继续只执行缺失且可安全判定的步骤。
- durable Task checkpoint 版本化并至少保存 `objective/current_step/next_step/files_read/modified_files/commands/test_results/build_results/failure/output_refs/git_before/git_after`。`files_read` 只存 path/range/content identity 等 bounded metadata，不保存整份源码；checkpoint 必须跨 ChatGPT 重连、MCP 重启和 LocalBridge 重启恢复，且不得持久化 secret-bearing stdin/env/plaintext credential。
- `workspace_context` 默认 `compact`，并支持 bounded `full`；在可确定时包含 project name/type/version、`git_root/branch/dirty/changed_count`、package/build/test system、`important_files`、适用 `instructions`、runtime/trusted shells/permission/current task。full 仍是结构化 metadata，不允许一次 bulk dump README/AGENTS/manifests/source bodies。
- 内部共享 `ContextService` 至少提供 `discover_instructions / search_text / select_related_files / read_relevant_ranges`，由 `agent_workflow` 使用，减少模型反复调用 rg/findstr/type/Get-Content；v0.1 不强制完整 semantic symbol/reference engine，除非未来另行实现和验收。
- 原子编辑服务至少具备 read range、exact replace、apply patch、create/delete/rename file、mkdir、search/replace，并使用 expected content hash/version 或等价 content identity 做乐观并发控制。独立变更返回 `FileChanged`，patch 上下文失配返回 `PatchConflict`，非唯一匹配返回 `AmbiguousMatch`，目标不存在继续使用 canonical `NotFound`；所有写入仍受现有 PathAuthority/policy 和原子写语义约束。
- `VerificationPlanner` 必须 deterministic：项目 instructions/显式 Gate 最高优先，其次 changed-file targeted tests → lint/typecheck → broader project gate → git diff checks；只有发现真实 manifest/script/rule evidence 才能生成 npm/cargo/pytest/dotnet/go 等命令，禁止猜测，并继续遵守现有 `pr_fast / pr_runtime / group_release` 分层。
- Coding result 统一公共 metadata envelope：`ok/state/summary/task_id/warnings/next_step/output_refs/data/error`；具体 command/Git/document/image/workflow 字段保留在 `data`，不得为了统一制造无意义空字段。默认响应保持 compact；大型 context/output/diff/verification log 通过 `output_refs` 或 bounded range continuation 获取。
- `coding-agent-v1` 机器验收语义固定为 Workspace discovery、Project instructions、Context search、Command execution、Persistent task、Resume、Patch/edit、Test/build、Git status/diff、Cancellation、Output continuation、Typed errors。兼容判断以这些行为是否真实可用为准，不以工具名称或 schema 外观是否模仿 coding-tools 为准。
- schema41 最早责任 PR 仍为 LB-006。schema40 的 generation28/18 目标保留历史含义，但不能验收新增 schema41；完成后需 fresh G2 generation29 / G3 generation19。合同修订本身不改 `PR_INDEX.json` / `PROJECT_STATE.json`，不自动推进任何 Gate。

```text
品牌图标       = assets/icons/localbridge.ico
品牌 PNG       = assets/icons/localbridge.png
替换/重绘      = 未经明确合同禁止
```

## Schema42 — Task/Command truth convergence + system-management classification（current）
- Backend derives one TaskAggregate projection from existing WorkflowCheckpoint + CommandTaskStateStore; no third persisted task database.
- current_workflow is active-only (running/waiting); current_command is active-only (running/waiting_input/cancelling); completed/failed/cancelled/timed_out/lost are history-only last_command truth.
- Overall idle is legal iff current_workflow and current_command are both absent; incomplete durable workflow must never project idle through task_control/workspace_context/UI/admission.
- task_control cancel ends the whole task once; command_control kill ends only the command and leaves an incomplete owner workflow explicitly waiting/resumable.
- Command summary is derived from structured status; document truncated reflects actual requested-range limiting, not EOF.
- pnputil/powercfg/wevtutil join Windows system-management classification; frozen read-only operations may be Full, mutation/unknown requires Elevated fail-closed.
- UI current status and last-command history are separate backend projections; idle text is 空闲. MCP/Tunnel transient network blips are not a current repair blocker and are deferred to the diagnostics/logging workstream where malformed HTTP and transport interruption must become distinguishable.
- Earliest owner LB-006; policy owner LB-007; UI owner LB-015; fresh G2 generation32 / G3 generation22 required. G4 remains blocked by human review.
- Schema42 Dashboard observability amendment: TaskAggregate observation truth also incorporates CurrentTaskProjection so every observable public action (workspace/context/workflow/command/control/Git/document/image/elevated) reaches one backend currentActivity projection; lastActivity selects the newest terminal activity across command and non-command completions. Frontend must not recompute activity priority or latest-history selection.
- Dashboard observation UI remains exactly two peer rows: current activity first, one latest result second. The second row prefix is exact `上次执行：`; no `结果：` label. The entire left result sentence is colored by terminal outcome (completed green, failed/lost red, cancelled neutral gray, timed_out amber), while the relative age stays far right.
- Optional safe command/activity summary uses one fixed maximum-width presentation token; only the summary segment may ellipsize. Action text, terminal suffix such as 成功/失败/已取消/已超时, and age must remain fully visible at 780x620. Unsafe/raw arguments are never shown.
- Existing workflow prepare/edit/verify/persist and verification progress may be projected as concise observation metadata without a new progress state machine. waiting_input/cancelling count as implemented only when backend production state can actually emit them.
- This amendment invalidates schema42 G2 generation32 evidence and requires fresh G2 generation33. G3 generation22 was not consumed and remains the next required G3 review; G4 remains blocked.
- Schema42 Windows execution/policy amendment: Full 的硬进程权限边界固定为当前 Windows 普通用户 Token；LocalBridge command classifier 只负责直接调用的路由与明显系统维护误操作保护，不声称能跨 `.cmd/.bat/.ps1/Python/npm` 等后代进程实现 transitive deny。若未来必须严格阻断普通用户 Token 本身可执行的后代系统操作，必须另立 restricted-token/AppContainer/等价 OS confinement 合同，不能继续靠扫描更多字符串实现。
- Windows system-management family 在现有 `reg/schtasks/sc/netsh/bcdedit/dism/pnputil/powercfg/wevtutil` 基础上追加 `net/net1/fsutil/mountvol/reagentc/manage-bde/fltmc/auditpol/vssadmin`；明确冻结的只读 query 可 Full，mutation 或 unknown operation 必须 Elevated/fail-closed。CMD 与 PowerShell 必须使用 shell-specific classifier；`set/copy/move/ren`、普通 `cmd /c`/`cmd /k`、普通 PowerShell 开发 cmdlet 与冻结只读 identity query 不得只因表面 token/alias/静态成员调用被判 privileged。
- Windows Shell fidelity 新增硬约束：`>nul` / `2>nul` 必须保持原生 NUL device 语义，不得创建 `nul.localbridge` 等磁盘文件；已知 UTF-8/OEM/ACP 的 native executable 输出必须按真实代码页解码为 Unicode，未知非 UTF-8 字节不得一律使用 lossy UTF-8 replacement 伪装成正确文本。
- Elevated 的 process/shell/filesystem 三条 route 必须共享同一 `PermissionMode=Elevated + Broker Active + UAC authorized` 真值并通过真实 MCP 客户端 parity；当前磁盘 filesystem route 不按“已断裂”处理，但 real-client 三路均必须验收。process/shell 结构化结果必须可分别观察 stdout/stderr，超过 inline limit 时提供真实可读 output_ref continuation。
- `view_image` 的 `OutputTruncated` 不得提示不存在的 output_ref；若不提供 continuation，remediation 必须明确为提高 `max_bytes` 或 resize。generic durable workflow 在执行 directory/patch/command side effect 前必须拥有有意义的 `current_step`，`next_step` 可确定时必须给出；context-only diagnose 固定为 non-durable `context_ready` terminal result，workflow/task id 为 null 且 summary 必须表达 context readiness，而不是声称 durable workflow completed。
- Tunnel/MCP 瞬时 HTTP 400 / connection failed 的根因归属继续保留未定；本轮 schema42 不以此阻塞修复。后续“诊断日志”工作必须把 malformed request、socket read failure、early EOF、unsupported transfer encoding 等 typed cause 分开记录，届时一起解决瞬断诊断与恢复证据问题。
- 本 amendment 的 generation 状态：G2 generation33 已消费并因 LB-012 retained-output 累计无界而 FAIL；当前 LB-012 rework 与 Toolbox amendment 完成后必须 fresh G2 generation34。G3 generation22 尚未消费；owner 仍为 LB-006 / LB-007 / LB-012，Tunnel 诊断延期项不新增当前 blocker。
- Schema42 Toolbox amendment：`aria2c 1.37.0`、逻辑 `7z`（7za 26.02 x64）与 `jq 1.8.2` 固定版本、来源及 SHA-256，仅在构建准备阶段下载/校验并随发布包携带；运行时禁止下载、安装或更新。`curl` 不打包，仅允许 `%SystemRoot%/System32/curl.exe`，启动时做 existence/capability probe；缺失返回 `RuntimeUnavailable`，能力不足返回 `CapabilityUnavailable`，不得静默下载替代品。
- Toolbox 不新增 MCP public tool，public surface 继续严格 8 core + `elevated_exec`；不持久修改系统/用户 PATH，由 LocalBridge `ToolboxResolver` 绑定真实 executable，且 `aria2c/7z/jq` 不得回退到 ambient PATH。同一执行入口继续继承 Edit/Full/Elevated 权限：Edit 无进程能力，Full 使用普通用户 Token，仅真实需要管理员目标/操作时走既有 elevated route。版本、来源、下载资产 SHA-256、发布 executable 路径与 executable SHA-256 必须进入 `runtime-manifest.toml` 与 `THIRD_PARTY_NOTICES.md`。
- Schema42 Unified Error Diagnostics amendment：保留现有细粒度 canonical `error.code`，所有 public failure 同时增加稳定 `error_code`（`InvalidRequest/Unavailable/Denied/Timeout/Cancelled/ExecutionFailed/Unknown`）、`phase`（`transport/mcp/runtime/policy/tool/process/unknown`）和可扩展 `cause`。所有失败必须经过一个共享 mapper，未知异常最终落 `Unknown`，不得逃逸为未映射错误。
- 请求诊断统一 `request_id/connection_id/attempt/error_code/phase/cause/http_status/duration_ms`；重试复用同一 `request_id` 并递增 `attempt`，成功重试仍与最初失败关联。记录必须由真实 MCP request / backend recovery 事件驱动，Dashboard/Diagnostics 读取不得创建或推进日志；recovery 直接复用既有 `OutageGeneration.request_id`。`connection_id` 表示逻辑连接/单次 retry attempt correlation；`timestamp` 为 Unix epoch milliseconds。工程字段只进入 bounded 脱敏日志/导出，不进入普通诊断 UI。
- Tunnel/MCP transport 错误属于 `phase=transport`；Tunnel HTTP 400 固定为 `Unavailable + transport + cause=http_400 + http_status=400`，除非本地 runtime 本身不可用，否则不得报告成 `RuntimeUnavailable`。MCP HTTP 层必须区分 malformed request、socket read failure、early EOF、unsupported transfer encoding 等 cause。
- 本 amendment 责任分层：LB-006 shared mapper/public MCP，LB-007 policy，LB-008 Tunnel transport，LB-010 retry correlation，LB-017 request log/export。generation34 保留历史 FAIL；从 LB-006 严格重验 G2，完成后 fresh G2 generation35。G3 generation22 尚未消费。
