# 07 — Acceptance Matrix v12

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

| A89 | Dashboard + Edit | 显示管理员权限“未启用”，无冗余启用按钮 |
| A90 | Dashboard + Full | 显示管理员权限“未启用”，无冗余启用按钮 |
| A91 | Dashboard + Elevated + Requested | 显示“等待授权”与“启用管理员权限” |
| A92 | Dashboard + Elevated + AwaitingUac | 显示“等待 UAC” |
| A93 | Dashboard + Elevated + Active | 显示“已启用”与“关闭管理员权限” |
| A94 | Broker Active→Faulted | Dashboard 立即显示“故障” |
| A95 | Dashboard privilege status | 由 PrivilegeState 驱动，不由 PermissionMode 猜测 |
| A96 | Dashboard | 不显示 PID/nonce/SID/IPC 等内部字段 |

| A97 | 主控界面 | 不出现 Dashboard/Settings/Diagnostics 等普通英文 |
| A98 | 权限模式 | 显示“编辑模式 / 完整模式 / 管理员模式” |
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

| A119 | 主控界面空闲 | 当前任务显示“空闲”，无活动历史 |
| A120 | read_file tools/call | 类型显示“读取文件”，任务显示安全路径摘要 |
| A121 | search tools/call | 类型显示“搜索代码”，显示安全搜索摘要 |
| A122 | command tools/call | 类型显示“执行命令/运行测试/构建”等稳定分类 |
| A123 | policy deny | 当前任务显示“已阻止”，不得先显示“执行中” |
| A124 | 管理员调用等待 UAC | 当前任务显示“管理员操作 / 等待授权” |
| A125 | task terminal | 最终回到“空闲”，不追加历史消息 |
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
| A136 | Idle | 显示低存在感“○ 空闲”或隐藏整行 |

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
| A172 | 3/5 项目与权限 | 新项目使用原生 Windows 文件夹选择器；权限按钮换行自动增高，900×620 视觉留白需人工 Gate；普通 selected 蓝色 `#0071e3`、管理员黄色/琥珀；有明确返回 |
| A173 | 4/5 创建自定义插件 | 固定开发者模式提示；`打开 ChatGPT插件设置` 左侧固定 Rust allowlist 系统浏览器入口；其下显示“打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件”；中部严格只有名称/Tunnel ID，持久化 Tunnel ID，禁止本地服务行；两行独立 3 秒绿色复制反馈不位移；`打开插件管理页` 同样左侧且固定 allowlist；无 WebView/任意前端 URL；底部返回/继续 |
| A174 | 3→4 runtime / 返回 | 第 3 屏保存后启动 selected project/runtime/MCP/Tunnel 并全就绪后才进入第 4 屏，唯一启动边沿不得延迟至第 5 屏；除第1屏外第2/3/4/5屏均有返回，失败不得锁死 |
| A175 | 5/5 状态点 | 仅本地运行环境/编码服务/OpenAI Tunnel；与 Dashboard 使用同源 typed 状态，Ready绿 / Starting琥珀 / Fault红 / Unknown灰 |
| A176 | 5/5 完成 Gate | 三项全绿前确定 disabled 且隐藏完成提示；全绿后显示固定完成提示并启用确定；不自动跳转；仍可返回第4屏 |
| A177 | G3 强调色 / 按钮 | 普通 primary、普通 selected 与主要交互统一蓝色 `#0071e3`，黑色不得作为普通 accent；管理员模式黄色/琥珀；按钮一致可辨识，禁止白底白按钮 |
| A178 | G3 提示 / 状态来源 | 最小必要，不重复堆叠自解释说明；复制/状态反馈不位移；Dashboard 与 onboarding 不得各维护冲突服务状态 |
| A179 | 固定窗口尺寸 | inner/minimum/maximum size 均为 900×620 |
| A180 | 禁止缩放 | `resizable=false`，拖拽边框不能改变窗口尺寸 |
| A181 | 禁止最大化 | `maximizable=false`，最大化入口不可用；Dashboard/onboarding 在固定 client area 内完整可操作 |
| A182 | 单层窗口 chrome | native `decorations=false`；自定义 chrome edge-to-edge 覆盖 client area，不存在双边框 |
| A183 | 自定义窗口控制 | 有拖拽区、最小化、关闭，无最大化 |
| A184 | onboarding 整页布局 | 5 屏直接使用 custom chrome 内容区；不得用居中 floating card/modal/dialog 或大圆角+整体阴影/边框制造“窗口里的窗口” |
