# Skill — User-facing Language Discipline

## 目标

所有用户可见 UI 使用自然、简洁、专业的中文，不暴露无必要英文或内部工程术语。

## 规则

1. 普通 UI 默认中文。
2. MCP / API / UAC / OpenAI / ChatGPT 等标准缩写或专有名词可保留。
3. Dashboard / Settings / Diagnostics / Edit / Full / Elevated / Broker / Runtime 等不得直接展示。
4. 内部 enum、错误码、进程名不能原样显示给普通用户。
5. UI 文案通过统一 presentation/i18n 层生成。
6. 错误提示必须中文、短、可行动。
7. 不做中英双语堆叠，例如 `管理员模式 (Elevated)`。
8. 新增英文 UI 文案必须证明无法自然中文化，否则拒绝。

## 冻结术语

- Dashboard → 主控界面
- Settings → 设置
- Diagnostics → 诊断
- Edit → 编辑模式
- Full → 完整模式
- Elevated → 管理员模式
- Broker → 管理员代理
- Coding Runtime → 编码服务
- Tunnel → 安全隧道
- Workspace → 项目目录
- Ready → 已就绪
- Faulted → 故障

## 当前任务

- Current Task → 当前任务
- Running → 执行中
- Idle → 空闲
- Blocked → 已阻止
- Awaiting Permission → 等待授权

工具内部名称不得直接显示给用户。
