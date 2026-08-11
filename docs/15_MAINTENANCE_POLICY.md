# 15 — Maintenance Policy

## 目标

LocalBridge 必须能在上游变化、依赖升级、配置演进和长期迭代下保持可维护，而不是依赖开发者记忆维持正确性。

## 核心原则

1. 架构不变量优先变成机器可执行检查。
2. 上游升级必须有工具面/能力面差异报告。
3. 持久化状态必须版本化、可迁移、可回滚。
4. Release 必须可追溯到完整依赖和源码版本。
5. 长期兼容规则写入文档与测试，不依赖口头约定。
6. 诊断信息围绕一次 runtime generation 关联，而不是堆积无上下文日志。
7. 任何“临时兼容层”必须有退出条件。

## 不允许

- 组件直接依赖上游私有实现细节；
- 新增跨层 shortcut；
- 依赖 README 代替 machine-readable contract；
- 更新上游后不重跑兼容/安全测试；
- 修改 settings schema 却不提供迁移；
- 通过删除旧数据来“解决”迁移问题；
- release artifact 无法追溯到 exact commit / checksum。

## Stable Adapter 原则

LocalBridge 依赖：

```text
LocalBridge domain
        ↓
stable adapter contract
        ↓
external runtime
```

禁止：

```text
LocalBridge domain
        ↓
external runtime internal details
```

任何外部 runtime 必须可替换，而不要求重写 UI、settings、tray、credential、orchestrator。

## Current Task Is Ephemeral

`CurrentTaskStatus` 是运行态投影，不是持久化业务数据。

v0.1 禁止新增：

- task history database；
- activity feed store；
- recent tool call persistence。

未来若要执行审计历史，必须单独设计 retention / redaction / privacy / storage / UI。
