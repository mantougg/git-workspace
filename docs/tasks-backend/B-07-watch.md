# B-07 拆 Watch（watch.rs → watch/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.7、§6 Phase 4。

| 项 | 值 |
|---|---|
| 阶段 | Phase 4 · 支撑模块 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | B-01 |
| 对应设计文档 | §4.7 目标目录、§6 Phase 4 |

## 目标

把约 727 行生产代码的 `runtime/watch.rs` 按「事件去抖 / 路径分类 / 影响分析 / 任务提交」拆成 `runtime/watch/` 子模块，使影响分析成为可独立验证的纯逻辑。

## 需求范围

- [x] 目标结构（§4.7）：`watch/{mod.rs, debounce.rs, classify.rs, impact.rs, submit.rs, tests.rs}`
- [x] `debounce.rs`：notify 事件收集和去抖
- [x] `classify.rs`：路径分类和 `ignore_path`
- [x] `impact.rs`：受影响模块、下游传播和 Closure 限制——**纯逻辑，不直接提交 Task**（§4.7）
- [x] `submit.rs`：RebuildRestart / Resolve 任务提交
- [x] `mod.rs`：RuntimeWatchEngine 和线程装配 + re-export
- [x] `impact.rs` 独立单测（§4.7）：变更模块只扩散到允许的下游；外部依赖不当成本地源码模块传播；Closure 之外模块不被加入重建集合；同批事件收敛为一个任务

## 架构 / 性能注意点

- 影响分析（`impact.rs`）与任务提交（`submit.rs`）分离是本任务的核心收益：纯函数可穷举测试，提交路径集中审计。
- 去抖窗口与事件聚合行为不变（避免文件保存风暴触发重复构建）。
- 任务提交仍走 TaskManager 现有通道，任务类型与 payload 不变。
- 路径分类遵守归一化规则（全局约束 §6）；ignore 规则行为不变。

## 验收标准

- [x] §4.7 四条影响分析单测全部落地并通过
- [x] 文件变更 → 增量构建/重启的端到端行为不变（既有 watch 测试全绿）
- [x] 同一批事件仍收敛为一个任务（无重复提交）
- [x] `impact.rs` 无任何 Task 提交依赖（代码走查确认：仅依赖 maven 类型与 classify 助手，无 models::task / submit 导入）
- [x] 四件套全绿；公共入口不变

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 | 开始开发：确认 `runtime/watch.rs` 实际 1075 行（spec 估算 727），测试与 RecordingSubmitter 内联于文件尾（外移至 `watch/tests.rs`） |
| 2026-08-29 | ✅ | 完成：`git mv` 保留历史后按 `classify → debounce → impact → submit + 测试外移` 迁移，每组跑四件套。1075 行 → `mod.rs` 534 行（引擎与线程装配）+ 4 个子模块（classify 36 / debounce 41 / impact 79 / submit 80）+ tests.rs 498。`impact.rs` 纯函数化：`affected_modules(closure, graph, paths)` 无任何 Task 提交依赖（代码走查：仅导入 maven 类型与 classify 助手），引擎方法变薄封装；§4.7 四条独立单测全部落地（新增 `affected_modules_pure_maps_paths_and_propagates` 与 `same_batch_of_events_converges_into_single_task`，watch 测试 9 → 11）。GitNexus impact：`RuntimeWatchEngine` / `ignore_path` 均 LOW。`detect_changes()` 风险 LOW、受影响执行流 0。全量 `cargo test` 与基线一致（唯一超额失败为既有 `logs::flood` flaky，单独复跑通过）；clippy 在 `runtime/watch/` 零告警（顺带修复随迁代码的 field_reassign / cloned_ref_to_slice_refs 两处 lint）。公共入口不变：`watch::RuntimeWatchEngine` / `WatchTaskSubmitter` / `ignore_path` 经 mod.rs re-export，调用方（lib.rs、git_link.rs、runtime/mod.rs re-export）零修改。 |

### 子任务清单

- [x] `classify.rs`（路径分类/ignore）
- [x] `debounce.rs`（事件收集/去抖）
- [x] `impact.rs`（纯影响分析 + 独立单测）
- [x] `submit.rs`（任务提交）
- [x] `mod.rs` 装配与 re-export
- [x] 测试归位与四件套验证
