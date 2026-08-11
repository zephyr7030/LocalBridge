# 21 — Credential Handling

## 目标

Runtime API Key 等敏感凭据不得以明文形式持久化、回显、记录或通过不安全参数传递。

## 凭据范围

至少包括：

```text
Runtime API Key
MCP bearer / Authorization material
Broker session secret / nonce
未来新增的任何外部服务 credential
```

## At-rest 规则

正式实现必须使用 Windows 安全凭据后端。

优先实现：

```text
Windows Credential Manager
```

允许在 LB-005 依据 Windows API 可维护性选择等价的 DPAPI-backed secure storage abstraction，但必须满足：

- secret 不出现在普通 JSON/TOML；
- secret 不出现在 SQLite/纯文本文件；
- secret 不出现在日志；
- secret 不进入 crash/diagnostics 明文；
- secret 与当前 Windows 用户安全上下文绑定；
- credential 读取失败 fail-closed。

禁止任何明文 fallback：

```text
settings.json
config.toml
.env
localStorage
sessionStorage
registry plaintext value
workspace metadata
```

## 普通设置只保存引用

允许普通配置保存：

```text
credential_id
credential_backend
credential_backend_version
has_runtime_key = true
```

禁止保存：

```text
runtime_api_key = "..."
```

## UI

运行密钥输入框：

- 输入时使用密码字段；
- 保存后不再回显原值；
- 只显示 `已保存`；
- 若用户选择替换，重新输入新值；
- 不提供“显示完整密钥”按钮；
- 不把 secret 放入前端持久化存储。

React 只在提交动作所需的最短生命周期内持有输入值。

## 运行时注入

LB-000 必须验证 `tunnel-client` 实际支持的安全注入机制。

优先级：

```text
1. stdin / inherited handle / dedicated secure channel
2. process environment（仅在上游实际要求且风险可接受时）
3. CLI argument = 禁止
```

若上游只支持把 secret 放进 CLI：

```text
SecretInjectionUnsupported
```

不得为了“能启动”破坏凭据边界。

若最终必须使用环境变量：

- 只注入目标子进程；
- 不写系统/用户环境变量；
- 不持久化；
- 不记录；
- 不传给无关子进程；
- diagnostics redaction；
- 明确记录为上游兼容性约束。

## 生命周期

```text
CredentialStore.get()
→ in-memory secret
→ launch/auth use
→ zero/drop best-effort
```

不得在应用状态树、UI event history 或可序列化 snapshot 中长期保存 secret。

## 删除

用户重置/删除运行密钥时：

```text
CredentialStore.delete()
→ verify absent
→ UI has_runtime_key=false
```

删除普通 settings 文件不能意外把凭据复制出来。

卸载策略由 LB-018 决定是否保留用户凭据，但无论保留/删除都必须显式且可测试。

## 日志与诊断

必须脱敏：

```text
Authorization: ***
Runtime API Key: ***
Bearer: ***
Broker secret: ***
```

禁止“只隐藏中间几位”的半明文策略。

## 测试

至少：

- settings 文件扫描无 secret；
- 日志扫描无 secret；
- diagnostics 导出无 secret；
- CLI/process arguments 无 secret；
- credential backend round-trip；
- credential delete；
- wrong-user / inaccessible credential fail-closed；
- app restart 后可从 secure backend 恢复；
- frontend 不持久化 secret。
