# 07 — Acceptance Matrix v14

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
| A55 | Elevated enable | 只有 Broker 触发 UAC |
| A56 | Elevated active | 主 LocalBridge/Tunnel/MCP 不因 Broker 提权 |
| A57 | Elevated | 无 15/30/60 分钟或 expires_at |
| A58 | Elevated disable | privileged call gate 立即关闭，Broker 停止 |
| A59 | Broker inactive + privileged call | typed `ElevationRequired` |
| A60 | reboot/background startup + Elevated preference | 不自动弹 UAC |
| A61 | user opens UI after reboot | 可显式重新激活 Broker |
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
| A93 | Dashboard + PrivilegeState::Active | 只读显示“管理员权限：已启用”；切换权限模式可进入设置页或显式重新打开的 onboarding 第3屏 |
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

| A119 | 主控界面无活动任务 | 第一行固定显示“等待命令”，不得在该行追加相对时间，无活动历史列表 |
| A120 | read_file tools/call | 类型显示“读取文件”，任务显示安全路径摘要 |
| A121 | search tools/call | 类型显示“搜索代码”，显示安全搜索摘要 |
| A122 | command tools/call | 类型显示“执行命令/运行测试/构建”等稳定分类 |
| A123 | policy deny | 当前任务显示“已阻止”，不得先显示“执行中” |
| A124 | 管理员调用等待 UAC | 当前任务显示“管理员操作 / 等待授权” |
| A125 | task terminal | 最终回到第一行“等待命令”，并保留唯一上一工具安全标签/摘要与完成时间元数据供第二行展示，不追加历史消息 |
| A126 | 主控界面 | 无最近活动、消息流、时间线 |
| A127 | raw tool id | 不直接显示 MCP tool identifier |
| A128 | secret-bearing args | 任务摘要不泄漏密钥/token/nonce |
| A129 | model prose only | 未发生 MCP/Broker 调用时不得伪造当前任务 |

| A130 | 运行测试中 | 单行显示“● 运行测试 cargo test · <持续时间>” |
| A131 | 当前执行状态 | 无“当前任务/类型/任务/状态”标题 |
| A132 | Running | 不额外显示“执行中”文字 |
| A133 | Running | 绿色活动点使用轻量脉冲动效 |
| A134 | reduced-motion | 活动点静态，不执行脉冲 |
| A135 | 动效 | 不推动布局、不造成文字位移 |
| A136 | Idle | 第一行必须显示低存在感“○ 等待命令”；不得显示“空闲”、隐藏整行或在第一行追加年龄；年龄属于第二行上一工具信息 |

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

| A171 | 首次启动 | 严格 5 屏，无第 6 屏；欢迎 → OpenAI → 项目与权限 → 创建自定义插件 → 启动检查 |
| A172 | 3/5 项目与权限 | 新项目使用原生 Windows 文件夹选择器；含标题+说明的权限按钮在 780×620 下真实 rendered 高度至少为单行控件 2 倍且两个 line box 完整，静态 CSS marker 不可自动 PASS，仍需人工 Gate；普通 selected 蓝色 `#0071e3`、管理员黄色/琥珀；有明确返回 |
| A173 | 4/5 创建自定义插件 | 固定开发者模式提示；`打开 ChatGPT插件设置` 左侧固定 Rust allowlist 系统浏览器入口；其下显示“打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件”；中部严格只有名称/Tunnel ID，持久化 Tunnel ID，禁止本地服务行；两行独立 3 秒绿色复制反馈不位移；`打开插件管理页` 同样左侧且固定 allowlist；无 WebView/任意前端 URL；底部返回/继续 |
| A174 | 3→4 runtime / 返回 | 第 3 屏保存后启动 selected project/runtime/MCP/Tunnel 并全就绪后才进入第 4 屏，唯一启动边沿不得延迟至第 5 屏；除第1屏外第2/3/4/5屏均有返回，失败不得锁死 |
| A175 | 5/5 状态点 | 仅本地运行环境/编码服务/OpenAI Tunnel；与 Dashboard 使用同源 typed 状态，Ready绿 / Starting琥珀 / Fault红 / Unknown灰 |
| A176 | 5/5 完成 Gate | 三项全绿前确定 disabled 且隐藏完成提示；全绿后显示固定完成提示并启用确定；不自动跳转；仍可返回第4屏 |
| A177 | G3 强调色 / 按钮 | 普通 primary、普通 selected 与主要交互统一蓝色 `#0071e3`，黑色不得作为普通 accent；管理员模式黄色/琥珀；按钮一致可辨识，禁止白底白按钮 |
| A178 | G3 提示 / 状态来源 | 最小必要，不重复堆叠自解释说明；复制/状态反馈不位移；Dashboard 与 onboarding 不得各维护冲突服务状态 |
| A179 | 固定窗口尺寸 | inner/minimum/maximum size 均为 780×620 |
| A180 | 禁止缩放 | `resizable=false`，拖拽边框不能改变窗口尺寸 |
| A181 | 禁止最大化 | `maximizable=false`，最大化入口不可用；Dashboard/onboarding 在固定 client area 内完整可操作 |
| A182 | 单层窗口 chrome | native `decorations=false`；自定义 chrome edge-to-edge 覆盖 client area，不存在双边框 |
| A183 | 自定义窗口控制 | 有拖拽区、最小化、关闭，无最大化 |
| A184 | onboarding 整页布局 | 5 屏直接使用 custom chrome 内容区；不得用居中 floating card/modal/dialog 或大圆角+整体阴影/边框制造“窗口里的窗口” |
| A205 | 管理员模式选择 | 仅设置页或 onboarding 第3屏可见；点击/重新点击“管理员模式”时若 Broker 未 Active，立即发起 Windows UAC；不存在单独“启用管理员权限”按钮；后台偏好恢复不自动 UAC |
| A206 | 离开管理员模式 | 在设置页或 onboarding 切换编辑/完整模式立即关闭 privileged call gate 并停止 Broker；Dashboard 无模式切换入口 |
| A207 | 前台启动顺序 | configured 前台先创建/显示并达到可交互 UI，再由前端发送一次 typed `UI-ready`；backend 收到后才启动原本停止的 selected project/runtime/MCP/OpenAI Tunnel，ready 前禁止提前启动服务 |
| A208 | 前台慢启动 / ready 幂等 | backend 故意延迟时 UI 仍可交互并投影 Starting/Ready/Fault；重复 ready 不产生第二 owner；`--background` 不等待 ready，健康后台实例不因此重启 |
| A209 | CurrentTask idle | 左下状态固定显示“等待命令”，不得显示“空闲”或隐藏 |
| A210 | CurrentTask 生产投影 | 真实 MCP/Broker 调用端到端改变 backend CurrentTaskStatus/timing 并通过唤醒式 delivery 反映到 UI；短 create/delete/modify/command 不依赖 polling；活动显示持续时间；terminal 回到第一行“等待命令”，第二行保留唯一上一工具+年龄；前端不伪造 |
| A211 | Dashboard 新项目 | “选择其他文件夹”打开原生 Windows 文件夹选择器，手填路径不是主流程 |
| A212 | 设置结构 | 仅常规/连接/权限三组；常规仅“开机启动/关闭窗口后继续运行”；底部“打开欢迎页/完成” |
| A213 | 设置连接固定态 | 字段严格为 `Tunnel ID` / `Runtime API Key`；密钥只显示“已保存/未保存”，两项各有同一最右“更换”；Key 已保存时“清除”紧邻位于 Key“更换”左侧；完整密钥永不回显 |
| A214 | 设置连接编辑 | 点“更换”才编辑；未改字段不要求重输、不被覆盖；保存密钥输入不得预填真实 secret |
| A215 | 设置部分更新 | 只改 Tunnel ID 不改/不要求 Runtime API Key；只改 Runtime API Key 不改 Tunnel ID |
| A216 | 设置保存 | 基础格式校验→安全写入→运行/连接中且有效连接配置变化时受控重连；Starting/connecting 且 active=false 也不得沿用旧 captured config；不存在“测试连接”按钮 |
| A217 | 关闭窗口继续运行=开 | X 仅隐藏窗口，runtime/tray 继续 |
| A218 | 关闭窗口继续运行=关 | X 有序关闭 privileged gate/Broker/Tunnel/PEP/MCP 后退出；偏好版本化持久化 |
| A219 | 3/5 权限按钮结构 | `min-height >= 80px` 仅作最低保护；780×620 rendered geometry 满足两行按钮≥单行 2 倍且文本完整；2026-08-14 scoped 人工视觉已 PASS，布局变化须复验 |
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
| A231 | 设置“更换”按钮对齐 | Tunnel ID 与 Runtime API Key 两个“更换”在 780×620 下保持同一最右动作列、几何差 ≤1 CSS px；Key“清除”不得推移该列 |
| A232 | 同级按钮对齐 | 同一页面/分组 peer actions 复用统一动作列/左基线和 shared button geometry，不允许任意 offset/第二套对齐语言 |
| A233 | Dashboard 重启服务 | 显示黄色/琥珀 `重启服务`；真实服务生命周期由 backend 执行，且满足机器合同的 single-owner 与最新持久化配置语义 |
| A234 | Dashboard 关闭服务 | 显示红色 `关闭服务`；真实服务生命周期由 backend 执行并记录显式 manual-stop，Dashboard 窗口保持可用并显示停止状态 |
| A235 | 服务按钮视觉 | `重启服务`/`关闭服务` 与现有按钮共享尺寸、字体、边界、圆角和水平对齐体系，仅逻辑色不同 |
| A236 | verbatim workspace 命令回归 | 当内部 `GetFinalPathNameByHandleW` 得到 `\\?\D:\project` 时，coding-tools/`exec_command` 或等价普通命令实际收到 `D:\project` cwd/workdir 并成功执行；MCP/Broker/sidecar/ManagedProcessSpec/process/tool invocation 不得收到 `\\?\` cwd/workdir/current_dir/路径参数 |
| A237 | 短工具调用唤醒 | 任意真实 MCP/Broker 工具调用开始/终止通过 backend push/event 或等价唤醒路径更新 Dashboard；polling 不得作为短任务主要传输 |
| A238 | 工具最低可见期 | 每个真实工具调用至少具有 500ms 可见 presentation interval；UI 停留不得延迟工具真实返回/响应 |
| A239 | 上次执行工具行 | 第二行固定 `上次执行工具：` + 脱敏用户标签/安全摘要，禁止 raw MCP id；相对时间在最右；仅保留一个上一工具元数据，第一行不显示年龄 |
| A240 | 2/5 已保存连接显示 | 已保存 Tunnel ID 预填当前值；已保存 Key 固定文本 `已安全保存至windows安全凭据`；安全提示严格为 `Runtime API Key 仅保存在 Windows 安全凭据中。` |
| A241 | 2/5 Key 掩码 | 聚焦已保存 Key 只基于长度元数据显示同位数 `*`；plaintext 不回前端，未真正替换时掩码不得被提交/保存 |
| A242 | 两行权限按钮真实几何 | 780×620 下两行权限按钮 rendered 高度 ≥ 单行控件 2 倍且两个 line box 完整；CSS marker 不构成 PASS；2026-08-14 用户 scoped 视觉审核已 PASS，布局变化须复验 |
| A243 | 固定窗口 780×620 | default/min/max inner size 全为 780×620，其余不可缩放/不可最大化/无原生 decorations/custom chrome 语义保持 |
| A244 | 设置 Key 清除位置 | Key 已保存时 `清除` 紧邻位于 Key `更换` 左侧；两项 `更换` 仍处同一最右动作列 |
| A245 | 设置 Key 清除语义 | `清除` 删除 Windows 安全凭据，不回显 secret，投影为未保存；active/connecting 时使用既有受控连接配置变化 lifecycle |
| A246 | 滚动圆角 / 无箭头 | rounded sheet/dialog/card 出现纵向 overflow 时四角仍完整，scrollbar 被裁切/内缩；顶部/底部箭头、三角形或等价按钮不显示，滚轮/轨道/滑块仍可用 |
