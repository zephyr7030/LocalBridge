# Skill — Sustainable Maintenance Discipline

## 目标

让实现不仅在当前 PR 正确，而且可升级、可替换、可诊断、可迁移。

## 每次改动检查

1. 是否破坏 stable adapter？
2. 是否新增持久化字段？若是，schema/migration 是否完整？
3. 是否改变外部 runtime surface？若是，compatibility baseline 是否更新？
4. 是否新增不可执行的“文档规则”？能否转成自动检查？
5. 是否影响 release provenance / SBOM / notices？
6. 是否引入未来必须推翻的临时结构？
7. 是否能用 contract/adversarial test 固化行为？

## 禁止

- 上游细节直接进入 domain；
- schema silent reset；
- upgrade without diff；
- release without provenance；
- temporary workaround without exit condition。
