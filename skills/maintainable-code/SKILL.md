# Skill — Maintainable Implementation

## 目的

交付当前需求的同时，保证代码可以继续维护和迭代；禁止短期 Patch 型实现成为正式代码。

## 执行规则

1. 最小必要代码必须是“最小完整实现”，不是临时 workaround。
2. 新代码必须具有清晰单一职责和明确模块边界。
3. 跨层交互通过稳定接口，不绕过 domain/runtime boundary。
4. 常量集中管理；状态有 typed model；错误有明确语义。
5. 关键行为必须可自动化测试。
6. 上游依赖通过 adapter/manifest/version contract 隔离。
7. 每次实现必须考虑下一次合理迭代是否需要推翻当前结构；若会，先调整结构再提交。

## Patch 规则

只有在外部缺陷或临时兼容性限制无法立即消除时允许 workaround，并必须同时提供：

- 原因；
- 影响范围；
- 退出条件；
- 对应 issue/ADR；
- 防止扩散的边界。

禁止以 TODO 代替这些要求。
