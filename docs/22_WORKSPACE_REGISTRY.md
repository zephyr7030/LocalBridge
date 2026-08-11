# 22 — Project Registry & Active Workspace

## 用户概念

界面统一使用“项目”。

内部可以使用：

```text
WorkspaceRegistry
WorkspaceEntry
active_workspace
candidate_workspace
```

## 核心安全模型

LocalBridge 可以记住多个项目，但**同一时间只有一个项目是实际授权根目录**。

```text
项目列表
├─ 项目 A
├─ 项目 B  ← 当前项目 / 唯一授权根
└─ 项目 C
```

重要：

```text
项目列表 ≠ 同时授权列表
```

MCP / PEP / coding runtime 只能访问：

```text
active_workspace
```

不得因为项目出现在“最近/已保存项目”列表中就获得访问权限。

## WorkspaceEntry

建议持久化：

```text
workspace_id
display_path
validated_identity
last_opened_at
```

可选：

```text
display_name
```

不得保存 secret。

`validated_identity` 必须由 WorkspaceValidator 产生，并复用 LB-000/LB-010 最终确定的 Windows reparse/junction/canonical-root 规则。

禁止仅依赖：

```text
path.to_lowercase()
```

进行安全身份判断。

## 新增项目

用户选择目录：

```text
folder picker
→ validate candidate
→ resolve safe workspace identity
→ de-duplicate
→ add/update registry entry
→ switch using two-phase workspace transition
→ Ready
→ commit active
```

如果目录已存在于项目列表：

- 不创建重复项；
- 更新显示路径/最近使用时间（如合理）；
- 直接按选择流程切换。

新增失败：

- 不修改 active workspace；
- 不留下半完成 registry entry；
- 返回可理解错误。

## 选择项目

选择已有项目必须继续遵守两阶段切换：

```text
candidate
→ validate
→ Tunnel ↓
→ PEP ↓
→ MCP ↓
→ MCP(candidate) ↑
→ PEP(candidate) ↑
→ Tunnel(candidate) ↑
→ Ready
→ commit active
```

切换失败：

- candidate 不成为 active；
- 原 active 元数据保留；
- 可以显式回滚/恢复原项目；
- 项目列表不因切换失败损坏。

## 移除非当前项目

允许直接移除 registry entry。

语义：

```text
remove from LocalBridge only
```

绝不：

- 删除目录；
- 删除代码；
- 移动文件；
- 清空项目；
- 调用递归删除。

因为只是可恢复的元数据移除，默认不需要额外确认弹窗。

## 移除当前项目

允许，但必须是显式用户动作。

流程：

```text
user chooses remove current project
→ one concise confirmation
→ clearly state "不会删除项目文件"
→ stop Tunnel
→ stop PEP
→ stop MCP
→ clear active_workspace
→ remove registry entry
→ enter NoActiveWorkspace
```

确认只需要一次，不增加额外解释页。

确认文案示例：

```text
从 LocalBridge 移除此项目？
不会删除项目文件。
```

按钮：

```text
取消
移除
```

移除完成后：

- LocalBridge 保持运行；
- runtime 服务保持停止；
- 主控界面显示 `选择项目`；
- 不将“没有当前项目”当成故障；
- 不自动选择其他项目，避免隐式授权。

## NoActiveWorkspace

这是正常产品状态，不是 Fault。

允许：

```text
App running
Tray running
Settings available
Diagnostics available
Project picker available
```

禁止启动：

```text
coding-tools-mcp
PEP
tunnel-client for workspace access
```

选择新项目后再进入正常启动链。

## 目录失效

项目列表中的非当前项目如果目录不存在：

- 列表可显示不可用状态或在使用时提示；
- 可以直接移除记录；
- 不需要当作系统故障。

当前项目在启动时不存在：

- 不自动授权其他项目；
- runtime 不启动；
- UI 提供重新选择/移除；
- 保留记录直到用户决定。

## 去重

同一真实授权根目录只能有一个 entry。

去重依据必须与安全边界一致：

```text
WorkspaceValidator identity
```

而不是纯字符串比较。

处理：

- Windows 路径大小写；
- `.` / `..`；
- separator differences；
- junction/symlink/reparse aliases；
- canonical/final-path semantics。

具体算法由 LB-000/LB-010 实证决定。

## UI

项目选择尽量极简。

主控界面：

```text
D:\project\LocalBridge   [切换]
```

项目选择器中：

- 已保存项目；
- `选择其他文件夹`；
- 非当前项目可 `移除`；
- 当前项目可 `移除`，但触发一次明确确认。

不要增加独立“项目管理中心”。

## 项目列表容量

v0.1 不需要人为限制很小的固定数量。

实现必须：

- 去重；
- 排序稳定；
- 最近使用优先可接受；
- 数据结构支持未来清理策略。

不要自动删除仍然有效的用户项目记录。

## Control-plane

MCP 无权：

- 新增项目；
- 移除项目；
- 切换 active workspace；
- 修改 registry；
- 修改 authorized root。

这些动作只能由 LocalBridge 用户控制面执行。
