# 13 — Distribution & Runtime Policy

## 冻结目标平台

v0.1.0 只支持：

```text
Windows 11 x64
```

明确不支持：

- Windows 10；
- Windows ARM64；
- Windows Server；
- Wine；
- Linux/macOS。

后续平台扩展必须独立立项，不允许在 v0.1 PR 中顺手增加兼容层。

---

# 安装包模型

LocalBridge v0.1.0 采用完整 Runtime Bundle。

安装范围固定为 Windows 机器级 `perMachine`，正式文件位于 Program Files 安装根。Privileged Broker 的 UAC 目标必须绑定为当前 LocalBridge 可执行文件的 canonical 同目录 sibling；在 `runas` 前还必须验证 Broker/安装目录/Program Files 祖先链的实际 owner+DACL 不向非受信任主体授予 mutation 权限，并用当前未提权 token 逐项确认危险写入、删除、子项创建/删除、DACL/owner 修改权限全部明确 `ACCESS_DENIED`。因此路径字符串在 Program Files 下并不足够：权限宽松的子目录仍必须拒绝。不得从 `%LOCALAPPDATA%`、临时目录或其他普通用户可写位置提权启动 Broker。v0.1 不声称 Broker 使用 Authenticode 或文件哈希绑定。

安装包必须包含：

```text
LocalBridge.exe
LocalBridge-Privileged-Broker.exe

runtime/
├─ python/
│  └─ Windows Embeddable Python 3.12.x
├─ coding-tools-mcp/
│  └─ pinned upstream source/dependencies
└─ tunnel-client/
   └─ pinned official Windows x64 binary
```

用户不需要：

- 系统 Python；
- pip；
- venv；
- Node.js；
- Rust；
- Docker；
- 手动下载 tunnel-client。

产品文案应准确描述为：

> LocalBridge 无外部 Python 依赖。

不要描述成：

> LocalBridge 不包含 Python。

因为内部仍使用私有、固定版本的 Python Embedded Runtime。

---

# Python 决策

v0.1 保留 Python。

原因：

1. `coding-tools-mcp` 本身是成熟 Python Runtime；
2. 重写为 Rust 会把项目从“可靠桌面壳”扩大成“自研 Coding Runtime”；
3. Python Embedded 体积相对有限；
4. vendor 后用户完全感知不到系统 Python；
5. 上游升级成本显著低于自行移植。

禁止：

- 为追求十几 MB 的体积优化重写 coding-tools-mcp；
- 先使用系统 Python 做正式实现；
- 依赖用户 PATH；
- 运行时 `pip install`；
- 首次启动联网下载 Python。

LB-006 必须使用最终 Embedded Python 模型。

---

# WebView2

Windows 11 自带 WebView2 Runtime。

因此：

```text
WebView2 = system dependency
```

安装包不携带：

- WebView2 Offline Installer；
- Fixed Version WebView2 Runtime。

原因：

- 会显著增加安装包体积；
- Windows 11 是唯一目标平台；
- 不属于 LocalBridge 自有 Runtime。

若系统 WebView2 损坏：

- diagnostics 给出明确错误；
- 引导用户修复系统 WebView2；
- 不在应用内静默下载大型 WebView2 Runtime。

---

# 第三方 Runtime 升级

禁止独立自动更新：

```text
coding-tools-mcp
tunnel-client
Python Embedded
Privileged Broker
```

版本关系：

```text
LocalBridge vX.Y.Z
        │
        └── exact Runtime Manifest
```

一个 LocalBridge Release 对应唯一 Runtime Manifest。

升级方式：

```text
发布新的 LocalBridge installer
→ 新 installer 携带新 runtime
→ compatibility + security suite
→ 整包升级
```

禁止：

- 启动时自动检查 coding-tools-mcp main；
- 自动下载 tunnel-client 新版；
- 自动更新 Python patch；
- runtime 独立漂移。

---

# v0.1 更新策略

v0.1 不实现自动 updater。

允许：

- About/Settings 显示当前版本；
- 可选“查看发布页”按钮。

不实现：

- 后台自动检查更新；
- 差分更新；
- 静默更新；
- 自更新服务；
- runtime hot swap。

如果加入“查看发布页”，必须遵循最小 UI 原则；不是 v0.1 必需功能。

---

# 零遥测

v0.1：

```text
Telemetry       OFF
Analytics       OFF
Crash Upload    OFF
Usage Metrics   OFF
```

只保留本地：

```text
logs
diagnostics
runtime status
```

诊断信息只有用户主动导出。

默认日志：

```text
5 MiB × 5 files
```

具体轮换实现可根据 logging crate 调整，但总量应有硬上限。

日志必须 redaction：

- Runtime API Key；
- MCP bearer；
- Authorization；
- Broker nonce/session secret；
- credential material。

禁止任何后台 telemetry endpoint。

---

# 体积目标

非硬性 release gate，但作为工程预算：

```text
Installer target: 45–70 MiB
Installed target: 90–150 MiB
```

如果明显超过：

```text
Installer > 100 MiB
Installed > 250 MiB
```

必须在 LB-018 输出体积归因报告。

不得为了压缩体积破坏：

- runtime pinning；
- offline readiness；
- debug/diagnostics；
- security boundary。

---

# generic reviewed elevated_exec

Elevated 模式正式支持：

```text
elevated_exec
```

它是最高风险 capability。“generic”只表示统一的 structured Broker gateway，并不表示允许任意管理员 executable/shell。

合同：

- Edit：deny；
- Full：deny；
- Elevated + Broker Active：allow if reviewed；
- policy decision 必须消费真实 `program / args / workdir`；
- executable identity 必须命中受信任系统位置和精确 reviewed action profile；
- arbitrary program、shell/interpreter、同名伪造 executable、未审核参数/workdir：deny；
- LocalBridge control-plane：仍 deny；
- 通过 `elevated_exec` 间接修改 LocalBridge settings/workspace/runtime policy/credential/tunnel 等 control-plane 资源：deny-always；
- UAC 必须由用户显式启用 Broker；
- Broker 不使用 `shell=true` 作为默认；
- structured command + args；
- explicit workdir；
- exit code/stdout/stderr；
- output size limit；
- timeout/cancellation；
- secret redaction；
- audit metadata；
- 不记录敏感参数明文。

`elevated_exec` 的存在不意味着管理员模式绕过 PEP。

v0.1 初始 reviewed profile 仅为受信任 Windows System32 `whoami.exe` 的窄只读身份查询；它用于证明 structured Broker 执行链，而不是提供管理员 shell。新增管理员动作必须逐项 review 并添加 negative/adversarial tests。
