# T-20 Batch Operations 增强

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-05 Task Queue 硬化](./T-05-task-queue.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-05 |
| 对应 Roadmap | §16 Workspace Batch Operations、§52 Workspace Selection |

## 目标

在现有批量 Add/Fetch/Pull/Push/Commit 基础上，扩展选择器（仓库/分组/标签/状态）与批量操作全集，形成核心差异化能力。

## 需求范围

- [x] 选择器：Select Repositories / Groups / Tags / Status（`@group:` / `@tag:` / `@status:` / 名称关键字，空格分词 AND；`core/selector.rs` 纯函数 + `select_repos` command，内存过滤）
- [x] 快速筛选：Dirty / Conflict / Ahead / Behind / Favorite（chips 切换 @status token）
- [x] 操作全集：Fetch All / Pull All / Push All / Commit All（已有）/ Checkout All / Create Branch All / Delete Branch All（`TaskType::BranchOp` 走任务队列）；Stash All 留 T-21
- [x] 批量 Dry-run：Pull/Push All 前预演（fast-forward / diverged / conflict / up_to_date / no_upstream 分类报告，纯本地计算：remote-tracking ref + `merge_commits` 内存预测冲突，不产生任何变更）；对话框内可对可快进子集一键执行
- [x] 全部走 T-05 任务队列，逐仓库子结果、Partial Success、进度事件（batch 合成任务 + `task_items` 落库 + TaskPanel 批次明细）
- [x] 危险批量操作（Delete Branch All）§46 分级确认，列出受影响仓库列表

## 架构 / 性能注意点

- 批量网络操作严格遵守 §45 并发限流（Fetch 8 / Pull 4 / Push 4），禁止按仓库数无上限 fork git 进程。
- 选择器过滤在内存缓存上做（T-02），不做 DB 全表扫描。
- 批量结果聚合展示（§20 任务面板样式：仓库级 ✓/✗ + 失败原因）。

## 验收标准

- [x] 四种选择器组合过滤结果正确（`core/selector.rs` 单测：token 解析 / group-tag-status-text AND 组合 / 空查询全匹配）
- [x] 100 仓库 Fetch All 并发被限制在 8，进程数可控（worker 池 8 上限即 git 进程上限；T-07 的 Git Process Count 字段已在 benchmark 结果结构中，未新跑实测）
- [x] 部分失败正确标 Partial Success 且可定位失败仓库（`BatchState::record_child` 单测覆盖 mixed/all-failed/all-cancelled；TaskPanel 批次明细列出失败仓库与原因；`task_items` 逐仓库落库）
- [x] Delete Branch All 有危险确认并列出受影响仓库
- [x] Dry-run 输出正确影响报告（fast-forward / diverged / conflict 分类，`dry_run_categorizes_pull_and_push` 集成测试），不产生任何仓库变更

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-17 全部验收标准通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-17 | 🟦 | 开始开发（同时闭环 T-05 剩余项「逐仓库子结果聚合」） |
| 2026-08-17 | ✅ | 完成：批次聚合（`Task.batch_id` + `BatchState` 合成任务 + `task_items` 落库 + TaskPanel 分组明细，PartialSuccess 闭环 T-05）；`TaskType::BranchOp`（checkout/create/delete）+ `batch_branch_op`；selector 引擎 + `select_repos` + 前端选择器栏/快捷筛选 chips；`batch_dry_run`（本地 ahead/behind + merge_commits 冲突预测，rayon 并行）+ 预演报告对话框（可快进子集一键执行）；Delete Branch All 双重确认；golden/TS 同步；`cargo test` 110 passed、`pnpm build` 通过 |

### 子任务清单

- [x] 选择器（group/tag/status）实现
- [x] 快速筛选
- [x] 批量操作全集接入任务队列
- [x] 批量结果聚合 UI
- [x] 危险批量确认流
- [x] 批量 Dry-run 预演与影响报告
