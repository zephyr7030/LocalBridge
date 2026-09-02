# 源码审查修复与验证记录

日期：2026-09-03。基线：`37fd0b1`，产品版本保持 `0.1.4`。

本轮仅本地修复、验证和提交，不推送 GitHub，不发布 Release。
下表中的测试均使用真实行为断言；残留扫描只作补充，不替代行为验证。

## 审查项与修复边界

| 审查项 | 根因修复 | 对应验证 |
| --- | --- | --- |
| LB-R-001 | PEP 每次启动生成独立 256-bit 系统随机 Bearer；每次 HTTP 请求先认证，再创建或访问 Session。受监督 Tunnel 经环境变量传递完整 Bearer 认证头，命令行不含秘密。 | 未认证 initialize 返回 401；精确匹配和脱敏单测；真实 bundled Tunnel 发往本机探针的认证头验证；所有外部黑盒调用经过认证后的真实 HTTP/MCP 入口。 |
| LB-R-002 | RequestKey 和取消目标在排队前注册；Scheduler ticket 绑定 RequestKey；关闭调度器会原子取消队列并拒绝迟到 admission。 | 同号请求跨 Session 隔离；真实队列取消且副作用文件不存在；真实停止 PEP 时队列请求取消且副作用文件不存在。 |
| LB-R-003 | 公开身份改为系统 CSPRNG；ID 不再授予权限。命令 adopt 要求 orphan 和独立 token，并轮换 token。get/cancel/read 校验 owner。Execution 创建时即写入 owner，删除事后绑定 writer。 | active-owner/wrong-token/replayed-token 拒绝；跨 Session 读取和取消拒绝；断开后的显式 adopt；真实 phased workflow 的执行、断连、凭证转移、取消和 terminal。 |
| LB-R-004 | DOCX edit 定点改写原 ZIP 的 document.xml，其他 parts 原样复制；保留未修改 XML、关系和包注释。不能确定保真的目标拒绝修改，不再用简化 IR 覆盖整个原文档。 | 富文本、超链接、关系及自定义元数据 fixture；合法局部替换后字节比对；不支持的目标拒绝后 SHA 不变；使用端黑盒复核。 |
| LB-R-005 | 采用审查允许的最小安全边界：公共 Patch 限制为一个文件操作，拒绝嵌入 move。移除伪多文件事务及进程内 best-effort rollback。单文件继续使用 hash 条件检查和原子替换。 | 多文件 Patch 返回 InvalidArgument，两个文件均保持原文；单文件 hash/并发写/硬链接/路径交换测试。**未实现多文件崩溃恢复，不宣称支持多文件原子事务。** |
| LB-R-006 | 权限降级先关闭高权限执行面，再持久化；保存失败独立发布故障，不得跳过 Broker 清理。 | 注入设置保存失败，验证清理仍被调用且先于保存；权限 gate 和快照一致性由既有 Authority/控制面测试覆盖。未将该顺序单测宣称为真实 UAC 验证。 |
| LB-R-007 | workspace_context 读取控制面状态及启动发现数据，不等待 Work 持有的 Runtime facade 锁。 | 长命令占据 Work 时，真实 workspace_context 在 1 秒内返回，并观察到 foreground_work_running=1。 |
| LB-R-008 | Git status 使用 porcelain v1 `-z`，按 NUL record 解析路径及 rename pair，严格解码 UTF-8。 | 真实 Git 仓库中的 Unicode、包含 ` -> ` 的路径和 rename。 |
| LB-R-009 | 前端独立维护 transport freshness；读取/解析失败即清除当前成功投影、展示 unavailable，并独立退避重试。 | 投影成功→失败→恢复单测；失败后不能继续保留旧在线/权限快照。 |
| LB-R-010 | Onboarding 和主界面共用 ready-only section 访问规则，faulted section 的 previous 不再作为当前事实。 | faulted settings/connection 带 previous fixture，断言不显示已完成、已存 Key 或旧 Tunnel ID。 |
| LB-R-011 | parseUiError 保存完整 envelope；命令错误不再定时消失；主界面和诊断共用可展开详情组件。诊断读取与操作错误分开，刷新不能清掉操作失败。 | 完整字段解析与渲染测试，包括 operation/session/request/task ID、category、retryable。 |
| LB-R-012 | 删除 SessionRecord.owned_tasks 及 writer。任务 ownership 查询回到 TaskRegistry/Workflow owner。 | Session lease、TTL/reaper 与大量任务相关测试；SessionRecord 无重复集合和 writer，列表从 TaskRegistry 查询。 |
| LB-R-013 | exec_command 公共合同集中到 public_contract；deny_unknown_fields 类型解析；公开 dry_run/verbosity；显式 null stdin 拒绝。 | schema/字段解析一致性测试；未知字段拒绝；dry_run 无副作用；使用端真实响应按 tools/list outputSchema 校验。 |
| LB-R-014 | Windows bounded process 使用 STARTUPINFOEXW 和显式 HANDLE_LIST；属性表及 handle 数组均持有至 CreateProcess 完成。 | 子进程仍存活时，父进程关闭无关管道写端后立即得到 EOF，证明子进程未继承无关句柄。 |
| LB-R-015 | 启用最小生产 CSP，限制脚本、连接、图片、字体、嵌入对象及表单。 | 生产静态资源的真实 WebView2 测试：页面加载、IPC 往返、未授权 inline script 被阻止；关闭销毁并重新创建 WebView。 |
| LB-R-016 | 拆出 public_contract、client_auth、observation、resource_access；队列取消交给 Scheduler；删除执行晚绑定 owner。测试客户端直接校验真实公开响应，不再只检查函数名或正则。 | 跨模块 owner/取消/输出 schema 黑盒；源码依赖方向和残留扫描。facade/server 仍较大，本轮未将机械减行数冒充架构验收。 |

## 验证过程中额外发现并修复

- common envelope 的 output_refs 原先可能复制成 stream map；现在公共层始终为列表，命令 data 保留按流索引的 map。
- stderr 被选为主 output_ref 时原先会先登记为 stdout；终态重读因此可能丢失 stderr handle。现在按真实流登记，并验证缓存终态后仍可读取 stderr。
- 按 CI 固定 Rust 1.85.0 复测，删除新代码中超出最低编译器版本的 let-chain 语法。
- 旧窗口 E2E 只写磁盘设置，没有发布后端快照；测试初始化现复用 SettingsProjection 发布入口，不绕过前端合同。
- 前端测试发现规则原先遗漏 `.test.tsx`；现同时收集 `.ts` 和 `.tsx`，避免新增组件测试被静默跳过。
- Tunnel 额外认证头的环境变量必须包含 `Bearer ` 前缀；已用真实 bundled 二进制对本机探针验证，不仅检查参数名称。
- 测试结构检查不再把 terminal driver 的调用次数写死为 5；新增场景继续复用 driver，由其行为测试校验等待/终态规则。

## 测试基座

入口仍在 `tests/black-box/chatgpt`，未增加任何生产工具、调试 HTTP API 或权限旁路。

- `client.mjs`：真实 initialize、Session headers、tools/list、tools/call、cancel 和 close。
- `schema_contract.mjs`：校验实际 outputSchema，不补字段、不改响应，不把缺失缓存 schema 当成缺失能力。
- `command_lifecycle.mjs`：复用唯一 terminal driver。
- `revision46.mjs`：原用户复现和本轮安全/数据完整性回归。
- `shutdown_queue.mjs`：真实排队后与 Rust fixture 协调停止服务。
- `tests/support/document_fixtures.rs`：单测和使用端黑盒共用富 DOCX 样本。
- `scripts/test/ci-gate.mjs`：本地和 CI 使用同一声明式门禁，固定 Rust 1.85.0。

## 验证记录

最终磁盘源码的本地验证结果：

| 门禁 | 结果 |
| --- | --- |
| Rust 1.85.0 全量 | 465 passed，0 failed，4 ignored；其中 3 项是进程测试辅助入口，1 项需要真实 UAC 授权。 |
| 使用端黑盒 | 2 项通过：完整用户复现场景，以及真实服务关闭时取消排队请求。包含真实 outputSchema 校验、DOCX 包保真、phased workflow 接管/取消、retained stderr 终态重读。 |
| Clippy | `--all-targets -- -D warnings` 通过。 |
| 前端 | 18 项通过，TypeScript/Vite 生产构建通过。 |
| Node 测试基座 | 14 项通过。 |
| WebView2 运行验证 | 引导页、主界面两个视图通过；生产静态资源加载、真实 IPC、CSP inline-script 拒绝、关闭销毁/重新创建窗口均验证。 |
| 其他门禁 | 格式、公开源码导出、许可证、Schema44 残留扫描、Runtime 精确哈希、UI 文案检查通过。 |
| NSIS | `LocalBridge_0.1.4_x64-setup.exe` 本地打包通过，未上传。 |

复测入口（本轮使用同一门禁分段运行，未改写/跳过其阶段）：

```text
node scripts/test/ci-gate.mjs --through frontend-build
node scripts/test/ci-gate.mjs --from rust-test --through rust-clippy
node scripts/test/ci-gate.mjs --from rust-test
node tests/e2e/onboarding/fixed_window_runtime_e2e.mjs --production-assets
npm run verify:runtime-manifest
npm run verify:ui-language
```

本地安装包：`src-tauri/target/release/bundle/nsis/LocalBridge_0.1.4_x64-setup.exe`。
大小：22,312,146 bytes。
SHA-256：`7b368837ce692345e3d7dfa4b8d202c9f7c573b5743681a3c261c6ffcd81e367`。

验证期间出现的旧文案断言、固定调用次数断言、MSRV、stderr 归属和认证头问题均先修复，再复测；没有把曾经通过的旧树结果当作最终树通过。

## 明确未覆盖的外部边界

- 黑盒运行于真实本机 PEP + bundled Runtime，不代表 OpenAI 云端 Tunnel 已验证；报告固定标记 `NOT_RUN_LOCAL_PEP_ONLY`。
- 没有自动弹出真实 UAC。显式授权型 Broker 测试仍保持 ignored；权限失败与投影一致性由现有注入/集成测试验证。
- 多文件 Patch 现为不支持，调用者不能用连续单文件调用获得跨文件原子保证。
- 当前 workflow 重连使用其独立持久凭证；命令 orphan adoption 凭证是一次性且会轮换。两者不能仅凭 task_id/session_id 接管。
