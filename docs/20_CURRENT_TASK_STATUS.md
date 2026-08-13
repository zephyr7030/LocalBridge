# 20 — Current Task Status Policy

## 目标

主控界面只显示一个极简、单行的当前执行状态。

不显示：

- “当前任务”标题；
- “类型 / 任务 / 状态”字段名；
- 卡片标题；
- 最近活动；
- 消息流；
- 历史列表；
- 时间线。

## 执行中

推荐唯一形态：

```text
●  运行测试  cargo test
```

其中：

- `●` 为绿色活动指示器；
- `运行测试` 为稳定中文工具类别；
- `cargo test` 为经过安全清洗的任务摘要；
- 整行不再额外显示“执行中”。

## 活动动效

执行中绿色指示器使用轻量脉冲动效：

```text
green dot
→ opacity / scale pulse
→ repeat while active
```

要求：

- 动效低干扰；
- 不闪烁；
- 不快速跳动；
- 不使用大面积动画；
- 不推动布局；
- 不引起文字位移；
- 推荐周期约 1.2–2.0 秒；
- 尊重系统 `prefers-reduced-motion`，减少动态时改为静态绿色点。

禁止：

- loading spinner 占据大面积区域；
- 进度条（没有真实进度时）；
- 跑马灯；
- 呼吸背景；
- 彩色渐变；
- 连续消息动画。

## 等待命令

无任务时使用低存在感状态：

```text
○  等待命令
```

固定要求：

- 中性灰色静态点；
- 无动效；
- 不显示上一条已完成任务；
- 该行始终可见，不允许隐藏；
- 禁止使用“空闲”文案。

`Idle` 只是 backend/domain 状态名；用户可见映射固定为 `等待命令`。

## 其他状态

仍保持单行，不新增标题字段。

等待管理员授权：

```text
●  管理员操作  安装设备驱动 · 等待授权
```

被策略阻止：

```text
●  管理员操作  安装设备驱动 · 已阻止
```

执行失败：

```text
●  运行测试  cargo test · 执行失败
```

已取消：

```text
●  执行命令  cargo build · 已取消
```

终态只允许短暂呈现，随后回到 `等待命令`；不得追加到历史。

视觉语义：

- 执行中：绿色活动点 + 脉冲；
- 等待授权：中性/警示状态，不伪装成正在执行；
- 已阻止/执行失败：错误状态；
- 等待命令：灰色静态点。

具体非活动颜色由 UI token 决定，不在业务逻辑中硬编码。

## 工具类别

稳定内部分类：

```text
ReadFile
SearchCode
ModifyFile
ExecuteCommand
GitOperation
Build
Test
ElevatedOperation
Other
```

中文显示：

```text
读取文件
搜索代码
修改文件
执行命令
Git 操作
构建
运行测试
管理员操作
其他操作
```

示例：

```text
●  读取文件  src-tauri/src/runtime/mod.rs
●  搜索代码  RuntimeState
●  修改文件  src/lib/presentation/status.ts
●  Git 操作  git status
●  构建  cargo build
●  运行测试  cargo test
```

不得直接显示：

```text
mcp__coding_tools__execute_command
tools/call
apply_patch
```

## 数据来源

必须来自真实 MCP / Broker 执行事件：

```text
tools/call / Broker request
→ capability classification
→ safe task summary
→ execution state
→ CurrentTaskStatus
```

`CurrentTaskStatus` 与 Idle/Running/Waiting/Blocked/terminal truth 由 backend 独占维护；React/WebView 只能消费 typed projection，不得维护平行任务状态、根据本地按钮/定时器伪造执行状态或用模型文字推测状态。真实生产 MCP/Broker 调用必须端到端驱动该投影，terminal 后回到 `等待命令`。

禁止：

- 根据模型文本猜测；
- 展示模型隐藏思考；
- 镜像 ChatGPT 最终自然语言回答；
- 将计划步骤显示为“正在执行”。

## 任务摘要

摘要必须脱敏并限制长度。

禁止回显：

- Runtime API Key；
- Authorization header；
- bearer token；
- Broker nonce/session secret；
- credential；
- 含 secret 的完整命令行；
- 超长 raw 参数；
- 文件正文/二进制内容。

无法安全生成时：

```text
●  执行操作
```

## 单状态投影

v0.1 只显示一个 `CurrentTaskStatus`。

即使未来底层支持并发，也不得通过最近活动或消息流解决 UI 展示。

## 持久化

当前状态是临时运行态，不建立任务历史数据库。

诊断日志独立存在，继续遵守本地保存、限额、脱敏、主动导出、零遥测。
