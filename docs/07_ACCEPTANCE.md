# 07 — Acceptance

| ID | 场景 | 预期 |
|---|---|---|
| A01 | 首次启动 | 进入 Wizard，不进入 Dashboard |
| A02 | Screen 2 | 字段标签严格为 `Tunnel ID` / `Runtime API Key` |
| A03 | Screen 4 Local Bridge 按钮 | 系统默认浏览器，不创建 WebView |
| A04 | ChatGPT URL | 只能打开 `https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins` |
| A05 | 选择 workspace | 不自动授权父目录/磁盘根 |
| A06 | Edit tools/list | 不出现 process-exec capability |
| A07 | Edit tools/call 绕过 list | 仍被拒绝 |
| A08 | Edit unknown tool/capability | 默认拒绝 |
| A09 | Edit workflow 间接 exec | 被拒绝 |
| A10 | Full | 只允许 reviewed coding capability |
| A11 | Full control-plane | 永远拒绝 |
| A12 | Full UX | 明示 Windows 用户 OS 权限风险 |
| A13 | mode switch 后客户端缓存旧 tools/list | tools/call 仍按新 policy 强制 |
| A14 | MCP listener | loopback only |
| A15 | PEP listener（若存在） | loopback only |
| A16 | Runtime Key | 不存在 settings/json/log/CLI args |
| A17 | 错误 Runtime Key | typed auth fault，无 restart storm |
| A18 | kill MCP | supervisor backoff 恢复 |
| A19 | kill Tunnel | supervisor backoff 恢复 |
| A20 | workspace 删除 | `WorkspaceMissing`，不无限拉起 |
| A21 | checksum mismatch | sidecar 不启动 |
| A22 | 点击窗口 X | 窗口隐藏，runtime 不停止 |
| A23 | Tray 打开 | 恢复/聚焦同一实例 |
| A24 | Tray 退出 | 所有 owned sidecar 被清理 |
| A25 | 开机启动 | 无可见主窗口 |
| A26 | 开机恢复 | autoStartServices 时恢复到 Ready |
| A27 | manual stop | 重启应用不擅自恢复服务 |
| A28 | switch workspace | 旧 Tunnel→PEP→MCP 顺序停止 |
| A29 | switch workspace | 新 runtime Ready 后才 commit active |
| A30 | candidate 启动失败 | active 不被提前覆盖，可 rollback |
| A31 | Tunnel 配置变化 | 不复用旧 generation |
| A32 | sidecar crash log | secret redacted |
| A33 | clean Windows x64 | 不要求预装 Python |
| A34 | install/uninstall | 不遗留 owned sidecar |
| A35 | upstream upgrade | compatibility suite 必须重跑 |
| A36 | junction escape | 无越权文件访问 |
| A37 | symlink escape | 无越权文件访问 |
| A38 | reparse-point escape | 无越权文件访问 |
| A39 | path case/canonicalization | 不产生 policy bypass |
| A40 | Job Object close | owned child tree 按 ADR 行为清理 |
| A41 | PID reuse / stale PID | 不误杀非 owned process |
| A42 | nested build child | ownership 行为与 ADR 一致 |
| A43 | app crash | owned runtime 不留下未定义 orphan |
| A44 | dummy sidecar package smoke | LB-001 release bundle 可启动 sidecar |
| A45 | portable Python | coding-tools-mcp 在最终 embedded runtime 模型运行 |
| A46 | deterministic CI | 不需要真实 OpenAI key/tunnel |
| A47 | live external test | 仅 LB-000/LB-019 |
| A48 | bad Tunnel ID | non-retryable typed fault |
| A49 | bad policy | non-retryable typed fault |
| A50 | PEP unknown upstream tool | fail-closed |
| A51 | manual app launch while background instance exists | 唤醒旧实例，不启动第二套 runtime |
| A52 | diagnostics auto-repair | 不修改 credential/permission/workspace policy |
| A53 | LocalBridge normal launch | 主进程不是 Administrator |
| A54 | Full mode | MCP/Tunnel/commands 使用当前普通用户 token |
| A55 | Elevated enable | 只有 Broker 在完成用户安全确认后触发 UAC |
| A56 | Elevated active | 主 LocalBridge/Tunnel/MCP 不因 Broker 提权 |
| A57 | Elevated | 无 15/30/60 分钟或 expires_at；schema26 的 9 秒只是每次激活前的确认等待期 |
| A58 | Elevated disable | privileged call gate 立即关闭，Broker 停止 |
| A59 | Broker inactive + privileged call | typed `ElevationRequired` / 用户授权所需状态 |
| A60 | reboot/background startup + Elevated preference | 不显示管理员安全确认，不自动弹 UAC |
| A61 | user opens UI after reboot | 可显式重新进入完整管理员安全确认流程后激活 Broker |
| A62 | low-integrity/unauthorized local IPC | Broker 拒绝 |
| A63 | stale broker nonce/generation | 拒绝 |
| A64 | replayed privileged request | 拒绝 |
| A65 | Broker listener | 无网络 listener |
| A66 | Broker crash | `PrivilegeState` 不再 Active |
| A67 | Tray exit | Elevated Broker 不遗留 |
| A68 | Full control-plane | 拒绝 |
| A69 | Elevated control-plane | 仍拒绝 |
| A70 | Docker/WSL/Podman capability | 标记 `privileged-external-runtime` 并进入 review |
| A71 | clean Windows 11 x64 无 Python | LocalBridge 正常运行 |
| A72 | 系统 PATH 含其他 Python | LocalBridge 仍只使用 bundled Python |
| A73 | bundled Python 缺失 | typed RuntimeMissing，不 fallback |
| A74 | bundled Python checksum 错 | RuntimeChecksumMismatch |
| A75 | 首次启动离线 | 不执行 pip install / runtime download |
| A76 | installer 内容 | 包含 Python/coding-tools/tunnel-client |
| A77 | installer 内容 | 不包含 WebView2 Offline/Fixed Runtime |
| A78 | runtime 上游出现新版 | 当前安装不自动升级 |
| A79 | LocalBridge v0.1 | 不存在 auto-updater |
| A80 | idle/normal usage | 无 telemetry/analytics/crash-upload 网络请求 |
| A81 | diagnostics export | 仅用户主动触发 |
| A82 | elevated_exec Edit | deny |
| A83 | elevated_exec Full | deny |
| A84 | elevated_exec Elevated+Broker Active | reviewed policy 后执行 |
| A85 | elevated_exec internal request | structured program/args，不以 shell string 为 canonical |
| A86 | elevated_exec | timeout/cancellation/output limit 生效 |
| A87 | installer >100 MiB | 必须生成体积归因 |
| A88 | installed >250 MiB | 必须生成体积归因 |
| A89 | Dashboard 任意 PermissionMode | 不显示“权限模式”行，不显示编辑/完整/管理员三档选择控件；只读管理员权限状态仍可见 |
| A90 | Dashboard 权限交互 | 不能修改 PermissionMode，不能通过权限模式控件触发 UAC；权限编辑允许设置页“权限”或用户显式重新打开的 onboarding 第3屏 |
| A91 | Dashboard + PrivilegeState::Requested | 只读显示“管理员权限：等待授权”，无模式选择器、无独立启用按钮 |
| A92 | Dashboard + PrivilegeState::AwaitingUac | 只读显示“管理员权限：等待系统授权” |
| A93 | Dashboard + PrivilegeState::Active | 只读显示“管理员权限：已启用”；切换权限模式使用合同允许的权限配置入口，Dashboard 本身不提供切换 |
| A94 | Broker Active→Faulted | Dashboard 立即显示“故障” |
| A95 | Dashboard privilege status | 由 PrivilegeState 驱动，不由 PermissionMode 猜测 |
| A96 | Dashboard | 不显示 PID/nonce/SID/IPC 等内部字段 |
| A97 | 主控界面 | 不出现 Dashboard/Settings/Diagnostics 等普通英文 |
| A98 | 设置/首次引导权限模式 | 仅设置页与 onboarding 第3屏显示“编辑模式 / 完整模式 / 管理员模式”；主控界面不得显示 |
| A99 | 状态 | 不直接显示 Ready/Active/Faulted/Requested 等内部英文 |
| A100 | 专业缩写 | MCP/API/UAC/OpenAI/ChatGPT 可按术语策略保留 |
| A101 | 错误提示 | 中文、简短、可行动，不暴露内部 fault enum |
| A102 | React UI | 不直接渲染 domain enum 字符串 |
| A103 | 主控界面 | 不做中英双语标签堆叠 |
| A104 | architecture verifier | 可检测已知违规 fixture |
| A105 | frontend/runtime boundary | 前端无法拥有 sidecar 生命周期 |
| A106 | system Python fallback | 架构检查阻止 |
| A107 | listener 0.0.0.0 | 架构检查阻止 |
| A108 | direct enum UI render | 架构/文本检查阻止 |
| A109 | upstream upgrade | 必须生成 compatibility diff |
| A110 | new upstream capability | 默认 deny |
| A111 | settings schema change | 必须有 migration |
| A112 | failed migration | 原始数据保持不变 |
| A113 | future settings schema | fail-safe，不覆盖 |
| A114 | stable release | 生成 SBOM |
| A115 | stable release | THIRD_PARTY_NOTICES 与 runtime 一致 |
| A116 | stable release | artifact 可追溯到 exact source/runtime |
| A117 | runtime adapter | domain 不直接依赖上游私有结构 |
| A118 | release rollback | migration/install failure 不破坏旧配置 |
| A119 | 主控界面无活动任务 | 第一行固定显示“等待命令”，不得在该行追加相对时间；无活动历史 |
| A120 | LocalBridge 稳定读取类 public action/tool | 类型显示“读取文件”，任务显示安全路径摘要；不得依赖 raw upstream tool id 作为 UI/安全身份 |
| A121 | LocalBridge 稳定搜索类 public action/tool | 类型显示“搜索代码”，显示安全搜索摘要；不得直接暴露 upstream private tool name |
| A122 | `exec_command` / `agent_workflow` 等稳定执行类 public action | 类型显示“执行命令/运行测试/构建”等稳定分类，能力判定来自 LocalBridge action/capability contract |
| A123 | policy deny | 当前任务显示“已阻止”，不得先显示“执行中” |
| A124 | 管理员调用等待 UAC | 当前任务显示“管理员操作 / 等待授权” |
| A125 | task terminal | 最终回到第一行“等待命令”，并保留唯一上一工具安全标签/摘要与完成时间元数据供第二行展示，不追加历史消息 |
| A126 | 主控界面 | 无最近活动、消息流、时间线 |
| A127 | raw tool id | 不直接显示 MCP tool identifier |
| A128 | secret-bearing args | 任务摘要不泄漏密钥/token/nonce |
| A129 | model prose only | 未发生 MCP/Broker 调用时不得伪造当前任务 |
| A130 | 运行测试中 | 单行显示“● 运行测试 cargo test” |
| A131 | 当前执行状态 | 无“当前任务/类型/任务/状态”标题 |
| A132 | Running | 不额外显示“执行中”文字 |
| A133 | Running | 绿色活动点使用轻量脉冲动效 |
| A134 | reduced-motion | 活动点静态，不执行脉冲 |
| A135 | 动效 | 不推动布局、不造成文字位移 |
| A136 | Idle | 第一行必须显示低存在感“○ 等待命令”，不得显示“空闲”、隐藏整行或在第一行追加年龄；上一工具年龄属于第二行 |
| A137 | 保存 Runtime API Key | settings/JSON/TOML 中不存在明文 |
| A138 | 保存 Runtime API Key | Windows secure credential backend 可恢复 |
| A139 | 启动安全隧道 | process command line 不包含 Runtime API Key |
| A140 | 日志/诊断 | 不包含 Runtime API Key/Authorization 明文 |
| A141 | UI 保存密钥后 | 只显示“已保存”，不能重新显示完整密钥 |
| A142 | 项目列表 3 项 | 只有 active 项目是 MCP 授权根 |
| A143 | 新增已有真实目录别名 | 按 validated identity 去重 |
| A144 | 选择已有项目 | 使用 candidate → Ready → commit |
| A145 | 移除非当前项目 | 只删除 LocalBridge metadata |
| A146 | 移除任意项目 | 磁盘目录和文件完全不变 |
| A147 | 移除当前项目 | 停止 Tunnel → PEP → MCP |
| A148 | 移除当前项目 | 进入 NoActiveWorkspace，不视为故障 |
| A149 | 移除当前项目后 | 不自动授权其他已保存项目 |
| A150 | NoActiveWorkspace | 可以选择新项目，runtime 之前保持停止 |
| A151 | MCP tool call | 不能新增/选择/移除项目 |
| A152 | 当前项目启动时不存在 | 不静默切换其他项目 |
| A153 | recoverable disconnect | 自动重连，无 toast/banner/重试计数 UI |
| A154 | retry attempt 1..5 | backoff 为 1/2/5/10/30s |
| A155 | reconnect succeeds before 5 | Ready，用户无额外成功提示 |
| A156 | retry 5 fails | 停止自动循环，进入 Faulted |
| A157 | retry 5 fails | 只显示一次极简错误窗口 |
| A158 | error window | 文案“连接失败 / 已自动重试 5 次” |
| A159 | error window retry | 开启新的 5 次 generation |
| A160 | same outage | 不重复弹错误窗口 |
| A161 | auto recovery | 只重启最小必要 runtime 层 |
| A162 | config/credential/integrity fault | 不执行自动重连 |
| A163 | UI dependency graph | 不因 Apple-inspired 视觉新增 UI/动画/图标/CSS/字体依赖 |
| A164 | visual system | 使用原生 CSS + system font stack |
| A165 | reduced motion | 动效尊重 prefers-reduced-motion |
| A166 | G0 last PR PASS | G1 仍 BLOCKED，直到 G0 对抗审查 PASS |
| A167 | group review PASS | 只解锁下一组首 PR |
| A168 | group review FAIL | 下一组仍 BLOCKED，按 reopen_from_pr 回滚执行 |
| A169 | review governance write | 只能改 PR_INDEX/PROJECT_STATE 状态字段 |
| A170 | execution | 组内仍严格按 LB 编号顺序 |
| A171 | 首次启动 | 总屏数严格为 5，不存在第 6 屏；顺序为欢迎 → OpenAI → 项目与权限 → 创建自定义插件 → 启动检查 |
| A172 | 1/5 | 标题为“简单设置 即可开始” |
| A173 | 1/5 | 说明为“LocalBridge是链接ChatGPT与本地代码的工具” |
| A174 | 1/5 | 只有一个“开始”主按钮；第 1 屏是唯一无需返回的屏幕 |
| A175 | 2/5 | 字段为 `Tunnel ID` / `Runtime API Key`，并有明确“返回” |
| A176 | 2/5 | 密钥下方仅一行安全保存说明；保存失败时仍可返回，不得锁死 |
| A177 | 3/5 | 项目与权限位于同一屏，新增项目使用原生 Windows 文件夹选择器，并有明确“返回” |
| A178 | 3/5 权限模式 | `min-height >= 80px` 仅作最低保护；含标题+说明的两行按钮在 780×620 实机真实 rendered 高度至少为普通单行控件 2 倍，两个 line box 均完整可见；必须人工视觉验收，不能凭 CSS marker 自动 PASS；普通 selected 蓝色 `#0071e3`、管理员模式入口橙色 `#ff9500` |
| A179 | 4/5 ChatGPT 插件设置 | 标题为“创建自定义插件”，提示为“在插件设置页面最底端，打开‘开发者模式’”；`打开 ChatGPT插件设置` 位于左侧操作流，只经 Rust 固定 allowlist + 系统浏览器打开 `https://chatgpt.com/plugins#settings/Plugins` |
| A180 | 4/5 信息与复制 | 插件设置按钮下显示“打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件”；信息严格只有“名称 / Tunnel ID”两行，禁止“本地服务”；Tunnel ID 来自当前持久化值；两行独立复制成功绿色 `已复制` 精确 3 秒且不位移 |
| A181 | 4/5 插件管理 | `打开插件管理页` 位于左侧操作流，只经 Rust 固定 allowlist + 系统浏览器打开 `https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins`；禁止 WebView/任意前端 URL；底部有“返回 / 继续” |
| A182 | 3→4 runtime 时序 | 第 3 屏保存项目与权限后立即启动 selected project/runtime/MCP/OpenAI Tunnel，三项 readiness 全部真实就绪后才可进入第 4 屏；唯一启动边沿不得延迟至第 5 屏 |
| A183 | 5/5 启动检查 | 严格只有本地运行环境/编码服务/OpenAI Tunnel；状态点与 Dashboard 使用同源 typed 状态并映射 Ready=绿、Starting=黄色/琥珀、Fault=红、Unknown=灰 |
| A184 | 5/5 未全部 Ready | “确定”灰色且 disabled，不显示完成提示；仍有明确“返回”到第 4 屏 |
| A185 | 5/5 全部 Ready | 显示“配置完成，在插件中选择刚刚添加的Local Bridge试试吧”；“确定”启用但不自动跳转，点击后标记 onboarding_complete 并进入主界面 |
| A186 | onboarding 返回路径 | 除第 1 屏外，第 2/3/4/5 屏均有明确“返回”；任何保存、启动、配置失败都不能锁死用户 |
| A187 | G3 普通强调色 | primary、普通 selected、主要交互统一使用原方案蓝色 `#0071e3`；黑色不得作为普通产品 accent |
| A188 | G3 管理员逻辑色 | onboarding 与设置页所有管理员模式入口统一使用橙色 `#ff9500` 警告色，不得被普通蓝色 selected 规则覆盖；Dashboard 无管理员模式选择控件；Starting 状态点和 Dashboard 重启按钮的黄色/琥珀语义不变 |
| A189 | Dashboard 服务状态点 | 主要服务状态旁显示状态圆点，与 onboarding 使用同一 typed 状态来源和 Ready/Starting/Fault/Unknown 颜色语义；不得维护冲突状态 |
| A190 | G3 UI 按钮与提示 | 主/次/ghost 一致且可辨识，白色表面无难识别白色按钮；自解释操作只保留最小必要提示，复制/状态反馈不得造成布局位移 |
| A191 | 品牌资源 | PNG 为 1024×1024 RGBA |
| A192 | Windows 图标 | ICO 包含 16/24/32/48/64/128/256 px |
| A193 | 应用/安装包 | 使用冻结的 LocalBridge 图标 |
| A194 | 系统托盘 | 使用冻结的 LocalBridge 图标，不使用占位图/emoji |
| A195 | UI 实现 | 不因品牌图标引入第三方 icon library |
| A196 | 资产完整性 | PNG SHA256 = `710690f2d70e3c69f13db9d4eaebc0bef5c80561c74acc7bc5a401c15c16e55a` |
| A197 | 资产完整性 | ICO SHA256 = `c995d6af01ebc5031950eb9ea6415b58671b31f84ed6b55baabe80ea51e33f78` |
| A198 | 主窗口尺寸 | inner/minimum/maximum size 均为 780×620，窗口始终保持该尺寸 |
| A199 | 窗口缩放 | `resizable=false`；拖拽边框不能改变窗口尺寸 |
| A200 | 窗口最大化 | `maximizable=false`；最大化入口不可用，Dashboard/onboarding 在固定 780×620 client area 内完整可操作 |
| A201 | 窗口外框 | native `decorations=false`；仅存在一层 edge-to-edge 自定义 chrome，不出现原生标题栏/边框 + 自定义边框的双框 |
| A202 | 自定义标题栏 | 可拖拽窗口；提供最小化与关闭；不提供最大化；chrome 从 client `(0,0)` 覆盖 100% 宽高 |
| A203 | 首次引导整页布局 | onboarding 直接占用 custom chrome 内容区，不存在“大面积空白画布 + 居中 floating card/modal/dialog”整体向导外壳；页面级 padding 与局部分组允许；schema26 管理员安全确认只作为局部 consent dialog 例外 |
| A204 | 3/5 权限模式按钮视觉 | 固定 780×620 下真实 computed/rendered 高度证明：含标题+说明的两行按钮至少为单行控件 2 倍且两个 line box 完整；`min-height >= 80px` 只作最低保护；该视觉项已由用户于 2026-08-14 PASS，后续若 Screen3 布局变化必须重新人工验收 |
| A205 | 管理员模式选择 | 仅设置页或 onboarding 第3屏可见；Broker 未 Active 时点击/重新点击管理员模式先进入 schema26 固定安全警告，禁止直接 UAC；只有完整 9000ms 后 enabled 红色 `确认` 被用户点击，backend 安全校验通过后才可请求 UAC；无单独“启用管理员权限”按钮；后台偏好恢复无 warning/UAC |
| A206 | 离开管理员模式 | 在设置页或 onboarding 切换编辑/完整模式立即关闭 privileged call gate 并停止 Broker；Dashboard 无模式切换入口 |
| A207 | 前台启动顺序 | onboarding 已完成且配置有效时，先创建/显示并达到可交互 UI；前端只发送一次 typed `UI-ready`，backend 收到后才异步启动原本停止的 selected project/runtime/MCP/OpenAI Tunnel，无额外“启动服务”动作；UI-ready 前不得提前启动服务 |
| A208 | 前台慢启动 / ready 幂等 | backend 故意延迟时窗口仍可交互，Starting/Ready/Fault 从 typed projection 更新；重复 UI-ready 不产生第二 runtime owner；`--background` 不等待 UI-ready，唤醒已有健康后台 runtime 不仅因 ready gate 重启 |
| A209 | CurrentTask idle | 左下状态固定显示“等待命令”，不得显示“空闲”或隐藏 |
| A210 | CurrentTask 生产投影 | 真实 MCP/Broker 调用端到端改变 backend CurrentTaskStatus/timing 并通过唤醒式 delivery 反映到 UI；短 create/delete/modify/command 不依赖 polling；活动显示持续时间，terminal 回到第一行“等待命令”，第二行保留唯一上一工具+年龄；前端不伪造 |
| A211 | Dashboard 新项目 | “选择其他文件夹”打开原生 Windows 文件夹选择器，手填路径不是主流程 |
| A212 | 设置结构 | 仅常规/连接/权限三组；常规仅“开机启动/关闭窗口后继续运行”；底部“打开欢迎页/完成” |
| A213 | 设置连接固定态 | 字段严格为 `Tunnel ID` / `Runtime API Key`；密钥只显示“已保存/未保存”，两项各有最右“更换”；Key 已保存时另有“清除”紧邻位于 Key“更换”左侧；完整密钥永不回显 |
| A214 | 设置连接编辑 | 点“更换”才编辑；未改字段不要求重输、不被覆盖；保存密钥输入不得预填真实 secret |
| A215 | 设置部分更新 | 只改 Tunnel ID 不改/不要求 Runtime API Key；只改 Runtime API Key 不改 Tunnel ID |
| A216 | 设置保存 | 基础格式校验→安全写入→运行/连接中且有效连接配置变化时受控重连；Starting/connecting 且 active=false 也不得沿用旧 captured config；不存在“测试连接”按钮 |
| A217 | 关闭窗口继续运行=开 | X 仅隐藏窗口，runtime/tray 继续 |
| A218 | 关闭窗口继续运行=关 | X 有序关闭 privileged gate/Broker/Tunnel/PEP/MCP 后退出；偏好版本化持久化 |
| A219 | 3/5 权限按钮结构 | `min-height >= 80px` 仅是最低保护；780×620 实际 rendered geometry 必须证明两行按钮高度至少为单行控件 2 倍且标题/说明均完整；2026-08-14 scoped 人工视觉项已 PASS，布局变化时重新验收 |
| A220 | Onboarding backend ownership | React 不拥有 runtime start/readiness polling 状态机；backend 持有并投影；慢 backend 时 UI 仍响应 |
| A221 | 诊断结构 | 仅运行状态/项目/日志；运行状态四行=本地运行环境/编码服务/OpenAI Tunnel/管理员权限；项目显示实际路径 |
| A222 | 诊断日志/动作 | 最近限量脱敏日志；页面动作仅“打开日志/导出诊断/完成”，无刷新/重试连接/打开欢迎页/工程 generation 字段 |
| A223 | UI/backend 分离 | WebView 只 render typed projection + send typed intent；耗时 process/filesystem/credential/UAC/recovery 工作不占用 UI 事件线程 |
| A224 | LB-018 Cloudflare retirement | 最终 bundle/runtime manifest/installer/launcher/fallback 不含 `cloudflared.exe`、Cloudflare managed tunnel 或 cloudflared manifest；历史 compatibility 证据不进入可执行发行物 |
| A225 | Dashboard 权限边界 | 主页无“权限模式”及三档选项、不能修改 PermissionMode/UAC；只读管理员权限状态来自 PrivilegeState；设置页与显式重新打开的 onboarding 第3屏均可编辑权限 |
| A226 | 临时操作提示 | `无法准备管理员权限`、一次性保存/选择失败等 one-shot 提示默认 3 秒自动清除；持续 runtime/reconnect Fault 不被临时规则隐藏 |
| A227 | 短任务状态捕获 | 文件新建、删除、修改及普通命令等真实工具调用必须由 backend 唤醒式 delivery 捕获；不得把周期 polling 当作短任务主要传输 |
| A228 | 执行持续时间 | 活动任务单行显示 backend-grounded elapsed duration；不得由前端伪造任务开始/结束状态 |
| A229 | 上次工具时间 | 第一行 Idle 只显示 `等待命令`；第二行上一工具相对时间覆盖 `59S前`、`59分钟前`、`大于1小时`、`大于n天` 并靠右；从未执行工具时无上一工具行 |
| A230 | workspace identity / execution / display path 边界 | `\\?\D:\project` 仅允许用于内部 filesystem identity 校验/去重/reparse/授权比较；UI 以及 MCP/Broker/sidecar/process/command/tool 的路径参数和 `cwd/workdir/current_dir` 必须使用与同一 freshly validated identity 绑定的普通 `D:\project`；execution/display 转换不得授权，identity 不一致 fail-closed |
| A231 | 设置“更换”按钮对齐 | Tunnel ID 与 Runtime API Key 两个“更换”在 780×620 下保持同一最右动作列、几何差 ≤1 CSS px；Key“清除”位于 Key“更换”左侧且不得推移最右列 |
| A232 | 同级按钮对齐 | 同一页面/分组 peer actions 复用统一动作列/左基线和 shared button geometry，不允许任意 offset/第二套对齐语言 |
| A233 | Dashboard 重启服务 | 显示黄色/琥珀 `重启服务`；真实服务 lifecycle 由 backend 执行，且必须满足机器合同的 single-owner/最新持久化配置语义 |
| A234 | Dashboard 关闭服务 | 显示红色 `关闭服务`；真实服务 lifecycle 由 backend 执行并记录显式 manual-stop，Dashboard 窗口保持可用并显示停止状态 |
| A235 | 服务按钮视觉 | `重启服务`/`关闭服务` 与现有按钮共享尺寸、字体、边界、圆角和水平对齐体系，仅逻辑色不同 |
| A236 | verbatim workspace 命令回归 | 当内部 `GetFinalPathNameByHandleW` 得到 `\\?\D:\project` 时，coding-tools/`exec_command` 或等价普通命令实际收到 `D:\project` cwd/workdir 并成功执行；MCP/Broker/sidecar/ManagedProcessSpec/process/tool invocation 不得收到 `\\?\` cwd/workdir/current_dir/路径参数 |
| A237 | 短工具调用唤醒 | 任意真实 MCP/Broker 工具调用开始/终止都通过 backend push/event 或等价唤醒路径使 Dashboard 更新；周期 polling 不得作为短任务的主要传输 |
| A238 | 工具最低可见期 | 每个真实工具调用即使瞬时完成也至少有 500ms 可见 presentation interval；该停留只约束 UI，不得人为延迟真实工具返回/响应 |
| A239 | 上次执行工具行 | 第一行 Idle 只显示“等待命令”；第二行固定前缀“上次执行工具：”+脱敏用户标签/安全摘要，禁止 raw MCP id；相对时间位于第二行最右并覆盖 nS前/n分钟前/大于1小时/大于n天；只保留一条上一工具元数据 |
| A240 | 2/5 已保存连接显示 | 已有 Tunnel ID 时输入框预填当前持久化值；已有 Runtime API Key 时固定显示“已安全保存至windows安全凭据”；安全提示严格为“Runtime API Key 仅保存在 Windows 安全凭据中。” |
| A241 | 2/5 已保存 Key 掩码 | 聚焦已保存 Runtime API Key 输入框时，只根据 backend 长度元数据显示与已存 key 字符数相同的 `*`；plaintext 永不返回前端，未真正输入新 key 时掩码不得被提交/保存为替代 key |
| A242 | 两行权限按钮真实几何 | 780×620 下含标题+说明的权限按钮实际 rendered 高度至少为普通单行控件 2 倍，两个文本 line box 均完整可见；仅有 `min-height/padding` CSS 不构成 PASS；2026-08-14 用户已通过该 scoped 视觉验收，后续布局变化须复验 |
| A243 | 固定窗口 780×620 | 主窗口 default/minimum/maximum inner size 全部严格为 780×620；`resizable=false`、`maximizable=false`、`decorations=false`、唯一 edge-to-edge custom chrome 等其余窗口语义不变 |
| A244 | 设置 Key 清除位置 | Runtime API Key 已保存时显示“清除”，紧邻位于该行“更换”的左侧；Tunnel ID/Runtime API Key 两个“更换”仍保持同一最右动作列 |
| A245 | 设置 Key 清除语义 | 点击“清除”真实删除 Windows 安全凭据中的 Runtime API Key，不读取/回显 secret，投影更新为“未保存”；runtime active/connecting 时按现有有效连接配置变化 lifecycle 受控处理，不允许旧 runtime 继续依赖已清除 credential |
| A246 | 滚动圆角 / 无箭头 | 强制 Settings 或其他 rounded sheet/dialog/card 产生纵向 overflow 时四个外层圆角仍完整；scrollbar 被裁切或内缩在圆角壳内；顶部/底部原生箭头、三角形或等价增减按钮不得显示，滚轮/轨道/滑块仍必须可用 |
| A247 | LocalBridge public Tool Registry | v1 非特权 core Registry 严格只有 `workspace_context / agent_workflow / exec_command / command_control / task_control / git_workflow / document_workflow / view_image`；实际 `tools/list` 只返回当前 policy 允许的 core 子集，并可附加当前 policy 允许的 LocalBridge 特权扩展；不得出现 upstream private tool name |
| A248 | `elevated_exec` public extension | 保持现有 conditional privileged extension：Edit/Full 仍 deny，只有 Elevated + Broker Active + reviewed policy 时可用；不得伪装成普通八工具 core |
| A249 | upstream schema 隔离 | upstream 新增 tool、修改 private schema/result/error 不会自动改变 LocalBridge public Registry/schema；private upstream error/schema 不得穿透 public API |
| A250 | runtime facade capability negotiation | adapter 初始化必须验证全部 mandatory LocalBridge facade capability/schema；缺失能力或不兼容时在提供 public facade 前 fail-closed，不得带病降级成 upstream passthrough |
| A251 | PEP capability/action 边界 | PEP 按稳定 LocalBridge public action/capability 分类而非 raw upstream tool id；raw upstream 名称不能成为绕过入口；高层 workflow 在执行前声明并检查全部 transitive write/process/network/privilege capability；unknown deny |
| A252 | ShellResolver `auto` | 只在已验证可信 shell 候选中按 semantic version 选择最高兼容 PowerShell Core；没有可信 Core 时回退 Windows PowerShell 5.1，再回退 `cmd.exe` |
| A253 | ShellResolver PATH 劫持 | PATH 仅作为候选发现线索；恶意更靠前的 `pwsh.exe` 在未通过可信安装位置或显式注册 executable identity 重新验证前，既不得执行也不得为了版本判断被 probe |
| A254 | Shell selector 安全 | public shell selector 仅 `auto / powershell / pwsh / windows_powershell / cmd`；禁止根据 command text 猜 shell、禁止 MCP 提供任意 shell executable path、禁止 LocalBridge 自动安装/更新 shell |
| A255 | direct process / shell 分离 | DirectProcessExecutor 与 ShellExecutor 使用不同 structured spec/执行边界；直接或 privileged process execution 不得退化成 shell string canonical representation |
| A256 | stable adapter / execution envelope | upstream result/error 在 adapter 边界归一化为稳定 LocalBridge contract；CurrentTask/执行 envelope 使用稳定 LocalBridge public tool/capability identity + secret-redacted safe summary，禁止 raw upstream tool id/private schema 泄漏 |
| A257 | schema26 管理员警告固定文案 | Intro 精确为 `启用管理员权限后，错误或恶意操作可能导致：`；八条 bullet 顺序/文字严格匹配机器合同；footer 精确为 `仅在你明确理解操作后果时授权。`；不得增删、改写或用旧三条后果版本 |
| A258 | schema26 确认按钮视觉 | 确认按钮整个按钮为红色，不是仅倒计时数字红色；管理员模式入口本身为橙色 `#ff9500`，二者语义不得混淆 |
| A259 | schema26 9 秒倒计时 | fresh dialog 初始 `确认9`，依次显示 `确认8…确认1`；完整 9000ms 内红色按钮始终 disabled；达到 trustworthy not-before 后同一红色按钮才 enabled 且标签精确为 `确认` |
| A260 | schema26 倒计时不可绕过 | pointer、keyboard、synthetic/repeated click、rerender、focus change、stale frontend state、旧 challenge replay 均不能在 9000ms 前获得授权；frontend interval 不是授权真相，backend/等价可信 monotonic eligibility fail-closed |
| A261 | schema26 取消/重开 | `取消`、Esc、close/dismiss 无 PermissionMode/Broker/UAC 副作用；每次 fresh open 都重新开始完整 9 秒；无 remember/skip/don't-show-again bypass |
| A262 | schema26 UAC 顺序 | Broker 未 Active 时，UAC/runas 与 PermissionMode/Broker 激活副作用只能发生在用户点击 enabled `确认` 之后，并且仍需通过既有 backend 安全校验；UAC cancel/failure 后 Broker 不得 Active |
| A263 | schema26 background / active broker | background preference restore 不显示 warning、不 UAC；Broker 已 Active 时仅因重新选择当前管理员模式不得重复 UAC |
| A264 | schema26 control-plane | AI/MCP 不能批准 warning、UAC、PermissionMode 或 Broker activation；模式警告不等价于 High/Critical 单操作确认，后者仍独立要求 |
| A265 | schema26 onboarding shell | 管理员 warning 可以作为局部安全 consent dialog，但不能被用来证明或允许 onboarding 重新出现“大面积空白 + 居中 card/modal/dialog”整体外壳 |
| A266 | `workspace_context` active workspace projection | active workspace 为 `D:\project` 时，public `workspace_context.workspace` 必须非空且精确投影 ordinary `D:\project`（不得 `\\?\`）；`default_cwd` 独立投影为 `.` 或 workspace-relative 子路径；不得因 private 字段名不匹配静默返回空字符串 |
| A267 | public command/session handle ownership | `exec_command` 返回的 public `session_id` / `output_ref` 由 LocalBridge Session Manager 发行并映射；不得等于或直接泄露 upstream private session/output handle，private handle 也不得进入 CurrentTask/日志/长期状态 |
| A268 | `command_control` 四动作闭环 | `poll` 只需 public `session_id` 并实际读取 live-session 状态/增量输出；`write`/`kill` 使用 public `session_id`；`read` 使用 public `output_ref` + stream/offset/limit；poll 不得错误调用只接受 output_ref 的 retained-output primitive；长命令可连续 poll/read/write/kill |
| A269 | public session terminal convergence | command 不依赖客户端持续 poll 也会由 backend 收敛 `running → completed/failed/timed_out/cancelled/lost`；upstream session 完成、300s pruning、runtime 重启或 private handle 丢失都不能让 public session/CurrentTask 永久 Running；未观察到 terminal outcome 即丢失时返回精确稳定错误 `SessionUnavailable` |
| A270 | silent nonzero process exit | `cmd /c exit 7` 或任意无 stdout/stderr 的非零退出必须 public `ok=false` / `isError=true`、稳定 `ProcessFailed`、session/task=Failed；只有 `exit_code=0` 才是普通 completed success |
| A271 | advertised public facade completeness | `tools/list` 中每个已广告 tool/action 都必须可真实执行；当前 v1 `agent_workflow` 九个 action、`task_control get/cancel`、`document_workflow inspect/create/convert/rebuild` 不得恒定返回“当前不可用”；新增 action 必须实现+分类+测试后才可进入 schema |
| A272 | workspace-bound public path inputs | `exec_command.workdir`、`git_workflow.path/paths`、`document_workflow.path`、`view_image.path` 统一使用 active-workspace-relative 路径；drive/UNC/verbatim/POSIX absolute 与 `..` traversal 在 LocalBridge boundary typed reject，不能把 upstream `ABSOLUTE_PATH_DENIED` 私有错误直接暴露，也不能靠 normalization 扩大授权 |
| A273 | mandatory private result semantic probe | adapter 读取的 private result 字段若未被 upstream `outputSchema` 精确保证，public facade 启动前必须执行 deterministic、non-destructive compatibility/result probe；至少 `get_default_cwd` 必须证明 `workspace:string(non-empty)` + `default_cwd:string`，否则 `RuntimeCapabilityMismatch` fail-closed，而不是运行后才产生错误投影 |
| A274 | nested Git repository consistency | active workspace=`D:\project` 且 `D:\project\LocalBridge` 为 nested repo 时，`git_status/git_log/git_show/git_diff(path="LocalBridge")` 必须解析到同一 repository；`git_blame(path="LocalBridge/package.json")` 必须从文件 parent 找到同一 enclosing repo；repo discovery 不得越过 active workspace root |
| A275 | Git resolver / diff fallback | `git_workflow` 五 action 共享一个 LocalBridge repo resolver；directory action 的 `path` 选择 repo context，action-specific `paths` 才是 path filter；resolver 已确认 Git repo 时 `git_diff` 必须使用 native Git semantics，禁止 silent `non-git diff fallback` |
| A276 | 测试 Gate 分层 | 固定 `PR Fast / PR Runtime / Group-Release` 三层；开发内循环优先当前 PR targeted cheap checks，完整全仓 Gate 只在正式 PR/group/release acceptance 时执行，不得每个小改后重复全跑 |
| A277 | 重型 fixture 压缩 | 共享同一 bundled runtime/PEP/process topology 且 isolation 不是被测行为的重测试必须共享 lifecycle；重复 startup 需明确 isolation 理由；廉价 isolated unit tests 不得被强制合成巨型测试 |
| A278 | static vs behavior | source-string/static contract test 只证明静态事实，不能因函数/测试名/marker 存在替代真实 unit/integration/E2E 行为 PASS；重复 static + executable check 必须保护不同合同 |
| A279 | bounded test/session lifecycle | 测试 runner 有 bounded timeout/cancel；command/session lost、控制句柄不可寻址或 Runtime Gate 失败必须进入明确 terminal failed/lost，不得无限等待 running |
| A280 | development console / packaged GUI | 开发/测试命令窗口或后台命令进程允许存在且本身不是产品缺陷；release-style/packaged GUI 中 LocalBridge-owned runtime/Tunnel/Broker/helper/shell/direct-command child 不得意外显示 console window，并保持 Job/process ownership |
| A281 | shell quoting fidelity | PowerShell `Write-Output "a|b"` / `Write-Output "a&b"` 以及单引号等价形式输出精确字面值；LocalBridge 只允许一次预期 shell parse，不得额外 reparse/quote-loss |
| A282 | incremental poll | 依次输出 `poll-1/poll-2/poll-3` 的 running command 在连续 public poll 中按顺序各交付一次且不丢 chunk；无新输出的后续 poll 返回空 delta + state/terminal metadata，不重放 `poll-3` |
| A283 | live command write | exec 已返回 running public session 后，`command_control.write(session_id, chars)` 仍有效；真实 stdin-waiting command 收到 post-start chars，不得立即 `SessionUnavailable` |
| A284 | kill terminal convergence | valid running session + healthy runtime 下 kill 不得 `RuntimeUnavailable`；成功 TERM/KILL/INT（按支持范围）收敛稳定 `cancelled` terminal，后续 poll 维持 cancelled snapshot，不退化 `SessionUnavailable` |
| A285 | view_image actual resize | 1024×1024 图像 `auto_resize=true` 在 max=512×512 与 64×64 时均成功得到比例保持且 dimensions 不超上限的 public image；无需 resize 路径仍成功；不能因实际 resize 需求返回 `ProcessFailed` |
| A286 | PowerShell UTF-8 | `Write-Output '中文输出测试'` 在 `windows_powershell` 及 `auto`→PowerShell 时 public output 精确为 UTF-8 原文，无 `�`/mojibake |
| A287 | Git blame inclusive range | `start_line/end_line` 为 1-based inclusive：5..5 只返回第5行，1..3 精确3行；start>end=`InvalidArgument`；`max_lines` 保持一致 bounded semantics |
| A288 | document inclusive range | document inspect/read line range 为 1-based inclusive；同时提供 start/end 且 start>end 必须返回 LocalBridge `InvalidArgument`，不得成功返回空文本 |
| A289 | atomic terminal finalizer | command/session 的 success、nonzero failure、timeout、cancel、kill、runtime/tool exception 等所有 terminal 路径都必须进入 `finally`/finally-equivalent unconditional finalizer；同一 owner transaction 内持久化 terminal snapshot、精确一次 `command_finished` 并清空 `current_command`，不得产生 terminal task + running current_command |
| A290 | durable terminal snapshot independent of retention | task-state 自身保存 bounded/redacted terminal snapshot，private session 即使立即 prune 或超过约 300 秒 retention 后仍能恢复相同 terminal outcome；session retention 只能支持临时输出读取，不能作为最终结果真相源 |
| A291 | task/session owner CAS | command start/replace/finish/clear 必须 compare-and-swap/owner-check `(task_id, session_id)`；旧 task/旧 session 的 delayed finalizer 不得覆盖或清除新 owner，owner mismatch no-op/typed conflict；相同 owner 重复 terminal callback 幂等且不重复 `command_finished` |
| A292 | default centered main window | 正常前台首次创建/首次显示固定 780×620 主窗口时，在当前 monitor work area 居中（允许 DPI rounding）；托盘重新显示已存在且可能被用户移动的窗口时保持当前位置，不得每次 show 强制重新居中 |
| A293 | `agent_workflow` nested project selection | active workspace=`D:\project` 时，`agent_workflow(path="LocalBridge")` 必须选择并识别 `D:\project\LocalBridge`，其 Git before/after 与 `git_workflow(path="LocalBridge")` 对同一 repo 给出一致 `is_repo=true`；`path="LocalBridge/src"` 向上解析最近 enclosing repo 但不得越过 active workspace；选择 nested project 只改变 workflow/project context，不改变 WorkspaceRegistry 或 active authorization root |
| A294 | trusted PowerShell standard cmdlet baseline | `windows_powershell` 与 `auto`→PowerShell 必须至少可正常执行 `Get-Location`、`Get-ChildItem`、`Test-Path` 等标准 coding/management cmdlet；允许通过固定/身份验证的系统模块预加载恢复该能力，但 arbitrary module autoload 继续禁用，用户控制的 `PSModulePath` 不得重定向 preload；`New-Item/Set-Content/Set-Item` 等 provider/dynamic-command mutation 仍保持现有 review-required 安全分类 |
| A295 | active-workspace structured directory write | `agent_workflow` 提供 optional bounded `directory_changes[]`，每项严格为 `{action,path}`，action 仅 `create_directory` / `remove_empty_directory`，path 为 active-workspace-relative。active workspace=`D:\project` 时可无需 process exec 创建 `D:\project\test` 并在目录为空时清理；Edit/Full 都可授权该 reviewed workspace write。absolute/`..`/reparse escape 必须 deny，非空递归目录删除不由本条授权，且该能力永远不能修改 WorkspaceRegistry、切换 active workspace 或扩大授权根 |
