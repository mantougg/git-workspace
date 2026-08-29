---
name: gitworkspace-backend-dev
description: GitWorkspace 后端模块拆分任务流程：如何读 docs/tasks-backend/ 文档（总索引/全局约束/任务spec）开始与继续后端重构（B-XX）任务开发、并同步进度。
---

# GitWorkspace 后端模块拆分任务开发流程

本 skill 教你在 **GitWorkspace** 项目中，如何基于 `docs/tasks-backend/` 的任务文档**开始开发**或**继续开发**某个后端模块拆分（B-XX）任务。

这套任务是**纯后端重构**（设计来源：`docs/backend-module-split-plan.md`）：把超长文件拆成 `xxx/mod.rs` + 子模块，**不改变** IPC 契约、任务语义、数据库结构、事件名和跨平台行为。它与功能任务（T-XX/R-XX/D-XX/F-XX/AI-XX）的区别是：**验收靠回归，不靠新功能**。

## 文档地图

| 文件 | 作用 | 何时读 |
|---|---|---|
| `docs/backend-module-split-plan.md` | 设计文档：拆分方案与规则的**单一事实来源**（§ 号被各 spec 引用） | 对目标结构/迁移顺序有疑问、spec 与它冲突时 |
| `docs/tasks-backend/README.md` | 总索引：10 个任务的阶段/优先级/状态/依赖总表 + 依赖链 + 维护规范 | 选任务、核对状态、同步进度时 |
| `docs/tasks-backend/00-全局开发约束.md` | 重构横切硬约束（不变式 / 迁移方式 / 小步验证 / GitNexus 流程 / DB 并发 / 跨平台 / 克制项 / 文档同步） | 任何 B-XX 任务开发前**必读** |
| `docs/tasks-backend/B-XX-*.md` | 任务 spec：目标 / 需求范围 / 架构性能注意点 / 验收标准 / 进度 | 开发目标任务时 |

## 关键边界（贯穿所有 B-XX 任务）

1. **不变式**：IPC 契约、事件名/payload、DB schema、配置文件格式、任务语义、跨平台行为、公共路径 re-export——拆分前后一致；必须破坏才能拆时，停下来改设计文档。
2. **小步移动**：一次只移一个职责组，移完立即跑四件套（`cargo fmt --check` / `check` / `test` / `clippy -D warnings`，均带 `--manifest-path src-tauri/Cargo.toml`）。
3. **可见性克制**：子模块间用 `pub(super)`；不为拆分或测试把字段/函数改 `pub`；测试放同父模块 `tests.rs`。
4. **GitNexus 必跑**：移动公共符号前 `impact`（HIGH/CRITICAL 必须提示用户）；改名用 `rename` 不用 find-and-replace；提交前 `detect_changes()`。
5. **回滚粒度 = 一个职责组**，不做整个后端大回滚。

## 任务地图速查

- **B-01（唯一硬前置）**：基线固定 + 测试外移（含 ipc_golden 按领域拆）。
- **B-02 / B-03**（Phase 1/2，P0）：RuntimeService、RuntimeProcessManager——风险最高，严格按 spec 内的迁移顺序。
- **B-04 / B-05**（Phase 3，P1）：Maven Index、Log Engine。
- **B-06 ~ B-09**（Phase 4，P1，可并行）：Config、Watch、GitOps、Operation Log。
- **B-10**（Phase 5，P2，**条件触发**）：无触发条件不启动，触发条件见 spec。

## 开始开发一个新任务

1. 确定任务编号（用户指定，或从 README 总表选「依赖均已就绪」的任务；B-01 未完成时不开始任何其他 B 任务）。
2. 读 `README.md` 总表，确认状态、优先级、依赖；读 `00-全局开发约束.md`（必读）。
3. 读目标任务文档顶部「**开发前必读**」与设计文档对应章节——**只读这几份**。
4. 通读目标任务文档：目标、迁移顺序、验收标准。
5. 状态 `⬜ → 🟦`（**同步**更新 README 总表 + 任务文档「进度」），时间线追加「开始开发」。
6. 对要移动的公共符号跑 GitNexus `impact`，向用户报告 blast radius 后开始。

## 继续开发（恢复一个进行中的任务）

1. 读目标任务文档「**进度**」：状态 + 时间线最后一条 + 子任务勾选情况。
2. 核对 README 总表状态一致（不一致以任务文档为准并修正 README）。
3. 从时间线最后一条恢复上下文，继续未勾选子任务；每个职责组移动后跑四件套。

## 完成一个任务

1. 逐条核对「验收标准」**全部满足**；重点核对：测试数量不减少、生产可见性无扩大、`cfg(windows)` 分支保留、公共路径兼容。
2. 跑四件套 + `detect_changes()`，确认影响范围不超预期。
3. 若被根 `AGENTS.md` 平台规范「参照实现」或其他文档引用的符号路径变了，**同任务内更新这些文档引用**（设计文档 §9 第 6 条）。
4. 任务文档「进度」：状态 `→ ✅`，时间线追加一行（日期 + 结果 + 验证命令）；README 总表同步 `→ ✅` 并更新「总体进度」计数。
5. 提示用户可开始的下游任务；B-03 完成后提示 B-10 的触发条件仍需等待。

## 必须遵守

- **全局约束优先**：`00-全局开发约束.md` 是硬约束；spec「架构/性能注意点」是叠加约束；冲突时在 spec 显式说明原因与边界。
- **进度与状态规则以 README 为准**：进度两处同步、状态流转权威定义在 README 末尾「维护规范」。
- **设计文档是单一事实来源**：spec 与 `docs/backend-module-split-plan.md` 冲突时，先改设计文档或在 spec 显式说明，不静默偏离。
- **不顺手重构**：不做与拆分无关的改名、格式化、清理；diff 只允许预期文件和符号变化。
- **克制项**（设计文档 §10）：不按行数硬切、不拆多 crate、不机械加 trait、不建大 Repository、不把测试全改 integration test。
